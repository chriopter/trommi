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
fn code_signin_joins_a_helper_founded_before_the_new_devices_stream_on_web_and_ios() {
    for platform in ["web", "ios"] {
        let mut world = World::new();
        let helper = {
            let mut hub = world.hub.borrow_mut();
            let packages = hub.claim(&[world.me]).expect("the human's KeyPackage");
            common::sync(&hub, &mut world.agent);
            let session = world.agent.found_helper(&world.session, &packages, now()).expect("helper founding");
            assert!(common::post_all(&mut hub, &mut world.agent).is_empty());
            GroupId::session(world.room.room_id(), session)
        };
        let mut sim = blank(&world);
        sim.send(Event::Start { config: Config { platform: platform.into(), version: "0".into(), hub: HUB_URL.into(), seed: 7, ..Default::default() } });
        join(&world, &mut sim, code_keys());
        settle(&world, &mut sim);
        assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined), "{platform}");
        assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]), "{platform}");
        assert!(in_room(&sim, &[world.room, world.group, helper]), "{platform}: an earlier helper must not depend on a new stream notification");
        no_forbidden(&world);
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
    assert_eq!(sim.view.phase, Phase::SignedOut, "{:?}", sim.log.iter().filter(|e| matches!(e, Effect::Log { .. })).collect::<Vec<_>>());
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

#[test]
fn a_code_signin_joins_at_its_anchor_before_fetching_older_room_history() {
    let world = World::new();
    world.hub.borrow_mut().seen.clear();
    let mut sim = blank(&world);
    sim.send(start());
    sim.hold = true;
    join(&world, &mut sim, code_keys());
    let commits = room_commits(&world);
    for _ in 0..50 {
        let Some((id, request)) = sim.held.front().cloned() else { break };
        if request.method == "POST" && request.url.ends_with(&commits["POST ".len()..]) { break; }
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert!(sim.held.front().is_some_and(|(_, r)| r.method == "POST" && r.url.ends_with(&commits["POST ".len()..])), "the authenticated room join is ready");
    let requests = world.hub.borrow().seen.clone();
    let room_path = format!("/v1/groups/{}", trommi_core::ids::base64url_encode(world.room.as_bytes()));
    let earlier: Vec<_> = requests.iter().filter(|r| r.contains(&room_path) && (r.contains("epoch=0") || r.contains("/log"))).collect();
    assert!(earlier.is_empty(), "older room history is deferred until needed, but before joining it fetched {earlier:?}");
}

#[test]
fn an_anchored_signin_without_live_sessions_leaves_old_room_commits_unread_across_restart() {
    let world = World::new();
    world.hub.borrow_mut().archive(&world.group);
    world.hub.borrow_mut().seen.clear();
    let mut sim = blank(&world);
    sim.send(start());
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    assert!(!sim.app.engine().unwrap().device().group_past(&world.room).unwrap().unwrap().learned);
    let room = trommi_core::ids::base64url_encode(world.room.as_bytes());
    let old_info = format!("GET /v1/groups/{room}/info?epoch=0");
    let old_log = format!("GET /v1/groups/{room}/log?");
    assert!(world.hub.borrow().seen.iter().all(|s| s != &old_info && !s.starts_with(&old_log)), "{:?}", world.hub.borrow().seen);
    let position = sim.app.engine().unwrap().position();
    sim.restart();
    world.attach(&mut sim);
    sim.send(start());
    for _ in 0..5 { serve_all(&world, &mut sim); }
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(sim.app.engine().unwrap().position() >= position);
    assert!(!sim.app.engine().unwrap().device().group_past(&world.room).unwrap().unwrap().learned);
    assert!(world.hub.borrow().seen.iter().all(|s| s != &old_info && !s.starts_with(&old_log)), "{:?}", world.hub.borrow().seen);
}

#[test]
fn a_hostile_room_prefix_leaves_sessions_unjoined_without_accepting_its_facts() {
    let world = World::new();
    let room = trommi_core::ids::base64url_encode(world.room.as_bytes());
    let log_path = format!("/v1/groups/{room}/log?");
    let mut old = world.hub.borrow().order.iter().find(|item|
        matches!(item, common::hub::Item::Log { group, commit: true, .. } if *group == world.room)
    ).unwrap().json();
    old["bytes"] = serde_json::json!(trommi_core::ids::base64url_encode(&[1, 2, 3, 4]));
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request| {
        (request.method == "GET" && request.url.contains(&log_path)).then(||
            Ok(answer(200, serde_json::json!({ "items": [old.clone()], "more": false })))
        )
    }));
    let mut sim = blank(&world);
    sim.send(start());
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[(world.group, "bad-group")][..]));
    let device = sim.app.engine().unwrap().device();
    assert!(!device.group_past(&world.room).unwrap().unwrap().learned);
    assert!(device.group(&world.group).is_err());
    assert_eq!(device.content_key(&world.room, 0), Err(trommi_core::Error::NoKey));
}

#[test]
fn a_live_sessions_signin_learns_the_shared_room_prefix_only_once() {
    let mut world = World::new();
    world.another_session();
    world.hub.borrow_mut().seen.clear();
    let mut sim = blank(&world);
    sim.send(start());
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    let room = trommi_core::ids::base64url_encode(world.room.as_bytes());
    assert_eq!(seen(&world, &format!("GET /v1/groups/{room}/info?epoch=0")), 1);
    assert_eq!(seen(&world, &format!("GET /v1/groups/{room}/log?")), 1);
    assert!(sim.app.engine().unwrap().device().group_past(&world.room).unwrap().unwrap().learned);
}

#[test]
fn an_anchored_signin_defers_a_room_prefix_of_thirty_two_updates() {
    let mut world = World::new();
    common::sync(&world.hub.borrow(), &mut world.founder);
    for _ in 0..32 {
        world.founder.update(&world.room, true, now()).unwrap();
        assert!(common::post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
        common::sync(&world.hub.borrow(), &mut world.founder);
    }
    world.hub.borrow_mut().archive(&world.group);
    let (old_commits, old_bytes) = world.hub.borrow().order.iter().filter_map(|item| match item {
        common::hub::Item::Log { group, commit: true, bytes, .. } if *group == world.room => Some(bytes.len()),
        _ => None,
    }).fold((0, 0), |(count, bytes), size| (count + 1, bytes + size));
    world.hub.borrow_mut().seen.clear();
    let mut sim = blank(&world);
    sim.send(start());
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    let room = trommi_core::ids::base64url_encode(world.room.as_bytes());
    let log = format!("GET /v1/groups/{room}/log?");
    assert_eq!(seen(&world, &log), 0);
    assert!(old_commits >= 32);
    println!("anchored sign-in: old prefix {old_commits} Commits / {old_bytes} bytes; fetched 0 Commits / 0 bytes");
}

#[test]
fn a_restart_after_the_room_join_asks_for_the_code_and_resumes_without_another_device() {
    let world = World::new();
    let mut sim = blank(&world);
    sim.send(start());
    sim.hold = true;
    join(&world, &mut sim, code_keys());
    for _ in 0..50 {
        if sim.app.room_join() == Some(RoomJoinStatus::Joined) { break; }
        let (id, request) = sim.held.front().cloned().unwrap();
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    let device = sim.app.engine().unwrap().device().id();
    sim.restart();
    sim.hold = false;
    sim.send(start());
    for _ in 0..5 { serve_all(&world, &mut sim); }
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.view.account.step, "sign_in");
    assert_eq!(sim.view.account.problem.as_ref().unwrap().key, "account.interrupted");
    // It remains resumable after another process stop; no recovery code was stored.
    sim.restart();
    sim.send(start());
    for _ in 0..5 { serve_all(&world, &mut sim); }
    assert_eq!(sim.view.phase, Phase::SignedOut);
    join(&world, &mut sim, RecoveryKeys::from_code(Secret::new([0xC1; 32])).unwrap());
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Failed(RoomJoinError::WrongCode)));
    assert_eq!(sim.app.engine().unwrap().device().id(), device, "a wrong retry never erases the room leaf");
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    assert!(in_room(&sim, &[world.room, world.group]));
    assert_eq!(sim.app.engine().unwrap().device().id(), device);
    assert_eq!(seen(&world, &room_commits(&world)), 1);
}

#[test]
fn old_room_registers_on_the_first_desk_trigger_verified_history_before_the_code_is_dropped() {
    let mut world = World::new();
    world.founder_sets("star/00000000000000000000000000000000", r#"{"at":1}"#);
    world.hub.borrow_mut().archive(&world.group);
    let mut sim = blank(&world);
    sim.send(start());
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
    let device = sim.app.engine().unwrap().device();
    assert!(device.group_past(&world.room).unwrap().unwrap().learned);
    assert_eq!(device.content_key(&world.room, 0), world.founder.content_key(&world.room, 0));
}

#[test]
fn a_restart_before_the_first_desk_can_resume_old_room_keys_without_live_sessions() {
    let mut world = World::new();
    world.founder_sets("star/00000000000000000000000000000000", r#"{"at":1}"#);
    world.hub.borrow_mut().archive(&world.group);
    let mut sim = blank(&world);
    sim.send(start());
    sim.hold = true;
    join(&world, &mut sim, code_keys());
    for _ in 0..50 {
        if sim.app.room_join() == Some(RoomJoinStatus::Joined) { break; }
        let (id, request) = sim.held.front().cloned().unwrap();
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    let id = sim.app.engine().unwrap().device().id();
    sim.restart();
    sim.hold = false;
    sim.send(start());
    for _ in 0..10 { serve_all(&world, &mut sim); }
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.view.account.problem.as_ref().unwrap().key, "account.interrupted");
    join(&world, &mut sim, code_keys());
    settle(&world, &mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    let device = sim.app.engine().unwrap().device();
    assert_eq!(device.id(), id);
    assert_eq!(device.content_key(&world.room, 0), world.founder.content_key(&world.room, 0));
}
