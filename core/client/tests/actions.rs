//! The person's actions through the App (`Event::Action`), against the stand-in hub (tests/common): the echo in the
//! very next `Render`, replaced by the hub's copy, rolled back with a toast that offers the action again when the hub
//! refuses it, and sent once across a restart; a write lost before it was stored is rolled back when the engine
//! opens the stored state again in place.

mod common;

use common::hub::answer;
use common::{start, World};
use serde_json::json;
use trommi_client_core::account::{AccountAction, Secret};
use trommi_client_core::actions::Action;
use trommi_client_core::app::io::{HttpRequest, IoError, IoErrorKind};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Effect, Event};
use trommi_client_core::views::Phase;

/// A room with one open Decision card on the App's Desk. Returns the card's id.
fn with_card(world: &mut World, sim: &mut Sim) -> String {
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    world.agent_asks("Ship it?");
    assert!(world.stream(sim));
    assert_eq!(sim.view.desk.rows.len(), 1);
    sim.view.desk.rows[0].id.clone()
}

fn act(sim: &mut Sim, action: Action) -> Vec<Effect> {
    let from = sim.log.len();
    sim.send(Event::Action { action });
    sim.log[from..].to_vec()
}

fn yes(card: &str) -> Action {
    Action::Answer { card_id: card.into(), choices: vec!["yes".into()], note: None, attachments: vec![], option_notes: Default::default(), trusted: false, marks: vec![] }
}

fn answer_of(sim: &Sim, card: &str) -> Option<(String, bool)> {
    let c = sim.app.board().model.cards.get(card)?;
    Some((c.object_state.clone(), c.answer.as_ref()?.pending))
}

/// Answers every held request at the stand-in hub.
fn serve(world: &World, sim: &mut Sim) {
    while let Some((id, request)) = sim.held.pop_front() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
}

fn envelope_posts(world: &World) -> usize {
    world.hub.borrow().seen.iter().filter(|s| s.starts_with("POST /v1/envelopes")).count()
}

#[test]
fn an_answer_shows_at_once_and_the_hubs_copy_replaces_it() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    sim.hold = true;
    let effects = act(&mut sim, yes(&card));
    // the echo comes in the very first effect, before the device's write and the post
    assert!(matches!(effects.first(), Some(Effect::Render { .. })), "{effects:?}");
    let store = effects.iter().position(|e| matches!(e, Effect::Store { .. })).expect("the device's write");
    assert!(effects.iter().position(|e| matches!(e, Effect::Http { .. })).is_none_or(|post| post > store));
    assert!(sim.view.desk.rows.is_empty(), "answered: off the Desk at once");
    assert_eq!(answer_of(&sim, &card), Some(("answered".into(), true)));
    assert!(sim.view.toasts.is_empty());
    // the hub takes it; its copy comes back in the stream and stands in place of the echo
    sim.hold = false;
    serve(&world, &mut sim);
    world.stream(&mut sim);
    assert_eq!(answer_of(&sim, &card), Some(("answered".into(), false)));
    assert!(sim.view.desk.rows.is_empty() && sim.view.toasts.is_empty());
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    assert_eq!(sim.app.board().model.cards[&card].answers.len(), 1);
}

#[test]
fn reading_and_shredding_close_the_card_at_once() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    // read and shred close it at once
    act(&mut sim, Action::Shred { card_id: card.clone(), note: None, attachments: vec![], marks: vec![] });
    assert_eq!(answer_of(&sim, &card).map(|a| a.0), Some("closed".into()));
    assert_eq!(sim.app.board().model.cards[&card].answer.as_ref().unwrap().answer_action, "shred");
}

#[test]
fn what_cannot_be_done_is_said_and_changes_nothing() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let posts = envelope_posts(&world);
    let wrong = Action::Answer { card_id: card.clone(), choices: vec!["maybe".into()], note: None, attachments: vec![], option_notes: Default::default(), trusted: false, marks: vec![] };
    act(&mut sim, wrong);
    assert_eq!(sim.view.toasts.len(), 1);
    assert_eq!(sim.view.toasts[0].text.key, "action.bad_choice");
    assert_eq!(sim.view.toasts[0].action, None, "a check is not offered again");
    assert_eq!(sim.view.desk.rows.len(), 1);
    act(&mut sim, yes("00112233445566778899aabbccddeeff"));
    assert_eq!(sim.view.toasts[1].text.key, "action.not_found");
    assert_eq!(envelope_posts(&world), posts, "nothing was sent");
    // a toast closed is gone
    let id = sim.view.toasts[0].id.clone();
    act(&mut sim, Action::Dismiss { toast_id: id });
    assert_eq!(sim.view.toasts.len(), 1);
}

#[test]
fn a_refusal_rolls_the_answer_back_with_a_toast_that_answers_again() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let mut once = Some(answer(403, json!({ "error": "forbidden", "message": "refused", "voided": true })));
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| if request.url.ends_with("/v1/envelopes") { once.take().map(Ok) } else { None }));
    act(&mut sim, yes(&card));
    // (the hub keeps the void record under the refused number)
    world.hub.borrow_mut().void_next(world.group, world.me);
    assert_eq!(sim.view.desk.rows.len(), 1, "back on the Desk");
    let card_now = &sim.app.board().model.cards[&card];
    assert_eq!((card_now.object_state.as_str(), card_now.answer.is_none()), ("open", true));
    assert_eq!(sim.view.toasts.len(), 1);
    let toast = sim.view.toasts[0].clone();
    assert_eq!((toast.text.key.as_str(), toast.text.args.get("code").map(String::as_str)), ("action.refused", Some("forbidden")));
    assert_eq!(toast.action, Some(yes(&card)));
    // "Try again": the same action, shown at once and now taken by the hub. (Its copy is not read back here: the
    // stand-in hub keeps no void record in its order, so the App waits for the refused number before it.)
    act(&mut sim, toast.action.unwrap());
    assert_eq!(answer_of(&sim, &card), Some(("answered".into(), true)));
    assert!(sim.view.desk.rows.is_empty());
    assert!(sim.app.engine().unwrap().device().outbox().is_empty(), "taken");
    assert_eq!(sim.view.toasts.len(), 1, "no new toast");
}

#[test]
fn snoozing_puts_the_card_off_at_once_until_the_next_morning_and_waking_brings_it_back() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    sim.hold = true;
    act(&mut sim, Action::Snooze { card_id: card.clone(), until_ms: None });
    assert!(sim.view.desk.rows.is_empty(), "put off: off the Desk at once");
    assert_eq!(sim.view.desk.later, std::slice::from_ref(&card));
    let until = sim.app.board().model.human[&format!("snooze/{card}")].value["until"].as_u64().unwrap();
    assert!(until > sim.now_ms && until <= sim.now_ms + 86_400_000);
    assert_eq!(until % 3_600_000, 7 * 3_600_000 % 3_600_000);
    sim.hold = false;
    serve(&world, &mut sim);
    world.stream(&mut sim);
    let reg = &sim.app.board().model.human[&format!("snooze/{card}")];
    assert!(!reg.pending && reg.envelope_number.is_some(), "the hub's copy");
    assert_eq!(sim.view.desk.later, std::slice::from_ref(&card));
    // woken by hand: back on the Desk at once
    act(&mut sim, Action::Unsnooze { card_id: card.clone() });
    assert_eq!(sim.view.desk.rows.len(), 1);
    assert!(sim.view.desk.later.is_empty());
}

#[test]
fn an_answer_to_a_card_put_off_wakes_it() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let later = sim.now_ms + 3_600_000;
    act(&mut sim, Action::Snooze { card_id: card.clone(), until_ms: Some(later) });
    world.stream(&mut sim);
    act(&mut sim, yes(&card));
    world.stream(&mut sim);
    let until = sim.app.board().model.human[&format!("snooze/{card}")].value["until"].as_u64().unwrap();
    assert!(until <= sim.now_ms, "its snooze ended now");
}

#[test]
fn an_answer_in_the_outbox_at_a_restart_is_sent_once() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    world.hub.borrow_mut().fault = Some(Box::new(|_, request: &HttpRequest| {
        request.url.ends_with("/v1/envelopes").then(|| Err(IoError::new(IoErrorKind::Offline)))
    }));
    act(&mut sim, yes(&card));
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 1);
    // the process dies; a new App opens what was stored and the hub is back
    sim.restart();
    world.hub.borrow_mut().fault = None;
    sim.send(start());
    world.stream(&mut sim);
    assert_eq!(answer_of(&sim, &card), Some(("answered".into(), false)));
    assert_eq!(sim.app.board().model.cards[&card].answers.len(), 1);
    assert!(sim.view.toasts.is_empty(), "nothing was lost");
}

#[test]
fn no_debug_of_an_action_shows_a_secret() {
    let secret = "hunter2-very-secret";
    let actions = [
        AccountAction::Found { email: "a@example.org".into(), password: Secret(secret.into()), hub: None },
        AccountAction::SignIn { email: "a@example.org".into(), password: Secret(secret.into()), hub: None },
        AccountAction::SignInWithKit { email: "a@example.org".into(), words: Secret(secret.into()), hub: None },
        AccountAction::Join { link: Secret(secret.into()) },
        AccountAction::DeleteAccount { password: Secret(secret.into()) },
    ];
    for action in actions {
        let event = Event::Action { action: Action::Account { action } };
        let shown = format!("{event:?} {event:#?}");
        assert!(!shown.contains(secret) && !shown.contains("example.org"), "{shown}");
    }
}

fn human(sim: &Sim, key: &str) -> Option<(serde_json::Value, bool)> {
    sim.app.board().model.human.get(key).map(|r| (r.value.clone(), r.pending))
}

#[test]
fn a_star_shows_at_once_and_is_taken_off_again() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    act(&mut sim, Action::Star { card_id: card.clone(), on: true });
    let key = format!("star/{card}");
    assert_eq!(human(&sim, &key).map(|h| h.1), Some(true), "at once");
    world.stream(&mut sim);
    assert_eq!(human(&sim, &key).map(|h| h.1), Some(false), "the hub's copy");
    act(&mut sim, Action::Star { card_id: card.clone(), on: false });
    world.stream(&mut sim);
    assert!(human(&sim, &key).is_none_or(|h| h.0.is_null()));
}

#[test]
fn deciding_again_opens_the_card_and_keeps_what_was_taken_back_as_its_draft() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    act(&mut sim, yes(&card));
    world.stream(&mut sim);
    act(&mut sim, Action::DecideAgain { card_id: card.clone() });
    world.stream(&mut sim);
    assert_eq!(sim.app.board().model.cards[&card].object_state, "open");
    assert_eq!(sim.view.desk.rows.len(), 1, "back on the Desk");
    let draft = human(&sim, &format!("draft/{card}")).expect("the draft").0;
    assert_eq!(draft["keys"], json!(["yes"]));
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
    act(&mut sim, Action::DecideAgain { card_id: card.clone() });
    assert_eq!(sim.view.toasts.last().map(|t| t.text.key.as_str()), Some("action.nothing_to_take_back"));
}

#[test]
fn a_line_and_a_hand_back_show_at_once_in_their_chats() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    sim.hold = true;
    act(&mut sim, Action::Write { session_id: session.clone(), card_id: None, text: "hello agent".into(), attachments: vec![], marks: vec![], copied_cards: vec![] });
    act(&mut sim, Action::HandBack { card_id: card.clone(), note: Some("what do you mean?".into()), rework: false, marks: vec![], attachments: vec![] });
    let model = &sim.app.board().model;
    let pending = |key: &str| model.timelines.get(key).map(|t| t.items.values().filter(|i| i.get("pending") == Some(&json!(true))).count());
    assert_eq!(pending(&format!("chat:session/{session}")), Some(1));
    assert_eq!(pending(&format!("chat:card/{card}")), Some(1));
    sim.hold = false;
    serve(&world, &mut sim);
    world.stream(&mut sim);
    let model = &sim.app.board().model;
    let line = model.timelines[&format!("chat:session/{session}")].items.values().find(|i| i["content"]["text"] == json!("hello agent")).cloned().expect("the line");
    assert_ne!(line.get("pending"), Some(&json!(true)), "the hub's copy in its place");
    assert!(sim.view.toasts.is_empty());
}

#[test]
fn desks_are_made_renamed_given_goals_and_the_last_one_stays() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let teams = |sim: &Sim| sim.app.board().model.human.iter().filter(|(k, r)| k.starts_with("desk/") && !r.value.is_null()).map(|(k, r)| (k.clone(), r.value.clone())).collect::<Vec<_>>();
    act(&mut sim, Action::AddTeam { name: "  Work  ".into() });
    let made = teams(&sim);
    assert_eq!(made.len(), 2, "the first team and the new one: {made:?}");
    assert_eq!(made.iter().find(|(k, _)| k == "desk/main").map(|d| d.1.clone()), Some(json!({ "name": "Personal", "created_at": 0 })));
    let (work, _) = made.iter().find(|(k, _)| k != "desk/main").cloned().unwrap();
    let work = work.trim_start_matches("desk/").to_string();
    assert_eq!(work.len(), 32);
    world.stream(&mut sim);
    act(&mut sim, Action::RenameTeam { team_id: work.clone(), name: "Job".into() });
    act(&mut sim, Action::SetGoal { team_id: work.clone(), text: "ship\n\n  launch \n".into() });
    let v = teams(&sim).into_iter().find(|(k, _)| k.ends_with(&work)).unwrap().1;
    assert_eq!((v["name"].clone(), v["goals"].clone()), (json!("Job"), json!("ship\nlaunch")));
    let session = sim.view.sidebar.sessions[0].id.clone();
    act(&mut sim, Action::MoveSession { session_id: session.clone(), team_id: work.clone() });
    act(&mut sim, Action::RenameSession { session_id: session.clone(), name: "Builder".into() });
    let s = human(&sim, &format!("session/{session}")).unwrap().0;
    assert_eq!((s["desk"].clone(), s["name"].clone()), (json!(work.clone()), json!("Builder")));
    world.stream(&mut sim);
    // the delete runs in steps (mark, board read, empty snapshot, register gone), each once the last shows
    act(&mut sim, Action::RemoveTeam { team_id: "main".into() });
    assert!(!sim.view.settings.deleting.is_empty() || teams(&sim).len() == 1);
    for _ in 0..40 {
        if teams(&sim).len() == 1 {
            break;
        }
        world.stream(&mut sim);
        sim.advance(5_000);
    }
    assert_eq!(teams(&sim).len(), 1, "{:?} {:?}", sim.view.settings.deleting, sim.view.toasts);
    assert!(sim.view.settings.deleting.is_empty());
    act(&mut sim, Action::RemoveTeam { team_id: work.clone() });
    assert_eq!(sim.view.toasts.last().map(|t| t.text.key.as_str()), Some("action.last_team"));
    assert_eq!(teams(&sim).len(), 1);
}

#[test]
fn a_verdict_closes_the_request() {
    use trommi_core::device::Draft;
    use trommi_core::envelope::Urgency;
    let mut world = World::new();
    let mut sim = world.sim();
    sim.send(start());
    {
        let mut hub = world.hub.borrow_mut();
        common::sync(&hub, &mut world.agent);
        let payload = common::text(r#"{"tool_name":"Bash","description":"list","input_preview":"ls"}"#);
        let draft = Draft::PermissionRequest { session: world.session, urgency: Urgency::Normal, expires_at: common::now() + 3_600_000, push: false, payload };
        world.agent.seal(&draft, None, &[], common::now()).expect("the request seals");
        assert!(common::post_all(&mut hub, &mut world.agent).is_empty());
    }
    world.stream(&mut sim);
    let request = sim.app.board().model.permissions.keys().next().cloned().expect("the request");
    act(&mut sim, Action::Verdict { request_id: request.clone(), allow: true });
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
    world.stream(&mut sim);
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    let p = &sim.app.board().model.permissions[&request];
    assert_eq!(p.permission_state, "allowed", "closed by the verdict");
    assert_eq!(p.verdict.as_ref().map(|v| v.allow), Some(true));
}

fn note(sim: &Sim, id: &str) -> serde_json::Map<String, serde_json::Value> {
    sim.app.board().model.notes.get(id).cloned().unwrap_or_default()
}

/// The ids of the notes the model holds that are not closed.
fn open_notes(sim: &Sim) -> Vec<String> {
    sim.app.board().model.notes.iter().filter(|(_, n)| n["object_state"] != json!("closed")).map(|(k, _)| k.clone()).collect()
}

fn write_note(sim: &mut Sim, id: Option<&str>, text: &str) {
    act(sim, Action::WriteNote { note_id: id.map(str::to_string), text: text.into() });
}

#[test]
fn a_new_note_shows_at_once_under_its_object_id_and_the_hubs_copy_replaces_it() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    sim.hold = true;
    write_note(&mut sim, None, "buy milk");
    let ids = open_notes(&sim);
    assert_eq!(ids.len(), 1);
    let id = ids[0].clone();
    assert_eq!(id.len(), 32, "the object id of its first envelope, at once");
    let n = note(&sim, &id);
    assert_eq!((n["text"].clone(), n["pending"].clone()), (json!("buy milk"), json!(true)));
    assert_eq!(n["created_at"], json!(sim.now_ms));
    sim.hold = false;
    serve(&world, &mut sim);
    world.stream(&mut sim);
    let n = note(&sim, &id);
    assert_eq!((n["text"].clone(), n["pending"].clone(), n["object_version"].clone()), (json!("buy milk"), json!(false), json!(1)));
    assert_eq!(n["causal"]["lamport"], json!(1.0));
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
}

#[test]
fn a_notes_edits_wait_for_the_version_on_its_way_and_the_newest_follows_it() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    write_note(&mut sim, None, "one");
    world.stream(&mut sim);
    let id = open_notes(&sim)[0].clone();
    let first = note(&sim, &id)["version_hash"].clone();
    sim.hold = true;
    write_note(&mut sim, Some(&id), "two");
    let posts = envelope_posts(&world);
    // two more edits while "two" is on its way: shown at once, sealed not yet; only the newest will follow
    write_note(&mut sim, Some(&id), "three");
    write_note(&mut sim, Some(&id), "four");
    assert_eq!(note(&sim, &id)["text"], json!("four"), "at once");
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), 1, "one version on its way");
    sim.hold = false;
    serve(&world, &mut sim);
    assert_eq!(envelope_posts(&world), posts + 1);
    // "two" is back: "four" is sealed on it
    world.stream(&mut sim);
    let n = note(&sim, &id);
    assert_eq!((n["text"].clone(), n["pending"].clone(), n["_base"]["object_version"].clone()), (json!("four"), json!(true), json!(2)), "the echo in front of version two");
    serve(&world, &mut sim);
    world.stream(&mut sim);
    let n = note(&sim, &id);
    assert_eq!((n["text"].clone(), n["object_version"].clone(), n["pending"].clone()), (json!("four"), json!(3), json!(false)));
    assert_eq!(n["version_hashes"].as_array().map(Vec::len), Some(3), "three versions, each on the one before");
    assert_ne!(n["version_hash"], first);
    assert_eq!(n["causal"]["lamport"], json!(3.0));
    assert_eq!(envelope_posts(&world), posts + 2, "three was never sent");
    assert!(sim.app.engine().unwrap().device().outbox().is_empty());
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
}

#[test]
fn a_note_deleted_is_gone_at_once_and_a_gone_note_is_said() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    write_note(&mut sim, None, "old");
    world.stream(&mut sim);
    let id = open_notes(&sim)[0].clone();
    sim.hold = true;
    act(&mut sim, Action::DeleteNote { note_id: id.clone() });
    assert!(open_notes(&sim).is_empty(), "at once");
    sim.hold = false;
    serve(&world, &mut sim);
    world.stream(&mut sim);
    assert_eq!(note(&sim, &id)["object_state"], json!("closed"));
    assert_eq!(note(&sim, &id)["pending"], json!(false));
    act(&mut sim, Action::DeleteNote { note_id: id.clone() });
    assert_eq!(sim.view.toasts.last().map(|t| t.text.key.as_str()), Some("action.note_gone"));
    write_note(&mut sim, Some("00112233445566778899aabbccddeeff"), "x");
    assert_eq!(sim.view.toasts.len(), 2);
    assert_eq!(sim.view.toasts[1].text.key, "action.note_gone");
}

#[test]
fn an_edit_waiting_behind_a_refused_version_is_sealed_on_what_stands() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    write_note(&mut sim, None, "one");
    world.stream(&mut sim);
    let id = open_notes(&sim)[0].clone();
    sim.hold = true;
    write_note(&mut sim, Some(&id), "two");
    write_note(&mut sim, Some(&id), "three");
    let mut once = Some(answer(403, json!({ "error": "forbidden", "message": "refused", "voided": true })));
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| if request.url.ends_with("/v1/envelopes") { once.take().map(Ok) } else { None }));
    // the post of "two" is refused; the hub keeps the void record under its number
    let (req, request) = sim.held.pop_front().expect("the post of two");
    let got = world.hub.borrow_mut().http(sim.now_ms, &request);
    world.hub.borrow_mut().void_next(world.room, world.me);
    sim.hold = false;
    sim.answer(req, got);
    serve(&world, &mut sim);
    // "two" refused, with a toast that offers it again; "three" went out on version one
    assert_eq!(sim.view.toasts.len(), 1);
    assert_eq!(sim.view.toasts[0].action, Some(Action::WriteNote { note_id: Some(id.clone()), text: "two".into() }));
    assert_eq!(note(&sim, &id)["text"], json!("three"), "the waiting edit stays in front");
    assert!(sim.app.engine().unwrap().device().outbox().is_empty(), "three taken");
}

#[test]
fn a_write_lost_before_it_was_stored_is_rolled_back_with_a_toast_that_does_it_again() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    // the store is full once: the device's write of the sealed star fails and it closes itself
    sim.device_full = 1;
    act(&mut sim, Action::Star { card_id: card.clone(), on: true });
    assert_eq!(sim.view.phase, Phase::Opening);
    for _ in 0..10 {
        if sim.view.phase == Phase::Live {
            break;
        }
        sim.advance(1_000);
        world.stream(&mut sim);
    }
    assert_eq!(sim.view.phase, Phase::Live, "opened again in place");
    assert!(human(&sim, &format!("star/{card}")).is_none_or(|h| h.0.is_null()), "the echo is rolled back");
    assert_eq!(sim.view.toasts.len(), 1);
    let toast = sim.view.toasts[0].clone();
    assert_eq!(toast.text.key, "action.lost");
    assert_eq!(toast.action, Some(Action::Star { card_id: card.clone(), on: true }));
    // "Try again": now stored and taken
    act(&mut sim, toast.action.unwrap());
    world.stream(&mut sim);
    assert_eq!(human(&sim, &format!("star/{card}")).map(|h| h.1), Some(false), "the hub's copy");
    assert_eq!(sim.view.toasts.len(), 1, "no new toast");
}

#[test]
fn a_draft_keeps_the_shape_both_apps_read_and_what_it_does_not_name() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let key = format!("draft/{card}");
    let draft = |text: &str, keys: Option<Vec<String>>| Action::Draft { place: card.clone(), text: text.into(), keys, notes: None, marks: None };
    act(&mut sim, draft("", Some(vec!["yes".into()])));
    let v = human(&sim, &key).unwrap().0;
    assert_eq!((v["keys"].clone(), v["note"].clone(), v["notes"].clone()), (json!(["yes"]), json!(""), json!({})));
    assert_eq!(v["ts"], json!(sim.now_ms));
    // words typed: the ticks stand
    act(&mut sim, draft("because", None));
    let v = human(&sim, &key).unwrap().0;
    assert_eq!((v["keys"].clone(), v["note"].clone()), (json!(["yes"]), json!("because")));
    // nothing left: the draft goes
    act(&mut sim, draft(" ", Some(vec![])));
    assert!(human(&sim, &key).unwrap().0.is_null());
}

#[test]
fn a_sessions_group_carries_its_desks_goals_and_is_written_once() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    let held = |sim: &Sim| sim.app.board().model.sessions[&session].registers.get("goals").map(|r| r.value.clone()).unwrap_or_default();
    act(&mut sim, Action::SetGoal { team_id: "main".into(), text: "ship it\nwrite the docs".into() });
    world.stream(&mut sim);
    assert!(held(&sim).is_null(), "the session is on no team yet");
    act(&mut sim, Action::MoveSession { session_id: session.clone(), team_id: "main".into() });
    world.stream(&mut sim);
    assert_eq!(held(&sim), json!({ "desk_id": "main", "desk_name": "Personal", "goals": "ship it\nwrite the docs" }));
    // nothing changed: nothing more is written
    let posts = envelope_posts(&world);
    world.agent_says("still here");
    world.stream(&mut sim);
    assert_eq!(envelope_posts(&world), posts);
    // the goals cleared: the session's copy goes
    act(&mut sim, Action::SetGoal { team_id: "main".into(), text: "  ".into() });
    world.stream(&mut sim);
    assert!(held(&sim).is_null());
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
}

#[test]
fn a_register_written_while_catching_up_waits_until_live() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    // the next start: held at the hub, so the device is still catching up
    sim.restart();
    sim.hold = true;
    sim.send(start());
    assert_ne!(sim.view.phase, Phase::Live);
    let outbox = sim.app.engine().unwrap().device().outbox().len();
    sim.send(Event::Action { action: Action::Star { card_id: card.clone(), on: true } });
    assert_eq!(sim.app.engine().unwrap().device().outbox().len(), outbox, "nothing sealed before the device is live");
    assert!(human(&sim, &format!("star/{card}")).is_none());
    sim.hold = false;
    for _ in 0..10 {
        serve(&world, &mut sim);
        world.stream(&mut sim);
        if sim.view.phase == Phase::Live {
            break;
        }
        sim.advance(1_000);
    }
    assert_eq!(sim.view.phase, Phase::Live);
    serve(&world, &mut sim);
    assert!(human(&sim, &format!("star/{card}")).is_some_and(|(v, _)| !v.is_null()), "written once live");
}

#[test]
fn archiving_a_session_archives_its_group_at_the_hub() {
    let mut world = World::new();
    let mut sim = world.sim();
    let _ = with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    sim.send(Event::Action { action: Action::ArchiveSession { session_id: session, on: true } });
    serve(&world, &mut sim);
    world.stream(&mut sim);
    assert!(world.hub.borrow().seen.iter().any(|s| s.starts_with("POST /v1/groups/") && s.ends_with("/archive")), "{:?}", world.hub.borrow().refused);
}

#[test]
fn sessions_get_marks_groups_places_and_the_crown_and_desks_their_order() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    let key = format!("session/{session}");
    act(&mut sim, Action::SessionIcon { session_id: session.clone(), icon: Some("rocket".into()) });
    act(&mut sim, Action::SessionGroup { session_id: session.clone(), group: Some(" Build ".into()) });
    // (one session: its place is already the last, nothing is written)
    act(&mut sim, Action::PlaceSession { session_id: session.clone(), before: None, after: None });
    let s = human(&sim, &key).unwrap().0;
    assert_eq!((s["icon"].clone(), s["group"].clone()), (json!("rocket"), json!("Build")));
    assert!(s.get("position").is_none());
    act(&mut sim, Action::SessionGroup { session_id: session.clone(), group: None });
    assert_eq!(human(&sim, &key).unwrap().0["group"], json!(null));
    world.stream(&mut sim);
    act(&mut sim, Action::CrownSession { session_id: session.clone(), on: true });
    // (a room without teams keeps its one crown)
    let crown = human(&sim, "crown").map(|v| v.0).unwrap_or_default();
    assert_eq!(crown["session_id"], json!(session), "{crown}");
    act(&mut sim, Action::AddTeam { name: "Work".into() });
    world.stream(&mut sim);
    let ids: Vec<String> = sim.app.board().model.human.keys().filter_map(|k| k.strip_prefix("desk/").map(String::from)).collect();
    let reversed: Vec<String> = ids.iter().rev().cloned().collect();
    act(&mut sim, Action::OrderTeams { team_ids: reversed.clone() });
    for (i, d) in reversed.iter().enumerate() {
        assert_eq!(human(&sim, &format!("desk/{d}")).unwrap().0["order"], json!(i));
    }
    let order = |sim: &Sim| sim.view.sidebar.teams.iter().map(|d| d.id.clone()).collect::<Vec<_>>();
    assert_eq!(order(&sim), reversed);
    // a new team stands after the ordered ones; an order naming one team, twice, and a team that is gone keeps the
    // others after it in the order they stand
    act(&mut sim, Action::AddTeam { name: "Late".into() });
    world.stream(&mut sim);
    let late = order(&sim).last().cloned().unwrap();
    assert!(!ids.contains(&late), "the new team is appended: {:?}", order(&sim));
    act(&mut sim, Action::OrderTeams { team_ids: vec![late.clone(), "gone".into(), late.clone()] });
    let mut want = vec![late.clone()];
    want.extend(reversed.iter().cloned());
    assert_eq!(order(&sim), want);
    // a deleted team (its register null, the deletion's last step) leaves the order, the others keep theirs
    act(&mut sim, Action::RoomRegister { key: format!("desk/{late}"), value: serde_json::Value::Null });
    world.stream(&mut sim);
    assert_eq!(order(&sim), reversed);
    act(&mut sim, Action::RenameDevice { name: "  Laptop ".into() });
    let me = sim.app.board().model.my_device_id.clone().unwrap();
    assert_eq!(human(&sim, &format!("device/{me}")).unwrap().0["device_name"], json!("Laptop"));
    // "Revise" and "take back" go to the card's chat
    sim.hold = true;
    act(&mut sim, Action::HandBack { card_id: card.clone(), note: Some("again".into()), rework: true, marks: vec![json!({ "picture": 0, "x": 1, "y": 2, "w": 3, "h": 4 })], attachments: vec![] });
    act(&mut sim, Action::TakeBack { card_id: card.clone() });
    let items: Vec<serde_json::Value> = sim.app.board().model.timelines[&format!("chat:card/{card}")].items.values().cloned().map(serde_json::Value::Object).collect();
    assert!(items.iter().any(|i| i["content"]["hand_back"] == json!(true) && i["content"]["marks"].as_array().is_some_and(|m| m.len() == 1)), "{items:?}");
    assert!(items.iter().any(|i| i["content"]["present_card"] == json!(true)), "{items:?}");
    assert!(sim.view.toasts.iter().all(|t| !t.alert && t.text.key == "team.added"), "only \"Team added\" with its Undo: {:?}", sim.view.toasts);
}

#[test]
fn a_bare_hand_back_says_the_cores_words_and_takes_files_along() {
    use trommi_client_core::views::talk::{EXPLAIN_TEXT, HAND_BACK_TEXT};
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    act(&mut sim, Action::Attach { local_id: "hb-1".into(), name: "why.png".into(), media_type: "image/png".into(), bytes: trommi_client_core::app::Bytes(b"png".to_vec()) });
    serve(&world, &mut sim);
    assert_eq!(sim.view.files["hb-1"].state, "ready");
    act(&mut sim, Action::HandBack { card_id: card.clone(), note: Some("  ".into()), rework: false, marks: vec![], attachments: vec![] });
    act(&mut sim, Action::HandBack { card_id: card.clone(), note: None, rework: true, marks: vec![], attachments: vec!["hb-1".into()] });
    let items: Vec<serde_json::Value> = sim.app.board().model.timelines[&format!("chat:card/{card}")].items.values().cloned().map(serde_json::Value::Object).collect();
    assert!(items.iter().any(|i| i["content"]["explain"] == json!(true) && i["content"]["text"] == json!(EXPLAIN_TEXT)), "{items:?}");
    let back = items.iter().find(|i| i["content"]["hand_back"] == json!(true)).expect("the hand-back");
    assert_eq!(back["content"]["text"], json!(HAND_BACK_TEXT));
    assert_eq!(back["content"]["attachments"].as_array().map(Vec::len), Some(1), "{back}");
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
}

#[test]
fn deleting_a_session_shreds_its_cards_and_archives_it() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    act(&mut sim, Action::DeleteSession { session_id: session.clone() });
    assert_eq!(answer_of(&sim, &card).map(|a| a.0), Some("closed".into()));
    assert_eq!(human(&sim, &format!("session/{session}")).unwrap().0["archived"], json!(true));
}

#[test]
fn a_note_sent_waits_for_undo_then_goes_to_its_session() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    write_note(&mut sim, None, "remember the milk");
    world.stream(&mut sim);
    let id = open_notes(&sim)[0].clone();
    // sent, then undone within the hold: nothing goes
    act(&mut sim, Action::SendNote { note_id: id.clone(), to: Some(session.clone()) });
    assert!(note(&sim, &id)["held"].is_object());
    act(&mut sim, Action::UnsendNote { note_id: id.clone() });
    assert!(note(&sim, &id)["held"].is_null());
    world.stream(&mut sim);
    // sent and left: after the hold the line goes to the session and the note is closed
    act(&mut sim, Action::SendNote { note_id: id.clone(), to: Some(session.clone()) });
    world.stream(&mut sim);
    sim.advance(4_000);
    world.stream(&mut sim);
    let chat = sim.app.board().model.timelines[&format!("chat:session/{session}")].items.values().any(|i| i["content"]["text"] == json!("remember the milk") && i["content"]["note"]["object_id"] == json!(id));
    assert!(chat, "the line names the note");
    assert!(!open_notes(&sim).contains(&id), "the note is gone");
    assert!(sim.view.toasts.iter().all(|t| t.text.key == "notes.sent"), "{:?}", sim.view.toasts);
}

fn keep(sim: &mut Sim, id: Option<&str>, text: &str) {
    act(sim, Action::KeepNote { note_id: id.map(str::to_string), text: text.into(), attachments: vec![], send: false, to: None });
}

#[test]
fn a_kept_note_is_made_once_written_when_it_changes_and_thrown_away_when_emptied() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    keep(&mut sim, None, "   ");
    assert!(open_notes(&sim).is_empty(), "no words: no note");
    // typed on before the page has the new note's id: the same note, never a second one
    keep(&mut sim, None, "buy");
    keep(&mut sim, None, "buy milk");
    let ids = open_notes(&sim);
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert_eq!(note(&sim, &ids[0])["text"], json!("buy milk"));
    assert_eq!(sim.view.notes.corner.as_deref(), Some(ids[0].as_str()));
    serve(&world, &mut sim);
    world.stream(&mut sim);
    serve(&world, &mut sim);
    world.stream(&mut sim);
    let posts = envelope_posts(&world);
    keep(&mut sim, Some(&ids[0]), "buy milk");
    assert_eq!(envelope_posts(&world), posts, "the same words: nothing is written");
    keep(&mut sim, Some(&ids[0]), "buy oat milk");
    assert_eq!(note(&sim, &ids[0])["text"], json!("buy oat milk"));
    keep(&mut sim, Some(&ids[0]), "");
    assert!(open_notes(&sim).is_empty(), "emptied: thrown away");
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);
    // a note gone meanwhile (sent, thrown away elsewhere): the words make a new one
    keep(&mut sim, Some(&ids[0]), "again");
    let now = open_notes(&sim);
    assert_eq!(now.len(), 1);
    assert_ne!(now[0], ids[0]);
}

#[test]
fn the_duck_for_all_clears_the_desk_with_one_toast_whose_undo_takes_all_back() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    world.agent_asks("Merge it?");
    assert!(world.stream(&mut sim));
    let duck = sim.view.desk.duck.clone().expect("the duck beside Blitz");
    assert_eq!((duck.card_ids.len(), duck.ask.en.as_str()), (2, "Duck all 2?"));
    act(&mut sim, Action::Batch { way: "sweep".into(), card_ids: duck.card_ids.clone() });
    assert!(sim.view.desk.rows.is_empty() && sim.view.desk.duck.is_none(), "cleared at once");
    for id in &duck.card_ids {
        let c = &sim.app.board().model.cards[id];
        assert_eq!((c.object_state.as_str(), c.answer.as_ref().map(|a| a.trusted)), ("answered", Some(true)), "left to the agent");
    }
    assert_eq!(sim.view.toasts.len(), 1, "one toast for all: {:?}", sim.view.toasts);
    let toast = sim.view.toasts[0].clone();
    assert_eq!((toast.text.en.as_str(), toast.line.as_ref().map(|l| l.en.as_str())), ("Left to the agents", Some("2 cards")));
    let undo = toast.action.clone().expect("Undo");
    assert_eq!(undo, Action::Batch { way: "reopen".into(), card_ids: duck.card_ids.clone() });
    serve(&world, &mut sim);
    world.stream(&mut sim);
    act(&mut sim, undo);
    world.stream(&mut sim);
    assert_eq!(sim.view.desk.rows.len(), 2, "both back on the Desk");
    assert_eq!(sim.view.toasts.len(), 1, "an Undo makes no toast of its own");
}

#[test]
fn a_note_written_and_sent_at_once_goes_as_one() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    act(&mut sim, Action::KeepNote { note_id: None, text: "ship friday".into(), attachments: vec![], send: true, to: Some(session) });
    assert!(sim.view.notes.corner.is_none(), "held for the Undo: it leaves the corner at once");
    assert_eq!(sim.view.toasts.last().map(|t| t.text.en.as_str()), Some("Note sent."));
    let held: Vec<_> = sim.app.board().model.notes.values().filter(|n| n.get("held").is_some_and(|h| !h.is_null())).collect();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0]["text"], json!("ship friday"));
}

/// The room's knowledge entry `id`, as the person's devices hold it.
fn entry(sim: &Sim, id: &str) -> serde_json::Value {
    human(sim, &format!("knowledge/{id}")).map(|(v, _)| v).unwrap_or_default()
}

/// The same in the session's group, as its agent reads it.
fn carried(sim: &Sim, session: &str, id: &str) -> serde_json::Value {
    sim.app.board().model.sessions[session].registers.get(&format!("knowledge/{id}")).map(|r| r.value.clone()).unwrap_or_default()
}

fn knowledge_page(sim: &mut Sim, query: &str, filter: &str) -> trommi_client_core::views::KnowledgeView {
    use trommi_client_core::views::{Focus, Screen};
    sim.send(Event::Focus { focus: Focus { screen: Screen::Knowledge { query: query.into(), filter: filter.into(), open: String::new() }, visible: true, ..Default::default() } });
    sim.view.knowledge.clone().expect("the page Knowledge")
}

/// The session's register `name` (model spelling), as the person's devices hold it.
fn session_register(sim: &Sim, session: &str, name: &str) -> serde_json::Value {
    sim.app.board().model.sessions[session].registers.get(name).map(|r| r.value.clone()).unwrap_or_default()
}

/// An agent's write of its team's knowledge: the register `knowledge_ask/<id>` in its session (the wire's names), at
/// the clock's time, and for a change the entry's last change as its agent reads it (`rev`).
fn agent_writes(world: &mut World, sim: &Sim, write: u8, mut value: serde_json::Value) -> String {
    value["at"] = json!(sim.now_ms);
    if let Some(entry) = value.get("entry").and_then(|e| e.as_str()).and_then(trommi_client_core::codec::hex_of_wire_id) {
        if value.get("rev").is_none() {
            let e = trommi_client_core::knowledge::entry_of(&sim.app.board().model, &entry).expect("the entry");
            value["rev"] = json!(e.changed());
        }
    }
    let id = [write; 16];
    world.agent_register(&format!("knowledge_ask/{}", trommi_core::ids::base64url_encode(&id)), &value.to_string());
    trommi_client_core::engine::feed::hex(&id)
}

/// spec/v1.md 9.3.7: an agent writes its team's knowledge without asking. The person's device applies the write on its
/// own (no card), answers it in the session, keeps a history with the text before, copies the entry to the team's
/// sessions; the page marks it new, undoes a change, brings a forgotten entry back; an agent never pins, and a write
/// that no longer fits what it read is refused and said so.
#[test]
fn an_agents_write_is_kept_on_its_own_with_its_history_and_the_way_back() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    let rows = sim.view.desk.rows.len();
    let w1 = agent_writes(&mut world, &sim, 1, json!({ "op": "add", "title": "Hub deploy", "text": "runs here", "pinned": true }));
    for _ in 0..3 {
        world.stream(&mut sim);
    }
    assert_eq!(sim.view.desk.rows.len(), rows, "no card: nothing to answer");
    let v = entry(&sim, &w1);
    assert_eq!((v["title"].clone(), v["text"].clone(), v["pinned"].clone(), v["desk"].clone()), (json!("Hub deploy"), json!("runs here"), serde_json::Value::Null, json!("main")), "{v}");
    assert!(v["by"].as_str().is_some_and(|b| !b.is_empty()), "{v}");
    assert_eq!(session_register(&sim, &session, &format!("knowledge_done/{w1}"))["ok"], json!(true));
    assert_eq!(carried(&sim, &session, &w1)["title"], json!("Hub deploy"), "its agent reads it");
    // nothing changed: nothing more is written
    let posts = envelope_posts(&world);
    world.agent_says("noted");
    world.stream(&mut sim);
    assert_eq!(envelope_posts(&world), posts);

    // new to him on his first look, with its way back
    let page = knowledge_page(&mut sim, "", "");
    assert_eq!((page.rows.len(), page.rows[0].new, page.rows[0].undo.is_some()), (1, true, true), "{:?}", page.rows);
    assert_eq!(page.filters.iter().map(|f| (f.key.as_str(), f.count)).collect::<Vec<_>>(), [("", 1), ("new", 1), ("pinned", 0), ("forgotten", 0)]);
    world.stream(&mut sim);
    leave_page(&mut sim);
    assert!(!knowledge_page(&mut sim, "", "").rows[0].new, "seen once he looked");

    // an update keeps the text before; the history shows it, and Undo brings it back
    sim.advance(1_000);
    agent_writes(&mut world, &sim, 2, json!({ "op": "update", "entry": wire(&w1), "was": "Hub deploy", "title": "Hub deploy", "text": "runs there" }));
    for _ in 0..3 {
        world.stream(&mut sim);
    }
    assert_eq!(entry(&sim, &w1)["text"], json!("runs there"));
    leave_page(&mut sim);
    let page = knowledge_open(&mut sim, &w1);
    let row = &page.rows[0];
    assert!(row.new && row.open, "{row:?}");
    assert_eq!(row.history.iter().map(|l| (l.text.as_str(), l.by_agent)).collect::<Vec<_>>(), [("runs here", true), ("", true)]);
    act(&mut sim, Action::KnowledgeUndo { change_id: row.history[0].id.clone() });
    world.stream(&mut sim);
    assert_eq!(entry(&sim, &w1)["text"], json!("runs here"));
    assert!(sim.view.toasts.is_empty(), "{:?}", sim.view.toasts);

    // a write that read another title: refused, said in the session, nothing changed
    let w3 = agent_writes(&mut world, &sim, 3, json!({ "op": "update", "entry": wire(&w1), "was": "Old", "title": "X", "text": "y" }));
    for _ in 0..2 {
        world.stream(&mut sim);
    }
    let done = session_register(&sim, &session, &format!("knowledge_done/{w3}"));
    assert_eq!((done["ok"].clone(), done["why"].clone()), (json!(false), json!("changed")));
    assert_eq!(entry(&sim, &w1)["title"], json!("Hub deploy"));
    // (the agent takes its write away: the answer goes too)
    world.agent_clears(&format!("knowledge_ask/{}", trommi_core::ids::base64url_encode(&[3; 16])));
    for _ in 0..2 {
        world.stream(&mut sim);
    }
    assert!(session_register(&sim, &session, &format!("knowledge_done/{w3}")).is_null());

    // forgotten by the agent: no longer copied, listed under Forgotten, brought back by him
    agent_writes(&mut world, &sim, 4, json!({ "op": "forget", "entry": wire(&w1), "was": "Hub deploy" }));
    for _ in 0..3 {
        world.stream(&mut sim);
    }
    assert!(entry(&sim, &w1)["deleted_at"].as_u64().is_some() && carried(&sim, &session, &w1).is_null());
    let page = knowledge_page(&mut sim, "", "forgotten");
    let restore = page.rows[0].restore.clone().expect("its way back");
    assert!(page.rows[0].forgotten && page.rows[0].gone_in.is_some());
    act(&mut sim, Action::KnowledgeUndo { change_id: restore });
    for _ in 0..2 {
        world.stream(&mut sim);
    }
    assert!(entry(&sim, &w1).get("deleted_at").is_none() && carried(&sim, &session, &w1)["title"] == json!("Hub deploy"));
    // (two writes of one title at once: the second one sees the first, in the same look)
    let (a, b) = (agent_writes(&mut world, &sim, 5, json!({ "op": "add", "title": "Twice", "text": "1" })), agent_writes(&mut world, &sim, 6, json!({ "op": "add", "title": "twice", "text": "2" })));
    for _ in 0..3 {
        world.stream(&mut sim);
    }
    let answers = [session_register(&sim, &session, &format!("knowledge_done/{a}")), session_register(&sim, &session, &format!("knowledge_done/{b}"))];
    assert_eq!(answers.iter().filter(|d| d["ok"] == json!(true)).count(), 1, "{answers:?}");
    assert!(answers.iter().any(|d| d["why"] == json!("duplicate")), "{answers:?}");
    // (only he pins)
    act(&mut sim, Action::KnowledgePin { entry_id: w1.clone(), on: true });
    world.stream(&mut sim);
    assert_eq!(entry(&sim, &w1)["pinned"], json!(true));
}

fn wire(hex: &str) -> String {
    trommi_client_core::codec::wire_of_hex_id(hex).unwrap()
}

fn leave_page(sim: &mut Sim) {
    use trommi_client_core::views::{Focus, Screen};
    sim.send(Event::Focus { focus: Focus { screen: Screen::Desk, visible: true, ..Default::default() } });
}

fn knowledge_open(sim: &mut Sim, open: &str) -> trommi_client_core::views::KnowledgeView {
    use trommi_client_core::views::{Focus, Screen};
    sim.send(Event::Focus { focus: Focus { screen: Screen::Knowledge { query: String::new(), filter: String::new(), open: open.into() }, visible: true, ..Default::default() } });
    sim.view.knowledge.clone().expect("the page Knowledge")
}

#[test]
fn the_person_writes_entries_on_the_page_and_finds_them() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let save = |title: &str, text: &str| Action::KnowledgeSave { team_id: "main".into(), entry_id: None, title: title.into(), text: text.into(), kind: String::new(), pinned: false };
    act(&mut sim, save("  Release branches ", "every change on release/<date>-<n>\n\n\n\nsquash merge"));
    act(&mut sim, save("OpenMLS", "version 0.9.1, one fixed cipher suite"));
    act(&mut sim, save("   ", "no title"));
    assert!(sim.view.toasts.iter().any(|t| t.text.key == "knowledge.no_title"));
    world.stream(&mut sim);
    let page = knowledge_page(&mut sim, "", "");
    assert_eq!(page.rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["OpenMLS", "Release branches"], "the newest first");
    assert_eq!(page.rows[1].text, "every change on release/<date>-<n>\n\nsquash merge");
    assert!(page.rows[0].by.en.starts_with("added by you"));
    assert_eq!(page.filters.iter().map(|f| (f.key.as_str(), f.count, f.on)).collect::<Vec<_>>(), [("", 2, true), ("new", 0, false), ("pinned", 0, false), ("forgotten", 0, false)]);
    assert!(page.rows.iter().all(|r| !r.new), "his own entries are not new to him");
    let found = knowledge_page(&mut sim, "squash", "");
    assert_eq!((found.rows.len(), found.rows[0].title.as_str(), found.query.as_str()), (1, "Release branches", "squash"));
    let none = knowledge_page(&mut sim, "nothing", "");
    assert_eq!(none.none.map(|t| t.en), Some("Nothing found for “nothing”.".into()));
    // edited in place: its id stays
    let id = page.rows[0].id.clone();
    act(&mut sim, Action::KnowledgeSave { team_id: "main".into(), entry_id: Some(id.clone()), title: "OpenMLS".into(), text: "=0.9.1".into(), kind: "decision".into(), pinned: true });
    world.stream(&mut sim);
    let page = knowledge_page(&mut sim, "", "");
    assert_eq!((page.rows[0].id.as_str(), page.rows[0].text.as_str(), page.rows[0].pinned, page.rows[0].kind_label.en.as_str()), (id.as_str(), "=0.9.1", true, "Decision"));
    assert_eq!(page.rows[1].kind, "fact");
    assert!(page.rows[0].confirmed.is_none(), "written and confirmed at once: said once");
    assert_eq!(page.kinds.iter().map(|k| k.key.as_str()).collect::<Vec<_>>(), ["rule", "decision", "fact", "where"]);
    // the chip on the team's Desk counts them
    use trommi_client_core::views::{Focus, Screen};
    sim.send(Event::Focus { focus: Focus { screen: Screen::Desk, visible: true, ..Default::default() } });
    assert!(sim.view.knowledge.is_none(), "the page is left");
    assert_eq!(sim.view.desk.knowledge.as_ref().map(|k| (k.count, k.label.en.as_str())), Some((2, "Knowledge")));
    // its tile shows the first titles, the pinned one first, as the page
    let page_titles: Vec<String> = page.rows.iter().map(|r| r.title.clone()).collect();
    assert_eq!(sim.view.desk.knowledge.as_ref().map(|k| k.first.clone()), Some(page_titles));
}

fn wakeups_page(sim: &mut Sim) -> trommi_client_core::views::WakeupsView {
    use trommi_client_core::views::{Focus, Screen};
    sim.send(Event::Focus { focus: Focus { screen: Screen::Wakeups, visible: true, ..Default::default() } });
    sim.view.wakeups.clone().expect("the page Wakeups")
}

/// spec/v1.md 9.3.8: an agent's wakeups (`wakeup/<id>` in its session) show on the team's page Wakeups, the next
/// first, and as a line in its chat; the person cancels one or moves it (`wakeup_edit/<id>`, shown as pending until
/// the agent's connector applied it), and his word goes once it is applied. No wakeups: the page says so.
#[test]
fn the_person_sees_the_agents_wakeups_and_cancels_or_moves_them() {
    let mut world = World::new();
    let mut sim = world.sim();
    with_card(&mut world, &mut sim);
    let session = sim.view.sidebar.sessions[0].id.clone();
    let none = wakeups_page(&mut sim);
    assert!(none.rows.is_empty() && none.none.is_some(), "no wakeups: the page says so");
    let wire = |b: u8| trommi_core::ids::base64url_encode(&[b; 16]);
    let hex = |b: u8| trommi_client_core::engine::feed::hex(&[b; 16]);
    let now = sim.now_ms;
    world.agent_register(&format!("wakeup/{}", wire(1)), &json!({ "at": now + 7_200_000, "note": "check the deploy", "set_at": now, "helper": "Testing" }).to_string());
    world.agent_register(&format!("wakeup/{}", wire(2)), &json!({ "at": now + 1_800_000, "every": 86_400_000, "note": "the daily report", "set_at": now, "last": { "at": now - 1000, "result": "ran", "detail": "" } }).to_string());
    world.stream(&mut sim);
    let page = wakeups_page(&mut sim);
    assert_eq!(page.rows.iter().map(|r| r.note.as_str()).collect::<Vec<_>>(), ["the daily report", "check the deploy"], "the next first");
    assert_eq!((page.rows[0].how_often.en.as_str(), page.rows[0].recurring, page.rows[1].helper.as_deref()), ("every day", true, Some("Testing")));
    assert!(page.rows[0].last.as_ref().is_some_and(|l| l.en.starts_with("Ran")), "{:?}", page.rows[0].last);
    assert_eq!(page.count, 2);
    sim.send(Event::Focus { focus: trommi_client_core::views::Focus { screen: trommi_client_core::views::Screen::Session { session_id: session.clone() }, ..Default::default() } });
    let line = sim.view.sessions[&session].wake_line.clone().expect("the chat's wake line");
    assert!(line.en.starts_with("Wakes at ") && line.en.contains("the daily report, and 1 more"), "{}", line.en);

    // he cancels one and moves the other: shown as pending, written as his word with its mark
    act(&mut sim, Action::WakeupCancel { session_id: session.clone(), wakeup_id: hex(2) });
    act(&mut sim, Action::WakeupMove { session_id: session.clone(), wakeup_id: hex(1), at_ms: now + 3 * 3_600_000 });
    let toasts = sim.view.toasts.len();
    act(&mut sim, Action::WakeupMove { session_id: session.clone(), wakeup_id: hex(1), at_ms: now - 1 });
    assert!(sim.view.toasts.len() > toasts, "a time gone by is refused");
    act(&mut sim, Action::WakeupCancel { session_id: session.clone(), wakeup_id: hex(9) });
    world.stream(&mut sim);
    let page = wakeups_page(&mut sim);
    assert!(page.rows.iter().all(|r| r.pending.is_some()), "{:?}", page.rows.iter().map(|r| &r.pending).collect::<Vec<_>>());
    let cancel = session_register(&sim, &session, &format!("wakeup_edit/{}", hex(2)));
    let moved = session_register(&sim, &session, &format!("wakeup_edit/{}", hex(1)));
    assert_eq!((cancel["cancel"].clone(), moved["at"].clone()), (json!(true), json!(now + 3 * 3_600_000)));
    assert!(cancel["n"].as_str().is_some_and(|n| !n.is_empty()) && cancel["n"] != moved["n"], "each word has its own mark");

    // its connector applies them: the cancelled one goes, the moved one carries his mark; his words then go
    world.agent_clears(&format!("wakeup/{}", wire(2)));
    world.agent_register(&format!("wakeup/{}", wire(1)), &json!({ "at": now + 3 * 3_600_000, "note": "check the deploy", "set_at": now, "edited": moved["n"] }).to_string());
    for _ in 0..3 {
        world.stream(&mut sim);
    }
    let page = wakeups_page(&mut sim);
    assert_eq!((page.rows.len(), page.rows[0].pending.clone()), (1, None));
    assert!(session_register(&sim, &session, &format!("wakeup_edit/{}", hex(1))).is_null(), "his applied word goes");
    assert!(session_register(&sim, &session, &format!("wakeup_edit/{}", hex(2))).is_null());
}
