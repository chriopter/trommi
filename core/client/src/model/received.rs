//! What an app hands the model of one envelope the protocol core took (the facade's `ReceivedEnvelope`, its ids
//! already as hex). The model reads it and nothing else: every check is the core's.

use serde::{Deserialize, Serialize};

/// What became of the envelope at the core.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    Chained,
    Void,
    Provisional,
    Refused,
}

/// The envelope's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeKind {
    Item,
    Version,
    Answer,
    Request,
    Verdict,
    Register,
    TakeBack,
    Reserved,
}

/// Which timeline an item stands on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelineKind {
    SessionChat,
    CardChat,
    Board,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimelineRef {
    pub kind: TimelineKind,
    pub id: String,
}

/// The object block of the header.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectHead {
    pub object_id: String,
    /// card | note | request | artifact | another a newer Trommi defines
    pub object_type: String,
    /// open | answered | closed
    pub object_state: String,
    /// low | normal | high | critical
    pub urgency: String,
    pub answered_at: u64,
    /// The version before (a version), the version answered (an answer, a take back), the request (a verdict);
    /// none for zeros.
    pub object_ref: Option<String>,
}

/// The object as the envelopes accepted so far leave it (the core's replay).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectAfter {
    pub owner: String,
    pub object_state: String,
    pub current: String,
}

/// What the body binds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Bind {
    Answer {
        version_hash: Option<String>,
        choices: Vec<String>,
    },
    TakeBack {
        previous_hash: Option<String>,
    },
    Request {
        expires_at: u64,
    },
    Verdict {
        allow: bool,
    },
    Other,
}

/// A register value that was taken.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegisterTaken {
    pub name: String,
    pub current: bool,
}

/// One received envelope.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Received {
    pub change: u64,
    pub hash: String,
    pub outcome: Outcome,
    pub code: Option<String>,
    pub kind: EnvelopeKind,
    pub sender: String,
    pub seq: u64,
    pub recipient: Option<String>,
    pub time: u64,
    pub session_id: Option<String>,
    pub timeline: Option<TimelineRef>,
    pub object: Option<ObjectHead>,
    pub object_after: Option<ObjectAfter>,
    pub bind: Option<Bind>,
    pub register: Option<RegisterTaken>,
    /// The body's JSON as the core opened it (wire form); none when it did not open.
    pub payload: Option<String>,
}
