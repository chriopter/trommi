//! One received envelope into the model: items, objects (cards, Notes, permission requests, Artifacts) and
//! registers, as the core took it (in the hub's order, or out of order: a page of a Chat, an object, the Desk).

use super::change::Change;
use super::received::*;
use super::types::*;
use super::{timeline_key, Model, Turn};
use crate::codec::{decode_body, BodyKind, Decoded};
use serde_json::{json, Map, Value};

/// Whether the envelope changed what the model shows, and the code of a finding it raised.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub applied: bool,
    pub refused: Option<String>,
}

impl Applied {
    fn yes() -> Self {
        Self {
            applied: true,
            refused: None,
        }
    }
    fn no() -> Self {
        Self::default()
    }
}

/// Codes under which an envelope took its place in its sender's chain with a body that cannot be read here: its
/// header still counts (9.2.1) and its place shows a placeholder.
fn unread_state(code: &str) -> Option<&'static str> {
    match code {
        "pruned" => Some("pruned"),
        "newer-version" => Some("newer_schema"),
        "no-key" | "decrypt-failed" | "bad-format" => Some("undecryptable"),
        _ => None,
    }
}
/// Findings a human is not told: an envelope met twice, a body the hub pruned, one whose key this device never held.
const QUIET: &[&str] = &["replay", "pruned", "no-key", "newer-version"];

const URGENCIES: &[&str] = &["low", "normal", "high", "critical"];

/// A card's level: the body's `urgency`, else the header's.
fn level_of(content: Option<&Obj>, header: &str) -> String {
    match content
        .and_then(|c| c.get("urgency"))
        .and_then(Value::as_str)
    {
        Some(u) if URGENCIES.contains(&u) => u.to_string(),
        _ => header.to_string(),
    }
}

/// A body as the model reads it, or why it is not read: (content, content_state).
fn body_of(r: &Received, kind: BodyKind) -> (Option<Obj>, String) {
    if matches!(r.outcome, Outcome::Applied | Outcome::Provisional) {
        if let Some(payload) = &r.payload {
            return match decode_body(kind, payload.as_bytes()) {
                Decoded::NewerSchema => (None, "newer_schema".into()),
                Decoded::Bad => (None, "undecryptable".into()),
                Decoded::Body(b) => (Some(b), "ok".into()),
            };
        }
    }
    (
        None,
        r.code
            .as_deref()
            .and_then(unread_state)
            .unwrap_or("header")
            .to_string(),
    )
}

fn is_zero_hash(h: Option<&str>) -> bool {
    h.is_none_or(|h| h.bytes().all(|b| b == b'0'))
}

/// When an answer was given: the body's `answered_at`, else the header's of before 11 October 2026, else its time.
fn answered_at(content: Option<&Obj>, header: u64, time: u64) -> u64 {
    match content
        .and_then(|c| c.get("answered_at"))
        .and_then(Value::as_f64)
    {
        Some(a) if a.fract() == 0.0 && a > 0.0 && a <= 9_007_199_254_740_991.0 => a as u64,
        _ if header != 0 => header,
        _ => time,
    }
}

/// A body field as an integer, if it is one.
fn int_of(content: Option<&Obj>, name: &str) -> Option<i64> {
    content
        .and_then(|c| c.get(name))
        .and_then(Value::as_f64)
        .filter(|n| n.fract() == 0.0)
        .map(|n| n as i64)
}

impl Model {
    fn finding(&mut self, change: &mut Change, r: &Received, now: u64, what: &str) -> Applied {
        let code = r
            .code
            .clone()
            .unwrap_or_else(|| outcome_name(r.outcome).to_string());
        if !QUIET.contains(&code.as_str()) {
            self.push_alert(
                change,
                &code,
                what,
                Some(r.change),
                Some(r.sender.clone()),
                "local",
                now,
            );
        }
        Applied {
            applied: false,
            refused: Some(code),
        }
    }

    /// One envelope as the core took it, and what stands again for register names whose value read out of the
    /// hub's order it dropped (spec 9.3.6). `now`: the clock, for alerts.
    pub fn apply_envelope(&mut self, r: &Received, change: &mut Change, now: u64) -> Applied {
        let applied = self.apply_one(r, change, now);
        self.apply_register_changes(r, change, now);
        applied
    }

    /// Apply only the verified register replacements in a report; its envelope fields provide deletion context.
    pub fn apply_register_changes(&mut self, r: &Received, change: &mut Change, now: u64) {
        for shown in &r.shown {
            // (a receipt's mark only rises as it comes; one that stood out of the hub's order and went is taken
            //  back, and the value that stands now sets it again)
            if let (Some(sid), "heard") = (&r.session_id, shown.name.as_str()) {
                let s = self.session_of(sid);
                s.heard_up_to = None;
                s.heard_at = None;
            }
            match &shown.now {
                Some(now_r) => {
                    self.apply_one(now_r, change, now);
                }
                None => {
                    // (the name has no value now: as if it were deleted, under the dropped envelope's place)
                    let payload =
                        serde_json::json!({ "name": shown.name, "value": null, "lamport": 0 });
                    let cleared = Received {
                        outcome: Outcome::Applied,
                        code: None,
                        kind: EnvelopeKind::Register,
                        register: Some(super::received::RegisterTaken {
                            name: shown.name.clone(),
                            current: true,
                        }),
                        payload: Some(payload.to_string()),
                        shown: Vec::new(),
                        ..r.clone()
                    };
                    self.apply_one(&cleared, change, now);
                }
            }
        }
    }

    fn apply_one(&mut self, r: &Received, change: &mut Change, now: u64) -> Applied {
        let local_id = self.by_hash.get(&r.hash).cloned();
        if r.outcome == Outcome::Refused {
            // (an epoch before this device came into the group, or an item that waits for a key: nothing it could
            // take, and no finding (16); the host reads that past again when its keys come)
            if matches!(r.code.as_deref(), Some("group-behind") | Some("no-key")) {
                return Applied {
                    applied: false,
                    refused: r.code.clone(),
                };
            }
            return self.finding(
                change,
                r,
                now,
                &format!(
                    "an envelope was refused ({})",
                    r.code.as_deref().unwrap_or("no reason")
                ),
            );
        }
        if r.outcome == Outcome::Void {
            if let Some(l) = &local_id {
                self.rollback_echo(l, change);
            }
            return self.finding(
                change,
                r,
                now,
                &format!(
                    "the hub holds envelope #{} of this sender as void ({})",
                    r.seq,
                    r.code.as_deref().unwrap_or("no reason")
                ),
            );
        }
        let ordered = r.outcome != Outcome::Provisional;
        if r.outcome == Outcome::Chained && r.code.as_deref().and_then(unread_state).is_none() {
            if let Some(l) = &local_id {
                self.rollback_echo(l, change);
            }
            return self.finding(
                change,
                r,
                now,
                &format!(
                    "envelope #{} of this sender does not count ({})",
                    r.seq,
                    r.code.as_deref().unwrap_or("no reason")
                ),
            );
        }
        if r.code.as_deref() == Some("newer-version") {
            self.note_newer(change, "body format", Some(r.change));
        }
        if ordered {
            if let Some(sid) = &r.session_id {
                let s = self.session_of(sid);
                s.last_activity_at = s.last_activity_at.max(r.time);
                change.sessions.insert(sid.clone());
            }
        }
        match r.kind {
            EnvelopeKind::Item => self.apply_item(r, change, local_id, ordered, false),
            EnvelopeKind::Register => self.apply_register(r, change, now, local_id, ordered),
            EnvelopeKind::Reserved => {
                self.note_newer(change, "envelope kind", Some(r.change));
                Applied {
                    applied: false,
                    refused: Some("needs-update".into()),
                }
            }
            _ => self.apply_object(r, change, now, local_id, ordered),
        }
    }
}

fn outcome_name(o: Outcome) -> &'static str {
    match o {
        Outcome::Applied => "applied",
        Outcome::Chained => "chained",
        Outcome::Void => "void",
        Outcome::Provisional => "provisional",
        Outcome::Refused => "refused",
    }
}

/// A board item's content types (10.5).
pub const BOARD_CONTENT_TYPES: &[&str] = &["strokes", "erase", "move", "send_away"];

/// A timeline item's content type this version knows (an item without one counts as a message).
pub fn content_type_known(content: &Obj) -> bool {
    match content.get("content_type") {
        None | Some(Value::Null) => true,
        Some(Value::String(t)) => {
            t == "message" || t == "stroke_piece" || BOARD_CONTENT_TYPES.contains(&t.as_str())
        }
        Some(_) => false,
    }
}

/// A timeline item's state from its body and content_state.
pub fn item_state_of(content: Option<&Obj>, content_state: &str) -> &'static str {
    match content {
        Some(c) => {
            if content_state != "ok" {
                match content_state {
                    "pruned" => "pruned",
                    "undecryptable" => "undecryptable",
                    "newer_schema" => "newer_schema",
                    "header" => "header",
                    "loading" => "loading",
                    _ => "loaded",
                }
            } else if content_type_known(c) {
                "loaded"
            } else {
                "unsupported"
            }
        }
        None => match content_state {
            "pruned" => "pruned",
            "undecryptable" => "undecryptable",
            "newer_schema" => "newer_schema",
            _ => "header",
        },
    }
}

impl Model {
    /// A timeline, made if it is new.
    pub fn timeline_of(&mut self, key: &str) -> &mut Timeline {
        let open = self.windows_open;
        self.timelines.entry(key.to_string()).or_insert_with(|| {
            let (kind, id) = key.split_once(':').unwrap_or((key, ""));
            let scope_id = id.split_once('/').map(|(_, s)| s).unwrap_or(id);
            Timeline {
                timeline_key: key.to_string(),
                timeline_kind: kind.to_string(),
                timeline_id: id.to_string(),
                object_id: scope_id.to_string(),
                item_count: 0,
                newest_envelope_number: 0,
                items: Default::default(),
                loaded_down_to: None,
                has_more: false,
                window_open: open,
            }
        })
    }
}

impl Model {
    /// A Chat message or a board item. `deeds_only`: only what it does to its card (a page of card Chats read
    /// out of the hub's order: `apply_card_deeds`), nothing of the timelines.
    pub(crate) fn apply_item(
        &mut self,
        r: &Received,
        change: &mut Change,
        local_id: Option<String>,
        ordered: bool,
        deeds_only: bool,
    ) -> Applied {
        let Some(tl) = &r.timeline else {
            return Applied::no();
        };
        let board = tl.kind == TimelineKind::Board;
        let on_card = tl.kind == TimelineKind::CardChat;
        let key = if board {
            timeline_key("scribble", &format!("desk/{}", tl.id))
        } else {
            timeline_key(
                "chat",
                &format!("{}/{}", if on_card { "card" } else { "session" }, tl.id),
            )
        };
        let sender = r.sender.clone();
        let recipient = r.recipient.clone();
        let (content, state) = body_of(
            r,
            if board {
                BodyKind::BoardItem
            } else {
                BodyKind::Message
            },
        );
        if !deeds_only {
            self.put_item(
                r,
                change,
                &key,
                &local_id,
                ordered,
                board,
                content.as_ref(),
                &state,
            );
        }
        if !ordered {
            return Applied::yes();
        }
        if board {
            if !deeds_only {
                // The finished form of a stroke replaces what was shown of its pieces.
                if content
                    .as_ref()
                    .and_then(|c| c.get("content_type"))
                    .and_then(Value::as_str)
                    == Some("strokes")
                {
                    let lives: Vec<String> = content
                        .as_ref()
                        .and_then(|c| c.get("strokes"))
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|e| e.get("live"))
                        .filter(|l| truthy(l))
                        .map(js_text)
                        .collect();
                    let t = self.timeline_of(&key);
                    for l in lives {
                        t.items
                            .remove(&ItemKey::Local(format!("live:{sender}/{l}")));
                    }
                }
            }
            return Applied::yes();
        }
        let human = self.is_human(&sender, recipient.as_deref());
        if on_card {
            if let Some(card) = self.cards.get_mut(&tl.id) {
                // In revision until the agent's next version or a message with present_card. Only an open card goes
                // back to its agent; an own answer still in flight does not count yet. The newest of these deeds
                // decides, by change number, not by the order they are read in.
                let flag = |n: &str| content.as_ref().and_then(|c| c.get(n)).is_some_and(truthy);
                if flag("present_card") {
                    card.presented_envelope_number =
                        Some(card.presented_envelope_number.unwrap_or(0).max(r.change));
                    if card
                        .in_revision
                        .as_ref()
                        .is_some_and(|ir| ir.envelope_number < r.change)
                    {
                        card.in_revision = None;
                    }
                } else if human
                    && content.is_some()
                    && (flag("hand_back") || flag("explain"))
                    && r.change > handed_back_after(card)
                    && (card.object_state == "open"
                        || card.answer.as_ref().is_some_and(|a| a.pending))
                {
                    card.in_revision = Some(InRevision {
                        by: if flag("hand_back") {
                            "hand_back".into()
                        } else {
                            "explain".into()
                        },
                        envelope_number: r.change,
                    });
                }
                change.cards.insert(card.object_id.clone());
                if let Some(s) = &card.session_id {
                    change.sessions.insert(s.clone());
                }
            }
        } else if !deeds_only {
            change.sessions.insert(tl.id.clone());
            // The agent's final text of a turn ends the trail of what it did.
            if !human
                && content
                    .as_ref()
                    .and_then(|c| c.get("terminal"))
                    .and_then(Value::as_str)
                    == Some("answer")
            {
                let k = key.clone();
                self.end_turns(
                    |t| t.timeline_key == k && t.sender == sender,
                    r.time,
                    change,
                );
            }
        }
        Applied::yes()
    }
}

/// A hand back counts only when it is newer than all of this: the card's newest version, the newest present, the hand
/// back in force.
fn handed_back_after(card: &Card) -> u64 {
    card.envelope_number
        .max(card.presented_envelope_number.unwrap_or(0))
        .max(card.in_revision.as_ref().map_or(0, |r| r.envelope_number))
}

/// JavaScript's truthiness of a JSON value.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// `${v}` of JavaScript, for an id-like value.
fn js_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Model {
    /// An item into its timeline's window: what has a body, what an opened timeline counts, an own echo's place
    /// (replaced in place); a board's window holds every item.
    #[allow(clippy::too_many_arguments)]
    fn put_item(
        &mut self,
        r: &Received,
        change: &mut Change,
        key: &str,
        local_id: &Option<String>,
        ordered: bool,
        board: bool,
        content: Option<&Obj>,
        state: &str,
    ) {
        let item_state = item_state_of(content, state);
        if item_state == "unsupported" {
            let what = format!(
                "content_type {}",
                content
                    .and_then(|c| c.get("content_type"))
                    .map(js_text)
                    .unwrap_or_default()
            );
            self.note_newer(change, &what, Some(r.change));
        }
        if let Some(l) = local_id {
            self.echoes.remove(l);
            self.by_hash.remove(&r.hash);
        }
        let t = self.timeline_of(key);
        if ordered && r.change > t.newest_envelope_number {
            t.item_count += 1;
            t.newest_envelope_number = r.change;
        }
        let mut item = Map::new();
        item.insert("envelope_number".into(), json!(r.change));
        item.insert("local_id".into(), json!(local_id));
        item.insert("pending".into(), json!(false));
        item.insert("envelope_hash".into(), json!(r.hash));
        item.insert("sender_device_id".into(), json!(r.sender));
        item.insert("sender_sequence".into(), json!(r.seq));
        item.insert("recipient_device_id".into(), json!(r.recipient));
        item.insert("sent_at".into(), json!(r.time));
        item.insert("item_state".into(), json!(item_state));
        item.insert(
            "content_type".into(),
            content
                .and_then(|c| c.get("content_type"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        item.insert(
            "content".into(),
            content
                .map(|c| Value::Object(c.clone()))
                .unwrap_or(Value::Null),
        );
        if !ordered {
            item.insert("provisional".into(), json!(true));
        }
        let at = ItemKey::Number(r.change);
        let had = t.items.get(&at).cloned();
        if let Some(l) = local_id {
            t.items.remove(&ItemKey::Local(l.clone()));
        }
        let had_full = had.as_ref().is_some_and(|h| {
            !h.get("provisional").is_some_and(truthy)
                && h.get("content").is_some_and(|c| !c.is_null())
        });
        if (content.is_some() || t.window_open || had.is_some() || local_id.is_some() || board)
            && !(had_full && !ordered)
        {
            t.items.insert(at, item.clone());
            change.add_item(key, &item);
        }
        change.timelines.insert(key.to_string());
    }

    /// End running turns: their last step says so.
    pub fn end_turns(&mut self, which: impl Fn(&Turn) -> bool, at: u64, change: &mut Change) {
        let ended: Vec<String> = self
            .turns
            .iter()
            .filter(|(_, t)| which(t))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ended {
            let Some(turn) = self.turns.remove(&id) else {
                continue;
            };
            let Some(t) = self.timelines.get_mut(&turn.timeline_key) else {
                continue;
            };
            let Some(item) = t.items.get_mut(&ItemKey::Number(turn.last)) else {
                continue;
            };
            let Some(content) = item.get_mut("content").and_then(Value::as_object_mut) else {
                continue;
            };
            let Some(work) = content.get_mut("work").and_then(Value::as_object_mut) else {
                continue;
            };
            work.insert("state".into(), json!("done"));
            work.insert("ended_at".into(), json!(at.max(turn.started_at)));
            let snapshot = item.clone();
            change.add_item(&turn.timeline_key, &snapshot);
            change.timelines.insert(turn.timeline_key.clone());
        }
    }
}

/// One step of a turn's trail in the form the views fold (the web app's `work.ts` `stepEnvelope`).
pub fn step_envelope(
    turn: &str,
    number: u64,
    time: u64,
    step: &str,
    started_at: u64,
) -> Option<Value> {
    let s: Value = serde_json::from_str(step).ok()?;
    let s = s.as_object()?;
    if !(1..=9_007_199_254_740_991).contains(&number) {
        return None;
    }
    let text = s.get("text")?.as_str()?;
    if text.len() > 30_000 {
        return None;
    }
    let tool = match s.get("tool") {
        None => None,
        Some(Value::String(t)) if !t.is_empty() && t.len() <= 80 => Some(t.clone()),
        Some(_) => return None,
    };
    let id = number.to_string();
    let item = match tool {
        Some(tool) => {
            json!({ "id": id, "kind": "step", "tool": tool, "title": text, "state": "ok", "at": time })
        }
        None => json!({ "id": id, "kind": "text", "text": text, "at": time }),
    };
    Some(
        json!({ "turn": turn, "seq": number, "state": "running", "started_at": started_at, "items": [item] }),
    )
}

impl Model {
    /// One step of a running turn, `change_number` the hub's change number of its log entry: an item of its own in the
    /// session's Chat under that number (never counted: no stored content). False for a step that is not readable.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_work_trail(
        &mut self,
        session_id: &str,
        from: &str,
        turn_id: &str,
        number: u64,
        time: u64,
        payload: &[u8],
        change_number: u64,
        change: &mut Change,
    ) -> bool {
        let key = timeline_key("chat", &format!("session/{session_id}"));
        let mut started = self
            .turns
            .get(turn_id)
            .map(|t| t.started_at)
            .unwrap_or(time);
        if !self.turns.contains_key(turn_id) {
            let t = self.timeline_of(&key);
            for it in t.items.values() {
                if let Some(w) = it.get("content").and_then(|c| c.get("work")) {
                    if w.get("turn").and_then(Value::as_str) == Some(turn_id) {
                        if let Some(s) = w.get("started_at").and_then(Value::as_u64) {
                            started = started.min(s);
                        }
                    }
                }
            }
        }
        let text = std::str::from_utf8(payload).unwrap_or("");
        let Some(env) = step_envelope(turn_id, number, time, text, started) else {
            return false;
        };
        if !self.turns.contains_key(turn_id) {
            let k = key.clone();
            let sender = from.to_string();
            self.end_turns(|t| t.sender == sender && t.timeline_key == k, time, change);
            self.turns.insert(
                turn_id.to_string(),
                Turn {
                    timeline_key: key.clone(),
                    session_id: session_id.to_string(),
                    sender: from.to_string(),
                    started_at: started,
                    last: change_number,
                },
            );
        }
        if let Some(turn) = self.turns.get_mut(turn_id) {
            turn.last = turn.last.max(change_number);
        }
        let item = json!({ "envelope_number": change_number, "local_id": null, "pending": false, "envelope_hash": null, "sender_device_id": from,
            "sender_sequence": null, "recipient_device_id": null, "sent_at": time, "item_state": "loaded", "content_type": "message",
            "content": { "content_type": "message", "terminal": "work", "work": env } });
        let item = item.as_object().cloned().unwrap_or_default();
        self.timeline_of(&key)
            .items
            .insert(ItemKey::Number(change_number), item.clone());
        change.add_item(&key, &item);
        change.timelines.insert(key);
        let s = self.session_of(session_id);
        s.last_activity_at = s.last_activity_at.max(time);
        change.sessions.insert(session_id.to_string());
        true
    }

    /// The points of a stroke someone is still drawing: an item of the board's window under `live:<sender>/<stroke>`,
    /// never counted and never stored; each piece replaces the one before it.
    pub fn apply_stroke_piece(
        &mut self,
        from: &str,
        board: &str,
        payload: &[u8],
        now: u64,
        change: &mut Change,
    ) -> bool {
        let Some(content) = crate::codec::decode_piece(payload) else {
            return false;
        };
        let key = timeline_key("scribble", &format!("desk/{board}"));
        let stroke = content.get("stroke").map(js_text).unwrap_or_default();
        let id = format!("live:{from}/{stroke}");
        let prefix = format!("live:{from}/");
        let t = self.timeline_of(&key);
        t.items
            .retain(|k, _| !matches!(k, ItemKey::Local(l) if l != &id && l.starts_with(&prefix)));
        let item = json!({ "envelope_number": null, "local_id": id, "pending": false, "envelope_hash": null, "sender_device_id": from,
            "sender_sequence": null, "recipient_device_id": null, "sent_at": now, "item_state": "loaded", "content_type": "stroke_piece", "content": content });
        let item = item.as_object().cloned().unwrap_or_default();
        t.items.insert(ItemKey::Local(id), item.clone());
        change.add_item(&key, &item);
        change.timelines.insert(key);
        true
    }
}

/// The state the envelope leaves its object in: the core's replay, else (out of order) the header's own word.
fn state_after(r: &Received) -> String {
    r.object_after
        .as_ref()
        .map(|o| o.object_state.clone())
        .or_else(|| r.object.as_ref().map(|o| o.object_state.clone()))
        .unwrap_or_else(|| "open".into())
}

fn default_of(f: &str) -> Value {
    match f {
        "options" | "attachments" => json!([]),
        "allows_multiple" => json!(false),
        "card_type" => json!("decision"),
        "title" => json!(""),
        _ => Value::Null,
    }
}

fn new_card(object_id: &str, owner: &str, r: &Received) -> Card {
    let mut fields = CardFields::default();
    for f in CARD_CONTENT_FIELDS {
        fields.insert(f, default_of(f));
    }
    Card {
        object_id: object_id.to_string(),
        catching_up: false,
        agent_device_id: owner.to_string(),
        session_id: r.session_id.clone(),
        object_state: "open".into(),
        urgency: "normal".into(),
        fields,
        object_version: 0,
        version_hash: None,
        envelope_number: 0,
        first_envelope_number: r.change,
        created_at: r.time,
        updated_at: r.time,
        versions: Vec::new(),
        history: Vec::new(),
        answer: None,
        answers: Vec::new(),
        closed_how: None,
        in_revision: None,
        presented_envelope_number: None,
        timeline_key: timeline_key("chat", &format!("card/{object_id}")),
        content_state: "header".into(),
        unsupported: None,
        state_envelope_number: 0,
        ..Default::default()
    }
}

/// How a card that its current version closed was closed.
fn version_closed_how(card: &Card) -> String {
    let action = card.answer.as_ref().map(|a| a.answer_action.as_str());
    if truthy(&card.field("merged_into_object_id")) {
        "merged"
    } else if truthy(&card.field("withdraw_reason")) {
        "withdrawn"
    } else if action == Some("read") {
        "read"
    } else if action == Some("shred") {
        "shredded"
    } else {
        "closed"
    }
    .into()
}

/// How an answer left its card.
fn answer_closed_how(card: &Card, a: &Answer) -> String {
    match a.answer_action.as_str() {
        "read" => "read".into(),
        "shred" => "shredded".into(),
        _ if card.object_state == "closed" => {
            if a.extra.get("unsupported").is_some_and(truthy)
                || a.extra.get("no_body").is_some_and(truthy)
            {
                "closed".into()
            } else {
                "settled".into()
            }
        }
        _ => "answered".into(),
    }
}

/// Put a record into a list kept ascending by envelope_number (none last).
fn insert_by_number<T>(list: &mut Vec<T>, x: T, number: impl Fn(&T) -> Option<u64>) -> usize {
    let key = |n: Option<u64>| n.map_or(f64::INFINITY, |n| n as f64);
    let k = key(number(&x));
    let mut at = list.len();
    while at > 0 && key(number(&list[at - 1])) > k {
        at -= 1;
    }
    list.insert(at, x);
    at
}

impl Model {
    fn apply_object(
        &mut self,
        r: &Received,
        change: &mut Change,
        now: u64,
        local_id: Option<String>,
        ordered: bool,
    ) -> Applied {
        let Some(o) = &r.object else {
            return Applied::no();
        };
        // In the hub's order only what the core's replay of headers counted moves an object (9.2.1).
        if ordered && r.object_after.is_none() {
            if let Some(l) = &local_id {
                self.rollback_echo(l, change);
            }
            return self.finding(
                change,
                r,
                now,
                &format!(
                    "envelope #{} of this sender did not change its object",
                    r.seq
                ),
            );
        }
        match o.object_type.as_str() {
            "card" => self.apply_card(r, change, local_id, ordered),
            "note" => self.apply_note(r, change, local_id),
            "request" => self.apply_request(r, change),
            "artifact" => self.apply_artifact(r, change),
            other => {
                self.note_newer(change, &format!("object type {other}"), Some(r.change));
                Applied {
                    applied: false,
                    refused: Some("needs-update".into()),
                }
            }
        }
    }

    fn card_touched(&self, id: &str, change: &mut Change) {
        change.cards.insert(id.to_string());
        if let Some(s) = self.cards.get(id).and_then(|c| c.session_id.clone()) {
            change.sessions.insert(s);
        }
        change.stack = true;
    }
}

impl Model {
    fn apply_card_version(&mut self, r: &Received, change: &mut Change, ordered: bool) -> Applied {
        let o = r.object.as_ref().expect("an object");
        let mut n = r.change;
        let object_id = o.object_id.clone();
        let state = state_after(r);
        let (content, content_state) = body_of(r, BodyKind::Card);
        if !self.cards.contains_key(&object_id) {
            let owner = r
                .object_after
                .as_ref()
                .map(|a| a.owner.clone())
                .unwrap_or_else(|| r.sender.clone());
            let card = new_card(&object_id, &owner, r);
            let (sid, first) = (card.session_id.clone(), card.first_envelope_number);
            self.cards.insert(object_id.clone(), card);
            if let Some(sid) = sid {
                // (kept sorted by the change each card was first filed at)
                let cards = &self.cards;
                let s = self
                    .sessions
                    .entry(sid.clone())
                    .or_insert_with(|| placeholder_session(&sid));
                let mut at = s.card_ids.len();
                while at > 0
                    && cards
                        .get(&s.card_ids[at - 1])
                        .map_or(0, |c| c.first_envelope_number)
                        > first
                {
                    at -= 1;
                }
                s.card_ids.insert(at, object_id.clone());
            }
        }
        let mut newer_type: Option<String> = None;
        // (the change the card was first filed at orders every list: whatever moves it below, the lists are
        //  worked out again)
        let first_before = self.cards[&object_id].first_envelope_number;
        {
            let card = self.cards.get_mut(&object_id).expect("the card");
            if let Some(a) = &r.object_after {
                card.agent_device_id = a.owner.clone();
            }
            // A version the card holds already has its number. Met again out of the hub's order it keeps it,
            // whatever number it comes under (nothing vouches for that one); taken in the hub's order it gets the
            // place the chain gives it, and what hung on the number it was first shown under moves with it.
            // (`object_after` comes only with an envelope the chain holds, and the core hands such an envelope
            //  over under the number of its place there, also when it is read back out of order.)
            if let Some(at) = card.versions.iter().position(|v| v.version_hash == r.hash) {
                let held = card.versions[at].envelope_number;
                if r.object_after.is_none() {
                    n = held;
                } else if held != n {
                    card.versions[at].envelope_number = n;
                    card.versions.sort_by_key(|v| v.envelope_number);
                    card.renumbered(held, n);
                }
            }
            let first = is_zero_hash(o.object_ref.as_deref());
            // Its first version says where the card stands and when it was asked: its number takes the place of
            // the one the card stood by until now (a later version's, or the hub's row, project.rs
            // `first_from_desk`). The time is shown with the card; no list goes by it.
            if first {
                card.created_at = r.time;
                card.first_envelope_number = n;
            }
            let v_at = match card.versions.iter().position(|v| v.version_hash == r.hash) {
                Some(at) => {
                    let v = &mut card.versions[at];
                    v.object_state = o.object_state.clone();
                    if let (Some(c), None) = (&content, &v.content) {
                        v.content = Some(RawObj::of(c));
                        v.urgency = level_of(Some(c), &v.urgency.clone());
                        if let Some(ov) = int_of(Some(c), "object_version") {
                            v.object_version = ov;
                        }
                    }
                    at
                }
                None => {
                    let v = CardVersion {
                        object_version: 0,
                        version_hash: r.hash.clone(),
                        previous_version_hash: o
                            .object_ref
                            .clone()
                            .filter(|h| !is_zero_hash(Some(h))),
                        envelope_number: n,
                        sent_at: r.time,
                        object_state: o.object_state.clone(),
                        urgency: level_of(content.as_ref(), &o.urgency),
                        content: content.as_ref().map(RawObj::of),
                    };
                    let at = insert_by_number(&mut card.versions, v, |v| Some(v.envelope_number));
                    card.versions[at].object_version =
                        int_of(content.as_ref(), "object_version").unwrap_or(at as i64 + 1);
                    at
                }
            };
            card.history = history_of(&card.versions);
            if n >= card.envelope_number {
                // the newest version this device knows: the card's own fields
                let v = card.versions[v_at].clone();
                card.envelope_number = n;
                card.version_hash = Some(r.hash.clone());
                card.object_version = v.object_version;
                card.urgency = v.urgency.clone();
                if let Some(vc) = v.content.as_ref().map(RawObj::obj) {
                    for f in CARD_CONTENT_FIELDS {
                        let val = vc
                            .get(*f)
                            .filter(|x| !x.is_null())
                            .cloned()
                            .unwrap_or_else(|| default_of(f));
                        card.fields.insert(f, val);
                    }
                    // (a proposal for the team's knowledge: drawn from what it asks, never from its agent's words)
                    crate::knowledge::dress(&mut card.fields, v.previous_version_hash.is_none());
                    card.content_state = "ok".into();
                } else {
                    card.content_state = content_state.clone();
                }
                let card_type = card.field("card_type").as_str().map(str::to_string);
                card.unsupported = Some(if card.content_state == "newer_schema" {
                    json!("newer_schema")
                } else if card.content_state == "ok"
                    && !card_type
                        .as_deref()
                        .is_some_and(|t| CARD_TYPES.contains(&t))
                {
                    newer_type = Some(
                        card.field("card_type")
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| js_text(&card.field("card_type"))),
                    );
                    json!("card_type")
                } else {
                    Value::Null
                });
                // a new version from the agent ends "in revision" (one read again does not end a later hand back)
                if ordered
                    && card
                        .in_revision
                        .as_ref()
                        .is_some_and(|ir| ir.envelope_number < n)
                {
                    card.in_revision = None;
                }
            }
            if n >= card.state_envelope_number {
                let was_open = card.object_state == "open";
                card.state_envelope_number = n;
                card.object_state = state.clone();
                card.updated_at = r.time;
                if state == "open" {
                    card.closed_how = None;
                    if !was_open && card.answer.as_ref().is_some_and(|a| !a.pending) {
                        card.answer = None;
                    }
                } else {
                    card.closed_how = Some(if state == "closed" {
                        version_closed_how(card)
                    } else {
                        "answered".into()
                    });
                }
            } else if card.version_hash.as_deref() == Some(r.hash.as_str())
                && card.object_state == "closed"
                && card.closed_how.as_deref() == Some("closed")
            {
                // its body came after its header
                card.closed_how = Some(version_closed_how(card));
            }
        }
        if let Some(t) = newer_type {
            self.note_newer(change, &format!("card_type {t}"), Some(n));
        }
        if self.cards[&object_id].first_envelope_number != first_before {
            self.desk_rebuild = true;
        }
        self.card_touched(&object_id, change);
        Applied::yes()
    }
}

impl Card {
    /// An envelope of this card that was first met out of the hub's order under the number `held` took its place
    /// in that order at `n`: what the card dated by `held` is dated by `n`.
    fn renumbered(&mut self, held: u64, n: u64) {
        for number in [
            &mut self.envelope_number,
            &mut self.state_envelope_number,
            &mut self.first_envelope_number,
        ] {
            if *number == held {
                *number = n;
            }
        }
    }

    /// The card holds its first version: the change it was first filed at is that version's, not the hub's word.
    pub(super) fn holds_first_version(&self) -> bool {
        self.versions
            .iter()
            .any(|v| is_zero_hash(v.previous_version_hash.as_deref()))
    }
}

/// A session record before its group is known (what `session_of` makes).
fn placeholder_session(session_id: &str) -> Session {
    let mut m = Model::default();
    m.session_of(session_id).clone()
}

impl Model {
    fn apply_card(
        &mut self,
        r: &Received,
        change: &mut Change,
        local_id: Option<String>,
        ordered: bool,
    ) -> Applied {
        if r.kind == EnvelopeKind::Version {
            return self.apply_card_version(r, change, ordered);
        }
        let o = r.object.as_ref().expect("an object");
        let object_id = o.object_id.clone();
        // (the number of an answer or a take back the card holds already: as for a version, `apply_card_version`)
        let mut n = r.change;
        let in_order = r.object_after.is_some();
        // an answer or a take back of a card this device holds no version of
        if !self.cards.contains_key(&object_id) {
            return Applied::no();
        }
        // (a renumbering below may move the change the card was first filed at: the lists go by it)
        let first_before = self.cards[&object_id].first_envelope_number;
        let state = state_after(r);
        match r.kind {
            EnvelopeKind::Answer => {
                // The hub's copy of an own answer: the echo goes first, the real one is put in as it counted.
                if let Some(l) = &local_id {
                    self.rollback_echo(l, change);
                }
                let (content, content_state) = body_of(r, BodyKind::Answer);
                let c = content.as_ref();
                let (bind_hash, bind_choices) = match &r.bind {
                    Some(Bind::Answer {
                        version_hash,
                        choices,
                    }) => (Some(version_hash.clone()), Some(choices.clone())),
                    _ => (None, None),
                };
                let bound = match &bind_hash {
                    Some(Some(h)) => Some(h.clone()),
                    _ => o.object_ref.clone().filter(|h| !is_zero_hash(Some(h))),
                };
                let action = c
                    .and_then(|c| c.get("answer_action"))
                    .and_then(Value::as_str)
                    .unwrap_or("answer")
                    .to_string();
                let newer =
                    content_state == "newer_schema" || !ANSWER_ACTIONS.contains(&action.as_str());
                if content.is_some() && !ANSWER_ACTIONS.contains(&action.as_str()) {
                    self.note_newer(change, &format!("answer_action {action}"), Some(n));
                }
                let get = |k: &str| c.and_then(|c| c.get(k)).filter(|v| !v.is_null()).cloned();
                let choices = get("choices")
                    .or(bind_choices.map(|cs| json!(cs)))
                    .unwrap_or(json!([]));
                let note = get("note").unwrap_or(Value::Null);
                let option_notes = get("option_notes").unwrap_or(json!({}));
                let attachments = get("attachments").unwrap_or(json!([]));
                let marks = get("marks").unwrap_or(json!([]));
                let trusted = c.and_then(|c| c.get("trusted")).is_some_and(truthy);
                let card = self.cards.get_mut(&object_id).expect("the card");
                let at = match card
                    .answers
                    .iter()
                    .position(|a| a.envelope_hash.as_deref() == Some(r.hash.as_str()))
                {
                    Some(mut at) => {
                        match card.answers[at].envelope_number {
                            Some(held) if !in_order => n = held,
                            Some(held) if held != n => {
                                card.answers[at].envelope_number = Some(n);
                                card.answers
                                    .sort_by_key(|a| a.envelope_number.unwrap_or(u64::MAX));
                                card.renumbered(held, n);
                                at = card
                                    .answers
                                    .iter()
                                    .position(|a| a.envelope_hash.as_deref() == Some(r.hash.as_str()))
                                    .expect("the answer");
                            }
                            _ => {}
                        }
                        let a = &mut card.answers[at];
                        if content.is_some() && a.extra.contains_key("no_body") {
                            a.answer_action = action.clone();
                            a.choices = choices;
                            a.note = note;
                            a.option_notes = option_notes;
                            a.attachments = attachments;
                            a.marks = marks;
                            a.trusted = trusted;
                            a.extra.remove("no_body");
                        }
                        at
                    }
                    None => {
                        let bound_object_version = card
                            .versions
                            .iter()
                            .find(|v| Some(&v.version_hash) == bound.as_ref())
                            .map(|v| v.object_version)
                            .unwrap_or(card.object_version);
                        let mut extra = Map::new();
                        if newer {
                            extra.insert("unsupported".into(), json!(true));
                        }
                        if content.is_none() {
                            extra.insert("no_body".into(), json!(true));
                        }
                        let a = Answer {
                            answer_action: action.clone(),
                            choices,
                            note,
                            option_notes,
                            attachments,
                            marks,
                            trusted,
                            bound_version_hash: bound.clone(),
                            bound_object_version,
                            envelope_number: Some(n),
                            envelope_hash: Some(r.hash.clone()),
                            by_device_id: r.sender.clone(),
                            answered_at: answered_at(c, o.answered_at, r.time),
                            taken_back_at: None,
                            taken_back_sent_at: None,
                            pending: false,
                            extra,
                        };
                        insert_by_number(&mut card.answers, a, |a| a.envelope_number)
                    }
                };
                let a = card.answers[at].clone();
                if n >= card.state_envelope_number && a.taken_back_at.is_none() {
                    card.state_envelope_number = n;
                    card.object_state = if state == "open" {
                        "answered".into()
                    } else {
                        state.clone()
                    };
                    card.closed_how = Some(answer_closed_how(card, &a));
                    card.answer = Some(a);
                    card.updated_at = r.time;
                    if ordered
                        && card
                            .in_revision
                            .as_ref()
                            .is_some_and(|ir| ir.envelope_number < n)
                    {
                        card.in_revision = None;
                    }
                } else if card.answer.is_none()
                    && card.object_state == "closed"
                    && a.taken_back_at.is_none()
                    && card.versions.last().is_some_and(|v| {
                        v.object_state == "closed"
                            && v.previous_version_hash.is_some()
                            && v.previous_version_hash == a.bound_version_hash
                            && n < v.envelope_number
                    })
                    && card.answers.iter().all(|other| other.envelope_number <= a.envelope_number)
                {
                    // A settled card's closing version may be read before its answer. Restore the answer
                    // bound to the version it closed, without moving the newer state backwards.
                    card.answer = Some(a);
                } else if card
                    .answer
                    .as_ref()
                    .is_some_and(|x| x.envelope_hash == a.envelope_hash && !x.pending)
                {
                    card.answer = Some(a.clone());
                    card.closed_how = Some(answer_closed_how(card, &a));
                }
            }
            EnvelopeKind::TakeBack => {
                // The bind names the answer taken back; where only the header is left, it was the one in force before.
                let previous = match &r.bind {
                    Some(Bind::TakeBack {
                        previous_hash: Some(p),
                    }) => Some(p.clone()),
                    _ => None,
                };
                let card = self.cards.get_mut(&object_id).expect("the card");
                let at = match &previous {
                    Some(p) => card
                        .answers
                        .iter()
                        .position(|x| x.envelope_hash.as_deref() == Some(p.as_str())),
                    // (in the hub's order also an answer whose take back was first shown out of order, under a
                    //  number beyond this one: it is this take back, an answer is taken back once)
                    None => card.answers.iter().rposition(|x| {
                        x.envelope_number.unwrap_or(0) < n
                            && x.taken_back_at.is_none_or(|at| in_order && at > n)
                    }),
                };
                // Out of the hub's order a take back counts only with the answer it names: without one (a header
                // alone) it would take back whichever answer stands, under a number nothing vouches for.
                if !in_order && previous.is_none() {
                    return Applied::no();
                }
                if let Some(at) = at {
                    // (taken back already: this take back has its number, as a version has)
                    match card.answers[at].taken_back_at {
                        Some(held) if !in_order => n = held,
                        Some(held) if held != n => {
                            card.answers[at].taken_back_at = Some(n);
                            card.renumbered(held, n);
                        }
                        _ => {}
                    }
                    let a = &mut card.answers[at];
                    if a.taken_back_at.is_none() {
                        a.taken_back_at = Some(n);
                        a.taken_back_sent_at = Some(r.time);
                        let hash = a.envelope_hash.clone();
                        let copy = a.clone();
                        if let Some(cur) = card.answer.as_mut() {
                            if cur.envelope_hash == hash && !cur.pending {
                                *cur = copy;
                            }
                        }
                    }
                }
                if n >= card.state_envelope_number {
                    card.state_envelope_number = n;
                    if !card.answer.as_ref().is_some_and(|a| a.pending) {
                        card.answer = None;
                    }
                    card.object_state = state.clone();
                    card.closed_how = None;
                    card.updated_at = r.time;
                }
            }
            _ => return Applied::no(),
        }
        if self.cards[&object_id].first_envelope_number != first_before {
            self.desk_rebuild = true;
        }
        self.card_touched(&object_id, change);
        Applied::yes()
    }
}

fn causal_of(r: &Received, c: Option<&Obj>) -> Causal {
    Causal {
        sender_device_id: r.sender.clone(),
        sender_sequence: r.seq,
        sent_at: r.time,
        lamport: c
            .and_then(|c| c.get("lamport"))
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    }
}

/// A Note as the model holds it, from one version: the app's fields beside the core's.
fn note_of(
    object_id: &str,
    r: &Received,
    c: Option<&Obj>,
    content_state: &str,
    old: Option<&Obj>,
) -> Obj {
    let mut out = Map::new();
    if c.is_none() {
        // a version that cannot be read keeps what was shown
        for (k, v) in old.into_iter().flatten() {
            if !["pending", "local_id", "_base"].contains(&k.as_str()) {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    for (k, v) in c.into_iter().flatten() {
        if ![
            "schema_version",
            "object_version",
            "previous_version_hash",
            "lamport",
        ]
        .contains(&k.as_str())
        {
            out.insert(k.clone(), v.clone());
        }
    }
    let old_text = old
        .and_then(|o| o.get("text"))
        .cloned()
        .unwrap_or(json!(""));
    let text = c
        .and_then(|c| c.get("text"))
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(old_text);
    let old_version = old
        .and_then(|o| o.get("object_version"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let mut hashes: Vec<Value> = old
        .and_then(|o| o.get("version_hashes"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    hashes.retain(|h| h.as_str() != Some(r.hash.as_str()));
    hashes.push(json!(r.hash));
    out.insert("object_id".into(), json!(object_id));
    out.insert("by_device_id".into(), json!(r.sender));
    out.insert("text".into(), text);
    out.insert(
        "object_version".into(),
        json!(int_of(c, "object_version").unwrap_or(old_version + 1)),
    );
    out.insert("version_hash".into(), json!(r.hash));
    out.insert("version_hashes".into(), Value::Array(hashes));
    out.insert(
        "causal".into(),
        serde_json::to_value(causal_of(r, c)).unwrap_or(Value::Null),
    );
    out.insert("envelope_number".into(), json!(r.change));
    out.insert("object_state".into(), json!(state_after(r)));
    out.insert("pending".into(), json!(false));
    out.insert("unsupported".into(), json!(content_state == "newer_schema"));
    out
}

impl Model {
    fn apply_note(
        &mut self,
        r: &Received,
        change: &mut Change,
        local_id: Option<String>,
    ) -> Applied {
        let object_id = r.object.as_ref().expect("an object").object_id.clone();
        let cur = self.notes.get(&object_id).cloned();
        let cur_pending = cur
            .as_ref()
            .is_some_and(|c| c.get("pending").is_some_and(truthy));
        // the hub's copy of the echo in front
        let mine = local_id.is_some()
            && cur_pending
            && cur
                .as_ref()
                .and_then(|c| c.get("local_id"))
                .and_then(Value::as_str)
                == local_id.as_deref();
        let old: Option<Obj> = if cur_pending {
            cur.as_ref()
                .and_then(|c| c.get("_base"))
                .and_then(Value::as_object)
                .cloned()
        } else {
            cur.clone()
        };
        let (content, state) = body_of(r, BodyKind::Note);
        // Which version is current is the core's word (9.2.1, 9.3.2). Out of order, a version counts where the model
        // holds none, or an older one.
        let current = match &r.object_after {
            Some(a) => a.current == r.hash,
            None => old.as_ref().is_none_or(|o| {
                r.change
                    >= o.get("envelope_number")
                        .and_then(Value::as_u64)
                        .unwrap_or(0)
            }),
        };
        if let Some(l) = &local_id {
            self.echoes.remove(l);
            self.by_hash.remove(&r.hash);
        }
        change.notes.insert(object_id.clone());
        if !current {
            let mut old = old;
            if let Some(o) = old.as_mut() {
                let hashes = o.entry("version_hashes").or_insert(json!([]));
                if let Some(list) = hashes.as_array_mut() {
                    if !list.iter().any(|h| h.as_str() == Some(r.hash.as_str())) {
                        list.push(json!(r.hash));
                    }
                }
                // (the confirmed note, also where it stands under an own echo)
                if cur_pending {
                    if let Some(c) = self.notes.get_mut(&object_id) {
                        c.insert("_base".into(), Value::Object(o.clone()));
                    }
                } else {
                    self.notes.insert(object_id.clone(), o.clone());
                }
            }
            if mine {
                // the own echo lost: show the winner
                match old {
                    Some(o) => self.notes.insert(object_id.clone(), o),
                    None => self.notes.remove(&object_id),
                };
            }
            return Applied::no();
        }
        let again = old
            .as_ref()
            .and_then(|o| o.get("version_hash"))
            .and_then(Value::as_str)
            == Some(r.hash.as_str());
        if again && content.is_none() {
            if mine {
                if let Some(o) = old {
                    self.notes.insert(object_id.clone(), o);
                }
            }
            return Applied::no();
        }
        let mut note = note_of(&object_id, r, content.as_ref(), &state, old.as_ref());
        if again && int_of(content.as_ref(), "object_version").is_none() {
            // the same version, now with its body
            if let Some(v) = old.as_ref().and_then(|o| o.get("object_version")) {
                note.insert("object_version".into(), v.clone());
            }
        }
        if state == "newer_schema" {
            self.note_newer(change, "note", Some(r.change));
        }
        // An own newer echo stays in front until its version comes back; it keeps the confirmed note as its base.
        if cur_pending && !mine {
            if let Some(c) = self.notes.get_mut(&object_id) {
                c.insert("_base".into(), Value::Object(note));
            }
        } else {
            self.notes.insert(object_id, note);
        }
        Applied::yes()
    }
}

impl Model {
    fn request_touched(&self, id: &str, change: &mut Change) {
        change.permissions.insert(id.to_string());
        if let Some(s) = self.permissions.get(id).and_then(|p| p.session_id.clone()) {
            change.sessions.insert(s);
        }
        change.stack = true;
    }

    fn apply_request(&mut self, r: &Received, change: &mut Change) -> Applied {
        let o = r.object.as_ref().expect("an object");
        let object_id = o.object_id.clone();
        let n = r.change;
        match r.kind {
            EnvelopeKind::Request => {
                let (content, state) = body_of(r, BodyKind::Request);
                let expires = match &r.bind {
                    Some(Bind::Request { expires_at }) => Some(*expires_at),
                    _ => None,
                };
                match self.permissions.get(&object_id) {
                    None => {
                        let p = PermissionRequest {
                            object_id: object_id.clone(),
                            catching_up: false,
                            agent_device_id: r
                                .object_after
                                .as_ref()
                                .map(|a| a.owner.clone())
                                .unwrap_or_else(|| r.sender.clone()),
                            session_id: r.session_id.clone(),
                            tool_name: String::new(),
                            description: String::new(),
                            input_preview: String::new(),
                            expires_at: 0,
                            version_hash: r.hash.clone(),
                            envelope_number: n,
                            sent_at: r.time,
                            permission_state: if state_after(r) == "open" {
                                "pending".into()
                            } else {
                                "denied".into()
                            },
                            content_state: "header".into(),
                            withdraw_reason: None,
                            verdict: None,
                        };
                        self.permissions.insert(object_id.clone(), p);
                    }
                    Some(p) if p.version_hash != r.hash => return Applied::no(),
                    Some(_) => {}
                }
                let p = self.permissions.get_mut(&object_id).expect("the request");
                if let Some(e) = expires {
                    p.expires_at = e;
                }
                if let Some(c) = &content {
                    let s = |k: &str| match c.get(k) {
                        None | Some(Value::Null) => String::new(),
                        Some(v) => js_text(v),
                    };
                    p.tool_name = s("tool_name");
                    p.description = s("description");
                    p.input_preview = s("input_preview");
                }
                p.content_state =
                    if content.is_some() || expires.is_some() || p.content_state == "ok" {
                        "ok".into()
                    } else {
                        state
                    };
            }
            EnvelopeKind::Verdict => {
                let Some(p) = self.permissions.get_mut(&object_id) else {
                    return Applied::no();
                };
                // The verdict stands in the bind, inside the body: of a pruned one only "closed" is left, shown as not allowed.
                let allow = match &r.bind {
                    Some(Bind::Verdict { allow }) => {
                        p.verdict = Some(Verdict {
                            allow: *allow,
                            by_device_id: r.sender.clone(),
                            envelope_number: n,
                        });
                        *allow
                    }
                    _ => false,
                };
                p.permission_state = if allow {
                    "allowed".into()
                } else {
                    "denied".into()
                };
            }
            _ => return Applied::no(),
        }
        self.request_touched(&object_id, change);
        Applied::yes()
    }
}

impl Model {
    fn apply_artifact(&mut self, r: &Received, change: &mut Change) -> Applied {
        let o = r.object.as_ref().expect("an object");
        let object_id = o.object_id.clone();
        let n = r.change;
        let old = self.published.get(&object_id).cloned();
        let (content, state) = body_of(r, BodyKind::Artifact);
        let first = is_zero_hash(o.object_ref.as_deref());
        let newest = old.as_ref().is_none_or(|p| n >= p.envelope_number);
        if let Some(p) = &old {
            if !newest && !(first && p.sent_at != r.time) {
                return Applied::no();
            }
        }
        let c = if newest { content.as_ref() } else { None };
        let get = |k: &str| c.and_then(|c| c.get(k)).filter(|v| !v.is_null()).cloned();
        let next = Published {
            object_id: object_id.clone(),
            agent_device_id: match (&r.object_after, newest) {
                (Some(a), true) => a.owner.clone(),
                _ => old
                    .as_ref()
                    .map(|p| p.agent_device_id.clone())
                    .unwrap_or_else(|| r.sender.clone()),
            },
            session_id: old
                .as_ref()
                .and_then(|p| p.session_id.clone())
                .or_else(|| r.session_id.clone()),
            attachments: get("attachments")
                .or_else(|| old.as_ref().map(|p| p.attachments.clone()))
                .unwrap_or(json!([])),
            title: get("title")
                .map(|t| js_text(&t))
                .or_else(|| old.as_ref().map(|p| p.title.clone()))
                .unwrap_or_default(),
            note: if c.is_some() {
                get("note")
            } else {
                old.as_ref().and_then(|p| p.note.clone())
            },
            released_until: if c.is_some() {
                get("released_until")
            } else {
                old.as_ref().and_then(|p| p.released_until.clone())
            },
            artifact_type: get("artifact_type")
                .or_else(|| old.as_ref().and_then(|p| p.artifact_type.clone())),
            object_version: match (&old, newest) {
                (Some(p), false) => p.object_version,
                (Some(p), true) if p.version_hash == r.hash => p.object_version,
                _ => int_of(c, "object_version")
                    .unwrap_or(old.as_ref().map_or(0, |p| p.object_version) + 1),
            },
            version_hash: if newest {
                r.hash.clone()
            } else {
                old.as_ref()
                    .map(|p| p.version_hash.clone())
                    .unwrap_or_default()
            },
            envelope_number: if newest {
                n
            } else {
                old.as_ref().map_or(n, |p| p.envelope_number)
            },
            sent_at: if first {
                r.time
            } else {
                old.as_ref().map_or(r.time, |p| p.sent_at)
            },
            object_state: if newest {
                state_after(r)
            } else {
                old.as_ref()
                    .map(|p| p.object_state.clone())
                    .unwrap_or_default()
            },
            content_state: if !newest {
                old.as_ref()
                    .map(|p| p.content_state.clone())
                    .unwrap_or_default()
            } else if c.is_some() {
                "ok".into()
            } else if let Some(p) = old.as_ref().filter(|p| p.version_hash == r.hash) {
                p.content_state.clone()
            } else {
                state
            },
        };
        if let Some(s) = &next.session_id {
            change.sessions.insert(s.clone());
        }
        self.published.insert(object_id.clone(), next);
        change.published.insert(object_id);
        Applied::yes()
    }
}

/// What a human register write carries.
#[derive(Clone, Debug, Default)]
pub struct RegisterWrite {
    pub envelope_number: Option<u64>,
    pub sender_device_id: Option<String>,
    pub pending: bool,
    pub causal: Option<Causal>,
}

impl Model {
    /// Put one room register into the model: its raw entry (a delete stays as `value: null`), and what it touches.
    pub fn set_human_register(
        &mut self,
        key: &str,
        value: Value,
        w: RegisterWrite,
        change: &mut Change,
    ) {
        self.human.insert(
            key.to_string(),
            HumanRegister {
                value: value.clone(),
                envelope_number: w.envelope_number,
                by_device_id: w.sender_device_id,
                pending: w.pending,
                causal: w.causal,
            },
        );
        let (prefix, id) = key.split_once('/').unwrap_or((key, ""));
        match prefix {
            "draft" | "duck" => {
                change.cards.insert(id.to_string());
            }
            "snooze" | "star" => {
                change.cards.insert(id.to_string());
                change.stack = true;
            }
            "session" => {
                // A session's settings are kept under its session id. A key that is an agent device of a known session
                // (an older web wrote them so) is that session's: no session of its own.
                let real = if self.sessions.contains_key(id) {
                    id.to_string()
                } else {
                    self.sessions
                        .values()
                        .find(|x| {
                            x.session_id != id
                                && (x.agent_device_id.as_deref() == Some(id)
                                    || x.agent_device_ids.iter().any(|d| d == id))
                        })
                        .map(|x| x.session_id.clone())
                        .unwrap_or_else(|| id.to_string())
                };
                if real != id && value.is_null() {
                    change.registers.insert(key.to_string());
                    return;
                }
                if real != id
                    && self
                        .human
                        .get(&format!("session/{real}"))
                        .is_some_and(|r| !r.value.is_null())
                {
                    change.registers.insert(key.to_string());
                    return;
                }
                let s = self.session_of(&real);
                s.settings = if value.is_null() { None } else { Some(value) };
                change.sessions.insert(real);
                change.stack = true;
            }
            "scribble_snapshot" => {
                change.timelines.insert(timeline_key("scribble", id));
            }
            _ => {}
        }
        change.registers.insert(key.to_string());
    }

    /// A device's own register `device/<id>`: its name and where it runs, shown on its member and its sessions.
    fn set_device_register(&mut self, device_id: &str, value: &Value, session: &Option<String>, change: &mut Change) {
        let v = value.as_object().cloned().unwrap_or_default();
        if self.device_register_groups.get(device_id) != Some(session) {
            self.device_register_groups.insert(device_id.to_string(), session.clone());
            change.room = true;
        }
        self.device_registers
            .insert(device_id.to_string(), v.clone());
        if let Some(m) = self.members.get_mut(device_id) {
            m.device_name = super::groups::js_string(v.get("device_name"));
            m.platform = v.get("platform").cloned().unwrap_or(Value::Null);
            m.folder = v.get("folder").cloned().unwrap_or(Value::Null);
            m.host = v.get("host").cloned().unwrap_or(Value::Null);
            change.members = true;
            self.touch_agent(device_id, change);
        }
    }
}

impl Model {
    /// A register of a session group: kept raw, and what the views read of it (profile, status lines, the receipt,
    /// alerts).
    #[allow(clippy::too_many_arguments)]
    fn set_session_register(
        &mut self,
        session_id: &str,
        key: &str,
        value: &Value,
        r: &Received,
        causal: Causal,
        change: &mut Change,
        now: u64,
        ordered: bool,
    ) {
        let mut alert: Option<(String, String)> = None;
        {
            let s = self.session_of(session_id);
            s.registers.insert(
                key.to_string(),
                RegisterValue {
                    value: value.clone(),
                    envelope_number: r.change,
                    sender_sequence: Some(r.seq),
                    causal: Some(causal),
                },
            );
            if key == "profile" {
                super::groups::compose_profile(s);
            } else if key == "heard" {
                // The mark only rises: a receipt is never taken back.
                let up_to = value
                    .get("up_to")
                    .and_then(Value::as_f64)
                    .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= 9_007_199_254_740_991.0)
                    .map(|n| n as u64);
                if let Some(up_to) = up_to {
                    if s.heard_up_to.is_none_or(|h| up_to >= h) {
                        s.heard_up_to = Some(up_to);
                        s.heard_at = Some(
                            value
                                .get("at")
                                .and_then(Value::as_f64)
                                .filter(|n| n.fract() == 0.0 && *n > 0.0)
                                .map(|n| n as u64)
                                .unwrap_or(r.time),
                        );
                    }
                }
            } else if let Some(id) = key.strip_prefix("status_line/") {
                let at = s.status_lines.iter().position(|l| l.id == id);
                if value.is_null() {
                    if let Some(at) = at {
                        s.status_lines.remove(at);
                    }
                } else {
                    let g = |k: &str| value.get(k).filter(|v| !v.is_null()).cloned();
                    let line = StatusLine {
                        id: id.to_string(),
                        label: g("label").unwrap_or_else(|| json!(id)),
                        state: g("state").unwrap_or(Value::Null),
                        detail: g("detail").unwrap_or(Value::Null),
                        object_id: g("object_id").unwrap_or(Value::Null),
                        helper: crate::helpers::name_of(value.get("helper")),
                        envelope_number: r.change,
                        updated_at: r.time,
                    };
                    match at {
                        Some(at) => s.status_lines[at] = line,
                        None => s.status_lines.push(line),
                    }
                }
            } else if key.starts_with("alert/") {
                let at = s.agent_alerts.iter().position(|a| a.key == key);
                if value.is_null() {
                    if let Some(at) = at {
                        s.agent_alerts.remove(at);
                    }
                } else {
                    let a = AgentAlert {
                        key: key.to_string(),
                        value: value.clone(),
                        envelope_number: r.change,
                    };
                    match at {
                        Some(at) => s.agent_alerts[at] = a,
                        None => s.agent_alerts.push(a),
                    }
                    // (told once, when it arrives in the hub's order)
                    if ordered && at.is_none() {
                        let code = value
                            .get("code")
                            .filter(|v| !v.is_null())
                            .map(js_text)
                            .unwrap_or_else(|| "agent-alert".into());
                        let message = value
                            .get("message")
                            .filter(|v| !v.is_null())
                            .map(js_text)
                            .unwrap_or_default();
                        alert = Some((code, message));
                    }
                }
            }
        }
        if let Some((code, message)) = alert {
            self.push_alert(
                change,
                &code,
                &message,
                Some(r.change),
                Some(r.sender.clone()),
                "agent",
                now,
            );
        }
        change.sessions.insert(session_id.to_string());
        change.registers.insert(key.to_string());
    }
}

impl Model {
    fn apply_register(
        &mut self,
        r: &Received,
        change: &mut Change,
        now: u64,
        local_id: Option<String>,
        ordered: bool,
    ) -> Applied {
        let d = match (&r.payload, r.outcome) {
            (Some(p), o) if o != Outcome::Chained => {
                match decode_body(BodyKind::Register, p.as_bytes()) {
                    Decoded::Body(b) => Some(b),
                    _ => None,
                }
            }
            _ => None,
        };
        if local_id.is_some() {
            self.by_hash.remove(&r.hash);
        }
        let Some(d) = d else {
            // not readable here: an own echo of it cannot be told from its key, so it is taken back
            if let Some(l) = &local_id {
                self.rollback_echo(l, change);
            }
            return Applied::no();
        };
        let key = d
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let value = d.get("value").cloned().unwrap_or(Value::Null);
        let causal = Causal {
            sender_device_id: r.sender.clone(),
            sender_sequence: r.seq,
            sent_at: r.time,
            lamport: d.get("lamport").and_then(Value::as_f64).unwrap_or(0.0),
        };
        let current = r.register.as_ref().is_some_and(|x| x.current);
        if key == "heads" {
            return Applied::no(); // the chains' heads (9.0.7): the core's own
        }
        if key.starts_with("device/") {
            if current && key == format!("device/{}", r.sender) {
                self.set_device_register(&r.sender, &value, &r.session_id, change);
                change.registers.insert(key);
            }
            return Applied {
                applied: current,
                refused: None,
            };
        }
        if let Some(sid) = &r.session_id {
            if current {
                self.set_session_register(sid, &key, &value, r, causal, change, now, ordered);
            }
            return Applied {
                applied: current,
                refused: None,
            };
        }
        let write = RegisterWrite {
            envelope_number: Some(r.change),
            sender_device_id: Some(r.sender.clone()),
            pending: false,
            causal: Some(causal.clone()),
        };
        let confirmed = HumanRegister {
            value: value.clone(),
            envelope_number: Some(r.change),
            by_device_id: Some(r.sender.clone()),
            pending: false,
            causal: Some(causal.clone()),
        };
        let echo_open = local_id.as_ref().and_then(|l| match self.echoes.get(l) {
            Some(super::Echo::Registers { open, before, .. }) if open.contains(&key) => {
                Some(before.get(&key).cloned().flatten())
            }
            _ => None,
        });
        if let (Some(before), Some(l)) = (echo_open, &local_id) {
            // The hub's copy of an own write: it stands if it is the current value, else what won comes back.
            let newer = self
                .echoes
                .iter()
                .find(|(id, e)| {
                    *id != l
                        && matches!(e, super::Echo::Registers { open, .. } if open.contains(&key))
                })
                .map(|(id, _)| id.clone());
            if let Some(nid) = newer {
                if current {
                    if let Some(super::Echo::Registers { before, .. }) = self.echoes.get_mut(&nid) {
                        before.insert(key.clone(), Some(confirmed));
                    }
                }
            } else if current {
                self.set_human_register(&key, value, write, change);
            } else {
                let b = before.unwrap_or(HumanRegister {
                    value: Value::Null,
                    envelope_number: None,
                    by_device_id: None,
                    pending: false,
                    causal: None,
                });
                self.set_human_register(
                    &key,
                    b.value,
                    RegisterWrite {
                        envelope_number: b.envelope_number,
                        sender_device_id: b.by_device_id,
                        pending: false,
                        causal: b.causal,
                    },
                    change,
                );
            }
            let mut drop = false;
            if let Some(super::Echo::Registers { open, .. }) = self.echoes.get_mut(l) {
                open.retain(|k| k != &key);
                drop = open.is_empty();
            }
            if drop {
                self.echoes.remove(l);
            }
            return Applied {
                applied: current,
                refused: None,
            };
        }
        if !current {
            return Applied::no();
        }
        // Another device's write under an own echo still in flight: the echo stays in front, and falls back to this.
        if self.human.get(&key).is_some_and(|h| h.pending) {
            let under = self.echoes.iter().find(|(_, e)| matches!(e, super::Echo::Registers { open, .. } if open.contains(&key))).map(|(id, _)| id.clone());
            if let Some(id) = under {
                if let Some(super::Echo::Registers { before, .. }) = self.echoes.get_mut(&id) {
                    before.insert(key, Some(confirmed));
                }
                return Applied::yes();
            }
        }
        self.set_human_register(&key, value, write, change);
        Applied::yes()
    }

    /// The deeds on cards (hand back, What??, present) of a page of card Chats read out of the hub's order: they settle
    /// whether each card is in revision, and nothing else of the model is touched.
    pub fn apply_card_deeds(&mut self, received: &[Received], change: &mut Change) {
        let mut ch = Change::new();
        for r in received {
            if r.outcome != Outcome::Refused
                && r.kind == EnvelopeKind::Item
                && r.timeline
                    .as_ref()
                    .is_some_and(|t| t.kind == TimelineKind::CardChat)
            {
                if matches!(r.outcome, Outcome::Void)
                    || (r.outcome == Outcome::Chained
                        && r.code.as_deref().and_then(unread_state).is_none())
                {
                    continue;
                }
                self.apply_item(r, &mut ch, None, r.outcome != Outcome::Provisional, true);
            }
        }
        for id in ch.cards {
            if let Some(s) = self.cards.get(&id).and_then(|c| c.session_id.clone()) {
                change.sessions.insert(s);
            }
            change.cards.insert(id);
        }
    }
}
