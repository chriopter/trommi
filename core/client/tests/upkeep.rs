//! Tending the groups against the stand-in hub (tests/common): a human device a session lacks, a stale session,
//! the own leaf, an epoch that is full.

mod common;

use common::hub::answer;
use common::{now, post_all, start, sync, Peer, TestStore, World};
use serde_json::json;
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::engine::sync::UPKEEP_EVERY_MS;
use trommi_client_core::views::Phase;
use trommi_core::crypto::SystemEntropy;
use trommi_core::device::{Device, Draft};
use trommi_core::ids::{DeviceId, GroupId};

/// Sends the App's stream what the hub's order holds beyond what it was sent, until nothing is new.
fn news(world: &World, sim: &mut Sim) {
    for _ in 0..16 {
        let Some(bytes) = world.hub.borrow_mut().stream_news(&world.me) else { return };
        sim.stream(StreamItem::Chunk { bytes: bytes.into() });
    }
}

fn count(world: &World, what: &str) -> usize {
    world.hub.borrow().seen.iter().filter(|r| r.starts_with(what)).count()
}

/// The Commits the hub took in `group` from `from`.
fn commits(world: &World, group: &GroupId, from: &DeviceId) -> usize {
    world.hub.borrow().order.iter().filter(|i| matches!(i, common::hub::Item::Log { group: g, commit: true, from: f, .. } if g == group && f == from)).count()
}

fn open(store: &TestStore) -> Peer {
    Device::open(store.clone(), Box::new(SystemEntropy)).expect("the stored device opens")
}

const CLAIM: &str = "POST /v1/key-packages/claim";

#[test]
fn a_human_device_a_live_session_lacks_is_added_once_it_has_a_key_package() {
    let mut world = World::new();
    // a new human device of the person: in the room group, in no session, and with no KeyPackage at the hub yet
    let (store, newcomer, _) = world.newcomer();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    // the App's device saw the room Commit that let it in, and the session without it: nothing to claim yet
    assert_eq!(count(&world, CLAIM), 1, "{:?}", world.hub.borrow().seen);
    assert_eq!(commits(&world, &world.group, &world.me), 0);
    assert!(sim.timer.is_some(), "the next look waits for the clock");
    // asked again after a wait, not in a loop
    sim.advance(500);
    assert_eq!(count(&world, CLAIM), 1);
    // the newcomer stocks its KeyPackages; the next look claims one and adds it
    let mut device = open(&store);
    device.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut device).is_empty());
    sim.advance(2_000);
    assert_eq!(count(&world, CLAIM), 2, "{:?}", world.hub.borrow().seen);
    assert_eq!(commits(&world, &world.group, &world.me), 1, "one Commit adds it");
    assert!(world.hub.borrow().welcomes.iter().any(|w| w.device == newcomer && w.group == world.group), "its Welcome waits at the hub");
    // the Commit comes back in the order: the group holds the newcomer, and nothing more is claimed or committed
    news(&world, &mut sim);
    let me = sim.app.engine().unwrap().device();
    assert!(me.group(&world.group).unwrap().leaves.contains(&newcomer));
    assert!(me.outbox().is_empty());
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    assert_eq!(count(&world, CLAIM), 2, "{:?}", world.hub.borrow().seen);
    assert_eq!(commits(&world, &world.group, &world.me), 1);
    // and the newcomer joins by that Welcome
    sync(&world.hub.borrow(), &mut device);
    assert!(device.group(&world.group).is_ok());
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn a_stale_session_is_cleaned_by_whoever_sees_it() {
    let mut world = World::new();
    world.agent_says("a line, so the Cut names something");
    let agent = world.agent.id();
    // the person's other device takes the agent out of the room and goes away before it cleaned the session
    world.peers_sync();
    world.founder.remove_agents(&[agent], now()).expect("the removal is built");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(commits(&world, &world.group, &world.me), 1, "the Commit that removes the agent's leaf: {:?}", world.hub.borrow().seen);
    assert_eq!(count(&world, CLAIM), 0, "nobody is added");
    news(&world, &mut sim);
    let me = sim.app.engine().unwrap().device();
    let group = me.group(&world.group).unwrap();
    assert!(!group.is_stale() && !group.leaves.contains(&agent) && !group.pending, "{group:?}");
    assert!(me.outbox().is_empty());
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
    // the other human device takes it, and writes there again
    world.peers_sync();
    assert!(!world.founder.group(&world.group).unwrap().is_stale());
    world.founder_says("back");
}

#[test]
fn a_repair_the_hub_refuses_is_built_again_only_after_a_wait() {
    let mut world = World::new();
    let agent = world.agent.id();
    world.peers_sync();
    world.founder.remove_agents(&[agent], now()).expect("the removal is built");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    // a hub that takes no Commit in the session
    let path = format!("/v1/groups/{}/commits", trommi_core::ids::base64url_encode(world.group.as_bytes()));
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request| request.url.ends_with(&path).then(|| Ok(answer(trommi_client_core::hub::status_of("bad-commit").unwrap_or(409), json!({ "error": "bad-commit", "message": "refused" }))))));
    let mut sim = world.sim();
    sim.send(start());
    let posts = |sim: &Sim| sim.log.iter().filter(|e| matches!(e, trommi_client_core::app::Effect::Http { request, .. } if request.url.ends_with("/commits"))).count();
    assert_eq!(posts(&sim), 1);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty(), "the refused Commit is rolled back");
    // (a wait is a second, doubling, each cut by up to a quarter)
    sim.advance(700);
    assert_eq!(posts(&sim), 1, "not built again at once");
    sim.advance(400);
    assert_eq!(posts(&sim), 2, "built again after the first wait");
    sim.advance(1_100);
    assert_eq!(posts(&sim), 2, "the wait doubles");
    // the hub takes it now
    world.hub.borrow_mut().fault = None;
    sim.advance(60_000);
    assert_eq!(commits(&world, &world.group, &world.me), 1);
}

#[test]
fn a_human_device_renews_its_leaf_when_it_is_old() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(commits(&world, &world.group, &world.me) + commits(&world, &world.room, &world.me), 0, "a young leaf is left alone");
    // eight days on
    let mut sim = world.sim();
    sim.now_ms += 8 * 86_400_000;
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    // the room group first, alone: a session's Commit beside it would be `room-behind`
    assert_eq!((commits(&world, &world.room, &world.me), commits(&world, &world.group, &world.me)), (1, 0));
    news(&world, &mut sim);
    assert_eq!(commits(&world, &world.group, &world.me), 1, "{:?}", world.hub.borrow().refused);
    news(&world, &mut sim);
    assert!(world.hub.borrow().refused.is_empty(), "{:?}", world.hub.borrow().refused);
    // not again with the next round
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    assert_eq!(commits(&world, &world.group, &world.me), 1);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn an_epoch_that_is_full_is_moved_on_by_an_update() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let before = commits(&world, &world.group, &world.me);
    // the hub keeps the envelope as a void record: the epoch took all it may hold
    let (group, me) = (world.group, world.me);
    let hub = world.hub.clone();
    let mut once = true;
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request| {
        (request.url.ends_with("/v1/envelopes") && std::mem::take(&mut once)).then(|| Ok(answer(409, json!({ "error": "epoch-full", "message": "refused", "voided": true }))))
    }));
    let draft = Draft::SessionChat { session: world.session, payload: common::text(r#"{"content_type":"message","text":"one too many"}"#) };
    let (sealed, effects) = sim.app.act(sim.now_ms, |device, now| device.seal(&draft, None, &[], now));
    sealed.expect("a device").expect("the line seals");
    hub.borrow_mut().void_next(group, me);
    sim.run(effects);
    assert_eq!(commits(&world, &world.group, &world.me), before + 1, "an update of the own leaf moves the group on: {:?}", world.hub.borrow().seen);
    news(&world, &mut sim);
    let device = sim.app.engine().unwrap().device();
    assert!(device.outbox().is_empty() && !device.group(&world.group).unwrap().pending);
}
