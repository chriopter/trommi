//! What an agent device writes ([`Engine::agent_send`]), and sealing it again when the hub voided it for its epoch:
//! what the connector's client did with the note beside each envelope of its outbox (`connector/src/client.rs`:
//! `Waiting`, `seal`, `pump_once`).
//!
//! - **Nothing is signed before the guard.** A write waits for the lease and the rollback guard ([`super::agent`]):
//!   before them it is refused `busy` and nothing is sealed. While the pump halts on an envelope the hub refused
//!   without a void record, a write is refused `chain-break` (the connector's `chain-halted`): nothing more is
//!   sealed behind it. At most [`WAITING_MAX`] writes wait for the hub at
//!   once; one beyond is refused `too-many`, not queued.
//! - **The note.** Beside every envelope the agent sealed the engine keeps what it was sealed from
//!   ([`AgentWrite`], with its files), in its record (`record`): written behind the device's write of the same
//!   step. A note whose envelope the hub took goes.
//! - **Sealed again.** An envelope the hub voids for its epoch (`wrong-epoch` with a void record, 9.0.8: its number
//!   is used up, the chain goes on) is sealed again in the epoch the group stands in now, once the hub's order was
//!   read (the device has taken the Commit that moved the group on): a new envelope under a new number, reported
//!   as [`Report::Resealed`] with the outbox id of the first one it stands in for. At most [`RESEALS`] times; then,
//!   and for any other void or refusal, the caller hears of it as [`Report::Refused`] of the envelope that was
//!   refused last. A note kept across a restart is sealed again after it.
//!
//! The payloads are the caller's, complete: a later version names its `object_version` and
//! `previous_version_hash` already. The caller seals a later version only on one the hub took (the version before
//! it is not voided under it), so a payload sealed again names the same version before it.

use super::{Ctx, Engine, Report};
use serde::{Deserialize, Serialize};
use trommi_core::crypto::SecretBytes;
use trommi_core::device::{Draft, Sealed};
use trommi_core::envelope::Urgency;
use trommi_core::ids::{base64url_decode, FileId, GroupId, ObjectId, SessionId};
use trommi_core::store::OutboxKind;
use trommi_core::Error;

/// How often an envelope the hub voided for its epoch is sealed again before the sender hears of it.
pub const RESEALS: u8 = 3;
/// The most of the agent's writes that wait for the hub at once.
pub const WAITING_MAX: usize = 500;

/// What an agent device writes into one of its sessions: a [`Draft`] the engine can keep. Ids as base64url; a
/// payload or value is JSON text, complete as it is sealed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentWrite {
    SessionChat { session: String, payload: String },
    CardChat { session: String, card: String, payload: String },
    /// A register of a group: none deletes it.
    Register { group: String, name: String, value: Option<String> },
    CardFirst { session: String, urgency: u8, push: bool, payload: String },
    CardVersion { session: String, object: String, closed: bool, urgency: u8, push: bool, payload: String },
    PermissionRequest { session: String, urgency: u8, expires_at: u64, push: bool, payload: String },
    ArtifactFirst { session: String, payload: String },
    ArtifactVersion { session: String, object: String, closed: bool, payload: String },
}

fn text(payload: &str) -> SecretBytes {
    SecretBytes::new(payload.as_bytes().to_vec())
}

impl AgentWrite {
    /// The draft it is sealed from; `bad-format` for an id that does not read.
    pub fn draft(&self) -> Result<Draft, Error> {
        let session = |s: &str| SessionId::from_base64url(s);
        let object = |o: &str| ObjectId::from_base64url(o);
        let urgency = |u: u8| Urgency::from_byte(u);
        Ok(match self {
            AgentWrite::SessionChat { session: s, payload } => Draft::SessionChat { session: session(s)?, payload: text(payload) },
            AgentWrite::CardChat { session: s, card, payload } => Draft::CardChat { session: session(s)?, card: object(card)?, payload: text(payload) },
            AgentWrite::Register { group, name, value } => Draft::Register {
                group: GroupId::from_bytes(&base64url_decode(group)?)?,
                name: name.clone(),
                value: value.as_deref().map(text),
            },
            AgentWrite::CardFirst { session: s, urgency: u, push, payload } => Draft::CardFirst { session: session(s)?, urgency: urgency(*u)?, push: *push, payload: text(payload) },
            AgentWrite::CardVersion { session: s, object: o, closed, urgency: u, push, payload } => {
                Draft::CardVersion { session: session(s)?, object_id: object(o)?, closed: *closed, urgency: urgency(*u)?, push: *push, payload: text(payload) }
            }
            AgentWrite::PermissionRequest { session: s, urgency: u, expires_at, push, payload } => {
                Draft::PermissionRequest { session: session(s)?, urgency: urgency(*u)?, expires_at: *expires_at, push: *push, payload: text(payload) }
            }
            AgentWrite::ArtifactFirst { session: s, payload } => Draft::ArtifactFirst { session: session(s)?, payload: text(payload) },
            AgentWrite::ArtifactVersion { session: s, object: o, closed, payload } => Draft::ArtifactVersion { session: session(s)?, object_id: object(o)?, closed: *closed, payload: text(payload) },
        })
    }
}

/// The note beside an envelope the agent sealed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WriteNote {
    write: AgentWrite,
    /// The files its header names, base64url.
    files: Vec<String>,
    /// How often it was sealed again, and the outbox id of the first envelope it stands in for.
    resealed: u8,
    first: u64,
    /// The hub voided it for its epoch: it is sealed again once the order was read.
    rebuild: bool,
}

impl WriteNote {
    /// Still owed after a restart: sealed again, or its envelope still waits in the outbox.
    pub(super) fn owed(&self, id: u64, outbox: &[u64]) -> bool {
        self.rebuild || outbox.contains(&id)
    }
}

impl Engine {
    /// The agent writes: sealed now (the hash and number are final), posted by the pump. Refused `forbidden` for no
    /// agent's engine, `busy` before the lease and the rollback guard, `chain-break` while the pump halts on an envelope
    /// the hub refused for good, `too-many` while [`WAITING_MAX`] writes wait.
    pub fn agent_send(&mut self, ctx: &mut Ctx<'_>, write: AgentWrite, files: &[FileId]) -> Result<Sealed, Error> {
        let Some(agent) = self.agent.as_ref() else { return Err(Error::Forbidden) };
        if self.stopped.is_some() {
            return Err(Error::Storage("the device is closed".into()));
        }
        if !agent.signs() {
            return Err(Error::Busy);
        }
        // (the hub refused an earlier envelope for good: nothing more is sealed behind it, as the connector did)
        if self.blocked.is_some() {
            return Err(Error::ChainBreak);
        }
        if agent.writes.len() >= WAITING_MAX {
            return Err(Error::TooMany);
        }
        let draft = write.draft()?;
        let sealed = match self.device.seal(&draft, None, files, ctx.now) {
            Ok(sealed) => sealed,
            Err(e) => {
                self.local(ctx, &e);
                self.drive(ctx);
                return Err(e);
            }
        };
        let note = WriteNote { write, files: files.iter().map(FileId::to_base64url).collect(), resealed: 0, first: sealed.outbox_id, rebuild: false };
        if let Some(agent) = self.agent.as_mut() {
            agent.writes.insert(sealed.outbox_id, note);
        }
        self.drive(ctx);
        Ok(sealed)
    }

    /// The hub took or refused an outbox entry: its note goes.
    pub(super) fn agent_written(&mut self, entry: u64) {
        if let Some(agent) = self.agent.as_mut() {
            agent.writes.remove(&entry);
        }
    }

    /// The hub voided an envelope of the agent: true when it is sealed again (`wrong-epoch`, fewer than [`RESEALS`]
    /// times); its sender hears nothing of it yet.
    pub(super) fn agent_voided(&mut self, entry: u64, code: &Error) -> bool {
        let Some(agent) = self.agent.as_mut() else { return false };
        let Some(note) = agent.writes.get_mut(&entry) else { return false };
        if *code != Error::WrongEpoch || note.resealed >= RESEALS {
            agent.writes.remove(&entry);
            return false;
        }
        note.rebuild = true;
        true
    }

    /// Seals again what the hub voided for its epoch, once the hub's order was read.
    pub(super) fn drive_reseal(&mut self, ctx: &mut Ctx<'_>) {
        let Some(agent) = self.agent.as_ref() else { return };
        if self.pass || self.page.is_some() || !self.todo.is_empty() || self.repair.is_some() || !agent.signs() {
            return;
        }
        let due: Vec<u64> = agent.writes.iter().filter(|(_, n)| n.rebuild).map(|(id, _)| *id).collect();
        for old in due {
            let Some(note) = self.agent.as_mut().and_then(|a| a.writes.remove(&old)) else { continue };
            let files: Vec<FileId> = note.files.iter().filter_map(|f| FileId::from_base64url(f).ok()).collect();
            let sealed = note.write.draft().and_then(|draft| self.device.seal(&draft, None, &files, ctx.now));
            match sealed {
                Ok(sealed) => {
                    let again = WriteNote { resealed: note.resealed + 1, rebuild: false, ..note.clone() };
                    if let Some(agent) = self.agent.as_mut() {
                        agent.writes.insert(sealed.outbox_id, again);
                    }
                    self.report(Report::Resealed { first: note.first, voided: old, id: sealed.outbox_id, envelope_hash: sealed.envelope_hash, object_id: sealed.object_id });
                }
                // (not now: a Commit of this device waits)
                Err(Error::Busy) => {
                    if let Some(agent) = self.agent.as_mut() {
                        agent.writes.insert(old, note);
                    }
                    return;
                }
                Err(e) => {
                    self.local(ctx, &e);
                    if self.stopped.is_some() {
                        return;
                    }
                    self.report(Report::Refused { id: old, kind: OutboxKind::Envelope, code: e.code(), voided: true });
                }
            }
        }
    }
}
