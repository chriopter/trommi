//! Asking for content keys a human device lacks, and answering such a request (7.1.1).
//!
//! A human device that could not open an envelope for lack of the content key of its (group, epoch), and
//! still lacks that key a while later, asks the human devices for the keys of that group
//! (`POST /v1/requests`, kind `handover`). A human device that holds keys of that group answers with a key
//! handover in the room group: the message of 7.1, addressed to the requester, carrying that group's keys
//! (every group's when the request names none). What is wanted is stored (a device that starts again still
//! asks); when it was asked, and what was answered, is kept in memory only.

use super::{wanted_key, Device};
use crate::error::Error;
use crate::ids::{DeviceId, GroupId};
use crate::mls::group;
use crate::store::{Batch, Storage};
use std::collections::BTreeMap;

/// How long a missing key is waited for before it is asked for: the handover that follows an Add normally
/// arrives within seconds of it.
pub const KEY_REQUEST_GRACE_MS: u64 = 5 * 60_000;
/// The pause after the first request for a group; it doubles with every further one, up to a day.
pub const KEY_REQUEST_FIRST_PAUSE_MS: u64 = 60 * 60_000;
/// The longest pause between two requests for one group.
pub const KEY_REQUEST_MAX_PAUSE_MS: u64 = 24 * 60 * 60_000;
/// At most so many requests at a time (the hub keeps a device's 16 newest).
pub const MAX_KEY_REQUESTS: usize = 4;
/// One answer per requester and group within this time, however often the request is seen (the stream and
/// the hub's list both show it).
pub const KEY_ANSWER_PAUSE_MS: u64 = 60 * 60_000;
/// At most so many epochs of one group are kept as wanted; the request names the group anyway.
const MAX_WANTED_EPOCHS: usize = 4096;

/// A request this device is to post now: `group` is the group whose keys it lacks, `epochs` those epochs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRequest {
    pub group: GroupId,
    pub epochs: Vec<u64>,
}

/// When a group's missing keys were first seen in this run, and asked for.
#[derive(Debug, Default, Clone)]
struct Asked {
    since_ms: u64,
    asked_ms: Option<u64>,
    tries: u32,
}

/// The memory of this module: no state, nothing of it is stored.
#[derive(Debug, Default)]
pub(super) struct KeyWants {
    asked: BTreeMap<GroupId, Asked>,
    answered: BTreeMap<(DeviceId, Option<GroupId>), u64>,
}

impl<S: Storage> Device<S> {
    /// An envelope of `group` written in `epoch` did not open for lack of its content key: a human device
    /// keeps that it wants it.
    pub(super) fn key_missing(
        &mut self,
        batch: &mut Batch,
        group: &GroupId,
        epoch: u64,
        now_ms: u64,
    ) {
        if !self.is_human() || self.memory.wanted.contains(&(*group, epoch)) {
            return;
        }
        let held = self
            .memory
            .wanted
            .range((*group, 0)..=(*group, u64::MAX))
            .count();
        if held >= MAX_WANTED_EPOCHS {
            return;
        }
        self.memory.wanted.insert((*group, epoch));
        batch.put(wanted_key(group, epoch), Vec::new());
        self.key_wants.asked.entry(*group).or_insert(Asked {
            since_ms: now_ms,
            ..Asked::default()
        });
    }

    /// Whether this device still wants the key of `group` at `epoch`: it holds the group, the key is not
    /// there, and the group did not fail its first contact. `None`: not yet (the group has not reached the
    /// epoch for this device).
    fn still_wanted(&self, group: &GroupId, epoch: u64) -> Option<bool> {
        let Some(meta) = self.memory.groups.get(group) else {
            return Some(false);
        };
        if meta.distrusted || meta.removed {
            return Some(false);
        }
        if self.memory.keys.contains_key(&(*group, epoch)) {
            return Some(false);
        }
        let current = group::load(&self.provider, group).ok()?.epoch().as_u64();
        (epoch <= current).then_some(true)
    }

    /// The key requests (7.1.1) to post now, at most [`MAX_KEY_REQUESTS`]: for each group of which an envelope
    /// did not open for lack of its key, once that key is still missing [`KEY_REQUEST_GRACE_MS`] later, and
    /// again after a pause that doubles up to a day while it stays missing. Only a human device asks. A
    /// returned request counts as asked: the client posts it as `{ kind: "handover", group }`. What is no
    /// longer wanted is forgotten.
    pub fn key_requests(&mut self, now_ms: u64) -> Result<Vec<KeyRequest>, Error> {
        self.owner()?;
        if !self.is_human() {
            return Ok(Vec::new());
        }
        // (a request that is not returned was not asked: the timers go back if nothing could be written)
        let before = self.key_wants.asked.clone();
        let due = self.transact(|this, batch| {
            let mut missing: BTreeMap<GroupId, Vec<u64>> = BTreeMap::new();
            let wanted: Vec<(GroupId, u64)> = this.memory.wanted.iter().copied().collect();
            for (group, epoch) in wanted {
                match this.still_wanted(&group, epoch) {
                    Some(true) => missing.entry(group).or_default().push(epoch),
                    Some(false) => {
                        this.memory.wanted.remove(&(group, epoch));
                        batch.delete(wanted_key(&group, epoch));
                    }
                    None => {}
                }
            }
            this.key_wants
                .asked
                .retain(|group, _| missing.contains_key(group));
            let mut due = Vec::new();
            for (group, epochs) in missing {
                // (a want stored by an earlier run is first seen now: its grace begins now)
                let asked = this.key_wants.asked.entry(group).or_insert(Asked {
                    since_ms: now_ms,
                    ..Asked::default()
                });
                let ready = match asked.asked_ms {
                    None => now_ms.saturating_sub(asked.since_ms) >= KEY_REQUEST_GRACE_MS,
                    Some(at) => {
                        let pause = KEY_REQUEST_FIRST_PAUSE_MS
                            .saturating_mul(1u64 << asked.tries.saturating_sub(1).min(16))
                            .min(KEY_REQUEST_MAX_PAUSE_MS);
                        now_ms.saturating_sub(at) >= pause
                    }
                };
                if ready && due.len() < MAX_KEY_REQUESTS {
                    asked.asked_ms = Some(now_ms);
                    asked.tries = asked.tries.saturating_add(1);
                    due.push(KeyRequest { group, epochs });
                }
            }
            Ok(due)
        });
        if due.is_err() {
            self.key_wants.asked = before;
        }
        due
    }

    /// Answers a human device's key request (7.1.1): a key handover in the room group to `recipient`,
    /// carrying every content key this device holds of `group`, or of every group of the room when `group`
    /// is none. Only a human device answers, and only a human device of the newest room state it knows is
    /// answered (an agent device is handed keys only by a human's decision, 5.3.2); `forbidden` otherwise.
    /// A request for the same recipient and group that was answered within [`KEY_ANSWER_PAUSE_MS`], one
    /// from this device itself, and one for keys this device does not hold, send nothing. Returns the
    /// outbox ids.
    pub fn answer_key_request(
        &mut self,
        group: Option<&GroupId>,
        recipient: &DeviceId,
        now_ms: u64,
    ) -> Result<Vec<u64>, Error> {
        self.owner()?;
        let room = self.history()?.newest().clone();
        if !room.is_human(&self.id) || !room.is_human(recipient) {
            return Err(Error::Forbidden);
        }
        if *recipient == self.id {
            return Ok(Vec::new());
        }
        let room_group = GroupId::room(self.memory.record.room.ok_or(Error::RoomBehind)?);
        if let Some(group) = group {
            if group.room_id() != room_group.room_id() {
                return Err(Error::NotFound);
            }
        }
        // A request naming the room group asks for the room group's keys; none names every group.
        let of = group.copied();
        let key = (*recipient, of);
        if self
            .key_wants
            .answered
            .get(&key)
            .is_some_and(|at| now_ms.saturating_sub(*at) < KEY_ANSWER_PAUSE_MS)
        {
            return Ok(Vec::new());
        }
        let ids = self.send_keys(&room_group, of.as_ref(), recipient)?;
        if !ids.is_empty() {
            self.key_wants
                .answered
                .retain(|_, at| now_ms.saturating_sub(*at) < KEY_ANSWER_PAUSE_MS);
            self.key_wants.answered.insert(key, now_ms);
        }
        Ok(ids)
    }
}
