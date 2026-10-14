//! Changes of the account's ways in on a device in its room (hub-api.md "The account"; spec/v1.md 8.6, 8.8): read
//! the account (`ReadAccount`), a new Emergency Kit (`NewKit`), the password (`SetPassword`), an e-mail for an
//! account without one (`SetEmail`), passkeys (`AddPasskey`, `RemovePasskey`) and the replacement of the room's
//! recovery code (`ReplaceRecoveryCode`). As the web's `account.ts` did them.
//!
//! Every change proves the way in of now once more ([`Unlock`]): the account is read, the copy of the code sealed
//! under that way is opened here, and the code must be the one in force by this device's own room state (a hub can
//! hand out an authentic OLD copy). The code, the keys and the words stay in this module: none of them is in a
//! `Debug`, a log or a stored entry; new kit words are shown once (`view.account.kit`, `step: "keep_kit"`).
//!
//! A change after the kit's words (a password or a passkey set with them) replaces the code as well (8.6): the way
//! set just now and a new kit are the ways in after it. The replacement is one outbox entry of the room group (its
//! Commit, the `RecoveryLink` and the account's new copies), whole or not at all; the new kit is shown once the hub
//! took it.
//!
//! One change at a time: a second one while one runs is refused with a toast.

use super::account_steps::Step;
use super::{log, App, Effect, LogLevel};
use crate::account::{AccountAction, Secret, Unlock};
use crate::app::bytes::Bytes;
use crate::app::io::{IoId, IoResult, PlatformAnswer, PlatformOp};
use crate::hub::{
    AccountCopies, AccountProof, AccountView as Held, Answer, Codec, CopyOfWayIn, Expect, Failure, Kdf, KdfValue, KitForm, KitPart, NewPassword,
    PasskeyAssertion, PasskeyRegistration, Route,
};
use crate::views::{AccountView, LoginView, PasskeyView, Text};
use std::collections::BTreeMap;
use trommi_core::account::{self, AccountId, AccountName, Way};
use trommi_core::crypto::{Secret as Key, SystemEntropy};
use trommi_core::ids::{base64url_decode, base64url_encode, RoomId};
use trommi_core::recovery::RecoveryKeys;

/// What a change does once the way in of now is proved.
enum Change {
    Read,
    Check,
    NewKit,
    Password { next: Secret },
    Email { email: String, words: Secret },
    ReplaceCode,
    AddPasskey,
    RemovePasskey { credential_id: Vec<u8> },
}

/// The way in of now, opened: the code in force, the proof the hub checks, and the way with its wrap key (a copy
/// of a new code is sealed under it).
struct Opened {
    code: Key<32>,
    proof: AccountProof,
    way: Owned,
}

/// A way in with its wrap key.
enum Owned {
    Password(Key<32>),
    Kit,
    Passkey { credential_id: Vec<u8>, wrap_key: Key<32> },
}

/// A passkey the platform made for `AddPasskey`, with its wrap key.
struct Created {
    credential_id: Vec<u8>,
    registration: PasskeyRegistration,
    wrap_key: Key<32>,
}

/// One change in progress.
pub(super) struct Flow {
    change: Change,
    unlock: Option<Unlock>,
    held: Option<Held>,
    /// A passkey's assertion that proves the way in, with its PRF output.
    asserted: Option<(PasskeyAssertion, Vec<u8>)>,
    opened: Option<Opened>,
    created: Option<Created>,
    /// The change's own request went out.
    sent: bool,
    /// Words of a new kit, shown once it stands.
    kit: Vec<String>,
    /// A replacement of the code (8.6): the account's revision it is made at; then its outbox entry.
    replace_at: Option<u64>,
    replacing: Option<u64>,
    /// What to say when the change is done without a new kit.
    done: Option<Text>,
}

/// An HTTP request of a change.
pub(super) enum Ask {
    Read,
    /// A challenge for a passkey: to prove the way in (`create` false) or to make a new one.
    Challenge { create: bool },
    Main,
}

/// The platform's passkey ceremonies in flight.
#[derive(Default)]
pub(super) struct Changes {
    pub(super) flow: Option<Flow>,
    platform: BTreeMap<IoId, bool>,
}

impl Changes {
    pub(super) fn owns_platform(&self, id: IoId) -> bool {
        self.platform.contains_key(&id)
    }

    /// The outbox entry of a replacement of the code, while it waits for the hub.
    pub(super) fn replacing(&self) -> Option<u64> {
        self.flow.as_ref().and_then(|f| f.replacing)
    }
}

fn problem(key: &str, en: &str) -> Text {
    Text::new(key, en)
}

/// What a refusal of the hub, or a step here, says.
fn said(code: &str) -> Text {
    match code {
        "wrong-login" => problem("account.wrong_login", "That password is not right."),
        "wrong-recovery" | "bad-recovery-words" => problem("account.wrong_kit", "Those are not the Emergency Kit's words."),
        "weak-password" => problem("account.weak_password", &format!("The password needs at least {} characters.", account::PASSWORD_MIN)),
        "bad-email" => problem("account.bad_email", "That is not an e-mail address."),
        "account-exists" => problem("account.exists", "An account with this e-mail exists."),
        "epoch-taken" | "wrong-epoch" | "group-behind" => problem("account.room_moved", "The room changed meanwhile. Try again."),
        "account-changed" => problem("account.changed", "The account changed meanwhile. Try again."),
        "needs-email" => problem("account.needs_email", "A password needs an e-mail: give the account one first."),
        "email-set" => problem("account.email_set", "This account has an e-mail already."),
        "no-password" => problem("account.no_password", "This account has no password."),
        "last-way-in" => problem("account.last_way_in", "This is the account's last way in: add another first."),
        "no-passkey" | "bad-passkey" | "no-prf" => problem("account.bad_passkey", "The passkey did not work here."),
        "refused-by-person" => problem("account.passkey_refused", "No passkey was used."),
        "rate-limited" | "throttled" => problem("account.throttled", "Too many tries. Wait a moment, then try again."),
        "offline" => problem("account.offline", "The hub cannot be reached. Try again."),
        code => Text::new("account.change_failed", &format!("That did not work ({code}).")),
    }
}

fn failure_code(failure: &Failure) -> String {
    match failure {
        Failure::Refused { code: Some(code), .. } => code.clone(),
        Failure::Refused { .. } => "refused".into(),
        Failure::NotNow { status: 429, .. } => "throttled".into(),
        Failure::NotNow { .. } | Failure::Offline => "offline".into(),
        Failure::BadAnswer { .. } => "bad-format".into(),
    }
}

/// The pinned derivation as the record a new password is stored with.
fn pinned_kdf() -> Kdf {
    let value: serde_json::Value = serde_json::from_str(account::KDF_RECORD).unwrap_or_default();
    let fields = value.as_object().map(|o| {
        o.iter()
            .filter_map(|(k, v)| match v {
                serde_json::Value::String(s) => Some((k.clone(), KdfValue::Text(s.clone()))),
                serde_json::Value::Number(n) => n.as_u64().map(|n| (k.clone(), KdfValue::Number(n))),
                _ => None,
            })
            .collect()
    });
    Kdf(fields.unwrap_or_default())
}

/// A record the hub gave, as the JSON the core's derivation checks.
fn kdf_text(kdf: &Kdf) -> String {
    let mut o = serde_json::Map::new();
    for (k, v) in &kdf.0 {
        o.insert(
            k.clone(),
            match v {
                KdfValue::Text(t) => serde_json::Value::from(t.clone()),
                KdfValue::Number(n) => serde_json::Value::from(*n),
            },
        );
    }
    serde_json::Value::Object(o).to_string()
}

fn copy61(bytes: Vec<u8>) -> Result<[u8; 61], String> {
    bytes.try_into().map_err(|_| "internal".to_string())
}

/// A new kit for `held`'s account: its words and keys, made under its e-mail where it has one, else its id.
fn new_kit(held: &Held) -> Result<(Vec<String>, account::KitKeys), String> {
    let words = account::generate_kit_words(&mut SystemEntropy).map_err(|e| e.code().to_string())?;
    let text = std::str::from_utf8(words.expose()).map_err(|_| "internal".to_string())?;
    let keys = kit_keys(held, text, None)?;
    Ok((text.split_whitespace().map(String::from).collect(), keys))
}

/// The kit's keys of `words` for `held`'s account: under `email` when given, else in the form its kit was made.
fn kit_keys(held: &Held, words: &str, email: Option<&str>) -> Result<account::KitKeys, String> {
    let id = AccountId::parse(&held.account).map_err(|e| e.code().to_string())?;
    let name = match (email, &held.email, held.kit_form) {
        (Some(email), _, _) => AccountName::Email(email),
        (None, Some(email), KitForm::Email) => AccountName::Email(email),
        (None, None, KitForm::Email) => return Err("internal".into()),
        (None, _, KitForm::Id) => AccountName::Id(&id),
    };
    account::kit_keys_for(name, words).map_err(|e| e.code().to_string())
}

/// A kit's part of a body: its login key and `code` sealed under it.
fn kit_part(keys: &account::KitKeys, room: &RoomId, code: &Key<32>) -> Result<KitPart, String> {
    let sealed = account::seal_code(&keys.wrap_key, room, Way::Kit, code, &mut SystemEntropy).map_err(|e| e.code().to_string())?;
    Ok(KitPart { auth_key: *keys.auth_key.expose(), sealed_copy: copy61(sealed)? })
}

impl App {
    /// The account's changes: true when `action` was one of them.
    pub(super) fn change_action(&mut self, now_ms: u64, action: &AccountAction, out: &mut Vec<Effect>) -> bool {
        let (change, unlock) = match action {
            AccountAction::ReadAccount => (Change::Read, None),
            AccountAction::CheckUnlock { unlock } => (Change::Check, Some(unlock.clone())),
            AccountAction::NewKit { unlock } => (Change::NewKit, Some(unlock.clone())),
            AccountAction::SetPassword { unlock, next } => (Change::Password { next: next.clone() }, Some(unlock.clone())),
            AccountAction::SetEmail { email, words } => (
                Change::Email { email: email.clone(), words: words.clone() },
                Some(Unlock::Words { words: words.clone() }),
            ),
            AccountAction::ReplaceRecoveryCode { unlock } => (Change::ReplaceCode, Some(unlock.clone())),
            AccountAction::AddPasskey { unlock } => (Change::AddPasskey, Some(unlock.clone())),
            AccountAction::RemovePasskey { credential_id, unlock } => match base64url_decode(credential_id) {
                Ok(id) if !id.is_empty() => (Change::RemovePasskey { credential_id: id }, Some(unlock.clone())),
                _ => {
                    out.push(log(LogLevel::Warn, "account: a passkey id that is not base64url"));
                    return true;
                }
            },
            _ => return false,
        };
        if self.engine.is_none() {
            out.push(log(LogLevel::Warn, "account: a change of the account without a room"));
            return true;
        }
        if self.changes.flow.is_some() {
            let text = Text::new("account.busy", "Another change of the account is still running.");
            self.sent.toast(&mut self.next_view.toasts, text, None);
            return true;
        }
        if matches!(change, Change::ReplaceCode) && matches!(unlock, Some(Unlock::Words { .. })) {
            // (after the kit the code is replaced with the new way in: SetPassword or AddPasskey)
            self.next_view.account = AccountView { step: "none".into(), problem: Some(said("wrong-recovery")), ..Default::default() };
            return true;
        }
        let quiet = matches!(change, Change::Read);
        self.changes.flow = Some(Flow { change, unlock, ..Flow::read() });
        if !quiet {
            self.next_view.account = AccountView { step: "working".into(), ..Default::default() };
        }
        self.change_send(now_ms, out, Ask::Read, |site| Codec.request(site, &Route::Account));
        true
    }

    fn change_send(
        &mut self,
        now_ms: u64,
        out: &mut Vec<Effect>,
        ask: Ask,
        build: impl FnOnce(&crate::hub::Site<'_>) -> Result<(crate::app::io::HttpRequest, Expect), Failure> + 'static,
    ) {
        if !self.step_send(now_ms, out, build, move |expect| Step::Change(ask, expect)) {
            self.changes.flow = None;
        }
    }

    fn change_failed(&mut self, code: &str) {
        let flow = self.changes.flow.take();
        if flow.as_ref().is_some_and(|f| matches!(f.change, Change::Read)) {
            return;
        }
        self.next_view.account = AccountView { step: "none".into(), problem: Some(said(code)), ..Default::default() };
    }

    fn change_done(&mut self) {
        let Some(flow) = self.changes.flow.take() else { return };
        if !flow.kit.is_empty() {
            self.next_view.account = AccountView { step: "keep_kit".into(), kit: flow.kit, ..Default::default() };
        } else {
            self.next_view.account = AccountView { step: "none".into(), ..Default::default() };
            if let Some(text) = flow.done {
                self.sent.toast(&mut self.next_view.toasts, text, None);
            }
        }
    }

    /// The hub's answer to a change's request.
    pub(super) fn change_answered(&mut self, now_ms: u64, ask: Ask, answer: Result<Answer, Failure>, out: &mut Vec<Effect>) {
        if self.changes.flow.is_none() {
            return;
        }
        let answer = match answer {
            Ok(answer) => answer,
            Err(failure) => return self.change_failed(&failure_code(&failure)),
        };
        match (ask, answer) {
            (Ask::Read, Answer::Account(held)) => {
                self.next_view.settings.account = held.email.clone();
                self.next_view.settings.login = Some(LoginView {
                    email: held.email.clone(),
                    account: held.account.clone(),
                    has_password: held.has_password,
                    kit_form: match held.kit_form {
                        KitForm::Email => "email",
                        KitForm::Id => "id",
                    }
                    .into(),
                    passkeys: held
                        .passkeys
                        .iter()
                        .map(|p| PasskeyView { credential_id: base64url_encode(&p.credential_id), created_at: p.created_at, last_used_at: p.last_used_at })
                        .collect(),
                });
                let flow = self.changes.flow.as_mut().expect("a change");
                if let Some(at) = flow.replace_at {
                    // the account read once more before a replacement: unchanged since the way in was set
                    if held.revision != at {
                        return self.change_failed("account-changed");
                    }
                    flow.held = Some(held);
                    return self.change_replace(now_ms, out);
                }
                flow.held = Some(held);
                self.change_next(now_ms, out);
            }
            (Ask::Challenge { create }, Answer::PasskeyChallenge(challenge)) => {
                let flow = self.changes.flow.as_ref().expect("a change");
                let Some(held) = flow.held.as_ref() else { return self.change_failed("internal") };
                if challenge.account != held.account {
                    return self.change_failed("bad-passkey");
                }
                let op = if create {
                    PlatformOp::PasskeyCreate {
                        rp_id: String::new(),
                        challenge: Bytes(challenge.challenge.to_vec()),
                        user_handle: Bytes(challenge.user_handle.to_vec()),
                        user_name: held.email.clone().unwrap_or_else(|| held.account.clone()),
                        prf_input: Bytes(account::PASSKEY_PRF_INPUT.to_vec()),
                    }
                } else {
                    PlatformOp::PasskeyGet {
                        rp_id: String::new(),
                        challenge: Bytes(challenge.challenge.to_vec()),
                        credential_ids: held.passkeys.iter().map(|p| Bytes(p.credential_id.clone())).collect(),
                        prf_input: Bytes(account::PASSKEY_PRF_INPUT.to_vec()),
                    }
                };
                let id = IoId(self.next_id);
                self.next_id += 1;
                self.changes.platform.insert(id, create);
                out.push(Effect::Platform { id, op });
            }
            (Ask::Main, answer) => self.change_main_answered(now_ms, answer, out),
            (_, answer) => {
                out.push(log(LogLevel::Warn, &format!("account: an answer the change did not ask for ({answer:?})")));
                self.change_failed("bad-format");
            }
        }
    }

    /// The platform's answer to a passkey ceremony.
    pub(super) fn change_platform(&mut self, now_ms: u64, id: IoId, result: IoResult<PlatformAnswer>, out: &mut Vec<Effect>) {
        let Some(create) = self.changes.platform.remove(&id) else { return };
        if self.changes.flow.is_none() {
            return;
        }
        let room = match self.engine.as_ref() {
            Some(engine) => engine.room(),
            None => return self.change_failed("internal"),
        };
        match (create, result) {
            (false, IoResult::Ok(PlatformAnswer::PasskeyAsserted { credential_id, authenticator_data, client_data_json, signature, prf, .. })) => {
                let flow = self.changes.flow.as_mut().expect("a change");
                flow.asserted = Some((
                    PasskeyAssertion { credential_id: credential_id.0, authenticator_data: authenticator_data.0, client_data_json: client_data_json.0, signature: signature.0, user_handle: None },
                    prf.0,
                ));
                self.change_next(now_ms, out);
            }
            (true, IoResult::Ok(PlatformAnswer::PasskeyCreated { credential_id, attestation_object, client_data_json, prf })) => {
                let flow = self.changes.flow.as_mut().expect("a change");
                let Some(opened) = flow.opened.as_ref() else { return self.change_failed("internal") };
                let made = account::passkey_wrap_key(&prf.0, &room, &credential_id.0)
                    .and_then(|wrap_key| {
                        let sealed = account::seal_code(&wrap_key, &room, Way::Passkey { credential_id: &credential_id.0 }, &opened.code, &mut SystemEntropy)?;
                        Ok((wrap_key, sealed))
                    })
                    .map_err(|e| e.code().to_string())
                    .and_then(|(wrap_key, sealed)| Ok((wrap_key, copy61(sealed)?)));
                match made {
                    Ok((wrap_key, sealed_copy)) => {
                        flow.created = Some(Created {
                            credential_id: credential_id.0,
                            registration: PasskeyRegistration { attestation_object: attestation_object.0, client_data_json: client_data_json.0, sealed_copy, transports: None },
                            wrap_key,
                        });
                        self.change_next(now_ms, out);
                    }
                    Err(code) => self.change_failed(&code),
                }
            }
            (_, IoResult::Ok(PlatformAnswer::Refused)) => self.change_failed("refused-by-person"),
            _ => self.change_failed("no-passkey"),
        }
    }

    /// The next step of the change in progress, once an answer came.
    fn change_next(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let Some(flow) = self.changes.flow.as_mut() else { return };
        if matches!(flow.change, Change::Read) {
            self.changes.flow = None;
            return;
        }
        if matches!(flow.unlock, Some(Unlock::Passkey)) && flow.asserted.is_none() {
            if flow.held.as_ref().is_some_and(|h| h.passkeys.is_empty()) {
                return self.change_failed("no-passkey");
            }
            return self.change_send(now_ms, out, Ask::Challenge { create: false }, |site| Codec.request(site, &Route::AccountPasskeyChallenge));
        }
        if flow.opened.is_none() {
            match self.change_open() {
                Ok(opened) => self.changes.flow.as_mut().expect("a change").opened = Some(opened),
                Err(code) => return self.change_failed(&code),
            }
        }
        let flow = self.changes.flow.as_mut().expect("a change");
        if matches!(flow.change, Change::Check) {
            self.changes.flow = None;
            self.next_view.account = AccountView { step: "unlocked".into(), ..Default::default() };
            return;
        }
        if flow.sent {
            return;
        }
        if matches!(flow.change, Change::AddPasskey) && flow.created.is_none() {
            return self.change_send(now_ms, out, Ask::Challenge { create: true }, |site| Codec.request(site, &Route::AccountPasskeyChallenge));
        }
        match self.change_request() {
            Ok(Some(request)) => {
                self.changes.flow.as_mut().expect("a change").sent = true;
                self.change_send(now_ms, out, Ask::Main, move |site| request(site));
            }
            Ok(None) => self.change_reread(now_ms, out),
            Err(code) => self.change_failed(&code),
        }
    }

    /// Opens the copy of the code under the way in given, and checks the code is the one in force.
    fn change_open(&mut self) -> Result<Opened, String> {
        let engine = self.engine.as_ref().ok_or("internal")?;
        let room = engine.room();
        let flow = self.changes.flow.as_mut().ok_or("internal")?;
        let held = flow.held.as_ref().ok_or("internal")?;
        let code = |e: account::AccountError| e.code().to_string();
        let (code_of, proof, way) = match flow.unlock.as_ref().ok_or("internal")? {
            Unlock::Password { password } => {
                let (Some(email), Some(copy)) = (held.email.as_deref(), held.password_copy.as_ref()) else { return Err("no-password".into()) };
                let kdf = held.kdf.as_ref().map(kdf_text);
                let keys = account::password_keys(email, &password.0, kdf.as_deref()).map_err(code)?;
                let opened = account::open_code(&keys.wrap_key, &room, Way::Password, copy).map_err(code)?;
                (opened, AccountProof::Password { auth_key: *keys.auth_key.expose() }, Owned::Password(keys.wrap_key))
            }
            Unlock::Words { words } => {
                let keys = kit_keys(held, &words.0, None)?;
                let opened = account::open_code(&keys.wrap_key, &room, Way::Kit, &held.kit_copy).map_err(code)?;
                (opened, AccountProof::Kit { auth_key: *keys.auth_key.expose() }, Owned::Kit)
            }
            Unlock::Passkey => {
                let (assertion, prf) = flow.asserted.as_ref().ok_or("internal")?;
                let row = held.passkeys.iter().find(|p| p.credential_id == assertion.credential_id).ok_or("wrong-login")?;
                let wrap_key = account::passkey_wrap_key(prf, &room, &assertion.credential_id).map_err(code)?;
                let opened = account::open_code(&wrap_key, &room, Way::Passkey { credential_id: &assertion.credential_id }, &row.sealed_copy).map_err(code)?;
                let way = Owned::Passkey { credential_id: assertion.credential_id.clone(), wrap_key };
                (opened, AccountProof::Passkey(assertion.clone()), way)
            }
        };
        // (a hub can hand out an authentic old copy: only the code in force is used)
        let keys = RecoveryKeys::from_code(code_of.duplicate()).map_err(|e| e.code().to_string())?;
        let history = engine.device().room_history().ok_or("internal")?;
        keys.check_room(&history.newest().room).map_err(|_| "wrong-recovery".to_string())?;
        Ok(Opened { code: code_of, proof, way })
    }

    /// The change's own request; none for a replacement of the code alone (it goes by the outbox).
    #[allow(clippy::type_complexity)]
    fn change_request(&mut self) -> Result<Option<Box<dyn FnOnce(&crate::hub::Site<'_>) -> Result<(crate::app::io::HttpRequest, Expect), Failure>>>, String> {
        let room = self.engine.as_ref().ok_or("internal")?.room();
        let flow = self.changes.flow.as_mut().ok_or("internal")?;
        let held = flow.held.as_ref().ok_or("internal")?;
        let opened = flow.opened.as_ref().ok_or("internal")?;
        let revision = held.revision;
        let proof = opened.proof.clone();
        Ok(Some(match &flow.change {
            Change::Read | Change::Check => return Ok(None),
            Change::ReplaceCode => {
                flow.replace_at = Some(revision);
                return Ok(None);
            }
            Change::NewKit => {
                let (words, keys) = new_kit(held)?;
                let kit = kit_part(&keys, &room, &opened.code)?;
                flow.kit = words;
                Box::new(move |site| Codec.request(site, &Route::PutKit { kit: &kit, revision, proof: &proof }))
            }
            Change::Password { next } => {
                account::check_password(&next.0).map_err(|e| e.code().to_string())?;
                let email = held.email.clone().ok_or("needs-email")?;
                let keys = account::password_keys(&email, &next.0, None).map_err(|e| e.code().to_string())?;
                let sealed = account::seal_code(&keys.wrap_key, &room, Way::Password, &opened.code, &mut SystemEntropy).map_err(|e| e.code().to_string())?;
                let password = NewPassword { auth_key: *keys.auth_key.expose(), sealed_copy: copy61(sealed)?, kdf: pinned_kdf() };
                // (after the kit's words the code is replaced under the new password)
                if matches!(opened.way, Owned::Kit) {
                    flow.opened.as_mut().expect("opened").way = Owned::Password(keys.wrap_key);
                }
                flow.done = Some(Text::new("account.password_set", "The password is set."));
                Box::new(move |site| Codec.request(site, &Route::PutPassword { password: &password, revision, proof: &proof }))
            }
            Change::Email { email, words } => {
                if held.email.is_some() {
                    return Err("email-set".into());
                }
                let email = account::normalise_email(email.trim()).map_err(|e| e.code().to_string())?;
                let keys = kit_keys(held, &words.0, Some(&email))?;
                let kit = kit_part(&keys, &room, &opened.code)?;
                flow.kit = std::str::from_utf8(account::parse_kit_words(&words.0).map_err(|e| e.code().to_string())?.expose())
                    .unwrap_or("")
                    .split_whitespace()
                    .map(String::from)
                    .collect();
                Box::new(move |site| Codec.request(site, &Route::PutEmail { email: &email, kit: &kit, revision, proof: &proof }))
            }
            Change::AddPasskey => {
                let created = flow.created.as_ref().ok_or("internal")?;
                let passkey = created.registration.clone();
                flow.done = Some(Text::new("account.passkey_added", "The passkey is added."));
                Box::new(move |site| Codec.request(site, &Route::AddPasskey { passkey: &passkey, proof: &proof }))
            }
            Change::RemovePasskey { credential_id } => {
                let id = credential_id.clone();
                flow.done = Some(Text::new("account.passkey_removed", "The passkey is removed."));
                Box::new(move |site| Codec.request(site, &Route::RemovePasskey { credential_id: &id, proof: &proof }))
            }
        }))
    }

    /// The answer to the change's own request.
    fn change_main_answered(&mut self, now_ms: u64, answer: Answer, out: &mut Vec<Effect>) {
        let flow = self.changes.flow.as_mut().expect("a change");
        let after_kit = matches!(flow.unlock, Some(Unlock::Words { .. }));
        match (&flow.change, answer) {
            (Change::Password { .. }, Answer::Revision { revision }) if after_kit => {
                flow.replace_at = Some(revision);
                self.change_reread(now_ms, out);
            }
            (Change::AddPasskey, Answer::PasskeyAdded { credential_id, .. }) => {
                let made = flow.created.as_ref().map(|c| c.credential_id.clone()).unwrap_or_default();
                if credential_id != made {
                    // (the hub read another id out of the attestation: the copy is not for it, it goes again)
                    let proof = flow.opened.as_ref().map(|o| o.proof.clone());
                    if let Some(proof) = proof {
                        self.step_send(now_ms, out, move |site| Codec.request(site, &Route::RemovePasskey { credential_id: &credential_id, proof: &proof }), |_| Step::Quiet);
                    }
                    return self.change_failed("bad-passkey");
                }
                if after_kit {
                    let created = flow.created.take().expect("created");
                    if let Some(opened) = flow.opened.as_mut() {
                        opened.way = Owned::Passkey { credential_id: created.credential_id, wrap_key: created.wrap_key };
                    }
                    let revision = flow.held.as_ref().map_or(0, |h| h.revision) + 1;
                    flow.replace_at = Some(revision);
                    return self.change_reread(now_ms, out);
                }
                self.change_done();
                self.change_refresh(now_ms, out);
            }
            (_, _) => {
                self.change_done();
                self.change_refresh(now_ms, out);
            }
        }
    }

    /// The page Account shows the account as it is after a change.
    fn change_refresh(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        self.changes.flow = Some(Flow::read());
        self.change_send(now_ms, out, Ask::Read, |site| Codec.request(site, &Route::Account));
    }

    /// The account read once more before the code is replaced (`replace_at` is the revision it must have).
    fn change_reread(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        self.change_send(now_ms, out, Ask::Read, |site| Codec.request(site, &Route::Account));
    }

    /// Replaces the code (8.6): a new code with its room Commit, link and the account's new copies, one entry.
    fn change_replace(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let made = self.change_copies();
        let (keys, copies, words) = match made {
            Ok(made) => made,
            Err(code) => return self.change_failed(&code),
        };
        let Some(engine) = self.engine.as_ref() else { return self.change_failed("internal") };
        let room = engine.room();
        let done = self.with_engine(now_ms, out, |engine, ctx| {
            engine.act(ctx, |d, now| {
                let history = d.room_history().ok_or(trommi_core::Error::Internal("no room history"))?;
                let replacement = keys.replace(&mut SystemEntropy, &room, history)?;
                let bytes = copies(&replacement.code).map_err(|_| trommi_core::Error::Internal("account copies"))?;
                d.replace_code(&keys, &replacement, &bytes, now)
            })
        });
        match done {
            Some(Ok(entry)) => {
                let flow = self.changes.flow.as_mut().expect("a change");
                flow.replacing = Some(entry);
                flow.kit = words;
            }
            Some(Err(e)) => self.change_failed(e.code()),
            None => self.change_failed("internal"),
        }
    }

    /// The keys of the code in force, and what seals a new code into the account's copies: the way in kept and a
    /// new kit (its words come back for the person).
    #[allow(clippy::type_complexity)]
    fn change_copies(&mut self) -> Result<(RecoveryKeys, Box<dyn FnOnce(&Key<32>) -> Result<Vec<u8>, String>>, Vec<String>), String> {
        let room = self.engine.as_ref().ok_or("internal")?.room();
        let flow = self.changes.flow.as_mut().ok_or("internal")?;
        let held = flow.held.as_ref().ok_or("internal")?;
        let opened = flow.opened.take().ok_or("internal")?;
        let revision = flow.replace_at.ok_or("internal")?;
        let (words, kit) = new_kit(held)?;
        let keys = RecoveryKeys::from_code(opened.code.duplicate()).map_err(|e| e.code().to_string())?;
        let way = opened.way;
        let seal = move |code: &Key<32>| -> Result<Vec<u8>, String> {
            let e = |e: account::AccountError| e.code().to_string();
            let way_in = match &way {
                Owned::Password(wrap) => CopyOfWayIn::Password { sealed_copy: copy61(account::seal_code(wrap, &room, Way::Password, code, &mut SystemEntropy).map_err(e)?)? },
                Owned::Passkey { credential_id, wrap_key } => CopyOfWayIn::Passkey {
                    credential_id: credential_id.clone(),
                    sealed_copy: copy61(account::seal_code(wrap_key, &room, Way::Passkey { credential_id }, code, &mut SystemEntropy).map_err(e)?)?,
                },
                Owned::Kit => return Err("internal".into()),
            };
            let copies = AccountCopies { kit: kit_part(&kit, &room, code)?, revision: Some(revision), way_in: Some(way_in) };
            crate::hub::account_copies_body(Some(&copies)).map_err(|f| failure_code(&f))
        };
        Ok((keys, Box::new(seal), words))
    }

    /// The hub took the replacement (`None`) or refused it.
    pub(super) fn change_replaced(&mut self, now_ms: u64, refused: Option<&str>, out: &mut Vec<Effect>) {
        match refused {
            None => {
                self.change_done();
                self.change_refresh(now_ms, out);
            }
            Some(code) => self.change_failed(code),
        }
    }
}

impl Flow {
    fn read() -> Self {
        Flow {
            change: Change::Read,
            unlock: None,
            held: None,
            asserted: None,
            opened: None,
            created: None,
            sent: false,
            kit: Vec::new(),
            replace_at: None,
            replacing: None,
            done: None,
        }
    }
}
