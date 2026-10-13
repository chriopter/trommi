//! One card's page: who it comes from, its number, its words, its answers (or what was said, and the way back), the
//! strip while its session works on it, whether that session has his answer, its history, and the comments under
//! it. A request for approval has the same page with two answers. A shell draws it and decides nothing.
//!
//! The rules are the web's of before (`card.mjs` `cardLead`, `fitText`, `optionsSaid`, `cardAnswer`, `cardLink`,
//! `askWords`, `workOf`, `placeOf`; `ui.mjs` `cardNote`, `knockWord`; `app.mjs` `boardCard`, `heardOf`):
//! - the card is as long as its words: every paragraph stands on it; code and tables stand in the comments, and one
//!   quiet line says so;
//! - the options stand in the tiles' order, the advised last; their own paragraphs (a card written as sections)
//!   only when one says more than its label;
//! - once answered, handed back, shredded or closed, the column says what was said, who did it, and offers the way
//!   back where there is one;
//! - a card not read on this device yet, and one of a newer Trommi, offer nothing to answer;
//! - under the answers: a session that does not hear (not one that hears on its next step, before he answered), and
//!   the receipt of his answer: on its way, with the session, or not picked up for two minutes.

use super::board::{is_loading, key_of, reads_as_info, Board};
use super::desk::{from_of, plain, shown_title, NEEDS_NEWER};
use super::session::{link_note, unheard_fix};
use super::sidebar::link_of;
use super::talk::{card_ref, choices_of, events_of, file_ids, in_order, items_of, Entry, Side};
use super::words::{minutes, minutes_tick, say};
use super::{AnswerView, CardView, CardWorkView, LinkNoteView, NotedView, OptionView, PlaceView, SectionView, Text, VersionView};
use crate::model::types::{Card, PermissionRequest, StatusLine};
use crate::off_mind::went_at;
use crate::status::card_place;
use serde_json::Value;

const MOVED_CODE: &str = "*↓ code, in the whole text below*";
const MOVED_TABLE: &str = "*↓ table, in the whole text below*";

/// The text that stands on a card, and whether the comments hold more: a fenced block and a table leave one quiet
/// line in their place (never two in a row).
pub fn fit_text(source: &str) -> (String, bool) {
    // (fenced blocks: each closed one becomes the line that says where it went)
    let mut text = String::new();
    let mut rest = source;
    while let Some(open) = rest.find("```") {
        match rest[open + 3..].find("```") {
            Some(close) => {
                text.push_str(&rest[..open]);
                text.push_str("\n\n");
                text.push_str(MOVED_CODE);
                text.push_str("\n\n");
                rest = &rest[open + 3 + close + 3..];
            }
            None => break,
        }
    }
    text.push_str(rest);
    let table_line = |l: &str| {
        let l = l.trim();
        l.len() > 1 && l.starts_with('|') && l.ends_with('|')
    };
    let mut parts: Vec<&str> = Vec::new();
    for p in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        let p = if p.lines().any(table_line) { MOVED_TABLE } else { p };
        let moved = |x: &str| x == MOVED_CODE || x == MOVED_TABLE;
        if moved(p) && parts.last().is_some_and(|last| moved(last)) {
            continue;
        }
        parts.push(p);
    }
    let more = source.contains("```") || source.lines().any(|l| l.trim_start().starts_with('|'));
    (parts.join("\n\n"), more)
}

/// A text as the comparison of a section with its label reads it: no signs, no case.
fn bare_words(s: &str) -> String {
    let spaced: String = s.chars().map(|c| if c.is_whitespace() || "*_.:,;!?-–—".contains(c) { ' ' } else { c }).collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Whether an option's paragraph says more than its label.
fn beyond(text: &str, label: &str, html: &str) -> bool {
    if !html.is_empty() {
        return true;
    }
    let (t, l) = (bare_words(text), bare_words(label));
    if l.is_empty() {
        !t.is_empty()
    } else {
        !t.replacen(&l, "", 1).trim().is_empty()
    }
}

fn s<'v>(v: &'v Value, key: &str) -> &'v str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn advised_keys(c: &Card) -> Vec<String> {
    match c.field("recommended") {
        Value::String(k) => vec![k],
        Value::Array(l) => l.iter().filter_map(Value::as_str).map(String::from).collect(),
        _ => Vec::new(),
    }
}

fn urgency_number(u: &str) -> u8 {
    match u {
        "low" => 0,
        "high" => 2,
        "critical" => 3,
        _ => 1,
    }
}

/// Whether the card's session is at work on it now, and by which status line: the strip above the title.
fn work_of(b: &Board, c: &Card, agent: usize, heard_up_to: Option<u64>) -> Option<CardWorkView> {
    let a = &b.agents[agent];
    let (place, until) = card_place(c, b.runs(agent), heard_up_to, b.now);
    if place != Some("works") {
        return None;
    }
    b.wake_at(until);
    if !a.row.online {
        return None;
    }
    let lines = b.model.sessions.get(&a.row.key).map(|x| x.status_lines.as_slice()).unwrap_or(&[]);
    let working = || lines.iter().filter(|t| a.row.lines.get(&t.id).map(String::as_str) == Some("working"));
    let names = |t: &StatusLine| t.object_id.as_str().filter(|x| !x.is_empty()).map(String::from);
    // (the working line that names it, else its session's newest working line that names no card)
    let line = working().find(|t| names(t).as_deref() == Some(c.object_id.as_str())).or_else(|| {
        working().filter(|t| names(t).is_none()).fold(None::<&StatusLine>, |best, t| match best {
            Some(x) if x.updated_at >= t.updated_at => Some(x),
            _ => Some(t),
        })
    });
    let with_agent = c.object_state == "open";
    if !with_agent && line.is_none() {
        return None;
    }
    let name = a.row.name.as_str();
    let words = match line {
        Some(t) => {
            let said = [t.label.as_str().unwrap_or(""), t.detail.as_str().unwrap_or("")].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
            say("card.work.line", said, &[])
        }
        None => say("card.work.reworking", "Reworking the card", &[]),
    };
    Some(CardWorkView {
        session_id: a.row.key.clone(),
        mark: if a.row.icon.is_empty() { a.row.id.clone() } else { a.row.icon.clone() },
        says: say("card.work", format!("{name} is working on it"), &[("name", name)]),
        words,
        at_ms: line.map(|t| t.updated_at).filter(|t| *t > 0).unwrap_or_else(|| went_at(b.model, c)),
    })
}

/// Under the answers: whether the card's session can hear him, and whether it has his answer.
fn link_of_card(b: &Board, c: &Card, agent: usize, status: &str, settled: bool) -> Option<LinkNoteView> {
    let link = link_of(b, agent)?;
    if matches!(status, "shredded" | "done") || settled {
        return None;
    }
    let a = &b.agents[agent];
    let n = a.row.name.as_str();
    let late_link = (link.state != "live").then_some(&link);
    let Some(h) = b.heard_of(c) else {
        // Not answered yet: only what he should know before answering.
        return late_link.filter(|l| l.state != "oncall").map(|l| link_note(l, None));
    };
    b.wake_at(h.late_at);
    let Some(heard) = h.heard else {
        // (a connector without receipts)
        return late_link.map(|l| link_note(l, Some(say("card.receipt.waits", format!("Your answer waits for {n}."), &[("name", n)]))));
    };
    let receipt = if heard {
        say("card.receipt.heard", format!("{n} has your answer."), &[("name", n)])
    } else if h.late {
        let min = minutes(h.waiting).to_string();
        b.wake_at(Some(minutes_tick(b.now.saturating_sub(h.waiting), b.now)));
        say("card.receipt.late", format!("{n} has not picked up your answer, sent {min} min ago."), &[("name", n), ("min", &min)])
    } else {
        say("card.receipt.sent", format!("Your answer is on its way to {n}."), &[("name", n)])
    };
    if let Some(l) = late_link {
        return Some(link_note(l, Some(receipt)));
    }
    Some(LinkNoteView {
        tone: if heard { "heard" } else if h.late { "unheard" } else { "sent" }.into(),
        receipt: Some(receipt),
        line: None,
        fix: h.late.then(|| unheard_fix(a.codex)),
    })
}

/// What the field under a card says: who reads it, and when, if the session does not hear at once.
fn ask_words(b: &Board, agent: Option<usize>) -> Text {
    let name = agent.map(|i| b.agents[i].row.name.as_str()).unwrap_or("");
    let who = if name.is_empty() { "The agent" } else { name };
    let whom = if name.is_empty() { "the agent" } else { name };
    let state = agent.and_then(|i| link_of(b, i)).map(|l| l.state).unwrap_or_default();
    match state.as_str() {
        "cut" => say("card.ask.cut", format!("{who} cannot hear you right now. What you write waits for it…"), &[("name", who)]),
        "gone" => say("card.ask.gone", format!("{who} is gone. What you write waits for it…"), &[("name", who)]),
        "asleep" | "oncall" if agent.is_some_and(|i| b.agents[i].codex) => say("card.ask.next_turn", format!("Reaches {whom} on its next turn…"), &[("name", whom)]),
        "asleep" | "oncall" => say("card.ask.next_step", format!("Reaches {whom} on its next step…"), &[("name", whom)]),
        _ if name.is_empty() => say("card.ask.anyone", "Ask something, or say what is missing…", &[]),
        _ => say("card.ask", format!("Ask {name} something, or say what is missing…"), &[("name", name)]),
    }
}

fn place_of(id: &str, walk: &[String]) -> Option<PlaceView> {
    let at = walk.iter().position(|x| x == id)?;
    let (n, of) = ((at + 1).to_string(), walk.len().to_string());
    Some(PlaceView {
        at: at as u32 + 1,
        of: walk.len() as u32,
        says: say("card.place", format!("{n} of {of}"), &[("at", &n), ("of", &of)]),
        prev: at.checked_sub(1).map(|i| walk[i].clone()),
        next: walk.get(at + 1).cloned(),
    })
}

/// The page of the card or request `id` (its id, or its number of the address); none when this device does not
/// hold it. `walk`: what waits on the Desk in view, in its order (`DeskView::stack`).
pub fn card(b: &Board, id: &str, walk: &[String]) -> Option<CardView> {
    let m = b.model;
    let by_number = || {
        let n: u32 = id.parse().ok().filter(|n| *n > 0)?;
        m.cards.keys().chain(m.permissions.keys()).find(|k| b.number_of(k) == n).cloned()
    };
    let id = if m.cards.contains_key(id) || m.permissions.contains_key(id) { id.to_string() } else { by_number()? };
    let number = b.number_of(&id);
    let nr = say("card.nr", format!("Nr. {number}"), &[("n", &number.to_string())]);
    let Some(c) = m.cards.get(&id) else {
        return m.permissions.get(&id).map(|p| request(b, p, number, nr, walk));
    };
    let agent = b.by_key.get(key_of(&c.session_id, &c.agent_device_id)).copied();
    let heard_up_to = m.sessions.get(key_of(&c.session_id, &c.agent_device_id)).and_then(|x| x.heard_up_to);
    let name = agent.map(|i| b.agents[i].row.name.clone());
    let status = Board::status_of(c);
    let (loading, newer, info) = (is_loading(c), c.is_unsupported(), reads_as_info(c));
    let a = c.answer.as_ref();
    let settled = c.closed_how.as_deref() == Some("settled") && a.is_some();
    let advised = advised_keys(c);
    let field = |k: &str| c.field(k).as_str().unwrap_or("").to_string();
    let draft = m.human.get(&format!("draft/{id}")).map(|r| &r.value).filter(|v| v.is_object() && status == "open");

    // ---- its words ----
    let sections: Vec<Value> = if loading || newer { Vec::new() } else { c.field("sections").as_array().cloned().unwrap_or_default() };
    let keyed = |x: &Value| x.get("key").is_some_and(|k| !k.is_null());
    let body = if loading {
        String::new()
    } else if newer {
        if c.unsupported.as_ref().and_then(Value::as_str) == Some("card_type") { field("body") } else { String::new() }
    } else if !sections.is_empty() {
        sections.iter().filter(|x| !keyed(x)).map(|x| s(x, "text")).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n")
    } else {
        field("body")
    };
    let (text, more) = fit_text(&body);
    let mut said: Vec<SectionView> = sections
        .iter()
        .filter(|x| keyed(x))
        .map(|x| SectionView {
            key: s(x, "key").into(),
            label: s(x, "label").into(),
            recommended: x.get("recommended") == Some(&Value::Bool(true)) || advised.iter().any(|k| k == s(x, "key")),
            text: if beyond(s(x, "text"), s(x, "label"), s(x, "html")) { s(x, "text").into() } else { String::new() },
            html: s(x, "html").into(),
        })
        .collect();
    if said.iter().all(|x| x.text.is_empty() && x.html.is_empty()) {
        said.clear();
    }
    said.sort_by_key(|x| advised.contains(&x.key));

    // ---- its options ----
    let chosen: Vec<&str> = a.map(|a| choices_of(&a.choices)).unwrap_or_default();
    let ticked: Vec<&str> = draft.and_then(|d| d.get("keys")).map(choices_of).unwrap_or_default();
    let raw_options: Vec<Value> = if info { Vec::new() } else { c.field("options").as_array().cloned().unwrap_or_default() };
    let label_of = |keys: &[&str]| -> String {
        keys.iter().map(|k| raw_options.iter().find(|o| s(o, "key") == *k).map(|o| s(o, "label")).unwrap_or(k).to_string()).collect::<Vec<_>>().join(", ")
    };
    let mut options: Vec<OptionView> = raw_options
        .iter()
        .map(|o| {
            let key = s(o, "key");
            let is = advised.iter().any(|k| k == key);
            let settles = o.get("final") == Some(&Value::Bool(true));
            let noted = if status == "open" {
                draft.and_then(|d| d.get("notes")).and_then(|n| n.get(key)).and_then(Value::as_str).unwrap_or("")
            } else {
                a.and_then(|a| a.option_notes.get(key)).and_then(Value::as_str).unwrap_or("")
            };
            OptionView {
                key: key.into(),
                label: s(o, "label").into(),
                recommended: is,
                chosen: if status == "open" { ticked.contains(&key) } else { chosen.contains(&key) },
                detail: plain(s(o, "detail")),
                settles,
                note: noted.into(),
                tip: match (is, settles) {
                    (true, true) => Some(say("option.tip.both", "Recommended · Settles it", &[])),
                    (true, false) => Some(say("tile.recommended", "Recommended", &[])),
                    (false, true) => Some(say("tile.settles", "Settles it", &[])),
                    _ => None,
                },
            }
        })
        .collect();
    options.sort_by_key(|o| o.recommended);
    let multiple = c.field("allows_multiple") == Value::Bool(true);

    // ---- the answers' column ----
    let with_agent = status == "open" && c.in_revision.is_some();
    let working = status == "open" && c.field("working") == Value::Bool(true) && c.field("card_type").as_str() == Some("info");
    let quiet = |mode: &str, says: Text| AnswerView { mode: mode.into(), says: Some(says), ..Default::default() };
    let who = name.clone().unwrap_or_else(|| "The agent".into());
    let mut you_chose = None;
    let answer = if status == "catching" {
        quiet("catching", say("card.catching", "Catching up: this question was settled on another device. Its outcome arrives in a moment.", &[]))
    } else if status != "open" {
        let action = a.map(|a| a.answer_action.as_str()).unwrap_or("");
        let trusted = a.is_some_and(|a| a.trusted);
        let advised_labels = label_of(&raw_options.iter().map(|o| s(o, "key")).filter(|k| advised.iter().any(|x| x == k)).collect::<Vec<_>>());
        let picked = label_of(&chosen);
        let label = if status == "shredded" {
            say("card.said.shredded", "Shredded", &[])
        } else if info {
            say("card.said.read", "Read", &[])
        } else if trusted && advised_labels.is_empty() {
            say("card.said.duck", "I don’t give a duck", &[])
        } else if trusted {
            say("card.said.duck_advised", format!("I don’t give a duck: {advised_labels}"), &[("labels", &advised_labels)])
        } else if picked.is_empty() {
            say("card.said.withdrawn", "Withdrawn by the agent", &[])
        } else {
            say("card.said", picked.as_str(), &[])
        };
        let can = status == "shredded" || !chosen.is_empty() || trusted || (info && action == "read");
        let did = if status == "shredded" {
            Some(say("card.did.shredded", "You shredded it", &[]))
        } else if info {
            Some(say("card.did.read", "You read it", &[]))
        } else if trusted {
            Some(say("card.did.duck", "You left it to the agent", &[]))
        } else if can {
            you_chose = Some(say("card.you_chose", format!("You chose: {picked}"), &[("labels", &picked)]));
            Some(say("card.did.chose", "You chose", &[]))
        } else {
            None
        };
        let note = a.and_then(|a| a.note.as_str()).unwrap_or("");
        let summary = field("close_summary");
        AnswerView {
            mode: "said".into(),
            label: Some(label),
            detail: (!note.is_empty()).then(|| say("card.your_note", format!("Your note: {note}"), &[("note", note)])),
            did,
            at_ms: a.map(|a| a.answered_at).filter(|t| *t > 0).or((status == "done").then_some(c.updated_at)),
            back: if can { "reopen".into() } else { String::new() },
            option_notes: options
                .iter()
                .filter(|o| !o.note.is_empty())
                .map(|o| NotedView { label: o.label.clone(), note: say("card.your_note", format!("Your note: {}", o.note), &[("note", &o.note)]) })
                .collect(),
            settled: settled.then(|| say("card.settled", format!("{who} marked this answer as final: nothing follows from it."), &[("name", &who)])),
            summary: if summary.is_empty() { field("withdraw_reason") } else { summary },
            // (its agent closed it after his answer: What?? asks about what it did)
            finished: status == "done" && c.closed_how.as_deref() == Some("closed") && a.is_some_and(|a| !a.pending && !matches!(a.answer_action.as_str(), "read" | "shred")),
            says: None,
        }
    } else if loading {
        quiet("loading", say("card.loading", "Loading … this question is still being decrypted on this device.", &[]))
    } else if newer {
        quiet("newer", say("card.newer", NEEDS_NEWER, &[]))
    } else if working {
        let its = name.clone().unwrap_or_else(|| "Its session".into());
        AnswerView {
            mode: "working".into(),
            label: Some(say("card.working", "Being worked on", &[])),
            detail: Some(say("card.working.detail", format!("{its} is on it; it comes to your Desk when it is done."), &[("name", &its)])),
            ..Default::default()
        }
    } else if with_agent {
        AnswerView {
            mode: "revising".into(),
            label: Some(say("card.revising", "In revision", &[])),
            detail: Some(say("card.revising.detail", "It is with its session and comes back reworked.", &[])),
            did: Some(say("card.did.handed_back", "You handed it back", &[])),
            at_ms: Some(went_at(m, c)),
            back: "takeback".into(),
            ..Default::default()
        }
    } else if info {
        AnswerView { mode: "read".into(), ..Default::default() }
    } else {
        quiet(
            "options",
            if multiple {
                say("card.answer.several", "Your answer. Tick what applies, then send.", &[])
            } else {
                say("card.answer.one", "Your answer. One tap answers.", &[])
            },
        )
    };

    // ---- the comments ----
    let mut entries: Vec<Entry> = events_of(b, c);
    let chat = m.timelines.get(&format!("chat:card/{id}"));
    if let Some(t) = chat {
        items_of(b, t, Some(c), heard_up_to, &mut entries);
    }
    in_order(&mut entries);
    let below = entries.iter().filter(|e| e.side != Side::Event && e.row.event.is_none() && (!e.row.text.is_empty() || !e.row.files.is_empty())).count();

    // ---- the quiet line ----
    let merged: Vec<String> = c.field("merged_from_object_ids").as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    let mut notes = Vec::new();
    if !loading {
        if merged.len() == 1 {
            notes.push(say("row.replaces.one", "replaces 1 question", &[]));
        } else if merged.len() > 1 {
            let n = merged.len().to_string();
            notes.push(say("row.replaces", format!("replaces {n} questions"), &[("n", &n)]));
        }
        if c.history.len() > 1 {
            notes.push(say("row.revised", "revised", &[]));
        }
    }
    if below == 1 {
        notes.push(say("card.below.one", "1 message below", &[]));
    } else if below > 1 {
        let n = below.to_string();
        notes.push(say("card.below", format!("{n} messages below"), &[("n", &n)]));
    }
    let knock = (status == "open" && !loading).then(|| match c.urgency.as_str() {
        "critical" => Some(say("row.knock.blocking", "Knock! Blocking", &[])),
        "high" => Some(say("row.knock", "Knock", &[])),
        _ => None,
    });
    let versions: Vec<VersionView> = if loading || c.history.len() < 2 {
        Vec::new()
    } else {
        // (each version with what its agent said changed when the next one came)
        c.history.windows(2).map(|w| VersionView { n: w[0].object_version.max(0) as u32, at_ms: w[0].sent_at, title: w[0].title.clone(), note: w[1].note.clone() }).collect()
    };
    let starred = m.human.get(&format!("star/{id}")).is_some_and(|r| !r.value.is_null() && r.value != Value::Bool(false));
    let other = |id: &str| match m.cards.get(id) {
        Some(x) => card_ref(b, x),
        None => super::CardRefView { id: id.into(), number: 0, title: String::new() },
    };
    let into = field("merged_into_object_id");
    Some(CardView {
        session_id: agent.map(|i| b.agents[i].row.key.clone()),
        kind: if info { "info" } else { "decision" }.into(),
        state: c.object_state.clone(),
        urgency: if loading { 1 } else { urgency_number(&c.urgency) },
        title: shown_title(c),
        body,
        options,
        you_chose,
        history: versions.len() as u32,
        starred,
        snoozed_until: c.snoozed_until.filter(|_| status == "open"),
        files: if loading || newer { Vec::new() } else { file_ids(&c.field("attachments")) },
        number,
        nr,
        status: status.into(),
        from: from_of(b, agent),
        knock: knock.flatten(),
        work: agent.and_then(|i| work_of(b, c, i, heard_up_to)),
        asked_ms: c.history.last().filter(|_| c.history.len() > 1).map(|h| h.sent_at).unwrap_or(c.created_at),
        place: place_of(&id, walk),
        notes,
        text,
        more,
        html: if newer || loading { String::new() } else { field("html") },
        sections: said,
        multiple,
        note: agent.and_then(|i| link_of_card(b, c, i, status, settled)),
        answer,
        ask: ask_words(b, agent),
        draft: draft.map(|d| s(d, "note").to_string()).unwrap_or_default(),
        versions,
        star_says: if starred { say("card.star.on", "Starred", &[]) } else { say("card.star", "Star", &[]) },
        talk: entries.into_iter().map(|e| e.row).collect(),
        has_more: chat.is_some_and(|t| t.has_more),
        merged_from: merged.iter().map(|x| other(x)).collect(),
        merged_into: (!into.is_empty()).then(|| other(&into)),
        id,
    })
}

/// A request for approval as a card's page: what it asks to run, and Allow or Deny.
fn request(b: &Board, p: &PermissionRequest, number: u32, nr: Text, walk: &[String]) -> CardView {
    let agent = b.by_key.get(key_of(&p.session_id, &p.agent_device_id)).copied();
    let read = p.content_state.is_empty() || p.content_state == "ok";
    let open = !p.catching_up && p.permission_state == "pending" && !(p.expires_at > 0 && p.expires_at < b.now);
    if open && p.expires_at > 0 {
        b.wake_at(Some(p.expires_at + 1));
    }
    let verdict = p.verdict.as_ref().map(|v| if v.allow { "allow" } else { "deny" });
    let option = |key: &str, label: &str| OptionView { key: key.into(), label: label.into(), chosen: verdict == Some(key), ..Default::default() };
    let body = format!("{}\n\n{}", p.description, p.input_preview);
    let (text, more) = fit_text(&body);
    let answer = if p.catching_up {
        AnswerView { mode: "catching".into(), says: Some(say("card.catching", "Catching up: this question was settled on another device. Its outcome arrives in a moment.", &[])), ..Default::default() }
    } else if !open {
        let summary = match p.permission_state.as_str() {
            "withdrawn" => "Answered in the terminal",
            "expired" => "Expired",
            _ => "",
        };
        AnswerView {
            mode: "said".into(),
            label: Some(match verdict {
                Some("allow") => say("card.said", "Allow", &[]),
                Some(_) => say("card.said", "Deny", &[]),
                None => say("card.said.withdrawn", "Withdrawn by the agent", &[]),
            }),
            did: verdict.map(|_| say("card.did.chose", "You chose", &[])),
            summary: summary.into(),
            ..Default::default()
        }
    } else if !read {
        AnswerView { mode: "loading".into(), says: Some(say("card.loading", "Loading … this question is still being decrypted on this device.", &[])), ..Default::default() }
    } else {
        AnswerView { mode: "request".into(), says: Some(say("card.answer.one", "Your answer. One tap answers.", &[])), ..Default::default() }
    };
    let live = open && read;
    CardView {
        id: p.object_id.clone(),
        session_id: agent.map(|i| b.agents[i].row.key.clone()),
        kind: if open && !read { "info" } else { "request" }.into(),
        state: if open { "open" } else { "closed" }.into(),
        urgency: if live { 3 } else { 1 },
        title: if open && !read { "Loading …".into() } else { format!("Approval: {}", p.tool_name) },
        body: if read { body } else { String::new() },
        options: if live || (!open && read) { vec![option("allow", "Allow"), option("deny", "Deny")] } else { Vec::new() },
        number,
        nr,
        status: if p.catching_up { "catching" } else if open { "open" } else { "done" }.into(),
        from: from_of(b, agent),
        knock: live.then(|| say("row.knock.permission", "Knock! Permission", &[])),
        asked_ms: p.sent_at,
        place: place_of(&p.object_id, walk),
        text: if read { text } else { String::new() },
        more: read && more,
        answer,
        ask: ask_words(b, agent),
        star_says: say("card.star", "Star", &[]),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::desk;
    use crate::views::testroom::{Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::json;

    fn view_at(r: &Room, id: &str, now: u64) -> (CardView, Option<u64>) {
        let m = r.model(now);
        let lens = Lens::default();
        let b = Board::new(&m, &lens, now);
        let walk = desk::desk(&b).stack;
        let v = card(&b, id, &walk).expect("the card is held");
        (v, b.wake())
    }
    fn view(r: &Room, id: &str) -> CardView {
        view_at(r, id, NOW).0
    }
    fn en(t: &Option<Text>) -> &str {
        t.as_ref().map(|t| t.en.as_str()).unwrap_or("")
    }
    fn notes(v: &CardView) -> Vec<&str> {
        v.notes.iter().map(|n| n.en.as_str()).collect()
    }

    /// The web's `fitText` (app/web/public/card.mjs).
    #[test]
    fn the_card_holds_its_paragraphs_code_and_tables_go_to_the_comments() {
        assert_eq!(fit_text("One.\n\n\n\nTwo.  "), ("One.\n\nTwo.".to_string(), false));
        let (shown, more) = fit_text("Run this:\n```sh\ndf -h\n```\nThen look:\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nDone.");
        assert_eq!((shown.as_str(), more), ("Run this:\n\n*↓ code, in the whole text below*\n\nThen look:\n\n*↓ table, in the whole text below*\n\nDone.", true));
        // (two moved blocks in a row leave one line; a fence that never closes stays as it is)
        assert_eq!(fit_text("```a```\n\n```b```\n\n| x |").0, "*↓ code, in the whole text below*");
        assert_eq!(fit_text("so ``` it goes"), ("so ``` it goes".to_string(), true));
        assert_eq!(fit_text(""), (String::new(), false));
    }

    #[test]
    fn a_section_shows_only_when_it_says_more_than_its_label() {
        assert!(!beyond("**Split.**", "Split", "") && !beyond("", "Split", "") && beyond("Split: shows the app at once", "Split", "") && beyond("", "Split", "<b>x</b>"));
        let mut r = Room::default();
        r.session("a", None, true);
        let c = r.card("q1", "a", "decision", "normal");
        c["options"] = json!([{ "key": "split", "label": "Split" }, { "key": "centered", "label": "Centered", "final": true, "detail": "Words **first**" }]);
        c["recommended"] = json!("split");
        c["body"] = json!("ignored beside sections");
        c["sections"] = json!([
            { "text": "Two layouts." }, { "key": "split", "label": "Split", "text": "Shows the phone." },
            { "text": "Same words." }, { "key": "centered", "label": "Centered", "text": "Centered." },
        ]);
        let v = view(&r, "q1");
        assert_eq!((v.body.as_str(), v.text.as_str(), v.more), ("Two layouts.\n\nSame words.", "Two layouts.\n\nSame words.", false));
        // (in the tiles' order: the advised last)
        assert_eq!(v.sections.iter().map(|x| (x.key.as_str(), x.text.as_str(), x.recommended)).collect::<Vec<_>>(), [("centered", "", false), ("split", "Shows the phone.", true)]);
        assert_eq!(v.options.iter().map(|o| (o.key.as_str(), o.recommended, o.settles, o.detail.as_str(), en(&o.tip))).collect::<Vec<_>>(), [("centered", false, true, "Words first", "Settles it"), ("split", true, false, "", "Recommended")]);
        assert_eq!((v.answer.mode.as_str(), en(&v.answer.says), v.multiple), ("options", "Your answer. One tap answers.", false));
        // none says more than its label: no paragraphs at all
        r.cards[0]["sections"] = json!([{ "key": "split", "label": "Split", "text": "Split" }]);
        assert!(view(&r, "q1").sections.is_empty());
    }

    #[test]
    fn the_lead_says_who_asks_its_number_and_its_place() {
        let mut r = Room { desks: json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } }), ..Default::default() };
        r.session("a", None, true);
        r.session("b", Some("a"), true);
        r.card("q1", "a", "decision", "normal");
        let c = r.card("q2", "b", "decision", "critical");
        c["merged_from_object_ids"] = json!(["q1", "gone"]);
        c["versions"] = json!([
            { "object_version": 1, "envelope_number": 101, "sent_at": NOW - 9 * MIN, "object_state": "open", "content": { "title": "First try" } },
            { "object_version": 2, "envelope_number": 120, "sent_at": NOW - 4 * MIN, "object_state": "open", "content": { "title": "q2", "change_note": "Shorter." } },
        ]);
        r.say("b", Some("q2"), 130, "me", 3 * MIN, json!({ "text": "Why?" }));
        r.say("b", Some("q2"), 131, "b", 2 * MIN, json!({ "text": "Because." }));
        r.request("p1", "a", "ok");
        r.stars = json!({ "q2": { "at": 5 } });
        let v = view(&r, "q2");
        assert_eq!((v.number, v.nr.en.as_str(), v.status.as_str(), v.kind.as_str(), v.urgency, en(&v.knock)), (2, "Nr. 2", "open", "decision", 3, "Knock! Blocking"));
        assert_eq!(v.from.as_ref().map(|f| f.path.clone()), Some(vec!["Work".to_string(), "a".into(), "b".into()]));
        assert_eq!(notes(&v), ["replaces 2 questions", "revised", "2 messages below"]);
        // (what blocks stands first on the Desk: its place among what waits, with its neighbours)
        let p = v.place.as_ref().unwrap();
        assert_eq!((p.at, p.of, p.says.en.as_str(), p.prev.as_deref(), p.next.as_deref()), (1, 3, "1 of 3", None, Some("p1")));
        assert_eq!((v.history, v.asked_ms), (1, NOW - 4 * MIN));
        assert_eq!(v.versions.iter().map(|x| (x.n, x.title.as_str(), x.note.as_str(), x.at_ms)).collect::<Vec<_>>(), [(1, "First try", "Shorter.", NOW - 9 * MIN)]);
        assert_eq!((v.starred, v.star_says.en.as_str(), v.ask.en.as_str()), (true, "Starred", "Ask b something, or say what is missing…"));
        assert_eq!(v.merged_from.iter().map(|x| (x.id.as_str(), x.number)).collect::<Vec<_>>(), [("q1", 1), ("gone", 0)]);
        // the comments: what happened to it and what was said, in the hub's order
        assert_eq!(v.talk.iter().map(|x| (x.id.as_str(), x.kind.as_str())).collect::<Vec<_>>(), [("v101a", "event"), ("v120r", "event"), ("e130", "user"), ("e131", "agent")]);
        // by its number of the address too
        assert_eq!(view(&r, "2").id, "q2");
        // a request for approval: the same page, two answers
        let v = view(&r, "p1");
        assert_eq!((v.kind.as_str(), v.title.as_str(), v.number, en(&v.knock), v.answer.mode.as_str(), v.urgency), ("request", "Approval: Bash", 3, "Knock! Permission", "request", 3));
        assert_eq!(v.options.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(), ["allow", "deny"]);
        assert_eq!(v.place.map(|p| p.at), Some(2));
    }

    #[test]
    fn what_was_said_and_the_way_back() {
        let mut r = Room::default();
        r.session("a", None, true)["heard_up_to"] = json!(40);
        let options = json!([{ "key": "a", "label": "Ship it" }, { "key": "b", "label": "Wait" }]);
        r.answered("q-chose", "a", NOW - 5 * MIN, 150);
        r.cards[0]["options"] = options.clone();
        r.cards[0]["answer"]["note"] = json!("on Monday");
        r.cards[0]["answer"]["option_notes"] = json!({ "b": "not this week" });
        let v = view(&r, "q-chose");
        let a = &v.answer;
        assert_eq!((a.mode.as_str(), en(&a.label), en(&a.detail), en(&a.did), a.at_ms, a.back.as_str()), ("said", "Ship it", "Your note: on Monday", "You chose", Some(NOW - 5 * MIN), "reopen"));
        assert_eq!((en(&v.you_chose), v.status.as_str(), v.state.as_str()), ("You chose: Ship it", "decided", "answered"));
        assert_eq!(v.options.iter().map(|o| (o.key.as_str(), o.chosen)).collect::<Vec<_>>(), [("a", true), ("b", false)]);
        assert_eq!(a.option_notes.iter().map(|n| (n.label.as_str(), n.note.en.as_str())).collect::<Vec<_>>(), [("Wait", "Your note: not this week")]);

        // the duck, with what the agent advised
        r.answered("q-duck", "a", NOW - 5 * MIN, 151);
        let c = r.cards.last_mut().unwrap();
        c["options"] = options.clone();
        c["recommended"] = json!("b");
        c["answer"] = json!({ "answer_action": "answer", "choices": [], "trusted": true, "envelope_number": 151, "answered_at": NOW - 5 * MIN });
        let v = view(&r, "q-duck");
        assert_eq!((en(&v.answer.label), en(&v.answer.did), v.answer.back.as_str(), v.you_chose.is_none()), ("I don’t give a duck: Wait", "You left it to the agent", "reopen", true));

        // settled by a final option; shredded; read; closed by its agent after his answer; withdrawn
        r.answered("q-settled", "a", NOW - 5 * MIN, 152);
        let c = r.cards.last_mut().unwrap();
        c["object_state"] = json!("closed");
        c["closed_how"] = json!("settled");
        assert_eq!((en(&view(&r, "q-settled").answer.settled), view(&r, "q-settled").note.is_none()), ("a marked this answer as final: nothing follows from it.", true));
        r.answered("q-shred", "a", NOW - 5 * MIN, 153);
        let c = r.cards.last_mut().unwrap();
        c["object_state"] = json!("closed");
        c["closed_how"] = json!("shredded");
        c["answer"]["answer_action"] = json!("shred");
        let v = view(&r, "q-shred");
        assert_eq!((v.status.as_str(), en(&v.answer.label), en(&v.answer.did), v.answer.back.as_str()), ("shredded", "Shredded", "You shredded it", "reopen"));
        r.answered("q-read", "a", NOW - 5 * MIN, 154);
        let c = r.cards.last_mut().unwrap();
        c["card_type"] = json!("info");
        c["object_state"] = json!("closed");
        c["closed_how"] = json!("closed");
        c["answer"] = json!({ "answer_action": "read", "choices": [], "envelope_number": 154, "answered_at": NOW - 5 * MIN });
        let v = view(&r, "q-read");
        assert_eq!((v.kind.as_str(), en(&v.answer.label), en(&v.answer.did), v.answer.back.as_str(), v.answer.finished), ("info", "Read", "You read it", "reopen", false));
        r.answered("q-finished", "a", NOW - 5 * MIN, 155);
        let c = r.cards.last_mut().unwrap();
        c["object_state"] = json!("closed");
        c["closed_how"] = json!("closed");
        c["close_summary"] = json!("Shipped at noon.");
        let v = view(&r, "q-finished");
        assert_eq!((v.status.as_str(), v.answer.summary.as_str(), v.answer.finished), ("done", "Shipped at noon.", true));
        let c = r.card("q-withdrawn", "a", "decision", "normal");
        c["object_state"] = json!("closed");
        c["closed_how"] = json!("withdrawn");
        c["withdraw_reason"] = json!("No longer needed");
        c["updated_at"] = json!(NOW - 3 * MIN);
        let v = view(&r, "q-withdrawn");
        assert_eq!((en(&v.answer.label), v.answer.did.is_none(), v.answer.back.as_str(), v.answer.summary.as_str(), v.answer.at_ms), ("Withdrawn by the agent", true, "", "No longer needed", Some(NOW - 3 * MIN)));
    }

    #[test]
    fn nothing_to_answer_while_it_is_elsewhere_or_unread() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q-back", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        r.cards[0]["updated_at"] = json!(NOW - 2 * MIN);
        let v = view(&r, "q-back");
        let a = &v.answer;
        assert_eq!((a.mode.as_str(), en(&a.label), en(&a.detail), en(&a.did), a.at_ms, a.back.as_str()), ("revising", "In revision", "It is with its session and comes back reworked.", "You handed it back", Some(NOW - 2 * MIN), "takeback"));
        // (its session works on it: the strip, without a status line "Reworking the card")
        let w = v.work.unwrap();
        assert_eq!((w.says.en.as_str(), w.words.en.as_str(), w.at_ms), ("a is working on it", "Reworking the card", NOW - 2 * MIN));
        assert!(v.place.is_none(), "with its agent: not on the Desk");
        r.sessions[0]["status_lines"] = json!([{ "id": "l1", "label": "Rework", "state": "working", "detail": "half way", "object_id": "q-back", "updated_at": NOW - MIN, "envelope_number": 1 }]);
        let w = view(&r, "q-back").work.unwrap();
        assert_eq!((w.words.en.as_str(), w.at_ms), ("Rework · half way", NOW - MIN));

        r.card("q-works", "a", "info", "normal")["working"] = json!(true);
        let a = view(&r, "q-works").answer;
        assert_eq!((a.mode.as_str(), en(&a.label), en(&a.detail)), ("working", "Being worked on", "a is on it; it comes to your Desk when it is done."));
        r.card("q-info", "a", "info", "normal");
        assert_eq!(view(&r, "q-info").answer.mode, "read");
        let c = r.card("q-loading", "a", "decision", "critical");
        c["content_state"] = json!("undecryptable");
        c["body"] = json!("secret");
        c["options"] = json!([{ "key": "y", "label": "Yes" }]);
        let v = view(&r, "q-loading");
        assert_eq!((v.answer.mode.as_str(), v.title.as_str(), v.body.as_str(), v.urgency, v.knock.is_none(), v.kind.as_str()), ("loading", "Loading …", "", 1, true, "info"));
        assert!(v.options.is_empty());
        let c = r.card("q-newer", "a", "decision", "normal");
        c["unsupported"] = json!("schema");
        let v = view(&r, "q-newer");
        assert_eq!((v.answer.mode.as_str(), en(&v.answer.says)), ("newer", NEEDS_NEWER));
        r.card("q-catching", "a", "decision", "normal");
        r.request("p-unread", "a", "header");
        let v = view(&r, "p-unread");
        assert_eq!((v.answer.mode.as_str(), v.title.as_str(), v.kind.as_str(), v.options.len(), v.knock.is_none()), ("loading", "Loading …", "info", 0, true));
        // several may be ticked: his draft's ticks and notes stand
        let c = r.card("q-many", "a", "decision", "normal");
        c["allows_multiple"] = json!(true);
        c["options"] = json!([{ "key": "x", "label": "X" }, { "key": "y", "label": "Y" }]);
        let m = {
            let mut m = r.model(NOW);
            m.human.insert("draft/q-many".into(), crate::model::types::HumanRegister { value: json!({ "keys": ["y"], "note": "both, really", "notes": { "x": "maybe" } }), ..Default::default() });
            // (settled on another device while this one was away: its outcome is on the way)
            m.cards.get_mut("q-catching").unwrap().catching_up = true;
            m
        };
        let lens = Lens::default();
        let b = Board::new(&m, &lens, NOW);
        assert_eq!(card(&b, "q-catching", &[]).unwrap().answer.mode, "catching");
        let v = card(&b, "q-many", &[]).unwrap();
        assert_eq!((v.multiple, en(&v.answer.says), v.draft.as_str()), (true, "Your answer. Tick what applies, then send.", "both, really"));
        assert_eq!(v.options.iter().map(|o| (o.key.as_str(), o.chosen, o.note.as_str())).collect::<Vec<_>>(), [("x", false, "maybe"), ("y", true, "")]);
    }

    /// The web's `cardLink` (app/web/public/card.mjs).
    #[test]
    fn under_the_answers_whether_its_session_has_his_answer() {
        let mut r = Room::default();
        r.session("a", None, true)["heard_up_to"] = json!(40);
        // open, the session hears at once: nothing to say
        r.card("q-open", "a", "decision", "normal");
        assert!(view(&r, "q-open").note.is_none());
        // answered a minute ago, not picked up yet: on its way; it turns late a minute on
        r.answered("q-sent", "a", NOW - MIN, 50);
        let (v, wake) = view_at(&r, "q-sent", NOW);
        let n = v.note.unwrap();
        assert_eq!((n.tone.as_str(), en(&n.receipt), n.fix.is_none()), ("sent", "Your answer is on its way to a.", true));
        assert_eq!(wake, Some(NOW + MIN));
        let n = view_at(&r, "q-sent", NOW + 2 * MIN).0.note.unwrap();
        assert_eq!((n.tone.as_str(), en(&n.receipt), n.fix.unwrap().code.as_str()), ("unheard", "a has not picked up your answer, sent 3 min ago.", "/mcp → trommi → Reconnect"));
        // picked up
        r.answered("q-heard", "a", NOW - 5 * MIN, 30);
        let n = view(&r, "q-heard").note.unwrap();
        assert_eq!((n.tone.as_str(), en(&n.receipt)), ("heard", "a has your answer."));

        // a session that is cut off: said before he answers, and with the receipt after
        let mut r = Room::default();
        let s = r.session("b", None, false);
        s["link"] = json!({ "hears": "live", "attached": true, "exit": { "reason": "stdin", "claude": "alive" } });
        s["heard_up_to"] = json!(40);
        r.card("q-open", "b", "decision", "normal");
        let v = view(&r, "q-open");
        let n = v.note.unwrap();
        assert_eq!((n.tone.as_str(), n.receipt.is_none(), n.line.is_some()), ("cut", true, true));
        assert_eq!(v.ask.en, "b cannot hear you right now. What you write waits for it…");
        r.answered("q-sent", "b", NOW - 5 * MIN, 50);
        let n = view(&r, "q-sent").note.unwrap();
        assert_eq!((n.tone.as_str(), en(&n.receipt)), ("cut", "b has not picked up your answer, sent 5 min ago."));
        // (a connector that writes no receipts: his answer waits for it)
        r.sessions[0]["heard_up_to"] = Value::Null;
        assert_eq!(en(&view(&r, "q-sent").note.unwrap().receipt), "Your answer waits for b.");
    }
}
