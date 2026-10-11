#!/usr/bin/env bash
# The iOS speed run in the simulator (Perf.swift SpeedScript): the app, built on a Mac (Release, for the simulator),
# on the big made-up room of tests/tools/mega-fixture.mjs, no hub, no network. One launch: cold start to the Desk,
# Desk scroll, news in bursts, the 5,000-item chat opened, scrolled, news there and elsewhere, typing; each step's
# hitches (frames later than 16.7 ms), stalls and memory. With SLOW=1 the app runs on the Mac's efficiency cores
# (taskpolicy -b), nearer to an older iPhone.
#
#   tests/ios/speed/sim.sh                 # build, run, print the SPEED/WINDOW lines
#   BEFORE=<ref> HARNESS=<commit> tests/ios/speed/sim.sh   # <ref>'s app with the harness of <commit> put in
#
# Environment: MAC [trommidev@mac], MAC_KEY (optional), SIM [speed iPhone], RUNS [3], SLOW [0], KEEP_BUILD [0]
set -euo pipefail
repo=$(cd "$(dirname "$0")/../../.." && pwd)
mac=${MAC:-trommidev@mac}
ssh_opts=(-o IdentitiesOnly=yes -o ServerAliveInterval=15)
[ -n "${MAC_KEY:-}" ] && ssh_opts+=(-i "$MAC_KEY")
sim=${SIM:-speed iPhone}
on_mac() { ssh "${ssh_opts[@]}" "$mac" "$@"; }
dir=ios-speed${BEFORE:+-before}
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT

# the tree to build: this checkout's app (or BEFORE's, with the harness commit's app changes put in), its core's
# Swift package (the library: MAC_CORE_LIB, a core/swift/TrommiCoreRust on the Mac that core/swift/build.sh filled), the demo
mkdir -p "$tmp/src"
if [ -n "${BEFORE:-}" ]; then
  git -C "$repo" archive "$BEFORE" app/ios core/swift demo CHANGELOG.md | tar xf - -C "$tmp/src"
  git -C "$repo" diff "${HARNESS:?HARNESS=<the harness commit>}~1" "$HARNESS" -- app/ios | (cd "$tmp/src" && git apply)
else
  git -C "$repo" ls-files -z app/ios core/swift demo CHANGELOG.md | (cd "$repo" && xargs -0 tar cf -) | tar xf - -C "$tmp/src"
fi
node "$repo/tests/tools/mega-fixture.mjs" "$tmp/mega.json" > /dev/null
on_mac "rm -rf ~/$dir && mkdir -p ~/$dir"
tar cf - -C "$tmp" src mega.json | on_mac "tar xf - -C ~/$dir"
lib=${MAC_CORE_LIB:-'~/ios-mega/core/swift/TrommiCoreRust'}
on_mac "set -e; cd ~/$dir/src; mkdir -p core/swift/TrommiCoreRust/lib; cp -R $lib/lib/ios-simulator core/swift/TrommiCoreRust/lib/; cp $lib/Sources/TrommiCoreRust/TrommiCoreRust.swift core/swift/TrommiCoreRust/Sources/TrommiCoreRust/; mkdir -p core/swift/TrommiCoreRust/Sources/TrommiCoreFFI/include; cp $lib/Sources/TrommiCoreFFI/include/TrommiCoreFFI.h core/swift/TrommiCoreRust/Sources/TrommiCoreFFI/include/
cd app/ios/TrommiApp/AppStore && ~/bin/xcodegen/bin/xcodegen generate --spec project.yml --quiet
TROMMI_IOS_SIMULATOR=1 xcodebuild build -project Trommi.xcodeproj -scheme Trommi -configuration Release -destination 'generic/platform=iOS Simulator' -derivedDataPath ~/Library/Caches/$dir-derived \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual CODE_SIGNING_REQUIRED=NO DEVELOPMENT_TEAM= ARCHS=arm64 ONLY_ACTIVE_ARCH=YES > ~/Library/Caches/$dir-build.log 2>&1 || { grep -E 'error:' ~/Library/Caches/$dir-build.log | sort -u | head -30; exit 1; }"
app="~/Library/Caches/$dir-derived/Build/Products/Release-iphonesimulator/Trommi.app"

udid=$(on_mac "xcrun simctl list devices available | grep -F '    $sim (' | tail -1 | sed -E 's/.*\(([0-9A-F-]{36})\).*/\1/'")
if [ -z "$udid" ]; then
  udid=$(on_mac "xcrun simctl create '$sim' 'iPhone 17e' \$(xcrun simctl list runtimes | grep -o 'com.apple.CoreSimulator.SimRuntime.iOS-27[-0-9]*' | tail -1)")
fi
on_mac "xcrun simctl boot $udid 2>/dev/null; xcrun simctl bootstatus $udid -b > /dev/null; xcrun simctl terminate $udid com.trommi.ios 2>/dev/null; xcrun simctl uninstall $udid com.trommi.ios 2>/dev/null; xcrun simctl install $udid $app"
for run in $(seq 1 "${RUNS:-3}"); do
  on_mac "data=\$(xcrun simctl get_app_container $udid com.trommi.ios data); rm -f \"\$data/Documents/perf.log\"; xcrun simctl terminate $udid com.trommi.ios 2>/dev/null; sleep 1
    pid=\$(env SIMCTL_CHILD_TROMMI_PERF=1 SIMCTL_CHILD_TROMMI_SPEED=1 SIMCTL_CHILD_TROMMI_SPEED_FIXTURE=\$HOME/$dir/mega.json xcrun simctl launch $udid com.trommi.ios | sed -E 's/.*: //')
    [ '${SLOW:-0}' = 1 ] && taskpolicy -b -p \$pid; true"
  log=""
  for _ in $(seq 1 180); do
    log=$(on_mac "cat \"\$(xcrun simctl get_app_container $udid com.trommi.ios data)/Documents/perf.log\" 2>/dev/null" || true)
    grep -q 'SPEED done' <<< "$log" && break
    sleep 1
  done
  echo "== run $run${BEFORE:+ (before: $BEFORE)}${SLOW:+ slow=$SLOW}"
  grep -E 'SPEED|WINDOW|cold start|speed run' <<< "$log" | cut -d' ' -f3- | cut -c1-300
  echo "   stalls > 30 ms: $(grep -c STALL <<< "$log" || true); the longest: $(grep -o 'STALL [0-9]*' <<< "$log" | sort -k2 -n | tail -1)"
  grep STALL <<< "$log" | sort -t' ' -k4 -n | tail -2 | cut -c1-400 || true
done
on_mac "xcrun simctl terminate $udid com.trommi.ios 2>/dev/null; true"
[ "${KEEP_BUILD:-0}" = 1 ] || on_mac "rm -rf ~/Library/Caches/$dir-derived ~/Library/Caches/$dir-build.log"
