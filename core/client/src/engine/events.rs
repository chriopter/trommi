//! The stream's events beside the hub's order: what has no change number and is not read again by a page.
//!
//! - **A relayed piece** (7.2, the room group alone): a stroke still being drawn. The protocol core opens it
//!   ([`trommi_core::device::Device::receive_relay`]); one that does not open, or opens to anything but a piece,
//!   is not shown, and nothing else follows from it. It is in no log: a piece missed is missed for good, and the
//!   finished stroke comes in the order.
//! - **A wish** (`POST /v1/requests`). One for content keys (`handover`, 7.1.1) is answered by a human device with
//!   a key handover ([`trommi_core::device::Device::answer_key_request`]), once the order is at rest, so that a
//!   newcomer's Commit is taken before its wish is judged. The hub keeps the wishes seven days: the timed round
//!   reads its list (`GET /v1/requests`) and answers those of the last day that this device has not answered yet
//!   (the ids answered are kept in the engine's record, the newest [`ANSWERED_MAX`]). A wish for a session
//!   (`session`) wants a look at the seats.
//! - **A wish to be let in again** (`readmit`, 3.7): a human device that could not use its Welcome into a session
//!   group asks with a fresh KeyPackage; a human device of that group removes its leaf and adds it again in one
//!   Commit ([`trommi_core::device::Device::readmit_human`]). Several devices hear the same wish, and a second
//!   readmit after the first would cut the device out again. So: only once the order is at rest (a readmit already
//!   in the log is seen first); one device answers at once, the human leaf of the group with the lowest id but the
//!   asker; any other waits [`READMIT_GRACE_MS`] and answers only if no Commit of that group removed and added the
//!   device meanwhile. Two that still meet at the hub are told apart there: the second gets `epoch-taken` and its
//!   Commit is dropped. A wish from the hub's list is taken like one from the stream, once.
//! - **A leaf's refusal of a Commit** (`reject`, 14.7): nothing is done for it here; the person decides (the device
//!   that made the Commit is removed by a person). It is handed to the App as [`super::Alert::Rejected`], once.
//! - **An invite's Request** (12.1): handed to the App ([`super::Report::InviteRequest`]); the invite flow reads
//!   the Request and shows the check code.
//! - **An archived session** (5.2.10): the device archives the group it holds. One the hub archived before this
//!   device held it is found by the hub's list of the room's groups, read once the order rests after a session
//!   group was joined ([`super::upkeep`]).
//! - **A file evicted** (11.3): handed to the App ([`super::Report::FileEvicted`]), which drops its cached bytes.

use super::{Asked, Ctx, Engine, Report};
use crate::app::io::IoId;
use crate::app::LogLevel;
use crate::hub::{Answer, Failure, Route, WishRow, WishRowKind};
use std::collections::VecDeque;
use trommi_core::ids::{DeviceId, GroupId};
use trommi_core::Error;

/// At most so many wishes for keys wait for the order to come to rest; the oldest gives way.
pub const WISHES_MAX: usize = 64;
/// The ids of so many answered wishes are kept (across a restart, in the record).
pub const ANSWERED_MAX: usize = 256;
/// A wish older than this (by the hub's clock) is passed over: a device that still lacks the keys asked again.
pub const WISH_FRESH_MS: u64 = 86_400_000;
/// A readmit wish is answered by any device but the first one after this long.
pub const READMIT_GRACE_MS: u64 = 300_000;
/// At most so many readmits seen in the log are remembered.
const READMITTED_MAX: usize = 256;

#[derive(Default)]
pub(super) struct Wishes {
    /// Wishes for keys that wait for the order to come to rest.
    queue: VecDeque<WishRow>,
    /// The ids of wishes this device answered with a handover, oldest first.
    pub(super) answered: VecDeque<u64>,
    /// The hub's list in flight.
    asked: Option<IoId>,
    /// Wishes to be let in again: the wish, and from when this device may answer it.
    readmits: VecDeque<(WishRow, u64)>,
    /// Session groups and devices a Commit removed and added again, as this run saw them, with the engine's time.
    readmitted: VecDeque<(GroupId, DeviceId, u64)>,
}

impl Wishes {
    fn mark_answered(&mut self, id: u64) {
        if self.answered.contains(&id) {
            return;
        }
        if self.answered.len() >= ANSWERED_MAX {
            self.answered.pop_front();
        }
        self.answered.push_back(id);
    }

    pub(super) fn restore(&mut self, answered: &[u64]) {
        self.answered = answered.iter().rev().take(ANSWERED_MAX).rev().copied().collect();
    }
}

impl Engine {
    /// A piece the hub only passed on: shown if the core opens it.
    pub(super) fn on_relay(&mut self, ctx: &mut Ctx<'_>, group: GroupId, sender: DeviceId, message: &[u8]) {
        if sender == self.device.id() || !group.is_room() {
            return;
        }
        match self.device.receive_relay(&group, message, ctx.now) {
            Ok(Some(received)) => self.report(Report::Relay(Box::new(received))),
            Ok(None) => {}
            Err(e) => {
                self.local(ctx, &e);
                if self.stopped.is_none() {
                    ctx.log(LogLevel::Debug, format!("relay: not shown ({})", e.code()));
                }
            }
        }
    }

    /// A wish of another device, from the stream or from the hub's list.
    pub(super) fn on_wish(&mut self, ctx: &mut Ctx<'_>, row: WishRow) {
        let seen = |w: &Wishes, id: u64| w.answered.contains(&id) || w.queue.iter().any(|q| q.id == id) || w.readmits.iter().any(|(q, _)| q.id == id);
        if seen(&self.wishes, row.id) {
            return;
        }
        match row.kind {
            WishRowKind::Handover => {
                let w = &mut self.wishes;
                if !self.device.is_human() || row.device == self.device.id() || w.answered.contains(&row.id) || w.queue.iter().any(|q| q.id == row.id) {
                    return;
                }
                if w.queue.len() >= WISHES_MAX {
                    w.queue.pop_front();
                }
                w.queue.push_back(row);
            }
            // (an agent asks for a session: the seat rule looks)
            WishRowKind::Session => self.tending.want = true,
            WishRowKind::Readmit => {
                let (Some(group), Some(_)) = (row.group, row.key_package.as_ref()) else { return };
                let me = self.device.id();
                if !self.device.is_human() || row.device == me || group.is_room() {
                    return;
                }
                let Ok(held) = self.device.group(&group) else { return };
                if !held.leaves.contains(&me) || held.archived {
                    return;
                }
                // the first human leaf but the asker answers at once, the others after the grace
                let first = held.leaves.iter().filter(|d| **d != row.device && self.device_is_human(d)).min() == Some(&me);
                let from = if first { ctx.now } else { ctx.now + READMIT_GRACE_MS };
                let w = &mut self.wishes;
                if w.readmits.len() >= WISHES_MAX {
                    w.readmits.pop_front();
                }
                w.readmits.push_back((row, from));
            }
            WishRowKind::Reject => {
                let (Some(group), Some(n)) = (row.group, row.n) else { return };
                if !self.device.is_human() || row.device == self.device.id() {
                    return;
                }
                self.wishes.mark_answered(row.id);
                self.report(Report::Alert(super::Alert::Rejected { group, n, from: row.device, committer: row.committer }));
            }
        }
    }

    fn device_is_human(&self, device: &DeviceId) -> bool {
        self.device.room_history().is_some_and(|h| h.newest().is_human(device))
    }

    /// A Commit of a session group was taken: one that removed and added the same device is a readmit.
    pub(super) fn note_readmit(&mut self, ctx: &mut Ctx<'_>, group: &GroupId, adds: &[DeviceId], removes: &[DeviceId]) {
        for device in adds.iter().filter(|d| removes.contains(d)) {
            let w = &mut self.wishes;
            if w.readmitted.len() >= READMITTED_MAX {
                w.readmitted.pop_front();
            }
            w.readmitted.push_back((*group, *device, ctx.now));
        }
    }

    /// When the next readmit wish may be answered.
    pub(super) fn readmit_wake(&self) -> Option<u64> {
        self.wishes.readmits.iter().map(|(_, from)| *from).min()
    }

    fn drive_readmits(&mut self, ctx: &mut Ctx<'_>) {
        let due: Vec<(WishRow, u64)> = {
            let w = &mut self.wishes;
            let (due, wait): (VecDeque<_>, VecDeque<_>) = w.readmits.drain(..).partition(|(_, from)| *from <= ctx.now);
            w.readmits = wait;
            due.into_iter().collect()
        };
        for (row, from) in due {
            let (Some(group), Some(key_package)) = (row.group, row.key_package.as_ref()) else { continue };
            // (one that waited: answered only if nobody let the device in again meanwhile)
            let queued = from.saturating_sub(READMIT_GRACE_MS);
            if self.wishes.readmitted.iter().any(|(g, d, at)| *g == group && *d == row.device && *at >= queued) {
                self.wishes.mark_answered(row.id);
                continue;
            }
            match self.device.readmit_human(&group, &row.device, key_package, ctx.now) {
                Ok(_) => {
                    self.wishes.mark_answered(row.id);
                    self.wishes.readmitted.push_back((group, row.device, ctx.now));
                }
                // (the device is no human leaf of it now, or the hub holds its envelope beyond the Cut: not now)
                Err(Error::BadCommit | Error::Forbidden | Error::NotFound) => self.wishes.mark_answered(row.id),
                Err(e) => {
                    self.local(ctx, &e);
                    if self.stopped.is_some() {
                        return;
                    }
                    self.wishes.mark_answered(row.id);
                }
            }
        }
    }

    /// The wishes for keys that wait, answered once the order is at rest.
    pub(super) fn drive_wishes(&mut self, ctx: &mut Ctx<'_>) {
        if (self.wishes.queue.is_empty() && self.wishes.readmits.is_empty()) || !self.caught_up || self.pass || !self.todo.is_empty() || self.repair.is_some() {
            return;
        }
        self.drive_readmits(ctx);
        if self.stopped.is_some() {
            return;
        }
        while let Some(row) = self.wishes.queue.pop_front() {
            match self.device.answer_key_request(row.group.as_ref(), &row.device, ctx.now) {
                Ok(made) if !made.is_empty() => self.wishes.mark_answered(row.id),
                // (nothing to hand over now, answered within the hour, or no human device of the room as this
                //  device knows it: the hub's list is read again with the next round)
                Ok(_) | Err(Error::Forbidden) => {}
                Err(e) => {
                    self.local(ctx, &e);
                    if self.stopped.is_some() {
                        return;
                    }
                }
            }
        }
    }

    /// The timed round's look at the hub's list of wishes (a human device's alone).
    pub(super) fn ask_wishes(&mut self, ctx: &mut Ctx<'_>) {
        if !self.device.is_human() || self.wishes.asked.is_some() {
            return;
        }
        self.wishes.asked = self.ask(ctx, Route::Wishes, true, Asked::Wishes);
    }

    pub(super) fn on_wishes(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>) {
        self.wishes.asked = None;
        // (a list not read is read with the next round)
        let Ok(Answer::Wishes(rows)) = answer else { return };
        for row in rows {
            let fresh = row.at.is_some_and(|at| at.saturating_add(WISH_FRESH_MS) >= ctx.now);
            if fresh && row.kind != WishRowKind::Session {
                self.on_wish(ctx, row);
            }
        }
    }

    /// The hub archived a session group: the device archives it, if it holds it.
    pub(super) fn on_archived(&mut self, ctx: &mut Ctx<'_>, group: GroupId) {
        if self.archive_held(ctx, &group) {
            self.report(Report::Groups);
            self.tending.changed();
        }
    }

    /// Archives a session group this device holds live. True when it did.
    pub(super) fn archive_held(&mut self, ctx: &mut Ctx<'_>, group: &GroupId) -> bool {
        if group.is_room() || !self.device.group(group).is_ok_and(|held| !held.archived) {
            return false;
        }
        match self.device.archive(group) {
            Ok(()) => true,
            Err(e) => {
                self.local(ctx, &e);
                false
            }
        }
    }
}
