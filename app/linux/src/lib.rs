//! Trommi for Linux: the shell over the app core (`shell.rs`), the desktop's services (`platform.rs`, `notify.rs`),
//! and the Qt bridge the window holds (`bridge.rs`). The window is `qml/`; `main.rs` only opens it.
pub mod notify;
pub mod platform;
pub mod shell;
mod bridge;
