//! The whole model as JSON, and back: for an app's first copy, and for a start from its cache. Maps cross as lists of
//! `[key, value]` pairs; a timeline's items as such a list under `items`; the room registers as `human` pairs (their
//! raw entries).

use super::types::*;
use super::Model;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn pairs<V: serde::Serialize>(map: &BTreeMap<String, V>) -> Value {
    Value::Array(map.iter().map(|(k, v)| json!([k, v])).collect())
}

fn read_pairs<V: DeserializeOwned>(
    v: Option<&Value>,
    what: &str,
) -> Result<BTreeMap<String, V>, String> {
    let mut out = BTreeMap::new();
    for e in v.and_then(Value::as_array).into_iter().flatten() {
        let (Some(k), Some(val)) = (e.get(0).and_then(Value::as_str), e.get(1)) else {
            return Err(format!("{what}: not a pair"));
        };
        if val.is_null() {
            continue;
        }
        out.insert(
            k.to_string(),
            serde_json::from_value(val.clone()).map_err(|err| format!("{what} {k}: {err}"))?,
        );
    }
    Ok(out)
}

impl Model {
    /// The whole model.
    pub fn snapshot(&self) -> Value {
        let timelines: Vec<Value> = self
            .timelines
            .iter()
            .map(|(k, t)| {
                let mut meta = serde_json::to_value(t).unwrap_or(Value::Null);
                if let Some(o) = meta.as_object_mut() {
                    o.insert(
                        "items".into(),
                        Value::Array(
                            t.items
                                .iter()
                                .map(|(ik, it)| json!([ik.to_json(), it]))
                                .collect(),
                        ),
                    );
                }
                json!([k, meta])
            })
            .collect();
        json!({
            "room": self.room_json(),
            "members": pairs(&self.members),
            "sessions": pairs(&self.sessions),
            "cards": pairs(&self.cards),
            "permissions": pairs(&self.permissions),
            "notes": pairs(&self.notes),
            "published": pairs(&self.published),
            "timelines": timelines,
            "human": pairs(&self.human),
            "alerts": self.alerts,
            "stack": self.stack,
            "open_permission_ids": self.open_permission_ids,
            "newer": self.newer,
        })
    }

    /// The records of a snapshot (an app's cache): what is not in it stays empty. The Desk is built anew at the next
    /// projection.
    pub fn load(&mut self, v: &Value) -> Result<(), String> {
        if let Some(room) = v.get("room") {
            if let Some(e) = room.get("key_epoch").and_then(Value::as_u64) {
                self.key_epoch = e;
            }
            self.recovery_key = room.get("recovery_key").filter(|r| !r.is_null()).cloned();
            if let Some(me) = room.get("my_device_id").and_then(Value::as_str) {
                self.my_device_id = Some(me.to_string());
            }
        }
        self.members = read_pairs(v.get("members"), "member")?;
        self.sessions = read_pairs(v.get("sessions"), "session")?;
        self.cards = read_pairs(v.get("cards"), "card")?;
        self.permissions = read_pairs(v.get("permissions"), "permission request")?;
        self.notes = read_pairs(v.get("notes"), "note")?;
        self.published = read_pairs(v.get("published"), "artifact")?;
        self.human = read_pairs(v.get("human"), "register")?;
        self.timelines.clear();
        for e in v
            .get("timelines")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (Some(k), Some(meta)) = (e.get(0).and_then(Value::as_str), e.get(1)) else {
                continue;
            };
            let mut t: Timeline = serde_json::from_value(meta.clone())
                .map_err(|err| format!("timeline {k}: {err}"))?;
            for it in meta
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(item) = it.get(1).and_then(Value::as_object) {
                    t.items.insert(ItemKey::of(item), item.clone());
                }
            }
            self.timelines.insert(k.to_string(), t);
        }
        self.device_registers = v
            .get("devregs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some((
                    e.get(0)?.as_str()?.to_string(),
                    e.get(1)?.as_object()?.clone(),
                ))
            })
            .collect();
        if let Some(a) = v.get("alerts") {
            self.alerts = serde_json::from_value(a.clone()).unwrap_or_default();
        }
        if let Some(n) = v.get("newer") {
            self.newer = serde_json::from_value(n.clone()).unwrap_or_default();
        }
        // (the Desk's own marks are its to set again)
        for c in self.cards.values_mut() {
            c.catching_up = false;
        }
        for r in self.permissions.values_mut() {
            r.catching_up = false;
        }
        self.desk_whole = false;
        Ok(())
    }
}

impl Model {
    /// Every record is named changed: the next patch is the whole model (a copy made anew).
    pub fn touch_all(&self, change: &mut super::change::Change) {
        change.cards.extend(self.cards.keys().cloned());
        change.sessions.extend(self.sessions.keys().cloned());
        change.permissions.extend(self.permissions.keys().cloned());
        change.notes.extend(self.notes.keys().cloned());
        change.published.extend(self.published.keys().cloned());
        change.timelines.extend(self.timelines.keys().cloned());
        change.registers.extend(self.human.keys().cloned());
        for (k, t) in &self.timelines {
            let list = change.items.entry(k.clone()).or_default();
            for ik in t.items.keys() {
                if !list.contains(ik) {
                    list.push(ik.clone());
                }
            }
        }
        change.members = true;
        change.alerts = true;
        change.stack = true;
        change.room = true;
    }
}
