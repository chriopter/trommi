//! The client core: what every human-device client of Trommi decides the same way, written once.
//!
//! The protocol (MLS, envelopes, chains, the command gate) is `trommi-core`. Above it, each app used to keep its
//! own copy of the board model, the Desk rules and the catch-up order (web TypeScript, iOS Swift, the connector's
//! Rust). This crate is that layer, without IO: the host fetches what a [`plan::CatchUp`] asks for, hands what
//! the protocol core accepted to the [`board::Board`] as [`board::Fact`]s, tells [`desk::DeskState`] what changed,
//! and renders its projection.
//!
//! # The rules
//!
//! - **Newest to oldest.** A catch-up first reads the hub's open list and each open card's current version: then
//!   the Desk is complete. Older history loads in the background and may only CLOSE cards ([`board::Source`]).
//!   Only the MLS Commits (epochs) and each sender's chain check go oldest-first; that is the protocol core's.
//! - **The Desk comes from the hub's open list** ([`desk::Listing`]). A listed card not read yet is a
//!   placeholder. An unread permission request older than [`desk::REQUEST_LIFE_MAX_MS`] counts as expired.
//! - **Session names are registers. A register never creates a session** ([`registers::Registers`]).
//!
//! # Bindings
//!
//! The facade in `core/swift` (`client.rs`) exports [`desk::DeskState`] as `ClientDesk` through UniFFI and
//! wasm-bindgen, with plain records. A host written in Rust (the Linux app) uses the types directly.

pub mod board;
pub mod codec;
pub mod desk;
pub mod desk_delete;
pub mod model;
pub mod off_pile;
pub mod plan;
pub mod presence;
pub mod registers;
pub mod seats;
pub mod selfjoin;
pub mod sessions;
pub mod stars;
pub mod status;
pub mod store;

pub use board::{Board, Fact, Kind, Object, Source, State};
pub use desk::{
    next_in_walk, walk_order, Delta, DeskCard, DeskRequest, DeskState, Listed, Listing, Projection,
    SessionOp, StackOp, WalkCard, REQUEST_LIFE_MAX_MS,
};
pub use plan::{CatchUp, Step};
pub use registers::{RegisterOutcome, Registers};
pub use store::{MemoryStore, Store};
