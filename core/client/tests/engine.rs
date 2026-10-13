//! The sync engine against the stand-in hub (tests/common): sign-in, catch-up from the cursor, the live stream,
//! the outbox, retries, restarts.

mod common;

use common::{start, World};
use trommi_client_core::app::Effect;
use trommi_client_core::views::Phase;

#[test]
fn a_stored_device_signs_in_catches_up_and_goes_live() {
    let mut world = World::new();
    let at = world.agent_says("one");
    world.agent_says("two");
    let last = world.agent_asks("Ship it?");
    let mut sim = world.sim();
    let before = world.reopen_me(&sim).cursor();
    assert!(before < at, "the lines are news to the App's device");
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.engine().unwrap().position(), last);
    // durable: what a restart finds is at the same place
    assert_eq!(world.reopen_me(&sim).cursor(), last);
    let seen = world.hub.borrow().seen.clone();
    assert!(seen[0].ends_with("/challenge") && seen[1].ends_with("/tokens"), "{seen:?}");
    assert!(seen[2].starts_with("GET /v1/desk"), "the Desk before the order: {seen:?}");
    assert!(seen[3].starts_with(&format!("GET /v1/changes?after={before}")), "{seen:?}");
    assert!(seen.iter().any(|s| s.starts_with("STREAM /v1/stream?after=")), "{seen:?}");
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
    // nothing waits for the clock but the stream's silence
    assert!(sim.timer.is_some());
    let _ = sim.log.iter().filter(|e| matches!(e, Effect::Http { .. })).count();
}

use common::hub::{answer, later};
use serde_json::json;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use trommi_client_core::app::io::{HttpRequest, HttpResponse, IoError, IoErrorKind, StoreOp, StreamItem};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::engine::Alert;
use trommi_core::device::Draft;
use trommi_core::store::OutboxKind;

fn offline() -> Result<HttpResponse, IoError> {
    Err(IoError::new(IoErrorKind::Offline))
}

/// Fires the timer the App holds: the clock moves to it.
fn fire(sim: &mut Sim) -> u64 {
    let at = sim.timer.expect("a timer is set");
    let wait = at.saturating_sub(sim.now_ms);
    sim.advance(wait);
    wait
}

/// The App's own device writes a line: sealed into the outbox, stored, and the pump is woken.
fn me_says(world: &World, sim: &mut Sim, line: &str) -> u64 {
    let draft = Draft::SessionChat { session: world.session, payload: common::text(&format!(r#"{{"content_type":"message","text":"{line}"}}"#)) };
    let (sealed, effects) = sim.app.act(sim.now_ms, |device, now| device.seal(&draft, None, &[], now));
    let id = sealed.expect("a device").expect("the line seals").outbox_id;
    sim.run(effects);
    id
}

fn position(sim: &Sim) -> u64 {
    sim.app.engine().expect("an engine").position()
}

fn requests(world: &World) -> Vec<String> {
    world.hub.borrow().seen.clone()
}

/// The envelopes the hub holds of `device`, as (change, seq).
fn chain_at_hub(world: &World, device: &trommi_core::ids::DeviceId) -> Vec<u64> {
    world
        .hub
        .borrow()
        .order
        .iter()
        .filter_map(|i| match i {
            common::hub::Item::Envelope { sender, seq, .. } if sender == device => Some(*seq),
            _ => None,
        })
        .collect()
}

#[test]
fn a_catch_up_reads_page_after_page_from_the_cursor_and_the_stream_begins_where_they_end() {
    let mut world = World::new();
    let mut last = 0;
    for n in 0..7 {
        last = world.agent_says(&format!("line {n}"));
    }
    world.hub.borrow_mut().page_max = Some(3);
    let mut sim = world.sim();
    let from = world.reopen_me(&sim).cursor();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(position(&sim), last);
    let pages: Vec<String> = requests(&world).into_iter().filter(|r| r.starts_with("GET /v1/changes")).collect();
    assert_eq!(pages.len(), 3, "{pages:?}");
    assert!(pages[0].contains(&format!("after={from}&")));
    // each page begins at the cursor the page before answered
    assert!(pages[1].contains(&format!("after={}&", from + 3)) && pages[2].contains(&format!("after={}&", from + 6)), "{pages:?}");
    // the stream was opened beside the first page at the head the hub named, and nothing was read twice
    let streams: Vec<String> = requests(&world).into_iter().filter(|r| r.starts_with("STREAM")).collect();
    assert_eq!(streams, vec![format!("STREAM /v1/stream?after={last}")]);
    assert!(sim.app.alerts().is_empty());
}

#[test]
fn a_head_that_lies_costs_a_second_stream_and_nothing_else() {
    for lie in [1u64, 1 << 40] {
        let mut world = World::new();
        let last = world.agent_says("before");
        world.hub.borrow_mut().head_lie = Some(lie);
        world.hub.borrow_mut().page_max = Some(1);
        let mut sim = world.sim();
        sim.send(start());
        assert_eq!(position(&sim), last);
        // what comes after is taken whatever the hub said its head was
        let next = world.agent_says("after");
        if !world.stream(&mut sim) {
            // (the stream that began beyond the end was closed; the next one opens from the position)
            fire(&mut sim);
            world.stream(&mut sim);
        }
        assert_eq!(position(&sim), next, "head said {lie}");
        assert_eq!(world.reopen_me(&sim).cursor(), next);
        assert_eq!(sim.view.phase, Phase::Live);
        let streams: Vec<String> = requests(&world).into_iter().filter(|r| r.starts_with("STREAM")).collect();
        assert!(streams.last().unwrap().ends_with(&format!("after={last}")) || streams.len() == 1, "{streams:?}");
    }
}

#[test]
fn the_stream_brings_changes_in_order_and_resumes_after_a_cut() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let a = world.agent_says("a");
    assert!(world.stream(&mut sim));
    assert_eq!(position(&sim), a);
    // the stream breaks; what is written meanwhile is not lost and not taken twice
    assert!(sim.stream(StreamItem::Failed { error: IoError::new(IoErrorKind::Offline) }));
    assert_ne!(sim.view.phase, Phase::Live);
    let b = world.agent_says("b");
    let c = world.agent_asks("Still there?");
    let wait = fire(&mut sim);
    assert!((750..=1_000).contains(&wait), "the first wait is a second, jittered down: {wait}");
    assert_eq!(position(&sim), c);
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(b < c);
    let streams: Vec<String> = requests(&world).into_iter().filter(|r| r.starts_with("STREAM")).collect();
    assert_eq!(streams.len(), 2);
    // the second stream resumes by the change number the device stands at
    let resumed: u64 = streams[1].rsplit('=').next().unwrap().parse().unwrap();
    assert!((a..=c).contains(&resumed), "{streams:?}");
    assert_eq!(world.reopen_me(&sim).cursor(), c);
    assert!(sim.app.alerts().is_empty());
}

#[test]
fn a_chunk_cut_anywhere_gives_the_same_events() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    world.agent_says("a");
    let last = world.agent_says("b");
    let bytes = world.hub.borrow_mut().stream_news(&world.me).expect("news");
    for piece in bytes.chunks(7) {
        sim.stream(StreamItem::Chunk { bytes: piece.to_vec().into() });
    }
    assert_eq!(position(&sim), last);
}

#[test]
fn waits_double_from_a_second_and_never_undercut_retry_after() {
    let world = World::new();
    let mut sim = world.sim();
    world.hub.borrow_mut().fault = Some(Box::new(|_, _| Some(offline())));
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Offline);
    let mut waits = Vec::new();
    for _ in 0..8 {
        waits.push(fire(&mut sim));
    }
    for (n, wait) in waits.iter().enumerate() {
        let base = (1_000u64 << n).min(60_000);
        assert!((base * 3 / 4..=base).contains(wait), "wait {n} is {wait}, of {base}");
    }
    // the hub says when: never sooner
    world.hub.borrow_mut().fault = Some(Box::new(|_, _| Some(Ok(later(429, 300)))));
    fire(&mut sim);
    assert!(fire(&mut sim) >= 300_000);
    // one event at a time: no request left without the timer
    let asked = sim.log.iter().filter(|e| matches!(e, Effect::Http { .. })).count();
    assert_eq!(asked, 11, "one try per wait, and the first");
    world.hub.borrow_mut().fault = None;
    fire(&mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
}

#[test]
fn a_token_that_ended_is_made_again_and_nothing_is_lost() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    world.hub.borrow_mut().end_tokens();
    let id = me_says(&world, &mut sim, "after the token ended");
    let _ = id;
    assert_eq!(chain_at_hub(&world, &world.me).len(), 1, "posted once signed in again: {:?}", requests(&world));
    let tokens = requests(&world).into_iter().filter(|r| r.ends_with("/tokens")).count();
    assert_eq!(tokens, 2);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    let _ = world.agent_says("x");
}

#[test]
fn the_outbox_goes_one_at_a_time_in_order_through_offline_and_not_now() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    // the hub is not reached, then answers 503 with a wait, then takes them
    let posts = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
    let seen = posts.clone();
    let mut script: VecDeque<Result<HttpResponse, IoError>> = [offline(), offline(), Ok(later(503, 5))].into_iter().collect();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        if !request.url.ends_with("/v1/envelopes") {
            return None;
        }
        seen.borrow_mut().push(request.body.as_ref().map(|b| b.0.clone()).unwrap_or_default());
        script.pop_front()
    }));
    for line in ["one", "two", "three"] {
        me_says(&world, &mut sim, line);
    }
    // one in flight: the second and third wait behind the first, which failed
    assert_eq!(posts.borrow().len(), 1);
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 3);
    fire(&mut sim);
    assert_eq!(posts.borrow().len(), 2);
    fire(&mut sim);
    assert_eq!(posts.borrow().len(), 3);
    let wait = fire(&mut sim);
    assert!(wait >= 5_000, "not sooner than Retry-After: {wait}");
    // the same bytes every time, then the next entries, each once
    let posts = posts.borrow();
    assert_eq!(posts.len(), 6, "four tries of the first, then the second and third");
    assert!(posts[..4].iter().all(|p| *p == posts[0]));
    assert!(posts[4] != posts[0] && posts[5] != posts[4]);
    assert_eq!(chain_at_hub(&world, &world.me), vec![1, 2, 3]);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    assert_eq!(world.reopen_me(&sim).outbox().len(), 0, "the answers are stored");
}

#[test]
fn a_refusal_ends_an_entry_only_when_it_is_the_hubs_own_word() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    // a proxy's page (a 403 that names no code of the protocol), then the hub's refusal with a void record
    let mut script: VecDeque<HttpResponse> = [
        HttpResponse { status: 403, headers: Vec::new(), body: b"<html>blocked</html>".to_vec().into() },
        answer(403, json!({ "error": "forbidden", "message": "refused", "voided": true })),
    ]
    .into_iter()
    .collect();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| if request.url.ends_with("/v1/envelopes") { script.pop_front().map(Ok) } else { None }));
    me_says(&world, &mut sim, "refused");
    // not the hub's refusal: the entry stays and goes again
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 1);
    me_says(&world, &mut sim, "taken");
    world.hub.borrow_mut().void_next(world.group, world.me);
    fire(&mut sim);
    // the void record ends the first (its number stays used); the second goes after it
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    assert_eq!(chain_at_hub(&world, &world.me), vec![2]);
}

#[test]
fn an_envelope_refused_without_a_void_record_blocks_the_pump_and_goes_again() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let mut script: VecDeque<HttpResponse> = [answer(409, json!({ "error": "epoch-full", "message": "refused" }))].into_iter().collect();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| if request.url.ends_with("/v1/envelopes") { script.pop_front().map(Ok) } else { None }));
    me_says(&world, &mut sim, "first");
    me_says(&world, &mut sim, "second");
    // its number may never be signed again (9.0.1): nothing passes it
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 2);
    assert!(chain_at_hub(&world, &world.me).is_empty());
    let wait = fire(&mut sim);
    assert_eq!(wait, 30_000);
    assert_eq!(chain_at_hub(&world, &world.me), vec![1, 2]);
}

#[test]
fn a_restart_mid_outbox_posts_the_stored_entries_again_unchanged_and_in_order() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let posts = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
    let (seen, down) = (posts.clone(), Rc::new(RefCell::new(true)));
    let is_down = down.clone();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        if request.url.ends_with("/v1/envelopes") {
            seen.borrow_mut().push(request.body.as_ref().map(|b| b.0.clone()).unwrap_or_default());
        }
        if *is_down.borrow() { Some(offline()) } else { None }
    }));
    for line in ["one", "two", "three"] {
        me_says(&world, &mut sim, line);
    }
    let first = posts.borrow()[0].clone();
    // the process dies; a new App opens what was stored
    sim.restart();
    *down.borrow_mut() = false;
    posts.borrow_mut().clear();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    // the stored entries first, in their order; what upkeep wrote after the restart (its `heads`) goes behind them
    let n = posts.borrow().len();
    assert!((3..=4).contains(&n), "{n}");
    assert_eq!(posts.borrow()[0], first, "the same bytes after the restart");
    let chain = chain_at_hub(&world, &world.me);
    assert_eq!(chain, (1..=chain.len() as u64).collect::<Vec<_>>());
    assert!(chain.len() >= 3);
    // the device's own envelopes came back in the hub's order and the outbox is empty, durably
    assert!(world.reopen_me(&sim).outbox().is_empty());
}

#[test]
fn what_the_device_wrote_is_stored_before_anything_it_implies_is_sent() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let draft = Draft::SessionChat { session: world.session, payload: common::text(r#"{"content_type":"message","text":"x"}"#) };
    let (_, effects) = sim.app.act(sim.now_ms, |device, now| device.seal(&draft, None, &[], now));
    let write = effects.iter().position(|e| matches!(e, Effect::Store { op: StoreOp::DeviceApply { .. }, .. })).expect("a write");
    let post = effects.iter().position(|e| matches!(e, Effect::Http { request, .. } if request.url.ends_with("/v1/envelopes"))).expect("a post");
    assert!(write < post);
    // in every answer of the whole run, no request stands before the write of the same answer's state
    sim.run(effects);
}

#[test]
fn another_owner_of_the_stored_state_stops_the_app() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    // somebody else wrote the device state
    sim.device.0 += 1;
    world.agent_says("news");
    world.stream(&mut sim);
    assert_eq!(sim.view.phase, Phase::Stopped);
    let before = sim.log.len();
    assert!(sim.streams.is_empty(), "the stream was cancelled");
    sim.advance(120_000);
    sim.send(trommi_client_core::app::Event::Foreground);
    assert!(!sim.log[before..].iter().any(|e| matches!(e, Effect::Http { .. } | Effect::OpenStream { .. } | Effect::Store { .. })));
}

#[test]
fn a_change_the_hub_skips_is_filled_from_its_senders_chain() {
    let mut world = World::new();
    let skipped = world.agent_says("skipped");
    world.hub.borrow_mut().hidden.insert(skipped);
    let last = world.agent_says("after it");
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(position(&sim), last);
    assert!(requests(&world).iter().any(|r| r.contains("/chains/")), "the gap was filled from the chain");
    assert!(sim.app.alerts().is_empty(), "nothing is withheld: {:?}", sim.app.alerts());
    let me = world.reopen_me(&sim);
    assert_eq!(me.chain_head(&world.group, &world.agent.id()).unwrap().seq, world.agent.chain_head(&world.group, &world.agent.id()).unwrap().seq);
}

#[test]
fn a_withheld_chain_is_said_in_every_phase_and_mended_when_the_hub_serves_it() {
    for live in [false, true] {
        let mut world = World::new();
        let agent = world.agent.id();
        let mut sim = world.sim();
        if live {
            sim.send(start());
        }
        let skipped = world.agent_says("withheld");
        let seq = world.agent.chain_head(&world.group, &agent).unwrap().seq;
        world.hub.borrow_mut().hidden.insert(skipped);
        world.hub.borrow_mut().withhold.insert(agent, seq);
        let last = world.agent_says("after it");
        if live {
            world.stream(&mut sim);
        } else {
            sim.send(start());
        }
        // the order goes on, the gap is said, and what follows it is not taken
        assert_eq!(position(&sim), last, "live: {live}");
        assert!(matches!(sim.app.alerts(), [Alert::Withheld { sender, .. }] if *sender == agent), "live: {live}: {:?}", sim.app.alerts());
        assert_eq!(sim.app.engine().unwrap().device().chain_head(&world.group, &agent).unwrap().seq, seq - 1);
        // the hub serves the chain again: asked for when the order next comes to rest, and taken whole
        world.hub.borrow_mut().withhold.clear();
        sim.stream(StreamItem::Failed { error: IoError::new(IoErrorKind::Offline) });
        fire(&mut sim);
        assert_eq!(sim.app.engine().unwrap().device().chain_head(&world.group, &agent).unwrap().seq, seq + 1, "live: {live}");
        assert_eq!(sim.app.alerts().len(), 1, "said once");
    }
}

#[test]
fn an_answer_longer_than_its_cap_is_cut_off_and_asked_for_smaller_after_a_wait() {
    let mut world = World::new();
    let last = world.agent_says("a");
    let mut sim = world.sim();
    let mut big = 1;
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        if request.url.contains("/v1/changes") && big > 0 {
            big -= 1;
            return Some(Ok(HttpResponse { status: 200, headers: Vec::new(), body: vec![b' '; request.max_answer as usize + 1].into() }));
        }
        None
    }));
    sim.send(start());
    // the shell stopped reading at the cap: no body reached the core, and the page was not asked again at once
    assert!(matches!(sim.app.alerts(), [Alert::BadAnswer { .. }]));
    let pages = |world: &World| requests(world).into_iter().filter(|r| r.starts_with("GET /v1/changes")).collect::<Vec<_>>();
    assert_eq!(pages(&world).len(), 0, "the fault answered in the hub's place");
    assert_ne!(position(&sim), last);
    let wait = fire(&mut sim);
    assert!(wait >= 750);
    let asked = pages(&world);
    assert!(asked[0].contains("limit=250"), "{asked:?}");
    assert_eq!(position(&sim), last);
}

/// The same stored state, the same answers, the same clock: the same effects, byte for byte.
#[test]
fn the_same_script_gives_the_same_effects() {
    type Http = (String, Result<HttpResponse, IoError>);
    let mut world = World::new();
    world.agent_says("a");
    world.agent_asks("b?");
    world.hub.borrow_mut().page_max = Some(1);
    let stored = { let s = world.me_store.0.borrow(); (s.0, s.1.clone()) };
    let t0 = common::now();

    // the first run, against the hub, recording what it answered
    let http: Rc<RefCell<Vec<Http>>> = Rc::default();
    let opens: Rc<RefCell<Vec<Vec<StreamItem>>>> = Rc::default();
    let seeded = || trommi_client_core::app::App::with_entropy(Box::new(trommi_core::crypto::SeededEntropy::new([9; 32])));
    let mut one = Sim::new(t0);
    one.app = seeded();
    one.device = stored.clone();
    let (hub, rec) = (world.hub.clone(), http.clone());
    one.hub = Some(Box::new(move |now, request| {
        let got = hub.borrow_mut().http(now, request);
        rec.borrow_mut().push((serde_json::to_string(request).unwrap(), got.clone()));
        got
    }));
    let (hub, rec) = (world.hub.clone(), opens.clone());
    one.stream_hub = Some(Box::new(move |now, request| {
        let got = hub.borrow_mut().open_stream(now, request);
        rec.borrow_mut().push(got.clone());
        got
    }));
    one.send(start());
    world.agent_says("c");
    let chunk = world.hub.borrow_mut().stream_news(&world.me).expect("news");
    one.stream(StreamItem::Chunk { bytes: chunk.clone().into() });
    one.stream(StreamItem::Failed { error: IoError::new(IoErrorKind::Offline) });
    let wait = fire(&mut one);
    assert_eq!(one.view.phase, Phase::Live);

    // the second run, against the record
    let mut two = Sim::new(t0);
    two.app = seeded();
    two.device = stored;
    let script: Rc<RefCell<VecDeque<Http>>> = Rc::new(RefCell::new(http.borrow().iter().cloned().collect()));
    two.hub = Some(Box::new(move |_, request| {
        let (asked, got) = script.borrow_mut().pop_front().expect("no request beyond the first run's");
        assert_eq!(serde_json::to_string(request).unwrap(), asked, "the same request, byte for byte");
        got
    }));
    let script: Rc<RefCell<VecDeque<Vec<StreamItem>>>> = Rc::new(RefCell::new(opens.borrow().iter().cloned().collect()));
    two.stream_hub = Some(Box::new(move |_, _| script.borrow_mut().pop_front().expect("no stream beyond the first run's")));
    two.send(start());
    two.stream(StreamItem::Chunk { bytes: chunk.into() });
    two.stream(StreamItem::Failed { error: IoError::new(IoErrorKind::Offline) });
    assert_eq!(fire(&mut two), wait, "the same jitter from the same seed");

    let log = |sim: &Sim| serde_json::to_string(&sim.log).unwrap();
    assert!(one.log.len() >= 15, "{}", one.log.len());
    assert_eq!(log(&one), log(&two));
    assert_eq!(one.device, two.device, "and the same stored state");
    let _ = OutboxKind::Envelope;
}

/// Answers the held requests that `which` picks, one after another, as the hub answers them. Returns how many.
fn serve(world: &World, sim: &mut Sim, which: impl Fn(&HttpRequest) -> bool) -> usize {
    serve_some(world, sim, usize::MAX, which)
}

fn serve_some(world: &World, sim: &mut Sim, most: usize, which: impl Fn(&HttpRequest) -> bool) -> usize {
    let mut served = 0;
    while let Some((id, request)) = sim.held.iter().find(|(_, r)| served < most && which(r)).cloned() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
        served += 1;
    }
    served
}

fn stack(sim: &Sim) -> Vec<String> {
    sim.app.board().model.stack.clone()
}

#[test]
fn the_desk_stands_complete_before_the_order_is_read_and_no_card_moves_while_it_is() {
    let mut world = World::new();
    let cards: Vec<_> = (0..6).map(|n| world.agent_card(&format!("card {n}"))).collect();
    // two cards are revised after they were asked: their newest version is younger than every other card
    world.agent_revises(cards[0].0, &cards[0].1, "card 0, said better", 2);
    let (again, _) = world.agent_revises(cards[1].0, &cards[1].1, "card 1, said better", 2);
    world.agent_revises(cards[1].0, &again, "card 1, said best", 3);
    for n in 0..20 {
        world.agent_says(&format!("line {n}"));
    }
    world.hub.borrow_mut().page_max = Some(7);
    let end = world.hub.borrow().change;
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve(&world, &mut sim, |r| r.url.contains("/rooms/"));
    // the Desk is asked for before the first page of the order
    let asked: Vec<String> = sim.log.iter().filter_map(|e| if let Effect::Http { request, .. } = e { Some(request.url.clone()) } else { None }).collect();
    let (desk, page) = (asked.iter().position(|u| u.contains("/v1/desk")).unwrap(), asked.iter().position(|u| u.contains("/v1/changes")).unwrap());
    assert!(desk < page, "{asked:?}");
    assert!(stack(&sim).is_empty(), "nothing stands before the hub's list");
    assert_eq!(serve(&world, &mut sim, |r| r.url.contains("/v1/desk")), 1);
    assert!(!sim.app.board().desk_complete, "the revised cards' first versions are still to come");
    assert_eq!(serve(&world, &mut sim, |r| r.url.contains("/v1/cards/")), 2, "one read per card revised since it was asked");
    assert!(sim.app.board().desk_complete);
    // complete, with not one page of the order read
    let first = stack(&sim);
    assert_eq!(first.len(), 6);
    assert_eq!(position(&sim), world.reopen_me(&sim).cursor());
    assert_ne!(sim.view.phase, Phase::Live);
    // in the order the cards were asked, whatever was revised since
    let ids: Vec<String> = cards.iter().map(|(id, ..)| trommi_client_core::engine::feed::hex(id.as_bytes())).collect();
    let places: Vec<usize> = ids.iter().map(|id| first.iter().position(|s| s == id).expect("on the Desk")).collect();
    assert!(places.windows(2).all(|w| w[0] < w[1]) || places.windows(2).all(|w| w[0] > w[1]), "by the time asked: {places:?}");
    // the order is read page by page: the Desk never changes
    let mut pages = 0;
    while serve_some(&world, &mut sim, 1, |r| r.url.contains("/v1/changes")) > 0 {
        pages += 1;
        assert_eq!(stack(&sim), first, "after page {pages}");
        serve(&world, &mut sim, |r| !r.url.contains("/v1/changes"));
    }
    assert!(pages >= 4, "{pages}");
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(position(&sim), end);
    assert_eq!(stack(&sim), first);
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn a_first_change_that_lies_costs_a_read_and_moves_nothing() {
    let mut world = World::new();
    let cards: Vec<_> = (0..3).map(|n| world.agent_card(&format!("card {n}"))).collect();
    world.agent_revises(cards[0].0, &cards[0].1, "again", 2);
    world.hub.borrow_mut().first_change_lie = Some(1);
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(sim.app.board().desk_complete);
    assert_eq!(stack(&sim).len(), 3);
    assert!(sim.app.alerts().is_empty());
}

#[test]
fn a_sign_in_with_a_thousand_open_cards_goes_live_with_the_whole_desk() {
    let mut world = World::new();
    for n in 0..1000 {
        world.agent_card(&format!("card {n}"));
    }
    let mut sim = world.sim();
    let began = std::time::Instant::now();
    sim.send(start());
    let took = began.elapsed();
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(sim.app.board().desk_complete);
    assert_eq!(stack(&sim).len(), 1000);
    let asked = requests(&world);
    let (desk, page) = (asked.iter().position(|u| u.contains("/v1/desk")).unwrap(), asked.iter().position(|u| u.contains("/v1/changes")).unwrap());
    assert!(desk < page);
    assert_eq!(asked.iter().filter(|u| u.contains("/v1/changes")).count(), 2, "1000 changes in pages of 500");
    eprintln!("a thousand cards: {took:?} of this machine's time, {} requests", asked.len());
}

#[test]
fn a_desk_one_above_the_hubs_limit_is_cut_short_and_the_order_brings_the_rest() {
    let mut world = World::new();
    for n in 0..=common::hub::DESK_MAX {
        world.agent_card(&format!("card {n}"));
    }
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve(&world, &mut sim, |r| r.url.contains("/rooms/"));
    assert_eq!(serve(&world, &mut sim, |r| r.url.contains("/v1/desk")), 1);
    // the hub's list is cut short: it decides nothing, but what it brought stands
    assert_eq!(stack(&sim).len(), common::hub::DESK_MAX);
    assert!(sim.app.board().desk_complete);
    while serve(&world, &mut sim, |_| true) > 0 {}
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(stack(&sim).len(), common::hub::DESK_MAX + 1, "the card the Desk left out came with the order");
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
#[ignore = "a measurement: where a thousand-card sign-in spends its time"]
fn where_the_time_goes() {
    let mut world = World::new();
    for n in 0..1000 {
        world.agent_card(&format!("card {n}"));
    }
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve(&world, &mut sim, |r| r.url.contains("/rooms/"));
    let t = std::time::Instant::now();
    let (id, request) = sim.held.iter().find(|(_, r)| r.url.contains("/v1/desk")).cloned().unwrap();
    let got = world.hub.borrow_mut().http(sim.now_ms, &request);
    eprintln!("the stand-in hub's Desk answer: {:?}", t.elapsed());
    let t = std::time::Instant::now();
    sim.answer(id, got);
    eprintln!("the Desk taken (1000 versions shown): {:?}", t.elapsed());
    for page in 0..2 {
        let (id, request) = sim.held.iter().find(|(_, r)| r.url.contains("/v1/changes")).cloned().unwrap();
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        let t = std::time::Instant::now();
        sim.answer(id, got);
        eprintln!("page {page} taken: {:?}", t.elapsed());
    }
}

#[test]
fn upkeep_stocks_key_packages_and_writes_heads_once_at_rest_then_every_ten_minutes() {
    let mut world = World::new();
    assert_eq!(world.hub.borrow().unused(&world.me), 99, "one was claimed for the session's founding");
    world.agent_says("something to have a head of");
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(world.hub.borrow().unused(&world.me), 100, "stocked up to the full number");
    // the device said which heads it holds (9.0.7), once
    assert_eq!(chain_at_hub(&world, &world.me).len(), 1);
    let rounds = |world: &World| requests(world).iter().filter(|r| r.starts_with("PUT /v1/key-packages")).count();
    assert_eq!(rounds(&world), 2, "the count, then the upload");
    // nothing more until the time is up, whatever happens meanwhile
    world.agent_says("more");
    world.stream(&mut sim);
    sim.advance(60_000);
    assert_eq!(rounds(&world), 2);
    sim.advance(trommi_client_core::engine::sync::UPKEEP_EVERY_MS);
    assert!(rounds(&world) >= 3, "{:?}", requests(&world));
    assert_eq!(chain_at_hub(&world, &world.me).len(), 2, "heads again: another sender's head moved");
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
}

use trommi_client_core::engine::sync::joins::SessionJoins;

/// Sends the App's stream what the hub's order holds beyond what it was sent, until nothing is new.
fn news(world: &World, sim: &mut Sim, device: &trommi_core::ids::DeviceId) {
    for _ in 0..16 {
        let Some(bytes) = world.hub.borrow_mut().stream_news(device) else { return };
        sim.stream(StreamItem::Chunk { bytes: bytes.into() });
    }
}

fn count(world: &World, what: &str) -> usize {
    requests(world).iter().filter(|r| r.starts_with(what)).count()
}

#[test]
fn a_sign_in_with_the_code_reads_its_sessions_in_one_request_and_posts_their_joins_in_one() {
    let mut world = World::new();
    let second = world.another_session();
    let third = world.another_session();
    world.agent_asks("Ship it?");
    let (store, id, keys) = world.newcomer();
    let mut sim = world.sim_of(&store);
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let groups = vec![world.group, second, third];
    let held = |sim: &Sim| groups.iter().filter(|g| sim.app.engine().unwrap().device().group(g).is_ok()).count();
    assert_eq!(held(&sim), 0, "in the room, in no session yet");
    assert!(stack(&sim).is_empty());
    let unused = world.hub.borrow().unused(&id);

    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: groups.clone() });
    sim.run(effects);
    news(&world, &mut sim, &id);
    assert_eq!(count(&world, "POST /v1/groups/read"), 1, "{:?}", requests(&world));
    assert_eq!(count(&world, "POST /v1/commits"), 1, "the joins together: {:?}", requests(&world));
    assert_eq!(count(&world, "POST /v1/groups/"), 1, "no join posted alone, no group read alone: {:?}", requests(&world));
    assert_eq!(count(&world, "GET /v1/groups/"), 0, "{:?}", requests(&world));
    assert_eq!(held(&sim), 3);
    assert!(!sim.app.engine().unwrap().joining_sessions());
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    // durable, and the session's open card stands on the Desk
    let again = world.reopen_me(&sim);
    assert!(groups.iter().all(|g| again.group(g).is_ok()));
    assert!(again.outbox().iter().all(|e| e.kind != OutboxKind::ExternalCommit));
    assert_eq!(stack(&sim).len(), 1, "the joined session's card");
    // its KeyPackages went up only after the joins
    assert!(world.hub.borrow().unused(&id) >= unused);
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn a_group_that_did_not_fit_is_read_alone_and_one_the_hub_hides_is_not_joined() {
    let mut world = World::new();
    let second = world.another_session();
    let third = world.another_session();
    let (store, id, keys) = world.newcomer();
    {
        let mut hub = world.hub.borrow_mut();
        hub.read_fits = Some(1);
        hub.log_max = Some(1);
        hub.unseen.insert(third);
    }
    let mut sim = world.sim_of(&store);
    sim.send(start());
    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: vec![third, world.group, second] });
    sim.run(effects);
    news(&world, &mut sim, &id);
    let device = sim.app.engine().unwrap().device();
    assert!(device.group(&world.group).is_ok() && device.group(&second).is_ok(), "{:?}", requests(&world));
    assert!(device.group(&third).is_err());
    assert_eq!(sim.app.sessions_not_joined(), Some(&[(third, "not-found")][..]));
    assert_eq!(count(&world, "POST /v1/groups/read"), 1);
    // each of the two that did not fit: its founding, its current GroupInfo, its log
    let infos = requests(&world).iter().filter(|r| r.contains("/info")).count();
    assert_eq!(infos, 4, "{:?}", requests(&world));
    assert!(requests(&world).iter().any(|r| r.contains("/log?after=0&") && r.contains("kind=commit")), "{:?}", requests(&world));
    // each join is posted as soon as it is built: the pump does not wait for the reading of the next group
    assert_eq!(requests(&world).iter().filter(|r| r.starts_with("POST /v1/groups/") && r.ends_with("/commits")).count(), 2, "{:?}", requests(&world));
}

#[test]
fn a_read_that_fails_is_asked_again_after_a_wait_and_a_restart_posts_the_joins_already_built() {
    let mut world = World::new();
    let second = world.another_session();
    let (store, id, keys) = world.newcomer();
    let mut sim = world.sim_of(&store);
    sim.send(start());
    let mut script: VecDeque<Result<HttpResponse, IoError>> = [offline(), Ok(later(503, 7))].into_iter().collect();
    let down = Rc::new(RefCell::new(true));
    let is_down = down.clone();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        if request.url.ends_with("/v1/groups/read") {
            return script.pop_front();
        }
        if request.url.ends_with("/v1/commits") && *is_down.borrow() {
            return Some(offline());
        }
        None
    }));
    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: vec![world.group, second] });
    sim.run(effects);
    assert_eq!(count(&world, "POST /v1/groups/read"), 0);
    fire(&mut sim);
    assert!(fire(&mut sim) >= 7_000, "not sooner than Retry-After");
    assert_eq!(count(&world, "POST /v1/groups/read"), 1);
    // the joins are built and stored; the hub is not reached; the process dies
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 2);
    sim.restart();
    *down.borrow_mut() = false;
    sim.send(start());
    news(&world, &mut sim, &id);
    assert_eq!(count(&world, "POST /v1/commits"), 1, "{:?}", requests(&world));
    let device = sim.app.engine().unwrap().device();
    assert!(device.group(&world.group).is_ok() && device.group(&second).is_ok());
}

/// What the order assertion of the Desk test is worth: without the first versions the revised cards stand where
/// their newest version puts them, which that assertion refuses; the order's own reading then puts them right.
#[test]
fn without_the_first_versions_the_revised_cards_stand_out_of_place_until_the_order_brings_them() {
    let mut world = World::new();
    let cards: Vec<_> = (0..6).map(|n| world.agent_card(&format!("card {n}"))).collect();
    world.agent_revises(cards[0].0, &cards[0].1, "card 0, said better", 2);
    world.agent_revises(cards[1].0, &cards[1].1, "card 1, said better", 2);
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve(&world, &mut sim, |r| r.url.contains("/rooms/"));
    serve(&world, &mut sim, |r| r.url.contains("/v1/desk"));
    // the hub refuses the reads of the first versions
    let refused: Vec<_> = sim.held.iter().filter(|(_, r)| r.url.contains("/v1/cards/")).cloned().collect();
    assert_eq!(refused.len(), 2);
    for (id, _) in refused {
        sim.answer(id, Ok(answer(404, json!({ "error": "not-found", "message": "refused" }))));
    }
    assert!(sim.app.board().desk_complete, "a refusal ends the read: the order brings the version");
    let ids: Vec<String> = cards.iter().map(|(id, ..)| trommi_client_core::engine::feed::hex(id.as_bytes())).collect();
    let places = |sim: &Sim| -> Vec<usize> { ids.iter().map(|id| stack(sim).iter().position(|s| s == id).expect("on the Desk")).collect() };
    let in_order = |p: &[usize]| p.windows(2).all(|w| w[0] < w[1]) || p.windows(2).all(|w| w[0] > w[1]);
    assert!(!in_order(&places(&sim)), "the revised cards stand by their newest version: {:?}", places(&sim));
    sim.hold = false;
    while serve(&world, &mut sim, |_| true) > 0 {}
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(in_order(&places(&sim)), "by the time asked once the order was read: {:?}", places(&sim));
}

#[test]
#[ignore = "a finding, not yet so: the core reads a register out of the order without saying whether it is current, so the model takes a session's or a device's register only when the order reaches it"]
fn a_register_of_the_desk_stands_on_the_board_before_the_order_is_read() {
    let mut world = World::new();
    world.agent_register("profile", r#"{"name":"Builder"}"#);
    world.agent_says("a line after it");
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve(&world, &mut sim, |r| r.url.contains("/rooms/"));
    assert_eq!(serve(&world, &mut sim, |r| r.url.contains("/v1/desk")), 1);
    let session = trommi_client_core::engine::feed::hex(world.session.as_bytes());
    let held = sim.app.board().model.sessions.get(&session).is_some_and(|s| s.registers.contains_key("profile"));
    assert!(held, "the Desk brought the session's profile");
}

#[test]
fn a_desk_cut_by_the_hubs_byte_budget_is_cut_short_and_the_order_brings_the_rest() {
    let mut world = World::new();
    for n in 0..6 {
        world.agent_card(&format!("card {n}"));
    }
    world.agent_register("profile", r#"{"name":"Builder"}"#);
    // the budget holds three versions: the fourth and the register are left out
    let one = world.hub.borrow().order.iter().find_map(|i| match i {
        common::hub::Item::Envelope { .. } => i.served()["envelope"].as_str().map(str::len),
        _ => None,
    });
    world.hub.borrow_mut().desk_bytes_max = one.expect("an envelope") * 3 + 10;
    let mut sim = world.sim();
    sim.hold = true;
    sim.send(start());
    serve(&world, &mut sim, |r| r.url.contains("/rooms/"));
    assert_eq!(serve(&world, &mut sim, |r| r.url.contains("/v1/desk")), 1);
    assert!((1..6).contains(&stack(&sim).len()), "cut short: {:?}", stack(&sim));
    assert!(sim.app.board().desk_complete, "a list cut short decides nothing, what it brought stands");
    while serve(&world, &mut sim, |_| true) > 0 {}
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(stack(&sim).len(), 6, "the order brought the rest");
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn the_desk_of_a_start_leaves_out_an_archived_sessions_cards() {
    let mut world = World::new();
    let other = world.another_session();
    world.agent_asks("in the live session");
    // a card in the other session, which is archived while the App is away
    {
        let agent = world.others.last_mut().expect("its agent");
        let hub = &mut *world.hub.borrow_mut();
        common::sync(hub, agent);
        let session = other.session_id().expect("a session");
        let payload = r#"{"card_type":"decision","title":"in the archived one","options":[{"key":"yes"},{"key":"no"}],"object_version":1,"previous_version_hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}"#;
        let draft = trommi_core::device::Draft::CardFirst { session, urgency: trommi_core::envelope::Urgency::Normal, push: false, payload: common::text(payload) };
        agent.seal(&draft, None, &[], common::now()).expect("the card seals");
        assert!(common::post_all(hub, agent).is_empty());
        hub.archive(&other);
    }
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(requests(&world).iter().any(|r| r == "GET /v1/desk?active=1"));
    assert_eq!(stack(&sim).len(), 1, "the live session's card alone: {:?}", stack(&sim));
    assert!(sim.app.engine().unwrap().device().group(&other).unwrap().archived);
}
