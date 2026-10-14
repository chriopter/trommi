//! The demo room (`demo/data/fixture.json`, demo/README.md) as a board model: the room every app shows in its demo
//! mode, and the room the view model's golden tests are made from. No I/O: the caller reads the file.
//!
//! The fixture is the board model written plainly (lists, the person's registers grouped by kind). Read here as the
//! web's demo reads it (`app/web/public/demo/demo.mjs`): a session's key is its agent's device id, a card belongs
//! to its agent's session, every time is moved by `now - made_at` so "5 min ago" stays five minutes ago.

use crate::model::change::Change;
use crate::model::Model;
use serde_json::{json, Map, Value};

/// Every time stamp (a number of ms under a key `at`, `until`, `since`, `*_at`, `*_since`) moved by `by`.
fn moved(v: &Value, key: &str, by: i64) -> Value {
    match v {
        Value::Number(n) => {
            let timed = key == "at"
                || key == "until"
                || key == "since"
                || key.ends_with("_at")
                || key.ends_with("_since");
            match n.as_i64() {
                Some(t) if timed && t > 1_000_000_000_000 => json!(t + by),
                _ => v.clone(),
            }
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| moved(x, key, by)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), moved(x, k, by))).collect()),
        _ => v.clone(),
    }
}

fn list<'a>(f: &'a Value, key: &str) -> impl Iterator<Item = &'a Map<String, Value>> {
    f.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_object)
}

fn text<'a>(o: &'a Map<String, Value>, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The demo room as a model, its Desk projected at `now_ms`. `fixture`: the parsed `fixture.json`.
pub fn model_of(fixture: &Value, now_ms: u64) -> Result<Model, String> {
    let made = fixture.get("made_at").and_then(Value::as_i64).ok_or("the fixture names no made_at")?;
    let f = moved(fixture, "", now_ms as i64 - made);
    let pair = |id: &str, o: Map<String, Value>| json!([id, o]);

    // (a session's key is its 16-byte id as every app spells it: the demo's agent device ids, cut to their first half)
    let sid = |dev: &str| dev.get(..32).unwrap_or(dev).to_string();
    let members: Vec<Value> = list(&f, "members").map(|m| pair(text(m, "device_id"), m.clone())).collect();
    // (a card, a request or an Artifact belongs to its agent's session)
    let owned = |key: &str| -> Vec<Value> {
        list(&f, key)
            .map(|o| {
                let mut o = o.clone();
                if o.get("session_id").is_none_or(Value::is_null) {
                    o.insert("session_id".into(), json!(sid(text(&o, "agent_device_id"))));
                }
                if !o.contains_key("content_state") {
                    o.insert("content_state".into(), json!("ok"));
                }
                pair(text(&o, "object_id"), o.clone())
            })
            .collect()
    };
    let cards: Vec<Value> = list(&f, "cards")
        .map(|c| {
            let mut c = c.clone();
            let agent = text(&c, "agent_device_id").to_string();
            if c.get("session_id").is_none_or(Value::is_null) {
                c.insert("session_id".into(), json!(sid(&agent)));
            }
            if !c.contains_key("state_envelope_number") {
                c.insert("state_envelope_number".into(), c.get("envelope_number").cloned().unwrap_or(json!(0)));
            }
            pair(text(&c, "object_id"), c.clone())
        })
        .collect();
    let sessions: Vec<Value> = list(&f, "sessions")
        .map(|s| {
            let dev = text(s, "agent_device_id").to_string();
            let id = sid(&dev);
            let mut o = s.clone();
            o.insert("session_id".into(), json!(id));
            o.insert("agent_device_ids".into(), json!([dev]));
            o.insert("timeline_key".into(), json!(format!("chat:session/{id}")));
            let mine: Vec<&str> = list(&f, "cards")
                .filter(|c| text(c, "agent_device_id") == dev)
                .map(|c| text(c, "object_id"))
                .collect();
            o.insert("card_ids".into(), json!(mine));
            o.insert("registers".into(), json!([]));
            pair(&id, o)
        })
        .collect();
    let notes: Vec<Value> = list(&f, "notes").map(|n| pair(text(n, "object_id"), n.clone())).collect();

    // The person's registers under their names: `session/<key>`, `desk/<id>`, `snooze/<card>`, `draft/<card>`,
    // `duck/<card>`, `star/<card>`, `crown`.
    let mut human: Vec<Value> = Vec::new();
    let h = f.get("human").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut put = |key: String, value: &Value| {
        if !value.is_null() {
            // (no change number: a "Later" then holds whatever version its card has, as in the web's demo)
            human.push(json!([key, { "value": value }]));
        }
    };
    for (group, name) in [
        ("desks", "desk"),
        ("snoozes", "snooze"),
        ("drafts", "draft"),
        ("ducks", "duck"),
        ("stars", "star"),
    ] {
        for (id, v) in h.get(group).and_then(Value::as_object).into_iter().flatten() {
            put(format!("{name}/{id}"), v);
        }
    }
    // (the demo's sessions stand in the order the fixture lists them, as the web's demo shows them: a session the
    //  person never placed gets its place in that list)
    let settings = h.get("session_settings").and_then(Value::as_object);
    for (i, s) in list(&f, "sessions").enumerate() {
        let dev = text(s, "agent_device_id");
        let id = sid(dev);
        let mut set = settings.and_then(|all| all.get(dev)).or(s.get("settings")).and_then(Value::as_object).cloned().unwrap_or_default();
        if !set.get("position").is_some_and(Value::is_number) {
            set.insert("position".into(), json!(i));
        }
        put(format!("session/{id}"), &Value::Object(set));
    }
    if let Some(c) = h.get("crown") {
        put("crown".into(), c);
    }

    // The conversations, every item in memory (the demo has no hub to page from).
    let timelines: Vec<Value> = f
        .get("timelines")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(key, items)| {
            // (a session's Chat under its session's key)
            let key = &match key.strip_prefix("chat:session/") {
                Some(dev) => format!("chat:session/{}", sid(dev)),
                None => key.clone(),
            };
            let (kind, id) = key.split_once(':').unwrap_or((key.as_str(), ""));
            let items: Vec<&Value> = items.as_array().into_iter().flatten().collect();
            let number = |i: &Value| i.get("envelope_number").and_then(Value::as_u64);
            json!([key, {
                "timeline_key": key,
                "timeline_kind": kind,
                "timeline_id": id,
                "object_id": id.split('/').nth(1).unwrap_or(""),
                "item_count": items.len(),
                "newest_envelope_number": items.iter().filter_map(|i| number(i)).max().unwrap_or(0),
                "loaded_down_to": items.iter().filter_map(|i| number(i)).min(),
                "has_more": false,
                "window_open": true,
                "items": items.iter().map(|i| json!([number(i).map(Value::from).unwrap_or_else(|| i.get("local_id").cloned().unwrap_or(Value::Null)), i])).collect::<Vec<_>>(),
            }])
        })
        .collect();

    let room = f.get("room").cloned().unwrap_or(Value::Null);
    let snapshot = json!({
        "room": room,
        "members": members,
        "sessions": sessions,
        "cards": cards,
        "permissions": owned("permissions"),
        "notes": notes,
        "published": owned("published"),
        "human": human,
        "timelines": timelines,
    });
    let mut m = Model::new();
    m.load(&snapshot)?;
    m.project(&mut Change::default(), now_ms);
    Ok(m)
}
