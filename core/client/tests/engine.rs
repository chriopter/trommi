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
    assert!(seen[2].starts_with(&format!("GET /v1/changes?after={before}")), "{seen:?}");
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
        answer(409, json!({ "error": "forbidden", "message": "refused", "voided": true })),
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
    assert_eq!(posts.borrow().len(), 3);
    assert_eq!(posts.borrow()[0], first, "the same bytes after the restart");
    assert_eq!(chain_at_hub(&world, &world.me), vec![1, 2, 3]);
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
    let mut one = Sim::new(t0);
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
