# aien-edge

## Copyright

Copyright (c) 2026 Drake Stapleton <aien@aienos.com> and AIEN Contributors. Authored by Drake Stapleton in collaboration with AIEN.

A small native Rust edge server for the AIEN static sites (`www.aienos.com`, `www.drakestapleton.com`). It runs on a Raspberry Pi at the network edge so the NVIDIA DGX Spark never faces the internet.

- **Everything in memory.** Each site is read once at startup (and again on reload). Text-like files of 1 KiB or more are precompressed with brotli (quality 11) and gzip, and every file is hashed for ETags. Requests never touch the disk.
- **Automatic HTTPS.** Let's Encrypt certificates through `rustls-acme` (TLS-ALPN-01 on port 443; port 80 only redirects), HTTP/2 and HTTP/1.1. TLS is `rustls`; nothing is hand-rolled. Without a `[tls]` table in the config the server runs plain HTTP only and serves content on the HTTP port (local testing).
- **Virtual hosts.** Canonical host per site, apex to `www` 301 redirects, HTTP to HTTPS redirects, 421 for unknown hosts.
- **Small attack surface.** GET and HEAD only, traversal-safe path normalization against an in-memory map (no filesystem lookups per request), hidden files and source maps never loaded, header-read and TLS-handshake timeouts, a connection cap, and security headers (HSTS, nosniff, frame denial, referrer policy).
- **Zero-downtime deploys.** `SIGHUP` (`systemctl reload aien-edge`) reloads every site's files; if the new copy fails to load, the old one keeps serving. The config file is read only at startup, so config changes need a restart.

## Measured

On the Spark (aarch64), serving both built sites from one process, HTTP/1.1 keep-alive, brotli, against a page that returns 200 (`/research`, 6036 bytes brotli). Three runs on commit 59df18c, recorded in the message of commit 843cc77. The built sites are not in this repo, so the run cannot be repeated from here alone:

```
$ ab -k -n 50000 -c 64 -H "Host: www.drakestapleton.com" -H "Accept-Encoding: br" http://127.0.0.1:18777/research
Document Length:        6036 bytes
Complete requests:      50000
Failed requests:        0
Requests per second:    208785.70 / 192780.75 / 196101.50 [#/sec] (mean)
Time per request:       0.307 / 0.332 / 0.326 [ms] (mean)
VmRSS:                  18316 kB   (VmHWM 35924 kB)
```

`ab` counts only dropped connections as failed. Check that the output has no `Non-2xx responses` line, or the run measured an error page.

A Pi will be slower than the Spark, and real visitors are bound by the home uplink long before either.

## Build and test

```bash
cargo test
cargo clippy --release -- -D warnings
cargo build --release   # target/release/aien-edge, aarch64 builds run on a 64-bit Pi OS
```

## Deploy on the Pi

```bash
sudo install -m 0755 target/release/aien-edge /usr/local/bin/aien-edge
sudo install -D -m 0644 deploy/config.example.toml /etc/aien-edge/config.toml
sudo install -m 0644 deploy/aien-edge.service /etc/systemd/system/aien-edge.service
sudo mkdir -p /srv/sites/releases
sudo systemctl daemon-reload && sudo systemctl enable --now aien-edge
```

The server exits at startup if a site root is missing or has no `index.html`, and `Restart=always` retries every 2 seconds, so publish the sites soon after the first start. Publish each site from the Spark with `deploy/publish-site.sh <name> <checkout>`: it runs `npm ci` and `npm run build` in the checkout, uploads `dist/` to `/srv/sites/releases/` on the Pi (`EDGE_HOST`, default `edge@10.10.10.2`), repoints `/srv/sites/<name>` and reloads, keeping the last three releases. Keep `production = false` (Let's Encrypt staging) until the certificates issue, then switch to `true` and restart (a reload does not re-read the config).

Network: forward TCP 80 and 443 on the router to the Pi only, and point the DNS A records for both apex and `www` hosts at the public IP. The Spark reaches the Pi (over ssh and rsync, on the direct link) only to deploy; aien-edge itself never connects to the Spark.

## License

Licensed under the GNU Affero General Public License v3.0 or later (AGPL-3.0-or-later). See [LICENSE](LICENSE). Values live in the nonbinding [COVENANT.md](COVENANT.md).
