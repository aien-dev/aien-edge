#!/usr/bin/env bash
# Build a site on the Spark and publish it to the edge host, then hot-reload.
#
#   deploy/publish-site.sh aienos.com ~/workspace/aienos.com
#   deploy/publish-site.sh drakestapleton.com ~/workspace/drakestapleton.com
#
# EDGE_HOST (ssh target of the edge host) comes from the environment or from
# deploy/publish.env (local-only, git-ignored; see deploy/publish.env.example).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
if [ -z "${EDGE_HOST:-}" ] && [ -f "$here/publish.env" ]; then
  . "$here/publish.env"
fi

name="${1:?site name, for example aienos.com}"
src="${2:?path to the site checkout}"
edge="${EDGE_HOST:?set EDGE_HOST or create deploy/publish.env}"

cd "$src"
npm ci --no-audit --no-fund
npm run build

# Upload into a fresh release directory, then repoint the /srv/sites/<name>
# symlink atomically so visitors never see a half-copied site. The server holds
# every file in memory, so it keeps serving the old copy until the reload.
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
rsync -a --delete dist/ "$edge:/srv/sites/releases/$name-$stamp/"
ssh "$edge" "set -e
  cd /srv/sites
  ln -sfn releases/$name-$stamp .next-$name
  mv -T .next-$name $name
  sudo systemctl reload aien-edge
  ls -1d releases/$name-* | head -n -3 | xargs -r rm -rf"
echo "published $name ($stamp)"
