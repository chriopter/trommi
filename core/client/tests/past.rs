//! The past on demand (`app::past`): a device whose model holds no history (a sign-in, a cache gone) reads a
//! session's Chat when the session is opened, the page before it on "Earlier messages", the starred cards it does
//! not hold, and counts the done cards from the hub's counts of the Desk. Against the stand-in hub (tests/common).

mod common;

use common::{start, World};
use trommi_client_core::actions::Action;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::{Effect, Event};
use trommi_client_core::views::{Focus, MoreOf, Phase, Screen};

fn focus(screen: Screen) -> Event {
    Event::Focus { focus: Focus { screen, visible: true, team: None, board: None, desk_all: false, open_teams: Vec::new(), chats_query: None, calm: false } }
}

/// The App's device as it stands, its model without the past: the stored device read the room up to the hub's
/// newest change (a first run), and the next run starts without a cache (as a sign-in on a new device does).
fn without_past(world: &World) -> Sim {
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    sim.cache.clear();
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    sim
}

fn talk(sim: &Sim, session: &str) -> Vec<String> {
    sim.view.sessions.get(session).map(|p| p.talk.iter().map(|r| r.text.clone()).collect()).unwrap_or_default()
}

#[test]
fn an_opened_session_reads_its_history_and_shows_no_welcome_while_it_comes() {
    let mut world = World::new();
    world.agent_says("one");
    world.agent_says("two");
    let mut sim = without_past(&world);
    let session = sim.view.sidebar.sessions[0].id.clone();
    assert!(world.hub.borrow().chat_reads.is_empty(), "nothing of a Chat is read before a page needs it");

    // opened: the newest page is asked; while it comes the page shows neither the talk nor the welcome
    sim.hold = true;
    sim.send(focus(Screen::Session { session_id: session.clone() }));
    let page = sim.view.sessions.get(&session).expect("the session's page");
    assert!(page.talk.is_empty() && page.empty.is_none(), "no welcome for a session whose history is still read: {:?}", page.empty);
    let (id, request) = sim.held.iter().find(|(_, r)| r.url.contains("/v1/chats/session/")).cloned().expect("the newest page of its Chat is asked");
    let got = world.hub.borrow_mut().http(sim.now_ms, &request);
    sim.answer(id, got);
    sim.hold = false;
    assert_eq!(talk(&sim, &session), ["one", "two"]);
    assert!(sim.view.sessions[&session].empty.is_none() && sim.view.sessions[&session].earlier.is_none());

    // asked once a run: opened again, nothing more is read
    sim.send(focus(Screen::Desk));
    sim.send(focus(Screen::Session { session_id: session.clone() }));
    assert_eq!(world.hub.borrow().chat_reads.len(), 1);
    assert_eq!(talk(&sim, &session), ["one", "two"]);
}

#[test]
fn a_session_the_hub_holds_nothing_of_shows_the_welcome_once_that_is_known() {
    let mut world = World::new();
    world.another_session();
    world.agent_says("hello");
    let mut sim = without_past(&world);
    let mut welcomed = 0;
    for row in sim.view.sidebar.sessions.clone() {
        sim.send(focus(Screen::Session { session_id: row.id.clone() }));
        let page = &sim.view.sessions[&row.id];
        if page.talk.is_empty() {
            assert!(page.empty.is_some(), "the hub holds nothing of it: the welcome");
            welcomed += 1;
        } else {
            assert_eq!(talk(&sim, &row.id), ["hello"]);
            assert!(page.empty.is_none());
        }
    }
    assert_eq!(sim.view.sidebar.sessions.len(), 2);
    assert_eq!(welcomed, 1, "one session is new, one has its line");
}

#[test]
fn earlier_messages_read_the_page_before_the_oldest_one_read() {
    let mut world = World::new();
    for n in 0..70 {
        world.agent_says(&format!("line {n}"));
    }
    let mut sim = without_past(&world);
    let session = sim.view.sidebar.sessions[0].id.clone();
    sim.send(focus(Screen::Session { session_id: session.clone() }));
    // the newest page (50) is held, 40 rows show, and the line above says the hub holds more
    let page = &sim.view.sessions[&session];
    assert_eq!((page.talk.len(), page.talk.last().map(|r| r.text.as_str())), (40, Some("line 69")));
    let earlier = page.earlier.clone().expect("Earlier messages");
    assert_eq!((earlier.left, earlier.hub), (10, true));
    sim.send(Event::Action { action: Action::ShowMore { of: MoreOf::Talk { session: session.clone() } } });
    let reads = world.hub.borrow().chat_reads.clone();
    assert_eq!(reads.len(), 2, "the page before the oldest one read: {reads:?}");
    assert!(reads[1].2.is_some(), "asked below the oldest change read");
    let page = &sim.view.sessions[&session];
    assert_eq!(page.talk.len(), 70, "every line is read now");
    assert_eq!(page.talk[0].text, "line 0");
    assert!(page.earlier.is_none(), "the hub holds no more");
}

#[test]
fn a_starred_card_this_device_does_not_hold_is_read_for_the_starred_stack() {
    let mut world = World::new();
    let (card, _, _) = world.agent_card("Kept for later");
    world.founder_answers(card, "yes");
    let id = trommi_core::ids::ObjectId::as_bytes(&card).iter().map(|b| format!("{b:02x}")).collect::<String>();
    world.founder_sets(&format!("star/{id}"), r#"{"at":1}"#);
    let sim = without_past(&world);
    let starred = sim.view.desk.starred.as_ref().expect("the starred stack stands at the start");
    assert_eq!(starred.rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Kept for later"]);
}

#[test]
fn the_done_count_is_the_hubs_for_cards_this_device_never_read() {
    let mut world = World::new();
    for n in 0..3 {
        let (card, _, _) = world.agent_card(&format!("Card {n}"));
        world.founder_answers(card, "yes");
    }
    world.agent_asks("Still open");
    let sim = without_past(&world);
    let off = sim.view.desk.off.as_ref().expect("Off your mind's line");
    assert_eq!((off.closed, off.says.en.as_str()), (3, "3 done"));
}

#[test]
fn an_idle_desk_asks_the_hub_almost_nothing() {
    let mut world = World::new();
    world.agent_says("hello");
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    sim.send(focus(Screen::Desk));
    let from = sim.log.len();
    // ten minutes on the Desk, the window in front: the page says what it says once more now and then
    for minute in 0..10 {
        for second in 0..60 {
            sim.advance(1_000);
            world.stream(&mut sim);
            // (the hub's keep-alive, every 25 s: HUB_PING_MS)
            if second % 25 == 0 {
                sim.stream(StreamItem::Chunk { bytes: b"event: ping\ndata: {}\n\n".to_vec().into() });
            }
        }
        sim.send(focus(Screen::Desk));
        if minute % 3 == 0 {
            sim.send(Event::Foreground);
            sim.send(Event::Net { online: true });
        }
    }
    let asked: Vec<String> = sim.log[from..]
        .iter()
        .filter_map(|e| match e {
            Effect::Http { request, .. } | Effect::OpenStream { request, .. } => Some(request.url.clone()),
            _ => None,
        })
        .collect();
    let cancelled = sim.log[from..].iter().filter(|e| matches!(e, Effect::Cancel { .. })).count();
    // (one stream, open all the while; the token renewed once with what follows a sign-in)
    assert!(asked.iter().filter(|u| u.contains("/v1/stream")).count() == 0, "the stream stays open: {asked:#?}");
    assert!(asked.len() <= 8, "ten idle minutes asked {} times: {asked:#?}", asked.len());
    assert!(cancelled <= 2, "{cancelled} requests cancelled");
}

#[test]
fn the_page_off_your_mind_reads_the_settled_cards_page_by_page() {
    let mut world = World::new();
    for n in 0..105 {
        let (card, _, _) = world.agent_card(&format!("Card {n:03}"));
        world.founder_answers(card, "yes");
    }
    let mut sim = without_past(&world);
    assert!(world.hub.borrow().settled_reads.is_empty(), "nothing settled is read before the page needs it");
    sim.send(focus(Screen::Off { query: String::new() }));
    let list = sim.view.off.clone().expect("the page Off your mind");
    // the newest page: 100 cards held, 100 rows show, and "Show more" for what the hub holds below
    assert_eq!((list.rest.len(), list.loading, list.none.is_none()), (100, false, true));
    assert_eq!(list.rest[0].title, "Card 104", "newest first");
    assert_eq!(list.more.as_ref().map(|m| (m.left, m.hub)), Some((0, true)));
    sim.send(Event::Action { action: Action::ShowMore { of: MoreOf::Off { query: String::new() } } });
    let list = sim.view.off.clone().unwrap();
    assert_eq!((list.rest.len(), list.more.is_none()), (105, true));
    assert_eq!(world.hub.borrow().settled_reads.len(), 2);
    // (each one answered after its version: its answer read too, so none stands on the Desk as open)
    assert!(sim.view.desk.rows.is_empty());
}

fn revised_closed_card(world: &mut World) -> String {
    let (card, hash, _) = world.agent_card("First version");
    let (hash, _) = world.agent_revises(card, &hash, "Second version", 2);
    world.founder_answers(card, "yes");
    world.agent_closes(card, &hash, "Second version", 3);
    card.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn history_down(sim: &mut Sim, world: &World) {
    let hub = world.hub.clone();
    sim.hub = Some(Box::new(move |now, request| {
        if request.url.contains("/v1/cards/") || request.url.contains("/v1/chats/") {
            Ok(trommi_client_core::app::io::HttpResponse { status: 503, headers: vec![trommi_client_core::app::io::Header { name: "Retry-After".into(), value: "10".into() }], body: b"{}".to_vec().into() })
        } else {
            hub.borrow_mut().http(now, request)
        }
    }));
}

#[test]
fn restarted_closed_card_recovers_its_answer_and_history_when_history_returns() {
    let mut world = World::new();
    let card = revised_closed_card(&mut world);
    let mut sim = without_past(&world);
    history_down(&mut sim, &world);
    sim.send(focus(Screen::Off { query: String::new() }));
    sim.send(focus(Screen::Card { card_id: card.clone() }));
    let failed_reads = sim.log.iter().filter(|e| matches!(e, Effect::Http { request, .. } if request.url.contains("/v1/cards/"))).count();
    world.attach(&mut sim);
    sim.advance(9_000);
    assert_eq!(sim.log.iter().filter(|e| matches!(e, Effect::Http { request, .. } if request.url.contains("/v1/cards/"))).count(), failed_reads, "Retry-After is honored");
    sim.advance(1_000);
    let got = &sim.view.cards[&card];
    assert_eq!(got.history, 1, "the previous revision is recovered");
    assert!(got.you_chose.is_some(), "the answer is recovered: {got:?}");
    assert_eq!(got.state, "closed");
}

#[test]
fn fresh_devices_agree_on_history_after_one_device_recovers_from_an_outage() {
    let mut world = World::new();
    let card = revised_closed_card(&mut world);
    let mut first = without_past(&world);
    let (store, device, keys) = world.newcomer();
    let mut second = world.sim_of(&store);
    second.send(start());
    let effects = second.app.join_sessions(second.now_ms, trommi_client_core::engine::sync::joins::SessionJoins { keys, later: None, groups: vec![world.group] });
    second.run(effects);
    for _ in 0..20 {
        let Some(bytes) = world.hub.borrow_mut().stream_news(&device) else { break };
        second.stream(StreamItem::Chunk { bytes: bytes.into() });
    }
    first.send(focus(Screen::Card { card_id: card.clone() }));
    history_down(&mut second, &world);
    second.send(focus(Screen::Card { card_id: card.clone() }));
    world.attach(&mut second);
    second.advance(10_000);
    assert_eq!(first.view.cards[&card].history, 1);
    assert_eq!(first.view.cards[&card].history, second.view.cards[&card].history);
    assert_eq!(first.view.cards[&card].versions, second.view.cards[&card].versions);
}
