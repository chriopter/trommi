//! An agent device's engine (`engine/agent.rs`) against the stand-in hub: the lease and `Trommi-Lease`, the
//! rollback guard, the command gate with its record across a restart, and leaving the room (followed in the order,
//! and read from the hub's removal route when the hub says so first). Its way in (`engine/agent_join.rs`): the room
//! followed from the Offer's epoch, the Welcome check, the past of a group joined late, the wish to be let in again;
//! and its writes, sealed again when the hub voids them for their epoch (`engine/agent_write.rs`).

mod common;

use common::{now, World};
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::Effect;
use trommi_client_core::app::sim::AgentSim;
use trommi_client_core::engine::agent::{NO_HUB_ROUNDS, STREAM_LIVED_MS};
use trommi_client_core::engine::{AgentInvite, AgentWrite, Report, Stopped, LEASE_EVERY_MS, RESEALS};
use trommi_client_core::hub::LinkReport;
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{base64url_encode, DeviceId, GroupId, SessionId};
use trommi_core::invite::{Offer, Role};

/// Sends the agent's open stream what the hub's order holds beyond what it was sent.
fn stream(world: &World, sim: &mut AgentSim) -> bool {
    stream_of(world, sim, &world.agent.id())
}

/// Sends the open stream of the agent device `id` what the hub's order holds beyond what it was sent.
fn stream_of(world: &World, sim: &mut AgentSim, id: &DeviceId) -> bool {
    let news = world.hub.borrow_mut().stream_news(id);
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
fn a_takeover_checks_the_session_removal_after_the_room_enrolment_was_replaced() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let session = world.session;
    let (_, _, _) = invited(&mut world, Some(session));
    invite_steps(&mut world);
    {
        let mut hub = world.hub.borrow_mut();
        let last = |group| hub.order.iter().filter_map(|i| match i {
            common::hub::Item::Log { group: g, n, commit: true, .. } if *g == group => Some(*n),
            _ => None,
        }).max().expect("a Commit");
        let (room, session) = (last(world.room), last(world.group));
        hub.removed_keys.insert(world.agent.id(), [(world.room, room), (world.group, session)].into());
    }
    stream(&world, &mut sim);
    assert_eq!(stopped(&mut sim), Some(Stopped::Replaced));
}

#[test]
fn a_takeover_waits_for_its_session_commit_and_signs_nothing_after_the_room_removal() {
    for started in [false, true] {
        let mut world = World::new();
        let mut sim = world.agent_sim();
        if started { sim.start(); }
        let session = world.session;
        let _ = invited(&mut world, Some(session));
        {
            let mut hub = world.hub.borrow_mut();
            let room = hub.order.iter().filter_map(|i| match i {
                common::hub::Item::Log { group, n, commit: true, .. } if *group == world.room => Some(*n),
                _ => None,
            }).max().expect("a room Commit");
            hub.removed_keys.insert(world.agent.id(), [(world.room, room)].into());
        }
        if started { stream(&world, &mut sim); } else { sim.start(); }
        assert_eq!(stopped(&mut sim), None, "the replacement Commit is not posted yet");
        assert!(!sim.engine().agent_signs(), "the verified room removal already stopped signing");
        assert_eq!(sim.with(|engine, ctx| engine.agent_send(ctx, session_line(&world, "nothing more"), &[])), Err(trommi_core::Error::Busy));
        invite_steps(&mut world);
        {
            let mut hub = world.hub.borrow_mut();
            let session = hub.order.iter().filter_map(|i| match i {
                common::hub::Item::Log { group, n, commit: true, .. } if *group == world.group => Some(*n),
                _ => None,
            }).max().expect("the replacement Commit");
            hub.removed_keys.get_mut(&world.agent.id()).expect("a removed key").insert(world.group, session);
        }
        sim.advance(1_000);
        assert_eq!(stopped(&mut sim), Some(Stopped::Replaced));
    }
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
fn a_room_the_hub_no_longer_knows_stops_the_agent_without_erasing_its_state() {
    let world = World::new();
    let mut sim = world.agent_sim();
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/tokens").then(|| Ok(common::hub::answer(
            trommi_client_core::hub::status_of("no-room").expect("a protocol refusal"),
            serde_json::json!({ "error": "no-room", "message": "no such room" }),
        )))
    }));
    let revision = sim.io.device.0;
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::NotMember));
    assert_eq!(sim.io.device.0, revision, "the hub's word never erases the device");
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

// ---- the way in

/// A new agent device that answered an invite of the founder (for `session`: a takeover; or for a new session) and
/// follows no room yet; its KeyPackages are at the hub, the founder's Commit that enrols it is posted. Returns its
/// stored state, its id and what its connector stored.
fn invited(world: &mut World, session: Option<SessionId>) -> (common::TestStore, DeviceId, AgentInvite) {
    let (mut agent, store) = common::new_peer();
    let mut hub = world.hub.borrow_mut();
    agent.key_packages_to_upload(0, now()).expect("key packages");
    assert!(common::post_all(&mut hub, &mut agent).is_empty());
    common::sync(&hub, &mut world.founder);
    let address = HubAddress::parse(common::hub::HUB_URL).expect("an address");
    let opened = world.founder.invite_open(Role::Agent, session.as_ref(), "https://app.example", &address, now()).expect("an invite");
    let link = String::from_utf8(opened.link.expose().to_vec()).expect("the link is text");
    let request = agent.join_request(&link, &opened.signed_offer, now()).expect("a request").signed_request;
    let accepted = world.founder.invite_accept(&opened.invite_id, &request, now()).expect("accepted");
    let code = agent.join_reveal(&accepted.signed_reveal).expect("the code");
    world.founder.invite_confirm(&opened.invite_id, &code, &accepted.request_hash, true, now()).expect("confirmed").expect("a Commit");
    assert!(common::post_all(&mut hub, &mut world.founder).is_empty());
    let offer = Offer::decode(&opened.signed_offer.offer).expect("an offer");
    let invite = AgentInvite { room: world.room.room_id(), room_epoch: offer.room_epoch, session };
    (store, agent.id(), invite)
}

/// The founder founds a new main session for `agent`. Returns its group.
fn found_for(world: &mut World, agent: DeviceId) -> GroupId {
    let mut hub = world.hub.borrow_mut();
    common::sync(&hub, &mut world.founder);
    let packages = hub.claim(&[world.me, agent]).expect("a KeyPackage of each");
    let session = world.founder.found_session(&agent, &packages, now()).expect("the founding is built");
    assert!(common::post_all(&mut hub, &mut world.founder).is_empty());
    GroupId::session(world.room.room_id(), session)
}

/// The founder takes the steps its invites owe (a takeover, a key handover), until none is left.
fn invite_steps(world: &mut World) {
    use trommi_core::device::InviteStep;
    for _ in 0..8 {
        let mut hub = world.hub.borrow_mut();
        common::sync(&hub, &mut world.founder);
        let steps = world.founder.invite_steps().expect("the steps");
        if steps.is_empty() {
            return;
        }
        for (invite, step) in steps {
            match step {
                InviteStep::TakeOver { group, cuts, agent, key_package } => {
                    let package = key_package.unwrap_or_else(|| hub.claim(&[agent]).expect("a KeyPackage").remove(0));
                    world.founder.clean_session(&group, &cuts, Some((&agent, &package)), now()).expect("the takeover is built");
                }
                InviteStep::Handover { .. } => {
                    world.founder.invite_handover(&invite).expect("the handover");
                }
                InviteStep::CheckHelpers { .. } => {
                    let _ = world.founder.invite_checked(&invite, &[]);
                }
                _ => {}
            }
            assert_eq!(common::post_all(&mut hub, &mut world.founder), Vec::<String>::new());
        }
    }
}

/// Streams what the hub holds for the agent until nothing is new, with the clock moved a little in between.
fn settle(world: &World, sim: &mut AgentSim) {
    let id = sim.engine().device().id();
    for _ in 0..12 {
        stream_of(world, sim, &id);
        sim.advance(1_000);
    }
}

fn texts(sim: &AgentSim) -> Vec<String> {
    sim.reports
        .iter()
        .filter_map(|r| match r {
            Report::Envelope { received, .. } => received.body.as_ref().map(|b| String::from_utf8_lossy(b.payload()).into_owned()),
            _ => None,
        })
        .filter_map(|p| serde_json::from_str::<serde_json::Value>(&p).ok()?.get("text")?.as_str().map(str::to_string))
        .collect()
}

#[test]
fn an_agent_reads_its_welcome_when_the_session_was_founded_before_its_stream_opened() {
    let mut world = World::new();
    let (store, id, invite) = invited(&mut world, None);
    let group = found_for(&mut world, id);
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite);
    sim.start();
    // No live stream notification: the founding is already in the hub's order at startup.
    for _ in 0..12 {
        sim.advance(1_000);
    }
    assert_eq!(stopped(&mut sim), None);
    assert_eq!(sim.engine().main_group(), Some(group));
}

#[test]
fn a_new_agent_follows_the_room_from_the_offers_epoch_and_learns_the_past_of_what_it_joins() {
    let mut world = World::new();
    let (store, id, invite) = invited(&mut world, None);
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite.clone());
    sim.start();
    let room = base64url_encode(world.room.as_bytes());
    let seen = seen(&world);
    let link = seen.iter().position(|s| s == "POST /v1/link").expect("the lease");
    assert_eq!(seen[link + 1], format!("GET /v1/groups/{room}/info?epoch={}", invite.room_epoch), "the room at the Offer's epoch, first: {seen:?}");
    assert!(seen[..link + 1].iter().all(|s| !s.starts_with("GET /v1/changes")), "{seen:?}");
    assert_eq!(stopped(&mut sim), None);
    let device = sim.engine().device();
    assert!(device.room_history().is_some_and(|h| h.newest().is_agent(&id)), "enrolled by its inviter's Commit");
    assert!(!device.group_past(&world.room).expect("the room").expect("followed").learned, "from the Offer's epoch on");
    // a session for it: the Welcome, then the room's past (the session's own begins at its founding)
    let group = found_for(&mut world, id);
    settle(&world, &mut sim);
    assert_eq!(stopped(&mut sim), None);
    assert_eq!(sim.engine().main_group(), Some(group));
    assert!(sim.engine().device().group_past(&world.room).expect("the room").expect("followed").learned, "the room's past is learned");
    let seen = self::seen(&world);
    assert!(seen.iter().any(|s| s == &format!("GET /v1/groups/{room}/info?epoch=0")), "{seen:?}");
    assert!(seen.iter().any(|s| s.starts_with(&format!("GET /v1/groups/{room}/log?")) && s.contains("kind=commit")), "{seen:?}");
    // and it reads its session in the hub's order
    world.founder_writes(&trommi_core::device::Draft::SessionChat {
        session: group.session_id().expect("a session"),
        payload: common::text(r#"{"content_type":"message","text":"hello new agent"}"#),
    });
    settle(&world, &mut sim);
    assert!(texts(&sim).iter().any(|t| t == "hello new agent"), "{:?}", texts(&sim));
}

#[test]
fn an_agent_that_takes_a_session_over_learns_its_past_and_reads_what_was_written_before() {
    let mut world = World::new();
    world.founder_says("before the takeover");
    world.agent_says("the first agent's line");
    let (store, id, invite) = { let session = world.session; invited(&mut world, Some(session)) };
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite);
    sim.start();
    assert_eq!(stopped(&mut sim), None);
    invite_steps(&mut world);
    settle(&world, &mut sim);
    invite_steps(&mut world);
    settle(&world, &mut sim);
    assert_eq!(stopped(&mut sim), None, "{:?}", world.hub.borrow().refused);
    assert_eq!(sim.engine().main_group(), Some(world.group), "the invited session");
    let past = sim.engine().device().group_past(&world.group).expect("the session").expect("held");
    assert!(past.from_epoch > 0 && past.learned, "joined late, its past learned: {past:?}");
    let seen = seen(&world);
    let group = base64url_encode(world.group.as_bytes());
    assert!(seen.iter().any(|s| s == &format!("GET /v1/groups/{group}/info?epoch=0")), "{seen:?}");
    let texts = texts(&sim);
    assert!(texts.iter().any(|t| t == "before the takeover") && texts.iter().any(|t| t == "the first agent's line"), "{texts:?}");
    let _ = id;
}

#[test]
fn a_welcome_for_another_group_than_the_hub_named_is_not_kept() {
    let mut world = World::new();
    let (store, id, invite) = invited(&mut world, None);
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite);
    sim.start();
    let group = found_for(&mut world, id);
    // the hub names another group for it
    let named = world.group;
    world.hub.borrow_mut().welcomes.iter_mut().filter(|w| w.device == id).for_each(|w| w.group = named);
    settle(&world, &mut sim);
    assert_eq!(stopped(&mut sim), Some(Stopped::BadWelcome));
    let stored = world.reopen_me(&sim.io);
    assert!(stored.group(&group).is_err() && stored.groups().expect("groups").is_empty(), "nothing of it was written");
}

#[test]
fn a_welcome_into_another_main_session_than_the_invites_is_not_kept() {
    let mut world = World::new();
    let (store, id, invite) = { let session = world.session; invited(&mut world, Some(session)) };
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite);
    sim.start();
    assert_eq!(stopped(&mut sim), None);
    // instead of the takeover, a new session
    let group = found_for(&mut world, id);
    settle(&world, &mut sim);
    assert_eq!(stopped(&mut sim), Some(Stopped::BadWelcome));
    assert!(world.reopen_me(&sim.io).group(&group).is_err(), "nothing of it was written");
}

#[test]
fn an_agent_passes_over_a_welcome_named_for_the_room_group() {
    let mut world = World::new();
    let (store, id, invite) = invited(&mut world, None);
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite);
    sim.start();
    let group = found_for(&mut world, id);
    let room = world.room;
    world.hub.borrow_mut().welcomes.iter_mut().filter(|w| w.device == id).for_each(|w| w.group = room);
    settle(&world, &mut sim);
    assert_eq!(stopped(&mut sim), None);
    assert!(sim.engine().device().group(&group).is_err(), "not taken");
    assert!(world.hub.borrow().wishes.is_empty(), "and nothing asked for it");
}

#[test]
fn a_room_state_at_the_offers_epoch_that_is_not_the_offers_stops_it() {
    let mut world = World::new();
    let (store, _, mut invite) = invited(&mut world, None);
    // the connector's record names another epoch: the GroupInfo served there is not the state the Offer named
    invite.room_epoch -= 1;
    let mut sim = AgentSim::invited(world.sim_of(&store), common::hub::HUB_URL, invite);
    let revision = sim.io.device.0;
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::BadInvite));
    assert_eq!(sim.io.device.0, revision, "nothing of the device was written");
    assert!(seen(&world).iter().all(|s| !s.starts_with("GET /v1/changes")));
}

#[test]
fn a_welcome_that_does_not_open_asks_once_to_be_let_in_again() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let agent = world.agent.id();
    let group = GroupId::session(world.room.room_id(), SessionId::new([0x42; 16]));
    for _ in 0..2 {
        {
            let mut hub = world.hub.borrow_mut();
            let id = 9_000 + hub.welcomes.len() as u64;
            hub.welcomes.push(common::hub::Welcome { id, group, bytes: vec![1, 2, 3], device: agent });
            hub.note(vec![agent], "welcome", serde_json::json!({ "group_id": base64url_encode(group.as_bytes()) }));
        }
        settle(&world, &mut sim);
    }
    assert_eq!(stopped(&mut sim), None);
    let hub = world.hub.borrow();
    let wishes: Vec<_> = hub.wishes.iter().filter(|(_, d, kind, g, _)| *d == agent && kind == "readmit" && *g == Some(group)).collect();
    assert_eq!(wishes.len(), 1, "once: {:?}", hub.wishes);
    assert!(hub.wish_packages.is_empty(), "an agent device asks without a KeyPackage");
}

// ---- its writes

fn session_line(world: &World, text: &str) -> AgentWrite {
    AgentWrite::SessionChat { session: world.session.to_base64url(), payload: format!(r#"{{"content_type":"message","text":"{text}"}}"#) }
}

#[test]
fn nothing_is_sealed_before_the_lease_and_the_guard() {
    let world = World::new();
    let mut sim = world.agent_sim();
    let write = session_line(&world, "too early");
    let refused = sim.with(|engine, ctx| engine.agent_send(ctx, write, &[]));
    assert_eq!(refused, Err(trommi_core::Error::Busy));
    assert!(sim.engine().device().outbox().is_empty());
}

#[test]
fn an_envelope_the_hub_voided_for_its_epoch_is_sealed_again_and_the_founder_reads_it_once() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    world.hub.borrow_mut().voids = 1;
    let write = session_line(&world, "sealed twice");
    let sealed = sim.with(|engine, ctx| engine.agent_send(ctx, write, &[])).expect("sealed");
    settle(&world, &mut sim);
    let resealed: Vec<(u64, u64, u64)> = sim.reports.iter().filter_map(|r| match r { Report::Resealed { first, voided, id, .. } => Some((*first, *voided, *id)), _ => None }).collect();
    assert_eq!(resealed.len(), 1, "{resealed:?}");
    let (first, voided, id) = resealed[0];
    assert_eq!((first, voided), (sealed.outbox_id, sealed.outbox_id));
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Accepted { id: taken, .. } if *taken == id)));
    assert!(!sim.reports.iter().any(|r| matches!(r, Report::Refused { .. })), "the sender hears nothing of the void");
    assert!(sim.engine().device().outbox().is_empty());
    // the founder reads it once, after the void record in its place
    world.peers_sync();
    world.founder_says("seen it");
    let hub = world.hub.borrow();
    let voids = hub.order.iter().filter(|i| matches!(i, common::hub::Item::Envelope { void_code: Some(_), .. })).count();
    assert_eq!(voids, 1);
}

#[test]
fn an_envelope_voided_again_and_again_is_given_up_after_the_last_reseal() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    world.hub.borrow_mut().voids = 100;
    let write = session_line(&world, "never");
    let sealed = sim.with(|engine, ctx| engine.agent_send(ctx, write, &[])).expect("sealed");
    settle(&world, &mut sim);
    let resealed: Vec<(u64, u64)> = sim.reports.iter().filter_map(|r| match r { Report::Resealed { first, id, .. } => Some((*first, *id)), _ => None }).collect();
    assert_eq!(resealed.len(), RESEALS as usize, "{resealed:?}");
    assert!(resealed.iter().all(|(first, _)| *first == sealed.outbox_id));
    let last = resealed.last().unwrap().1;
    let refused: Vec<_> = sim.reports.iter().filter_map(|r| match r { Report::Refused { id, code, voided, .. } => Some((*id, *code, *voided)), _ => None }).collect();
    assert_eq!(refused, [(last, "wrong-epoch", true)]);
    assert_eq!(world.hub.borrow().voids, 100 - 1 - RESEALS as u32);
    assert!(sim.engine().device().outbox().is_empty());
}

#[test]
fn the_note_beside_an_envelope_outlives_a_restart_and_a_void_after_it_is_sealed_again() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    // sealed, and the process dies before the hub answered
    sim.io.hold = true;
    let write = session_line(&world, "across a restart");
    let sealed = sim.with(|engine, ctx| engine.agent_send(ctx, write, &[])).expect("sealed");
    sim.io.hold = false;
    sim.io.held.clear();
    sim.restart();
    sim.reports.clear();
    world.hub.borrow_mut().voids = 1;
    sim.start();
    settle(&world, &mut sim);
    let resealed: Vec<(u64, u64)> = sim.reports.iter().filter_map(|r| match r { Report::Resealed { first, voided, .. } => Some((*first, *voided)), _ => None }).collect();
    assert_eq!(resealed, [(sealed.outbox_id, sealed.outbox_id)]);
    assert!(sim.engine().device().outbox().is_empty(), "posted after the restart");
}

#[test]
fn nothing_more_is_sealed_while_the_hub_refuses_an_envelope_for_good() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    // the hub refuses the envelope without a void record: its number is not used up, the pump halts on it
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        (request.method == "POST" && request.url.ends_with("/v1/envelopes"))
            .then(|| Ok(common::hub::answer(403, serde_json::json!({ "error": "forbidden", "message": "refused", "voided": false }))))
    }));
    let first = session_line(&world, "refused for now");
    sim.with(|engine, ctx| engine.agent_send(ctx, first, &[])).expect("sealed");
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Blocked(Some(_)))));
    let second = session_line(&world, "behind it");
    assert_eq!(sim.with(|engine, ctx| engine.agent_send(ctx, second, &[])).err(), Some(trommi_core::Error::ChainBreak));
    assert_eq!(sim.engine().device().outbox().len(), 1, "nothing sealed behind it");
    // the hub takes it at the next try: writing goes on
    world.hub.borrow_mut().fault = None;
    sim.advance(trommi_client_core::engine::sync::BLOCKED_RETRY_MS);
    assert!(sim.engine().device().outbox().is_empty());
    let third = session_line(&world, "after it");
    sim.with(|engine, ctx| engine.agent_send(ctx, third, &[])).expect("sealed again");
}

/// What answers at the hub's address is a web server's page, not the protocol's.
fn no_hub(world: &World) {
    world.hub.borrow_mut().fault = Some(Box::new(|_, _| {
        Some(Ok(trommi_client_core::app::io::HttpResponse { status: 404, headers: Vec::new(), body: b"<html>not here</html>".to_vec().into() }))
    }));
}

#[test]
fn no_hub_of_this_protocol_at_the_first_round_stops_it_at_once() {
    let world = World::new();
    let mut sim = world.agent_sim();
    no_hub(&world);
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::HubUnusable));
}

#[test]
fn no_hub_of_this_protocol_round_after_round_stops_it_after_the_last() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    assert_eq!(stopped(&mut sim), None);
    no_hub(&world);
    let mut rounds = 0;
    for _ in 0..40 {
        // (the stand-in does not see what the page answers: the requests are counted as they leave)
        let links = |sim: &AgentSim| leases(sim).iter().filter(|(_, url, _)| url.ends_with("/v1/link")).count();
        let before = links(&sim);
        sim.advance(LEASE_EVERY_MS);
        rounds += links(&sim) - before;
        if stopped(&mut sim).is_some() {
            break;
        }
        assert!(rounds < NO_HUB_ROUNDS as usize, "{rounds} rounds and still asking");
    }
    assert_eq!(stopped(&mut sim), Some(Stopped::HubUnusable));
    assert_eq!(rounds, NO_HUB_ROUNDS as usize);
}

#[test]
fn a_stream_that_ends_at_once_waits_longer_each_time_and_one_that_lived_starts_small_again() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let streams = |world: &World| seen(world).iter().filter(|s| s.starts_with("STREAM")).count();
    let n = streams(&world);
    sim.stream(StreamItem::Ended);
    sim.advance(1_000);
    assert_eq!(streams(&world), n + 1, "after a second");
    sim.stream(StreamItem::Ended);
    sim.advance(1_000);
    assert_eq!(streams(&world), n + 1, "the second pause is longer");
    sim.advance(1_500);
    assert_eq!(streams(&world), n + 2);
    // this one works for half a minute: the pause after it is small again
    sim.advance(STREAM_LIVED_MS);
    sim.stream(StreamItem::Ended);
    sim.advance(1_000);
    assert_eq!(streams(&world), n + 3);
}

/// The Commits of a group's log as the hub's removal route serves them.
fn log_items(hub: &common::hub::Hub, group: &GroupId) -> Vec<trommi_client_core::hub::LogItem> {
    hub.order
        .iter()
        .filter_map(|item| match item {
            common::hub::Item::Log { change, group: g, n, epoch, commit: true, bytes, recovery_auth, from } if g == group => Some(trommi_client_core::hub::LogItem {
                change: *change,
                group: *g,
                n: *n,
                epoch: *epoch,
                at: 0,
                bytes: bytes.clone(),
                sender: *from,
                recovery_auth: recovery_auth.clone(),
            }),
            _ => None,
        })
        .collect()
}

#[test]
fn an_agent_archives_a_session_group_it_holds_and_retires_nobody() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let me = world.agent.id();
    let refused = sim.with(|engine, ctx| engine.archive_session(ctx, world.group, vec![me]));
    assert_eq!(refused, Err("forbidden"), "an agent retires no device");
    let room = world.room;
    assert_eq!(sim.with(|engine, ctx| engine.archive_session(ctx, room, Vec::new())), Err("forbidden"));
    let id = sim.with(|engine, ctx| engine.archive_session(ctx, world.group, Vec::new())).expect("asked");
    assert!(seen(&world).iter().any(|s| s.starts_with("POST /v1/groups/") && s.ends_with("/archive")), "{:?}", seen(&world));
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Answer(a) if a.id == id)));
    assert!(sim.engine().device().group(&world.group).expect("held").archived, "archived on the device once the hub took it");
}

#[test]
fn commits_handed_in_from_outside_verify_a_removal_only_when_they_take_the_device_out() {
    let mut world = World::new();
    let mut sim = world.agent_sim();
    let agent = world.agent.id();
    // the hub says this key is out and serves nothing of it: out by its word alone
    world.hub.borrow_mut().removed_keys.insert(agent, Default::default());
    sim.start();
    assert_eq!(stopped(&mut sim), Some(Stopped::NotMember));
    assert!(!sim.with(|engine, ctx| engine.verify_removal(ctx, Vec::new())), "nothing verifies nothing");
    assert_eq!(stopped(&mut sim), Some(Stopped::NotMember));
    // the Commit that took it out, read by another way: verified by the device itself
    let items = {
        let mut hub = world.hub.borrow_mut();
        common::sync(&hub, &mut world.founder);
        world.founder.remove_agents(&[agent], now()).expect("the Commit is built");
        assert!(common::post_all(&mut hub, &mut world.founder).is_empty());
        log_items(&hub, &world.room)
    };
    assert!(sim.with(|engine, ctx| engine.verify_removal(ctx, items)));
    assert_eq!(stopped(&mut sim), Some(Stopped::Removed));
    assert!(sim.reports.iter().any(|r| matches!(r, Report::Stopped(Stopped::Removed))));
}

#[test]
fn a_device_that_is_in_stays_in_whatever_list_it_is_handed() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    let mut items = log_items(&world.hub.borrow(), &world.room);
    for item in &mut items {
        item.change += 1000;
    }
    assert!(!sim.with(|engine, ctx| engine.verify_removal(ctx, items)));
    assert_eq!(stopped(&mut sim), None);
    assert!(sim.engine().agent_signs());
}

#[test]
fn an_agents_write_is_read_back_at_its_place_without_a_stream() {
    let world = World::new();
    let mut sim = world.agent_sim();
    sim.start();
    // the stream ends and is not open again before the write is taken
    sim.stream(StreamItem::Ended);
    let write = session_line(&world, "read back");
    let sealed = sim.with(|engine, ctx| engine.agent_send(ctx, write, &[])).expect("sealed");
    let change = sim.reports.iter().find_map(|r| match r { Report::Accepted { id, change, .. } if *id == sealed.outbox_id => *change, _ => None }).expect("taken");
    assert!(sim.engine().position() >= change, "its place was read: {} < {change}", sim.engine().position());
    assert!(texts(&sim).iter().any(|t| t == "read back"), "{:?}", texts(&sim));
}

#[test]
fn a_round_that_did_not_reach_the_hub_is_said_to_fail() {
    let world = World::new();
    let mut sim = world.agent_sim();
    assert!(!sim.engine().failing());
    world.hub.borrow_mut().fault = Some(Box::new(|_, _| Some(Err(trommi_client_core::app::io::IoError::new(trommi_client_core::app::io::IoErrorKind::Offline)))));
    sim.start();
    assert!(sim.engine().failing(), "the sign-in waits for its next try");
    assert!(!sim.engine().agent_signs());
}
