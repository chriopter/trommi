//! The App's `Render` carries the board's views: what a shell draws is made from the engine's board model after
//! every batch, against the stand-in hub (tests/common).

mod common;

use common::{start, World};
use trommi_client_core::app::io::HttpRequest;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Effect, Event};
use trommi_client_core::views::{Focus, Phase, Screen, ViewPart};

fn serve_some(world: &World, sim: &mut Sim, most: usize, which: impl Fn(&HttpRequest) -> bool) -> usize {
    let mut served = 0;
    while let Some((id, request)) = sim.held.iter().find(|(_, r)| served < most && which(r)).cloned() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
        served += 1;
    }
    served
}

/// The parts of every `Render` since `from` in the log, by name.
fn rendered(sim: &Sim, from: usize) -> Vec<&'static str> {
    sim.log[from..]
        .iter()
        .filter_map(|e| if let Effect::Render { delta } = e { Some(&delta.parts) } else { None })
        .flatten()
        .map(|p| match p {
            ViewPart::Whole { .. } => "whole",
            ViewPart::Phase { .. } => "phase",
            ViewPart::Sidebar { .. } => "sidebar",
            ViewPart::Desk { .. } => "desk",
            ViewPart::Session { .. } => "session",
            ViewPart::Card { .. } => "card",
            _ => "other",
        })
        .collect()
}

#[test]
fn an_empty_room_shows_its_session_and_an_empty_desk() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    // (what the shell folded is what the core holds)
    assert_eq!(&sim.view, sim.app.view());
    assert_eq!((sim.view.desk.mode.as_str(), sim.view.desk.rows.len(), sim.view.sidebar.badge), ("desk", 0, 0));
    assert_eq!(sim.view.desk.empty.as_ref().map(|t| t.en.as_str()), Some("Questions land here."));
    assert_eq!(sim.view.sidebar.sessions.len(), 1, "the agent's session has its row");
}

#[test]
fn a_decision_that_arrives_stands_on_the_desk_and_on_its_sessions_page() {
    let mut world = World::new();
    world.agent_says("hello");
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!((sim.view.phase, sim.view.desk.rows.len()), (Phase::Live, 0));
    let from = sim.log.len();
    world.agent_asks("Ship it?");
    assert!(world.stream(&mut sim), "the stream brings it");
    assert_eq!(sim.view.desk.rows.iter().map(|r| (r.title.as_str(), r.kind.as_str())).collect::<Vec<_>>(), [("Ship it?", "decision")]);
    assert_eq!((sim.view.sidebar.badge, sim.view.sidebar.sessions[0].open, sim.view.desk.empty.is_none()), (1, 1, true));
    // (only the units that changed travel)
    let parts = rendered(&sim, from);
    assert!(parts.contains(&"desk") && parts.contains(&"sidebar") && !parts.contains(&"whole"), "{parts:?}");

    // the session's page and the card's page are made while they are in view, and taken away after
    let session = sim.view.sidebar.sessions[0].id.clone();
    let card = sim.view.desk.rows[0].id.clone();
    sim.send(Event::Focus { focus: Focus { screen: Screen::Session { session_id: session.clone() }, visible: true, desk: None } });
    let page = sim.view.sessions.get(&session).expect("the session in view has its unit");
    assert_eq!(page.talk.iter().map(|r| (r.kind.as_str(), r.text.as_str())).collect::<Vec<_>>(), [("agent", "hello"), ("ask", "")]);
    assert_eq!((page.open, page.talk[1].row.as_ref().map(|r| r.title.as_str())), (1, Some("Ship it?")));
    sim.send(Event::Focus { focus: Focus { screen: Screen::Card { card_id: card.clone() }, visible: true, desk: None } });
    assert!(sim.view.sessions.is_empty());
    let page = sim.view.cards.get(&card).expect("the card in view has its unit");
    assert_eq!((page.title.as_str(), page.nr.en.as_str(), page.answer.mode.as_str(), page.place.as_ref().map(|p| (p.at, p.of))), ("Ship it?", "Nr. 1", "options", Some((1, 1))));
    // the page Off your mind, with its search: made while in view
    sim.send(Event::Focus { focus: Focus { screen: Screen::Off { query: "nothing like this".into() }, visible: true, desk: None } });
    assert!(sim.view.cards.is_empty());
    assert_eq!(sim.view.off.as_ref().and_then(|o| o.none.as_ref()).map(|t| t.en.as_str()), Some("Nothing here has these words."));
    sim.send(Event::Focus { focus: Focus::default() });
    assert!(sim.view.sessions.is_empty() && sim.view.cards.is_empty() && sim.view.off.is_none());
    assert_eq!(&sim.view, sim.app.view());
}

/// The owner's complaint of 13 October 2026: the Desk moved while the room's history was read. The Desk stands
/// complete before the first page of the order, and no row moves, comes or goes while page after page is read.
#[test]
fn the_desks_rows_do_not_move_while_history_is_read() {
    let mut world = World::new();
    let cards: Vec<_> = (0..6).map(|n| world.agent_card(&format!("card {n}"))).collect();
    world.agent_revises(cards[0].0, &cards[0].1, "card 0, said better", 2);
    for n in 0..20 {
        world.agent_says(&format!("line {n}"));
    }
    world.hub.borrow_mut().page_max = Some(7);
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve_some(&world, &mut sim, usize::MAX, |r| r.url.contains("/rooms/"));
    assert!(sim.view.desk.rows.is_empty(), "nothing stands before the hub's list");
    serve_some(&world, &mut sim, usize::MAX, |r| r.url.contains("/v1/desk"));
    serve_some(&world, &mut sim, usize::MAX, |r| r.url.contains("/v1/cards/"));
    assert!(sim.app.board().desk_complete);
    let first = sim.view.desk.stack.clone();
    let titles: Vec<String> = sim.view.desk.rows.iter().map(|r| r.title.clone()).collect();
    assert_eq!(first.len(), 6, "{titles:?}");
    assert!(titles.contains(&"card 0, said better".to_string()), "{titles:?}");
    let from = sim.log.len();
    let mut pages = 0;
    while serve_some(&world, &mut sim, 1, |r| r.url.contains("/v1/changes")) > 0 {
        pages += 1;
        assert_eq!(sim.view.desk.stack, first, "after page {pages}");
        assert_eq!(sim.view.desk.rows.iter().map(|r| r.title.clone()).collect::<Vec<_>>(), titles, "after page {pages}");
        serve_some(&world, &mut sim, usize::MAX, |r| !r.url.contains("/v1/changes"));
    }
    assert!(pages >= 4, "{pages}");
    assert_eq!((sim.view.phase, &sim.view.desk.stack), (Phase::Live, &first));
    // (the rows themselves never travelled again while the pages were read: at most the head changed, once, when the
    //  reading ended)
    let desks = rendered(&sim, from).iter().filter(|p| **p == "desk").count();
    assert!(desks <= 1, "the Desk was sent {desks} times while history was read");
}
