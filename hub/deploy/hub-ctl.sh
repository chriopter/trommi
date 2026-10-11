#!/bin/sh
# Runs as root for one connection to trommi-hub-ctl.socket (see trommi-hub-ctl@.service). It reads one line. The
# line must be exactly one of four words; then it does that and answers on one line. Nothing the caller sends
# reaches a command: the commands are written out below, and the units they start are fixed on the server.
#   start    starts the hub's unit                                 ok | failed: <why>
#   stop     stops it                                              ok | failed: <why>
#   dryrun   runs trommi-hub-dryrun.service: the release behind /srv/trommi/deploy/candidate tried on a copy of the
#            data, as the hub's user, without network               ok <verdict> | failed: <why>
#   restore  runs trommi-hub-restore.service while the hub stands: the data put back to the copy made before the
#            release in place started, only while that release has served no request
#                                                                  ok restored <what> | kept: <why> | failed: <why>
# What the two units found, they write to /run/trommi-hub-dryrun/result and /run/trommi-hub-restore/result (each
# removes its old result before it runs).
run=${TROMMI_CTL_RUN:-/run}
first() { printf '%s' "$1" | head -n 1 | cut -c1-300; }
IFS= read -r verb || true
case "$verb" in
  start)
    # a start the unit's own pre-start check declined (no copy of the database could be made) is no start
    out=$(systemctl start trommi-hub.service 2>&1) && systemctl is-active --quiet trommi-hub.service && { echo ok; exit 0; }
    [ -n "$out" ] || out="the hub's unit did not start (journalctl -u trommi-hub -n 20)"
    ;;
  stop) out=$(systemctl stop trommi-hub.service 2>&1) && { echo ok; exit 0; } ;;
  dryrun)
    rm -f "$run/trommi-hub-dryrun/result" 2>/dev/null
    # the unit succeeds only when the dry run held (hub-dryrun.sh ends with the program's exit code)
    out=$(systemctl start trommi-hub-dryrun.service 2>&1) && ok=1 || ok=''
    said=$(first "$(cat "$run/trommi-hub-dryrun/result" 2>/dev/null)")
    [ -n "$ok" ] && { echo "ok $said"; exit 0; }
    [ -n "$said" ] && out=$said
    [ -n "$out" ] || out="the dry run did not finish (journalctl -u trommi-hub-dryrun -n 20)"
    ;;
  restore)
    if systemctl is-active --quiet trommi-hub.service; then echo "failed: the hub runs; it is stopped first"; exit 0; fi
    rm -f "$run/trommi-hub-restore/result" 2>/dev/null
    out=$(systemctl start trommi-hub-restore.service 2>&1)
    said=$(first "$(cat "$run/trommi-hub-restore/result" 2>/dev/null)")
    case "$said" in
      "restored "*) echo "ok $said"; exit 0 ;;
      "kept: "*) echo "$said"; exit 0 ;;
      "failed: "*) out=${said#failed: } ;;
      *) [ -n "$out" ] || out="the restore did not finish (journalctl -u trommi-hub-restore -n 20)" ;;
    esac
    ;;
  *) echo "failed: only start, stop, dryrun and restore are taken"; exit 0 ;;
esac
echo "failed: $(first "$out")"
