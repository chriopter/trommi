//! Trommi for Linux: the engine (a human device over the shared core, `engine/`), the app's copy of the board model
//! and its views (`view.rs`), its folders on disk (`store.rs`), and the Qt bridge the window holds (`bridge.rs`).
//! The window is `qml/`; `main.rs` only opens it.
pub mod engine;
pub mod store;
pub mod view;
mod bridge;
