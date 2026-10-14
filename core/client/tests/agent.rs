//! An agent device's engine (`engine/agent.rs`) against the stand-in hub: the lease and `Trommi-Lease`, the
//! rollback guard, the command gate with its record across a restart, and leaving the room (followed in the order,
//! and read from the hub's removal route when the hub says so first).

mod common;

use common::{now, World};
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::Effect;
use trommi_client_core::app::sim::AgentSim;
use trommi_client_core::engine::{Report, Stopped, LEASE_EVERY_MS};
use trommi_client_core::hub::LinkReport;

/// Sends the agent's open stream what the hub's order holds beyond what it was sent.
fn stream(world: &World, sim: &mut AgentSim) -> bool {
    let news = world.hub.borrow_mut().stream_news(&world.agent.id());
    match news {
        Some(bytes) => sim.stream(StreamItem::Chunk { bytes: bytes.into() }),
        None => false,
    }
}

fn seen(world: &World) -> Vec<String> {
    world.hub.borrow().seen.clone()
}

fn stopped(sim: &mut AgentSim) -> Option<Stopped> {
    sim.engine().stopped()
}

/// The requests the engine sent with a token, with the lease each carried.
fn leases(sim: &AgentSim) -> Vec<(String, String, Option<String>)> {
    sim.io
        .log
        .iter()
        .filter_map(|e| match e {
            Effect::Http { request, .. } | Effect::OpenStream { request, .. } if request.headers.iter().any(|h| h.name == "authorization") => Some((
                request.method.clone(),
                request.url.clone(),
                request.headers.iter().find(|h| h.name == "trommi-lease").map(|h| h.value.clone()),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn the_lease_comes_first_and_every_write_and_the_stream_carry_it() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let seen = seen(&world);
    assert!(seen[0].ends_with("/challenge") && seen[1].ends_with("/tokens"), "{seen:?}");
    assert_eq!(seen[2], "POST /v1/link", "the lease before anything else: {seen:?}");
    let me = world.agent.id().to_base64url();
    assert!(seen[3].starts_with("GET /v1/groups/") && seen[3].contains(&format!("/chains/{me}?after=")), "the guard next: {seen:?}");
    assert!(seen.iter().all(|s| !s.starts_with("GET /v1/desk")), "no Desk first for an agent: {seen:?}");
    assert!(seen.iter().any(|s| s.starts_with("GET /v1/changes")), "{seen:?}");
    assert!(seen.iter().any(|s| s.starts_with("STREAM /v1/stream")), "{seen:?}");
    assert_eq!(sim.engine().lease(), Some(1));
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Lease(Some(1)))));
    assert_eq!(sim.engine().main_group(), Some(world.group), "its main session");
    let (process, generation) = world.hub.borrow().leases[&world.agent.id()].clone();
    assert_eq!(generation, 1);
    let body = world.hub.borrow().links[0].1.clone();
    assert!(body.get("generation").is_none() && body["process"].as_str().is_some());
    for (method, url, lease) in leases(&sim) {
        let write = method != "GET" && !url.ends_with("/v1/link") || url.contains("/v1/stream");
        assert_eq!(lease.is_some(), write, "{method} {url}");
        if let Some(lease) = lease {
            assert_eq!(lease, "1");
        }
    }
    // renewed every 20 s by the same process, under its generation
    let links = world.hub.borrow().links.len();
    sim.advance(LEASE_EVERY_MS);
    let hub = world.hub.borrow();
    assert_eq!(hub.links.len(), links + 1);
    assert_eq!(hub.links.last().unwrap().1["generation"].as_u64(), Some(1));
    assert_eq!(hub.leases[&world.agent.id()], (process, 1));
}

#[test]
fn what_the_process_reports_goes_at_once_and_leaving_gives_the_lease_up() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let report = LinkReport { hears: true, working: true, last_call_at: 1234 };
    sim.with(|engine, ctx| engine.link_report(ctx, report));
    let last = world.hub.borrow().links.last().unwrap().1.clone();
    assert_eq!((last["hears"].as_bool(), last["working"].as_bool(), last["last_call_at"].as_u64()), (Some(true), Some(true), Some(1234)));
    sim.with(|engine, ctx| engine.leave(ctx));
    let last = world.hub.borrow().links.last().unwrap().1.clone();
    assert_eq!((last["leaving"].as_bool(), last["generation"].as_u64(), last["hears"].as_bool()), (Some(true), Some(1), Some(false)));
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Lease(None))));
    let before = seen(&world).len();
    sim.advance(5 * LEASE_EVERY_MS);
    assert_eq!(seen(&world).len(), before, "nothing more after the last word");
}

#[test]
fn a_newer_process_takes_the_lease_and_this_one_stops() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    assert_eq!(sim.engine().lease(), Some(1));
    // another process on the same state acquires it
    world.hub.borrow_mut().leases.get_mut(&world.agent.id()).unwrap().1 = 2;
    // the next renewal is refused, and nothing is signed any more
    sim.advance(LEASE_EVERY_MS);
    assert_eq!(stopped(&mut sim), Some(Stopped::LeaseLost));
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Stopped(Stopped::LeaseLost))));
    world.founder_says("still there?");
    let before = seen(&world).len();
    sim.advance(5 * LEASE_EVERY_MS);
    assert_eq!(seen(&world).len(), before);
}

#[test]
fn a_write_refused_lease_lost_is_checked_by_renewing() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    world.hub.borrow_mut().leases.get_mut(&world.agent.id()).unwrap().1 = 7;
    // a write of this process (its stream, here) is refused; the renewal says the lease is gone
    sim.stream(StreamItem::Ended);
    for _ in 0..10 {
        if stopped(&mut sim).is_some() {
            break;
        }
        sim.advance(2_000);
    }
    assert_eq!(stopped(&mut sim), Some(Stopped::LeaseLost));
}

#[test]
fn a_command_is_reported_once_kept_until_done_and_uncertain_after_a_restart() {
    let mut world = World::new();
    let (card, _, _) = world.agent_card("Ship it?");
    let mut sim = world.agent_sim();
    sim.start();
    assert!(sim.commands().is_empty(), "nothing waits");
    world.founder_says("hello agent");
    world.founder_answers(card, "yes");
    stream(&world, &mut sim);
    let commands = sim.commands();
    assert_eq!(commands.iter().map(|c| c.command.as_str()).collect::<Vec<_>>(), ["message", "answer"], "{commands:?}");
    let session = trommi_client_core::engine::feed::hex(world.session.as_bytes());
    assert_eq!(commands[0].session_id.as_deref(), Some(session.as_str()));
    assert_eq!(commands[0].timeline_key.as_deref(), Some(format!("chat:session/{session}").as_str()));
    assert_eq!(commands[0].content()["text"], "hello agent");
    assert_eq!(commands[1].choices, ["yes"]);
    assert_eq!(commands[1].object_id.as_deref(), Some(trommi_client_core::engine::feed::hex(card.as_bytes()).as_str()));
    assert!(commands[0].envelope_number < commands[1].envelope_number && !commands[0].uncertain);
    assert_eq!(sim.engine().commands_kept().len(), 2);
    // the message was handed over; the answer was not when the process died
    let done = commands[0].envelope_hash.clone();
    sim.with(|engine, ctx| engine.command_done(ctx, &done)).expect("done");
    assert_eq!(sim.engine().commands_kept().len(), 1);
    sim.restart();
    sim.reports.clear();
    sim.start();
    let again = sim.commands();
    assert_eq!(again.len(), 1, "{again:?}");
    assert!(again[0].uncertain && again[0].command == "answer" && again[0].envelope_hash == commands[1].envelope_hash);
    // not let through a second time, whatever the order brings again
    let answer = again[0].envelope_hash.clone();
    sim.with(|engine, ctx| engine.command_done(ctx, &answer)).expect("done");
    sim.restart();
    sim.reports.clear();
    sim.start();
    stream(&world, &mut sim);
    assert!(sim.commands().is_empty(), "{:?}", sim.commands());
}

#[test]
fn a_state_older_than_what_the_device_sent_signs_nothing() {
    let mut world = World::new();
    // a copy of the state, then the device writes on (the copy is what a restored backup would be)
    let old = world.agent_sim();
    world.agent_says("written after the copy");
    let mut sim = old;
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::RolledBack));
    let seen = seen(&world);
    assert!(seen.iter().all(|s| !s.starts_with("GET /v1/changes") && !s.starts_with("STREAM")), "nothing read past the guard: {seen:?}");
    assert!(world.hub.borrow().links.len() == 1);
}

#[test]
fn an_own_envelope_the_order_brings_beyond_the_state_halts_too() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    assert_eq!(stopped(&mut sim), None);
    // a second writer of this device (a copy that runs on): its envelope comes in the order
    world.agent_says("from another copy");
    stream(&world, &mut sim);
    assert_eq!(stopped(&mut sim), Some(Stopped::RolledBack));
}

#[test]
fn a_room_commit_that_takes_the_agent_out_stops_it() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let agent = world.agent.id();
    {
        let mut hub = world.hub.borrow_mut();
        common::sync(&hub, &mut world.founder);
        world.founder.remove_agents(&[agent], now()).expect("the Commit is built");
        assert!(common::post_all(&mut hub, &mut world.founder).is_empty());
    }
    stream(&world, &mut sim);
    assert_eq!(stopped(&mut sim), Some(Stopped::Removed));
}

#[test]
fn the_hubs_word_is_checked_against_the_commits_it_still_serves() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    let agent = world.agent.id();
    {
        let mut hub = world.hub.borrow_mut();
        common::sync(&hub, &mut world.founder);
        world.founder.remove_agents(&[agent], now()).expect("the Commit is built");
        assert!(common::post_all(&mut hub, &mut world.founder).is_empty());
        let n = hub.log_n(&world.room);
        hub.removed_keys.entry(agent).or_default().insert(world.room, n);
    }
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::Removed));
    let seen = seen(&world);
    assert!(seen.iter().any(|s| s.contains("/removal?after=0")), "{seen:?}");
    assert!(seen.iter().all(|s| s != "POST /v1/link"), "{seen:?}");
}

#[test]
fn the_hubs_word_alone_keeps_the_state() {
    let world = World::new();
    let mut sim = world.agent_sim();
    let agent = world.agent.id();
    // the hub says this key is out, and serves no Commit that removed it
    world.hub.borrow_mut().removed_keys.insert(agent, Default::default());
    let revision = sim.io.device.0;
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::NotMember));
    assert_eq!(sim.io.device.0, revision, "nothing of the device was written");
}

#[test]
fn a_human_device_does_not_become_an_agent() {
    let world = World::new();
    let io = world.sim();
    let entries = io.device.1.iter().map(|(k, v)| trommi_client_core::app::io::DeviceEntry { key: k.clone().into(), value: v.clone().into() }).collect();
    let mut engine = trommi_client_core::engine::Engine::open(common::hub::HUB_URL, "test/0.0.0", 7, io.device.0, entries).expect("it opens");
    assert!(engine.become_agent().is_err());
    assert!(!engine.is_agent() && engine.lease().is_none());
}
