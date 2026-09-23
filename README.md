# aien-edge

A small native Rust edge server for the AIEN static sites (`www.aienos.com`, `www.drakestapleton.com`). It runs on a Raspberry Pi at the network edge so the NVIDIA DGX Spark never faces the internet.

- **Everything in memory.** Each site is read once at startup, precompressed with brotli (quality 11) and gzip, and hashed for ETags. Requests never touch the disk.
- **Automatic HTTPS.** Let's Encrypt certificates through `rustls-acme` (TLS-ALPN-01 on port 443; port 80 only redirects). TLS is `rustls`; nothing is hand-rolled.
- **Virtual hosts.** Canonical host per site, apex to `www` 301 redirects, HTTP to HTTPS redirects, 421 for unknown hosts.
- **Small attack surface.** GET and HEAD only, traversal-safe path normalization against an in-memory map (no filesystem lookups per request), hidden files and source maps never loaded, header-read and TLS-handshake timeouts, a connection cap, and security headers (HSTS, nosniff, frame denial, referrer policy).
- **Zero-downtime deploys.** `SIGHUP` (`systemctl reload aien-edge`) reloads every site; if the new copy fails to load, the old one keeps serving.

## Measured

On the Spark (aarch64), serving both built sites from one process, HTTP/1.1 keep-alive, brotli:

```
$ ab -k -n 50000 -c 64 -H "Host: www.drakestapleton.com" -H "Accept-Encoding: br" http://127.0.0.1:18777/dad
Failed requests:        0
Requests per second:    191460.85 [#/sec] (mean)
Time per request:       0.334 [ms] (mean)
VmRSS:                  18276 kB   (VmHWM 30452 kB)
```

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

Then publish each site from the Spark with `deploy/publish-site.sh <name> <checkout>`. Keep `production = false` (Let's Encrypt staging) until both hosts issue certificates, then switch to `true` and reload.

Network: forward TCP 80 and 443 on the router to the Pi only, and point the DNS A records for both apex and `www` hosts at the public IP. The Pi reaches the Spark only for deploys, over the direct link.

## License

Apache License 2.0 with LLVM Exception. See [LICENSE](LICENSE). Values live in the nonbinding [COVENANT.md](COVENANT.md).
