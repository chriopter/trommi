//! Every human action, by name: what a tap or a typed line becomes before the core decides what it means.
//!
//! The shell sends `Event::Action`. The core checks it, shows it at once (an echo in the view model), seals it,
//! puts it in the device's outbox and sends it; the hub's copy replaces the echo, a refusal rolls it back with a
//! toast. W3 implements them (sources: web `client.ts`, iOS `RoomActions.swift`); this enum is the contract.
//!
//! Ids are the model's spelling (hex). Bytes the person picked (a file, a picture) come inside the action: picking
//! is the shell's (a platform dialog), deciding what to do with it is the core's.

pub mod run;
pub mod sent;

use crate::account::AccountAction;
use crate::app::bytes::Bytes;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    // ---- cards and requests ----
    Answer {
        card_id: String,
        #[serde(default)]
        choices: Vec<String>,
        #[serde(default)]
        note: Option<String>,
        #[serde(default)]
        attachments: Vec<String>,
        /// A note per option, by its key.
        #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
        option_notes: serde_json::Map<String, serde_json::Value>,
        /// "Do what you think best": the agent still chooses (the choices are its recommendation).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        trusted: bool,
        /// Marks drawn on the card's pictures (`{picture, x, y, w, h, note?}` as the card's body has them).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        marks: Vec<serde_json::Value>,
    },
    /// An info card read: closed.
    Read {
        card_id: String,
    },
    /// Thrown away: closed without a choice.
    Shred {
        card_id: String,
        #[serde(default)]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        marks: Vec<serde_json::Value>,
    },
    /// Hand the card back to its agent: "What??" (`rework` false: explain it) or "Revise" (`rework` true: make it
    /// again). The card comes back up awake.
    HandBack {
        card_id: String,
        #[serde(default)]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rework: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        marks: Vec<serde_json::Value>,
    },
    /// Take a card back from its agent while it reworks or explains it: no need any more.
    TakeBack {
        card_id: String,
    },
    /// Take an answer back and decide again.
    DecideAgain {
        card_id: String,
    },
    /// Allow or deny a permission request.
    Verdict {
        request_id: String,
        allow: bool,
    },
    /// Put a card off until a time (`None`: until the next morning, the core's rule) or bring it back.
    Snooze {
        card_id: String,
        #[serde(default)]
        until_ms: Option<u64>,
    },
    Unsnooze {
        card_id: String,
    },
    Star {
        card_id: String,
        on: bool,
    },

    // ---- talk ----
    /// A line to a session's agent (or to a card's chat).
    Write {
        session_id: String,
        #[serde(default)]
        card_id: Option<String>,
        text: String,
        #[serde(default)]
        attachments: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        marks: Vec<serde_json::Value>,
        /// Cards copied into the line (their ids).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        copied_cards: Vec<String>,
    },
    /// What the person has typed and not sent yet, kept per place (a card's id): the register `draft/<place>` as both
    /// apps keep it, `{keys, note, notes, marks?, ts}`. `keys` and `notes` left out keep what stands.
    Draft {
        place: String,
        text: String,
        /// The options ticked.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<Vec<String>>,
        /// A note per option, by its key.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notes: Option<serde_json::Map<String, serde_json::Value>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        marks: Option<Vec<serde_json::Value>>,
    },

    // ---- sessions and desks ----
    RenameSession {
        session_id: String,
        name: String,
    },
    ArchiveSession {
        session_id: String,
        on: bool,
    },
    MoveSession {
        session_id: String,
        desk_id: String,
    },
    /// A session's mark (an icon's name); none takes it off.
    SessionIcon {
        session_id: String,
        #[serde(default)]
        icon: Option<String>,
    },
    /// A session's group in the list (a heading); none takes it out.
    SessionGroup {
        session_id: String,
        #[serde(default)]
        group: Option<String>,
    },
    /// The session a helper is shown under; none makes it a main session again.
    SessionParent {
        session_id: String,
        #[serde(default)]
        parent: Option<String>,
    },
    /// A new place in the list: directly before or after another session (neither: last).
    PlaceSession {
        session_id: String,
        #[serde(default)]
        before: Option<String>,
        #[serde(default)]
        after: Option<String>,
    },
    /// The crown of the session's desk (one per desk): the session the person's notes go to.
    CrownSession {
        session_id: String,
        on: bool,
    },
    /// Deletes a session: its open cards are shredded, it and its helpers are archived, and the agent devices that
    /// carry nothing else leave the room.
    DeleteSession {
        session_id: String,
    },
    AddDesk {
        name: String,
    },
    RenameDesk {
        desk_id: String,
        name: String,
    },
    RemoveDesk {
        desk_id: String,
    },
    /// Every desk's place, in one write: the desks in this order.
    OrderDesks {
        desk_ids: Vec<String>,
    },
    SetGoal {
        desk_id: String,
        text: String,
    },

    // ---- notes, scribble, files ----
    WriteNote {
        #[serde(default)]
        note_id: Option<String>,
        text: String,
    },
    DeleteNote {
        note_id: String,
    },
    /// One stroke on a board, as the pen made it (points in board units).
    Stroke {
        board_id: String,
        stroke: Bytes,
    },
    /// Bytes the person picked, to attach: `ViewModel.files[local_id]` shows it uploading, then ready with its
    /// attachment id, or failed.
    Attach {
        local_id: String,
        name: String,
        media_type: String,
        bytes: Bytes,
    },
    /// Get a file to show (`Effect::FileReady`) or to save (`Effect::SaveFile`): the core fetches and decrypts.
    OpenFile {
        attachment_id: String,
        save: bool,
    },
    Share {
        attachment_id: String,
        #[serde(default)]
        expires_ms: Option<u64>,
    },
    RevokeShare {
        share_id: String,
    },

    // ---- the account, devices, invites (account/) ----
    Account {
        action: AccountAction,
    },

    /// This device's name, as the person's other devices list it.
    RenameDevice {
        name: String,
    },
    /// More of a list in view: the older talk of a session, the comments of a card, the rest of Off your mind.
    ShowMore {
        of: crate::views::board::MoreOf,
    },

    // ---- the app ----
    /// Try now what waits for a retry.
    Retry,
    /// Stop an upload or download the person started.
    CancelFile {
        file_id: String,
    },
    /// A toast was closed.
    Dismiss {
        toast_id: String,
    },
}
