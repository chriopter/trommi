//! Signing in to the account (account/signin): the password or the Emergency Kit opens the copy of the recovery
//! code the hub keeps, and the room is joined with it, against the stand-in hub (tests/common).

mod common;

use common::hub::{Account, HUB_URL};
use common::{now, start, World};
use trommi_client_core::account::{AccountAction, Secret};
use trommi_client_core::actions::Action;
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Config, Effect, Event};
use trommi_client_core::engine::room_join::RoomJoinStatus;
use trommi_client_core::views::Phase;
use trommi_core::account::{self, Way};
use trommi_core::crypto::{Secret as Key, SystemEntropy};

const EMAIL: &str = "Ada@Example.org";
const PASSWORD: &str = "correct horse battery staple";
const ACCOUNT_ID: &str = "b3909710-0275-4714-a504-a872deaddff8";

/// The world's room with an account whose copies of the code (the world's, 0xC0…) the hub keeps. Returns the kit's
/// words.
fn with_account(world: &World) -> String {
    let room = world.room.room_id();
    let code = Key::new([0xC0; 32]);
    let pw = account::password_keys(EMAIL, PASSWORD, Some(account::KDF_RECORD)).expect("the password's keys");
    let words = account::generate_kit_words(&mut SystemEntropy).expect("words");
    let words = String::from_utf8(words.expose().to_vec()).expect("text");
    let kit = account::kit_keys(EMAIL, &words).expect("the kit's keys");
    let sealed_password = account::seal_code(&pw.wrap_key, &room, Way::Password, &code, &mut SystemEntropy).expect("sealed");
    let sealed_kit = account::seal_code(&kit.wrap_key, &room, Way::Kit, &code, &mut SystemEntropy).expect("sealed");
    world.hub.borrow_mut().accounts.push(Account {
        name: account::normalise_email(EMAIL).unwrap(),
        id: ACCOUNT_ID.into(),
        password_auth: *pw.auth_key.expose(),
        kit_auth: *kit.auth_key.expose(),
        sealed_password,
        sealed_kit,
    });
    words
}

fn blank(world: &World, hub: &str) -> Sim {
    let mut sim = Sim::new(now());
    world.attach(&mut sim);
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), hub: hub.into(), seed: 7, ..Default::default() } });
    assert_eq!(sim.view.phase, Phase::SignedOut);
    sim
}

fn act(sim: &mut Sim, action: AccountAction) {
    sim.send(Event::Action { action: Action::Account { action } });
}

fn settle(world: &World, sim: &mut Sim) {
    for _ in 0..200 {
        if let Some(me) = sim.app.engine().map(|e| e.device().id()) {
            for _ in 0..16 {
                let Some(bytes) = world.hub.borrow_mut().stream_news(&me) else { break };
                sim.stream(StreamItem::Chunk { bytes: bytes.into() });
            }
        }
        if sim.view.phase == Phase::Live && sim.app.sessions_not_joined().is_some() {
            return;
        }
        if sim.view.account.problem.is_some() {
            return;
        }
        sim.advance(1_000);
    }
}

fn no_secret_shown(sim: &Sim, words: &str) {
    let first = words.split_whitespace().next().unwrap_or("-");
    for effect in &sim.log {
        let shown = match effect {
            Effect::Log { text, .. } => text.clone(),
            Effect::Render { delta } => format!("{delta:?}") + &serde_json::to_string(delta).unwrap_or_default(),
            other => format!("{other:?}"),
        };
        assert!(!shown.contains(PASSWORD) && !shown.contains(&format!("{first} ")), "{shown}");
    }
}

#[test]
fn the_password_opens_the_code_and_the_room_is_joined() {
    let world = World::new();
    let words = with_account(&world);
    let mut sim = blank(&world, HUB_URL);
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret(PASSWORD.into()), hub: None });
    assert!(sim.log.iter().any(|e| matches!(e, Effect::Render { delta } if format!("{delta:?}").contains("step: \"working\""))), "working is shown");
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined), "{:?}", sim.view.account.problem);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.view.account.step, "none");
    assert!(sim.app.engine().unwrap().device().group(&world.group).is_ok(), "and its session");
    no_secret_shown(&sim, &words);
}

#[test]
fn the_kit_opens_the_code_with_the_hub_named_in_the_action() {
    let world = World::new();
    let words = with_account(&world);
    let mut sim = blank(&world, "");
    act(&mut sim, AccountAction::SignInWithKit { email: EMAIL.into(), words: Secret(words.clone()), hub: Some(format!("{HUB_URL}/")) });
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined), "{:?}", sim.view.account.problem);
    assert_eq!(sim.app.engine().map(|e| e.hub().to_string()), Some(HUB_URL.to_string()));
    no_secret_shown(&sim, &words);
}

#[test]
fn a_wrong_password_is_said_and_nothing_is_stored() {
    let world = World::new();
    with_account(&world);
    let mut sim = blank(&world, HUB_URL);
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret("not the password at all".into()), hub: None });
    settle(&world, &mut sim);
    assert_eq!(sim.view.account.step, "sign_in");
    assert_eq!(sim.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("account.wrong_login"));
    assert!(sim.app.engine().is_none() && sim.device.1.is_empty());
    // bad words and a bad address are said before anything is sent
    let sent = world.hub.borrow().seen.len();
    act(&mut sim, AccountAction::SignInWithKit { email: EMAIL.into(), words: Secret("one two three".into()), hub: None });
    assert_eq!(sim.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("account.bad_words"));
    act(&mut sim, AccountAction::SignIn { email: "no address".into(), password: Secret(PASSWORD.into()), hub: None });
    assert_eq!(sim.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("account.bad_account"));
    assert_eq!(world.hub.borrow().seen.len(), sent);
    // and the right password afterwards
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
}

#[test]
fn an_unreachable_hub_is_said_and_can_be_tried_again() {
    let world = World::new();
    with_account(&world);
    let mut sim = blank(&world, HUB_URL);
    let hub = sim.hub.take();
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret(PASSWORD.into()), hub: None });
    assert_eq!(sim.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("account.offline"));
    sim.hub = hub;
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&world, &mut sim);
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    let _ = start;
}

#[test]
fn a_password_signin_resumes_unfinished_sessions_on_its_existing_room_leaf() {
    let world = World::new();
    let words = with_account(&world);
    let mut sim = blank(&world, HUB_URL);
    sim.hold = true;
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret(PASSWORD.into()), hub: None });
    for _ in 0..60 {
        if sim.app.room_join() == Some(RoomJoinStatus::Joined) { break; }
        let (id, request) = sim.held.front().cloned().expect("the next sign-in request");
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
    assert_eq!(sim.app.room_join(), Some(RoomJoinStatus::Joined));
    let id = sim.app.engine().unwrap().device().id();
    sim.restart();
    sim.hold = false;
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.view.account.problem.as_ref().unwrap().key, "account.interrupted");
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret("wrong password".into()), hub: None });
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert_eq!(sim.app.engine().unwrap().device().id(), id);
    assert!(sim.view.account.problem.is_some());
    act(&mut sim, AccountAction::SignIn { email: EMAIL.into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&world, &mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.app.sessions_not_joined(), Some(&[][..]));
    assert!(sim.app.engine().unwrap().device().group(&world.group).is_ok());
    assert_eq!(sim.app.engine().unwrap().device().id(), id);
    no_secret_shown(&sim, &words);
}
