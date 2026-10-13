//! One outbox entry to its route (spec/hub-api.md), by kind, and its answer.
//!
//! The entry is posted exactly as the protocol core made it: `parts` in the order `trommi_core::store::OutboxKind`
//! names. A repeated post of the same bytes gets the hub's first answer again, so an entry whose answer was lost
//! is simply posted again; a relayed message is the exception (the hub passes it on each time).
//!
//! | Kind | Route | Token |
//! | --- | --- | --- |
//! | `RoomFounding` | `POST /v1/rooms` | none |
//! | `GroupFounding` | `POST /v1/groups` | the device's |
//! | `Commit`, `ExternalCommit` | `POST /v1/groups/{group}/commits` | the device's; a join with the code: the recovery key's |
//! | `Message`, `RelayMessage` | `POST /v1/groups/{group}/messages` | the device's |
//! | `Envelope` | `POST /v1/envelopes` | the device's |
//! | `KeyPackages` | `PUT /v1/key-packages` | the device's |
//! | `SealedKey` | `PUT /v1/sealed-keys` | the device's |
//! | `RecoveryCode` | `POST /v1/rooms/{room}/recovery-code` | the device's |
//! | `RecoveryCommit`, `RecoveryFinish` | `POST /v1/rooms/{room}/recovery/{id}/commits`, `…/finish` | the recovery key's |
//!
//! Which token a request carries is the engine's: it is the one in the [`Site`].

use super::json::{bad, Json, Obj, Read, MAX_NUMBER};
use super::limits::{CAP_WRITE, MAX_KEY_PACKAGES};
use super::request::{b64, begin};
use super::types::{Finished, OutboxAnswer, OutboxExtra, Site};
use crate::app::io::HttpRequest;
use serde_json::{json, Map, Value};
use trommi_core::ids::{DeviceId, GroupId, RoomId, SessionId};
use trommi_core::store::{OutboxEntry, OutboxKind};

/// What an entry's answer is checked against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    pub kind: OutboxKind,
    pub group: Option<GroupId>,
    pub epoch: u64,
}

/// Part `i`, which must be there and not empty.
fn part(entry: &OutboxEntry, i: usize) -> Read<String> {
    match entry.parts.get(i) {
        Some(bytes) if !bytes.is_empty() => Ok(b64(bytes)),
        _ => bad("the request: an outbox entry lacks a part"),
    }
}

/// Part `i` under `name`, when it is there and not empty (a Welcome, a RecoveryAuth).
fn put_optional(body: &mut Map<String, Value>, name: &str, entry: &OutboxEntry, i: usize) {
    if let Some(bytes) = entry.parts.get(i).filter(|b| !b.is_empty()) {
        body.insert(name.to_owned(), json!(b64(bytes)));
    }
}

fn count(entry: &OutboxEntry, n: usize) -> Read<()> {
    if entry.parts.len() != n {
        return bad("the request: an outbox entry with another number of parts than its kind has");
    }
    Ok(())
}

fn group(entry: &OutboxEntry) -> Read<String> {
    match &entry.group {
        Some(group) => Ok(b64(group.as_bytes())),
        None => bad("the request: an outbox entry of this kind names its group"),
    }
}

/// The members of a Commit as its routes take them; `epoch` is the epoch it builds on.
pub(crate) fn commit_body(
    entry: &OutboxEntry,
    commit: usize,
    group_info: usize,
    sealed_key: usize,
    welcome: Option<usize>,
    recovery_auth: Option<usize>,
) -> Read<Map<String, Value>> {
    if entry.epoch > MAX_NUMBER {
        return bad("the request: an epoch beyond any number of the protocol");
    }
    let mut body = Map::new();
    body.insert("epoch".to_owned(), json!(entry.epoch));
    body.insert("commit".to_owned(), json!(part(entry, commit)?));
    body.insert("group_info".to_owned(), json!(part(entry, group_info)?));
    body.insert("sealed_key".to_owned(), json!(part(entry, sealed_key)?));
    if let Some(i) = welcome {
        put_optional(&mut body, "welcome", entry, i);
    }
    if let Some(i) = recovery_auth {
        put_optional(&mut body, "recovery_auth", entry, i);
    }
    Ok(body)
}

/// The account's new sealed copies that travel as the last part of a `RecoveryCode` or `RecoveryFinish` entry:
/// the JSON of the hub's body, or no bytes for a room without an account. Read strictly: exactly the members
/// the body has, each a byte string of the hub's length, one way in at most.
fn account_copies(bytes: &[u8]) -> Read<Value> {
    if bytes.is_empty() {
        return Ok(Value::Null);
    }
    const WHY: &str = "the request: the account's copies are not the body the hub takes";
    let only = |o: Obj<'_>, names: &[&str]| -> Read<()> {
        if o.members()
            .iter()
            .any(|(n, _)| !names.contains(&n.as_str()))
        {
            return bad(WHY);
        }
        Ok(())
    };
    let parsed = Json::parse(bytes, 64 << 10).or(bad(WHY))?;
    let o = parsed.obj(WHY)?;
    only(o, &["kit", "password", "passkey", "revision"])?;
    let kit = o.need("kit").obj(WHY)?;
    only(kit, &["auth_key", "sealed_copy"])?;
    kit.need("auth_key").bytes(32, 32, WHY)?;
    kit.need("sealed_copy").bytes(61, 61, WHY)?;
    if let Some(revision) = o.maybe("revision") {
        revision.int(WHY)?;
    } else if o.has("revision") {
        return bad(WHY);
    }
    if o.has("password") && o.has("passkey") {
        return bad(WHY);
    }
    if o.has("password") {
        let w = o.need("password").obj(WHY)?;
        w.need("sealed_copy").bytes(61, 61, WHY)?;
        if w.has("auth_key") {
            only(w, &["auth_key", "sealed_copy", "kdf"])?;
            w.need("auth_key").bytes(32, 32, WHY)?;
            let kdf = w.need("kdf").obj(WHY)?;
            only(kdf, &["alg", "v", "m", "t", "p"])?;
            for (_, v) in kdf.members() {
                if v.int(WHY).is_err() {
                    v.text(64, WHY)?;
                }
            }
        } else {
            only(w, &["sealed_copy"])?;
        }
    }
    if o.has("passkey") {
        let k = o.need("passkey").obj(WHY)?;
        k.need("sealed_copy").bytes(61, 61, WHY)?;
        if k.has("attestation_object") {
            only(
                k,
                &[
                    "attestation_object",
                    "client_data_json",
                    "sealed_copy",
                    "transports",
                ],
            )?;
            k.need("attestation_object").bytes(1, 8192, WHY)?;
            k.need("client_data_json").bytes(1, 4096, WHY)?;
            if k.has("transports") {
                for t in k.need("transports").list(8, WHY)? {
                    let t = t.text(16, WHY)?;
                    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
                        return bad(WHY);
                    }
                }
            }
        } else {
            only(k, &["credential_id", "sealed_copy"])?;
            k.need("credential_id").bytes(1, 1023, WHY)?;
        }
    }
    serde_json::from_slice(bytes).or(bad(WHY))
}

/// The body of `POST /v1/account` that founds the account with the room: a JSON object of at most 64 KiB (the
/// account routes check its members where they build it).
fn new_account(bytes: &[u8]) -> Read<Value> {
    const WHY: &str = "the request: the account to found is not a JSON object";
    Json::parse(bytes, 64 << 10).or(bad(WHY))?.obj(WHY)?;
    serde_json::from_slice(bytes).or(bad(WHY))
}

fn recovery_id(extra: Option<&OutboxExtra>) -> Read<String> {
    match extra.and_then(|x| x.recovery_id.as_deref()) {
        Some(id) if id.len() == 16 => Ok(b64(id)),
        _ => bad("the request: a part of a recovery names the recovery it belongs to"),
    }
}

pub(crate) fn request(
    site: &Site<'_>,
    room: &RoomId,
    entry: &OutboxEntry,
    extra: Option<&OutboxExtra>,
) -> Read<(HttpRequest, Asked)> {
    let room = b64(room.as_bytes());
    let post = |path: &str, body: Value| -> Read<HttpRequest> {
        Ok(begin(site, "POST", path, true, CAP_WRITE)?
            .json(&body)
            .done())
    };
    let put = |path: &str, body: Value| -> Read<HttpRequest> {
        Ok(begin(site, "PUT", path, true, CAP_WRITE)?
            .json(&body)
            .done())
    };
    let request = match entry.kind {
        OutboxKind::RoomFounding => {
            count(entry, 2)?;
            let mut body = json!({ "group_info": part(entry, 0)?, "sealed_key": part(entry, 1)? });
            if let Some(account) = extra.and_then(|x| x.account.as_deref()) {
                body["account"] = new_account(account)?;
            }
            let mut build = begin(site, "POST", "/v1/rooms", false, CAP_WRITE)?.json(&body);
            if let Some(word) = extra.and_then(|x| x.found_token.as_deref()) {
                if word.is_empty()
                    || word.len() > 256
                    || !word.bytes().all(|b| b.is_ascii_graphic())
                {
                    return bad("the request: the founding word is not plain text");
                }
                build = build.header("x-found-token", word.to_owned());
            }
            build.done()
        }
        OutboxKind::GroupFounding => {
            count(entry, 6)?;
            let mut body = Map::new();
            for (name, i) in [
                ("group_info_0", 0),
                ("sealed_key_0", 1),
                ("commit", 2),
                ("group_info", 3),
                ("sealed_key", 5),
            ] {
                body.insert(name.to_owned(), json!(part(entry, i)?));
            }
            put_optional(&mut body, "welcome", entry, 4);
            post("/v1/groups", Value::Object(body))?
        }
        OutboxKind::Commit => {
            count(entry, 4)?;
            let body = commit_body(entry, 0, 1, 3, Some(2), None)?;
            post(
                &format!("/v1/groups/{}/commits", group(entry)?),
                Value::Object(body),
            )?
        }
        OutboxKind::ExternalCommit => {
            count(entry, 4)?;
            part(entry, 3)?;
            let body = commit_body(entry, 0, 1, 2, None, Some(3))?;
            post(
                &format!("/v1/groups/{}/commits", group(entry)?),
                Value::Object(body),
            )?
        }
        OutboxKind::Message | OutboxKind::RelayMessage => {
            count(entry, 1)?;
            if entry.epoch > MAX_NUMBER {
                return bad("the request: an epoch beyond any number of the protocol");
            }
            let mut body = json!({ "epoch": entry.epoch, "message": part(entry, 0)? });
            if entry.kind == OutboxKind::RelayMessage {
                body["relay"] = json!(true);
            }
            post(&format!("/v1/groups/{}/messages", group(entry)?), body)?
        }
        OutboxKind::Envelope => {
            count(entry, 1)?;
            post("/v1/envelopes", json!({ "envelope": part(entry, 0)? }))?
        }
        OutboxKind::KeyPackages => {
            // the last-resort one (or no bytes), then the single-use ones
            let Some((last_resort, single_use)) = entry.parts.split_first() else {
                return bad("the request: an outbox entry lacks a part");
            };
            if single_use.len() > MAX_KEY_PACKAGES || single_use.iter().any(Vec::is_empty) {
                return bad("the request: at most 100 single-use KeyPackages, none empty");
            }
            let mut body =
                json!({ "single_use": single_use.iter().map(|k| b64(k)).collect::<Vec<_>>() });
            if !last_resort.is_empty() {
                body["last_resort"] = json!(b64(last_resort));
            }
            put("/v1/key-packages", body)?
        }
        OutboxKind::SealedKey => {
            count(entry, 1)?;
            put("/v1/sealed-keys", json!({ "sealed_key": part(entry, 0)? }))?
        }
        OutboxKind::RecoveryCode => {
            count(entry, 5)?;
            let body = json!({
                "commit": Value::Object(commit_body(entry, 0, 1, 2, None, None)?),
                "recovery_link": part(entry, 3)?,
                "account": account_copies(&entry.parts[4])?,
            });
            post(&format!("/v1/rooms/{room}/recovery-code"), body)?
        }
        OutboxKind::RecoveryCommit => {
            count(entry, 5)?;
            let mut body = commit_body(entry, 0, 1, 3, Some(2), Some(4))?;
            body.insert("group_id".to_owned(), json!(group(entry)?));
            post(
                &format!("/v1/rooms/{room}/recovery/{}/commits", recovery_id(extra)?),
                Value::Object(body),
            )?
        }
        OutboxKind::RecoveryFinish => {
            count(entry, 2)?;
            let body = json!({ "recovery_link": part(entry, 0)?, "account": account_copies(&entry.parts[1])? });
            post(
                &format!("/v1/rooms/{room}/recovery/{}/finish", recovery_id(extra)?),
                body,
            )?
        }
    };
    let asked = Asked {
        kind: entry.kind,
        group: entry.group,
        epoch: entry.epoch,
    };
    Ok((request, asked))
}

/// `{ epoch, change }` of an accepted Commit: the epoch is the one it was built on plus one.
pub(crate) fn committed(o: Obj<'_>, built_on: u64) -> Read<OutboxAnswer> {
    let epoch = o.need("epoch").int("a Commit's answer: epoch")?;
    let change = o.need("change").count("a Commit's answer: change")?;
    if built_on.checked_add(1) != Some(epoch) {
        return bad("the epoch after a Commit is not the next one");
    }
    Ok(OutboxAnswer {
        change: Some(change),
        epoch: Some(epoch),
        ..OutboxAnswer::default()
    })
}

pub(crate) fn read(asked: &Asked, body: &Json) -> Read<OutboxAnswer> {
    let o = body.obj("an answer to a write: an object")?;
    let none = OutboxAnswer::default();
    Ok(match asked.kind {
        OutboxKind::RoomFounding => {
            let room = o
                .need("room_id")
                .id(RoomId::from_slice, "a founding's room_id")?;
            if asked.group.is_some_and(|g| g != GroupId::room(room)) {
                return bad("another room than the one founded");
            }
            OutboxAnswer {
                founded: Some(GroupId::room(room)),
                ..none
            }
        }
        OutboxKind::GroupFounding => {
            let bytes = o.need("group_id").bytes(48, 48, "a founding's group_id")?;
            let (room, session) = bytes.split_at(RoomId::LEN);
            let founded = match (RoomId::from_slice(room), SessionId::from_slice(session)) {
                (Ok(room), Ok(session)) => GroupId::session(room, session),
                _ => return bad("a founding's group_id"),
            };
            if asked.group.is_some_and(|g| g != founded) {
                return bad("another group than the one founded");
            }
            OutboxAnswer {
                founded: Some(founded),
                ..none
            }
        }
        OutboxKind::Commit | OutboxKind::ExternalCommit | OutboxKind::RecoveryCode => {
            committed(o, asked.epoch)?
        }
        OutboxKind::Message => OutboxAnswer {
            n: Some(o.need("n").count("a message's n")?),
            ..none
        },
        OutboxKind::RelayMessage => {
            if !o.need("n").is_null() {
                return bad("n of a relayed message");
            }
            none
        }
        OutboxKind::Envelope => OutboxAnswer {
            change: Some(o.need("change").count("an envelope's change")?),
            ..none
        },
        OutboxKind::KeyPackages => OutboxAnswer {
            unused: Some(unused(o)?),
            ..none
        },
        OutboxKind::SealedKey => {
            if !matches!(o.need("stored"), Json::Bool(true)) {
                return bad("stored: true");
            }
            none
        }
        OutboxKind::RecoveryCommit => OutboxAnswer {
            epoch: Some(o.need("epoch").int("a recovery part's epoch")?),
            kept: Some(o.need("kept").bool("a recovery part's kept")?),
            ..none
        },
        OutboxKind::RecoveryFinish => {
            let first_change = o.need("first_change").int("a finish's first_change")?;
            let change = o.need("change").int("a finish's change")?;
            if first_change > change {
                return bad("a finish whose first change lies behind its last");
            }
            OutboxAnswer {
                change: Some(change),
                finished: Some(Finished {
                    published: o.need("published").bool("a finish's published")?,
                    first_change,
                    device: o
                        .need("device")
                        .id(DeviceId::from_slice, "a finish's device")?,
                }),
                ..none
            }
        }
    })
}

/// `unused` of `PUT /v1/key-packages`: how many single-use KeyPackages the hub holds for the device.
pub(crate) fn unused(o: Obj<'_>) -> Read<u32> {
    u32::try_from(o.need("unused").int("unused: a count")?).or(bad("unused: a count"))
}

/// `PUT /v1/key-packages` with nothing to add: how many unused ones the hub holds.
pub(crate) fn held_request(site: &Site<'_>) -> Read<HttpRequest> {
    Ok(begin(site, "PUT", "/v1/key-packages", true, CAP_WRITE)?
        .json(&json!({ "single_use": [] }))
        .done())
}
