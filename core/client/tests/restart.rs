//! Restart safety: the App is killed before every effect of a run in turn, started again over what was stored, and
//! must end where the uninterrupted run ends.

mod common;

use common::{now, post_all, start, sync, text, World};
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::engine::sync::joins::SessionJoins;
use trommi_client_core::views::Phase;
use trommi_core::crypto::SystemEntropy;
use trommi_core::device::{Device, Draft};
use trommi_core::ids::{DeviceId, GroupId};

fn news(world: &World, sim: &mut Sim, device: &DeviceId) {
    for _ in 0..16 {
        let Some(bytes) = world.hub.borrow_mut().stream_news(device) else { return };
        sim.stream(StreamItem::Chunk { bytes: bytes.into() });
    }
}

/// Where a run ended, in what does not depend on the room's random keys.
#[derive(Debug, PartialEq, Eq)]
struct End {
    phase: Phase,
    /// The engine stands at the hub's newest change.
    at_head: bool,
    /// Nothing waits to be posted, nothing passed over is still owed.
    outbox: usize,
    owes: (usize, usize, usize, usize),
    /// The session groups this device holds, each at the hub's epoch, and how many leaves each has.
    groups: Vec<(bool, usize)>,
    /// Open cards on the Desk.
    desk: usize,
    stored_as_held: bool,
}

fn end(world: &World, sim: &Sim) -> End {
    let engine = sim.app.engine().expect("an engine");
    let device = engine.device();
    let hub = world.hub.borrow();
    let mut groups: Vec<(bool, usize)> = hub
        .session_groups()
        .iter()
        .filter_map(|g| device.group(g).ok().map(|held| (hub.observer(g).and_then(|o| o.epoch().ok()) == Some(held.epoch) && !held.pending && !held.is_stale(), held.leaves.len())))
        .collect();
    groups.sort();
    let stored = world.reopen_me(sim);
    End {
        phase: sim.view.phase,
        at_head: engine.position() == hub.change,
        outbox: device.outbox().len(),
        owes: engine.owes(),
        groups,
        desk: sim.app.board().model.stack.len(),
        stored_as_held: stored.cursor() == device.cursor() && stored.outbox().len() == device.outbox().len(),
    }
}

/// Runs the App to rest; killed before its `die_in`th effect, it is started again once.
fn run(world: &World, store: &common::TestStore, me: &DeviceId, die_in: Option<usize>, after_start: &dyn Fn(&mut Sim)) -> (Sim, bool) {
    let mut sim = world.sim_of(store);
    sim.die_in = die_in;
    sim.send(start());
    after_start(&mut sim);
    let mut died = false;
    for _ in 0..24 {
        if sim.dead {
            died = true;
            sim.restart();
            sim.send(start());
        }
        news(world, &mut sim, me);
        sim.advance(5_000);
    }
    (sim, died)
}

/// A room with something of everything the engine mends: lines and cards, a card revised, a session this device
/// joins late by a Welcome with lines in it, a change the hub's pages skip, and a human device a session lacks.
fn scene() -> World {
    let mut world = World::new();
    world.agent_says("one");
    let (card, hash, _) = world.agent_card("Ship it?");
    world.agent_revises(card, &hash, "Ship it now?", 2);
    let skipped = world.agent_says("a line the pages skip");
    world.hub.borrow_mut().hidden.insert(skipped);
    world.agent_asks("And this?");
    // a second session, founded after this device's stored state, with a line of its agent
    let second = world.another_session();
    {
        let hub = &mut *world.hub.borrow_mut();
        let session = hub.observer(&second).unwrap().session().unwrap().session_id;
        let agent = world.others.last_mut().unwrap();
        sync(hub, agent);
        agent.seal(&Draft::SessionChat { session, payload: text(r#"{"content_type":"message","text":"in the second"}"#) }, None, &[], now()).expect("seals");
        assert!(post_all(hub, agent).is_empty());
    }
    // a new human device of the person, in no session, with KeyPackages at the hub: this device adds it
    let (store, _, _) = world.newcomer();
    let mut newcomer = Device::open(store, Box::new(SystemEntropy)).expect("opens");
    newcomer.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut newcomer).is_empty());
    world.agent_says("last");
    world
}

#[test]
fn killed_before_any_effect_the_app_ends_where_an_uninterrupted_run_ends() {
    let world = scene();
    let (sim, died) = run(&world, &world.me_store, &world.me, None, &|_| {});
    assert!(!died);
    let whole = end(&world, &sim);
    let effects = sim.log.len();
    assert_eq!(whole.phase, Phase::Live);
    assert!(whole.at_head && whole.outbox == 0 && whole.owes == (0, 0, 0, 0) && whole.stored_as_held, "{whole:?} {:?}", sim.app.alerts());
    assert_eq!(whole.groups, vec![(true, 4), (true, 4)], "both sessions, the newcomer in each: {:?}", world.hub.borrow().refused);
    assert_eq!(whole.desk, 2);
    assert!(sim.cache.keys().any(|k| k.starts_with("engine/")), "the engine kept its record");
    // every point up to the end of what the uninterrupted run did
    let mut kills = 0;
    for at in 1..effects {
        let world = scene();
        let (sim, died) = run(&world, &world.me_store, &world.me, Some(at), &|_| {});
        kills += usize::from(died);
        assert_eq!(end(&world, &sim), whole, "killed before effect {at} of {effects}: {:?} {:?}", sim.app.alerts(), world.hub.borrow().refused);
    }
    assert!(kills > 40, "the kills fell inside the run: {kills} of {effects}");
}

#[test]
fn a_record_of_another_stored_state_is_left_and_one_that_lags_costs_a_second_reading() {
    let world = scene();
    let (sim, _) = run(&world, &world.me_store, &world.me, None, &|_| {});
    let whole = end(&world, &sim);
    let key = sim.cache.keys().find(|k| k.starts_with("engine/")).expect("a record").clone();
    let record = sim.cache[&key].clone();
    // the record of the end over the device's state of the start (a store put back from a copy): not taken
    let world = scene();
    let mut again = world.sim();
    again.cache.insert(key.clone(), record.clone());
    again.send(start());
    for _ in 0..24 {
        news(&world, &mut again, &world.me);
        again.advance(5_000);
    }
    assert_eq!(end(&world, &again).owes, whole.owes);
    assert!(end(&world, &again).at_head);
    assert_eq!(end(&world, &again).groups, whole.groups, "what the device had not taken was read, not skipped");
    // one that is no record
    let world = scene();
    let mut again = world.sim();
    again.cache.insert(key, b"{\"v\":1,".to_vec());
    again.send(start());
    for _ in 0..24 {
        news(&world, &mut again, &world.me);
        again.advance(5_000);
    }
    assert_eq!(end(&world, &again).groups, whole.groups);
}

/// A sign-in with the code: three sessions to join.
fn sign_in() -> (World, common::TestStore, DeviceId, Vec<GroupId>) {
    let mut world = World::new();
    let second = world.another_session();
    let third = world.another_session();
    world.agent_asks("Ship it?");
    let (store, id, _) = world.newcomer();
    let groups = vec![world.group, second, third];
    (world, store, id, groups)
}

fn keys() -> trommi_core::recovery::RecoveryKeys {
    trommi_core::recovery::RecoveryKeys::from_code(trommi_core::crypto::Secret::new([0xC0; 32])).expect("the keys of the code")
}

#[test]
fn killed_in_the_middle_of_a_sign_ins_joins_the_app_says_which_are_left_and_the_code_finishes_them() {
    let join = |groups: &[GroupId]| {
        let groups = groups.to_vec();
        move |sim: &mut Sim| {
            let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys: keys(), later: None, groups: groups.clone() });
            sim.run(effects);
        }
    };
    let (world, store, id, groups) = sign_in();
    let before = {
        let mut sim = world.sim_of(&store);
        sim.send(start());
        sim.log.len()
    };
    let (world, store, id, groups, effects) = {
        let _ = (world, store, id, groups);
        let (world, store, id, groups) = sign_in();
        let (sim, _) = run(&world, &store, &id, None, &join(&groups));
        assert!(groups.iter().all(|g| sim.app.engine().unwrap().device().group(g).is_ok()));
        assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
        let effects = sim.log.len();
        (world, store, id, groups, effects)
    };
    let _ = (world, store, id, groups);
    let (mut asked_again, mut kills, mut unknown) = (0, 0, 0);
    for at in before..effects {
        let (world, store, id, groups) = sign_in();
        let (mut sim, died) = run(&world, &store, &id, Some(at), &join(&groups));
        kills += usize::from(died);
        // what the engine could not finish without the code it says, and the code finishes it
        let left: Vec<GroupId> = sim.app.sessions_not_joined().map(|left| left.iter().map(|(group, _)| *group).collect()).unwrap_or_default();
        let held = |sim: &Sim, g: &GroupId| sim.app.engine().unwrap().device().group(g).is_ok();
        if died && !groups.iter().all(|g| held(&sim, g)) {
            assert!(!sim.app.engine().unwrap().joining_sessions(), "killed before effect {at}: the joins came to an end");
            // (killed before the joins were kept at all, the App knows of none: the sign-in that was cut asks again
            //  for every session the device is not in)
            let known = sim.app.sessions_not_joined().is_some();
            unknown += usize::from(!known);
            let left: Vec<GroupId> = if known { left } else { groups.iter().filter(|g| !held(&sim, g)).copied().collect() };
            for group in &groups {
                assert!(held(&sim, group) || left.contains(group), "killed before effect {at}: a session neither joined nor named as left: {:?} outbox {:?} refused {:?} record {:?}", sim.app.sessions_not_joined().map(|l| l.len()), sim.app.engine().unwrap().device().outbox().iter().map(|e| e.kind).collect::<Vec<_>>(), world.hub.borrow().refused, sim.cache.iter().find(|(k, _)| k.starts_with("engine/")).map(|(_, v)| String::from_utf8_lossy(v).chars().take(600).collect::<String>()));
            }
            asked_again += usize::from(known);
            join(&left)(&mut sim);
            for _ in 0..8 {
                news(&world, &mut sim, &id);
                sim.advance(5_000);
            }
        }
        assert!(groups.iter().all(|g| held(&sim, g)), "killed before effect {at} of {effects}: {:?} {:?}", sim.app.sessions_not_joined(), world.hub.borrow().refused);
        let device = sim.app.engine().unwrap().device();
        assert!(device.outbox().is_empty() && sim.app.engine().unwrap().position() == world.hub.borrow().change, "killed before effect {at}");
        assert_eq!(sim.app.board().model.stack.len(), 1, "killed before effect {at}: the joined session's card");
    }
    assert!(kills > 5 && asked_again > 0 && unknown <= 3, "{kills} kills, {asked_again} named as left, {unknown} not known, effects {before}..{effects}");
}
