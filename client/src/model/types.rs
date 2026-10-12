//! The shapes of the board model, as the apps read it (the web app's `types.ts` is the same contract): snake_case
//! names, lowercase hex ids, times in milliseconds, envelope numbers are the hub's change numbers. A record crosses to
//! an app as its JSON; fields a newer client wrote are kept (`extra`).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub type Obj = Map<String, Value>;

fn is_false(b: &bool) -> bool {
    !*b
}

/// The signed facts of a register or note write (spec 9.3.2), kept for display and for the cache.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Causal {
    pub sender_device_id: String,
    pub sender_sequence: u64,
    pub sent_at: u64,
    pub lamport: f64,
}

/// One version of a card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CardVersion {
    pub object_version: i64,
    pub version_hash: String,
    pub previous_version_hash: Option<String>,
    pub envelope_number: u64,
    pub sent_at: u64,
    pub object_state: String,
    pub urgency: String,
    pub content: Option<Obj>,
}

/// An answer to a card: the known fields, and whatever an echo or a newer client put beside them (`extra`:
/// `unsupported`, `no_body`, `local_id`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Answer {
    pub answer_action: String,
    pub choices: Value,
    pub note: Value,
    pub option_notes: Value,
    pub attachments: Value,
    pub marks: Value,
    pub trusted: bool,
    pub bound_version_hash: Option<String>,
    pub bound_object_version: i64,
    pub envelope_number: Option<u64>,
    pub envelope_hash: Option<String>,
    pub by_device_id: String,
    pub answered_at: u64,
    pub taken_back_at: Option<u64>,
    pub taken_back_sent_at: Option<u64>,
    pub pending: bool,
    #[serde(flatten)]
    pub extra: Obj,
}

/// A hand back (or What??) in force.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InRevision {
    pub by: String,
    pub envelope_number: u64,
}

/// The fields of a card that come from its newest version's body (`CARD_CONTENT_FIELDS`).
pub const CARD_CONTENT_FIELDS: &[&str] = &[
    "card_type",
    "title",
    "teaser",
    "body",
    "options",
    "sections",
    "html",
    "allows_multiple",
    "recommended",
    "urgency_reason",
    "attachments",
    "change_note",
    "close_summary",
    "withdraw_reason",
    "merged_into_object_id",
    "merged_from_object_ids",
    "working",
];
/// A card's card_type this version knows.
pub const CARD_TYPES: &[&str] = &["decision", "info"];
/// An answer's answer_action this version knows.
pub const ANSWER_ACTIONS: &[&str] = &["answer", "read", "shred"];

/// A card. The fields of its newest version's body live in `fields` (every one of `CARD_CONTENT_FIELDS`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Card {
    pub object_id: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub catching_up: bool,
    pub agent_device_id: String,
    pub session_id: Option<String>,
    pub object_state: String,
    pub urgency: String,
    #[serde(flatten)]
    pub fields: Obj,
    pub object_version: i64,
    pub version_hash: Option<String>,
    pub envelope_number: u64,
    pub first_envelope_number: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub versions: Vec<CardVersion>,
    pub answer: Option<Answer>,
    pub answers: Vec<Answer>,
    pub closed_how: Option<String>,
    pub in_revision: Option<InRevision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presented_envelope_number: Option<u64>,
    pub timeline_key: String,
    pub content_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<Value>,
    pub state_envelope_number: u64,
    /// The person's "Later" that holds it off the Desk now (desk.rs `snooze_holds`): until when, and when he put it off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_at: Option<u64>,
    /// An open card whose snooze ended (by the clock, by hand, by a newer version): until when it was put off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsnoozed: Option<u64>,
}

impl Card {
    pub fn field(&self, name: &str) -> &Value {
        self.fields.get(name).unwrap_or(&Value::Null)
    }
}

/// A permission request's verdict.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Verdict {
    pub allow: bool,
    pub by_device_id: String,
    pub envelope_number: u64,
}

/// A permission request.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionRequest {
    pub object_id: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub catching_up: bool,
    pub agent_device_id: String,
    pub session_id: Option<String>,
    pub tool_name: String,
    pub description: String,
    pub input_preview: String,
    pub expires_at: u64,
    pub version_hash: String,
    pub envelope_number: u64,
    pub sent_at: u64,
    pub permission_state: String,
    pub content_state: String,
    pub withdraw_reason: Option<String>,
    pub verdict: Option<Verdict>,
}

/// An Artifact (the model's map keeps its old name, `published`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Published {
    pub object_id: String,
    pub agent_device_id: String,
    pub session_id: Option<String>,
    pub attachments: Value,
    pub title: String,
    pub note: Option<Value>,
    pub released_until: Option<Value>,
    pub artifact_type: Option<Value>,
    pub object_version: i64,
    pub version_hash: String,
    pub envelope_number: u64,
    pub sent_at: u64,
    pub object_state: String,
    pub content_state: String,
}

/// A note: the app's fields beside these.
pub type Note = Obj;

/// One item of a timeline's window.
pub type TimelineItem = Obj;

/// A timeline: its counts and the window of items in memory (by envelope number, or local id while pending).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timeline {
    pub timeline_key: String,
    pub timeline_kind: String,
    pub timeline_id: String,
    pub object_id: String,
    pub item_count: u64,
    pub newest_envelope_number: u64,
    #[serde(skip)]
    pub items: BTreeMap<ItemKey, TimelineItem>,
    /// `Infinity` in the web model: no item loaded yet.
    pub loaded_down_to: Option<u64>,
    pub has_more: bool,
    pub window_open: bool,
}

/// An item's key in its timeline's window.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ItemKey {
    Number(u64),
    Local(String),
}

impl ItemKey {
    pub fn to_json(&self) -> Value {
        match self {
            ItemKey::Number(n) => Value::from(*n),
            ItemKey::Local(s) => Value::from(s.clone()),
        }
    }
    pub fn of(item: &TimelineItem) -> ItemKey {
        match item.get("envelope_number").and_then(Value::as_u64) {
            Some(n) => ItemKey::Number(n),
            None => ItemKey::Local(
                item.get("local_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ),
        }
    }
}

/// An agent's link report as the model keeps it (`clean_link`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Link {
    pub hears: String,
    pub attached: bool,
    pub last_call_at: Option<u64>,
    pub working: bool,
    pub since: Option<u64>,
    pub cut_since: Option<u64>,
    pub exit: Option<LinkExit>,
}

/// The connector's last word before its stream closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkExit {
    pub reason: String,
    pub claude: String,
}

/// A device of the room: a human device, an agent device or a helper device.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Member {
    pub device_id: String,
    pub device_role: String,
    pub device_name: String,
    pub fingerprint: String,
    pub platform: Value,
    pub folder: Value,
    pub host: Value,
    pub is_active: bool,
    pub added_entry_number: u64,
    pub removed_entry_number: Option<u64>,
    pub added_at: Option<u64>,
    pub is_me: bool,
    pub is_online: bool,
    pub offline_since: Option<u64>,
    pub link: Option<Link>,
}

/// One status line an agent keeps in its session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusLine {
    pub id: String,
    pub label: Value,
    pub state: Value,
    pub detail: Value,
    pub object_id: Value,
    pub envelope_number: u64,
    pub updated_at: u64,
}

/// A register of a session group, raw.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RegisterValue {
    pub value: Value,
    pub envelope_number: u64,
    pub sender_sequence: Option<u64>,
    pub causal: Option<Causal>,
}

/// An alert an agent keeps in its session (`alert/<key>`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentAlert {
    pub key: String,
    pub value: Value,
    pub envelope_number: u64,
}

/// A session: one session group, its agent device, its cards and Chat.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub session_id: String,
    pub group_id: Option<String>,
    pub agent_device_ids: Vec<String>,
    pub agent_device_id: Option<String>,
    pub agent_session_id: Option<String>,
    pub device_name: String,
    pub is_active: bool,
    pub stale: bool,
    pub group_archived: bool,
    pub is_online: bool,
    pub offline_since: Option<u64>,
    pub link: Option<Link>,
    pub heard_up_to: Option<u64>,
    pub heard_at: Option<u64>,
    pub session_key_epoch: u64,
    pub created_by_agent: bool,
    pub creator_device_id: Option<String>,
    pub parent_session_id: Option<String>,
    pub profile: Option<Obj>,
    pub status_lines: Vec<StatusLine>,
    pub agent_alerts: Vec<AgentAlert>,
    /// Every current register of the session's group, raw (a Map in the web model: entries as pairs).
    #[serde(with = "pairs")]
    pub registers: BTreeMap<String, RegisterValue>,
    pub settings: Option<Value>,
    pub card_ids: Vec<String>,
    pub open_card_ids: Vec<String>,
    pub timeline_key: String,
    pub last_activity_at: u64,
}

/// A room register as the human devices keep it: its raw entry. A delete stays as `value: null`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HumanRegister {
    pub value: Value,
    pub envelope_number: Option<u64>,
    pub by_device_id: Option<String>,
    pub pending: bool,
    pub causal: Option<Causal>,
}

/// Something the app tells the person (a finding, an agent's alert).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alert {
    pub alert_id: String,
    pub code: String,
    pub message: String,
    pub envelope_number: Option<u64>,
    pub sender_device_id: Option<String>,
    pub at: u64,
    pub source: String,
}

/// What a newer client wrote, counted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Newer {
    pub count: u64,
    pub what: Vec<String>,
    pub envelope_number: u64,
}

/// A map that crosses as a list of `[key, value]` pairs (a JavaScript Map in the web model).
pub mod pairs {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::BTreeMap;

    pub fn serialize<S: Serializer, V: Serialize>(
        map: &BTreeMap<String, V>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        let list: Vec<(&String, &V)> = map.iter().collect();
        list.serialize(s)
    }
    pub fn deserialize<'de, D: Deserializer<'de>, V: Deserialize<'de>>(
        d: D,
    ) -> Result<BTreeMap<String, V>, D::Error> {
        let list: Vec<(String, V)> = Vec::deserialize(d)?;
        Ok(list.into_iter().collect())
    }
}
