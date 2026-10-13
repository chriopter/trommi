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
    /// Show a local notification (only where `Config.can.notify`).
    Notify {
        notice: Notice,
    },
    /// The numbers widgets and Live Activities show (only where `Config.can.snapshots`).
    Snapshot {
        snapshot: WidgetSnapshot,
    },
    /// Offer these bytes to the person as a file (download, share sheet, save dialog).
    SaveFile {
        name: String,
        media_type: String,
        bytes: Bytes,
    },
    /// Open this link outside the app (a share link, the help page).
    OpenUrl {
        url: String,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetSnapshot {
    pub waiting: u32,
    pub working: u32,
    pub urgent: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}
