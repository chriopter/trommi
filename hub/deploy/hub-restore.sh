#!/bin/sh
# Runs as the hub's user (trommi-hub-restore.service, started by hub-ctl.sh `restore` while the hub stands):
#   hub-restore.sh <deploy folder> <data folder> <backups folder> <result file>
#
# Puts the hub's data back to the copy hub-prestart.sh made right before the release in place first started, and
# only then: a release that failed before it served a single request leaves no change anyone made. It declines
# (`kept: <why>`, nothing changed) when
# - no copy was made right before this release started (<backups>/last-copy names none, or another release's, or
#   <backups>/last-release is not this release), or the copy is gone;
# - the hub has served a request on this data (its mark <data>/hub.served): from then on only the program goes back.
# The uploaded files come back too when the copy holds their hard-link snapshot (files/): <data>/files is put aside
# with the rest and the snapshot is linked into its place (the copy keeps its own links). A copy without the
# snapshot leaves <data>/files as it is.
# What it puts aside is never deleted here: the data as the failed release left it goes to
# <backups>/after-<release>-<time>/ (the three newest are kept).
# One line goes to <result file>: `restored <copy>; ...`, `kept: <why>` or `failed: <why>` (the data may then be
# half put back; the updater leaves the hub stopped and tries again at its next start).
set -u
deploy=${1:-/srv/trommi/deploy} data=${2:-/srv/trommi/data} backups=${3:-/srv/trommi/backups}
result=${4:-/run/trommi-hub-restore/result}
umask 077
say() { printf '%s\n' "$1" > "$result.part" && mv -f "$result.part" "$result"; printf '%s\n' "$1"; }
kept() { say "kept: $1"; exit 0; }
failed() { say "failed: $1"; exit 1; }

release=$(basename "$(readlink "$deploy/current" 2>/dev/null)" 2>/dev/null)
number=${release#hub-v}
number=${number#v}
case "$number" in ''|*[!0-9]*) kept "the hub's link names no release" ;; esac
[ "hub-v$number" = "$release" ] || [ "v$number" = "$release" ] || kept "the hub's link names no release"
copy=$(cat "$backups/last-copy" 2>/dev/null)
case "$copy" in
  "before-$release-"[0-9]*) ;;
  '') kept "no copy was made right before $release started" ;;
  *) kept "the last copy ($copy) is not the one made before $release started" ;;
esac
case "$copy" in *[!A-Za-z0-9.-]*) kept "the last copy's name is not plain" ;; esac
[ "$(cat "$backups/last-release" 2>/dev/null)" = "$release" ] || kept "$release is not the release the last copy was made for"
[ -d "$backups/$copy" ] && [ -f "$backups/$copy/hub.db" ] || kept "the copy $copy is not there"
[ -e "$data/hub.served" ] && kept "$release has served requests: its data stays"

stamp=$(printf '%012d' "$(date +%s)")
part="$data/.restore.part"
aside="$backups/after-$release-$stamp"
# 1. the copy beside the data, whole and flushed; nothing of the data is touched yet
rm -rf "${part:?}"
laid=1
mkdir "$part" || laid=''
for f in "$backups/$copy"/*; do
  [ -f "$f" ] && [ -n "$laid" ] && { cp "$f" "$part/" || laid=''; }
done
files=''
if [ -n "$laid" ] && [ -d "$backups/$copy/files" ]; then
  cp -al "$backups/$copy/files" "$part/files" && files=1 || laid=''
fi
[ -n "$laid" ] && { sync "$part"/* "$part" || laid=''; }
[ -n "$laid" ] || { rm -rf "${part:?}"; kept "the copy could not be laid out beside the data; nothing changed"; }
# 2. the data as it is goes aside; should that fail halfway, what was moved comes back
mkdir "$aside.part" || { rm -rf "${part:?}"; kept "no room to put the data aside; nothing changed"; }
moved=''
set -- "$data"/hub.db* "$data"/*.key "$data"/hub.served
[ -n "$files" ] && set -- "$@" "$data/files"
for f in "$@"; do
  [ -e "$f" ] || continue
  if mv "$f" "$aside.part/"; then
    moved="$moved $(basename "$f")"
  else
    for m in $moved; do mv "$aside.part/$m" "$data/" || failed "the data is half aside in $aside.part"; done
    rm -rf "${part:?}" "${aside:?}.part"
    kept "the data could not be put aside; nothing changed"
  fi
done
mv -T "$aside.part" "$aside" || failed "the data is aside in $aside.part; the copy is beside it in $part"
# 3. the copy in its place
for f in "$part"/*; do
  [ -e "$f" ] || continue
  mv "$f" "$data/" || failed "the copy is only partly in place; the data as it was is in $aside"
done
sync "$data" 2>/dev/null || true
rmdir "$part" 2>/dev/null || true
# the copy is used up: it is not put back a second time over data that served since
rm -f "$backups/last-copy"
sync "$backups" 2>/dev/null || true
# the three newest of what was put aside stay
for old in $(cd "$backups" && ls -d after-* 2>/dev/null | awk -F- '!/\.part$/ {print $NF " " $0}' | sort | awk '{print $2}' | head -n -3); do
  rm -rf "${backups:?}/$old"
done
say "restored $copy${files:+ with the uploaded files}; the data $release left is in $(basename "$aside")"
