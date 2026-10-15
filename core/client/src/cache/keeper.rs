//! The App's cache: read once at the start, before the engine asks the hub anything, and written behind the board.
//!
//! # Writing
//!
//! What a batch of the engine's reports touched (`Change`) marks its records due. They go out at the end of the
//! event, unless a write is under way: then they wait for it to end, and everything that came meanwhile goes in one
//! write (the store's own pace is the batching; no timer). A write is three steps, each one `Put` that waits for the
//! answer of the one before:
//!
//! 1. `meta` says `writing`: from here until step 3 the records may be a mix of two states.
//! 2. The due records, each as the model holds it now (or deleted), and the Desk's last list.
//! 3. `meta` names the device's store revision the board stood at in step 2, and no longer says `writing`.
//!
//! A step's `Put` is behind every `DeviceApply` the App handed out before it, and a `DeviceApply` is a barrier in
//! the shell, so the records never hold anything the device has not stored: the revision in `meta` is one the
//! device reached (or passed). When nothing but the revision moved, step 3 goes alone.
//!
//! # Reading
//!
//! `meta` and the Desk's list are read by key, the records by one scan of `c1/<room>/model/` (files are not read at
//! the start). The cache is taken when `meta` is this format's, of this device, not `writing`, and at a revision
//! the stored device reached:
//!
//! - **at the device's revision**: it is the board as the App last had it;
//! - **behind it** (the process died after the device stored something and before the cache was written): every
//!   record is still true, but the device may have taken chat lines since that it will not hand over again (a
//!   replay). So every timeline's window is closed and its items are dropped; a timeline is read from the hub when
//!   it is opened, and the hub's Desk brings the cards;
//! - **anything else** (no `meta` beside records, another device, a newer revision than the device's, `writing`,
//!   another format, a value that does not read): the cache is not used, its records are deleted, and the board is
//!   built from the hub as on a first start.
//!
//! A cache that cannot be read at all (the store failed) is left as it is and not written in this run: writing due
//! records over records not read would mix two states.
//!
//! The cache holds the board decrypted: titles, chat lines, answers. Never keys, the recovery code or tokens (the
//! device space and `Secret` hold those). It stays on the device; nothing of it is sent anywhere.

use super::layout::{self, Rec};
use crate::app::io::{Entry, IoId, IoResult, StoreAnswer, StoreOp};
use crate::app::{Bytes, Effect};
use crate::model::change::Change;
use crate::model::types::ItemKey;
use crate::model::Model;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The layout's revision inside the format `c1`: a `meta` of another one is not read.
// Older snapshots did not record the group of a displayed device register.
const VERSION: u32 = 2;
/// A write that failed is tried again after this long.
const RETRY_MS: u64 = 5_000;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Meta {
    v: u32,
    /// The device (hex) whose board this is.
    device: String,
    /// The device's store revision the records stand at.
    rev: u64,
    /// A write of the records began and did not end.
    #[serde(default)]
    writing: bool,
}

/// The hub's Desk as last read: what it listed as open (by id, the change of its version), at which change.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    pub at: u64,
    pub open: Vec<(String, u64)>,
}

/// What the start's read of the cache gave the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Nothing was kept (or the store did not answer): the board starts empty.
    Empty,
    /// The cache was not this device's or not whole: deleted, the board starts empty.
    Dropped,
    /// The board is the cached one; `behind`: the device stored more since, the timelines' items were dropped.
    Loaded { behind: bool, listing: Option<Listing> },
}

/// Whose a store answer was.
#[derive(Debug, PartialEq, Eq)]
pub enum Took {
    /// Not the cache's.
    No,
    Mine,
    /// The start's read is through.
    Read(Read),
}

enum State {
    Reading {
        meta: IoId,
        scan: IoId,
        values: Option<Vec<Option<Bytes>>>,
        entries: Option<Vec<Entry>>,
        failed: bool,
    },
    Open,
    /// Not written in this run.
    Off,
}

enum Flight {
    Idle,
    Mark(IoId),
    Records(IoId, Batch),
    Stamp(IoId, u64),
}

struct Batch {
    recs: Vec<Rec>,
    drops: Vec<String>,
    listing: bool,
    rev: u64,
}

pub struct Keeper {
    room: String,
    device: String,
    state: State,
    /// The device's store revision the board stands at: that of the last `DeviceApply` handed out.
    rev: u64,
    /// The revision `meta` names in the store; none while it says `writing` or names none.
    stored: Option<u64>,
    dirty: BTreeSet<Rec>,
    /// Timelines whose items are to be held against what was written (an item may have gone).
    windows: BTreeSet<String>,
    /// The items written per timeline.
    written: BTreeMap<String, BTreeSet<ItemKey>>,
    /// Keys to delete with the next records.
    drops: BTreeSet<String>,
    listing: Option<Listing>,
    listing_due: bool,
    due: Option<u64>,
    flight: Flight,
}

fn key(room: &str, tail: &str) -> String {
    format!("{}{}/{}", super::room_prefix(room), layout::PART, tail)
}

impl Keeper {
    /// Asks the store for the room's cache; the App starts the engine once [`Keeper::on_store`] says it is read.
    pub fn open(room: &str, device: &str, revision: u64, next_id: &mut u64, out: &mut Vec<Effect>) -> Keeper {
        let mut id = || {
            let id = IoId(*next_id);
            *next_id += 1;
            id
        };
        let (meta, scan) = (id(), id());
        out.push(Effect::Store {
            id: meta,
            op: StoreOp::Get {
                keys: vec![super::key(room, super::part::META, None), super::key(room, super::part::DESK, None)],
            },
        });
        out.push(Effect::Store {
            id: scan,
            op: StoreOp::Scan {
                prefix: format!("{}{}/", super::room_prefix(room), layout::PART),
            },
        });
        Keeper {
            room: room.to_string(),
            device: device.to_string(),
            state: State::Reading { meta, scan, values: None, entries: None, failed: false },
            rev: revision,
            stored: None,
            dirty: BTreeSet::new(),
            windows: BTreeSet::new(),
            written: BTreeMap::new(),
            drops: BTreeSet::new(),
            listing: None,
            listing_due: false,
            due: None,
            flight: Flight::Idle,
        }
    }

    /// The start's read is through: the engine may start.
    pub fn ready(&self) -> bool {
        !matches!(self.state, State::Reading { .. })
    }

    /// The device's store revision the board stands at now.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    /// The revision the stored cache names, once a write of it ended (or the one read at the start).
    pub fn stored(&self) -> Option<u64> {
        self.stored
    }

    /// An answer to a store op: whether it was one of the cache's, and once the start's read is through what it gave
    /// (the model then holds it).
    pub fn on_store(&mut self, id: IoId, result: &IoResult<StoreAnswer>, model: &mut Model, now: u64, next_id: &mut u64, out: &mut Vec<Effect>) -> Took {
        if let State::Reading { meta, scan, values, entries, failed } = &mut self.state {
            if id == *meta || id == *scan {
                match (result, id == *meta) {
                    (IoResult::Ok(StoreAnswer::Values { values: got }), true) if got.len() == 2 => *values = Some(got.clone()),
                    (IoResult::Ok(StoreAnswer::Entries { entries: got }), false) => *entries = Some(got.clone()),
                    _ => *failed = true,
                }
                let through = *failed || (values.is_some() && entries.is_some());
                if through {
                    let State::Reading { values, entries, failed, .. } = std::mem::replace(&mut self.state, State::Open) else {
                        unreachable!()
                    };
                    return Took::Read(if failed {
                        self.state = State::Off;
                        Read::Empty
                    } else {
                        self.take(values.unwrap_or_default(), entries.unwrap_or_default(), model)
                    });
                }
                return Took::Mine;
            }
            return Took::No;
        }
        let ok = matches!(result, IoResult::Ok(StoreAnswer::Done));
        match std::mem::replace(&mut self.flight, Flight::Idle) {
            Flight::Mark(at) if at == id => {
                if ok {
                    self.stored = None;
                    self.records(model, next_id, out);
                } else {
                    self.failed(now);
                }
            }
            Flight::Records(at, batch) if at == id => {
                if ok {
                    let rev = batch.rev;
                    let id = io(next_id);
                    out.push(self.meta_put(id, rev, false));
                    self.flight = Flight::Stamp(id, rev);
                } else {
                    self.dirty.extend(batch.recs);
                    self.drops.extend(batch.drops);
                    self.listing_due |= batch.listing;
                    self.failed(now);
                }
            }
            Flight::Stamp(at, rev) if at == id => {
                if ok {
                    self.stored = Some(rev);
                    self.step(now, next_id, out);
                } else {
                    self.failed(now);
                }
            }
            other => {
                self.flight = other;
                return Took::No;
            }
        }
        Took::Mine
    }

    /// The start's read, into the model.
    fn take(&mut self, values: Vec<Option<Bytes>>, entries: Vec<Entry>, model: &mut Model) -> Read {
        let prefix = key(&self.room, "");
        let meta: Option<Meta> = values.first().cloned().flatten().and_then(|b| serde_json::from_slice(&b.0).ok());
        let listing: Option<Listing> = values.get(1).cloned().flatten().and_then(|b| serde_json::from_slice(&b.0).ok());
        let usable = meta.as_ref().is_some_and(|m| m.v == VERSION && m.device == self.device && !m.writing && m.rev <= self.rev);
        let mut records = Vec::with_capacity(entries.len());
        let mut unknown = false;
        for e in &entries {
            match e.key.strip_prefix(&prefix).and_then(Rec::of_tail) {
                Some(rec) => records.push((rec, e.value.0.as_slice())),
                None => unknown = true,
            }
        }
        let loaded = usable && !unknown && layout::snapshot_text(&records).is_some_and(|text| model.load_text(&text).is_ok());
        if !loaded {
            if meta.is_none() && entries.is_empty() {
                return Read::Empty;
            }
            // (the App starts the board anew: what the read put into it goes with the cache)
            self.drops.extend(entries.into_iter().map(|e| e.key));
            self.drops.insert(super::key(&self.room, super::part::DESK, None));
            return Read::Dropped;
        }
        let meta = meta.expect("a usable cache has its meta");
        let behind = meta.rev < self.rev;
        for (rec, _) in &records {
            if let Rec::Item(timeline, item) = rec {
                self.written.entry(timeline.clone()).or_default().insert(item.clone());
            }
        }
        if behind {
            for (id, t) in model.timelines.iter_mut() {
                t.items.clear();
                t.window_open = false;
                t.loaded_down_to = None;
                t.has_more = true;
                self.dirty.insert(Rec::Timeline(id.clone()));
                self.windows.insert(id.clone());
            }
        } else {
            self.stored = Some(meta.rev);
        }
        self.listing = listing.clone();
        Read::Loaded { behind, listing }
    }

    /// What a batch of the engine's reports touched.
    pub fn note(&mut self, change: &Change) {
        if matches!(self.state, State::Off) || change.is_empty() {
            return;
        }
        let d = &mut self.dirty;
        d.extend(change.cards.iter().cloned().map(Rec::Card));
        d.extend(change.sessions.iter().cloned().map(Rec::Session));
        d.extend(change.permissions.iter().cloned().map(Rec::Permission));
        d.extend(change.notes.iter().cloned().map(Rec::Note));
        d.extend(change.published.iter().cloned().map(Rec::Published));
        d.extend(change.registers.iter().cloned().map(Rec::Register));
        d.extend(change.timelines.iter().cloned().map(Rec::Timeline));
        for (timeline, items) in &change.items {
            d.extend(items.iter().map(|k| Rec::Item(timeline.clone(), k.clone())));
            d.insert(Rec::Timeline(timeline.clone()));
        }
        self.windows.extend(change.timelines.iter().cloned());
        self.windows.extend(change.items.keys().cloned());
        if change.members {
            d.insert(Rec::Members);
        }
        if change.alerts {
            d.insert(Rec::Alerts);
        }
        if change.room {
            d.insert(Rec::Room);
            d.insert(Rec::Newer);
        }
    }

    /// The effects the App hands out now: a device write moves the revision the board stands at.
    pub fn saw(&mut self, effects: &[Effect]) {
        for e in effects {
            if let Effect::Store { op: StoreOp::DeviceApply { expected_revision, .. }, .. } = e {
                self.rev = self.rev.max(expected_revision + 1);
                    }
        }
    }

    /// The hub's Desk as read now.
    pub fn listed(&mut self, listing: Listing) {
        if self.listing.as_ref() != Some(&listing) {
            self.listing = Some(listing);
            self.listing_due = true;
            }
    }

    /// The device stopped (a write failed, another owner): what the board holds may not be stored, so the cache is
    /// not written again in this run.
    pub fn stop(&mut self) {
        self.state = State::Off;
        self.flight = Flight::Idle;
        self.due = None;
    }

    /// When a write that failed is tried again.
    pub fn next_wake(&self) -> Option<u64> {
        match (&self.state, &self.flight) {
            (State::Open, Flight::Idle) => self.due,
            _ => None,
        }
    }

    /// Writes what is due, unless a write is under way or one failed a moment ago.
    pub fn step(&mut self, now: u64, next_id: &mut u64, out: &mut Vec<Effect>) {
        if !matches!((&self.state, &self.flight), (State::Open, Flight::Idle)) || !self.pending() {
            return;
        }
        if self.due.is_some_and(|at| at > now) {
            return;
        }
        self.due = None;
        let id = io(next_id);
        if self.dirty.is_empty() && self.drops.is_empty() && !self.listing_due && self.windows.is_empty() {
            // only the revision moved: the records stand as they are
            out.push(self.meta_put(id, self.rev, false));
            self.flight = Flight::Stamp(id, self.rev);
            return;
        }
        out.push(self.meta_put(id, self.stored.unwrap_or(0), true));
        self.flight = Flight::Mark(id);
    }

    fn pending(&self) -> bool {
        !self.dirty.is_empty() || !self.drops.is_empty() || self.listing_due || !self.windows.is_empty() || self.stored != Some(self.rev)
    }

    fn failed(&mut self, now: u64) {
        self.due = Some(now + RETRY_MS);
    }

    fn meta_put(&self, id: IoId, rev: u64, writing: bool) -> Effect {
        let meta = Meta { v: VERSION, device: self.device.clone(), rev, writing };
        Effect::Store {
            id,
            op: StoreOp::Put {
                put: vec![Entry {
                    key: super::key(&self.room, super::part::META, None),
                    value: serde_json::to_vec(&meta).unwrap_or_default().into(),
                }],
                delete: Vec::new(),
            },
        }
    }

    /// Step 2: the due records as the model holds them now.
    fn records(&mut self, model: &Model, next_id: &mut u64, out: &mut Vec<Effect>) {
        for timeline in std::mem::take(&mut self.windows) {
            let held = self.written.get(&timeline);
            match model.timelines.get(&timeline) {
                Some(t) => {
                    if let Some(held) = held {
                        self.dirty.extend(held.iter().filter(|k| !t.items.contains_key(*k)).map(|k| Rec::Item(timeline.clone(), k.clone())));
                    }
                    self.dirty.extend(t.items.keys().filter(|k| held.is_none_or(|h| !h.contains(*k))).map(|k| Rec::Item(timeline.clone(), k.clone())));
                }
                None => {
                    self.dirty.insert(Rec::Timeline(timeline.clone()));
                    self.dirty.extend(held.into_iter().flatten().map(|k| Rec::Item(timeline.clone(), k.clone())));
                }
            }
        }
        let recs: Vec<Rec> = std::mem::take(&mut self.dirty).into_iter().collect();
        let drops: Vec<String> = std::mem::take(&mut self.drops).into_iter().collect();
        let mut put = Vec::new();
        let mut delete = drops.clone();
        for rec in &recs {
            let k = key(&self.room, &rec.tail());
            let value = rec.value(model);
            if let Rec::Item(timeline, item) = rec {
                let written = self.written.entry(timeline.clone()).or_default();
                if value.is_some() {
                    written.insert(item.clone());
                } else {
                    written.remove(item);
                }
            }
            match value {
                Some(value) => put.push(Entry { key: k, value: value.into() }),
                None => delete.push(k),
            }
        }
        self.written.retain(|_, items| !items.is_empty());
        let listing = std::mem::take(&mut self.listing_due);
        if listing {
            if let Some(l) = &self.listing {
                put.push(Entry {
                    key: super::key(&self.room, super::part::DESK, None),
                    value: serde_json::to_vec(l).unwrap_or_default().into(),
                });
            }
        }
        let id = io(next_id);
        out.push(Effect::Store { id, op: StoreOp::Put { put, delete } });
        self.flight = Flight::Records(id, Batch { recs, drops, listing, rev: self.rev });
    }
}

fn io(next_id: &mut u64) -> IoId {
    let id = IoId(*next_id);
    *next_id += 1;
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::change::Change;
    use serde_json::json;
    use std::collections::BTreeMap;

    type Store = BTreeMap<String, Vec<u8>>;

    /// Runs a store op as a shell does, applying at most `keys` of a `Put`'s keys (the process dies in it): the
    /// cache space is atomic per key only.
    fn run(store: &mut Store, op: &StoreOp, keys: Option<usize>) -> IoResult<StoreAnswer> {
        match op {
            StoreOp::Get { keys } => IoResult::Ok(StoreAnswer::Values { values: keys.iter().map(|k| store.get(k).map(|v| v.clone().into())).collect() }),
            StoreOp::Scan { prefix } => IoResult::Ok(StoreAnswer::Entries {
                entries: store.iter().filter(|(k, _)| k.starts_with(prefix.as_str())).map(|(k, v)| Entry { key: k.clone(), value: v.clone().into() }).collect(),
            }),
            StoreOp::Put { put, delete } => {
                let mut left = keys.unwrap_or(usize::MAX);
                for k in delete {
                    if left == 0 {
                        break;
                    }
                    left -= 1;
                    store.remove(k);
                }
                for e in put {
                    if left == 0 {
                        break;
                    }
                    left -= 1;
                    store.insert(e.key.clone(), e.value.0.clone());
                }
                IoResult::Ok(StoreAnswer::Done)
            }
            _ => unreachable!(),
        }
    }

    /// Opens a keeper over the store and answers its read: what it gave, the model, the keeper.
    fn open(store: &mut Store, rev: u64) -> (Read, Model, Keeper, u64) {
        let (mut model, mut next, mut out) = (Model::new(), 1, Vec::new());
        let mut keeper = Keeper::open("ab", "d1", rev, &mut next, &mut out);
        let mut read = None;
        for e in std::mem::take(&mut out) {
            if let Effect::Store { id, op } = e {
                let answer = run(store, &op, None);
                match keeper.on_store(id, &answer, &mut model, 0, &mut next, &mut out) {
                    Took::Read(r) => read = Some(r),
                    took => assert_eq!(took, Took::Mine),
                }
            }
        }
        (read.expect("read through"), model, keeper, next)
    }

    fn card(m: &mut Model, c: &mut Change, id: &str, title: &str) {
        m.cards.insert(id.into(), serde_json::from_value(json!({ "object_id": id, "title": title })).unwrap());
        c.cards.insert(id.into());
    }

    fn line(m: &mut Model, c: &mut Change, n: u64) {
        let item = json!({ "envelope_number": n, "content": format!("line {n}") }).as_object().unwrap().clone();
        let t = m.timelines.entry("chat:s".into()).or_default();
        t.item_count += 1;
        t.newest_envelope_number = n;
        t.items.insert(ItemKey::of(&item), item.clone());
        c.add_item("chat:s", &item);
        c.timelines.insert("chat:s".into());
    }

    /// A run of batches; the store dies inside the `kill`th `Put` after `keys` of its keys. Returns the snapshot of
    /// every state a write ended at (with its revision), and whether the store died.
    fn batches(store: &mut Store, kill: usize, keys: usize) -> (Vec<(u64, serde_json::Value)>, bool) {
        let (_, mut m, mut keeper, mut next) = open(store, 1);
        let mut stamped = vec![(1, m.snapshot())];
        let mut puts = 0;
        for step in 0..6u64 {
            let mut c = Change::new();
            match step % 3 {
                0 => card(&mut m, &mut c, &format!("c{step}"), "Ask"),
                1 => line(&mut m, &mut c, 10 + step),
                _ => {
                    card(&mut m, &mut c, "c0", &format!("Revised {step}"));
                    m.timelines.get_mut("chat:s").unwrap().items.clear();
                    c.timelines.insert("chat:s".into());
                }
            }
            keeper.note(&c);
            let rev = 2 + step;
            let mut out = vec![Effect::Store { id: IoId(999), op: StoreOp::DeviceApply { expected_revision: rev - 1, put: vec![], delete: vec![] } }];
            keeper.saw(&out);
            out.clear();
            keeper.step(0, &mut next, &mut out);
            while let Some(Effect::Store { id, op }) = out.pop() {
                puts += 1;
                let dies = puts == kill;
                let answer = run(store, &op, dies.then_some(keys));
                if dies {
                    // (a stamp that landed before the death is a state the cache stands at)
                    if let StoreOp::Put { put, .. } = &op {
                        let meta = put.first().filter(|e| keys > 0 && e.key.ends_with("/meta")).and_then(|e| serde_json::from_slice::<Meta>(&e.value.0).ok());
                        if let Some(meta) = meta.filter(|m| !m.writing) {
                            stamped.push((meta.rev, m.snapshot()));
                        }
                    }
                    return (stamped, true);
                }
                assert_eq!(keeper.on_store(id, &answer, &mut m, 0, &mut next, &mut out), Took::Mine);
                if keeper.stored() == Some(rev) && !stamped.iter().any(|(r, _)| *r == rev) {
                    stamped.push((rev, m.snapshot()));
                }
            }
        }
        (stamped, false)
    }

    #[test]
    fn killed_inside_any_write_the_cache_reads_as_a_state_it_stamped_or_not_at_all() {
        let mut ran = 0;
        for kill in 1..=24 {
            for keys in 0..6 {
                let mut store = Store::new();
                let (stamped, died) = batches(&mut store, kill, keys);
                let device = stamped.last().unwrap().0 + 1;
                let (read, m, ..) = open(&mut store, device);
                match read {
                    Read::Loaded { behind: false, .. } => panic!("the device is ahead of every stamp"),
                    Read::Loaded { behind: true, .. } => {
                        // a stamped state, its lines dropped
                        let strip = |v: &serde_json::Value| {
                            let mut v = v.clone();
                            for t in v["timelines"].as_array_mut().into_iter().flatten() {
                                t[1]["items"] = json!([]);
                                t[1]["window_open"] = json!(false);
                                t[1]["loaded_down_to"] = json!(null);
                                t[1]["has_more"] = json!(true);
                            }
                            v
                        };
                        assert!(stamped.iter().any(|(_, s)| strip(s) == strip(&m.snapshot())), "kill {kill}/{keys}: {}", m.snapshot());
                    }
                    Read::Dropped | Read::Empty => {}
                }
                // read at the stamp's own revision: the stamped state itself, lines and all
                let (rev, _) = stamped.last().unwrap();
                let (read, m, ..) = open(&mut store, *rev);
                if let Read::Loaded { behind: false, .. } = read {
                    assert!(stamped.iter().any(|(r, s)| r == rev && *s == m.snapshot()), "kill {kill}/{keys}");
                    ran += 1;
                }
                if !died {
                    break;
                }
            }
        }
        assert!(ran > 10);
    }
}
