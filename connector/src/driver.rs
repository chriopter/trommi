//! The driver: this device at work in its room, on the app core's engine in agent mode
//! (`core/client/src/engine/agent.rs`). The engine signs in, holds the lease, follows the room from the invite's
//! epoch, takes Welcomes and a group's past, reads the hub's order, puts what the protocol core marks a command to
//! the gate, posts the outbox and seals again what the hub voided; the driver runs its I/O and gives the rest of the
//! connector what it needs: the board model (`core/client/src/model`, fed by the engine's reports), the writes of an
//! agent, the human's commands, the lease's report, and why the work stopped.
//!
//! ```text
//! bridge, server, member ──▶ Driver ──lock──▶ Inner { Engine (agent), Board (the model), cache keeper }
//!                              │   effects of a call queue in `Inner::pending`, a poke wakes the loop
//!                              ▼
//!                        trommi-io's loop (one task, one queue): Http, OpenStream, Store, Timer
//!                              │   Store: the slot's journal (`Stores::in_journal`): the device's entries, and
//!                              ▼   the engine's record and the model's cache under the side tag `c`
//!                        DriverEvent ──▶ member.rs (commands, removed, errors, the session)
//! ```
//!
//! **One order.** Every call into the engine holds [`Inner`]'s lock; the effects a call makes wait in `pending` and
//! go out in front of those of the next event the loop handles (a poke is such an event), so what the engine asked
//! for leaves in the order it was asked, its device writes first (the loop's barrier).
//!
//! **Where state lives.** The slot's journal (`store.rs`) holds the device, the engine's record (its place, the
//! commands not yet handed over, the note beside each write), the model as the core's cache keeps it, the room
//! record (`k`/`room`) and the connector's own values (`k`/<name>). Nothing of the old client's tags is read.
//!
//! **Waiting.** A write returns once the hub took it and the model reached its place, or when the hub is away
//! (then it waits in the outbox and goes when the hub is back), or with the hub's refusal.
use crate::error::{Fault, Result};
use crate::store::{Journal, SIDE};
use crate::util::{hex, now_ms, unhex};
use crate::vault::side_key;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use trommi_client_core::app::io::{DeviceEntry, HttpRequest};
use trommi_client_core::app::{Effect, Event, IoId, LogLevel};
use trommi_client_core::cache;
use trommi_client_core::engine::feed::Board;
use trommi_client_core::engine::{
    AgentCommand, AgentInvite, AgentWrite, Alert, Connection, Ctx, Engine, Report, Stopped,
};
use trommi_client_core::hub::{Answer, Codec, Expect, Failure, LinkReport, LogItem, Route, Site};
use trommi_client_core::model::Model;
use trommi_core::device::{EnvelopeOutcome, Sealed};
use trommi_core::ids::{DeviceId, GroupId, Hash32, ObjectId, RoomId, SessionId};
use trommi_core::store::{OutboxKind, Storage as _};

/// A boxed future, for the traits the server implements.
pub type BoxFut<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// The connector's own values in the journal: the room record and the bridge's state, by name.
const TAG_KV: u8 = b'k';
/// Tags the client before the driver wrote (its model, outbox notes, halt, commands, own heads): taken away at
/// the first open.
const OLD_TAGS: [u8; 5] = *b"xqmdw";
/// The most work-trail steps that wait for the hub at once; a step beyond it is dropped (a trail is live).
pub const MAX_WAITING_STEPS: usize = 100;
/// How long a write waits for the hub to take it before it is left to the outbox.
const WRITE_WAIT: Duration = Duration::from_secs(30);
/// How long a write waits for the model to reach its place after the hub took it.
const PLACE_WAIT: Duration = Duration::from_secs(20);
/// How long a request of an agent's own (a file, a Share link, a claim) waits for its answer.
const REQUEST_WAIT: Duration = Duration::from_secs(60);
/// How long [`Driver::start`] waits for the first round to end.
const START_WAIT: Duration = Duration::from_secs(60);

/// Sleeps `ms` milliseconds.
pub async fn sleep(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// A command of a human for the agent, after the core's gate (9.0.9).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Command {
    /// It was let through before this process started and never noted as handed over: the agent may or may
    /// not have had it (9.0.9: reported, not repeated).
    pub uncertain: bool,
    /// `message`, `answer`, `trust`, `read`, `shred`, `decide_again`, `verdict`, `unsupported`.
    pub command: String,
    pub session_id: Option<String>,
    /// The hub's change number of the envelope.
    pub envelope_number: u64,
    /// The envelope's hash, base64url.
    pub envelope_hash: String,
    pub sender_device_id: String,
    pub sender_sequence: u64,
    pub sent_at: u64,
    pub object_id: Option<String>,
    /// `chat:session/<id>` or `chat:card/<id>` for a message.
    pub timeline_key: Option<String>,
    /// The envelope's payload.
    pub content: Map<String, Value>,
    pub late: bool,
    pub history: bool,
    pub choices: Vec<String>,
    pub previous_choices: Vec<String>,
    pub allow: bool,
    /// Whether the answer closed the card by itself (a final option).
    pub settled: bool,
    pub unsupported: Option<String>,
    pub what: Option<String>,
}

impl Command {
    /// The command as the core's gate let it through; `previous_choices` of a take back from the model, which
    /// keeps the answer taken back.
    fn of(c: &AgentCommand) -> Command {
        Command {
            uncertain: c.uncertain,
            command: c.command.clone(),
            session_id: c.session_id.clone(),
            envelope_number: c.envelope_number,
            envelope_hash: c.envelope_hash.clone(),
            sender_device_id: c.sender_device_id.clone(),
            sender_sequence: c.sender_sequence,
            sent_at: c.sent_at,
            object_id: c.object_id.clone(),
            timeline_key: c.timeline_key.clone(),
            content: c.content(),
            choices: c.choices.clone(),
            previous_choices: c.previous_choices.clone(),
            allow: c.allow,
            settled: c.settled,
            what: c.what.clone(),
            ..Command::default()
        }
    }
}

/// What the driver tells its owner.
#[derive(Clone, Debug)]
pub enum DriverEvent {
    /// A human's command.
    Command(Box<Command>),
    /// This device is out of the room or of its session: removed, or another connector took the session over.
    /// `verified`: it processed the Commit that removed it. Otherwise the hub says so (it ends a removed
    /// device's access at once, so the Commit may never reach it): the device stops, and keeps its state.
    Removed { replaced: bool, verified: bool },
    /// Something the owner must know: `lease-lost`, `client-too-old`, `bad-group`, …
    Error(Fault),
    /// This device is a leaf of its main session now.
    Session { session_id: String },
}

/// What a joined slot knows of its room beside the device: stored when the invite was answered.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RoomRecord {
    /// The hub's canonical address.
    pub hub: String,
    /// The app's origin, for Share links.
    pub app: String,
    /// The room, base64url.
    pub room: String,
    /// The session the invite was for, as hex; empty for a new session.
    #[serde(default)]
    pub invited_session: String,
    /// The room epoch the Offer named: where this device starts to follow the room group.
    #[serde(default)]
    pub room_epoch: u64,
    /// Whether the inviter's enrolment of this device was processed.
    #[serde(default)]
    pub enrolled: bool,
}

/// The room record of a slot's journal, if the slot was joined.
pub fn room_record(journal: &Journal) -> Option<RoomRecord> {
    journal
        .get(&side_key(TAG_KV, &[b"room"]))
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// Stages the room record.
pub fn put_room_record(journal: &Journal, record: &RoomRecord) {
    if let Ok(bytes) = serde_json::to_vec(record) {
        journal.put(side_key(TAG_KV, &[b"room"]), bytes);
    }
}

/// A value of the connector's own in a journal, by name (the bridge's state, what waited for another session).
pub fn kv_get(journal: &Journal, name: &str) -> Option<Value> {
    journal
        .get(&side_key(TAG_KV, &[name.as_bytes()]))
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}

/// Writes such a value at once.
pub fn kv_set(journal: &Journal, name: &str, value: &Value) -> Result<()> {
    journal.put(
        side_key(TAG_KV, &[name.as_bytes()]),
        value.to_string().into_bytes(),
    );
    Ok(journal.commit()?)
}

/// What a send reports.
#[derive(Clone, Debug, Default)]
pub struct SentInfo {
    /// The envelope's hash, base64url.
    pub hash: String,
    /// The object the envelope belongs to (of a first version or a request: the new object), as hex.
    pub object_id: Option<String>,
    /// The hub's change number, once the hub took the envelope.
    pub envelope_number: Option<u64>,
}

/// The hub's failure as the connector's fault: the hub's own refusal under its code, a proxy's under
/// `http-<status>`, "not now" and "no answer" as transient, an answer the codec refused as `bad-format`.
pub fn fault_of(failure: &Failure) -> Fault {
    match failure {
        Failure::Refused {
            status,
            code: Some(code),
        } => Fault::new(code.clone(), format!("the hub refused it ({code})")).status(*status),
        Failure::Refused { status, code: None } => Fault::new(
            format!("http-{status}"),
            format!("what answers at the hub's address is no Trommi hub (HTTP {status})"),
        )
        .status(*status),
        Failure::NotNow { status, retry_after_ms } => {
            let mut fault = Fault::new("overloaded", "the hub asked to try again later").status(*status);
            fault.retry_after = retry_after_ms.map(|ms| ms.div_ceil(1_000));
            fault
        }
        Failure::Offline => Fault::new("offline", "the hub could not be reached"),
        Failure::BadAnswer { why } => {
            Fault::new("bad-format", format!("the hub's answer does not read ({why})"))
        }
    }
}

/// Whether a fault is worth another try later with the same bytes: the hub was not reached, or it asked to wait.
pub fn is_transient(fault: &Fault) -> bool {
    matches!(
        fault.code.as_str(),
        "offline" | "overloaded" | "rate-limited" | "internal"
    ) || fault.status.is_some_and(|status| status >= 500)
}

/// Whether what answered is no hub of this protocol.
pub fn is_no_hub(fault: &Fault) -> bool {
    fault.code.starts_with("http-") || fault.code == "bad-format" || fault.code == "client-too-old"
}

/// base64url, as the wire spells bytes.
pub fn b64(bytes: &[u8]) -> String {
    trommi_core::ids::base64url_encode(bytes)
}

/// The fault a stopped engine stands for, and its words.
fn stop_fault(why: Stopped) -> Fault {
    let (code, text) = match why {
        Stopped::Removed | Stopped::Replaced | Stopped::NotMember => {
            ("removed", "this device is out of the room")
        }
        Stopped::LeaseLost => ("lease-lost", "another process took over this key; this one stops"),
        Stopped::RolledBack => (
            "state-rolled-back",
            "this connector's stored state is older than what it already sent; nothing is signed with it",
        ),
        Stopped::BadInvite => (
            "bad-invite",
            "this device was enrolled by another device than the one that invited it",
        ),
        Stopped::BadWelcome => (
            "bad-welcome",
            "the hub handed this device a Welcome into a group it must not hold; nothing of it was kept",
        ),
        Stopped::HubUnusable => (
            "hub-unusable",
            "the hub does not answer as a Trommi hub of this version; the connector stopped asking",
        ),
        Stopped::Store | Stopped::Write | Stopped::Device => (
            "storage",
            "this connector's state could not be written; it stopped working on its room",
        ),
    };
    Fault::new(code, text)
}

/// What the loop and the owner share beside the lock.
struct Shared {
    events: tokio::sync::mpsc::UnboundedSender<DriverEvent>,
    /// Fires after every step: waiters look again.
    changed: tokio::sync::Notify,
    /// The owner stopped the driver: no command is handed out any more.
    stopped: AtomicBool,
}

impl Shared {
    fn emit(&self, event: DriverEvent) {
        let _ = self.events.send(event);
    }
}

/// The engine and what follows from its reports. Taken with [`Driver::lock`].
pub struct Inner {
    engine: Engine,
    board: Board,
    cache: Option<cache::Keeper>,
    next_id: u64,
    /// Effects of a call, started in front of those of the next event the loop handles.
    pending: Vec<Effect>,
    device_writes: BTreeSet<IoId>,
    timer: Option<u64>,
    /// The cache was read; the engine starts once the owner said so too.
    cache_read: bool,
    start_wanted: bool,
    started: bool,
    /// The main session, as hex.
    main: Option<String>,
    /// What the hub said of this device's outbox entries: taken (with its change), refused (with the code).
    accepted: BTreeMap<u64, Option<u64>>,
    refused: BTreeMap<u64, &'static str>,
    /// An envelope the hub voided for its epoch, by its outbox id: the one sealed in its place.
    resealed: BTreeMap<u64, (u64, Hash32, Option<ObjectId>)>,
    /// The pump halts on this entry, with the hub's code.
    blocked: Option<(u64, &'static str)>,
    /// Answers to this driver's own requests, by their id.
    answers: BTreeMap<IoId, std::result::Result<trommi_client_core::app::io::HttpResponse, Failure>>,
    /// Why the engine stopped, and whether this device is out: `(replaced, verified)`.
    stopped: Option<Stopped>,
    removal: Option<(bool, bool)>,
    /// The code of the last fault the owner was told of: the same one is not said again and again.
    told: String,
}

impl Inner {
    /// The board model.
    pub fn model(&self) -> &Model {
        &self.board.model
    }

    /// The engine (read only: what the device holds).
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// The main session, as hex.
    pub fn session_id(&self) -> Option<String> {
        self.main.clone()
    }

    /// The live session groups this device is a leaf of, by session id as hex.
    pub fn groups(&self) -> BTreeMap<String, GroupId> {
        self.engine
            .device()
            .groups()
            .unwrap_or_default()
            .into_iter()
            .filter(|g| !g.archived)
            .filter_map(|g| Some((hex(g.session?.session_id.as_bytes()), g.group)))
            .collect()
    }

    /// Every session this device is a leaf of.
    pub fn session_ids(&self) -> Vec<String> {
        self.groups().into_keys().collect()
    }

    /// The helper sessions this device is in beside its main one.
    pub fn child_session_ids(&self) -> Vec<String> {
        self.groups()
            .into_keys()
            .filter(|sid| Some(sid) != self.main.as_ref())
            .collect()
    }

    /// The group of a session; the main session's for none.
    pub fn group_of(&self, session_id: Option<&str>) -> Result<GroupId> {
        let sid = session_id
            .map(str::to_owned)
            .or_else(|| self.main.clone())
            .ok_or_else(|| Fault::new("no-session", "this agent has no session yet"))?;
        self.groups()
            .get(&sid)
            .copied()
            .ok_or_else(|| Fault::new("no-session", "this agent is not in that session"))
    }

    /// The outbox ids of this device's envelopes the hub has not taken yet, oldest first.
    pub fn outbox(&self) -> Vec<u64> {
        self.engine
            .device()
            .outbox()
            .iter()
            .filter(|entry| entry.kind == OutboxKind::Envelope)
            .map(|entry| entry.id)
            .collect()
    }

    /// This device's place in the hub's order.
    pub fn cursor(&self) -> u64 {
        self.engine.position()
    }

    /// `connected`, `offline` or `removed`.
    pub fn connection(&self) -> &'static str {
        if self.removal.is_some() {
            "removed"
        } else if self.engine.connection() == Connection::Live {
            "connected"
        } else {
            "offline"
        }
    }

    /// Whether this device is out, and how: `(replaced, verified)`.
    pub fn removal(&self) -> Option<(bool, bool)> {
        self.removal
    }

    /// The code the pump halts on (an envelope the hub refused for good), if it does.
    pub fn blocked(&self) -> Option<&'static str> {
        self.blocked.map(|(_, code)| code)
    }

    /// Calls the engine with a context; what it asks for waits in `pending`.
    fn call<T>(&mut self, now: u64, shared: &Shared, step: impl FnOnce(&mut Engine, &mut Ctx<'_>) -> T) -> T {
        let mut out = std::mem::take(&mut self.pending);
        let result = {
            let mut ctx = Ctx {
                now,
                next_id: &mut self.next_id,
                out: &mut out,
            };
            step(&mut self.engine, &mut ctx)
        };
        self.after(now, &mut out, shared);
        self.pending = out;
        result
    }

    /// One event of the loop.
    fn handle(&mut self, now: u64, event: Event, shared: &Shared) -> Vec<Effect> {
        let mut out = std::mem::take(&mut self.pending);
        {
            let mut ctx = Ctx {
                now,
                next_id: &mut self.next_id,
                out: &mut out,
            };
            match event {
                Event::Http { id, result } => {
                    self.engine.on_http(&mut ctx, id, result);
                }
                Event::Stream { id, item } => {
                    self.engine.on_stream(&mut ctx, id, item);
                }
                Event::Store { id, result } => {
                    self.device_writes.remove(&id);
                    let took = match self.cache.as_mut() {
                        Some(cache) => cache.on_store(id, &result, &mut self.board.model, now, ctx.next_id, ctx.out),
                        None => cache::Took::No,
                    };
                    match took {
                        cache::Took::No => {
                            self.engine.on_store(&mut ctx, id, &result);
                        }
                        cache::Took::Mine => {}
                        cache::Took::Read(read) => {
                            if let cache::Read::Dropped = read {
                                self.board = fresh_board(&self.engine);
                            }
                            let change = self.board.finish(now);
                            if let Some(cache) = self.cache.as_mut() {
                                cache.note(&change);
                            }
                            self.cache_read = true;
                            if self.start_wanted && !self.started {
                                self.started = true;
                                self.engine.start(&mut ctx);
                            }
                        }
                    }
                }
                Event::Wake => {
                    self.timer = None;
                    self.engine.on_wake(&mut ctx);
                }
                // (the driver's poke: what a call asked for goes out)
                _ => {}
            }
        }
        self.after(now, &mut out, shared);
        out
    }

    /// What follows every step: the reports into the model and to the owner, the cache behind the board, the timer.
    fn after(&mut self, now: u64, out: &mut Vec<Effect>, shared: &Shared) {
        self.device_writes.extend(out.iter().filter_map(|effect| match effect {
            Effect::Store { id, op: trommi_client_core::app::StoreOp::DeviceApply { .. } } => Some(*id),
            _ => None,
        }));
        let mut fed = false;
        while let Some(report) = self.engine.next_report() {
            self.board.take(&report, self.engine.device(), now);
            fed = true;
            self.report(report, shared);
        }
        let model = &self.board.model;
        let due = |at: u64| at != 0 && now >= at;
        if fed || due(model.desk_wake) || due(model.status_wake) {
            let change = self.board.finish(now);
            if let Some(cache) = self.cache.as_mut() {
                cache.note(&change);
            }
        }
        let mut cache_wake = None;
        if let Some(cache) = self.cache.as_mut() {
            cache.saw(out);
            cache.step(now, &mut self.next_id, out);
            cache_wake = cache.next_wake();
        }
        let model = &self.board.model;
        let wake = [
            self.engine.next_wake(),
            cache_wake,
            Some(model.desk_wake).filter(|at| *at != 0),
            Some(model.status_wake).filter(|at| *at != 0),
        ]
        .into_iter()
        .flatten()
        .min();
        if wake != self.timer {
            self.timer = wake;
            out.push(Effect::Timer { at_ms: wake });
        }
    }

    fn tell(&mut self, shared: &Shared, fault: Fault) {
        // (said once per kind of fault, not once per try)
        if self.told != fault.code {
            self.told = fault.code.clone();
            shared.emit(DriverEvent::Error(fault));
        }
    }

    fn report(&mut self, report: Report, shared: &Shared) {
        match report {
            Report::Envelope { received, .. } => {
                let me = self.engine.device().id();
                let code = received.code.as_ref().map_or("", |code| code.code());
                if received.outcome == EnvelopeOutcome::Chained && code == "newer-version" && received.header.sender != me {
                    // 9.0.4: no command, whoever sent it: a notice that a newer connector is needed
                    self.tell(shared, Fault::new("newer-version", "an item of a newer Trommi arrived that this connector cannot read"));
                }
                if received.header.sender == me && matches!(received.outcome, EnvelopeOutcome::Void | EnvelopeOutcome::Chained) {
                    eprintln!("[trommi] an envelope of this device was not applied: {code}");
                }
            }
            Report::Groups => {
                let main = self.engine.main_group().and_then(|g| g.session_id()).map(|s| hex(s.as_bytes()));
                if main.is_some() && main != self.main {
                    if let Some(session_id) = main.clone() {
                        shared.emit(DriverEvent::Session { session_id });
                    }
                }
                self.main = main;
            }
            Report::Alert(alert) => {
                let fault = match alert {
                    Alert::Withheld { .. } => Some(Fault::new("withheld", "the hub withholds part of a session's history; the Trommi app shows it")),
                    Alert::BadGroup { code, .. } => Some(Fault::new(
                        "bad-group",
                        format!("the hub handed out a group change this device cannot take ({code}); the human looks at the room in the Trommi app and reconnects this session"),
                    )),
                    Alert::Finding { code, .. } if matches!(code, "equivocation" | "bad-group" | "withheld") => {
                        Some(Fault::new(code, "the device found something wrong in a session; the Trommi app shows it"))
                    }
                    Alert::ClientTooOld => Some(Fault::new("client-too-old", "the hub needs a newer connector")),
                    Alert::NewerVersion { .. } => Some(Fault::new("newer-version", "a message of a newer Trommi arrived")),
                    Alert::Finding { code, .. } | Alert::Welcome { code, .. } | Alert::NotAnEnvelope { code, .. } => {
                        eprintln!("[trommi] finding: {code}");
                        None
                    }
                    other => {
                        eprintln!("[trommi] {other:?}");
                        None
                    }
                };
                if let Some(fault) = fault {
                    self.tell(shared, fault);
                }
            }
            Report::Accepted { id, change, .. } => {
                if self.accepted.len() >= 4096 {
                    self.accepted.pop_first();
                }
                self.accepted.insert(id, change);
            }
            Report::Refused { id, code, .. } => {
                if self.refused.len() >= 4096 {
                    self.refused.pop_first();
                }
                if code != "epoch-taken" {
                    eprintln!("[trommi] the hub refused a request of this device: {code}");
                }
                self.refused.insert(id, code);
            }
            Report::Resealed { voided, id, envelope_hash, object_id, .. } => {
                if self.resealed.len() >= 4096 {
                    self.resealed.pop_first();
                }
                self.resealed.insert(voided, (id, envelope_hash, object_id));
            }
            Report::Blocked(blocked) => {
                if let Some((_, code)) = blocked {
                    eprintln!("[trommi] the hub refused an envelope of this device: {code}; it stays in the outbox, and nothing more is sent");
                }
                self.blocked = blocked;
            }
            Report::Answer(answered) => {
                self.answers.insert(answered.id, answered.result);
            }
            Report::Command(command) => {
                if !shared.stopped.load(Ordering::SeqCst) {
                    let command = Command::of(&command.with_model(&self.board.model));
                    shared.emit(DriverEvent::Command(Box::new(command)));
                }
            }
            Report::Stopped(why) => {
                self.stopped = Some(why);
                match why {
                    Stopped::Removed | Stopped::Replaced | Stopped::NotMember => {
                        let verified = why != Stopped::NotMember;
                        let replaced = why == Stopped::Replaced || self.removal.is_some_and(|(r, _)| r);
                        if self.removal != Some((replaced, verified)) {
                            self.removal = Some((replaced, verified));
                            shared.emit(DriverEvent::Removed { replaced, verified });
                        }
                    }
                    other => {
                        let fault = stop_fault(other);
                        eprintln!("[trommi] {}", fault.text());
                        self.tell(shared, fault);
                    }
                }
            }
            _ => {}
        }
    }
}

fn fresh_board(engine: &Engine) -> Board {
    let mut board = Board::new(engine.device());
    // (an agent reads no Desk: nothing is held for one, and its sessions are read in the hub's order)
    board.model.set_desk_hold(false);
    board.model.reading = false;
    board
}

/// The loop's side of the driver (`trommi_io::Core`).
struct Run {
    inner: Arc<Mutex<Inner>>,
    shared: Arc<Shared>,
}

fn lock<T>(inner: &Mutex<T>) -> MutexGuard<'_, T> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

impl trommi_io::Core for Run {
    fn handle(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        let out = lock(&self.inner).handle(now_ms, event, &self.shared);
        self.shared.changed.notify_waiters();
        out
    }
}

/// The loop's window: only the core's log lines.
struct Logs;

impl trommi_io::Shell for Logs {
    fn show(&mut self, effect: Effect) {
        if let Effect::Log { level, text } = effect {
            if level != LogLevel::Debug {
                eprintln!("[trommi] core: {text}");
            }
        }
    }
}

/// This device at work. See the module's documentation.
pub struct Driver {
    inner: Arc<Mutex<Inner>>,
    shared: Arc<Shared>,
    handle: Mutex<Option<trommi_io::Handle>>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    journal: Journal,
    me: DeviceId,
    room: RoomId,
    record: RoomRecord,
    /// Files this process uploaded or fetched, by their whole reference: the newest few.
    files: Mutex<Vec<(String, Vec<u8>)>>,
}

impl Driver {
    /// Opens the driver over a joined slot: `journal` holds the device and the room record. The model is read
    /// from the slot's cache before this returns; nothing is sent yet: [`Driver::start`] goes online.
    pub async fn open(
        journal: Journal,
    ) -> Result<(Arc<Driver>, tokio::sync::mpsc::UnboundedReceiver<DriverEvent>)> {
        let record = room_record(&journal).ok_or_else(|| {
            Fault::new("no-room", "this slot holds no room: join with an invite link")
        })?;
        let room = RoomId::from_base64url(&record.room)
            .map_err(|_| Fault::new("state-damaged", "the stored room does not read"))?;
        // what the client before the driver kept beside the device is not read any more
        let mut old = false;
        for tag in OLD_TAGS {
            for (key, _) in journal.scan(&[SIDE, tag]) {
                journal.delete(key);
                old = true;
            }
        }
        if old {
            journal.commit()?;
        }
        let loaded = journal
            .core_store()
            .load()
            .map_err(|_| Fault::new("state-damaged", "the stored state does not read"))?;
        let entries: Vec<DeviceEntry> = loaded
            .entries
            .iter()
            .map(|e| DeviceEntry {
                key: e.key.clone().into(),
                value: e.value.clone().into(),
            })
            .collect();
        let session = match record.invited_session.as_str() {
            "" => None,
            text => Some(
                unhex(text)
                    .and_then(|bytes| SessionId::from_slice(&bytes).ok())
                    .ok_or_else(|| Fault::new("state-damaged", "the stored invite does not read"))?,
            ),
        };
        let invite = AgentInvite {
            room,
            room_epoch: record.room_epoch,
            session,
        };
        let seed = u64::from_le_bytes(crate::util::random::<8>()?) >> 11;
        let engine = Engine::open_agent(&record.hub, crate::CLIENT, seed, loaded.revision, entries, invite)
            .map_err(|e| match e {
                trommi_client_core::engine::OpenError::Hub => {
                    Fault::new("bad-format", "the hub's address is not canonical")
                }
                other => Fault::new("state-damaged", format!("the stored state does not open ({other:?})")),
            })?;
        let me = engine.device().id();
        let (events, rx) = tokio::sync::mpsc::unbounded_channel();
        let shared = Arc::new(Shared {
            events,
            changed: tokio::sync::Notify::new(),
            stopped: AtomicBool::new(false),
        });
        let board = fresh_board(&engine);
        let mut inner = Inner {
            engine,
            board,
            cache: None,
            next_id: 1,
            pending: Vec::new(),
            device_writes: BTreeSet::new(),
            timer: None,
            cache_read: false,
            start_wanted: false,
            started: false,
            main: None,
            accepted: BTreeMap::new(),
            refused: BTreeMap::new(),
            resealed: BTreeMap::new(),
            blocked: None,
            answers: BTreeMap::new(),
            stopped: None,
            removal: None,
            told: String::new(),
        };
        inner.main = inner
            .engine
            .main_group()
            .and_then(|g| g.session_id())
            .map(|s| hex(s.as_bytes()));
        // the model as it was last kept, read before anything is asked of the hub
        let mut out = Vec::new();
        inner.cache = Some(cache::Keeper::open(
            &hex(room.as_bytes()),
            &hex(me.as_bytes()),
            loaded.revision,
            &mut inner.next_id,
            &mut out,
        ));
        inner.pending = out;
        let inner = Arc::new(Mutex::new(inner));
        let stores = trommi_io::Stores::in_journal(journal.clone());
        let run = Run {
            inner: inner.clone(),
            shared: shared.clone(),
        };
        let (handle, task) = trommi_io::start(run, Logs, stores)
            .map_err(|e| Fault::new("internal", format!("the driver's loop did not start: {e}")))?;
        let driver = Arc::new(Driver {
            inner,
            shared,
            handle: Mutex::new(Some(handle)),
            task: Mutex::new(Some(task)),
            journal,
            me,
            room,
            record,
            files: Mutex::new(Vec::new()),
        });
        driver.poke();
        driver
            .wait(Duration::from_secs(30), |inner| inner.cache_read.then_some(()))
            .await;
        Ok((driver, rx))
    }

    /// Takes the lock on the engine and the model. Never held across an await.
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        lock(&self.inner)
    }

    /// Wakes the loop: what a call asked for goes out.
    fn poke(&self) {
        if let Some(handle) = lock(&self.handle).as_ref() {
            let _ = handle.send(Event::Foreground);
        }
    }

    /// Calls the engine and lets what it asked for go out.
    fn call<T>(&self, step: impl FnOnce(&mut Engine, &mut Ctx<'_>) -> T) -> T {
        let result = self.lock().call(now_ms(), &self.shared, step);
        self.shared.changed.notify_waiters();
        self.poke();
        result
    }

    /// Waits until `check` finds what it looks for, at most `timeout`; looked at after every step of the loop.
    async fn wait<T>(&self, timeout: Duration, mut check: impl FnMut(&mut Inner) -> Option<T>) -> Option<T> {
        let until = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.shared.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if let Some(found) = check(&mut self.lock()) {
                return Some(found);
            }
            tokio::select! {
                _ = notified => {}
                _ = tokio::time::sleep_until(until) => return check(&mut self.lock()),
                // (a step that changed nothing the check sees is looked at again now and then)
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
        }
    }

    /// This device's id, base64url.
    pub fn me(&self) -> String {
        self.me.to_base64url()
    }

    /// This device's id.
    pub fn device_id(&self) -> DeviceId {
        self.me
    }

    /// This device's id as the model writes device ids: hex.
    pub fn me_hex(&self) -> String {
        hex(self.me.as_bytes())
    }

    /// The room record the slot was joined with.
    pub fn room(&self) -> &RoomRecord {
        &self.record
    }

    /// The room.
    pub fn room_id(&self) -> RoomId {
        self.room
    }

    /// The slot's journal (the connector's own values).
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// Resolves once this device is a leaf of its main session; returns the session id.
    pub async fn when_session(&self) -> String {
        loop {
            if let Some(session) = self
                .wait(Duration::from_secs(3600), |inner| inner.session_id())
                .await
            {
                return session;
            }
        }
    }

    /// Waits for the founding's Commit to be verified and merged, not merely accepted by the hub.
    pub(crate) async fn group_ready(&self, session: &str) -> bool {
        self.wait(PLACE_WAIT, |inner| {
            inner.engine.device().groups().ok()?.into_iter().any(|group| {
                !group.archived && !group.pending && group.session.is_some_and(|s| hex(s.session_id.as_bytes()) == session)
            }).then_some(())
        }).await.is_some()
    }

    /// Whether the hub's stream is open and the order read to its end.
    pub fn is_online(&self) -> bool {
        self.lock().engine.connection() == Connection::Live
    }

    /// Goes online and stays: the engine signs in, takes the lease, holds its chains against the hub's, catches
    /// up and follows the stream, coming back by itself whenever the hub is away. Returns once the first round is
    /// over; a hub that cannot be reached is no error (the outbox waits), a device that is out is, and so is a hub
    /// that is none of this protocol.
    pub async fn start(&self) -> Result<()> {
        self.call(|_, _| ());
        {
            let mut inner = self.lock();
            inner.start_wanted = true;
            if inner.cache_read && !inner.started {
                inner.started = true;
                inner.call(now_ms(), &self.shared, |engine, ctx| engine.start(ctx));
            }
        }
        self.shared.changed.notify_waiters();
        self.poke();
        let outcome = self
            .wait(START_WAIT, |inner| {
                if let Some(why) = inner.stopped {
                    return Some(Err(why));
                }
                let through = inner.engine.caught_up() && inner.engine.agent_signs();
                (through || inner.engine.failing()).then_some(Ok(()))
            })
            .await;
        match outcome {
            Some(Err(Stopped::HubUnusable)) => Err(Fault::new(
                "hub-unusable",
                format!(
                    "the hub does not answer as a Trommi hub of this version: {}",
                    crate::host::update_line()
                ),
            )),
            Some(Err(Stopped::Removed | Stopped::Replaced | Stopped::NotMember)) => {
                let replaced = self.lock().removal.is_some_and(|(r, _)| r);
                Err(Fault::new("not-member", "this device is out of the room").with("replaced", replaced))
            }
            Some(Err(why)) => Err(stop_fault(why)),
            Some(Ok(())) | None => Ok(()),
        }
    }

    /// Stops the work: the lease is given up, nothing is read, written or handed out any more. What waits in the
    /// outbox stays stored.
    pub async fn stop(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        self.call(|engine, ctx| engine.leave(ctx));
        let _ = self.durable().await;
        // Dropping the queue closes streams and the storage worker. Await that closure before another
        // computer operation can reopen the same journal; stop must not leave a detached store lock behind.
        lock(&self.handle).take();
        let task = lock(&self.task).take();
        if let Some(task) = task { let _ = task.await; }
    }

    async fn durable(&self) -> Result<()> {
        match self.wait(WRITE_WAIT, |inner| {
            // A stopped engine can still have a queued device write. Drain it before a verified removal wipes storage.
            inner.device_writes.is_empty().then_some(Ok(()))
        }).await {
            Some(result) => result,
            None => Err(Fault::new("storage", "the device write did not finish")),
        }
    }

    /// Whether this driver may still sign, post and hand out: not stopped by its owner, not out of the room, its
    /// engine at work.
    pub fn active(&self) -> Result<()> {
        let inner = self.lock();
        if inner.removal.is_some() {
            return Err(Fault::new("removed", "this device is out of the room"));
        }
        if self.shared.stopped.load(Ordering::SeqCst) {
            return Err(Fault::new("stopped", "this connector stopped working on its room"));
        }
        match inner.stopped {
            None => Ok(()),
            Some(why) => {
                let fault = stop_fault(why);
                Err(Fault::new(
                    fault.code,
                    "this connector stopped working on its room; the human reconnects this session in the Trommi app",
                ))
            }
        }
    }

    /// Whether this device is out, and how: `(replaced, verified)`.
    pub async fn removal(&self) -> Option<(bool, bool)> {
        self.lock().removal
    }

    /// Hands the device Commits the hub serves a device it no longer takes (`GET /v1/groups/{group}/removal`
    /// items, read by another way): each is verified by the device itself; nothing is taken on whoever's word.
    /// Returns whether one of them removed this device; then the owner is told `Removed { verified: true }`.
    pub async fn verify_removal(&self, items: &[Value]) -> Result<bool> {
        let mut log = Vec::new();
        for item in items {
            if item.get("kind").and_then(Value::as_str) != Some("commit") {
                continue;
            }
            let bytes = |key: &str| {
                item.get(key)
                    .and_then(Value::as_str)
                    .and_then(|text| trommi_core::ids::base64url_decode(text).ok())
            };
            let (Some(change), Some(group), Some(commit)) = (
                item.get("change").and_then(Value::as_u64),
                bytes("group_id").and_then(|b| GroupId::from_bytes(&b).ok()),
                bytes("bytes"),
            ) else {
                continue;
            };
            log.push(LogItem {
                change,
                group,
                n: item.get("n").and_then(Value::as_u64).unwrap_or(0),
                epoch: item.get("epoch").and_then(Value::as_u64).unwrap_or(0),
                at: item.get("at").and_then(Value::as_u64).unwrap_or(0),
                bytes: commit,
                sender: bytes("sender")
                    .and_then(|b| DeviceId::from_slice(&b).ok())
                    .unwrap_or(DeviceId::new([0; 32])),
                recovery_auth: bytes("recovery_auth"),
            });
        }
        Ok(self.call(|engine, ctx| engine.verify_removal(ctx, log)))
    }

    /// Reports with the lease what this process hears and does (13.7): `hears`, `working`, `last_call_at`.
    pub async fn report_link(&self, report: Value) -> Result<()> {
        let report = LinkReport {
            hears: report.get("hears").and_then(Value::as_bool).unwrap_or(false),
            working: report.get("working").and_then(Value::as_bool).unwrap_or(false),
            last_call_at: report.get("last_call_at").and_then(Value::as_u64).unwrap_or(0),
        };
        self.call(|engine, ctx| engine.link_report(ctx, report));
        Ok(())
    }

    /// Records that a command had its effect (it was handed to the agent): it is never handed out again.
    pub async fn ledger_mark(&self, envelope_hash: &str) -> Result<()> {
        self.call(|engine, ctx| engine.command_done(ctx, envelope_hash))?;
        self.durable().await
    }

    /// Waits until nothing of this device waits for the hub, at most `timeout_ms`. `chain-halted` when the
    /// hub refused an envelope for good.
    pub async fn settle(&self, timeout_ms: u64) -> Result<()> {
        // (the network may be back: what waits for a try goes now)
        self.call(|engine, ctx| engine.on_net(ctx, true));
        let outcome = self
            .wait(Duration::from_millis(timeout_ms), |inner| {
                if let Some(code) = inner.blocked() {
                    return Some(Err(Fault::new("chain-halted", code)));
                }
                inner.outbox().is_empty().then_some(Ok(()))
            })
            .await;
        outcome.unwrap_or_else(|| {
            Err(Fault::new(
                "offline",
                "the hub has not confirmed everything yet; it is sent when the hub is back",
            ))
        })
    }

    /// A value of the connector's own, by name.
    pub fn kv_get(&self, name: &str) -> Option<Value> {
        kv_get(&self.journal, name)
    }

    /// Writes such a value.
    pub fn kv_set(&self, name: &str, value: &Value) -> Result<()> {
        kv_set(&self.journal, name, value)
    }

    /// A file this process holds by its whole reference.
    pub(crate) fn file_cached(&self, reference: &str) -> Option<Vec<u8>> {
        self.files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|(id, _)| id == reference)
            .map(|(_, bytes)| bytes.clone())
    }

    /// Keeps a file by its whole reference: the newest 16.
    pub(crate) fn file_keep(&self, reference: String, bytes: Vec<u8>) {
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        files.push((reference, bytes));
        if files.len() > 16 {
            files.remove(0);
        }
    }

    // ---- writing

    /// Seals an agent's write, lets the hub take it, and waits for the model to reach its place. With the hub
    /// away the envelope waits in the outbox and this returns without a change number.
    pub(crate) async fn write(&self, write: AgentWrite, files: &[trommi_core::ids::FileId]) -> Result<SentInfo> {
        self.active()?;
        let sealed = self.call(|engine, ctx| engine.agent_send(ctx, write, files));
        let sealed: Sealed = match sealed {
            Ok(sealed) => sealed,
            Err(trommi_core::Error::Busy) => {
                return Err(Fault::new(
                    "offline",
                    "the Trommi hub has not been reached since this connector started (no network, or the hub is down); it keeps trying by itself: call the tool again in a minute. Nothing was sent",
                ))
            }
            Err(trommi_core::Error::ChainBreak) => {
                let code = self.lock().blocked().unwrap_or("refused");
                return Err(Fault::new(
                    "chain-halted",
                    format!("the hub refused an earlier envelope of this session for good ({code})"),
                ));
            }
            Err(trommi_core::Error::TooMany) => {
                return Err(Fault::new(
                    "offline",
                    "the Trommi hub has been away for too long and too much waits for it (no network, or the hub is down); nothing more is queued until it is back: call the tool again later",
                ))
            }
            Err(e) => return Err(e.into()),
        };
        let mut info = SentInfo {
            hash: sealed.envelope_hash.to_base64url(),
            object_id: sealed.object_id.map(|id| hex(id.as_bytes())),
            envelope_number: None,
        };
        let change = self.taken(sealed.outbox_id, Some(&mut info)).await?;
        info.envelope_number = change;
        Ok(info)
    }

    /// Waits for the hub's word on an outbox entry of this device: its change once taken and the model reached
    /// it; none when the hub is away (it waits in the outbox); the hub's refusal as a fault. `info` follows an
    /// envelope sealed again in the place of one the hub voided.
    async fn taken(&self, id: u64, mut info: Option<&mut SentInfo>) -> Result<Option<u64>> {
        let began = tokio::time::Instant::now();
        let mut id = id;
        let outcome = self
            .wait(WRITE_WAIT, |inner| {
                while let Some((again, hash, object)) = inner.resealed.get(&id).copied() {
                    id = again;
                    if let Some(info) = info.as_deref_mut() {
                        info.hash = hash.to_base64url();
                        info.object_id = object.map(|o| hex(o.as_bytes()));
                    }
                }
                if let Some(change) = inner.accepted.get(&id) {
                    return Some(Ok(*change));
                }
                if let Some(code) = inner.refused.get(&id) {
                    return Some(Err(Fault::new(*code, format!("the hub refused it ({code})"))));
                }
                if let Some((blocked, code)) = inner.blocked {
                    if blocked == id {
                        return Some(Err(Fault::new(code, format!("the hub refused it ({code}); it stays in the outbox, and nothing more is sent"))));
                    }
                }
                if let Some(why) = inner.stopped {
                    return Some(Err(stop_fault(why)));
                }
                if began.elapsed() > Duration::from_millis(300) && inner.engine.failing() {
                    return Some(Ok(None));
                }
                None
            })
            .await;
        let change = match outcome {
            Some(Ok(change)) => change,
            Some(Err(fault)) => return Err(fault),
            None => None,
        };
        if let Some(change) = change {
            // the model reaches its place: what was written is on the board when the caller looks
            self.wait(PLACE_WAIT, |inner| {
                (inner.engine.position() >= change || inner.stopped.is_some()).then_some(())
            })
            .await;
        }
        Ok(change)
    }

    /// A step of the device that puts a request into its outbox (a founding, a Commit, a message): done with the
    /// engine's device, then waited for like a write. The outbox id of the last entry it made is waited for.
    pub(crate) async fn device_step<T: Send>(
        &self,
        step: impl FnOnce(&mut crate::driver::EngineDevice, u64) -> std::result::Result<T, trommi_core::Error>,
    ) -> Result<(T, Option<u64>)> {
        self.active()?;
        let done = self.call(|engine, ctx| {
            if !engine.agent_signs() {
                return Err(trommi_core::Error::Busy);
            }
            engine.act(ctx, |device, now| {
                let before = device.outbox().last().map_or(0, |e| e.id);
                let value = step(device, now)?;
                let last = device.outbox().last().map(|e| e.id).filter(|id| *id > before);
                Ok((value, last))
            })
        });
        let (value, last) = match done {
            Ok(done) => done,
            Err(trommi_core::Error::Busy) => {
                return Err(Fault::new(
                    "offline",
                    "the Trommi hub has not been reached since this connector started (no network, or the hub is down); it keeps trying by itself: call the tool again in a minute. Nothing was sent",
                ))
            }
            Err(e) => return Err(e.into()),
        };
        let change = match last {
            Some(id) => self.taken(id, None).await?,
            None => None,
        };
        Ok((value, change))
    }

    /// A step of the device that writes nothing to the hub yet (a work-trail step waits in the outbox); `false`
    /// from `step` drops it.
    pub(crate) fn device_queue(
        &self,
        step: impl FnOnce(&mut crate::driver::EngineDevice, u64) -> std::result::Result<(), trommi_core::Error>,
    ) -> Result<()> {
        self.active()?;
        let done = self.call(|engine, ctx| {
            if !engine.agent_signs() {
                return Err(trommi_core::Error::Busy);
            }
            engine.act(ctx, step)
        });
        match done {
            Ok(()) => Ok(()),
            Err(trommi_core::Error::Busy) => Err(Fault::new(
                "offline",
                "the Trommi hub has not been reached since this connector started; nothing was sent",
            )),
            Err(e) => Err(e.into()),
        }
    }

    /// Asks the hub through the engine (which holds the token and the lease): a route built by the core's codec,
    /// its answer read strictly. A request not answered in time is given up (`offline`).
    pub(crate) async fn ask(&self, route: Route<'_>) -> Result<Answer> {
        self.active()?;
        let mut expect: Option<Expect> = None;
        let asked = self.call(|engine, ctx| {
            engine.request(
                ctx,
                |site: &Site<'_>| {
                    let (request, how): (HttpRequest, Expect) = Codec.request(site, &route)?;
                    expect = Some(how);
                    Ok(request)
                },
                None,
            )
        });
        let id = asked.map_err(|code| Fault::new(code, format!("the request was not sent ({code})")))?;
        let Some(expect) = expect else {
            return Err(Fault::new("internal", "a request without its expectation"));
        };
        let answered = self
            .wait(REQUEST_WAIT, |inner| {
                if let Some(answer) = inner.answers.remove(&id) {
                    return Some(Ok(answer));
                }
                inner.stopped.map(|why| Err(stop_fault(why)))
            })
            .await;
        let response = match answered {
            Some(Ok(Ok(response))) => response,
            Some(Ok(Err(failure))) => return Err(fault_of(&failure)),
            Some(Err(fault)) => return Err(fault),
            None => {
                self.call(|engine, ctx| {
                    if let Some(flying) = engine.forget_request(id) {
                        ctx.out.push(Effect::Cancel { id: flying });
                    }
                });
                return Err(Fault::new("offline", "the hub did not answer in time"));
            }
        };
        Codec.answer(&expect, &response).map_err(|failure| fault_of(&failure))
    }

    /// Archives a session group at the hub and on the device (5.2.10), retiring nobody; waits for the hub.
    pub(crate) async fn archive(&self, group: GroupId) -> Result<()> {
        self.active()?;
        let id = self
            .call(|engine, ctx| engine.archive_session(ctx, group, Vec::new()))
            .map_err(|code| Fault::new(code, format!("not archived ({code})")))?;
        let answered = self
            .wait(REQUEST_WAIT, |inner| {
                if let Some(answer) = inner.answers.remove(&id) {
                    return Some(Ok(answer));
                }
                inner.stopped.map(|why| Err(stop_fault(why)))
            })
            .await;
        match answered {
            Some(Ok(Ok(response))) => match trommi_client_core::hub::classify(&response) {
                Ok(()) => Ok(()),
                Err(Failure::Refused { code: Some(code), .. }) if code == "gone" => Ok(()),
                Err(failure) => Err(fault_of(&failure)),
            },
            Some(Ok(Err(failure))) => Err(fault_of(&failure)),
            Some(Err(fault)) => Err(fault),
            // (the archive is kept across a restart: it goes once the hub is back)
            None => Err(Fault::new("offline", "the hub did not answer in time; the archive goes once it is back")),
        }
    }
}

/// The device inside the engine, as a step of [`Driver::device_step`] sees it.
pub type EngineDevice = trommi_client_core::engine::Device;

/// The hub without the engine, for a device that is not in its room yet (the invite's three messages, the first
/// look for a Welcome): the core's codec over `trommi-io`'s transport, with the token of a sign-in it does itself.
pub struct OpenHub {
    http: trommi_io::Http,
    url: String,
}

impl OpenHub {
    /// The hub at a canonical address.
    pub fn new(address: &str) -> Result<OpenHub> {
        trommi_core::hub_auth::HubAddress::parse(address)
            .map_err(|_| Fault::new("bad-format", "the hub's address is not canonical"))?;
        Ok(OpenHub {
            http: trommi_io::Http::new().map_err(|e| Fault::new("internal", e))?,
            url: address.trim_end_matches('/').to_string(),
        })
    }

    /// One request: built and read by the codec; a failure as a fault.
    pub async fn call(&self, token: Option<&str>, route: &Route<'_>) -> Result<Answer> {
        let site = Site {
            url: &self.url,
            token,
            client: crate::CLIENT,
            lease: None,
        };
        let (request, expect) = Codec.request(&site, route).map_err(|f| fault_of(&f))?;
        let response = match self.http.fetch(&request).await {
            Ok(response) => response,
            Err(error) => {
                return Err(match trommi_client_core::hub::classify_io(&error) {
                    Some(failure) => fault_of(&failure),
                    None => Fault::new("offline", "the hub could not be reached"),
                })
            }
        };
        Codec.answer(&expect, &response).map_err(|f| fault_of(&f))
    }

    /// Signs in to `room` with `sign`, the device's answer to the hub's challenge (`Device::hub_sign_in`); the
    /// token.
    pub async fn sign_in(
        &self,
        room: &RoomId,
        sign: impl FnOnce([u8; 32]) -> BoxFut<'static, Result<trommi_core::hub_auth::SignedHubAuth>>,
    ) -> Result<String> {
        let challenge = match self.call(None, &Route::Challenge { room }).await? {
            Answer::Challenge(challenge) => challenge,
            _ => return Err(Fault::new("bad-format", "the hub's challenge does not read")),
        };
        let signed = sign(challenge).await?;
        match self
            .call(None, &Route::Token { room, auth: &signed.auth, signature: &signed.signature })
            .await?
        {
            Answer::Token { token, .. } => Ok(token),
            _ => Err(Fault::new("bad-format", "the hub's answer has no token")),
        }
    }
}
