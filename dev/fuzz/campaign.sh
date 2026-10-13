#!/usr/bin/env bash
# Runs fuzz targets for a while with libFuzzer's forked workers, at low priority and with a memory cap per worker.
#   dev/fuzz/campaign.sh <seconds> <workers per target> <target>…
# Built targets come from $FUZZ_BIN (default: target/x86_64-unknown-linux-gnu/release under CARGO_TARGET_DIR).
# The corpus grows in $FUZZ_CORPUS/<target> (default tests/fuzz/corpus, git-ignored), seeded from
# tests/fuzz/seeds/<target>; crashes land in $FUZZ_ARTIFACTS/<target>/.
set -euo pipefail
here="$(cd "$(dirname "$0")/../.." && pwd)"
secs="$1"; workers="$2"; shift 2
bin="${FUZZ_BIN:-${CARGO_TARGET_DIR:-$here/tests/fuzz/target}/x86_64-unknown-linux-gnu/release}"
corpus="${FUZZ_CORPUS:-$here/tests/fuzz/corpus}"
artifacts="${FUZZ_ARTIFACTS:-$here/tests/fuzz/artifacts}"
stamp="$(date +%Y%m%d-%H%M%S)"
pids=()
for t in "$@"; do
  mkdir -p "$corpus/$t" "$artifacts/$t"
  seeds=()
  [ -d "$here/tests/fuzz/seeds/$t" ] && seeds=("$here/tests/fuzz/seeds/$t")
  dict=()
  [ -f "$here/tests/fuzz/dict/$t.dict" ] && dict=("-dict=$here/tests/fuzz/dict/$t.dict")
  nice -n 15 "$bin/$t" "$corpus/$t" "${seeds[@]}" "${dict[@]}" \
    -fork="$workers" -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 \
    -rss_limit_mb=1024 -timeout=30 -max_total_time="$secs" -max_len="${FUZZ_MAX_LEN:-65536}" \
    -artifact_prefix="$artifacts/$t/" -print_final_stats=1 \
    > "$artifacts/$t.$stamp.log" 2>&1 &
  pids+=($!)
done
rc=0
for p in "${pids[@]}"; do wait "$p" || rc=1; done
for t in "$@"; do
  log="$artifacts/$t.$stamp.log"
  execs="$(grep -o '^#[0-9]*' "$log" | tail -1 | tr -d '#')"
  echo "$t $stamp ${execs:-0}" >> "$artifacts/execs.txt"
  echo "== $t: ${execs:-0} execs, $(ls "$artifacts/$t" | grep -c -E '^(crash|leak|oom|timeout)' || true) artifacts"
done
exit $rc
