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
//! - `meta` names the room, its hub and this device; without it the cache counts as empty.

/// The cache format. A change of the layout or of a stored type's meaning counts up.
pub const FORMAT: &str = "c1";

/// The parts of a room's cache, as key segments after `c1/<room>/`.
pub mod part {
    /// The room, its hub address, this device's id, the cursor (the newest change read).
    pub const META: &str = "meta";
    /// The board model's snapshot (`Model`, core/client/src/model/snapshot.rs).
    pub const MODEL: &str = "model";
    /// The Desk's state (snoozes, the hub's last open list).
    pub const DESK: &str = "desk";
    /// A loaded window of a timeline: `win/<timeline key>`.
    pub const WINDOW: &str = "win";
    /// Encrypted file bytes: `file/<file id>`.
    pub const FILE: &str = "file";
    /// What the person typed and did not send: `draft/<place>`.
    pub const DRAFT: &str = "draft";
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
