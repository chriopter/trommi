//! The hub's API (spec/hub-api.md) on requests made of the fuzzer's bytes: every route that does not stream, with
//! ids of the fuzzer's room, of another room, of nothing, and its own text; signed in to either room, to none,
//! or with a token of its own; bodies of real field names with values of any kind, or the fuzzer's own JSON.
//! One hub per process (its rooms founded once over HTTP, by the scenario clients of tests/hub); requests go
//! to `api::fuzz_route`, without HTTP.
//!
//! Properties:
//! - nothing panics, and no request is answered `internal` (a 500): bad input is the client's error;
//! - a request signed in to room A, or not signed in at all, is never answered with what only room B holds
//!   (its GroupInfo, its sealed key, its device's KeyPackage).

#[allow(dead_code, unused_imports, clippy::all)]
#[path = "../../hub/common/mod.rs"]
mod common;

use super::Bytes;
use crate::content::Dice;
use common::{found_room, TestHub};
use serde_json::{json, Map, Value};
use std::sync::{Mutex, OnceLock};
use trommi_hub::util::b64;

struct Setup {
    hub: TestHub,
    /// Per room: its id, its token, its groups, and what nobody outside it may be served.
    rooms: Vec<Room>,
}

struct Room {
    id: String,
    token: String,
    groups: Vec<String>,
    device: String,
    secrets: Vec<String>,
}

fn setup() -> &'static Mutex<Setup> {
    static SETUP: OnceLock<Mutex<Setup>> = OnceLock::new();
    SETUP.get_or_init(|| {
        let hub = TestHub::start_with(&[
            ("HUB_LIMIT_OPEN_PER_IP_MINUTE", "100000000"),
            ("HUB_LIMIT_FOUND_PER_IP_HOUR", "100000000"),
            ("HUB_QUIET", "1"),
        ]);
        let mut rooms = Vec::new();
        for _ in 0..2 {
            let (id, _recovery, dev) = found_room(&hub);
            let token = dev.headers()[0].1.trim_start_matches("Bearer ").to_owned();
            let groups: Vec<String> = dev
                .get(&hub, &format!("/v1/rooms/{}/groups", b64(&id)))
                .json()
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|g| g["group_id"].as_str().map(str::to_owned))
                .collect();
            let info = dev.get(
                &hub,
                &format!(
                    "/v1/groups/{}/info",
                    groups.first().cloned().unwrap_or_default()
                ),
            );
            let secrets: Vec<String> = info.json()["group_info"]
                .as_str()
                .map(str::to_owned)
                .into_iter()
                .collect();
            assert!(
                !secrets.is_empty() && !groups.is_empty(),
                "the fuzz hub's rooms are set up"
            );
            rooms.push(Room {
                id: b64(&id),
                token,
                groups,
                device: b64(&dev.id()),
                secrets,
            });
        }
        Mutex::new(Setup { hub, rooms })
    })
}

const ROUTES: [&str; 69] = [
    "DELETE account",
    "DELETE account/passkeys/{id}",
    "DELETE files/{id}",
    "DELETE invites/{id}",
    "DELETE push",
    "DELETE rooms/{room}/recovery/{id}",
    "DELETE shares/{id}",
    "DELETE token",
    "GET account",
    "GET account/usage",
    "GET boards/{id}",
    "GET cards/{id}",
    "GET changes",
    "GET chats/{scope}/{id}/items",
    "GET desk",
    "GET files/{id}",
    "GET groups/{group}/chains/{device}",
    "GET groups/{group}/info",
    "GET groups/{group}/log",
    "GET groups/{group}/removal",
    "GET invites/{id}",
    "GET invites/{id}/reveal",
    "GET push",
    "GET push-changes",
    "GET push-envelope",
    "GET requests",
    "GET rooms/{room}/challenge",
    "GET rooms/{room}/groups",
    "GET sealed-keys",
    "GET shares/{id}",
    "GET stream",
    "GET welcomes",
    "POST account",
    "POST account/login",
    "POST account/passkey/challenge",
    "POST account/passkey/login",
    "POST account/passkeys",
    "POST account/passkeys/challenge",
    "POST account/recover",
    "POST boards/{id}/frontier",
    "POST envelopes",
    "POST groups",
    "POST groups/{group}/archive",
    "POST groups/{group}/commits",
    "POST groups/{group}/messages",
    "POST groups/{group}/reject",
    "POST invites",
    "POST invites/{id}/request",
    "POST key-packages/claim",
    "POST link",
    "POST live-activity",
    "POST push",
    "POST requests",
    "POST rooms",
    "POST rooms/{room}/recovery",
    "POST rooms/{room}/recovery-code",
    "POST rooms/{room}/recovery/{id}/commits",
    "POST rooms/{room}/recovery/{id}/finish",
    "POST rooms/{room}/tokens",
    "POST shares",
    "PUT account/email",
    "PUT account/password",
    "PUT files/{id}",
    "PUT invites/{id}/reveal",
    "PUT key-packages",
    "PUT sealed-keys",
    "GET {any}",
    "POST {any}/{any}",
    "GET groups/{any}/{any}",
];

const FIELDS: [&str; 73] = [
    "account",
    "after",
    "after_change",
    "apns",
    "auth",
    "before",
    "brief",
    "challenge",
    "commit",
    "devices",
    "email",
    "endpoint",
    "envelope",
    "environment",
    "epoch",
    "expires_at",
    "file_id",
    "files",
    "frontier",
    "generation",
    "group",
    "group_id",
    "group_info",
    "hears",
    "kdf",
    "key_package",
    "kind",
    "kit",
    "last_call_at",
    "last_resort",
    "leaving",
    "level",
    "limit",
    "mac",
    "message",
    "n",
    "offer",
    "ok",
    "origin",
    "partial",
    "passkey",
    "password",
    "port",
    "process",
    "proof",
    "reason",
    "recovery_auth",
    "recovery_link",
    "relay",
    "request",
    "retry_after",
    "reveal",
    "revision",
    "sealed_key",
    "secret_hash",
    "share_id",
    "signature",
    "since",
    "single_use",
    "snapshot",
    "tag",
    "ticket",
    "token",
    "topic",
    "transports",
    "type",
    "user_handle",
    "web_push",
    "welcome",
    "working",
    "room",
    "key_packages",
    "commits",
];

fn text(dice: &mut Dice<Bytes>, max: u64) -> String {
    let len = dice.below(max + 1).unwrap_or(0) as usize;
    let mut out = String::new();
    for _ in 0..len {
        let Ok(b) = dice.bytes::<1>() else { break };
        out.push(match b[0] {
            0..=9 => ['/', '.', '%', '?', '#', '&', '=', '\\', '\0', '\u{202e}'][b[0] as usize],
            b => char::from(b'A' + b % 58),
        });
    }
    out
}

fn some_b64(dice: &mut Dice<Bytes>, max: u64) -> String {
    let len = dice.below(max + 1).unwrap_or(0) as usize;
    let mut bytes = vec![0u8; len];
    for b in &mut bytes {
        *b = dice.bytes::<1>().map(|b| b[0]).unwrap_or(0);
    }
    b64(&bytes)
}

fn value(dice: &mut Dice<Bytes>, setup: &Setup, depth: u32) -> Value {
    match dice.below(12).unwrap_or(0) {
        0 => Value::Null,
        1 => json!(dice.one_in(2).unwrap_or(false)),
        2 => json!(i64::from_le_bytes(dice.bytes().unwrap_or_default())),
        3 => json!(dice.below(100).unwrap_or(0)),
        4 => json!(f64::from(dice.between(-1000, 1000).unwrap_or(0)) / 7.0),
        5 => json!(text(dice, 40)),
        6 | 7 => {
            let max = if dice.one_in(8).unwrap_or(false) {
                70_000
            } else {
                64
            };
            json!(some_b64(dice, max))
        }
        8 => {
            let room = &setup.rooms[dice.below(2).unwrap_or(0) as usize];
            let group = room.groups.first().cloned().unwrap_or_default();
            json!(
                [room.id.clone(), room.device.clone(), group][dice.below(3).unwrap_or(0) as usize]
            )
        }
        9 if depth < 3 => Value::Array(
            (0..dice.below(4).unwrap_or(0))
                .map(|_| value(dice, setup, depth + 1))
                .collect(),
        ),
        10 if depth < 3 => object(dice, setup, depth + 1),
        _ => json!(""),
    }
}

fn object(dice: &mut Dice<Bytes>, setup: &Setup, depth: u32) -> Value {
    let mut map = Map::new();
    for _ in 0..dice.below(8).unwrap_or(0) {
        let key = FIELDS[dice.below(FIELDS.len() as u64).unwrap_or(0) as usize];
        map.insert(key.to_owned(), value(dice, setup, depth));
    }
    Value::Object(map)
}

fn segment(dice: &mut Dice<Bytes>, setup: &Setup, what: &str) -> String {
    let room = &setup.rooms[dice.below(2).unwrap_or(0) as usize];
    match (what, dice.below(6).unwrap_or(0)) {
        ("room", 0..=2) => room.id.clone(),
        ("group", 0..=2) => room
            .groups
            .get(dice.below(4).unwrap_or(0) as usize)
            .cloned()
            .unwrap_or_default(),
        ("device", 0..=2) => room.device.clone(),
        ("scope", 0..=3) => {
            ["session", "card", "desk", "x"][dice.below(4).unwrap_or(0) as usize].to_owned()
        }
        (_, 3) => some_b64(dice, 32),
        (_, 4) => {
            let bytes: [u8; 16] = dice.bytes().unwrap_or_default();
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        }
        _ => text(dice, 48),
    }
}

pub fn run(data: &[u8]) {
    let mut dice = Dice(Bytes::new(data));
    let guard = setup()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let setup = &*guard;
    let (method, template) = ROUTES[dice.below(ROUTES.len() as u64).unwrap_or(0) as usize]
        .split_once(' ')
        .expect("a method and a path");
    let method = if dice.one_in(16).unwrap_or(false) {
        ["GET", "POST", "PUT", "DELETE", "PATCH"][dice.below(5).unwrap_or(0) as usize]
    } else {
        method
    };
    let mut path = String::from("/v1");
    for part in template.split('/') {
        path.push('/');
        match part.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
            Some(what) => path.push_str(&segment(&mut dice, setup, what)),
            None => path.push_str(part),
        }
    }
    if path.contains("__test") {
        return;
    }
    let query = (!dice.one_in(3).unwrap_or(true)).then(|| {
        (0..dice.below(4).unwrap_or(0))
            .map(|_| {
                let key = [
                    "after", "limit", "since", "before", "epoch", "group", "kind", "n", "from",
                ][dice.below(9).unwrap_or(0) as usize];
                let v = if dice.one_in(2).unwrap_or(false) {
                    dice.below(u64::MAX).unwrap_or(0).to_string()
                } else {
                    text(&mut dice, 24)
                };
                format!("{key}={v}")
            })
            .collect::<Vec<_>>()
            .join("&")
    });
    let signed_in = dice.below(5).unwrap_or(0) as usize;
    let bearer = match signed_in {
        0 | 1 => Some(setup.rooms[signed_in].token.clone()),
        2 => None,
        3 => Some(some_b64(&mut dice, 48)),
        _ => Some(text(&mut dice, 80)),
    };
    let lease = dice
        .one_in(3)
        .unwrap_or(false)
        .then(|| dice.below(5).unwrap_or(0));
    let found_token = dice
        .one_in(8)
        .unwrap_or(false)
        .then(|| some_b64(&mut dice, 32));
    let ticket = dice
        .one_in(8)
        .unwrap_or(false)
        .then(|| some_b64(&mut dice, 32));
    let body = if dice.one_in(4).unwrap_or(false) {
        match serde_json::from_slice::<Value>(dice.0.rest()) {
            Ok(v @ Value::Object(_)) => v,
            _ => json!({}),
        }
    } else {
        object(&mut dice, setup, 0)
    };
    let answer = trommi_hub::api::fuzz_route(
        &setup.hub.app,
        method,
        &path,
        query.as_deref(),
        bearer,
        lease,
        found_token,
        ticket,
        body,
    );
    if std::env::var_os("FUZZ_DEBUG").is_some() {
        eprintln!(
            "hub: {method} {template} signed {signed_in} -> {}",
            match &answer {
                Ok(_) => "ok",
                Err(r) => r.code,
            }
        );
    }
    match answer {
        Err(refused) => assert_ne!(
            refused.code, "internal",
            "{method} {path}: internal error on a client's input: {}",
            refused.message
        ),
        Ok(text) => {
            for (i, room) in setup.rooms.iter().enumerate() {
                if i == signed_in {
                    continue;
                }
                for secret in &room.secrets {
                    assert!(
                        !text.contains(secret.as_str()),
                        "{method} {path}: room {i}'s GroupInfo served to a request not signed in to it"
                    );
                }
            }
        }
    }
}
