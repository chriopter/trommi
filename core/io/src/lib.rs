//! The native executor of the app core (core/client/README.md, "The app core"): everything a shell on a desktop
//! or a server does for an [`App`](trommi_client_core::app::App) that is not drawing.
//!
//! ```text
//! shell (Qt, a test)                 trommi-io                                  core/client
//!   Handle::send(Event)   ──▶  one task owns the App, one queue of events ──▶   App::handle(now, event)
//!   Shell::show(effect)   ◀──  Render, Notify, FileReady, SaveFile, …      ◀──  [Effect, …]
//!                              Http, OpenStream, Cancel   http.rs (reqwest)
//!                              Store                      store.rs (the journal, the cache folder)
//!                              Timer                      one tokio timer
//! ```
//!
//! It decides nothing: no retry, no redirect, no header of its own, no parsing of an answer or of the event
//! stream (the core's codec does that). What it holds to is the contract's letter:
//!
//! - the effects of one answer start in the order given; a `Store` with `DeviceApply` is a barrier (nothing after
//!   it starts before it is durable, and nothing after it starts at all when it failed);
//! - an answer's body is read up to `HttpRequest::max_answer` and not one byte further (`too_large`);
//! - a cancelled request's or stream's results are dropped;
//! - one timer, replacing the last.
//!
//! tokio and reqwest stay in this crate: the wasm and iOS builds of the core never see them.

pub mod http;
pub mod run;
pub mod store;

pub use http::Http;
pub use run::{start, Core, Handle, Shell};
pub use store::{OpenError, Stores};
