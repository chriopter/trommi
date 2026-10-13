#!/usr/bin/env bash
# Adds a fuzz target that runs trommi_tests::fuzz::<module>::run: ./add_target.sh <name> <module>
set -euo pipefail
cd "$(dirname "$0")"
printf '#![no_main]\nlibfuzzer_sys::fuzz_target!(|data: &[u8]| trommi_tests::fuzz::%s::run(data));\n' "$2" > "fuzz_targets/$1.rs"
printf '\n[[bin]]\nname = "%s"\npath = "fuzz_targets/%s.rs"\ntest = false\ndoc = false\nbench = false\n' "$1" "$1" >> Cargo.toml
