//! TOML configuration for aien-edge.

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Address for plain HTTP. Serves redirects to HTTPS when `tls` is set, content otherwise.
    #[serde(default = "default_http")]
    pub http_listen: String,
    /// TLS settings. Omit to run plain HTTP only (local testing, or behind a tunnel).
    pub tls: Option<Tls>,
    /// Upper bound on concurrent connections across both listeners.
    #[serde(default = "default_max_conns")]
    pub max_connections: usize,
    #[serde(rename = "site", default)]
    pub sites: Vec<Site>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    #[serde(default = "default_https")]
    pub https_listen: String,
    /// ACME account contact, for example "aien@aienos.com".
    pub contact: String,
    /// Directory where certificates and the ACME account key are cached.
    pub cache_dir: PathBuf,
    /// false uses the Let's Encrypt staging directory.
    #[serde(default)]
    pub production: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Site {
    /// Canonical host, for example "www.aienos.com".
    pub host: String,
    /// Hosts that permanently redirect to `host`, for example "aienos.com".
    #[serde(default)]
    pub redirect_from: Vec<String>,
    /// Directory holding the built site.
    pub root: PathBuf,
    /// Serve index.html for unknown paths (single page apps). Otherwise 404.html with status 404.
    #[serde(default)]
    pub spa: bool,
}

fn default_http() -> String {
    "[::]:80".into()
}
fn default_https() -> String {
    "[::]:443".into()
}
fn default_max_conns() -> usize {
    2048
}

impl Config {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let cfg: Config = toml::from_str(&text).map_err(|e| format!("parse {}: {e}", path.display()))?;
        if cfg.sites.is_empty() {
            return Err("config has no [[site]] entries".into());
        }
        Ok(cfg)
    }

    /// Every host name that needs a certificate.
    pub fn all_hosts(&self) -> Vec<String> {
        let mut hosts: Vec<String> = self
            .sites
            .iter()
            .flat_map(|s| std::iter::once(s.host.clone()).chain(s.redirect_from.iter().cloned()))
            .collect();
        hosts.sort();
        hosts.dedup();
        hosts
    }
}
