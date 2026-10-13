#!/bin/sh
# What a Rust check job of build.yml runs (step outputs for $GITHUB_OUTPUT):
#   compile=true|false  its clippy and tests are compiled
#   run=--no-run|       and run (empty), or only compiled (--no-run)
# CHECKED: the job's part passed its check on the pull request for this very tree (main; .github/scripts/checked_tree.sh)
# HIT:     the job's Rust cache was an exact match (Swatinem/rust-cache's cache-hit)
# Not checked: everything runs. Checked and the cache hit: nothing runs. Checked and the cache missed (a new Cargo.lock
# or compiler): the tests and clippy are compiled but not run, so that the cache main saves for the pull requests
# holds them (main is the only one that saves; a cache without them would make every pull request compile them).
set -eu
if [ "${CHECKED:-}" != true ]; then
  echo "compile=true"; echo "run="
  echo "checks run" >&2
elif [ "${HIT:-}" = true ]; then
  echo "compile=false"; echo "run="
  echo "passed on the pull request for this tree: not run again" >&2
else
  echo "compile=true"; echo "run=--no-run"
  echo "passed on the pull request for this tree: compiled for the cache, not run" >&2
fi >> "$GITHUB_OUTPUT"
