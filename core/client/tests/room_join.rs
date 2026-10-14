//! The join of the room with the recovery code (engine/room_join) against the stand-in hub (tests/common): a new
//! slot ends in the room and its sessions; a wrong code, a room that moves on, a hub that is away, a restart
//! before and after the join was posted.
//!
//! The stand-in hub holds the recovery key's token to the routes the real hub allows it, and the joining reads
//! nothing else: a request beyond them would be refused as `forbidden` and is looked for in every scene.

mod common;

use common::hub::{answer, HUB_URL};
use common::{code_keys, now, start, World};
use trommi_client_core::app::io::{IoError, IoErrorKind, StoreOp, StreamItem};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Config, Effect, Event, ROOM_KEY};
use trommi_client_core::engine::room_join::{RoomJoin, RoomJoinError, RoomJoinStatus};
use trommi_client_core::views::Phase;
use trommi_core::crypto::Secret;
use trommi_core::ids::GroupId;
use trommi_core::recovery::RecoveryKeys;

/// A slot with nothing stored, against the world's hub.
fn blank(world: &World) -> Sim {
    let mut sim = Sim::new(now());
    world.attach(&mut sim);
    sim
}

fn join(world: &World, sim: &mut Sim, keys: RecoveryKeys) {
    let effects = sim.app.join_room(sim.now_ms, RoomJoin { hub: HUB_URL.into(), room: world.room.room_id(), keys, recover: None });
    sim.run(effects);
}

fn serve_all(world: &World, sim: &mut Sim) -> usize {
    let mut served = 0;
    while let Some((id, request)) = sim.held.front().cloned() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
        served += 1;
    }
    served
}

/// Everything answered, the stream told, the clock moved when nothing else is left: until the join failed, or the
/// App is live with its sessions joined.
fn settle(world: &World, sim: &mut Sim) {
    for _ in 0..200 {
        let served = serve_all(world, sim);
        let mut told = false;
        if let Some(me) = sim.app.engine().map(|e| e.device().id()) {
            for _ in 0..16 {
                let Some(bytes) = world.hub.borrow_mut().stream_news(&me) else { break };
                sim.stream(StreamItem::Chunk { bytes: bytes.into() });
                told = true;
            }
        }
        if served == 0 && !told {
            match sim.app.room_join() {
                Some(RoomJoinStatus::Failed(_)) => return,
                Some(RoomJoinStatus::Joined) if sim.app.sessions_not_joined().is_some() && sim.view.phase == Phase::Live => return,
                _ => sim.advance(1_000),
            }
        }
    }
}

fn seen(world: &World, what: &str) -> usize {
    world.hub.borrow().seen.iter().filter(|r| r.starts_with(what)).count()
}

fn room_commits(world: &World) -> String {
    format!("POST /v1/groups/{}/commits", trommi_core::ids::base64url_encode(world.room.as_bytes()))
}

fn no_forbidden(world: &World) {
    let refused = world.hub.borrow().refused.clone();
    assert!(!refused.iter().any(|r| r.ends_with("forbidden")), "the join read only what it needs: {refused:?}");
}

fn in_room(sim: &Sim, groups: &[GroupId]) -> bool {
    sim.app.engine().is_some_and(|e| groups.iter().all(|g| e.device().group(g).is_ok()))
}

#[test]
fn a_new_slot_joins_the_room_and_its_sessions_with_the_code() {
    let mut world = World::new();
    let second = world.another_session();
    let mut sim = blank(&world);
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    assert!(in_room(&sim, &[world.room, world.group, second]));
    assert!(sim.app.engine().unwrap().device().is_human());
    assert_eq!(sim.view.phase, Phase::Live);
    no_forbidden(&world);
    // the room's hub is kept for a start that names none
    let stored = String::from_utf8(sim.cache[ROOM_KEY].clone()).unwrap();
    assert!(stored.contains(HUB_URL), "{stored}");
    // nothing of the content before the room was joined: the Desk and the order come after the join's post
    let requests = world.hub.borrow().seen.clone();
    let posted = requests.iter().position(|r| r.starts_with(&room_commits(&world))).expect("the join was posted");
    assert!(requests[..posted].iter().all(|r| !r.contains("/v1/desk") && !r.contains("/v1/changes") && !r.contains("/v1/stream")), "{requests:?}");
    assert_eq!(seen(&world, &room_commits(&world)), 1);
    // the code's keys are in no effect and in no stored entry beside the device's own
    for effect in &sim.log {
        if let Effect::Log { text, .. } = effect {
            assert!(!text.contains("C0C0") && !text.contains("wDA"), "{text}");
        }
    }
}

#[test]
fn a_wrong_code_ends_with_nothing_stored() {
    let world = World::new();
    let mut sim = blank(&world);
    sim.send(start());
    join(&world, &mut sim, RecoveryKeys::from_code(Secret::new([0xC1; 32])).unwrap());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Failed(RoomJoinError::WrongCode)));
    assert!(sim.app.engine().is_none());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert!(sim.device.1.is_empty(), "no half device stays");
    // and the right code afterwards joins on the same slot
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert!(in_room(&sim, &[world.room, world.group]));
}

#[test]
fn a_room_that_moved_on_before_the_post_is_read_again_and_joined() {
    let mut world = World::new();
    let mut sim = blank(&world);
    sim.send(start());
    sim.hold = true;
    join(&world, &mut sim, code_keys());
    let commits = room_commits(&world);
    // everything up to the post is answered; the post waits
    for _ in 0..50 {
        let Some((id, request)) = sim.held.front().cloned() else { break };
        if request.method == "POST" && request.url.ends_with(&commits["POST ".len()..]) {
            break;
        }
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert_eq!(sim.held.len(), 1, "the join waits to be posted");
    // the person's other device moves the room on meanwhile
    common::sync(&world.hub.borrow(), &mut world.founder);
    world.founder.update(&world.room, true, now()).expect("an update").expect("a Commit");
    assert!(common::post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert!(in_room(&sim, &[world.room, world.group]));
    assert_eq!(seen(&world, &commits), 2, "refused once (epoch-taken), built again and taken");
    no_forbidden(&world);
}

#[test]
fn a_hub_that_is_away_is_waited_for() {
    let world = World::new();
    let mut sim = blank(&world);
    sim.send(start());
    let mut away = 4;
    world.hub.borrow_mut().fault = Some(Box::new(move |_, _| {
        if away == 0 {
            return None;
        }
        away -= 1;
        Some(if away % 2 == 0 { Err(IoError::new(IoErrorKind::Offline)) } else { Ok(answer(503, serde_json::json!({ "error": "unavailable" }))) })
    }));
    join(&world, &mut sim, code_keys());
    assert!(matches!(sim.app.room_join(), Some(RoomJoinStatus::Waiting { .. })), "{:?}", sim.app.room_join());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert!(in_room(&sim, &[world.room, world.group]));
}

/// The join is posted and taken, the answer never comes back, the process dies.
#[test]
fn a_join_posted_without_its_answer_goes_on_after_a_restart_and_is_not_posted_twice() {
    let world = World::new();
    let mut sim = blank(&world);
    sim.send(start());
    sim.hold = true;
    join(&world, &mut sim, code_keys());
    let commits = room_commits(&world);
    for _ in 0..50 {
        let Some((id, request)) = sim.held.pop_front() else { break };
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        if request.method == "POST" && request.url.ends_with(&commits["POST ".len()..]) {
            assert_eq!(got.as_ref().map(|r| r.status).ok(), Some(200));
            break; // taken by the hub, the answer lost
        }
        sim.answer(id, got);
    }
    assert_eq!(seen(&world, &commits), 1);
    sim.restart();
    sim.hold = false;
    // a shell that names no hub: the stored room names it
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), seed: 7, ..Default::default() } });
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Failed(RoomJoinError::Interrupted)));
    assert!(!sim.device.1.is_empty(), "a posted join is never wiped");
    // the code again, with the hub named by the stored room
    let effects = sim.app.join_room(sim.now_ms, RoomJoin { hub: HUB_URL.into(), room: world.room.room_id(), keys: code_keys(), recover: None });
    sim.run(effects);
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert!(in_room(&sim, &[world.room, world.group]));
    assert_eq!(seen(&world, &commits), 1, "found in the room's log, not posted again");
    no_forbidden(&world);
    // a later start without a hub opens the device on the stored room's hub
    sim.restart();
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), seed: 7, ..Default::default() } });
    assert!(sim.log.iter().any(|e| matches!(e, Effect::Store { op: StoreOp::Get { keys }, .. } if keys == &vec![ROOM_KEY.to_string()])));
    assert_eq!(sim.app.engine().map(|e| e.hub().to_string()), Some(HUB_URL.to_string()));
}

/// The process dies before the join is posted: the next start finds a device in no room, and the code starts
/// afresh over it.
#[test]
fn a_join_cut_before_its_post_starts_afresh() {
    let world = World::new();
    let mut sim = blank(&world);
    sim.send(start());
    sim.hold = true;
    join(&world, &mut sim, code_keys());
    // the challenge and the token only
    for _ in 0..2 {
        let (id, request) = sim.held.pop_front().expect("a request");
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert!(!sim.device.1.is_empty(), "the device was made and stored");
    sim.restart();
    sim.hold = false;
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.app.room_join(), None);
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert!(in_room(&sim, &[world.room, world.group]));
}

#[test]
fn a_slot_in_a_room_refuses_a_second_join() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    join(&world, &mut sim, code_keys());
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Failed(RoomJoinError::RoomExists)));
    assert!(sim.app.engine().is_some(), "the device in the room stays");
}
