//! aien-edge: a small native edge server for AIEN static sites.
//!
//! Usage: aien-edge <config.toml>
//! Send SIGHUP to reload every site from disk after a deploy.

mod config;
mod handler;
mod site;

use arc_swap::ArcSwap;
use handler::{Scheme, Sites};
use hyper::service::service_fn;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use rustls_acme::caches::DirCache;
use rustls_acme::{is_tls_alpn_challenge, AcmeConfig};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::LazyConfigAcceptor;
use tokio_stream::StreamExt;

const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const TLS_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

fn load_sites(cfg: &config::Config) -> Result<Sites, String> {
    let mut by_host = HashMap::new();
    let mut redirects = HashMap::new();
    for s in &cfg.sites {
        let files = site::SiteFiles::load(&s.root, s.spa)?;
        tracing::info!(host = %s.host, files = files.files.len(), bytes = files.bytes_raw, "loaded site");
        by_host.insert(s.host.to_ascii_lowercase(), Arc::new(files));
        for alias in &s.redirect_from {
            redirects.insert(alias.to_ascii_lowercase(), s.host.to_ascii_lowercase());
        }
    }
    Ok(Sites { by_host, redirects })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/etc/aien-edge/config.toml".into());
    let cfg = match config::Config::load(std::path::Path::new(&path)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("aien-edge: {e}");
            std::process::exit(2);
        }
    };
    let sites = match load_sites(&cfg) {
        Ok(s) => Arc::new(ArcSwap::from_pointee(s)),
        Err(e) => {
            eprintln!("aien-edge: {e}");
            std::process::exit(2);
        }
    };
    let permits = Arc::new(Semaphore::new(cfg.max_connections));

    // SIGHUP: reload sites from disk; keep serving the old copy if the new one fails to load.
    {
        let sites = sites.clone();
        let cfg = cfg.clone();
        tokio::spawn(async move {
            let mut hup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
                .expect("SIGHUP handler");
            while hup.recv().await.is_some() {
                match load_sites(&cfg) {
                    Ok(s) => {
                        sites.store(Arc::new(s));
                        tracing::info!("reloaded sites");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "reload failed; still serving previous sites")
                    }
                }
            }
        });
    }

    let http_scheme = if cfg.tls.is_some() {
        Scheme::HttpRedirect
    } else {
        Scheme::HttpServe
    };
    let http = tokio::spawn(serve_plain(
        cfg.http_listen.clone(),
        sites.clone(),
        permits.clone(),
        http_scheme,
    ));

    if let Some(tls) = cfg.tls.clone() {
        let https = tokio::spawn(serve_tls(
            tls,
            cfg.all_hosts(),
            sites.clone(),
            permits.clone(),
        ));
        let _ = tokio::join!(http, https);
    } else {
        let _ = http.await;
    }
}

fn builder() -> Builder<TokioExecutor> {
    let mut b = Builder::new(TokioExecutor::new());
    b.http1()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT);
    b.http2().timer(TokioTimer::new());
    b
}

async fn serve_plain(
    addr: String,
    sites: Arc<ArcSwap<Sites>>,
    permits: Arc<Semaphore>,
    scheme: Scheme,
) {
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    tracing::info!(%addr, "http listening");
    loop {
        let Ok((tcp, _)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let sites = sites.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tcp.set_nodelay(true);
            let svc = service_fn(move |req| {
                let sites = sites.load();
                let resp = handler::handle(&req, &sites, scheme);
                async move { Ok::<_, std::convert::Infallible>(resp) }
            });
            let _ = builder().serve_connection(TokioIo::new(tcp), svc).await;
        });
    }
}

async fn serve_tls(
    tls: config::Tls,
    hosts: Vec<String>,
    sites: Arc<ArcSwap<Sites>>,
    permits: Arc<Semaphore>,
) {
    let mut state = AcmeConfig::new(hosts)
        .contact_push(format!("mailto:{}", tls.contact))
        .cache(DirCache::new(tls.cache_dir.clone()))
        .directory_lets_encrypt(tls.production)
        .state();
    let challenge_config = state.challenge_rustls_config();
    let mut server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(state.resolver());
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    let server_config = Arc::new(server_config);

    tokio::spawn(async move {
        while let Some(event) = state.next().await {
            match event {
                Ok(ok) => tracing::info!(event = ?ok, "acme"),
                Err(err) => tracing::error!(error = ?err, "acme"),
            }
        }
    });

    let addr = tls.https_listen.clone();
    let listener = TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    tracing::info!(%addr, production = tls.production, "https listening");
    loop {
        let Ok((tcp, _)) = listener.accept().await else {
            continue;
        };
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let sites = sites.clone();
        let challenge_config = challenge_config.clone();
        let server_config = server_config.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = tcp.set_nodelay(true);
            let handshake = async {
                let start = LazyConfigAcceptor::new(Default::default(), tcp)
                    .await
                    .ok()?;
                if is_tls_alpn_challenge(&start.client_hello()) {
                    let _ = start.into_stream(challenge_config).await;
                    return None;
                }
                start.into_stream(server_config).await.ok()
            };
            let Ok(Some(stream)) = tokio::time::timeout(TLS_HANDSHAKE_TIMEOUT, handshake).await
            else {
                return;
            };
            let svc = service_fn(move |req| {
                let sites = sites.load();
                let resp = handler::handle(&req, &sites, Scheme::Https);
                async move { Ok::<_, std::convert::Infallible>(resp) }
            });
            let _ = builder().serve_connection(TokioIo::new(stream), svc).await;
        });
    }
}
