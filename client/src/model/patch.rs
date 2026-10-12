//! What an app's copy of the model needs after one change: the records it names (the web app's `ModelPatch`,
//! mirror.ts), as JSON. A Map of the web model crosses as a list of `[key, value]` pairs.

use super::change::Change;
use super::types::*;
use super::Model;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

fn entries<V: Serialize>(
    map: &BTreeMap<String, V>,
    ids: &std::collections::BTreeSet<String>,
) -> Value {
    Value::Array(
        ids.iter()
            .map(|id| {
                json!([
                    id,
                    map.get(id)
                        .map(|v| serde_json::to_value(v).unwrap_or(Value::Null))
                        .unwrap_or(Value::Null)
                ])
            })
            .collect(),
    )
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
        let mut p = Map::new();
        p.insert("room".into(), self.room_json());
        if change.room {
            p.insert(
                "newer".into(),
                serde_json::to_value(&self.newer).unwrap_or(Value::Null),
            );
        }
        if change.members {
            p.insert(
                "members".into(),
                Value::Array(self.members.iter().map(|(id, m)| json!([id, m])).collect()),
            );
        }
        if change.members || change.registers.iter().any(|k| k.starts_with("device/")) {
            p.insert(
                "devregs".into(),
                Value::Array(
                    self.device_registers
                        .iter()
                        .map(|(id, r)| json!([id, r]))
                        .collect(),
                ),
            );
        }
        if !change.sessions.is_empty() {
            p.insert("sessions".into(), entries(&self.sessions, &change.sessions));
        }
        if !change.cards.is_empty() {
            p.insert("cards".into(), entries(&self.cards, &change.cards));
        }
        if !change.permissions.is_empty() {
            p.insert(
                "permissions".into(),
                entries(&self.permissions, &change.permissions),
            );
        }
        if !change.notes.is_empty() {
            p.insert("notes".into(), entries(&self.notes, &change.notes));
        }
        if !change.published.is_empty() {
            p.insert(
                "published".into(),
                entries(&self.published, &change.published),
            );
        }
        let mut keys: std::collections::BTreeSet<String> = change.timelines.clone();
        keys.extend(change.items.keys().cloned());
        if !keys.is_empty() {
            let list: Vec<Value> = keys
                .iter()
                .map(|key| match self.timelines.get(key) {
                    None => json!({ "key": key, "meta": null, "items": [], "keys": [] }),
                    Some(t) => {
                        let items: Vec<Value> = change
                            .items
                            .get(key)
                            .into_iter()
                            .flatten()
                            .filter_map(|k| t.items.get(k))
                            .map(|it| Value::Object(it.clone()))
                            .collect();
                        let ks: Vec<Value> = t.items.keys().map(ItemKey::to_json).collect();
                        json!({ "key": key, "meta": timeline_meta(t), "items": items, "keys": ks })
                    }
                })
                .collect();
            p.insert("timelines".into(), Value::Array(list));
        }
        if !change.registers.is_empty() {
            let human: Vec<Value> = change
                .registers
                .iter()
                .filter(|k| !k.starts_with("device/"))
                .map(|k| json!({ "key": k, "raw": self.human.get(k) }))
                .collect();
            p.insert("human".into(), Value::Array(human));
        }
        if change.alerts {
            p.insert(
                "alerts".into(),
                serde_json::to_value(&self.alerts).unwrap_or(Value::Null),
            );
        }
        if change.stack {
            p.insert("stack".into(), json!(self.stack));
            p.insert(
                "open_permission_ids".into(),
                json!(self.open_permission_ids),
            );
        }
        // The session list every app shows (sessions.rs), whenever something it is made from may have changed: the
        // sessions, their devices, the person's settings, the Desk (the red hand reads it), the clock.
        if change.room
            || change.members
            || change.stack
            || change.status
            || !change.sessions.is_empty()
            || !change.registers.is_empty()
            || !change.cards.is_empty()
            || !change.permissions.is_empty()
        {
            let rows = crate::sessions::session_list(self, self.reading);
            self.status_wake = rows.iter().filter_map(|r| r.recheck_at).min().unwrap_or(0);
            p.insert(
                "session_list".into(),
                serde_json::to_value(rows).unwrap_or(Value::Null),
            );
        }
        // When the app is to project again although nothing came: the next snooze or request to run out, the next
        // change of a session's status by the clock (0: none).
        let wake = [self.desk_wake, self.status_wake]
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
        Value::Object(p)
    }
}
