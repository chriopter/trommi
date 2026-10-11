#!/bin/sh
# Expand, then contract (hub/deploy/README.md, "Migrations"): the hub of the newest release v<N> must still start
# and answer /healthz on a database that the hub of this build has migrated. Otherwise putting the program back
# after a failed deploy (the updater does that once the new hub has served) would leave a hub that cannot run.
#
#   .github/scripts/hub_compat.sh <this build's hub binary> <target>      (GH_TOKEN for gh; or OLD_HUB=<binary>)
#
# 1. The newest release's manifest and hub, the signature checked against release/public-key.pem and the binary
#    against the manifest (OLD_HUB names a binary instead, for a run by hand).
# 2. When hub/data-epoch is higher than the release's "data_epoch" (0 when it names none), this build declares a
#    migration the hub before cannot run on: the check is skipped and says so.
# 3. Otherwise: the hub before makes a database, this build's hub migrates it, the hub before starts on it again
#    and must answer /healthz with 200. Its last lines are shown when it does not.
set -eu
new=${1:?usage: $0 <hub binary> <target>}
target=${2:?usage: $0 <hub binary> <target>}
repository=${GITHUB_REPOSITORY:-trommi/trommi}
here=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'kill "$pid" 2>/dev/null || true; rm -rf "$work"' EXIT
pid=''
epoch=$(tr -d ' \n' < "$here/hub/data-epoch")

old=${OLD_HUB:-}
old_epoch=0
if [ -z "$old" ]; then
  if ! newest=$(gh release list --repo "$repository" --exclude-drafts --limit 100 --json tagName --jq '.[].tagName' | sed -n 's/^v\([1-9][0-9]*\)$/\1/p' | sort -n | tail -n 1); then
    echo "::warning::the releases could not be listed: no check against the hub before"
    exit 0
  fi
  if [ -z "$newest" ]; then
    echo "no release v<N> yet: nothing to check against"
    exit 0
  fi
  asset="trommi-hub-$target"
  gh release download "v$newest" --repo "$repository" --pattern manifest.json --pattern manifest.json.sig --pattern "$asset" --dir "$work"
  sh "$here/release/sign.sh" verify "$work/manifest.json"
  jq -e --arg r "$repository" --arg t "v$newest" '.repository == $r and .tag == $t' "$work/manifest.json" > /dev/null \
    || { echo "::error::the manifest of v$newest is not that release of this repository"; exit 1; }
  sum=$(sha256sum "$work/$asset" | cut -d' ' -f1)
  jq -e --arg n "$asset" --arg s "$sum" '[.assets[] | select(.name == $n and .sha256 == $s)] | length == 1' "$work/manifest.json" > /dev/null \
    || { echo "::error::the hub of v$newest is not the file its manifest names"; exit 1; }
  old_epoch=$(jq -r '.data_epoch // 0' "$work/manifest.json")
  old="$work/$asset"
  chmod +x "$old"
  echo "the hub before: v$newest (data epoch $old_epoch); this build: data epoch $epoch"
fi

if [ "$epoch" -gt "$old_epoch" ]; then
  echo "::notice::hub/data-epoch is $epoch, the release before has $old_epoch: this build declares a migration the hub before cannot run on. Not checked; the updater will not put the program back past it once the new hub has served."
  exit 0
fi

data="$work/data"
mkdir -p "$data"
# starts a hub on the data, waits for its port and health; prints the answer of /healthz
run() {
  : > "$work/out"
  HUB_HOST=127.0.0.1 HUB_PORT=0 HUB_DATA="$data" HUB_QUIET=1 "$1" > "$work/out" 2> "$work/err" &
  pid=$!
  port=''
  for _ in $(seq 1 60); do
    port=$(sed -n '1s/.*"port":\([0-9]*\).*/\1/p' "$work/out")
    [ -n "$port" ] && break
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.5
  done
  got=''
  if [ -n "$port" ]; then
    for _ in $(seq 1 40); do
      if got=$(curl -fsS --max-time 2 "http://127.0.0.1:$port/healthz"); then break; fi
      got=''
      kill -0 "$pid" 2>/dev/null || break
      sleep 0.5
    done
  fi
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  pid=''
  if [ -z "$got" ]; then
    tail -n 5 "$work/err" >&2
    return 1
  fi
  printf '%s\n' "$got"
}
run "$old" > /dev/null || { echo "::error::the hub before does not start on an empty folder"; exit 1; }
run "$new" > /dev/null || { echo "::error::this build's hub does not start on the database of the hub before"; exit 1; }
if ! run "$old"; then
  echo "::error::the hub before does not run on a database this build migrated. Make the migration expand-only (hub/deploy/README.md, \"Migrations\"), or raise hub/data-epoch to say that the program cannot go back past this release."
  exit 1
fi
echo "the hub before runs on data this build migrated"
