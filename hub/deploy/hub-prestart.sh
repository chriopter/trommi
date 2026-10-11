#!/bin/sh
# Runs before every start of the hub (ExecCondition of trommi-hub.service), as the hub's user:
#   hub-prestart.sh <deploy folder> <data folder> <backups folder> <runtime folder>
#
# When the release about to start is newer than the last one that started, it copies the database
# (hub.db with its -wal and -shm files: the hub stands still at this moment, so the three are one consistent state)
# and the hub's own keys (*.key) into <backups>/before-<release>-<time>/ and keeps the three newest copies. The
# files people uploaded (<data>/files) go into the copy's files/ as hard links (`cp -al`): a stored file is written
# once and never changed in place, so this costs no room and keeps each file as it was. Where hard links cannot be
# made (another file system), the copy is made without them and says so; that never stops a start.
#
# When the copy fails:
# - a NEWER release than the last one is not started on data that could not be copied first: this ends with 1, which
#   tells systemd to skip the start (no retry by itself); the updater then puts the release before back.
#   Asked a second time for the same release, it lets it start without a copy and says so: a full disk must not
#   keep the hub down for good. (The note of the first try lies in the runtime folder, which is memory.)
# The release before coming back (a rollback), or the same one, always starts, and no copy is made for it.
#
# For hub-restore.sh: <backups>/last-copy names the copy made right before the release in place first started
# (and nothing when no copy was made for it), and the hub's mark <data>/hub.served (written by the hub before its
# first request) is removed when a newer release begins on this data. A copy may be put back only while both hold.
set -u
deploy=${1:-/srv/trommi/deploy} data=${2:-/srv/trommi/data} backups=${3:-/srv/trommi/backups} run=${4:-/run/trommi-hub}
umask 077
release=$(basename "$(readlink "$deploy/current" 2>/dev/null)" 2>/dev/null)
# v<N> (the release of every part) or hub-v<N> (the hub's own releases before it)
number=${release#hub-v}
number=${number#v}
# not a release name: nothing to copy for (and nothing of that text is used any further)
case "$number" in ''|*[!0-9]*) exit 0 ;; esac
[ "hub-v$number" = "$release" ] || [ "v$number" = "$release" ] || exit 0
last=$(cat "$backups/last-release" 2>/dev/null)
[ "$last" = "$release" ] && exit 0
last_number=${last#hub-v}
last_number=${last_number#v}
case "$last_number" in ''|*[!0-9]*) last_number=0 ;; esac

# written whole and flushed, or not at all
note() { printf '%s\n' "$2" > "$backups/$1.part" && sync "$backups/$1.part" && mv -f "$backups/$1.part" "$backups/$1" && sync "$backups"; }
# Another release than the last one begins on this data: whatever copy last-copy named is no longer one to go back
# to. (Should it stay, hub-restore.sh still takes it only together with last-release naming the release in place.)
rm -f "$backups/last-copy" 2>/dev/null || true
# going back (a rollback): no copy, so that the copies made before newer releases are not pushed out by it
if [ "$number" -lt "$last_number" ]; then
  note last-release "$release" || true
  exit 0
fi
# a newer release: what it serves is its own (the mark is the hub's, written before its first request)
rm -f "$data/hub.served" 2>/dev/null || true

set -- "$data"/hub.db* "$data"/*.key
found=''
for f in "$@"; do [ -f "$f" ] && found=1; done
if [ -z "$found" ]; then
  note last-release "$release" || true
  exit 0
fi
dir="$backups/before-$release-$(printf '%012d' "$(date +%s)")"
rm -rf "${dir:?}.part"
ok=1
mkdir -p "$dir.part" || ok=''
for f in "$@"; do
  [ -f "$f" ] || continue
  [ -n "$ok" ] && { cp "$f" "$dir.part/" || ok=''; }
done
if [ -n "$ok" ] && [ -d "$data/files" ]; then
  cp -al "$data/files" "$dir.part/files" 2>/dev/null || {
    rm -rf "${dir:?}.part/files"
    echo "hub-prestart: no hard-link snapshot of the uploaded files (another file system?); the copy holds the database and keys only"
  }
fi
[ -n "$ok" ] && { sync "$dir.part"/* "$dir.part" || ok=''; }
[ -n "$ok" ] && { mv -T "$dir.part" "$dir" || ok=''; }
[ -n "$ok" ] && { note last-release "$release" || ok=''; }
# should this note not be written, the copy is still there; it is only never put back by itself
[ -n "$ok" ] && { note last-copy "$(basename "$dir")" || true; }
if [ -z "$ok" ]; then
  rm -rf "${dir:?}.part" "${dir:?}"
  if [ "$(cat "$run/copy-failed" 2>/dev/null)" = "$release" ]; then
    echo "hub-prestart: the copy before $release failed again; starting without it"
    exit 0
  fi
  printf '%s\n' "$release" > "$run/copy-failed" 2>/dev/null || true
  echo "hub-prestart: the database could not be copied before $release starts; not starting" >&2
  exit 1
fi
rm -f "${run:?}/copy-failed"
echo "hub-prestart: copied to $dir"
# the three newest stay; the names end in the time
for old in $(cd "$backups" && ls -d before-* 2>/dev/null | awk -F- '{print $NF " " $0}' | sort | awk '{print $2}' | head -n -3); do
  rm -rf "${backups:?}/$old"
done
exit 0
