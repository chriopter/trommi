#!/usr/bin/env bash
# Puts the hub's secret settings from the 1Password Environment into the container's configuration, run on the
# owner's machine from a checkout of this repository:
#
#   op run --environment <hub production environment id> -- hub/container/secrets.sh
#
# Today it carries one: HUB_TINFOIL_API_KEY (sealed dictation's relay, spec/v1.md 18). It replaces that line of
# /home/docker/trommi/config/hub-secrets.env (every other line is kept), recreates the container so that it reads
# it, and waits for the hub to be healthy. The value travels on standard input, never on a command line, and is
# never printed: only its name and whether it arrived.
#
# It reaches the server with ssh as root (1Password approves the key): HUB_SSH (default
# root@yolodeploy.tail276436.ts.net), HUB_SSH_PORT (default 22). `secrets.sh check` checks this script alone.
set -euo pipefail

HUB_SSH=${HUB_SSH:-root@yolodeploy.tail276436.ts.net}
HUB_SSH_PORT=${HUB_SSH_PORT:-22}

die() { printf 'error: %s\n' "$*" >&2; exit 1; }

if [ -z "${SSH_AUTH_SOCK:-}" ] && [ -S "$HOME/.1password/agent.sock" ]; then
  export SSH_AUTH_SOCK="$HOME/.1password/agent.sock"
fi

remote() {
  cat <<'REMOTE'
set -euo pipefail
DIR=/home/docker/trommi
FILE=$DIR/config/hub-secrets.env
IFS= read -r key
[ -n "$key" ] || { echo "the key did not arrive" >&2; exit 1; }
umask 077
{ grep -v '^HUB_TINFOIL_API_KEY=' "$FILE" 2>/dev/null || true
  printf 'HUB_TINFOIL_API_KEY=%s\n' "$key"
} > "$FILE.part"
chown --reference="$DIR/config" "$FILE.part" 2>/dev/null || true
chmod 0600 "$FILE.part"
mv -f "$FILE.part" "$FILE"
unset key
echo "written: HUB_TINFOIL_API_KEY in $FILE"
cd "$DIR"
docker compose up -d --force-recreate trommi >/dev/null
for _ in $(seq 1 30); do
  state=$(docker inspect -f '{{.State.Health.Status}}' trommi 2>/dev/null || echo none)
  [ "$state" = healthy ] && { echo "the hub is healthy and reads the key"; exit 0; }
  sleep 2
done
echo "the hub is NOT healthy after the restart: docker logs --tail 30 trommi" >&2
exit 1
REMOTE
}

case "${1:-send}" in
  check)
    remote > /dev/null
    bash -n <(remote)
    if shellcheck --version >/dev/null 2>&1; then shellcheck -s bash -S warning <(remote); fi
    echo "check: ok"
    ;;
  send)
    [ -n "${HUB_TINFOIL_API_KEY:-}" ] || die "HUB_TINFOIL_API_KEY is not set; run this under: op run --environment <hub production environment id> -- $0"
    case "$HUB_TINFOIL_API_KEY" in *[!A-Za-z0-9._-]*) die "HUB_TINFOIL_API_KEY holds characters a key does not have (a space or a line break?)" ;; esac
    echo "Send HUB_TINFOIL_API_KEY to $HUB_SSH (its value is never shown)"
    printf '%s\n' "$HUB_TINFOIL_API_KEY" | ssh -p "$HUB_SSH_PORT" -o StrictHostKeyChecking=accept-new -o ControlPath=none -o ConnectTimeout=15 "$HUB_SSH" "bash -c $(printf '%q' "$(remote)")"
    ;;
  *) die "usage: $0 [send|check]" ;;
esac
