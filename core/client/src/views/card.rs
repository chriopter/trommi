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

use super::board::{is_loading, key_of, reads_as_info, Board, Lens, COMMENTS_PAGE};
use super::desk::{from_of, shown_title, NEEDS_NEWER};
use super::links::{plain, titled};
use super::session::{link_note, unheard_fix};
use super::sidebar::link_of;
use super::talk::{card_ref, choices_of, events_of, file_ids, in_order, items_of, Entry, Side};
use super::words::{minutes, minutes_tick, say};
use super::{AfterView, AnswerView, LeadView, MoreView, OptionLinkView, CardRefView, CardMenuItem, CardView, PictureView, CardWorkView, LinkNoteView, NotedView, OptionView, PlaceView, SectionView, TalkRow, Text, VersionView};
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
        mark: a.row.mark(),
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

fn slug(text: &str) -> String {
    let lower = text.to_lowercase();
    let mut out = String::new();
    let mut dash = false;
    for ch in lower.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(ch);
        } else {
            dash = true;
        }
    }
    out
}

/// A card's pictures and the option each shows (the web's `pictureKeys`).
pub fn pictures_of(attachments: &[Value], sections: &[Value], options: &[super::desk::Opt]) -> Vec<PictureView> {
    let s = |a: &Value, k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    for a in attachments {
        super::files_seen::saw(a);
    }
    let images: Vec<(usize, &Value)> = attachments
        .iter()
        .enumerate()
        .filter(|(_, a)| a.get("media_type").and_then(Value::as_str).unwrap_or("").starts_with("image/"))
        .collect();
    let index_of = |raw: usize| images.iter().position(|(i, _)| *i == raw);
    // (a section names its picture by its place among the attachments)
    let section_pics: Vec<(usize, String)> = sections
        .iter()
        .filter_map(|x| {
            let key = x.get("key").and_then(Value::as_str)?;
            let raw = x.get("picture").and_then(Value::as_u64)? as usize;
            Some((index_of(raw)?, key.to_string()))
        })
        .collect();
    let mut keys: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for (at, key) in &section_pics {
        keys.entry(*at).or_insert_with(|| key.clone());
    }
    if keys.is_empty() && images.len() >= 2 && options.len() >= 2 {
        let mut unclear = false;
        let mut owners = std::collections::BTreeSet::new();
        for (i, (_, img)) in images.iter().enumerate() {
            let file = s(img, "file_name");
            let stem = match file.rfind('.') {
                Some(dot) if dot + 1 < file.len() && file[dot + 1..].chars().all(|c| c.is_ascii_alphanumeric()) => &file[..dot],
                _ => file.as_str(),
            };
            let name = format!("-{}-", slug(stem));
            let mut hits: Vec<&super::desk::Opt> = options
                .iter()
                .filter(|o| [slug(&o.key), slug(&o.label)].iter().any(|w| !w.is_empty() && name.contains(&format!("-{w}-"))))
                .collect();
            hits.sort_by_key(|o| std::cmp::Reverse(slug(&o.key).len()));
            let Some(first) = hits.first() else { continue };
            if hits.len() > 1 && slug(&hits[1].key).len() == slug(&first.key).len() {
                unclear = true;
                continue;
            }
            keys.insert(i, first.key.clone());
            owners.insert(first.key.clone());
        }
        if unclear || owners.len() < 2 {
            keys = if images.len() == options.len() && options.len() >= 3 {
                options.iter().enumerate().map(|(i, o)| (i, o.key.clone())).collect()
            } else {
                Default::default()
            };
        }
    }
    images
        .iter()
        .enumerate()
        .map(|(i, (_, a))| {
            let name = s(a, "file_name");
            let title = s(a, "title");
            let key = keys.get(&i).cloned();
            let shared: Vec<String> = section_pics.iter().filter(|(at, _)| *at == i).map(|(_, k)| k.clone()).collect();
            let title = if title != name { title } else { String::new() };
            let label = key.as_ref().and_then(|k| options.iter().find(|o| &o.key == k)).map(|o| o.label.clone()).filter(|l| !l.is_empty());
            PictureView {
                file_id: s(a, "attachment_id"),
                caption: if !title.is_empty() { title.clone() } else { label.unwrap_or_else(|| name.clone()) },
                title,
                name,
                width: a.get("width").and_then(Value::as_u64).filter(|w| *w > 0),
                height: a.get("height").and_then(Value::as_u64).filter(|h| *h > 0),
                page: a.get("page").and_then(Value::as_str).and_then(|p| p.strip_prefix("attachment:")).map(String::from),
                keys: if shared.is_empty() { key.iter().cloned().collect() } else { shared },
                key,
            }
        })
        .collect()
}

/// The links an agent wrote into its options' labels and details (the web's `optionLinks`): a web address, or a
/// path to a page (`/a/b.html`), with the punctuation after it left off; each with its option.
pub fn option_links(options: &Value) -> Vec<OptionLinkView> {
    let mut out = Vec::new();
    for o in options.as_array().into_iter().flatten() {
        let get = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let (key, label) = (get("key"), get("label"));
        let found = links_in(&format!("{label} {}", get("detail")));
        if found.is_empty() {
            continue;
        }
        let mut bare = label.clone();
        for (at, end) in links_in(&label).into_iter().rev() {
            bare.replace_range(at..end, "");
        }
        let text = format!("{label} {}", get("detail"));
        let bare = bare.trim();
        for (at, end) in found {
            let url = text[at..end].trim_end_matches(['.', ',', ';', ':', '!', '?']).to_string();
            let short = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).unwrap_or(&url);
            let short = short.strip_prefix("www.").unwrap_or(short).to_string();
            out.push(OptionLinkView { key: key.clone(), label: if bare.is_empty() { key.clone() } else { bare.to_string() }, url, short });
        }
    }
    out
}

/// Where links stand in a text, as byte ranges (the web's LINK pattern).
fn links_in(text: &str) -> Vec<(usize, usize)> {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let in_name = |c: char| word(c) || c == '-' || c == '.';
    let mut out = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let Some(c) = rest.chars().next() else { break };
        if rest.starts_with("http://") || rest.starts_with("https://") {
            let len = rest.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | ')' | '`')).unwrap_or(rest.len());
            out.push((i, i + len));
            i += len;
            continue;
        }
        let before = text[..i].chars().next_back();
        if c == '/' && !before.is_some_and(|p| word(p) || matches!(p, '/' | ':' | '.' | '~' | '-')) {
            // (/name/…/name.htm[l], then a query and a fragment)
            let run = rest[1..].find(|c: char| !(in_name(c) || c == '/')).map_or(rest.len(), |n| n + 1);
            let path = &rest[..run];
            let mut end = None;
            for (at, _) in path.match_indices(".htm") {
                let stop = if path[at + 4..].starts_with('l') { at + 5 } else { at + 4 };
                let head = &path[1..at];
                let ok = !head.is_empty() && head.split('/').all(|seg| !seg.is_empty()) && !head.ends_with('/');
                if ok {
                    end = Some(stop);
                }
            }
            if let Some(mut stop) = end {
                for sign in ['?', '#'] {
                    if rest[stop..].starts_with(sign) {
                        stop += 1 + rest[stop + 1..].find(|c: char| !(word(c) || matches!(c, '-' | '.' | '=' | '&' | '%' | '+'))).unwrap_or(rest.len() - stop - 1);
                    }
                }
                out.push((i, i + stop));
                i += stop;
                continue;
            }
        }
        i += c.len_utf8();
    }
    out
}

/// A card's comments as its page groups them (the web's `cardThread`): a new version says its number, and "as you
/// asked" when his hand-back came before it; an answer to an older version says which; the agent's words after a bare
/// What?? answer it (one block), or the What?? waits for them.
fn thread(c: &Card, mut talk: Vec<TalkRow>) -> Vec<TalkRow> {
    let version_at = |n: u64| c.history.iter().find(|h| h.envelope_number == n).map(|h| h.object_version.max(0) as u32);
    let current = c.history.last().map(|h| h.object_version.max(0) as u32).unwrap_or(0);
    let mut asked = false;
    let mut explain: Option<usize> = None;
    for i in 0..talk.len() {
        let kind = talk[i].event.as_ref().or(talk[i].deed.as_ref()).map(|e| e.kind.clone()).unwrap_or_default();
        // (the agent's words right after a bare What?? answer it)
        if let Some(at) = explain {
            if talk[i].kind == "agent" {
                talk[i].answers = Some(talk[at].id.clone());
                continue;
            }
            explain = None;
        }
        match kind.as_str() {
            "handed_back" => asked = true,
            "took_back" => asked = false,
            "explain" if talk[i].kind == "event" => explain = Some(i),
            "revised" => {
                let n = version_at(talk[i].number).unwrap_or(0);
                if let Some(ev) = talk[i].event.as_mut() {
                    ev.version = (n > 0).then_some(n);
                    ev.asked = asked;
                    let ns = n.to_string();
                    ev.label = if asked {
                        say("event.version.asked", format!("Version {n}, as you asked"), &[("n", &ns)])
                    } else {
                        say("event.version", format!("Version {n}"), &[("n", &ns)])
                    };
                }
                asked = false;
            }
            "decided" => {
                let bound = c.answers.iter().find(|a| a.envelope_number == Some(talk[i].number)).map(|a| a.bound_object_version.max(0) as u32).unwrap_or(0);
                if bound > 0 && bound < current {
                    if let Some(ev) = talk[i].event.as_mut() {
                        ev.version = Some(bound);
                        let ns = bound.to_string();
                        ev.label = say("event.decided.version", format!("You chose in version {bound}"), &[("n", &ns)]);
                    }
                }
            }
            _ => {}
        }
    }
    // (a What?? that nothing answered yet)
    for i in 0..talk.len() {
        let bare = talk[i].kind == "event" && talk[i].event.as_ref().is_some_and(|e| e.kind == "explain");
        if bare && !talk.iter().any(|r| r.answers.as_deref() == Some(talk[i].id.as_str())) {
            talk[i].waits = Some(say("card.explain.waits", "Waiting for the explanation.", &[]));
        }
    }
    talk
}

/// The way on once a card is settled: the next open question in the Desk's order after the place it had (the core's
/// walk, `next_open_after`, kept to the team in view), else the first that waits (the web's `nextOpen`).
fn after_of(b: &Board, v: &CardView, walk: &[String]) -> Option<AfterView> {
    if v.status == "open" || v.status == "catching" || v.status.is_empty() {
        return None;
    }
    let here: Vec<String> = b.agents.iter().filter(|a| a.on_team).map(|a| a.row.key.clone()).collect();
    let next = b
        .model
        .next_open_after(&v.id, Some(&here))
        .filter(|n| walk.contains(n))
        .or_else(|| walk.iter().find(|x| **x != v.id).cloned())
        .filter(|n| *n != v.id);
    let next = next.map(|id| {
        let title = match (b.model.cards.get(&id), b.model.permissions.get(&id)) {
            (Some(c), _) => shown_title(c),
            (None, Some(p)) => format!("Approval: {}", p.tool_name),
            _ => String::new(),
        };
        CardRefView { number: b.number_of(&id), id, title }
    });
    let (says, go) = if next.is_some() {
        (say("card.next", "Next question", &[]), say("card.next.go", "Next", &[]))
    } else {
        (say("card.next.none", "That was the last open question.", &[]), say("card.next.done", "All done", &[]))
    };
    Some(AfterView { next, says, go })
}

/// The page's small fixed words; `talk`: how many rows its talk holds (comments, events and versions).
fn words_of(talk: usize) -> super::CardWordsView {
    let n = talk.to_string();
    super::CardWordsView {
        recommended: say("card.recommended", "Recommended", &[]),
        got_it: say("card.got_it", "Got it", &[]),
        duck: say("card.duck", "Duck it", &[]),
        what: say("card.what", "What??", &[]),
        rework: say("card.rework", "Rework", &[]),
        undo: say("card.undo", "Undo", &[]),
        talk: (talk > 0).then(|| say("card.talk.fold", format!("Talk · {n}"), &[("n", &n)])),
    }
}

/// The page of the card or request `id` (its id, or its number of the address); none when this device does not
/// hold it. `walk`: what waits on the Desk in view, in its order (`DeskView::stack`).
pub fn card(b: &Board, id: &str, walk: &[String]) -> Option<CardView> {
    let mut v = page(b, id, walk)?;
    // (the words as the page draws them: an Artifact link by its title)
    let assets = b.assets();
    v.body = titled(&v.body, assets);
    v.text = titled(&v.text, assets);
    for x in &mut v.sections {
        x.text = titled(&x.text, assets);
    }
    v.after = after_of(b, &v, walk);
    v.words = words_of(v.talk.len() + v.versions.len());
    // The comments' window: the newest page stands, each "Load earlier (N)" shows a page more above it.
    let pages = Lens::pages(&b.lens.comments_window, &v.id);
    let left = v.talk.len().saturating_sub(COMMENTS_PAGE * pages as usize);
    if left > 0 {
        v.talk.drain(..left);
    }
    if left > 0 || v.has_more {
        let n = left.to_string();
        let says = if left > 0 {
            say("card.earlier", format!("Load earlier ({n})"), &[("left", &n)])
        } else {
            say("card.earlier.hub", "Earlier comments", &[])
        };
        v.earlier = Some(MoreView {
            left: left as u32,
            hub: v.has_more,
            says,
            pages: pages + 1,
        });
    }
    Some(v)
}

fn page(b: &Board, id: &str, walk: &[String]) -> Option<CardView> {
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
                detail: plain(s(o, "detail"), b.assets()),
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
            notes.push(say("row.replaces.one", "replaces 1 question", &[("n", "1")]));
        } else if merged.len() > 1 {
            let n = merged.len().to_string();
            notes.push(say("row.replaces", format!("replaces {n} questions"), &[("n", &n)]));
        }
        if c.history.len() > 1 {
            notes.push(say("row.revised", "revised", &[]));
        }
    }
    if below == 1 {
        notes.push(say("card.below.one", "1 message below", &[("n", "1")]));
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
        c.history
            .windows(2)
            .map(|w| {
                let n = w[0].object_version.max(0);
                // (the version as it was: its body, when this device holds it)
                let body = c.versions.iter().find(|v| v.object_version == n).and_then(|v| v.content.as_ref()).map(|r| r.obj());
                let text = body.as_ref().map(|o| {
                    let get = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                    let sections = o.get("sections").and_then(Value::as_array).cloned().unwrap_or_default();
                    let whole = if sections.is_empty() {
                        get("body")
                    } else {
                        sections.iter().filter(|x| x.get("key").is_none_or(Value::is_null)).filter_map(|x| x.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n\n")
                    };
                    titled(&fit_text(&whole).0, b.assets())
                });
                let options = body
                    .as_ref()
                    .and_then(|o| o.get("options"))
                    .and_then(Value::as_array)
                    .map(|l| {
                        l.iter()
                            .map(|o| OptionView {
                                key: o.get("key").and_then(Value::as_str).unwrap_or("").into(),
                                label: o.get("label").and_then(Value::as_str).unwrap_or("").into(),
                                detail: plain(o.get("detail").and_then(Value::as_str).unwrap_or(""), b.assets()),
                                ..Default::default()
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let attachments = body.as_ref().and_then(|o| o.get("attachments")).cloned().unwrap_or(Value::Null);
                let pictures = body
                    .as_ref()
                    .map(|o| {
                        let sections = o.get("sections").and_then(Value::as_array).cloned().unwrap_or_default();
                        let opts = super::desk::options_of(o.get("options").unwrap_or(&Value::Null));
                        pictures_of(attachments.as_array().map(Vec::as_slice).unwrap_or(&[]), &sections, &opts)
                    })
                    .unwrap_or_default();
                // (its other files: what is no picture of it)
                let files: Vec<String> = file_ids(&attachments).into_iter().filter(|f| !pictures.iter().any(|p| p.file_id == *f)).collect();
                VersionView { n: n as u32, at_ms: w[0].sent_at, title: w[0].title.clone(), note: w[1].note.clone(), loaded: body.is_some(), text: text.unwrap_or_default(), options, pictures, files }
            })
            .collect()
    };
    // ---- what the card does not hold, before the comments ----
    let html = if newer || loading { String::new() } else { field("html") };
    let mut lead: Vec<LeadView> = Vec::new();
    if more {
        lead.push(LeadView { kind: "whole".into(), label: say("card.lead.whole", "The whole text", &[]), text: titled(&body, b.assets()), html: html.clone(), ..Default::default() });
    } else if !html.is_empty() {
        lead.push(LeadView { kind: "with".into(), label: say("card.lead.with", "With the text", &[]), html: html.clone(), ..Default::default() });
    }
    let links = if loading || newer { Vec::new() } else { option_links(&c.field("options")) };
    if !links.is_empty() {
        lead.push(LeadView { kind: "links".into(), label: say("card.lead.links", "Links of the options", &[]), links, ..Default::default() });
    }
    if status == "open" && c.snoozed_until.is_some() {
        lead.push(LeadView { kind: "later".into(), label: say("card.lead.later", "You put it off: it waits on “Later”", &[]), at_ms: c.snoozed_at, ..Default::default() });
    }
    let starred = m.human.get(&format!("star/{id}")).is_some_and(|r| !r.value.is_null() && r.value != Value::Bool(false));
    let other = |id: &str| match m.cards.get(id) {
        Some(x) => card_ref(b, x),
        None => super::CardRefView { id: id.into(), number: 0, title: String::new() },
    };
    let into = field("merged_into_object_id");
    let mut v = CardView {
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
        pictures: if loading || newer { Vec::new() } else { pictures_of(c.field("attachments").as_array().map(Vec::as_slice).unwrap_or(&[]), &sections, &super::desk::options_of(&c.field("options"))) },
        number,
        nr,
        status: status.into(),
        from: from_of(b, agent, Some(c)),
        knock: knock.flatten(),
        work: agent.and_then(|i| work_of(b, c, i, heard_up_to)),
        asked_ms: c.history.last().filter(|_| c.history.len() > 1).map(|h| h.sent_at).unwrap_or(c.created_at),
        place: place_of(&id, walk),
        notes,
        text,
        more,
        html,
        lead,
        sections: said,
        multiple,
        note: agent.and_then(|i| link_of_card(b, c, i, status, settled)),
        answer,
        ask: ask_words(b, agent),
        draft: draft.map(|d| s(d, "note").to_string()).unwrap_or_default(),
        versions,
        star_says: if starred { say("card.star.on", "Starred", &[]) } else { say("card.star", "Star", &[]) },
        talk: thread(c, entries.into_iter().map(|e| e.row).collect()),
        has_more: chat.is_some_and(|t| t.has_more),
        merged_from: merged.iter().map(|x| other(x)).collect(),
        merged_into: (!into.is_empty()).then(|| other(&into)),
        after: None,
        earlier: None,
        words: Default::default(),
        menu: Vec::new(),
        id,
    };
    v.menu = menu_of(&v);
    Some(v)
}

/// The card's ⋯ menu, from what the page shows of it: the questions before and after it while it has a place,
/// the pen while it can be drawn on (an open decision, an info still to be read), its session's conversation when
/// it has one, Wake up while it is put off, Shred while it waits for an answer; focus and copy always.
pub(crate) fn menu_of(v: &CardView) -> Vec<CardMenuItem> {
    let item = |key: &str, en: &str, keys: &str, now_only: bool| CardMenuItem { key: key.into(), label: say(&format!("card.menu.{key}"), en, &[]), keys: keys.into(), now_only };
    let open = v.state == "open";
    let answering = matches!(v.answer.mode.as_str(), "options" | "read");
    let mut menu = Vec::new();
    if let Some(p) = &v.place {
        if p.prev.is_some() {
            menu.push(item("prev", "Previous question", "Shift ↑", true));
        }
        if p.next.is_some() {
            menu.push(item("next", "Next question", "Shift ↓", true));
        }
    }
    if (open && v.kind == "decision") || (answering && v.kind == "info") {
        menu.push(item("draw", "Draw on the card", "", true));
    }
    menu.push(item("focus", "Focus", "", false));
    if v.session_id.is_some() && v.from.is_some() {
        menu.push(item("chat", "Open the conversation", "", false));
    }
    menu.push(item("copy", "Copy", "", false));
    if open && v.kind != "request" && v.snoozed_until.is_some() {
        menu.push(item("wake", "Wake up", "", true));
    }
    if answering {
        menu.push(item("shred", "Shred", "", true));
    }
    menu
}

/// A request for approval as a card's page: what it asks to run, and Allow or Deny.
fn request(b: &Board, p: &PermissionRequest, number: u32, nr: Text, walk: &[String]) -> CardView {
    let agent = b.by_key.get(key_of(&p.session_id, &p.agent_device_id)).copied();
    let read = p.content_state.is_empty() || p.content_state == "ok";
    let expired = p.permission_state == "expired" || (p.permission_state == "pending" && p.expires_at > 0 && p.expires_at < b.now);
    let open = !p.catching_up && p.permission_state == "pending" && !expired;
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
            _ => "",
        };
        AnswerView {
            mode: "said".into(),
            label: Some(match verdict {
                Some("allow") => say("card.said", "Allow", &[]),
                Some(_) => say("card.said", "Deny", &[]),
                None if expired => say("card.said.expired", "Expired", &[]),
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
    let mut v = CardView {
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
        from: from_of(b, agent, None),
        knock: live.then(|| say("row.knock.permission", "Knock! Permission", &[])),
        asked_ms: p.sent_at,
        place: place_of(&p.object_id, walk),
        text: if read { text } else { String::new() },
        more: read && more,
        answer,
        ask: ask_words(b, agent),
        star_says: say("card.star", "Star", &[]),
        ..Default::default()
    };
    v.menu = menu_of(&v);
    v
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

    #[test]
    fn the_comments_show_the_newest_ten_and_ten_more_each_time() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q", "a", "decision", "normal");
        for n in 0..24u64 {
            r.say("a", Some("q"), 200 + n, if n % 2 == 0 { "me" } else { "a" }, (30 - n) * MIN, json!({ "text": format!("comment {n}") }));
        }
        let at = |lens: &Lens| {
            let m = r.model(NOW);
            let b = Board::new(&m, lens, NOW);
            let walk = desk::desk(&b).stack;
            card(&b, "q", &walk).unwrap()
        };
        let mut lens = Lens::default();
        let whole = { let mut l = lens.clone(); l.comments_window = Some(("q".into(), 99)); at(&l).talk.len() };
        assert!(whole > 20, "the asked line and the 24 comments: {whole}");
        let v = at(&lens);
        assert_eq!(v.talk.len(), COMMENTS_PAGE);
        assert_eq!(v.talk.last().map(|t| t.text.as_str()), Some("comment 23"));
        let more = v.earlier.clone().unwrap();
        let left = (whole - COMMENTS_PAGE).to_string();
        assert_eq!((more.left as usize, more.hub, more.pages, more.says.en.clone()), (whole - COMMENTS_PAGE, false, 2, format!("Load earlier ({left})")));
        lens.show_more(crate::views::MoreOf::Comments { card: "q".into() });
        assert_eq!(at(&lens).talk.len(), 2 * COMMENTS_PAGE);
        lens.show_more(crate::views::MoreOf::Comments { card: "q".into() });
        let v = at(&lens);
        assert_eq!((v.talk.len(), v.earlier), (whole, None));
        // (another card's window is its own)
        lens.show_more(crate::views::MoreOf::Comments { card: "other".into() });
        assert_eq!(at(&lens).talk.len(), COMMENTS_PAGE);
    }

    fn menu(v: &CardView) -> Vec<&str> {
        v.menu.iter().map(|m| m.key.as_str()).collect()
    }

    /// The ⋯ beside the title: an open decision can be drawn on and shredded, and walks to its neighbour.
    #[test]
    fn an_open_decision_has_the_whole_menu() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q", "a", "decision", "normal");
        r.card("p", "a", "decision", "normal");
        let v = view(&r, "q");
        let keys = menu(&v);
        assert!(keys.contains(&"prev") || keys.contains(&"next"), "{keys:?}");
        for k in ["draw", "focus", "chat", "copy", "shred"] {
            assert!(keys.contains(&k), "{k} in {keys:?}");
        }
        assert!(!keys.contains(&"wake"), "{keys:?}");
        assert!(v.menu.iter().all(|m| !m.label.en.is_empty()));
        assert_eq!(v.menu.iter().find(|m| m.key == "focus").map(|m| m.now_only), Some(false));
        assert_eq!(v.menu.iter().find(|m| m.key == "shred").map(|m| m.now_only), Some(true));
    }

    /// Once answered, nothing is drawn on or shredded any more; focus and copy stay.
    #[test]
    fn an_answered_card_keeps_only_the_quiet_entries() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.answered("q", "a", NOW - MIN, 500);
        let v = view(&r, "q");
        let keys = menu(&v);
        assert!(!keys.contains(&"draw") && !keys.contains(&"shred"), "{keys:?}");
        assert!(keys.contains(&"focus") && keys.contains(&"copy"), "{keys:?}");
    }

    /// An info still to be read can be drawn on and shredded, like a question.
    #[test]
    fn an_open_info_can_be_drawn_on() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("i", "a", "info", "normal");
        let v = view(&r, "i");
        let keys = menu(&v);
        assert!(keys.contains(&"draw") && keys.contains(&"shred"), "{keys:?}");
    }

    /// A request for approval has no pen and no shredder.
    #[test]
    fn a_request_has_no_pen_and_no_shredder() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.request("perm", "a", "ok");
        let m = r.model(NOW);
        let lens = Lens::default();
        let b = Board::new(&m, &lens, NOW);
        let walk = desk::desk(&b).stack;
        let v = card(&b, "perm", &walk).expect("held");
        let keys = menu(&v);
        assert!(!keys.contains(&"draw") && !keys.contains(&"shred"), "{keys:?}");
        assert!(keys.contains(&"copy"), "{keys:?}");
    }

    /// The menu is always written, an empty one too: a shell never finds the field missing.
    #[test]
    fn the_menu_is_always_written() {
        let v = serde_json::to_value(CardView::default()).unwrap();
        assert_eq!(v.get("menu"), Some(&json!([])));
        let back: CardView = serde_json::from_value(json!({ "id": "x" })).unwrap_or_default();
        assert!(back.menu.is_empty());
    }

    /// The web's `pictureKeys`, case by case.
    #[test]
    fn pictures_find_their_options() {
        let opt = |k: &str, l: &str| crate::views::desk::Opt { key: k.into(), label: l.into(), ..Default::default() };
        let img = |id: &str, name: &str| json!({ "attachment_id": id, "file_name": name, "media_type": "image/png" });
        let keys = |p: &[PictureView]| p.iter().map(|x| x.key.clone().unwrap_or_default()).collect::<Vec<_>>();
        let two = [opt("centered", "Centered"), opt("split", "Split view")];
        // by file name: the option's key or label in it
        let atts = [img("a", "hero-split-view.png"), img("b", "Hero_Centered.PNG"), json!({ "attachment_id": "d", "file_name": "notes.pdf", "media_type": "application/pdf" })];
        assert_eq!(keys(&pictures_of(&atts, &[], &two)), ["split", "centered"]);
        // one owner only: none
        let atts = [img("a", "split.png"), img("b", "other.png")];
        assert_eq!(keys(&pictures_of(&atts, &[], &two)), ["", ""]);
        // as many as options, three or more: in order
        let three = [opt("a", "One"), opt("b", "Two"), opt("c", "Three")];
        let atts = [img("1", "x.png"), img("2", "y.png"), img("3", "z.png")];
        assert_eq!(keys(&pictures_of(&atts, &[], &three)), ["a", "b", "c"]);
        // two options of the same key length in one name: unclear, then in order only with three
        let atts = [img("1", "a-b.png"), img("2", "c.png"), img("3", "z.png")];
        assert_eq!(keys(&pictures_of(&atts, &[], &three)), ["a", "b", "c"]);
        // a section decides, by its place among the attachments; two sections may share one picture
        let atts = [json!({ "attachment_id": "doc", "media_type": "text/html" }), img("p", "anything.png")];
        let sections = [json!({ "key": "centered", "picture": 1 }), json!({ "key": "split", "picture": 1 })];
        let p = pictures_of(&atts, &sections, &two);
        assert_eq!((p[0].key.as_deref(), p[0].keys.clone()), (Some("centered"), vec!["centered".to_string(), "split".into()]));
    }

    /// The viewer's line under a picture: its caption, else its option's label, else its file name.
    #[test]
    fn pictures_say_their_caption() {
        let opt = |k: &str, l: &str| crate::views::desk::Opt { key: k.into(), label: l.into(), ..Default::default() };
        let two = [opt("centered", "Centered"), opt("split", "Split view")];
        let atts = [
            json!({ "attachment_id": "a", "file_name": "hero-split.png", "media_type": "image/png", "title": "1 · Folded" }),
            json!({ "attachment_id": "b", "file_name": "hero-centered.png", "media_type": "image/png" }),
            json!({ "attachment_id": "c", "file_name": "today.png", "media_type": "image/png", "title": "today.png" }),
        ];
        let p = pictures_of(&atts, &[], &two);
        let says = p.iter().map(|x| x.caption.as_str()).collect::<Vec<_>>();
        assert_eq!(says, ["1 · Folded", "Centered", "today.png"]);
        assert_eq!(p[2].title, "");
    }

    /// The comments grouped as the page draws them: "Version 2, as you asked" after a hand-back, the agent's words
    /// answering a bare What??, an old version as it was.
    #[test]
    fn the_thread_says_versions_and_explanations() {
        let mut r = Room::default();
        r.session("a", None, true);
        let c = r.card("q", "a", "decision", "normal");
        c["versions"] = json!([
            { "object_version": 1, "envelope_number": 101, "sent_at": NOW - 9 * MIN, "object_state": "open",
              "content": { "title": "First", "body": "Old words.\n\n```\ncode\n```", "options": [{ "key": "y", "label": "Yes", "detail": "**now**" }] } },
            { "object_version": 2, "envelope_number": 120, "sent_at": NOW - 4 * MIN, "object_state": "open", "content": { "title": "q", "change_note": "Shorter." } },
        ]);
        r.say("a", Some("q"), 110, "me", 8 * MIN, json!({ "text": crate::views::talk::HAND_BACK_TEXT, "hand_back": true }));
        r.say("a", Some("q"), 130, "me", 3 * MIN, json!({ "text": crate::views::talk::EXPLAIN_TEXT, "explain": true }));
        r.say("a", Some("q"), 131, "a", 2 * MIN, json!({ "text": "It means this." }));
        r.say("a", Some("q"), 140, "me", MIN, json!({ "text": crate::views::talk::EXPLAIN_TEXT, "explain": true }));
        let v = view(&r, "q");
        let rev = v.talk.iter().find(|x| x.id == "v120r").unwrap().event.clone().unwrap();
        assert_eq!((rev.version, rev.asked, rev.label.en.as_str()), (Some(2), true, "Version 2, as you asked"));
        let answer = v.talk.iter().find(|x| x.text == "It means this.").unwrap();
        assert_eq!(answer.answers.as_deref(), Some("e130"));
        let last = v.talk.iter().find(|x| x.id == "e140").unwrap();
        assert_eq!(last.waits.as_ref().map(|t| t.en.as_str()), Some("Waiting for the explanation."));
        assert!(v.talk.iter().find(|x| x.id == "e130").unwrap().waits.is_none());
        let old = &v.versions[0];
        assert_eq!((old.n, old.loaded, old.text.as_str()), (1, true, "Old words.\n\n*↓ code, in the whole text below*"));
        assert_eq!(old.options.iter().map(|o| (o.key.as_str(), o.detail.as_str())).collect::<Vec<_>>(), [("y", "now")]);
    }

    /// What the card does not hold leads its comments; an answer says the labels of the version it answered, with
    /// what he wrote and sent along; an old version keeps its pictures.
    #[test]
    fn the_thread_leads_with_what_the_card_does_not_hold() {
        let mut r = Room::default();
        r.session("a", None, true);
        let c = r.card("q", "a", "decision", "normal");
        c["body"] = json!("Look:\n```\ncode\n```");
        c["html"] = json!("<table></table>");
        c["options"] = json!([
            { "key": "a", "label": "Plan A https://example.com/a.", "detail": "see /docs/plan.html?x=1#top, then" },
            { "key": "b", "label": "https://www.trommi.com", "detail": "" },
            { "key": "c", "label": "Plain", "detail": "no link: a/b.html and http//x" },
        ]);
        c["versions"] = json!([
            { "object_version": 1, "envelope_number": 101, "sent_at": NOW - 9 * MIN, "object_state": "open",
              "content": { "title": "First", "body": "Old.", "options": [{ "key": "a", "label": "Old A" }, { "key": "b", "label": "Old B" }],
                "attachments": [{ "attachment_id": "p1", "file_name": "a.png", "media_type": "image/png" }, { "attachment_id": "f1", "file_name": "notes.pdf", "media_type": "application/pdf" }] } },
            { "object_version": 2, "envelope_number": 120, "sent_at": NOW - 4 * MIN, "object_state": "open", "content": { "title": "q" } },
        ]);
        c["answers"] = json!([{ "answer_action": "answer", "choices": ["a"], "note": "because", "option_notes": { "b": "maybe", "a": "yes" },
            "attachments": [{ "attachment_id": "f2", "file_name": "x.txt", "media_type": "text/plain" }], "bound_object_version": 1,
            "envelope_number": 115, "answered_at": NOW - 5 * MIN, "taken_back_at": 118, "taken_back_sent_at": NOW - 5 * MIN + 1 }]);
        r.snoozes = json!({ "q": { "until": NOW + 60 * MIN, "at": NOW - MIN } });
        let v = view(&r, "q");
        let kinds: Vec<&str> = v.lead.iter().map(|l| l.kind.as_str()).collect();
        assert_eq!(kinds, ["whole", "links", "later"]);
        assert_eq!(v.lead[2].label.en, "You put it off: it waits on “Later”");
        assert_eq!((v.lead[0].label.en.as_str(), v.lead[0].text.as_str(), v.lead[0].html.as_str()), ("The whole text", "Look:\n```\ncode\n```", "<table></table>"));
        let links: Vec<(&str, &str, &str, &str)> = v.lead[1].links.iter().map(|l| (l.key.as_str(), l.label.as_str(), l.url.as_str(), l.short.as_str())).collect();
        assert_eq!(
            links,
            [
                ("a", "Plan A", "https://example.com/a", "example.com/a"),
                ("a", "Plan A", "/docs/plan.html?x=1#top", "/docs/plan.html?x=1#top"),
                ("b", "b", "https://www.trommi.com", "trommi.com"),
            ]
        );
        let chose = v.talk.iter().find(|x| x.id == "v115d").unwrap().event.clone().unwrap();
        assert_eq!((chose.text.as_str(), chose.label.en.as_str()), ("Old A", "You chose in version 1"));
        assert_eq!(chose.said.iter().map(|t| t.en.as_str()).collect::<Vec<_>>(), ["“because”", "On Old A: “yes”", "On Old B: “maybe”"]);
        assert_eq!(chose.files, ["f2"]);
        let old = &v.versions[0];
        assert_eq!((old.pictures.iter().map(|p| p.file_id.as_str()).collect::<Vec<_>>(), old.files.clone()), (vec!["p1"], vec!["f1".to_string()]));

        // only the agent's layout, the words all on the card
        let c = &mut r.cards[0];
        c["body"] = json!("Short.");
        c["options"] = json!([]);
        let v = view(&r, "q");
        assert_eq!(v.lead.iter().map(|l| (l.kind.as_str(), l.label.en.as_str())).collect::<Vec<_>>(), [("with", "With the text"), ("later", "You put it off: it waits on “Later”")]);
        // (not put off: no line of it)
        r.snoozes = json!({});
        assert_eq!(view(&r, "q").lead.iter().map(|l| l.kind.as_str()).collect::<Vec<_>>(), ["with"]);
    }

    /// Settled, a card's page offers the next open question of the team in view, else says none is left.
    #[test]
    fn after_an_answer_the_next_question() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.answered("done", "a", NOW - MIN, 300);
        r.card("q1", "a", "decision", "normal");
        r.card("q2", "a", "decision", "normal");
        let v = view(&r, "done");
        let after = v.after.unwrap();
        assert_eq!((after.next.as_ref().map(|n| n.id.as_str()), after.says.en.as_str(), after.go.en.as_str()), (Some("q1"), "Next question", "Next"));
        // an open card offers none: the shell shows it right after an answer only
        assert!(view(&r, "q1").after.is_none());
        r.cards.retain(|c| c["object_id"] == "done");
        let after = view(&r, "done").after.unwrap();
        assert_eq!((after.next, after.says.en.as_str(), after.go.en.as_str()), (None, "That was the last open question.", "All done"));
    }

    /// The calm page's small words: the side ways, Undo, and the talk folded with its count (none without talk).
    #[test]
    fn the_calm_words_and_the_folded_talk() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q", "a", "decision", "normal");
        let v = view(&r, "q");
        let w = &v.words;
        assert_eq!(
            [&w.recommended, &w.got_it, &w.duck, &w.what, &w.rework, &w.undo].map(|t| t.en.as_str()),
            ["Recommended", "Got it", "Duck it", "What??", "Rework", "Undo"]
        );
        let rows = v.talk.len() + v.versions.len();
        assert_eq!(w.talk.as_ref().map(|t| t.en.clone()), (rows > 0).then(|| format!("Talk · {rows}")));
        assert_eq!(words_of(0).talk, None);
        assert_eq!(words_of(3).talk.map(|t| (t.en, t.args.get("n").cloned())), Some(("Talk · 3".into(), Some("3".into()))));
    }

    /// An Artifact link on the card's page is named by its title (markdown the shell draws); on the Desk's row too,
    /// where the agent's layout and a table stand in no line.
    #[test]
    fn artifact_links_are_named_by_their_titles() {
        let mut r = Room::default();
        r.session("a", None, true);
        let url = format!("https://app.trommi.com/artifact/0123456789abcdef#{}", "k".repeat(43));
        r.published.push(json!({ "object_id": "0123456789abcdef", "title": "Launch plan", "object_state": "open", "attachments": [{ "media_type": "text/html" }] }));
        r.card("q", "a", "decision", "normal")["body"] = json!(format!("The plan: {url}\n```html\n<b>tiles</b>\n```\n| a | b |\n|---|---|"));
        let v = view(&r, "q");
        assert!(v.body.starts_with(&format!("The plan: [Launch plan]({url})")), "{}", v.body);
        let m = r.model(NOW);
        let lens = Lens::default();
        let row = desk::desk(&Board::new(&m, &lens, NOW)).rows.into_iter().find(|x| x.id == "q").unwrap();
        assert_eq!(row.words, "The plan: [Launch plan]");
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
        let mut r = Room { teams: json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } }), ..Default::default() };
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
        // (b is a helper session of before: its card stands in its main's path, the helper named beside it)
        assert_eq!(v.from.as_ref().map(|f| f.path.clone()), Some(vec!["Work".to_string(), "a".into()]));
        assert_eq!(v.from.as_ref().and_then(|f| f.helper.as_ref()).map(|t| t.en.as_str()), Some("Helper b"));
        assert_eq!(notes(&v), ["replaces 2 questions", "revised", "2 messages below"]);
        // (what blocks stands first on the Desk: its place among what waits, with its neighbours)
        let p = v.place.as_ref().unwrap();
        assert_eq!((p.at, p.of, p.says.en.as_str(), p.prev.as_deref(), p.next.as_deref()), (1, 3, "1 of 3", None, Some("p1")));
        assert_eq!((v.history, v.asked_ms), (1, NOW - 4 * MIN));
        assert_eq!(v.versions.iter().map(|x| (x.n, x.title.as_str(), x.note.as_str(), x.at_ms)).collect::<Vec<_>>(), [(1, "First try", "Shorter.", NOW - 9 * MIN)]);
        assert_eq!((v.starred, v.star_says.en.as_str(), v.ask.en.as_str()), (true, "Starred", "Ask a something, or say what is missing…"));
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
    fn an_expired_approval_has_one_shared_expired_label_and_no_live_verdict_buttons() {
        for state in ["pending", "expired"] {
            let mut r = Room::default();
            r.session("a", None, true);
            r.request("late", "a", "ok");
            r.permissions[0]["permission_state"] = json!(state);
            r.permissions[0]["expires_at"] = json!(NOW - 1);
            let v = view(&r, "late");
            assert_eq!(v.state, "closed");
            assert_eq!(v.answer.mode, "said");
            assert_eq!(en(&v.answer.label), "Expired");
            assert!(v.answer.summary.is_empty());
        }
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
