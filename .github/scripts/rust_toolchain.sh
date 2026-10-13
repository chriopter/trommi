#!/bin/sh
# Installs the Rust compiler that rust-toolchain.toml pins, with only the targets (and components) one CI job needs,
# and makes it this checkout's compiler. `rustup toolchain install` alone installs every target the file lists (seven
# besides the machine's own: ten downloads, about 20 seconds per job); a job that builds for one target needs one.
#
#   sh .github/scripts/rust_toolchain.sh [--component <name>]... [<target>]...
#
# The compiler is the very one the file names (its channel); only the set of targets differs. The directory override
# (`rustup override set`, for this checkout only) takes precedence over the file, so rustup does not fetch the file's
# other targets when cargo starts. Run it from the root of the checkout.
set -eu
[ -f rust-toolchain.toml ] || { echo "rust_toolchain.sh: run it from the root of the checkout" >&2; exit 1; }
channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' rust-toolchain.toml)
[ -n "$channel" ] || { echo "rust_toolchain.sh: rust-toolchain.toml names no channel" >&2; exit 1; }
profile=$(sed -n 's/^profile = "\(.*\)"$/\1/p' rust-toolchain.toml)
args=
while [ $# -gt 0 ]; do
  case $1 in
    --component) args="$args --component $2"; shift 2 ;;
    -*) echo "rust_toolchain.sh: unknown option $1" >&2; exit 1 ;;
    *)
      # only a target the file lists: a job never builds for one the workspace does not pin
      grep -Eq "^  \"$1\",?$" rust-toolchain.toml || { echo "rust_toolchain.sh: rust-toolchain.toml does not list the target $1" >&2; exit 1; }
      args="$args --target $1"
      shift
      ;;
  esac
done
# shellcheck disable=SC2086
rustup toolchain install "$channel" --profile "${profile:-minimal}" --no-self-update $args
rustup override set "$channel"
rustc --version
