#!/usr/bin/env bash
# Builds what the mega test of the clients runs, per checkout ("tree"): the core for the browser (core/wasm/pkg),
# the core for Swift on this machine, and the iOS tests' runner. One heavy step at a time: on a machine that shares
# its builds, HEAVY names the lock wrapper (e.g. HEAVY="flock $HOME/.cache/trommi-work/heavy.lock").
#   tests/client/mega/prepare.sh <tree> [<tree> …]
set -euo pipefail
heavy=${HEAVY:-}
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
for tree in "$@"; do
  echo "== $tree"
  (cd "$tree" && $heavy sh core/wasm/build.sh > /dev/null)
  (cd "$tree" && $heavy sh core/swift/build.sh host > /dev/null)
  (cd "$tree" && $heavy swift build -j "$CARGO_BUILD_JOBS" --build-tests --package-path tests/ios 2>&1 | tail -1)
done
