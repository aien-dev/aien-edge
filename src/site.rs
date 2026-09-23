//! In-memory site store. Every file is read once at load, precompressed, and
//! hashed, so serving a request never touches the disk.

use bytes::Bytes;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

/// Files smaller than this are served uncompressed; the headers would cost more than the savings.
const COMPRESS_MIN: usize = 1024;

#[derive(Debug)]
pub struct Asset {
    pub body: Bytes,
    pub br: Option<Bytes>,
    pub gzip: Option<Bytes>,
    pub content_type: String,
    pub etag: String,
    pub cache_control: &'static str,
}

#[derive(Debug)]
pub struct SiteFiles {
    /// Keyed by URL path without the leading slash, for example "assets/index-abc.js".
    pub files: HashMap<String, Asset>,
    pub spa: bool,
    pub bytes_raw: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Brotli,
    Gzip,
    Identity,
}

impl SiteFiles {
    pub fn load(root: &Path, spa: bool) -> Result<Self, String> {
        let mut files = HashMap::new();
        let mut bytes_raw = 0;
        walk(root, root, &mut files, &mut bytes_raw)?;
        if !files.contains_key("index.html") {
            return Err(format!("{} has no index.html", root.display()));
        }
        Ok(SiteFiles { files, spa, bytes_raw })
    }

    /// Resolve a request path to an asset and the status to serve it with.
    /// Returns None when nothing matches and the site has no 404 page.
    pub fn resolve(&self, path: &str) -> Option<(&Asset, u16)> {
        let key = normalize(path)?;
        let candidates = [
            key.clone(),
            if key.is_empty() { "index.html".into() } else { format!("{key}/index.html") },
            format!("{key}.html"),
        ];
        for c in candidates.iter() {
            if let Some(a) = self.files.get(c.trim_start_matches('/')) {
                return Some((a, 200));
            }
        }
        if self.spa {
            return self.files.get("index.html").map(|a| (a, 200));
        }
        self.files.get("404.html").map(|a| (a, 404))
    }
}

/// Decode and normalize a URL path. Rejects traversal and anything outside the site.
pub fn normalize(path: &str) -> Option<String> {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let decoded = percent_decode(path)?;
    let mut parts: Vec<&str> = Vec::new();
    for seg in decoded.split('/') {
        match seg {
            "" | "." => {}
            ".." => return None,
            s if s.contains('\\') || s.contains('\0') => return None,
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

pub fn pick_encoding(accept: Option<&str>, asset: &Asset) -> Encoding {
    let Some(accept) = accept else { return Encoding::Identity };
    let allows = |name: &str| {
        accept.split(',').any(|tok| {
            let mut it = tok.trim().split(';');
            let coding = it.next().unwrap_or("").trim();
            let q_zero = it.any(|p| {
                let p = p.trim();
                p == "q=0" || p == "q=0.0" || p == "q=0.00" || p == "q=0.000"
            });
            coding.eq_ignore_ascii_case(name) && !q_zero
        })
    };
    if asset.br.is_some() && allows("br") {
        Encoding::Brotli
    } else if asset.gzip.is_some() && allows("gzip") {
        Encoding::Gzip
    } else {
        Encoding::Identity
    }
}

fn walk(root: &Path, dir: &Path, out: &mut HashMap<String, Asset>, total: &mut usize) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            walk(root, &path, out, total)?;
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.ends_with(".map") || rel.split('/').any(|s| s.starts_with('.') && s != ".well-known") {
            continue;
        }
        let body = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        *total += body.len();
        out.insert(rel.clone(), build_asset(&rel, body));
    }
    Ok(())
}

fn build_asset(rel: &str, body: Vec<u8>) -> Asset {
    let mime = mime_guess::from_path(rel).first_or_octet_stream();
    let text_like = mime.type_() == "text"
        || matches!(mime.subtype().as_str(), "javascript" | "json" | "xml" | "svg")
        || mime.essence_str() == "image/svg+xml"
        || rel.ends_with(".webmanifest");
    let content_type = if mime.type_() == "text" || mime.subtype() == "javascript" {
        format!("{}; charset=utf-8", mime.essence_str())
    } else {
        mime.essence_str().to_string()
    };
    let (br, gzip) = if text_like && body.len() >= COMPRESS_MIN {
        let br = brotli_bytes(&body);
        let gz = gzip_bytes(&body);
        (
            (br.len() < body.len()).then(|| Bytes::from(br)),
            (gz.len() < body.len()).then(|| Bytes::from(gz)),
        )
    } else {
        (None, None)
    };
    let etag = format!("\"{}\"", &blake3::hash(&body).to_hex()[..20]);
    // Vite emits content-hashed names under assets/, so they can be cached forever.
    let cache_control = if rel.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else if rel.ends_with(".html") {
        "public, max-age=0, must-revalidate"
    } else {
        "public, max-age=3600"
    };
    Asset { body: Bytes::from(body), br, gzip, content_type, etag, cache_control }
}

fn brotli_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 3);
    {
        let mut w = brotli::CompressorWriter::new(&mut out, 4096, 11, 22);
        w.write_all(data).expect("brotli write to Vec");
    }
    out
}

fn gzip_bytes(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(data).expect("gzip write to Vec");
    enc.finish().expect("gzip finish")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(files: &[(&str, &str)], spa: bool) -> (tempfile::TempDir, SiteFiles) {
        let dir = tempfile::tempdir().unwrap();
        for (p, c) in files {
            let full = dir.path().join(p);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, c).unwrap();
        }
        let s = SiteFiles::load(dir.path(), spa).unwrap();
        (dir, s)
    }

    #[test]
    fn normalize_rejects_traversal() {
        assert_eq!(normalize("/a/../b"), None);
        assert_eq!(normalize("/%2e%2e/etc/passwd"), None);
        assert_eq!(normalize("/a%5cb"), None);
        assert_eq!(normalize("/a//b/./c?x=1"), Some("a/b/c".into()));
        assert_eq!(normalize("/"), Some("".into()));
    }

    #[test]
    fn resolves_prerendered_routes_and_404() {
        let (_d, s) = site(&[("index.html", "home"), ("dad/index.html", "dad"), ("404.html", "nf")], false);
        assert_eq!(s.resolve("/").unwrap().1, 200);
        assert_eq!(&s.resolve("/dad").unwrap().0.body[..], b"dad");
        assert_eq!(&s.resolve("/dad/").unwrap().0.body[..], b"dad");
        let (a, code) = s.resolve("/missing").unwrap();
        assert_eq!((&a.body[..], code), (&b"nf"[..], 404));
    }

    #[test]
    fn spa_falls_back_to_index() {
        let (_d, s) = site(&[("index.html", "app")], true);
        let (a, code) = s.resolve("/deep/link").unwrap();
        assert_eq!((&a.body[..], code), (&b"app"[..], 200));
    }

    #[test]
    fn hidden_files_and_sourcemaps_are_not_served() {
        let (_d, s) = site(&[("index.html", "x"), (".env", "secret"), ("assets/a.js.map", "{}")], false);
        assert!(!s.files.contains_key(".env"));
        assert!(!s.files.contains_key("assets/a.js.map"));
    }

    #[test]
    fn encoding_negotiation() {
        let big = "x".repeat(4096);
        let (_d, s) = site(&[("index.html", &big)], false);
        let a = s.files.get("index.html").unwrap();
        assert_eq!(pick_encoding(Some("gzip, deflate, br"), a), Encoding::Brotli);
        assert_eq!(pick_encoding(Some("gzip"), a), Encoding::Gzip);
        assert_eq!(pick_encoding(Some("br;q=0, gzip"), a), Encoding::Gzip);
        assert_eq!(pick_encoding(None, a), Encoding::Identity);
        assert_eq!(a.cache_control, "public, max-age=0, must-revalidate");
    }
}
