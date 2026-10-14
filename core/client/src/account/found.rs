//! Making an account with a new room (`AccountAction::Found`; spec/v1.md 8.9): the e-mail and the password are
//! checked, the password's keys and a new Emergency Kit's are derived, and a fresh recovery code is made; the room is
//! founded under the code's keys (`engine::found`), and the account's body binds the room: the code sealed under the
//! password and under the kit. As the web's `account.ts createAccount`. The kit's words are shown once
//! (`view.account.kit`, `step: "keep_kit"`) and never stored; the code never leaves the core.

use super::signin::Problem;
use super::Secret;
use crate::views::Text;
use serde_json::json;
use trommi_core::account::{self, Way};
use trommi_core::crypto::{Entropy, Secret as Key, SecretBytes};
use trommi_core::ids::RoomId;
use trommi_core::recovery::RecoveryKeys;

/// The way in an account is founded with.
enum FoundWay {
    Password(account::PasswordKeys),
    /// A passkey the platform made on the hub's challenge, which named the account's id.
    Passkey { credential_id: Vec<u8>, attestation_object: Vec<u8>, client_data_json: Vec<u8>, prf: Key<32> },
}

/// Everything a founding needs once the room's id is known. Wiped when dropped.
pub(crate) struct Founding {
    pub(crate) hub: String,
    email: Option<String>,
    way: FoundWay,
    kit: account::KitKeys,
    pub(crate) words: SecretBytes,
    code: Key<32>,
    pub(crate) keys: RecoveryKeys,
}

/// Checks and derives (Argon2id: about a second); nothing is sent.
pub(crate) fn begin(hub: String, email: &str, password: &Secret, entropy: &mut dyn Entropy) -> Result<Founding, Text> {
    let email = account::normalise_email(email.trim()).map_err(|_| Problem::BadAccount.text())?;
    account::check_password(&password.0).map_err(|e| match e.code() {
        "weak-password" => Text::new("account.weak_password", &format!("The password needs at least {} characters.", account::PASSWORD_MIN)),
        code => Text::new("account.refused", &format!("That password cannot be used ({code}).")),
    })?;
    let pw = account::password_keys(&email, &password.0, None).map_err(|_| Text::new("account.refused", "That password cannot be used."))?;
    let words = account::generate_kit_words(entropy).map_err(|_| Text::new("account.refused", "No Emergency Kit could be made."))?;
    let text = std::str::from_utf8(words.expose()).map_err(|_| Text::new("account.refused", "No Emergency Kit could be made."))?.to_string();
    let kit = account::kit_keys(&email, &text).map_err(|_| Text::new("account.refused", "No Emergency Kit could be made."))?;
    let (code, keys) = RecoveryKeys::generate(entropy).map_err(|_| Text::new("account.refused", "No recovery code could be made."))?;
    Ok(Founding { hub, email: Some(email), way: FoundWay::Password(pw), kit, words, code, keys })
}

/// A founding with a passkey (no password): `account` is the id the hub's challenge named, the kit is salted with
/// the e-mail where one is given, else with that id.
#[allow(clippy::too_many_arguments)]
pub(crate) fn begin_with_passkey(
    hub: String,
    email: Option<&str>,
    account_id: &str,
    credential_id: Vec<u8>,
    attestation_object: Vec<u8>,
    client_data_json: Vec<u8>,
    prf: &[u8],
    entropy: &mut dyn Entropy,
) -> Result<Founding, Text> {
    let refused = || Text::new("account.refused", "No account could be made with this passkey.");
    let email = match email.map(str::trim).filter(|e| !e.is_empty()) {
        Some(e) => Some(account::normalise_email(e).map_err(|_| Problem::BadAccount.text())?),
        None => None,
    };
    let id = account::AccountId::parse(account_id).map_err(|_| refused())?;
    let prf = Key::from_slice(prf).map_err(|_| Text::new("account.bad_passkey", "This passkey cannot keep a key (no PRF)."))?;
    let words = account::generate_kit_words(entropy).map_err(|_| refused())?;
    let text = std::str::from_utf8(words.expose()).map_err(|_| refused())?.to_string();
    let kit = match &email {
        Some(e) => account::kit_keys(e, &text),
        None => account::kit_keys_for(account::AccountName::Id(&id), &text),
    }
    .map_err(|_| refused())?;
    let (code, keys) = RecoveryKeys::generate(entropy).map_err(|_| refused())?;
    let way = FoundWay::Passkey { credential_id, attestation_object, client_data_json, prf };
    Ok(Founding { hub, email, way, kit, words, code, keys })
}

impl Founding {
    /// The account's body for `POST /v1/rooms` (the copies of the code bind the room).
    pub(crate) fn account_body(&self, room: &RoomId, entropy: &mut dyn Entropy) -> Result<Vec<u8>, &'static str> {
        let b64 = trommi_core::ids::base64url_encode;
        let sealed_kit = account::seal_code(&self.kit.wrap_key, room, Way::Kit, &self.code, entropy).map_err(|e| e.code())?;
        let mut body = json!({ "kit": { "auth_key": b64(self.kit.auth_key.expose()), "sealed_copy": b64(&sealed_kit) } });
        if let Some(email) = &self.email {
            body["email"] = json!(email);
        }
        match &self.way {
            FoundWay::Password(password) => {
                let sealed = account::seal_code(&password.wrap_key, room, Way::Password, &self.code, entropy).map_err(|e| e.code())?;
                let kdf: serde_json::Value = serde_json::from_str(account::KDF_RECORD).map_err(|_| "internal")?;
                body["password"] = json!({ "auth_key": b64(password.auth_key.expose()), "sealed_copy": b64(&sealed), "kdf": kdf });
            }
            FoundWay::Passkey { credential_id, attestation_object, client_data_json, prf } => {
                let wrap = account::passkey_wrap_key(prf.expose(), room, credential_id).map_err(|e| e.code())?;
                let sealed = account::seal_code(&wrap, room, Way::Passkey { credential_id }, &self.code, entropy).map_err(|e| e.code())?;
                let sealed: [u8; 61] = sealed.try_into().map_err(|_| "internal")?;
                let passkey = crate::hub::PasskeyRegistration { attestation_object: attestation_object.clone(), client_data_json: client_data_json.clone(), sealed_copy: sealed, transports: None };
                let new = crate::hub::NewAccount {
                    email: None,
                    kit: crate::hub::KitPart { auth_key: [0; 32], sealed_copy: [0; 61] },
                    password: None,
                    passkey: Some(passkey),
                };
                let made: serde_json::Value = serde_json::from_slice(&crate::hub::new_account_body(&new).map_err(|_| "internal")?).map_err(|_| "internal")?;
                body["passkey"] = made.get("passkey").cloned().ok_or("internal")?;
            }
        }
        serde_json::to_vec(&body).map_err(|_| "internal")
    }

    /// The kit's words, one by one, for the person to keep.
    pub(crate) fn kit_words(&self) -> Vec<String> {
        std::str::from_utf8(self.words.expose()).unwrap_or("").split_whitespace().map(String::from).collect()
    }
}

/// The account's new copies of a recovery (8.7): a new password and a new kit under `email`. Checked and derived
/// here (Argon2id); the new code is sealed once the recovery made it. Returns the new kit's words and the sealer.
pub(crate) fn recovery_copies(email: &str, password: &Secret, entropy: &mut dyn Entropy) -> Result<(Vec<String>, crate::engine::recover::Copies), Text> {
    let email = account::normalise_email(email.trim()).map_err(|_| Problem::BadAccount.text())?;
    account::check_password(&password.0).map_err(|e| match e.code() {
        "weak-password" => Text::new("account.weak_password", &format!("The password needs at least {} characters.", account::PASSWORD_MIN)),
        code => Text::new("account.refused", &format!("That password cannot be used ({code}).")),
    })?;
    let pw = account::password_keys(&email, &password.0, None).map_err(|_| Text::new("account.refused", "That password cannot be used."))?;
    let words = account::generate_kit_words(entropy).map_err(|_| Text::new("account.refused", "No Emergency Kit could be made."))?;
    let text = std::str::from_utf8(words.expose()).map_err(|_| Text::new("account.refused", "No Emergency Kit could be made."))?.to_string();
    let kit = account::kit_keys(&email, &text).map_err(|_| Text::new("account.refused", "No Emergency Kit could be made."))?;
    let shown = text.split_whitespace().map(String::from).collect();
    let seal: crate::engine::recover::Copies = Box::new(move |code: &Key<32>, room: &RoomId| {
        let mut entropy = trommi_core::crypto::SystemEntropy;
        let sealed_password = account::seal_code(&pw.wrap_key, room, Way::Password, code, &mut entropy).map_err(|e| e.code())?;
        let sealed_kit = account::seal_code(&kit.wrap_key, room, Way::Kit, code, &mut entropy).map_err(|e| e.code())?;
        let sixty_one = |v: Vec<u8>| -> Result<[u8; 61], &'static str> { v.try_into().map_err(|_| "internal") };
        let kdf: serde_json::Value = serde_json::from_str(account::KDF_RECORD).map_err(|_| "internal")?;
        let kdf = crate::hub::Kdf(
            kdf.as_object()
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, v)| match v {
                            serde_json::Value::String(s) => Some((k.clone(), crate::hub::KdfValue::Text(s.clone()))),
                            serde_json::Value::Number(n) => n.as_u64().map(|n| (k.clone(), crate::hub::KdfValue::Number(n))),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        );
        let copies = crate::hub::AccountCopies {
            kit: crate::hub::KitPart { auth_key: *kit.auth_key.expose(), sealed_copy: sixty_one(sealed_kit)? },
            revision: None,
            way_in: Some(crate::hub::CopyOfWayIn::NewPassword(crate::hub::NewPassword { auth_key: *pw.auth_key.expose(), sealed_copy: sixty_one(sealed_password)?, kdf })),
        };
        crate::hub::account_copies_body(Some(&copies)).map_err(|_| "internal")
    });
    Ok((shown, seal))
}
