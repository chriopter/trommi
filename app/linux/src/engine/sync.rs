//! Catching up, the live stream and the outbox: the iOS app's `Room` (catchUp, readChanges, process, runLive,
//! pumpOutbox), in Rust over the same facade.

use super::{bytes_of, b64s, fault, Engine};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use trommi_connector::error::{Fault, Result};
use trommi_connector::hub::is_transient;
use trommi_connector::util::{hex, now_ms};
use trommi_core_ffi::{
    error_code_from_text, error_code_text, log_finding, DeskListed, EnvelopeKind, EnvelopeOutcome,
    LogEntry, LogEntryKind, LogFinding, OutboxEntry, OutboxKind, ProcessedKind, ReceivedEnvelope,
    ReceivedKind,
};

/// One item of `GET /v1/changes` or of the stream.
pub(crate) enum Item {
    Log { entry: LogEntry },
    Envelope { change: u64, bytes: Vec<u8>, void: Option<String> },
}

impl Item {
    fn change(&self) -> u64 {
        match self {
            Item::Log { entry, .. } => entry.change,
            Item::Envelope { change, .. } => *change,
        }
    }

    /// None for a kind a newer hub adds (passed over); an error for anything else that does not read.
    fn of(j: &Value) -> Result<Option<Item>> {
        let change = j["change"].as_u64().filter(|c| *c > 0);
        let bad = || Fault::new("bad-format", "an item of the hub's changes does not read");
        match j["kind"].as_str() {
            Some("envelope") => Ok(Some(Item::Envelope {
                change: change.ok_or_else(bad)?,
                bytes: bytes_of(j, "envelope")?,
                void: j["void_code"].as_str().map(str::to_string),
            })),
            Some(k @ ("commit" | "message")) => {
                let recovery_auth = match j.get("recovery_auth") {
                    Some(v) if v.is_string() => Some(bytes_of(j, "recovery_auth")?),
                    _ => None,
                };
                Ok(Some(Item::Log {
                    entry: LogEntry {
                        change: change.ok_or_else(bad)?,
                        group: bytes_of(j, "group_id")?,
                        kind: if k == "commit" { LogEntryKind::Commit } else { LogEntryKind::Message },
                        bytes: bytes_of(j, "bytes")?,
                        recovery_auth,
                    },
                }))
            }
            _ => Ok(None),
        }
    }
}

/// The items of a page as they must be: each readable, each with a change number above the one before.
fn ordered(raw: &[Value], after: u64) -> Result<Vec<Item>> {
    let mut last = after;
    let mut out = Vec::new();
    for j in raw {
        let Some(item) = Item::of(j)? else {
            if let Some(n) = j["change"].as_u64() {
                last = last.max(n);
            }
            continue;
        };
        if item.change() <= last {
            return Err(Fault::new("bad-format", "the hub's changes are not in order"));
        }
        last = item.change();
        out.push(item);
    }
    Ok(out)
}

/// Whether the hub's code is its last word on an outbox entry (LiveCore.swift `isFinalRefusal`).
fn is_final_refusal(code: &str) -> bool {
    const PASSING: &[&str] = &["internal", "overloaded", "rate-limited", "unauthorised", "bad-challenge", "client-too-old", "lease-lost"];
    const NOT_OF_A_HUB: &[&str] = &[
        "storage", "entropy", "busy", "weak-password", "bad-kdf", "bad-recovery-words", "bad-recovery-code", "no-prf",
        "withheld", "hub-voided-other", "bad-group", "no-key", "pruned", "decrypt-failed", "code-not-confirmed",
        "hash-mismatch", "cut",
    ];
    error_code_from_text(code.to_string()).is_some() && !PASSING.contains(&code) && !NOT_OF_A_HUB.contains(&code)
}

/// What an outbox entry's answer must hold to count as taken; the change number the device wants.
fn accepted(kind: OutboxKind, r: &Value) -> Option<Option<u64>> {
    match kind {
        OutboxKind::Envelope | OutboxKind::Commit | OutboxKind::ExternalCommit | OutboxKind::RecoveryCode => {
            r["change"].as_u64().filter(|c| *c > 0).map(Some)
        }
        OutboxKind::Message => r["n"].as_u64().map(|_| None),
        OutboxKind::RelayMessage => r["n"].is_null().then_some(None),
        OutboxKind::KeyPackages => r["unused"].as_u64().map(|_| None),
        OutboxKind::GroupFounding => r["group_id"].is_string().then_some(None),
        OutboxKind::RoomFounding => r["room_id"].is_string().then_some(None),
        OutboxKind::SealedKey => Some(None),
        OutboxKind::RecoveryCommit => (r["kept"] == json!(true)).then_some(None),
        OutboxKind::RecoveryFinish => r["change"].as_u64().map(Some),
    }
}

fn carries_commit(kind: OutboxKind) -> bool {
    matches!(
        kind,
        OutboxKind::Commit | OutboxKind::GroupFounding | OutboxKind::ExternalCommit | OutboxKind::RecoveryCode
    )
}

/// Posts one outbox entry exactly as the core made it (iOS `HubClient.post`).
pub(crate) async fn post_entry(hub: &trommi_connector::hub::Hub, room: &[u8], e: &OutboxEntry) -> Result<Value> {
    let part = |i: usize| e.parts.get(i).map(|p| b64s(p)).unwrap_or_default();
    let optional = |i: usize| match e.parts.get(i) {
        Some(p) if !p.is_empty() => json!(b64s(p)),
        _ => Value::Null,
    };
    let group = e.group.as_deref().map(b64s).unwrap_or_default();
    match e.kind {
        OutboxKind::Commit => {
            hub.post(
                &format!("/v1/groups/{group}/commits"),
                &json!({ "epoch": e.epoch, "commit": part(0), "group_info": part(1), "welcome": optional(2), "sealed_key": part(3) }),
            )
            .await
        }
        OutboxKind::ExternalCommit => {
            hub.post(
                &format!("/v1/groups/{group}/commits"),
                &json!({ "epoch": e.epoch, "commit": part(0), "group_info": part(1), "sealed_key": part(2), "recovery_auth": optional(3) }),
            )
            .await
        }
        OutboxKind::GroupFounding => {
            hub.post(
                "/v1/groups",
                &json!({ "group_info_0": part(0), "sealed_key_0": part(1), "commit": part(2), "group_info": part(3), "welcome": optional(4), "sealed_key": part(5) }),
            )
            .await
        }
        OutboxKind::Message | OutboxKind::RelayMessage => {
            hub.post(
                &format!("/v1/groups/{group}/messages"),
                &json!({ "epoch": e.epoch, "message": part(0), "relay": e.kind == OutboxKind::RelayMessage }),
            )
            .await
        }
        OutboxKind::Envelope => hub.post("/v1/envelopes", &json!({ "envelope": part(0) })).await,
        OutboxKind::KeyPackages => {
            let single: Vec<String> = e.parts.iter().skip(1).map(|p| b64s(p)).collect();
            let mut body = json!({ "single_use": single });
            if e.parts.first().is_some_and(|p| !p.is_empty()) {
                body["last_resort"] = json!(part(0));
            }
            hub.put("/v1/key-packages", &body).await
        }
        OutboxKind::SealedKey => hub.put("/v1/sealed-keys", &json!({ "sealed_key": part(0) })).await,
        OutboxKind::RecoveryCode => {
            let account: Value = e
                .parts
                .get(4)
                .and_then(|p| serde_json::from_slice(p).ok())
                .unwrap_or(Value::Null);
            hub.post(
                &format!("/v1/rooms/{}/recovery-code", b64s(room)),
                &json!({ "commit": { "epoch": e.epoch, "commit": part(0), "group_info": part(1), "sealed_key": part(2) },
                         "recovery_link": part(3), "account": account }),
            )
            .await
        }
        // (a room is founded, and a recovery run, only by the web and iOS apps)
        OutboxKind::RoomFounding | OutboxKind::RecoveryCommit | OutboxKind::RecoveryFinish => {
            Err(Fault::new("not-supported", "this app does not found rooms or recover them"))
        }
    }
}

impl Engine {
    /// Signs in, joins what this device was welcomed to, and reads everything after the cursor in the hub's order.
    pub async fn sync(self: &Arc<Self>) -> Result<()> {
        let _one = self.serial.lock().await;
        self.catch_up().await
    }

    async fn catch_up(self: &Arc<Self>) -> Result<()> {
        let groups_before = self.session_groups();
        self.take_welcomes().await?;
        let mut until = self.read_desk().await;
        // (a sign-in with the code: the live session groups, joined from outside, before the history is read)
        if self.code.lock().unwrap_or_else(|e| e.into_inner()).is_some() {
            self.say("Joining the sessions …");
            self.join_sessions_with_code().await;
        }
        // The hub's Desk lists only what lies in the groups this device is in: read again after a join.
        if self.session_groups() != groups_before {
            until = self.read_desk().await;
        }
        if self.publish_key_packages().await.is_ok() {
            self.pump_outbox();
        }
        self.say("Catching up …");
        self.read_changes().await?;
        let _ = until;
        // upkeep: a failure here does not keep the board from showing; the next sync tries again
        let _ = self.publish_key_packages().await;
        self.write_heads();
        self.tend_keys().await;
        self.synced.store(true, Ordering::SeqCst);
        self.emit();
        self.save_cache();
        self.pump_outbox();
        Ok(())
    }

    /// The hub's Desk: what is open now decides the Desk (the client core's rule); returns its change.
    async fn read_desk(&self) -> Option<u64> {
        let desk = match self.hub.get("/v1/desk?brief=1&active=1").await {
            Ok(d) if d["truncated"] != json!(true) && d["change"].is_u64() => d,
            _ => self.hub.get("/v1/desk?active=1").await.ok()?,
        };
        let at = desk["change"].as_u64()?;
        let mut listed = Vec::new();
        for field in ["cards", "permission_requests"] {
            for o in desk[field].as_array().into_iter().flatten() {
                let Ok(id) = bytes_of(o, "object_id") else { continue };
                let version = o["version"]["change"].as_u64().or(o["version_change"].as_u64()).unwrap_or(0);
                listed.push(DeskListed {
                    object_id: hex(&id),
                    version_change: version,
                });
            }
        }
        let _ = self.model.set_desk_listing(at, listed);
        self.emit();
        Some(at)
    }

    /// The Welcomes waiting for this device: the session groups it was added to.
    async fn take_welcomes(self: &Arc<Self>) -> Result<()> {
        let list = self.hub.get("/v1/welcomes").await?;
        let mut joined = Vec::new();
        for w in list.as_array().into_iter().flatten() {
            let Ok(bytes) = bytes_of(w, "welcome") else { continue };
            match self.device.join_welcome(bytes, self.room.clone(), None, now_ms()) {
                Ok(j) => joined.push(j.group),
                Err(e) if error_code_text(e.code()) == "wrong-epoch" => {}
                Err(e) => {
                    let _ = self.model.push_alert(
                        "welcome".into(),
                        format!("a Welcome was refused: {}", error_code_text(e.code())),
                        None,
                        None,
                        now_ms(),
                    );
                }
            }
        }
        let upto = self.device.cursor().map_err(fault)?;
        for group in &joined {
            self.hand_group_log(group, upto).await;
        }
        self.refresh_groups()?;
        Ok(())
    }

    /// Hands a group's log again from its start up to `upto`: for a Welcome taken after the device read past its
    /// Add (iOS `handGroupLog`).
    async fn hand_group_log(&self, group: &[u8], upto: u64) {
        let mut after = 0u64;
        for _ in 0..10_000 {
            let Ok(page) = self
                .hub
                .get(&format!("/v1/groups/{}/log?after={after}&limit=200", b64s(group)))
                .await
            else {
                return;
            };
            let raw = page["items"].as_array().cloned().unwrap_or_default();
            let mut end = page["more"] != json!(true) || raw.is_empty();
            for j in &raw {
                let (Some(n), Some(c)) = (j["n"].as_u64(), j["change"].as_u64()) else { return };
                after = n;
                if c > upto {
                    end = true;
                    break;
                }
                let Ok(bytes) = bytes_of(j, "bytes") else { return };
                let kind = match j["kind"].as_str() {
                    Some("commit") => LogEntryKind::Commit,
                    Some("message") => LogEntryKind::Message,
                    _ => continue,
                };
                let recovery_auth = j.get("recovery_auth").filter(|v| v.is_string()).and_then(|_| bytes_of(j, "recovery_auth").ok());
                let entry = LogEntry { change: c, group: group.to_vec(), kind, bytes, recovery_auth };
                match self.device.process_log_entry(entry, now_ms()) {
                    Ok(_) => {}
                    Err(e) => match log_finding(e.code()) {
                        LogFinding::Duplicate => {}
                        _ => return,
                    },
                }
            }
            if end {
                return;
            }
        }
    }

    /// Keeps the hub stocked with this device's KeyPackages (spec 14.2).
    pub(crate) async fn publish_key_packages(&self) -> Result<()> {
        let known = *self.key_packages_at_hub.lock().unwrap_or_else(|e| e.into_inner());
        let unused = match known {
            Some(n) => n,
            None => {
                let r = self.hub.put("/v1/key-packages", &json!({ "single_use": [] })).await?;
                let n = r["unused"].as_u64().unwrap_or(0) as u32;
                *self.key_packages_at_hub.lock().unwrap_or_else(|e| e.into_inner()) = Some(n);
                n
            }
        };
        self.device.key_packages_to_upload(unused, now_ms()).map_err(fault)?;
        Ok(())
    }

    /// `heads` (9.0.7) in each live group when the core says they are due.
    fn write_heads(&self) {
        let groups = self.groups.lock().unwrap_or_else(|e| e.into_inner()).clone();
        for g in groups.iter().filter(|g| !g.archived) {
            if let Ok(Some(value)) = self.device.heads_due(g.group.clone(), now_ms()) {
                let mut d = Self::draft(trommi_core_ffi::DraftKind::Register);
                d.group = Some(g.group.clone());
                d.name = Some("heads".into());
                d.value = Some(value);
                let _ = self.device.seal(d, None, Vec::new(), now_ms());
            }
        }
    }

    /// The content keys this device still lacks a while after an envelope did not open for them (7.1.1).
    async fn tend_keys(&self) {
        for r in self.device.key_requests(now_ms()).unwrap_or_default() {
            let _ = self
                .hub
                .post("/v1/requests", &json!({ "kind": "handover", "group": b64s(&r.group) }))
                .await;
        }
    }

    /// `GET /v1/changes` from the cursor until the hub has no more.
    pub(crate) async fn read_changes(&self) -> Result<()> {
        for _ in 0..100_000 {
            let before = self.cursor.load(Ordering::SeqCst);
            let page = self
                .hub
                .get(&format!("/v1/changes?after={before}&limit=500&active=1"))
                .await?;
            let raw = page["items"].as_array().cloned().unwrap_or_default();
            let items = ordered(&raw, before)?;
            self.process(items).await?;
            if let Some(upto) = page["change"].as_u64() {
                if upto > self.cursor.load(Ordering::SeqCst) {
                    self.cursor.store(upto, Ordering::SeqCst);
                }
            }
            if page["more"] != json!(true) {
                return Ok(());
            }
            if self.cursor.load(Ordering::SeqCst) <= before {
                return Err(Fault::new("bad-format", "the hub says there is more and gives nothing new"));
            }
            // a long catch-up shows what it has page by page
            self.emit();
        }
        Err(Fault::new("bad-format", "the hub's changes do not end"))
    }

    /// A run of items in the hub's order: each to the core, then what it gave to the model.
    pub(crate) async fn process(&self, items: Vec<Item>) -> Result<()> {
        let before = self.device.cursor().map_err(fault)?;
        let now = now_ms();
        let mut groups_stale = false;
        for item in items {
            let change = item.change();
            match item {
                Item::Log { entry, .. } => {
                    // (taken before the cache was last written: done with)
                    if entry.change <= before {
                        self.advance(change);
                        continue;
                    }
                    let group = entry.group.clone();
                    match self.device.process_log_entry(entry, now) {
                        Ok(p) => match p.kind {
                            ProcessedKind::Commit | ProcessedKind::Observed | ProcessedKind::JoinSuperseded => {
                                groups_stale = true;
                                if let Some(facts) = p.commit {
                                    self.refresh_groups()?;
                                    groups_stale = false;
                                    let by_recovery = facts.external && facts.group.len() == 32;
                                    let _ = self.model.apply_commit(facts, by_recovery, p.removed && group.len() == 32, now);
                                }
                            }
                            ProcessedKind::OwnCommit => groups_stale = true,
                            ProcessedKind::Message => {
                                if groups_stale {
                                    self.refresh_groups()?;
                                    groups_stale = false;
                                }
                                if let Some(m) = p.message {
                                    self.apply_message(m, &group, change);
                                }
                            }
                            ProcessedKind::Skipped => {}
                        },
                        Err(e) => match log_finding(e.code()) {
                            LogFinding::Duplicate => {}
                            LogFinding::Local | LogFinding::Early => {
                                return Err(fault(e));
                            }
                            LogFinding::BadGroup => {
                                let _ = self.model.push_alert(
                                    "bad-group".into(),
                                    format!("an item from the hub was refused (change {change})"),
                                    Some(change),
                                    None,
                                    now,
                                );
                            }
                        },
                    }
                }
                Item::Envelope { change, bytes, void } => {
                    let void = void.and_then(error_code_from_text);
                    let e = self
                        .device
                        // (taken before the cache was last written: read again without touching the chain)
                        .receive_envelope(bytes, change, change > before, void, now)
                        .map_err(fault)?;
                    if e.outcome == EnvelopeOutcome::Refused && change > before {
                        let code = e.code.map(error_code_text).unwrap_or_default();
                        // (its group's next Commits are still to come: read again with what it needs)
                        if code == "group-behind" && self.device.cursor().map_err(fault)? < change {
                            return Err(Fault::new("group-behind", "the catch-up stopped before a change it could not take yet"));
                        }
                    }
                    if groups_stale {
                        self.refresh_groups()?;
                        groups_stale = false;
                    }
                    self.apply_envelope(e, now);
                }
            }
            self.advance(change);
        }
        if groups_stale {
            self.refresh_groups()?;
        }
        if self.unsaved.load(Ordering::SeqCst) > 2_000 {
            self.save_cache();
        }
        Ok(())
    }

    fn advance(&self, change: u64) {
        if change > self.cursor.load(Ordering::SeqCst) {
            self.cursor.store(change, Ordering::SeqCst);
        }
        self.unsaved.fetch_add(1, Ordering::SeqCst);
    }

    /// One received envelope into the model.
    fn apply_envelope(&self, e: ReceivedEnvelope, now: u64) {
        match e.outcome {
            EnvelopeOutcome::Refused => return,
            EnvelopeOutcome::Void | EnvelopeOutcome::Chained | EnvelopeOutcome::Applied | EnvelopeOutcome::Provisional => {}
        }
        // `heads` is the devices' own bookkeeping (9.0.7): nothing of it is on the board
        if e.header.kind == EnvelopeKind::Register && e.register.as_ref().is_some_and(|r| r.name == "heads") {
            return;
        }
        let _ = self.model.apply_envelopes(vec![e], now);
    }

    /// An application message of a group: a step of a work trail, a newer version's notice.
    fn apply_message(&self, m: trommi_core_ffi::ReceivedMessage, group: &[u8], change: u64) {
        match m.kind {
            ReceivedKind::WorkTrail => {
                if group.len() != 48 {
                    return;
                }
                let session = hex(&group[32..]);
                let (Some(from), Some(turn)) = (m.from.clone(), m.turn.clone()) else { return };
                let _ = self
                    .model
                    .apply_work_trail(session, from, turn, m.number, m.time, m.payload.clone(), change);
            }
            ReceivedKind::NewerVersion => {
                let _ = self.model.push_alert(
                    "newer-version".into(),
                    "a newer Trommi wrote here: update this app".into(),
                    None,
                    m.from.as_deref().map(hex),
                    now_ms(),
                );
            }
            _ => {}
        }
    }

    // ---- the outbox ----------------------------------------------------------------------------------------

    /// Posts the core's outbox in order and reports each answer back (iOS `pumpOutbox`).
    pub fn pump_outbox(self: &Arc<Self>) {
        if self.pumping.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        tokio::spawn(async move {
            let mut backoff = 300u64;
            loop {
                if me.closed.load(Ordering::SeqCst) {
                    break;
                }
                let Some(entry) = me.device.outbox().ok().and_then(|o| o.into_iter().next()) else {
                    break;
                };
                match post_entry(&me.hub, &me.room, &entry).await {
                    Ok(r) => {
                        let Some(answer) = accepted(entry.kind, &r) else {
                            tokio::time::sleep(Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(30_000);
                            continue;
                        };
                        if entry.kind == OutboxKind::KeyPackages {
                            *me.key_packages_at_hub.lock().unwrap_or_else(|e| e.into_inner()) =
                                r["unused"].as_u64().map(|n| n as u32);
                        }
                        let _ = me.device.outbox_accepted(entry.id, answer);
                        backoff = 300;
                        // an own Commit takes effect from the log, at its place among the changes
                        if carries_commit(entry.kind) {
                            let _one = me.serial.lock().await;
                            let _ = me.read_changes().await;
                            let _ = me.refresh_groups();
                            me.emit();
                        }
                    }
                    Err(f) => {
                        if f.status == Some(426) {
                            me.say("This hub needs a newer Trommi");
                            break;
                        }
                        if is_transient(&f) || f.status.is_none() || !is_final_refusal(&f.code) {
                            tokio::time::sleep(Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(30_000);
                            continue;
                        }
                        let voided = f.extra.get("voided") == Some(&Value::Bool(true));
                        let Some(code) = error_code_from_text(f.code.clone()) else { break };
                        let done = if entry.kind != OutboxKind::Envelope {
                            me.device.outbox_refused(entry.id, code)
                        } else if voided {
                            me.device.outbox_voided(entry.id)
                        } else if f.code == "not-member" || f.code == "removed-sender" {
                            me.device.envelope_abandon(entry.id)
                        } else {
                            // (an envelope the hub refused without a number waits and goes again)
                            tokio::time::sleep(Duration::from_millis(backoff)).await;
                            backoff = (backoff * 2).min(30_000);
                            continue;
                        };
                        if done.is_err() {
                            break;
                        }
                        let _ = me.model.push_alert(
                            f.code.clone(),
                            format!("the hub refused something this device sent: {}", f.message),
                            None,
                            None,
                            now_ms(),
                        );
                        me.emit();
                    }
                }
            }
            me.pumping.store(false, Ordering::SeqCst);
        });
    }

    // ---- live ----------------------------------------------------------------------------------------------

    /// The live stream while the app runs: every item as the hub takes it, presence as devices come and go.
    /// Reconnects with a growing pause; resumes after the cursor, so nothing between two streams is missed.
    pub async fn run_live(self: Arc<Self>) {
        let mut pause = 500u64;
        while !self.closed.load(Ordering::SeqCst) {
            if !self.synced.load(Ordering::SeqCst) {
                match self.sync().await {
                    Ok(()) => {}
                    Err(f) => {
                        self.say(format!("Offline: {}", f.text()));
                        tokio::time::sleep(Duration::from_millis(pause)).await;
                        pause = (pause * 2).min(30_000);
                        continue;
                    }
                }
            }
            let after = self.cursor.load(Ordering::SeqCst);
            match self.hub.stream(after).await {
                Ok(mut stream) => {
                    self.live.store(true, Ordering::SeqCst);
                    let _ = self.model.stream_opened();
                    (self.sink)(super::Update {
                        live: Some(true),
                        status: Some("Live".into()),
                        ..Default::default()
                    });
                    self.emit();
                    loop {
                        // (a wake of the clock: a snooze or a status that runs out with nothing arriving)
                        let wake = self.next_wake.load(Ordering::SeqCst);
                        let wait = if wake == 0 { 40_000 } else { wake.saturating_sub(now_ms()).clamp(200, 40_000) };
                        let next = tokio::time::timeout(Duration::from_millis(wait + 5_000), stream.next(Duration::from_secs(60))).await;
                        match next {
                            Err(_) => {
                                self.emit();
                                continue;
                            }
                            Ok(Ok(Some(event))) => {
                                pause = 500;
                                self.on_event(&event.name, &event.data).await;
                            }
                            Ok(Ok(None)) | Ok(Err(_)) => break,
                        }
                        if self.closed.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                }
                Err(f) => {
                    if f.code == "unauthorised" || f.code == "not-member" {
                        self.synced.store(false, Ordering::SeqCst);
                    }
                    self.say(format!("Offline: {}", f.text()));
                }
            }
            self.live.store(false, Ordering::SeqCst);
            (self.sink)(super::Update {
                live: Some(false),
                ..Default::default()
            });
            tokio::time::sleep(Duration::from_millis(pause)).await;
            pause = (pause * 2).min(30_000);
        }
    }

    async fn on_event(self: &Arc<Self>, name: &str, data: &Value) {
        match name {
            "envelope" | "log" => {
                let item = Item::of(data).ok().flatten();
                if item.as_ref().is_some_and(|i| i.change() <= self.cursor.load(Ordering::SeqCst)) {
                    return;
                }
                let _one = self.serial.lock().await;
                let next = item.filter(|i| i.change() > self.cursor.load(Ordering::SeqCst));
                let taken = match next {
                    Some(i) => self.process(vec![i]).await.is_ok(),
                    None => false,
                };
                if !taken {
                    let _ = self.read_changes().await;
                }
                self.emit();
                drop(_one);
                self.pump_outbox();
            }
            "welcome" => {
                let _one = self.serial.lock().await;
                let before = self.session_groups();
                let _ = self.take_welcomes().await;
                if self.session_groups() != before {
                    self.read_desk().await;
                    let _ = self.read_changes().await;
                }
                self.emit();
            }
            "presence" => {
                let _ = self.model.apply_presence_event(data.to_string(), now_ms());
                self.emit();
            }
            "here" => {
                let _ = self.model.apply_here(now_ms());
                self.emit();
            }
            "ping" => {
                if self.unsaved.load(Ordering::SeqCst) > 0 {
                    self.save_cache();
                }
                self.emit();
            }
            _ => {}
        }
    }
}
