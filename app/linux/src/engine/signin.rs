//! Signing in on this machine (spec 8.4, 8.5): the account's e-mail and password, or the Emergency Kit's words,
//! open the room's recovery code; with it this device joins the room group from outside, then each live session
//! group, every step checked by the core from the room's founding (iOS `RoomAccount.joinWithRecoveryCode`,
//! `LiveRecovery`).

use super::{bytes_of, b64s, fault, sync::post_entry, Engine, Sink};
use crate::store::{self, DurableStore, RoomRecord};
use serde_json::{json, Value};
use std::sync::Arc;
use trommi_connector::error::{Fault, Result};
use trommi_connector::hub::{Hub, Signer};
use trommi_connector::store::Journal;
use trommi_connector::util::{hex, now_ms};
use trommi_core::hub_auth::{HubAddress, SignedHubAuth};
use trommi_core::ids::RoomId;
use trommi_core_ffi::{
    account_id_parse, kit_keys_for, normalise_email, open_recovery_code, parse_kit_words, password_keys,
    recovery_anchor, recovery_sign_in, AccountName, AccountWay, CoreDevice, OutboxKind, PlacedCommit,
    ServedCommit, ServedEnd, ServedStart,
};
use zeroize::Zeroizing;

/// The hub's address in its one canonical form.
pub fn canonical_hub(text: &str) -> Result<String> {
    let text = text.trim().trim_end_matches('/');
    HubAddress::parse(text)
        .map(|a| a.as_str().to_string())
        .map_err(|_| Fault::new("bad-hub", "not a hub address (https://…, or http://127.0.0.1:… for a local one)"))
}

/// A hub client that signs in as the recovery key: it may read what a join with the code needs, and post it.
fn recovery_hub(hub_url: &str, room: &[u8], code: Zeroizing<Vec<u8>>) -> Result<Hub> {
    let room_id = RoomId::from_slice(room).map_err(Fault::from)?;
    let (room, address) = (room.to_vec(), hub_url.to_string());
    let signer: Signer = Arc::new(move |challenge| {
        let (code, room, address) = (code.clone(), room.clone(), address.clone());
        Box::pin(async move {
            let signed = recovery_sign_in(code.to_vec(), room, address, challenge.to_vec()).map_err(fault)?;
            Ok(SignedHubAuth {
                auth: signed.auth,
                signature: signed.signature,
            })
        })
    });
    Hub::new(hub_url, Some(room_id), Some(signer))
}

/// The first room of a login answer: its id and the sealed copy of its code.
fn login_room(r: &Value) -> Result<(Vec<u8>, Vec<u8>)> {
    let first = r["rooms"]
        .as_array()
        .and_then(|rooms| rooms.first())
        .ok_or_else(|| Fault::new("bad-format", "the login answer names no room"))?;
    let room = bytes_of(first, "room_id")?;
    if room.len() != 32 {
        return Err(Fault::new("bad-format", "the login answer's room"));
    }
    Ok((room, bytes_of(first, "sealed_copy")?))
}

/// Sign in with the account's e-mail and password.
pub async fn with_password(hub_url: &str, email: &str, password: &str, sink: Sink) -> Result<Arc<Engine>> {
    let hub_url = canonical_hub(hub_url)?;
    let email = normalise_email(email.trim().to_string())
        .map_err(|_| Fault::new("needs-email", "a password signs in with the account's e-mail"))?;
    say(&sink, "Deriving the keys …");
    let keys = password_keys(email.clone(), password.to_string(), None).map_err(fault)?;
    let open = Hub::new(&hub_url, None, None)?;
    say(&sink, "Signing in …");
    let answer = open
        .open_call(
            reqwest::Method::POST,
            "/v1/account/login",
            Some(&json!({ "account": email, "auth_key": b64s(&keys.auth_key) })),
        )
        .await
        .map_err(|f| if f.code == "wrong-login" { Fault::new("wrong-login", "the e-mail or the password is wrong") } else { f })?;
    let (room, sealed) = login_room(&answer)?;
    let code = Zeroizing::new(
        open_recovery_code(keys.wrap_key.clone(), room.clone(), AccountWay::Password, None, sealed)
            .map_err(|_| Fault::new("wrong-login", "the e-mail or the password is wrong"))?,
    );
    join_with_code(&hub_url, room, code, sink).await
}

/// Sign in with the Emergency Kit: the account (its e-mail, or its id for an account without one) and the words.
pub async fn with_kit(hub_url: &str, account: &str, words: &str, sink: Sink) -> Result<Arc<Engine>> {
    let hub_url = canonical_hub(hub_url)?;
    let account = account.trim();
    let (name, named) = if account.contains('@') {
        let email = normalise_email(account.to_string()).map_err(fault)?;
        (AccountName { email: Some(email.clone()), id: None }, email)
    } else {
        let id = account_id_parse(account.to_string()).map_err(|_| Fault::new("bad-account", "neither an e-mail nor an account id"))?;
        (AccountName { email: None, id: Some(id.clone()) }, id)
    };
    let words = parse_kit_words(words.to_string()).map_err(fault)?;
    say(&sink, "Deriving the keys …");
    let keys = kit_keys_for(name, words).map_err(fault)?;
    let open = Hub::new(&hub_url, None, None)?;
    say(&sink, "Opening the Emergency Kit …");
    let answer = open
        .open_call(
            reqwest::Method::POST,
            "/v1/account/recover",
            Some(&json!({ "account": named, "auth_key": b64s(&keys.auth_key) })),
        )
        .await?;
    let (room, sealed) = login_room(&answer)?;
    let code = Zeroizing::new(
        open_recovery_code(keys.wrap_key.clone(), room.clone(), AccountWay::Kit, None, sealed)
            .map_err(|_| Fault::new("wrong-recovery", "the account or the words are wrong"))?,
    );
    join_with_code(&hub_url, room, code, sink).await
}

fn say(sink: &Sink, text: &str) {
    sink(super::Update {
        status: Some(text.to_string()),
        ..Default::default()
    });
}

/// Every SealedKey and RecoveryLink of the room, page by page.
async fn sealed_keys(hub: &Hub) -> Result<(Vec<Vec<u8>>, Vec<Vec<u8>>)> {
    let (mut rows, mut links, mut after) = (Vec::new(), Vec::new(), 0u64);
    loop {
        let page = hub.get(&format!("/v1/sealed-keys?after={after}")).await?;
        for r in page["rows"].as_array().into_iter().flatten() {
            rows.push(bytes_of(r, "sealed_key")?);
        }
        for l in page["links"].as_array().into_iter().flatten() {
            links.push(bytes_of(l, "recovery_link")?);
        }
        if page["more"] != json!(true) {
            return Ok((rows, links));
        }
        let next = page["change"].as_u64().unwrap_or(0);
        if next <= after {
            return Err(Fault::new("bad-format", "the hub's list of sealed keys does not move on"));
        }
        after = next;
    }
}

/// A group's GroupInfo: the current one, or of that epoch.
async fn info(hub: &Hub, group: &[u8], epoch: Option<u64>) -> Result<(u64, Vec<u8>)> {
    let path = match epoch {
        Some(e) => format!("/v1/groups/{}/info?epoch={e}", b64s(group)),
        None => format!("/v1/groups/{}/info", b64s(group)),
    };
    let r = hub.get(&path).await?;
    let named = r["epoch"].as_u64().or(epoch).ok_or_else(|| Fault::new("bad-format", "the hub's GroupInfo names no epoch"))?;
    Ok((named, bytes_of(&r, "group_info")?))
}

/// The live session groups, main sessions before helper sessions.
async fn live_sessions(hub: &Hub, room: &[u8]) -> Result<Vec<Vec<u8>>> {
    let (mut main, mut helper) = (Vec::new(), Vec::new());
    let mut after: Option<String> = None;
    for _ in 0..10_000 {
        let path = match &after {
            Some(a) => format!("/v1/rooms/{}/groups?limit=1000&after={a}", b64s(room)),
            None => format!("/v1/rooms/{}/groups?limit=1000", b64s(room)),
        };
        let page = hub.get(&path).await?;
        let items = page["items"].as_array().cloned().or_else(|| page.as_array().cloned()).unwrap_or_default();
        for g in &items {
            if g["live"] != json!(true) {
                continue;
            }
            let id = bytes_of(g, "group_id")?;
            match g["kind"].as_str() {
                Some("main") => main.push(id),
                Some("helper") => helper.push(id),
                _ => {}
            }
        }
        let next = page["after"].as_str().map(str::to_string).or(page["after"].as_u64().map(|n| n.to_string()));
        if page["more"] != json!(true) || next.is_none() || next == after {
            break;
        }
        after = next;
    }
    main.extend(helper);
    Ok(main)
}

/// A group's Commits below the epoch `below`, page by page, in the hub's order.
async fn commits(hub: &Hub, group: &[u8], below: u64, mut take: impl FnMut(Vec<ServedCommit>) -> Result<()>) -> Result<()> {
    if below == 0 {
        return Ok(());
    }
    let mut after = 0u64;
    loop {
        let page = hub
            .get(&format!("/v1/groups/{}/log?after={after}&limit=200&kind=commit", b64s(group)))
            .await?;
        let items = page["items"].as_array().cloned().unwrap_or_default();
        let mut batch = Vec::new();
        let mut ended = false;
        for item in &items {
            let n = item["n"].as_u64().filter(|n| *n > after).ok_or_else(|| Fault::new("bad-format", "the hub's log of a group is not in order"))?;
            after = n;
            if item["kind"] != json!("commit") {
                continue;
            }
            let (Some(change), Some(epoch)) = (item["change"].as_u64(), item["epoch"].as_u64()) else {
                return Err(Fault::new("bad-format", "a Commit of the hub's log has no change number or epoch"));
            };
            if epoch >= below {
                ended = true;
                break;
            }
            let recovery_auth = item.get("recovery_auth").filter(|v| v.is_string()).map(|_| bytes_of(item, "recovery_auth")).transpose()?;
            batch.push(ServedCommit {
                change,
                commit: bytes_of(item, "bytes")?,
                recovery_auth,
            });
        }
        if !batch.is_empty() {
            take(batch)?;
        }
        if ended || page["more"] != json!(true) || items.is_empty() {
            return Ok(());
        }
    }
}

/// Posts one outbox entry of a join and reports the hub's answer to the device.
async fn post_join(device: &CoreDevice, hub: &Hub, room: &[u8], id: u64) -> Result<()> {
    let entry = device
        .outbox()
        .map_err(fault)?
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| Fault::new("internal", "an entry the core named is not in the outbox"))?;
    let answer = post_entry(hub, room, &entry).await;
    match answer {
        Ok(a) => {
            let change = a["change"].as_u64().ok_or_else(|| Fault::new("bad-format", "the hub's answer to a join names no change number"))?;
            device.outbox_accepted(id, Some(change)).map_err(fault)
        }
        Err(f) => {
            if f.status.is_some_and(|s| s < 500) {
                if let Some(code) = trommi_core_ffi::error_code_from_text(f.code.clone()) {
                    let _ = device.outbox_refused(id, code);
                }
            }
            Err(f)
        }
    }
}

/// The device joins the room group with the code; a folder of its own, kept only once the hub took the join.
pub async fn join_with_code(hub_url: &str, room: Vec<u8>, code: Zeroizing<Vec<u8>>, sink: Sink) -> Result<Arc<Engine>> {
    let dir = store::base_dir().join(hex(&room));
    // (this room is on this machine already: opened as it is)
    if RoomRecord::load(&dir).is_some() {
        if let Some(engine) = Engine::open(&dir, sink.clone())? {
            return Ok(engine);
        }
    }
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    store::private_dir(&store::base_dir()).map_err(Fault::from)?;
    store::private_dir(&dir).map_err(Fault::from)?;
    let journal = Journal::open(&dir.join("device"))?;
    let device = Arc::new(CoreDevice::create_on(Box::new(DurableStore::new(journal))).map_err(fault)?);
    let joined = async {
        let rhub = recovery_hub(hub_url, &room, code.clone())?;
        say(&sink, "Reading the room …");
        let (rows, links) = sealed_keys(&rhub).await?;
        let anchor = recovery_anchor(code.to_vec(), room.clone(), rows.clone()).map_err(fault)?;
        let (_, anchor_info) = info(&rhub, &room, Some(anchor.epoch)).await?;
        let (epoch, current) = info(&rhub, &room, None).await?;
        let founding = if epoch == 0 { current.clone() } else { info(&rhub, &room, Some(0)).await?.1 };
        let later = live_sessions(&rhub, &room).await?;
        device
            .code_check_start(
                code.to_vec(),
                ServedStart {
                    room: room.clone(),
                    founding,
                    anchor: anchor_info,
                    rows: rows.clone(),
                    sessions: Vec::new(),
                },
            )
            .map_err(fault)?;
        device.code_check_defer(later).map_err(fault)?;
        say(&sink, "Checking the room's history …");
        commits(&rhub, &room, epoch, |batch| {
            let placed = batch
                .into_iter()
                .map(|commit| PlacedCommit { group: room.clone(), commit })
                .collect();
            device.code_check_slice(placed).map_err(fault)
        })
        .await?;
        say(&sink, "Joining the room …");
        let join = device
            .join_room_checked(code.to_vec(), ServedEnd { current, sessions: Vec::new(), rows, links }, now_ms())
            .map_err(fault)?;
        for id in join.outbox {
            post_join(&device, &rhub, &room, id).await?;
        }
        Ok::<(), Fault>(())
    }
    .await;
    if let Err(f) = joined {
        device.close();
        let _ = std::fs::remove_dir_all(&dir);
        return Err(f);
    }
    let me = hex(&device.id().map_err(fault)?);
    let record = RoomRecord {
        hub: hub_url.to_string(),
        room: hex(&room),
        device: me,
        code_join: true,
    };
    record.save(&dir).map_err(Fault::from)?;
    let engine = Engine::with_device(&dir, record, device, sink)?;
    *engine.code.lock().unwrap_or_else(|e| e.into_inner()) = Some(code);
    engine.refresh_groups()?;
    // (its KeyPackages at once: without them every new session of the account leaves this device out)
    let _ = engine.publish_key_packages().await;
    engine.pump_outbox();
    engine.emit();
    Ok(engine)
}

impl Engine {
    /// Joins every live session group this device is not in yet with the code, each read from the hub and checked
    /// from its founding, main sessions first. A session that fails is left to another device's Welcome or the next
    /// start; the device is in the room either way.
    pub(crate) async fn join_sessions_with_code(&self) {
        let Some(code) = self.code.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        let outcome = async {
            let rhub = recovery_hub(&self.hub_url, &self.room, code.clone())?;
            let held = self.session_groups();
            let live = live_sessions(&rhub, &self.room).await?;
            let mut keys: Option<(Vec<Vec<u8>>, Vec<Vec<u8>>)> = None;
            let mut missed = 0;
            for group in live {
                if held.contains(&hex(&group)) {
                    continue;
                }
                let waiting = self
                    .device
                    .outbox()
                    .map_err(fault)?
                    .iter()
                    .any(|e| e.kind == OutboxKind::ExternalCommit && e.group.as_deref() == Some(&group[..]));
                if waiting {
                    missed += 1;
                    continue;
                }
                let one = async {
                    if keys.is_none() {
                        keys = Some(sealed_keys(&rhub).await?);
                    }
                    let (rows, links) = keys.clone().unwrap_or_default();
                    let (epoch, current) = info(&rhub, &group, None).await?;
                    let founding = if epoch == 0 { current.clone() } else { info(&rhub, &group, Some(0)).await?.1 };
                    let started = self.device.session_check_start(founding).map_err(fault)?;
                    if started != group {
                        return Err(Fault::new("bad-group", "the hub served another group's founding"));
                    }
                    commits(&rhub, &group, epoch, |batch| self.device.session_check_slice(group.clone(), batch).map_err(fault)).await?;
                    let id = self
                        .device
                        .join_session_checked_keys(code.to_vec(), group.clone(), current, rows, links, now_ms())
                        .map_err(fault)?;
                    post_join(&self.device, &rhub, &self.room, id).await
                }
                .await;
                if one.is_err() {
                    missed += 1;
                }
            }
            Ok::<usize, Fault>(missed)
        }
        .await;
        let _ = self.refresh_groups();
        if let Ok(0) = outcome {
            *self.code.lock().unwrap_or_else(|e| e.into_inner()) = None;
            let mut record = self.record.lock().unwrap_or_else(|e| e.into_inner());
            record.code_join = false;
            let _ = record.save(&self.dir);
        }
    }
}
