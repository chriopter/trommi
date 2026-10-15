//! What the engine keeps across a restart: one small record in the app's store.
//!
//! The device's state is the protocol core's and is stored by its own writes. What the ENGINE knows beyond it would
//! be lost with the process: where it stands in the hub's order when that is not the device's own cursor, and what
//! it passed over to read later (envelopes for a group it did not hold that far, envelopes chained without a key,
//! chains the hub withheld, the mends queued for them, a sign-in's session groups still to join). A restart that
//! forgot those would never read them.
//!
//! # What makes it safe
//!
//! - **The record never runs ahead of the device.** It is written at the end of a step, after the device's write
//!   of that step, and that write is a barrier in the shell: if it fails, nothing behind it is done. So a record
//!   that says "everything up to here was handed to the device" is only ever stored once the device's state that
//!   holds it is.
//! - **A record that lags is harmless.** The process may die between the device's write and the record's. The
//!   restart then reads the order again from the older position: what the device took already is a replay there
//!   and is passed, what was noted is noted again, a mend that was through is made once more.
//! - **A record for another state is not taken.** It names the device and the store revision it was written at; a
//!   stored device that is another one, or older than the record (a store put back from a copy), starts without it,
//!   from the device's own cursor, as before there was a record. One that does not parse is treated the same way.
//!
//! Also kept: the ids of the wishes for keys this device answered (`events`), so a restart does not hand the same
//! keys over again for a wish the hub still lists. And on an agent device the commands the gate let through that
//! nobody said were done (`agent`): reported again after a restart, marked uncertain; and the notes beside the
//! envelopes it sealed (`agent_write`), so that one the hub voids for its epoch is sealed again after a restart too.
//!
//! Not kept: the page in hand, the repair the head of the order waits for, the Welcomes read so far (all read again
//! from the position), the stream, tokens, waits, and the keys of the code (see `joins`: a sign-in's joins that
//! were not built yet are reported as `interrupted`, and whoever holds the code asks again).
//!
//! `floor` is where this device's reading of the order began when that was not its start (a device that takes the
//! hub's Desk as its first picture never pages below it): kept and handed back, set by nobody yet.

use super::{joins, Asked, Ctx, Engine, Mend};
use crate::app::io::{Entry, IoResult, StoreAnswer, StoreOp};
use crate::app::{Effect, LogLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use trommi_core::ids::{base64url_decode, base64url_encode, DeviceId, GroupId};
use trommi_core::store::OutboxKind;
use trommi_core::Error;

const VERSION: u32 = 1;

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
enum MendRecord {
    Group { group: String, after: u64, upto: u64 },
    Chain { group: String, sender: String },
}

// (ids are kept as their base64url text)
fn g(group: &GroupId) -> String {
    base64url_encode(group.as_bytes())
}
fn ug(text: &str) -> Option<GroupId> {
    GroupId::from_bytes(&base64url_decode(text).ok()?).ok()
}
fn ud(text: &str) -> Option<DeviceId> {
    DeviceId::from_base64url(text).ok()
}

/// A sign-in's session groups still to join when the record was written.
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug, Default)]
pub(super) struct JoinsRecord {
    waiting: Vec<String>,
    joined: u32,
    not_joined: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Debug)]
pub(super) struct Record {
    v: u32,
    device: String,
    /// The shell's store revision the device's state had when this was written.
    revision: u64,
    position: u64,
    #[serde(default)]
    floor: Option<u64>,
    #[serde(default)]
    behind: Vec<(String, u64, u64)>,
    #[serde(default)]
    keyless: Vec<(String, u64)>,
    #[serde(default)]
    withheld: Vec<(String, String, u64)>,
    #[serde(default)]
    mends: Vec<MendRecord>,
    #[serde(default)]
    reread_from: Option<u64>,
    #[serde(default)]
    joins: Option<JoinsRecord>,
    /// The wishes for keys this device answered with a handover (`events`), the newest last.
    #[serde(default)]
    answered: Vec<u64>,
    /// An agent device's commands the gate let through that nobody said were done (`agent`), oldest first.
    #[serde(default)]
    commands: Vec<super::agent::AgentCommand>,
    /// An agent device's notes beside the envelopes it sealed, by outbox id (`agent_write`).
    #[serde(default)]
    writes: Vec<(u64, super::agent_write::WriteNote)>,
}

/// A refusal's code as this build names it, from a record.
fn code_of(code: &str) -> &'static str {
    match code {
        "interrupted" => "interrupted",
        "bad-format" => "bad-format",
        other => Error::from_code(other).map_or("refused", |e| e.code()),
    }
}

impl JoinsRecord {
    pub(super) fn new(waiting: impl Iterator<Item = GroupId>, joined: u32, not_joined: &[(GroupId, &'static str)]) -> Self {
        JoinsRecord { waiting: waiting.map(|group| g(&group)).collect(), joined, not_joined: not_joined.iter().map(|(group, code)| (g(group), code.to_string())).collect() }
    }
}

impl Engine {
    fn record_key(&self) -> String {
        format!("engine/{}", self.room.to_base64url())
    }

    /// Asks the store for the record; the engine waits for it before anything else.
    pub(super) fn restore(&mut self, ctx: &mut Ctx<'_>) {
        let id = ctx.id();
        self.pending.insert(id, Asked::Record);
        self.restoring = Some(id);
        ctx.out.push(Effect::Store { id, op: StoreOp::Get { keys: vec![self.record_key()] } });
    }

    /// The store's answer: a record that fits this device's stored state is taken, any other is left.
    pub(super) fn restored(&mut self, ctx: &mut Ctx<'_>, result: &IoResult<StoreAnswer>) {
        self.restoring = None;
        let bytes = match result {
            IoResult::Ok(StoreAnswer::Values { values }) => values.first().cloned().flatten(),
            // (a store that does not answer: as if nothing was kept; the device's cursor is the place)
            _ => None,
        };
        let Some(bytes) = bytes else { return };
        let record: Record = match serde_json::from_slice(&bytes.0) {
            Ok(record) => record,
            Err(_) => return ctx.log(LogLevel::Warn, "the engine's record does not read: left"),
        };
        if record.v != VERSION || record.device != self.device.id().to_base64url() || record.revision > self.writes.revision() {
            return ctx.log(LogLevel::Warn, "the engine's record is not of this stored state: left");
        }
        if self.take_record(&record).is_none() {
            return ctx.log(LogLevel::Warn, "the engine's record does not read: left");
        }
        self.written = Some(record);
    }

    /// Takes the record's content; none, and nothing taken, when one of its ids does not read.
    fn take_record(&mut self, record: &Record) -> Option<()> {
        let max = super::BEHIND_MAX;
        let behind = record.behind.iter().take(max).map(|(group, change, epoch)| Some((ug(group)?, (*change, *epoch)))).collect::<Option<_>>()?;
        let keyless = record.keyless.iter().take(max).map(|(group, change)| Some((ug(group)?, *change))).collect::<Option<_>>()?;
        let withheld = record.withheld.iter().take(max).map(|(group, sender, seq)| Some(((ug(group)?, ud(sender)?), *seq))).collect::<Option<_>>()?;
        let mends = record
            .mends
            .iter()
            .take(max)
            .map(|mend| {
                Some(match mend {
                    MendRecord::Group { group, after, upto } => Mend::Group { group: ug(group)?, after: *after, upto: *upto },
                    MendRecord::Chain { group, sender } => Mend::Chain { group: ug(group)?, sender: ud(sender)? },
                })
            })
            .collect::<Option<_>>()?;
        let joining = match &record.joins {
            None => None,
            Some(joins) => {
                // The joins that were built are in the outbox and go on; the others need the code again.
                let outbox = self.device.outbox();
                let mut posted = BTreeSet::new();
                let mut joined = joins.joined;
                let mut not_joined: Vec<(GroupId, &'static str)> = joins.not_joined.iter().map(|(group, code)| Some((ug(group)?, code_of(code)))).collect::<Option<_>>()?;
                let waiting: BTreeSet<GroupId> = joins.waiting.iter().map(|group| ug(group)).collect::<Option<_>>()?;
                for group in waiting {
                    if not_joined.iter().any(|(named, _)| *named == group) {
                        continue;
                    }
                    if outbox.iter().any(|e| e.kind == OutboxKind::ExternalCommit && e.group == Some(group)) {
                        posted.insert(group);
                    } else if self.device.group(&group).is_ok() {
                        joined += 1;
                    } else {
                        not_joined.push((group, "interrupted"));
                    }
                }
                Some(joins::Joining::resumed(posted, joined, not_joined))
            }
        };
        self.position = record.position;
        self.todo_upto = record.position;
        self.floor = record.floor;
        (self.behind, self.keyless, self.withheld, self.mends) = (behind, keyless, withheld, mends);
        self.reread_from = record.reread_from;
        self.joining = joining;
        self.wishes.restore(&record.answered);
        if let Some(agent) = self.agent.as_mut() {
            agent.commands = record.commands.iter().take(super::agent::COMMANDS_MAX).cloned().collect();
            // (a note whose envelope the hub took before the record was written again goes)
            let outbox: Vec<u64> = self.device.outbox().iter().map(|e| e.id).collect();
            agent.writes = record.writes.iter().filter(|(id, note)| note.owed(*id, &outbox)).cloned().collect();
        }
        self.written = Some(record.clone());
        Some(())
    }

    fn record(&self) -> Record {
        Record {
            v: VERSION,
            device: self.device.id().to_base64url(),
            revision: self.writes.revision(),
            position: self.position,
            floor: self.floor,
            behind: self.behind.iter().map(|(group, (change, epoch))| (g(group), *change, *epoch)).collect(),
            keyless: self.keyless.iter().map(|(group, change)| (g(group), *change)).collect(),
            withheld: self.withheld.iter().map(|((group, sender), seq)| (g(group), sender.to_base64url(), *seq)).collect(),
            mends: self
                .mends
                .iter()
                .map(|mend| match mend {
                    Mend::Group { group, after, upto } => MendRecord::Group { group: g(group), after: *after, upto: *upto },
                    Mend::Chain { group, sender } => MendRecord::Chain { group: g(group), sender: sender.to_base64url() },
                })
                .collect(),
            reread_from: self.reread_from,
            joins: self.joining.as_ref().map(joins::Joining::record),
            answered: self.wishes.answered.iter().copied().collect(),
            commands: self.agent.as_ref().map(|a| a.commands.clone()).unwrap_or_default(),
            writes: self.agent.as_ref().map(|a| a.writes.iter().map(|(id, note)| (*id, note.clone())).collect()).unwrap_or_default(),
        }
    }

    /// Writes the record if what it holds changed: at the end of a step, behind the device's write of that step.
    pub(super) fn keep(&mut self, ctx: &mut Ctx<'_>) {
        if self.stopped.is_some() || self.restoring.is_some() {
            return;
        }
        let mut record = self.record();
        // (the revision alone is no reason to write: a record may name an older one)
        if let Some(written) = &self.written {
            record.revision = written.revision;
            if record == *written {
                return;
            }
            record.revision = self.writes.revision();
        } else if record.position == self.device.cursor() && record.behind.is_empty() && record.keyless.is_empty() && record.withheld.is_empty() && record.mends.is_empty() && record.reread_from.is_none() && record.joins.is_none() && record.floor.is_none() && record.answered.is_empty() && record.commands.is_empty() && record.writes.is_empty() {
            // (nothing the device's own cursor does not say)
            return;
        }
        let Ok(value) = serde_json::to_vec(&record) else { return };
        // (what the device wrote goes first: the record never names a state that is not stored)
        self.flush(ctx);
        let id = ctx.id();
        self.pending.insert(id, Asked::RecordWrite);
        ctx.out.push(Effect::Store { id, op: StoreOp::Put { put: vec![Entry { key: self.record_key(), value: value.into() }], delete: Vec::new() } });
        self.written = Some(record);
    }
}
