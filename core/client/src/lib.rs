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
//! # The app core
//!
//! [`app::App`] is the whole app's logic for one room: events in, effects out, no I/O ([`app`], and
//! core/client/README.md "The app core" for the contract every shell is written against).
//!
//! # Bindings
//!
//! The facade in `core/bindings/swift` (`client.rs`) exports [`desk::DeskState`] as `ClientDesk` through UniFFI and
//! wasm-bindgen, with plain records. A host written in Rust (the Linux app) uses the types directly.

pub mod account;
pub mod actions;
pub mod app;
pub mod board;
pub mod cache;
pub mod codec;
pub mod desk;
pub mod team_delete;
pub mod engine;
pub mod faces;
pub mod files;
pub mod helpers;
pub mod hub;
pub mod machines;
pub mod model;
pub mod off_mind;
pub mod plan;
pub mod presence;
pub mod registers;
pub mod scribble;
pub mod seats;
pub mod selfjoin;
pub mod sessions;
pub mod status;
pub mod views;

pub use board::{Board, Fact, Kind, Object, Source, State};
pub use desk::{
    next_in_walk, walk_order, Delta, DeskCard, DeskRequest, DeskState, Listed, Listing, Projection,
    SessionOp, StackOp, WalkCard, REQUEST_LIFE_MAX_MS,
};
pub use plan::{CatchUp, Step};
pub use registers::{RegisterOutcome, Registers};
