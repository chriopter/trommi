#!/bin/sh
# Match the deployment verifier; never install an unverified download.
set -eu
output=${1:?usage: install_cosign.sh output}
curl -fsSL --retry 3 https://github.com/sigstore/cosign/releases/download/v3.1.3/cosign-linux-amd64 -o "$output"
printf '%s  %s\n' 4629c757b7618056f8ddd7e2625ae9fdd94c0372a65049520bc7d9df9efc7f71 "$output" | sha256sum -c -
chmod 700 "$output"
