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
