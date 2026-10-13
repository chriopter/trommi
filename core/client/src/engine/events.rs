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
//!   (`session`) wants a look at the seats. `readmit` and `reject` are not answered here yet.
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

#[derive(Default)]
pub(super) struct Wishes {
    /// Wishes for keys that wait for the order to come to rest.
    queue: VecDeque<WishRow>,
    /// The ids of wishes this device answered with a handover, oldest first.
    pub(super) answered: VecDeque<u64>,
    /// The hub's list in flight.
    asked: Option<IoId>,
}

impl Wishes {
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
    pub(super) fn on_wish(&mut self, row: WishRow) {
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
            WishRowKind::Readmit | WishRowKind::Reject => {}
        }
    }

    /// The wishes for keys that wait, answered once the order is at rest.
    pub(super) fn drive_wishes(&mut self, ctx: &mut Ctx<'_>) {
        if self.wishes.queue.is_empty() || !self.caught_up || self.pass || !self.todo.is_empty() || self.repair.is_some() {
            return;
        }
        while let Some(row) = self.wishes.queue.pop_front() {
            match self.device.answer_key_request(row.group.as_ref(), &row.device, ctx.now) {
                Ok(made) if !made.is_empty() => {
                    if self.wishes.answered.len() >= ANSWERED_MAX {
                        self.wishes.answered.pop_front();
                    }
                    self.wishes.answered.push_back(row.id);
                }
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
            if row.kind == WishRowKind::Handover && row.at.is_some_and(|at| at.saturating_add(WISH_FRESH_MS) >= ctx.now) {
                self.on_wish(row);
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
