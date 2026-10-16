//! The sidebar: the teams, and the sessions of the team in view as rows, in the order they are drawn. A shell draws
//! the rows one after the other and decides nothing: which sessions show, connected or away, what waits, who is
//! stopped and why, the small line under a name.
//!
//! The rules are the web's of before (`app.mjs` `boardModel` units, `sidebar.mjs` `sidebarParts` and `desksOf`,
//! `ui.mjs` `badge` and `linkCap`):
//! - a row per session that is not archived and stands on the team in view;
//! - a row is "away" when it is not connected: the connected rows first, then the rest, with no line between them;
//! - a row's open questions are those of its session on the Desk now (its helpers' too: they write into it);
//! - a session's helpers (its agent's subagents, `helpers.rs`) have no row: its row says how many there are, with
//!   a dot each that is filled while that helper works ([`HelperDotsView`]).

use super::board::{key_of, Board, DEFAULT_TEAM};
use super::links::plain;
use super::words::{badge_words, hand_words, link_words, local, next_midnight, say, unheard_words, LinkSubject};
use super::{CapView, HandView, HelperDotsView, LastSaidView, LinkWords, SessionRowView, SidebarView, StackEdgeView, TeamStackView, TeamTab, Text, STACK_EDGES};
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

pub fn sidebar(b: &Board) -> SidebarView {
    let units = units_of(b);

    // ---- the teams ----
    let teams: Vec<(String, String)> = if b.teams.is_empty() {
        vec![(DEFAULT_TEAM.to_string(), "Personal".to_string())]
    } else {
        b.teams.iter().map(|d| (d.id.clone(), d.name.clone())).collect()
    };
    let first = teams[0].0.clone();
    let waits_on = |team: &str| {
        let mine = b.all_fresh.iter().filter(|f| b.team_of(f.agent).unwrap_or(first.as_str()) == team);
        (mine.clone().count() as u32, mine.clone().any(|f| f.knocks))
    };
    let in_view = b.team.clone().unwrap_or_else(|| first.clone());
    let tabs: Vec<TeamTab> = teams
        .iter()
        .map(|(id, name)| {
            let (waiting, knocks) = waits_on(id);
            TeamTab {
                id: id.clone(),
                name: team_name(name),
                waiting,
                selected: *id == in_view,
                knocks,
            }
        })
        .collect();
    // The tab's number: the open questions of the team in view, never red (the owner, 13 October 2026, card
    // c628976b: "Nur der Desk im Blick"; the web counted every team and showed red on a knock). What waits on the
    // other teams stands in the menu, team by team.
    let badge = b.fresh().count() as u32;
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
            let sessions = if t.selected {
                rows.clone()
            } else {
                // (in a room without teams there is one, and it is in view)
                let mine = units_where(b, |a| a.team.as_deref() == Some(t.id.as_str()));
                rows_of(b, &mine, &asked)
            };
            TeamStackView {
                id: t.id.clone(),
                name: t.name.clone(),
                selected: t.selected && !b.lens.desk_all,
                open: !sessions.is_empty() && b.lens.open_teams.contains(&t.id),
                waiting: t.waiting,
                knocks: t.knocks,
                says: stack_words(sessions.len(), t.waiting),
                edges: sessions.iter().take(STACK_EDGES).map(|r| StackEdgeView { id: r.id.clone(), mark: r.mark.clone() }).collect(),
                // (a group's picture: the crowned one first, then in the rows' order)
                faces: sessions.iter().filter(|r| r.crowned).chain(sessions.iter().filter(|r| !r.crowned)).take(TEAM_FACES).map(|r| StackEdgeView { id: r.id.clone(), mark: r.mark.clone() }).collect(),
                sessions,
            }
        })
        .collect();

    // (a teaser's "14:05" turns into "Yesterday" at midnight)
    if rows.iter().any(|r| r.last.is_some()) {
        b.wake_at(Some(next_midnight(b.now, b.lens.utc_offset_min)));
    }
    SidebarView {
        stacks,
        desk_all: b.lens.desk_all,
        teams: tabs,
        sessions: rows,
        badge,
        team: in_view,
        place,
        waiting: b.fresh().count() as u32,
        knocking: b.fresh().filter(|f| f.knocks).count() as u32,
        stopped: units.iter().filter(|u| u.hand.is_some()).count() as u32,
        working: units.iter().filter(|u| u.working).count() as u32,
        unheard: units.iter().map(|u| u.unheard).sum(),
    }
}

/// The rows of these units in drawing order: the connected ones first, then those that are away, with no line
/// between them (a row that is away says so itself, greyed, with its cap).
fn rows_of(b: &Board, units: &[Unit], asked: &HashMap<usize, (u64, String)>) -> Vec<SessionRowView> {
    let mut rows = Vec::new();
    let on: Vec<usize> = (0..units.len()).filter(|u| units[*u].online).collect();
    let off: Vec<usize> = (0..units.len()).filter(|u| !units[*u].online).collect();
    for (away, list) in [(false, on), (true, off)] {
        for u in list {
            let group = format!("{}:{}", b.agents[units[u].agent].team.as_deref().unwrap_or(""), if away { "off" } else { "on" });
            rows.push(row(b, units, u, away, group, asked));
        }
    }
    rows
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
        mark: if a.row.icon.is_empty() { a.row.id.clone() } else { a.row.icon.clone() },
        team_id: a.team.clone().unwrap_or_else(|| DEFAULT_TEAM.into()),
        online: unit.online,
        working: unit.working,
        main: a.row.main,
        crowned: a.crowned,
        helpers,
        open: unit.open,
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
    fn the_connected_rows_first_then_disconnected() {
        let mut r = Room::default();
        r.session("a-off", None, false);
        r.session("b-on", None, true);
        r.session("e-off", None, false);
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["b-on", "a-off", "e-off"]);
        assert_eq!(v.sessions.iter().map(|x| x.away).collect::<Vec<_>>(), [false, true, true]);
        assert_eq!(row(&v, "a-off").group, ":off");
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
        // (a team that is gone counts as the first)
        r.on_team("c-work", "nowhere");
        r.card("q1", "d-lab", "decision", "normal");
        let v = view(&r, Some("lab"));
        assert_eq!(slugs(&v), ["b-lab", "d-lab"]);
        assert_eq!((v.team.as_str(), v.place.en.as_str(), v.badge), ("lab", "Lab", 1));
        assert_eq!(row(&v, "d-lab").group, "lab:off");

        // one team in view: only its sessions; the tab counts this team alone, the menu says what waits on the other
        let v = view(&r, Some("main"));
        assert_eq!(slugs(&v), ["c-work", "a-work"]);
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
    fn a_row_names_its_mark_and_its_address() {
        let mut r = Room::default();
        r.session("a", None, true)["profile"]["icon"] = json!("draw:leaf");
        r.session("b", None, true);
        r.settings.insert(dev("b"), json!({ "name": "Renamed", "icon": "draw:key" }));
        r.session("c", None, true);
        let v = view(&r, None);
        assert_eq!(v.sessions.iter().map(|x| (x.slug.as_str(), x.name.en.as_str(), x.mark.as_str())).collect::<Vec<_>>(), [("a", "a", "draw:leaf"), ("b", "Renamed", "draw:key"), ("c", "c", "c")]);
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
}
