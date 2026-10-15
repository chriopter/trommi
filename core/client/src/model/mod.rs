//! The board model: what every human device shows, built from what the protocol core accepted, once for every app.
//! The rules are the web app's `model.ts` of before (core/protocol/README.md "The model"); an app keeps a copy of the records
//! (`patch`) and renders from it.

pub mod apply;
pub mod change;
pub mod echo;
pub mod groups;
pub mod patch;
pub mod project;
pub mod received;
pub mod snapshot;
pub mod types;
pub mod walk;
pub mod window;

use crate::desk::DeskState;
use change::Change;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use types::*;

/// At most this many alerts are kept.
pub const ALERTS_MAX: usize = 200;
/// At most this many kinds of newer things are named.
pub const NEWER_WHAT_MAX: usize = 16;

/// An own send shown before the hub confirmed it, and what undoes it.
#[derive(Clone, Debug)]
pub enum Echo {
    Item {
        timeline_key: String,
    },
    Registers {
        before: BTreeMap<String, Option<HumanRegister>>,
        open: Vec<String>,
        /// The values it showed, and its place among the echoes (a later one stands in front).
        values: BTreeMap<String, serde_json::Value>,
        seq: u64,
    },
    Answer {
        object_id: String,
        state: String,
        version_hash: Option<String>,
        saved: Box<SavedCard>,
    },
    Note {
        key: String,
    },
}

/// What an answer echo replaced.
#[derive(Clone, Debug)]
pub struct SavedCard {
    pub answer: Option<Answer>,
    pub object_state: String,
    pub closed_how: Option<String>,
}

/// A running turn of an agent (its work trail, 7.3).
#[derive(Clone, Debug)]
pub struct Turn {
    pub timeline_key: String,
    pub session_id: String,
    pub sender: String,
    pub started_at: u64,
    pub last: u64,
}

/// The board model of one device.
#[derive(Debug, Default)]
pub struct Model {
    pub my_device_id: Option<String>,
    pub key_epoch: u64,
    pub recovery_key: Option<Value>,
    pub members: BTreeMap<String, Member>,
    pub sessions: BTreeMap<String, Session>,
    pub cards: BTreeMap<String, Card>,
    pub permissions: BTreeMap<String, PermissionRequest>,
    pub notes: BTreeMap<String, Note>,
    pub published: BTreeMap<String, Published>,
    pub timelines: BTreeMap<String, Timeline>,
    pub human: BTreeMap<String, HumanRegister>,
    pub alerts: Vec<Alert>,
    pub newer: Newer,
    pub stack: Vec<String>,
    pub open_permission_ids: Vec<String>,
    /// The walk: every open card and request that waits for the person, in the Desk's order (walk.rs).
    pub desk_order: Vec<String>,
    // what the builder keeps for itself
    pub device_registers: HashMap<String, Map<String, Value>>,
    /// The group whose device-name value the model is showing (None: the room).
    pub device_register_groups: BTreeMap<String, Option<String>>,
    pub echoes: HashMap<String, Echo>,
    pub by_hash: HashMap<String, String>,
    pub turns: BTreeMap<String, Turn>,
    pub room_commits: Option<(u64, Option<u64>)>,
    pub alert_seq: u64,
    pub echo_seq: u64,
    pub desk: DeskState,
    pub desk_sent: HashMap<String, String>,
    pub desk_archived: Option<String>,
    /// The change each object of the hub's last list was first filed at (project.rs `first_from_desk`).
    pub desk_first: HashMap<String, u64>,
    /// That list is new: every card it names is asked once.
    pub desk_first_new: bool,
    /// The Desk is to be built again from every card (a card took another place in the lists).
    pub desk_rebuild: bool,
    /// The Desk engine holds every card and request of the model.
    pub desk_whole: bool,
    /// The list or the hold changed: the next projection asks the engine.
    pub desk_dirty: bool,
    /// When the Desk changes by the clock alone; 0: never.
    pub desk_wake: u64,
    /// Presence reports of devices the groups have not named yet, by device (groups.rs `apply_presence`).
    pub presence_held: HashMap<String, groups::DeviceReport>,
    /// Every timeline's window is open (an app that keeps every item header: iOS).
    pub windows_open: bool,
    /// The patch carries cards lean (`LeanCard`): an app that reads the rest of a card on demand (`card_json`; iOS).
    pub lean_cards: bool,
    /// The cards a patch in parts has still to give (`finish_part`).
    pub patch_rest: std::collections::BTreeSet<String>,
    /// The room is still being read (a sign-in, a read back): a session whose profile has not come is "Loading …".
    pub reading: bool,
    /// The hub's counts of the Desk as last read (hub-api.md point 47): at which change, and per session (its id)
    /// how many of its cards were answered or closed then. A device that did not read the room's past counts its
    /// done cards from these (off_mind.rs).
    pub hub_settled: Option<(u64, BTreeMap<String, u64>)>,
    /// The cards that carry a snooze mark (`snoozed_until`, `unsnoozed`).
    pub snooze_marked: std::collections::BTreeSet<String>,
    /// The model's clock: the `now` of the last projection (the session list's status is decided at it).
    pub clock: u64,
    /// When this app's run started: the first projection's clock.
    pub started_at: u64,
    /// When the session list's status may change by the clock alone (0: never); the projection past it sends the
    /// list again.
    pub status_wake: u64,
    /// The devices the hub named online since the live stream opened, until its `here` ends that list (presence.rs).
    pub named_here: Option<std::collections::BTreeSet<String>>,
    /// Which agent of the room waits for its main session, and since when (seats.rs).
    pub seats: crate::seats::SeatPlanner,
}

/// `<kind>:<id>`: a timeline's key.
pub fn timeline_key(kind: &str, id: &str) -> String {
    format!("{kind}:{id}")
}

impl Model {
    pub fn new() -> Self {
        Self::default()
    }

    /// Something only a newer Trommi understands was met: counted, so the app says so once.
    pub fn note_newer(&mut self, change: &mut Change, what: &str, at: Option<u64>) {
        let n = &mut self.newer;
        n.count += 1;
        if !n.what.iter().any(|w| w == what) {
            n.what.push(what.to_string());
            if n.what.len() > NEWER_WHAT_MAX {
                n.what.remove(0);
            }
        }
        n.envelope_number = n.envelope_number.max(at.unwrap_or(0));
        change.room = true;
    }

    /// An alert for the person.
    #[allow(clippy::too_many_arguments)]
    pub fn push_alert(
        &mut self,
        change: &mut Change,
        code: &str,
        message: &str,
        envelope_number: Option<u64>,
        sender: Option<String>,
        source: &str,
        at: u64,
    ) -> Alert {
        // The same finding about the same envelope, met again (a catch-up or a reading of the past that hands it
        // once more), is one alert, not another.
        if envelope_number.is_some() {
            if let Some(held) = self.alerts.iter().find(|held| {
                held.code == code
                    && held.envelope_number == envelope_number
                    && held.sender_device_id == sender
                    && held.source == source
            }) {
                return held.clone();
            }
        }
        let alert = Alert {
            alert_id: format!("{}-{}", radix36(at), radix36(self.alert_seq)),
            code: code.to_string(),
            message: message.to_string(),
            envelope_number,
            sender_device_id: sender,
            at,
            source: source.to_string(),
        };
        self.alert_seq += 1;
        self.alerts.push(alert.clone());
        if self.alerts.len() > ALERTS_MAX {
            let cut = self.alerts.len() - ALERTS_MAX;
            self.alerts.drain(0..cut);
        }
        change.alerts = true;
        alert
    }
}

/// A number in base 36, as JavaScript's `toString(36)`.
pub fn radix36(mut n: u64) -> String {
    if n == 0 {
        return "0".into();
    }
    let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    while n > 0 {
        out.push(digits[(n % 36) as usize]);
        n /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}
