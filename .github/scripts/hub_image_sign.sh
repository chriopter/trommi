#!/bin/sh
# Called only inside op run; the dedicated Cosign key signs the immutable OCI digest too.
set -eu
: "${SIGN_IMAGE_KEY:?1Password image signing key missing}"
: "${COSIGN:?Cosign path missing}"
: "${IMAGE_DIGEST:?image digest missing}"
umask 077
work=$(mktemp -d "${RUNNER_TEMP:-/tmp}/cosign-key.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM
# The owner's 1Password value is an unencrypted PKCS8 PEM. Only the ephemeral imported key uses this password.
body=$(printf '%s' "$SIGN_IMAGE_KEY" | sed -e 's/-----[A-Z ]*-----//g' | tr -d ' \n\r\t')
printf -- '-----BEGIN PRIVATE KEY-----\n%s\n-----END PRIVATE KEY-----\n' "$(printf '%s' "$body" | fold -w64)" > "$work/source.pem"
unset body
COSIGN_PASSWORD=$(openssl rand -hex 32)
export COSIGN_PASSWORD
(cd "$work" && "$COSIGN" import-key-pair --key source.pem --output-key-prefix imported >/dev/null)
key="$work/imported.key"
unset SIGN_IMAGE_KEY SIGN_IMAGE_PUBLIC_KEY SIGN_RELEASE_KEY OP_SERVICE_ACCOUNT_TOKEN OP_ENVIRONMENT_ID
"$COSIGN" public-key --key "$key" > "$work/public.pem"
cmp "$work/public.pem" release/hub-cosign-public.pem
"$COSIGN" sign --yes --key "$key" -a "commit=$GITHUB_SHA" -a "version=$VERSION" "$IMAGE_DIGEST"
"$COSIGN" verify --key release/hub-cosign-public.pem "$IMAGE_DIGEST" > /dev/null
