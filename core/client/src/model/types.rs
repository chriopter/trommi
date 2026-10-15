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
    pub content: Option<RawObj>,
}

/// A version's body as its JSON text: read seldom (a card's fields hold the newest one), kept small (as a `Value` a
/// body takes several times its text).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawObj(Box<serde_json::value::RawValue>);

impl RawObj {
    pub fn of(o: &Obj) -> RawObj {
        RawObj(serde_json::value::to_raw_value(o).expect("an object is JSON"))
    }
    /// The body as an object (an empty one for what is no object).
    pub fn obj(&self) -> Obj {
        serde_json::from_str(self.0.get()).unwrap_or_default()
    }
    pub fn text(&self) -> &str {
        self.0.get()
    }
}

impl PartialEq for RawObj {
    fn eq(&self, other: &RawObj) -> bool {
        self.0.get() == other.0.get() || self.obj() == other.obj()
    }
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
/// One content field of a card: a plain value as it is; a list or object (options, sections, attachments) as its JSON
/// text, read only by the apps (as `Value`s they would take several times their text, a thousand cards' worth).
#[derive(Clone, Debug)]
pub enum Slot {
    Plain(Value),
    Text(Box<serde_json::value::RawValue>),
}

impl Default for Slot {
    fn default() -> Slot {
        Slot::Plain(Value::Null)
    }
}

impl Slot {
    pub fn of(v: Value) -> Slot {
        match &v {
            Value::Array(a) if !a.is_empty() => serde_json::value::to_raw_value(&v).map(Slot::Text).unwrap_or(Slot::Plain(v)),
            Value::Object(o) if !o.is_empty() => serde_json::value::to_raw_value(&v).map(Slot::Text).unwrap_or(Slot::Plain(v)),
            _ => Slot::Plain(v),
        }
    }
    pub fn value(&self) -> Value {
        match self {
            Slot::Plain(v) => v.clone(),
            Slot::Text(t) => serde_json::from_str(t.get()).unwrap_or(Value::Null),
        }
    }
}

impl PartialEq for Slot {
    fn eq(&self, other: &Slot) -> bool {
        match (self, other) {
            (Slot::Plain(a), Slot::Plain(b)) => a == b,
            (Slot::Text(a), Slot::Text(b)) if a.get() == b.get() => true,
            _ => self.value() == other.value(),
        }
    }
}

impl Serialize for Slot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Slot::Plain(v) => v.serialize(s),
            Slot::Text(t) => t.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for Slot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Slot, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        let t = raw.get().trim_start();
        if (t.starts_with('[') || t.starts_with('{')) && t.len() > 2 {
            // (an empty list or object, with blanks in it, is kept as text: it reads the same)
            Ok(Slot::Text(raw))
        } else {
            serde_json::from_str(raw.get()).map(Slot::Plain).map_err(serde::de::Error::custom)
        }
    }
}

/// A card's content fields, one slot per name of `CARD_CONTENT_FIELDS` (the newest version's body; a field it lacks
/// holds its default). Slots, not a map: a thousand cards would keep a thousand maps of the same seventeen names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CardFields([Slot; 17]);

impl CardFields {
    fn slot(name: &str) -> Option<usize> {
        CARD_CONTENT_FIELDS.iter().position(|f| *f == name)
    }
    /// A content field's value (null for a name that is none).
    pub fn get(&self, name: &str) -> Value {
        Self::slot(name).map(|i| self.0[i].value()).unwrap_or(Value::Null)
    }
    /// Sets a content field; a name that is none is not kept.
    pub fn insert(&mut self, name: &str, v: Value) {
        if let Some(i) = Self::slot(name) {
            self.0[i] = Slot::of(v);
        }
    }
}

impl CardFields {
    /// The plain fields, and the names of the ones kept as text (a lean card names them in `lazy`).
    pub fn lean(&self) -> (Vec<(&'static str, &Value)>, Vec<&'static str>) {
        let (mut plain, mut lazy) = (Vec::new(), Vec::new());
        for (name, slot) in CARD_CONTENT_FIELDS.iter().zip(self.0.iter()) {
            match slot {
                Slot::Plain(v) => plain.push((*name, v)),
                Slot::Text(_) => lazy.push(*name),
            }
        }
        (plain, lazy)
    }
}

impl Serialize for CardFields {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(CARD_CONTENT_FIELDS.iter().zip(self.0.iter()))
    }
}

/// A card's card_type this version knows.
pub const CARD_TYPES: &[&str] = &["decision", "info"];
/// An answer's answer_action this version knows.
pub const ANSWER_ACTIONS: &[&str] = &["answer", "read", "shred"];

/// A card. The fields of its newest version's body live in `fields` (every one of `CARD_CONTENT_FIELDS`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, from = "CardIn")]
pub struct Card {
    pub object_id: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub catching_up: bool,
    pub agent_device_id: String,
    pub session_id: Option<String>,
    pub object_state: String,
    pub urgency: String,
    #[serde(flatten)]
    pub fields: CardFields,
    pub object_version: i64,
    pub version_hash: Option<String>,
    pub envelope_number: u64,
    pub first_envelope_number: u64,
    pub created_at: u64,
    pub updated_at: u64,
    pub versions: Vec<CardVersion>,
    /// What the card went through, the oldest first: every version a human reads as the card (`history_of`). Kept
    /// with the versions (the apps list it under the card, the newest version stands on top).
    pub history: Vec<Revision>,
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

/// A card as it is read: its content fields by name (no flattened map, so that its records are read straight).
#[derive(Default, Deserialize)]
#[serde(default)]
struct CardIn {
    object_id: String,
    catching_up: bool,
    agent_device_id: String,
    session_id: Option<String>,
    object_state: String,
    urgency: String,
    card_type: Option<Slot>,
    title: Option<Slot>,
    teaser: Option<Slot>,
    body: Option<Slot>,
    options: Option<Slot>,
    sections: Option<Slot>,
    html: Option<Slot>,
    allows_multiple: Option<Slot>,
    recommended: Option<Slot>,
    urgency_reason: Option<Slot>,
    attachments: Option<Slot>,
    change_note: Option<Slot>,
    close_summary: Option<Slot>,
    withdraw_reason: Option<Slot>,
    merged_into_object_id: Option<Slot>,
    merged_from_object_ids: Option<Slot>,
    working: Option<Slot>,
    object_version: i64,
    version_hash: Option<String>,
    envelope_number: u64,
    first_envelope_number: u64,
    created_at: u64,
    updated_at: u64,
    versions: Vec<CardVersion>,
    answer: Option<Answer>,
    answers: Vec<Answer>,
    closed_how: Option<String>,
    in_revision: Option<InRevision>,
    presented_envelope_number: Option<u64>,
    timeline_key: String,
    content_state: String,
    unsupported: Option<Value>,
    state_envelope_number: u64,
    snoozed_until: Option<u64>,
    snoozed_at: Option<u64>,
    unsnoozed: Option<u64>,
}

impl From<CardIn> for Card {
    fn from(c: CardIn) -> Card {
        let slots = [
            c.card_type,
            c.title,
            c.teaser,
            c.body,
            c.options,
            c.sections,
            c.html,
            c.allows_multiple,
            c.recommended,
            c.urgency_reason,
            c.attachments,
            c.change_note,
            c.close_summary,
            c.withdraw_reason,
            c.merged_into_object_id,
            c.merged_from_object_ids,
            c.working,
        ];
        Card {
            object_id: c.object_id,
            catching_up: c.catching_up,
            agent_device_id: c.agent_device_id,
            session_id: c.session_id,
            object_state: c.object_state,
            urgency: c.urgency,
            fields: CardFields(slots.map(Option::unwrap_or_default)),
            object_version: c.object_version,
            version_hash: c.version_hash,
            envelope_number: c.envelope_number,
            first_envelope_number: c.first_envelope_number,
            created_at: c.created_at,
            updated_at: c.updated_at,
            history: history_of(&c.versions),
            versions: c.versions,
            answer: c.answer,
            answers: c.answers,
            closed_how: c.closed_how,
            in_revision: c.in_revision,
            presented_envelope_number: c.presented_envelope_number,
            timeline_key: c.timeline_key,
            content_state: c.content_state,
            unsupported: c.unsupported,
            state_envelope_number: c.state_envelope_number,
            snoozed_until: c.snoozed_until,
            snoozed_at: c.snoozed_at,
            unsnoozed: c.unsnoozed,
        }
    }
}

/// One entry of a card's history: a version a human reads as the card, with what its agent said changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Revision {
    pub object_version: i64,
    pub envelope_number: u64,
    pub sent_at: u64,
    /// The version's `change_note` (revise_card's note), "" without one.
    pub note: String,
    /// The version's title, "" where its body is not read.
    pub title: String,
}

/// A card's history from its versions (oldest first): the first version, and every later one that leaves the card
/// open (a revision). A version that closes it (close, withdraw, merge) is no revision: the apps show it as the end.
/// Every revision stays: a newer one never replaces the ones before it.
pub fn history_of(versions: &[CardVersion]) -> Vec<Revision> {
    versions
        .iter()
        .filter(|v| v.object_version == 1 || v.object_state == "open")
        .map(|v| {
            let body = v.content.as_ref().map(RawObj::obj).unwrap_or_default();
            let text = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
            Revision {
                object_version: v.object_version,
                envelope_number: v.envelope_number,
                sent_at: v.sent_at,
                note: text("change_note"),
                title: text("title"),
            }
        })
        .collect()
}

impl Card {
    pub fn field(&self, name: &str) -> Value {
        self.fields.get(name)
    }
    /// A content field as text, using the same spelling as an agent's tool result.
    pub fn s(&self, name: &str) -> String {
        match self.field(name) { Value::Null => String::new(), v => crate::codec::js_string(&v) }
    }
    pub fn title(&self) -> String {
        self.s("title")
    }
    pub fn card_type(&self) -> String {
        self.field("card_type").as_str().unwrap_or("decision").to_string()
    }
    pub fn options(&self) -> Vec<Value> {
        self.field("options").as_array().cloned().unwrap_or_default()
    }
    pub fn option_keys(&self) -> Vec<String> {
        self.options().iter().map(|o| o.get("key").map(crate::codec::js_string).unwrap_or_else(|| "undefined".into())).collect()
    }
    pub fn recommended_list(&self) -> Vec<String> {
        match self.field("recommended") {
            Value::Null => Vec::new(),
            Value::Array(a) => a.iter().map(crate::codec::js_string).collect(),
            v => vec![crate::codec::js_string(&v)],
        }
    }
    pub fn attachments(&self) -> Vec<Value> {
        self.field("attachments").as_array().cloned().unwrap_or_default()
    }
    /// A card of a newer Trommi (its type or schema): read as an info. (`unsupported` may stand as null.)
    pub fn is_unsupported(&self) -> bool {
        match &self.unsupported {
            None | Some(Value::Null) | Some(Value::Bool(false)) => false,
            Some(Value::String(s)) => !s.is_empty(),
            Some(_) => true,
        }
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

#[cfg(test)]
mod history_tests {
    use super::*;
    use serde_json::json;

    /// A card asked once, revised five times (each with its note), then closed: the version that closes it is no revision.
    fn revised_five_times() -> Card {
        let mut versions = vec![json!({ "object_version": 1, "envelope_number": 10, "sent_at": 1000, "object_state": "open", "content": { "title": "Working on: the history" } })];
        for n in 2..=6 {
            versions.push(json!({ "object_version": n, "envelope_number": 10 * n, "sent_at": 1000 * n, "object_state": "open",
                "content": { "title": format!("Step {n}"), "change_note": format!("step {n} done") } }));
        }
        versions.push(json!({ "object_version": 7, "envelope_number": 70, "sent_at": 7000, "object_state": "closed", "content": { "title": "Step 6", "close_summary": "done" } }));
        serde_json::from_value(json!({ "object_id": "c1", "agent_device_id": "a1", "object_state": "closed", "versions": versions })).unwrap()
    }

    #[test]
    fn a_card_revised_five_times_keeps_five_revisions_under_its_first_version() {
        let card = revised_five_times();
        let h = &card.history;
        assert_eq!(h.len(), 6, "the first version and every revision; the closing version is none");
        assert_eq!(h.iter().filter(|r| r.object_version > 1).count(), 5);
        assert_eq!(h.iter().map(|r| r.object_version).collect::<Vec<_>>(), vec![1, 2, 3, 4, 5, 6]);
        assert_eq!(h[0].title, "Working on: the history");
        assert_eq!(h[0].note, "");
        assert_eq!((h[3].note.as_str(), h[3].title.as_str(), h[3].sent_at, h[3].envelope_number), ("step 4 done", "Step 4", 4000, 40));
        // (written and read again, the history is the same: worked out from the versions)
        let again: Card = serde_json::from_value(serde_json::to_value(&card).unwrap()).unwrap();
        assert_eq!(again.history, card.history);
    }

    #[test]
    fn a_version_whose_body_is_not_read_is_still_an_entry() {
        let card: Card = serde_json::from_value(json!({ "object_id": "c1", "versions": [
            { "object_version": 1, "envelope_number": 1, "object_state": "open", "content": { "title": "Q" } },
            { "object_version": 2, "envelope_number": 2, "object_state": "open" },
        ] }))
        .unwrap();
        assert_eq!(card.history.len(), 2);
        assert_eq!((card.history[1].note.as_str(), card.history[1].title.as_str()), ("", ""));
    }
}
