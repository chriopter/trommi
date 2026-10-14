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
    act(&mut sim, Action::HandBack { card_id: card.clone(), note: Some("what do you mean?".into()), rework: false, marks: vec![] });
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
    let desks = |sim: &Sim| sim.app.board().model.human.iter().filter(|(k, r)| k.starts_with("desk/") && !r.value.is_null()).map(|(k, r)| (k.clone(), r.value.clone())).collect::<Vec<_>>();
    act(&mut sim, Action::AddDesk { name: "  Work  ".into() });
    let made = desks(&sim);
    assert_eq!(made.len(), 2, "the first desk and the new one: {made:?}");
    assert_eq!(made.iter().find(|(k, _)| k == "desk/main").map(|d| d.1.clone()), Some(json!({ "name": "Personal", "created_at": 0 })));
    let (work, _) = made.iter().find(|(k, _)| k != "desk/main").cloned().unwrap();
    let work = work.trim_start_matches("desk/").to_string();
    assert_eq!(work.len(), 32);
    world.stream(&mut sim);
    act(&mut sim, Action::RenameDesk { desk_id: work.clone(), name: "Job".into() });
    act(&mut sim, Action::SetGoal { desk_id: work.clone(), text: "ship\n\n  launch \n".into() });
    let v = desks(&sim).into_iter().find(|(k, _)| k.ends_with(&work)).unwrap().1;
    assert_eq!((v["name"].clone(), v["goals"].clone()), (json!("Job"), json!("ship\nlaunch")));
    let session = sim.view.sidebar.sessions[0].id.clone();
    act(&mut sim, Action::MoveSession { session_id: session.clone(), desk_id: work.clone() });
    act(&mut sim, Action::RenameSession { session_id: session.clone(), name: "Builder".into() });
    let s = human(&sim, &format!("session/{session}")).unwrap().0;
    assert_eq!((s["desk"].clone(), s["name"].clone()), (json!(work.clone()), json!("Builder")));
    world.stream(&mut sim);
    act(&mut sim, Action::RemoveDesk { desk_id: "main".into() });
    assert_eq!(desks(&sim).len(), 1);
    act(&mut sim, Action::RemoveDesk { desk_id: work.clone() });
    assert_eq!(sim.view.toasts.last().map(|t| t.text.key.as_str()), Some("action.last_desk"));
    assert_eq!(desks(&sim).len(), 1);
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
    act(&mut sim, Action::SetGoal { desk_id: "main".into(), text: "ship it\nwrite the docs".into() });
    world.stream(&mut sim);
    assert!(held(&sim).is_null(), "the session is on no desk yet");
    act(&mut sim, Action::MoveSession { session_id: session.clone(), desk_id: "main".into() });
    world.stream(&mut sim);
    assert_eq!(held(&sim), json!({ "desk_id": "main", "desk_name": "Personal", "goals": "ship it\nwrite the docs" }));
    // nothing changed: nothing more is written
    let posts = envelope_posts(&world);
    world.agent_says("still here");
    world.stream(&mut sim);
    assert_eq!(envelope_posts(&world), posts);
    // the goals cleared: the session's copy goes
    act(&mut sim, Action::SetGoal { desk_id: "main".into(), text: "  ".into() });
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
    // (a room without desks keeps its one crown)
    let crown = human(&sim, "crown").map(|v| v.0).unwrap_or_default();
    assert_eq!(crown["session_id"], json!(session), "{crown}");
    act(&mut sim, Action::AddDesk { name: "Work".into() });
    world.stream(&mut sim);
    let ids: Vec<String> = sim.app.board().model.human.keys().filter_map(|k| k.strip_prefix("desk/").map(String::from)).collect();
    let reversed: Vec<String> = ids.iter().rev().cloned().collect();
    act(&mut sim, Action::OrderDesks { desk_ids: reversed.clone() });
    for (i, d) in reversed.iter().enumerate() {
        assert_eq!(human(&sim, &format!("desk/{d}")).unwrap().0["order"], json!(i));
    }
    act(&mut sim, Action::RenameDevice { name: "  Laptop ".into() });
    let me = sim.app.board().model.my_device_id.clone().unwrap();
    assert_eq!(human(&sim, &format!("device/{me}")).unwrap().0["device_name"], json!("Laptop"));
    // "Revise" and "take back" go to the card's chat
    sim.hold = true;
    act(&mut sim, Action::HandBack { card_id: card.clone(), note: Some("again".into()), rework: true, marks: vec![json!({ "picture": 0, "x": 1, "y": 2, "w": 3, "h": 4 })] });
    act(&mut sim, Action::TakeBack { card_id: card.clone() });
    let items: Vec<serde_json::Value> = sim.app.board().model.timelines[&format!("chat:card/{card}")].items.values().cloned().map(serde_json::Value::Object).collect();
    assert!(items.iter().any(|i| i["content"]["hand_back"] == json!(true) && i["content"]["marks"].as_array().is_some_and(|m| m.len() == 1)), "{items:?}");
    assert!(items.iter().any(|i| i["content"]["present_card"] == json!(true)), "{items:?}");
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
