//! Off your mind, of the team in view: the two stacks docked at the Desk's foot, the short hinted list under the Desk,
//! and the page with every card off the Desk. Which card lies where, in what order, and how many are closed is the
//! core's `off_mind` (crate::off_mind); here it is kept to the team in view and said.
//!
//! The rules are the web's of before (`desk.mjs` `offMind`, `deskDock`, `offHint`, `endList`, `restRow`; iOS
//! `DeskDock`, `OffHint`, `OffList`):
//! - a card counts where its session stands now; a card of a session the list does not show is in none;
//! - the starred stack: the last starred first; the running stack: the one that moved last first, starred ones left
//!   out (a card has one home);
//! - the hint: the running stack's first three cards at its head ("2 more running" beyond them), then the put-off cards, then the
//!   closed ones the newest first, the first three; "2 put off · 14 done";
//! - the page: every card with its agent (starred ones too, each in its place by time), then the put-off cards, then
//!   every closed card (done and thrown away merged, the newest first); a search keeps the cards whose title, session
//!   name (and, for one with its agent, status line) hold every word.

use super::board::{key_of, Board, Lens, OFF_PAGE};
use super::desk::{from_of, shown_title};
use super::words::{local, say};
use super::{DeskView, DockRowView, DockView, FromView, MoreView, OffHintView, OffListView, OffRowView, OffWorkView, Text};
use crate::model::types::Card;
use crate::off_mind::{closed_at, off_mind, OffMind, Running};
use crate::status::card_place;
use serde_json::Value;

/// How many rows the hint under the Desk shows.
pub const HINT_ROWS: usize = 3;

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// What has left the Desk, for the whole room (the core's rule).
pub fn of_board(b: &Board) -> OffMind {
    let off = off_mind(b.model, b.agents.iter().map(|a| &a.row), b.now);
    b.wake_at(off.wake);
    off
}

/// A card of this device whose session stands on the team in view, with its session.
fn here<'b>(b: &'b Board, id: &str) -> Option<(&'b Card, usize)> {
    let c = b.model.cards.get(id)?;
    let agent = b.by_key.get(key_of(&c.session_id, &c.agent_device_id)).copied()?;
    b.agents[agent].on_team.then_some((c, agent))
}

fn starred(b: &Board, id: &str) -> bool {
    b.model.human.get(&format!("star/{id}")).is_some_and(|r| !r.value.is_null() && r.value != Value::Bool(false))
}

fn from(b: &Board, agent: usize, c: &Card) -> FromView {
    from_of(b, Some(agent), Some(c)).unwrap_or_default()
}

/// "Tue 07:00" in the person's zone (en-GB, as the web writes it).
pub fn weekday_time(ms: u64, zone: i32) -> String {
    let l = local(ms, zone);
    format!("{} {:02}:{:02}", WEEKDAYS[(l.days + 4).rem_euclid(7) as usize], l.hour, l.minute)
}

/// A put-off or closed card's row. `place`: "later", "done" or "trash".
pub fn rest_row(b: &Board, c: &Card, place: &str, at: u64) -> OffRowView {
    let shredded = c.closed_how.as_deref() == Some("shredded");
    let (mark, word) = match place {
        "later" => ("snooze", "Later"),
        "trash" if shredded => ("shred", "Shredded"),
        "trash" => ("withdraw", "Withdrawn"),
        _ => ("tick", "Done"),
    };
    let until = (place == "later").then_some(c.snoozed_until).flatten().map(|u| {
        let when = weekday_time(u, b.lens.utc_offset_min);
        say("off.until", format!("until {when}"), &[("when", &when)])
    });
    OffRowView {
        id: c.object_id.clone(),
        place: place.into(),
        mark: mark.into(),
        title: shown_title(c),
        starred: starred(b, &c.object_id),
        at_ms: if place == "later" { 0 } else { at },
        until,
        says: say(&format!("off.place.{}", mark), word, &[]),
    }
}

fn dock_row(b: &Board, r: &Running, c: &Card, agent: usize) -> DockRowView {
    let from = from(b, agent, c);
    let name = from.name.clone();
    let line = if r.label.is_empty() {
        say("dock.line.on_it", format!("{name} · on it"), &[("name", &name)])
    } else {
        say("dock.line.label", format!("{name} · {}", r.label), &[("name", &name), ("label", &r.label)])
    };
    DockRowView { id: c.object_id.clone(), title: shown_title(c), from, line, running: true, at_ms: r.at }
}

fn count_says(key: &str, n: usize, word: &str) -> Text {
    let n = n.to_string();
    say(key, format!("{n} {word}"), &[("n", &n)])
}

/// Fills the Desk's Off your mind: the ids, the two docked stacks, the hint.
pub fn fill_desk(b: &Board, off: &OffMind, desk: &mut DeskView) {
    // ---- the stacks ----
    let star_rows: Vec<DockRowView> = off
        .starred
        .iter()
        .filter_map(|s| here(b, &s.object_id).map(|(c, a)| (s, c, a)))
        .map(|(s, c, agent)| {
            let from = from(b, agent, c);
            let name = from.name.clone();
            let line = if s.running {
                say("dock.line.starred_running", format!("{name} · running"), &[("name", &name)])
            } else {
                say("dock.line.starred", name.as_str(), &[("name", &name)])
            };
            DockRowView { id: c.object_id.clone(), title: shown_title(c), from, line, running: s.running, at_ms: s.at }
        })
        .collect();
    let run_rows: Vec<DockRowView> = off
        .running
        .iter()
        .filter_map(|r| here(b, &r.object_id).map(|(c, a)| dock_row(b, r, c, a)))
        .collect();
    desk.starred = (!star_rows.is_empty()).then(|| DockView {
        key: "star".into(),
        count: star_rows.len() as u32,
        says: count_says("dock.star.says", star_rows.len(), "starred"),
        rows: star_rows,
        all: None,
    });
    desk.running = (!run_rows.is_empty()).then(|| DockView {
        key: "run".into(),
        count: run_rows.len() as u32,
        says: count_says("dock.run.says", run_rows.len(), "running"),
        rows: run_rows,
        all: Some(say("dock.all", "All running and done", &[])),
    });

    // ---- the lists by id ----
    desk.off_your_mind = off.with_agents.iter().filter(|r| here(b, &r.object_id).is_some()).map(|r| r.object_id.clone()).collect();
    desk.later = off.put_off.iter().filter(|id| here(b, id).is_some()).cloned().collect();
    let recent: Vec<(&Card, &str, u64)> = off
        .recent
        .iter()
        .filter_map(|r| here(b, &r.object_id).map(|(c, _)| (c, r.place.as_str(), r.at)))
        .collect();
    desk.done = recent.iter().map(|(c, _, _)| c.object_id.clone()).collect();

    // ---- the hint ----
    let closed: u64 = b.agents.iter().filter(|a| a.on_team).filter_map(|a| off.closed.get(&a.row.key)).sum();
    let put_off = desk.later.len();
    let running: Vec<DockRowView> = desk.running.as_ref().map(|d| d.rows.iter().take(HINT_ROWS).cloned().collect()).unwrap_or_default();
    let more = desk.running.as_ref().map_or(0, |d| d.rows.len().saturating_sub(running.len()));
    let running_more = (more > 0).then(|| count_says("off.hint.running_more", more, "more running"));
    desk.off = (put_off > 0 || closed > 0 || !running.is_empty()).then(|| {
        let rows = desk
            .later
            .iter()
            .filter_map(|id| b.model.cards.get(id).map(|c| rest_row(b, c, "later", 0)))
            .chain(recent.iter().map(|(c, place, at)| rest_row(b, c, place, *at)))
            .take(HINT_ROWS)
            .collect();
        let (p, d) = (put_off.to_string(), closed.to_string());
        let says = match (put_off > 0, closed > 0) {
            (true, true) => say("off.hint.both", format!("{p} put off · {d} done"), &[("put_off", &p), ("done", &d)]),
            (true, false) => say("off.hint.put_off", format!("{p} put off"), &[("put_off", &p)]),
            (false, true) => say("off.hint.done", format!("{d} done"), &[("done", &d)]),
            (false, false) => say("off.hint.none", "Nothing done yet", &[]),
        };
        OffHintView { running, running_more, rows, says, put_off: put_off as u32, closed: closed as u32 }
    });
}

/// The page Off your mind for the search's words `query`.
pub fn off_list(b: &Board, off: &OffMind, query: &str) -> OffListView {
    let terms: Vec<String> = query.to_lowercase().split_whitespace().map(String::from).collect();
    let has = |words: &str| {
        let words = words.to_lowercase();
        terms.iter().all(|t| words.contains(t.as_str()))
    };
    let works: Vec<OffWorkView> = off
        .with_agents
        .iter()
        .filter_map(|r| here(b, &r.object_id).map(|(c, a)| (r, c, a)))
        .map(|(r, c, agent)| {
            let from = from(b, agent, c);
            let title = shown_title(c);
            let label = if r.label.is_empty() { say("off.work.on_it", "on it", &[]) } else { say("off.work.label", r.label.as_str(), &[("label", &r.label)]) };
            let says = say("off.work.says", format!("{title}, {} is on it", from.name), &[("title", &title), ("name", &from.name)]);
            OffWorkView { id: c.object_id.clone(), title, starred: starred(b, &c.object_id), from, label, at_ms: r.at, says }
        })
        .filter(|w| has(&format!("{} {} {}", w.title, w.from.name, w.label.en)))
        .collect();
    // (every closed card of the team in view, not only the window the patch names: done and thrown away merged)
    let mut closed: Vec<(u64, &Card, &'static str)> = Vec::new();
    for c in b.model.cards.values() {
        let Some(agent) = b.by_key.get(key_of(&c.session_id, &c.agent_device_id)).copied() else { continue };
        let a = &b.agents[agent];
        if !a.on_team || a.row.archived {
            continue;
        }
        let heard = b.model.sessions.get(&a.row.key).and_then(|s| s.heard_up_to);
        if let (Some(p @ ("done" | "trash")), _) = card_place(c, b.runs(agent), heard, b.now) {
            closed.push((closed_at(c, p), c, p));
        }
    }
    closed.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.object_id.cmp(&y.1.object_id)));
    let name_of = |c: &Card| b.by_key.get(key_of(&c.session_id, &c.agent_device_id)).map(|i| b.agents[*i].row.name.as_str()).unwrap_or("");
    let rest: Vec<OffRowView> = off
        .put_off
        .iter()
        .filter_map(|id| here(b, id))
        .map(|(c, _)| (rest_row(b, c, "later", 0), name_of(c)))
        .chain(closed.iter().map(|(at, c, p)| (rest_row(b, c, p, *at), name_of(c))))
        .filter(|(r, name)| has(&format!("{} {name}", r.title)))
        .map(|(r, _)| r)
        .collect();
    let reading = b.lens.off_reading;
    let none = (works.is_empty() && rest.is_empty() && !reading).then(|| {
        if terms.is_empty() {
            say("off.none", "Nothing yet.", &[])
        } else {
            say("off.none.search", "Nothing here has these words.", &[])
        }
    });
    // (the window: the first pages of what is put off and closed; what runs shows whole, it is short)
    let pages = Lens::pages(&b.lens.off_window, query);
    let mut rest = rest;
    let left = rest.len().saturating_sub(OFF_PAGE * pages as usize);
    rest.truncate(OFF_PAGE * pages as usize);
    // (past what this device holds, the hub's next page of settled cards: "Show more" reads it)
    let more = if left > 0 {
        let n = left.to_string();
        Some(MoreView { left: left as u32, hub: b.lens.off_hub_more, says: say("off.more", format!("Show more ({n})"), &[("left", &n)]), pages: pages + 1 })
    } else {
        (b.lens.off_hub_more && !reading).then(|| MoreView { left: 0, hub: true, says: say("off.more.hub", "Show more", &[]), pages: pages + 1 })
    };
    OffListView { query: query.into(), works, rest, none, more, older: None, loading: reading }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::json;

    fn render(r: &Room, lens: &Lens) -> (DeskView, Option<OffListView>) {
        let m = r.model(NOW);
        let b = Board::new(&m, lens, NOW);
        let mut desk = DeskView::default();
        let off = of_board(&b);
        fill_desk(&b, &off, &mut desk);
        let list = lens.off.as_deref().map(|q| off_list(&b, &off, q));
        (desk, list)
    }
    fn ids<T>(l: &[T], id: impl Fn(&T) -> &str) -> Vec<String> {
        l.iter().map(|x| id(x).to_string()).collect()
    }

    /// A room with every place: two running, one starred and running, one starred on the Desk, put off, done,
    /// shredded, withdrawn; and a card of the other team.
    fn room() -> Room {
        let mut r = Room { teams: json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } }), ..Default::default() };
        r.session("a", None, true);
        r.session("b", None, true);
        r.session("c-lab", None, true);
        r.on_team("c-lab", "lab");
        r.card("run-old", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 500 });
        r.cards.last_mut().unwrap()["updated_at"] = json!(NOW - 30 * MIN);
        r.card("run-new", "b", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 501 });
        r.cards.last_mut().unwrap()["updated_at"] = json!(NOW - 2 * MIN);
        r.card("run-star", "a", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 502 });
        r.cards.last_mut().unwrap()["updated_at"] = json!(NOW - 10 * MIN);
        r.card("on-desk-star", "a", "decision", "normal");
        r.card("later-1", "a", "decision", "normal");
        r.card("later-2", "b", "decision", "normal");
        r.snoozes = json!({ "later-1": { "until": NOW + 15 * 60 * MIN, "at": NOW - 9 * MIN }, "later-2": { "until": NOW + 60 * MIN, "at": NOW - 3 * MIN } });
        r.answered("done-1", "a", NOW - 8 * 60 * MIN, 40);
        let c = r.card("shredded", "b", "decision", "normal");
        c["object_state"] = json!("closed");
        c["closed_how"] = json!("shredded");
        c["answer"] = json!({ "answer_action": "shred", "answered_at": NOW - 60 * MIN });
        r.card("lab-run", "c-lab", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 503 });
        r.stars = json!({ "run-star": { "at": NOW - MIN }, "on-desk-star": { "at": NOW - 5 * MIN } });
        r
    }

    #[test]
    fn the_two_stacks_of_the_desk_in_view_a_starred_card_in_one_only() {
        let main = Lens { team: Some("main".into()), ..Default::default() };
        let (d, _) = render(&room(), &main);
        let star = d.starred.as_ref().unwrap();
        assert_eq!((star.key.as_str(), star.count, star.says.en.as_str()), ("star", 2, "2 starred"));
        assert_eq!(ids(&star.rows, |r| &r.id), ["run-star", "on-desk-star"], "the last starred on top");
        assert_eq!((star.rows[0].line.en.as_str(), star.rows[0].running, star.rows[1].line.en.as_str()), ("a · running", true, "a"));
        let run = d.running.as_ref().unwrap();
        assert_eq!(ids(&run.rows, |r| &r.id), ["run-new", "run-old"], "the one that moved last on top, no starred card, no other team's");
        assert_eq!((run.count, run.says.en.as_str(), run.rows[0].line.en.as_str()), (2, "2 running", "b · on it"));
        assert_eq!(run.all.as_ref().map(|t| t.en.as_str()), Some("All running and done"));
        assert_eq!(d.off_your_mind, ["run-new", "run-star", "run-old"], "the complete list holds the starred one in its place by time");
        // the other team: its own running card only
        let (d, _) = render(&room(), &Lens { team: Some("lab".into()), ..Default::default() });
        assert!(d.starred.is_none());
        assert_eq!(ids(&d.running.unwrap().rows, |r| &r.id), ["lab-run"]);
        // (what runs stands in the hint too, so the iPhone needs no stack over the Desk)
        let h = d.off.unwrap();
        assert_eq!(ids(&h.running, |r| &r.id), ["lab-run"]);
        assert_eq!((h.running_more, h.says.en.as_str(), h.rows.len()), (None, "Nothing done yet", 0));
    }

    #[test]
    fn the_hint_put_off_first_then_closed_and_how_many() {
        let (d, _) = render(&room(), &Lens::default());
        assert_eq!(d.later, ["later-2", "later-1"]);
        assert_eq!(d.done, ["shredded", "done-1"]);
        let h = d.off.unwrap();
        assert_eq!((h.says.en.as_str(), h.put_off, h.closed), ("2 put off · 2 done", 2, 2));
        assert_eq!(ids(&h.running, |r| &r.id), ["run-new", "run-old"], "the running stack's cards head the hint, no starred one");
        assert!(h.running_more.is_none(), "all of them shown");
        assert_eq!(h.rows.iter().map(|r| (r.id.as_str(), r.mark.as_str(), r.says.en.as_str())).collect::<Vec<_>>(), [("later-2", "snooze", "Later"), ("later-1", "snooze", "Later"), ("shredded", "shred", "Shredded")]);
        // (put off until the next morning, in the person's zone: NOW is Tue 13 Oct 2026 14:05 UTC)
        let (d, _) = render(&room(), &Lens { utc_offset_min: 120, ..Default::default() });
        assert_eq!(d.off.unwrap().rows[1].until.as_ref().unwrap().en, "until Wed 07:05");
        // nothing put off or closed: no hint
        let mut r = Room::default();
        r.session("a", None, true);
        let (d, _) = render(&r, &Lens::default());
        assert!(d.off.is_none() && d.starred.is_none() && d.running.is_none());
    }

    #[test]
    fn the_complete_list_and_its_search() {
        let lens = |q: &str| Lens { off: Some(q.into()), team: Some("main".into()), ..Default::default() };
        let (_, l) = render(&room(), &lens(""));
        let l = l.unwrap();
        assert_eq!(ids(&l.works, |w| &w.id), ["run-new", "run-star", "run-old"]);
        assert!(l.works[1].starred && !l.works[0].starred);
        assert_eq!((l.works[0].label.en.as_str(), l.works[0].says.en.as_str()), ("on it", "run-new, b is on it"));
        assert_eq!(ids(&l.rest, |r| &r.id), ["later-2", "later-1", "shredded", "done-1"]);
        assert_eq!((l.rest[2].at_ms, l.rest[3].at_ms, l.rest[0].at_ms), (NOW - 60 * MIN, NOW - 8 * 60 * MIN, 0));
        assert!(l.none.is_none() && l.older.is_none() && !l.loading);
        // every word, in the title or the session's name, any case
        let (_, l) = render(&room(), &lens("LATER b"));
        let l = l.unwrap();
        assert_eq!((ids(&l.works, |w| &w.id), ids(&l.rest, |r| &r.id)), (vec![], vec!["later-2".to_string()]));
        let (_, l) = render(&room(), &lens("nothing-like-this"));
        assert_eq!(l.unwrap().none.unwrap().en, "Nothing here has these words.");
        let mut r = Room::default();
        r.session("a", None, true);
        assert_eq!(render(&r, &lens("")).1.unwrap().none.unwrap().en, "Nothing yet.");
    }

    #[test]
    fn every_closed_card_is_on_the_page_not_only_the_window() {
        let mut r = Room::default();
        r.session("a", None, false);
        for i in 0..(crate::off_mind::RECENT as u64 + 30) {
            r.answered(&format!("k{i:03}"), "a", NOW - 7 * 60 * MIN - i, 10 + i);
        }
        let (d, l) = render(&r, &Lens { off: Some(String::new()), ..Default::default() });
        assert_eq!(d.done.len(), crate::off_mind::RECENT);
        assert_eq!(d.off.unwrap().closed as usize, crate::off_mind::RECENT + 30);
        // (all of them, through the window's pages)
        let l = l.unwrap();
        assert_eq!(l.rest.len() + l.more.map_or(0, |m| m.left as usize), crate::off_mind::RECENT + 30);
    }

    /// The complete list shows its first page of what is put off and closed; each "more" adds one.
    #[test]
    fn the_complete_list_shows_a_window() {
        let mut r = Room::default();
        r.session("a", None, false);
        for n in 0..(OFF_PAGE as u64 + 30) {
            r.answered(&format!("d{n:03}"), "a", NOW - 7 * 60 * MIN - n, 1000 + n);
        }
        let mut lens = Lens { off: Some(String::new()), ..Default::default() };
        let (_, list) = render(&r, &lens);
        let list = list.unwrap();
        assert_eq!(list.rest.len(), OFF_PAGE);
        assert_eq!(list.rest[0].id, "d000");
        let more = list.more.unwrap();
        assert_eq!((more.left, more.says.en.as_str(), more.pages), (30, "Show more (30)", 2));
        lens.show_more(crate::views::MoreOf::Off { query: String::new() });
        let (_, list) = render(&r, &lens);
        let list = list.unwrap();
        assert_eq!((list.rest.len(), list.more), (OFF_PAGE + 30, None));
        // a search has its own window
        lens.off = Some("d0".into());
        assert!(render(&r, &lens).1.unwrap().rest.len() <= OFF_PAGE);
    }
}
