//! Where each record of the board model lives in the cache, and how a scan of them becomes a model again.
//!
//! One key per record, so a live entry writes the few records it touched and not the board:
//!
//! | key after `c1/<room>/model/` | value |
//! | --- | --- |
//! | `room` | the room's own fields (`Model::room_json`: key epoch, the recovery key's public fingerprint) |
//! | `members` | every member, as `[id, member]` pairs |
//! | `alerts`, `newer` | the alerts kept, what only a newer Trommi understands |
//! | `card/<id>`, `sess/<id>`, `perm/<id>`, `note/<id>`, `pub/<id>`, `reg/<name>` | one record each |
//! | `tl/<timeline>` | a timeline's window (count, newest, how far loaded, whether more is older) |
//! | `item/<timeline>/n<number>`, `item/<timeline>/l<local id>` | one item of a timeline's window |
//!
//! An id is one key segment: `%` and `/` in it are written `%25` and `%2F`.
//!
//! Reading builds the model's snapshot text from the values as they stand (each one checked to be one JSON value,
//! never parsed into a tree) and hands it to `Model::load_text`, the one reader of a snapshot.

use crate::model::types::{ItemKey, TimelineItem};
use crate::model::Model;
use serde_json::value::RawValue;
use std::fmt::Write as _;

/// The part every board record is under: `c1/<room>/model/…`.
pub const PART: &str = super::part::MODEL;

/// One record of the board model, as the cache keys it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Rec {
    Room,
    Members,
    Alerts,
    Newer,
    Card(String),
    Session(String),
    Permission(String),
    Note(String),
    Published(String),
    Register(String),
    Timeline(String),
    Item(String, ItemKey),
}

/// `%` and `/` of an id, escaped: an id stays one key segment.
pub fn escape(id: &str) -> String {
    if !id.contains(['%', '/']) {
        return id.to_string();
    }
    id.replace('%', "%25").replace('/', "%2F")
}

pub fn unescape(segment: &str) -> String {
    if !segment.contains('%') {
        return segment.to_string();
    }
    segment.replace("%2F", "/").replace("%25", "%")
}

fn item_segment(key: &ItemKey) -> String {
    match key {
        ItemKey::Number(n) => format!("n{n}"),
        ItemKey::Local(id) => format!("l{}", escape(id)),
    }
}

fn item_key(segment: &str) -> Option<ItemKey> {
    match segment.split_at_checked(1)? {
        ("n", n) => n.parse().ok().map(ItemKey::Number),
        ("l", id) => Some(ItemKey::Local(unescape(id))),
        _ => None,
    }
}

impl Rec {
    /// The key's tail after `c1/<room>/model/`.
    pub fn tail(&self) -> String {
        match self {
            Rec::Room => "room".into(),
            Rec::Members => "members".into(),
            Rec::Alerts => "alerts".into(),
            Rec::Newer => "newer".into(),
            Rec::Card(id) => format!("card/{}", escape(id)),
            Rec::Session(id) => format!("sess/{}", escape(id)),
            Rec::Permission(id) => format!("perm/{}", escape(id)),
            Rec::Note(id) => format!("note/{}", escape(id)),
            Rec::Published(id) => format!("pub/{}", escape(id)),
            Rec::Register(id) => format!("reg/{}", escape(id)),
            Rec::Timeline(id) => format!("tl/{}", escape(id)),
            Rec::Item(timeline, key) => format!("item/{}/{}", escape(timeline), item_segment(key)),
        }
    }

    /// The record a key's tail names; none for a tail this format does not know.
    pub fn of_tail(tail: &str) -> Option<Rec> {
        let (kind, rest) = tail.split_once('/').unwrap_or((tail, ""));
        Some(match (kind, rest) {
            ("room", "") => Rec::Room,
            ("members", "") => Rec::Members,
            ("alerts", "") => Rec::Alerts,
            ("newer", "") => Rec::Newer,
            (_, "") => return None,
            ("card", id) => Rec::Card(unescape(id)),
            ("sess", id) => Rec::Session(unescape(id)),
            ("perm", id) => Rec::Permission(unescape(id)),
            ("note", id) => Rec::Note(unescape(id)),
            ("pub", id) => Rec::Published(unescape(id)),
            ("reg", id) => Rec::Register(unescape(id)),
            ("tl", id) => Rec::Timeline(unescape(id)),
            ("item", rest) => {
                let (timeline, item) = rest.split_once('/')?;
                Rec::Item(unescape(timeline), item_key(item)?)
            }
            _ => return None,
        })
    }

    /// The record's value as the model holds it now; none when the model holds no such record (it is deleted).
    pub fn value(&self, m: &Model) -> Option<Vec<u8>> {
        fn json<T: serde::Serialize>(value: &T) -> Option<Vec<u8>> {
            serde_json::to_vec(value).ok()
        }
        match self {
            Rec::Room => json(&m.room_json()),
            Rec::Members => json(&m.members.iter().collect::<Vec<_>>()),
            Rec::Alerts => json(&m.alerts),
            Rec::Newer => json(&m.newer),
            Rec::Card(id) => json(m.cards.get(id)?),
            Rec::Session(id) => json(m.sessions.get(id)?),
            Rec::Permission(id) => json(m.permissions.get(id)?),
            Rec::Note(id) => json(m.notes.get(id)?),
            Rec::Published(id) => json(m.published.get(id)?),
            Rec::Register(id) => json(m.human.get(id)?),
            Rec::Timeline(id) => json(m.timelines.get(id)?),
            Rec::Item(timeline, key) => json::<TimelineItem>(m.timelines.get(timeline)?.items.get(key)?),
        }
    }
}

/// The snapshot text of the records a scan found (`(tail, value)` in key order), for `Model::load_text`. None when
/// a value is not one JSON value or a timeline's window is no object: the cache is not this format's.
pub fn snapshot_text(records: &[(Rec, &[u8])]) -> Option<String> {
    let mut lists: [Vec<(&str, &str)>; 7] = Default::default();
    let mut single: [Option<&str>; 4] = [None; 4];
    let mut timelines: std::collections::BTreeMap<&str, (Option<&str>, Vec<&str>)> = Default::default();
    for (rec, bytes) in records {
        let text = serde_json::from_slice::<&RawValue>(bytes).ok()?.get();
        let slot = |rec: &Rec| -> Option<usize> {
            Some(match rec {
                Rec::Card(_) => 0,
                Rec::Session(_) => 1,
                Rec::Permission(_) => 2,
                Rec::Note(_) => 3,
                Rec::Published(_) => 4,
                Rec::Register(_) => 5,
                _ => return None,
            })
        };
        match rec {
            Rec::Room => single[0] = Some(text),
            Rec::Members => single[1] = Some(text),
            Rec::Alerts => single[2] = Some(text),
            Rec::Newer => single[3] = Some(text),
            Rec::Timeline(id) => timelines.entry(id.as_str()).or_default().0 = Some(text),
            Rec::Item(id, _) => timelines.entry(id.as_str()).or_default().1.push(text),
            Rec::Card(id) | Rec::Session(id) | Rec::Permission(id) | Rec::Note(id) | Rec::Published(id) | Rec::Register(id) => {
                lists[slot(rec)?].push((id.as_str(), text));
            }
        }
    }
    let mut out = String::with_capacity(records.iter().map(|(_, b)| b.len() + 24).sum::<usize>() + 256);
    out.push('{');
    let _ = write!(out, r#""room":{},"members":{}"#, single[0].unwrap_or("null"), single[1].unwrap_or("[]"));
    if let Some(alerts) = single[2] {
        let _ = write!(out, r#","alerts":{alerts}"#);
    }
    if let Some(newer) = single[3] {
        let _ = write!(out, r#","newer":{newer}"#);
    }
    for (name, list) in ["cards", "sessions", "permissions", "notes", "published", "human"].iter().zip(&lists) {
        let _ = write!(out, r#","{name}":["#);
        for (i, (id, text)) in list.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_pair(&mut out, id, text);
        }
        out.push(']');
    }
    out.push_str(r#","timelines":["#);
    let mut first = true;
    for (id, (meta, items)) in &timelines {
        // (items without their window are left: the window was deleted and its items are on their way out)
        let Some(meta) = meta else { continue };
        let body = meta.strip_prefix('{')?.strip_suffix('}')?;
        if !first {
            out.push(',');
        }
        first = false;
        out.push('[');
        out.push_str(&serde_json::to_string(id).ok()?);
        out.push_str(",{");
        out.push_str(body);
        out.push_str(if body.is_empty() { r#""items":["# } else { r#","items":["# });
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("[0,");
            out.push_str(item);
            out.push(']');
        }
        out.push_str("]}]");
    }
    out.push_str("]}");
    Some(out)
}

fn push_pair(out: &mut String, id: &str, text: &str) {
    out.push('[');
    out.push_str(&serde_json::to_string(id).unwrap_or_default());
    out.push(',');
    out.push_str(text);
    out.push(']');
}

/// Every record the model holds now (a cache written whole, and the tests' reference).
pub fn every_record(m: &Model) -> Vec<Rec> {
    let mut all = vec![Rec::Room, Rec::Members, Rec::Alerts, Rec::Newer];
    all.extend(m.cards.keys().cloned().map(Rec::Card));
    all.extend(m.sessions.keys().cloned().map(Rec::Session));
    all.extend(m.permissions.keys().cloned().map(Rec::Permission));
    all.extend(m.notes.keys().cloned().map(Rec::Note));
    all.extend(m.published.keys().cloned().map(Rec::Published));
    all.extend(m.human.keys().cloned().map(Rec::Register));
    for (id, t) in &m.timelines {
        all.push(Rec::Timeline(id.clone()));
        all.extend(t.items.keys().map(|k| Rec::Item(id.clone(), k.clone())));
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_tail_names_its_record_and_back() {
        for rec in [
            Rec::Room,
            Rec::Members,
            Rec::Card("ab".into()),
            Rec::Register("star/c1".into()),
            Rec::Register("odd%2F/".into()),
            Rec::Timeline("session_chat:s1".into()),
            Rec::Item("session_chat:s1".into(), ItemKey::Number(42)),
            Rec::Item("card_chat:c/1".into(), ItemKey::Local("l/1%".into())),
        ] {
            assert_eq!(Rec::of_tail(&rec.tail()), Some(rec.clone()), "{}", rec.tail());
        }
        assert_eq!(Rec::Register("star/c1".into()).tail(), "reg/star%2Fc1");
        for odd in ["", "card", "card/", "item/x", "item/x/q1", "item/x/nq", "what/x"] {
            assert_eq!(Rec::of_tail(odd), None, "{odd}");
        }
    }

    #[test]
    fn records_read_back_as_the_snapshot_they_were_written_from() {
        let mut m = Model::new();
        m.cards.insert("c1".into(), serde_json::from_value(json!({ "object_id": "c1", "title": "Ask", "object_version": 2 })).unwrap());
        m.human.insert("star/c1".into(), serde_json::from_value(json!({ "value": true, "envelope_number": 3 })).unwrap());
        m.members.insert("a1".into(), serde_json::from_value(json!({ "device_id": "a1", "device_role": "agent" })).unwrap());
        let mut t = crate::model::types::Timeline { timeline_key: "session_chat:s1".into(), item_count: 2, ..Default::default() };
        for it in [json!({ "envelope_number": 4, "content": "hi" }), json!({ "local_id": "l1", "pending": true })] {
            let o = it.as_object().unwrap().clone();
            t.items.insert(ItemKey::of(&o), o);
        }
        m.timelines.insert("session_chat:s1".into(), t);
        m.timelines.insert("card_chat:c1".into(), Default::default());
        let values: Vec<(Rec, Vec<u8>)> = every_record(&m).into_iter().filter_map(|r| Some((r.clone(), r.value(&m)?))).collect();
        let records: Vec<(Rec, &[u8])> = values.iter().map(|(r, v)| (r.clone(), v.as_slice())).collect();
        let text = snapshot_text(&records).expect("the records read");
        let mut back = Model::new();
        back.load_text(&text).unwrap();
        assert_eq!(back.snapshot(), m.snapshot());
        // (a value that is not JSON makes the whole cache unreadable)
        let mut broken = records.clone();
        broken[0].1 = b"{\"key_epoch\":";
        assert_eq!(snapshot_text(&broken), None);
    }
}
