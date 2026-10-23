//! One session's page: its heading (the dot, the model, the red hand or the quiet hint), its conversation as rows,
//! the status lines of its running work, and what stands above the field to write in. A shell draws the rows one
//! after the other and decides nothing.
//!
//! The rules are the web's of before (`session.mjs` `sessionOf`, `ask`, `message`, `logItems`, `sessionWho`,
//! `sessionDot`, `sessionStatus`, `sessionOpen`, `sessionLink`, `composeWords`; `app.mjs` `lineCard`):
//! - the conversation is the session's chat, its cards' comments and what happened to its cards, in the hub's order;
//!   an approval it waits for stands at the end;
//! - a question stands where it was last put: open and waiting, as its Desk row with its answers; else one quiet
//!   line that says what became of it (answered, done, shredded, with the agent, put off);
//! - rows of one side that follow each other within five minutes of the same day are a run; a new day gets a line;
//! - a status line shows while it is working, waits on him or is put off with its card; a done one for ten minutes;
//!   a stale one (nobody updated it for two hours) greyed and still for a day; never one of a gone session, and a
//!   gone or cut off session is never "working" on what he wrote;
//! - its open questions are the decisions and approvals that wait for him (`waiting`, its count `open`): a card to
//!   read is not one;
//! - above the field: why the session does not hear (cut off, gone, not listening) with the step in its terminal,
//!   or that his last message was not picked up for two minutes;
//! - its helpers (its agent's subagents, `helpers.rs`) stand in a short list above the status lines: name and state,
//!   no way into a chat of their own; a status line of a helper names it (`StatusLineView::helper`).

use super::board::{key_of, reads_as_info, Board, Lens, TALK_PAGE, UNHEARD_MS};
use super::desk::{card_row, request_row, shown_title};
use super::sidebar::{hand_of, link_of};
use super::talk::{card_ref, choices_of, event_label_by, events_of, in_order, items_of, Entry, Side, UNSENT};
use super::words::{local, next_midnight, quiet_words, say};
use super::{CardRefView, DotView, EmptyTalkView, EventChipView, EventView, FixView, HelperListView, HelperRowView, LinkNoteView, LinkWords, MoreView, PanelTabView, SessionView, StatusLineView, QuoteView, TalkRow, Text, WorkingView};
use crate::model::types::Card;
use crate::off_mind::went_at;
use crate::status::card_place;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Messages of one side closer than this stand as one run.
pub const GROUP_GAP_MS: u64 = 5 * 60_000;
/// So long after his last word the session counts as answering; a done status line shows so long.
pub const WORKING_WINDOW_MS: u64 = 10 * 60_000;
/// A stale status line shows so long.
pub const STALE_SHOWN_MS: u64 = 24 * 60 * 60_000;

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// The line above the first row of a day: "Today", "Yesterday", else "Tuesday 13 October".
pub fn day_label(ts: u64, now: u64, zone: i32) -> Text {
    let (a, n) = (local(ts, zone), local(now, zone));
    if a.days == n.days {
        say("day.today", "Today", &[])
    } else if a.days + 1 == n.days {
        say("day.yesterday", "Yesterday", &[])
    } else {
        let weekday = WEEKDAYS[(a.days + 4).rem_euclid(7) as usize];
        let (day, month) = (a.day.to_string(), MONTHS[(a.month - 1) as usize]);
        say("day.date", format!("{weekday} {day} {month}"), &[("weekday", weekday), ("day", &day), ("month", month)])
    }
}

/// The note for a session that has not picked his word up: look at its terminal.
pub fn unheard_fix(codex: bool) -> FixView {
    if codex {
        FixView {
            say: say("link.fix.look_codex", "Look at its terminal: type anything to start a turn, or start it again with", &[]),
            code: "codex resume".into(),
        }
    } else {
        FixView {
            say: say("link.fix.look", "Look at its terminal: type anything to wake it, or reconnect it with", &[]),
            code: "/mcp → trommi → Reconnect".into(),
        }
    }
}

/// A link's note: its sentence and its way out, under a receipt.
pub fn link_note(link: &LinkWords, receipt: Option<Text>) -> LinkNoteView {
    LinkNoteView {
        tone: link.state.clone(),
        receipt,
        line: (link.state != "live").then(|| link.line.clone()),
        fix: link.fix.clone(),
    }
}

/// A card that waits on the Desk now: open, read or not, and neither put off nor with its agent.
fn waits(c: &Card, on_stack: &HashSet<&str>) -> bool {
    c.object_state == "open" && !c.catching_up && c.in_revision.is_none() && on_stack.contains(c.object_id.as_str())
}

/// What became of a question, for its line where it was asked.
fn outcome(c: &Card) -> (&'static str, String) {
    let a = c.answer.as_ref();
    let settled = c.closed_how.as_deref() == Some("settled") && a.is_some();
    let options = c.field("options");
    let label_of = |keys: &[&str]| -> String {
        let name = |k: &str| {
            options.as_array().into_iter().flatten().find(|o| o.get("key").and_then(Value::as_str) == Some(k)).and_then(|o| o.get("label")).and_then(Value::as_str).unwrap_or(k).to_string()
        };
        keys.iter().map(|k| name(k)).collect::<Vec<_>>().join(", ")
    };
    match Board::status_of(c) {
        s if s == "decided" || settled => {
            let trusted = a.is_some_and(|a| a.trusted);
            let chosen = a.map(|a| label_of(&choices_of(&a.choices))).unwrap_or_default();
            if !trusted {
                return ("decided", chosen);
            }
            if !chosen.is_empty() {
                return ("trusted", chosen);
            }
            let advised: Vec<String> = match c.field("recommended") {
                Value::String(s) => vec![s],
                Value::Array(l) => l.iter().filter_map(Value::as_str).map(String::from).collect(),
                _ => Vec::new(),
            };
            // (the advised options, in the card's order)
            let keys: Vec<&str> = options.as_array().into_iter().flatten().filter_map(|o| o.get("key").and_then(Value::as_str)).filter(|k| advised.iter().any(|x| x == k)).collect();
            let said = label_of(&keys);
            ("trusted", if said.is_empty() { "your call".into() } else { said })
        }
        "shredded" => ("shredded", String::new()),
        "open" => {
            let working = c.field("working") == Value::Bool(true) && c.field("card_type").as_str() == Some("info");
            (if c.in_revision.is_some() || working { "handed" } else { "snoozed" }, String::new())
        }
        _ => {
            let summary = c.field("close_summary").as_str().unwrap_or("").to_string();
            ("done", if summary.is_empty() { c.field("withdraw_reason").as_str().unwrap_or("").to_string() } else { summary })
        }
    }
}

/// The page of the session `id` (its key, or its short id of the address); none when it is not on the board.
pub fn session(b: &Board, id: &str) -> Option<SessionView> {
    let mut v = page(b, id)?;
    piles(b, &mut v);
    // The talk's window: the newest pages (the day line and the runs stay as the whole talk has them).
    let pages = Lens::pages(&b.lens.talk_window, &v.id).max(Lens::pages(&b.lens.talk_window, &v.slug));
    let keep = TALK_PAGE * pages as usize;
    let left = v.talk.len().saturating_sub(keep);
    if left > 0 {
        v.talk.drain(..left);
    }
    if left > 0 || v.has_more {
        let n = left.to_string();
        v.earlier = Some(MoreView {
            left: left as u32,
            hub: v.has_more,
            says: say("session.earlier", "Earlier messages", &[("left", &n)]),
            pages: pages + 1,
        });
    }
    if b.lens.calm {
        calm(&mut v);
    }
    Some(v)
}

/// The calm chat (`Focus.calm`: the iPhone, the web): only the words and what waits; the agent at work is `working`
/// (its face), its trail and the cards its lines stand for are not in the talk. A run of card events becomes one row of chips (`kind` "events";
/// an echo of what the asked line already says is left out), the helper list stays off the page (their status lines
/// name them), and the rest of the working lines is "and N more".
fn calm(v: &mut SessionView) {
    // The agent at work: only its face at the chat's end; its turn's trail goes into the details, not the talk.
    let trail = v.talk.iter().rfind(|r| r.kind == "work").and_then(|r| r.work.clone()).filter(|w| w.state == "running");
    let works = v.answering.is_some() || v.status_lines.iter().any(|l| l.state == "working");
    v.working = works.then(|| WorkingView {
        says: v.answering.clone().unwrap_or_else(|| say("session.answering", format!("{} is working", v.name.en), &[("name", v.name.en.as_str())])),
        head: match trail.as_ref().map(|w| w.now.as_str()).filter(|n| !n.is_empty()) {
            Some(n) => say("working.head", format!("Working · {n}"), &[("now", n)]),
            None => say("work.running", "Working", &[]),
        },
        now: trail.as_ref().map(|w| w.now.clone()).unwrap_or_default(),
        trail,
    });
    let mut talk: Vec<TalkRow> = Vec::with_capacity(v.talk.len());
    for mut row in std::mem::take(&mut v.talk) {
        if row.kind == "work" {
            continue;
        }
        if row.kind != "event" {
            row.quote = quote_of(&row);
            talk.push(row);
            continue;
        }
        let Some(ev) = row.event.as_ref() else { continue };
        if ev.echo {
            continue;
        }
        let text = [row.card.as_ref().map(|c| c.title.as_str()).unwrap_or(""), ev.about.as_str(), ev.text.as_str(), ev.label.en.as_str()]
            .into_iter()
            .find(|t| !t.is_empty())
            .unwrap_or("")
            .to_string();
        let says = if text == ev.label.en { ev.label.clone() } else { say("event.chip", format!("{}: {text}", ev.label.en), &[("what", &ev.label.en), ("about", &text)]) };
        let chip = EventChipView { id: row.id.clone(), kind: ev.kind.clone(), text, says, card: row.card.clone() };
        match talk.last_mut() {
            Some(last) if last.kind == "events" && row.day.is_none() => {
                last.time_ms = row.time_ms;
                last.events.push(chip);
            }
            _ => talk.push(TalkRow { id: row.id.clone(), kind: "events".into(), time_ms: row.time_ms, number: row.number, day: row.day.clone(), events: vec![chip], ..Default::default() }),
        }
    }
    // (a run of the agent's words broken by chips starts again after them)
    for i in 1..talk.len() {
        if talk[i - 1].kind == "events" {
            talk[i].cont = false;
        }
    }
    v.talk = talk;
    v.helpers = None;
    v.tabs.retain(|t| t.key != "helpers");
    let behind = v.status_lines.len().saturating_sub(1).to_string();
    v.status_more = (v.status_lines.len() > 1).then(|| say("session.status_more.calm", format!("and {behind} more"), &[("n", behind.as_str())]));
    // (its open questions pinned above the field, the newest first; their rows where they were asked leave the talk)
    let reads = std::mem::take(&mut v.pinned);
    v.pinned = v.waiting.iter().rev().filter(|w| !w.id.trim().is_empty()).cloned().chain(reads).collect();
    let pinned: HashSet<&str> = v.pinned.iter().map(|w| w.id.as_str()).collect();
    v.talk.retain(|r| !(r.kind == "ask" && r.row.as_ref().is_some_and(|w| pinned.contains(w.id.as_str()))));
    if v.first_open.as_deref().is_some_and(|id| !v.talk.iter().any(|r| r.id == id)) {
        v.first_open = None;
    }
}

/// The card a bubble quotes in the calm chat: his words with what he did to a card (a hand-back, What??), or the
/// agent's words or his own on a card. None without words, without a card, or for a card this device does not know
/// (no id, no title: nothing to open or to name).
fn quote_of(row: &TalkRow) -> Option<QuoteView> {
    if !matches!(row.kind.as_str(), "user" | "agent") || row.text.trim().is_empty() {
        return None;
    }
    let card = row.card.as_ref().filter(|c| !c.id.trim().is_empty() && !c.title.trim().is_empty())?;
    let label = row.deed.as_ref().map(|d| d.label.clone()).filter(|l| !l.en.is_empty()).unwrap_or_else(|| say("quote.about", "About", &[]));
    Some(QuoteView { label, card: card.clone() })
}

fn page(b: &Board, id: &str) -> Option<SessionView> {
    let m = b.model;
    let idx = b.by_key.get(id).or_else(|| b.by_id.get(id)).copied()?;
    let a = &b.agents[idx];
    let s = m.sessions.get(&a.row.key);
    let (now, zone) = (b.now, b.lens.utc_offset_min);
    let heard_up_to = s.and_then(|s| s.heard_up_to);
    let name = a.row.name.as_str();

    // ---- the conversation ----
    let cards: Vec<&Card> = m.cards.values().filter(|c| b.by_key.get(key_of(&c.session_id, &c.agent_device_id)) == Some(&idx)).collect();
    let on_stack: HashSet<&str> = m.stack.iter().map(String::as_str).collect();
    let mut entries: Vec<Entry> = Vec::new();
    for c in &cards {
        entries.extend(events_of(b, c));
        if let Some(t) = m.timelines.get(&format!("chat:card/{}", c.object_id)) {
            items_of(b, t, Some(c), heard_up_to, &mut entries);
        }
    }
    let chat = m.timelines.get(&format!("chat:session/{}", a.row.key));
    if let Some(t) = chat {
        items_of(b, t, None, heard_up_to, &mut entries);
    }
    // (a helper session of before: its Chat stands in its main's, its words under the helper's name)
    for (key, name) in b.legacy.iter().filter(|(k, _)| b.by_key.get(*k) == Some(&idx)) {
        if let Some(t) = m.timelines.get(&format!("chat:session/{key}")) {
            let from = entries.len();
            items_of(b, t, None, heard_up_to, &mut entries);
            for e in &mut entries[from..] {
                if !e.row.from_me {
                    e.row.helper = Some(name.clone());
                }
            }
        }
    }
    in_order(&mut entries);
    // An approval it waits for stands at the end of the conversation, as its Desk row (it has no thread of its own).
    let asks: Vec<&str> = b.all_fresh.iter().filter(|f| f.permission && f.agent == Some(idx)).map(|f| f.id.as_str()).collect();
    for p in asks.iter().filter_map(|id| m.permissions.get(*id)) {
        entries.push(Entry {
            seq: UNSENT - 1,
            half: false,
            side: Side::Event,
            card_id: Some(p.object_id.clone()),
            row: TalkRow {
                id: format!("p{}", p.object_id),
                kind: "event".into(),
                time_ms: p.sent_at,
                card: Some(CardRefView { id: p.object_id.clone(), number: b.number_of(&p.object_id), title: format!("Approval: {}", p.tool_name) }),
                event: Some(EventView { kind: "asked".into(), label: event_label_by("asked", &a.row.name), ..Default::default() }),
                ..Default::default()
            },
        });
    }
    let is_asked = |e: &Entry| e.row.event.as_ref().is_some_and(|ev| ev.kind == "asked") && e.side == Side::Event;
    // Where a question stands in the conversation: at the last time it was put there.
    let mut ask_at: HashMap<String, String> = HashMap::new();
    for e in entries.iter().filter(|e| is_asked(e)) {
        if let Some(card) = &e.card_id {
            ask_at.insert(card.clone(), e.row.id.clone());
        }
    }
    // (its questions that wait for him: something to read is not a question, and its line in the talk is the
    //  board's; the count says what the page "Questions" lists)
    let mut fresh: Vec<(u64, &str)> = cards
        .iter()
        .filter(|c| waits(c, &on_stack) && !reads_as_info(c))
        .map(|c| (c.first_envelope_number, c.object_id.as_str()))
        .chain(asks.iter().filter_map(|id| m.permissions.get(*id)).map(|p| (p.envelope_number, p.object_id.as_str())))
        .collect();
    fresh.sort();
    let fresh: Vec<&str> = fresh.into_iter().map(|(_, id)| id).collect();
    let waiting: Vec<super::DeskRowView> = fresh
        .iter()
        .filter_map(|id| match (m.cards.get(*id), m.permissions.get(*id)) {
            (Some(c), _) => Some(card_row(b, c, Some(idx))),
            (None, Some(p)) => Some(request_row(b, p, Some(idx))),
            _ => None,
        })
        .collect();

    let has_more = chat.is_some_and(|t| t.has_more);
    let day_of = |ts: u64| local(ts, zone).days;
    // (what the runs need of the row before: its side, typed in the terminal or not, its time, its helper)
    let mut prev: Option<(Side, bool, u64, Option<String>)> = None;
    let mut talk: Vec<TalkRow> = Vec::with_capacity(entries.len());
    let last_said = entries.iter().rfind(|e| e.side != Side::Event).map(|e| (e.side, e.row.time_ms));
    // (its turn still runs: the newest of the talk is a trail that has not ended; the connector streams it)
    let trail_runs = entries.iter().rfind(|e| e.side != Side::Event).is_some_and(|e| e.side == Side::Work && e.row.work.as_ref().is_some_and(|w| w.state == "running"));
    let mine = entries.iter().rfind(|e| e.side == Side::User && !e.row.terminal && e.seq != UNSENT).map(|e| (e.seq, e.row.time_ms));
    for e in entries {
        let Entry { side, card_id, mut row, .. } = e;
        let ts = row.time_ms;
        // (a helper's words start a run of their own: its name stands in the head)
        let run = prev.as_ref().is_some_and(|(p_side, p_terminal, p_ts, p_helper)| {
            *p_side == side && (side != Side::User || *p_terminal == row.terminal) && *p_helper == row.helper && day_of(*p_ts) == day_of(ts) && (side == Side::Event || ts.saturating_sub(*p_ts) < GROUP_GAP_MS)
        });
        row.day = match &prev {
            None => (!has_more).then(|| day_label(ts, now, zone)),
            Some((_, _, p_ts, _)) => (day_of(*p_ts) != day_of(ts)).then(|| day_label(ts, now, zone)),
        };
        prev = Some((side, row.terminal, ts, row.helper.clone()));
        let asked = side == Side::Event && row.event.as_ref().is_some_and(|ev| ev.kind == "asked");
        if asked {
            let id = card_id.as_deref().unwrap_or("");
            let here = ask_at.get(id) == Some(&row.id);
            if here && fresh.contains(&id) {
                // Open and waiting for him: its Desk row (the tiles answer, the words open the card's page).
                let desk_row = match (m.cards.get(id), m.permissions.get(id)) {
                    (Some(c), _) => Some(card_row(b, c, Some(idx))),
                    (None, Some(p)) => Some(request_row(b, p, Some(idx))),
                    _ => None,
                };
                if let Some(r) = desk_row {
                    row.kind = "ask".into();
                    row.event = None;
                    row.row = Some(Box::new(r));
                }
            } else if let Some(c) = m.cards.get(id) {
                // Anything else: one quiet line saying what became of it.
                let title = shown_title(c);
                let (kind, text) = if here { outcome(c) } else { ("asked", String::new()) };
                let asked_as = row.event.as_ref().map(|ev| ev.text.clone()).unwrap_or_default();
                row.event = Some(EventView {
                    kind: kind.into(),
                    label: event_label_by(if kind == "done" && c.object_state == "closed" && c.field("close_summary").as_str().unwrap_or("").is_empty() && !c.field("withdraw_reason").as_str().unwrap_or("").is_empty() { "withdrawn" } else { kind }, &a.row.name),
                    about: if title.is_empty() { asked_as } else { title.clone() },
                    text: if text == title { String::new() } else { text },
                    echo: false,
                    ..Default::default()
                });
            }
        } else if side == Side::Event {
            // What became of a question (answered, done, shredded) is said by its line where it was asked.
            if let Some(ev) = row.event.as_mut() {
                ev.echo = matches!(ev.kind.as_str(), "decided" | "done" | "shredded") && card_id.as_deref().is_some_and(|c| ask_at.contains_key(c));
            }
            row.cont = run;
        } else {
            row.cont = run;
        }
        talk.push(row);
    }
    if !talk.is_empty() {
        b.wake_at(Some(next_midnight(now, zone)));
    }

    // ---- the heading ----
    let link = link_of(b, idx);
    let late = link.as_ref().filter(|l| l.state != "live");
    let hand = hand_of(b, idx);
    let quiet = match (hand.is_none() && !a.row.archived && a.row.online, a.row.quiet_since) {
        (true, Some(since)) => {
            let (text, wake) = quiet_words(since, now);
            b.wake_at(Some(wake));
            Some(text)
        }
        _ => None,
    };
    let profile = s.and_then(|s| s.profile.as_ref());
    let of_profile = |k: &str| profile.and_then(|p| p.get(k)).and_then(Value::as_str).unwrap_or("").to_string();
    let word = match late.and_then(|l| l.word.clone()) {
        Some(w) => w,
        None if a.row.online => say("session.connected", "connected", &[]),
        None => say("session.disconnected", "disconnected", &[]),
    };
    let task = if a.row.online && late.is_none() { of_profile("task") } else { String::new() };
    let dot = DotView {
        on: a.row.online,
        link: late.map(|l| l.state.clone()).unwrap_or_default(),
        says: if task.is_empty() { word.clone() } else { say("session.dot", format!("{} · {task}", word.en), &[("word", &word.en), ("task", &task)]) },
        word,
    };

    // ---- above the field ----
    // (his last message on the board, and whether its session picked it up)
    let unheard = match (mine, heard_up_to) {
        (Some((seq, ts)), Some(mark)) if seq > mark => {
            if now.saturating_sub(ts) >= UNHEARD_MS {
                true
            } else {
                b.wake_at(Some(ts + UNHEARD_MS));
                false
            }
        }
        _ => false,
    };
    let receipt = unheard.then(|| say("session.unheard", format!("{name} has not picked up your last message."), &[("name", name)]));
    let note = match link.as_ref().filter(|l| matches!(l.state.as_str(), "cut" | "gone" | "asleep")) {
        Some(l) => Some(link_note(l, receipt)),
        None if unheard => Some(LinkNoteView { tone: "unheard".into(), receipt, line: None, fix: Some(unheard_fix(a.codex)) }),
        None => None,
    };
    let compose = match link.as_ref().map(|l| l.state.as_str()) {
        Some("cut") => say("compose.cut", "It cannot hear you right now. What you write waits for it", &[]),
        Some("gone") => say("compose.gone", "It is gone. What you write waits for it", &[]),
        Some("asleep" | "oncall") if a.codex => say("compose.next_turn", "Reaches the agent on its next turn", &[]),
        Some("asleep" | "oncall") => say("compose.next_step", "Reaches the agent on its next step", &[]),
        _ => say("compose", "Message to the agent", &[]),
    };
    // (a session that is gone, or cut off from its tools, does not work on what he wrote, and runs nothing)
    let link_state = link.as_ref().map(|l| l.state.as_str()).unwrap_or("");
    let gone = link_state == "gone";
    let answering = match last_said {
        _ if gone || link_state == "cut" => None,
        // (a turn that runs: working as long as its trail runs and the session is connected, however long it takes)
        Some((Side::Work, _)) if trail_runs && a.row.online => Some(say("session.answering", format!("{name} is working"), &[("name", name)])),
        Some((Side::User, ts)) if now.saturating_sub(ts) < WORKING_WINDOW_MS => {
            b.wake_at(Some(ts + WORKING_WINDOW_MS));
            Some(say("session.answering", format!("{name} is working"), &[("name", name)]))
        }
        _ => None,
    };

    // ---- the status lines ----
    let lines = s.map(|s| s.status_lines.as_slice()).unwrap_or(&[]);
    let state_of = |id: &str| a.row.lines.get(id).map(String::as_str).unwrap_or("");
    let names_card = |t: &crate::model::types::StatusLine| t.object_id.as_str().filter(|x| !x.is_empty()).map(String::from);
    // (the card a working line that names none stands for: the newest of the session's cards with it whose line it
    //  is, by the Desk's own pairing, off_mind.rs: the line that names a card, else the newest that names none)
    let working: Vec<&crate::model::types::StatusLine> = lines.iter().filter(|t| state_of(&t.id) == "working").collect();
    let loose = working.iter().filter(|t| names_card(t).is_none()).fold(None::<&crate::model::types::StatusLine>, |best, t| match best {
        Some(x) if x.updated_at >= t.updated_at => Some(x),
        _ => Some(t),
    });
    let with_it = || {
        cards
            .iter()
            .filter(|c| card_place(c, b.runs(idx), heard_up_to, now).0 == Some("works"))
            .filter(|c| !working.iter().any(|t| names_card(t).as_deref() == Some(c.object_id.as_str())))
            .max_by_key(|c| went_at(m, c))
            .copied()
    };
    let status_lines: Vec<StatusLineView> = lines
        .iter()
        .filter(|_| !gone)
        .filter_map(|t| {
            let state = state_of(&t.id);
            let age = now.saturating_sub(t.updated_at);
            let (says, until) = match state {
                "working" => (say("line.working", "working", &[]), None),
                "decision" => (say("line.decision", "waits on you", &[]), None),
                "later" => (say("line.later", "put off", &[]), None),
                "done" if age < WORKING_WINDOW_MS => (say("line.done", "done", &[]), Some(t.updated_at + WORKING_WINDOW_MS)),
                "stale" if age < STALE_SHOWN_MS => (say("line.stale", "no news for a while", &[]), Some(t.updated_at + STALE_SHOWN_MS)),
                _ => return None,
            };
            b.wake_at(until);
            let card = match names_card(t) {
                Some(id) => m.cards.get(&id),
                None if state == "working" && loose.is_some_and(|l| l.id == t.id) => with_it(),
                None => None,
            };
            Some(StatusLineView {
                id: t.id.clone(),
                helper: t.helper.clone(),
                label: t.label.as_str().unwrap_or("").to_string(),
                detail: t.detail.as_str().unwrap_or("").to_string(),
                state: state.into(),
                says,
                updated_ms: t.updated_at,
                card: card.map(|c| card_ref(b, c)),
            })
        })
        .collect();
    // (the line set last first: the foot shows it alone; a sort that keeps the agent's order between equal times)
    let mut status_lines = status_lines;
    status_lines.sort_by_key(|l| std::cmp::Reverse(l.updated_ms));
    let behind = status_lines.len().saturating_sub(1).to_string();
    let status_more = (status_lines.len() > 1).then(|| say("session.status_more", format!("+{behind}"), &[("n", behind.as_str())]));

    // (the calm chat: a card a status line that shows names is said by that line, in the details: no chip of it)
    if b.lens.calm {
        let named: HashSet<String> = lines.iter().filter(|t| status_lines.iter().any(|l| l.id == t.id)).filter_map(names_card).collect();
        talk.retain(|r| !(r.kind == "event" && r.card.as_ref().is_some_and(|c| named.contains(&c.id))));
    }
    let open = waiting.len() as u32;
    let n = open.to_string();
    let account_of = |key: &str| m.sessions.get(key).and_then(|s| s.profile.as_ref()).and_then(|p| p.get("account")).filter(|v| v.is_object());
    let usage = account_of(&a.row.key).and_then(|acc| super::settings::usage_of(acc, now));
    Some(SessionView {
        usage,
        helpers: helper_list(b, idx),
        archived: a.row.archived,
        tabs: Vec::new(),
        questions: Vec::new(),
        files: Vec::new(),
        id: a.row.key.clone(),
        name: say("session.name", name, &[]),
        link: link.as_ref().map(|l| l.state.clone()).unwrap_or_default(),
        blocked: hand.is_some(),
        // (the welcome only for a session whose Chat the hub holds nothing of: none while its history is read)
        empty: (talk.is_empty() && b.lens.talk_waits.as_deref() != Some(a.row.key.as_str())).then(|| EmptyTalkView {
            meta: a.row.model_label.clone(),
            text: say("session.empty", "Joined and waiting for its first task. Write below, or start with:", &[]),
            starters: vec![
                say("session.starter.stand", "Where do we stand?", &[]),
                say("session.starter.need", "What do you need from me?", &[]),
                say("session.starter.sum", "Sum up what you did last.", &[]),
            ],
        }),
        waiting,
        first_open: fresh.iter().find_map(|id| ask_at.get(*id)).filter(|row| talk.iter().any(|r| &r.id == *row && r.kind == "ask")).cloned(),
        talk,
        status_lines,
        status_more,
        wake_line: super::wakeups::line(b, &a.row.key),
        working: None,
        // (the calm chat: its cards to read that wait on the Desk, the newest first, never a task it still works on;
        //  calm() puts its questions before them)
        pinned: if b.lens.calm {
            let mut reads: Vec<&Card> = cards
                .iter()
                .copied()
                .filter(|c| waits(c, &on_stack) && reads_as_info(c) && !c.is_unsupported() && !super::board::is_loading(c) && c.field("working") != Value::Bool(true))
                .collect();
            reads.sort_by_key(|c| std::cmp::Reverse(c.first_envelope_number));
            reads.into_iter().map(|c| card_row(b, c, Some(idx))).collect()
        } else {
            Vec::new()
        },
        has_more,
        earlier: None,
        slug: a.row.id.clone(),
        mark: a.row.mark(),
        team_id: a.team.clone().unwrap_or_default(),
        main: a.row.main,
        crowned: a.crowned,
        online: a.row.online,
        path: a.row.path.clone(),
        model_label: a.row.model_label.clone(),
        model: of_profile("model"),
        dot,
        hand,
        quiet,
        note,
        compose,
        answering,
        open,
        open_word: (open > 0).then(|| say("session.open", format!("{n} open"), &[("n", &n)])),
        open_says: (open > 0).then(|| {
            if open == 1 {
                say("session.open.one", "One open question: go to it", &[("n", "1")])
            } else {
                say("session.open.some", format!("{n} open questions: go to the first one"), &[("n", &n)])
            }
        }),
    })
}

/// A session's helpers on its page: "3 helpers", "1 working · their cards come with this session", a row each.
fn helper_list(b: &Board, idx: usize) -> Option<HelperListView> {
    let list = b.helpers(idx);
    if list.is_empty() {
        return None;
    }
    let online = b.agents[idx].row.online;
    let n = list.len().to_string();
    let working = list.iter().filter(|h| h.working && online).count();
    let w = working.to_string();
    let rows = list
        .iter()
        .map(|h| {
            let works = h.working && online;
            let q = h.waiting.to_string();
            let open = match h.waiting {
                0 => None,
                1 => Some("1 open question".to_string()),
                k => Some(format!("{k} open questions")),
            };
            let says = match (works, open) {
                (true, Some(o)) => say("helper.working.open", format!("Working · {o}"), &[("n", &q)]),
                (true, None) => say("helper.working", "Working", &[]),
                (false, Some(o)) => say("helper.open", o, &[("n", &q)]),
                (false, None) => say("helper.idle", "Not working", &[]),
            };
            HelperRowView { name: h.name.clone(), working: works, says }
        })
        .collect();
    Some(HelperListView {
        head: if list.len() == 1 { say("session.helpers.one", "1 helper", &[("n", "1")]) } else { say("session.helpers", format!("{n} helpers"), &[("n", &n)]) },
        sub: say("helpers.sub", format!("{w} working · their cards come with this session"), &[("n", &w)]),
        rows,
    })
}

/// The session's questions (every card, newest first) and its files (the attachments of its cards, their answers,
/// its Chats and its Artifacts, newest first, each once).
fn piles(b: &Board, v: &mut SessionView) {
    let m = b.model;
    let Some(&idx) = b.by_key.get(v.id.as_str()).or_else(|| b.by_id.get(v.id.as_str())) else { return };
    let mut cards: Vec<&Card> = m.cards.values().filter(|c| b.by_key.get(key_of(&c.session_id, &c.agent_device_id)) == Some(&idx)).collect();
    cards.sort_by_key(|c| std::cmp::Reverse(c.first_envelope_number));
    v.questions = cards
        .iter()
        .map(|c| super::CardRefView { id: c.object_id.clone(), number: b.number_of(&c.object_id), title: c.fields.get("title").as_str().unwrap_or("").to_string() })
        .collect();
    let mut found: Vec<(u64, String)> = Vec::new();
    let mut take = |at: u64, list: &serde_json::Value| {
        for a in list.as_array().into_iter().flatten() {
            super::files_seen::saw(a);
            if let Some(id) = a.get("attachment_id").and_then(serde_json::Value::as_str) {
                found.push((at, id.to_string()));
            }
        }
    };
    for c in &cards {
        take(c.first_envelope_number, &c.fields.get("attachments"));
        for a in c.answers.iter() {
            take(a.envelope_number.unwrap_or(0), &a.attachments);
        }
        for key in [format!("chat:card/{}", c.object_id)] {
            if let Some(t) = m.timelines.get(&key) {
                for it in t.items.values() {
                    let at = it.get("envelope_number").and_then(serde_json::Value::as_u64).unwrap_or(0);
                    take(at, it.get("content").and_then(|c| c.get("attachments")).unwrap_or(&serde_json::Value::Null));
                }
            }
        }
    }
    if let Some(t) = m.timelines.get(&format!("chat:session/{}", b.agents[idx].row.key)) {
        for it in t.items.values() {
            let at = it.get("envelope_number").and_then(serde_json::Value::as_u64).unwrap_or(0);
            take(at, it.get("content").and_then(|c| c.get("attachments")).unwrap_or(&serde_json::Value::Null));
        }
    }
    for p in m.published.values().filter(|p| b.by_key.get(key_of(&p.session_id, &p.agent_device_id)) == Some(&idx)) {
        take(p.envelope_number, &p.attachments);
    }
    found.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    let mut seen = std::collections::HashSet::new();
    v.files = found.into_iter().filter(|(_, id)| seen.insert(id.clone())).map(|(_, id)| id).collect();
    v.tabs = panel_tabs(v);
}

/// The tabs of a session's side panel: Details always; Questions, Helpers and Files where it has some, each with
/// what it counts (the questions that wait for him, the helpers, the files).
fn panel_tabs(v: &SessionView) -> Vec<PanelTabView> {
    let tab = |key: &str, label: &str, count: usize| PanelTabView { key: key.into(), label: say(&format!("panel.{key}"), label, &[]), count: count as u32 };
    let mut tabs = vec![tab("details", "Details", 0)];
    if !v.questions.is_empty() || v.open > 0 {
        tabs.push(tab("questions", "Questions", v.open as usize));
    }
    if let Some(h) = &v.helpers {
        tabs.push(tab("helpers", "Helpers", h.rows.len()));
    }
    if !v.files.is_empty() {
        tabs.push(tab("files", "Files", v.files.len()));
    }
    tabs
}

/// Every Artifact this device holds, the newest first, with its session's name and its files.
pub fn artifacts(b: &Board) -> Vec<super::ArtifactView> {
    let m = b.model;
    let mut list: Vec<&crate::model::types::Published> = m.published.values().filter(|p| p.object_state != "closed").collect();
    list.sort_by_key(|p| std::cmp::Reverse((p.sent_at, p.envelope_number)));
    list.into_iter()
        .map(|p| {
            let idx = b.by_key.get(key_of(&p.session_id, &p.agent_device_id)).copied();
            let mut files = Vec::new();
            for a in p.attachments.as_array().into_iter().flatten() {
                super::files_seen::saw(a);
                if let Some(id) = a.get("attachment_id").and_then(serde_json::Value::as_str) {
                    files.push(id.to_string());
                }
            }
            super::ArtifactView {
                id: p.object_id.clone(),
                title: p.title.clone(),
                kind: p.artifact_type.as_ref().and_then(serde_json::Value::as_str).unwrap_or("").to_string(),
                session_id: idx.map(|i| b.agents[i].row.key.clone()),
                session: idx.map(|i| b.agents[i].row.name.clone()).unwrap_or_default(),
                sent_at: p.sent_at,
                files,
            }
        })
        .collect()
}

/// A page: an Artifact whose first file is HTML (the web's gallery filter, `media.mjs` `typeOf`).
fn is_page(b: &Board, a: &super::ArtifactView) -> bool {
    let Some(first) = a.files.first() else { return false };
    b.model.published.get(&a.id).and_then(|p| p.attachments.as_array()).into_iter().flatten()
        .find(|x| x.get("attachment_id").and_then(serde_json::Value::as_str) == Some(first.as_str()))
        .and_then(|x| x.get("media_type")).and_then(serde_json::Value::as_str) == Some("text/html")
}

/// The gallery's filter over every Artifact (`all`, the newest first): its words, and what it lets through.
/// `kind`: "" every one, "media" the pictures, videos and files, "pages" the pages; another word counts as "".
pub fn gallery(b: &Board, kind: &str, all: Vec<super::ArtifactView>) -> (super::GalleryView, Vec<super::ArtifactView>) {
    let kind = if matches!(kind, "media" | "pages") { kind } else { "" };
    let pages = all.iter().filter(|a| is_page(b, a)).count() as u32;
    let total = all.len() as u32;
    let kinds = if total == 0 {
        Vec::new()
    } else {
        [("", say("gallery.all", "All", &[]), total), ("media", say("gallery.media", "Media", &[]), total - pages), ("pages", say("gallery.pages", "Pages", &[]), pages)]
            .into_iter()
            .map(|(key, label, count)| super::GalleryKindView { key: key.into(), label, count, on: key == kind })
            .collect()
    };
    let shown: Vec<super::ArtifactView> = all.into_iter().filter(|a| kind.is_empty() || is_page(b, a) == (kind == "pages")).collect();
    let lead = if total == 0 {
        say("gallery.lead.empty", "Nothing yet: pictures, videos, pages and files your agents send show up here.", &[])
    } else {
        say("gallery.lead", "Every picture, video, file and page your agents made, the newest first.", &[])
    };
    let none = (total > 0 && shown.is_empty()).then(|| {
        if kind == "pages" { say("gallery.none.pages", "No pages yet.", &[]) } else { say("gallery.none.media", "No pictures, videos or files yet.", &[]) }
    });
    (super::GalleryView { kind: kind.into(), kinds, lead, none }, shown)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{dev, Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::json;

    fn view_at(r: &Room, of: &str, now: u64) -> (SessionView, Option<u64>) {
        let m = r.model(now);
        let lens = Lens::default();
        let b = Board::new(&m, &lens, now);
        let v = session(&b, &dev(of)).expect("the session is on the board");
        (v, b.wake())
    }
    fn view(r: &Room, of: &str) -> SessionView {
        view_at(r, of, NOW).0
    }
    fn en(t: &Option<Text>) -> &str {
        t.as_ref().map(|t| t.en.as_str()).unwrap_or("")
    }
    fn kinds(v: &SessionView) -> Vec<(&str, &str)> {
        v.talk.iter().map(|r| (r.id.as_str(), r.kind.as_str())).collect()
    }
    fn event(r: &TalkRow) -> (&str, &str, &str, &str) {
        let e = r.event.as_ref().expect("an event's line");
        (e.kind.as_str(), e.label.en.as_str(), e.about.as_str(), e.text.as_str())
    }
    /// A long talk shows its newest page; each "more" adds one; the line above says what stands before.
    #[test]
    fn a_long_talk_shows_a_window_of_pages() {
        let mut r = Room::default();
        r.session("a", None, true);
        for n in 0..100u64 {
            r.say("a", None, 1000 + n, if n % 2 == 0 { "a" } else { "me" }, (100 - n) * MIN, json!({ "text": format!("m{n}") }));
        }
        let m = r.model(NOW);
        let mut lens = Lens::default();
        let at = |lens: &Lens| session(&Board::new(&m, lens, NOW), &dev("a")).unwrap();
        let v = at(&lens);
        assert_eq!(v.talk.len(), TALK_PAGE);
        assert_eq!(v.talk.last().unwrap().text, "m99");
        let e = v.earlier.clone().unwrap();
        assert_eq!((e.left, e.hub, e.says.en.as_str(), e.pages), (60, false, "Earlier messages", 2));
        lens.show_more(crate::views::MoreOf::Talk { session: dev("a") });
        let v = at(&lens);
        assert_eq!((v.talk.len(), v.earlier.as_ref().map(|e| (e.left, e.pages))), (80, Some((20, 3))));
        lens.show_more(crate::views::MoreOf::Talk { session: dev("a") });
        let v = at(&lens);
        assert_eq!((v.talk.len(), v.talk[0].text.as_str()), (100, "m0"));
        assert!(v.earlier.is_none());
        // another session's window is its own
        lens.show_more(crate::views::MoreOf::Talk { session: dev("b") });
        assert_eq!(at(&lens).talk.len(), TALK_PAGE);
    }

    fn msg(text: &str) -> Value {
        json!({ "text": text })
    }

    #[test]
    fn the_day_above_a_row_is_the_persons() {
        assert_eq!(day_label(NOW - MIN, NOW, 0).en, "Today");
        assert_eq!(day_label(NOW - 20 * 60 * MIN, NOW, 0).en, "Yesterday");
        assert_eq!(day_label(NOW - 3 * 24 * 60 * MIN, NOW, 0).en, "Saturday 10 October");
        // (13 October 14:05 UTC; a message of 23:30 UTC the day before is already today in Berlin)
        assert_eq!(day_label(NOW - (14 * 60 + 35) * MIN, NOW, 120).en, "Today");
    }

    #[test]
    fn rows_of_one_side_run_within_five_minutes_of_a_day() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.say("a", None, 10, "me", 3 * 24 * 60 * MIN, msg("Morning."));
        r.say("a", None, 11, "a", 30 * MIN, msg("On it."));
        r.say("a", None, 12, "a", 28 * MIN, msg("Done."));
        r.say("a", None, 13, "a", 20 * MIN, msg("One more thing."));
        r.say("a", None, 14, "me", 19 * MIN, msg("Go on."));
        // (typed into its terminal: his words, sent by the agent's connector; never in a run with what he wrote here)
        r.say("a", None, 15, "a", 18 * MIN, json!({ "text": "and hurry", "terminal": "input" }));
        r.say("a", None, 16, "a", 17 * MIN, json!({ "text": "Hurrying.", "terminal": "answer" }));
        let v = view(&r, "a");
        let rows: Vec<(&str, &str, bool, bool, bool, &str)> = v.talk.iter().map(|x| (x.id.as_str(), x.kind.as_str(), x.from_me, x.cont, x.terminal, en(&x.day))).collect();
        assert_eq!(
            rows,
            [
                ("e10", "user", true, false, false, "Saturday 10 October"),
                ("e11", "agent", false, false, false, "Today"),
                ("e12", "agent", false, true, false, ""),
                ("e13", "agent", false, false, false, ""),
                ("e14", "user", true, false, false, ""),
                ("e15", "user", true, false, true, ""),
                ("e16", "agent", false, false, true, ""),
            ]
        );
        assert_eq!((v.talk[0].text.as_str(), v.talk[0].number, v.talk[0].time_ms), ("Morning.", 10, NOW - 3 * 24 * 60 * MIN));
        assert!(v.empty.is_none() && !v.has_more);
    }

    #[test]
    fn a_question_stands_where_it_was_asked_as_its_desk_row_or_one_quiet_line() {
        let mut r = Room::default();
        r.session("a", None, true);
        let history = |n: u64, title: &str| json!([{ "object_version": 1, "envelope_number": n, "sent_at": NOW - 10 * MIN, "object_state": "open", "content": { "title": title } }]);
        let c = r.card("q-open", "a", "decision", "normal");
        c["options"] = json!([{ "key": "y", "label": "Yes" }, { "key": "n", "label": "No" }]);
        c["versions"] = history(100, "q-open");
        r.card("q-read", "a", "info", "normal")["versions"] = history(101, "q-read");
        let c = r.card("q-back", "a", "decision", "normal");
        c["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        c["versions"] = history(102, "q-back");
        r.card("q-later", "a", "decision", "normal")["versions"] = history(103, "q-later");
        r.snoozes = json!({ "q-later": { "until": NOW + 60 * MIN } });
        r.answered("q-done", "a", NOW - 5 * MIN, 150);
        let c = r.cards.last_mut().unwrap();
        c["options"] = json!([{ "key": "a", "label": "Ship it" }]);
        c["versions"] = history(104, "q-done");
        c["answers"] = json!([c["answer"].clone()]);
        r.request("p1", "a", "ok");
        r.say("a", None, 99, "a", 30 * MIN, msg("Three things for you."));
        let v = view(&r, "a");
        assert_eq!(kinds(&v), [("e99", "agent"), ("v100a", "ask"), ("v101i", "event"), ("v102a", "event"), ("v103a", "event"), ("v104a", "event"), ("v150d", "event"), ("pp1", "ask")]);
        // (open and waiting: its Desk row, with its answers)
        let ask = v.talk[1].row.as_ref().unwrap();
        assert_eq!((ask.id.as_str(), ask.tiles.kind.as_str(), v.talk[1].card.as_ref().unwrap().number), ("q-open", "pair", 1));
        assert_eq!((v.talk[7].row.as_ref().unwrap().kind.as_str(), v.talk[7].card.as_ref().unwrap().number), ("request", 6));
        // (something to read is the board's line; it is no open question)
        assert_eq!(event(&v.talk[2]), ("info", "a wrote", "", "q-read"));
        assert_eq!(event(&v.talk[3]), ("handed", "With the agent", "q-back", ""));
        assert_eq!(event(&v.talk[4]), ("snoozed", "Later", "q-later", ""));
        assert_eq!(event(&v.talk[5]), ("decided", "You chose", "q-done", "Ship it"));
        // (the answer's own event only repeats what the question's line says)
        assert_eq!((event(&v.talk[6]), v.talk[6].event.as_ref().unwrap().echo), (("decided", "You chose", "q-done", "Ship it"), true));
        assert_eq!((v.open, en(&v.open_word), en(&v.open_says), v.first_open.as_deref()), (2, "2 open", "2 open questions: go to the first one", Some("v100a")));
        // (the page "Questions" lists as open exactly what the count counts)
        assert_eq!(v.waiting.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["q-open", "p1"]);
    }

    /// The calm chat (the iPhone, the web): a run of card events is one row of chips (an echo left out), the agent's words
    /// after it start a run again, the helper list stays off the page and the rest of the working lines is "and N more".
    #[test]
    fn the_calm_chat_folds_card_events_into_chips_and_leaves_the_helpers_out() {
        let mut r = Room::default();
        r.session("a", None, true)["status_lines"] = json!([
            { "id": "l1", "label": "iPhone Desk", "state": "working", "helper": "iOS", "updated_at": NOW - MIN },
            { "id": "l2", "label": "Required check", "state": "working", "helper": "CI", "updated_at": NOW - 2 * MIN },
            { "id": "l3", "label": "Connector", "state": "working", "updated_at": NOW - 3 * MIN },
        ]);
        let history = |n: u64, title: &str| json!([{ "object_version": 1, "envelope_number": n, "sent_at": NOW - 10 * MIN, "object_state": "open", "content": { "title": title } }]);
        r.card("q-read", "a", "info", "normal")["versions"] = history(101, "q-read");
        let c = r.card("q-back", "a", "decision", "normal");
        c["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        c["versions"] = history(102, "q-back");
        r.answered("q-done", "a", NOW - 5 * MIN, 150);
        let c = r.cards.last_mut().unwrap();
        c["options"] = json!([{ "key": "a", "label": "Ship it" }]);
        c["versions"] = history(104, "q-done");
        c["answers"] = json!([c["answer"].clone()]);
        r.card("q-i", "a", "decision", "normal")["helper"] = json!("iOS");
        r.say("a", None, 99, "a", 30 * MIN, msg("Three things for you."));
        r.say("a", None, 160, "a", 4 * MIN, msg("Both started."));
        r.say("a", None, 161, "a", 4 * MIN, msg("I need you in the terminal."));
        let wide = view(&r, "a");
        assert!(wide.helpers.is_some() && wide.talk.iter().any(|t| t.kind == "event"), "without calm (Linux) the lines and the helper list stay");
        let m = r.model(NOW);
        let lens = Lens { calm: true, ..Default::default() };
        let v = session(&Board::new(&m, &lens, NOW), &dev("a")).unwrap();
        let rows: Vec<(&str, &str, bool)> = v.talk.iter().map(|t| (t.id.as_str(), t.kind.as_str(), t.cont)).collect();
        assert_eq!(rows.iter().filter(|r| r.1 == "event").count(), 0, "{rows:?}");
        let folded: Vec<&TalkRow> = v.talk.iter().filter(|t| t.kind == "events").collect();
        assert_eq!(folded.len(), 1, "one run, one row: {rows:?}");
        let chips: Vec<(&str, &str, &str)> = folded[0].events.iter().map(|c| (c.kind.as_str(), c.text.as_str(), c.says.en.as_str())).collect();
        assert_eq!(chips, [("info", "q-read", "a wrote: q-read"), ("handed", "q-back", "With the agent: q-back"), ("decided", "q-done", "You chose: q-done")]);
        assert!(folded[0].events.iter().all(|c| c.card.is_some()), "each chip opens its card");
        let at = v.talk.iter().position(|t| t.kind == "events").unwrap();
        assert_eq!((v.talk[at + 1].text.as_str(), v.talk[at + 1].cont, v.talk[at + 2].cont), ("Both started.", false, true), "the words after the chips start a run");
        assert!(v.helpers.is_none() && !v.tabs.iter().any(|t| t.key == "helpers"));
        assert_eq!(v.status_lines.iter().map(|l| l.helper.as_deref()).collect::<Vec<_>>(), [Some("iOS"), Some("CI"), None], "the lines still name the helpers");
        let more = v.status_more.expect("two behind the first");
        assert_eq!((more.key.as_str(), more.en.as_str()), ("session.status_more.calm", "and 2 more"));
    }

    /// The count of open questions is what the page "Questions" lists as open: cards to read that wait on the Desk
    /// count for nothing, and a question whose line is not in the talk (not read yet, or before the window) counts
    /// and is listed.
    #[test]
    fn the_open_count_is_the_open_questions_listed() {
        let mut r = Room::default();
        r.session("a", None, false);
        let history = |n: u64, title: &str| json!([{ "object_version": 1, "envelope_number": n, "sent_at": NOW - 10 * MIN, "object_state": "open", "content": { "title": title } }]);
        // (two infos, done and working, on the Desk: no question)
        r.card("done", "a", "info", "normal")["versions"] = history(101, "Done: two new bikes");
        r.card("works", "a", "info", "normal")["versions"] = history(102, "Working on: used bikes");
        let v = view(&r, "a");
        assert_eq!((v.open, v.open_word.is_none(), v.open_says.is_none(), v.first_open.is_none(), v.waiting.is_empty()), (0, true, true, true, true));
        assert_eq!(v.questions.len(), 2, "both are in the list of every question");
        // (a decision with no line in the talk: counted and listed all the same)
        r.card("q", "a", "decision", "normal");
        let v = view(&r, "a");
        assert_eq!((v.open, en(&v.open_word), v.waiting.len(), v.waiting[0].id.as_str()), (1, "1 open", 1, "q"));
    }

    /// A session that is gone works on nothing: neither his last word ("is working") nor a status line it left
    /// running (a card of two days ago); cut off, it does not work on his word either.
    #[test]
    fn a_gone_session_is_not_working() {
        let line = json!([{ "id": "w", "label": "Used bikes", "state": "working", "detail": "", "object_id": "works", "updated_at": NOW - 48 * 60 * MIN, "envelope_number": 1 }]);
        let mut r = Room::default();
        r.session("a", None, false)["status_lines"] = line.clone();
        r.card("works", "a", "info", "normal")["working"] = json!(true);
        r.say("a", None, 41, "me", MIN, msg("hi"));
        let v = view(&r, "a");
        assert_eq!(v.link, "gone");
        assert_eq!((v.answering.as_ref(), v.status_lines.len()), (None, 0));
        assert!(v.note.is_some(), "it says that it is gone instead");
        // (connected, the same session works on his word and shows its line)
        let mut r = Room::default();
        r.session("a", None, true)["status_lines"] = line;
        r.card("works", "a", "info", "normal")["working"] = json!(true);
        r.say("a", None, 41, "me", MIN, msg("hi"));
        let v = view(&r, "a");
        assert_eq!((en(&v.answering), v.status_lines.len()), ("a is working", 1));
        // (cut off: it cannot hear him, so it is not working on what he wrote)
        let mut r = Room::default();
        r.session("b", None, false)["link"] = json!({ "hears": "live", "attached": true, "exit": { "reason": "stdin", "claude": "alive" } });
        r.say("b", None, 41, "me", MIN, msg("hi"));
        let v = view(&r, "b");
        assert_eq!((v.link.as_str(), v.answering.as_ref()), ("cut", None));
    }

    #[test]
    fn the_heading_has_the_accounts_rings_and_the_archive() {
        let mut r = Room::default();
        r.session("a", None, true)["profile"]["account"] = json!({ "plan": "max", "email": "a@b.c", "five_hour": { "used": 42, "resets_at": NOW + 60 * MIN }, "weekly": { "used": 85.4 } });
        r.session("b", None, false);
        r.settings.insert(dev("b"), json!({ "archived": true }));
        let main = view(&r, "a");
        let usage = main.usage.clone().expect("its account");
        assert_eq!(usage.plan, "Claude Max");
        assert_eq!(usage.meters.iter().map(|m| (m.key.as_str(), m.used, m.hot)).collect::<Vec<_>>(), [("five_hour", 42, false), ("weekly", 85, true)]);
        assert!(!main.archived);
        assert!(view(&r, "b").archived);
    }

    /// The chat's foot shows one line: the status line set last comes first, the rest counted behind it as "+N".
    #[test]
    fn the_line_set_last_comes_first_and_the_rest_are_counted() {
        let mut r = Room::default();
        r.session("a", None, true)["status_lines"] = json!([
            { "id": "old", "label": "Older", "state": "working", "updated_at": NOW - 9 * MIN },
            { "id": "new", "label": "Newest", "state": "working", "helper": "Icons", "updated_at": NOW - MIN },
            { "id": "mid", "label": "Middle", "state": "working", "updated_at": NOW - 4 * MIN },
        ]);
        let v = view(&r, "a");
        let ids: Vec<&str> = v.status_lines.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ids, ["new", "mid", "old"]);
        let more = v.status_more.expect("two behind the first");
        assert_eq!((more.key.as_str(), more.en.as_str()), ("session.status_more", "+2"));
        r.session("a", None, true)["status_lines"] = json!([{ "id": "one", "label": "Alone", "state": "working", "updated_at": NOW - MIN }]);
        assert_eq!(view(&r, "a").status_more, None, "one line: nothing behind it");
    }

    /// Its helpers stand in a short list: name and state; a helper's status line names it, its words carry its name;
    /// a helper session of before is one of them, its Chat in its main's.
    #[test]
    fn the_helpers_stand_in_the_session() {
        let mut r = Room::default();
        r.session("a", None, true)["status_lines"] = json!([
            { "id": "l1", "label": "Tab bar icons", "state": "working", "helper": "Icons", "updated_at": NOW - MIN },
            { "id": "l2", "label": "Own work", "state": "working", "updated_at": NOW - MIN },
        ]);
        r.card("q1", "a", "decision", "normal")["helper"] = json!("Icons");
        r.say("a", None, 41, "a", 3 * MIN, json!({ "text": "Tabs drawn.", "helper": "Icons" }));
        r.session("old", Some("a"), false);
        r.card("q2", "old", "decision", "normal");
        r.say("old", None, 42, "old", 2 * MIN, json!({ "text": "From before." }));
        let v = view(&r, "a");
        let h = v.helpers.clone().expect("its helpers");
        assert_eq!((h.head.en.as_str(), h.sub.en.as_str()), ("2 helpers", "1 working · their cards come with this session"));
        let rows: Vec<(&str, bool, &str)> = h.rows.iter().map(|x| (x.name.as_str(), x.working, x.says.en.as_str())).collect();
        assert!(rows.contains(&("Icons", true, "Working · 1 open question")), "{rows:?}");
        assert!(rows.contains(&("old", false, "1 open question")), "{rows:?}");
        let labels: Vec<(Option<&str>, &str)> = v.status_lines.iter().map(|l| (l.helper.as_deref(), l.label.as_str())).collect();
        assert_eq!(labels, [(Some("Icons"), "Tab bar icons"), (None, "Own work")]);
        let said: Vec<(&str, Option<&str>)> = v.talk.iter().filter(|t| t.kind == "agent").map(|t| (t.text.as_str(), t.helper.as_deref())).collect();
        assert_eq!(said, [("Tabs drawn.", Some("Icons")), ("From before.", Some("old"))]);
        assert_eq!(v.waiting.len(), 2, "the helpers' questions are the session's");
        assert_eq!(view(&r, "old").id, v.id, "no page of its own: its main's");
    }

    #[test]
    fn a_question_says_who_asked_and_who_closed_it() {
        let mut r = Room::default();
        r.session("fernly", None, true);
        let c = r.card("q-gone", "fernly", "decision", "normal");
        c["versions"] = json!([{ "object_version": 1, "envelope_number": 100, "sent_at": NOW - 10 * MIN, "object_state": "open", "content": { "title": "q-gone" } }]);
        c["object_state"] = json!("closed");
        c["close_summary"] = json!("Shipped without it");
        let m = r.model(NOW);
        let lens = Lens::default();
        let b = Board::new(&m, &lens, NOW);
        let card = m.cards.get("q-gone").unwrap();
        let labels: Vec<(String, String)> = super::super::talk::events_of(&b, card).into_iter().map(|e| { let ev = e.row.event.unwrap(); (ev.kind, ev.label.en) }).collect();
        assert_eq!(labels, [("asked".to_string(), "fernly asked".to_string()), ("done".to_string(), "fernly closed it".to_string())]);
        let c = r.cards.last_mut().unwrap();
        c["close_summary"] = Value::Null;
        c["withdraw_reason"] = json!("not needed");
        let m = r.model(NOW);
        let b = Board::new(&m, &lens, NOW);
        let done = super::super::talk::events_of(&b, m.cards.get("q-gone").unwrap()).pop().unwrap().row.event.unwrap();
        assert_eq!((done.label.en.as_str(), done.text.as_str()), ("fernly withdrew it", "not needed"));
    }

    #[test]
    fn a_deed_is_a_quiet_line_and_his_own_words_stay_his() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q1", "a", "decision", "normal");
        r.say("a", Some("q1"), 200, "me", 9 * MIN, json!({ "text": HAND_BACK, "hand_back": true }));
        r.say("a", Some("q1"), 201, "me", 8 * MIN, json!({ "text": "Shorter, please.", "hand_back": true }));
        r.say("a", Some("q1"), 202, "me", 7 * MIN, json!({ "text": "whatever", "present_card": true }));
        r.say("a", Some("q1"), 203, "me", 6 * MIN, json!({ "text": super::super::talk::EXPLAIN_TEXT, "explain": true }));
        let v = view(&r, "a");
        let rows: Vec<(&str, &str, &str)> = v.talk.iter().map(|x| (x.kind.as_str(), x.text.as_str(), x.event.as_ref().or(x.deed.as_ref()).map(|e| e.label.en.as_str()).unwrap_or(""))).collect();
        assert_eq!(rows, [("event", "", "You handed it back"), ("user", "Shorter, please.", "You handed it back"), ("event", "", "You took it back"), ("event", "", "You asked for an explanation")]);
        assert!(v.talk[1].deed.is_some() && v.talk[1].event.is_none());
        assert_eq!(v.talk[0].card.as_ref().map(|c| (c.id.as_str(), c.number)), Some(("q1", 1)));
    }
    const HAND_BACK: &str = super::super::talk::HAND_BACK_TEXT;

    /// The calm chat pins its open questions above the field, the newest first, then its cards to read (never a task it
    /// still works on), and leaves the questions' rows out of the talk; without open cards nothing is pinned.
    #[test]
    fn the_calm_chat_pins_its_open_questions() {
        let mut r = Room::default();
        r.session("a", None, true);
        let history = |n: u64, title: &str| json!([{ "object_version": 1, "envelope_number": n, "sent_at": NOW - 10 * MIN, "object_state": "open", "content": { "title": title } }]);
        r.card("q-old", "a", "decision", "normal")["versions"] = history(100, "q-old");
        r.card("q-new", "a", "decision", "normal")["versions"] = history(101, "q-new");
        r.card("read-old", "a", "info", "normal")["versions"] = history(98, "read-old");
        r.card("read-new", "a", "info", "low")["versions"] = history(102, "read-new");
        r.card("task", "a", "info", "normal")["working"] = json!(true);
        r.say("a", None, 99, "a", 30 * MIN, msg("Two things for you."));
        let m = r.model(NOW);
        let calm = Lens { calm: true, ..Default::default() };
        let v = session(&Board::new(&m, &calm, NOW), &dev("a")).unwrap();
        assert_eq!(v.pinned.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(), ["q-new", "q-old", "read-new", "read-old"], "questions first, the newest first; no task");
        assert_eq!(v.pinned[2].kind, "info");
        assert!(!v.talk.iter().any(|t| t.kind == "ask"), "no row of them in the talk");
        assert_eq!(v.open, 2);
        assert_eq!(v.first_open, None, "nothing to jump to: they stand above the field");
        // (not calm: nothing pinned, the questions stand where they were asked)
        let wide = view(&r, "a");
        assert!(wide.pinned.is_empty() && wide.talk.iter().filter(|t| t.kind == "ask").count() == 2);
        // (none open: nothing pinned)
        let mut r = Room::default();
        r.session("b", None, true);
        r.say("b", None, 99, "b", 30 * MIN, msg("Nothing for you."));
        let m = r.model(NOW);
        assert!(session(&Board::new(&m, &calm, NOW), &dev("b")).unwrap().pinned.is_empty());
    }

    /// In the calm chat his answer on a card and the agent's words on it quote the card at the top of the bubble;
    /// without words, or for a card it cannot name, nothing is quoted.
    #[test]
    fn the_calm_chat_quotes_the_card_a_bubble_answers() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q1", "a", "decision", "normal");
        r.say("a", Some("q1"), 200, "me", 9 * MIN, json!({ "text": HAND_BACK, "hand_back": true }));
        r.say("a", Some("q1"), 201, "me", 8 * MIN, json!({ "text": "Shorter, please.", "hand_back": true }));
        r.say("a", Some("q1"), 202, "a", 7 * MIN, json!({ "text": "Shortened." }));
        r.say("a", None, 203, "me", 6 * MIN, json!({ "text": "And the rest?" }));
        let m = r.model(NOW);
        let calm = Lens { calm: true, ..Default::default() };
        let v = session(&Board::new(&m, &calm, NOW), &dev("a")).unwrap();
        let q: Vec<(&str, Option<(&str, &str)>)> = v.talk.iter().filter(|t| t.kind != "events").map(|t| (t.text.as_str(), t.quote.as_ref().map(|q| (q.label.en.as_str(), q.card.id.as_str())))).collect();
        assert_eq!(q, [("Shorter, please.", Some(("You handed it back", "q1"))), ("Shortened.", Some(("About", "q1"))), ("And the rest?", None)]);
        // (not calm: no quote, the deed and the line above stay as before)
        assert!(view(&r, "a").talk.iter().all(|t| t.quote.is_none()));
        // (a card it cannot name or open is not quoted)
        let mut row = TalkRow { kind: "agent".into(), text: "On it.".into(), card: Some(CardRefView { id: " ".into(), number: 0, title: "x".into() }), ..Default::default() };
        assert_eq!(quote_of(&row), None);
        row.card = Some(CardRefView { id: "c".into(), number: 0, title: String::new() });
        assert_eq!(quote_of(&row), None);
        row.card = Some(CardRefView { id: "c".into(), number: 3, title: "Ship it?".into() });
        assert_eq!(quote_of(&row).map(|q| q.label.en), Some("About".to_string()));
        row.text = String::new();
        assert_eq!(quote_of(&row), None, "no words, nothing to quote for");
    }

    #[test]
    fn what_a_message_carries() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q1", "a", "decision", "normal");
        r.say("a", None, 10, "me", 9 * MIN, json!({ "text": "", "attachments": [{ "attachment_id": "s1", "file_name": "selection.png", "media_type": "image/png" }] }));
        r.say("a", None, 11, "me", 8 * MIN, json!({ "text": "Buy milk", "note": { "written_at": NOW - 60 * MIN } }));
        r.say("a", None, 12, "a", 7 * MIN, json!({ "text": "See **this**.", "details": "The long story.", "html": "<table></table>", "attachments": [
            { "attachment_id": "p1", "media_type": "image/png", "page": "attachment:h1" }, { "attachment_id": "h1", "media_type": "text/html" } ] }));
        r.say("a", None, 13, "me", 6 * MIN, json!({ "text": "About these", "copied_cards": ["q1", "nowhere"] }));
        r.timelines.get_mut(&format!("chat:session/{}", dev("a"))).unwrap().as_array_mut().unwrap().extend([
            json!({ "envelope_number": 14, "sender_device_id": dev("a"), "sent_at": NOW - 5 * MIN, "item_state": "newer_schema", "content_type": "hologram", "content": {} }),
            json!({ "envelope_number": 15, "sender_device_id": dev("a"), "sent_at": NOW - 4 * MIN, "item_state": "pruned", "content_type": "message", "content": {} }),
            json!({ "envelope_number": 16, "sender_device_id": dev("a"), "sent_at": NOW - 4 * MIN, "item_state": "loaded", "content_type": "reaction", "content": {} }),
            json!({ "envelope_number": null, "local_id": "l1", "pending": true, "sender_device_id": "me", "sent_at": NOW - MIN, "content_type": "message", "content": { "text": "on its way" } }),
        ]);
        let v = view(&r, "a");
        assert_eq!(kinds(&v), [("e10", "user"), ("e11", "note"), ("e12", "agent"), ("e13", "user"), ("e14", "newer"), ("e15", "agent"), ("l1", "user")]);
        assert_eq!((v.talk[0].scribble, v.talk[0].files.clone()), (true, vec!["s1".to_string()]));
        assert_eq!(v.talk[1].written_ms, Some(NOW - 60 * MIN));
        // (a page that belongs to a picture is no file of its own)
        assert_eq!((v.talk[2].files.clone(), v.talk[2].details.as_str(), v.talk[2].html.as_str()), (vec!["p1".to_string()], "The long story.", "<table></table>"));
        assert_eq!(v.talk[3].chips.iter().map(|c| (c.id.as_str(), c.number, c.title.as_str())).collect::<Vec<_>>(), [("q1", 1, "q1"), ("nowhere", 0, "nowhere")]);
        assert_eq!((v.talk[4].text.as_str(), v.talk[5].text.as_str()), (crate::views::desk::NEEDS_NEWER, "(removed after 30 days)"));
        assert_eq!((v.talk[6].pending, v.talk[6].number, v.talk[6].text.as_str()), (true, 0, "on its way"));
    }

    #[test]
    fn a_turns_trail_is_one_row() {
        let mut r = Room::default();
        r.session("a", None, true);
        let step = |seq: u64, id: &str, state: &str| json!({ "terminal": "work", "work": { "turn": "t1", "seq": seq, "state": state, "started_at": NOW - 9 * MIN, "items": [{ "id": id, "kind": "step", "tool": "Bash", "title": format!("step {id}"), "state": "ok" }] } });
        r.say("a", None, 20, "me", 10 * MIN, msg("Run the tests."));
        r.say("a", None, 21, "a", 9 * MIN, step(1, "1", "running"));
        r.say("a", None, 22, "a", 8 * MIN, json!({ "text": "and lint", "terminal": "input" }));
        r.say("a", None, 23, "a", 7 * MIN, step(2, "2", "running"));
        r.say("a", None, 24, "a", 6 * MIN, step(3, "3", "running"));
        // (a trail's envelope from one of his devices is none; one that is no trail's is left out)
        r.say("a", None, 25, "me", 5 * MIN, step(4, "4", "running"));
        r.say("a", None, 26, "a", 5 * MIN, json!({ "terminal": "work", "work": { "items": [] } }));
        let v = view(&r, "a");
        // (the block stands behind what he typed while the turn ran, with all its steps)
        assert_eq!(kinds(&v), [("e20", "user"), ("e22", "user"), ("e23", "work")]);
        let w = v.talk[2].work.as_ref().unwrap();
        assert_eq!((w.head.en.as_str(), en(&w.sum), w.now.as_str(), w.lines.len()), ("Working", "", "", 3), "running: no counts, it is live");
        assert_eq!(v.talk[2].time_ms, NOW - 7 * MIN);
    }

    /// While its turn runs the session is working, however long the turn takes and after its first step (the trail
    /// streams); a turn that ended, or a session that is no longer connected, works no more.
    #[test]
    fn a_running_trail_keeps_it_working() {
        let mut r = Room::default();
        r.session("a", None, true);
        let step = |seq: u64, state: &str, item: &str| json!({ "terminal": "work", "work": { "turn": "t1", "seq": seq, "state": state, "started_at": NOW - 40 * MIN, "items": [{ "id": "1", "kind": "step", "tool": "Bash", "title": "run the tests", "state": item }] } });
        r.say("a", None, 20, "me", 40 * MIN, msg("Run the tests."));
        r.say("a", None, 21, "a", 39 * MIN, step(1, "running", "running"));
        let v = view(&r, "a");
        assert_eq!(en(&v.answering), "a is working", "the first step came and half an hour went by: still working");
        let m = r.model(NOW);
        let calm = Lens { calm: true, ..Default::default() };
        let c = session(&Board::new(&m, &calm, NOW), &dev("a")).unwrap();
        let w = c.working.clone().expect("the face at the chat's end");
        assert_eq!((w.says.en.as_str(), w.head.en.as_str(), w.now.as_str()), ("a is working", "Working · Bash · run the tests", "Bash · run the tests"));
        assert_eq!(w.trail.as_ref().map(|t| (t.state.as_str(), t.lines.len())), Some(("running", 1)), "the details hold the trail");
        assert!(c.talk.iter().all(|t| t.kind != "work"), "the calm talk holds no trail");
        // (ended: no longer working, no face)
        r.say("a", None, 22, "a", 2 * MIN, step(2, "done", "ok"));
        let v = view(&r, "a");
        assert_eq!(v.answering, None);
        let m = r.model(NOW);
        assert_eq!(session(&Board::new(&m, &calm, NOW), &dev("a")).unwrap().working, None);
        // (running, but the session is no longer connected: not working)
        let mut r = Room::default();
        r.session("b", None, false);
        r.say("b", None, 20, "me", 40 * MIN, msg("Run the tests."));
        r.say("b", None, 21, "b", 39 * MIN, step(1, "running", "running"));
        assert_eq!(view(&r, "b").answering, None);
    }

    /// A card a working line stands for has no chip in the calm talk: the line says it, in the details.
    #[test]
    fn the_calm_talk_leaves_out_the_chip_of_a_lined_card() {
        let mut r = Room::default();
        r.session("a", None, true)["status_lines"] = json!([{ "id": "l1", "label": "Duck symbol", "state": "working", "object_id": "task", "updated_at": NOW - MIN }]);
        let history = |n: u64, title: &str| json!([{ "object_version": 1, "envelope_number": n, "sent_at": NOW - 3 * MIN, "object_state": "open", "content": { "title": title } }]);
        r.card("task", "a", "info", "normal")["versions"] = history(101, "task");
        r.card("other", "a", "info", "normal")["versions"] = history(102, "other");
        r.say("a", None, 99, "me", 4 * MIN, msg("Make the duck."));
        let m = r.model(NOW);
        let calm = Lens { calm: true, ..Default::default() };
        let v = session(&Board::new(&m, &calm, NOW), &dev("a")).unwrap();
        let chips: Vec<&str> = v.talk.iter().flat_map(|t| t.events.iter().map(|c| c.text.as_str())).collect();
        assert_eq!(chips, ["other"]);
        assert!(v.working.is_some(), "a working line: the face");
    }

    #[test]
    fn the_heading_says_how_it_hears_and_why_it_is_stopped() {
        let mut r = Room::default();
        let s = r.session("a", None, true);
        s["profile"]["model"] = json!("claude-opus-5-5");
        s["profile"]["task"] = json!("Launch on Monday");
        let v = view(&r, "a");
        assert_eq!((v.name.en.as_str(), v.slug.as_str(), v.model_label.as_str(), v.model.as_str(), v.main, v.online), ("a", "a", "Opus 5.5", "claude-opus-5-5", true, true));
        assert_eq!((v.dot.on, v.dot.link.as_str(), v.dot.word.en.as_str(), v.dot.says.en.as_str()), (true, "", "connected", "connected · Launch on Monday"));
        assert_eq!((v.link.as_str(), v.blocked, v.note.is_none(), v.compose.en.as_str()), ("live", false, true, "Message to the agent"));
        // an empty conversation: the agent that joined, and three ways to start
        let e = v.empty.unwrap();
        assert_eq!((e.meta.as_str(), e.starters.len(), e.starters[0].en.as_str()), ("Opus 5.5", 3, "Where do we stand?"));

        // cut off: the dot says so, the red hand, the step in its terminal above the field
        let mut r = Room::default();
        r.session("b", None, false)["link"] = json!({ "hears": "live", "attached": true, "exit": { "reason": "stdin", "claude": "alive" } });
        let v = view(&r, "b");
        assert_eq!((v.link.as_str(), v.dot.on, v.dot.link.as_str(), v.dot.word.en.as_str(), v.dot.says.en.as_str()), ("cut", false, "cut", "cut off · 13:27", "cut off · 13:27"));
        assert_eq!(v.hand.as_ref().map(|h| (h.why.as_str(), h.text.en.as_str())), Some(("cut", "Cut off since 13:27")));
        let n = v.note.unwrap();
        assert_eq!((n.tone.as_str(), n.receipt.is_none(), n.fix.unwrap().code.as_str()), ("cut", true, "/mcp → trommi → Reconnect"));
        assert!(n.line.unwrap().en.starts_with("b is cut off since 13:27"));
        assert_eq!(v.compose.en, "It cannot hear you right now. What you write waits for it");
        // disconnected, no link reported: a grey dot
        let mut r = Room::default();
        r.session("c", None, false)["offline_since"] = Value::Null;
        let v = view(&r, "c");
        assert_eq!((v.dot.on, v.dot.word.en.as_str()), (false, if v.link == "gone" { "gone" } else { "disconnected" }));
    }

    #[test]
    fn his_last_message_waits_and_the_session_is_answering() {
        let mut r = Room::default();
        r.session("a", None, true)["heard_up_to"] = json!(40);
        r.say("a", None, 39, "me", 30 * MIN, msg("Heard."));
        r.say("a", None, 41, "me", MIN, msg("Still there?"));
        let (v, wake) = view_at(&r, "a", NOW);
        assert_eq!((v.talk[0].heard, v.talk[1].heard, en(&v.answering), v.note.is_none()), (true, false, "a is working", true));
        // (it turns late two minutes after he wrote)
        assert_eq!(wake, Some(NOW + MIN));
        let (v, wake) = view_at(&r, "a", NOW + MIN);
        let n = v.note.unwrap();
        assert_eq!((n.tone.as_str(), en(&n.receipt), n.fix.unwrap().code.as_str()), ("unheard", "a has not picked up your last message.", "/mcp → trommi → Reconnect"));
        // (and it counts as answering for ten minutes after his word)
        assert_eq!(wake, Some(NOW + 9 * MIN));
        assert_eq!(view_at(&r, "a", NOW + 9 * MIN).0.answering, None);
        // the agent's word was the last: nothing waits
        r.say("a", None, 42, "a", 0, msg("Here."));
        r.sessions[0]["heard_up_to"] = json!(41);
        let v = view(&r, "a");
        assert_eq!((v.answering.is_none(), v.note.is_none()), (true, true));
    }

    #[test]
    fn the_status_lines_that_show_and_the_card_each_stands_for() {
        let mut r = Room::default();
        let line = |id: &str, state: &str, ago: u64, card: Option<&str>| json!({ "id": id, "label": format!("L {id}"), "state": state, "detail": "d", "object_id": card, "updated_at": NOW - ago, "envelope_number": 1 });
        r.session("a", None, true)["status_lines"] = json!([
            line("w-named", "working", 5 * MIN, Some("q-named")),
            line("w-old", "working", 50 * MIN, None),
            line("w-new", "working", 4 * MIN, None),
            line("ask", "decision", 5 * MIN, None),
            line("done-new", "done", 9 * MIN, None),
            line("done-old", "done", 11 * MIN, None),
            line("stale", "working", 3 * 60 * MIN, None),
            line("stale-old", "working", 25 * 60 * MIN, None),
            line("put-off", "working", 5 * MIN, Some("q-later")),
        ]);
        r.card("q-named", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        r.card("q-loose", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 501 });
        r.card("q-later", "a", "decision", "normal");
        r.snoozes = json!({ "q-later": { "until": NOW + 60 * MIN } });
        let (v, wake) = view_at(&r, "a", NOW);
        let l: Vec<(&str, &str, &str, Option<&str>)> = v.status_lines.iter().map(|t| (t.id.as_str(), t.state.as_str(), t.says.en.as_str(), t.card.as_ref().map(|c| c.id.as_str()))).collect();
        assert_eq!(
            l,
            [
                // (the line set last first; lines of the same moment in the agent's order)
                // (the newest working line that names no card stands for the card with the session that no line names)
                ("w-new", "working", "working", Some("q-loose")),
                ("w-named", "working", "working", Some("q-named")),
                ("ask", "decision", "waits on you", None),
                ("put-off", "later", "put off", Some("q-later")),
                ("done-new", "done", "done", None),
                ("w-old", "working", "working", None),
                ("stale", "stale", "no news for a while", None),
            ]
        );
        assert_eq!((v.status_lines[1].label.as_str(), v.status_lines[1].detail.as_str(), v.status_lines[1].updated_ms), ("L w-named", "d", NOW - 5 * MIN));
        assert_eq!(v.status_more.as_ref().map(|t| t.en.as_str()), Some("+6"));
        // (the done line goes a minute from now)
        assert_eq!(wake, Some(NOW + MIN));
        // a session that is archived shows none of its lines (a connected main would come back by itself)
        r.sessions[0]["is_online"] = json!(false);
        r.settings.insert(dev("a"), json!({ "archived": true }));
        assert!(view(&r, "a").status_lines.is_empty());
    }

    #[test]
    fn a_session_that_is_not_on_the_board_has_no_page() {
        let mut r = Room::default();
        r.session("a", None, true);
        let m = r.model(NOW);
        let lens = Lens::default();
        let b = Board::new(&m, &lens, NOW);
        assert!(session(&b, "nobody").is_none());
        // (by its key, or by its short id of the address)
        assert_eq!(session(&b, "a").map(|v| v.id), Some(dev("a")));
    }

    #[test]
    fn the_gallery_filters_media_and_pages() {
        let lens = Lens::default();
        let mut r = Room::default();
        r.session("a", None, true);
        let art = |id: &str, at: u64, media_type: &str| json!({
            "object_id": id, "agent_device_id": dev("a"), "title": id, "sent_at": at, "envelope_number": at, "object_state": "open",
            "attachments": [{ "attachment_id": format!("f-{id}"), "media_type": media_type, "file_name": "x" }],
        });
        let m = r.model(NOW);
        let b = Board::new(&m, &lens, NOW);
        let (g, shown) = gallery(&b, "", artifacts(&b));
        assert_eq!((g.kinds.len(), g.lead.key.as_str(), g.none.is_none(), shown.len()), (0, "gallery.lead.empty", true, 0));
        r.published = vec![art("pic", 1, "image/png"), art("page", 2, "text/html"), art("doc", 3, "application/pdf")];
        let m = r.model(NOW);
        let b = Board::new(&m, &lens, NOW);
        let ids = |l: &[crate::views::ArtifactView]| l.iter().map(|a| a.id.clone()).collect::<Vec<_>>();
        let (g, shown) = gallery(&b, "", artifacts(&b));
        assert_eq!(ids(&shown), ["doc", "page", "pic"]);
        assert_eq!(g.kinds.iter().map(|k| (k.key.as_str(), k.label.en.as_str(), k.count, k.on)).collect::<Vec<_>>(), [("", "All", 3, true), ("media", "Media", 2, false), ("pages", "Pages", 1, false)]);
        let (g, shown) = gallery(&b, "pages", artifacts(&b));
        assert_eq!((ids(&shown), g.kind.as_str(), g.kinds[2].on), (vec!["page".to_string()], "pages", true));
        let (_, shown) = gallery(&b, "media", artifacts(&b));
        assert_eq!(ids(&shown), ["doc", "pic"]);
        let (g, _) = gallery(&b, "bogus", artifacts(&b));
        assert_eq!(g.kind, "", "an unknown filter shows every one");
        // a filter that lets nothing through says so
        r.published = vec![art("pic", 1, "image/png")];
        let m = r.model(NOW);
        let b = Board::new(&m, &lens, NOW);
        let (g, shown) = gallery(&b, "pages", artifacts(&b));
        assert_eq!((shown.len(), g.none.map(|t| t.en)), (0, Some("No pages yet.".to_string())));
    }

    #[test]
    fn the_desks_foot_keeps_every_artifact_and_only_the_gallery_filters() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.published = vec![json!({
            "object_id": "pic", "agent_device_id": dev("a"), "title": "pic", "sent_at": 1, "envelope_number": 1, "object_state": "open",
            "attachments": [{ "attachment_id": "f-pic", "media_type": "image/png", "file_name": "x" }],
        })];
        let m = r.model(NOW);
        let mut view = crate::views::ViewModel::default();
        let desk = Lens { artifacts: true, ..Default::default() };
        crate::views::board::render(&m, &desk, NOW, &mut view);
        assert_eq!((view.artifacts.as_ref().map(Vec::len), view.gallery.is_none()), (Some(1), true));
        let pages = Lens { artifacts: true, gallery: Some("pages".into()), ..Default::default() };
        crate::views::board::render(&m, &pages, NOW, &mut view);
        assert_eq!((view.artifacts.as_ref().map(Vec::len), view.gallery.as_ref().map(|g| g.kind.as_str())), (Some(0), Some("pages")));
    }
}
