# Audit scope

Written for an outside, independent security audit. Revision described:
`fce9843500629b7049853a9bedd41990e2c8bf39` on `main`, 2026-10-26. Everything below is taken from
the repository's own files at that revision; items marked NOT_RUN were not executed while writing
this document.

## What this repository is

aien-edge is a small native Rust web server (hyper and rustls) that serves prebuilt static sites
from memory, with brotli and gzip precompression, ETags, virtual hosts and automatic Let's
Encrypt TLS (TLS-ALPN-01). It was built to run on a Raspberry Pi at the network edge. Status
(README, 2026-10-18): not in use; the sites are served by GitHub Pages. It is hosting
infrastructure, not part of the AIEN core. One binary crate.

## What to review, in priority order

1. `src/site.rs`: site loading into memory, traversal-safe path normalization against an in-memory
   map, exclusion of hidden files and source maps, encoding negotiation, ETags.
2. `src/handler.rs`: GET and HEAD only, canonical host and apex to `www` redirects, HTTP to HTTPS
   redirects, 421 for unknown hosts, security headers (HSTS, nosniff, frame denial, referrer
   policy).
3. `src/main.rs`: listeners (plain HTTP and TLS-ALPN-01), connection cap, header-read and TLS
   handshake timeouts, `SIGHUP` reload that keeps the old site if the new one fails to load.
4. `src/config.rs`: TOML schema with `deny_unknown_fields`; `[tls]` with `production` switching
   between Let's Encrypt staging and production; `cache_dir` for ACME state.
5. `deploy/aien-edge.service`: `DynamicUser=yes`, `CAP_NET_BIND_SERVICE` only, read-only
   `/srv/sites`.
6. `deploy/publish-site.sh` and `deploy/publish.env.example`: builds a site, uploads it over ssh
   and rsync to `EDGE_HOST`, repoints a symlink and reloads.
7. There are no CI workflows in this repository.

## Trust boundaries

- Untrusted: every network client (request line, Host header, Accept-Encoding, path).
- Trusted: the config file (read at startup only); site files under the configured roots; the
  ACME cache directory; the operator who runs the publish script.
- The server never connects to the build machine; deploys push to it over ssh (README).

## Threat model summary

- Assets: integrity of served sites, TLS private keys in the ACME cache, availability.
- Attackers considered: remote clients sending crafted paths, hosts or slow connections;
  connection floods up to and beyond the cap.
- Defended: no per-request filesystem access, traversal-safe normalization, method restriction,
  timeouts, connection cap, systemd sandboxing, security headers.
- Not defended / explicitly out of scope: compromise of the publishing host or ssh credentials;
  content of the sites; rate limiting beyond the connection cap is not described in the
  repository.

## Build from source at a pin

A stable Rust toolchain; edition 2021; no `rust-toolchain.toml`, so the exact version is unknown
from the repository. `Cargo.lock` is committed.

```sh
git clone https://github.com/aien-dev/aien-edge.git
cd aien-edge
cargo build --release
```

## Run every test

```sh
cargo test
cargo clippy --release -- -D warnings
```

- AGENTS.md: `cargo test` runs 9 unit tests in `src/site.rs` and `src/handler.rs`.
- The Pi deployment and `deploy/publish-site.sh` need sudo and a remote host and are not a test.
- All commands above: NOT_RUN while writing this document.

## Known limitations and NOT_RUN

- Not deployed; no current production use.
- `deploy/publish-site.sh` still runs `npm ci` and `npm run build`, which no longer matches how
  the sites are built; it must be updated before any redeploy.
- The config is read only at startup; changes need a restart.
- The README throughput figures were measured on other hardware from commit 59df18c and cannot be
  repeated from this repository alone; not measured on a Pi.

## Dependencies and provenance

- `Cargo.lock`: hyper, rustls, rustls-acme, tracing, brotli and gzip encoders, `tempfile` for
  tests. The TLS library was updated for security on 2026-10-17 (README).
- External services: Let's Encrypt.
- Licence: AGPL-3.0-or-later (`LICENSE`, `Cargo.toml`); `COVENANT.md` is nonbinding.

## Evidence

- No `evidence/` directory. The recorded throughput run is in the message of commit 843cc77.
- Whether `aien-dev/aien-receipts` holds `aien-edge/<sha>/` entries is unknown from the
  repository. Receipts are append-only and never edited.
