//! The sidebar: the teams, and the sessions of the team in view as rows, in the order they are drawn. A shell draws
//! the rows one after the other and decides nothing: which sessions show, connected or away, what waits, who is
//! stopped and why, the small line under a name.
//!
//! The rules are the web's of before (`app.mjs` `boardModel` units, `sidebar.mjs` `sidebarParts` and `desksOf`,
//! `ui.mjs` `badge` and `linkCap`):
//! - a row per session that is not archived and stands on the team in view;
//! - chat rows follow newest actual message first, with immutable admission time for empty chats and the
//!   session list's manual/id order for ties; connection state only changes the row's presentation;
//! - a row's open questions are those of its session on the Desk now (its helpers' too: they write into it);
//! - a session's helpers (its agent's subagents, `helpers.rs`) have no row: its row says how many there are, with
//!   a dot each that is filled while that helper works ([`HelperDotsView`]).

use super::board::{key_of, Board, DEFAULT_TEAM};
use super::links::plain;
use super::words::{badge_words, hand_words, link_words, local, next_midnight, say, unheard_words, LinkSubject};
use super::{CapView, ChatGroupView, GlobalBlitzView, HandView, HelperDotsView, LastSaidView, LinkWords, NewChatView, TeamPlaceView, SessionRowView, SidebarView, StackEdgeView, TeamStackView, TeamTab, Text, STACK_EDGES};
use crate::model::Model;
use serde_json::Value;
use std::collections::HashMap;

/// The Chat list's teaser is cut to so many characters.
pub const TEASER_MAX: usize = 200;

/// A chat item's sender is the person (one of his devices).
fn from_human(m: &Model, item: &crate::model::types::TimelineItem) -> bool {
    let dev = item.get("sender_device_id").and_then(Value::as_str).unwrap_or("");
    m.my_device_id.as_deref() == Some(dev) || m.members.get(dev).is_some_and(|x| x.device_role == "human")
}

/// A chat item that is something said: a message whose words are read here, not a step of a turn's trail.
fn said(item: &crate::model::types::TimelineItem) -> Option<&Value> {
    let state = item.get("item_state").and_then(Value::as_str).unwrap_or("");
    let kind = item.get("content_type").and_then(Value::as_str).unwrap_or("message");
    if kind != "message" || !matches!(state, "" | "loaded") {
        return None;
    }
    let c = item.get("content")?;
    (c.get("terminal").and_then(Value::as_str) != Some("work")).then_some(c)
}

/// The newest envelope number of what a session's agent said in its chat (`key`: the session's key); none while
/// this device holds nothing it said. The Chat list's dot compares it with `Lens::seen`.
pub fn newest_word(m: &Model, key: &str) -> Option<u64> {
    let t = m.timelines.get(&format!("chat:session/{key}"))?;
    t.items.values().rev().find_map(|i| {
        let c = said(i)?;
        let typed = c.get("terminal").and_then(Value::as_str) == Some("input");
        (!from_human(m, i) && !typed).then(|| i.get("envelope_number").and_then(Value::as_u64)).flatten()
    })
}

/// The newest word of every session this device holds a chat of, by session key.
pub fn newest_words(m: &Model) -> std::collections::BTreeMap<String, u64> {
    m.timelines
        .keys()
        .filter_map(|k| k.strip_prefix("chat:session/"))
        .filter_map(|key| newest_word(m, key).map(|n| (key.to_string(), n)))
        .collect()
}

/// When, as a chat list says it: today the time, yesterday "Yesterday", within the week the weekday, this year the
/// day and month, else the date (the iPhone's `ChatTeaser.stamp`).
pub fn chat_when(ts: u64, now: u64, zone: i32) -> Text {
    const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (a, n) = (local(ts, zone), local(now, zone));
    let days = n.days - a.days;
    if days <= 0 {
        let time = format!("{:02}:{:02}", a.hour, a.minute);
        say("chat.when.today", time.clone(), &[("time", &time)])
    } else if days == 1 {
        say("chat.when.yesterday", "Yesterday", &[])
    } else if days < 7 {
        let weekday = WEEKDAYS[(a.days + 4).rem_euclid(7) as usize];
        say("chat.when.weekday", weekday, &[("weekday", weekday)])
    } else if a.year == n.year {
        let (day, month) = (a.day.to_string(), MONTHS[(a.month - 1) as usize]);
        say("chat.when.date", format!("{day} {month}"), &[("day", &day), ("month", month)])
    } else {
        let date = format!("{:02}.{:02}.{:02}", a.day, a.month, a.year.rem_euclid(100));
        say("chat.when.long", date.clone(), &[("date", &date)])
    }
}

/// The newest question each session put to the person (its time and title), by index of `Board::agents`.
fn asked_last(b: &Board) -> HashMap<usize, (u64, String)> {
    let mut out: HashMap<usize, (u64, String)> = HashMap::new();
    for c in b.model.cards.values() {
        let Some(&i) = b.by_key.get(key_of(&c.session_id, &c.agent_device_id)) else { continue };
        let title = c.field("title").as_str().unwrap_or("").trim().to_string();
        if title.is_empty() || out.get(&i).is_some_and(|(at, _)| *at >= c.created_at) {
            continue;
        }
        out.insert(i, (c.created_at, title));
    }
    out
}

/// What a session's chat said last: his words ("You: …") or the agent's, or the question it put to him last.
fn last_said(b: &Board, agent: usize, asked: Option<&(u64, String)>) -> Option<LastSaidView> {
    let m = b.model;
    let key = &b.agents[agent].row.key;
    let words = m.timelines.get(&format!("chat:session/{key}")).and_then(|t| {
        t.items.values().rev().find_map(|i| {
            let c = said(i)?;
            let mut text = plain(c.get("text").and_then(Value::as_str).unwrap_or(""), b.assets());
            if text.is_empty() {
                let n = c.get("attachments").and_then(Value::as_array).map_or(0, Vec::len);
                text = match n {
                    0 => return None,
                    1 => "Attachment".into(),
                    n => format!("{n} attachments"),
                };
            }
            let mine = from_human(m, i) || c.get("terminal").and_then(Value::as_str) == Some("input");
            let at = i.get("sent_at").and_then(Value::as_u64).unwrap_or(0);
            Some((at, if mine { format!("You: {text}") } else { text }))
        })
    });
    let (at, text) = match (words, asked) {
        (Some(w), Some(q)) if q.0 > w.0 => q.clone(),
        (Some(w), _) => w,
        (None, Some(q)) => q.clone(),
        (None, None) => return None,
    };
    let text: String = text.chars().take(TEASER_MAX).collect();
    Some(LastSaidView { text, at_ms: at, when: chat_when(at, b.now, b.lens.utc_offset_min) })
}

/// Its agent said something this device has not shown yet.
fn unread(b: &Board, agent: usize) -> bool {
    let Some(seen) = b.lens.seen.as_ref() else { return false };
    let key = &b.agents[agent].row.key;
    newest_word(b.model, key).is_some_and(|n| n > seen.get(key).copied().unwrap_or(0))
}

/// One row before it is laid out: a session with what waits on it.
pub(super) struct Unit {
    /// Index into `Board::agents`.
    pub agent: usize,
    pub open: u32,
    /// Its open cards, infos too, and its requests (`Board::all_cards`): the calm grey count.
    pub cards: u32,
    pub knocks: bool,
    pub online: bool,
    pub working: bool,
    pub hand: Option<HandView>,
    pub link: Option<LinkWords>,
    pub unheard: u32,
}

/// A session's link in words (none for an archived or removed one), with the wake noted on the board.
pub fn link_of(b: &Board, agent: usize) -> Option<LinkWords> {
    let a = &b.agents[agent];
    if a.row.archived || a.row.removed {
        return None;
    }
    let v = a.row.link.as_ref()?;
    let who = LinkSubject {
        name: &a.row.name,
        online: a.row.online,
        codex: a.codex,
        said_goodbye: a.said_goodbye,
    };
    let (words, wake) = link_words(&who, v, b.now, b.lens.utc_offset_min);
    b.wake_at(wake);
    Some(words)
}

/// Why a session is stopped, in words (none for an archived one), with the wake noted on the board.
pub fn hand_of(b: &Board, agent: usize) -> Option<HandView> {
    let a = &b.agents[agent];
    let blocked = a.row.blocked.as_ref().filter(|_| !a.row.archived)?;
    let short = if blocked.why == "cut" { link_of(b, agent).and_then(|l| l.short) } else { None };
    let (hand, wake) = hand_words(blocked, short.as_ref(), b.now, b.lens.utc_offset_min);
    b.wake_at(wake);
    Some(hand)
}

fn team_name(name: &str) -> Text {
    if name.trim().is_empty() || name == "Personal" {
        say("team.personal", "Personal", &[])
    } else {
        say("team.name", name, &[])
    }
}

/// The units of the team in view: one per session that is not archived and stands on it, in the list's order.
pub(super) fn units_of(b: &Board) -> Vec<Unit> {
    units_where(b, |a| a.on_team)
}

/// So many of a team's sessions make its picture.
const TEAM_FACES: usize = 3;

/// The units of every team's sessions that are not archived (home's Desk).
pub(super) fn units_all(b: &Board) -> Vec<Unit> {
    units_where(b, |_| true)
}

/// The units of the sessions that are not archived and pass `on`, in the list's order.
fn units_where(b: &Board, on: impl Fn(&super::board::Agent) -> bool) -> Vec<Unit> {
    // ---- the units: one per session that stands here ----
    let here: Vec<usize> = (0..b.agents.len()).filter(|i| !b.agents[*i].row.archived && on(&b.agents[*i])).collect();
    // (a session's questions are its own wherever its team is: Fresh.agent names it)
    let fresh_of = |i: usize| b.all_fresh.iter().filter(move |f| f.agent == Some(i));
    let units: Vec<Unit> = here
        .iter()
        .map(|&i| {
            let a = &b.agents[i];
            Unit {
                agent: i,
                open: fresh_of(i).count() as u32,
                cards: b.all_cards.iter().filter(|f| f.agent == Some(i)).count() as u32,
                knocks: fresh_of(i).any(|f| f.knocks),
                online: a.row.online,
                working: a.row.online && a.row.working,
                hand: hand_of(b, i),
                link: link_of(b, i),
                unheard: b.unheard.get(&i).copied().unwrap_or(0),
            }
        })
        .collect();
    units
}

/// The person's initial from the account's e-mail: its first letter or digit, upper case; empty without one.
pub fn initial_of(email: Option<&str>) -> String {
    email.and_then(|e| e.chars().find(|c| c.is_alphanumeric())).map(|c| c.to_uppercase().collect()).unwrap_or_default()
}

pub fn sidebar(b: &Board) -> SidebarView {
    let units = units_of(b);

    // ---- the teams ----
    let teams: Vec<(String, String)> = if b.teams.is_empty() {
        vec![(DEFAULT_TEAM.to_string(), "Personal".to_string())]
    } else {
        b.teams.iter().map(|d| (d.id.clone(), d.name.clone())).collect()
    };
    let first = teams[0].0.clone();
    // (what waits on a chat on no team counts on no team: on home, and on the bolt over every team; what belongs to a
    //  session the list does not show counts nowhere)
    let team_of = |agent: Option<usize>| match agent {
        Some(i) if b.agents[i].root => None,
        Some(_) => Some(b.team_of(agent).unwrap_or(first.as_str())),
        None => None,
    };
    let waits_on = |team: &str| {
        let mine = b.all_fresh.iter().filter(|f| team_of(f.agent) == Some(team));
        (mine.clone().count() as u32, mine.clone().any(|f| f.knocks))
    };
    let cards_on = |team: &str| b.all_cards.iter().filter(|f| team_of(f.agent) == Some(team)).count() as u32;
    let in_view = b.team.clone().unwrap_or_else(|| first.clone());
    let tabs: Vec<TeamTab> = teams
        .iter()
        .map(|(id, name)| {
            let (waiting, knocks) = waits_on(id);
            TeamTab {
                id: id.clone(),
                name: team_name(name),
                waiting,
                cards: cards_on(id),
                selected: *id == in_view && !b.lens.desk_all,
                knocks,
            }
        })
        .collect();
    // The tab's number: the open questions of the team in view, never red (the owner, 13 October 2026, card
    // c628976b: "Nur der Desk im Blick"; the web counted every team and showed red on a knock). What waits on the
    // other teams stands in the menu, team by team.
    // On home the Desk in view is every team's, and so are its numbers.
    let on_desk = || b.all_fresh.iter().filter(|f| b.on_desk(f.agent));
    let badge = on_desk().count() as u32;
    // (home: what every row shows, so not what belongs to a session the list does not show)
    let shown_fresh = || b.all_fresh.iter().filter(|f| f.agent.is_some());
    let home = TeamTab {
        id: String::new(),
        name: say("team.all", "All teams", &[]),
        waiting: shown_fresh().count() as u32,
        cards: b.all_cards.len() as u32,
        selected: b.lens.desk_all,
        knocks: shown_fresh().any(|f| f.knocks),
    };
    let place = if b.lens.desk_all {
        say("team.all", "All teams", &[])
    } else {
        team_name(teams.iter().find(|(id, _)| *id == in_view).map(|(_, n)| n.as_str()).unwrap_or(""))
    };

    // ---- the rows, in drawing order ----
    let asked = asked_last(b);
    let rows = rows_of(b, &units, &asked);

    // ---- every team as a stack (the wide sidebar's first level) ----
    let stacks: Vec<TeamStackView> = tabs
        .iter()
        .map(|t| {
            let sessions = if t.id == in_view {
                rows.clone()
            } else {
                // (in a room without teams there is one, and it is in view)
                let mine = units_where(b, |a| a.team.as_deref() == Some(t.id.as_str()));
                rows_of(b, &mine, &asked)
            };
            TeamStackView {
                id: t.id.clone(),
                name: t.name.clone(),
                selected: t.selected,
                open: !sessions.is_empty() && b.lens.open_teams.contains(&t.id),
                waiting: t.waiting,
                cards: t.cards,
                knocks: t.knocks,
                says: stack_words(sessions.len(), t.waiting),
                edges: sessions.iter().take(STACK_EDGES).map(|r| StackEdgeView { id: r.id.clone(), mark: r.mark.clone() }).collect(),
                // (a group's picture: the crowned one first, then in the rows' order)
                faces: sessions.iter().filter(|r| r.crowned).chain(sessions.iter().filter(|r| !r.crowned)).take(TEAM_FACES).map(|r| StackEdgeView { id: r.id.clone(), mark: r.mark.clone() }).collect(),
                sessions,
            }
        })
        .collect();

    // ---- the chats on no team: at the top of the list, alongside the teams ----
    let root = rows_of(b, &units_where(b, |a| a.root), &asked);

    // ---- the search among every team's chats (the phone's Search tab), the chats on no team first ----
    let found: Vec<ChatGroupView> = match &b.lens.chats_query {
        Some(q) => std::iter::once(ChatGroupView { id: String::new(), name: say("chats.root", "Chats", &[]), rows: root.iter().filter(|r| matches(r, q)).cloned().collect() })
            .chain(stacks.iter().map(|s| ChatGroupView { id: s.id.clone(), name: s.name.clone(), rows: s.sessions.iter().filter(|r| matches(r, q)).cloned().collect() }))
            .filter(|g| !g.rows.is_empty())
            .collect(),
        None => Vec::new(),
    };
    let found_none = b.lens.chats_query.as_ref().filter(|_| found.is_empty()).map(|q| say("chats.none.found", format!("No chat matches \u{201c}{q}\u{201d}."), &[("query", q)]));

    // ---- the bolt over every team (the phone's top right): the walk of every team's Desk, whatever team is in view;
    // its first card is the first of that Desk's rows (one whose session the list shows) ----
    let all = super::desk::walk_of(b, |_| true);
    let shown = |id: &str| b.all_cards.iter().any(|f| f.id == id) && (b.model.cards.contains_key(id) || b.model.permissions.contains_key(id));
    let global_blitz = all.iter().find(|id| shown(id)).map(|id| GlobalBlitzView { card: id.to_string(), says: super::desk::blitz_words(all.len() as u32) });

    // (a teaser's "14:05" turns into "Yesterday" at midnight)
    if stacks.iter().flat_map(|s| s.sessions.iter()).chain(rows.iter()).chain(root.iter()).any(|r| r.last.is_some()) {
        b.wake_at(Some(next_midnight(b.now, b.lens.utc_offset_min)));
    }
    // ---- "New chat with …": the chat agents that start a new chat when asked, once each (their main session) ----
    let new_chat: Vec<NewChatView> = b
        .agents
        .iter()
        .filter(|a| a.row.main && !a.row.archived && !a.row.removed && !a.row.replaced)
        .filter(|a| {
            a.row.session_id.as_ref().and_then(|s| b.model.sessions.get(s)).and_then(|s| s.profile.as_ref()).and_then(|p| p.get("chats")).and_then(Value::as_bool) == Some(true)
        })
        .filter_map(|a| Some(NewChatView { session_id: a.row.session_id.clone()?, slug: a.row.id.clone(), name: say("agent.name", a.row.name.clone(), &[("name", a.row.name.as_str())]), mark: a.row.mark() }))
        .collect();
    SidebarView {
        places: places(b),
        root,
        me: String::new(),
        new_chat,
        found,
        found_none,
        global_blitz,
        stacks,
        desk_all: b.lens.desk_all,
        teams: tabs,
        home,
        sessions: rows,
        badge,
        team: in_view,
        place,
        waiting: badge,
        knocking: on_desk().filter(|f| f.knocks).count() as u32,
        stopped: units.iter().filter(|u| u.hand.is_some()).count() as u32,
        working: units.iter().filter(|u| u.working).count() as u32,
        unheard: units.iter().map(|u| u.unheard).sum(),
    }
}

/// "No team", then the teams in the menu's order, the team in view chosen ("No team" in a room without teams). Where an
/// agent invited now can stand (`SidebarView.places`).
pub(super) fn places(b: &Board) -> Vec<TeamPlaceView> {
    std::iter::once(TeamPlaceView { team_id: None, name: say("place.none", "No team", &[]), chosen: b.teams.is_empty() })
        .chain(b.teams.iter().map(|d| TeamPlaceView { team_id: Some(d.id.clone()), name: team_name(&d.name), chosen: b.team.as_deref() == Some(d.id.as_str()) }))
        .collect()
}

/// Newest actual message this device holds, never a card update, status, live preview or lease tick.
fn message_time(b: &Board, agent: usize) -> u64 {
    let key = &b.agents[agent].row.key;
    b.model.timelines.get(&format!("chat:session/{key}")).into_iter()
        .flat_map(|t| t.items.values())
        .filter_map(|i| {
            let content = said(i)?;
            if from_human(b.model, i) && content.get("new_chat").is_some_and(Value::is_object) {
                return None;
            }
            let has_words = ["text", "details", "html"].iter().any(|k| content.get(*k).and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty()));
            let has_files = ["attachments", "copied_cards"].iter().any(|k| content.get(*k).and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
            (has_words || has_files).then(|| i.get("sent_at").and_then(Value::as_u64).unwrap_or(0))
        })
        .max().unwrap_or_else(|| {
            b.agents[agent].row.agent_device_id.as_deref()
                .and_then(|id| b.model.members.get(id))
                .and_then(|m| m.added_at).unwrap_or(0)
        })
}

/// Every chat surface takes the same message-recency order. Existing manual order breaks equal timestamps;
/// online/offline is only presentation and never moves a chat on a lease or clock tick. Newest first leaves a chat
/// no place of its own to be dragged to within its list (`group` empty); it still moves to another team or to none.
fn rows_of(b: &Board, units: &[Unit], asked: &HashMap<usize, (u64, String)>) -> Vec<SessionRowView> {
    let mut indices: Vec<usize> = (0..units.len()).collect();
    indices.sort_by_cached_key(|u| std::cmp::Reverse(message_time(b, units[*u].agent)));
    indices.into_iter().map(|u| row(b, units, u, !units[u].online, String::new(), asked)).collect()
}

/// A chat the search's words find: they stand in its name or in its teaser, whatever the case.
fn matches(r: &SessionRowView, query: &str) -> bool {
    let q = query.to_lowercase();
    r.name.en.to_lowercase().contains(&q) || r.last.as_ref().is_some_and(|l| l.text.to_lowercase().contains(&q))
}

/// The one quiet sign of a calm chat row: stopped before a waiting question before new words.
fn signal(stopped: bool, open: u32, unread: bool) -> &'static str {
    if stopped {
        "stopped"
    } else if open > 0 {
        "waiting"
    } else if unread {
        "unread"
    } else {
        ""
    }
}

/// A team's head in words: "3 agents, 2 questions open".
fn stack_words(agents: usize, waiting: u32) -> Text {
    let a = agents.to_string();
    let w = waiting.to_string();
    match (agents, waiting) {
        (1, 0) => say("team.stack.one", "1 agent", &[("agents", &a)]),
        (_, 0) => say("team.stack", format!("{agents} agents"), &[("agents", &a)]),
        (1, 1) => say("team.stack.one.open.one", "1 agent, 1 question open", &[("agents", &a), ("open", &w)]),
        (1, _) => say("team.stack.one.open", format!("1 agent, {waiting} questions open"), &[("agents", &a), ("open", &w)]),
        (_, 1) => say("team.stack.open.one", format!("{agents} agents, 1 question open"), &[("agents", &a), ("open", &w)]),
        _ => say("team.stack.open", format!("{agents} agents, {waiting} questions open"), &[("agents", &a), ("open", &w)]),
    }
}

fn row(b: &Board, units: &[Unit], u: usize, away: bool, group: String, asked: &HashMap<usize, (u64, String)>) -> SessionRowView {
    let unit = &units[u];
    let a = &b.agents[unit.agent];
    // The small line: a link that does not hear at once says so; else the answers that wait.
    let cap = match unit.link.as_ref().filter(|l| l.state != "live") {
        Some(l) => l.word.clone().zip(l.short.clone()).map(|(word, short)| CapView {
            kind: l.state.clone(),
            word,
            short,
            line: Some(l.line.clone()),
        }),
        None if unit.unheard > 0 => {
            let (word, short) = unheard_words(unit.unheard);
            Some(CapView {
                kind: "unheard".into(),
                word,
                short,
                line: None,
            })
        }
        None => None,
    };
    let says = |hand: Option<&HandView>, open: u32, online: bool, working: bool| {
        (open > 0 || hand.is_some()).then(|| badge_words(hand, open, online, working))
    };
    let helpers = b.helpers(unit.agent);
    let n = helpers.len();
    let ns = n.to_string();
    let helpers = (n > 0).then(|| HelperDotsView {
        says: if n == 1 { say("session.helpers.one", "1 helper", &[("n", "1")]) } else { say("session.helpers", format!("{n} helpers"), &[("n", &ns)]) },
        working: helpers.iter().map(|h| h.working && unit.online).collect(),
    });
    SessionRowView {
        id: a.row.key.clone(),
        name: say("session.name", a.row.name.as_str(), &[]),
        link: unit.link.as_ref().map(|l| l.state.clone()).unwrap_or_default(),
        blocked: unit.hand.is_some(),
        unheard: unit.unheard,
        slug: a.row.id.clone(),
        mark: a.row.mark(),
        team_id: if a.root { String::new() } else { a.team.clone().unwrap_or_else(|| DEFAULT_TEAM.into()) },
        online: unit.online,
        working: unit.working,
        main: a.row.main,
        crowned: a.crowned,
        helpers,
        open: unit.open,
        cards: unit.cards,
        knocks: unit.knocks,
        says: says(unit.hand.as_ref(), unit.open, unit.online, unit.working),
        hand: unit.hand.clone(),
        cap,
        away,
        group,
        last: last_said(b, unit.agent, asked.get(&unit.agent)),
        unread: unread(b, unit.agent),
        signal: signal(unit.hand.is_some(), unit.open, unread(b, unit.agent)).into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{dev, Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::{json, Value};

    fn view_at(r: &Room, team: Option<&str>, now: u64) -> (SidebarView, Option<u64>) {
        let m = r.model(now);
        let lens = Lens { team: team.map(String::from), ..Default::default() };
        let b = Board::new(&m, &lens, now);
        let v = sidebar(&b);
        (v, b.wake())
    }
    fn view(r: &Room, team: Option<&str>) -> SidebarView {
        view_at(r, team, NOW).0
    }

    fn slugs(v: &SidebarView) -> Vec<String> {
        v.sessions.iter().map(|r| r.slug.clone()).collect()
    }
    fn row<'v>(v: &'v SidebarView, slug: &str) -> &'v SessionRowView {
        v.sessions.iter().find(|r| r.slug == slug).unwrap_or_else(|| panic!("no row {slug}"))
    }
    #[test]
    fn chat_recency_message_bumps_root_and_team() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        for name in ["a", "b", "c", "d"] {
            r.session(name, None, name != "d");
        }
        for name in ["a", "b"] {
            r.settings.insert(dev(name), json!({ "desk": null }));
        }
        for name in ["c", "d"] { r.on_team(name, "lab"); }
        let ids = |rows: &[SessionRowView]| rows.iter().map(|s| s.slug.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&view(&r, Some("lab")).root), ["a", "b"]);
        r.say("b", None, 1, "me", MIN, json!({ "text": "Newest root message" }));
        r.say("d", None, 2, "d", MIN, json!({ "text": "Newest team message" }));
        let v = view(&r, Some("lab"));
        assert_eq!(ids(&v.root), ["b", "a"], "an outgoing message bumps a root chat");
        assert_eq!(ids(&v.sessions), ["d", "c"], "an offline chat's actual message beats an online chat");
        assert_eq!(ids(&v.stacks.iter().find(|s| s.id == "lab").unwrap().sessions), ["d", "c"]);
        let unfocused = view(&r, Some("main"));
        assert_eq!(ids(&unfocused.stacks.iter().find(|s| s.id == "lab").unwrap().sessions), ["d", "c"], "unfocused teams use the same canonical order");
        assert_eq!(v.stacks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["main", "lab"], "chat activity never reorders team folders");
    }

    #[test]
    fn newest_first_chats_offer_no_rank_moves_team_folders_keep_theirs() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        for (name, online) in [("a", true), ("b", false), ("c", true), ("d", false)] {
            r.session(name, None, online);
        }
        r.settings.insert(dev("a"), json!({ "desk": null }));
        for name in ["c", "d"] { r.on_team(name, "lab"); }
        let v = view(&r, Some("lab"));
        let rows: Vec<&SessionRowView> = v.root.iter().chain(&v.sessions).chain(v.stacks.iter().flat_map(|s| &s.sessions)).collect();
        assert!(rows.len() >= 5, "root, team in view and every folder's chats: {rows:?}");
        assert!(rows.iter().all(|r| r.group.is_empty()), "newest first: no chat has a part of the list to be dragged within");
        let m = r.model(NOW);
        let lens = Lens { team: Some("lab".into()), chats_query: Some("b".into()), ..Default::default() };
        let found = sidebar(&Board::new(&m, &lens, NOW)).found;
        assert!(!found.is_empty(), "the search finds b");
        assert!(found.iter().flat_map(|g| &g.rows).all(|r| r.group.is_empty()), "the search's rows neither");
        r.teams["main"]["order"] = json!(1);
        r.teams["lab"]["order"] = json!(0);
        assert_eq!(view(&r, Some("lab")).stacks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["lab", "main"], "Team folders keep their manual order");
    }

    #[test]
    fn chat_recency_empty_chats_use_admission_time_and_stable_ties() {
        let mut r = Room::default();
        for name in ["a", "b", "c"] {
            r.session(name, None, true);
            r.members.push(json!({ "device_id": dev(name), "device_role": "agent", "added_at": NOW - if name == "a" { 3 * MIN } else { MIN } }));
        }
        assert_eq!(slugs(&view(&r, None)), ["b", "c", "a"], "empty chats use their immutable admission time; equal times retain manual/id order");
        r.say("a", None, 1, "a", 2 * MIN, json!({ "text": "An older message" }));
        assert_eq!(slugs(&view(&r, None)), ["b", "c", "a"]);
    }

    #[test]
    fn chat_recency_ignores_cards_status_activity_and_clock() {
        let mut r = Room::default();
        for name in ["a", "b"] { r.session(name, None, true); }
        r.say("a", None, 1, "a", MIN, json!({ "text": "Actual newest" }));
        r.say("b", None, 2, "b", 2 * MIN, json!({ "text": "Older" }));
        r.sessions[1]["last_activity_at"] = json!(NOW);
        r.sessions[1]["status_lines"] = json!([{ "id": "busy", "label": "Manual status", "state": "working", "updated_at": NOW }]);
        r.card("A newer card is not a message", "b", "decision", "high")["created_at"] = json!(NOW);
        r.say("b", None, 3, "b", 0, json!({ "terminal": "work", "work": { "turn": "t", "items": [] } }));
        assert_eq!(slugs(&view(&r, None)), ["a", "b"]);
        r.sessions[0]["is_online"] = json!(false);
        assert_eq!(slugs(&view_at(&r, None, NOW + MIN).0), ["a", "b"], "lease/clock/status never bump chats");
    }

    #[test]
    fn chat_recency_counts_attachments_and_out_of_order_messages_not_headers() {
        let mut r = Room::default();
        for name in ["a", "b"] { r.session(name, None, true); }
        r.say("a", None, 10, "a", 2 * MIN, json!({ "text": "Earlier" }));
        r.say("b", None, 20, "b", MIN, json!({ "attachments": [{ "attachment_id": "file" }] }));
        r.say("b", None, 30, "b", 4 * MIN, json!({ "text": "Late-arriving older envelope" }));
        assert_eq!(slugs(&view(&r, None)), ["b", "a"]);
        r.say("a", None, 40, "a", 0, json!({ "text": "Not loaded yet" }));
        r.timelines.get_mut(&format!("chat:session/{}", dev("a"))).unwrap().as_array_mut().unwrap().last_mut().unwrap()["item_state"] = json!("header");
        let m = r.model(NOW);
        let lens = Lens { chats_query: Some("a".into()), ..Default::default() };
        let v = sidebar(&Board::new(&m, &lens, NOW));
        assert_eq!(slugs(&v), ["b", "a"], "unread headers are not messages");
        let found: Vec<_> = v.found.iter().flat_map(|g| &g.rows).map(|x| x.slug.as_str()).collect();
        assert_eq!(found, ["b", "a"], "search filters the shared ordered rows without another sort");
    }

    #[test]
    fn chat_recency_visible_rich_messages_count_but_hidden_new_chat_requests_do_not() {
        let mut r = Room::default();
        for name in ["a", "b"] { r.session(name, None, true); }
        r.say("a", None, 1, "a", 3 * MIN, json!({ "text": "Earlier words" }));
        r.say("b", None, 2, "me", 2 * MIN, json!({ "copied_cards": ["card"] }));
        assert_eq!(slugs(&view(&r, None)), ["b", "a"], "a visible copied-card message bumps its chat");
        r.say("a", None, 3, "a", MIN, json!({ "details": "A detailed reply" }));
        assert_eq!(slugs(&view(&r, None)), ["a", "b"], "details-only replies are still visible messages");
        r.say("b", None, 4, "me", 0, json!({ "text": "Start a new chat", "new_chat": { "request_id": "req" } }));
        assert_eq!(slugs(&view(&r, None)), ["a", "b"], "the hidden new-chat control request is not a chat message");
    }

    fn two_teams() -> Value {
        json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } })
    }

    /// Helpers have no row: the agent's row says how many, a dot each, filled while that helper works. A helper
    /// session of before has no row either; its open question counts on its main.
    #[test]
    fn helpers_are_dots_on_their_agents_row() {
        let mut r = Room::default();
        r.session("a", None, true)["status_lines"] = json!([
            { "id": "l1", "label": "Tokens", "state": "working", "helper": "Tokens", "updated_at": NOW - MIN },
            { "id": "l2", "label": "Own", "state": "working", "updated_at": NOW - MIN },
        ]);
        r.card("q1", "a", "decision", "normal")["helper"] = json!("Icons");
        r.session("b-old", Some("a"), true);
        r.card("q2", "b-old", "decision", "high");
        r.session("c", None, true);
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["a", "c"]);
        let a = row(&v, "a");
        let h = a.helpers.as_ref().unwrap();
        // (in the order they first showed: the two cards ten minutes ago, the line a minute ago)
        assert_eq!((h.says.en.as_str(), h.working.clone()), ("3 helpers", vec![false, false, true]));
        assert_eq!((a.open, a.knocks), (2, true), "the helpers' questions are the session's");
        assert!(row(&v, "c").helpers.is_none());
        // (a disconnected agent's helpers work no more)
        r.sessions[0]["is_online"] = json!(false);
        assert_eq!(row(&view(&r, None), "a").helpers.as_ref().unwrap().working, [false, false, false]);
    }

    #[test]
    fn connection_state_does_not_reorder_tied_chats() {
        let mut r = Room::default();
        r.session("a-off", None, false);
        r.session("b-on", None, true);
        r.session("e-off", None, false);
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["a-off", "b-on", "e-off"]);
        assert_eq!(v.sessions.iter().map(|x| x.away).collect::<Vec<_>>(), [true, false, true]);
        assert_eq!(row(&v, "a-off").group, "", "newest first: no rank move within the list");
        // (gone since 13:27, said under its name)
        let cap = row(&v, "a-off").cap.as_ref().unwrap();
        assert_eq!((cap.kind.as_str(), cap.word.en.as_str(), cap.short.en.as_str()), ("gone", "gone · 13:27", "Gone since 13:27"));
        assert_eq!(row(&v, "b-on").cap, None);
    }

    #[test]
    fn one_team_at_a_time_and_the_switch_counts_what_waits_on_each() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.teams["empty"] = json!({ "name": "Empty", "created_at": 3 });
        r.session("a-work", None, false);
        r.session("b-lab", None, true);
        r.session("c-work", None, true);
        r.session("d-lab", None, false);
        r.on_team("b-lab", "lab");
        r.on_team("d-lab", "lab");
        // (a team that is gone: on no team, at the top, never the first team's)
        r.on_team("c-work", "nowhere");
        r.card("q1", "d-lab", "decision", "normal");
        let v = view(&r, Some("lab"));
        assert_eq!(slugs(&v), ["b-lab", "d-lab"]);
        assert_eq!((v.team.as_str(), v.place.en.as_str(), v.badge), ("lab", "Lab", 1));
        assert_eq!(row(&v, "d-lab").group, "", "newest first: no rank move within the team");

        // one team in view: only its sessions; the tab counts this team alone, the menu says what waits on the other
        let v = view(&r, Some("main"));
        assert_eq!(slugs(&v), ["a-work"]);
        assert_eq!(v.root.iter().map(|r| r.slug.as_str()).collect::<Vec<_>>(), ["c-work"]);
        assert_eq!((v.badge, v.waiting, v.place.en.as_str()), (0, 0, "Work"));
        assert_eq!(view(&r, Some("lab")).badge, 1);
        // ("all" is no team any more: the first)
        assert_eq!(view(&r, Some("all")).team, "main");
        assert_eq!(v.teams.iter().map(|d| (d.id.as_str(), d.waiting, d.selected)).collect::<Vec<_>>(), [("main", 0, true), ("lab", 1, false), ("empty", 0, false)]);
        // a team that is not there: the first
        assert_eq!(view(&r, Some("nowhere")).team, "main");
        // none chosen yet (a fresh sign-in): the first
        assert_eq!(view(&r, None).team, "main");
    }

    /// The wide sidebar: every team as a stack of its sessions' card edges with what waits on it, open where this
    /// device unfolded it; on home (`desk_all`) no team is marked.
    #[test]
    fn every_team_is_a_stack_open_where_unfolded() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.teams["empty"] = json!({ "name": "Empty", "created_at": 3 });
        r.session("a-work", None, true);
        r.session("b-lab", None, true);
        r.session("d-lab", None, false);
        r.on_team("b-lab", "lab");
        r.on_team("d-lab", "lab");
        r.card("q1", "d-lab", "decision", "high");
        r.card("q2", "d-lab", "decision", "normal");
        let m = r.model(NOW);
        let mut lens = Lens { team: Some("main".into()), ..Default::default() };
        let v = sidebar(&Board::new(&m, &lens, NOW));
        let st = |v: &SidebarView| v.stacks.iter().map(|s| (s.id.clone(), s.selected, s.open, s.waiting, s.knocks, s.edges.len())).collect::<Vec<_>>();
        assert_eq!(st(&v), [("main".into(), true, false, 0, false, 1), ("lab".into(), false, false, 2, true, 2), ("empty".into(), false, false, 0, false, 0)]);
        assert_eq!(v.stacks[1].says.en, "2 agents, 2 questions open");
        assert_eq!(v.stacks[1].sessions.iter().map(|r| (r.slug.as_str(), r.signal.as_str())).collect::<Vec<_>>(), [("b-lab", ""), ("d-lab", "waiting")]);
        assert_eq!(v.sessions.len(), 1, "the phone's rows: the team in view alone");
        lens.open_teams = vec!["lab".into(), "empty".into()];
        lens.desk_all = true;
        let v = sidebar(&Board::new(&m, &lens, NOW));
        assert_eq!(st(&v)[..2], [("main".into(), false, false, 0, false, 1), ("lab".into(), false, true, 2, true, 2)]);
        assert!(!v.stacks[2].open && v.stacks[2].faces.is_empty(), "an empty team never opens and has no faces");
        assert_eq!(v.stacks[1].faces.len(), 2);
        assert_eq!((v.desk_all, v.place.en.as_str()), (true, "All teams"));
    }

    /// The person's picture: the e-mail's first letter; nothing known, nothing drawn but a figure.
    #[test]
    fn the_persons_initial_comes_from_the_email() {
        assert_eq!(super::initial_of(Some("christopher@example.com")), "C");
        assert_eq!(super::initial_of(Some("_ölaf@x.de")), "Ö");
        assert_eq!(super::initial_of(Some("7up@x.de")), "7");
        assert_eq!(super::initial_of(Some("")), "");
        assert_eq!(super::initial_of(Some("@@")), "");
        assert_eq!(super::initial_of(None), "");
    }

    /// Home in the menu (the phone's place pill): "All teams" above the teams with what waits on all of them, marked
    /// while it is the Desk in view; then no team is marked, and the tab and the lamp count every team's questions.
    #[test]
    fn home_stands_above_the_teams_and_counts_them_all() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("a-work", None, true);
        r.session("b-lab", None, true);
        r.on_team("b-lab", "lab");
        r.card("q1", "a-work", "decision", "normal");
        r.card("q2", "b-lab", "decision", "high");
        r.card("q3", "b-lab", "decision", "normal");
        let m = r.model(NOW);
        let mut lens = Lens { team: Some("main".into()), ..Default::default() };
        let marks = |v: &SidebarView| v.teams.iter().map(|d| d.selected).collect::<Vec<_>>();
        let v = sidebar(&Board::new(&m, &lens, NOW));
        assert_eq!((v.home.id.as_str(), v.home.name.en.as_str(), v.home.name.key.as_str()), ("", "All teams", "team.all"));
        assert_eq!((v.home.waiting, v.home.knocks, v.home.selected), (3, true, false));
        assert_eq!(marks(&v), [true, false]);
        assert_eq!((v.badge, v.waiting, v.knocking), (1, 1, 0), "a team's Desk counts that team alone");
        lens.desk_all = true;
        let v = sidebar(&Board::new(&m, &lens, NOW));
        assert!(v.home.selected);
        assert_eq!(marks(&v), [false, false], "on home no team is marked");
        assert_eq!((v.badge, v.waiting, v.knocking), (3, 3, 1), "home counts every team's questions");
        assert_eq!(slugs(&v), ["a-work"], "the chats stay the team in view's");
    }

    /// The bolt at the phone's top right walks every team's Desk whatever team is in view: its first card is that
    /// Desk's first row and it says that Desk's Blitz; it follows the core's order; with nothing open it is none.
    #[test]
    fn the_global_blitz_is_every_teams_desk_from_any_team() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("a-work", None, true);
        r.session("b-lab", None, true);
        r.on_team("b-lab", "lab");
        let m = r.model(NOW);
        assert_eq!(sidebar(&Board::new(&m, &Lens { team: Some("main".into()), ..Default::default() }, NOW)).global_blitz, None, "nothing open: no bolt");

        r.card("q1", "a-work", "decision", "normal");
        r.card("q2", "b-lab", "decision", "high");
        r.card("i3", "b-lab", "info", "normal");
        let mut m = r.model(NOW);
        let lenses = |team: &str| {
            [Lens { team: Some(team.into()), ..Default::default() }, Lens { team: Some(team.into()), desk_all: true, ..Default::default() }]
        };
        let home = |m: &Model| super::super::desk::desk(&Board::new(m, &lenses("main")[1], NOW));
        let check = |m: &Model| {
            let all = home(m);
            assert_eq!(all.rows.len(), 3, "every team's Desk");
            for lens in lenses("main").into_iter().chain(lenses("lab")) {
                let g = sidebar(&Board::new(m, &lens, NOW)).global_blitz.expect("open cards: a bolt");
                assert_eq!(g.card, all.rows[0].id, "the first of every team's Desk, from {lens:?}");
                assert_eq!(Some(&g.says), all.blitz.as_ref());
                assert_eq!((g.says.key.as_str(), g.says.en.as_str()), ("desk.blitz", "Blitz: 3 open cards"));
            }
            all.rows[0].id.clone()
        };
        check(&m);
        // the core's order leads, and from a team's chats the bolt starts on another team's card
        m.desk_order = ["q2", "i3", "q1"].map(String::from).to_vec();
        assert_eq!(check(&m), "q2");
        let main = super::super::desk::desk(&Board::new(&m, &lenses("main")[0], NOW));
        assert_eq!(main.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["q1"], "the team's own Desk is the team's");
        m.desk_order = ["q1", "q2", "i3"].map(String::from).to_vec();
        assert_eq!(check(&m), "q1");
    }

    /// A chat on no team stands at the top, alongside the teams, from whichever team is in view: in no team's stack
    /// or rows, with no team's id; its questions count on home and on the bolt, on no team; the search finds it first;
    /// the places offer "No team" first and the team in view chosen; its page moves it onto a team.
    #[test]
    fn a_chat_on_no_team_stands_at_the_top_alongside_the_teams() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("a-work", None, true);
        r.session("b-lab", None, true);
        r.session("c-root", None, true);
        r.on_team("b-lab", "lab");
        r.settings.insert(dev("c-root"), json!({ "desk": null }));
        r.card("q1", "c-root", "decision", "high");
        for team in ["main", "lab"] {
            let m = r.model(NOW);
            let v = sidebar(&Board::new(&m, &Lens { team: Some(team.into()), ..Default::default() }, NOW));
            assert_eq!(v.root.iter().map(|r| (r.slug.as_str(), r.team_id.as_str())).collect::<Vec<_>>(), [("c-root", "")]);
            assert!(v.stacks.iter().all(|s| s.sessions.iter().all(|r| r.slug != "c-root") && s.waiting == 0), "in no team");
            assert!(!slugs(&v).contains(&"c-root".to_string()));
            assert_eq!((v.home.waiting, v.badge), (1, 0), "home counts it, the team's Desk does not");
            assert_eq!(v.global_blitz.as_ref().map(|g| g.card.as_str()), Some("q1"));
            let places: Vec<(Option<&str>, bool)> = v.places.iter().map(|p| (p.team_id.as_deref(), p.chosen)).collect();
            assert_eq!(places, [(None, false), (Some("main"), team == "main"), (Some("lab"), team == "lab")]);
            let page = super::super::session::session(&Board::new(&m, &Lens { team: Some(team.into()), session: Some(dev("c-root")), ..Default::default() }, NOW), &dev("c-root")).unwrap();
            assert_eq!(page.move_to.iter().map(|p| p.team_id.as_deref()).collect::<Vec<_>>(), [Some("main"), Some("lab")]);
            let page = super::super::session::session(&Board::new(&m, &Lens { team: Some(team.into()), session: Some(dev("b-lab")), ..Default::default() }, NOW), &dev("b-lab")).unwrap();
            assert_eq!(page.move_to.iter().map(|p| p.team_id.as_deref()).collect::<Vec<_>>(), [None, Some("main")]);
        }
        let m = r.model(NOW);
        let v = sidebar(&Board::new(&m, &Lens { chats_query: Some("root".into()), ..Default::default() }, NOW));
        assert_eq!(v.found.iter().map(|g| (g.id.as_str(), g.rows.len())).collect::<Vec<_>>(), [("", 1)]);
        // without teams: "No team", chosen, so a new chat or an invite always has its place
        let mut r = Room::default();
        r.session("a", None, true);
        let m = r.model(NOW);
        let places: Vec<(Option<String>, bool)> = sidebar(&Board::new(&m, &Lens::default(), NOW)).places.into_iter().map(|p| (p.team_id, p.chosen)).collect();
        assert_eq!(places, [(None, true)]);
    }

    /// The phone's Search: every team's chats whose name or teaser has the words, under their team's title; a
    /// search that finds none says so; no search, no list.
    #[test]
    fn a_search_finds_chats_across_the_teams() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("a-work", None, true);
        r.session("b-lab", None, true);
        r.session("d-lab", None, false);
        r.on_team("b-lab", "lab");
        r.on_team("d-lab", "lab");
        let m = r.model(NOW);
        let mut lens = Lens { team: Some("main".into()), ..Default::default() };
        let v = sidebar(&Board::new(&m, &lens, NOW));
        assert!(v.found.is_empty() && v.found_none.is_none(), "no search, no list");
        lens.chats_query = Some("LAB".into());
        let v = sidebar(&Board::new(&m, &lens, NOW));
        let found = v.found.iter().map(|g| (g.name.en.as_str(), g.rows.iter().map(|r| r.slug.as_str()).collect::<Vec<_>>())).collect::<Vec<_>>();
        assert_eq!(found, [("Lab", vec!["b-lab", "d-lab"])]);
        lens.chats_query = Some("zebra".into());
        let v = sidebar(&Board::new(&m, &lens, NOW));
        assert!(v.found.is_empty());
        assert_eq!(v.found_none.map(|t| t.en), Some("No chat matches \u{201c}zebra\u{201d}.".to_string()));
    }

    #[test]
    fn a_room_without_teams_has_one_personal() {
        let mut r = Room::default();
        r.session("a", None, true);
        let v = view(&r, Some("all"));
        assert_eq!(v.teams.iter().map(|d| (d.id.as_str(), d.name.en.as_str(), d.name.key.as_str(), d.selected)).collect::<Vec<_>>(), [("main", "Personal", "team.personal", true)]);
        assert_eq!((v.team.as_str(), v.place.en.as_str()), ("main", "Personal"));
        assert_eq!(slugs(&v), ["a"]);
        let one = Room { teams: json!({ "main": { "name": "Solo" } }), ..Default::default() };
        let v = view(&one, Some("all"));
        assert_eq!((v.team.as_str(), v.place.en.as_str()), ("main", "Solo"));
    }

    #[test]
    fn open_counts_what_waits_for_him_on_the_desk() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q-plain", "a", "decision", "normal");
        r.card("q-info", "a", "info", "normal");
        r.card("q-back", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        r.card("q-later", "a", "decision", "normal");
        r.snoozes = json!({ "q-later": { "until": NOW + 60 * MIN } });
        // (not read on this device yet: a quiet placeholder, no question and no knock)
        // (blocking, even: no knock and no red hand until it is read. This fails without the rule in status.rs)
        r.card("q-loading", "a", "decision", "critical")["content_state"] = json!("header");
        r.card("q-newer", "a", "decision", "high")["unsupported"] = json!("card_type");
        r.answered("q-done", "a", NOW - MIN, 10);
        let v = view(&r, None);
        let a = row(&v, "a");
        assert_eq!((a.open, a.knocks, v.badge, v.waiting, v.knocking), (1, false, 1, 1, 0));
        assert_eq!(a.says.as_ref().map(|t| t.en.as_str()), Some("1 question open"));
        assert!(!a.blocked && a.hand.is_none() && v.stopped == 0);

        r.card("q-high", "a", "decision", "high");
        let v = view(&r, None);
        assert_eq!((row(&v, "a").open, row(&v, "a").knocks, v.knocking), (2, true, 1));

        // an approval request counts and knocks, and stops its session; one not read here yet waits quietly
        r.request("p-unread", "a", "header");
        let v = view(&r, None);
        assert_eq!((v.badge, v.stopped, row(&v, "a").blocked), (2, 0, false), "an unread request stops nobody yet");
        r.request("p-read", "a", "ok");
        let v = view(&r, None);
        let a = row(&v, "a");
        assert_eq!((a.open, v.badge, v.knocking, v.stopped), (3, 3, 2, 1));
        assert_eq!(a.hand.as_ref().map(|h| (h.why.as_str(), h.text.en.as_str())), Some(("permission", "Waiting for permission")));
        assert_eq!(a.says.as_ref().map(|t| t.en.as_str()), Some("Stopped: Waiting for permission, 3 questions open"));
        assert!(a.blocked);
    }

    /// The calm grey counts (a row's, a team's, the Chats list's) count every open card, infos too, and the
    /// requests: what the Desk's rows and Blitz walk, from the one list (`Board::all_cards`). A team whose session
    /// has only infos open showed no number although Blitz said "3 open cards" (the owner, 23 October 2026).
    #[test]
    fn the_grey_counts_are_what_blitz_walks() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("valiido", None, true);
        r.session("bravo", None, true);
        r.on_team("bravo", "lab");
        r.card("d1", "valiido", "decision", "normal");
        r.card("d2", "valiido", "decision", "high");
        r.card("i0", "valiido", "info", "normal");
        r.card("i1", "bravo", "info", "normal");
        r.card("i2", "bravo", "info", "high");
        r.card("i3", "bravo", "info", "normal")["helper"] = json!("Icons");
        // (what is not on the walk counts nowhere: one with its agent again, one decided)
        r.card("i-back", "bravo", "info", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        r.answered("d-done", "bravo", NOW - MIN, 10);
        let blitz = |r: &Room, team: &str| {
            let m = r.model(NOW);
            let lens = Lens { team: Some(team.into()), ..Default::default() };
            let b = Board::new(&m, &lens, NOW);
            let d = super::super::desk::desk(&b);
            (d.stack.len() as u32, d.blitz.map(|t| t.en))
        };
        assert_eq!(blitz(&r, "lab"), (3, Some("Blitz: 3 open cards".into())));
        assert_eq!(blitz(&r, "main"), (3, Some("Blitz: 3 open cards".into())));
        for team in ["main", "lab"] {
            let v = view(&r, Some(team));
            let tab = |id: &str| v.teams.iter().find(|t| t.id == id).unwrap().cards;
            let stack = |id: &str| v.stacks.iter().find(|t| t.id == id).unwrap().cards;
            assert_eq!((tab("main"), tab("lab"), v.home.cards), (3, 3, 6));
            assert_eq!((stack("main"), stack("lab")), (3, 3));
            let rows: Vec<(String, u32)> = v.stacks.iter().flat_map(|s| s.sessions.iter()).map(|x| (x.slug.clone(), x.cards)).collect();
            assert_eq!(rows, [("valiido".to_string(), 3), ("bravo".to_string(), 3)]);
        }
        // (the questions stay questions: bravo asks nothing, the red and the "questions open" words are theirs)
        let v = view(&r, Some("lab"));
        let bravo = row(&v, "bravo");
        assert_eq!((bravo.open, bravo.knocks, bravo.says.as_ref()), (0, false, None));
        assert_eq!(v.teams.iter().map(|t| (t.waiting, t.cards)).collect::<Vec<_>>(), [(2, 3), (0, 3)]);

        // a request counts as it stands on the Desk, read or not yet
        r.request("p1", "bravo", "header");
        let v = view(&r, Some("lab"));
        assert_eq!((row(&v, "bravo").cards, blitz(&r, "lab").0), (4, 4));
    }

    #[test]
    fn an_archived_sessions_questions_count_nowhere() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.session("b-old", None, false);
        r.settings.insert(dev("b-old"), json!({ "archived": true }));
        r.card("q1", "b-old", "decision", "high");
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["a"]);
        assert_eq!((v.badge, v.knocking), (0, 0));
    }

    #[test]
    fn one_crown_per_team() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        for (name, team) in [("a-work", "main"), ("b-work", "main"), ("c-lab", "lab"), ("d-lab", "lab")] {
            r.session(name, None, true);
            r.on_team(name, team);
        }
        let crowned = |r: &Room| -> Vec<String> {
            ["main", "lab"].iter().flat_map(|t| view(r, Some(t)).sessions.into_iter().filter(|x| x.crowned).map(|x| x.slug)).collect()
        };
        // the room's single crown shows on the team its session stands on
        r.crown = json!({ "agent_device_id": dev("a-work") });
        assert_eq!(crowned(&r), ["a-work"]);
        // a team's own crown decides on that team
        r.teams["lab"]["crown"] = json!({ "agent_device_id": dev("d-lab") });
        assert_eq!(crowned(&r), ["a-work", "d-lab"]);
        r.teams["main"]["crown"] = json!({ "session_id": dev("b-work") });
        assert_eq!(crowned(&r), ["b-work", "d-lab"]);
        // a team whose crown was taken away has none, whatever the room's says
        r.teams["main"]["crown"] = Value::Null;
        r.teams["lab"] = json!({ "name": "Lab", "created_at": 2 });
        r.crown = json!({ "agent_device_id": dev("c-lab") });
        assert_eq!(crowned(&r), ["c-lab"]);
    }

    #[test]
    fn an_answer_not_picked_up_for_two_minutes_waits_on_the_session() {
        let mut r = Room::default();
        r.session("a", None, true)["heard_up_to"] = json!(40);
        r.answered("heard", "a", NOW - 30 * MIN, 30);
        r.answered("fresh", "a", NOW - MIN, 50);
        let (v, wake) = view_at(&r, None, NOW);
        assert_eq!((row(&v, "a").unheard, row(&v, "a").cap.is_none(), v.unheard), (0, true, 0));
        assert_eq!(wake, Some(NOW + MIN), "it turns late two minutes after he answered");

        let (v, wake) = view_at(&r, None, NOW + MIN);
        let cap = row(&v, "a").cap.clone().unwrap();
        assert_eq!((cap.kind.as_str(), cap.word.en.as_str(), cap.short.en.as_str(), cap.line), ("unheard", "1 answer waits", "Not heard yet", None));
        // (nothing turns late any more; the teasers' times change at midnight)
        assert_eq!((v.unheard, wake), (1, Some(next_midnight(NOW + MIN, 0))));

        // (a helper's answer is its session's: it was filed there)
        r.answered("helper", "a", NOW - 5 * MIN, 60);
        r.cards.last_mut().unwrap()["helper"] = json!("Icons");
        // (a hand back its session has not picked up counts too: since its card went back)
        r.card("back", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 70 });
        let v = view_at(&r, None, NOW + MIN).0;
        assert_eq!((row(&v, "a").unheard, v.unheard), (3, 3));
        assert_eq!(row(&v, "a").cap.as_ref().map(|c| (c.word.en.as_str(), c.short.en.as_str())), Some(("3 answers wait", "3 not heard")));

        // (a session whose connector writes no receipts: nothing is known, nothing is said)
        r.sessions[0]["heard_up_to"] = Value::Null;
        assert_eq!(row(&view_at(&r, None, NOW + MIN).0, "a").unheard, 0);
    }

    #[test]
    fn a_link_that_does_not_hear_at_once_is_said_before_what_waits() {
        let mut r = Room::default();
        let s = r.session("a", None, true);
        s["heard_up_to"] = json!(40);
        s["link"] = json!({ "hears": "oncall", "attached": true, "last_call_at": NOW - 46 * MIN });
        r.answered("late", "a", NOW - 30 * MIN, 50);
        let (v, wake) = view_at(&r, None, NOW);
        let a = row(&v, "a");
        let cap = a.cap.clone().unwrap();
        assert_eq!((a.link.as_str(), cap.kind.as_str(), cap.word.en.as_str(), a.unheard), ("asleep", "asleep", "not listening · 46 min", 1));
        assert_eq!(cap.line.map(|l| l.en), Some("a is not listening: it hears you only on its next step, and its last one was 46 min ago.".into()));
        assert_eq!(wake, Some(NOW + 30_000), "the minutes count on");
    }

    #[test]
    fn a_row_names_its_face_and_its_address() {
        let mut r = Room::default();
        r.session("a", None, true)["profile"]["icon"] = json!("draw:leaf");
        r.session("b", None, true);
        r.settings.insert(dev("b"), json!({ "name": "Renamed", "icon": "draw:key" }));
        r.session("c", None, true);
        let v = view(&r, None);
        assert_eq!(v.sessions.iter().map(|x| (x.slug.as_str(), x.name.en.as_str(), x.mark.as_str())).collect::<Vec<_>>(), [("a", "a", "draw:leaf"), ("b", "Renamed", "draw:acorn"), ("c", "c", row(&v, "c").mark.as_str())]);
        assert!(crate::faces::face_of(&row(&v, "c").mark).is_some(), "a session without an icon wears a face too");
        assert_eq!(row(&v, "a").id, dev("a"));
    }

    #[test]
    fn a_row_says_what_was_said_last_and_when() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.session("b", None, true);
        r.session("c", None, true);
        r.session("d", None, true);
        r.say("a", None, 10, "a", 30 * MIN, json!({ "text": "## Plan\n- **ship** it\n```\ncode\n```" }));
        r.say("a", None, 11, "me", 20 * MIN, json!({ "text": "Go ahead" }));
        // (a turn's trail is no word: the teaser stays on his)
        r.say("a", None, 12, "a", 10 * MIN, json!({ "terminal": "work", "work": { "turn": "t1", "items": [] } }));
        r.say("b", None, 20, "b", 2 * 24 * 60 * MIN, json!({ "text": "", "attachments": [{ "attachment_id": "f1" }, { "attachment_id": "f2" }] }));
        // (a question put to him after the last word is what was said last)
        r.say("c", None, 30, "c", 24 * 60 * MIN, json!({ "text": "earlier" }));
        r.card("Ship it?", "c", "decision", "normal");
        let (v, wake) = view_at(&r, None, NOW);
        let last = |slug: &str| row(&v, slug).last.as_ref().map(|l| (l.text.as_str(), l.when.en.as_str()));
        assert_eq!(last("a"), Some(("You: Go ahead", "13:45")));
        assert_eq!(last("b"), Some(("2 attachments", "Sunday")));
        assert_eq!(last("c"), Some(("Ship it?", "13:55")));
        assert_eq!(last("d"), None);
        assert_eq!(row(&v, "a").last.as_ref().unwrap().at_ms, NOW - 20 * MIN);
        assert_eq!(wake, Some(next_midnight(NOW, 0)), "at midnight 13:45 becomes Yesterday");
        assert_eq!(chat_when(NOW - 24 * 60 * MIN, NOW, 0).en, "Yesterday");
        assert_eq!(chat_when(NOW - 30 * 24 * 60 * MIN, NOW, 0).en, "13 Sep");
        assert_eq!(chat_when(NOW - 400 * 24 * 60 * MIN, NOW, 0).en, "08.09.25");
        // (the agent's words only, its newest first)
        let m = r.model(NOW);
        assert_eq!((newest_word(&m, &dev("a")), newest_word(&m, &dev("d"))), (Some(10), None));
    }

    #[test]
    fn a_dot_marks_words_this_device_has_not_shown() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.session("b", None, true);
        r.say("a", None, 10, "a", 5 * MIN, json!({ "text": "one" }));
        r.say("b", None, 20, "b", 4 * MIN, json!({ "text": "two", "helper": "Icons" }));
        let m = r.model(NOW);
        let at = |seen: Option<Vec<(&str, u64)>>| {
            let lens = Lens { seen: seen.map(|l| l.into_iter().map(|(k, n)| (dev(k), n)).collect()), ..Default::default() };
            let v = sidebar(&Board::new(&m, &lens, NOW));
            (row(&v, "a").unread, row(&v, "b").unread)
        };
        assert_eq!(at(None), (false, false), "nothing counts as unread before the marks are read");
        assert_eq!(at(Some(vec![])), (true, true));
        assert_eq!(at(Some(vec![("a", 10)])), (false, true));
        assert_eq!(at(Some(vec![("a", 10), ("b", 20)])), (false, false));
        // (his own words are never unread)
        let mut r2 = Room::default();
        r2.session("a", None, true);
        r2.say("a", None, 10, "me", MIN, json!({ "text": "mine" }));
        let m2 = r2.model(NOW);
        let lens = Lens { seen: Some(Default::default()), ..Default::default() };
        assert!(!row(&sidebar(&Board::new(&m2, &lens, NOW)), "a").unread);
    }
    /// The grey counts leave out what no row shows (the owner, 27 October 2026: "kein unread badge hier in der gruppe
    /// personal mehr. der ist für karten vorbehalten"; Personal showed a 2 and none of its chats a number): an info its
    /// agent still works on (`start_task`) lies under Off your mind, not on the Desk, and counts nowhere; a card of a
    /// session the list does not show counted on the first team and on home, though no row and no Blitz shows it.
    #[test]
    fn the_grey_counts_leave_out_work_in_progress_and_unknown_sessions() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("valiido", None, true);
        r.session("bravo", None, true);
        r.on_team("bravo", "lab");
        r.card("w1", "valiido", "info", "low")["working"] = json!(true);
        r.card("w2", "bravo", "info", "low")["working"] = json!(true);
        r.card("ghost", "nobody", "info", "normal");
        r.request("p-ghost", "nobody", "ok");
        let counts = |r: &Room, team: &str| {
            let m = r.model(NOW);
            let lens = Lens { team: Some(team.into()), ..Default::default() };
            let b = Board::new(&m, &lens, NOW);
            let v = sidebar(&b);
            let d = super::super::desk::desk(&b);
            let tabs: Vec<u32> = v.teams.iter().map(|t| t.cards).collect();
            let stacks: Vec<u32> = v.stacks.iter().map(|t| t.cards).collect();
            let rows: Vec<u32> = v.stacks.iter().flat_map(|s| s.sessions.iter()).map(|x| x.cards).collect();
            (tabs, stacks, rows, v.home.cards, d.stack.len(), d.blitz.map(|t| t.en), v.global_blitz.map(|g| g.card))
        };
        for team in ["main", "lab"] {
            assert_eq!(counts(&r, team), (vec![0, 0], vec![0, 0], vec![0, 0], 0, 0, None, None), "nothing to count in {team}");
        }
        // (once it is done, the card comes to the Desk and counts where its chat stands, as Blitz walks it)
        r.cards.iter_mut().find(|c| c["object_id"] == "w2").unwrap().as_object_mut().unwrap().remove("working");
        assert_eq!(
            counts(&r, "lab"),
            (vec![0, 1], vec![0, 1], vec![0, 1], 1, 1, Some("Blitz: 1 open card".into()), Some("w2".into()))
        );
    }
    /// The red counts as the grey: a question or a request of a session the list does not show waited on the first
    /// team (its menu row, the tab's badge, home and the Desk's head) though no row shows it; it waits nowhere now.
    #[test]
    fn a_question_of_an_unknown_session_waits_nowhere() {
        let mut r = Room { teams: two_teams(), ..Default::default() };
        r.session("valiido", None, true);
        r.session("bravo", None, true);
        r.on_team("bravo", "lab");
        r.card("q-ghost", "nobody", "decision", "high");
        r.request("p-ghost", "nobody", "ok");
        r.card("q-lab", "bravo", "decision", "normal");
        let counts = |r: &Room, lens: Lens| {
            let m = r.model(NOW);
            let b = Board::new(&m, &lens, NOW);
            let v = sidebar(&b);
            let d = super::super::desk::desk(&b);
            let tabs: Vec<(u32, bool)> = v.teams.iter().map(|t| (t.waiting, t.knocks)).collect();
            (tabs, (v.home.waiting, v.home.knocks), (v.badge, v.waiting, v.knocking), d.waiting, d.empty.map(|t| t.en))
        };
        let main = Lens { team: Some("main".into()), ..Default::default() };
        let lab = Lens { team: Some("lab".into()), ..Default::default() };
        let home = Lens { desk_all: true, ..Default::default() };
        let tabs = vec![(0, false), (1, false)];
        assert_eq!(counts(&r, main), (tabs.clone(), (1, false), (0, 0, 0), 0, Some("Questions land here.".into())));
        assert_eq!(counts(&r, lab), (tabs.clone(), (1, false), (1, 1, 0), 1, None));
        assert_eq!(counts(&r, home), (tabs, (1, false), (1, 1, 0), 1, None));
    }
}
