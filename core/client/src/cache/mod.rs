//! The one cache format of every app, over the key-value contract of [`StoreOp`](crate::app::io::StoreOp)'s cache
//! space (get, put, delete, scan by prefix). W5 builds it (sources: web `store-idb.ts`/`mirror.ts`, iOS
//! `RoomCache.swift`/`RecordStore.swift`, Linux `store.rs`).
//!
//! The format the contract fixes:
//!
//! - Every key starts with `FORMAT/<room id hex>/`, so one store can hold several rooms and a format change starts
//!   a fresh cache (the old one is deleted by prefix, then filled from the hub).
//! - Values are the serde JSON of the client core's own types, as UTF-8 bytes; files and pictures are their
//!   encrypted bytes as the hub stores them.
//! - The cache is only a cache: losing it costs a catch-up, never data. What must survive (keys, groups, chains,
//!   the outbox) is the device space (`DeviceLoad`/`DeviceApply`), the protocol core's own format.
//! - `meta` names this device and the device's store revision the board stands at; without it the cache counts as
//!   empty, and records beside no `meta` are deleted.
//!
//! [`keeper`] reads it at the start and writes it behind the board; [`layout`] says where each record lives.

pub mod keeper;
pub mod layout;

pub use keeper::{Keeper, Listing, Read, Took};

/// The cache format. A change of the layout or of a stored type's meaning counts up.
pub const FORMAT: &str = "c1";

/// The parts of a room's cache, as key segments after `c1/<room>/`.
pub mod part {
    /// This device's id, the device's store revision the board stands at, whether a write is under way.
    pub const META: &str = "meta";
    /// The board model, one key per record: `model/card/<id>`, `model/item/<timeline>/n<number>`, … (`layout`).
    pub const MODEL: &str = "model";
    /// The hub's Desk as last read (what it listed as open).
    pub const DESK: &str = "desk";
    /// A loaded window of a timeline: `win/<timeline key>`.
    pub const WINDOW: &str = "win";
    /// Encrypted file bytes: `file/<file id>`.
    pub const FILE: &str = "file";
    /// What the person typed and did not send: `draft/<place>`.
    pub const DRAFT: &str = "draft";
    /// What the Chat list counts as read on this device: one record (`app::seen`).
    pub const SEEN: &str = "seen";
    /// Whether dictation is on on this device (`app::dictation`): `0` when switched off.
    pub const DICTATION: &str = "dictation";
}

/// The key of a part of a room's cache: `c1/<room>/<part>` or `c1/<room>/<part>/<id>`.
pub fn key(room_hex: &str, part: &str, id: Option<&str>) -> String {
    match id {
        Some(id) => format!("{FORMAT}/{room_hex}/{part}/{id}"),
        None => format!("{FORMAT}/{room_hex}/{part}"),
    }
}

/// The prefix of everything of one room.
pub fn room_prefix(room_hex: &str) -> String {
    format!("{FORMAT}/{room_hex}/")
}

#[cfg(test)]
mod tests {
    #[test]
    fn keys_carry_format_and_room() {
        assert_eq!(
            super::key("ab", super::part::WINDOW, Some("s:1")),
            "c1/ab/win/s:1"
        );
        assert!(super::key("ab", super::part::META, None).starts_with(&super::room_prefix("ab")));
    }
}
