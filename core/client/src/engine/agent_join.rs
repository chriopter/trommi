//! An agent device's way into the room and its sessions, beside what [`super::agent`] keeps: what the connector's
//! client did from the invite on (`connector/src/client.rs`: `observe_room`, `take_welcomes_now`, `learn_past`).
//!
//! - **The invite** ([`AgentInvite`]): what the connector stored when it answered the invite (its room record):
//!   the room, the room epoch the Offer named and the session it was for. A device that answered an invite and
//!   follows no room yet opens with it ([`Engine::open_agent`]).
//! - **Following the room from the Offer's epoch** (4.4, 12.1.6). Once the lease is held and before anything else
//!   is read, a device that follows no room reads the room group's GroupInfo of the Offer's epoch
//!   (`GET /v1/groups/{room}/info?epoch=`) and the protocol core holds it against the state the Offer named
//!   ([`trommi_core::device::Device::join_observe`]). One it refuses stops the engine ([`Stopped::BadInvite`]); a
//!   hub that does not answer is asked again after the back-off. The room's log then comes in the hub's order,
//!   where the core takes the device for enrolled only by its inviter's Commit.
//! - **The Welcome check.** An agent device is never a leaf of the room group (5.1): a Welcome the hub names for
//!   it is passed over. A Welcome that joins another group than the hub named, or the room group, or, while the
//!   device holds no main session, a main session other than the one the invite was for (12.1.6), is a Welcome it
//!   must not hold: nothing of it is written (the step's writes are dropped) and the engine stops
//!   ([`Stopped::BadWelcome`]). The stored state stays as it was.
//! - **Learning a group's past** (4.6). After a Welcome, and for every live group whose past is not learned when a
//!   lease is acquired (a join while the hub was away), the device learns from what the hub keeps: first the room
//!   group's, then a helper session's main session's, then the group's own, each from its founding GroupInfo
//!   (`?epoch=0`) and all its Commits (`GET /v1/groups/{group}/log?kind=commit`), verified by the core
//!   ([`trommi_core::device::Device::learn_history`]). Then the group's envelopes up to the position are read
//!   again (after a Welcome always; for a group found unlearned once it was learned). A history the core refuses
//!   is said and not taken: what was written in the epochs before the join stays unread. A hub away is asked again
//!   after the back-off.
//!
//! The wish to be let in again after a Welcome that did not open (3.7) is asked in the sync's Welcome step for every
//! device ([`super::Engine`]): an agent device asks without a KeyPackage, as its connector did.

use super::{Asked, Ctx, Engine, OpenError, Retry, Stopped};
use crate::app::io::{DeviceEntry, IoId};
use crate::app::LogLevel;
use crate::hub::{Answer, Failure, Route};
use trommi_core::crypto::{Entropy, SystemEntropy};
use trommi_core::device::{Device, Joined};
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{GroupId, RoomId, SessionId};

/// What the connector stored when this device answered its invite (its room record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentInvite {
    /// The room invited into.
    pub room: RoomId,
    /// The room epoch the Offer named: where the device starts to follow the room group.
    pub room_epoch: u64,
    /// The session the invite was for (a takeover); none for a new session.
    pub session: Option<SessionId>,
}

/// What an agent device's way in holds.
#[derive(Default)]
pub(super) struct Way {
    pub(super) invite: Option<AgentInvite>,
    observe_asked: Option<IoId>,
    pub(super) observe_retry: Retry,

}

impl Way {
    pub(super) fn wake(&self) -> Option<u64> {
        self.observe_retry.at
    }
}

impl Engine {
    /// Opens an agent device's stored state as an agent's engine, with what the connector stored when it answered
    /// the invite. A device that answered the invite and follows no room yet opens too: it follows the room from
    /// the Offer's epoch first. `Device("wrong-room")` for a state of another room than the invite's.
    pub fn open_agent(hub: &str, client: &str, seed: u64, revision: u64, entries: Vec<DeviceEntry>, invite: AgentInvite) -> Result<Self, OpenError> {
        Self::open_agent_with(hub, client, seed, revision, entries, invite, Box::new(SystemEntropy))
    }

    pub fn open_agent_with(
        hub: &str,
        client: &str,
        seed: u64,
        revision: u64,
        entries: Vec<DeviceEntry>,
        invite: AgentInvite,
        entropy: Box<dyn Entropy + Send>,
    ) -> Result<Self, OpenError> {
        if revision == 0 || entries.is_empty() {
            return Err(OpenError::NoRoom);
        }
        let address = HubAddress::parse(hub).map_err(|_| OpenError::Hub)?;
        let (store, writes) = super::memstore::store(revision, entries.into_iter().map(|e| (e.key.0, e.value.0)).collect());
        let device = Device::open(store, entropy).map_err(|e| OpenError::Device(e.code()))?;
        let room = match device.room() {
            Some(room) if room != invite.room => return Err(OpenError::Device("wrong-room")),
            Some(room) => room,
            // (it answered the invite and follows no room yet: the invite's room, checked by the core when the
            //  room's GroupInfo is held against the Offer)
            None => invite.room,
        };
        let mut engine = Self::over(device, writes, room, hub, address, client, seed);
        engine.become_agent().map_err(|e| OpenError::Device(e.code()))?;
        if let Some(agent) = engine.agent.as_mut() {
            agent.way.invite = Some(invite);
        }
        Ok(engine)
    }

    /// The invite this agent engine was opened with.
    pub fn agent_invite(&self) -> Option<&AgentInvite> {
        self.agent.as_ref()?.way.invite.as_ref()
    }

    /// Before anything else is read: a device that follows no room yet starts following it at the Offer's epoch.
    /// True: the room is followed.
    pub(super) fn drive_observe(&mut self, ctx: &mut Ctx<'_>) -> bool {
        if self.device.room_history().is_some() {
            return true;
        }
        let room = GroupId::room(self.room);
        let Some(agent) = self.agent.as_mut() else { return true };
        let Some(epoch) = agent.way.invite.as_ref().map(|i| i.room_epoch) else {
            // (a state that follows no room, opened without its invite: nothing to follow it from)
            ctx.log(LogLevel::Error, "an agent device that follows no room was opened without its invite");
            self.stop(ctx, Stopped::NotMember);
            return false;
        };
        if agent.way.observe_asked.is_some() || agent.way.observe_retry.waiting(ctx.now) {
            return false;
        }
        let asked = self.ask(ctx, Route::GroupInfo { group: &room, epoch: Some(epoch) }, true, |expect| Asked::ObserveRoom { expect });
        if let Some(agent) = self.agent.as_mut() {
            agent.way.observe_asked = asked;
        }
        false
    }

    pub(super) fn on_observe(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>) {
        let Some(agent) = self.agent.as_mut() else { return };
        agent.way.observe_asked = None;
        let info = match answer {
            Ok(Answer::GroupInfo { group_info, .. }) => group_info,
            Ok(_) | Err(_) => {
                agent.way.observe_retry.fail(ctx.now, None, &mut self.jitter);
                return;
            }
        };
        agent.way.observe_retry.ok();
        match self.device.join_observe(&info) {
            Ok(()) => {
                ctx.log(LogLevel::Info, "the room is followed from the invite's epoch");
                self.report(super::Report::Groups);
            }
            Err(e) => {
                self.local(ctx, &e);
                if self.stopped.is_none() {
                    ctx.log(LogLevel::Error, format!("the room's state at the invite's epoch is not the one the invite named: {}", e.code()));
                    self.stop(ctx, Stopped::BadInvite);
                }
            }
        }
    }

    // ---- the Welcome check

    /// Whether a Welcome the hub names for `group` is passed over: an agent device is never a leaf of the room group.
    pub(super) fn agent_passes_welcome(&self, group: &GroupId) -> bool {
        self.agent.is_some() && group.is_room()
    }

    /// Whether this agent device holds a main session (taken before a Welcome is read, for the invite's check).
    pub(super) fn agent_has_main(&self) -> bool {
        self.agent.is_some() && self.main_group().is_some()
    }

    /// A Welcome an agent device joined: whether it is one the device must not hold. `named`: the group the hub
    /// named; `had_main`: the device held a main session before this answer's Welcomes.
    pub(super) fn agent_welcome_wrong(&self, named: &GroupId, joined: &Joined, had_main: bool) -> bool {
        let Some(agent) = self.agent.as_ref() else { return false };
        if joined.group != *named || joined.group.is_room() {
            return true;
        }
        let invited = agent.way.invite.as_ref().and_then(|i| i.session);
        let session = self.device.group(&joined.group).ok().and_then(|s| s.session);
        session.is_some_and(|s| s.parent.is_zero() && !had_main && invited.is_some_and(|wanted| wanted != s.session_id))
    }

    /// A Welcome the device must not hold: nothing of this step is written, and the engine stops. The stored state
    /// stays as it was.
    pub(super) fn bad_welcome(&mut self, ctx: &mut Ctx<'_>) {
        ctx.log(LogLevel::Error, "a Welcome was for another group than the hub said, or for a session this device was not invited to");
        // (what the device wrote since the last write that left is dropped, not handed to the shell)
        let _ = self.writes.take();
        self.stop(ctx, Stopped::BadWelcome);
    }

}
