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
    cards: Option<Entries<'a, Card>>,
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
            cards: entries(&change.cards).map(|ids| Entries { map: &this.cards, ids }),
            permissions: entries(&change.permissions).map(|ids| Entries { map: &this.permissions, ids }),
            notes: entries(&change.notes).map(|ids| Entries { map: &this.notes, ids }),
            published: entries(&change.published).map(|ids| Entries { map: &this.published, ids }),
            timelines,
        }
    }
}
