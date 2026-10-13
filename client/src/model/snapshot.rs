//! The whole model as JSON, and back: for an app's first copy, and for a start from its cache. Maps cross as lists of
//! `[key, value]` pairs; a timeline's items as such a list under `items`; the room registers as `human` pairs (their
//! raw entries).

use super::types::*;
use super::Model;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A map as its list of `[key, value]` pairs, written straight from the records (no `Value` in between).
struct Pairs<'a, V>(&'a BTreeMap<String, V>);

impl<V: Serialize> Serialize for Pairs<'_, V> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.0.iter())
    }
}

/// The whole model as `snapshot()` gives it, written as text without building it as a `Value` first.
#[derive(Serialize)]
struct SnapshotOut<'a> {
    room: Value,
    members: Pairs<'a, Member>,
    sessions: Pairs<'a, Session>,
    cards: Pairs<'a, Card>,
    permissions: Pairs<'a, PermissionRequest>,
    notes: Pairs<'a, Note>,
    published: Pairs<'a, Published>,
    timelines: Vec<Value>,
    human: Pairs<'a, HumanRegister>,
    alerts: &'a Vec<Alert>,
    stack: &'a Vec<String>,
    open_permission_ids: &'a Vec<String>,
    newer: &'a Newer,
}

/// A snapshot's records as they are read: each map typed at once (no `Value` in between); the rest as it stands.
#[derive(Deserialize, Default)]
#[serde(default)]
struct SnapshotIn {
    room: Value,
    members: Vec<(String, Option<Member>)>,
    sessions: Vec<(String, Option<Session>)>,
    cards: Vec<(String, Option<Card>)>,
    permissions: Vec<(String, Option<PermissionRequest>)>,
    notes: Vec<(String, Option<Note>)>,
    published: Vec<(String, Option<Published>)>,
    human: Vec<(String, Option<HumanRegister>)>,
    timelines: Value,
    devregs: Value,
    alerts: Option<Value>,
    newer: Option<Value>,
}

fn typed<V>(list: Vec<(String, Option<V>)>) -> BTreeMap<String, V> {
    list.into_iter().filter_map(|(k, v)| Some((k, v?))).collect()
}

impl Model {
    /// The whole model.
    pub fn snapshot(&self) -> Value {
        serde_json::to_value(self.snapshot_out()).unwrap_or(Value::Null)
    }

    /// The whole model as JSON text, as `snapshot()` (an app's cache; written without a `Value` of it all).
    pub fn snapshot_text(&self) -> String {
        serde_json::to_string(&self.snapshot_out()).unwrap_or_default()
    }

    fn snapshot_out(&self) -> SnapshotOut<'_> {
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
        SnapshotOut {
            room: self.room_json(),
            members: Pairs(&self.members),
            sessions: Pairs(&self.sessions),
            cards: Pairs(&self.cards),
            permissions: Pairs(&self.permissions),
            notes: Pairs(&self.notes),
            published: Pairs(&self.published),
            timelines,
            human: Pairs(&self.human),
            alerts: &self.alerts,
            stack: &self.stack,
            open_permission_ids: &self.open_permission_ids,
            newer: &self.newer,
        }
    }

    /// The records of a snapshot (an app's cache): what is not in it stays empty. The Desk is built anew at the next
    /// projection.
    pub fn load(&mut self, v: &Value) -> Result<(), String> {
        self.load_from(SnapshotIn::deserialize(v).map_err(|e| e.to_string())?)
    }

    /// As `load`, from the snapshot's text (each record typed as it is read).
    pub fn load_text(&mut self, text: &str) -> Result<(), String> {
        self.load_from(serde_json::from_str(text).map_err(|e| e.to_string())?)
    }

    fn load_from(&mut self, v: SnapshotIn) -> Result<(), String> {
        let room = &v.room;
        if !room.is_null() {
            if let Some(e) = room.get("key_epoch").and_then(Value::as_u64) {
                self.key_epoch = e;
            }
            self.recovery_key = room.get("recovery_key").filter(|r| !r.is_null()).cloned();
            if let Some(me) = room.get("my_device_id").and_then(Value::as_str) {
                self.my_device_id = Some(me.to_string());
            }
        }
        self.members = typed(v.members);
        self.sessions = typed(v.sessions);
        self.cards = typed(v.cards);
        self.permissions = typed(v.permissions);
        self.notes = typed(v.notes);
        self.published = typed(v.published);
        self.human = typed(v.human);
        self.timelines.clear();
        for e in v
            .timelines
            .as_array()
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
            .devregs
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|e| {
                Some((
                    e.get(0)?.as_str()?.to_string(),
                    e.get(1)?.as_object()?.clone(),
                ))
            })
            .collect();
        if let Some(a) = v.alerts {
            self.alerts = serde_json::from_value(a).unwrap_or_default();
        }
        if let Some(n) = v.newer {
            self.newer = serde_json::from_value(n).unwrap_or_default();
        }
        // (the Desk's own marks are its to set again)
        for c in self.cards.values_mut() {
            c.catching_up = false;
        }
        // (a snooze mark a copy kept is looked at again by the next projection: its register may be gone since)
        self.snooze_marked = self
            .cards
            .values()
            .filter(|c| c.snoozed_until.is_some() || c.snoozed_at.is_some() || c.unsnoozed.is_some())
            .map(|c| c.object_id.clone())
            .collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn filled() -> Model {
        let mut m = Model::new();
        let card: Card = serde_json::from_value(json!({
            "object_id": "c1", "agent_device_id": "a1", "object_version": 2, "catching_up": true,
            "title": "Ask", "options": [{ "key": "a", "label": "A" }], "sections": [], "teaser": null,
            "versions": [{ "object_version": 1, "content": { "title": "Before" } }],
        }))
        .unwrap();
        m.cards.insert("c1".into(), card);
        m.members.insert("a1".into(), serde_json::from_value(json!({ "device_id": "a1", "device_role": "agent" })).unwrap());
        m.sessions.insert("s1".into(), serde_json::from_value(json!({ "session_id": "s1" })).unwrap());
        m.human.insert("star/c1".into(), serde_json::from_value(json!({ "value": true, "envelope_number": 3 })).unwrap());
        let mut t = Timeline { timeline_key: "chat:s1".into(), item_count: 2, ..Timeline::default() };
        for it in [json!({ "envelope_number": 4, "content": "hi" }), json!({ "local_id": "l1", "pending": true })] {
            let o = it.as_object().unwrap().clone();
            t.items.insert(ItemKey::of(&o), o);
        }
        m.timelines.insert("chat:s1".into(), t);
        m.stack = vec!["c1".into()];
        m
    }

    trait Tap {
        fn tap_catching_up(self, v: bool) -> Self;
    }
    impl Tap for Card {
        fn tap_catching_up(mut self, v: bool) -> Card {
            self.catching_up = v;
            self
        }
    }

    #[test]
    fn the_snapshot_text_is_the_snapshot_and_loads_as_it() {
        let m = filled();
        let text = m.snapshot_text();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), m.snapshot());
        let (mut a, mut b) = (Model::new(), Model::new());
        a.load_text(&text).unwrap();
        b.load(&m.snapshot()).unwrap();
        assert_eq!(a.snapshot(), b.snapshot());
        assert_eq!(a.cards["c1"].fields.get("title"), json!("Ask"));
        assert!(!a.cards["c1"].catching_up);
        // (the fields as slots and a version's body as text read and write as the values they are)
        assert_eq!(a.cards["c1"].fields.get("options"), json!([{ "key": "a", "label": "A" }]));
        assert_eq!(a.cards["c1"].fields.get("sections"), json!([]));
        assert_eq!(a.cards["c1"].fields.get("nothing"), Value::Null);
        assert_eq!(a.cards["c1"], m.cards["c1"].clone().tap_catching_up(false));
        assert_eq!(a.cards["c1"].versions[0].content.as_ref().map(|c| c.obj()), json!({ "title": "Before" }).as_object().cloned());
        let card = serde_json::to_value(&a.cards["c1"]).unwrap();
        assert_eq!((card["options"].clone(), card["working"].clone(), card["versions"][0]["content"].clone()),
            (json!([{ "key": "a", "label": "A" }]), Value::Null, json!({ "title": "Before" })));
        assert_eq!(a.timelines["chat:s1"].items.len(), 2);
        // (a pair whose value is null stands for no record; what is not a snapshot is refused)
        assert!(a.load_text(r#"{"cards":[["c2",null]]}"#).is_ok() && a.cards.is_empty());
        assert!(a.load_text(r#"{"cards":[["c2"]]}"#).is_err());
        assert!(a.load_text("[").is_err());
    }

    #[test]
    fn the_patch_text_is_the_patch() {
        let mut m = filled();
        let mut c = crate::model::change::Change::new();
        m.touch_all(&mut c);
        c.cards.insert("gone".into());
        c.items.entry("chat:gone".into()).or_default();
        let text = m.patch_text(&c);
        let v = m.patch(&c);
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), v);
        assert_eq!(v["cards"], json!([["c1", m.snapshot()["cards"][0][1]], ["gone", null]]));
        assert_eq!(v["timelines"][0]["meta"], Value::Null);
        assert_eq!(v["timelines"][1]["items"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_lean_patch_leaves_what_a_card_view_reads_to_card_json() {
        let mut m = filled();
        m.lean_cards = true;
        let mut c = crate::model::change::Change::new();
        m.touch_all(&mut c);
        let text = m.patch_text(&c);
        let v = m.patch(&c);
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), v);
        let lean = &v["cards"][0][1];
        assert_eq!((lean["lean"].clone(), lean["title"].clone(), lean["lazy"].clone()), (json!(true), json!("Ask"), json!(["options"])));
        assert_eq!((lean.get("options"), lean.get("answers")), (None, None));
        assert_eq!(lean["sections"], json!([]));
        assert_eq!(lean["versions"][0].get("content"), None);
        assert_eq!(lean["versions"][0]["object_version"], json!(1));
        let whole: Value = serde_json::from_str(&m.card_json("c1")).unwrap();
        assert_eq!(whole, m.snapshot()["cards"][0][1]);
        assert_eq!(m.card_json("none"), "");
    }
}
