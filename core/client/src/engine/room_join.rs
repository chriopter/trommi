//! The join of the room with the recovery code (8.4, 8.5): a device that is not in the room yet joins the room
//! group from outside. It ends with the device in the room; then the engine joins the session groups the same
//! way ([`super::joins`]) and runs as for any device.
//!
//! # What is read, and why
//!
//! Signed in with the code's key (the hub serves a room's keys and logs only to a member or to its recovery key):
//!
//! 1. every `SealedKey` and `RecoveryLink` of the room (`GET /v1/sealed-keys`): the anchor is the newest row of
//!    the room group whose MAC holds under the code, the one starting point the code authenticates
//!    ([`select_anchor`]); the rows also hand the device the content keys of the past, so that history opened
//!    later can be read. Keys, never content.
//! 2. the room group's GroupInfos of epoch 0 (where the walk starts), of the anchor's epoch (held against the
//!    anchor) and the current one (what the join is built on, and what the walk must arrive at).
//! 3. the room's live session groups (`GET /v1/rooms/{room}/groups`): their checks come after the join of the
//!    room (`code_check_defer`), and they are joined next.
//! 4. the room group's Commits below the current epoch (`GET /v1/groups/{room}/log?kind=commit`), handed to the
//!    protocol core's check in slices: the walk from the founding that proves who holds which role.
//!
//! Nothing else: no envelope, no Desk, no Chat. Those come after, as for any device.
//!
//! # What is right by construction
//!
//! - **One read at a time, each bounded.** Every answer goes through the strict codec; the lists read are capped
//!   ([`KEYS_BYTES_MAX`], [`SESSIONS_MAX`]); the walk is the core's, in slices of its size.
//! - **No half device.** Until the join is posted, any failure takes the device's whole stored state away
//!   (`DeviceApply` deleting every key): the next try begins with nothing.
//! - **No ghost device.** Once the join was posted and its answer not read (lost, or the process died), the
//!   device is never wiped: the hub may hold its leaf. Before it is posted again, the room group's current epoch
//!   is read: still the join's epoch, so it is posted again; moved on, so the Commit at the join's epoch is read
//!   from the log, and is either this device's (accepted under that change) or another's (refused, and the room
//!   is read again).
//! - **Restart-safe by the device's own outbox.** The posted join is the one thing that must survive a restart,
//!   and the device's outbox holds it (with the copy of the state it was built on). A device that opens in no
//!   room with such an entry is [`OpenError::Interrupted`]: with the code in hand again, the join goes on from it.
//!   The engine's record keeps nothing of the join, and the code's keys are never stored.

use super::joins::{Later, SessionJoins};
use super::*;
use crate::hub::{GroupRowKind, SealedKeysPage};
use trommi_core::crypto::SigningKey;
use trommi_core::device::CodeJoin;
use trommi_core::hub_auth::SignedHubAuth;
use trommi_core::recovery::{select_anchor, PlacedCommit, RecoveryKeys, ServedCommit, ServedEnd, ServedStart};

/// Sealed keys asked for in one page (the hub serves at most 2000).
pub const KEYS_PAGE: u32 = 1000;
/// The most bytes of sealed keys and links the join holds.
pub const KEYS_BYTES_MAX: usize = 64 << 20;
/// The most live session groups the join takes; a room with more is refused as `too-large`.
pub const SESSIONS_MAX: usize = 8 * 1000;
/// The room is read and the join built at most so many times (it moved on under each).
pub const ROOM_JOIN_TRIES: u32 = 3;
/// Commits handed to the core's check in one slice; halved when the core says `too-large`.
const SLICE: usize = 200;

/// What [`Engine::join_room`] takes: the hub (canonical), the room and the keys of the code in hand.
pub struct RoomJoin {
    pub hub: String,
    pub room: RoomId,
    pub keys: RecoveryKeys,
    /// A recovery (8.7): every other human device is removed and the code replaced; this seals the new code into
    /// the account's new copies.
    pub recover: Option<super::recover::Copies>,
}

/// Where the join stands, for the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomJoinStep {
    /// The room's keys, GroupInfos and sessions are read.
    Reading,
    /// The room's Commits are checked from its founding.
    Checking,
    /// The join is posted.
    Joining,
}

/// Why a join of the room with the code did not happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomJoinError {
    /// The code is not this room's: the hub refused its key, or no row of the room opens under it.
    WrongCode,
    /// The hub knows no such room.
    NotFound,
    /// The room holds as many human devices as it may.
    TooMany,
    /// This slot holds a device in a room already.
    RoomExists,
    /// A join was posted and its answer never read; the code is asked for again, and the join goes on from it.
    Interrupted,
    /// The room moved on under every try ([`ROOM_JOIN_TRIES`]).
    Busy,
    /// The room does not verify (the core's code): it is not joined.
    Unverified(&'static str),
    /// The hub refused the join or a read for good (its code).
    Refused(&'static str),
    /// The device failed in itself (its code).
    Device(&'static str),
}

impl RoomJoinError {
    pub fn code(&self) -> &'static str {
        match self {
            RoomJoinError::WrongCode => "wrong-recovery",
            RoomJoinError::NotFound => "not-found",
            RoomJoinError::TooMany => "too-many",
            RoomJoinError::RoomExists => "room-exists",
            RoomJoinError::Interrupted => "interrupted",
            RoomJoinError::Busy => "busy",
            RoomJoinError::Unverified(code) | RoomJoinError::Refused(code) | RoomJoinError::Device(code) => code,
        }
    }

    /// The core's error, sorted.
    pub(super) fn of(error: &Error) -> Self {
        match error {
            Error::WrongRecovery => RoomJoinError::WrongCode,
            Error::TooMany => RoomJoinError::TooMany,
            Error::RoomExists => RoomJoinError::RoomExists,
            Error::Storage(_) | Error::Entropy | Error::Internal(_) => RoomJoinError::Device(error.code()),
            _ => RoomJoinError::Unverified(error.code()),
        }
    }
}

/// How the join stands. `Joined` comes once; the sessions follow as [`Report::SessionsJoined`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomJoinStatus {
    Step(RoomJoinStep),
    /// A read or the post failed for now (offline, not now, a 5xx): tried again at `retry_at`.
    Waiting { step: RoomJoinStep, retry_at: u64 },
    Joined,
    Failed(RoomJoinError),
}

/// A read of the join in flight.
#[derive(Clone, Copy, Debug)]
pub(super) enum Read {
    Keys,
    /// A GroupInfo of the room group: the current one (`None`) or of an epoch.
    Info(Option<u64>),
    Sessions,
    Log,
    /// The room group's log, looking for the Commit at the epoch of the join posted earlier.
    Find,
    /// A recovery's reads: a session group's founding (`true`) or current GroupInfo, a group's Commits, a removed
    /// device's chain, the recovery opened.
    Session(GroupId, bool),
    SessionLog(GroupId),
    Chain,
    Open,
}

/// The join while it runs. The keys of the code live here only.
pub(super) struct Joiner {
    pub(super) keys: RecoveryKeys,
    pub(super) retry: Retry,
    pub(super) asked: Option<IoId>,
    pub(super) tries: u32,
    pub(super) step: RoomJoinStep,
    // what was read
    pub(super) rows: Vec<Vec<u8>>,
    pub(super) links: Vec<Vec<u8>>,
    pub(super) bytes: usize,
    pub(super) keys_after: u64,
    pub(super) keys_done: bool,
    pub(super) anchor_epoch: Option<u64>,
    pub(super) current: Option<(u64, Vec<u8>)>,
    pub(super) founding: Option<Vec<u8>>,
    pub(super) anchor: Option<Vec<u8>>,
    pub(super) main: Vec<GroupId>,
    pub(super) helpers: Vec<GroupId>,
    pub(super) sessions_after: u64,
    pub(super) sessions_done: bool,
    /// The core holds the check.
    pub(super) started: bool,
    pub(super) log_after: u64,
    pub(super) reached: u64,
    /// The join in the device's outbox, and whether it may be posted as it is (else the room's epoch is read
    /// first: its answer may have been lost).
    pub(super) entry: Option<u64>,
    /// The epoch the join is built on.
    pub(super) entry_epoch: u64,
    pub(super) may_post: bool,
    /// The hub answered `epoch-taken`: the core holds the join back until the room's log shows whose Commit
    /// took that epoch (13.2).
    pub(super) awaiting: bool,
    pub(super) find_after: u64,
    /// A join of this run left for the hub: from then on the device is never wiped.
    pub(super) posted: bool,
    /// The room moved on under the join: it is read again from the start.
    pub(super) restart: bool,
    /// A recovery (8.7) instead of a join.
    pub(super) recover: Option<super::recover::Recovering>,
}

/// The device's join of the room group, posted and not answered yet.
pub(super) fn staged(device: &Device<memstore::MemStore>) -> Option<OutboxEntry> {
    device.outbox().into_iter().find(|e| e.kind == OutboxKind::ExternalCommit && e.group.is_some_and(|g| g.is_room()))
}

impl Joiner {
    fn new(keys: RecoveryKeys, entry: Option<(u64, u64)>) -> Self {
        Joiner {
            keys,
            retry: Retry::default(),
            asked: None,
            tries: 0,
            step: if entry.is_some() { RoomJoinStep::Joining } else { RoomJoinStep::Reading },
            rows: Vec::new(),
            links: Vec::new(),
            bytes: 0,
            keys_after: 0,
            keys_done: false,
            anchor_epoch: None,
            current: None,
            founding: None,
            anchor: None,
            main: Vec::new(),
            helpers: Vec::new(),
            sessions_after: 0,
            sessions_done: false,
            started: false,
            log_after: 0,
            reached: 0,
            entry: entry.map(|(id, _)| id),
            entry_epoch: entry.map_or(0, |(_, epoch)| epoch),
            may_post: false,
            awaiting: false,
            find_after: 0,
            posted: entry.is_some(),
            restart: false,
            recover: None,
        }
    }

    /// Everything read is forgotten: the room is read again from the start.
    fn again(self) -> Self {
        let mut next = Joiner::new(self.keys, None);
        (next.tries, next.retry, next.posted) = (self.tries, self.retry, self.posted);
        next.recover = self.recover.and_then(|r| r.again());
        next
    }

    /// The hub's sign-in signed with the code's key.
    pub(super) fn sign_in(&self, room: RoomId, address: &HubAddress, challenge: [u8; 32]) -> Result<SignedHubAuth, Error> {
        let key: &SigningKey = self.keys.signing_key();
        trommi_core::hub_auth::sign(key, room, address, challenge)
    }
}

impl Engine {
    /// Joins `join.room` with the code, on the device state the shell holds in this slot (`revision`, `entries`:
    /// what `DeviceLoad` brought; empty for a new slot). A device in no room is made anew, unless it holds a join
    /// of this room posted earlier, which goes on. `room-exists` when the slot holds a device in a room.
    #[allow(clippy::too_many_arguments)]
    pub fn join_room(
        client: &str,
        seed: u64,
        revision: u64,
        entries: Vec<DeviceEntry>,
        join: RoomJoin,
        entropy: Box<dyn Entropy + Send>,
    ) -> Result<Self, RoomJoinError> {
        let address = HubAddress::parse(&join.hub).map_err(|_| RoomJoinError::Refused("bad-hub"))?;
        if address.as_str() != join.hub {
            return Err(RoomJoinError::Refused("bad-hub"));
        }
        let entries: Vec<(Vec<u8>, Vec<u8>)> = entries.into_iter().map(|e| (e.key.0, e.value.0)).collect();
        let mut entropy = Some(entropy);
        let mut staged_entry = None;
        let (device, writes) = if revision == 0 || entries.is_empty() {
            let (store, writes) = memstore::fresh(revision, Vec::new());
            (Device::create(store, entropy.take().expect("entropy")).map_err(|e| RoomJoinError::Device(e.code()))?, writes)
        } else {
            let stale: Vec<Vec<u8>> = entries.iter().map(|(k, _)| k.clone()).collect();
            let (store, writes) = memstore::wipeable(revision, entries);
            match Device::open(store, entropy.take().expect("entropy")) {
                Ok(device) if device.room().is_some() => return Err(RoomJoinError::RoomExists),
                Ok(device) if staged(&device).is_some_and(|e| e.group == Some(GroupId::room(join.room))) => {
                    staged_entry = staged(&device).map(|e| (e.id, e.epoch));
                    (device, writes)
                }
                // (a device in no room, without a join of this room: nothing of it is kept)
                Ok(_) | Err(_) => {
                    let (store, writes) = memstore::fresh(revision, stale);
                    (Device::create(store, Box::new(SystemEntropy)).map_err(|e| RoomJoinError::Device(e.code()))?, writes)
                }
            }
        };
        let mut engine = Self::over(device, writes, join.room, &join.hub, address, client, seed);
        let mut joiner = Joiner::new(join.keys, staged_entry);
        joiner.recover = join.recover.map(super::recover::Recovering::new);
        engine.room_join = Some(joiner);
        Ok(engine)
    }

    /// The ids of everything in flight, forgotten: the App drops this engine (a join of the room that failed).
    pub fn forget(&mut self) -> Vec<IoId> {
        std::mem::take(&mut self.pending).into_keys().collect()
    }

    /// Whether the room is being joined with the code.
    pub fn joining_room(&self) -> bool {
        self.room_join.is_some()
    }

    /// The hub this engine runs against.
    pub fn hub(&self) -> &str {
        &self.hub
    }

    /// The first step of a join with the code (in place of [`Engine::start`]: there is no record to read).
    pub fn start_room_join(&mut self, ctx: &mut Ctx<'_>) {
        let step = self.room_join.as_ref().map_or(RoomJoinStep::Reading, |j| j.step);
        self.report(Report::RoomJoin(RoomJoinStatus::Step(step)));
        self.drive(ctx);
    }

    pub(super) fn room_status(&mut self, j: &Joiner) {
        self.report(Report::RoomJoin(RoomJoinStatus::Step(j.step)));
    }

    /// The join ends without the room: the device's state goes unless a join of it may stand at the hub.
    fn room_failed(&mut self, ctx: &mut Ctx<'_>, j: Joiner, error: RoomJoinError) {
        // (a recovery opened and not finished gives the room back; its device holds nothing the hub keeps)
        self.recover_drop(ctx, &j);
        let keep = j.posted && j.entry.is_some();
        if !keep {
            // (what the device wrote first, then everything gone in one write)
            self.flush(ctx);
            if let Some(write) = self.writes.wipe() {
                let id = ctx.id();
                self.pending.insert(id, Asked::DeviceWrite);
                ctx.out.push(Effect::Store {
                    id,
                    op: StoreOp::DeviceApply { expected_revision: write.expected_revision, put: Vec::new(), delete: write.delete.into_iter().map(Into::into).collect() },
                });
            }
        }
        ctx.log(LogLevel::Warn, format!("room join: {}", error.code()));
        drop(j);
        self.token = None;
        self.report(Report::RoomJoin(RoomJoinStatus::Failed(error)));
    }

    /// A transient failure: the same step again after a wait.
    pub(super) fn room_wait(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner, failure: &Failure) {
        let retry_after = match failure {
            Failure::NotNow { retry_after_ms, .. } => *retry_after_ms,
            Failure::BadAnswer { why } => {
                self.report(Report::Alert(Alert::BadAnswer { why }));
                None
            }
            _ => None,
        };
        j.retry.fail(ctx.now, retry_after, &mut self.jitter);
        if let Some(retry_at) = j.retry.at {
            self.report(Report::RoomJoin(RoomJoinStatus::Waiting { step: j.step, retry_at }));
        }
    }

    /// The challenge or the token was refused: for good when the hub said so.
    pub(super) fn room_auth_failed(&mut self, ctx: &mut Ctx<'_>, failure: Failure) {
        let Some(mut j) = self.room_join.take() else { return };
        match &failure {
            Failure::Refused { code: Some(code), .. } if code == "not-found" => self.room_failed(ctx, j, RoomJoinError::NotFound),
            // (a key the room does not name: the code is not this room's)
            Failure::Refused { code: Some(_), .. } => self.room_failed(ctx, j, RoomJoinError::WrongCode),
            _ => {
                self.auth_failed(ctx, failure.clone());
                if let Some(retry_at) = self.auth_retry.at {
                    self.report(Report::RoomJoin(RoomJoinStatus::Waiting { step: j.step, retry_at }));
                }
                j.retry = Retry::default();
                self.room_join = Some(j);
            }
        }
    }

    pub(super) fn drive_room_join(&mut self, ctx: &mut Ctx<'_>) {
        let Some(mut j) = self.room_join.take() else { return };
        if j.asked.is_some() || j.retry.waiting(ctx.now) {
            self.room_join = Some(j);
            return;
        }
        let result = self.room_next(ctx, &mut j);
        self.room_after(ctx, j, result);
    }

    /// After a step: failed, in the room, read again, or on.
    fn room_after(&mut self, ctx: &mut Ctx<'_>, mut j: Joiner, result: Result<(), RoomJoinError>) {
        match result {
            Err(error) => self.room_failed(ctx, j, error),
            Ok(()) if j.entry.is_none() && j.keys_done && j.sessions_done && self.device.room() == Some(self.room) => self.room_joined(ctx, j),
            Ok(()) => {
                if j.restart {
                    j = j.again();
                    self.room_status(&j);
                }
                self.room_join = Some(j);
            }
        }
    }

    pub(super) fn room_ask(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner, route: Route<'_>, what: Read) -> Result<(), RoomJoinError> {
        j.asked = self.ask(ctx, route, true, |expect| Asked::RoomRead { expect, what });
        j.asked.map(|_| ()).ok_or(RoomJoinError::Device("bad-format"))
    }

    /// Asks for what comes next, or builds and posts the join once all is read.
    fn room_next(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner) -> Result<(), RoomJoinError> {
        let room = self.room;
        let group = GroupId::room(room);
        if let Some(id) = j.entry {
            if j.awaiting || !j.may_post {
                return match j.current {
                    None if !j.awaiting => self.room_ask(ctx, j, Route::GroupInfo { group: &group, epoch: None }, Read::Info(None)),
                    _ => self.room_ask(ctx, j, Route::GroupLog { group: &group, after: j.find_after, limit: LOG_PAGE, commits_only: true }, Read::Find),
                };
            }
            let Some(entry) = self.device.outbox().into_iter().find(|e| e.id == id) else {
                return Err(RoomJoinError::Device("internal"));
            };
            j.step = RoomJoinStep::Joining;
            let kind = entry.kind;
            self.posting = self.ask(ctx, Route::Outbox { room: &room, entry: &entry, extra: None }, true, |expect| Asked::Post { expect, entry: id, kind, group: Some(group) });
            j.asked = self.posting;
            j.posted = true;
            return j.asked.map(|_| ()).ok_or(RoomJoinError::Device("bad-format"));
        }
        if !j.keys_done {
            return self.room_ask(ctx, j, Route::SealedKeys { after: j.keys_after, limit: KEYS_PAGE }, Read::Keys);
        }
        if self.device.room() == Some(room) {
            // (a join that went on after a restart is through: what the sessions' joins need is read, no more)
            if !j.sessions_done {
                return self.room_ask(ctx, j, Route::RoomGroups { room: &room, after: j.sessions_after, limit: crate::hub::limits::MAX_GROUPS_PAGE }, Read::Sessions);
            }
            return Ok(());
        }
        let anchor_epoch = match j.anchor_epoch {
            Some(epoch) => epoch,
            None => {
                let anchor = select_anchor(&j.keys, &room, &j.rows).map_err(|e| RoomJoinError::of(&e))?;
                j.anchor_epoch = Some(anchor.epoch);
                anchor.epoch
            }
        };
        let Some((current_epoch, current)) = j.current.clone() else {
            return self.room_ask(ctx, j, Route::GroupInfo { group: &group, epoch: None }, Read::Info(None));
        };
        if j.founding.is_none() {
            if current_epoch == 0 {
                j.founding = Some(current.clone());
            } else {
                return self.room_ask(ctx, j, Route::GroupInfo { group: &group, epoch: Some(0) }, Read::Info(Some(0)));
            }
        }
        if j.anchor.is_none() {
            if anchor_epoch == 0 {
                j.anchor = j.founding.clone();
            } else if anchor_epoch == current_epoch {
                j.anchor = Some(current.clone());
            } else {
                return self.room_ask(ctx, j, Route::GroupInfo { group: &group, epoch: Some(anchor_epoch) }, Read::Info(Some(anchor_epoch)));
            }
        }
        if !j.sessions_done {
            return self.room_ask(ctx, j, Route::RoomGroups { room: &room, after: j.sessions_after, limit: crate::hub::limits::MAX_GROUPS_PAGE }, Read::Sessions);
        }
        if j.recover.is_some() {
            let sessions: Vec<GroupId> = j.main.iter().chain(&j.helpers).copied().collect();
            let (founding, anchor) = (j.founding.clone().unwrap_or_default(), j.anchor.clone().unwrap_or_default());
            return self.recover_next(ctx, j, sessions, (current_epoch, current), founding, anchor);
        }
        if !j.started {
            j.step = RoomJoinStep::Checking;
            self.room_status(j);
            let (founding, anchor) = (j.founding.clone().unwrap_or_default(), j.anchor.clone().unwrap_or_default());
            self.device.code_check_start(&j.keys, &ServedStart { room, founding: &founding, anchor: &anchor, rows: &j.rows, sessions: &[] }).map_err(|e| RoomJoinError::of(&e))?;
            let later: Vec<GroupId> = j.main.iter().chain(&j.helpers).copied().collect();
            self.device.code_check_defer(&later).map_err(|e| RoomJoinError::of(&e))?;
            j.started = true;
        }
        if j.reached < current_epoch {
            return self.room_ask(ctx, j, Route::GroupLog { group: &group, after: j.log_after, limit: LOG_PAGE, commits_only: true }, Read::Log);
        }
        // the walk reached the current GroupInfo: the join is built on it
        j.step = RoomJoinStep::Joining;
        self.room_status(j);
        let built: CodeJoin = self
            .device
            .join_room_checked(&j.keys, &ServedEnd { current: &current, sessions: &[], rows: &j.rows, links: &j.links }, ctx.now)
            .map_err(|e| RoomJoinError::of(&e))?;
        if built.missing_link.is_some() {
            self.report(Report::Alert(Alert::MissingLink));
        }
        j.entry = built.outbox.last().copied();
        j.entry_epoch = current_epoch;
        j.may_post = true;
        if j.entry.is_none() {
            return Err(RoomJoinError::Device("internal"));
        }
        // (stored before it is posted: the barrier)
        self.room_next(ctx, j)
    }

    /// The Commits of a page of the room group's log, handed to the check below the current epoch.
    fn room_slices(&mut self, j: &mut Joiner, log: &[LogItem]) -> Result<(), RoomJoinError> {
        let upto = j.current.as_ref().map_or(0, |(epoch, _)| *epoch);
        let group = GroupId::room(self.room);
        let commits: Vec<&LogItem> = log.iter().filter(|c| c.n > j.log_after && c.epoch < upto).collect();
        let (mut at, mut size) = (0, SLICE);
        while at < commits.len() {
            let slice: Vec<PlacedCommit<'_>> = commits[at..(at + size).min(commits.len())]
                .iter()
                .map(|c| PlacedCommit { group, commit: ServedCommit { change: c.change, commit: &c.bytes, recovery_auth: c.recovery_auth.as_deref() } })
                .collect();
            match self.device.code_check_slice(&slice) {
                Ok(()) => {
                    at += slice.len();
                    let last = commits[at - 1];
                    (j.log_after, j.reached) = (last.n, last.epoch + 1);
                }
                Err(Error::TooLarge) if size > 1 => size /= 2,
                Err(e) => return Err(RoomJoinError::of(&e)),
            }
        }
        Ok(())
    }

    pub(super) fn on_room_read(&mut self, ctx: &mut Ctx<'_>, what: Read, answer: Result<Answer, Failure>, unauthorised: bool, _refusal: Option<Refusal>) {
        let Some(mut j) = self.room_join.take() else { return };
        j.asked = None;
        let result = self.room_read(ctx, &mut j, what, answer, unauthorised);
        self.room_after(ctx, j, result);
    }

    fn room_read(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner, what: Read, answer: Result<Answer, Failure>, unauthorised: bool) -> Result<(), RoomJoinError> {
        let answer = match answer {
            Ok(answer) => answer,
            // (signed in again, and asked again)
            Err(_) if unauthorised => return Ok(()),
            Err(Failure::Refused { code: Some(code), .. }) => {
                return Err(match Error::from_code(&code) {
                    Some(Error::NotFound) if matches!(what, Read::Keys | Read::Info(None)) => RoomJoinError::NotFound,
                    Some(e) => RoomJoinError::Refused(e.code()),
                    None => RoomJoinError::Refused("refused"),
                });
            }
            Err(failure) => {
                self.room_wait(ctx, j, &failure);
                return Ok(());
            }
        };
        j.retry.ok();
        if matches!(what, Read::Session(..) | Read::SessionLog(_) | Read::Chain | Read::Open) {
            return self.recover_read(j, what, answer);
        }
        let room = GroupId::room(self.room);
        match (what, answer) {
            (Read::Keys, Answer::SealedKeys(SealedKeysPage { rows, links, change, more })) => {
                j.bytes += rows.iter().map(|(_, b)| b.len()).chain(links.iter().map(|l| l.recovery_link.len())).sum::<usize>();
                if j.bytes > KEYS_BYTES_MAX {
                    return Err(RoomJoinError::Refused("too-large"));
                }
                j.rows.extend(rows.into_iter().map(|(_, b)| b));
                j.links.extend(links.into_iter().map(|l| l.recovery_link));
                // (the codec holds the cursor to where the lists end, and to move on when there is more)
                j.keys_after = change;
                j.keys_done = !more;
            }
            (Read::Info(None), Answer::GroupInfo { epoch, group_info }) => {
                j.current = Some((epoch, group_info));
                // the room's epoch is the join's still: nobody took it, so it is posted (again)
                if j.entry.is_some() && epoch <= j.entry_epoch {
                    j.may_post = true;
                }
            }
            (Read::Info(Some(0)), Answer::GroupInfo { group_info, .. }) => j.founding = Some(group_info),
            (Read::Info(Some(_)), Answer::GroupInfo { group_info, .. }) => j.anchor = Some(group_info),
            (Read::Sessions, Answer::RoomGroups { items, more, after }) => {
                let any = !items.is_empty();
                for row in items.into_iter().filter(|r| r.live) {
                    match row.kind {
                        GroupRowKind::Main => j.main.push(row.group),
                        GroupRowKind::Helper => j.helpers.push(row.group),
                        GroupRowKind::Room => {}
                    }
                }
                if j.main.len() + j.helpers.len() > SESSIONS_MAX {
                    return Err(RoomJoinError::Refused("too-large"));
                }
                j.sessions_after = after;
                // (a page that brings nothing ends the list: one that does not move on would be asked for ever)
                j.sessions_done = !more || !any;
            }
            (Read::Log, Answer::GroupLog { items, more }) => {
                let log: Vec<LogItem> = items.into_iter().filter_map(|i| if let ChangeItem::Commit(l) = i { Some(l) } else { None }).collect();
                let before = j.log_after;
                self.room_slices(j, &log)?;
                let upto = j.current.as_ref().map_or(0, |(epoch, _)| *epoch);
                if j.reached < upto && (!more || j.log_after == before) {
                    // (the log ends before the epoch the GroupInfo names: the hub withholds a Commit)
                    return Err(RoomJoinError::Unverified("withheld"));
                }
            }
            (Read::Find, Answer::GroupLog { items, more }) => {
                if j.entry.is_none() {
                    return Ok(());
                }
                let before = j.find_after;
                for item in items {
                    let ChangeItem::Commit(c) = item else { continue };
                    j.find_after = j.find_after.max(c.n);
                    if c.epoch < j.entry_epoch || c.group != room {
                        continue;
                    }
                    // 13.2: the core decides whose Commit took the join's epoch, its own or another's
                    let entry = trommi_core::device::LogEntry { change: c.change, group: room, kind: LogKind::Commit { bytes: &c.bytes, recovery_auth: c.recovery_auth.as_deref() } };
                    match self.device.process_log_entry(&entry, ctx.now) {
                        Ok(Processed::OwnCommit) => {
                            j.entry = None;
                            return Ok(());
                        }
                        Ok(Processed::JoinSuperseded { .. }) => return self.room_superseded(j),
                        Ok(_) => continue,
                        Err(e) => return Err(RoomJoinError::of(&e)),
                    }
                }
                if !more || j.find_after == before {
                    // (the epoch moved on and the log has no Commit at the join's: the hub's word against itself)
                    return Err(RoomJoinError::Unverified("withheld"));
                }
            }
            _ => {
                self.room_wait(ctx, j, &Failure::BadAnswer { why: "an answer to another read" });
            }
        }
        Ok(())
    }

    /// The hub's answer to the posted join, or what the log said of it: in the room, or refused.
    fn room_answered(&mut self, ctx: &mut Ctx<'_>, j: &mut Joiner, entry: u64, answer: Result<Option<u64>, Error>) -> Result<(), RoomJoinError> {
        match answer {
            Ok(change) => match self.device.outbox_accepted(entry, trommi_core::device::Accepted { change }) {
                Ok(()) | Err(Error::NotFound) => self.room_settled(ctx, j),
                Err(e) => Err(RoomJoinError::of(&e)),
            },
            Err(code) => {
                match self.device.outbox_refused(entry, &code) {
                    Ok(()) | Err(Error::NotFound) => {}
                    Err(e) => return Err(RoomJoinError::of(&e)),
                }
                if code == Error::EpochTaken {
                    // (whose Commit took the epoch is read from the room's log: it may even be this one's)
                    (j.awaiting, j.may_post, j.find_after) = (true, false, 0);
                    return Ok(());
                }
                j.entry = None;
                j.posted = false;
                j.tries += 1;
                if has_moved(&code) && j.tries < ROOM_JOIN_TRIES {
                    // the room moved on under the join: read again, built again
                    j.restart = true;
                    return Ok(());
                }
                Err(match code {
                    _ if has_moved(&code) => RoomJoinError::Busy,
                    Error::TooMany => RoomJoinError::TooMany,
                    Error::NotFound => RoomJoinError::NotFound,
                    Error::Unauthorised | Error::Forbidden => RoomJoinError::WrongCode,
                    other => RoomJoinError::Refused(other.code()),
                })
            }
        }
    }

    /// The join was accepted: the device is in the room.
    fn room_settled(&mut self, _ctx: &mut Ctx<'_>, j: &mut Joiner) -> Result<(), RoomJoinError> {
        if self.device.room() != Some(self.room) {
            return Err(RoomJoinError::Device("internal"));
        }
        j.entry = None;
        Ok(())
    }

    /// Another Commit took the join's epoch: the core dropped the join, and the room is read again.
    fn room_superseded(&mut self, j: &mut Joiner) -> Result<(), RoomJoinError> {
        (j.entry, j.posted, j.awaiting) = (None, false, false);
        j.tries += 1;
        if j.tries >= ROOM_JOIN_TRIES {
            return Err(RoomJoinError::Busy);
        }
        j.restart = true;
        Ok(())
    }

    /// The hub's answer to a posted join.
    pub(super) fn on_room_posted(&mut self, ctx: &mut Ctx<'_>, entry: u64, answer: Result<Answer, Failure>, unauthorised: bool, refusal: Option<Refusal>) {
        let Some(mut j) = self.room_join.take() else { return };
        j.asked = None;
        if j.recover.as_ref().is_some_and(|r| r.posting() == Some(entry)) {
            let result = self.recover_posted(ctx, &mut j, entry, answer, unauthorised);
            return self.room_after(ctx, j, result);
        }
        let result = match answer {
            Ok(Answer::Outbox(taken)) => {
                j.retry.ok();
                self.room_answered(ctx, &mut j, entry, Ok(taken.change))
            }
            // (the hub's own refusal, by a code this build knows and that is not of the moment)
            Err(Failure::Refused { code: Some(code), .. }) if !unauthorised && Error::from_code(&code).is_some_and(|c| !refusal_is_passing(&c)) => {
                let code = Error::from_code(&code).unwrap_or(Error::BadFormat);
                self.room_answered(ctx, &mut j, entry, Err(code))
            }
            other => {
                // Anything else may have reached the hub: before it is posted again, the room's epoch is read.
                j.may_post = false;
                j.current = None;
                j.find_after = 0;
                if !unauthorised {
                    let failure = match other {
                        Err(failure) => failure,
                        Ok(_) => Failure::BadAnswer { why: "an outbox answer" },
                    };
                    let failure = match (failure, refusal) {
                        (Failure::Refused { .. }, Some(r)) => Failure::NotNow { status: 503, retry_after_ms: r.retry_after_ms },
                        (failure, _) => failure,
                    };
                    self.room_wait(ctx, &mut j, &failure);
                }
                Ok(())
            }
        };
        self.room_after(ctx, j, result);
    }

    /// In the room: the session groups are joined next with the same keys, and the engine runs as for any device.
    fn room_joined(&mut self, ctx: &mut Ctx<'_>, j: Joiner) {
        // (the recovery key's token ends here: from now on the device signs in as itself)
        self.token = None;
        self.position = self.device.cursor();
        self.todo_upto = self.position;
        self.report(Report::RoomJoin(RoomJoinStatus::Joined));
        self.report(Report::Groups);
        let recovered = j.recover.is_some();
        let Joiner { keys, rows, links, main, helpers, .. } = j;
        // (a recovery joined every live session with the room)
        let groups: Vec<GroupId> = if recovered { Vec::new() } else { main.into_iter().chain(helpers).collect() };
        self.join_sessions(ctx, SessionJoins { keys, later: Some(Later { rows, links }), groups });
    }
}
