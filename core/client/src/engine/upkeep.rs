//! Tending the groups: the gaps any human device closes when it sees them, and what the core found.
//!
//! - **5.2.8, a stale session.** A session group with a leaf the room no longer allows, or a helper session without
//!   its opener, takes nothing until one Commit removes every such leaf (each with its Cut) and adds the opener it
//!   lacks. Whichever human device of the group comes first makes it, so a removal another device began is
//!   finished here.
//! - **5.2.7, a human device a live session lacks.** One left out of a founding (it had no KeyPackage at the hub),
//!   or let into the room since: one KeyPackage of it is claimed and one Commit adds it.
//! - **The invite steps** (12.1.5, 12.1.6, 5.3): what is left to do for a device this one let in by link, as the
//!   core lists it from the state of the groups: the Commit built again after it lost its epoch, the key handover,
//!   the newcomer added to each live session, an agent's main session founded, a takeover's sessions cleaned with
//!   the new agent in the seat, and at a takeover's end the hub's list of helper sessions held against this device's.
//!   What an open invite still has to do is the invite's: those groups and devices are left out of the two rules
//!   above.
//! - **Seats** (5.2.2, [`crate::seats`]): an agent device of the room that sits in no session gets its main
//!   session from any human device, once it is connected and waited out its grace. The rule says who is due; the
//!   hub's list of the room's groups says who sits in one already; one founding at a time.
//! - **Sessions the hub archived** (5.2.10) while this device was away, or before it held them: the hub's list of the
//!   room's groups is read once per run and after a session group was joined, and a held session it lists as no
//!   longer live is archived here too (the stream says it only to who is connected then).
//! - **The core's findings** (a Cut that names another envelope than the one shown): said once, then cleared.
//!
//! # How it runs
//!
//! A **look** reads the device's groups and lists what is to mend ([`Job`]); the jobs are done one after another,
//! each looked at again right before it acts (the group may have moved on while a KeyPackage was claimed). A look
//! is wanted when the groups changed, when a Commit of this device was answered, and with every timed round; it is
//! made only when the hub's order is at rest, since the core judges a group against the room states it holds.
//!
//! Nothing here is remembered across a restart, and nothing needs to be: the jobs are read from the state of the
//! groups, so a look after a restart finds what is still to do, and a job that was done is not listed again (its
//! group is `pending` until the Commit has its place in the log, then mended).
//!
//! **What fails is tried again after a wait, never in a loop.** A KeyPackage the hub does not have yet (a device let
//! in a moment ago), a hub that is not reached, a Commit the core does not build or the hub refuses: the next look
//! waits ([`super::Backoff::HUB`], doubling to a minute). A change of the groups ends the wait, since what stood
//! in the way may be gone. A KeyPackage is claimed only for a group this device is a leaf of and has no Commit
//! waiting in, so one is not spent on a Commit that cannot be made.

use super::{Alert, Asked, Ctx, Engine, Report, Retry};
use crate::app::io::IoId;
use crate::hub::{Answer, Failure, GroupRow, GroupRowKind, Presence, Route};
use crate::seats::{Known, Plan, SeatPlanner};
use std::collections::{BTreeSet, VecDeque};
use trommi_core::device::InviteStep;
use trommi_core::ids::{DeviceId, GroupId, InviteId, SessionId};
use trommi_core::mls::profile::Cut;
use trommi_core::Error;

/// At most so many devices are held as connected.
const ONLINE_MAX: usize = 4096;
/// Groups asked for in one page of the hub's list, and the most that are read for one job.
const GROUPS_PAGE: u32 = 1000;
const GROUPS_MAX: usize = 8 * GROUPS_PAGE as usize;
/// At most so many repairs that failed are remembered as said (a group and a reason each).
const SAID_MAX: usize = 1024;

/// One thing to mend in a group, as a look found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Job {
    /// 12.1.6, 5.2.2: an agent device's main session is founded. `invite`: a step of that invite, whose KeyPackage
    /// of the agent is used; none: a seat, and one is claimed.
    Found { agent: DeviceId, invite: Option<InviteId> },
    /// 5.3.1: a session is taken over in this group (the invite's step says with which Cuts and which agent).
    TakeOver { invite: InviteId, group: GroupId },
    /// 5.3.1 (c): the hub's list of the helper sessions under a session that was taken over.
    Check { invite: InviteId, session: SessionId },
    /// The hub's list of who sits in a session, for the agents the seat rule holds due.
    Seat { candidates: Vec<String> },
    /// The hub's list of the room's groups, for the sessions it archived before this device held them (5.2.10).
    Archived,
    /// 5.2.8: the Commit that makes a stale session group live again.
    Clean { group: GroupId },
    /// 5.2.7: a human device of the room is added to a live session group.
    Add { group: GroupId, device: DeviceId },
}

#[derive(Default)]
pub(super) struct Tending {
    /// A look is wanted.
    pub(super) want: bool,
    /// What the look in hand still has to do, in order.
    jobs: VecDeque<Job>,
    /// A look is in hand: made, and not yet through.
    looking: bool,
    /// The claim in flight for the job being done.
    asked: Option<IoId>,
    /// A Commit of this device was taken by the hub and has not come back in the log yet.
    unmerged: bool,
    /// The look in hand found something to do.
    did: bool,
    /// Something of the look in hand failed or could not be done yet: the next one waits.
    owing: bool,
    /// The longest wait the hub named in the look in hand.
    retry_after: Option<u64>,
    pub(super) retry: Retry,
    /// The own leaves are looked at with the next look (5.2.9): set by the timed round.
    leaves: bool,
    /// The hub's list is read for sessions it archived before this device held them: set when one was joined.
    pub(super) archived: bool,
    /// The hub's list of the room's groups, as far as it was read for the job in hand.
    rows: Vec<GroupRow>,
    /// The seat rule, who is connected (as the stream said), and when the rule wants to look again.
    seats: SeatPlanner,
    online: BTreeSet<DeviceId>,
    recheck: Option<u64>,
    /// Repairs that failed and were said: a group and the reason, once each.
    said: BTreeSet<(GroupId, &'static str)>,
}

impl Tending {
    /// The first look is wanted as soon as the order is at rest.
    pub(super) fn new() -> Self {
        // (and the hub's list is read once per run for sessions archived while this device was away: the stream
        //  says so only to who is connected then)
        Tending { want: true, archived: true, ..Default::default() }
    }
    /// A Commit of this device was refused for good: whatever builds it again waits first.
    pub(super) fn refused(&mut self) {
        self.owing = true;
        self.want = true;
        self.looking = true;
    }
    /// The hub took a Commit of this device. Until it has its place in the log (`merged`: it has), the groups are
    /// not looked at: between the hub's answer and the log the core lists what that Commit does as still to do (a
    /// founding that was taken shows as a session to found), and a look then would build it a second time.
    pub(super) fn sent(&mut self, merged: bool) {
        self.want = true;
        self.unmerged = !merged;
    }
    /// The groups changed: looked at now, whatever waited.
    pub(super) fn changed(&mut self) {
        self.unmerged = false;
        self.want = true;
        self.retry.hurry();
    }
    /// The timed round: a look, and the own leaves with it.
    pub(super) fn timed(&mut self) {
        // (a Commit of this device that never showed in the log does not hold the looks back for good)
        self.unmerged = false;
        self.want = true;
        self.leaves = true;
    }
    pub(super) fn wake(&self) -> Option<u64> {
        let retry = self.retry.at.filter(|_| self.want);
        match (retry, self.recheck) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
    /// The stream opened again: who is here is told anew.
    pub(super) fn stream_opened(&mut self) {
        self.online.clear();
    }
    /// A session group was joined: the hub's list of the room's groups is read with the next look, for one it
    /// archived before.
    pub(super) fn look_for_archived(&mut self) {
        self.archived = true;
        self.want = true;
    }
    /// A device came or went: one that came may be an agent that waits for its seat.
    pub(super) fn presence(&mut self, presence: &Presence) {
        if !presence.online {
            self.online.remove(&presence.device);
        } else if self.online.len() < ONLINE_MAX && self.online.insert(presence.device) {
            self.want = true;
        }
    }
}

impl Engine {
    pub(super) fn drive_tending(&mut self, ctx: &mut Ctx<'_>) {
        // Repairs can introduce roles and must not judge them against an incomplete revocation past.
        if self.anchored_past_missing() { return; }
        // (the core judges a session against the room states this device holds: the order first; and a sign-in's
        //  own joins of its sessions come before anybody adds it by Welcome)
        if self.joining.is_some() || !self.caught_up || self.pass || !self.todo.is_empty() || self.repair.is_some() || self.tending.asked.is_some() || self.tending.unmerged {
            return;
        }
        if self.tending.recheck.is_some_and(|at| ctx.now >= at) {
            self.tending.recheck = None;
            self.tending.want = true;
        }
        if self.tending.jobs.is_empty() && !self.tending.looking {
            if !self.tending.want || self.tending.retry.waiting(ctx.now) {
                return;
            }
            self.tending.want = false;
            self.tending.looking = true;
            let outbox = self.device.outbox().len();
            self.look(ctx);
            self.tending.did = !self.tending.jobs.is_empty() || self.device.outbox().len() != outbox;
        }
        while self.tending.asked.is_none() && self.stopped.is_none() {
            let Some(job) = self.tending.jobs.pop_front() else { break };
            self.tend(ctx, job);
        }
        if self.tending.asked.is_none() && self.tending.looking {
            // the look is through: the next waits if something of it is still owed
            self.tending.looking = false;
            if std::mem::take(&mut self.tending.owing) {
                let retry_after = self.tending.retry_after.take();
                self.tending.retry.fail(ctx.now, retry_after, &mut self.jitter);
                self.tending.want = true;
            } else if !self.tending.did {
                // (a look that found nothing to do: only then is what failed before forgotten, since a Commit
                //  made here may still be refused)
                self.tending.retry.ok();
            }
        }
    }

    /// Reads the groups and lists what is to mend.
    fn look(&mut self, ctx: &mut Ctx<'_>) {
        self.tell_findings(ctx);
        if std::mem::take(&mut self.tending.archived) && self.device.groups().unwrap_or_default().iter().any(|g| g.session.is_some() && !g.archived) {
            self.tending.jobs.push_back(Job::Archived);
        }
        if self.stopped.is_some() || !self.device.is_human() {
            return;
        }
        let Some(humans) = self.device.room_history().map(|history| history.newest().humans.clone()) else { return };
        let me = self.device.id();
        let groups = match self.device.groups() {
            Ok(groups) => groups,
            Err(e) => return self.local(ctx, &e),
        };
        if std::mem::take(&mut self.tending.leaves) && !self.renew_leaves(ctx, &groups) {
            // (the room group first: the sessions with the look its Commit's return wants)
            self.tending.leaves = true;
        }
        if self.stopped.is_some() {
            return;
        }
        // what is left to do for the devices this one let in by link
        let steps = match self.device.invite_steps() {
            Ok(steps) => steps,
            Err(e) => return self.failed(ctx, &e),
        };
        let mut taken: Vec<GroupId> = Vec::new();
        let mut coming: BTreeSet<DeviceId> = BTreeSet::new();
        for (invite, step) in steps {
            let made = match step {
                InviteStep::Wait => Ok(()),
                InviteStep::Commit => self.device.invite_recommit(&invite, ctx.now).map(drop),
                InviteStep::Handover { device, .. } => {
                    coming.insert(device);
                    self.device.invite_handover(&invite).map(drop)
                }
                InviteStep::AddToSession { group, device } => {
                    coming.insert(device);
                    self.tending.jobs.push_back(Job::Add { group, device });
                    Ok(())
                }
                InviteStep::FoundSession { agent, .. } => {
                    coming.insert(agent);
                    self.tending.jobs.push_back(Job::Found { agent, invite: Some(invite) });
                    Ok(())
                }
                InviteStep::TakeOver { group, agent, .. } => {
                    coming.insert(agent);
                    taken.push(group);
                    self.tending.jobs.push_back(Job::TakeOver { invite, group });
                    Ok(())
                }
                InviteStep::CheckHelpers { session } => {
                    self.tending.jobs.push_back(Job::Check { invite, session });
                    Ok(())
                }
            };
            if let Err(e) = made {
                self.failed(ctx, &e);
            }
            if self.stopped.is_some() {
                return;
            }
        }
        // (a takeover's session and its helper sessions keep the old leaf until the new one takes the seat)
        let covered = |group: &trommi_core::device::GroupSummary| {
            taken.contains(&group.group) || group.session.is_some_and(|s| s.parent != SessionId::ZERO && taken.contains(&GroupId::session(s.room_id, s.parent)))
        };
        let mut seated: BTreeSet<String> = BTreeSet::new();
        let mut founding_pending = false;
        for group in groups {
            let Some(session) = group.session else { continue };
            seated.extend(group.leaves.iter().map(DeviceId::to_base64url));
            founding_pending |= session.parent == SessionId::ZERO && group.pending && group.epoch == 0;
            if group.archived || !group.leaves.contains(&me) || covered(&group) {
                continue;
            }
            if group.is_stale() {
                self.tending.jobs.push_back(Job::Clean { group: group.group });
                continue;
            }
            for device in humans.iter().filter(|device| **device != me && !coming.contains(device) && !group.leaves.contains(device)) {
                self.tending.jobs.push_back(Job::Add { group: group.group, device: *device });
            }
        }
        // the agents without a session: who is due is the seat rule's
        let agents = self.device.room_history().map(|history| history.newest().room.agents.clone()).unwrap_or_default();
        let known = Known {
            agents: agents.iter().map(DeviceId::to_base64url).collect(),
            seated,
            coming: coming.iter().map(DeviceId::to_base64url).collect(),
            founding_pending,
            online: self.tending.online.iter().map(DeviceId::to_base64url).collect(),
            now_ms: ctx.now,
        };
        match self.tending.seats.plan(&known) {
            Plan::Idle { recheck_at } => self.tending.recheck = recheck_at,
            Plan::AskHub { candidates } => self.tending.jobs.push_back(Job::Seat { candidates }),
        }
    }

    /// A step the core does not take now (`busy`, an epoch that was lost), or cannot: with the next look.
    fn failed(&mut self, ctx: &mut Ctx<'_>, error: &Error) {
        ctx.log(crate::app::LogLevel::Info, format!("upkeep: a step waits ({})", error.code()));
        self.local(ctx, error);
        self.tending.owing = true;
    }

    /// The step of an invite that a job was listed for, as it stands now.
    fn step(&mut self, invite: &InviteId, which: impl Fn(&InviteStep) -> bool) -> Option<InviteStep> {
        self.device.invite_steps().ok()?.into_iter().find(|(of, step)| of == invite && which(step)).map(|(_, step)| step)
    }

    /// Whether a founding of this device with `agent` in it waits for the hub's answer or its place in the log.
    fn founding(&self, agent: &DeviceId) -> bool {
        self.device.groups().unwrap_or_default().iter().any(|g| g.session.is_some_and(|s| s.parent == SessionId::ZERO) && g.pending && g.epoch == 0 && g.leaves.contains(agent))
    }

    /// 5.2.9: a human device renews its leaf where the core says it is due (an empty Commit). The room group first
    /// and alone: a session's Commit names the room's epoch, so one built beside a Commit of the room group would be
    /// `room-behind` once that is taken. False: the sessions are still to look at.
    fn renew_leaves(&mut self, ctx: &mut Ctx<'_>, groups: &[trommi_core::device::GroupSummary]) -> bool {
        let live = |group: &&trommi_core::device::GroupSummary| !group.archived && !group.is_stale();
        if let Some(room) = groups.iter().find(|group| group.group.is_room()) {
            if room.pending {
                return false;
            }
            match self.device.update(&room.group, false, ctx.now) {
                Ok(Some(_)) => return false,
                Ok(None) => {}
                Err(e) => self.local(ctx, &e),
            }
        }
        for group in groups.iter().filter(live).filter(|group| !group.group.is_room() && !group.pending) {
            if let Err(e) = self.device.update(&group.group, false, ctx.now) {
                self.local(ctx, &e);
            }
            if self.stopped.is_some() {
                break;
            }
        }
        true
    }

    /// Does one job as far as it goes without the hub; a KeyPackage it needs is claimed, and the job goes on with
    /// the answer.
    fn tend(&mut self, ctx: &mut Ctx<'_>, job: Job) {
        let mut partial = false;
        let devices: Vec<DeviceId> = match &job {
            Job::Clean { group } => {
                let Some(opener) = self.to_clean(group) else { return };
                match opener {
                    Some(opener) => vec![opener],
                    None => return self.clean(ctx, *group, None),
                }
            }
            Job::Add { group, device } => {
                if !self.to_add(group, device) {
                    return;
                }
                vec![*device]
            }
            Job::Found { agent, invite } => {
                // (one that went out is not built a second time: the hub refuses it, and the step shows again
                //  once the first is through)
                if self.founding(agent) {
                    return;
                }
                // One of every other human device the hub has one of (a partial claim: one left out is added
                // later, 5.2.7); for a seat, the agent's own with them.
                let me = self.device.id();
                let mut devices: Vec<DeviceId> = self.device.room_history().map(|h| h.newest().humans.iter().copied().filter(|d| *d != me).collect()).unwrap_or_default();
                if invite.is_none() {
                    devices.insert(0, *agent);
                }
                if devices.is_empty() {
                    return self.found(ctx, *agent, *invite, Vec::new());
                }
                partial = true;
                devices
            }
            Job::TakeOver { invite, group } => {
                let Some(InviteStep::TakeOver { cuts, agent, key_package, .. }) = self.step(invite, |s| matches!(s, InviteStep::TakeOver { group: of, .. } if of == group)) else { return };
                match key_package {
                    // (the main session: with the KeyPackage of the confirmed Request)
                    Some(key_package) => {
                        if let Err(e) = self.device.clean_session(group, &cuts, Some((&agent, &key_package)), ctx.now) {
                            self.failed(ctx, &e);
                        }
                        return;
                    }
                    // (a helper session's new opener: with one claimed at the hub)
                    None => vec![agent],
                }
            }
            Job::Check { .. } | Job::Seat { .. } | Job::Archived => {
                self.tending.rows.clear();
                return self.ask_groups(ctx, job, 0);
            }
        };
        self.tending.asked = self.ask(ctx, Route::ClaimKeyPackages { devices: &devices, partial }, true, |expect| Asked::Claim { expect, job });
        if self.tending.asked.is_none() {
            self.tending.owing = true;
        }
    }

    /// Whether the group is stale and can be cleaned now; inside, the opener it lacks.
    fn to_clean(&self, group: &GroupId) -> Option<Option<DeviceId>> {
        let held = self.device.group(group).ok()?;
        // (a Commit of this device waits there: its answer, or its place in the log, wants a look again)
        (!held.pending && !held.archived && held.is_stale()).then_some(held.missing_opener)
    }

    fn to_add(&self, group: &GroupId, device: &DeviceId) -> bool {
        let me = self.device.id();
        let human = self.device.room_history().is_some_and(|history| history.newest().is_human(device));
        self.device.group(group).is_ok_and(|held| human && !held.pending && !held.archived && !held.is_stale() && held.leaves.contains(&me) && !held.leaves.contains(device))
    }

    pub(super) fn on_claimed(&mut self, ctx: &mut Ctx<'_>, job: Job, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.tending.asked = None;
        let key_packages = match answer {
            Ok(Answer::Claimed { key_packages, .. }) => key_packages,
            // (asked again, unchanged, once signed in)
            Err(_) if unauthorised => return self.tending.jobs.push_front(job),
            // none at the hub yet (a device let in a moment ago), or the hub was not reached: with the next look
            Ok(_) | Err(_) => {
                if let Err(Failure::BadAnswer { why }) = &answer {
                    ctx.log(crate::app::LogLevel::Warn, format!("upkeep: an answer the protocol does not allow ({why})"));
                }
                if let Err(Failure::NotNow { retry_after_ms, .. }) = answer {
                    self.tending.retry_after = self.tending.retry_after.max(retry_after_ms);
                }
                self.tending.owing = true;
                return;
            }
        };
        let wanted = match &job {
            Job::Add { device, .. } => Some(*device),
            Job::Clean { group } => self.to_clean(group).flatten(),
            Job::Found { agent, invite } => return self.found(ctx, *agent, *invite, key_packages),
            Job::TakeOver { invite, group } => {
                let step = self.step(invite, |s| matches!(s, InviteStep::TakeOver { group: of, key_package: None, .. } if of == group));
                let Some(InviteStep::TakeOver { cuts, agent, .. }) = step else { return };
                let made = match key_packages.iter().find(|(device, _)| *device == agent) {
                    Some((_, key_package)) => self.device.clean_session(group, &cuts, Some((&agent, key_package)), ctx.now).map(drop),
                    None => Err(Error::NotFound),
                };
                if let Err(e) = made {
                    self.failed(ctx, &e);
                }
                return;
            }
            Job::Check { .. } | Job::Seat { .. } | Job::Archived => return,
        };
        let Some((device, key_package)) = key_packages.into_iter().find(|(device, _)| Some(*device) == wanted) else {
            // (not the device asked for, or the group moved on meanwhile: the next look says what is left)
            self.tending.owing = true;
            return;
        };
        match job {
            Job::Add { group, .. } => {
                // (looked at again: the group may have moved on while the claim was away)
                if !self.to_add(&group, &device) {
                    return;
                }
                if let Err(e) = self.device.add_to_session(&group, &device, &key_package, ctx.now) {
                    self.local(ctx, &e);
                    self.tending.owing = true;
                }
            }
            Job::Clean { group } => self.clean(ctx, group, Some((device, key_package))),
            _ => {}
        }
    }

    /// Founds the main session of `agent` with the KeyPackages claimed: the agent's own from the invite's step, or
    /// (a seat) from the claim.
    fn found(&mut self, ctx: &mut Ctx<'_>, agent: DeviceId, invite: Option<InviteId>, claimed: Vec<(DeviceId, Vec<u8>)>) {
        let name = agent.to_base64url();
        let mut own = None;
        let mut packages = Vec::new();
        for (device, key_package) in claimed {
            if device == agent {
                own = Some(key_package);
            } else {
                packages.push(key_package);
            }
        }
        if let Some(invite) = &invite {
            own = match self.step(invite, |s| matches!(s, InviteStep::FoundSession { agent: of, .. } if *of == agent)) {
                Some(InviteStep::FoundSession { key_package, .. }) => Some(key_package),
                _ => return, // (the step is gone: founded meanwhile)
            };
        }
        let made = match own {
            Some(own) if !self.founding(&agent) => {
                packages.push(own);
                self.device.found_session(&agent, &packages, ctx.now).map(drop)
            }
            Some(_) => return,
            None => Err(Error::NotFound), // (the agent has no KeyPackage at the hub yet)
        };
        match made {
            Ok(()) if invite.is_none() => self.tending.seats.founded(&name),
            Ok(()) => {}
            Err(e) => {
                self.failed(ctx, &e);
                if invite.is_none() {
                    // (tried again soon: a pause that grows, the seat rule's)
                    let at = self.tending.seats.failed(&name, ctx.now);
                    self.tending.recheck = Some(self.tending.recheck.map_or(at, |r| r.min(at)));
                }
            }
        }
    }

    fn ask_groups(&mut self, ctx: &mut Ctx<'_>, job: Job, after: u64) {
        let room = self.room;
        self.tending.asked = self.ask(ctx, Route::RoomGroups { room: &room, after, limit: GROUPS_PAGE }, true, |expect| Asked::RoomGroups { expect, job });
        if self.tending.asked.is_none() {
            self.tending.owing = true;
        }
    }

    /// A page of the hub's list of the room's groups, for a takeover's check or a seat.
    pub(super) fn on_room_groups(&mut self, ctx: &mut Ctx<'_>, job: Job, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.tending.asked = None;
        let (items, more, after) = match answer {
            Ok(Answer::RoomGroups { items, more, after }) => (items, more, after),
            Err(_) if unauthorised => return self.tending.jobs.push_front(job),
            Ok(_) | Err(_) => {
                // (the list is read again with the next look)
                self.tending.archived |= job == Job::Archived;
                if let Err(Failure::BadAnswer { why }) = &answer {
                    ctx.log(crate::app::LogLevel::Warn, format!("upkeep: an answer the protocol does not allow ({why})"));
                }
                if let Err(Failure::NotNow { retry_after_ms, .. }) = answer {
                    self.tending.retry_after = self.tending.retry_after.max(retry_after_ms);
                }
                self.tending.owing = true;
                return;
            }
        };
        let any = !items.is_empty();
        self.tending.rows.extend(items);
        if more && any && self.tending.rows.len() < GROUPS_MAX {
            return self.ask_groups(ctx, job, after);
        }
        let rows = std::mem::take(&mut self.tending.rows);
        if more {
            // (a list that does not end is no list to decide by)
            self.tending.archived |= job == Job::Archived;
            self.tending.owing = true;
            return;
        }
        match job {
            Job::Check { invite, session } => {
                let helpers: Vec<GroupId> = rows.iter().filter(|g| g.live && g.kind == GroupRowKind::Helper && g.parent == Some(session)).map(|g| g.group).collect();
                match self.device.invite_checked(&invite, &helpers) {
                    Ok(()) => self.tending.want = true, // (the next look finishes the invite)
                    Err(e) => {
                        // a helper session this device does not hold yet: its Welcome waits, and is read now
                        if e == Error::GroupBehind && self.repair.is_none() {
                            self.welcomes_idle = None;
                            self.repair = Some(super::Repair::Welcomes { after: self.welcomes_after, joined: Vec::new(), asked: false });
                        }
                        self.failed(ctx, &e);
                    }
                }
            }
            Job::Seat { candidates } => {
                let seated: BTreeSet<String> = rows.iter().filter(|g| g.kind != GroupRowKind::Room).flat_map(|g| g.leaves.iter().map(DeviceId::to_base64url)).collect();
                if let Some(name) = self.tending.seats.after_hub(&candidates, &seated) {
                    if let Ok(agent) = DeviceId::from_base64url(&name) {
                        self.tending.jobs.push_front(Job::Found { agent, invite: None });
                    }
                }
            }
            Job::Archived => {
                let mut any = false;
                for row in rows.iter().filter(|g| !g.live && g.kind != GroupRowKind::Room) {
                    any |= self.archive_held(ctx, &row.group);
                    if self.stopped.is_some() {
                        return;
                    }
                }
                if any {
                    self.report(Report::Groups);
                    self.tending.want = true;
                }
            }
            _ => {}
        }
    }

    /// The Commit that removes a session group's leaves the room no longer allows, each with its Cut, and adds the
    /// opener it lacks.
    fn clean(&mut self, ctx: &mut Ctx<'_>, group: GroupId, opener: Option<(DeviceId, Vec<u8>)>) {
        let Ok(held) = self.device.group(&group) else { return };
        if held.pending || !held.is_stale() || held.missing_opener != opener.as_ref().map(|(device, _)| *device) {
            // (it moved on: the next look says what is left)
            self.tending.want = true;
            return;
        }
        let cuts: Result<Vec<Cut>, Error> = held.disallowed.iter().map(|gone| self.device.cut_of(&group, gone)).collect();
        let made = cuts.and_then(|cuts| self.device.clean_session(&group, &cuts, opener.as_ref().map(|(device, key_package)| (device, key_package.as_slice())), ctx.now));
        if let Err(e) = made {
            self.local(ctx, &e);
            if self.stopped.is_some() {
                return;
            }
            // Nothing is written in the group until it is repaired: said once per group and reason.
            self.tending.owing = true;
            if self.tending.said.len() < SAID_MAX && self.tending.said.insert((group, e.code())) {
                self.report(Report::Alert(Alert::Unrepaired { group, code: e.code() }));
            }
        }
    }

    /// What the core found while it processed Commits: said, then cleared.
    fn tell_findings(&mut self, ctx: &mut Ctx<'_>) {
        let found = match self.device.findings() {
            Ok(found) => found,
            Err(e) => return self.local(ctx, &e),
        };
        if found.is_empty() {
            return;
        }
        for finding in found {
            self.report(Report::Alert(Alert::Finding { group: finding.group, sender: finding.sender, code: finding.code.code() }));
        }
        if let Err(e) = self.device.findings_read() {
            self.local(ctx, &e);
        }
    }
}
