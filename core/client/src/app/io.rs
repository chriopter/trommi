//! The I/O the app core asks of its shell, and what comes back: plain data, no handles, no callbacks.
//!
//! Every request carries an [`IoId`] the core chose; its result comes back as an [`Event`](super::Event) with the
//! same id. An HTTP answer with any status is a [`HttpResponse`], not an error: the core's `hub` module reads the
//! status. An [`IoError`] is only what kept the shell from getting an answer at all.

use crate::app::bytes::Bytes;
use serde::{Deserialize, Serialize};

/// The id of one piece of I/O, chosen by the core: unique per [`App`](super::App), counting up from 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IoId(pub u64);

/// The outcome of one piece of I/O.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoResult<T> {
    Ok(T),
    Err(IoError),
}

impl<T> From<Result<T, IoError>> for IoResult<T> {
    fn from(result: Result<T, IoError>) -> Self {
        match result {
            Ok(value) => IoResult::Ok(value),
            Err(error) => IoResult::Err(error),
        }
    }
}

/// Why the shell got no answer. `detail` is the platform's words for a log; it never holds content or a key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IoError {
    pub kind: IoErrorKind,
    #[serde(default)]
    pub detail: String,
}

impl IoError {
    pub fn new(kind: IoErrorKind) -> Self {
        Self {
            kind,
            detail: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoErrorKind {
    /// No network, no route, DNS failed, the connection was refused or cut before an answer.
    Offline,
    /// The request's `timeout_ms` passed.
    Timeout,
    /// The answer's body is longer than the request's `max_answer`: the shell stopped reading there.
    TooLarge,
    /// The core cancelled it ([`Effect::Cancel`](super::Effect::Cancel)); the shell may also just drop it silently.
    Cancelled,
    /// A store write met another owner's revision ([`StoreOp::DeviceApply`]).
    Conflict,
    /// The platform's store is full.
    Full,
    /// The platform refused (permission, a locked Keychain before the first unlock).
    Denied,
    /// Anything else the platform reported.
    Failed,
}

/// One header, as written. Names are compared without case by the core. `Debug` shows the name only: a value
/// may be a token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Header({}: <redacted>)", self.name)
    }
}

/// How the platform should treat a request. Only a hint: a shell may run all of them the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HttpClass {
    /// The person waits for it.
    #[default]
    Interactive,
    /// Sync in the background.
    Background,
    /// A file going up: iOS may hand it to a background URLSession.
    Upload,
    /// A file coming down.
    Download,
}

/// One HTTP request, whole: the shell adds nothing (no auth, no cookies, no retries, no redirects followed).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    /// Absolute URL; the core builds it from the hub address it was started with.
    pub url: String,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Bytes>,
    pub timeout_ms: u32,
    /// Most bytes the answer's body may have, set by the core for every route (the hub is not trusted with how
    /// much it sends). The shell stops reading at the first byte beyond it, closes the connection and answers
    /// `IoError` of kind `too_large`: it never hands over a body cut short and never buffers more than this. A
    /// `Content-Length` above it fails before the body is read. Of a stream ([`Effect::OpenStream`](super::Effect::OpenStream))
    /// it bounds the body of an answer that is no 200; the events of an open stream are bounded by the core,
    /// which parses them. Left out in JSON: [`HttpRequest::MAX_ANSWER_DEFAULT`].
    #[serde(default = "HttpRequest::max_answer_default")]
    pub max_answer: u32,
    #[serde(default)]
    pub class: HttpClass,
    /// A transfer that may outlive this App (an iOS background upload) carries a stable key; its end then comes
    /// as `Event::Finished { key }` to whichever App runs. Everything else: `None`, and it dies with its App.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl HttpRequest {
    /// The cap of a request that names none: small, so that a route somebody forgot to cap fails closed.
    pub const MAX_ANSWER_DEFAULT: u32 = 64 << 10;

    fn max_answer_default() -> u32 {
        Self::MAX_ANSWER_DEFAULT
    }
}

/// An answer with any status.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: Vec<Header>,
    #[serde(default)]
    pub body: Bytes,
}

impl HttpResponse {
    /// The first header of this name, compared without case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
    }
}

/// What a stream opened by [`Effect::OpenStream`](super::Effect::OpenStream) brings, in order. The shell passes the
/// raw bytes as they arrive; the core parses the event stream (one parser for every app).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum StreamItem {
    /// The answer's head. A status other than 200 ends the stream after its body (as `chunk`s, then `ended`).
    Opened {
        status: u16,
        #[serde(default)]
        headers: Vec<Header>,
    },
    /// The next bytes, cut anywhere.
    Chunk { bytes: Bytes },
    /// The server ended it.
    Ended,
    /// It broke; nothing more comes for this id.
    Failed { error: IoError },
}

/// A key-value store, two spaces.
///
/// - **cache** (`Get`, `Scan`, `Put`): string keys, last write wins per key, may lose the last writes in a crash;
///   everything in it can be built again from the hub ([`crate::cache`] owns the layout).
/// - **device** (`DeviceLoad`, `DeviceApply`): the protocol core's state (keys, groups, chains, the outbox) with the
///   contract of `trommi_core::store`: all or nothing, durable on return, one revision checked against
///   `expected_revision`, one owner (the shell holds a file lock or a Web Lock before it sends `Start`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StoreOp {
    Get {
        keys: Vec<String>,
    },
    /// Every entry whose key starts with `prefix`, in key order.
    Scan {
        prefix: String,
    },
    /// Deletions first, then puts.
    Put {
        #[serde(default)]
        put: Vec<Entry>,
        #[serde(default)]
        delete: Vec<String>,
    },
    DeviceLoad,
    DeviceApply {
        expected_revision: u64,
        #[serde(default)]
        put: Vec<DeviceEntry>,
        #[serde(default)]
        delete: Vec<Bytes>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    pub value: Bytes,
}

/// A device-state entry: keys are bytes, values may be private keys (never logged).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub key: Bytes,
    pub value: Bytes,
}

impl std::fmt::Debug for DeviceEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceEntry(<redacted>)")
    }
}

/// What a store op answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum StoreAnswer {
    /// For `Get`: one per key asked, in the same order; `value` absent when not stored.
    Values { values: Vec<Option<Bytes>> },
    /// For `Scan`.
    Entries { entries: Vec<Entry> },
    /// For `Put` and `DeviceApply`.
    Done,
    /// For `DeviceLoad`: everything, and its revision (0 for an empty store).
    Device {
        revision: u64,
        entries: Vec<DeviceEntry>,
    },
}

/// The platform's secret store (Keychain, the browser's non-extractable keys stay the web shell's own business).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SecretOp {
    Get { name: String },
    Put { name: String, value: Bytes },
    Delete { name: String },
}

/// A platform service the core needs an answer from: what only the platform can do, asked for by the core so the
/// decision stays here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PlatformOp {
    /// Make a passkey for the hub's registration `challenge`, with the PRF extension evaluated at `prf_input`.
    PasskeyCreate {
        rp_id: String,
        challenge: Bytes,
        user_handle: Bytes,
        user_name: String,
        prf_input: Bytes,
    },
    /// Sign the hub's `challenge` with one of these passkeys, evaluating PRF at `prf_input`.
    PasskeyGet {
        rp_id: String,
        challenge: Bytes,
        credential_ids: Vec<Bytes>,
        prf_input: Bytes,
    },
    /// Ask for the right to notify (the platform's prompt), then register for push; the token comes as
    /// `PlatformAnswer::Push` and later changes as `Event::PushToken`.
    PushRegister {
        /// Web Push: the hub's VAPID key.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        server_key: Option<Bytes>,
    },
    PushUnregister,
}

/// What a platform service answered.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "answer", rename_all = "snake_case")]
pub enum PlatformAnswer {
    /// A new passkey: what the hub's registration route takes, and the PRF output (key material).
    PasskeyCreated {
        credential_id: Bytes,
        attestation_object: Bytes,
        client_data_json: Bytes,
        prf: Bytes,
    },
    /// A signed challenge: what the hub's sign-in route takes, and the PRF output (key material).
    PasskeyAsserted {
        credential_id: Bytes,
        authenticator_data: Bytes,
        client_data_json: Bytes,
        signature: Bytes,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_handle: Option<Bytes>,
        prf: Bytes,
    },
    Push {
        kind: String,
        token: Bytes,
    },
    /// The person said no, or the platform cannot.
    Refused,
    Done,
}

impl std::fmt::Debug for PlatformAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformAnswer::PasskeyCreated { .. } => f.write_str("PasskeyCreated(<redacted>)"),
            PlatformAnswer::PasskeyAsserted { .. } => f.write_str("PasskeyAsserted(<redacted>)"),
            PlatformAnswer::Push { kind, .. } => write!(f, "Push({kind}, <redacted>)"),
            PlatformAnswer::Refused => f.write_str("Refused"),
            PlatformAnswer::Done => f.write_str("Done"),
        }
    }
}
