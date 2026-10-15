# Trommi for Linux

Qt/QML drawing the [shared Rust app core](../../core/client/README.md). `App` owns account flows, sync, actions,
cache, the board model and every screen's ordered ViewModel. `trommi-io` executes HTTP, raw streams, storage and
timers with a device-write barrier. The app forwards taps/focus and draws renders; it keeps no second engine,
board model or list-order policy.

## Layout

| Path | Responsibility |
| --- | --- |
| `src/shell.rs` | Runs AppCore through `trommi-io`, folds renders and forwards platform effects |
| `src/platform.rs`, `src/notify.rs` | Desktop/file/notification services |
| `src/` and `qml/` | cxx-qt properties, native window, UI input and QML drawing |
| `dev/pen.mjs` | Generated pen drawing data, checked with `--check` |
| `tests/` | Shell, drawing and local-hub integration tests |

This is a separate Cargo workspace so the root Rust workspace does not require Qt. Its dependencies on
`core/client` and `core/io` are direct path dependencies. The stored device has one owner; cache data is disposable.
The core handles sign-out, signed-removal verification, history, retries and refusal rollback.

## Build and check

Requires the pinned Rust toolchain, Qt 6 development tools and `qmake6`. From the repository root, one build at a
time:

```sh
CARGO_BUILD_JOBS=4 QMAKE=qmake6 nice -n 19 ionice -c3 cargo build --locked -j4 \
  --manifest-path app/linux/Cargo.toml
QMAKE=qmake6 nice -n 19 ionice -c3 cargo test --locked -j4 --manifest-path app/linux/Cargo.toml
QMAKE=qmake6 nice -n 19 ionice -c3 cargo clippy --locked -j4 --manifest-path app/linux/Cargo.toml \
  --all-targets -- -D warnings
node app/linux/dev/pen.mjs --check
```

Run `app/linux/target/debug/trommi-linux` (or the binary under your `CARGO_TARGET_DIR`). For an isolated smoke test,
set `TROMMI_LINUX_DIR` to an empty temporary folder, `QT_QPA_PLATFORM=offscreen`, `TROMMI_LINUX_SHOT` to the output
PNG path and `TROMMI_LINUX_SHOT_AFTER_MS=500`. The picture test is opt-in; ordinary tests need no owner account.
`ship-linux.yml` runs checks and the offscreen smoke on main; it does not publish an installer yet.
