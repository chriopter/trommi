//! The account's steps of a device in its room (`app::account_steps`): signing out wipes what the slot stored and
//! ends the token at the hub; removing an agent device takes it out of the room.

mod common;

use common::{start, World};
use trommi_client_core::account::AccountAction;
use trommi_client_core::actions::Action;
use trommi_client_core::app::Event;
use trommi_client_core::views::Phase;

#[test]
fn signing_out_wipes_the_slot() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(!sim.device.1.is_empty() && !sim.cache.is_empty());
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.view.account.notice.as_ref().map(|t| t.en.as_str()), Some("Logged out."), "the start screen says so");
    assert!(sim.app.engine().is_none());
    assert!(sim.device.1.is_empty(), "the device's state is gone");
    assert!(sim.cache.is_empty(), "the cache is gone: {:?}", sim.cache.keys().collect::<Vec<_>>());
    assert!(world.hub.borrow().seen.iter().any(|s| s == "DELETE /v1/token"), "the token ends at the hub");
    // a start over the wiped slot shows the sign-in
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
}

#[test]
fn an_agent_device_is_removed() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let agent = world.agent.id();
    let (_, agents) = sim.app.engine().unwrap().roles();
    assert!(agents.contains(&agent));
    let id: String = agent.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    sim.send(Event::Action { action: Action::Account { action: AccountAction::RemoveDevices { device_ids: vec![id] } } });
    for _ in 0..5 {
        world.stream(&mut sim);
        sim.advance(1_000);
    }
    let (_, agents) = sim.app.engine().unwrap().roles();
    assert!(!agents.contains(&agent), "{:?}", world.hub.borrow().refused);
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
}

#[test]
fn hub_data_is_account_authenticated_core_presented_and_forgotten() {
    use trommi_client_core::app::io::HttpResponse;
    let world = World::new();
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/v1/account/usage").then(|| {
            assert_eq!(request.method, "GET");
            assert!(request.headers.iter().any(|header| header.name.eq_ignore_ascii_case("authorization") && !header.value.is_empty()));
            Ok(HttpResponse { status: 200, headers: vec![], body: br#"{"rooms":1,"groups":2,"live_groups":3,"devices":4,"commits":5,"messages":6,"envelopes":7,"cards":8,"open_cards":9,"files":10,"file_bytes":11,"log_bytes":12,"envelope_bytes":13,"total_bytes":14}"#.to_vec().into() })
        })
    }));
    let mut sim = world.sim();
    sim.send(start());
    sim.send(Event::Action { action: Action::Account { action: AccountAction::ReadUsage } });
    let data = sim.view.settings.data.clone().expect("Settings → Data");
    assert_eq!(data.state, "ready");
    assert_eq!(data.rows.len(), 14);
    assert_eq!(data.rows[0].id, "rooms");
    assert_eq!(data.rows[0].value.en, "1");
    assert_eq!(data.rows[13].id, "total_bytes");
    assert_eq!(data.rows[13].value.en, "14 bytes");
    sim.advance(1_000);
    assert_eq!(sim.view.settings.data.as_ref(), Some(&data), "render retains the account read");
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert!(sim.view.settings.data.is_none());
    let seen = world.hub.borrow().seen.len();
    sim.send(Event::Action { action: Action::Account { action: AccountAction::ReadUsage } });
    assert_eq!(world.hub.borrow().seen.len(), seen, "signed out sends no account request");
    assert_eq!(sim.view.settings.data.as_ref().unwrap().state, "failed");
    assert!(sim.view.settings.data.as_ref().unwrap().rows.is_empty());
}

#[test]
fn invalid_hub_counts_never_become_data_rows() {
    use trommi_client_core::app::io::HttpResponse;
    let world = World::new();
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/v1/account/usage").then(|| Ok(HttpResponse {
            status: 200, headers: vec![], body: br#"{"rooms":-1,"email":"not a count"}"#.to_vec().into(),
        }))
    }));
    let mut sim = world.sim();
    sim.send(start());
    sim.send(Event::Action { action: Action::Account { action: AccountAction::ReadUsage } });
    let data = sim.view.settings.data.as_ref().unwrap();
    assert_eq!(data.state, "failed");
    assert!(data.rows.is_empty());
    assert_eq!(data.says.as_ref().unwrap().key, "data.failed");
}

fn deliver_to(world: &World, sim: &mut trommi_client_core::app::sim::Sim, device: trommi_core::ids::DeviceId) {
    let bytes = world.hub.borrow_mut().stream_news(&device);
    if let Some(bytes) = bytes {
        sim.stream(trommi_client_core::app::io::StreamItem::Chunk { bytes: bytes.into() });
    }
    sim.advance(1_000);
}

#[test]
fn logout_retires_an_unfinished_device_through_another_humans_verified_commit() {
    let mut world = World::new();
    let (store, leaving, _) = world.newcomer();
    let mut keeper = world.sim();
    keeper.send(start());
    let id: String = leaving.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    assert!(keeper.view.settings.devices.iter().any(|d| d.id == id && d.state == "unfinished"));
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        (request.method == "POST" && request.url.ends_with("/v1/envelopes")).then(|| Ok(trommi_client_core::app::io::HttpResponse { status:503, headers:vec![], body:Vec::new().into() }))
    }));
    let mut unfinished = world.sim_of(&store);
    unfinished.send(start());
    assert!(unfinished.app.engine().is_some());
    unfinished.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(unfinished.view.phase, Phase::SignedOut);
    assert!(unfinished.device.1.is_empty() && unfinished.cache.is_empty());
    assert!(world.hub.borrow().departures.contains_key(&leaving), "signed departure is emitted despite the unsent profile and local wipe");
    world.hub.borrow_mut().fault = None;
    for _ in 0..20 {
        deliver_to(&world, &mut keeper, world.me);
        if !keeper.app.engine().unwrap().roles().0.contains(&leaving) { break; }
    }
    assert!(!keeper.app.engine().unwrap().roles().0.contains(&leaving), "{:?}", world.hub.borrow().refused);
    assert!(!keeper.view.settings.devices.iter().any(|d| d.id == id), "logout must not leave an away or unfinished active device");
    assert!(keeper.view.settings.removed.iter().any(|d| d.id == id));
}

#[test]
fn a_queued_logout_is_replayed_to_the_next_human_app() {
    let mut world = World::new();
    let mut leaving = world.sim();
    leaving.send(start());
    leaving.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert!(world.hub.borrow().departures.contains_key(&world.me));
    // No other app was running. A later password/code join starts its stream and sees the signed intent.
    let (store, new_device, _) = world.newcomer();
    let mut next = world.sim_of(&store);
    next.send(start());
    for _ in 0..20 {
        deliver_to(&world, &mut next, new_device);
        if !next.app.engine().unwrap().roles().0.contains(&world.me) { break; }
    }
    assert!(!next.app.engine().unwrap().roles().0.contains(&world.me), "{:?}", world.hub.borrow().refused);
    assert!(next.app.engine().unwrap().roles().0.contains(&new_device));
}

#[test]
fn cancelling_an_unfinished_sign_in_can_retire_its_key_without_a_bearer() {
    let mut world = World::new();
    let (store, leaving, _) = world.newcomer();
    let mut keeper = world.sim();
    keeper.send(start());
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/challenge").then(|| Ok(trommi_client_core::app::io::HttpResponse { status:503, headers:vec![], body:Vec::new().into() }))
    }));
    let mut interrupted = world.sim_of(&store);
    interrupted.send(start());
    assert_ne!(interrupted.view.phase, Phase::Live);
    assert!(interrupted.app.engine().is_some());
    interrupted.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(interrupted.view.phase, Phase::SignedOut);
    assert!(world.hub.borrow().departures.contains_key(&leaving), "retirement must not wait for the token whose sign-in failed");
    world.hub.borrow_mut().fault = None;
    for _ in 0..20 {
        deliver_to(&world, &mut keeper, world.me);
        if !keeper.app.engine().unwrap().roles().0.contains(&leaving) { break; }
    }
    assert!(!keeper.app.engine().unwrap().roles().0.contains(&leaving));
}

#[test]
fn a_hostile_hub_cannot_retire_humans_with_forged_or_agent_departures() {
    use trommi_core::ids::base64url_encode as b64;
    let mut world = World::new();
    let (_, other, _) = world.newcomer();
    let mut keeper = world.sim();
    keeper.send(start());
    let before = keeper.app.engine().unwrap().roles();
    let proof = world.agent.sign_departure_for(&world.room.room_id()).unwrap();
    for (device, signature) in [(other, vec![0;64]), (world.agent.id(), proof)] {
        let data = serde_json::json!({"device":device.to_base64url(),"signature":b64(&signature)});
        let bytes = format!("event: departure\ndata: {data}\n\n").into_bytes();
        assert!(keeper.stream(trommi_client_core::app::io::StreamItem::Chunk { bytes:bytes.into() }));
        for _ in 0..3 { deliver_to(&world, &mut keeper, world.me); }
    }
    assert_eq!(keeper.app.engine().unwrap().roles(), before);
}

#[test]
fn offline_logout_keeps_only_its_public_notice_and_retries_after_restart_without_keys() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/departures").then(|| Err(trommi_client_core::app::io::IoError::new(trommi_client_core::app::io::IoErrorKind::Offline)))
    }));
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert!(sim.device.1.is_empty());
    assert_eq!(sim.cache.len(), 1, "all private account cache is wiped; just one public notice waits");
    let (key, bytes) = sim.cache.iter().next().unwrap();
    assert!(key.starts_with("public-departure/"));
    let notice: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let fields: Vec<_> = notice.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(fields, ["device","hub","room","signature"], "no bearer, recovery code or content key is retained");
    assert_eq!(notice["device"], world.me.to_base64url());
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert!(sim.app.engine().is_none());
    assert_eq!(sim.cache.len(), 1);
    world.hub.borrow_mut().fault = None;
    sim.send(Event::Net { online:true });
    assert!(world.hub.borrow().departures.contains_key(&world.me));
    assert!(sim.device.1.is_empty() && sim.cache.is_empty());
    assert!(sim.app.engine().is_none());
}

#[test]
fn a_crash_after_the_public_notice_was_synced_finishes_local_key_erasure_on_restart() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    let before_wipe = sim.device.clone();
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/departures").then(|| Err(trommi_client_core::app::io::IoError::new(trommi_client_core::app::io::IoErrorKind::Offline)))
    }));
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(sim.cache.len(), 1);
    // Replay the durable boundary: the public notice is on disk, but the process died before the
    // following DeviceApply erased its old state. No new private state may be opened for the person.
    sim.device = before_wipe;
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert!(sim.device.1.is_empty() && sim.app.engine().is_none());
    assert_eq!(sim.cache.len(), 1);
}

#[test]
fn logout_during_the_initial_device_load_retires_and_erases_that_device_once_loaded() {
    let world = World::new();
    let mut sim = world.sim();
    let effects = sim.app.handle(common::now(), start());
    let (loads, other): (Vec<_>,Vec<_>) = effects.into_iter().partition(|e| matches!(e, trommi_client_core::app::Effect::Store { op:trommi_client_core::app::io::StoreOp::DeviceLoad, .. }));
    assert_eq!(loads.len(), 1);
    sim.run(other);
    assert!(sim.app.engine().is_none());
    sim.send(Event::Action { action:Action::Account { action:AccountAction::SignOut } });
    sim.run(loads);
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert!(sim.device.1.is_empty() && sim.app.engine().is_none());
    assert!(world.hub.borrow().departures.contains_key(&world.me));
}

#[test]
fn durable_departures_bound_in_flight_requests_and_keep_new_logout_writes_ahead_of_erasure() {
    use trommi_client_core::app::{App, Effect, StoreAnswer, StoreOp, Entry, IoResult};
    let world = World::new();
    let room = world.room.room_id();
    let mut entries = Vec::new();
    for _ in 0..12 {
        let (device, _) = common::new_peer();
        let signature = device.sign_departure_for(&room).unwrap();
        let key = format!("public-departure/{}.{}", room.to_base64url(), device.id().to_base64url());
        let value = serde_json::to_vec(&serde_json::json!({"hub":"https://hub.example", "room":room.to_base64url(), "device":device.id().to_base64url(), "signature":signature})).unwrap().into();
        entries.push(Entry { key, value });
    }
    let mut app = App::new();
    let effects = app.handle(common::now(), start());
    let scan = effects.iter().find_map(|e| match e { Effect::Store { id, op:StoreOp::Scan { prefix } } if prefix == "public-departure/" => Some(*id), _ => None }).unwrap();
    let load = effects.iter().find_map(|e| match e { Effect::Store { id, op:StoreOp::DeviceLoad } => Some(*id), _ => None }).unwrap();
    let effects = app.handle(common::now(), Event::Store { id:scan, result:IoResult::Ok(StoreAnswer::Entries { entries }) });
    let requests: Vec<_> = effects.iter().filter_map(|e| match e { Effect::Http { id, .. } => Some(*id), _ => None }).collect();
    assert_eq!(requests.len(), 8, "only eight durable notices are in flight");
    let wake = app.handle(common::now(), Event::Wake);
    assert!(!wake.iter().any(|e| matches!(e, Effect::Http { .. })), "waiting notices do not bypass the limit");
    let stored = world.sim().device;
    app.handle(common::now(), Event::Store { id:load, result:IoResult::Ok(StoreAnswer::Device {
        revision:stored.0, entries:stored.1.into_iter().map(|(key,value)| trommi_client_core::app::DeviceEntry { key:key.into(), value:value.into() }).collect(),
    }) });
    assert!(app.engine().is_some());
    let logout = app.handle(common::now(), Event::Action { action:Action::Account { action:AccountAction::SignOut } });
    let save = logout.iter().position(|e| matches!(e, Effect::Store { op:StoreOp::PutDurable { put, .. }, .. } if !put.is_empty())).unwrap();
    let erase = logout.iter().rposition(|e| matches!(e, Effect::Store { op:StoreOp::DeviceApply { delete, .. }, .. } if !delete.is_empty())).unwrap();
    assert!(save < erase, "the signed public notice is written before private key erasure");
}

#[test]
fn a_departure_before_its_own_join_waits_for_verified_human_membership() {
    use trommi_core::ids::base64url_encode as b64;
    let mut world = World::new();
    let mut keeper = world.sim();
    keeper.send(start());
    let before = keeper.app.engine().unwrap().roles();
    let (mut joiner, _) = common::new_peer();
    let id = joiner.id();
    let signature = joiner.sign_departure_for(&world.room.room_id()).unwrap();
    let data = serde_json::json!({"device":id.to_base64url(),"signature":b64(&signature)});
    keeper.stream(trommi_client_core::app::io::StreamItem::Chunk { bytes:format!("event: departure\ndata: {data}\n\n").into_bytes().into() });
    assert_eq!(keeper.app.engine().unwrap().roles(), before, "a signed unknown key never invents membership");
    common::sync(&world.hub.borrow(), &mut world.founder);
    common::invite(&mut world.founder, &mut joiner, trommi_core::invite::Role::Human);
    assert!(common::post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    let hex: String = id.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
    for _ in 0..20 {
        deliver_to(&world, &mut keeper, world.me);
        if keeper.view.settings.removed.iter().any(|row| row.id == hex) { break; }
    }
    assert!(!keeper.app.engine().unwrap().roles().0.contains(&id));
    assert!(keeper.view.settings.removed.iter().any(|row| row.id == hex), "the held proof retires the device only after its actual human join verifies");
}

#[test]
fn a_public_queue_prefix_cannot_hide_private_or_forged_cache_from_logout_erasure() {
    let world = World::new();
    let mut sim = world.sim();
    sim.cache.insert("public-departure/private".into(), b"private cached content".to_vec());
    sim.cache.insert(format!("public-departure/{}.{}", world.room.room_id().to_base64url(), world.agent.id().to_base64url()), br#"{"hub":"https://hub.example","room":"invalid","device":"invalid","signature":[],"token":"must not survive"}"#.to_vec());
    sim.send(start());
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/departures").then(|| Err(trommi_client_core::app::IoError::new(trommi_client_core::app::IoErrorKind::Offline)))
    }));
    sim.send(Event::Action { action:Action::Account { action:AccountAction::SignOut } });
    assert_eq!(sim.cache.len(), 1);
    assert!(!sim.cache.contains_key("public-departure/private"));
    let notice: serde_json::Value = serde_json::from_slice(sim.cache.values().next().unwrap()).unwrap();
    assert_eq!(notice["device"], world.me.to_base64url());
    assert!(notice.get("token").is_none());
}

#[test]
fn logout_erases_large_cached_values_by_name_without_loading_them_through_the_core() {
    let world = World::new();
    let mut sim = world.sim();
    sim.cache.insert("c1/unrelated/large-private-file".into(), vec![7; 8 * 1024 * 1024]);
    sim.send(start());
    sim.log.clear();
    sim.send(Event::Action { action:Action::Account { action:AccountAction::SignOut } });
    assert!(sim.cache.is_empty() && sim.device.1.is_empty());
    assert!(sim.log.iter().any(|effect| matches!(effect, trommi_client_core::app::Effect::Store { op:trommi_client_core::app::StoreOp::Keys { prefix }, .. } if prefix.is_empty())));
    assert!(!sim.log.iter().any(|effect| matches!(effect, trommi_client_core::app::Effect::Store { op:trommi_client_core::app::StoreOp::Scan { prefix }, .. } if prefix.is_empty())), "private cached bodies never need to cross the binding just to be erased");
}

#[test]
fn logout_preserves_unread_public_notices_when_the_initial_notice_scan_failed() {
    use trommi_client_core::app::{Effect, StoreOp, IoResult, IoError, IoErrorKind};
    let world = World::new();
    let (old, _) = common::new_peer();
    let room = world.room.room_id();
    let old_key = format!("public-departure/{}.{}", room.to_base64url(), old.id().to_base64url());
    let notice = serde_json::to_vec(&serde_json::json!({"hub":"https://hub.example", "room":room.to_base64url(), "device":old.id().to_base64url(), "signature":old.sign_departure_for(&room).unwrap()})).unwrap();
    let mut sim = world.sim();
    sim.cache.insert(old_key.clone(), notice);
    let effects = sim.app.handle(common::now(), start());
    let scan = effects.iter().find_map(|e| match e { Effect::Store { id, op:StoreOp::Scan { prefix } } if prefix == "public-departure/" => Some(*id), _ => None }).unwrap();
    sim.run(effects.into_iter().filter(|e| !matches!(e, Effect::Store { id, .. } if *id == scan)).collect());
    sim.send(Event::Store { id:scan, result:IoResult::Err(IoError::new(IoErrorKind::Failed)) });
    world.hub.borrow_mut().fault = Some(Box::new(|_, request| {
        request.url.ends_with("/departures").then(|| Err(IoError::new(IoErrorKind::Offline)))
    }));
    sim.log.clear();
    sim.send(Event::Action { action:Action::Account { action:AccountAction::SignOut } });
    assert!(sim.device.1.is_empty());
    assert!(sim.cache.contains_key(&old_key), "a failed startup scan must not turn a valid old retirement into private cache to erase");
    assert_eq!(sim.cache.len(), 2, "only the two validated public notices remain");
    assert!(sim.log.iter().any(|e| matches!(e, Effect::Store { op:StoreOp::Scan { prefix }, .. } if prefix.is_empty())), "the exceptional path first validates the unread records");
}
