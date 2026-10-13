//! What an app's copy of the model needs after one change: the records it names (the web app's `ModelPatch`,
//! mirror.ts), as JSON. A Map of the web model crosses as a list of `[key, value]` pairs.

use super::change::Change;
use super::types::*;
use super::Model;
use serde::{Serialize, Serializer};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The records a change names, as `[id, record]` pairs (`null`: gone), written straight from the records.
struct Entries<'a, V> {
    map: &'a BTreeMap<String, V>,
    ids: &'a BTreeSet<String>,
}

impl<V: Serialize> Serialize for Entries<'_, V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.ids.iter().map(|id| (id, self.map.get(id))))
    }
}

/// A card as a lean patch carries it: the card without what only a card's own view reads (a list or object field, a
/// version's body, the answers before the one in force). Its history goes whole (each entry's note and title). `lazy` names the fields left out; `lean: true` says the rest
/// is read on demand (`Model::card_json`).
pub struct LeanCard<'a>(pub &'a Card);

/// A version's header (a lean card's `versions`).
#[derive(Serialize)]
struct VersionHead<'a> {
    object_version: i64,
    version_hash: &'a str,
    previous_version_hash: &'a Option<String>,
    envelope_number: u64,
    sent_at: u64,
    object_state: &'a str,
    urgency: &'a str,
}

impl Serialize for LeanCard<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let c = self.0;
        let (plain, lazy) = c.fields.lean();
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("lean", &true)?;
        m.serialize_entry("object_id", &c.object_id)?;
        if c.catching_up {
            m.serialize_entry("catching_up", &true)?;
        }
        m.serialize_entry("agent_device_id", &c.agent_device_id)?;
        m.serialize_entry("session_id", &c.session_id)?;
        m.serialize_entry("object_state", &c.object_state)?;
        m.serialize_entry("urgency", &c.urgency)?;
        for (k, v) in plain {
            m.serialize_entry(k, v)?;
        }
        m.serialize_entry("lazy", &lazy)?;
        m.serialize_entry("object_version", &c.object_version)?;
        m.serialize_entry("version_hash", &c.version_hash)?;
        m.serialize_entry("envelope_number", &c.envelope_number)?;
        m.serialize_entry("first_envelope_number", &c.first_envelope_number)?;
        m.serialize_entry("created_at", &c.created_at)?;
        m.serialize_entry("updated_at", &c.updated_at)?;
        let heads: Vec<VersionHead> = c
            .versions
            .iter()
            .map(|v| VersionHead {
                object_version: v.object_version,
                version_hash: &v.version_hash,
                previous_version_hash: &v.previous_version_hash,
                envelope_number: v.envelope_number,
                sent_at: v.sent_at,
                object_state: &v.object_state,
                urgency: &v.urgency,
            })
            .collect();
        m.serialize_entry("versions", &heads)?;
        m.serialize_entry("history", &c.history)?;
        m.serialize_entry("answer", &c.answer)?;
        m.serialize_entry("closed_how", &c.closed_how)?;
        m.serialize_entry("in_revision", &c.in_revision)?;
        if let Some(p) = c.presented_envelope_number {
            m.serialize_entry("presented_envelope_number", &p)?;
        }
        m.serialize_entry("timeline_key", &c.timeline_key)?;
        m.serialize_entry("content_state", &c.content_state)?;
        if let Some(u) = &c.unsupported {
            m.serialize_entry("unsupported", u)?;
        }
        m.serialize_entry("state_envelope_number", &c.state_envelope_number)?;
        for (k, v) in [("snoozed_until", c.snoozed_until), ("snoozed_at", c.snoozed_at), ("unsnoozed", c.unsnoozed)] {
            if let Some(v) = v {
                m.serialize_entry(k, &v)?;
            }
        }
        m.end()
    }
}

/// The cards a change names, lean.
struct LeanEntries<'a> {
    map: &'a BTreeMap<String, Card>,
    ids: &'a BTreeSet<String>,
}

impl Serialize for LeanEntries<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.ids.iter().map(|id| (id, self.map.get(id).map(LeanCard))))
    }
}

/// Cards whole or lean.
enum CardEntries<'a> {
    Whole(Entries<'a, Card>),
    Lean(LeanEntries<'a>),
}

impl Serialize for CardEntries<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            CardEntries::Whole(e) => e.serialize(s),
            CardEntries::Lean(e) => e.serialize(s),
        }
    }
}

/// One timeline in a patch: its meta, the items the change names, the keys of its window.
#[derive(Serialize)]
struct TimelineOut<'a> {
    key: &'a str,
    meta: Value,
    items: Vec<&'a TimelineItem>,
    keys: Vec<Value>,
}

/// A patch as it is written: the few small parts as values, the records straight from the model.
#[derive(Serialize)]
struct PatchOut<'a> {
    #[serde(flatten)]
    small: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sessions: Option<Entries<'a, Session>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cards: Option<CardEntries<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    permissions: Option<Entries<'a, PermissionRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<Entries<'a, Note>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    published: Option<Entries<'a, Published>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timelines: Option<Vec<TimelineOut<'a>>>,
}

fn timeline_meta(t: &Timeline) -> Value {
    let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
    if let Some(o) = v.as_object_mut() {
        // (`Infinity` in the web model while nothing is loaded: JSON has no such number)
        if t.loaded_down_to.is_none() {
            o.insert("loaded_down_to".into(), Value::Null);
        }
    }
    v
}

impl Model {
    /// The room facts the model keeps.
    pub fn room_json(&self) -> Value {
        json!({ "key_epoch": self.key_epoch, "recovery_key": self.recovery_key })
    }

    /// One card whole, as JSON text (a lean copy reads the rest of a card by it); empty for none.
    pub fn card_json(&self, id: &str) -> String {
        self.cards.get(id).and_then(|c| serde_json::to_string(c).ok()).unwrap_or_default()
    }

    /// The patch for one change.
    pub fn patch(&mut self, change: &Change) -> Value {
        let out = self.patch_out(change);
        serde_json::to_value(out).unwrap_or(Value::Null)
    }

    /// The patch for one change as JSON text, as `patch` (written without a `Value` of its records).
    pub fn patch_text(&mut self, change: &Change) -> String {
        let out = self.patch_out(change);
        serde_json::to_string(&out).unwrap_or_default()
    }

    fn patch_out<'a>(&'a mut self, change: &'a Change) -> PatchOut<'a> {
        // The session list every app shows (sessions.rs), whenever something it is made from may have changed: the
        // sessions, their devices, the person's settings, the Desk (the red hand reads it), the clock.
        let rows = (change.room
            || change.members
            || change.stack
            || change.status
            || !change.sessions.is_empty()
            || !change.registers.is_empty()
            || !change.cards.is_empty()
            || !change.permissions.is_empty())
        .then(|| {
            let rows = crate::sessions::session_list(self, self.reading);
            self.status_wake = rows.iter().filter_map(|r| r.recheck_at).min().unwrap_or(0);
            rows
        });
        let this: &'a Model = self;
        let mut p = Map::new();
        p.insert("room".into(), this.room_json());
        if change.room {
            p.insert("newer".into(), serde_json::to_value(&this.newer).unwrap_or(Value::Null));
        }
        if change.members {
            p.insert(
                "members".into(),
                Value::Array(this.members.iter().map(|(id, m)| json!([id, m])).collect()),
            );
        }
        if change.members || change.registers.iter().any(|k| k.starts_with("device/")) {
            p.insert(
                "devregs".into(),
                Value::Array(this.device_registers.iter().map(|(id, r)| json!([id, r])).collect()),
            );
        }
        if !change.registers.is_empty() {
            let human: Vec<Value> = change
                .registers
                .iter()
                .filter(|k| !k.starts_with("device/"))
                .map(|k| json!({ "key": k, "raw": this.human.get(k) }))
                .collect();
            p.insert("human".into(), Value::Array(human));
        }
        if change.alerts {
            p.insert("alerts".into(), serde_json::to_value(&this.alerts).unwrap_or(Value::Null));
        }
        if change.stack {
            p.insert("stack".into(), json!(this.stack));
            p.insert("open_permission_ids".into(), json!(this.open_permission_ids));
            p.insert("desk_order".into(), json!(this.desk_order));
        }
        if let Some(rows) = rows {
            p.insert("session_list".into(), serde_json::to_value(rows).unwrap_or(Value::Null));
        }
        // When the app is to project again although nothing came: the next snooze or request to run out, the next
        // change of a session's status by the clock (0: none).
        let wake = [this.desk_wake, this.status_wake]
            .into_iter()
            .filter(|w| *w != 0)
            .min()
            .unwrap_or(0);
        p.insert("wake_at".into(), json!(wake));
        let items: Vec<Value> = change
            .items
            .iter()
            .map(|(k, ks)| json!([k, ks.iter().map(ItemKey::to_json).collect::<Vec<_>>()]))
            .collect();
        p.insert("change".into(), json!({
            "cards": change.cards, "sessions": change.sessions, "permissions": change.permissions, "notes": change.notes,
            "published": change.published, "timelines": change.timelines, "registers": change.registers, "invites": [],
            "members": change.members, "alerts": change.alerts, "outbox": false, "stack": change.stack, "room": change.room, "status": change.status,
            "items": items,
        }));
        let entries = |ids: &'a BTreeSet<String>| (!ids.is_empty()).then_some(ids);
        let mut keys: BTreeSet<&'a String> = change.timelines.iter().collect();
        keys.extend(change.items.keys());
        let timelines = (!keys.is_empty()).then(|| {
            keys.iter()
                .map(|key| match this.timelines.get(*key) {
                    None => TimelineOut { key, meta: Value::Null, items: vec![], keys: vec![] },
                    Some(t) => TimelineOut {
                        key,
                        meta: timeline_meta(t),
                        items: change.items.get(*key).into_iter().flatten().filter_map(|k| t.items.get(k)).collect(),
                        keys: t.items.keys().map(ItemKey::to_json).collect(),
                    },
                })
                .collect()
        });
        PatchOut {
            small: p,
            sessions: entries(&change.sessions).map(|ids| Entries { map: &this.sessions, ids }),
            cards: entries(&change.cards).map(|ids| {
                if this.lean_cards {
                    CardEntries::Lean(LeanEntries { map: &this.cards, ids })
                } else {
                    CardEntries::Whole(Entries { map: &this.cards, ids })
                }
            }),
            permissions: entries(&change.permissions).map(|ids| Entries { map: &this.permissions, ids }),
            notes: entries(&change.notes).map(|ids| Entries { map: &this.notes, ids }),
            published: entries(&change.published).map(|ids| Entries { map: &this.published, ids }),
            timelines,
        }
    }
}
