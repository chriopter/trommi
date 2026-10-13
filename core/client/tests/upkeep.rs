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

use trommi_core::hub_auth::HubAddress;
use trommi_core::invite::Role;

/// The App's device lets `joiner` in by link, up to "They match": the Commit is in its outbox, and is posted.
fn app_invites(sim: &mut Sim, joiner: &mut Peer, role: Role) {
    let address = HubAddress::parse(common::hub::HUB_URL).expect("an address");
    let (opened, effects) = sim.app.act(sim.now_ms, |device, now| device.invite_open(role, None, "https://app.example", &address, now));
    let opened = opened.expect("a device").expect("an invite");
    sim.run(effects);
    let link = String::from_utf8(opened.link.expose().to_vec()).expect("the link is text");
    let request = joiner.join_request(&link, &opened.signed_offer, now()).expect("a request").signed_request;
    let (accepted, effects) = sim.app.act(sim.now_ms, |device, now| device.invite_accept(&opened.invite_id, &request, now));
    let accepted = accepted.expect("a device").expect("accepted");
    sim.run(effects);
    let code = joiner.join_reveal(&accepted.signed_reveal).expect("the code");
    let (confirmed, effects) = sim.app.act(sim.now_ms, |device, now| device.invite_confirm(&opened.invite_id, &code, &accepted.request_hash, true, now));
    confirmed.expect("a device").expect("confirmed").expect("a Commit");
    sim.run(effects);
}

#[test]
fn a_human_device_let_in_by_link_gets_the_handover_and_its_place_in_every_session() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let (mut joiner, _) = common::new_peer();
    app_invites(&mut sim, &mut joiner, Role::Human);
    assert_eq!(commits(&world, &world.room, &world.me), 1, "the Commit that lets it in");
    // the Commit comes back; the newcomer joins and stocks its KeyPackages
    news(&world, &mut sim);
    let welcome = world.hub.borrow().welcomes.last().expect("a Welcome").bytes.clone();
    joiner.join_invited(&welcome, now()).expect("the newcomer joins the room");
    sync(&world.hub.borrow(), &mut joiner);
    joiner.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut joiner).is_empty());
    // the steps go on by themselves: the handover in the room group, then the Add into the session
    for _ in 0..6 {
        sim.advance(3_000);
        news(&world, &mut sim);
    }
    assert_eq!(commits(&world, &world.group, &world.me), 1, "one Commit adds it to the session: {:?} {:?}", world.hub.borrow().seen, world.hub.borrow().refused);
    let me = sim.app.engine().unwrap().device();
    assert!(me.group(&world.group).unwrap().leaves.contains(&joiner.id()));
    assert!(me.outbox().is_empty());
    assert!(world.hub.borrow().order.iter().any(|i| matches!(i, common::hub::Item::Log { commit: false, from, .. } if *from == world.me)), "the handover went out");
    // (the only refusals: a claim before the newcomer had stocked)
    assert!(world.hub.borrow().refused.iter().all(|r| r == "POST /v1/key-packages/claim: not-found"), "{:?}", world.hub.borrow().refused);
    // the invite is finished: nothing is left, and nothing more is asked or committed
    let (steps, _) = sim.app.act(sim.now_ms, |device, _| device.invite_steps());
    assert!(steps.unwrap().unwrap().is_empty());
    let claims = count(&world, CLAIM);
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    assert_eq!((count(&world, CLAIM), commits(&world, &world.group, &world.me)), (claims, 1));
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn an_agent_let_in_by_link_gets_its_main_session() {
    let mut world = World::new();
    stock_founder(&mut world);
    let mut sim = world.sim();
    sim.send(start());
    let (mut agent, _) = common::new_peer();
    let sessions = world.hub.borrow().session_groups().len();
    app_invites(&mut sim, &mut agent, Role::Agent);
    for _ in 0..4 {
        news(&world, &mut sim);
        sim.advance(3_000);
    }
    let groups = world.hub.borrow().session_groups();
    assert_eq!(groups.len(), sessions + 1, "one session is founded: {:?}", sim.log.iter().filter(|e| matches!(e, trommi_client_core::app::Effect::Log { .. })).collect::<Vec<_>>());
    let founded = groups.iter().find(|g| **g != world.group).unwrap();
    let leaves = world.hub.borrow().observer(founded).unwrap().leaves().unwrap();
    assert!(leaves.contains(&agent.id()) && leaves.contains(&world.me) && leaves.contains(&world.founder.id()), "the agent, and every human device with a KeyPackage");
    assert_eq!(world.hub.borrow().seen.iter().filter(|r| *r == "POST /v1/groups").count(), 1, "{:?} {:?}", world.hub.borrow().refused, sim.log.iter().filter(|e| matches!(e, trommi_client_core::app::Effect::Log { .. })).collect::<Vec<_>>());
    let (steps, _) = sim.app.act(sim.now_ms, |device, _| device.invite_steps());
    assert!(steps.unwrap().unwrap().is_empty());
    assert!(world.hub.borrow().refused.is_empty(), "{:?}", world.hub.borrow().refused);
}

/// The person's other device has KeyPackages at the hub.
fn stock_founder(world: &mut World) {
    world.peers_sync();
    world.founder.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
}

/// An agent device another human device let into the room, without a session: its inviter went away.
fn seatless(world: &mut World) -> Peer {
    stock_founder(world);
    let (mut agent, _) = common::new_peer();
    agent.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut agent).is_empty());
    world.peers_sync();
    common::invite(&mut world.founder, &mut agent, Role::Agent);
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    agent
}

fn online(sim: &mut Sim, device: &DeviceId) {
    let event = format!("event: presence\ndata: {{\"device\":\"{}\",\"online\":true}}\n\n", device.to_base64url());
    sim.stream(StreamItem::Chunk { bytes: event.into_bytes().into() });
}

#[test]
fn an_agent_without_a_session_gets_one_from_whoever_is_up_once_it_waited_connected() {
    let mut world = World::new();
    let agent = seatless(&mut world);
    let mut sim = world.sim();
    sim.send(start());
    let sessions = world.hub.borrow().session_groups().len();
    let groups = "GET /v1/rooms/";
    // (the start reads the list once, for sessions archived meanwhile)
    let started = world.hub.borrow().seen.iter().filter(|r| r.starts_with(groups) && r.contains("/groups")).count();
    let lists = |world: &World| world.hub.borrow().seen.iter().filter(|r| r.starts_with(groups) && r.contains("/groups")).count() - started;
    // not while it is away, however long
    sim.advance(120_000);
    assert_eq!((lists(&world), world.hub.borrow().session_groups().len()), (0, sessions));
    // connected, and its grace was waited out (it counts from when this device first saw it without a session)
    online(&mut sim, &agent.id());
    assert_eq!(lists(&world), 1, "the hub's list is asked: {:?}", world.hub.borrow().seen);
    assert_eq!(world.hub.borrow().session_groups().len(), sessions + 1, "{:?} {:?}", world.hub.borrow().seen, world.hub.borrow().refused);
    let founded = *world.hub.borrow().session_groups().iter().find(|g| **g != world.group).unwrap();
    assert!(world.hub.borrow().observer(&founded).unwrap().leaves().unwrap().contains(&agent.id()));
    // never a second
    news(&world, &mut sim);
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    assert_eq!(world.hub.borrow().session_groups().len(), sessions + 1);
    // (the only refusal: a token that ended over the ten minutes)
    assert!(world.hub.borrow().refused.iter().all(|r| r.ends_with(": unauthorised")), "{:?}", world.hub.borrow().refused);
}

#[test]
fn an_agent_the_hub_lists_in_a_session_is_not_given_a_second() {
    let mut world = World::new();
    let agent = seatless(&mut world);
    let mut sim = world.sim();
    sim.send(start());
    let sessions = world.hub.borrow().session_groups().len();
    sim.advance(60_000);
    // its inviter founds the session after all, and this device has not read that yet; the list comes page by page
    world.hub.borrow_mut().groups_max = Some(1);
    let packages = world.hub.borrow_mut().claim(&[agent.id()]).expect("a KeyPackage");
    world.founder.found_session(&agent.id(), &packages, now()).expect("the founding is built");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    online(&mut sim, &agent.id());
    assert_eq!(world.hub.borrow().session_groups().len(), sessions + 1, "only the inviter's");
    assert_eq!(world.hub.borrow().seen.iter().filter(|r| *r == "POST /v1/groups").count(), 0, "{:?}", world.hub.borrow().seen);
    assert_eq!(count(&world, CLAIM), 0);
}

/// The App's device lets `joiner` in by link to take over `session` (5.3.1), up to "They match".
fn app_invites_to(sim: &mut Sim, joiner: &mut Peer, session: &trommi_core::ids::SessionId) {
    let address = HubAddress::parse(common::hub::HUB_URL).expect("an address");
    let (opened, effects) = sim.app.act(sim.now_ms, |device, now| device.invite_open(Role::Agent, Some(session), "https://app.example", &address, now));
    let opened = opened.expect("a device").expect("an invite");
    sim.run(effects);
    let link = String::from_utf8(opened.link.expose().to_vec()).expect("the link is text");
    let request = joiner.join_request(&link, &opened.signed_offer, now()).expect("a request").signed_request;
    let (accepted, effects) = sim.app.act(sim.now_ms, |device, now| device.invite_accept(&opened.invite_id, &request, now));
    let accepted = accepted.expect("a device").expect("accepted");
    sim.run(effects);
    let code = joiner.join_reveal(&accepted.signed_reveal).expect("the code");
    let (confirmed, effects) = sim.app.act(sim.now_ms, |device, now| device.invite_confirm(&opened.invite_id, &code, &accepted.request_hash, true, now));
    confirmed.expect("a device").expect("confirmed").expect("a Commit");
    sim.run(effects);
}

#[test]
fn a_takeover_by_link_seats_the_new_agent_in_the_session_and_every_helper_session_and_checks_the_hubs_list() {
    let mut world = World::new();
    stock_founder(&mut world);
    // the old agent opens a helper session under its main session, with the person's two devices in it
    let helper = {
        let mut hub = world.hub.borrow_mut();
        let packages = hub.claim(&[world.founder.id(), world.me]).expect("a KeyPackage of each");
        sync(&hub, &mut world.agent);
        let session = world.agent.found_helper(&world.session, &packages, now()).expect("the helper session is founded");
        assert!(post_all(&mut hub, &mut world.agent).is_empty());
        GroupId::session(world.room.room_id(), session)
    };
    let mut sim = world.sim();
    sim.send(start());
    news(&world, &mut sim);
    assert!(sim.app.engine().unwrap().device().group(&helper).is_ok(), "the App's device joined the helper session founded while it was away");
    let old = world.agent.id();
    // a new agent device takes the session over; it has KeyPackages at the hub for the helper session
    let (mut new, _) = common::new_peer();
    new.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut new).is_empty());
    app_invites_to(&mut sim, &mut new, &world.session);
    for _ in 0..8 {
        news(&world, &mut sim);
        sim.advance(3_000);
    }
    news(&world, &mut sim);
    let leaves = |group: &GroupId| world.hub.borrow().observer(group).unwrap().leaves().unwrap();
    assert!(leaves(&world.group).contains(&new.id()) && !leaves(&world.group).contains(&old), "the main session: {:?}", world.hub.borrow().refused);
    assert!(leaves(&helper).contains(&new.id()) && !leaves(&helper).contains(&old), "the helper session: {:?} {:?}", world.hub.borrow().seen, world.hub.borrow().refused);
    // the hub's list of the helper sessions was held against this device's, and the invite is finished
    assert!(world.hub.borrow().seen.iter().any(|r| r.starts_with("GET /v1/rooms/") && r.contains("/groups?")));
    let (steps, _) = sim.app.act(sim.now_ms, |device, _| device.invite_steps());
    assert_eq!(steps.unwrap().unwrap(), Vec::new(), "{:?}", world.hub.borrow().seen);
    assert_eq!(commits(&world, &world.group, &world.me), 1, "one Commit takes the main session over");
    assert_eq!(commits(&world, &helper, &world.me), 1, "one Commit takes the helper session over");
    assert!(world.hub.borrow().refused.iter().all(|r| r.ends_with(": unauthorised")), "{:?}", world.hub.borrow().refused);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}
