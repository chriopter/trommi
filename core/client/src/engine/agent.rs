//! An agent device in the engine ([`Engine::become_agent`]): what the connector's own client did beside the sync
//! that every device does.
//!
//! - **The lease** (13.7). Nothing is read or written before this process holds the device's lease: a new random
//!   process id acquires it (`POST /v1/link`, no generation), the same process renews it every
//!   [`LEASE_EVERY_MS`] with its generation, and every write and the stream carry the generation
//!   (`Trommi-Lease`, [`crate::hub::Site::lease`]). A refusal `lease-lost` of any write is checked by renewing at
//!   once; a renewal refused `lease-lost` means another process took the device over: the engine stops
//!   ([`Stopped::LeaseLost`]) and signs nothing more. With the lease goes what the process reports (`hears`,
//!   `working`, `last_call_at`: [`Engine::link_report`]), and its last word gives it up ([`Engine::leave`]).
//! - **The rollback guard.** Each time a lease is acquired, and before anything else is read or written, the hub's
//!   copy of this device's own chain in each live session group is held against the last envelope this state
//!   signed there ([`trommi_core::device::Device::own_head`]); and so is every envelope of this device the order
//!   brings. Only an envelope this device signed itself, in that group, under a number beyond its own head is
//!   evidence (the hub's word alone halts nothing): the stored state is older than what the device already sent
//!   (a restored copy, a write the disk lost), and signing on would put a second envelope under a used number.
//!   The engine stops ([`Stopped::RolledBack`]).
//! - **The command gate** (9.0.9). An envelope in the hub's order that the protocol core marks as a possible
//!   command ([`ReceivedEnvelope::command`]) is put to the gate in the same step, so the gate's record is in the
//!   same write as the envelope. What it lets through is reported as an [`AgentCommand`] (after the envelope's
//!   own report) and kept in the engine's record until the caller says it had its effect
//!   ([`Engine::command_done`]). After a restart every kept command is reported again, marked `uncertain`: the
//!   agent may or may not have had it (reported, not repeated); so is one the gate started and the record never
//!   kept (a crash between the two writes), with its hash only.
//! - **Leaving the room.** A Commit in the order that ends this device's place is followed by the device itself:
//!   the room's newest state no longer names it an agent ([`Stopped::Removed`]), or a Commit removed it from its
//!   main session (another connector took the session over: [`Stopped::Replaced`]). When the hub refuses the device
//!   as no member (`not-member`, `removed-sender`, `no-room`, a token for a `removed` key), its word alone proves
//!   nothing: the engine reads what the hub still serves such a key (`GET /v1/groups/{group}/removal`, for the room
//!   group and each session group it holds), hands those Commits to the device in the hub's order and stops as the
//!   device found ([`Stopped::Removed`], [`Stopped::Replaced`]), or, when none of them removed it,
//!   [`Stopped::NotMember`]: out by the hub's word, the state kept.
//! - **The invite.** A room Commit that enrols this device by another than its inviter (12.1.6) stops it
//!   ([`Stopped::BadInvite`]).
//!
//! The Desk is not read first (an agent device reads its own session groups in the hub's order) and the human
//! device's upkeep is not done (it is `is_human`-bound already).

use super::{Asked, Ctx, Engine, Report, Retry, Stopped, Taken};
use crate::app::LogLevel;
use crate::engine::feed::hex;
use crate::hub::{Answer, ChainItem, Failure, LinkReport, LogItem, Route};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::VecDeque;
use trommi_core::device::{log_finding, LogEntry, LogFinding, LogKind, Processed, ReceivedEnvelope};
use trommi_core::envelope::{Envelope, ObjectState, Subject, Timeline, Verdict};
use trommi_core::ids::{DeviceId, GroupId, Hash32};
use trommi_core::objects::{AnswerAction, Command, Decision, Refusal};
use trommi_core::Error;

/// The lease lives 60 s at the hub and is renewed every 20 s (13.7).
pub const LEASE_EVERY_MS: u64 = 20_000;
/// The most commands kept until they are done; beyond it the oldest is reported uncertain at a restart no more.
pub const COMMANDS_MAX: usize = 1000;
/// The most pages of a group's removal that are read (the hub serves at most 200 Commits a page).
const REMOVAL_PAGES_MAX: u32 = 500;
/// The text of a command whose effect is uncertain and whose content is not known any more.
pub const UNCERTAIN: &str = "a command whose effect is uncertain after a restart; ask the human what they meant";

/// A human's command for the agent, after the gate (9.0.9). Ids as the agent sees them: a session or object as
/// hex, a device and an envelope hash as base64url, an envelope's place as the hub's change number.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentCommand {
    /// Let through before this process started and never said to be done: the agent may or may not have had it.
    pub uncertain: bool,
    /// `message`, `answer`, `trust`, `read`, `shred`, `decide_again`, `verdict`, `unsupported`.
    pub command: String,
    pub session_id: Option<String>,
    pub envelope_number: u64,
    /// The envelope's hash, base64url: what [`Engine::command_done`] takes.
    pub envelope_hash: String,
    pub sender_device_id: String,
    pub sender_sequence: u64,
    pub sent_at: u64,
    pub object_id: Option<String>,
    /// `chat:session/<id>` or `chat:card/<id>` for a message.
    pub timeline_key: Option<String>,
    /// The envelope's payload as it was sealed (a JSON object); empty when it is not known.
    pub payload: String,
    pub choices: Vec<String>,
    /// For `decide_again`: the choices of the answer taken back. The engine does not keep answers; the caller's
    /// model fills them in.
    pub previous_choices: Vec<String>,
    pub allow: bool,
    /// The answer closed the card by itself (a final option).
    pub settled: bool,
    pub what: Option<String>,
}

impl AgentCommand {
    /// The payload's fields; empty when it is not known or no object.
    pub fn content(&self) -> Map<String, Value> {
        serde_json::from_str::<Value>(&self.payload).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default()
    }

    /// What a gate's decision on a received envelope hands the agent; none for what is not acted on.
    pub fn of(received: &ReceivedEnvelope, decision: &Decision) -> Option<AgentCommand> {
        let header = &received.header;
        let session = header.group.session_id().map(|s| hex(s.as_bytes()));
        let payload = received.body.as_ref().map(|b| String::from_utf8_lossy(b.payload()).into_owned()).unwrap_or_default();
        let mut command = AgentCommand {
            session_id: session.clone(),
            envelope_number: received.change,
            envelope_hash: received.envelope_hash.to_base64url(),
            sender_device_id: header.sender.to_base64url(),
            sender_sequence: header.seq,
            sent_at: header.time,
            object_id: header.subject.object().map(|f| hex(f.object_id.as_bytes())),
            payload,
            ..AgentCommand::default()
        };
        match decision {
            Decision::Done | Decision::Refused(_) => return None,
            Decision::Uncertain => {
                // started before a crash and never finished: reported, not repeated
                command.command = "unsupported".into();
                command.uncertain = true;
                command.what = Some(UNCERTAIN.into());
            }
            Decision::Act(Command::Chat) => {
                command.command = "message".into();
                command.timeline_key = Some(match header.subject {
                    Subject::Item(Timeline::CardChat(card)) => {
                        let id = hex(card.as_bytes());
                        command.object_id = Some(id.clone());
                        format!("chat:card/{id}")
                    }
                    _ => format!("chat:session/{}", session.unwrap_or_default()),
                });
            }
            Decision::Act(Command::Answer { action, choices }) => {
                let closed = received.object_after.as_ref().is_some_and(|(_, o)| o.state == ObjectState::Closed);
                command.settled = *action == AnswerAction::Answer && closed;
                command.command = match action {
                    AnswerAction::Read => "read",
                    AnswerAction::Shred => "shred",
                    AnswerAction::Answer if command.content().get("trusted").is_some_and(truthy) => "trust",
                    AnswerAction::Answer => "answer",
                }
                .into();
                command.choices = choices.clone();
            }
            Decision::Act(Command::Verdict(verdict)) => {
                command.command = "verdict".into();
                command.allow = *verdict == Verdict::Allow;
            }
            Decision::Act(Command::TakeBack) => command.command = "decide_again".into(),
        }
        Some(command)
    }
}

/// JavaScript's truthiness of a JSON value, as the board writes flags.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

/// The rollback guard of one acquisition of the lease.
#[derive(Default)]
enum Guard {
    /// To be done before anything else.
    #[default]
    Due,
    /// The groups still to ask about, with the number of this device's own head in each.
    Asking { todo: VecDeque<(GroupId, u64)>, asked: Option<super::IoId> },
    Passed,
}

/// What the hub still serves this device as a removed key, being read.
struct RemovalRead {
    groups: VecDeque<GroupId>,
    after: u64,
    pages: u32,
    items: Vec<LogItem>,
    asked: Option<super::IoId>,
}

/// The agent's part of the engine's state.
pub(super) struct Agent {
    process: [u8; 16],
    generation: Option<u64>,
    renew_at: u64,
    /// Renew now: the report changed, or a write was refused `lease-lost` (the renewal tells whether it is).
    renew_now: bool,
    asked: Option<super::IoId>,
    retry: Retry,
    report: LinkReport,
    leaving: bool,
    left: bool,
    guard: Guard,
    guard_retry: Retry,
    /// Commands the gate let through and nobody said were done, oldest first (kept in the record).
    pub(super) commands: Vec<AgentCommand>,
    /// The kept commands were reported again after the start.
    told: bool,
    removal: Option<RemovalRead>,
}

impl Agent {
    fn new(process: [u8; 16]) -> Self {
        Agent {
            process,
            generation: None,
            renew_at: 0,
            renew_now: false,
            asked: None,
            retry: Retry::default(),
            report: LinkReport::default(),
            leaving: false,
            left: false,
            guard: Guard::Due,
            guard_retry: Retry::default(),
            commands: Vec::new(),
            told: false,
            removal: None,
        }
    }
}

/// How this device stands in the room's newest state: an agent the room names.
fn enrolled(device: &super::Device<super::memstore::MemStore>) -> bool {
    let me = device.id();
    device.room_history().is_some_and(|h| h.newest().is_agent(&me))
}

impl Engine {
    // ---- what the owner calls

    /// Makes this engine an agent device's: before [`Engine::start`]. Refused for a human device.
    pub fn become_agent(&mut self) -> Result<(), Error> {
        if self.device.is_human() {
            return Err(Error::Forbidden);
        }
        let mut process = [0u8; 16];
        self.with_entropy(|entropy| entropy.fill(&mut process))?;
        self.agent = Some(Agent::new(process));
        // (an agent reads its own session groups in the hub's order: no Desk first)
        self.desk.want = false;
        Ok(())
    }

    /// Whether this engine is an agent device's.
    pub fn is_agent(&self) -> bool {
        self.agent.is_some()
    }

    /// The lease this process holds: its generation.
    pub fn lease(&self) -> Option<u64> {
        self.agent.as_ref().and_then(|a| a.generation)
    }

    /// What the process tells the hub with its lease; sent at once.
    pub fn link_report(&mut self, ctx: &mut Ctx<'_>, report: LinkReport) {
        if let Some(agent) = self.agent.as_mut().filter(|a| !a.leaving) {
            agent.renew_now |= agent.report != report;
            agent.report = report;
            agent.retry.hurry();
        }
        self.drive(ctx);
    }

    /// The process ends: its last renewal gives the lease up (`leaving`), so the human devices see the device
    /// go at once. Nothing more is read or written.
    pub fn leave(&mut self, ctx: &mut Ctx<'_>) {
        if let Some(agent) = self.agent.as_mut().filter(|a| a.generation.is_some() && !a.leaving) {
            agent.leaving = true;
            agent.renew_now = true;
            agent.report.hears = false;
            agent.report.working = false;
            agent.retry.hurry();
        }
        self.close_stream(ctx);
        self.drive(ctx);
    }

    /// The command with this envelope hash had its effect (it was handed to the agent): it is never handed out
    /// again. `not-found` for one the gate never let through.
    pub fn command_done(&mut self, ctx: &mut Ctx<'_>, hash: &str) -> Result<(), Error> {
        let parsed = Hash32::from_base64url(hash)?;
        let result = self.act(ctx, |device, _| device.command_finished(&parsed));
        if let Some(agent) = self.agent.as_mut() {
            agent.commands.retain(|c| c.envelope_hash != hash);
        }
        // (the record without it is written behind the device's write)
        self.drive(ctx);
        result
    }

    /// The commands let through that nobody said were done.
    pub fn commands_kept(&self) -> &[AgentCommand] {
        self.agent.as_ref().map_or(&[], |a| a.commands.as_slice())
    }

    /// This agent device's main session group: the live session group without a parent where the room names it
    /// the agent.
    pub fn main_group(&self) -> Option<GroupId> {
        self.device.groups().ok()?.into_iter().map(|g| g.group).find(|group| self.is_main(group))
    }

    /// Whether `group` is this agent device's main session group.
    fn is_main(&self, group: &GroupId) -> bool {
        let (Some(history), Ok(summary)) = (self.device.room_history(), self.device.group(group)) else { return false };
        let room = history.newest();
        let me = self.device.id();
        let seat = summary.leaves.iter().find(|leaf| room.is_agent(leaf));
        summary.session.is_some_and(|s| s.parent.is_zero()) && !summary.archived && seat == Some(&me)
    }

    // ---- inside the engine

    /// The lease generation the requests carry.
    pub(super) fn lease_header(&self) -> Option<u64> {
        self.agent.as_ref().and_then(|a| a.generation)
    }

    pub(super) fn agent_wake(&self) -> Option<u64> {
        let agent = self.agent.as_ref()?;
        if agent.left {
            return None;
        }
        let mut at = [agent.retry.at, agent.guard_retry.at].into_iter().flatten().min();
        if agent.generation.is_some() && agent.asked.is_none() {
            at = Some(at.map_or(agent.renew_at, |a| a.min(agent.renew_at)));
        }
        at
    }

    /// The agent's turn of the drive, once signed in: the lease, the rollback guard, a removal being read. False:
    /// nothing else may be read or written now.
    pub(super) fn drive_agent(&mut self, ctx: &mut Ctx<'_>) -> bool {
        let Some(agent) = self.agent.as_mut() else { return true };
        if agent.left {
            return false;
        }
        if agent.removal.is_some() {
            self.drive_removal(ctx);
            return false;
        }
        let agent = self.agent.as_mut().expect("an agent");
        let due = agent.generation.is_none() || agent.renew_now || ctx.now >= agent.renew_at;
        if due && agent.asked.is_none() && !agent.retry.waiting(ctx.now) {
            let (process, generation, report, leaving) = (agent.process, agent.generation, agent.report, agent.leaving);
            agent.renew_now = false;
            let route = Route::Link { process: &process, generation, report: &report, leaving };
            let asked = self.ask(ctx, route, true, Asked::Lease);
            self.agent.as_mut().expect("an agent").asked = asked;
        }
        let agent = self.agent.as_ref().expect("an agent");
        if agent.generation.is_none() || agent.leaving {
            return false;
        }
        self.drive_guard(ctx)
    }

    /// Tells the kept commands again after a start, as uncertain, and what the gate started that the record did
    /// not keep. Once, when the record was read.
    pub(super) fn agent_restart(&mut self) {
        let Some(agent) = self.agent.as_mut().filter(|a| !a.told) else { return };
        agent.told = true;
        let mut told: Vec<AgentCommand> = agent.commands.iter().cloned().map(|c| AgentCommand { uncertain: true, ..c }).collect();
        let started = self.device.commands_uncertain().unwrap_or_default();
        for hash in started {
            let text = hash.to_base64url();
            if !told.iter().any(|c| c.envelope_hash == text) {
                told.push(AgentCommand { uncertain: true, command: "unsupported".into(), envelope_hash: text, what: Some(UNCERTAIN.into()), ..AgentCommand::default() });
            }
        }
        for command in told {
            self.report(Report::Command(Box::new(command)));
        }
    }

    pub(super) fn on_lease(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>, refusal: Option<&str>) {
        let Some(agent) = self.agent.as_mut() else { return };
        agent.asked = None;
        match answer {
            Ok(Answer::Linked { generation, .. }) => {
                agent.retry.ok();
                if agent.leaving {
                    agent.left = true;
                    agent.generation = None;
                    self.report(Report::Lease(None));
                    return;
                }
                let acquired = agent.generation.is_none();
                agent.generation = Some(generation);
                agent.renew_at = ctx.now + LEASE_EVERY_MS;
                if acquired {
                    // a new holder: its own chains are held against the hub's before anything is written
                    agent.guard = Guard::Due;
                    self.report(Report::Lease(Some(generation)));
                }
            }
            Ok(_) => agent.retry.fail(ctx.now, None, &mut self.jitter),
            Err(_) if refusal == Some("lease-lost") => {
                ctx.log(LogLevel::Warn, "lease: another process took this device's lease");
                self.stop(ctx, Stopped::LeaseLost);
            }
            Err(failure) => {
                let retry_after = match failure {
                    Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
                    _ => None,
                };
                agent.retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        }
    }

    /// A refusal of any request: `lease-lost` is checked by renewing; a key the hub takes for no member reads
    /// what the hub still serves it.
    pub(super) fn agent_refused(&mut self, ctx: &mut Ctx<'_>, code: &str) {
        let Some(agent) = self.agent.as_mut() else { return };
        match code {
            "lease-lost" => {
                agent.renew_now = true;
                agent.retry.hurry();
            }
            "not-member" | "removed-sender" | "no-room" => self.agent_out(ctx),
            _ => {}
        }
    }

    /// The hub takes this device for one it removed: what it still serves of the removal is read and followed.
    pub(super) fn agent_out(&mut self, ctx: &mut Ctx<'_>) {
        let groups: VecDeque<GroupId> = std::iter::once(GroupId::room(self.room))
            .chain(self.device.groups().unwrap_or_default().into_iter().filter(|g| !g.group.is_room() && !g.archived).map(|g| g.group))
            .collect();
        let Some(agent) = self.agent.as_mut().filter(|a| a.removal.is_none()) else { return };
        ctx.log(LogLevel::Warn, "the hub takes this device for a removed one: its Commits are read");
        agent.removal = Some(RemovalRead { groups, after: 0, pages: 0, items: Vec::new(), asked: None });
        self.close_stream(ctx);
    }

    fn drive_removal(&mut self, ctx: &mut Ctx<'_>) {
        let Some(read) = self.agent.as_mut().and_then(|a| a.removal.as_mut()) else { return };
        if read.asked.is_some() {
            return;
        }
        let Some(group) = read.groups.front().copied() else {
            return self.follow_removal(ctx);
        };
        let after = read.after;
        let asked = self.ask(ctx, Route::Removal { group: &group, after }, true, |expect| Asked::Removal { expect, group });
        match (asked, self.agent.as_mut().and_then(|a| a.removal.as_mut())) {
            (Some(id), Some(read)) => read.asked = Some(id),
            (None, _) => self.stop(ctx, Stopped::NotMember),
            _ => {}
        }
    }

    pub(super) fn on_removal(&mut self, ctx: &mut Ctx<'_>, group: GroupId, answer: Result<Answer, Failure>, refusal: Option<&str>) {
        let Some(read) = self.agent.as_mut().and_then(|a| a.removal.as_mut()) else { return };
        read.asked = None;
        let next = |read: &mut RemovalRead| {
            read.groups.pop_front();
            read.after = 0;
            read.pages = 0;
        };
        match answer {
            Ok(Answer::Removal { items, more, .. }) => {
                let last = items.last().map(|i| i.n);
                read.items.extend(items.into_iter().filter(|i| i.group == group));
                read.pages += 1;
                match last {
                    Some(n) if more && n > read.after && read.pages < REMOVAL_PAGES_MAX => read.after = n,
                    _ => next(read),
                }
            }
            // this device was not removed from that group, or the hub keeps nothing of it
            Err(Failure::Refused { .. }) if matches!(refusal, Some("not-found" | "not-member" | "gone")) => next(read),
            _ => {
                // what the hub serves cannot be read: out by its word alone, the state kept
                ctx.log(LogLevel::Warn, "the removal could not be read");
                self.stop(ctx, Stopped::NotMember);
            }
        }
    }

    /// Hands the Commits the hub served after it stopped taking this device to the device, in the hub's order:
    /// each is verified as any other, nothing is taken on the hub's word.
    fn follow_removal(&mut self, ctx: &mut Ctx<'_>) {
        let Some(read) = self.agent.as_mut().and_then(|a| a.removal.take()) else { return };
        let mut items = read.items;
        items.sort_by_key(|i| i.change);
        items.dedup_by_key(|i| i.change);
        for item in items {
            if item.change <= self.device.cursor() {
                continue;
            }
            let before = self.agent_before(&item.group);
            let entry = LogEntry { change: item.change, group: item.group, kind: LogKind::Commit { bytes: &item.bytes, recovery_auth: item.recovery_auth.as_deref() } };
            match self.device.process_log_entry(&entry, ctx.now) {
                Ok(processed) => {
                    if self.agent_processed(ctx, item.group, &processed, before) {
                        return;
                    }
                }
                Err(e) if log_finding(&e) == LogFinding::Local => {
                    self.local(ctx, &e);
                    return;
                }
                Err(_) => break,
            }
        }
        self.flush(ctx);
        self.stop(ctx, Stopped::NotMember);
    }

    fn drive_guard(&mut self, ctx: &mut Ctx<'_>) -> bool {
        let me = self.device.id();
        let agent = self.agent.as_mut().expect("an agent");
        match &agent.guard {
            Guard::Passed => return true,
            Guard::Asking { asked: Some(_), .. } => return false,
            _ if agent.guard_retry.waiting(ctx.now) => return false,
            Guard::Due => {
                let groups = self.device.groups().unwrap_or_default();
                let todo = groups
                    .into_iter()
                    .filter(|g| !g.group.is_room() && !g.archived)
                    .filter_map(|g| Some((g.group, self.device.own_head(&g.group).ok()?.seq)))
                    .collect();
                self.agent.as_mut().expect("an agent").guard = Guard::Asking { todo, asked: None };
            }
            Guard::Asking { .. } => {}
        }
        let Some(Agent { guard: Guard::Asking { todo, .. }, .. }) = self.agent.as_mut() else { return false };
        let Some((group, own)) = todo.front().copied() else {
            self.agent.as_mut().expect("an agent").guard = Guard::Passed;
            return true;
        };
        let route = Route::Chain { group: &group, sender: &me, after: own, limit: 1 };
        let id = self.ask(ctx, route, true, |expect| Asked::Guard { expect, group, own });
        if let Some(Agent { guard: Guard::Asking { asked, .. }, .. }) = self.agent.as_mut() {
            *asked = id;
        }
        false
    }

    pub(super) fn on_guard(&mut self, ctx: &mut Ctx<'_>, group: GroupId, own: u64, answer: Result<Answer, Failure>, refusal: Option<&str>) {
        let me = self.device.id();
        let Some(agent) = self.agent.as_mut() else { return };
        let Guard::Asking { todo, asked } = &mut agent.guard else { return };
        *asked = None;
        let ahead = match answer {
            Ok(Answer::Chain { items, .. }) => items.iter().any(|ChainItem { item, .. }| is_ahead(&item.envelope, &me, &group, own)),
            Err(Failure::Refused { .. }) if refusal == Some("not-found") => false,
            Ok(_) | Err(_) => {
                agent.guard_retry.fail(ctx.now, None, &mut self.jitter);
                return;
            }
        };
        agent.guard_retry.ok();
        if ahead {
            return self.rolled_back(ctx);
        }
        todo.pop_front();
    }

    fn rolled_back(&mut self, ctx: &mut Ctx<'_>) {
        ctx.log(LogLevel::Error, "the stored state is older than what this device already sent: nothing is signed with it");
        self.stop(ctx, Stopped::RolledBack);
    }

    /// Before an envelope of the order is handed to the device: one of this device beyond its own head halts.
    pub(super) fn agent_guard_envelope(&mut self, ctx: &mut Ctx<'_>, bytes: &[u8]) -> Option<Taken> {
        self.agent.as_ref()?;
        let envelope = Envelope::decode(bytes).ok()?;
        let me = self.device.id();
        let group = envelope.header.group;
        let own = self.device.own_head(&group).ok()?.seq;
        if envelope.header.sender == me && envelope.header.seq > own && envelope.verify().is_ok() {
            self.rolled_back(ctx);
            return Some(Taken::Halt);
        }
        None
    }

    /// The gate on an envelope the device took in the hub's order, in the same step.
    pub(super) fn agent_gate(&mut self, ctx: &mut Ctx<'_>, received: &ReceivedEnvelope) -> Option<AgentCommand> {
        if self.agent.is_none() || !received.command {
            return None;
        }
        let decision = match self.device.command(&received.envelope_hash, ctx.now) {
            Ok(decision) => decision,
            Err(e) => {
                self.local(ctx, &e);
                ctx.log(LogLevel::Warn, format!("command gate: {}", e.code()));
                return None;
            }
        };
        if let Decision::Refused(refusal) = &decision {
            if !matches!(refusal, Refusal::NotAddressed | Refusal::NotACommand) {
                // (said without content: which rule held it back)
                ctx.log(LogLevel::Info, format!("a human's envelope was not acted on: {refusal:?}"));
            }
        }
        let command = AgentCommand::of(received, &decision)?;
        let agent = self.agent.as_mut().expect("an agent");
        if matches!(decision, Decision::Act(_)) {
            if agent.commands.len() >= COMMANDS_MAX {
                agent.commands.remove(0);
            }
            agent.commands.push(command.clone());
        }
        Some(command)
    }

    /// What a log entry of `group` is held against: whether the room names this device an agent, and whether
    /// `group` is its main session.
    pub(super) fn agent_before(&self, group: &GroupId) -> Option<(bool, bool)> {
        self.agent.as_ref()?;
        Some((enrolled(&self.device), !group.is_room() && self.is_main(group)))
    }

    /// After a log entry the device processed: whether it ended this device's place (the engine stopped).
    pub(super) fn agent_processed(&mut self, ctx: &mut Ctx<'_>, group: GroupId, processed: &Processed, before: Option<(bool, bool)>) -> bool {
        let Some((was_enrolled, was_main)) = before else { return false };
        let why = match processed {
            Processed::Commit { removed: true, .. } if was_main => Some(Stopped::Replaced),
            Processed::Observed(_) if group.is_room() && was_enrolled && !enrolled(&self.device) => Some(Stopped::Removed),
            _ => None,
        };
        match why {
            Some(why) => {
                // (what the device wrote with the Commit is stored before the engine stops)
                self.flush(ctx);
                self.stop(ctx, why);
                true
            }
            None => false,
        }
    }

    /// A log entry the device refused: an enrolment by another than the inviter stops an agent device.
    pub(super) fn agent_log_refused(&mut self, ctx: &mut Ctx<'_>, error: &Error) -> bool {
        if self.agent.is_none() || *error != Error::BadInvite {
            return false;
        }
        ctx.log(LogLevel::Error, "this device was enrolled by another device than the one that invited it");
        self.stop(ctx, Stopped::BadInvite);
        true
    }
}

/// Whether `bytes` are an envelope this device signed in `group` under a number beyond `own`.
fn is_ahead(bytes: &[u8], me: &DeviceId, group: &GroupId, own: u64) -> bool {
    Envelope::decode(bytes).is_ok_and(|e| e.header.sender == *me && e.header.group == *group && e.header.seq > own && e.verify().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_is_javascripts() {
        for (v, t) in [(Value::Null, false), (Value::Bool(true), true), (serde_json::json!(0), false), (serde_json::json!(""), false), (serde_json::json!("x"), true), (serde_json::json!({}), true)] {
            assert_eq!(truthy(&v), t, "{v}");
        }
    }

    #[test]
    fn a_kept_command_reads_back() {
        let command = AgentCommand { command: "answer".into(), payload: r#"{"trusted":true,"note":"x"}"#.into(), choices: vec!["yes".into()], ..AgentCommand::default() };
        let text = serde_json::to_string(&command).expect("JSON");
        assert_eq!(serde_json::from_str::<AgentCommand>(&text).expect("it reads"), command);
        assert_eq!(command.content().get("note"), Some(&Value::String("x".into())));
        assert!(AgentCommand::default().content().is_empty());
    }
}
