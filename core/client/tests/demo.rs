//! The demo room through the App (`Config.demo`): no hub and no store; the views come at once, a page in focus
//! shows, and what the person does stays as done.

use serde_json::Value;
use trommi_client_core::actions::Action;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Config, Effect, Event};
use trommi_client_core::views::{Focus, Phase, Screen};

fn fixture() -> Value {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../demo/data/fixture.json");
    serde_json::from_str(&std::fs::read_to_string(root).expect("demo/data/fixture.json")).unwrap()
}

#[test]
fn the_demo_room_runs_without_a_hub() {
    let f = fixture();
    let mut sim = Sim::new(f["made_at"].as_u64().unwrap());
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), seed: 7, demo: Some(f), ..Default::default() } });
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(!sim.view.desk.rows.is_empty(), "the Desk of the demo");
    assert!(sim.log.iter().all(|e| !matches!(e, Effect::Http { .. } | Effect::Store { .. } | Effect::OpenStream { .. })), "no I/O");
    // a card opened, answered: off the Desk, nothing pending
    let row = sim.view.desk.rows[0].clone();
    sim.send(Event::Focus { focus: Focus { screen: Screen::Card { card_id: row.id.clone() }, ..Default::default() } });
    assert!(sim.view.cards.contains_key(&row.id));
    let card = sim.app.board().model.cards[&row.id].clone();
    let action = if card.fields.get("card_type").as_str() == Some("info") {
        Action::Read { card_id: row.id.clone() }
    } else {
        Action::Shred { card_id: row.id.clone(), note: None, attachments: vec![], marks: vec![] }
    };
    sim.send(Event::Action { action });
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
    assert!(!sim.view.desk.rows.iter().any(|r| r.id == row.id), "done with: off the Desk");
    assert!(sim.app.board().model.cards[&row.id].answer.as_ref().is_some_and(|a| !a.pending));
}
