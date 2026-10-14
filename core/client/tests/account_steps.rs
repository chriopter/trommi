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
