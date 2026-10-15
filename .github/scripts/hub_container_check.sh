#!/bin/sh
# A fresh isolated data tmpfs, no network, no published port, no secrets. Runs only in the producing build job.
set -eu
binary=${1:?usage: hub_container_check.sh binary}
context=$(mktemp -d)
name="trommi-hub-check-$(basename "$context" | tr '[:upper:]' '[:lower:]')"
owned_container=''
trap '[ -z "$owned_container" ] || docker rm -f "$owned_container" >/dev/null 2>&1 || true; rm -rf "$context"' EXIT INT TERM
cp "$binary" "$context/trommi-hub"
cp hub/container/Dockerfile "$context/Dockerfile"
docker build --label "io.yolodeploy.sequence=${VERSION:-0}" -t "$name" "$context"
owned_container=$(docker run -d --name "$name" --read-only --cap-drop=ALL --security-opt=no-new-privileges --network=none \
  --tmpfs /tmp --tmpfs /data:uid=65532,gid=65532 -e HUB_QUIET=1 "$name")
for _ in $(seq 1 60); do
  state=$(docker inspect --format '{{.State.Health.Status}}' "$name")
  [ "$state" != unhealthy ] || { echo '::error::container is unhealthy'; exit 1; }
  if [ "$state" = healthy ]; then
    [ "$(docker inspect --format '{{.Config.User}}' "$name")" = 65532:65532 ]
    [ "$(docker exec "$name" /usr/local/bin/trommi-hub version)" = "$GITHUB_SHA" ]
    if [ -n "${VERSION:-}" ]; then
      [ "$(docker inspect --format '{{index .Config.Labels "io.yolodeploy.sequence"}}' "$name")" = "$VERSION" ]
    fi
    echo 'Isolated non-root container healthy; expected build identity verified.'
    exit 0
  fi
  sleep 1
done
echo '::error::container healthcheck did not pass'
exit 1
