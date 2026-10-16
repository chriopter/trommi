//! The start's line (`ViewModel.status`, app/start.rs): a start against the stand-in hub says its steps in order,
//! counts the catching up against the hub's head, and leaves no line once live.

mod common;

use common::{start, World};
use trommi_client_core::app::Effect;
use trommi_client_core::views::{Phase, ViewModel};

#[test]
fn a_start_says_each_step_in_order_and_how_far_the_catching_up_is() {
    let mut world = World::new();
    // more than one page of the hub's order (500 a page): the first page leaves the rest to count
    for n in 0..520 {
        world.agent_says(&format!("line {n}"));
    }
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    // every line the renders said, in order, as a shell folds them
    let mut view = ViewModel::default();
    let mut lines: Vec<(String, String)> = Vec::new();
    for effect in &sim.log {
        if let Effect::Render { delta } = effect {
            view.apply(delta).expect("renders come in order");
            let line = view.status.as_ref().map(|t| (t.key.clone(), t.en.clone())).unwrap_or_default();
            if lines.last() != Some(&line) {
                lines.push(line);
            }
        }
    }
    let keys: Vec<&str> = lines.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys.first(), Some(&"start.device"), "{lines:?}");
    assert_eq!(keys.last(), Some(&""), "no line once live: {lines:?}");
    let at = |key: &str| keys.iter().position(|k| *k == key).unwrap_or_else(|| panic!("{key} missing: {lines:?}"));
    assert!(at("start.device") < at("start.desk") && at("start.desk") < at("start.catching_up"), "{lines:?}");
    // the count: what the first page read, of the changes up to the hub's head
    let counted: Vec<&str> = lines.iter().filter(|(k, _)| k == "start.catching_up").map(|(_, en)| en.as_str()).filter(|en| en.contains(" of ")).collect();
    assert!(!counted.is_empty(), "a count while catching up: {lines:?}");
    let (done, total) = counted[0].trim_start_matches("Catching up: ").split_once(" of ").expect("done of total");
    let number = |s: &str| s.replace(',', "").parse::<u64>().expect("a number");
    assert!(number(done) > 0 && number(done) < number(total), "{lines:?}");
    assert!(number(total) >= 520, "{lines:?}");
    assert_eq!(sim.view.status, None);
}

#[test]
fn a_device_that_does_not_load_says_it_stopped_instead_of_showing_an_empty_desk() {
    let world = World::new();
    let mut sim = world.sim();
    sim.load_fails = u32::MAX;
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Stopped);
    let status = sim.view.status.clone().expect("a stopped app says so");
    assert_eq!(status.key, "phase.stopped");
    assert!(!status.en.is_empty());
}
