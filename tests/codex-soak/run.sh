#!/bin/sh
# The delivery soak of two real Codex sessions (tests/connector/codex_real.rs, a_real_codex_delivery_soak): chat,
# card, "What??", hand-back and answer every few minutes for TROMMI_SOAK_MINUTES (default 120), with a hot swap, a
# network drop, a suspended connector, a busy turn, a burst and a late resume. Needs the codex CLI and a Codex home
# of its own for tests, logged in once (CODEX_HOME, never ~/.codex). The report goes to TROMMI_SOAK_LOG.
#   CODEX_HOME=<test home> tests/codex-soak/run.sh
set -eu
cd "$(dirname "$0")/../.."
: "${CODEX_HOME:?set CODEX_HOME to a Codex home for tests}"
export TROMMI_SOAK_MINUTES="${TROMMI_SOAK_MINUTES:-120}"
export TROMMI_SOAK_LOG="${TROMMI_SOAK_LOG:-$PWD/target/codex-soak.log}"
cargo build -p trommi-connector -p trommi-hub
exec cargo test -p trommi-tests --test connector_codex_real -- --ignored a_real_codex_delivery_soak --nocapture
