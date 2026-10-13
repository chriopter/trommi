//! The stream's events beside the hub's order, against the stand-in hub (tests/common): a relayed piece, a wish for
//! keys, an archived session, an invite's Request, an evicted file.

mod common;

use common::{now, post_all, start, World};
use trommi_client_core::app::io::{StoreOp, StreamItem};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::Effect;
use trommi_client_core::engine::sync::UPKEEP_EVERY_MS;
use trommi_client_core::model::types::ItemKey;
use trommi_client_core::views::Phase;
use trommi_core::board_items::{Ink, Point};
use trommi_core::ids::{BoardId, DeviceId, FileId, GroupId, InviteId};

const HOUR: u64 = 3_600_000;
const DAY: u64 = 24 * HOUR;

/// Sends the App's stream what the hub holds for it beyond what it was sent, until nothing is new.
fn news(world: &World, sim: &mut Sim) {
    for _ in 0..16 {
        let Some(bytes) = world.hub.borrow_mut().stream_news(&world.me) else { return };
        sim.stream(StreamItem::Chunk { bytes: bytes.into() });
    }
}

fn count(world: &World, what: &str) -> usize {
    world.hub.borrow().seen.iter().filter(|r| r.starts_with(what)).count()
}

/// The messages (not Commits) the hub took in `group` from `from`: a key handover is one.
fn messages(world: &World, group: &GroupId, from: &DeviceId) -> usize {
    world.hub.borrow().order.iter().filter(|i| matches!(i, common::hub::Item::Log { group: g, commit: false, from: f, .. } if g == group && f == from)).count()
}

fn piece() -> Vec<u8> {
    let p = |x, y, t| Point { x, y, t, force: 128, azimuth: 0, altitude: 0 };
    let points = Ink::new(vec![p(0, 0, 0), p(160, 80, 16)], false, false).expect("ink").to_base64url();
    let stroke = trommi_core::ids::base64url_encode(&[0x5a; 16]);
    format!(r#"{{"stroke":"{stroke}","number":1,"tool":"pen","color":"blue","width":64,"points":"{points}"}}"#).into_bytes()
}

/// The live strokes the board model shows: their item ids.
fn live_strokes(sim: &Sim) -> Vec<String> {
    sim.app
        .board()
        .model
        .timelines
        .values()
        .flat_map(|t| t.items.keys())
        .filter_map(|k| match k {
            ItemKey::Local(id) if id.starts_with("live:") => Some(id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_stroke_piece_the_hub_relays_is_drawn_while_it_is_drawn() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    world.peers_sync();
    let board = BoardId::new([7; 16]);
    world.founder.send_stroke_piece(&board, &piece()).expect("the piece is sent");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    news(&world, &mut sim);
    let founder = trommi_client_core::engine::feed::hex(world.founder.id().as_bytes());
    assert_eq!(live_strokes(&sim), vec![format!("live:{founder}/{}", "5a".repeat(16))]);
    // (in no log: the order did not move for it)
    assert!(sim.app.alerts().is_empty(), "{:?}", sim.app.alerts());
}

#[test]
fn a_wish_for_keys_is_answered_once_with_a_handover_and_not_again_after_a_restart() {
    let mut world = World::new();
    let (_, newcomer, _) = world.newcomer();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let before = messages(&world, &world.room, &world.me);
    let at = sim.now_ms;
    world.hub.borrow_mut().wish(&newcomer, "handover", Some(world.group), at);
    news(&world, &mut sim);
    assert_eq!(messages(&world, &world.room, &world.me), before + 1, "one handover: {:?}", world.hub.borrow().refused);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    // the hub still lists the wish: neither the next round nor a restart past the core's hour answers it again
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    assert!(count(&world, "GET /v1/requests") >= 1);
    sim.now_ms += 2 * HOUR;
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let asked = count(&world, "GET /v1/requests");
    news(&world, &mut sim);
    sim.advance(UPKEEP_EVERY_MS + 1);
    assert!(count(&world, "GET /v1/requests") > asked, "the list was read after the restart");
    assert_eq!(messages(&world, &world.room, &world.me), before + 1, "not again");
}

#[test]
fn a_wish_made_while_this_device_was_away_is_answered_by_the_timed_round_unless_it_is_old() {
    let mut world = World::new();
    let (_, newcomer, _) = world.newcomer();
    let before = messages(&world, &world.room, &world.me);
    // a wish of two days ago: passed over
    world.hub.borrow_mut().wish(&newcomer, "handover", Some(world.group), now() - 2 * DAY);
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(count(&world, "GET /v1/requests"), 1, "the first round reads the list");
    assert_eq!(messages(&world, &world.room, &world.me), before);
    // one of now, said to no stream: the next round finds it
    let at = sim.now_ms;
    world.hub.borrow_mut().wish(&newcomer, "handover", Some(world.group), at);
    world.hub.borrow_mut().notes.clear();
    sim.advance(UPKEEP_EVERY_MS + 1);
    assert_eq!(messages(&world, &world.room, &world.me), before + 1, "{:?}", world.hub.borrow().seen);
}

#[test]
fn a_wish_of_this_device_itself_or_of_an_agent_is_not_answered() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let before = messages(&world, &world.room, &world.me);
    let at = sim.now_ms;
    world.hub.borrow_mut().wish(&world.me, "handover", Some(world.group), at);
    world.hub.borrow_mut().wish(&world.agent.id(), "handover", Some(world.group), at);
    news(&world, &mut sim);
    sim.advance(UPKEEP_EVERY_MS + 1);
    assert_eq!(messages(&world, &world.room, &world.me), before);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
}

#[test]
fn a_session_the_hub_archives_is_archived_here() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert!(!sim.app.engine().unwrap().device().group(&world.group).unwrap().archived);
    world.hub.borrow_mut().archive(&world.group);
    news(&world, &mut sim);
    assert!(sim.app.engine().unwrap().device().group(&world.group).unwrap().archived);
    let session = trommi_client_core::engine::feed::hex(world.session.as_bytes());
    assert!(sim.app.board().model.sessions.get(&session).is_some_and(|s| s.group_archived), "the board shows it archived");
}

#[test]
fn a_session_archived_while_this_device_was_away_is_archived_at_the_start() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let other = world.another_session();
    news(&world, &mut sim);
    assert!(sim.app.engine().unwrap().device().group(&other).is_ok_and(|g| !g.archived), "joined by its Welcome, read again after the room Commit it builds on");
    // archived at the hub while the App was not running: its stream is not told
    sim.restart();
    world.hub.borrow_mut().archive(&other);
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let device = sim.app.engine().unwrap().device();
    assert!(device.group(&other).unwrap().archived, "{:?}", world.hub.borrow().seen);
    assert!(!device.group(&world.group).unwrap().archived);
    // once per run: the next rounds do not read the list again
    let lists = |world: &World| world.hub.borrow().seen.iter().filter(|r| r.starts_with("GET /v1/rooms/") && r.contains("/groups?")).count();
    let before = lists(&world);
    sim.advance(UPKEEP_EVERY_MS + 1);
    assert_eq!(lists(&world), before);
}

#[test]
fn an_invite_request_and_an_evicted_file_reach_the_app() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let invite = InviteId::new([3; 16]);
    let event = format!("event: request\ndata: {{\"kind\":\"invite\",\"invite_id\":\"{}\"}}\n\n", invite.to_base64url());
    sim.stream(StreamItem::Chunk { bytes: event.clone().into_bytes().into() });
    sim.stream(StreamItem::Chunk { bytes: event.into_bytes().into() });
    assert_eq!(sim.app.take_invite_requests(), vec![invite], "each once");
    assert!(sim.app.take_invite_requests().is_empty());
    // a file the hub deleted: its cached bytes go
    let file = FileId::new([9; 16]);
    let room = trommi_client_core::engine::feed::hex(world.room.room_id().as_bytes());
    let key = trommi_client_core::cache::key(&room, trommi_client_core::cache::part::FILE, Some(&"09".repeat(16)));
    sim.cache.insert(key.clone(), vec![1, 2, 3]);
    world.hub.borrow_mut().evict(&file);
    news(&world, &mut sim);
    assert!(sim.log.iter().any(|e| matches!(e, Effect::Store { op: StoreOp::Put { delete, .. }, .. } if delete.contains(&key))));
    assert!(!sim.cache.contains_key(&key));
}

fn commits_of(world: &World, group: &GroupId, from: &DeviceId) -> usize {
    world.hub.borrow().order.iter().filter(|i| matches!(i, common::hub::Item::Log { group: g, commit: true, from: f, .. } if g == group && f == from)).count()
}

#[test]
fn a_wish_to_be_let_in_again_is_answered_once_by_one_commit() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    // the founder could not use a Welcome into the session: it asks with a fresh KeyPackage
    let founder = world.founder.id();
    world.founder.key_packages_to_upload(0, now()).expect("key packages");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    let package = world.hub.borrow_mut().claim(&[founder]).expect("a KeyPackage").remove(0);
    let before = commits_of(&world, &world.group, &world.me);
    let at = sim.now_ms;
    world.hub.borrow_mut().wish_with(&founder, "readmit", Some(world.group), at, Some(&package), None, None);
    news(&world, &mut sim);
    for _ in 0..5 {
        sim.advance(1_000);
        news(&world, &mut sim);
    }
    assert_eq!(commits_of(&world, &world.group, &world.me), before + 1, "one readmit: {:?}", world.hub.borrow().refused);
    // the same wish again (the hub's list, or another wish soon after): not a second Commit, which would cut it out
    world.hub.borrow_mut().wish_with(&founder, "readmit", Some(world.group), sim.now_ms, Some(&package), None, None);
    news(&world, &mut sim);
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    assert_eq!(commits_of(&world, &world.group, &world.me), before + 1, "not again");
    assert!(sim.app.engine().unwrap().device().group(&world.group).is_ok_and(|g| g.leaves.contains(&founder)));
}

#[test]
fn a_leafs_refusal_of_a_commit_is_said_once() {
    use trommi_client_core::engine::Alert;
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let (founder, agent) = (world.founder.id(), world.agent.id());
    world.hub.borrow_mut().wish_with(&founder, "reject", Some(world.group), sim.now_ms, None, Some(3), Some(agent));
    news(&world, &mut sim);
    sim.advance(UPKEEP_EVERY_MS + 1);
    news(&world, &mut sim);
    let said: Vec<&Alert> = sim.app.alerts().iter().filter(|a| matches!(a, Alert::Rejected { .. })).collect();
    assert_eq!(said, vec![&Alert::Rejected { group: world.group, n: 3, from: founder, committer: Some(agent) }]);
}
