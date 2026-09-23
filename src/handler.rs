//! Request handling: host routing, redirects, conditional GET, and encoding negotiation.

use crate::site::{pick_encoding, Encoding, SiteFiles};
use bytes::Bytes;
use http::{header, HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::Full;
use std::collections::HashMap;
use std::sync::Arc;

pub struct Sites {
    pub by_host: HashMap<String, Arc<SiteFiles>>,
    /// Alias host to canonical host.
    pub redirects: HashMap<String, String>,
}

#[derive(Clone, Copy)]
pub enum Scheme {
    /// Plain HTTP on a TLS-enabled server: redirect everything to HTTPS.
    HttpRedirect,
    /// Plain HTTP without TLS configured: serve content.
    HttpServe,
    Https,
}

pub fn handle<B>(req: &Request<B>, sites: &Sites, scheme: Scheme) -> Response<Full<Bytes>> {
    let host = request_host(req);
    let path_and_query = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");

    if let Some(canonical) = sites.redirects.get(&host) {
        return redirect(&format!("https://{canonical}{path_and_query}"), scheme);
    }
    let Some(site) = sites.by_host.get(&host) else {
        return plain(StatusCode::MISDIRECTED_REQUEST, "unknown host\n", scheme);
    };
    if let Scheme::HttpRedirect = scheme {
        return redirect(&format!("https://{host}{path_and_query}"), scheme);
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        let mut r = plain(StatusCode::METHOD_NOT_ALLOWED, "method not allowed\n", scheme);
        r.headers_mut().insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
        return r;
    }
    let Some((asset, code)) = site.resolve(req.uri().path()) else {
        return plain(StatusCode::NOT_FOUND, "not found\n", scheme);
    };

    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, &asset.content_type)
        .header(header::ETAG, &asset.etag)
        .header(header::CACHE_CONTROL, asset.cache_control)
        .header(header::VARY, "Accept-Encoding");
    if code == 200 {
        let inm = req.headers().get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok());
        if inm.is_some_and(|v| v.split(',').any(|t| t.trim() == asset.etag || t.trim() == "*")) {
            let mut r = builder.status(StatusCode::NOT_MODIFIED).body(Full::new(Bytes::new())).unwrap();
            security_headers(&mut r, scheme);
            return r;
        }
    }
    let accept = req.headers().get(header::ACCEPT_ENCODING).and_then(|v| v.to_str().ok());
    let body = match pick_encoding(accept, asset) {
        Encoding::Brotli => {
            builder = builder.header(header::CONTENT_ENCODING, "br");
            asset.br.clone().unwrap()
        }
        Encoding::Gzip => {
            builder = builder.header(header::CONTENT_ENCODING, "gzip");
            asset.gzip.clone().unwrap()
        }
        Encoding::Identity => asset.body.clone(),
    };
    builder = builder.header(header::CONTENT_LENGTH, body.len());
    let body = if req.method() == Method::HEAD { Bytes::new() } else { body };
    let mut r = builder.status(code).body(Full::new(body)).unwrap();
    security_headers(&mut r, scheme);
    r
}

fn request_host<B>(req: &Request<B>) -> String {
    let raw = req
        .uri()
        .host()
        .map(str::to_string)
        .or_else(|| req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).map(str::to_string))
        .unwrap_or_default();
    // Strip a port and any trailing dot, and compare case-insensitively.
    let no_port = match raw.rsplit_once(':') {
        Some((h, p)) if !h.contains(']') && p.chars().all(|c| c.is_ascii_digit()) => h.to_string(),
        _ => raw,
    };
    no_port.trim_end_matches('.').to_ascii_lowercase()
}

fn redirect(location: &str, scheme: Scheme) -> Response<Full<Bytes>> {
    let mut r = Response::builder()
        .status(StatusCode::MOVED_PERMANENTLY)
        .header(header::LOCATION, location)
        .header(header::CONTENT_LENGTH, 0)
        .body(Full::new(Bytes::new()))
        .unwrap();
    security_headers(&mut r, scheme);
    r
}

fn plain(status: StatusCode, msg: &'static str, scheme: Scheme) -> Response<Full<Bytes>> {
    let mut r = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CONTENT_LENGTH, msg.len())
        .body(Full::new(Bytes::from_static(msg.as_bytes())))
        .unwrap();
    security_headers(&mut r, scheme);
    r
}

fn security_headers(r: &mut Response<Full<Bytes>>, scheme: Scheme) {
    let h = r.headers_mut();
    h.insert(header::SERVER, HeaderValue::from_static("aien-edge"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert("permissions-policy", HeaderValue::from_static("camera=(), microphone=(), geolocation=()"));
    if let Scheme::Https = scheme {
        h.insert(header::STRICT_TRANSPORT_SECURITY, HeaderValue::from_static("max-age=31536000"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::site::SiteFiles;

    fn sites() -> (tempfile::TempDir, Sites) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("index.html"), "x".repeat(3000)).unwrap();
        let s = Arc::new(SiteFiles::load(dir.path(), true).unwrap());
        let mut by_host = HashMap::new();
        by_host.insert("www.example.com".to_string(), s);
        let mut redirects = HashMap::new();
        redirects.insert("example.com".to_string(), "www.example.com".to_string());
        (dir, Sites { by_host, redirects })
    }

    fn get(host: &str, path: &str) -> Request<()> {
        Request::builder().uri(path).header("host", host).body(()).unwrap()
    }

    #[test]
    fn apex_redirects_to_canonical_https() {
        let (_d, s) = sites();
        let r = handle(&get("Example.com.", "/a?b=1"), &s, Scheme::Https);
        assert_eq!(r.status(), 301);
        assert_eq!(r.headers()[header::LOCATION], "https://www.example.com/a?b=1");
    }

    #[test]
    fn http_redirects_when_tls_enabled() {
        let (_d, s) = sites();
        let r = handle(&get("www.example.com:80", "/x"), &s, Scheme::HttpRedirect);
        assert_eq!(r.headers()[header::LOCATION], "https://www.example.com/x");
    }

    #[test]
    fn serves_brotli_and_honors_etag() {
        let (_d, s) = sites();
        let mut req = get("www.example.com", "/");
        req.headers_mut().insert(header::ACCEPT_ENCODING, HeaderValue::from_static("br"));
        let r = handle(&req, &s, Scheme::Https);
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()[header::CONTENT_ENCODING], "br");
        assert!(r.headers().contains_key(header::STRICT_TRANSPORT_SECURITY));
        let etag = r.headers()[header::ETAG].clone();
        req.headers_mut().insert(header::IF_NONE_MATCH, etag);
        assert_eq!(handle(&req, &s, Scheme::Https).status(), 304);
    }

    #[test]
    fn unknown_host_and_bad_method() {
        let (_d, s) = sites();
        assert_eq!(handle(&get("evil.test", "/"), &s, Scheme::Https).status(), 421);
        let post = Request::builder().method("POST").uri("/").header("host", "www.example.com").body(()).unwrap();
        assert_eq!(handle(&post, &s, Scheme::Https).status(), 405);
    }
}
