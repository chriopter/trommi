//! The account (spec/hub-api.md "The account"): logging in, the account's view, its ways in.
//!
//! Login keys and sealed copies are secrets: they go in bodies only, never in a URL or a header, and the types
//! that hold them show nothing in `Debug`. An account's id is taken only in its one printed form (a UUID, lower
//! case, with dashes), and where the answer also carries its 16 bytes the two must agree.

use super::json::{bad, Json, Obj, Read, MAX_NUMBER};
use super::limits::{
    CAP_SMALL, CAP_WRITE, MAX_CREDENTIAL_ID, MAX_EMAIL, MAX_PASSKEYS, MAX_ROOMS, SEALED_COPY,
};
use super::request::{b64, begin};
use super::types::{
    AccountCopies, AccountProof, AccountUsage, AccountView, Answer, CopyOfWayIn, Kdf, KdfValue,
    KitForm, KitPart, LoginAnswer, LoginRoom, NewAccount, NewPassword, PasskeyAssertion,
    PasskeyChallenge, PasskeyRegistration, PasskeyRow, Site,
};
use crate::app::io::HttpRequest;
use serde_json::{json, Map, Value};
use trommi_core::ids::RoomId;

const MAX_ATTESTATION: usize = 8192;
const MAX_CLIENT_DATA: usize = 4096;
const MAX_AUTHENTICATOR_DATA: usize = 4096;
const MAX_SIGNATURE: usize = 1024;

const WRONG: &str = "the request: a part of the account's body is missing or too large";

// ---- bodies

fn sized(bytes: &[u8], max: usize) -> Read<String> {
    if bytes.is_empty() || bytes.len() > max {
        return bad(WRONG);
    }
    Ok(b64(bytes))
}

fn kdf_body(kdf: &Kdf) -> Read<Value> {
    let mut out = Map::new();
    for (name, value) in &kdf.0 {
        let value = match value {
            KdfValue::Text(text) if text.len() <= 64 => json!(text),
            KdfValue::Number(n) if *n <= MAX_NUMBER => json!(n),
            _ => return bad(WRONG),
        };
        if name.is_empty() || name.len() > 16 || out.insert(name.clone(), value).is_some() {
            return bad(WRONG);
        }
    }
    if out.is_empty() || out.len() > 8 {
        return bad(WRONG);
    }
    Ok(Value::Object(out))
}

fn kit_body(kit: &KitPart) -> Value {
    json!({ "auth_key": b64(&kit.auth_key), "sealed_copy": b64(&kit.sealed_copy) })
}

fn password_body(password: &NewPassword) -> Read<Value> {
    Ok(json!({
        "auth_key": b64(&password.auth_key),
        "sealed_copy": b64(&password.sealed_copy),
        "kdf": kdf_body(&password.kdf)?,
    }))
}

fn passkey_body(passkey: &PasskeyRegistration) -> Read<Value> {
    let mut body = json!({
        "attestation_object": sized(&passkey.attestation_object, MAX_ATTESTATION)?,
        "client_data_json": sized(&passkey.client_data_json, MAX_CLIENT_DATA)?,
        "sealed_copy": b64(&passkey.sealed_copy),
    });
    if let Some(transports) = &passkey.transports {
        let plain = |t: &String| {
            !t.is_empty() && t.len() <= 16 && t.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
        };
        if transports.len() > 8 || !transports.iter().all(plain) {
            return bad(WRONG);
        }
        body["transports"] = json!(transports);
    }
    Ok(body)
}

fn assertion_body(a: &PasskeyAssertion) -> Read<Value> {
    Ok(json!({
        "credential_id": sized(&a.credential_id, MAX_CREDENTIAL_ID)?,
        "authenticator_data": sized(&a.authenticator_data, MAX_AUTHENTICATOR_DATA)?,
        "client_data_json": sized(&a.client_data_json, MAX_CLIENT_DATA)?,
        "signature": sized(&a.signature, MAX_SIGNATURE)?,
    }))
}

fn proof_body(proof: &AccountProof) -> Read<Value> {
    Ok(match proof {
        AccountProof::Password { auth_key } => json!({ "password": { "auth_key": b64(auth_key) } }),
        AccountProof::Kit { auth_key } => json!({ "kit": { "auth_key": b64(auth_key) } }),
        AccountProof::Passkey(assertion) => json!({ "passkey": assertion_body(assertion)? }),
    })
}

fn email(text: &str) -> Read<&str> {
    if text.is_empty() || text.len() > MAX_EMAIL {
        return bad("the request: an e-mail address of at most 254 bytes");
    }
    Ok(text)
}

fn account_value(account: &NewAccount) -> Read<Value> {
    if account.password.is_none() && account.passkey.is_none() {
        return bad("the request: an account has at least one way in");
    }
    let mut body = json!({ "kit": kit_body(&account.kit) });
    if let Some(address) = &account.email {
        body["email"] = json!(email(address)?);
    }
    if let Some(password) = &account.password {
        body["password"] = password_body(password)?;
    }
    if let Some(passkey) = &account.passkey {
        body["passkey"] = passkey_body(passkey)?;
    }
    Ok(body)
}

/// The body of `POST /v1/account` as bytes: what `OutboxExtra::account` carries when a room is founded with its
/// account.
pub fn new_account_body(account: &NewAccount) -> Result<Vec<u8>, super::Failure> {
    Ok(account_value(account)?.to_string().into_bytes())
}

/// The account's new sealed copies as the bytes the protocol core keeps with a `RecoveryCode` or
/// `RecoveryFinish` outbox entry (its last part): the JSON of the hub's body. A room without an account: no
/// bytes. The outbox reads them back strictly when it posts the entry.
pub fn account_copies_body(copies: Option<&AccountCopies>) -> Result<Vec<u8>, super::Failure> {
    let Some(copies) = copies else {
        return Ok(Vec::new());
    };
    let mut body = json!({ "kit": kit_body(&copies.kit) });
    match &copies.way_in {
        None => {}
        Some(CopyOfWayIn::Password { sealed_copy }) => {
            body["password"] = json!({ "sealed_copy": b64(sealed_copy) })
        }
        Some(CopyOfWayIn::NewPassword(password)) => body["password"] = password_body(password)?,
        Some(CopyOfWayIn::Passkey {
            credential_id,
            sealed_copy,
        }) => {
            body["passkey"] = json!({
                "credential_id": sized(credential_id, MAX_CREDENTIAL_ID)?,
                "sealed_copy": b64(sealed_copy),
            });
        }
        Some(CopyOfWayIn::NewPasskey(passkey)) => body["passkey"] = passkey_body(passkey)?,
    }
    if let Some(revision) = copies.revision {
        if revision > MAX_NUMBER {
            return bad(WRONG);
        }
        body["revision"] = json!(revision);
    }
    Ok(body.to_string().into_bytes())
}

// ---- requests

fn post(site: &Site<'_>, path: &str, token: bool, body: &Value) -> Read<HttpRequest> {
    Ok(begin(site, "POST", path, token, CAP_SMALL)?
        .json(body)
        .done())
}

fn put(site: &Site<'_>, path: &str, body: &Value) -> Read<HttpRequest> {
    Ok(begin(site, "PUT", path, true, CAP_WRITE)?.json(body).done())
}

/// `login` or `recover`: the one field that names an account, as typed, and the key the device derived.
pub(crate) fn login_request(
    site: &Site<'_>,
    recover: bool,
    account: &str,
    auth_key: &[u8; 32],
) -> Read<HttpRequest> {
    if account.is_empty() || account.len() > MAX_EMAIL {
        return bad("the request: an account is named by its e-mail or its id");
    }
    let path = if recover {
        "/v1/account/recover"
    } else {
        "/v1/account/login"
    };
    post(
        site,
        path,
        false,
        &json!({ "account": account, "auth_key": b64(auth_key) }),
    )
}

pub(crate) fn passkey_challenge_request(site: &Site<'_>, of_account: bool) -> Read<HttpRequest> {
    let path = if of_account {
        "/v1/account/passkeys/challenge"
    } else {
        "/v1/account/passkey/challenge"
    };
    post(site, path, of_account, &json!({}))
}

pub(crate) fn passkey_login_request(
    site: &Site<'_>,
    assertion: &PasskeyAssertion,
) -> Read<HttpRequest> {
    let mut body = assertion_body(assertion)?;
    if let Some(handle) = assertion.user_handle.as_deref().filter(|h| !h.is_empty()) {
        body["user_handle"] = json!(sized(handle, 64)?);
    }
    post(site, "/v1/account/passkey/login", false, &body)
}

pub(crate) fn create_request(site: &Site<'_>, account: &NewAccount) -> Read<HttpRequest> {
    post(site, "/v1/account", true, &account_value(account)?)
}

pub(crate) fn view_request(site: &Site<'_>) -> Read<HttpRequest> {
    Ok(begin(site, "GET", "/v1/account", true, CAP_SMALL)?.done())
}

pub(crate) fn usage_request(site: &Site<'_>) -> Read<HttpRequest> {
    Ok(begin(site, "GET", "/v1/account/usage", true, CAP_WRITE)?.done())
}

fn revision(n: u64) -> Read<u64> {
    if n > MAX_NUMBER {
        return bad(WRONG);
    }
    Ok(n)
}

pub(crate) fn put_password_request(
    site: &Site<'_>,
    password: &NewPassword,
    at: u64,
    proof: &AccountProof,
) -> Read<HttpRequest> {
    let mut body = password_body(password)?;
    body["revision"] = json!(revision(at)?);
    body["proof"] = proof_body(proof)?;
    put(site, "/v1/account/password", &body)
}

pub(crate) fn put_kit_request(
    site: &Site<'_>,
    kit: &KitPart,
    at: u64,
    proof: &AccountProof,
) -> Read<HttpRequest> {
    let mut body = kit_body(kit);
    body["revision"] = json!(revision(at)?);
    body["proof"] = proof_body(proof)?;
    put(site, "/v1/account/kit", &body)
}

pub(crate) fn put_email_request(
    site: &Site<'_>,
    address: &str,
    kit: &KitPart,
    at: u64,
    proof: &AccountProof,
) -> Read<HttpRequest> {
    let body = json!({
        "email": email(address)?,
        "kit": kit_body(kit),
        "revision": revision(at)?,
        "proof": proof_body(proof)?,
    });
    put(site, "/v1/account/email", &body)
}

pub(crate) fn add_passkey_request(
    site: &Site<'_>,
    passkey: &PasskeyRegistration,
    proof: &AccountProof,
) -> Read<HttpRequest> {
    let mut body = passkey_body(passkey)?;
    body["proof"] = proof_body(proof)?;
    Ok(
        begin(site, "POST", "/v1/account/passkeys", true, CAP_WRITE)?
            .json(&body)
            .done(),
    )
}

pub(crate) fn remove_passkey_request(
    site: &Site<'_>,
    credential_id: &[u8],
    proof: &AccountProof,
) -> Read<HttpRequest> {
    let path = format!(
        "/v1/account/passkeys/{}",
        sized(credential_id, MAX_CREDENTIAL_ID)?
    );
    Ok(begin(site, "DELETE", &path, true, CAP_WRITE)?
        .json(&json!({ "proof": proof_body(proof)? }))
        .done())
}

pub(crate) fn delete_request(site: &Site<'_>, proof: &AccountProof) -> Read<HttpRequest> {
    Ok(begin(site, "DELETE", "/v1/account", true, CAP_WRITE)?
        .json(&proof_body(proof)?)
        .done())
}

// ---- answers

fn kdf(v: Option<&Json>) -> Read<Option<Kdf>> {
    let Some(v) = v else { return Ok(None) };
    let members = v.obj("kdf: an object")?.members();
    if members.len() > 8 {
        return bad("kdf: at most 8 members");
    }
    let mut out = Vec::with_capacity(members.len());
    for (name, value) in members {
        if name.len() > 16 {
            return bad("kdf: a member's name");
        }
        out.push((
            name.clone(),
            match value {
                Json::Int(n) => KdfValue::Number(*n),
                other => KdfValue::Text(other.text(64, "kdf: texts and whole numbers")?.to_owned()),
            },
        ));
    }
    // (one order for every reader, whatever order a hub writes its members in)
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(Some(Kdf(out)))
}

fn text_opt(o: Obj<'_>, name: &str, max: usize, why: &'static str) -> Read<Option<String>> {
    o.maybe(name)
        .map(|v| v.text(max, why).map(str::to_owned))
        .transpose()
}

/// An account id in its one printed form, which is the text of `handle` when that is given.
fn account_id(v: &Json, handle: Option<&[u8; 16]>) -> Read<String> {
    let text = v.text(36, "account: an id")?;
    let shape = text.len() == 36
        && text.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        });
    if !shape {
        return bad("account: an id");
    }
    if let Some(handle) = handle {
        let hex: String = handle.iter().map(|b| format!("{b:02x}")).collect();
        if hex != text.replace('-', "") {
            return bad("account: not the id of user_handle");
        }
    }
    Ok(text.to_owned())
}

fn kit_form(v: &Json) -> Read<KitForm> {
    v.one_of(
        &[("email", KitForm::Email), ("id", KitForm::Id)],
        "kit_form",
    )
}

/// What a login answers: per room (each once) the sealed copy of its recovery code and a sign-in challenge.
pub(crate) fn login(body: &Json) -> Read<Answer> {
    let o = body.obj("a login answer: an object")?;
    let mut rooms: Vec<LoginRoom> = Vec::new();
    for v in o.need("rooms").list(MAX_ROOMS, "rooms: at most 16")? {
        let r = v.obj("a room: an object")?;
        let room = r.need("room_id").id(RoomId::from_slice, "room_id")?;
        if rooms.iter().any(|known| known.room == room) {
            return bad("a room twice");
        }
        rooms.push(LoginRoom {
            room,
            sealed_copy: r
                .need("sealed_copy")
                .fixed::<SEALED_COPY>("sealed_copy: 61 bytes")?,
            challenge: r.need("challenge").fixed("challenge: 32 bytes")?,
        });
    }
    Ok(Answer::Login(LoginAnswer {
        rooms,
        kdf: kdf(o.maybe("kdf"))?,
        account: account_id(o.need("account"), None)?,
        email: text_opt(o, "email", MAX_EMAIL, "email")?,
    }))
}

/// A passkey challenge; `of_account`: this room's account's, which also says what a new kit is salted with.
pub(crate) fn passkey_challenge(of_account: bool, body: &Json) -> Read<Answer> {
    let o = body.obj("a challenge answer: an object")?;
    let user_handle: [u8; 16] = o.need("user_handle").fixed("user_handle: 16 bytes")?;
    Ok(Answer::PasskeyChallenge(PasskeyChallenge {
        challenge: o.need("challenge").fixed("challenge: 32 bytes")?,
        account: account_id(o.need("account"), Some(&user_handle))?,
        user_handle,
        email: if of_account {
            text_opt(o, "email", MAX_EMAIL, "email")?
        } else {
            None
        },
        kit_form: if of_account {
            Some(kit_form(o.need("kit_form"))?)
        } else {
            None
        },
    }))
}

pub(crate) fn view(body: &Json) -> Read<Answer> {
    let o = body.obj("an account: an object")?;
    let user_handle: [u8; 16] = o.need("user_handle").fixed("user_handle: 16 bytes")?;
    let mut passkeys: Vec<PasskeyRow> = Vec::new();
    for v in o.need("passkeys").list(MAX_PASSKEYS, "passkeys")? {
        let k = v.obj("a passkey: an object")?;
        let mut transports = Vec::new();
        if let Some(list) = k.maybe("transports") {
            for t in list.list(8, "transports")? {
                transports.push(t.text(16, "a transport")?.to_owned());
            }
        }
        let credential_id = k
            .need("credential_id")
            .bytes(1, MAX_CREDENTIAL_ID, "credential_id")?
            .0;
        if passkeys
            .iter()
            .any(|known| known.credential_id == credential_id)
        {
            return bad("a passkey twice");
        }
        passkeys.push(PasskeyRow {
            credential_id,
            sealed_copy: k
                .need("sealed_copy")
                .fixed::<SEALED_COPY>("sealed_copy: 61 bytes")?,
            transports,
            created_at: k.need("created_at").int("created_at")?,
            last_used_at: k
                .maybe("last_used_at")
                .map(|v| v.int("last_used_at"))
                .transpose()?,
        });
    }
    let mut rooms: Vec<RoomId> = Vec::new();
    for v in o.need("rooms").list(MAX_ROOMS, "rooms: at most 16")? {
        let room = v.id(RoomId::from_slice, "a room")?;
        if rooms.contains(&room) {
            return bad("a room twice");
        }
        rooms.push(room);
    }
    Ok(Answer::Account(AccountView {
        email: text_opt(o, "email", MAX_EMAIL, "email")?,
        account: account_id(o.need("account"), Some(&user_handle))?,
        kit_form: kit_form(o.need("kit_form"))?,
        revision: o.need("revision").int("revision")?,
        has_password: o.need("has_password").bool("has_password")?,
        kdf: kdf(o.maybe("kdf"))?,
        password_copy: o
            .maybe("password_copy")
            .map(|v| v.fixed::<SEALED_COPY>("password_copy: 61 bytes"))
            .transpose()?,
        kit_copy: o
            .need("kit_copy")
            .fixed::<SEALED_COPY>("kit_copy: 61 bytes")?,
        user_handle,
        passkeys,
        rooms,
    }))
}

pub(crate) fn usage(body: &Json) -> Read<Answer> {
    let o = body.obj("the account's usage: an object")?;
    let n = |name: &str| o.need(name).int("usage: counts and sizes");
    Ok(Answer::Usage(AccountUsage {
        rooms: n("rooms")?,
        groups: n("groups")?,
        live_groups: n("live_groups")?,
        devices: n("devices")?,
        commits: n("commits")?,
        messages: n("messages")?,
        envelopes: n("envelopes")?,
        cards: n("cards")?,
        open_cards: n("open_cards")?,
        files: n("files")?,
        file_bytes: n("file_bytes")?,
        log_bytes: n("log_bytes")?,
        envelope_bytes: n("envelope_bytes")?,
        total_bytes: n("total_bytes")?,
    }))
}

pub(crate) fn revised(body: &Json) -> Read<Answer> {
    let o = body.obj("a revision: an object")?;
    Ok(Answer::Revision {
        revision: o.need("revision").int("revision")?,
    })
}

pub(crate) fn passkey_added(body: &Json) -> Read<Answer> {
    let o = body.obj("a passkey answer: an object")?;
    Ok(Answer::PasskeyAdded {
        credential_id: o
            .need("credential_id")
            .bytes(1, MAX_CREDENTIAL_ID, "credential_id")?
            .0,
        created_at: o.need("created_at").int("created_at")?,
    })
}
