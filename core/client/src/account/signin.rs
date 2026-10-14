//! Signing in to the account (`AccountAction::SignIn`, `SignInWithKit`) on a slot without a device in a room: the
//! keys of the password or of the Emergency Kit are derived, the hub's login answers with the room and the copy of
//! the recovery code sealed under that way in, the copy is opened, and the room is joined with the code
//! ([`crate::app::App::join_room`]). As the web's `room.ts` and the Linux app's `engine/signin.rs` do it.
//!
//! The login routes take no token, so the App sends them itself (one request, built by the codec with its
//! `max_answer`); a failure is shown and the person tries again. The password, the words, the keys and the code
//! stay in this module and in the join: none of them is in a `Debug`, a log, a view or a stored entry. The slow
//! step (Argon2id, about a second, optimised in every build) runs inside the action's `handle`.

use super::Secret;
use crate::hub::{Answer, Codec, Expect, Failure, Route, Site};
use crate::views::Text;
use trommi_core::account::{self, AccountError, AccountId, AccountName, Way};
use trommi_core::crypto::Secret as Key;
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::RoomId;
use trommi_core::recovery::RecoveryKeys;

/// The way in a sign-in uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    Password,
    Kit,
    Passkey,
}

/// A sign-in waiting for the hub's login answer. Holds the wrap key only (wiped when dropped).
pub(crate) struct Waiting {
    pub(crate) hub: String,
    pub(crate) by: By,
    wrap_key: Wrap,
    pub(crate) expect: Expect,
}

/// What opens the copy of the code: a wrap key, or a passkey's PRF output, whose wrap key is bound to the room the
/// answer names.
enum Wrap {
    Key(Key<32>),
    Passkey { prf: Key<32>, credential_id: Vec<u8> },
}

/// Why a sign-in did not get to the join.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The e-mail or the password is wrong (`wrong-login`).
    WrongLogin,
    /// The account or the words are wrong (`wrong-recovery`).
    WrongKit,
    /// Not an e-mail address, or neither an e-mail nor an account id.
    BadAccount,
    /// Not twelve words of the list.
    BadWords,
    /// The hub address is none.
    BadHub,
    /// The account names no room.
    NoRoom,
    /// The hub was not reached, or not now: try again.
    Offline,
    /// Too many tries: wait.
    Throttled,
    /// The hub refused with this code, or answered what the protocol does not allow.
    Other(&'static str),
}

impl Problem {
    pub fn text(&self) -> Text {
        use crate::views::words::say;
        match self {
            Problem::WrongLogin => say("account.wrong_login", "The e-mail or the password is wrong.", &[]),
            Problem::WrongKit => say("account.wrong_kit", "The account or the Emergency Kit words are wrong.", &[]),
            Problem::BadAccount => say("account.bad_account", "That is not an e-mail address or an account id.", &[]),
            Problem::BadWords => say("account.bad_words", "The Emergency Kit has twelve words from its list.", &[]),
            Problem::BadHub => say("account.bad_hub", "That is not a hub address.", &[]),
            Problem::NoRoom => say("account.no_room", "This account has no room yet.", &[]),
            Problem::Offline => say("account.offline", "The hub cannot be reached. Try again.", &[]),
            Problem::Throttled => say("account.throttled", "Too many tries. Wait a moment, then try again.", &[]),
            Problem::Other(code) => say("account.refused", format!("Signing in did not work ({code})."), &[("code", code)]),
        }
    }
}

/// The hub address of a sign-in: the action's, else the shell's, else the default; canonical.
pub(crate) fn hub_of(action: Option<&str>, config: &str) -> Result<String, Problem> {
    let typed = action.map(str::trim).filter(|h| !h.is_empty()).unwrap_or(config);
    let typed = if typed.is_empty() { crate::app::DEFAULT_HUB } else { typed };
    HubAddress::parse(typed.trim_end_matches('/')).map(|a| a.as_str().to_string()).map_err(|_| Problem::BadHub)
}

fn sorted(e: AccountError, by: By) -> Problem {
    match e.code() {
        "bad-email" => Problem::BadAccount,
        "bad-recovery-words" => Problem::BadWords,
        "wrong-login" => Problem::WrongLogin,
        "wrong-recovery" if by == By::Kit => Problem::WrongKit,
        code => Problem::Other(code),
    }
}

/// Derives the keys and builds the login request. `account`: the e-mail (password), or e-mail or account id (kit).
pub(crate) fn begin(hub: String, by: By, account: &str, secret: &Secret, client: &str) -> Result<(Waiting, crate::app::io::HttpRequest), Problem> {
    let account = account.trim();
    let (named, auth_key, wrap_key) = match by {
        By::Passkey => return Err(Problem::Other("internal")),
        By::Password => {
            let email = account::normalise_email(account).map_err(|e| sorted(e, by))?;
            let keys = account::password_keys(&email, &secret.0, None).map_err(|e| sorted(e, by))?;
            (email, keys.auth_key, keys.wrap_key)
        }
        By::Kit => {
            let (name, named) = if account.contains('@') {
                let email = account::normalise_email(account).map_err(|e| sorted(e, by))?;
                (None, email)
            } else {
                let id = AccountId::parse(account).map_err(|_| Problem::BadAccount)?;
                (Some(id), id.to_string())
            };
            let keys = match &name {
                Some(id) => account::kit_keys_for(AccountName::Id(id), &secret.0),
                None => account::kit_keys_for(AccountName::Email(&named), &secret.0),
            }
            .map_err(|e| sorted(e, by))?;
            (named, keys.auth_key, keys.wrap_key)
        }
    };
    let site = Site { url: &hub, token: None, client, lease: None };
    let auth: [u8; 32] = *auth_key.expose();
    let route = match by {
        By::Password | By::Passkey => Route::Login { account: &named, auth_key: &auth },
        By::Kit => Route::Recover { account: &named, auth_key: &auth },
    };
    let (request, expect) = Codec.request(&site, &route).map_err(|_| Problem::BadAccount)?;
    Ok((Waiting { hub, by, wrap_key: Wrap::Key(wrap_key), expect }, request))
}

/// The challenge of a sign-in with a passkey (no token).
pub(crate) fn passkey_challenge(hub: &str, client: &str) -> Result<(crate::app::io::HttpRequest, Expect), Problem> {
    let site = Site { url: hub, token: None, client, lease: None };
    Codec.request(&site, &Route::PasskeyChallenge).map_err(|_| Problem::BadHub)
}

/// The login with a passkey's assertion; `prf`: its PRF output (32 bytes).
pub(crate) fn passkey_begin(hub: String, assertion: crate::hub::PasskeyAssertion, prf: &[u8], client: &str) -> Result<(Waiting, crate::app::io::HttpRequest), Problem> {
    let prf = Key::from_slice(prf).map_err(|_| Problem::Other("no-prf"))?;
    let site = Site { url: &hub, token: None, client, lease: None };
    let (request, expect) = Codec.request(&site, &Route::PasskeyLogin { assertion: &assertion }).map_err(|_| Problem::Other("bad-passkey"))?;
    let credential_id = assertion.credential_id;
    Ok((Waiting { hub, by: By::Passkey, wrap_key: Wrap::Passkey { prf, credential_id }, expect }, request))
}

/// The hub's login answer: the room and the keys of its code, or why not.
pub(crate) fn answered(waiting: &Waiting, answer: Result<Answer, Failure>) -> Result<(RoomId, RecoveryKeys), Problem> {
    let login = match answer {
        Ok(Answer::Login(login)) => login,
        Ok(_) => return Err(Problem::Other("bad-format")),
        Err(Failure::Refused { code: Some(code), .. }) => {
            return Err(match code.as_str() {
                "wrong-login" if waiting.by == By::Password => Problem::WrongLogin,
                "wrong-login" | "wrong-recovery" => Problem::WrongKit,
                "rate-limited" | "throttled" => Problem::Throttled,
                "not-found" => Problem::NoRoom,
                other => Problem::Other(trommi_core::Error::from_code(other).map_or("refused", |e| e.code())),
            });
        }
        Err(Failure::Refused { .. }) => return Err(Problem::Other("refused")),
        // (the hub's throttle after a wrong try answers 429 with the wait: said as such, not as "offline")
        Err(Failure::NotNow { status: 429, .. }) => return Err(Problem::Throttled),
        Err(Failure::NotNow { .. } | Failure::Offline) => return Err(Problem::Offline),
        Err(Failure::BadAnswer { .. }) => return Err(Problem::Other("bad-format")),
    };
    let room = login.rooms.first().ok_or(Problem::NoRoom)?;
    let code = match &waiting.wrap_key {
        Wrap::Key(key) => {
            let way = if waiting.by == By::Kit { Way::Kit } else { Way::Password };
            account::open_code(key, &room.room, way, &room.sealed_copy)
        }
        Wrap::Passkey { prf, credential_id } => account::passkey_wrap_key(prf.expose(), &room.room, credential_id)
            .and_then(|key| account::open_code(&key, &room.room, Way::Passkey { credential_id }, &room.sealed_copy)),
    }
    .map_err(|e| sorted(e, waiting.by))?;
    let keys = RecoveryKeys::from_code(code).map_err(|e| Problem::Other(e.code()))?;
    Ok((room.room, keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting() -> Waiting {
        let (waiting, _) = begin("http://127.0.0.1:1".into(), By::Password, "ada@example.org", &Secret("pw".into()), "test/0").expect("begun");
        waiting
    }

    #[test]
    fn the_throttle_is_said_as_such() {
        let w = waiting();
        assert_eq!(answered(&w, Err(Failure::NotNow { status: 429, retry_after_ms: Some(1_000) })).err(), Some(Problem::Throttled));
        assert_eq!(answered(&w, Err(Failure::NotNow { status: 503, retry_after_ms: None })).err(), Some(Problem::Offline));
        assert_eq!(answered(&w, Err(Failure::Offline)).err(), Some(Problem::Offline));
    }
}
