#!/usr/bin/env bash
# The iOS mega run in the simulator: the app itself (built on a Mac, Release, for the simulator) against the hub and
# the big account of run.sh, reached through an ssh tunnel. Four launches: cold login, warm start, start after an
# away round of the other device, then logout. The app writes its perf log (TROMMI_PERF, TROMMI_MEGA: Perf.swift);
# this prints its MEGA, WINDOW and STALL lines per launch.
#
#   SEED_ONLY=1 KEEP=1 tests/ios/mega/run.sh sim     # first: hub and account, left running
#   tests/ios/mega/sim.sh                            # then this, from the same machine
#
# Environment (defaults in brackets):
#   MAC     [trommidev@mac]   ssh target of the Mac; MAC_KEY: its ssh key (optional)
#   APP_PATH  the built Trommi.app on the Mac [~/Library/Caches/mega-derived/Build/Products/Release-iphonesimulator/Trommi.app]
#   SIM     [mega iPhone]     the simulator's name (made as an iPhone 18 Pro on iOS 27 if missing)
#   WORK, PORT                as for run.sh
#   AWAY    ["60 80 3"]       the away round
set -euo pipefail
work=${WORK:-${TMPDIR:-/tmp}/trommi-mega}
port=${PORT:-18470}
mac=${MAC:-trommidev@mac}
ssh_opts=(-o IdentitiesOnly=yes -o ServerAliveInterval=15)
[ -n "${MAC_KEY:-}" ] && ssh_opts+=(-i "$MAC_KEY")
app=${APP_PATH:-'~/Library/Caches/mega-derived/Build/Products/Release-iphonesimulator/Trommi.app'}
sim=${SIM:-mega iPhone}
. <(grep -E '^APPSTORE_DEMO_(USER|PASSWORD)=' "$work/seed.env")
on_mac() { ssh "${ssh_opts[@]}" "$mac" "$@"; }

# the tunnel: the Mac's 127.0.0.1:$port is this machine's hub
ssh "${ssh_opts[@]}" -N -R "$port:127.0.0.1:$port" "$mac" &
tunnel=$!
trap 'kill $tunnel 2>/dev/null' EXIT
sleep 2
on_mac "curl -sf http://127.0.0.1:$port/healthz > /dev/null" || { echo "the hub is not reachable from the Mac"; exit 1; }

udid=$(on_mac "xcrun simctl list devices available | grep -F '    $sim (' | tail -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/'")
if [ -z "$udid" ]; then
  udid=$(on_mac "xcrun simctl create '$sim' 'iPhone 18 Pro' \$(xcrun simctl list runtimes | grep -o 'com.apple.CoreSimulator.SimRuntime.iOS-27[-0-9]*' | tail -1)")
fi
on_mac "xcrun simctl boot $udid 2>/dev/null; xcrun simctl bootstatus $udid -b > /dev/null; xcrun simctl terminate $udid com.trommi.ios 2>/dev/null; xcrun simctl uninstall $udid com.trommi.ios 2>/dev/null; xcrun simctl install $udid $app"

# one launch: env for the app, then wait until its log says the first catch-up ended (or `until`)
launch() {   # launch <label> <until regex> [KEY=VALUE…]
  local label=$1 until=$2; shift 2
  local envs="SIMCTL_CHILD_TROMMI_PERF=1 SIMCTL_CHILD_TROMMI_MEGA=1 SIMCTL_CHILD_TROMMI_HUB_URL=http://127.0.0.1:$port"
  for kv in "$@"; do envs="$envs SIMCTL_CHILD_$kv"; done
  on_mac "data=\$(xcrun simctl get_app_container $udid com.trommi.ios data); rm -f \"\$data/Documents/perf.log\"; xcrun simctl terminate $udid com.trommi.ios 2>/dev/null; env $envs xcrun simctl launch $udid com.trommi.ios > /dev/null"
  local log=""
  for _ in $(seq 1 600); do
    log=$(on_mac "cat \"\$(xcrun simctl get_app_container $udid com.trommi.ios data)/Documents/perf.log\" 2>/dev/null" || true)
    grep -Eq "$until" <<< "$log" && break
    on_mac "xcrun simctl spawn $udid launchctl list 2>/dev/null | grep -q com.trommi.ios" || { echo "$label: THE APP IS NOT RUNNING (crashed?)"; break; }
    sleep 1
  done
  sleep 2
  log=$(on_mac "cat \"\$(xcrun simctl get_app_container $udid com.trommi.ios data)/Documents/perf.log\" 2>/dev/null" || true)
  echo "== $label"
  grep -E 'MEGA|WINDOW|cold start' <<< "$log" | cut -d' ' -f3- | cut -c1-600
  echo "   stalls > 30 ms: $(grep -c STALL <<< "$log" || true); the longest: $(grep -o 'STALL [0-9]*' <<< "$log" | sort -k2 -n | tail -1)"
  grep STALL <<< "$log" | sort -t' ' -k4 -n | tail -3 | cut -c1-400
  printf '%s\n' "$log" > "$work/sim-$label.log"
}

launch cold 'MEGA sync 1 ended' "TROMMI_MEGA_EMAIL=$APPSTORE_DEMO_USER" "TROMMI_MEGA_PASSWORD=$APPSTORE_DEMO_PASSWORD"
launch warm 'MEGA sync 1 ended'
cmd="$work/serve/sim-$(date +%s).cmd"
echo "${AWAY:-60 80 3}" > "$cmd"
until [ -e "${cmd%.cmd}.done" ]; do sleep 2; done
launch away 'MEGA sync 1 ended'
launch logout 'MEGA alive after logout|THE APP' TROMMI_MEGA_LOGOUT=1
on_mac "xcrun simctl spawn $udid launchctl list 2>/dev/null | grep -q com.trommi.ios" && echo "after logout: the app runs" || echo "after logout: THE APP IS GONE"
