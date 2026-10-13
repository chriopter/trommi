//! The app's copy of the board model, kept by the client core's patches (as the web's `mirror.ts` and iOS's
//! `BoardMirror.swift` keep theirs), and the views the window draws from it, as JSON text for QML.
//!
//! Nothing is decided here: the Desk's order is the core's `stack`, the session list with its links and red hands
//! is the core's `session_list`, what a card shows is its record. This only picks the fields a view reads.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

/// One timeline as the patches leave it: its meta and its items by key.
#[derive(Default)]
struct Timeline {
    items: BTreeMap<ItemKey, Value>,
}

/// A timeline item's key: its envelope number, or the local id of an own echo (shown after every numbered item).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ItemKey {
    Number(u64),
    Local(String),
}

impl ItemKey {
    fn of_key(v: &Value) -> Option<ItemKey> {
        match v {
            Value::Number(n) => n.as_u64().map(ItemKey::Number),
            Value::String(s) => Some(ItemKey::Local(s.clone())),
            _ => None,
        }
    }
    fn of_item(item: &Value) -> ItemKey {
        match item["envelope_number"].as_u64() {
            Some(n) => ItemKey::Number(n),
            None => ItemKey::Local(item["local_id"].as_str().unwrap_or("").to_string()),
        }
    }
}

#[derive(Default)]
pub struct Mirror {
    cards: BTreeMap<String, Value>,
    permissions: BTreeMap<String, Value>,
    sessions: BTreeMap<String, Value>,
    members: BTreeMap<String, Value>,
    timelines: BTreeMap<String, Timeline>,
    stack: Vec<String>,
    open_permissions: Vec<String>,
    session_list: Vec<Value>,
    alerts: Vec<Value>,
    /// The person's registers (desks, the crown, drafts), by name: their values.
    human: BTreeMap<String, Value>,
    /// When the core wants to be asked again by the clock alone (ms); 0: never.
    pub wake_at: u64,
}

fn entries(v: &Value) -> impl Iterator<Item = (String, Value)> + '_ {
    v.as_array().into_iter().flatten().filter_map(|pair| {
        let id = pair.get(0)?.as_str()?.to_string();
        Some((id, pair.get(1).cloned().unwrap_or(Value::Null)))
    })
}

fn put(map: &mut BTreeMap<String, Value>, v: &Value) {
    for (id, record) in entries(v) {
        if record.is_null() {
            map.remove(&id);
        } else {
            map.insert(id, record);
        }
    }
}

impl Mirror {
    /// One patch of the client core (`ClientModel::finish`), as its JSON text.
    pub fn apply(&mut self, patch: &str) {
        let Ok(p) = serde_json::from_str::<Value>(patch) else {
            return;
        };
        put(&mut self.cards, &p["cards"]);
        put(&mut self.permissions, &p["permissions"]);
        put(&mut self.sessions, &p["sessions"]);
        if p["members"].is_array() {
            self.members.clear();
            put(&mut self.members, &p["members"]);
        }
        if let Some(stack) = p["stack"].as_array() {
            self.stack = stack.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        }
        if let Some(open) = p["open_permission_ids"].as_array() {
            self.open_permissions = open.iter().filter_map(|v| v.as_str().map(str::to_string)).collect();
        }
        if let Some(rows) = p["session_list"].as_array() {
            self.session_list = rows.clone();
        }
        for r in p["human"].as_array().into_iter().flatten() {
            let Some(key) = r["key"].as_str() else { continue };
            match r["raw"]["value"].clone() {
                Value::Null => self.human.remove(key),
                v => self.human.insert(key.to_string(), v),
            };
        }
        if let Some(alerts) = p["alerts"].as_array() {
            self.alerts = alerts.clone();
        }
        self.wake_at = p["wake_at"].as_u64().unwrap_or(0);
        for t in p["timelines"].as_array().into_iter().flatten() {
            let Some(key) = t["key"].as_str() else { continue };
            if t["meta"].is_null() {
                self.timelines.remove(key);
                continue;
            }
            let timeline = self.timelines.entry(key.to_string()).or_default();
            for item in t["items"].as_array().into_iter().flatten() {
                timeline.items.insert(ItemKey::of_item(item), item.clone());
            }
            let keep: std::collections::BTreeSet<ItemKey> = t["keys"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(ItemKey::of_key)
                .collect();
            timeline.items.retain(|k, _| keep.contains(k));
        }
    }

    /// A session's name as the session list shows it.
    fn session_name(&self, session_id: Option<&str>) -> String {
        let Some(sid) = session_id else {
            return String::new();
        };
        self.session_list
            .iter()
            .find(|r| r["session_id"].as_str() == Some(sid) || r["key"].as_str() == Some(sid))
            .and_then(|r| r["name"].as_str())
            .unwrap_or("")
            .to_string()
    }

    /// The Desk: the open cards in the core's order, then the open permission requests.
    pub fn desk_json(&self, now: u64) -> String {
        let mut rows = Vec::new();
        for id in &self.stack {
            let Some(c) = self.cards.get(id) else { continue };
            rows.push(card_row(c, &self.session_name(c["session_id"].as_str()), now));
        }
        for id in &self.open_permissions {
            let Some(p) = self.permissions.get(id) else { continue };
            rows.push(json!({
                "kind": "request",
                "id": id,
                "session": self.session_name(p["session_id"].as_str()),
                "session_id": p["session_id"],
                "title": p["tool_name"],
                "body": p["description"],
                "preview": p["input_preview"],
                "urgency": "high",
                "options": [
                    { "key": "allow", "label": "Allow" },
                    { "key": "deny", "label": "Deny" },
                ],
                "ago": ago(p["sent_at"].as_u64().unwrap_or(0), now),
            }));
        }
        Value::Array(rows).to_string()
    }

    /// The sidebar: the desks in their order, and every session that is not archived, each helper under its main
    /// session, with what the web draws of it: its mark and board id (its drawing and hue: the pen), its link (live,
    /// asleep, gone …), the questions it waits with, the red hand, the crown of its desk.
    pub fn sessions_json(&self) -> String {
        let live: Vec<&Value> = self
            .session_list
            .iter()
            .filter(|r| r["archived"] != json!(true) && r["removed"] != json!(true))
            .collect();
        // the desks: the registers `desk/<id>` (and the old `desks` map), by when they were made
        let mut desks: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(map) = self.human.get("desks").and_then(Value::as_object) {
            for (id, v) in map {
                desks.insert(id.clone(), v.clone());
            }
        }
        for (k, v) in &self.human {
            if let Some(id) = k.strip_prefix("desk/") {
                desks.insert(id.to_string(), v.clone());
            }
        }
        let crowned = |r: &Value| {
            let desk = r["desk"].as_str().filter(|d| !d.is_empty()).unwrap_or("main");
            let crown = desks.get(desk).map(|d| &d["crown"]).filter(|c| c.is_object()).or_else(|| self.human.get("crown"));
            r["starred"] == json!(true)
                || crown.is_some_and(|c| {
                    (c["session_id"].is_string() && c["session_id"] == r["session_id"])
                        || (c["agent_device_id"].is_string() && c["agent_device_id"] == r["agent_device_id"])
                })
        };
        let row = |r: &Value, depth: u32, helpers: Vec<Value>| {
            json!({
                "id": r["session_id"].as_str().or(r["key"].as_str()).unwrap_or(""),
                "board_id": r["id"],
                "mark": r["icon"],
                "name": r["name"],
                "online": r["online"],
                "link": r["link"]["state"].as_str().unwrap_or(""),
                "waiting": r["waiting"],
                "blocked": r["blocked"]["why"].as_str(),
                "working": r["working"],
                "desk": r["desk"].as_str().filter(|d| !d.is_empty()).unwrap_or("main"),
                "crown": crowned(r),
                "depth": depth,
                "helpers": helpers,
            })
        };
        let mut sessions = Vec::new();
        for main in live.iter().filter(|r| r["parent_key"].is_null()) {
            let key = main["key"].as_str();
            let subs: Vec<&&Value> = live.iter().filter(|r| r["parent_key"].as_str() == key && key.is_some()).collect();
            let edges = subs.iter().map(|h| json!({ "board_id": h["id"], "mark": h["icon"], "waiting": h["waiting"] })).collect();
            sessions.push(row(main, 0, edges));
            for helper in subs {
                sessions.push(row(helper, 1, Vec::new()));
            }
        }
        // (a helper whose main session is not listed still shows)
        for orphan in live.iter().filter(|r| {
            let parent = r["parent_key"].as_str();
            parent.is_some() && !live.iter().any(|m| m["key"].as_str() == parent)
        }) {
            sessions.push(row(orphan, 0, Vec::new()));
        }
        let mut order: Vec<(u64, String, String)> = desks
            .iter()
            .map(|(id, v)| (v["created_at"].as_u64().unwrap_or(0), id.clone(), v["name"].as_str().unwrap_or("").to_string()))
            .collect();
        // (a session on a desk no register names: the desk still shows)
        for s in &sessions {
            let d = s["desk"].as_str().unwrap_or("main");
            if !order.iter().any(|(_, id, _)| id == d) {
                order.push((u64::MAX, d.to_string(), String::new()));
            }
        }
        order.sort();
        let desks: Vec<Value> = order
            .into_iter()
            .map(|(_, id, name)| {
                let waiting: u64 = sessions.iter().filter(|s| s["desk"] == json!(id)).filter_map(|s| s["waiting"].as_u64()).sum();
                json!({ "id": id, "name": if name.is_empty() { if id == "main" { "Personal".to_string() } else { id.clone() } } else { name }, "waiting": waiting })
            })
            .collect();
        json!({ "desks": desks, "sessions": sessions }).to_string()
    }

    /// One session's Chat, oldest first, with who wrote each line.
    pub fn chat_json(&self, session_id: &str, me: &str, now: u64) -> String {
        let key = format!("chat:session/{session_id}");
        let name = self.session_name(Some(session_id));
        let Some(t) = self.timelines.get(&key) else {
            return "[]".into();
        };
        let rows: Vec<Value> = t
            .items
            .values()
            .filter(|i| i["content_type"] == json!("message") || i["content"]["text"].is_string())
            .map(|i| {
                let sender = i["sender_device_id"].as_str().unwrap_or("");
                let mine = sender == me;
                let human = self
                    .members
                    .get(sender)
                    .map(|m| m["device_role"] == json!("human"))
                    .unwrap_or(mine);
                json!({
                    "key": i["envelope_number"].as_u64().map(|n| n.to_string()).unwrap_or_else(|| i["local_id"].as_str().unwrap_or("").to_string()),
                    "text": i["content"]["text"],
                    "mine": mine,
                    "human": human,
                    "who": if mine { "You".to_string() } else if human { "You (other device)".to_string() } else { name.clone() },
                    "pending": i["pending"] == json!(true),
                    "ago": ago(i["sent_at"].as_u64().unwrap_or(0), now),
                    "at": i["sent_at"],
                })
            })
            .collect();
        Value::Array(rows).to_string()
    }

    /// The newest alerts, for the status line.
    pub fn last_alert(&self) -> Option<String> {
        self.alerts.last().and_then(|a| a["message"].as_str().map(str::to_string))
    }

    pub fn card(&self, id: &str) -> Option<&Value> {
        self.cards.get(id)
    }

    pub fn permission(&self, id: &str) -> Option<&Value> {
        self.permissions.get(id)
    }

    /// The agent device a session's messages go to.
    pub fn agent_of(&self, session_id: &str) -> Option<String> {
        self.session_list
            .iter()
            .find(|r| r["session_id"].as_str() == Some(session_id))
            .and_then(|r| r["agent_device_id"].as_str())
            .or_else(|| self.sessions.get(session_id).and_then(|s| s["agent_device_id"].as_str()))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.stack.len() + self.open_permissions.len(), self.session_list.len())
    }
}

/// "5 min ago" and the like.
pub fn ago(at: u64, now: u64) -> String {
    if at == 0 || at > now + 60_000 {
        return String::new();
    }
    let s = now.saturating_sub(at) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

/// A card as the Desk draws it: its newest version's fields, and its options (from `sections` where those carry
/// them).
fn card_row(c: &Value, session: &str, now: u64) -> Value {
    let mut options: Vec<Value> = c["options"].as_array().cloned().unwrap_or_default();
    let mut context = Vec::new();
    if options.is_empty() {
        for s in c["sections"].as_array().into_iter().flatten() {
            match s["key"].as_str() {
                Some(_) => options.push(json!({
                    "key": s["key"], "label": s["label"], "detail": s["text"],
                    "recommended": s["recommended"], "final": s["final"],
                })),
                None => {
                    if let Some(t) = s["text"].as_str() {
                        context.push(t.to_string());
                    }
                }
            }
        }
    }
    let recommended: Vec<String> = match &c["recommended"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        _ => Vec::new(),
    };
    let options: Vec<Value> = options
        .into_iter()
        .map(|mut o| {
            let key = o["key"].as_str().unwrap_or("").to_string();
            if let Some(m) = o.as_object_mut() {
                let pick = m.get("recommended") == Some(&json!(true)) || recommended.contains(&key);
                m.insert("recommended".into(), json!(pick));
                m.entry("detail").or_insert(Value::Null);
            }
            o
        })
        .collect();
    let mut body = c["body"].as_str().unwrap_or("").to_string();
    if !context.is_empty() {
        let joined = context.join("\n\n");
        body = if body.is_empty() { joined } else { format!("{body}\n\n{joined}") };
    }
    let attachments = c["attachments"].as_array().map(|a| a.len()).unwrap_or(0);
    let mut row = Map::new();
    row.insert("kind".into(), json!("card"));
    row.insert("id".into(), c["object_id"].clone());
    row.insert("card_type".into(), c["card_type"].clone());
    row.insert("title".into(), c["title"].clone());
    row.insert("teaser".into(), c["teaser"].clone());
    row.insert("body".into(), json!(body));
    row.insert("urgency".into(), c["urgency"].clone());
    row.insert("urgency_reason".into(), c["urgency_reason"].clone());
    row.insert("session".into(), json!(session));
    row.insert("session_id".into(), c["session_id"].clone());
    row.insert("options".into(), Value::Array(options));
    row.insert("multiple".into(), json!(c["allows_multiple"] == json!(true)));
    row.insert("state".into(), c["object_state"].clone());
    row.insert("answered".into(), json!(!c["answer"].is_null()));
    row.insert("pending".into(), json!(c["answer"]["envelope_hash"].is_null() && !c["answer"].is_null()));
    row.insert("working".into(), json!(c["working"] == json!(true)));
    row.insert("handed_back".into(), json!(!c["in_revision"].is_null()));
    row.insert("attachments".into(), json!(attachments));
    row.insert("readable".into(), json!(c["content_state"] == json!("ok")));
    row.insert("ago".into(), json!(ago(c["updated_at"].as_u64().or(c["created_at"].as_u64()).unwrap_or(0), now)));
    Value::Object(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_puts_and_removes_records_and_keeps_the_window() {
        let mut m = Mirror::default();
        m.apply(&json!({
            "cards": [["a", { "object_id": "a", "title": "A", "options": [{ "key": "x", "label": "X" }], "recommended": "x", "content_state": "ok", "session_id": "s" }]],
            "stack": ["a"],
            "session_list": [{ "key": "s", "session_id": "s", "name": "Website", "parent_key": null, "waiting": 1 }],
            "timelines": [{ "key": "chat:session/s", "meta": {}, "items": [
                { "envelope_number": 5, "content_type": "message", "content": { "text": "hi" }, "sender_device_id": "me" },
                { "envelope_number": 3, "content_type": "message", "content": { "text": "first" }, "sender_device_id": "agent" }
            ], "keys": [3, 5] }],
        }).to_string());
        let desk: Value = serde_json::from_str(&m.desk_json(0)).unwrap();
        assert_eq!(desk[0]["title"], "A");
        assert_eq!(desk[0]["session"], "Website");
        assert_eq!(desk[0]["options"][0]["recommended"], true);
        let chat: Value = serde_json::from_str(&m.chat_json("s", "me", 0)).unwrap();
        assert_eq!(chat[0]["text"], "first");
        assert_eq!(chat[1]["mine"], true);
        // the card leaves; an item leaves the window
        m.apply(&json!({ "cards": [["a", null]], "stack": [], "timelines": [{ "key": "chat:session/s", "meta": {}, "items": [], "keys": [5] }] }).to_string());
        assert_eq!(m.desk_json(0), "[]");
        let chat: Value = serde_json::from_str(&m.chat_json("s", "me", 0)).unwrap();
        assert_eq!(chat.as_array().unwrap().len(), 1);
    }

    #[test]
    fn sections_give_the_options() {
        let c = json!({ "object_id": "c", "sections": [
            { "text": "context" },
            { "key": "m", "label": "Monday", "text": "**Monday**", "recommended": true },
            { "key": "t", "label": "Tuesday", "text": "later" }
        ] });
        let row = card_row(&c, "", 0);
        assert_eq!(row["options"].as_array().unwrap().len(), 2);
        assert_eq!(row["options"][0]["recommended"], true);
        assert_eq!(row["body"], "context");
    }
}
