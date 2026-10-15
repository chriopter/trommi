#!/bin/sh
# Bind an artifact to the digest exported by its producing GitHub job, before secrets enter a command.
set -eu
expected=${1:?expected SHA-256}
file=${2:?artifact file}
printf '%s' "$expected" | grep -Eq '^[0-9a-f]{64}$' || { echo 'invalid producer digest' >&2; exit 1; }
actual=$(shasum -a 256 "$file" | awk '{print $1}')
[ "$actual" = "$expected" ] || { echo "::error::$file is not the artifact its producing job checked"; exit 1; }
