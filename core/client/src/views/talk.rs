//! A conversation as rows: the messages of a chat's window in memory, what happened to a card (asked, revised,
//! answered, taken back, closed) as quiet lines, and a turn's trail folded into one block. A session's page and a
//! card's comments are both made from this.
//!
//! The rules are the web's of before (`app.mjs` `itemsOf`, `eventsOf`; `core/work.ts` `foldWork`, `workAnchor`;
//! `session.mjs` `deedOf`, `workBlock`, `workLine`, `span`):
//! - a message is his when one of his devices sent it, or when he typed it into the session's terminal (the agent's
//!   connector sent it for him: `terminal`);
//! - what a newer Trommi wrote stands as a placeholder in its place; a message that announces an Artifact which was
//!   revoked since leaves the conversation;
//! - a hand-back, a take-back and "What??" are deeds: with nothing of his own in them they are one quiet line, never
//!   his bubble (the words that went to the session with them are the machine's);
//! - the envelopes of one turn's trail are one row, at the turn's first envelope, or behind what he typed into the
//!   terminal while the turn ran.
//!
//! Nothing in a step is trusted beyond its shape: an agent wrote it. Texts are cut, unknown kinds and states are
//! left out, and a block lists at most [`WORK_ITEMS_MAX`] lines.

use super::board::Board;
use super::desk::{shown_title, NEEDS_NEWER};
use super::links::titled;
use super::words::say;
use super::{CardRefView, EventView, TalkRow, Text, WorkLineView, WorkView};
use crate::model::types::{Card, Timeline};
use serde_json::Value;
use std::collections::HashMap;

/// What a bare hand-back says to the session (the machine's words: a conversation shows a quiet line instead).
pub const HAND_BACK_TEXT: &str = "Back to you: please rework this question and present it again. Take the comments under the card into account.";
/// What a bare "What??" says to the session.
pub const EXPLAIN_TEXT: &str = "Explain this question in more detail and in plain words: what it is about, what each option means for me, and what you would do.";
/// The one file of a Scribble Board selection sent to a session.
pub const SCRIBBLE_FILE: &str = "selection.png";
pub const WORK_ITEMS_MAX: usize = 400;
/// A message without an envelope number yet (an echo) stands last.
pub const UNSENT: u64 = u64::MAX;

/// Who a row is from, for the runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    User,
    Agent,
    Event,
    Work,
}

/// A row before the page lays it out: with its place in the hub's order and what the page's rules need.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The envelope number; a card's closing line stands half a step behind its envelope (`half`).
    pub seq: u64,
    pub half: bool,
    pub side: Side,
    /// The card an event is of, or a comment is under.
    pub card_id: Option<String>,
    pub row: TalkRow,
}

/// The label of an event's line, with who did it: `who` is the agent's name ("" names it "The agent"); what the
/// person did is "You …".
pub fn event_label_by(kind: &str, who: &str) -> Text {
    let who = if who.is_empty() { "The agent" } else { who };
    let by = |key: &str, en: String| say(key, en, &[("name", who)]);
    match kind {
        "asked" => by("event.asked_by", format!("{who} asked")),
        "info" => by("event.info_by", format!("{who} wrote")),
        "revised" => by("event.revised_by", format!("{who} revised it")),
        "done" => by("event.done_by", format!("{who} closed it")),
        "withdrawn" => by("event.withdrawn_by", format!("{who} withdrew it")),
        "decided" => say("event.you_chose", "You chose", &[]),
        "trusted" => say("event.you_trusted", "You left it to the agent", &[]),
        "read" => say("event.you_read", "You read it", &[]),
        "shredded" => say("event.you_shredded", "You shredded it", &[]),
        "reopened" => say("event.you_reopened", "You took it back", &[]),
        other => event_label(other),
    }
}

/// The label of an event's line, without who did it.
pub fn event_label(kind: &str) -> Text {
    let (key, en) = match kind {
        "asked" => ("event.asked", "New question"),
        "decided" => ("event.decided", "Answered"),
        "done" => ("event.done", "Done"),
        "urgency" => ("event.urgency", "Urgency"),
        "reopened" => ("event.reopened", "Taken back"),
        "revised" => ("event.revised", "Question revised"),
        "trusted" => ("event.trusted", "I don’t give a duck"),
        "snoozed" => ("event.snoozed", "Later"),
        "handed" => ("event.handed", "With the agent"),
        "shredded" => ("event.shredded", "Shredded"),
        "took_back" => ("event.took_back", "You took it back"),
        "handed_back" => ("event.handed_back", "You handed it back"),
        "explain" => ("event.explain", "You asked for an explanation"),
        _ => ("event.board", "Board"),
    };
    say(key, en, &[])
}

/// A card as a row names it.
pub fn card_ref(b: &Board, c: &Card) -> CardRefView {
    CardRefView {
        id: c.object_id.clone(),
        number: b.number_of(&c.object_id),
        title: shown_title(c),
    }
}

fn str_of<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// The options of a card by key, for an answer's words.
fn labels(options: &Value, keys: &[&str]) -> String {
    let name = |k: &str| {
        options
            .as_array()
            .into_iter()
            .flatten()
            .find(|o| str_of(o, "key") == k)
            .map(|o| str_of(o, "label").to_string())
            .unwrap_or_else(|| k.to_string())
    };
    keys.iter().map(|k| name(k)).collect::<Vec<_>>().join(", ")
}

/// What he wrote along with an answer, a line each: his note in quotes, then a note on an option ("On Label: “…”"),
/// in the options' order.
fn said_along(note: &str, option_notes: &Value, options: &Value) -> Vec<Text> {
    let mut out = Vec::new();
    if !note.trim().is_empty() {
        out.push(say("deed.note", format!("“{note}”"), &[("note", note)]));
    }
    let Some(notes) = option_notes.as_object() else { return out };
    let mut keys: Vec<&str> = options.as_array().into_iter().flatten().map(|o| str_of(o, "key")).filter(|k| notes.contains_key(*k)).collect();
    let mut rest: Vec<&str> = notes.keys().map(String::as_str).filter(|k| !keys.contains(k)).collect();
    rest.sort_unstable();
    keys.extend(rest);
    for k in keys {
        let Some(text) = notes.get(k).and_then(Value::as_str).filter(|t| !t.trim().is_empty()) else { continue };
        let label = labels(options, &[k]);
        out.push(say("deed.option_note", format!("On {label}: “{text}”"), &[("label", &label), ("note", text)]));
    }
    out
}

/// The keys of an answer.
pub fn choices_of(v: &Value) -> Vec<&str> {
    v.as_array().into_iter().flatten().filter_map(Value::as_str).collect()
}

/// What happened to a card, as quiet lines in the hub's order: asked (an info: "info"), each revision, each answer
/// (read, shredded, decided), each taking back, and its end when its agent closed or withdrew it with a word.
pub fn events_of(b: &Board, c: &Card) -> Vec<Entry> {
    let title = shown_title(c);
    let who = b.by_key.get(super::board::key_of(&c.session_id, &c.agent_device_id)).map(|i| b.agents[*i].row.name.clone()).unwrap_or_default();
    let withdrawn_card = c.object_state == "closed" && !c.field("withdraw_reason").as_str().unwrap_or("").is_empty();
    let info = super::board::reads_as_info(c);
    let options = if info { Value::Null } else { c.field("options") };
    let ev = |n: u64, half: bool, kind: &str, text: String, ts: u64| Entry {
        seq: n,
        half,
        side: Side::Event,
        card_id: Some(c.object_id.clone()),
        row: TalkRow {
            id: format!("v{n}{}{}", if half { ".5" } else { "" }, &kind[..1]),
            kind: "event".into(),
            number: n,
            time_ms: ts,
            card: Some(card_ref(b, c)),
            event: Some(EventView {
                kind: kind.into(),
                label: event_label_by(if kind == "done" && withdrawn_card { "withdrawn" } else { kind }, &who),
                // (the card's title, when the words do not say it already)
                about: if text.is_empty() || text != title { title.clone() } else { String::new() },
                text,
                echo: false,
                ..Default::default()
            }),
            ..Default::default()
        },
    };
    let or_title = |s: &str| if s.is_empty() { title.clone() } else { s.to_string() };
    let mut out = Vec::new();
    for h in &c.history {
        if h.object_version == 1 {
            out.push(ev(h.envelope_number, false, if info { "info" } else { "asked" }, or_title(&h.title), h.sent_at));
        } else {
            let said = if h.note.is_empty() { or_title(&h.title) } else { h.note.clone() };
            out.push(ev(h.envelope_number, false, "revised", said, h.sent_at));
        }
    }
    for a in &c.answers {
        let n = a.envelope_number.unwrap_or(UNSENT);
        let keys = choices_of(&a.choices);
        let note = a.note.as_str().unwrap_or("");
        // (the labels as they stood in the version he answered, while this device holds its body)
        let bound = c
            .versions
            .iter()
            .find(|v| v.object_version == a.bound_object_version && a.bound_object_version > 0)
            .and_then(|v| v.content.as_ref())
            .map(|r| r.obj().get("options").cloned().unwrap_or(Value::Null))
            .filter(|o| !info && o.is_array());
        let options = bound.as_ref().unwrap_or(&options);
        let what = match a.answer_action.as_str() {
            "read" => "read",
            "shred" => "shredded",
            _ => "decided",
        };
        match what {
            "read" => out.push(ev(n, false, "read", title.clone(), a.answered_at)),
            "shredded" => {
                let said = if note.is_empty() { title.clone() } else { format!("{title} · {note}") };
                out.push(ev(n, false, "shredded", said, a.answered_at));
            }
            _ => {
                let said = if a.trusted {
                    if keys.is_empty() {
                        "Duck: your call".to_string()
                    } else {
                        format!("Duck: your call · {}", labels(options, &keys))
                    }
                } else {
                    labels(options, &keys)
                };
                out.push(ev(n, false, "decided", said, a.answered_at));
            }
        }
        // (what he wrote and sent along stands under the deed)
        if matches!(what, "decided" | "shredded") {
            if let Some(last) = out.last_mut().and_then(|e| e.row.event.as_mut()) {
                last.said = said_along(note, &a.option_notes, options);
                last.files = file_ids(&a.attachments);
            }
        }
        // (taken back: the same thing again, with the time of the taking back)
        if let Some(back) = a.taken_back_at {
            out.push(ev(back, false, "reopened", title.clone(), a.taken_back_sent_at.unwrap_or(a.answered_at + 1)));
        }
    }
    let summary = c.field("close_summary").as_str().unwrap_or("").to_string();
    let withdrawn = c.field("withdraw_reason").as_str().unwrap_or("").to_string();
    if c.object_state == "closed" && !(summary.is_empty() && withdrawn.is_empty()) {
        // (withdrawn: its label says so, "X withdrew it"; the words are its reason)
        let said = if withdrawn.is_empty() { summary } else { withdrawn };
        out.push(ev(c.envelope_number, true, "done", said, c.updated_at));
    }
    out
}

/// The files of a content's attachments: a page that belongs to a picture is no file of its own.
pub fn file_ids(attachments: &Value) -> Vec<String> {
    let list: Vec<&Value> = attachments.as_array().into_iter().flatten().collect();
    for a in &list {
        super::files_seen::saw(a);
    }
    let pages: Vec<&str> = list.iter().filter_map(|a| a.get("page").and_then(Value::as_str)).filter_map(|p| p.strip_prefix("attachment:")).collect();
    list.iter().map(|a| str_of(a, "attachment_id")).filter(|id| !id.is_empty() && !pages.contains(id)).map(String::from).collect()
}

/// A turn's trail while a chat is read: where its row stands in the list, its envelopes, and the place of each
/// (envelope number, row id, time).
struct Trail<'v> {
    at: usize,
    envelopes: Vec<&'v Value>,
    places: Vec<(u64, String, u64)>,
}

/// The messages of one chat (a session's, or a card's comments: `card`), as far as memory holds them, added to `out`.
pub fn items_of(b: &Board, t: &Timeline, card: Option<&Card>, heard_up_to: Option<u64>, out: &mut Vec<Entry>) {
    let m = b.model;
    let me = m.my_device_id.as_deref();
    let human = |dev: &str| Some(dev) == me || m.members.get(dev).is_some_and(|x| x.device_role == "human");
    let mut trails: Vec<Trail> = Vec::new();
    let mut at_turn: HashMap<String, usize> = HashMap::new();
    let mut typed: Vec<u64> = Vec::new();
    let null = Value::Null;
    for i in t.items.values() {
        let state = i.get("item_state").and_then(Value::as_str).unwrap_or("");
        let newer = matches!(state, "unsupported" | "newer_schema");
        let kind = i.get("content_type").and_then(Value::as_str).unwrap_or("message");
        if kind != "message" && !newer {
            continue;
        }
        let from_human = human(i.get("sender_device_id").and_then(Value::as_str).unwrap_or(""));
        let c = i.get("content").unwrap_or(&null);
        let number = i.get("envelope_number").and_then(Value::as_u64);
        let id = match number {
            Some(n) => format!("e{n}"),
            None => i.get("local_id").and_then(Value::as_str).unwrap_or("").to_string(),
        };
        let seq = number.unwrap_or(UNSENT);
        let ts = i.get("sent_at").and_then(Value::as_u64).unwrap_or(0);
        let mut row = TalkRow {
            id,
            number: number.unwrap_or(0),
            time_ms: ts,
            from_me: from_human,
            kind: if from_human { "user" } else { "agent" }.into(),
            card: card.map(|c| card_ref(b, c)),
            ..Default::default()
        };
        let entry = |row: TalkRow, side: Side| Entry {
            seq,
            half: false,
            side,
            card_id: card.map(|c| c.object_id.clone()),
            row,
        };
        if newer {
            row.kind = "newer".into();
            row.text = NEEDS_NEWER.into();
            out.push(entry(row, if from_human { Side::User } else { Side::Agent }));
            continue;
        }
        row.text = match state {
            "" | "loaded" => titled(str_of(c, "text"), b.assets()),
            "pruned" => "(removed after 30 days)".into(),
            _ => String::new(),
        };
        row.files = file_ids(c.get("attachments").unwrap_or(&null));
        let list = c.get("attachments").and_then(Value::as_array);
        row.scribble = from_human && list.is_some_and(|l| l.len() == 1 && str_of(&l[0], "file_name") == SCRIBBLE_FILE);
        row.details = titled(str_of(c, "details"), b.assets());
        row.html = str_of(c, "html").to_string();
        if !from_human {
            row.helper = crate::helpers::name_of(c.get("helper"));
        }
        let published = str_of(c, "published_object_id");
        if !published.is_empty() {
            // (a revoked Artifact leaves the conversation)
            if m.published.get(published).is_some_and(|p| p.object_state == "closed") {
                continue;
            }
            row.kind = "artifact".into();
            row.published = Some(published.to_string());
        }
        if let Some(note) = c.get("note").filter(|n| !n.is_null() && **n != Value::Bool(false)) {
            if from_human {
                row.kind = "note".into();
                row.written_ms = note.get("written_at").and_then(Value::as_u64);
            }
        }
        let terminal = str_of(c, "terminal");
        let mut side = if from_human { Side::User } else { Side::Agent };
        // The terminal mirror: counts only from an agent.
        if !from_human && matches!(terminal, "input" | "answer") {
            row.terminal = true;
            if terminal == "input" {
                side = Side::User;
                row.from_me = true;
                row.kind = "user".into();
                typed.push(seq);
            }
        }
        if terminal == "work" {
            let Some(w) = c.get("work").filter(|w| !from_human && is_work(w)) else { continue };
            let turn = str_of(w, "turn").to_string();
            let place = (seq, row.id.clone(), ts);
            match at_turn.get(&turn) {
                Some(&k) => {
                    trails[k].envelopes.push(w);
                    trails[k].places.push(place);
                }
                None => {
                    at_turn.insert(turn, trails.len());
                    trails.push(Trail { at: out.len(), envelopes: vec![w], places: vec![place] });
                    row.kind = "work".into();
                    row.text = String::new();
                    row.files = Vec::new();
                    out.push(entry(row, Side::Work));
                }
            }
            continue;
        }
        row.pending = i.get("pending") == Some(&Value::Bool(true));
        row.chips = c
            .get("copied_cards")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|id| match m.cards.get(id) {
                Some(card) => card_ref(b, card),
                None => CardRefView { id: id.into(), number: 0, title: id.into() },
            })
            .collect();
        if side == Side::User {
            row.heard = matches!((number, heard_up_to), (Some(n), Some(mark)) if n <= mark);
            // A hand-back, a take-back, What??: what he did, not what he said.
            let flag = |k: &str| c.get(k).is_some_and(|v| !v.is_null() && *v != Value::Bool(false));
            let deed = if flag("present_card") {
                Some("took_back")
            } else if flag("hand_back") {
                Some("handed_back")
            } else if flag("explain") {
                Some("explain")
            } else {
                None
            };
            if let Some(kind) = deed {
                let said = row.text.trim();
                let own = !said.is_empty() && kind != "took_back" && !(kind == "handed_back" && said == HAND_BACK_TEXT) && !(kind == "explain" && said == EXPLAIN_TEXT);
                let line = EventView {
                    kind: kind.into(),
                    label: event_label(kind),
                    about: card.map(shown_title).unwrap_or_default(),
                    text: String::new(),
                    echo: false,
                    ..Default::default()
                };
                if !own && row.files.is_empty() && row.chips.is_empty() {
                    row.kind = "event".into();
                    row.text = String::new();
                    row.event = Some(line);
                } else {
                    row.deed = Some(line);
                }
            }
        }
        out.push(entry(row, side));
    }
    // (what the session's agent holds of its team's knowledge: the count its knowledge steps name)
    let held = match t.timeline_id.strip_prefix("session/") {
        Some(session) if !trails.is_empty() => crate::knowledge::held_by(m, session),
        _ => 0,
    };
    for Trail { at, envelopes, places } in trails {
        let work = fold_work(&envelopes, held);
        let seqs: Vec<u64> = places.iter().map(|p| p.0).collect();
        if let Some((seq, id, ts)) = work_anchor(&seqs, &typed).and_then(|s| places.iter().find(|p| p.0 == s)) {
            let e = &mut out[at];
            e.seq = *seq;
            e.row.number = if *seq == UNSENT { 0 } else { *seq };
            e.row.id = id.clone();
            e.row.time_ms = *ts;
        }
        out[at].row.work = Some(work);
    }
}

/// The rows in the hub's order: by envelope number, then by time.
pub fn in_order(out: &mut [Entry]) {
    out.sort_by_key(|e| (e.seq, e.half, e.row.time_ms));
}

// ---- a turn's trail ----

/// Whether a message's `work` is a step of a trail.
pub fn is_work(w: &Value) -> bool {
    !str_of(w, "turn").is_empty() && w.get("items").is_some_and(Value::is_array)
}

/// A time as a trail says it: "0.3 s", "12 s", "2 min", "1 h 5 min".
pub fn span(ms: u64) -> String {
    if ms < 950 {
        let tenths = (ms.max(100) + 50) / 100;
        return format!("{}.{} s", tenths / 10, tenths % 10);
    }
    let sec = (ms + 500) / 1000;
    if sec < 60 {
        return format!("{sec} s");
    }
    let min = (sec + 30) / 60;
    if min < 60 {
        format!("{min} min")
    } else if min.is_multiple_of(60) {
        format!("{} h", min / 60)
    } else {
        format!("{} h {} min", min / 60, min % 60)
    }
}

fn count(n: u64, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

fn cut(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

#[derive(Default)]
struct Line {
    id: String,
    kind: String,
    tool: String,
    title: String,
    subject: String,
    state: String,
    ms: Option<u64>,
    steps: Option<u64>,
    exit: Option<u64>,
    text: String,
    input: String,
    output: String,
    /// A step that read the team's knowledge (the connector marks `search_knowledge` and `read_knowledge`).
    knowledge: bool,
}

/// The envelopes of ONE turn as one block. They are applied in the order of their `seq`, whatever order they are
/// given in; a line (by `id`) stands where it first came and takes the fields of every later envelope that names
/// it; the turn's state and times are those of the last envelope that says them.
/// `held`: the knowledge entries the session's agent holds, for a step that read them ("reads team knowledge (12)").
pub fn fold_work(envelopes: &[&Value], held: usize) -> WorkView {
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0).map(|n| n as u64);
    let mut list: Vec<&Value> = envelopes.iter().copied().filter(|w| w.is_object()).collect();
    list.sort_by_key(|w| num(w.get("seq")).unwrap_or(0));
    let (mut turn, mut state) = (String::new(), "running".to_string());
    let (mut started, mut ended, mut more) = (None, None, 0u64);
    let mut lines: Vec<Line> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for w in list {
        if let Some(t) = w.get("turn").and_then(Value::as_str) {
            turn = cut(t, 64);
        }
        let s = str_of(w, "state");
        if matches!(s, "running" | "done" | "interrupted" | "failed") {
            state = s.into();
        }
        started = num(w.get("started_at")).or(started);
        ended = num(w.get("ended_at")).or(ended);
        more = num(w.get("more")).unwrap_or(more);
        for x in w.get("items").and_then(Value::as_array).into_iter().flatten() {
            let (Some(id), kind) = (x.get("id").and_then(Value::as_str), str_of(x, "kind")) else { continue };
            if !matches!(kind, "step" | "text" | "helper") {
                continue;
            }
            let k = match at.get(id) {
                Some(k) => *k,
                None => {
                    if lines.len() >= WORK_ITEMS_MAX {
                        continue;
                    }
                    at.insert(id.to_string(), lines.len());
                    lines.push(Line { id: cut(id, 80), kind: kind.into(), ..Default::default() });
                    lines.len() - 1
                }
            };
            let line = &mut lines[k];
            if line.kind != kind {
                continue;
            }
            for (key, max, slot) in [
                ("tool", 80, &mut line.tool),
                ("title", 200, &mut line.title),
                ("subject", 300, &mut line.subject),
                ("text", 30_000, &mut line.text),
                ("input", 4_000, &mut line.input),
                ("output", 8_000, &mut line.output),
            ] {
                if let Some(s) = x.get(key).and_then(Value::as_str) {
                    *slot = cut(s, max);
                }
            }
            if x.get("knowledge") == Some(&Value::Bool(true)) && line.kind == "step" {
                line.knowledge = true;
            }
            let s = str_of(x, "state");
            if matches!(s, "running" | "ok" | "failed" | "interrupted") {
                line.state = s.into();
            }
            line.ms = num(x.get("ms")).or(line.ms);
            line.steps = num(x.get("steps")).or(line.steps);
            if let Some(exit) = x.get("exit").and_then(Value::as_u64).filter(|e| line.kind == "step" && (1..256).contains(e)) {
                line.exit = Some(exit);
            }
        }
    }

    // ---- in words (the web's `workBlock` and `workLine`) ----
    let steps = lines.iter().filter(|x| x.kind == "step").count() as u64 + more;
    let helpers = lines.iter().filter(|x| x.kind == "helper").count() as u64;
    let failed = lines.iter().filter(|x| x.state == "failed").count() as u64;
    let took = match (started, ended) {
        (Some(s), Some(e)) if e >= s => span(e - s),
        _ => String::new(),
    };
    let head = match state.as_str() {
        "running" => say("work.running", "Working", &[]),
        "done" if took.is_empty() => say("work.done", "Worked", &[]),
        "done" => say("work.done_took", format!("Worked {took}"), &[("took", &took)]),
        "interrupted" if took.is_empty() => say("work.interrupted", "Interrupted", &[]),
        "interrupted" => say("work.interrupted_took", format!("Interrupted after {took}"), &[("took", &took)]),
        _ if took.is_empty() => say("work.failed", "Stopped by an error", &[]),
        _ => say("work.failed_took", format!("Stopped by an error after {took}"), &[("took", &took)]),
    };
    let mut parts = Vec::new();
    if steps > 0 {
        parts.push(count(steps, "step"));
    }
    if helpers > 0 {
        parts.push(count(helpers, "helper"));
    }
    if failed > 0 {
        parts.push(format!("{failed} with {}", if failed == 1 { "an error" } else { "errors" }));
    }
    let sum = (!parts.is_empty()).then(|| {
        say("work.sum", parts.join(" · "), &[("steps", &steps.to_string()), ("helpers", &helpers.to_string()), ("failed", &failed.to_string())])
    });
    let bold = |x: &Line| {
        if x.kind == "helper" {
            format!("Helper {}", x.tool).trim().to_string()
        } else if x.tool.is_empty() {
            "Step".to_string()
        } else {
            x.tool.clone()
        }
    };
    // (a step that read the team's knowledge says so in the person's words, with what it searched for)
    let knows = |x: &Line| super::knowledge::work_words(held, &x.subject).en;
    let now = if state == "running" {
        lines
            .iter()
            .rfind(|x| x.kind != "text" && (x.state.is_empty() || x.state == "running"))
            .map(|x| {
                if x.knowledge {
                    return knows(x);
                }
                let tool = if x.kind == "helper" { bold(x) } else { x.tool.clone() };
                let what = if x.title.is_empty() { x.subject.clone() } else { x.title.clone() };
                [tool, what].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
            })
            .unwrap_or_default()
    } else {
        String::new()
    };
    let rows = lines
        .iter()
        .map(|x| {
            if x.kind == "text" {
                return WorkLineView { id: x.id.clone(), kind: "text".into(), text: x.text.clone(), ..Default::default() };
            }
            let st = if x.state.is_empty() { "running" } else { x.state.as_str() };
            let exit = x.exit.filter(|_| st == "failed").map(|e| e.to_string());
            let helper = x.kind == "helper";
            WorkLineView {
                id: x.id.clone(),
                kind: x.kind.clone(),
                state: st.into(),
                says: Some(match (&exit, st) {
                    (Some(e), _) => say("work.step.exit", format!("exit code {e}"), &[("code", e)]),
                    (None, "ok") => say("work.step.ok", "done", &[]),
                    (None, "failed") => say("work.step.failed", "failed", &[]),
                    (None, "interrupted") => say("work.step.interrupted", "interrupted", &[]),
                    _ => say("work.step.running", "running", &[]),
                }),
                tool: if x.knowledge { String::new() } else { bold(x) },
                what: if x.knowledge { knows(x) } else { [x.title.as_str(), x.subject.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ") },
                count: match (helper, x.steps.filter(|n| *n > 0), &exit) {
                    (true, Some(n), _) => Some(say("work.step.steps", count(n, "step"), &[("n", &n.to_string())])),
                    (false, _, Some(e)) => Some(say("work.step.exit", format!("exit code {e}"), &[("code", e)])),
                    _ => None,
                },
                took: x.ms.map(span).unwrap_or_default(),
                text: String::new(),
                input: x.input.clone(),
                output: x.output.clone(),
            }
        })
        .collect();
    WorkView {
        turn,
        state,
        head,
        sum,
        now,
        lines: rows,
        rest: (more > 0).then(|| say("work.rest", format!("and {}, not listed", count(more, "more step")), &[("n", &more.to_string())])),
    }
}

/// Where a turn's block stands in its chat. `envelopes`: the places (envelope numbers) of the turn's envelopes;
/// `typed`: the places of what he typed into the terminal in the same chat. The block stands at the turn's first
/// envelope; but what he types while a turn runs goes into that turn, and the trail goes on behind it: then the
/// block stands behind his last such message, at the first envelope after it. So his words never stand under the
/// work they were typed into, and the block is drawn once.
pub fn work_anchor(envelopes: &[u64], typed: &[u64]) -> Option<u64> {
    let mut at = envelopes.to_vec();
    at.sort_unstable();
    let (first, last) = (*at.first()?, *at.last()?);
    let cut = typed.iter().copied().filter(|t| *t > first && *t < last).max();
    match cut {
        Some(c) => at.iter().copied().find(|n| *n > c).or(Some(first)),
        None => Some(first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The web's `span` (app/web/public/session.mjs).
    #[test]
    fn a_span_in_a_trails_words() {
        assert_eq!([span(0), span(300), span(949), span(950), span(12_400), span(59_400)], ["0.1 s", "0.3 s", "0.9 s", "1 s", "12 s", "59 s"]);
        assert_eq!([span(59_600), span(2 * 60_000), span(65 * 60_000), span(120 * 60_000)], ["1 min", "2 min", "1 h 5 min", "2 h"]);
    }

    /// The web's `foldWork` tests (tests/web/model/timelines.test.mjs): the order of `seq`, a line takes later fields.
    #[test]
    fn the_envelopes_of_a_turn_fold_into_one_block() {
        let a = json!({ "turn": "t1", "seq": 1, "state": "running", "started_at": 1_000, "items": [
            { "id": "1", "kind": "step", "tool": "Bash", "title": "run the tests", "state": "running" },
            { "id": "2", "kind": "text", "text": "Two fail." },
        ] });
        let b = json!({ "turn": "t1", "seq": 2, "items": [
            { "id": "1", "kind": "step", "state": "failed", "exit": 2, "ms": 12_400, "output": "FAILED" },
            { "id": "3", "kind": "helper", "tool": "Explore", "steps": 3, "state": "ok" },
            { "id": "4", "kind": "mystery" }, { "id": "2", "kind": "step", "title": "not its kind" },
            { "id": "5", "kind": "step", "tool": "Edit", "subject": "views.rs" },
        ] });
        // (given out of order: applied by seq)
        let w = fold_work(&[&b, &a], 0);
        assert_eq!((w.turn.as_str(), w.state.as_str(), w.head.en.as_str(), w.now.as_str()), ("t1", "running", "Working", "Edit · views.rs"));
        assert_eq!(w.sum.as_ref().unwrap().en, "2 steps · 1 helper · 1 with an error");
        let l: Vec<(&str, &str, &str, &str, &str)> = w.lines.iter().map(|x| (x.id.as_str(), x.kind.as_str(), x.state.as_str(), x.tool.as_str(), x.what.as_str())).collect();
        assert_eq!(l, [("1", "step", "failed", "Bash", "run the tests"), ("2", "text", "", "", ""), ("3", "helper", "ok", "Helper Explore", ""), ("5", "step", "running", "Edit", "views.rs")]);
        assert_eq!((w.lines[0].says.as_ref().unwrap().en.as_str(), w.lines[0].count.as_ref().unwrap().en.as_str(), w.lines[0].took.as_str(), w.lines[0].output.as_str()), ("exit code 2", "exit code 2", "12 s", "FAILED"));
        assert_eq!((w.lines[1].text.as_str(), w.lines[2].count.as_ref().unwrap().en.as_str()), ("Two fail.", "3 steps"));
        // the turn ended: how long it took, and what is not listed
        let c = json!({ "turn": "t1", "seq": 3, "state": "done", "ended_at": 121_000, "more": 12, "items": [] });
        let w = fold_work(&[&a, &b, &c], 0);
        assert_eq!((w.head.en.as_str(), w.now.as_str(), w.rest.unwrap().en.as_str()), ("Worked 2 min", "", "and 12 more steps, not listed"));
        assert_eq!(w.sum.unwrap().en, "14 steps · 1 helper · 1 with an error");
        let d = json!({ "turn": "t1", "seq": 4, "state": "interrupted" });
        assert_eq!(fold_work(&[&a, &c, &d], 0).head.en, "Interrupted after 2 min");
        let e = json!({ "turn": "t2", "seq": 1, "state": "failed", "items": [] });
        assert_eq!(fold_work(&[&e], 0).head.en, "Stopped by an error");
    }

    #[test]
    fn a_step_that_read_the_team_knowledge_says_so() {
        let a = json!({ "turn": "t1", "seq": 1, "state": "running", "items": [
            { "id": "1", "kind": "step", "tool": "trommi: search_knowledge", "knowledge": true, "subject": "deploy", "state": "ok" },
            { "id": "2", "kind": "step", "tool": "trommi: read_knowledge", "knowledge": true },
        ] });
        let w = fold_work(&[&a], 12);
        assert_eq!(w.now, "reads team knowledge (12)");
        let l: Vec<(&str, &str)> = w.lines.iter().map(|x| (x.tool.as_str(), x.what.as_str())).collect();
        assert_eq!(l, [("", "reads team knowledge (12) · “deploy”"), ("", "reads team knowledge (12)")]);
        // (the mark only counts on a step)
        let b = json!({ "turn": "t1", "seq": 1, "items": [{ "id": "1", "kind": "helper", "tool": "Explore", "knowledge": true }] });
        assert_eq!(fold_work(&[&b], 3).lines[0].tool, "Helper Explore");
    }

    /// The web's `workAnchor` tests.
    #[test]
    fn the_block_stands_behind_what_he_typed_while_the_turn_ran() {
        assert_eq!(work_anchor(&[], &[4]), None);
        assert_eq!(work_anchor(&[12, 10, 14], &[]), Some(10));
        // (typed before the turn, or after its last step: the first envelope)
        assert_eq!(work_anchor(&[10, 12, 14], &[9, 15]), Some(10));
        // (typed while it ran: the first envelope after his last such message)
        assert_eq!(work_anchor(&[10, 12, 14, 16], &[11, 13]), Some(14));
    }
}
