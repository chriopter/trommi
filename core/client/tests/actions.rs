//! The person's actions through the App (`Event::Action`), against the stand-in hub (tests/common): the echo in the
//! very next `Render`, replaced by the hub's copy, rolled back with a toast that offers the action again when the hub
//! refuses it, and sent once across a restart. (A write lost before it was stored is rolled back when the engine
//! opens the stored state again in place: that test comes with the engine's reopening.)

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
    Action::Answer { card_id: card.into(), choices: vec!["yes".into()], note: None, attachments: vec![], option_notes: Default::default(), trusted: false }
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
    act(&mut sim, Action::Shred { card_id: card.clone(), note: None });
    assert_eq!(answer_of(&sim, &card).map(|a| a.0), Some("closed".into()));
    assert_eq!(sim.app.board().model.cards[&card].answer.as_ref().unwrap().answer_action, "shred");
}

#[test]
fn what_cannot_be_done_is_said_and_changes_nothing() {
    let mut world = World::new();
    let mut sim = world.sim();
    let card = with_card(&mut world, &mut sim);
    let posts = envelope_posts(&world);
    let wrong = Action::Answer { card_id: card.clone(), choices: vec!["maybe".into()], note: None, attachments: vec![], option_notes: Default::default(), trusted: false };
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
        AccountAction::Found { email: "a@example.org".into(), password: Secret(secret.into()) },
        AccountAction::SignIn { email: "a@example.org".into(), password: Secret(secret.into()) },
        AccountAction::SignInWithKit { email: "a@example.org".into(), words: Secret(secret.into()) },
        AccountAction::Join { link: Secret(secret.into()) },
        AccountAction::DeleteAccount { password: Secret(secret.into()) },
    ];
    for action in actions {
        let event = Event::Action { action: Action::Account { action } };
        let shown = format!("{event:?} {event:#?}");
        assert!(!shown.contains(secret) && !shown.contains("example.org"), "{shown}");
    }
}
