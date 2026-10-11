#!/bin/sh
# Runs as the hub's user (trommi-hub-dryrun.service, started by hub-ctl.sh `dryrun` when the updater asks, before
# it swaps the hub):
#   hub-dryrun.sh <candidate release folder> <data folder> <scratch folder> <result file> [seconds]
#
# Tries the hub of the release the updater is about to put in place (the link <deploy>/candidate) on a copy of the
# live data: `trommi-hub dry-run` (hub/src/dry_run.rs) copies the database while the running hub goes on, starts
# itself on the copy and checks it. The unit gives it no network (only its own loopback) and the live data only to
# read. Its one line of JSON goes to <result file>; this script ends with its exit code (0: the dry run held).
set -u
candidate=${1:-/srv/trommi/deploy/candidate} data=${2:-/srv/trommi/data}
scratch=${3:-/var/cache/trommi-hub-dryrun} result=${4:-/run/trommi-hub-dryrun/result} seconds=${5:-60}
umask 077
say() { printf '%s\n' "$1" > "$result.part" && mv -f "$result.part" "$result"; printf '%s\n' "$1"; }
[ -x "$candidate/trommi-hub" ] || { say '{"ok":false,"detail":"no candidate release to try"}'; exit 1; }
rm -rf "${scratch:?}/copy"
out=$("$candidate/trommi-hub" dry-run "$data" "$scratch/copy" "$seconds")
code=$?
rm -rf "${scratch:?}/copy"
line=$(printf '%s\n' "$out" | tail -n 1)
[ -n "$line" ] || line="{\"ok\":false,\"detail\":\"the candidate said nothing (exit $code)\"}"
say "$line"
exit "$code"
