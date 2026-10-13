//! The sidebar: the desks, and the sessions of the desk in view as rows, in the order they are drawn. A shell draws
//! the rows one after the other and decides nothing: which sessions show, under which main, connected or under
//! "Disconnected", what waits, who is stopped and why, the small line under a name.
//!
//! The rules are the web's of before (`app.mjs` `boardModel` units, `sidebar.mjs` `sidebarParts` and `desksOf`,
//! `ui.mjs` `badge` and `linkCap`):
//! - a row per session that is not archived and stands on the desk in view;
//! - a helper lies under its main when that main has a row here and is no helper itself, one level deep; else it
//!   has a row of its own;
//! - a row is "away" when neither it nor one of its helpers is connected: the connected rows first, then
//!   "Disconnected" and the rest; on All desks desk by desk, each under its name, connected first;
//! - a row's open questions are those of its session on the Desk now; a main drawn folded shows its helpers'
//!   with its own ([`FoldView`]);
//! - a helper has no link of its own (its main's says it for both); what a helper's session has not picked up is
//!   told once, on its main.

use super::board::{Board, ALL_DESKS, DEFAULT_DESK};
use super::words::{badge_words, hand_words, link_words, say, unheard_words, LinkSubject};
use super::{CapView, DeskTab, FoldView, HandView, HeadingView, LinkWords, SessionRowView, SidebarView, Text};

/// One row before it is laid out: a session with what waits on it.
pub(super) struct Unit {
    /// Index into `Board::agents`.
    pub agent: usize,
    pub parent: Option<usize>,
    pub subs: Vec<usize>,
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

fn desk_name(name: &str) -> Text {
    if name.trim().is_empty() || name == "Personal" {
        say("desk.personal", "Personal", &[])
    } else {
        say("desk.name", name, &[])
    }
}

/// The units of the desk in view: one per session that is not archived and stands on it, in the list's order.
pub(super) fn units_of(b: &Board) -> Vec<Unit> {
    // ---- the units: one per session that stands here ----
    let here: Vec<usize> = (0..b.agents.len()).filter(|i| !b.agents[*i].row.archived && b.agents[*i].on_desk).collect();
    let fresh_of = |i: usize| b.fresh().filter(move |f| f.agent == Some(i));
    let mut units: Vec<Unit> = here
        .iter()
        .map(|&i| {
            let a = &b.agents[i];
            Unit {
                agent: i,
                parent: None,
                subs: Vec::new(),
                open: fresh_of(i).count() as u32,
                knocks: fresh_of(i).any(|f| f.knocks),
                online: a.row.online,
                working: a.row.online && a.row.working,
                hand: hand_of(b, i),
                link: None,
                unheard: b.unheard.get(&i).copied().unwrap_or(0),
            }
        })
        .collect();
    let unit_of = |units: &[Unit], id: &str| units.iter().position(|u| b.agents[u.agent].row.id == id);
    for u in 0..units.len() {
        let Some(main) = b.agents[units[u].agent].row.parent.as_deref().and_then(|p| unit_of(&units, p)) else { continue };
        if main != u && b.agents[units[main].agent].row.parent.is_none() {
            units[u].parent = Some(main);
            units[main].subs.push(u);
        }
    }
    // Each row's link, and what it has not picked up. A helper runs inside its main's process: it never listens on
    // its own, and what it has not picked up is told once, on its main.
    for u in units.iter_mut().filter(|u| u.parent.is_none()) {
        u.link = link_of(b, u.agent);
    }
    for u in 0..units.len() {
        let subs = units[u].subs.clone();
        for s in subs {
            units[u].unheard += std::mem::take(&mut units[s].unheard);
        }
    }
    units
}

pub fn sidebar(b: &Board) -> SidebarView {
    let units = units_of(b);

    // ---- the desks ----
    let desks: Vec<(String, String)> = if b.desks.is_empty() {
        vec![(DEFAULT_DESK.to_string(), "Personal".to_string())]
    } else {
        b.desks.iter().map(|d| (d.id.clone(), d.name.clone())).collect()
    };
    let first = desks[0].0.clone();
    let waits_on = |desk: &str| {
        let mine = b.all_fresh.iter().filter(|f| b.desk_of(f.agent).unwrap_or(first.as_str()) == desk);
        (mine.clone().count() as u32, mine.clone().any(|f| f.knocks))
    };
    let in_view = b.desk.clone().unwrap_or_else(|| first.clone());
    let tabs: Vec<DeskTab> = desks
        .iter()
        .map(|(id, name)| {
            let (waiting, knocks) = waits_on(id);
            DeskTab {
                id: id.clone(),
                name: desk_name(name),
                waiting,
                selected: !b.all && *id == in_view,
                knocks,
            }
        })
        .collect();
    // The tab's number: the open questions of the desk in view, never red (the owner, 13 October 2026, card
    // c628976b: "Nur der Desk im Blick"; the web counted every desk and showed red on a knock). What waits on the
    // other desks stands in the menu, desk by desk.
    let badge = b.fresh().count() as u32;
    let everywhere = b.all_fresh.len() as u32;
    let knocks = b.all_fresh.iter().any(|f| f.knocks);
    let all = (desks.len() > 1).then(|| DeskTab {
        id: ALL_DESKS.into(),
        name: say("desk.all", "All Desks", &[]),
        waiting: everywhere,
        selected: b.all,
        knocks,
    });
    let place = if b.all {
        say("desk.all_place", "All desks", &[])
    } else {
        desk_name(desks.iter().find(|(id, _)| *id == in_view).map(|(_, n)| n.as_str()).unwrap_or(""))
    };

    // ---- the rows, in drawing order ----
    let live = |u: &Unit| u.online || u.subs.iter().any(|s| units[*s].online);
    let top: Vec<usize> = (0..units.len()).filter(|u| units[*u].parent.is_none()).collect();
    // (the parts: on All desks one per desk, under its name; else the desk in view, with no line above it)
    let parts: Vec<(Option<&DeskTab>, Vec<usize>)> = if b.all {
        tabs.iter()
            .map(|t| (Some(t), top.iter().copied().filter(|u| b.agents[units[*u].agent].desk.as_deref() == Some(t.id.as_str())).collect::<Vec<_>>()))
            .filter(|(_, mine)| !mine.is_empty())
            .collect()
    } else {
        vec![(None, top.clone())]
    };
    let mut rows: Vec<SessionRowView> = Vec::new();
    for (desk, mine) in parts {
        let on: Vec<usize> = mine.iter().copied().filter(|u| live(&units[*u])).collect();
        let off: Vec<usize> = mine.iter().copied().filter(|u| !live(&units[*u])).collect();
        let mut heading = desk.map(|t| HeadingView {
            kind: "desk".into(),
            text: t.name.clone(),
            desk_id: Some(t.id.clone()),
            waiting: t.waiting,
        });
        for (away, list) in [(false, on), (true, off)] {
            // ("Disconnected" stands over the rows that are away; on All desks the desk's name is the only line)
            if away && desk.is_none() && !list.is_empty() {
                heading = Some(HeadingView {
                    kind: "away".into(),
                    text: say("sidebar.away", "Disconnected", &[]),
                    desk_id: None,
                    waiting: 0,
                });
            }
            for u in list {
                let group = format!("{}:{}", b.agents[units[u].agent].desk.as_deref().unwrap_or(""), if away { "off" } else { "on" });
                rows.push(row(b, &units, u, away, group, heading.take()));
                for s in units[u].subs.clone() {
                    rows.push(row(b, &units, s, away, String::new(), None));
                }
            }
        }
    }

    SidebarView {
        desks: tabs,
        sessions: rows,
        badge,
        desk: in_view,
        all,
        place,
        waiting: b.fresh().count() as u32,
        knocking: b.fresh().filter(|f| f.knocks).count() as u32,
        stopped: units.iter().filter(|u| u.hand.is_some()).count() as u32,
        working: units.iter().filter(|u| u.working).count() as u32,
        unheard: units.iter().map(|u| u.unheard).sum(),
    }
}

fn row(b: &Board, units: &[Unit], u: usize, away: bool, group: String, heading: Option<HeadingView>) -> SessionRowView {
    let unit = &units[u];
    let a = &b.agents[unit.agent];
    let key = |x: usize| b.agents[units[x].agent].row.key.clone();
    let main = unit.parent.map(|p| &units[p]);
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
    let folded = (!unit.subs.is_empty()).then(|| {
        let all: Vec<&Unit> = std::iter::once(unit).chain(unit.subs.iter().map(|s| &units[*s])).collect();
        let open = all.iter().map(|x| x.open).sum();
        let online = all.iter().any(|x| x.online);
        let working = all.iter().any(|x| x.working);
        // (the first of them that is stopped, in the list's order)
        let mut order = all.clone();
        order.sort_by_key(|x| x.agent);
        let hand = order.iter().find_map(|x| x.hand.clone());
        FoldView {
            says: says(hand.as_ref(), open, online, working),
            open,
            knocks: all.iter().any(|x| x.knocks),
            online,
            working,
            hand,
            stopped: unit.subs.iter().filter(|s| units[**s].hand.is_some()).map(|s| key(*s)).collect(),
        }
    });
    SessionRowView {
        id: a.row.key.clone(),
        name: say("session.name", a.row.name.as_str(), &[]),
        link: unit.link.as_ref().map(|l| l.state.clone()).unwrap_or_default(),
        blocked: unit.hand.is_some(),
        unheard: unit.unheard,
        depth: u32::from(unit.parent.is_some()),
        slug: a.row.id.clone(),
        mark: if a.row.icon.is_empty() { a.row.id.clone() } else { a.row.icon.clone() },
        desk_id: a.desk.clone().unwrap_or_else(|| DEFAULT_DESK.into()),
        online: unit.online,
        working: unit.working,
        main: a.row.main || !unit.subs.is_empty(),
        crowned: a.crowned,
        parent: unit.parent.map(key),
        subs: unit.subs.iter().map(|s| key(*s)).collect(),
        open: unit.open,
        knocks: unit.knocks,
        says: says(unit.hand.as_ref(), unit.open, unit.online, unit.working),
        hand: unit.hand.clone(),
        cap,
        hushed: main.and_then(|m| m.link.as_ref()).is_some_and(|l| matches!(l.state.as_str(), "asleep" | "cut" | "gone")),
        away,
        heading,
        group,
        folded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{dev, Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::{json, Value};

    fn view_at(r: &Room, desk: Option<&str>, now: u64) -> (SidebarView, Option<u64>) {
        let m = r.model(now);
        let lens = Lens { desk: desk.map(String::from), ..Default::default() };
        let b = Board::new(&m, &lens, now);
        let v = sidebar(&b);
        (v, b.wake())
    }
    fn view(r: &Room, desk: Option<&str>) -> SidebarView {
        view_at(r, desk, NOW).0
    }

    fn slugs(v: &SidebarView) -> Vec<String> {
        v.sessions.iter().map(|r| format!("{}{}", "  ".repeat(r.depth as usize), r.slug)).collect()
    }
    fn row<'v>(v: &'v SidebarView, slug: &str) -> &'v SessionRowView {
        v.sessions.iter().find(|r| r.slug == slug).unwrap_or_else(|| panic!("no row {slug}"))
    }
    fn two_desks() -> Value {
        json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } })
    }

    #[test]
    fn a_helper_lies_under_its_main_one_level_deep() {
        let mut r = Room::default();
        r.session("a-main", None, true);
        r.session("b-help", Some("a-main"), true);
        // (a helper's helper has a row of its own: one level only)
        r.session("c-deep", Some("b-help"), true);
        // (a helper whose main is not in the list has a row of its own, with its own link)
        r.session("d-lost", Some("nobody"), true);
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["a-main", "  b-help", "c-deep", "d-lost"]);
        let (main, help) = (row(&v, "a-main"), row(&v, "b-help"));
        assert_eq!(main.subs, std::slice::from_ref(&help.id));
        assert_eq!(help.parent.as_ref(), Some(&main.id));
        assert_eq!((main.link.as_str(), help.link.as_str(), row(&v, "d-lost").link.as_str()), ("live", "", "live"));
        assert!(main.main && main.folded.is_some() && !help.main && help.folded.is_none());
        assert_eq!((main.group.as_str(), help.group.as_str()), (":on", ""));
    }

    #[test]
    fn the_connected_rows_first_then_disconnected() {
        let mut r = Room::default();
        r.session("a-off", None, false);
        r.session("b-on", None, true);
        // (a main that is gone with a helper that is connected is not away)
        r.session("c-off", None, false);
        r.session("d-help", Some("c-off"), true);
        r.session("e-off", None, false);
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["b-on", "c-off", "  d-help", "a-off", "e-off"]);
        let heads: Vec<(&str, &str)> = v.sessions.iter().filter_map(|x| x.heading.as_ref().map(|h| (x.slug.as_str(), h.text.en.as_str()))).collect();
        assert_eq!(heads, [("a-off", "Disconnected")]);
        assert_eq!(v.sessions.iter().map(|x| x.away).collect::<Vec<_>>(), [false, false, false, true, true]);
        assert_eq!(row(&v, "a-off").group, ":off");
        // (gone since 13:27, said under its name)
        let cap = row(&v, "a-off").cap.as_ref().unwrap();
        assert_eq!((cap.kind.as_str(), cap.word.en.as_str(), cap.short.en.as_str()), ("gone", "gone · 13:27", "Gone since 13:27"));
        assert_eq!(row(&v, "b-on").cap, None);
    }

    #[test]
    fn all_desks_shows_desk_by_desk_under_each_desks_name() {
        let mut r = Room { desks: two_desks(), ..Default::default() };
        r.desks["empty"] = json!({ "name": "Empty", "created_at": 3 });
        r.session("a-work", None, false);
        r.session("b-lab", None, true);
        r.session("c-work", None, true);
        r.session("d-lab", None, false);
        r.on_desk("b-lab", "lab");
        r.on_desk("d-lab", "lab");
        // (a desk that is gone counts as the first)
        r.on_desk("c-work", "nowhere");
        r.card("q1", "d-lab", "decision", "normal");
        let v = view(&r, Some("all"));
        assert_eq!(slugs(&v), ["c-work", "a-work", "b-lab", "d-lab"]);
        let heads: Vec<_> = v.sessions.iter().map(|x| x.heading.as_ref().map(|h| (h.kind.as_str(), h.text.en.as_str(), h.desk_id.as_deref(), h.waiting))).collect();
        assert_eq!(heads, [Some(("desk", "Work", Some("main"), 0)), None, Some(("desk", "Lab", Some("lab"), 1)), None]);
        assert_eq!((v.desk.as_str(), v.place.en.as_str()), ("all", "All desks"));
        assert_eq!(v.all.as_ref().map(|a| (a.selected, a.waiting, a.name.en.as_str())), Some((true, 1, "All Desks")));
        assert!(v.desks.iter().all(|d| !d.selected));
        assert_eq!(row(&v, "d-lab").group, "lab:off");

        // one desk in view: only its sessions; the tab counts this desk alone, the menu says what waits on the other
        let v = view(&r, Some("main"));
        assert_eq!(slugs(&v), ["c-work", "a-work"]);
        assert_eq!((v.badge, v.waiting, v.place.en.as_str()), (0, 0, "Work"));
        assert_eq!(view(&r, Some("lab")).badge, 1);
        assert_eq!(view(&r, Some("all")).badge, 1);
        assert_eq!(v.desks.iter().map(|d| (d.id.as_str(), d.waiting, d.selected)).collect::<Vec<_>>(), [("main", 0, true), ("lab", 1, false), ("empty", 0, false)]);
        // a desk that is not there: the first
        assert_eq!(view(&r, Some("nowhere")).desk, "main");
        assert_eq!(view(&r, None).desk, "main");
    }

    #[test]
    fn a_room_without_desks_has_one_personal() {
        let mut r = Room::default();
        r.session("a", None, true);
        let v = view(&r, Some("all"));
        assert_eq!(v.desks.iter().map(|d| (d.id.as_str(), d.name.en.as_str(), d.name.key.as_str(), d.selected)).collect::<Vec<_>>(), [("main", "Personal", "desk.personal", true)]);
        assert_eq!((v.all.is_none(), v.desk.as_str(), v.place.en.as_str()), (true, "main", "Personal"));
        assert_eq!(slugs(&v), ["a"]);
        // (All desks needs two)
        let one = Room { desks: json!({ "main": { "name": "Solo" } }), ..Default::default() };
        let v = view(&one, Some("all"));
        assert_eq!((v.all.is_none(), v.desk.as_str(), v.place.en.as_str()), (true, "main", "Solo"));
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
        r.session("b-old", None, true);
        r.settings.insert(dev("b-old"), json!({ "archived": true, "parent": "a" }));
        r.card("q1", "b-old", "decision", "high");
        let v = view(&r, None);
        assert_eq!(slugs(&v), ["a"]);
        assert_eq!((v.badge, v.knocking, row(&v, "a").folded.is_none()), (0, 0, true));
    }

    #[test]
    fn one_crown_per_desk() {
        let mut r = Room { desks: two_desks(), ..Default::default() };
        for (name, desk) in [("a-work", "main"), ("b-work", "main"), ("c-lab", "lab"), ("d-lab", "lab")] {
            r.session(name, None, true);
            r.on_desk(name, desk);
        }
        let crowned = |r: &Room| -> Vec<String> { view(r, Some("all")).sessions.iter().filter(|x| x.crowned).map(|x| x.slug.clone()).collect() };
        // the room's single crown shows on the desk its session stands on
        r.crown = json!({ "agent_device_id": dev("a-work") });
        assert_eq!(crowned(&r), ["a-work"]);
        // a desk's own crown decides on that desk
        r.desks["lab"]["crown"] = json!({ "agent_device_id": dev("d-lab") });
        assert_eq!(crowned(&r), ["a-work", "d-lab"]);
        r.desks["main"]["crown"] = json!({ "session_id": dev("b-work") });
        assert_eq!(crowned(&r), ["b-work", "d-lab"]);
        // a desk whose crown was taken away has none, whatever the room's says
        r.desks["main"]["crown"] = Value::Null;
        r.desks["lab"] = json!({ "name": "Lab", "created_at": 2 });
        r.crown = json!({ "agent_device_id": dev("c-lab") });
        assert_eq!(crowned(&r), ["c-lab"]);
    }

    #[test]
    fn a_desks_crown_by_device_is_its_main_never_a_helper() {
        let mut r = Room { desks: json!({ "main": { "name": "Work", "crown": { "agent_device_id": dev("b-help") } } }), ..Default::default() };
        r.session("a", None, true);
        r.session("b-help", Some("a"), true);
        assert!(view(&r, None).sessions.iter().all(|x| !x.crowned));
    }

    #[test]
    fn an_answer_not_picked_up_for_two_minutes_waits_on_the_main() {
        let mut r = Room::default();
        r.session("a", None, true)["heard_up_to"] = json!(40);
        r.session("b-help", Some("a"), true)["heard_up_to"] = json!(40);
        r.answered("heard", "a", NOW - 30 * MIN, 30);
        r.answered("fresh", "a", NOW - MIN, 50);
        let (v, wake) = view_at(&r, None, NOW);
        assert_eq!((row(&v, "a").unheard, row(&v, "a").cap.is_none(), v.unheard), (0, true, 0));
        assert_eq!(wake, Some(NOW + MIN), "it turns late two minutes after he answered");

        let (v, wake) = view_at(&r, None, NOW + MIN);
        let cap = row(&v, "a").cap.clone().unwrap();
        assert_eq!((cap.kind.as_str(), cap.word.en.as_str(), cap.short.en.as_str(), cap.line), ("unheard", "1 answer waits", "Not heard yet", None));
        assert_eq!((v.unheard, wake), (1, None));

        // (a helper's is told once, on its main: the helper's row stays calm)
        r.answered("helper", "b-help", NOW - 5 * MIN, 60);
        // (a hand back its session has not picked up counts too: since its card went back)
        r.card("back", "b-help", "decision", "normal")["in_revision"] = json!({ "by": "me", "envelope_number": 70 });
        let v = view_at(&r, None, NOW + MIN).0;
        assert_eq!((row(&v, "a").unheard, row(&v, "b-help").unheard, v.unheard), (3, 0, 3));
        assert_eq!(row(&v, "a").cap.as_ref().map(|c| (c.word.en.as_str(), c.short.en.as_str())), Some(("3 answers wait", "3 not heard")));
        assert_eq!(row(&v, "b-help").cap, None);

        // (a session whose connector writes no receipts: nothing is known, nothing is said)
        r.sessions[0]["heard_up_to"] = Value::Null;
        assert_eq!(row(&view_at(&r, None, NOW + MIN).0, "a").unheard, 2);
    }

    #[test]
    fn a_link_that_does_not_hear_at_once_is_said_before_what_waits() {
        let mut r = Room::default();
        let s = r.session("a", None, true);
        s["heard_up_to"] = json!(40);
        s["link"] = json!({ "hears": "oncall", "attached": true, "last_call_at": NOW - 46 * MIN });
        r.session("b-help", Some("a"), true);
        r.answered("late", "a", NOW - 30 * MIN, 50);
        let (v, wake) = view_at(&r, None, NOW);
        let a = row(&v, "a");
        let cap = a.cap.clone().unwrap();
        assert_eq!((a.link.as_str(), cap.kind.as_str(), cap.word.en.as_str(), a.unheard), ("asleep", "asleep", "not listening · 46 min", 1));
        assert_eq!(cap.line.map(|l| l.en), Some("a is not listening: it hears you only on its next step, and its last one was 46 min ago.".into()));
        assert_eq!(wake, Some(NOW + 30_000), "the minutes count on");
        // (its helper cannot be reached either: drawn hushed)
        assert!(row(&v, "b-help").hushed && !a.hushed);
    }

    #[test]
    fn a_folded_main_shows_its_helpers_with_its_own() {
        let mut r = Room::default();
        r.session("a", None, false);
        r.session("b-help", Some("a"), true)["status_lines"] = json!([{ "id": "l", "label": "L", "state": "working", "updated_at": NOW - MIN }]);
        r.session("c-help", Some("a"), true);
        r.card("q1", "a", "decision", "normal");
        r.card("q2", "b-help", "decision", "normal");
        r.card("q3", "c-help", "decision", "critical");
        let v = view(&r, None);
        let (a, c) = (row(&v, "a"), row(&v, "c-help"));
        assert_eq!((a.open, a.knocks, a.online, a.working, a.blocked), (1, false, false, false, false));
        assert_eq!(a.says.as_ref().map(|t| t.en.as_str()), Some("Disconnected, 1 question open"));
        let f = a.folded.as_ref().unwrap();
        assert_eq!((f.open, f.knocks, f.online, f.working), (3, true, true, true));
        assert_eq!(f.hand.as_ref().map(|h| h.why.as_str()), Some("blocking"));
        assert_eq!(f.says.as_ref().map(|t| t.en.as_str()), Some("Stopped: Waiting for you: a blocking question, 3 questions open"));
        assert_eq!(f.stopped, std::slice::from_ref(&c.id));
        assert_eq!((c.blocked, c.says.as_ref().map(|t| t.en.as_str())), (true, Some("Stopped: Waiting for you: a blocking question, 1 question open")));
        assert!(row(&v, "b-help").working);
        assert_eq!((v.stopped, v.working, a.away), (1, 1, false));
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
}
