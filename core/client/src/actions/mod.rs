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
    /// Without a note (or a blank one) the agent reads the core's words for it (`views::talk::HAND_BACK_TEXT`,
    /// `EXPLAIN_TEXT`), and the conversation shows a quiet line. `attachments`: files attached (`ViewModel.files`).
    HandBack {
        card_id: String,
        #[serde(default)]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        rework: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        marks: Vec<serde_json::Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        attachments: Vec<String>,
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
    /// Several cards at once, one toast for all with an Undo that takes them all back. `way`: "sweep" (the duck for
    /// all: a decision is left to its agent, a card to read is closed as read), "later", "wake", "duck", "read",
    /// "shred", "reopen" (the Undo of the others). A card the way does not fit (a request, a card of another kind)
    /// is left as it is.
    Batch {
        way: String,
        card_ids: Vec<String>,
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

    // ---- sessions and teams ----
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
        team_id: String,
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
    /// The crown of the session's team (one per team): the session the person's notes go to.
    CrownSession {
        session_id: String,
        on: bool,
    },
    /// Deletes a session: its open cards are shredded, it and its helpers are archived, and the agent devices that
    /// carry nothing else leave the room.
    DeleteSession {
        session_id: String,
    },
    AddTeam {
        name: String,
    },
    RenameTeam {
        team_id: String,
        name: String,
    },
    RemoveTeam {
        team_id: String,
    },
    /// A room register the App writes on its own: a step of a team's deletion (`app::teams`), the kit's flag
    /// (`app::kit`). Not the shell's to send.
    #[serde(skip)]
    RoomRegister {
        key: String,
        value: serde_json::Value,
    },
    /// Every team's place, in one write: the teams in this order.
    OrderTeams {
        team_ids: Vec<String>,
    },
    SetGoal {
        team_id: String,
        text: String,
    },
    /// A main session's helpers unfolded under it (`open`) or folded into its row (`SessionRowView.unfolded`). This
    /// device's alone: the shell keeps the unfolded mains and sends them again after a start.
    FoldHelpers {
        session_id: String,
        open: bool,
    },
    /// A team's goals folded to their first line, or open again (`GoalsView.folded`). This device's alone: the shell
    /// keeps the folded teams and sends them again after a start.
    FoldGoals {
        team_id: String,
        folded: bool,
    },

    // ---- notes, scribble, files ----
    /// A sealed native share inbox item. Recipient selection and upload completion belong to the core.
    ImportShare {
        id: String,
        #[serde(default)] room: Option<String>,
        #[serde(default)] team: Option<String>,
        #[serde(default)] to: Option<String>,
        #[serde(default)] send: bool,
        text: String,
        #[serde(default)] files: Vec<SharedFile>,
    },
    /// The note being written, kept as it stands (the note page saves by itself while he types and when he leaves
    /// it): `note_id` is the note the page shows (none: a new one). The core decides: a new note once there are
    /// words, a new version when they changed, the note thrown away when it was emptied, nothing else. A file still
    /// uploading is left out until the next keep. `send`: then sent as `SendNote` (to `to`, else the crown's), also
    /// a note made by this very keep.
    KeepNote {
        #[serde(default)]
        note_id: Option<String>,
        text: String,
        #[serde(default)]
        attachments: Vec<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        send: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        to: Option<String>,
    },
    WriteNoteFiles { #[serde(default)] note_id: Option<String>, text: String, attachments: Vec<String> },
    WriteNote {
        #[serde(default)]
        note_id: Option<String>,
        text: String,
    },
    DeleteNote {
        note_id: String,
    },
    /// Send a note to a session (none: the crown's), after a short hold for Undo (`UnsendNote`).
    SendNote {
        note_id: String,
        #[serde(default)]
        to: Option<String>,
    },
    UnsendNote {
        note_id: String,
    },
    /// An item on a Scribble Board (`desk/<32 hex>`), in the model's form (board units, ids in hex):
    /// `{content_type: "strokes", strokes: [shape, …]}`, `{content_type: "erase" | "send_away", stroke_ids: […]}` or
    /// `{content_type: "move", stroke_ids: […], offset: [dx, dy]}`. A shape is `{tool, z?, group?}` with, by its
    /// tool, `color, width, points` (packed, base64url; `live`: the stroke id its pieces went under), `at, text,
    /// size, color, wrap?`, or `rect, attachment` (a picture: its file's id, or the attachment's reference) and
    /// `nw?, nh?, mime?, name?`. Shown at once (`BoardShape.local` is `<local_id>/<index>`), taken back with a toast
    /// if the hub refuses it. `local_id`: the shell's own, unique per item.
    /// A native pen's uncompressed points in board units: x, y, elapsed ms, force 0..1. Packing is shared.
    DrawStroke { board_id: String, local_id: String, points: Vec<serde_json::Value>, tool: String, color: String, width: u32 },
    BoardItem {
        board_id: String,
        local_id: String,
        content: serde_json::Value,
    },
    /// The points of a stroke still being drawn, to the person's other devices that are connected (spec 7.2): not
    /// stored, not shown here. `piece`: `{stroke (32 hex), number (from 1), tool, color?, width?, points}`.
    StrokePiece {
        board_id: String,
        piece: serde_json::Value,
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
    /// Open a Share link (spec/v1.md 11.5) for someone outside the room (the web's page `/artifact/<share id>`):
    /// no device, no sign-in. The file comes as `Effect::FileReady` under the id `shared`, or a toast says why not.
    OpenShared {
        link: String,
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
    /// Push on this device: off, every card, or only what knocks.
    Push {
        level: crate::app::push::PushChoice,
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

/// Bytes picked in a native share sheet; never logged.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFile { pub name: String, pub media_type: String, pub bytes: Bytes }
