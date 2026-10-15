//! Files through the App (`crate::files`): attached, uploaded and named in a line; opened from memory and, after a
//! restart, fetched, checked and decrypted; shared and the link ended; a file the views never showed is refused.

mod common;

use common::{start, World};
use trommi_client_core::actions::Action;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Bytes, Effect, Event};
use trommi_client_core::views::Phase;

fn act(sim: &mut Sim, action: Action) -> Vec<Effect> {
    let from = sim.log.len();
    sim.send(Event::Action { action });
    sim.log[from..].to_vec()
}

fn session_of(sim: &Sim) -> String {
    sim.view.sidebar.sessions.first().map(|s| s.id.clone()).expect("a session")
}

const PICTURE: &[u8] = b"\x89PNG not really, but bytes all the same";

#[test]
fn a_file_is_attached_sent_opened_and_shared() {
    let world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    let session = session_of(&sim);

    // attached: uploading, then ready with its attachment id
    sim.hold = true;
    act(&mut sim, Action::Attach { local_id: "pick-1".into(), name: "plan.png".into(), media_type: "image/png".into(), bytes: Bytes(PICTURE.to_vec()) });
    let f = &sim.view.files["pick-1"];
    assert_eq!((f.state.as_str(), f.name.as_str(), f.size), ("uploading", "plan.png", PICTURE.len() as u64));
    // a line cannot name it before it is stored
    act(&mut sim, Action::Write { session_id: session.clone(), card_id: None, text: "see".into(), attachments: vec!["pick-1".into()], marks: vec![], copied_cards: vec![] });
    assert!(sim.view.toasts.iter().any(|t| t.text.key == "action.file_not_ready"), "{:?}", sim.view.toasts);
    sim.hold = false;
    while let Some((id, request)) = sim.held.pop_front() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    let f = sim.view.files["pick-1"].clone();
    assert_eq!(f.state, "ready");
    let attachment = f.attachment_id.clone().expect("its id");
    assert_eq!(world.hub.borrow().files.len(), 1);
    assert!(!world.hub.borrow().files.values().next().unwrap().windows(PICTURE.len()).any(|w| w == PICTURE), "the hub holds ciphertext");

    // named in a line: the timeline shows it, its reference with the line
    act(&mut sim, Action::Write { session_id: session.clone(), card_id: None, text: "the plan".into(), attachments: vec!["pick-1".into()], marks: vec![], copied_cards: vec![] });
    world.stream(&mut sim);
    sim.send(Event::Focus { focus: trommi_client_core::views::Focus { screen: trommi_client_core::views::Screen::Session { session_id: session.clone() }, ..Default::default() } });
    let page = &sim.view.sessions[&session];
    assert!(page.talk.iter().any(|r| r.files == [attachment.clone()]), "the line names the file");
    assert_eq!(sim.view.files[&attachment].name, "plan.png");

    // opened: from memory at once
    let effects = act(&mut sim, Action::OpenFile { attachment_id: attachment.clone(), save: false });
    assert!(effects.iter().any(|e| matches!(e, Effect::FileReady { file_id, bytes, .. } if *file_id == attachment && bytes.0 == PICTURE)));
    assert!(sim.view.files[&attachment].loaded);

    // after a restart: fetched from the hub, checked and decrypted; saved as a file
    sim.restart();
    sim.send(start());
    world.stream(&mut sim);
    sim.send(Event::Focus { focus: trommi_client_core::views::Focus { screen: trommi_client_core::views::Screen::Session { session_id: session.clone() }, ..Default::default() } });
    sim.hold = true;
    let effects = act(&mut sim, Action::OpenFile { attachment_id: attachment.clone(), save: true });
    assert!(!effects.iter().any(|e| matches!(e, Effect::SaveFile { .. })), "not from memory now");
    assert_eq!(sim.view.files[&attachment].state, "loading");
    sim.hold = false;
    while let Some((id, request)) = sim.held.pop_front() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert!(sim.log.iter().any(|e| matches!(e, Effect::SaveFile { name, bytes, .. } if name == "plan.png" && bytes.0 == PICTURE)), "fetched and saved");

    // shared: a link with the key behind its '#'; the hub learns no key
    act(&mut sim, Action::Share { attachment_id: attachment.clone(), expires_ms: None });
    let shares = sim.view.files[&attachment].shares.clone();
    assert_eq!(shares.len(), 1, "{:?}", sim.view.toasts);
    assert!(shares[0].link.starts_with("https://app.trommi.com/artifact/") && shares[0].link.contains('#'));
    assert_eq!(world.hub.borrow().shares.len(), 1);
    act(&mut sim, Action::RevokeShare { share_id: shares[0].share_id.clone() });
    assert!(sim.view.files[&attachment].shares.is_empty());
    assert!(world.hub.borrow().shares.is_empty());

    // a file id the views never showed is refused
    act(&mut sim, Action::OpenFile { attachment_id: "00112233445566778899aabbccddeeff".into(), save: false });
    assert!(sim.view.toasts.iter().any(|t| t.text.key == "file.unknown"));
}

#[test]
fn a_note_keeps_its_attached_file_and_a_native_pen_stroke_round_trips() {
    let world = World::new(); let mut sim = world.sim(); sim.send(start());
    act(&mut sim, Action::Attach { local_id: "native-file".into(), name: "plan.png".into(), media_type: "image/png".into(), bytes: Bytes(PICTURE.to_vec()) });
    act(&mut sim, Action::WriteNoteFiles { note_id: None, text: "Keep this plan".into(), attachments: vec!["native-file".into()] });
    assert_eq!(sim.view.notes.rows.len(), 1);
    assert_eq!(sim.view.notes.rows[0].files.len(), 1);
    let file = sim.view.notes.rows[0].files[0].file_id.clone();
    assert_eq!(sim.view.files[&file].name, "plan.png");
    let board = trommi_client_core::scribble::desk_board("all");
    sim.send(Event::Focus { focus: trommi_client_core::views::Focus { screen: trommi_client_core::views::Screen::Scribble { board_id: board.clone() }, ..Default::default() } });
    act(&mut sim, Action::DrawStroke { board_id: board, local_id: "native-pen".into(), points: vec![serde_json::json!([1.5,2.5,0,0.5]), serde_json::json!([3.5,4.5,5,1])], tool: "pen".into(), color: "ink".into(), width: 3 });
    let view = sim.view.board.as_ref().expect("a board");
    assert_eq!(view.shapes.len(), 1, "{:?}", sim.view.toasts);
    assert_eq!(view.shapes[0].pts, [1.5,2.5,3.5,4.5]);
}
