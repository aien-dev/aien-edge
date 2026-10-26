# Security policy

## Reporting a vulnerability

Report privately. Do not open a public issue, pull request or discussion about it.

1. GitHub: open this repository's Security tab and choose "Report a vulnerability". This
   creates a private advisory that only the maintainers can see. If that option is not shown,
   private vulnerability reporting has not been enabled for this repository yet; use email.
2. Email: drake@aienos.com with the subject `[aien-edge] security report`.

Include the commit or release you tested, the component (path), steps to reproduce, and what an
attacker gains. If the report involves a secret, say where it was exposed and do not send the
secret itself. Encrypted mail is not required; do not post proof-of-concept payloads publicly.

## Response targets

This project has one maintainer; the targets below are best effort and stated honestly.

- Acknowledgement: within 3 working days.
- Triage and severity: within 7 days of acknowledgement.
- Fix or mitigation for confirmed issues: aim for 30 days for high and critical, 90 days
  otherwise, with a status update at least every 14 days until closed.
- Disclosure: coordinated. An advisory is published with credit (unless you ask otherwise)
  once the fix is on the default branch. Default embargo 90 days, negotiable.

## Scope

In scope:

- Path traversal, hidden file or source map disclosure through request paths (`src/site.rs`).
- Request handling: methods other than GET and HEAD, host and redirect handling, missing
  security headers, timeouts and the connection cap (`src/handler.rs`, `src/main.rs`).
- TLS and ACME configuration (`rustls-acme`, `[tls]` in `src/config.rs`).
- Reload behaviour on `SIGHUP` serving a partial or wrong site.
- `deploy/aien-edge.service` hardening and `deploy/publish-site.sh`.

Out of scope: the static sites themselves (report to their repositories); `rustls`, `hyper` and
other upstream crates except how this server uses them; Let's Encrypt.

## Supported versions

Only the default branch `main`; there is no release. Fixes land on the default branch only.

## Status

Not in use: the sites it was written for are served by GitHub Pages (README, status
2026-10-18). There is no certification and no bug bounty. See AUDIT_SCOPE.md for what to review and
how to build and test it.
