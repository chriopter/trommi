//! The recovery of a room with its code (8.7): the person holds the code (opened with the Emergency Kit) and no
//! device of theirs; every other human device is removed, the code is replaced, and the account gets new copies
//! of the new code (a new password and a new kit). As the web's `room.ts joinWithCode({ recover: true })`.
//!
//! It runs inside the join with the code ([`super::room_join`]), signed in with the code's key, and reads more
//! than a join: besides the room group, every live session group from its founding, because the recovery removes
//! the other human devices from each of them.
//!
//! 1. The sealed keys, the room group's GroupInfos and the live sessions, as for a join.
//! 2. Each session group's founding and current GroupInfo; every group's Commits below its current epoch.
//! 3. Two walks of one reading, in the hub's order: the device's own check (for `recover_checked`) and the plan's
//!    (`RoomCheck`): the plan makes the new code and names the devices that go.
//! 4. The chains of the devices that go (their Cuts end their chains for everyone, 9.0.10), in the hub's order.
//! 5. The new code's copies for the account are sealed (the App's closure: the new password and the new kit),
//!    BEFORE the recovery is opened: the hub locks the room for ten minutes from then on.
//! 6. `POST /v1/rooms/{room}/recovery`, then `recover_checked` builds the entries (a Commit per group and the
//!    finish), posted one by one with the recovery's id. Any failure after the opening gives the room back
//!    (`DELETE …/recovery/{id}`); the device's state changes only when the hub accepted the finish.

use super::room_join::{Joiner, Read, RoomJoinError, RoomJoinStep};
use crate::hub::OutboxExtra;
use super::*;
use std::collections::BTreeMap;
use trommi_core::crypto::Secret as Key;
use trommi_core::device::ServedEnvelope;
use trommi_core::recovery::{self, PlacedCommit, Replacement, RoomCheck, ServedCommit, ServedEnd, ServedStart};

/// Seals the new code into the account's new copies (the body part of the finish); `room` is the room's.
pub type Copies = Box<dyn FnOnce(&Key<32>, &RoomId) -> Result<Vec<u8>, &'static str> + Send>;

/// Commits handed to the walks in one slice; halved when the core says `too-large`.
const SLICE: usize = 200;
/// The most bytes of Commits and chains a recovery holds.
const BYTES_MAX: usize = 256 << 20;

/// The recovery while it runs (beside the join's own reading).
pub struct Recovering {
    copies: Option<Copies>,
    /// Each session group's founding and current GroupInfo, and where its log was read to.
    founding: BTreeMap<GroupId, Vec<u8>>,
    current: BTreeMap<GroupId, (u64, Vec<u8>)>,
    logs: BTreeMap<GroupId, (u64, bool)>,
    commits: Vec<LogItem>,
    bytes: usize,
    checked: bool,
    replacement: Option<Replacement>,
    removals: Vec<(GroupId, Vec<DeviceId>)>,
    /// The chain read now: (removal, device), its `after`.
    chain_at: (usize, usize, u64),
    chains: Vec<(u64, Vec<u8>, Option<Error>)>,
    recovery_id: Option<[u8; 16]>,
    /// The entries to post, in order; the one in flight.
    entries: Vec<u64>,
    posting: Option<u64>,
    pub(super) finished: bool,
}

impl Recovering {
    pub fn new(copies: Copies) -> Self {
        Recovering {
            copies: Some(copies),
            founding: BTreeMap::new(),
            current: BTreeMap::new(),
            logs: BTreeMap::new(),
            commits: Vec::new(),
            bytes: 0,
            checked: false,
            replacement: None,
            removals: Vec::new(),
            chain_at: (0, 0, 0),
            chains: Vec::new(),
            recovery_id: None,
            entries: Vec::new(),
            posting: None,
            finished: false,
        }
    }

    /// The recovery read again from the start (the room moved on under it); none once its copies were used.
    pub(super) fn again(mut self) -> Option<Self> {
        self.copies.take().map(Recovering::new)
    }

    /// Whether an outbox entry of the recovery is in flight.
    pub(super) fn posting(&self) -> Option<u64> {
        self.posting
    }

}

fn of(e: &Error) -> RoomJoinError {
    RoomJoinError::of(e)
}

impl Engine {
    /// The recovery's next read or step, once the sealed keys, the room group's GroupInfos and the sessions are
    /// read.
    pub(super) fn recover_next(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner, sessions: Vec<GroupId>, current: (u64, Vec<u8>), founding: Vec<u8>, anchor: Vec<u8>) -> Result<(), RoomJoinError> {
        let room = self.room;
        let room_group = GroupId::room(room);
        let r = j.recover.as_mut().ok_or(RoomJoinError::Device("internal"))?;
        // 2. the session groups' GroupInfos, and every group's Commits
        for group in &sessions {
            if !r.founding.contains_key(group) {
                let group = *group;
                return self.room_ask(ctx, j, Route::GroupInfo { group: &group, epoch: Some(0) }, Read::Session(group, true));
            }
            if !r.current.contains_key(group) {
                let group = *group;
                return self.room_ask(ctx, j, Route::GroupInfo { group: &group, epoch: None }, Read::Session(group, false));
            }
        }
        for group in std::iter::once(&room_group).chain(&sessions) {
            let (after, done) = r.logs.get(group).copied().unwrap_or((0, false));
            if !done {
                let group = *group;
                return self.room_ask(ctx, j, Route::GroupLog { group: &group, after, limit: LOG_PAGE, commits_only: true }, Read::SessionLog(group));
            }
        }
        // 3. both walks, in the hub's order
        let foundings: Vec<Vec<u8>> = sessions.iter().map(|g| r.founding.get(g).cloned().unwrap_or_default()).collect();
        let currents: Vec<Vec<u8>> = sessions.iter().map(|g| r.current.get(g).map(|(_, c)| c.clone()).unwrap_or_default()).collect();
        let founding_refs: Vec<&[u8]> = foundings.iter().map(Vec::as_slice).collect();
        let current_refs: Vec<&[u8]> = currents.iter().map(Vec::as_slice).collect();
        let end = ServedEnd { current: &current.1, sessions: &current_refs, rows: &j.rows, links: &j.links };
        if !r.checked {
            j.step = RoomJoinStep::Checking;
            let start = ServedStart { room, founding: &founding, anchor: &anchor, rows: &j.rows, sessions: &founding_refs };
            self.device.code_check_start(&j.keys, &start).map_err(|e| of(&e))?;
            let mut plan = RoomCheck::start(&j.keys, &start).map_err(|e| of(&e))?;
            r.commits.sort_by_key(|c| c.change);
            let (mut at, mut size) = (0, SLICE);
            while at < r.commits.len() {
                let slice: Vec<PlacedCommit<'_>> = r.commits[at..(at + size).min(r.commits.len())]
                    .iter()
                    .map(|c| PlacedCommit { group: c.group, commit: ServedCommit { change: c.change, commit: &c.bytes, recovery_auth: c.recovery_auth.as_deref() } })
                    .collect();
                let device = self.device.code_check_slice(&slice);
                match device.and_then(|()| plan.slice(&slice)) {
                    Ok(()) => at += slice.len(),
                    Err(Error::TooLarge) if size > 1 => size /= 2,
                    Err(e) => return Err(of(&e)),
                }
            }
            let checked = plan.finish(&j.keys, &end).map_err(|e| of(&e))?;
            let history = checked.observer.history().ok_or(RoomJoinError::Device("internal"))?;
            let replacement = j.keys.replace(&mut SystemEntropy, &room, history).map_err(|e| of(&e))?;
            r.removals = recovery::removals(&checked).map_err(|e| of(&e))?;
            r.replacement = Some(replacement);
            r.commits = Vec::new();
            r.checked = true;
        }
        // 4. the chains of the devices that go
        loop {
            let (removal, device, after) = r.chain_at;
            let Some((group, devices)) = r.removals.get(removal) else { break };
            let Some(sender) = devices.get(device) else {
                r.chain_at = (removal + 1, 0, 0);
                continue;
            };
            let (group, sender) = (*group, *sender);
            return self.room_ask(ctx, j, Route::Chain { group: &group, sender: &sender, after, limit: CHAIN_PAGE }, Read::Chain);
        }
        // 5. the account's copies, then 6. the recovery opened
        if r.recovery_id.is_none() {
            return self.room_ask(ctx, j, Route::OpenRecovery { room: &room }, Read::Open);
        }
        if r.entries.is_empty() && !r.finished {
            j.step = RoomJoinStep::Joining;
            self.room_status(j);
            let r = j.recover.as_mut().ok_or(RoomJoinError::Device("internal"))?;
            let replacement = r.replacement.as_ref().ok_or(RoomJoinError::Device("internal"))?;
            let copies = r.copies.take().ok_or(RoomJoinError::Device("internal"))?;
            let account = copies(&replacement.code, &room).map_err(RoomJoinError::Device)?;
            r.chains.sort_by_key(|(change, _, _)| *change);
            let chains: Vec<ServedEnvelope<'_>> = r.chains.iter().map(|(change, bytes, void)| ServedEnvelope { bytes, change: *change, void_code: void.as_ref() }).collect();
            let built = self.device.recover_checked(&j.keys, &end, replacement, &chains, &account, ctx.now).map_err(|e| of(&e))?;
            if built.missing_link.is_some() {
                self.report(Report::Alert(Alert::MissingLink));
            }
            let r = j.recover.as_mut().ok_or(RoomJoinError::Device("internal"))?;
            r.entries = built.outbox;
            r.chains = Vec::new();
            if r.entries.is_empty() {
                return Err(RoomJoinError::Device("internal"));
            }
        }
        self.recover_post(ctx, j)
    }

    /// Posts the next entry of the recovery, with the recovery's id.
    fn recover_post(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner) -> Result<(), RoomJoinError> {
        let room = self.room;
        let r = j.recover.as_mut().ok_or(RoomJoinError::Device("internal"))?;
        let Some(&id) = r.entries.first() else { return Ok(()) };
        let Some(entry) = self.device.outbox().into_iter().find(|e| e.id == id) else {
            return Err(RoomJoinError::Device("internal"));
        };
        let extra = OutboxExtra { recovery_id: r.recovery_id.map(|i| i.to_vec()), ..Default::default() };
        let (kind, group) = (entry.kind, entry.group);
        self.posting = self.ask(ctx, Route::Outbox { room: &room, entry: &entry, extra: Some(&extra) }, true, |expect| Asked::Post { expect, entry: id, kind, group });
        j.asked = self.posting;
        r.posting = Some(id);
        j.posted = true;
        j.asked.map(|_| ()).ok_or(RoomJoinError::Device("bad-format"))
    }

    /// The hub's answer to an entry of the recovery.
    pub(super) fn recover_posted(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner, entry: u64, answer: Result<Answer, Failure>, unauthorised: bool) -> Result<(), RoomJoinError> {
        let r = j.recover.as_mut().ok_or(RoomJoinError::Device("internal"))?;
        r.posting = None;
        match answer {
            Ok(Answer::Outbox(taken)) => {
                j.retry.ok();
                match self.device.outbox_accepted(entry, trommi_core::device::Accepted { change: taken.change }) {
                    Ok(()) | Err(Error::NotFound) => {}
                    Err(e) => return Err(of(&e)),
                }
                r.entries.retain(|e| *e != entry);
                if r.entries.is_empty() {
                    r.finished = true;
                    return Ok(());
                }
                self.recover_post(ctx, j)
            }
            Err(_) if unauthorised => Ok(()),
            Err(Failure::Refused { code: Some(code), .. }) => {
                let code = Error::from_code(&code).unwrap_or(Error::BadFormat);
                let _ = self.device.outbox_refused(entry, &code);
                Err(RoomJoinError::Refused(code.code()))
            }
            Err(failure) => {
                self.room_wait(ctx, j, &failure);
                Ok(())
            }
            Ok(_) => Err(RoomJoinError::Refused("bad-format")),
        }
    }

    /// A read of the recovery answered (`Session`, `SessionLog`, `Chain`, `Open`).
    pub(super) fn recover_read(&mut self, j: &mut Joiner, what: Read, answer: Answer) -> Result<(), RoomJoinError> {
        let r = j.recover.as_mut().ok_or(RoomJoinError::Device("internal"))?;
        match (what, answer) {
            (Read::Session(group, true), Answer::GroupInfo { group_info, .. }) => {
                r.founding.insert(group, group_info);
            }
            (Read::Session(group, false), Answer::GroupInfo { epoch, group_info }) => {
                r.current.insert(group, (epoch, group_info));
            }
            (Read::SessionLog(group), Answer::GroupLog { items, more }) => {
                let (after, _) = r.logs.get(&group).copied().unwrap_or((0, false));
                let upto = if group.is_room() { j.current.as_ref().map_or(0, |(e, _)| *e) } else { r.current.get(&group).map_or(0, |(e, _)| *e) };
                let mut last = after;
                for item in items {
                    let ChangeItem::Commit(c) = item else { continue };
                    last = last.max(c.n);
                    if c.n > after && c.epoch < upto && c.group == group {
                        r.bytes += c.bytes.len();
                        r.commits.push(c);
                    }
                }
                if r.bytes > BYTES_MAX {
                    return Err(RoomJoinError::Refused("too-large"));
                }
                r.logs.insert(group, (last, !more || last == after));
            }
            (Read::Chain, Answer::Chain { items, more }) => {
                let (removal, device, after) = r.chain_at;
                let mut last = after;
                for link in items {
                    last = last.max(link.seq);
                    r.bytes += link.item.envelope.len();
                    let void = link.item.void_code.as_deref().map(|c| Error::from_code(c).unwrap_or(Error::HubVoidedOther));
                    r.chains.push((link.item.change, link.item.envelope, void));
                }
                if r.bytes > BYTES_MAX {
                    return Err(RoomJoinError::Refused("too-large"));
                }
                r.chain_at = if more && last > after { (removal, device, last) } else { (removal, device + 1, 0) };
            }
            (Read::Open, Answer::Recovery { id, .. }) => r.recovery_id = Some(id),
            _ => return Err(RoomJoinError::Refused("bad-format")),
        }
        Ok(())
    }

    /// Gives the room back after a recovery that was opened and not finished (best effort, nothing waits).
    pub(super) fn recover_drop(&mut self, ctx: &mut Ctx<'_>, j: &Joiner) {
        let Some(r) = j.recover.as_ref() else { return };
        let (Some(id), false) = (r.recovery_id, r.finished) else { return };
        let room = self.room;
        let _ = self.ask(ctx, Route::DropRecovery { room: &room, recovery: &id }, true, Asked::Told);
    }
}
