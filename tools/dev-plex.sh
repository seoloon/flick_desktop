#!/usr/bin/env bash
# Disposable, *unclaimed* Plex Media Server for integration tests. Unclaimed
# servers accept unauthenticated API calls from ALLOWED_NETWORKS only; the
# plex.tv PIN flow cannot be exercised without a real Plex account.
#   tools/dev-plex.sh   -> http://localhost:32401
#   cleanup: docker rm -f oneshot-test-plex && docker volume rm oneshot-plex-config
set -euo pipefail
export MSYS_NO_PATHCONV=1  # Git Bash would rewrite /data/movies into a Windows path
PORT=${PORT:-32401}
MEDIA=${MEDIA:-$(cd "$(dirname "$0")/.." && pwd)/test-media}
B=http://localhost:$PORT
if docker ps -a --format '{{.Names}}' | grep -q '^oneshot-test-plex$'; then
  docker start oneshot-test-plex >/dev/null   # no-op if already running
else
  MSYS_NO_PATHCONV=1 docker run -d --name oneshot-test-plex -p "$PORT:32400" -e TZ=UTC \
    -e "ALLOWED_NETWORKS=172.16.0.0/12,192.168.0.0/16,10.0.0.0/8" \
    -v oneshot-plex-config:/config -v "$MEDIA:/data/movies:ro" plexinc/pms-docker:latest >/dev/null
fi
until [ "$(curl -s -o /dev/null -w '%{http_code}' "$B/identity")" = 200 ]; do sleep 2; done
# /identity reports "startState" until plugins are loaded; mutations fail before.
while curl -s -H 'Accept: application/json' "$B/identity" | grep -q startState; do sleep 3; done
if ! curl -s -H 'Accept: application/json' "$B/library/sections" | grep -q '"title":"Movies"'; then
  curl -sf -X POST -G "$B/library/sections" --data-urlencode name=Movies --data-urlencode type=movie \
    --data-urlencode agent=tv.plex.agents.none --data-urlencode "scanner=Plex Movie" \
    --data-urlencode language=xn --data-urlencode location=/data/movies >/dev/null
fi
echo "Plex ready at $B"
