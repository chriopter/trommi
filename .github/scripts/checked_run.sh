#!/bin/sh
# What a Rust check job of build.yml runs (step outputs for $GITHUB_OUTPUT):
#   compile=true|false  its clippy and tests are compiled
#   run=--no-run|       and run (empty), or only compiled (--no-run)
#   profile=fast|ci     the nextest profile (.config/nextest.toml): fast leaves the slow scenario tests out
# TIER:    fast (a pull request: clippy and every test but the slow ones) or full (the merge queue, main, by hand)
# CHECKED: the job's part passed its check in the merge queue for this very tree (main; .github/scripts/checked_tree.sh)
# HIT:     the job's Rust cache was an exact match (Swatinem/rust-cache's cache-hit)
# Not checked: the tier runs. Checked and the cache hit: nothing runs. Checked and the cache missed (a new Cargo.lock
# or compiler): the tests and clippy are compiled but not run, so that the cache main saves for the pull requests
# holds them (main is the only one that saves; a cache without them would make every pull request compile them).
set -eu
profile=ci
[ "${TIER:-full}" = fast ] && profile=fast
if [ "${CHECKED:-}" != true ]; then
  echo "compile=true"; echo "run="; echo "profile=$profile"
  echo "checks run ($profile)" >&2
elif [ "${HIT:-}" = true ]; then
  echo "compile=false"; echo "run="; echo "profile=ci"
  echo "passed before the merge for this tree: not run again" >&2
else
  echo "compile=true"; echo "run=--no-run"; echo "profile=ci"
  echo "passed before the merge for this tree: compiled for the cache, not run" >&2
fi >> "$GITHUB_OUTPUT"
