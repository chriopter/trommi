//! The App at the edge of a binding: one event in as JSON text plus its blobs, the effects out the same way.
//!
//! One crossing per event, whatever it carries: the JSON names [`Bytes`](super::Bytes) by their index in the blobs
//! list beside it, so envelopes and files cross as bytes (a `Uint8Array`, `Data`), never as base64 in a string.
//! The JSON is the serde form of [`Event`] and [`Effect`] (tag `type`, snake case): the shells' own types mirror it.

use super::{App, Effect, Event};
use crate::app::bytes::with_blobs;

/// JSON text and the blobs it refers to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Packet {
    pub json: String,
    pub blobs: Vec<Vec<u8>>,
}

/// Where the JSON failed, never what it held: serde's own words can quote a value, and an event may carry a
/// password.
fn why(e: serde_json::Error) -> String {
    format!(
        "not the contract's JSON ({:?} at line {}, column {})",
        e.classify(),
        e.line(),
        e.column()
    )
}

/// Reads an event. `Err` holds where it failed, for a `bad-format` error at the binding.
pub fn decode_event(packet: Packet) -> Result<Event, String> {
    let (event, _) = with_blobs(packet.blobs, || serde_json::from_str::<Event>(&packet.json));
    event.map_err(why)
}

/// Writes effects.
pub fn encode_effects(effects: &[Effect]) -> Packet {
    let (json, blobs) = with_blobs(Vec::new(), || {
        serde_json::to_string(effects).expect("the effects are plain data")
    });
    Packet { json, blobs }
}

/// Writes an event (what a shell written in Rust, or a test, sends).
pub fn encode_event(event: &Event) -> Packet {
    let (json, blobs) = with_blobs(Vec::new(), || {
        serde_json::to_string(event).expect("the event is plain data")
    });
    Packet { json, blobs }
}

/// Reads effects (what a test of a binding checks).
pub fn decode_effects(packet: Packet) -> Result<Vec<Effect>, String> {
    let (effects, _) = with_blobs(packet.blobs, || {
        serde_json::from_str::<Vec<Effect>>(&packet.json)
    });
    effects.map_err(why)
}

/// `handle` across the edge.
pub fn handle(app: &mut App, now_ms: u64, event: Packet) -> Result<Packet, String> {
    let event = decode_event(event)?;
    Ok(encode_effects(&app.handle(now_ms, event)))
}
