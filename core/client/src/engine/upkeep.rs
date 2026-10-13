//! Tending the groups: the gaps any human device closes when it sees them, and what the core found.
//!
//! - **5.2.8, a stale session.** A session group with a leaf the room no longer allows, or a helper session without
//!   its opener, takes nothing until one Commit removes every such leaf (each with its Cut) and adds the opener it
//!   lacks. Whichever human device of the group comes first makes it, so a removal another device began is
//!   finished here.
//! - **5.2.7, a human device a live session lacks.** One left out of a founding (it had no KeyPackage at the hub),
//!   or let into the room since: one KeyPackage of it is claimed and one Commit adds it.
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
use crate::hub::{Answer, Failure, Route};
use std::collections::{BTreeSet, VecDeque};
use trommi_core::ids::{DeviceId, GroupId};
use trommi_core::mls::profile::Cut;
use trommi_core::Error;

/// At most so many repairs that failed are remembered as said (a group and a reason each).
const SAID_MAX: usize = 1024;

/// One thing to mend in a group, as a look found it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Job {
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
    /// The look in hand found something to do.
    did: bool,
    /// Something of the look in hand failed or could not be done yet: the next one waits.
    owing: bool,
    /// The longest wait the hub named in the look in hand.
    retry_after: Option<u64>,
    pub(super) retry: Retry,
    /// The own leaves are looked at with the next look (5.2.9): set by the timed round.
    leaves: bool,
    /// Repairs that failed and were said: a group and the reason, once each.
    said: BTreeSet<(GroupId, &'static str)>,
}

impl Tending {
    /// The first look is wanted as soon as the order is at rest.
    pub(super) fn new() -> Self {
        Tending { want: true, ..Default::default() }
    }
    /// A Commit of this device was refused for good: whatever builds it again waits first.
    pub(super) fn refused(&mut self) {
        self.owing = true;
        self.want = true;
        self.looking = true;
    }
    /// The groups changed: looked at now, whatever waited.
    pub(super) fn changed(&mut self) {
        self.want = true;
        self.retry.hurry();
    }
    /// The timed round: a look, and the own leaves with it.
    pub(super) fn timed(&mut self) {
        self.want = true;
        self.leaves = true;
    }
    pub(super) fn wake(&self) -> Option<u64> {
        self.retry.at.filter(|_| self.want)
    }
}

impl Engine {
    pub(super) fn drive_tending(&mut self, ctx: &mut Ctx<'_>) {
        // (the core judges a session against the room states this device holds: the order first; and a sign-in's
        //  own joins of its sessions come before anybody adds it by Welcome)
        if self.joining.is_some() || !self.caught_up || self.pass || !self.todo.is_empty() || self.repair.is_some() || self.tending.asked.is_some() {
            return;
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
        for group in groups {
            if group.session.is_none() || group.archived || !group.leaves.contains(&me) {
                continue;
            }
            if group.is_stale() {
                self.tending.jobs.push_back(Job::Clean { group: group.group });
                continue;
            }
            for device in humans.iter().filter(|device| **device != me && !group.leaves.contains(device)) {
                self.tending.jobs.push_back(Job::Add { group: group.group, device: *device });
            }
        }
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
        let wanted = match job {
            Job::Clean { group } => {
                let Some(opener) = self.to_clean(&group) else { return };
                match opener {
                    Some(opener) => opener,
                    None => return self.clean(ctx, group, None),
                }
            }
            Job::Add { group, device } => {
                if !self.to_add(&group, &device) {
                    return;
                }
                device
            }
        };
        let devices = [wanted];
        self.tending.asked = self.ask(ctx, Route::ClaimKeyPackages { devices: &devices, partial: false }, true, |expect| Asked::Claim { expect, job });
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
                if let Err(Failure::NotNow { retry_after_ms, .. }) = answer {
                    self.tending.retry_after = self.tending.retry_after.max(retry_after_ms);
                }
                self.tending.owing = true;
                return;
            }
        };
        let wanted = match job {
            Job::Add { device, .. } => Some(device),
            Job::Clean { group } => self.to_clean(&group).flatten(),
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
