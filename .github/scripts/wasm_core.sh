#!/bin/sh
# The Rust core for the browser (core/bindings/wasm/pkg), for the web app's jobs (pr-check.yml, ship-web.yml):
#   wasm_core.sh key     the cache key: the committed inputs of the core (the core, the client core, the workspace's
#                        pins, this script). The build is reproducible: the same sources give the same bytes.
#   wasm_core.sh build   HIT=true (restored from main's cache): the files are marked as made now (the web app's build
#                        makes the core anew when they look older than its sources, app/web/dev/build.mjs) and checked
#                        against their build.json. Otherwise: wasm-bindgen of the pinned version, then core/bindings/wasm/build.sh.
# Needs the compiler with the wasm32 target (.github/scripts/rust_toolchain.sh wasm32-unknown-unknown).
set -eu
case "${1:-}" in
  key)
    tree=$(sh .github/scripts/source_tree.sh HEAD core client Cargo.toml Cargo.lock rust-toolchain.toml .github/scripts/wasm_core.sh)
    echo "wasm-pkg-v2-$(printf '%s\n' "$tree" | sha256sum | cut -d' ' -f1)"
    ;;
  build)
    if [ "${HIT:-}" = true ]; then
      touch core/bindings/wasm/pkg/*
      cat core/bindings/wasm/pkg/build.json
      for f in trommi_core_wasm_bg.wasm trommi_core_wasm.js trommi-core.js idb-store.js; do
        [ "$(sha256sum "core/bindings/wasm/pkg/$f" | cut -d' ' -f1)" = "$(jq -r --arg f "$f" '.[$f].sha256' core/bindings/wasm/pkg/build.json)" ] \
          || { echo "::error::core/bindings/wasm/pkg/$f is not the file its build.json names"; exit 1; }
      done
      exit 0
    fi
    # the command must be the version of the crate in Cargo.lock (core/bindings/wasm/build.sh checks it)
    want=$(sed -n 's/^wasm-bindgen = "=\(.*\)"$/\1/p' core/bindings/wasm/Cargo.toml)
    have=$(wasm-bindgen --version 2>/dev/null | sed 's/^wasm-bindgen //') || have=
    [ "$have" = "$want" ] || cargo install wasm-bindgen-cli --version "$want" --locked
    sh core/bindings/wasm/build.sh
    ;;
  *) echo "usage: wasm_core.sh key|build" >&2; exit 2 ;;
esac
