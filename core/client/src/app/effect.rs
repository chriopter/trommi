//! What [`App::handle`](super::App::handle) answers: everything the shell has to do, in order.
//!
//! **Order.** The shell starts the effects of one answer in the order given. They may then run at the same time,
//! with one exception: a `Store` with [`StoreOp::DeviceApply`](super::io::StoreOp::DeviceApply) is a barrier. No
//! effect after it, of the same answer or a later one, starts before it finished (the shell keeps handling events
//! and queues their effects); if it failed, the shell drops every effect queued behind it and sends only the
//! `Store` result with the error (the core then stops: it is the owner no more).

use crate::app::bytes::Bytes;
use crate::app::io::{HttpRequest, IoId, PlatformOp, SecretOp, StoreOp};
use crate::views::{Text, ViewDelta};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Effect {
    /// Apply this to the view state (fold with [`ViewModel::apply`](crate::views::ViewModel::apply)) and draw.
    Render {
        delta: ViewDelta,
        /// The window it is for (`Event` `window`, a browser tab sharing this App); 0, and left out, where a shell
        /// has one window.
        #[serde(default, skip_serializing_if = "is_zero")]
        window: u32,
    },
    /// Send it; answer with `Event::Http` of the same id.
    Http {
        id: IoId,
        request: HttpRequest,
    },
    /// Open a streamed GET (the hub's event stream); answer with `Event::Stream` items of the same id until
    /// `ended` or `failed`.
    OpenStream {
        id: IoId,
        request: HttpRequest,
    },
    /// Stop an `Http` or a stream. The core forgot the id before it said so: whatever still comes for it (a
    /// success that was already on its way included) is dropped. The request may have reached the hub; the
    /// core's requests are safe to repeat for that reason (the outbox's entries carry their own ids).
    Cancel {
        id: IoId,
    },
    /// A store operation; answer with `Event::Store`.
    Store {
        id: IoId,
        op: StoreOp,
    },
    /// A secret operation; answer with `Event::Secret`.
    Secret {
        id: IoId,
        op: SecretOp,
    },
    /// A platform service (passkeys, push registration); answer with `Event::Platform`.
    Platform {
        id: IoId,
        op: PlatformOp,
        /// The window whose step led to it (`App::handle_in`): its prompt shows there and in no other window. 0,
        /// and left out, where a shell has one window.
        #[serde(default, skip_serializing_if = "is_zero")]
        window: u32,
    },
    /// Keep these bytes (a decrypted picture or file) under `file_id` for drawing: views name files by that id.
    /// The shell drops them on `FileGone`, or whenever it likes and then shows the file as not loaded.
    FileReady {
        file_id: String,
        media_type: String,
        bytes: Bytes,
    },
    FileGone {
        file_id: String,
    },
    /// Send `Event::Wake` at this time (ms since 1970, the shell's clock), replacing any earlier timer. `None`:
    /// no timer. A shell that sleeps (a phone in the background) may be late; never early by more than a second.
    Timer {
        at_ms: Option<u64>,
    },
    /// Show a local notification (only where `Config.can.notify`): a card or request that newly waits for the
    /// person while the app is in the back (`app::outside`). Never at the first reading of the room, never for one
    /// that waited before, never while the app is in front. The push level of this device decides too: off, none;
    /// knocking, only the urgent ones.
    Notify {
        notice: Notice,
    },
    /// What widgets and Live Activities show (only where `Config.can.snapshots`): sent when it changed, at most
    /// once per answer, not before the room was read up to its end once; an empty one at a sign-out.
    Snapshot {
        snapshot: WidgetSnapshot,
    },
    /// What the phone's notification extension reads (only where `Config.can.notify` and `Config.can.push` is
    /// "apns"): the shell writes it where only the app and its extension read it (iOS: the context's Keychain item
    /// and the notify keys' item), replacing what was there. `None`: signed out, both go (and what the extension
    /// read ahead). Sent when it changed; the cursor moves with it, and at `Event::Background`. Never log it.
    NotifyContext {
        context: Option<NotifyContext>,
    },
    /// Native share inbox item sealed into the outbox (or discarded for another room): remove it. False:
    /// keep it for another attempt. Device writes before this effect are barriers as for every other effect.
    ShareInbox { id: String, consumed: bool },
    /// The account was deleted or this device verified its removal: erase the native share inbox and its key.
    ForgetShared,
    /// Offer these bytes to the person as a file (download, share sheet, save dialog).
    SaveFile {
        name: String,
        media_type: String,
        bytes: Bytes,
        /// The window whose step led to it (as `Platform`'s).
        #[serde(default, skip_serializing_if = "is_zero")]
        window: u32,
    },
    /// Open this link outside the app (a share link, the help page).
    OpenUrl {
        url: String,
        /// The window whose step led to it (as `Platform`'s).
        #[serde(default, skip_serializing_if = "is_zero")]
        window: u32,
    },
    /// A line for the platform's log. Never holds content, keys, e-mail addresses or tokens.
    Log {
        level: LogLevel,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// Stable per thing notified: a newer notice with the same id replaces the shown one.
    pub id: String,
    pub title: Text,
    pub body: Text,
    /// Where a tap goes (a card's or a session's id); the shell sends it back as `Focus`.
    #[serde(default)]
    pub target: Option<String>,
    /// Ring (urgent) or arrive quietly.
    #[serde(default)]
    pub urgent: bool,
}

/// The board as a widget or a Live Activity shows it, on every team.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetSnapshot {
    /// Live Activity policy: unknown until the push level is read; false when off or signed out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_activity: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room: Option<String>,
    /// The native share sheet's recipients, already in team order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub teams: Vec<ShareTeam>,
    /// How many cards and requests wait for the person: open, no info, not with its agent, not put off, not of an
    /// archived session, read on this device.
    pub waiting: u32,
    /// How many sessions are at work: online, not archived, with a status line that says "working".
    pub working: u32,
    /// One of the waiting knocks (urgent, blocking, or a request).
    pub urgent: bool,
    /// The waiting ones, the most urgent first (a request or a blocking card, then a knock, then the rest; the one
    /// that waits longest first among equals), at most [`WAITING_MAX`](super::outside::WAITING_MAX). The Live
    /// Activity shows the first. A decision is answered from it with `Action::Answer { card_id, choices: [key] }`.
    #[serde(default)]
    pub cards: Vec<WaitingCard>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingCard {
    pub id: String,
    /// As it was written (the shell cuts it to its space).
    pub title: String,
    /// Its session's name ("Agent" for one the list does not show).
    pub session: String,
    /// "decision" (or another card type the agent named), or "permission" for a request.
    pub kind: String,
    /// "low", "normal", "high", "critical" (a request: "critical").
    pub urgency: String,
    /// When it was written, ms since 1970.
    pub created_ms: u64,
    /// A decision's options (none for a request).
    #[serde(default)]
    pub options: Vec<WaitingOption>,
    /// More than one option may be chosen.
    #[serde(default)]
    pub multiple: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitingOption {
    pub key: String,
    pub label: String,
}

/// What the notification extension needs to open a push and show the card that rang (spec/v1.md 15.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyContext {
    /// The room's id, hex.
    pub room: String,
    /// The hub's address.
    pub hub: String,
    /// The client name the extension sends to the hub ("ios/1.2").
    pub client: String,
    /// The room's change this device read up to: the extension reads ahead from here.
    pub cursor: u64,
    /// The APNs notify key the hub seals each notification under (the secret `push-notify-key`), 32 bytes.
    pub push_key: Bytes,
    /// Of each session group this device is in, the content key of its current epoch, and nothing else (no room
    /// group's key, no older epoch). Empty while push is off on this device: the shell clears the keys' item.
    pub keys: Vec<NotifyKey>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyKey {
    /// The session group, hex: the room's 32 bytes, then the session's 16.
    pub group: String,
    pub epoch: u64,
    /// 32 bytes (its `Debug` shows only the length).
    pub key: Bytes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareTeam { pub id: String, pub name: String, pub crown: Option<crate::views::NoteToView> }

fn is_zero(n: &u32) -> bool {
    *n == 0
}
