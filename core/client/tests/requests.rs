//! The actions' requests through the engine (engine/requests) against the stand-in hub: the engine adds the token,
//! renews it on a 401 and sends the same request again, retries by the request's class, and sends a kept request
//! again after a restart.

mod common;

use common::hub::HUB_URL;
use common::{start, World};
use trommi_client_core::app::io::{Header, HttpClass, HttpRequest, IoError, IoErrorKind, IoId};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::engine::requests::{Answered, KEPT_PREFIX};
use trommi_client_core::hub::{Failure, Site};
use trommi_client_core::views::Phase;
use trommi_core::ids::base64url_encode;

/// `POST /v1/groups/{group}/reject`, built as a codec builder does: with the token of the site.
fn reject(group: &str, class: HttpClass) -> impl FnOnce(&Site<'_>) -> Result<HttpRequest, Failure> {
    let group = group.to_string();
    move |site: &Site<'_>| {
        Ok(HttpRequest {
            method: "POST".into(),
            url: format!("{}/v1/groups/{group}/reject", site.url),
            headers: vec![
                Header { name: "trommi-client".into(), value: site.client.into() },
                Header { name: "authorization".into(), value: format!("Bearer {}", site.token.unwrap_or_default()) },
            ],
            body: Some(br#"{"n":1}"#.to_vec().into()),
            timeout_ms: 30_000,
            max_answer: 4096,
            class,
            key: None,
        })
    }
}

fn live(world: &World, sim: &mut Sim) {
    for _ in 0..60 {
        world.stream(sim);
        if sim.view.phase == Phase::Live {
            return;
        }
        sim.advance(1_000);
    }
    panic!("not live");
}

fn ask(sim: &mut Sim, build: impl FnOnce(&Site<'_>) -> Result<HttpRequest, Failure>, keep: Option<&str>) -> IoId {
    let (id, effects) = sim.app.request(sim.now_ms, build, keep.map(str::to_string));
    sim.run(effects);
    id.expect("a device").expect("sent")
}

fn rejects(world: &World) -> usize {
    world.hub.borrow().seen.iter().filter(|r| r.starts_with("POST /v1/groups/") && r.ends_with("/reject")).count()
}

fn settle(world: &World, sim: &mut Sim, answers: &mut Vec<Answered>) {
    for _ in 0..120 {
        answers.extend(sim.app.take_answers());
        world.stream(sim);
        sim.advance(1_000);
    }
    answers.extend(sim.app.take_answers());
}

#[test]
fn a_token_that_ended_is_renewed_and_the_same_request_sent_again() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    live(&world, &mut sim);
    world.hub.borrow_mut().end_tokens();
    let group = base64url_encode(world.group.as_bytes());
    let id = ask(&mut sim, reject(&group, HttpClass::Interactive), None);
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0].id, id);
    assert_eq!(answers[0].result.as_ref().map(|r| r.status).ok(), Some(200));
    assert_eq!(rejects(&world), 2, "refused once as unauthorised, then sent with the new token");
    assert!(world.hub.borrow().refused.iter().any(|r| r.ends_with("/reject: unauthorised")));
}

#[test]
fn a_refusal_is_reported_as_the_hubs_answer() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    live(&world, &mut sim);
    // (a route the stand-in does not know: its refusal)
    let id = ask(
        &mut sim,
        |site: &Site<'_>| {
            let mut request = reject("x", HttpClass::Interactive)(site)?;
            request.url = format!("{}/v1/nowhere", site.url);
            Ok(request)
        },
        None,
    );
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].id, id);
    let status = answers[0].result.as_ref().map(|r| r.status).expect("an answer");
    assert!((400..500).contains(&status), "{status}");    // the actions draw from the device's own source
    let drawn = sim.app.engine().unwrap().with_entropy(|e| {
        let mut b = [0u8; 32];
        e.fill(&mut b).map(|()| b)
    });
    assert!(drawn.is_ok_and(|b| b != [0; 32]));
}

#[test]
fn offline_an_interactive_request_is_given_up_after_two_tries_a_background_one_waits() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    live(&world, &mut sim);
    let mut away = 5u32;
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        if !request.url.ends_with("/reject") || away == 0 {
            return None;
        }
        away -= 1;
        Some(Err(IoError::new(IoErrorKind::Offline)))
    }));
    let group = base64url_encode(world.group.as_bytes());
    let waiting = ask(&mut sim, reject(&group, HttpClass::Interactive), None);
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert_eq!(answers.len(), 1);
    assert_eq!((answers[0].id, answers[0].result.clone().err()), (waiting, Some(Failure::Offline)));
    let background = ask(&mut sim, reject(&group, HttpClass::Background), None);
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].id, background);
    assert_eq!(answers[0].result.as_ref().map(|r| r.status).ok(), Some(200), "tried until it went through");
}

#[test]
fn a_kept_request_is_sent_again_after_a_restart() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    live(&world, &mut sim);
    let group = base64url_encode(world.group.as_bytes());
    sim.hold = true;
    ask(&mut sim, reject(&group, HttpClass::Background), Some("reject/one"));
    assert!(sim.cache.contains_key(&format!("{KEPT_PREFIX}reject/one")), "kept before it is sent");
    let stored = String::from_utf8(sim.cache[&format!("{KEPT_PREFIX}reject/one")].clone()).unwrap();
    assert!(!stored.contains("Bearer token"), "kept without its token: {stored}");
    assert!(sim.held.iter().any(|(_, r)| r.url.ends_with("/reject")));
    // the process dies before the answer
    sim.restart();
    sim.hold = false;
    sim.send(start());
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert_eq!(answers.len(), 1, "{answers:?}");
    assert_eq!(answers[0].keep.as_deref(), Some("reject/one"));
    assert_eq!(answers[0].result.as_ref().map(|r| r.status).ok(), Some(200));
    assert!(answers[0].result.as_ref().unwrap().body.len() < 64);
    assert!(!sim.cache.contains_key(&format!("{KEPT_PREFIX}reject/one")), "deleted once answered");
    assert!(sim.log.iter().all(|e| !format!("{e:?}").contains(&format!("{HUB_URL}/v1/nowhere"))));
}

fn archived_here(sim: &Sim, group: &trommi_core::ids::GroupId) -> bool {
    sim.app.engine().is_some_and(|e| e.device().group(group).is_ok_and(|g| g.archived))
}

#[test]
fn a_session_is_archived_at_the_hub_and_here_and_its_vouched_agent_retired() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    live(&world, &mut sim);
    let agent = world.agent.id();
    assert!(world.hub.borrow().is_agent(&agent));
    let (id, effects) = sim.app.archive_session(sim.now_ms, world.group, vec![agent]);
    sim.run(effects);
    let id = id.expect("a device").expect("sent");
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert_eq!(answers.len(), 1);
    assert_eq!((answers[0].id, answers[0].keep.clone()), (id, Some(format!("archive/{}", hex(world.group.as_bytes())))));
    assert!(world.hub.borrow().is_archived(&world.group));
    assert!(archived_here(&sim, &world.group));
    assert!(!world.hub.borrow().is_agent(&agent), "the vouched agent is out of the room's agents");
    assert!(sim.cache.keys().all(|k| !k.starts_with(KEPT_PREFIX)));
    // the room group cannot be archived, nor a group this device does not hold
    let (refused, _) = sim.app.archive_session(sim.now_ms, world.room, Vec::new());
    assert_eq!(refused, Some(Err("forbidden")));
}

#[test]
fn an_archive_cut_by_a_restart_is_finished_after_it() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    live(&world, &mut sim);
    let agent = world.agent.id();
    sim.hold = true;
    let (_, effects) = sim.app.archive_session(sim.now_ms, world.group, vec![agent]);
    sim.run(effects);
    // the hub takes the archive, the answer is lost with the process
    let (_, request) = sim.held.iter().find(|(_, r)| r.url.ends_with("/archive")).cloned().expect("the archive is sent");
    assert_eq!(world.hub.borrow_mut().http(sim.now_ms, &request).map(|r| r.status).ok(), Some(200));
    assert!(!archived_here(&sim, &world.group));
    sim.restart();
    sim.hold = false;
    sim.send(start());
    let mut answers = Vec::new();
    settle(&world, &mut sim, &mut answers);
    assert!(archived_here(&sim, &world.group), "archived after the restart");
    assert!(!world.hub.borrow().is_agent(&agent), "and the vouched agent retired");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
