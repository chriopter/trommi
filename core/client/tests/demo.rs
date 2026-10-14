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

#[test]
fn the_demo_opens_its_files_by_their_address_and_keeps_what_is_attached() {
    let f = fixture();
    let mut sim = Sim::new(f["made_at"].as_u64().unwrap());
    sim.hub = Some(Box::new(|_, request| {
        assert!(request.url.starts_with("/demo/files/"), "{}", request.url);
        Ok(trommi_client_core::app::HttpResponse { status: 200, headers: vec![], body: b"picture bytes".to_vec().into() })
    }));
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), seed: 7, demo: Some(f), ..Default::default() } });
    let row = sim.view.desk.rows.iter().find(|r| !r.media.is_empty()).expect("a row with a picture").clone();
    let file = row.media[0].file_id.clone();
    sim.send(Event::Action { action: Action::OpenFile { attachment_id: file.clone(), save: false } });
    assert!(sim.log.iter().any(|e| matches!(e, Effect::FileReady { file_id, bytes, .. } if *file_id == file && bytes.0 == b"picture bytes")));
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
    // attached: ready at once, opened from memory
    sim.send(Event::Action { action: Action::Attach { local_id: "local-1".into(), name: "a.txt".into(), media_type: "text/plain".into(), bytes: b"mine".to_vec().into() } });
    assert_eq!(sim.view.files.get("local-1").map(|f| f.state.as_str()), Some("ready"));
    sim.log.clear();
    sim.send(Event::Action { action: Action::OpenFile { attachment_id: "local-1".into(), save: false } });
    assert!(sim.log.iter().any(|e| matches!(e, Effect::FileReady { bytes, .. } if bytes.0 == b"mine")));
}
