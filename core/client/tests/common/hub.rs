//! A stand-in hub for the engine's tests: the routes the engine asks, in the wire form of spec/hub-api.md, over
//! state in memory. Commits are checked by the protocol core's public observer (14.1), as the real hub does, so a
//! Welcome goes to the devices its Commit added. Everything else is written by hand here and is NOT the real hub
//! (W6 checks these against it first):
//!
//! - sign-in: a challenge and a token per device, checked with `hub_auth::verify`; a token lives ten minutes of
//!   the sim's clock; no roles but `human` and `agent`;
//! - visibility: every signed-in device is served every item of the room: right for a human device (the real hub
//!   serves a human device all of the room), not for an agent or helper device, which asks nothing over HTTP here;
//! - envelopes: filed by their header; refused as `gap` unless the number is the next of its chain; no other check
//!   (no epoch, no membership, no void records unless a test makes one);
//! - `GET /v1/changes`: `head` is the room's newest change; `active=1` leaves out the archived sessions;
//! - Welcomes: kept until the test drops them (the real hub deletes one when its device writes in the group);
//! - the Desk: an object is open while the header of its newest envelope says so (the real hub replays 9.2); the
//!   order is by `first_change` alone (the real hub puts the knocking ones first), `urgency` and `owner` are made
//!   up; `active=1` and `archived=1` as the real hub; cut as the real hub cuts (1000 objects of a kind, one budget of
//!   envelope bytes, 20 000 registers); every writer's newest value per register is served;
//! - `POST /v1/key-packages/claim`: a human device claims of any device (the real hub also lets an opener claim for
//!   its helper session);
//! - `GET /v1/rooms/{room}/groups`: every group to every device, in one order of the stand-in's own; `stale` by the
//!   core's rule under the newest room state, `room_epoch` the one of the group's last Commit;
//! - the stream's events beside the order: a Welcome to each device added, relays to the human devices but the sender, wishes to the human
//!   devices (a device keeps every wish, not its 16 newest), an archived session to its leaves, an evicted file
//!   to the human devices (not to the leaves of its group);
//! - files, pushes, accounts, invites, recovery: not there.

#![allow(dead_code)]

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use trommi_client_core::app::io::{Header, HttpRequest, HttpResponse, IoError, StreamItem};
use trommi_core::envelope::Envelope;
use trommi_core::hub_auth::{self, HubAddress, IssuedChallenge, SignedHubAuth};
use trommi_core::ids::{base64url_decode, base64url_encode as b64, DeviceId, GroupId, RoomId, SessionId};
use trommi_core::mls::observer::{Context, Observer, PostedCommit};
use trommi_core::mls::profile::MAX_HUMAN_DEVICES;
use trommi_core::mls::rules::{Parent, SessionFacts};
use trommi_core::recovery::PublicRules;
use trommi_core::store::{OutboxEntry, OutboxKind};
use trommi_core::Error;

pub const HUB_URL: &str = "https://hub.example";
/// The real hub lists at most so many open objects of a kind on the Desk.
pub const DESK_MAX: usize = 1000;
/// The real Desk's one budget of envelope bytes, and its most registers.
pub const DESK_BYTES: usize = 8 << 20;
pub const DESK_REGISTERS: usize = 20_000;

/// One entry of the room's order.
#[derive(Clone)]
pub enum Item {
    Log { change: u64, group: GroupId, n: u64, epoch: u64, commit: bool, bytes: Vec<u8>, recovery_auth: Option<Vec<u8>>, from: DeviceId },
    Envelope { change: u64, group: GroupId, sender: DeviceId, seq: u64, bytes: Vec<u8>, void_code: Option<String> },
}

impl Item {
    pub fn change(&self) -> u64 {
        match self {
            Item::Log { change, .. } | Item::Envelope { change, .. } => *change,
        }
    }
    pub fn group(&self) -> GroupId {
        match self {
            Item::Log { group, .. } | Item::Envelope { group, .. } => *group,
        }
    }
    pub fn json(&self) -> Value {
        match self {
            Item::Log { change, group, n, epoch, commit, bytes, recovery_auth, from } => {
                let mut v = json!({ "change": change, "kind": if *commit { "commit" } else { "message" }, "group_id": b64(group.as_bytes()),
                    "n": n, "epoch": epoch, "at": 0, "bytes": b64(bytes), "sender": from.to_base64url() });
                if let Some(auth) = recovery_auth {
                    v["recovery_auth"] = json!(b64(auth));
                }
                v
            }
            Item::Envelope { .. } => {
                let mut v = self.served();
                v["kind"] = json!("envelope");
                v
            }
        }
    }
    /// An envelope as a route serves it that brings envelopes alone (the Desk, an object, a Chat).
    pub fn served(&self) -> Value {
        let Item::Envelope { change, bytes, void_code, .. } = self else { return self.json() };
        let mut v = json!({ "change": change, "envelope": b64(bytes), "received_at": 0 });
        if let Some(code) = void_code {
            v["void_code"] = json!(code);
        }
        v
    }
    /// An envelope as a link of its sender's chain.
    pub fn link(&self) -> Value {
        let mut v = self.served();
        if let Item::Envelope { seq, .. } = self {
            v["seq"] = json!(seq);
        }
        v
    }
    /// As the stream sends it.
    pub fn sse(&self) -> Vec<u8> {
        let name = if matches!(self, Item::Envelope { .. }) { "envelope" } else { "log" };
        format!("id: {}\nevent: {name}\ndata: {}\n\n", self.change(), self.json()).into_bytes()
    }
}

/// An object as its envelopes' headers leave it.
#[derive(Clone)]
pub struct Obj {
    pub group: GroupId,
    pub request: bool,
    pub card: bool,
    pub open: bool,
    pub first_change: u64,
    pub head_change: u64,
    /// The change of its newest version (of a permission request: the request).
    pub version: u64,
    pub envelopes: Vec<u64>,
}

pub struct Welcome {
    pub id: u64,
    pub group: GroupId,
    pub bytes: Vec<u8>,
    pub device: DeviceId,
}

#[derive(Default)]
struct Pool {
    single_use: Vec<Vec<u8>>,
    last_resort: Option<Vec<u8>>,
}

/// A refusal: status, code, and whether the hub keeps the envelope as a void record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct No {
    pub status: u16,
    pub code: String,
    pub voided: bool,
}

fn no(error: Error) -> No {
    let code = error.code();
    No {
        // (the status the protocol gives the code: the codec takes a refusal only under its own)
        status: trommi_client_core::hub::status_of(code).unwrap_or(409),
        code: code.to_string(),
        voided: false,
    }
}

struct Sessions<'a> {
    sessions: &'a BTreeMap<GroupId, Observer>,
    archived: &'a BTreeSet<GroupId>,
}

impl Sessions<'_> {
    fn live(&self) -> impl Iterator<Item = &Observer> {
        self.sessions.iter().filter(|(group, _)| !self.archived.contains(group)).map(|(_, o)| o)
    }
}

impl SessionFacts for Sessions<'_> {
    fn main_session(&self, session: &SessionId, room_epoch: u64) -> Parent {
        self.live()
            .find(|o| o.session().is_some_and(|s| s.session_id == *session))
            .map_or(Parent::NotAMainSession, |o| o.seat_at(room_epoch))
    }
    fn main_session_of(&self, agent: &DeviceId) -> Option<SessionId> {
        self.live()
            .filter(|o| o.seat_at(u64::MAX) == Parent::Seat(Some(*agent)))
            .find_map(|o| o.session().map(|s| s.session_id))
    }
    fn live_helpers(&self, parent: &SessionId) -> usize {
        self.live().filter(|o| o.session().is_some_and(|s| s.parent == *parent)).count()
    }
}

/// What a test makes of the next requests: looked at before the hub answers.
pub type Fault = Box<dyn FnMut(u64, &HttpRequest) -> Option<Result<HttpResponse, IoError>>>;

pub struct Hub {
    pub change: u64,
    pub order: Vec<Item>,
    room: Option<Observer>,
    sessions: BTreeMap<GroupId, Observer>,
    archived: BTreeSet<GroupId>,
    epochs: BTreeMap<GroupId, u64>,
    log_n: BTreeMap<GroupId, u64>,
    pub group_infos: BTreeMap<(GroupId, u64), Vec<u8>>,
    pub welcomes: Vec<Welcome>,
    pools: BTreeMap<DeviceId, Pool>,
    chains: BTreeMap<(GroupId, DeviceId), u64>,
    /// The first answer of every post, by its bytes: a repeated post gets it again (13.2).
    answers: BTreeMap<Vec<u8>, Result<Value, No>>,
    challenges: Vec<[u8; 32]>,
    counter: u64,
    pub tokens: BTreeMap<String, (DeviceId, u64)>,
    /// Every request that reached the hub, as "METHOD /path?query".
    pub seen: Vec<String>,
    pub fault: Option<Fault>,
    /// The streams that are open: the device, the last change sent on it, and how many of `notes` it was past.
    pub streaming: Vec<(DeviceId, u64, usize)>,
    /// The stream's events beside the order (relays, wishes, archived sessions, evicted files): to whom, and the
    /// event as the stream sends it. Never replayed: a stream opened later does not get them.
    pub notes: Vec<(Vec<DeviceId>, Vec<u8>)>,
    /// The wishes (`POST /v1/requests`): id, device, kind, group, the hub's clock.
    pub wishes: Vec<(u64, DeviceId, String, Option<GroupId>, u64)>,
    /// The `head` a page of the hub's order names; none: the truth.
    pub head_lie: Option<u64>,
    /// The chains of these senders are served cut short before this number (a hub that withholds).
    pub withhold: BTreeMap<DeviceId, u64>,
    /// A page of the hub's order holds at most so many items, whatever was asked.
    pub page_max: Option<usize>,
    /// Changes the pages and the stream leave out (their chain still serves them): a hub that skips.
    pub hidden: BTreeSet<u64>,
    pub objects: BTreeMap<trommi_core::ids::ObjectId, Obj>,
    /// The Desk says it was cut short.
    pub desk_truncated: bool,
    /// Every writer's newest value per register: the change of its envelope.
    pub registers: BTreeMap<(GroupId, DeviceId, Vec<u8>), u64>,
    /// The `first_change` the Desk names for every object; none: the truth.
    pub first_change_lie: Option<u64>,
    /// Every SealedKey the hub was posted, in order (8.3).
    pub rows: Vec<Vec<u8>>,
    /// `POST /v1/groups/read` holds at most so many groups in one answer; the rest are `too-large` rows.
    pub read_fits: Option<usize>,
    /// A page of a group's log holds at most so many entries, whatever was asked.
    pub log_max: Option<usize>,
    /// A page of the room's groups holds at most so many, whatever was asked.
    pub groups_max: Option<usize>,
    /// Every refusal the hub answered, as "METHOD /path: code".
    pub refused: Vec<String>,
    /// Groups the hub says it does not know (`not-found`) on the read routes.
    pub unseen: BTreeSet<GroupId>,
    /// The Desk's one budget of envelope bytes (the real hub's 8 MiB) and its most registers (20 000); a test may
    /// lower them.
    pub desk_bytes_max: usize,
    pub desk_registers_max: usize,
}

impl Default for Hub {
    fn default() -> Self {
        Hub {
            change: 0,
            order: Vec::new(),
            room: None,
            sessions: BTreeMap::new(),
            archived: BTreeSet::new(),
            epochs: BTreeMap::new(),
            log_n: BTreeMap::new(),
            group_infos: BTreeMap::new(),
            welcomes: Vec::new(),
            pools: BTreeMap::new(),
            chains: BTreeMap::new(),
            answers: BTreeMap::new(),
            challenges: Vec::new(),
            counter: 0,
            tokens: BTreeMap::new(),
            seen: Vec::new(),
            fault: None,
            streaming: Vec::new(),
            notes: Vec::new(),
            wishes: Vec::new(),
            head_lie: None,
            withhold: BTreeMap::new(),
            page_max: None,
            hidden: BTreeSet::new(),
            objects: BTreeMap::new(),
            desk_truncated: false,
            registers: BTreeMap::new(),
            first_change_lie: None,
            rows: Vec::new(),
            read_fits: None,
            log_max: None,
            groups_max: None,
            refused: Vec::new(),
            unseen: BTreeSet::new(),
            desk_bytes_max: DESK_BYTES,
            desk_registers_max: DESK_REGISTERS,
        }
    }
}

fn part(entry: &OutboxEntry, at: usize) -> &[u8] {
    entry.parts.get(at).map_or(&[], Vec::as_slice)
}
fn optional(bytes: &[u8]) -> Option<&[u8]> {
    Some(bytes).filter(|b| !b.is_empty())
}

impl Hub {
    pub fn room_id(&self) -> RoomId {
        self.room.as_ref().expect("a room").group().room_id()
    }
    pub fn observer(&self, group: &GroupId) -> Option<&Observer> {
        if group.is_room() {
            self.room.as_ref()
        } else {
            self.sessions.get(group)
        }
    }
    /// A group as the hub serves it to a device that joins with the code: its founding GroupInfo, its Commits (the
    /// change, the Commit, its RecoveryAuth) and its current GroupInfo.
    #[allow(clippy::type_complexity)]
    pub fn served(&self, group: &GroupId) -> (Vec<u8>, Vec<(u64, Vec<u8>, Option<Vec<u8>>)>, Vec<u8>) {
        let epoch = self.epochs[group];
        let commits = self
            .order
            .iter()
            .filter_map(|i| match i {
                Item::Log { change, group: g, commit: true, bytes, recovery_auth, .. } if g == group => Some((*change, bytes.clone(), recovery_auth.clone())),
                _ => None,
            })
            .collect();
        (self.group_infos[&(*group, 0)].clone(), commits, self.group_infos[&(*group, epoch)].clone())
    }
    pub fn session_groups(&self) -> Vec<GroupId> {
        self.sessions.keys().copied().collect()
    }
    pub fn unused(&self, device: &DeviceId) -> usize {
        self.pools.get(device).map_or(0, |p| p.single_use.len())
    }

    /// One KeyPackage of each device, all or nothing.
    pub fn claim(&mut self, devices: &[DeviceId]) -> Result<Vec<Vec<u8>>, No> {
        let has = |p: &Pool| !p.single_use.is_empty() || p.last_resort.is_some();
        if !devices.iter().all(|d| self.pools.get(d).is_some_and(has)) {
            return Err(no(Error::NotFound));
        }
        Ok(devices
            .iter()
            .map(|d| {
                let pool = self.pools.get_mut(d).expect("checked");
                pool.single_use.pop().or_else(|| pool.last_resort.clone()).expect("checked")
            })
            .collect())
    }

    fn append(&mut self, group: GroupId, commit: bool, epoch: u64, bytes: &[u8], recovery_auth: Option<&[u8]>, from: &DeviceId) -> (u64, u64) {
        self.change += 1;
        let n = self.log_n.entry(group).or_default();
        *n += 1;
        self.order.push(Item::Log { change: self.change, group, n: *n, epoch, commit, bytes: bytes.to_vec(), recovery_auth: recovery_auth.map(<[u8]>::to_vec), from: *from });
        (self.change, *n)
    }

    fn accept_commit(&mut self, group: GroupId, epoch: u64, posted: &PostedCommit<'_>, from: &DeviceId, adds: &[DeviceId]) -> Value {
        self.epochs.insert(group, epoch + 1);
        self.group_infos.insert((group, epoch + 1), posted.group_info.to_vec());
        if !posted.sealed_key.is_empty() {
            self.rows.push(posted.sealed_key.to_vec());
        }
        let (change, _) = self.append(group, true, epoch, posted.commit, posted.recovery_auth, from);
        if let Some(welcome) = posted.welcome {
            for device in adds {
                self.counter += 1;
                self.welcomes.push(Welcome { id: self.counter, group, bytes: welcome.to_vec(), device: *device });
            }
            // (the stream tells each device added that a Welcome waits)
            self.note(adds.to_vec(), "welcome", json!({ "group_id": b64(group.as_bytes()) }));
        }
        json!({ "epoch": epoch + 1, "change": change })
    }

    fn check(&mut self, group: &GroupId, posted: &PostedCommit<'_>, from: &DeviceId) -> Result<Vec<DeviceId>, Error> {
        let facts = if group.is_room() {
            let context = Context { room: None, sessions: &Sessions { sessions: &self.sessions, archived: &self.archived }, recovery: &PublicRules, max_human_devices: MAX_HUMAN_DEVICES };
            self.room.as_mut().ok_or(Error::NoRoom)?.check_posted_commit(posted, &context)?
        } else {
            let mut observer = self.sessions.remove(group).ok_or(Error::NotFound)?;
            let context = Context {
                room: self.room.as_ref().and_then(Observer::history),
                sessions: &Sessions { sessions: &self.sessions, archived: &self.archived },
                recovery: &PublicRules,
                max_human_devices: MAX_HUMAN_DEVICES,
            };
            let result = observer.check_posted_commit(posted, &context);
            self.sessions.insert(*group, observer);
            result?
        };
        if !facts.external && facts.committer != *from {
            return Err(Error::WrongSender);
        }
        Ok(facts.adds)
    }

    /// Takes one outbox entry of `from`: the body of the answer, or the refusal. A repeated post of the same bytes
    /// gets the first answer again.
    pub fn post(&mut self, from: &DeviceId, entry: &OutboxEntry) -> Result<Value, No> {
        let mut key: Vec<u8> = vec![entry.kind as u8];
        key.extend_from_slice(from.as_bytes());
        key.extend(entry.group.iter().flat_map(|g| g.as_bytes().to_vec()));
        for p in &entry.parts {
            key.extend_from_slice(&(p.len() as u32).to_be_bytes());
            key.extend_from_slice(p);
        }
        if let Some(answer) = self.answers.get(&key) {
            return answer.clone();
        }
        let answer = self.take(from, entry);
        // a refusal that names a state of the moment is not remembered: the same bytes may fit later
        let passing = matches!(&answer, Err(n) if ["epoch-taken", "wrong-epoch", "room-behind", "stale-session", "gap"].contains(&n.code.as_str()));
        if !passing {
            self.answers.insert(key, answer.clone());
        }
        answer
    }

    fn take(&mut self, from: &DeviceId, entry: &OutboxEntry) -> Result<Value, No> {
        match entry.kind {
            OutboxKind::KeyPackages => {
                let pool = self.pools.entry(*from).or_default();
                for (at, package) in entry.parts.iter().enumerate() {
                    if package.is_empty() {
                        continue;
                    }
                    if at == 0 {
                        pool.last_resort = Some(package.clone());
                    } else {
                        pool.single_use.push(package.clone());
                    }
                }
                Ok(json!({ "unused": pool.single_use.len() }))
            }
            OutboxKind::Message | OutboxKind::RelayMessage => {
                let group = entry.group.ok_or(no(Error::BadFormat))?;
                if self.epochs.get(&group) != Some(&entry.epoch) {
                    return Err(no(Error::WrongEpoch));
                }
                if entry.kind == OutboxKind::RelayMessage {
                    // (7.2: the room group alone, to the human devices but the sender, never stored)
                    if !group.is_room() {
                        return Err(no(Error::Forbidden));
                    }
                    let to = self.humans().into_iter().filter(|d| d != from).collect();
                    let data = json!({ "group_id": b64(group.as_bytes()), "epoch": entry.epoch, "sender": from.to_base64url(), "message": b64(part(entry, 0)) });
                    self.note(to, "relay", data);
                    return Ok(json!({ "n": null }));
                }
                let (_, n) = self.append(group, false, entry.epoch, part(entry, 0), None, from);
                Ok(json!({ "n": n }))
            }
            OutboxKind::RoomFounding => {
                let group = entry.group.ok_or(no(Error::BadFormat))?;
                if self.room.is_some() {
                    return Err(no(Error::RoomExists));
                }
                let (observer, founder) = Observer::found_room(part(entry, 0), part(entry, 1), &PublicRules).map_err(no)?;
                if founder != *from || observer.group() != group {
                    return Err(no(Error::WrongSender));
                }
                self.room = Some(observer);
                self.epochs.insert(group, 0);
                self.group_infos.insert((group, 0), part(entry, 0).to_vec());
                self.rows.push(part(entry, 1).to_vec());
                self.change += 1;
                Ok(json!({ "room_id": group.room_id().to_base64url() }))
            }
            OutboxKind::GroupFounding => {
                let posted = PostedCommit { commit: part(entry, 2), group_info: part(entry, 3), welcome: optional(part(entry, 4)), sealed_key: part(entry, 5), recovery_auth: None, base_group_info: None };
                let room = self.room.as_ref().ok_or(no(Error::NoRoom))?;
                let context = Context { room: room.history(), sessions: &Sessions { sessions: &self.sessions, archived: &self.archived }, recovery: &PublicRules, max_human_devices: MAX_HUMAN_DEVICES };
                let (observer, facts) = Observer::found_session(part(entry, 0), part(entry, 1), &posted, &context).map_err(no)?;
                // (over HTTP the group is the founding's own: the request does not name it)
                let group = observer.group();
                if facts.committer != *from || entry.group.is_some_and(|named| named != group) {
                    return Err(no(Error::WrongSender));
                }
                if self.epochs.contains_key(&group) {
                    return Err(no(Error::RoomExists));
                }
                self.sessions.insert(group, observer);
                self.group_infos.insert((group, 0), part(entry, 0).to_vec());
                self.rows.push(part(entry, 1).to_vec());
                self.epochs.insert(group, 0);
                self.accept_commit(group, 0, &posted, from, &facts.adds);
                Ok(json!({ "group_id": b64(group.as_bytes()) }))
            }
            OutboxKind::Commit | OutboxKind::ExternalCommit => {
                let group = entry.group.ok_or(no(Error::BadFormat))?;
                let external = entry.kind == OutboxKind::ExternalCommit;
                let current = *self.epochs.get(&group).ok_or(no(Error::NotFound))?;
                let base = self.group_infos.get(&(group, current)).cloned();
                let posted = PostedCommit {
                    commit: part(entry, 0),
                    group_info: part(entry, 1),
                    welcome: if external { None } else { optional(part(entry, 2)) },
                    sealed_key: part(entry, if external { 2 } else { 3 }),
                    recovery_auth: if external { optional(part(entry, 3)) } else { None },
                    base_group_info: base.as_deref(),
                };
                if self.archived.contains(&group) {
                    return Err(no(Error::Gone));
                }
                if entry.epoch != current {
                    return Err(no(Error::EpochTaken));
                }
                let adds = self.check(&group, &posted, from).map_err(no)?;
                Ok(self.accept_commit(group, current, &posted, from, &adds))
            }
            OutboxKind::Envelope => {
                let bytes = part(entry, 0);
                let envelope = Envelope::decode(bytes).map_err(no)?;
                let header = &envelope.header;
                let (group, sender, seq) = (header.group, header.sender, header.seq);
                if sender != *from {
                    return Err(no(Error::WrongSender));
                }
                let head = self.chains.entry((group, sender)).or_default();
                if seq != *head + 1 {
                    return Err(no(if seq <= *head { Error::Replay } else { Error::Gap }));
                }
                *head = seq;
                self.change += 1;
                if let trommi_core::envelope::Subject::Register(register) = &header.subject {
                    self.registers.insert((group, sender, register.as_bytes().to_vec()), self.change);
                }
                if let Some(fields) = header.subject.object() {
                    use trommi_core::envelope::{ObjectState, ObjectType, Subject};
                    let change = self.change;
                    let object = self.objects.entry(fields.object_id).or_insert(Obj {
                        group,
                        request: fields.object_type == ObjectType::Request,
                        card: fields.object_type == ObjectType::Card,
                        open: true,
                        first_change: change,
                        head_change: change,
                        version: change,
                        envelopes: Vec::new(),
                    });
                    object.head_change = change;
                    object.open = fields.state == ObjectState::Open;
                    if matches!(header.subject, Subject::Version(_) | Subject::Request(_)) {
                        object.version = change;
                    }
                    object.envelopes.push(change);
                }
                self.order.push(Item::Envelope { change: self.change, group, sender, seq, bytes: bytes.to_vec(), void_code: None });
                Ok(json!({ "change": self.change }))
            }
            OutboxKind::SealedKey => {
                self.rows.push(part(entry, 0).to_vec());
                Ok(Value::Null)
            }
            _ => Err(no(Error::BadFormat)),
        }
    }

    // ---- over HTTP

    fn device_of(&self, now: u64, request: &HttpRequest) -> Option<DeviceId> {
        let value = request.headers.iter().find(|h| h.name.eq_ignore_ascii_case("authorization"))?.value.strip_prefix("Bearer ")?.to_string();
        self.tokens.get(&value).filter(|(_, until)| now < *until).map(|(device, _)| *device)
    }

    fn log_page(&self, group: &GroupId, after: u64, limit: usize, commits_only: bool) -> Value {
        let limit = limit.min(self.log_max.unwrap_or(usize::MAX));
        let mut items: Vec<Value> = self
            .order
            .iter()
            .filter(|i| matches!(i, Item::Log { group: g, n, commit, .. } if g == group && *n > after && (*commit || !commits_only)))
            .map(Item::json)
            .collect();
        let more = items.len() > limit;
        items.truncate(limit);
        json!({ "items": items, "more": more })
    }

    fn info(&self, group: &GroupId, epoch: Option<u64>) -> Result<Value, No> {
        if self.unseen.contains(group) {
            return Err(no(Error::NotFound));
        }
        let epoch = epoch.or(self.epochs.get(group).copied()).ok_or(no(Error::NotFound))?;
        let info = self.group_infos.get(&(*group, epoch)).ok_or(no(Error::NotFound))?;
        Ok(json!({ "epoch": epoch, "group_info": b64(info) }))
    }

    /// The hub keeps a void record under the next number of this chain (a test's refusal with `voided: true`).
    pub fn void_next(&mut self, group: GroupId, sender: DeviceId) {
        *self.chains.entry((group, sender)).or_default() += 1;
    }

    /// The human devices of the room's newest state.
    pub fn humans(&self) -> Vec<DeviceId> {
        self.room.as_ref().and_then(Observer::history).map(|h| h.newest().humans.iter().copied().collect()).unwrap_or_default()
    }

    /// An event beside the order for `to`.
    pub fn note(&mut self, to: Vec<DeviceId>, name: &str, data: Value) {
        self.notes.push((to, format!("event: {name}\ndata: {data}\n\n").into_bytes()));
    }

    /// A device's wish to the human devices (`POST /v1/requests`), at the hub's clock `at`. Returns its id.
    pub fn wish(&mut self, device: &DeviceId, kind: &str, group: Option<GroupId>, at: u64) -> u64 {
        self.counter += 1;
        let id = self.counter;
        self.wishes.push((id, *device, kind.to_string(), group, at));
        let to = self.humans();
        self.note(to, "request", json!({ "id": id, "kind": kind, "device": device.to_base64url(), "group_id": group.map(|g| b64(g.as_bytes())), "n": null }));
        id
    }

    /// The hub deletes a file (11.3): the human devices are told.
    pub fn evict(&mut self, file: &trommi_core::ids::FileId) {
        let to = self.humans();
        self.note(to, "file_evicted", json!({ "file_id": file.to_base64url() }));
    }

    /// A human device archives a session (5.2.10): its leaves are told.
    pub fn archive(&mut self, group: &GroupId) {
        if self.archived.insert(*group) {
            let to = self.observer(group).and_then(|o| o.leaves().ok()).unwrap_or_default().into_iter().collect();
            self.note(to, "presence", json!({ "group_id": b64(group.as_bytes()), "archived": true }));
        }
    }

    /// Ends every token: the next request of each device is `unauthorised`.
    pub fn end_tokens(&mut self) {
        self.tokens.clear();
    }

    pub fn http(&mut self, now: u64, request: &HttpRequest) -> Result<HttpResponse, IoError> {
        if let Some(fault) = self.fault.as_mut() {
            if let Some(answer) = fault(now, request) {
                return answer;
            }
        }
        let path = request.url.strip_prefix(HUB_URL).unwrap_or("/nowhere").to_string();
        self.seen.push(format!("{} {path}", request.method));
        Ok(match self.route(now, request, &path) {
            Ok(body) => {
                super::shape::check(&request.method, &path, &body);
                answer(200, body)
            }
            Err(n) => {
                self.refused.push(format!("{} {path}: {}", request.method, n.code));
                answer(n.status, json!({ "error": n.code, "message": "refused", "voided": n.voided }))
            }
        })
    }

    fn route(&mut self, now: u64, request: &HttpRequest, path: &str) -> Result<Value, No> {
        let (path, query) = path.split_once('?').unwrap_or((path, ""));
        let q = |name: &str| query.split('&').find_map(|pair| pair.strip_prefix(name).and_then(|rest| rest.strip_prefix('=')));
        let num = |name: &str| q(name).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let body: Value = request.body.as_ref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or(Value::Null);
        let bytes = |name: &str| body.get(name).and_then(Value::as_str).and_then(|s| base64url_decode(s).ok()).unwrap_or_default();
        let group_at = |at: usize| segs.get(at).and_then(|s| base64url_decode(s).ok()).and_then(|b| GroupId::from_bytes(&b).ok()).ok_or(no(Error::BadFormat));
        let method = request.method.as_str();
        // the two routes without a token
        match (method, segs.as_slice()) {
            ("GET", ["v1", "rooms", _, "challenge"]) => {
                self.counter += 1;
                let mut challenge = [0u8; 32];
                challenge[..8].copy_from_slice(&self.counter.to_be_bytes());
                self.challenges.push(challenge);
                return Ok(json!({ "challenge": b64(&challenge) }));
            }
            ("POST", ["v1", "rooms", _, "tokens"]) => {
                let signed = SignedHubAuth { auth: bytes("auth"), signature: bytes("signature") };
                let auth = hub_auth::HubAuth::decode(&signed.auth).map_err(no)?;
                let at = self.challenges.iter().position(|c| *c == auth.challenge).ok_or(no(Error::BadChallenge))?;
                let issued = IssuedChallenge { challenge: self.challenges.remove(at), expires_at: now + 120_000 };
                let device = hub_auth::verify(&signed, &self.room_id(), &HubAddress::parse(HUB_URL).expect("an address"), &issued, now).map_err(no)?;
                let history = self.room.as_ref().and_then(Observer::history).ok_or(no(Error::NoRoom))?.newest();
                let role = if history.is_human(&device) {
                    "human"
                } else if history.is_agent(&device) {
                    "agent"
                } else {
                    return Err(no(Error::NotMember));
                };
                self.counter += 1;
                let token = format!("token-of-the-stand-in-{}", self.counter);
                self.tokens.insert(token.clone(), (device, now + 600_000));
                return Ok(json!({ "token": token, "expires_at": now + 600_000, "role": role }));
            }
            _ => {}
        }
        let device = self.device_of(now, request).ok_or(no(Error::Unauthorised))?;
        match (method, segs.as_slice()) {
            ("GET", ["v1", "changes"]) => {
                let (after, limit) = (num("after"), num("limit").clamp(1, 1000) as usize);
                let only: Option<GroupId> = q("group").and_then(|g| base64url_decode(g).ok()).and_then(|b| GroupId::from_bytes(&b).ok());
                // (point 46: `active=1` leaves out what lies in an archived session)
                let active = q("active") == Some("1");
                let limit = limit.min(self.page_max.unwrap_or(usize::MAX));
                let mut items: Vec<&Item> = self
                    .order
                    .iter()
                    .filter(|i| i.change() > after && !self.hidden.contains(&i.change()) && only.is_none_or(|g| i.group() == g && matches!(i, Item::Envelope { .. })))
                    .filter(|i| !active || !self.archived.contains(&i.group()))
                    .collect();
                let more = items.len() > limit;
                items.truncate(limit);
                let change = if more { items.last().map_or(after, |i| i.change()) } else { self.change.max(after) };
                Ok(json!({ "items": items.iter().map(|i| i.json()).collect::<Vec<_>>(), "change": change, "more": more, "head": self.head_lie.unwrap_or(self.change).max(change) }))
            }
            ("GET", ["v1", "welcomes"]) => {
                let after = num("after");
                Ok(Value::Array(
                    self.welcomes
                        .iter()
                        .filter(|w| w.device == device && w.id > after)
                        .map(|w| json!({ "id": w.id, "group_id": b64(w.group.as_bytes()), "welcome": b64(&w.bytes), "at": 0 }))
                        .collect(),
                ))
            }
            ("GET", ["v1", "groups", _, "log"]) => {
                let group = group_at(2)?;
                let (after, limit) = (num("after"), num("limit").clamp(1, 1000) as usize);
                if self.unseen.contains(&group) {
                    return Err(no(Error::NotFound));
                }
                Ok(self.log_page(&group, after, limit, q("kind") == Some("commit")))
            }
            ("GET", ["v1", "groups", _, "info"]) => self.info(&group_at(2)?, q("epoch").and_then(|e| e.parse().ok())),
            ("POST", ["v1", "groups", "read"]) => {
                let asked: Vec<GroupId> = body["groups"].as_array().into_iter().flatten().filter_map(|g| g.as_str().and_then(|s| base64url_decode(s).ok()).and_then(|b| GroupId::from_bytes(&b).ok())).collect();
                let distinct: BTreeSet<&GroupId> = asked.iter().collect();
                if asked.is_empty() || asked.len() > 64 || distinct.len() != asked.len() {
                    return Err(no(Error::BadFormat));
                }
                let fits = self.read_fits.unwrap_or(usize::MAX).max(1);
                let rows: Vec<Value> = asked
                    .iter()
                    .enumerate()
                    .map(|(at, group)| {
                        let name = b64(group.as_bytes());
                        if at >= fits {
                            return json!({ "group": name, "error": "too-large", "message": "refused", "status": 413 });
                        }
                        match (self.info(group, None), self.epochs.get(group)) {
                            (Ok(info), Some(epoch)) => json!({ "group": name, "info": info,
                                "founding": if *epoch == 0 { Value::Null } else { self.info(group, Some(0)).unwrap_or(Value::Null) },
                                "log": self.log_page(group, 0, 256, true) }),
                            _ => json!({ "group": name, "error": "not-found", "message": "refused", "status": 404 }),
                        }
                    })
                    .collect();
                Ok(json!({ "groups": rows }))
            }
            ("POST", ["v1", "commits"]) => {
                let commits = body["commits"].as_array().cloned().unwrap_or_default();
                if commits.is_empty() || commits.len() > 64 {
                    return Err(no(Error::BadFormat));
                }
                let b = |v: &Value, name: &str| v.get(name).and_then(Value::as_str).and_then(|s| base64url_decode(s).ok()).unwrap_or_default();
                let answers: Vec<Value> = commits
                    .iter()
                    .map(|c| {
                        let Some(group) = GroupId::from_bytes(&b(c, "group")).ok() else {
                            return json!({ "error": "bad-format", "message": "refused", "status": 400 });
                        };
                        let entry = OutboxEntry { id: 0, kind: OutboxKind::ExternalCommit, group: Some(group), epoch: c["epoch"].as_u64().unwrap_or(0),
                            parts: vec![b(c, "commit"), b(c, "group_info"), b(c, "sealed_key"), b(c, "recovery_auth")] };
                        match self.post(&device, &entry) {
                            Ok(v) => v,
                            Err(n) => json!({ "error": n.code, "message": "refused", "status": n.status }),
                        }
                    })
                    .collect();
                Ok(json!({ "answers": answers }))
            }
            ("GET", ["v1", "groups", _, "chains", sender]) => {
                let group = group_at(2)?;
                let sender = DeviceId::from_base64url(sender).map_err(no)?;
                let (after, limit) = (num("after"), num("limit").clamp(1, 2000) as usize);
                let stop = self.withhold.get(&sender).copied().unwrap_or(u64::MAX);
                let mut items: Vec<Value> = self
                    .order
                    .iter()
                    .filter(|i| matches!(i, Item::Envelope { group: g, sender: s, seq, .. } if *g == group && *s == sender && *seq > after && *seq < stop))
                    .map(Item::link)
                    .collect();
                let more = items.len() > limit;
                items.truncate(limit);
                Ok(json!({ "items": items, "more": more }))
            }
            ("GET", ["v1", "desk"]) => {
                let item = |change: u64| self.order.iter().find(|i| i.change() == change).map(Item::served);
                // (point 46: `active=1` the live sessions alone, `archived=1` the archived ones alone)
                let (active, archived) = (q("active") == Some("1"), q("archived") == Some("1"));
                let sees = |group: &GroupId| {
                    let live = !self.archived.contains(group);
                    !(active && !live || archived && live)
                };
                // As the real hub: by first_change, at most 1000 of a kind, one budget of envelope bytes for the whole
                // Desk (the first item always goes in), at most 20 000 registers, and `truncated` when one was cut.
                let mut bytes = 0usize;
                let mut any = false;
                let mut cut = false;
                let list = |request: bool, bytes: &mut usize, any: &mut bool, cut: &mut bool| -> Vec<Value> {
                    let mut open: Vec<_> = self.objects.iter().filter(|(_, o)| o.open && (o.card || o.request) && o.request == request && sees(&o.group)).collect();
                    open.sort_by_key(|(_, o)| o.first_change);
                    let mut rows = Vec::new();
                    for (id, o) in open {
                        if rows.len() >= DESK_MAX {
                            *cut = true;
                            break;
                        }
                        let version = item(o.version);
                        *bytes += version.as_ref().and_then(|v| v["envelope"].as_str()).map_or(0, str::len);
                        if *bytes > self.desk_bytes_max && *any {
                            *cut = true;
                            break;
                        }
                        rows.push(json!({ "object_id": id.to_base64url(), "group_id": b64(o.group.as_bytes()), "state": 1, "urgency": 1, "answered_at": 0,
                            "owner": DeviceId::new([0; 32]).to_base64url(), "first_change": self.first_change_lie.unwrap_or(o.first_change), "head_change": o.head_change,
                            "version": version }));
                        *any = true;
                    }
                    rows
                };
                let cards = list(false, &mut bytes, &mut any, &mut cut);
                let requests = list(true, &mut bytes, &mut any, &mut cut);
                let mut newest: Vec<(u64, GroupId)> = self.registers.iter().map(|((group, ..), change)| (*change, *group)).collect();
                newest.sort_unstable();
                let mut registers: Vec<Value> = Vec::new();
                for (change, group) in newest {
                    if !sees(&group) {
                        continue;
                    }
                    if registers.len() >= self.desk_registers_max {
                        cut = true;
                        break;
                    }
                    let Some(served) = item(change) else { continue };
                    bytes += served["envelope"].as_str().map_or(0, str::len);
                    if bytes > self.desk_bytes_max {
                        cut = true;
                        break;
                    }
                    registers.push(served);
                }
                Ok(json!({ "cards": cards, "permission_requests": requests, "notes": [], "artifacts": [], "registers": registers, "groups": [],
                    "truncated": self.desk_truncated || cut, "change": self.change }))
            }
            ("GET", ["v1", kind @ ("cards" | "permission-requests"), object]) => {
                let _ = kind;
                let id = trommi_core::ids::ObjectId::from_base64url(object).map_err(no)?;
                let o = self.objects.get(&id).ok_or(no(Error::NotFound))?;
                let (after, limit) = (num("after"), num("limit").clamp(1, 2000) as usize);
                let mut items: Vec<Value> = o.envelopes.iter().filter(|c| **c > after).filter_map(|c| self.order.iter().find(|i| i.change() == *c).map(Item::served)).collect();
                let more = items.len() > limit;
                items.truncate(limit);
                Ok(json!({ "object_id": object, "group_id": b64(o.group.as_bytes()), "state": 1, "urgency": 1, "owner": DeviceId::new([0; 32]).to_base64url(),
                    "first_change": o.first_change, "head_change": o.head_change, "items": items, "more": more }))
            }
            ("PUT", ["v1", "key-packages"]) => {
                let list = |name: &str| -> Vec<Vec<u8>> {
                    body.get(name).and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().and_then(|s| base64url_decode(s).ok())).collect()
                };
                let mut parts = vec![bytes("last_resort")];
                parts.extend(list("single_use"));
                if parts.iter().all(Vec::is_empty) {
                    return Ok(json!({ "unused": self.unused(&device) }));
                }
                self.post(&device, &OutboxEntry { id: 0, kind: OutboxKind::KeyPackages, group: None, epoch: 0, parts })
            }
            ("GET", ["v1", "rooms", _, "groups"]) => {
                // (every group in one list, numbered from 1 in a fixed order; `stale` is not told)
                let (after, limit) = (num("after") as usize, num("limit").clamp(1, 1000) as usize);
                let limit = limit.min(self.groups_max.unwrap_or(usize::MAX));
                let history = self.room.as_ref().and_then(Observer::history);
                let facts = Sessions { sessions: &self.sessions, archived: &self.archived };
                let row = |o: &Observer, kind: &str| {
                    let session = o.session();
                    let parent = session.filter(|s| s.parent != SessionId::ZERO).map(|s| s.parent.to_base64url());
                    // (5.2.8 under the newest room state, the one predicate of the hub and every device)
                    let stale = history.is_some_and(|h| o.staleness(h, h.newest(), &facts).is_ok_and(|s| s.is_stale()));
                    json!({ "group_id": b64(o.group().as_bytes()), "kind": kind, "epoch": o.epoch().unwrap_or(0), "room_epoch": o.previous_room_epoch(),
                        "leaves": o.leaves().unwrap_or_default().iter().map(DeviceId::to_base64url).collect::<Vec<_>>(),
                        "live": !self.archived.contains(&o.group()), "stale": stale, "parent": parent, "session_id": session.map(|s| s.session_id.to_base64url()) })
                };
                let mut rows: Vec<Value> = self.room.iter().map(|o| row(o, "room")).collect();
                rows.extend(self.sessions.values().map(|o| row(o, if o.session().is_some_and(|s| s.parent != SessionId::ZERO) { "helper" } else { "main" })));
                let page: Vec<Value> = rows.iter().skip(after).take(limit).cloned().collect();
                let end = after + page.len();
                Ok(json!({ "items": page, "more": end < rows.len(), "after": end }))
            }
            ("POST", ["v1", "key-packages", "claim"]) => {
                // (a human device claims; an agent only as the opener of a helper session, which no test asks here)
                if !self.humans().contains(&device) {
                    return Err(no(Error::Forbidden));
                }
                let asked: Vec<DeviceId> = body["devices"].as_array().into_iter().flatten().filter_map(|d| d.as_str().and_then(|s| DeviceId::from_base64url(s).ok())).collect();
                let has = |p: &Pool| !p.single_use.is_empty() || p.last_resort.is_some();
                if asked.is_empty() || asked.len() != body["devices"].as_array().map_or(0, Vec::len) {
                    return Err(no(Error::BadFormat));
                }
                // (all or nothing, unless `partial`: then the devices without one are left out)
                if body["partial"] != json!(true) && !asked.iter().all(|d| self.pools.get(d).is_some_and(has)) {
                    return Err(no(Error::NotFound));
                }
                let mut claimed = serde_json::Map::new();
                let mut missing = Vec::new();
                for device in asked {
                    match self.pools.get_mut(&device).filter(|p| has(p)) {
                        Some(pool) => {
                            let package = pool.single_use.pop().or_else(|| pool.last_resort.clone()).expect("checked");
                            claimed.insert(device.to_base64url(), json!(b64(&package)));
                        }
                        None => missing.push(device.to_base64url()),
                    }
                }
                let mut answer = json!({ "key_packages": claimed });
                if body["partial"] == json!(true) {
                    answer["missing"] = json!(missing);
                }
                Ok(answer)
            }
            ("POST", ["v1", "envelopes"]) => self.post(&device, &OutboxEntry { id: 0, kind: OutboxKind::Envelope, group: None, epoch: 0, parts: vec![bytes("envelope")] }),
            ("POST", ["v1", "groups"]) => {
                let parts = vec![bytes("group_info_0"), bytes("sealed_key_0"), bytes("commit"), bytes("group_info"), bytes("welcome"), bytes("sealed_key")];
                self.post(&device, &OutboxEntry { id: 0, kind: OutboxKind::GroupFounding, group: None, epoch: 0, parts })
            }
            ("POST", ["v1", "groups", _, "commits"]) => {
                let external = body.get("recovery_auth").is_some_and(Value::is_string);
                let parts = if external {
                    vec![bytes("commit"), bytes("group_info"), bytes("sealed_key"), bytes("recovery_auth")]
                } else {
                    vec![bytes("commit"), bytes("group_info"), bytes("welcome"), bytes("sealed_key")]
                };
                let kind = if external { OutboxKind::ExternalCommit } else { OutboxKind::Commit };
                self.post(&device, &OutboxEntry { id: 0, kind, group: Some(group_at(2)?), epoch: body["epoch"].as_u64().unwrap_or(0), parts })
            }
            ("POST", ["v1", "groups", _, "messages"]) => {
                let kind = if body["relay"] == json!(true) { OutboxKind::RelayMessage } else { OutboxKind::Message };
                self.post(&device, &OutboxEntry { id: 0, kind, group: Some(group_at(2)?), epoch: body["epoch"].as_u64().unwrap_or(0), parts: vec![bytes("message")] })
            }
            ("POST", ["v1", "groups", _, "reject"]) => Ok(json!({ "id": 1 })),
            ("POST", ["v1", "requests"]) => {
                let kind = body["kind"].as_str().filter(|k| ["readmit", "handover", "session"].contains(k)).ok_or(no(Error::BadFormat))?.to_string();
                let group = body["group"].as_str().and_then(|g| base64url_decode(g).ok()).and_then(|b| GroupId::from_bytes(&b).ok());
                Ok(json!({ "id": self.wish(&device, &kind, group, now) }))
            }
            ("GET", ["v1", "requests"]) => {
                // (a human device sees every wish of the room, another device its own)
                let human = self.humans().contains(&device);
                Ok(Value::Array(
                    self.wishes
                        .iter()
                        .filter(|(_, d, ..)| human || *d == device)
                        .map(|(id, d, kind, group, at)| json!({ "id": id, "device": d.to_base64url(), "kind": kind, "group_id": group.map(|g| b64(g.as_bytes())), "key_package": null, "n": null, "at": at, "committer": null }))
                        .collect(),
                ))
            }
            ("POST", ["v1", "groups", _, "archive"]) => {
                let group = group_at(2)?;
                if group.is_room() || !self.sessions.contains_key(&group) {
                    return Err(no(Error::NotFound));
                }
                self.archive(&group);
                Ok(json!({ "archived": true }))
            }
            _ => Err(no(Error::NotFound)),
        }
    }

    /// A stream is opened: its head, then what the hub's order holds after `after`, then `here`.
    pub fn open_stream(&mut self, now: u64, request: &HttpRequest) -> Vec<StreamItem> {
        let path = request.url.strip_prefix(HUB_URL).unwrap_or("").to_string();
        self.seen.push(format!("STREAM {path}"));
        let Some(device) = self.device_of(now, request) else {
            return vec![
                StreamItem::Opened { status: 401, headers: Vec::new() },
                StreamItem::Chunk { bytes: json!({ "error": "unauthorised", "message": "refused" }).to_string().into_bytes().into() },
                StreamItem::Ended,
            ];
        };
        let after: u64 = path.split_once("after=").and_then(|(_, n)| n.parse().ok()).unwrap_or(0);
        let mut bytes = Vec::new();
        for item in self.order.iter().filter(|i| i.change() > after && !self.hidden.contains(&i.change())) {
            bytes.extend(item.sse());
        }
        bytes.extend(b"event: here\ndata: {}\n\n");
        self.streaming.retain(|(d, ..)| *d != device);
        self.streaming.push((device, self.change.max(after), self.notes.len()));
        vec![StreamItem::Opened { status: 200, headers: vec![Header { name: "content-type".into(), value: "text/event-stream".into() }] }, StreamItem::Chunk { bytes: bytes.into() }]
    }

    /// What the open stream of `device` has not been sent yet, as one chunk; none when nothing is new.
    pub fn stream_news(&mut self, device: &DeviceId) -> Option<Vec<u8>> {
        let at = self.streaming.iter().position(|(d, ..)| d == device)?;
        let (_, sent, noted) = self.streaming[at];
        let mut bytes = Vec::new();
        for item in self.order.iter().filter(|i| i.change() > sent && !self.hidden.contains(&i.change())) {
            bytes.extend(item.sse());
        }
        for (to, note) in &self.notes[noted..] {
            if to.contains(device) {
                bytes.extend(note);
            }
        }
        self.streaming[at] = (*device, self.change.max(sent), self.notes.len());
        (!bytes.is_empty()).then_some(bytes)
    }
}

pub fn answer(status: u16, body: Value) -> HttpResponse {
    HttpResponse { status, headers: Vec::new(), body: if body.is_null() { Vec::new().into() } else { body.to_string().into_bytes().into() } }
}

/// An answer with a `Retry-After` in seconds.
pub fn later(status: u16, seconds: u64) -> HttpResponse {
    HttpResponse { status, headers: vec![Header { name: "Retry-After".into(), value: seconds.to_string() }], body: Vec::new().into() }
}
