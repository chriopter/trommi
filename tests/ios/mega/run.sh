#!/usr/bin/env bash
# The iOS mega run, headless (tests/ios/TrommiCoreLiveTests/MegaRunTests.swift): a hub of its own, a BIG account seeded
# by seed-account (the demo fixture plus BULK cards) that stays online as the person's other device, then the
# engine of the app (Room, the real core) signs in, catches up, starts again warm, catches up after an away round
# and logs out. Prints one "MEGA <phase>:" line per phase; the JSON goes to OUT.
#
#   tests/ios/mega/run.sh [label]          (from the repository's root; the hub and seed-account built in release)
#
# Environment (defaults in brackets):
#   CARGO_TARGET_DIR  [target]   where trommi-hub and seed-account are (release)
#   WORK              [$TMPDIR/trommi-mega]   hub data, the seeder's folder, logs, results
#   BULK              [450]      cards on top of the fixture's (one in five stays open)
#   AWAY              ["60 80 3"]   the away round: open cards settled elsewhere, new cards, chat lines each
#   PORT              [18470]    the hub's port (the simulator run reaches it through an ssh tunnel)
#   KEEP=1                       leave the hub and the seeder running afterwards (for the simulator run)
#   SWIFT_SCRATCH     [.build]   swift's build folder for tests/ios
set -euo pipefail
root=$(cd "$(dirname "$0")/../../.." && pwd)
label=${1:-run}
bin=${CARGO_TARGET_DIR:-$root/target}/release
work=${WORK:-${TMPDIR:-/tmp}/trommi-mega}
port=${PORT:-18470}
mkdir -p "$work"
curl -sf "http://127.0.0.1:$port/healthz" > /dev/null && { echo "a hub answers on port $port already: stop it first"; exit 1; }
rm -rf "$work/hub-data" "$work/serve" "$work/seed.env"
mkdir -p "$work/hub-data"

env HUB_HOST=127.0.0.1 HUB_PORT="$port" HUB_DATA="$work/hub-data" HUB_QUIET=1 HUB_LOGIN_THROTTLE=off \
  HUB_LIMIT_ENVELOPES_PER_SECOND=100000 HUB_LIMIT_ENVELOPE_BURST=100000 HUB_LIMIT_HEAVY_PER_SECOND=10000 HUB_LIMIT_HEAVY_BURST=10000 \
  HUB_LIMIT_FOUND_PER_IP_HOUR=1000 HUB_LIMIT_LOGINS_PER_IP_10MIN=10000 HUB_LIMIT_OPEN_REQUESTS_PER_IP_MINUTE=100000 \
  HUB_LIMIT_PIECES_PER_SECOND=10000 HUB_ADMITTED_PER_ADDRESS=4096 HUB_ADMITTED=8192 HUB_ADMIN_PORT=$((port + 1)) \
  "$bin/trommi-hub" > "$work/hub.log" 2>&1 &
hub=$!
seeder=
stop() { [ -n "${KEEP:-}" ] && return; touch "$work/serve/stop" 2>/dev/null || true; sleep 1; [ -n "$seeder" ] && kill "$seeder" 2>/dev/null; kill "$hub" 2>/dev/null || true; }
trap stop EXIT
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$port/healthz" > /dev/null && break; sleep 0.2; done

"$bin/seed-account" --hub "http://127.0.0.1:$port" --email "mega-$label-$(date +%s)@example.org" --env-out "$work/seed.env" \
  --pace-ms 0 --bulk "${BULK:-450}" --serve "$work/serve" > "$work/seed-$label.log" 2>&1 &
seeder=$!
started=$(date +%s)
until [ -e "$work/serve/ready" ]; do
  kill -0 "$seeder" 2>/dev/null || { echo "the seeder ended:"; tail -5 "$work/seed-$label.log"; exit 1; }
  sleep 2
done
echo "seeded in $(( $(date +%s) - started )) s"
[ -n "${SEED_ONLY:-}" ] && { KEEP=1; echo "hub http://127.0.0.1:$port, account in $work/seed.env"; exit 0; }

cd "$root/tests/ios"
TROMMI_MEGA_HUB="http://127.0.0.1:$port" TROMMI_MEGA_ENV="$work/seed.env" TROMMI_MEGA_SERVE="$work/serve" \
  TROMMI_MEGA_OUT="$work/$label.json" TROMMI_MEGA_AWAY="${AWAY:-60 80 3}" TROMMI_SIGNIN_LOG=1 \
  swift test --skip-build -c release --scratch-path "${SWIFT_SCRATCH:-.build}" --filter MegaRunTests 2>&1 | tee "$work/$label.log" | grep -E '^MEGA|^sign-in|error|failed'
