//! A small room for the views' rule tests, written as the demo's fixture is: sessions by name, cards by id.

use super::demo;
use crate::model::Model;
use serde_json::{json, Value};

pub const NOW: u64 = 1_791_900_300_000;
pub const MIN: u64 = 60_000;

/// A room written as the demo's fixture is: sessions by name, cards by id.
#[derive(Default)]
pub struct Room {
    pub sessions: Vec<Value>,
    pub cards: Vec<Value>,
    pub permissions: Vec<Value>,
    pub desks: Value,
    pub settings: serde_json::Map<String, Value>,
    pub crown: Value,
    pub snoozes: Value,
    pub stars: Value,
}

/// A session's device id: its name, padded to an id.
pub fn dev(name: &str) -> String {
    format!("{name:0<32}")
}

impl Room {
    pub fn session(&mut self, name: &str, parent: Option<&str>, online: bool) -> &mut Value {
        self.sessions.push(json!({
            "agent_device_id": dev(name), "agent_session_id": name, "device_name": name, "is_active": true, "is_online": online,
            "offline_since": if online { Value::Null } else { json!(NOW - 38 * MIN) },
            "profile": { "agent_name": name, "parent_session": parent, "is_main": parent.is_none() },
            "status_lines": [],
        }));
        self.sessions.last_mut().unwrap()
    }
    pub fn on_desk(&mut self, name: &str, desk: &str) {
        self.settings.insert(dev(name), json!({ "desk": desk }));
    }
    pub fn card(&mut self, id: &str, of: &str, kind: &str, urgency: &str) -> &mut Value {
        let n = 100 + self.cards.len() as u64;
        self.cards.push(json!({
            "object_id": id, "agent_device_id": dev(of), "card_type": kind, "title": id, "object_state": "open", "urgency": urgency,
            "created_at": NOW - 10 * MIN, "updated_at": NOW - 10 * MIN, "envelope_number": n, "first_envelope_number": n,
            "content_state": "ok", "versions": [], "answers": [], "timeline_key": format!("chat:card/{id}"),
        }));
        self.cards.last_mut().unwrap()
    }
    pub fn answered(&mut self, id: &str, of: &str, at: u64, number: u64) {
        let c = self.card(id, of, "decision", "normal");
        c["object_state"] = json!("answered");
        c["closed_how"] = json!("answered");
        c["answer"] = json!({ "answer_action": "answer", "choices": ["a"], "envelope_number": number, "answered_at": at });
    }
    pub fn request(&mut self, id: &str, of: &str, content_state: &str) {
        self.permissions.push(json!({
            "object_id": id, "agent_device_id": dev(of), "tool_name": "Bash", "permission_state": "pending", "content_state": content_state,
            "envelope_number": 900, "sent_at": NOW - MIN, "expires_at": NOW + 30 * MIN,
        }));
    }
    /// The room as a model, its Desk projected at `now`.
    pub fn model(&self, now: u64) -> Model {
        let fixture = json!({
            "made_at": now, "room": { "my_device_id": "me" }, "members": [], "sessions": self.sessions, "cards": self.cards,
            "permissions": self.permissions,
            "human": { "desks": self.desks, "session_settings": self.settings, "crown": self.crown, "snoozes": self.snoozes, "stars": self.stars },
        });
        demo::model_of(&fixture, now).unwrap()
    }
}
