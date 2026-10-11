//! A change of a way in needs the way in of now, proved once more (spec/hub-api.md "The account"): a stolen token
//! alone sets no password, kit, e-mail or passkey and removes none. The proof is checked in the same request, as
//! for deleting the account, and a wrong one counts with the logins and makes its source wait.

mod common;

use common::passkey::{Authenticator, ORIGIN, RP, UP_UV};
use common::*;
use serde_json::{json, Value};
use trommi_hub::util::{b64, random, unb64};

fn copy(mark: u8) -> String {
    let mut c = vec![mark; 61];
    c[0] = 2;
    b64(&c)
}

fn kdf() -> Value {
    json!({ "alg": "argon2id", "v": 1, "m": 65536, "t": 3, "p": 1 })
}

fn db(hub: &TestHub) -> rusqlite::Connection {
    rusqlite::Connection::open(hub.dir.join("hub.db")).unwrap()
}

/// The test's own source forgets its waits, so the next refusal is checked at once.
fn forget_waits(hub: &TestHub) {
    db(hub).execute_batch("DELETE FROM login_sources; DELETE FROM login_turns;").unwrap();
}

fn by_password(auth: &[u8; 32]) -> Value {
    json!({ "password": { "auth_key": b64(auth) } })
}
fn by_kit(kit: &[u8; 32]) -> Value {
    json!({ "kit": { "auth_key": b64(kit) } })
}

fn with(mut body: Value, proof: Value) -> Value {
    body["proof"] = proof;
    body
}

fn revision(w: &World) -> i64 {
    w.ada.get(&w.hub, "/v1/account").ok()["revision"].as_i64().unwrap()
}

/// A challenge of `POST /v1/account/passkeys/challenge`: for this account at its revision, two minutes, one use.
fn account_challenge(w: &World) -> Vec<u8> {
    unb64(w.ada.post(&w.hub, "/v1/account/passkeys/challenge", &json!({})).ok()["challenge"].as_str().unwrap()).unwrap()
}

fn registration(key: &Authenticator, challenge: &[u8]) -> Value {
    json!({
        "attestation_object": b64(&key.attestation(RP, UP_UV)),
        "client_data_json": b64(&Authenticator::client_data("webauthn.create", challenge, ORIGIN)),
        "sealed_copy": copy(3), "transports": ["internal"],
    })
}

fn by_passkey(key: &Authenticator, challenge: &[u8]) -> Value {
    let client = Authenticator::client_data("webauthn.get", challenge, ORIGIN);
    let (data, signature) = key.assertion(RP, UP_UV, &client);
    json!({ "passkey": { "credential_id": b64(&key.credential_id), "authenticator_data": b64(&data), "client_data_json": b64(&client), "signature": b64(&signature) } })
}

/// What the hub holds of the account's ways in: compared before and after a refused change.
fn ways(hub: &TestHub) -> Vec<Option<Vec<u8>>> {
    let c = db(hub);
    let mut out: Vec<Option<Vec<u8>>> = c
        .query_row("SELECT auth_hash, kit_hash, password_copy, kit_copy, email FROM accounts", [], |r| {
            Ok(vec![r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, Option<String>>(4)?.map(String::into_bytes)])
        })
        .unwrap();
    let mut keys = c.prepare("SELECT credential_id FROM passkeys ORDER BY credential_id").unwrap();
    out.extend(keys.query_map([], |r| r.get(0)).unwrap().map(|k| Some(k.unwrap())));
    out
}

#[test]
fn a_token_alone_changes_no_way_in() {
    let w = World::new();
    let (auth, kit) = (random::<32>(), random::<32>());
    w.ada.post(&w.hub, "/v1/account", &json!({
        "email": "ada@example.org",
        "kit": { "auth_key": b64(&kit), "sealed_copy": copy(9) },
        "password": { "auth_key": b64(&auth), "sealed_copy": copy(7), "kdf": kdf() },
    })).ok();
    // a passkey beside the password, added with the password proved
    let key = Authenticator::new();
    let challenge = account_challenge(&w);
    w.ada.post(&w.hub, "/v1/account/passkeys", &with(registration(&key, &challenge), by_password(&auth))).ok();
    let before = ways(&w.hub);
    let r = revision(&w);
    let hub = &w.hub;

    // ---- the token alone: every change of a way in is refused, and nothing is changed
    let attacker = random::<32>();
    let password = json!({ "auth_key": b64(&attacker), "sealed_copy": copy(1), "kdf": kdf(), "revision": r });
    let new_kit = json!({ "auth_key": b64(&attacker), "sealed_copy": copy(1), "revision": r });
    let stranger = Authenticator::new();
    let gone = format!("/v1/account/passkeys/{}", b64(&key.credential_id));
    w.ada.put(hub, "/v1/account/password", &password).refused(400, "bad-format");
    w.ada.put(hub, "/v1/account/kit", &new_kit).refused(400, "bad-format");
    w.ada.post(hub, "/v1/account/passkeys", &registration(&stranger, &account_challenge(&w))).refused(400, "bad-format");
    w.ada.call(hub, "DELETE", &gone, &Value::Null).refused(400, "bad-format");
    w.ada.call(hub, "DELETE", &gone, &json!({})).refused(400, "bad-format");
    // a proof that is no proof object, or names nothing
    w.ada.put(hub, "/v1/account/password", &with(password.clone(), json!("yes"))).refused(400, "bad-format");
    w.ada.put(hub, "/v1/account/password", &with(password.clone(), json!({}))).refused(400, "bad-format");
    assert_eq!(ways(hub), before, "a token alone changed a way in");

    // ---- a wrong proof: refused as a wrong login, and its source then waits (the login's lasting back-off, on
    // the account, whichever route it uses: a change and a deletion count together)
    w.ada.put(hub, "/v1/account/password", &with(password.clone(), by_password(&random()))).refused(401, "wrong-login");
    let waiting = w.ada.put(hub, "/v1/account/kit", &with(new_kit.clone(), by_kit(&kit)));
    waiting.refused(429, "rate-limited");
    assert!(waiting.header("retry-after").is_some(), "a wait says how long");
    w.ada.call(hub, "DELETE", "/v1/account", &by_kit(&kit)).refused(429, "rate-limited");
    let waits: i64 = db(hub).query_row("SELECT count(*) FROM login_sources WHERE failures > 0", [], |r| r.get(0)).unwrap();
    assert_eq!(waits, 1, "the failure is on record, in the database");
    forget_waits(hub);
    // the kit's key is not the password's, a passkey on a challenge the hub did not give is no proof, nor one
    // on a sign-in challenge (not this account's)
    w.ada.put(hub, "/v1/account/password", &with(password.clone(), by_password(&kit))).refused(401, "wrong-login");
    forget_waits(hub);
    w.ada.call(hub, "DELETE", &gone, &json!({ "proof": by_passkey(&key, &[1; 32]) })).refused(401, "wrong-login");
    forget_waits(hub);
    let sign_in = unb64(hub.post("/v1/account/passkey/challenge", &json!({})).ok()["challenge"].as_str().unwrap()).unwrap();
    w.ada.call(hub, "DELETE", &gone, &json!({ "proof": by_passkey(&key, &sign_in) })).refused(401, "wrong-login");
    forget_waits(hub);
    // a passkey that is not the account's, on the account's challenge
    w.ada.call(hub, "DELETE", &gone, &json!({ "proof": by_passkey(&stranger, &account_challenge(&w)) })).refused(401, "wrong-login");
    forget_waits(hub);
    // the account's passkey, but its challenge used twice (the first use holds, and the change is refused after
    // it for its stale revision)
    let proof = by_passkey(&key, &account_challenge(&w));
    let mut stale = new_kit.clone();
    stale["revision"] = json!(r - 1);
    w.ada.put(hub, "/v1/account/kit", &with(stale, proof.clone())).refused(409, "account-changed");
    assert_eq!(revision(&w), r);
    w.ada.call(hub, "DELETE", &gone, &json!({ "proof": proof })).refused(401, "wrong-login");
    forget_waits(hub);
    assert_eq!(ways(hub), before, "a wrong proof changed a way in");
    // an agent does not get as far as a proof
    let mut w = w;
    let agent = w.enrol_agent();
    agent.put(&w.hub, "/v1/account/password", &with(password.clone(), by_password(&auth))).refused(403, "forbidden");
}

#[test]
fn with_the_way_in_of_now_proved_each_change_is_made() {
    let w = World::new();
    let hub = &w.hub;
    // an account of a passkey only, without e-mail: its passkey and its kit are its proofs
    let key = Authenticator::new();
    let start = hub.post("/v1/account/passkey/challenge", &json!({})).ok();
    let first = unb64(start["challenge"].as_str().unwrap()).unwrap();
    let kit = random::<32>();
    w.ada.post(hub, "/v1/account", &json!({ "kit": { "auth_key": b64(&kit), "sealed_copy": copy(9) }, "passkey": registration(&key, &first) })).ok();

    // a second passkey: proved with the first; the proof and the registration each take a challenge of their own
    let second = Authenticator::new();
    let (proving, registering) = (account_challenge(&w), account_challenge(&w));
    w.ada.post(hub, "/v1/account/passkeys", &with(registration(&second, &registering), by_passkey(&key, &proving))).ok();
    // an e-mail, proved with the kit; the kit is made anew under it in the same request
    let mail_kit = random::<32>();
    w.ada.put(hub, "/v1/account/email", &json!({
        "email": "ada@example.org", "kit": { "auth_key": b64(&mail_kit), "sealed_copy": copy(4) }, "revision": revision(&w), "proof": by_kit(&kit),
    })).ok();
    // the kit of before proves nothing now
    let auth = random::<32>();
    let password = |w: &World, proof: Value| with(json!({ "auth_key": b64(&auth), "sealed_copy": copy(5), "kdf": kdf(), "revision": revision(w) }), proof);
    w.ada.put(hub, "/v1/account/password", &password(&w, by_kit(&kit))).refused(401, "wrong-login");
    forget_waits(hub);
    // a password, proved with a passkey on the account's challenge
    let proving = account_challenge(&w);
    w.ada.put(hub, "/v1/account/password", &password(&w, by_passkey(&second, &proving))).ok();
    // a challenge given before a change proves nothing after it (it was for the account at its revision then)
    let stale = account_challenge(&w);
    let next = random::<32>();
    w.ada.put(hub, "/v1/account/password", &with(json!({ "auth_key": b64(&next), "sealed_copy": copy(6), "kdf": kdf(), "revision": revision(&w) }), by_password(&auth))).ok();
    w.ada.put(hub, "/v1/account/kit", &json!({ "auth_key": b64(&random::<32>()), "sealed_copy": copy(7), "revision": revision(&w), "proof": by_passkey(&key, &stale) })).refused(401, "wrong-login");
    forget_waits(hub);
    // the password of before proves nothing; the one of now does
    let new_kit = random::<32>();
    let kit_body = |w: &World, proof: Value| json!({ "auth_key": b64(&new_kit), "sealed_copy": copy(7), "revision": revision(w), "proof": proof });
    w.ada.put(hub, "/v1/account/kit", &kit_body(&w, by_password(&auth))).refused(401, "wrong-login");
    forget_waits(hub);
    w.ada.put(hub, "/v1/account/kit", &kit_body(&w, by_password(&next))).ok();
    // a passkey removed, proved with the new kit; the last way in still stays
    w.ada.call(hub, "DELETE", &format!("/v1/account/passkeys/{}", b64(&key.credential_id)), &json!({ "proof": by_kit(&new_kit) })).ok();
    let account = w.ada.get(hub, "/v1/account").ok();
    assert_eq!(account["passkeys"].as_array().unwrap().len(), 1);
    assert_eq!(account["has_password"], true);
    assert_eq!(account["email"], "ada@example.org");
    // and the account is deleted with the password of now
    assert_eq!(w.ada.call(hub, "DELETE", "/v1/account", &by_password(&next)).ok(), json!({ "deleted": true }));
}
