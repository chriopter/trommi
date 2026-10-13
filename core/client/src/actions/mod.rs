//! Every human action, by name: what a tap or a typed line becomes before the core decides what it means.
//!
//! The shell sends `Event::Action`. The core checks it, shows it at once (an echo in the view model), seals it,
//! puts it in the device's outbox and sends it; the hub's copy replaces the echo, a refusal rolls it back with a
//! toast. W3 implements them (sources: web `client.ts`, iOS `RoomActions.swift`); this enum is the contract.
//!
//! Ids are the model's spelling (hex). Bytes the person picked (a file, a picture) come inside the action: picking
//! is the shell's (a platform dialog), deciding what to do with it is the core's.

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
    },
    /// "What??": hand the card back for an explanation.
    HandBack {
        card_id: String,
        #[serde(default)]
        note: Option<String>,
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
    /// Done with it: off the Desk.
    Archive {
        card_id: String,
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
    },
    /// What the person has typed and not sent yet, kept per place.
    Draft {
        place: String,
        text: String,
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
    /// Bytes the person picked, to attach: answered with the attachment's id in the view model.
    Attach {
        local_id: String,
        name: String,
        media_type: String,
        bytes: Bytes,
    },
    /// Get a file to show or save: the core fetches, decrypts and answers with `SaveFile` or a view.
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

    // ---- the app ----
    /// Try now what waits for a retry.
    Retry,
    /// A toast was closed.
    Dismiss {
        toast_id: String,
    },
}
