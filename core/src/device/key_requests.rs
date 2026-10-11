//! Asking for content keys a human device lacks, and answering such a request (7.1.1).
//!
//! A human device that could not open an envelope for lack of the content key of its (group, epoch), and
//! still lacks that key a while later, asks the human devices for the keys of that group
//! (`POST /v1/requests`, kind `handover`). A human device that holds keys of that group answers with a key
//! handover in the room group: the message of 7.1, addressed to the requester, carrying that group's keys
//! (every group's when the request names none). Nothing here is state: what was wanted, asked and answered is
//! kept in memory only, and a device that starts again finds what it lacks by reading again.

use super::Device;
use crate::error::Error;
use crate::ids::{DeviceId, GroupId};
use crate::mls::group;
use crate::store::Storage;
use std::collections::{BTreeMap, BTreeSet};

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

/// A request this device is to post now: `group` is the group whose keys it lacks, `epochs` those epochs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRequest {
    pub group: GroupId,
    pub epochs: Vec<u64>,
}

/// What one group lacks, and when it was asked for.
#[derive(Debug, Default)]
struct Want {
    epochs: BTreeSet<u64>,
    since_ms: u64,
    asked_ms: Option<u64>,
    tries: u32,
}

/// The memory of this module: no state, nothing of it is stored.
#[derive(Debug, Default)]
pub(super) struct KeyWants {
    wants: BTreeMap<GroupId, Want>,
    answered: BTreeMap<(DeviceId, Option<GroupId>), u64>,
}

impl<S: Storage> Device<S> {
    /// An envelope of `group` written in `epoch` did not open for lack of its content key.
    pub(super) fn key_missing(&mut self, group: &GroupId, epoch: u64, now_ms: u64) {
        let want = self.key_wants.wants.entry(*group).or_insert_with(|| Want {
            since_ms: now_ms,
            ..Want::default()
        });
        // A bound: a group lists at most so many epochs; the request names the group anyway.
        if want.epochs.len() < 4096 {
            want.epochs.insert(epoch);
        }
    }

    /// The epochs of `group` this device still lacks of those it wanted: the group is held, it reached the
    /// epoch, and the key is not there.
    fn still_missing(&self, group: &GroupId, epochs: &BTreeSet<u64>) -> Vec<u64> {
        let Some(meta) = self.memory.groups.get(group) else {
            return Vec::new();
        };
        if meta.distrusted || meta.removed {
            return Vec::new();
        }
        let Ok(loaded) = group::load(&self.provider, group) else {
            return Vec::new();
        };
        let current = loaded.epoch().as_u64();
        epochs
            .iter()
            .copied()
            .filter(|epoch| *epoch <= current && !self.memory.keys.contains_key(&(*group, *epoch)))
            .collect()
    }

    /// The key requests (7.1.1) to post now, at most [`MAX_KEY_REQUESTS`]: for each group of which an envelope
    /// did not open for lack of its key, once that key is still missing [`KEY_REQUEST_GRACE_MS`] later, and
    /// again after a pause that doubles up to a day while it stays missing. Only a human device asks. A
    /// returned request counts as asked: the client posts it as `{ kind: "handover", group }`.
    pub fn key_requests(&mut self, now_ms: u64) -> Vec<KeyRequest> {
        if self.owner().is_err() || !self.is_human() {
            return Vec::new();
        }
        let groups: Vec<GroupId> = self.key_wants.wants.keys().copied().collect();
        let mut due = Vec::new();
        for group in groups {
            let Some(wanted) = self.key_wants.wants.get(&group) else {
                continue;
            };
            let missing = self.still_missing(&group, &wanted.epochs);
            if missing.is_empty() {
                self.key_wants.wants.remove(&group);
                continue;
            }
            let Some(want) = self.key_wants.wants.get_mut(&group) else {
                continue;
            };
            want.epochs = missing.iter().copied().collect();
            let ready = match want.asked_ms {
                None => now_ms.saturating_sub(want.since_ms) >= KEY_REQUEST_GRACE_MS,
                Some(asked) => {
                    let pause = KEY_REQUEST_FIRST_PAUSE_MS
                        .saturating_mul(1u64 << want.tries.saturating_sub(1).min(16))
                        .min(KEY_REQUEST_MAX_PAUSE_MS);
                    now_ms.saturating_sub(asked) >= pause
                }
            };
            if ready && due.len() < MAX_KEY_REQUESTS {
                want.asked_ms = Some(now_ms);
                want.tries = want.tries.saturating_add(1);
                due.push(KeyRequest {
                    group,
                    epochs: missing,
                });
            }
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
