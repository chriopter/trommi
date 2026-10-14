//! Making an account with a new room through the App (`account::found`, `engine::found`): the room is founded on
//! the stand-in hub with the account, the kit's words are shown once, the device is live in its room and signs in
//! again with the password; an e-mail that has an account is refused and leaves the slot empty.

mod common;

use common::hub::{Hub, HUB_URL};
use common::now;
use std::cell::RefCell;
use std::rc::Rc;
use trommi_client_core::account::{AccountAction, Secret};
use trommi_client_core::actions::Action;
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Config, Event};
use trommi_client_core::views::Phase;

const PASSWORD: &str = "correct horse battery staple";

fn blank(hub: &Rc<RefCell<Hub>>) -> Sim {
    let mut sim = Sim::new(now());
    let h = hub.clone();
    sim.hub = Some(Box::new(move |now, request| h.borrow_mut().http(now, request)));
    let h = hub.clone();
    sim.stream_hub = Some(Box::new(move |now, request| h.borrow_mut().open_stream(now, request)));
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), hub: HUB_URL.into(), seed: 7, ..Default::default() } });
    assert_eq!(sim.view.phase, Phase::SignedOut);
    sim
}

fn account(sim: &mut Sim, action: AccountAction) {
    sim.send(Event::Action { action: Action::Account { action } });
}

fn settle(hub: &Rc<RefCell<Hub>>, sim: &mut Sim) {
    for _ in 0..60 {
        if let Some(me) = sim.app.engine().map(|e| e.device().id()) {
            for _ in 0..16 {
                let Some(bytes) = hub.borrow_mut().stream_news(&me) else { break };
                sim.stream(StreamItem::Chunk { bytes: bytes.into() });
            }
        }
        if sim.view.phase == Phase::Live {
            return;
        }
        sim.advance(1_000);
    }
}

#[test]
fn an_account_is_made_with_its_room_and_signs_in_again() {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let mut sim = blank(&hub);
    account(&mut sim, AccountAction::Found { email: "Ada@Example.org".into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&hub, &mut sim);
    assert_eq!(sim.view.account.step, "keep_kit", "{:?}", sim.view.account.problem);
    assert_eq!(sim.view.account.kit.len(), 12, "twelve words");
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(hub.borrow().accounts.len(), 1, "the account was made with the room");
    for effect in &sim.log {
        assert!(!format!("{effect:?}").contains(PASSWORD));
    }
    assert!(sim.view.account.kit_due, "the kit waits to be saved");
    sim.advance(100);
    let pending = |sim: &Sim| sim.app.board().model.human.get("kit").map(|r| r.value.clone());
    assert_eq!(pending(&sim), Some(serde_json::json!({ "pending": true })), "the room register says it");

    // another slot signs in to it with the password: the kit waits there too
    let mut other = blank(&hub);
    account(&mut other, AccountAction::SignIn { email: "ada@example.org".into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&hub, &mut other);
    assert_eq!(other.view.phase, Phase::Live, "{:?}", other.view.account.problem);
    assert_eq!(other.app.engine().map(|e| e.room()), sim.app.engine().map(|e| e.room()));
    assert!(other.view.account.kit_due, "every device of the person sees the kit is not saved");

    account(&mut sim, AccountAction::KitKept);
    assert_eq!(sim.view.account.step, "none");
    assert!(sim.view.account.kit.is_empty());
    assert!(!sim.view.account.kit_due, "kept");
    sim.advance(100);
    assert_eq!(pending(&sim), Some(serde_json::Value::Null));
}

#[test]
fn an_email_with_an_account_is_refused_and_nothing_stays() {
    let hub = Rc::new(RefCell::new(Hub::default()));
    let mut first = blank(&hub);
    account(&mut first, AccountAction::Found { email: "ada@example.org".into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&hub, &mut first);
    // a second room for the same e-mail: the stand-in, as the hub, refuses the account
    let accounts = std::mem::take(&mut hub.borrow_mut().accounts);
    let mut fresh = Hub::default();
    fresh.accounts = accounts;
    *hub.borrow_mut() = fresh;
    let mut second = blank(&hub);
    account(&mut second, AccountAction::Found { email: "ada@example.org".into(), password: Secret(PASSWORD.into()), hub: None });
    settle(&hub, &mut second);
    assert_eq!(second.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("account.exists"), "{:?}", second.view.account);
    assert_eq!(second.view.phase, Phase::SignedOut);
    assert!(second.app.engine().is_none());
    assert!(second.device.1.is_empty(), "the device's state is gone");
    // a weak password is said before anything is made
    account(&mut second, AccountAction::Found { email: "bob@example.org".into(), password: Secret("short".into()), hub: None });
    assert_eq!(second.view.account.problem.as_ref().map(|p| p.key.as_str()), Some("account.weak_password"));
}
