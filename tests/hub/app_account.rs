//! The app core's changes of the account's ways in (core/client `app::account_change`) against the real hub: a
//! founded account gets a new kit, a new password, a passkey, a replaced recovery code and a password after the
//! kit's words, and each way in signs in on a new slot afterwards; a wrong password changes nothing.

mod common;

use common::passkey::{Authenticator, ORIGIN, RP, UP_UV};
use common::*;
use std::io::{BufReader, Read};
use std::time::Duration;
use trommi_client_core::account::{AccountAction, Secret, Unlock};
use trommi_client_core::actions::Action;
use trommi_client_core::app::bytes::Bytes;
use trommi_client_core::app::io::{IoResult, PlatformAnswer, PlatformOp};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Config, Event, Header, HttpResponse, StreamItem};
use trommi_client_core::views::Phase;

const EMAIL: &str = "ada@example.org";

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

fn headers_of(request: &trommi_client_core::app::HttpRequest) -> Vec<(&str, String)> {
    request.headers.iter().filter(|h| h.name != "trommi-client").map(|h| (h.name.as_str(), h.value.clone())).collect()
}

/// A slot of the app on this hub, with a passkey authenticator as its platform.
fn slot(hub: &TestHub, key: std::rc::Rc<Authenticator>) -> Sim {
    let mut sim = Sim::new(now());
    let (port, url) = (hub.port, hub.url.clone());
    sim.hub = Some(Box::new(move |_, request| {
        let path = request.url.strip_prefix(&url).expect("this hub").to_string();
        let body = request.body.as_ref().map_or(&[][..], |b| &b[..]);
        let reply = request_of(port, &request.method, &path, &headers_of(request), body);
        Ok(HttpResponse { status: reply.status, headers: reply.headers.iter().map(|(n, v)| Header { name: n.clone(), value: v.clone() }).collect(), body: reply.body.into() })
    }));
    let (port, url) = (hub.port, hub.url.clone());
    sim.stream_hub = Some(Box::new(move |_, request| {
        // the head and what the hub sends at once; what follows is read again by the changes
        let path = request.url.strip_prefix(&url).expect("this hub").to_string();
        let socket = send_request(port, &request.method, &path, &headers_of(request), &[]);
        socket.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut reader = BufReader::new(socket);
        let (status, headers) = read_head(&mut reader);
        let mut bytes = vec![0u8; 65536];
        let n = reader.read(&mut bytes).unwrap_or(0);
        bytes.truncate(n);
        vec![
            StreamItem::Opened { status, headers: headers.into_iter().map(|(name, value)| Header { name, value }).collect() },
            StreamItem::Chunk { bytes: bytes.into() },
        ]
    }));
    let prf = [7u8; 32];
    sim.platform = Some(Box::new(move |op| match op {
        PlatformOp::PasskeyCreate { challenge, .. } => {
            let client = Authenticator::client_data("webauthn.create", &challenge.0, ORIGIN);
            IoResult::Ok(PlatformAnswer::PasskeyCreated {
                credential_id: Bytes(key.credential_id.clone()),
                attestation_object: Bytes(key.attestation(RP, UP_UV)),
                client_data_json: Bytes(client),
                prf: Bytes(prf.to_vec()),
            })
        }
        PlatformOp::PasskeyGet { challenge, .. } => {
            let client = Authenticator::client_data("webauthn.get", &challenge.0, ORIGIN);
            let (data, signature) = key.assertion(RP, UP_UV, &client);
            IoResult::Ok(PlatformAnswer::PasskeyAsserted {
                credential_id: Bytes(key.credential_id.clone()),
                authenticator_data: Bytes(data),
                client_data_json: Bytes(client),
                signature: Bytes(signature),
                user_handle: None,
                prf: Bytes(prf.to_vec()),
            })
        }
        _ => IoResult::Ok(PlatformAnswer::Refused),
    }));
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), hub: hub.url.clone(), seed: 7, ..Default::default() } });
    sim
}

fn request_of(port: u16, method: &str, path: &str, headers: &[(&str, String)], body: &[u8]) -> Reply {
    request(port, method, path, headers, body)
}

fn account(sim: &mut Sim, action: AccountAction) {
    sim.send(Event::Action { action: Action::Account { action } });
}

/// Moves the clock until `done` holds (a few rounds of the engine's timers).
fn until(sim: &mut Sim, what: &str, done: impl Fn(&Sim) -> bool) {
    for _ in 0..200 {
        if done(sim) {
            return;
        }
        sim.advance(500);
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("{what}: never; account view {:?}, phase {:?}, toasts {:?}", sim.view.account, sim.view.phase, sim.view.toasts);
}

/// The stream here brings only what the hub has at its opening: ended, the engine opens it again and reads what
/// other devices did meanwhile.
fn catch_up(sim: &mut Sim) {
    sim.stream(StreamItem::Ended);
    for _ in 0..20 {
        sim.advance(1_000);
    }
}

fn secret(s: &str) -> Secret {
    Secret(s.into())
}

fn kit(sim: &mut Sim) -> String {
    until(sim, "a kit to keep", |s| s.view.account.step == "keep_kit" || s.view.account.problem.is_some());
    assert_eq!(sim.view.account.step, "keep_kit", "{:?}", sim.view.account.problem);
    let words = sim.view.account.kit.join(" ");
    assert_eq!(sim.view.account.kit.len(), 12);
    account(sim, AccountAction::KitKept);
    words
}

fn done(sim: &mut Sim) {
    until(sim, "the change done", |s| s.view.account.step == "none");
    assert_eq!(sim.view.account.problem, None);
}

fn signs_in(hub: &TestHub, action: AccountAction) {
    signs_in_with(hub, std::rc::Rc::new(Authenticator::new()), action)
}

fn signs_in_with(hub: &TestHub, key: std::rc::Rc<Authenticator>, action: AccountAction) {
    let mut sim = slot(hub, key);
    account(&mut sim, action);
    until(&mut sim, "signed in", |s| s.view.phase == Phase::Live || s.view.account.problem.is_some());
    assert_eq!(sim.view.phase, Phase::Live, "{:?}", sim.view.account.problem);
}

#[test]
fn every_way_in_is_changed_and_signs_in_after() {
    let hub = TestHub::start();
    let key = std::rc::Rc::new(Authenticator::new());
    let mut sim = slot(&hub, key.clone());
    account(&mut sim, AccountAction::Found { email: EMAIL.into(), password: secret("first password here"), hub: None });
    let _founded = kit(&mut sim);
    until(&mut sim, "live", |s| s.view.phase == Phase::Live);

    account(&mut sim, AccountAction::ReadAccount);
    until(&mut sim, "the account read", |s| s.view.settings.login.is_some());
    let login = sim.view.settings.login.clone().unwrap();
    assert!(login.has_password && login.passkeys.is_empty());
    assert_eq!((login.email.as_deref(), login.kit_form.as_str()), (Some(EMAIL), "email"));

    // a wrong password changes nothing
    account(&mut sim, AccountAction::NewKit { unlock: Unlock::Password { password: secret("not the password") } });
    until(&mut sim, "refused", |s| s.view.account.problem.is_some());
    assert_eq!(sim.view.account.problem.as_ref().unwrap().key, "account.wrong_login");

    let kit1 = {
        account(&mut sim, AccountAction::NewKit { unlock: Unlock::Password { password: secret("first password here") } });
        kit(&mut sim)
    };
    account(&mut sim, AccountAction::SetPassword { unlock: Unlock::Password { password: secret("first password here") }, next: secret("second password here") });
    done(&mut sim);
    signs_in(&hub, AccountAction::SignIn { email: EMAIL.into(), password: secret("second password here"), hub: None });
    signs_in(&hub, AccountAction::SignInWithKit { email: EMAIL.into(), words: secret(&kit1), hub: None });

    account(&mut sim, AccountAction::AddPasskey { unlock: Unlock::Password { password: secret("second password here") } });
    done(&mut sim);
    until(&mut sim, "the passkey listed", |s| s.view.settings.login.as_ref().is_some_and(|l| l.passkeys.len() == 1));

    // the code replaced with the passkey as the way in: the password and the old kit open nothing after it
    catch_up(&mut sim);
    account(&mut sim, AccountAction::ReplaceRecoveryCode { unlock: Unlock::Passkey });
    let kit2 = kit(&mut sim);
    until(&mut sim, "the account read again", |s| s.view.settings.login.as_ref().is_some_and(|l| !l.has_password));
    signs_in_with(&hub, key.clone(), AccountAction::SignInWithPasskey { hub: None });

    // a password after the kit's words: the code is replaced once more, with a new kit
    catch_up(&mut sim);
    account(&mut sim, AccountAction::SetPassword { unlock: Unlock::Words { words: secret(&kit2) }, next: secret("third password here") });
    let kit3 = kit(&mut sim);
    assert_ne!(kit3, kit2);
    signs_in(&hub, AccountAction::SignIn { email: EMAIL.into(), password: secret("third password here"), hub: None });
    signs_in(&hub, AccountAction::SignInWithKit { email: EMAIL.into(), words: secret(&kit3), hub: None });
}

#[test]
fn the_room_is_recovered_with_the_kit() {
    let hub = TestHub::start();
    let mut first = slot(&hub, std::rc::Rc::new(Authenticator::new()));
    account(&mut first, AccountAction::Found { email: EMAIL.into(), password: secret("first password here"), hub: None });
    let kit1 = kit(&mut first);
    until(&mut first, "live", |s| s.view.phase == Phase::Live);
    signs_in(&hub, AccountAction::SignIn { email: EMAIL.into(), password: secret("first password here"), hub: None });

    // every device is lost: the kit recovers the room on a new one, with a new password and a new kit
    let mut new = slot(&hub, std::rc::Rc::new(Authenticator::new()));
    account(&mut new, AccountAction::Recover { email: EMAIL.into(), words: secret(&kit1), password: secret("second password here"), hub: None });
    let kit2 = kit(&mut new);
    assert_ne!(kit1, kit2);
    until(&mut new, "live", |s| s.view.phase == Phase::Live);
    let (humans, _) = new.app.engine().expect("in the room").roles();
    assert_eq!(humans, vec![new.app.engine().unwrap().device().id()], "the other human devices are out");

    signs_in(&hub, AccountAction::SignIn { email: EMAIL.into(), password: secret("second password here"), hub: None });
    signs_in(&hub, AccountAction::SignInWithKit { email: EMAIL.into(), words: secret(&kit2), hub: None });
    // the old password and the old kit open nothing any more
    let mut old = slot(&hub, std::rc::Rc::new(Authenticator::new()));
    account(&mut old, AccountAction::SignIn { email: EMAIL.into(), password: secret("first password here"), hub: None });
    until(&mut old, "refused", |s| s.view.account.problem.is_some());
}
