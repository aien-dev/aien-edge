# aien-edge (native Rust edge server for AIEN static sites)

> A small Rust web server (hyper + rustls) that serves prebuilt static sites from memory, with precompressed assets, virtual hosts and automatic Let's Encrypt TLS. It was built to run on a Raspberry Pi at the network edge. It is not in use today: the AIEN sites are served by GitHub Pages. This is hosting infrastructure, not part of the AIEN core.
Shared rules: read https://www.aienos.com/agents/RULES.md first. This file adds what is specific to this repository; where the two differ, this file wins here.

## Scope

- Applies to the whole repository: one binary crate.
- `src/main.rs`: startup, listeners (plain HTTP and TLS-ALPN-01 ACME), connection cap, timeouts, `SIGHUP` reload.
- `src/config.rs`: the TOML config schema (`deny_unknown_fields`).
- `src/site.rs`: loads a site into memory, path normalization, brotli/gzip precompression, ETags, encoding negotiation.
- `src/handler.rs`: request handling, redirects, security headers.
- `deploy/`: example config, systemd unit, publish script. `deploy/publish-site.sh` still runs `npm ci` and `npm run build`, which no longer matches how the sites are built (both use Rust generators); it must be updated before any redeploy.

## Installation

```bash
git clone https://github.com/aien-dev/aien-edge.git
cd aien-edge
cargo build --release   # writes target/release/aien-edge
```

Needs a stable Rust toolchain. `Cargo.lock` is committed. aarch64 builds run on a 64-bit Raspberry Pi OS.

## Configuration

- The binary takes one argument, the config path; default `/etc/aien-edge/config.toml`. The config is read only at startup; changing it needs a restart, not a reload.
- `deploy/config.example.toml` is the reference. Keys: `http_listen` (default `[::]:80`), `max_connections` (default 2048), optional `[tls]` (`https_listen` default `[::]:443`, `contact`, `cache_dir`, `production`), and one or more `[[site]]` tables (`host`, `redirect_from`, `root`, `spa`).
- Without `[tls]` the server runs plain HTTP only and serves content on the HTTP port (local testing).
- `production = false` uses the Let's Encrypt staging directory. Keep it false until certificates issue cleanly.
- Log level comes from `RUST_LOG` (tracing `EnvFilter`), default `info`.
- The server exits at startup if a config has no `[[site]]`, or a site root is missing or has no `index.html`.

## Usage

```bash
cargo test                              # 9 unit tests in src/site.rs and src/handler.rs
cargo clippy --release -- -D warnings
cargo build --release
target/release/aien-edge <config.toml>  # serves until stopped; SIGHUP reloads every site from disk
```

- Running the binary opens network listeners. For a local run use a config without `[tls]`, a non-privileged `http_listen`, and a `[[site]]` root that holds a built site with `index.html`.
- Deploying (`README.md`, "Deploy on the Pi") needs a Raspberry Pi with systemd, sudo, ports 80 and 443 forwarded to it, and DNS pointing at it. `deploy/aien-edge.service` runs it with `DynamicUser=yes`, `CAP_NET_BIND_SERVICE` only and a read-only `/srv/sites`.
- There are no CI workflows in this repository. Run the three cargo commands above before opening a pull request.

## Conventions

- Rust 2021 edition. Tests live beside the code in `#[cfg(test)]` modules and use `tempfile` for site fixtures. Add a test when a rule (traversal rejection, redirects, hidden files, encoding choice) could regress silently.
- Keep the attack surface small: GET and HEAD only, no per-request filesystem access, hidden files and source maps never loaded. TLS comes from `rustls`/`rustls-acme`; do not hand-roll crypto or HTTP parsing.
- Performance claims need a recorded run (command, commit, output), as in the "Measured" section of `README.md`. Check `ab` output has no `Non-2xx responses` line.
- License is AGPL-3.0-or-later (`LICENSE`, `Cargo.toml`). `COVENANT.md` is nonbinding and says `LICENSE` governs where they disagree.
- No em dashes or en dashes.

## Safety and approvals

- Ask before deploying, changing DNS, or switching `production = true`; the sites are live on GitHub Pages and this server is not.
- Ask before relaxing a security default (methods, headers, timeouts, connection cap, systemd hardening in `deploy/aien-edge.service`).
- Never commit certificates, the ACME cache directory or private keys. `deploy/publish-site.sh` takes the target host from `EDGE_HOST`; set it in the environment rather than hardcoding new addresses.
- Do not run `deploy/publish-site.sh` or the Pi install commands as a check; they need sudo and a remote host.

## Further reading

- `README.md`: features, measured throughput, build and Pi deployment guide.
- `deploy/config.example.toml`, `deploy/aien-edge.service`, `deploy/publish-site.sh`.
- `COVENANT.md`: nonbinding values statement.
