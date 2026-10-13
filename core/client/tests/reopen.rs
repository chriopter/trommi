//! A device that closed itself (a write of its state failed) is opened again in place from its stored state, against
//! the stand-in hub (tests/common). Another owner of the state stops the App (tests/engine.rs).

mod common;

use common::{start, World};
use trommi_client_core::app::io::StoreOp;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Effect, Event, REOPEN_TRIES};
use trommi_client_core::views::Phase;

/// Whether the board shows a line with this text.
fn shows(sim: &Sim, text: &str) -> bool {
    sim.app.board().model.timelines.values().flat_map(|t| t.items.values()).any(|item| serde_json::to_string(item).is_ok_and(|json| json.contains(text)))
}

fn loads(sim: &Sim) -> usize {
    sim.log.iter().filter(|e| matches!(e, Effect::Store { op: StoreOp::DeviceLoad, .. })).count()
}

/// Moves the clock a second at a time until the App is live again, at most `seconds`.
fn until_live(world: &World, sim: &mut Sim, seconds: u32) {
    for _ in 0..seconds {
        if sim.view.phase == Phase::Live {
            return;
        }
        sim.advance(1_000);
        world.stream(sim);
    }
}

#[test]
fn a_write_that_fails_opens_the_stored_state_again_and_nothing_is_lost() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    world.agent_says("before the failure");
    world.stream(&mut sim);
    assert!(shows(&sim, "before the failure"));
    // the store is full once: the write of the next line's state fails
    sim.device_full = 1;
    world.agent_says("while the store was full");
    world.stream(&mut sim);
    assert!(sim.log.iter().any(|e| matches!(e, Effect::Store { op: StoreOp::DeviceApply { .. }, .. })));
    assert_eq!(sim.view.phase, Phase::Opening, "not stopped: opened again in place");
    assert!(sim.streams.is_empty(), "the closed device's stream was cancelled");
    let revision = sim.device.0;
    // after a wait the stored state is read again, and the new device reads what the stored one lacks
    until_live(&world, &mut sim, 10);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(loads(&sim), 2, "the start's load and one more");
    assert!(sim.device.0 > revision, "the line is stored now");
    assert!(shows(&sim, "before the failure"), "the board stayed as it was shown");
    assert!(shows(&sim, "while the store was full"));
    world.agent_says("after it");
    world.stream(&mut sim);
    assert!(shows(&sim, "after it"));
    // what the device stored is what a restart finds
    let device = world.reopen_me(&sim);
    assert_eq!(device.cursor(), sim.app.engine().unwrap().device().cursor());
}

#[test]
fn a_device_that_does_not_open_again_stops_and_tries_again_when_the_app_comes_to_the_front() {
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    sim.device_full = 1;
    sim.load_fails = REOPEN_TRIES;
    world.agent_says("a line");
    world.stream(&mut sim);
    for _ in 0..120 {
        sim.advance(1_000);
    }
    assert_eq!(sim.view.phase, Phase::Stopped);
    assert_eq!(loads(&sim), 1 + REOPEN_TRIES as usize, "each try read the store once, and no more");
    let failed = sim.log.iter().filter(|e| matches!(e, Effect::Log { text, .. } if text.starts_with("device load"))).count();
    assert_eq!(failed, REOPEN_TRIES as usize);
    // the person comes back: tried again, and it opens
    sim.send(Event::Foreground);
    until_live(&world, &mut sim, 10);
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(shows(&sim, "a line"));
}

#[test]
fn a_store_that_refuses_in_another_way_is_no_other_owner() {
    // (only `conflict` means another owner: then the App stops and the shell opens the state with a new App)
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    sim.device_full = 1;
    let draft = trommi_core::device::Draft::SessionChat { session: world.session, payload: common::text(r#"{"content_type":"message","text":"lost"}"#) };
    let (sealed, effects) = sim.app.act(sim.now_ms, |device, now| device.seal(&draft, None, &[], now));
    sealed.expect("a device").expect("it seals");
    sim.run(effects);
    assert_eq!(sim.view.phase, Phase::Opening);
    assert!(!sim.log.iter().any(|e| matches!(e, Effect::Http { request, .. } if request.url.ends_with("/v1/envelopes"))), "nothing of the unstored state left");
    until_live(&world, &mut sim, 10);
    // the sealed line was never stored: the device that opens again holds no trace of it, and sends nothing
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
}
