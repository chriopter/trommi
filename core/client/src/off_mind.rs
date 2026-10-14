//! Off your mind: what has left the Desk, as every app shows it at the Desk's foot (the owner's pick of 13 October
//! 2026, "zwei Stapel wie HEY"). Two stacks are docked there, each its top card with the others behind it, fanned out
//! on a tap; under them the section is a short hinted list of what is put off and closed.
//!
//! One rule for every app, decided here and handed over in the patch (`off_mind`); the apps keep only the cards of
//! the desk in view and draw:
//!
//! - **the starred stack**: his starred cards (the register `star/<card>`, `{ at }`) that this device holds, the
//!   last starred first, then by id. A star taken away (`null`) is none. Each says whether it is being worked on.
//! - **the running stack**: the cards with their agents ([`crate::status::card_place`] says `works`: handed back,
//!   an info still worked on, or answered a while ago by a session that runs), the one that moved last first: by its
//!   working status line (the one that names it, else its session's newest working line that names no card), else
//!   by the moment it went to its agent. With that line's label.
//! - **a card has one home.** A starred card lies in the starred stack only, also while it is being worked on: the
//!   running stack leaves it out, so its count is exactly what its fan shows.
//! - **the rest**: the put-off cards, the last put off first; how many closed cards (done, withdrawn, shredded) each
//!   session has, so an app adds up the sessions of its desk; and the [`RECENT`] cards closed last, for the hint.
//!
//! A card of an archived session, or of a session the list does not show, is in none of these (its star stays).
//! `wake`: when a running card stops running by the clock alone.
use crate::model::types::{Card, Session};
use crate::model::Model;
use crate::sessions::SessionRow;
use crate::status::card_place;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// How many of the cards closed last the patch names (an app shows the first few of its desk).
pub const RECENT: usize = 120;

/// A starred card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Starred {
    pub object_id: String,
    /// When he set the star (ms); 0 when the register names no time.
    pub at: u64,
    /// It is being worked on (and so not in the running stack).
    pub running: bool,
}

/// A card being worked on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Running {
    pub object_id: String,
    /// When it last moved: its status line's update, else when it went to its agent.
    pub at: u64,
    /// Its status line's words; empty without a line.
    pub label: String,
}

/// A card closed lately: `place` is `done` or `trash`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Closed {
    pub object_id: String,
    pub place: String,
    pub at: u64,
}

/// What has left the Desk.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OffMind {
    pub starred: Vec<Starred>,
    pub running: Vec<Running>,
    pub put_off: Vec<String>,
    /// The closed cards of each session, counted, by the session's key.
    pub closed: BTreeMap<String, u64>,
    pub recent: Vec<Closed>,
    /// Every card with its agent, starred ones too, in the running stack's order: the complete list's top part.
    #[serde(skip)]
    pub with_agents: Vec<Running>,
    #[serde(skip)]
    pub wake: Option<u64>,
}

fn star_at(v: &Value) -> Option<u64> {
    (!v.is_null() && v != &Value::Bool(false)).then(|| v.get("at").and_then(Value::as_u64).unwrap_or(0))
}

/// When a card went to its agent: the hand-back's message, an info's start, his answer.
pub fn went_at(m: &Model, c: &Card) -> u64 {
    if c.object_state == "open" {
        if let Some(r) = &c.in_revision {
            let said = m.timelines.get(&c.timeline_key).and_then(|t| {
                t.items
                    .values()
                    .find(|i| i.get("envelope_number").and_then(Value::as_u64) == Some(r.envelope_number))
                    .and_then(|i| i.get("sent_at").and_then(Value::as_u64))
            });
            return said.unwrap_or(c.updated_at);
        }
        return c.created_at;
    }
    c.answer.as_ref().map(|a| a.answered_at).unwrap_or(c.updated_at)
}

/// When a closed card closed: his answer (a shredding is one), else its last change; a withdrawn one, when it came.
pub fn closed_at(c: &Card, place: &str) -> u64 {
    match (&c.answer, place) {
        (Some(a), _) if a.answered_at > 0 => a.answered_at,
        (_, "done") => c.updated_at,
        _ => c.created_at,
    }
}

/// Off your mind at `now`, for the sessions as listed (`rows`: sessions.rs `session_list`).
pub fn off_mind<'r>(m: &Model, rows: impl IntoIterator<Item = &'r SessionRow>, now: u64) -> OffMind {
    // (a card names its session, else its agent device: the list's row of either, as the apps pair them)
    let mut by_session: HashMap<&str, &SessionRow> = HashMap::new();
    let mut by_device: HashMap<&str, &SessionRow> = HashMap::new();
    for r in rows {
        by_session.insert(r.key.as_str(), r);
        if let Some(s) = r.session_id.as_deref() {
            by_session.insert(s, r);
        }
        if let Some(d) = r.agent_device_id.as_deref() {
            if r.parent.is_none() || !by_device.contains_key(d) {
                by_device.insert(d, r);
            }
        }
    }
    let row_of = |c: &Card| -> Option<&SessionRow> {
        c.session_id
            .as_deref()
            .and_then(|s| by_session.get(s))
            .or_else(|| by_device.get(c.agent_device_id.as_str()))
            .copied()
            .filter(|r| !r.archived)
    };
    let session_of = |r: &SessionRow| -> Option<&Session> { m.sessions.get(&r.key) };
    let stars: HashMap<&str, u64> = m
        .human
        .iter()
        .filter_map(|(k, r)| k.strip_prefix("star/").zip(star_at(&r.value)))
        .collect();

    let mut out = OffMind::default();
    let mut running_ids: HashSet<&str> = HashSet::new();
    let mut put_off: Vec<(u64, &str)> = vec![];
    // (a large room is mostly closed cards: counted by reference and named only for the newest RECENT)
    let mut closed: HashMap<&str, u64> = HashMap::new();
    let mut recent: Vec<(u64, &str, &'static str)> = vec![];
    for c in m.cards.values() {
        let Some(row) = row_of(c) else { continue };
        let s = session_of(row);
        let runs = row.online || row.link.as_ref().is_some_and(|l| l.state == "cut");
        let (place, until) = card_place(c, runs, s.and_then(|s| s.heard_up_to), now);
        match place {
            Some("later") => put_off.push((c.snoozed_at.unwrap_or(0), c.object_id.as_str())),
            Some("works") => {
                out.wake = match (out.wake, until) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                };
                running_ids.insert(c.object_id.as_str());
                // (its working line: the one that names it, else its session's newest working line naming no card;
                //  of two equally new, the first, as the web's workLine and the session's and card's pages)
                let working = s.into_iter().flat_map(|s| s.status_lines.iter()).filter(|t| {
                    row.lines.get(&t.id).map(String::as_str) == Some("working")
                });
                let named = working.clone().find(|t| t.object_id.as_str() == Some(c.object_id.as_str()));
                let line = named.or_else(|| {
                    working
                        .filter(|t| t.object_id.as_str().is_none_or(str::is_empty))
                        .fold(None, |best: Option<&crate::model::types::StatusLine>, t| match best {
                            Some(b) if b.updated_at >= t.updated_at => Some(b),
                            _ => Some(t),
                        })
                });
                out.with_agents.push(Running {
                    object_id: c.object_id.clone(),
                    at: line.map(|t| t.updated_at).unwrap_or_else(|| went_at(m, c)),
                    label: line.and_then(|t| t.label.as_str()).unwrap_or("").to_string(),
                });
            }
            Some(p @ ("done" | "trash")) => {
                *closed.entry(row.key.as_str()).or_default() += 1;
                recent.push((closed_at(c, p), c.object_id.as_str(), if p == "done" { "done" } else { "trash" }));
            }
            _ => {}
        }
    }
    out.with_agents.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.object_id.cmp(&b.object_id)));
    // (a card has one home: a starred one lies in the starred stack only)
    out.running = out.with_agents.iter().filter(|r| !stars.contains_key(r.object_id.as_str())).cloned().collect();
    put_off.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    out.put_off = put_off.into_iter().map(|(_, id)| id.to_string()).collect();
    out.closed = closed.into_iter().map(|(k, n)| (k.to_string(), n)).collect();
    let newest = |a: &(u64, &str, &str), b: &(u64, &str, &str)| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1));
    if recent.len() > RECENT {
        recent.select_nth_unstable_by(RECENT, newest);
        recent.truncate(RECENT);
    }
    recent.sort_by(newest);
    out.recent = recent.into_iter().map(|(at, id, place)| Closed { object_id: id.to_string(), place: place.into(), at }).collect();
    out.starred = stars
        .iter()
        .filter(|(id, _)| m.cards.contains_key(**id))
        .map(|(id, at)| Starred { object_id: id.to_string(), at: *at, running: running_ids.contains(id) })
        .collect();
    out.starred.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.object_id.cmp(&b.object_id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Answer, HumanRegister, InRevision, Member, StatusLine};
    use crate::sessions::session_list;
    use crate::status::ACTING_MS;
    use serde_json::json;

    const NOW: u64 = 100_000_000;
    const S1: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const S2: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn session(m: &mut Model, sid: &str, dev: &str, online: bool) {
        m.members.insert(
            dev.into(),
            Member { device_id: dev.into(), device_role: "agent".into(), is_active: true, is_online: online, added_entry_number: 1, ..Default::default() },
        );
        let s = m.session_of(sid);
        s.group_id = Some(format!("g{sid}"));
        s.agent_device_id = Some(dev.into());
        s.agent_device_ids = vec![dev.into()];
        s.is_online = online;
        let mut p = serde_json::Map::new();
        p.insert("is_main".into(), json!(true));
        s.profile = Some(p);
    }
    fn card<'a>(m: &'a mut Model, id: &str, sid: &str, state: &str) -> &'a mut Card {
        m.cards.insert(
            id.into(),
            Card { object_id: id.into(), session_id: Some(sid.into()), agent_device_id: "d1".into(), object_state: state.into(), ..Default::default() },
        );
        m.cards.get_mut(id).unwrap()
    }
    /// A card handed back at `at`: with its agent.
    fn handed_back(m: &mut Model, id: &str, sid: &str, at: u64) {
        let c = card(m, id, sid, "open");
        c.in_revision = Some(InRevision { by: "me".into(), envelope_number: 7 });
        c.updated_at = at;
    }
    fn answered(m: &mut Model, id: &str, sid: &str, at: u64, state: &str) {
        let c = card(m, id, sid, state);
        c.closed_how = Some(if state == "answered" { "answered" } else { "closed" }.into());
        c.answer = Some(Answer { answer_action: "choose".into(), choices: json!(["a"]), answered_at: at, ..Default::default() });
    }
    fn star(m: &mut Model, id: &str, v: Value) {
        m.human.insert(format!("star/{id}"), HumanRegister { value: v, ..Default::default() });
    }
    fn model() -> Model {
        let mut m = Model::new();
        m.clock = NOW;
        session(&mut m, S1, "d1", true);
        session(&mut m, S2, "d2", true);
        m
    }
    fn off(m: &Model) -> OffMind {
        off_mind(m, &session_list(m, false), NOW)
    }
    fn ids(l: &[Running]) -> Vec<&str> {
        l.iter().map(|r| r.object_id.as_str()).collect()
    }

    #[test]
    fn the_running_card_that_moved_last_lies_on_top() {
        let mut m = model();
        handed_back(&mut m, "old", S1, NOW - 9_000);
        handed_back(&mut m, "new", S2, NOW - 1_000);
        answered(&mut m, "acting", S1, NOW - 5_000, "answered");
        let o = off(&m);
        assert_eq!(ids(&o.running), ["new", "acting", "old"]);
        // a working status line that names a card moves it: the line's update counts, and its words are shown
        m.sessions.get_mut(S1).unwrap().status_lines = vec![StatusLine {
            id: "l1".into(), label: json!("Reworking"), state: json!("working"), object_id: json!("old"), updated_at: NOW - 10, ..Default::default()
        }];
        let o = off(&m);
        assert_eq!(ids(&o.running), ["old", "new", "acting"]);
        assert_eq!(o.running[0].label, "Reworking");
        assert_eq!(o.running[1].label, "");
    }

    #[test]
    fn of_two_unnamed_working_lines_of_the_same_time_the_first_stands() {
        let mut m = model();
        handed_back(&mut m, "a", S1, NOW - 9_000);
        let line = |id: &str, label: &str| StatusLine { id: id.into(), label: json!(label), state: json!("working"), updated_at: NOW - 10, ..Default::default() };
        m.sessions.get_mut(S1).unwrap().status_lines = vec![line("l1", "First"), line("l2", "Second")];
        let o = off(&m);
        assert_eq!(o.running[0].label, "First", "as the web's workLine and the session's and card's pages");
        assert_eq!(o.with_agents, o.running);
    }

    #[test]
    fn a_starred_card_has_one_home_the_starred_stack() {
        let mut m = model();
        handed_back(&mut m, "a", S1, NOW - 3_000);
        handed_back(&mut m, "b", S1, NOW - 2_000);
        handed_back(&mut m, "c", S2, NOW - 1_000);
        answered(&mut m, "d", S1, NOW - ACTING_MS - 1, "answered");
        star(&mut m, "b", json!({ "at": 50 }));
        star(&mut m, "d", json!({ "at": 90 }));
        let o = off(&m);
        assert_eq!(ids(&o.running), ["c", "a"], "the starred running card is not in the running stack");
        assert_eq!(
            o.starred,
            [
                Starred { object_id: "d".into(), at: 90, running: false },
                Starred { object_id: "b".into(), at: 50, running: true },
            ]
        );
        // unstarred: it is back in the running stack, in its place by time
        star(&mut m, "b", Value::Null);
        let o = off(&m);
        assert_eq!(ids(&o.running), ["c", "b", "a"]);
        assert_eq!(o.starred.len(), 1);
    }

    #[test]
    fn the_last_starred_first_ties_by_id_none_taken_away_or_unknown() {
        let mut m = model();
        for id in ["c1", "c2", "c3", "c4"] {
            card(&mut m, id, S1, "open");
        }
        star(&mut m, "c1", json!({ "at": 100 }));
        star(&mut m, "c2", json!({ "at": 300 }));
        star(&mut m, "c3", Value::Null);
        star(&mut m, "c4", json!({ "at": 300 }));
        star(&mut m, "gone", json!({ "at": 999 }));
        let o = off(&m);
        assert_eq!(o.starred.iter().map(|s| s.object_id.as_str()).collect::<Vec<_>>(), ["c2", "c4", "c1"]);
        star(&mut m, "c3", json!(true));
        assert_eq!(off(&m).starred.last().map(|s| (s.object_id.as_str(), s.at)), Some(("c3", 0)), "no time: last");
    }

    #[test]
    fn put_off_and_closed_are_counted_whole_per_session() {
        let mut m = model();
        for i in 0..150u64 {
            answered(&mut m, &format!("k{i:03}"), if i % 3 == 0 { S2 } else { S1 }, 1_000 + i, "closed");
        }
        let c = card(&mut m, "shred", S1, "closed");
        c.closed_how = Some("shredded".into());
        c.answer = Some(Answer { answer_action: "shred".into(), answered_at: 5_000, ..Default::default() });
        let c = card(&mut m, "later1", S1, "open");
        c.snoozed_until = Some(NOW + 1);
        c.snoozed_at = Some(10);
        let c = card(&mut m, "later2", S2, "open");
        c.snoozed_until = Some(NOW + 1);
        c.snoozed_at = Some(20);
        let o = off(&m);
        assert_eq!(o.put_off, ["later2", "later1"]);
        assert_eq!((o.closed[S1], o.closed[S2]), (101, 50), "every closed card is counted, not only the ones named");
        assert_eq!(o.recent.len(), RECENT);
        assert_eq!((o.recent[0].object_id.as_str(), o.recent[0].place.as_str()), ("shred", "trash"));
        assert_eq!((o.recent[1].object_id.as_str(), o.recent[1].place.as_str()), ("k149", "done"));
        assert!(o.running.is_empty());
    }

    #[test]
    fn an_answered_card_runs_while_its_session_does_and_says_when_it_stops() {
        let mut m = model();
        answered(&mut m, "x", S1, NOW - 1_000, "answered");
        let o = off(&m);
        assert_eq!(ids(&o.running), ["x"]);
        assert_eq!(o.wake, Some(NOW - 1_000 + ACTING_MS));
        // its session gone: done, counted with the closed
        m.sessions.get_mut(S1).unwrap().is_online = false;
        m.members.get_mut("d1").unwrap().is_online = false;
        let o = off(&m);
        assert!(o.running.is_empty());
        assert_eq!(o.closed[S1], 1);
    }

    #[test]
    fn an_archived_session_s_cards_are_in_no_stack() {
        let mut m = model();
        handed_back(&mut m, "a", S1, NOW - 1);
        answered(&mut m, "z", S1, 5, "closed");
        m.human.insert(format!("session/{S1}"), HumanRegister { value: json!({ "archived": true }), ..Default::default() });
        // (an archived session that is connected comes back by itself: this one is not)
        m.sessions.get_mut(S1).unwrap().is_online = false;
        m.members.get_mut("d1").unwrap().is_online = false;
        assert!(session_list(&m, false).iter().any(|r| r.key == S1 && r.archived));
        let o = off(&m);
        assert!(o.running.is_empty() && o.closed.is_empty() && o.recent.is_empty());
    }
}
