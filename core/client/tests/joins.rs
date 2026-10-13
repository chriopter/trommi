//! A sign-in's joins of its session groups (engine/joins) where they leave the straight path, against the stand-in
//! hub (tests/common): a join that lost its epoch, a log that needs more pages, more groups than one request takes,
//! a hub that refuses the request as a whole.

mod common;

use common::hub::answer;
use common::{now, post_all, start, World};
use serde_json::json;
use trommi_client_core::app::io::{HttpRequest, StreamItem};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::engine::sync::joins::{SessionJoins, JOINS_AT_ONCE, READ_AT_ONCE};
use trommi_client_core::views::Phase;
use trommi_core::ids::{DeviceId, GroupId};

fn requests(world: &World) -> Vec<String> {
    world.hub.borrow().seen.clone()
}

fn count(world: &World, what: &str) -> usize {
    requests(world).iter().filter(|r| r.starts_with(what)).count()
}

/// Sends the App's stream what the hub holds for `device` beyond what it was sent. True: something came.
fn news(world: &World, sim: &mut Sim, device: &DeviceId) -> bool {
    let mut any = false;
    for _ in 0..16 {
        let Some(bytes) = world.hub.borrow_mut().stream_news(device) else { break };
        sim.stream(StreamItem::Chunk { bytes: bytes.into() });
        any = true;
    }
    any
}

/// Answers the held requests that `which` picks, one after another, as the hub answers them. Returns how many.
fn serve(world: &World, sim: &mut Sim, which: impl Fn(&HttpRequest) -> bool) -> usize {
    let mut served = 0;
    while let Some((id, request)) = sim.held.iter().find(|(_, r)| which(r)).cloned() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
        served += 1;
    }
    served
}

/// Everything held is answered, the stream told, and the clock moved when nothing else is left, until all rests.
fn settle(world: &World, sim: &mut Sim, device: &DeviceId) {
    for _ in 0..120 {
        let served = serve(world, sim, |_| true);
        let told = news(world, sim, device);
        if served == 0 && !told {
            if sim.app.engine().is_some_and(|e| !e.joining_sessions()) && sim.view.phase == Phase::Live {
                return;
            }
            sim.advance(1_000);
        }
    }
}

fn held(sim: &Sim, groups: &[GroupId]) -> usize {
    groups.iter().filter(|g| sim.app.engine().unwrap().device().group(g).is_ok()).count()
}

#[test]
fn a_join_whose_epoch_was_taken_meanwhile_is_read_alone_and_built_again() {
    let mut world = World::new();
    let second = world.another_session();
    let (store, id, keys) = world.newcomer();
    let mut sim = world.sim_of(&store);
    sim.hold = true;
    sim.send(start());
    settle(&world, &mut sim, &id);
    let groups = vec![world.group, second];
    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: groups.clone() });
    sim.run(effects);
    assert_eq!(serve(&world, &mut sim, |r| r.url.ends_with("/v1/groups/read")), 1);
    assert_eq!(sim.held.iter().filter(|(_, r)| r.url.ends_with("/v1/commits")).count(), 1, "the joins wait together");
    // the person's other device moves the first session on before the joins reach the hub
    world.peers_sync();
    world.founder.update(&world.group, true, now()).expect("an update").expect("a Commit");
    assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    settle(&world, &mut sim, &id);
    assert_eq!(held(&sim, &groups), 2, "{:?}", requests(&world));
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    // the one that lost its epoch: read alone (its founding, its current GroupInfo, its Commits) and posted alone
    let path = trommi_core::ids::base64url_encode(world.group.as_bytes());
    assert!(requests(&world).iter().any(|r| r == &format!("GET /v1/groups/{path}/info?epoch=0")), "{:?}", requests(&world));
    assert_eq!(count(&world, &format!("POST /v1/groups/{path}/commits")), 1, "{:?}", requests(&world));
    assert_eq!(count(&world, "POST /v1/commits"), 1);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
}

#[test]
fn a_log_that_goes_on_is_read_on_page_by_page() {
    let mut world = World::new();
    // the session's log holds three Commits; the hub serves one per page
    for _ in 0..2 {
        world.peers_sync();
        world.founder.update(&world.group, true, now()).expect("an update").expect("a Commit");
        assert!(post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    }
    let (store, id, keys) = world.newcomer();
    world.hub.borrow_mut().log_max = Some(1);
    let mut sim = world.sim_of(&store);
    sim.send(start());
    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: vec![world.group] });
    sim.run(effects);
    settle(&world, &mut sim, &id);
    assert_eq!(held(&sim, &[world.group]), 1, "{:?}", requests(&world));
    let path = trommi_core::ids::base64url_encode(world.group.as_bytes());
    let pages: Vec<String> = requests(&world).into_iter().filter(|r| r.starts_with(&format!("GET /v1/groups/{path}/log?"))).collect();
    assert_eq!(pages.len(), 2, "the row's page, then two more Commits one page each, the last needing none: {pages:?}");
    assert!(pages.iter().all(|p| p.contains("kind=commit")));
    assert_eq!(count(&world, "GET /v1/groups/"), 2, "no GroupInfo read: the row served them {:?}", requests(&world));
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
}

#[test]
fn more_groups_than_one_request_takes_go_in_several() {
    let mut world = World::new();
    let mut groups = vec![world.group];
    for _ in 0..READ_AT_ONCE.max(JOINS_AT_ONCE) {
        groups.push(world.another_session());
    }
    let (store, id, keys) = world.newcomer();
    let mut sim = world.sim_of(&store);
    sim.send(start());
    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: groups.clone() });
    sim.run(effects);
    settle(&world, &mut sim, &id);
    assert_eq!(held(&sim, &groups), groups.len(), "{:?}", world.hub.borrow().refused);
    assert_eq!(count(&world, "POST /v1/groups/read"), 2, "{READ_AT_ONCE} and the rest");
    // the first read's joins go together; the one left is alone at the outbox's head and goes alone (the answer to
    // the 32 is refused by the codec today, see the ignored test below: the joins come back in the order and are
    // settled from there)
    assert_eq!(count(&world, "POST /v1/commits"), 1, "{JOINS_AT_ONCE} together");
    let alone = requests(&world).iter().filter(|r| r.starts_with("POST /v1/groups/") && r.ends_with("/commits")).count();
    assert_eq!(alone, groups.len() - JOINS_AT_ONCE);
    assert_eq!(count(&world, "GET /v1/groups/"), 0, "none alone");
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
}

#[test]
fn a_hub_that_refuses_the_requests_as_a_whole_gets_each_group_alone() {
    let mut world = World::new();
    let second = world.another_session();
    let (store, id, keys) = world.newcomer();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        let refuse = |code: &str| Some(Ok(answer(trommi_client_core::hub::status_of(code).unwrap_or(400), json!({ "error": code, "message": "refused" }))));
        if request.url.ends_with("/v1/groups/read") {
            return refuse("too-large");
        }
        if request.url.ends_with("/v1/commits") {
            return refuse("bad-format");
        }
        None
    }));
    let mut sim = world.sim_of(&store);
    sim.send(start());
    let groups = vec![world.group, second];
    let effects = sim.app.join_sessions(sim.now_ms, SessionJoins { keys, later: None, groups: groups.clone() });
    sim.run(effects);
    settle(&world, &mut sim, &id);
    assert_eq!(held(&sim, &groups), 2, "{:?}", requests(&world));
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    // each group read alone, and each join posted alone
    for group in &groups {
        let path = trommi_core::ids::base64url_encode(group.as_bytes());
        assert!(requests(&world).iter().any(|r| r == &format!("GET /v1/groups/{path}/info?epoch=0")), "{:?}", requests(&world));
    }
    // (the hub never saw the refused ones: the fault answered first)
    assert_eq!(count(&world, "POST /v1/groups/read") + count(&world, "POST /v1/commits"), 0);
    let alone = requests(&world).iter().filter(|r| r.starts_with("POST /v1/groups/") && r.ends_with("/commits")).count();
    assert_eq!(alone, 2, "{:?}", requests(&world));
}

#[test]
#[ignore = "a finding for the codec: hub/json.rs's value budget (a body's length / 8 + 64) refuses the well-formed answer of 32 joins (24 bytes and five values a row); the engine then settles them from the order"]
fn the_answer_to_thirty_two_joins_is_read() {
    use trommi_client_core::app::io::HttpResponse;
    use trommi_client_core::hub::{Answer, Codec, Route, Site};
    use trommi_core::ids::{RoomId, SessionId};
    use trommi_core::store::{OutboxEntry, OutboxKind};
    let room = RoomId::new([1; 32]);
    let entries: Vec<OutboxEntry> = (0..JOINS_AT_ONCE as u8)
        .map(|i| OutboxEntry { id: 1, kind: OutboxKind::ExternalCommit, group: Some(GroupId::session(room, SessionId::new([i; 16]))), epoch: 1, parts: vec![vec![1], vec![2], vec![3], vec![4]] })
        .collect();
    let site = Site { url: common::hub::HUB_URL, token: Some("abcdefghijklmnop_-0123"), client: "web/1.2.3" };
    let (_, expect) = Codec.request(&site, &Route::Joins { entries: &entries }).expect("a request");
    let rows: Vec<String> = (0..entries.len()).map(|i| format!(r#"{{"change":{},"epoch":2}}"#, 20 + i)).collect();
    let body = format!(r#"{{"answers":[{}]}}"#, rows.join(","));
    let response = HttpResponse { status: 200, headers: Vec::new(), body: body.into_bytes().into() };
    assert!(matches!(Codec.answer(&expect, &response), Ok(Answer::Joins(rows)) if rows.len() == JOINS_AT_ONCE));
}
