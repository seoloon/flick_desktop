#!/usr/bin/env bash
# Starts a disposable Jellyfin server with the synthetic corpus for
# integration tests (tests are skipped unless ONESHOT_JELLYFIN_URL is set).
#   tools/dev-jellyfin.sh          -> http://localhost:18096  user oneshot / oneshot
#   docker rm -f oneshot-test-jellyfin && docker volume rm oneshot-jf-config oneshot-jf-cache   (cleanup)
set -euo pipefail
PORT=${PORT:-18096}
MEDIA=${MEDIA:-$(cd "$(dirname "$0")/.." && pwd)/test-media}
B=http://localhost:$PORT
AUTH='MediaBrowser Client="Flick-dev", Device="dev", DeviceId="oneshot-dev-setup", Version="0.1.0"'

if docker ps -a --format '{{.Names}}' | grep -q '^oneshot-test-jellyfin$'; then
  docker start oneshot-test-jellyfin >/dev/null   # no-op if already running
else
  MSYS_NO_PATHCONV=1 docker run -d --name oneshot-test-jellyfin -p "$PORT:8096" \
    -v oneshot-jf-config:/config -v oneshot-jf-cache:/cache -v "$MEDIA:/media/movies:ro" jellyfin/jellyfin:latest >/dev/null
fi
until curl -sf "$B/System/Info/Public" | grep -q Version; do sleep 2; done

if curl -s "$B/System/Info/Public" | grep -q '"StartupWizardCompleted":false'; then
  curl -sf -X POST "$B/Startup/Configuration" -H 'Content-Type: application/json' \
    -d '{"UICulture":"en-US","MetadataCountryCode":"US","PreferredMetadataLanguage":"en"}'
  curl -sf "$B/Startup/User" >/dev/null
  curl -sf -X POST "$B/Startup/User" -H 'Content-Type: application/json' -d '{"Name":"oneshot","Password":"oneshot"}'
  curl -sf -X POST "$B/Startup/RemoteAccess" -H 'Content-Type: application/json' -d '{"EnableRemoteAccess":true}'
  curl -sf -X POST "$B/Startup/Complete"
fi

TOKEN=$(curl -sf -X POST "$B/Users/AuthenticateByName" -H "Authorization: $AUTH" -H 'Content-Type: application/json' \
  -d '{"Username":"oneshot","Pw":"oneshot"}' | sed -E 's/.*"AccessToken":"([^"]+)".*/\1/')
if ! curl -sf "$B/Library/VirtualFolders" -H "Authorization: $AUTH, Token=\"$TOKEN\"" | grep -q '"Movies"'; then
  curl -sf -X POST "$B/Library/VirtualFolders?name=Movies&collectionType=movies&paths=%2Fmedia%2Fmovies&refreshLibrary=true" \
    -H "Authorization: $AUTH, Token=\"$TOKEN\"" -H 'Content-Type: application/json' -d '{"LibraryOptions":{}}'
fi
echo "Jellyfin ready at $B (user oneshot/oneshot)"
