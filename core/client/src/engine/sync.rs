//! The engine itself: one state, driven by events, answering with effects. See [`super`] for the design.

use crate::hub::{
    Answer, ChangeItem, Codec, DeskOnly, EnvelopeItem, Expect, GroupRead, LogItem, ObjectRoute, Only, OutboxAnswer, Presence, Refusal, Role, Route, Site,
    StreamEvent, StreamReader, WishKind,
};
use super::memstore::{self, Writes};
use super::{Backoff, Jitter};
use crate::app::io::{DeviceEntry, HttpResponse, IoId, IoResult, StoreAnswer, StoreOp, StreamItem};
use crate::app::{Effect, LogLevel};
use crate::hub::{classify_io, Failure};

#[path = "joins.rs"]
pub mod joins;
#[path = "upkeep.rs"]
pub mod upkeep;
#[path = "record.rs"]
pub mod record;
#[path = "events.rs"]
pub mod events;
#[path = "room_join.rs"]
pub mod room_join;
/// The recovery of the room with its code (8.7), inside the join.
#[path = "recover.rs"]
pub mod recover;
#[path = "requests.rs"]
pub mod requests;
#[path = "found.rs"]
pub mod found;
#[path = "link_join.rs"]
pub mod link_join;
#[path = "agent.rs"]
pub mod agent;
use std::collections::{BTreeMap, VecDeque};
use trommi_core::crypto::{Entropy, SecretBytes, SystemEntropy};
use trommi_core::device::{
    log_finding, Draft, refusal_is_passing, Accepted, Device, EnvelopeOutcome, LogEntry, LogFinding, LogKind, Processed, Received, ReceivedEnvelope,
    WelcomeExpectation,
};
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{DeviceId, FileId, GroupId, InviteId, ObjectId, RoomId};
use trommi_core::store::{OutboxEntry, OutboxKind};
use trommi_core::Error;

/// Items asked for in one page of the hub's order. Also the most the engine holds of the order at once.
pub const PAGE: u32 = 500;
/// Links asked for in one page of a sender's chain.
pub const CHAIN_PAGE: u32 = 500;
/// Entries asked for in one page of a group's log.
pub const LOG_PAGE: u32 = 200;
/// At most so many changes the stream brought wait for their turn; more are left to the next page.
pub const LIVE_MAX: usize = 512;
/// At most so many bytes of them.
pub const LIVE_BYTES_MAX: usize = 8 << 20;
/// After a read of the Welcomes that joined nothing, so many items that wait for a group pass before the next.
pub const WELCOMES_AGAIN: u32 = 64;
/// At most so many groups are noted as behind; beyond that one number says from where to read again.
pub const BEHIND_MAX: usize = 4096;
/// A catch-up that halted at an item is tried again after this long.
pub const HALTED_RETRY_MS: u64 = 15_000;
/// The pump tries a blocking entry again after this long.
pub const BLOCKED_RETRY_MS: u64 = 30_000;
/// A token is renewed this long before the engine takes it to end.
pub const TOKEN_RENEW_BEFORE_MS: u64 = 60_000;
/// A token lives at most this long (12.3.1), whatever the hub says.
pub const TOKEN_MAX_MS: u64 = 600_000;
/// A stream that brought nothing for this long is taken for dead (the hub pings every 20 s).
pub const STREAM_SILENT_MS: u64 = 75_000;
/// How often the timed upkeep runs: KeyPackages, `heads`, the keys still missing.
pub const UPKEEP_EVERY_MS: u64 = 600_000;
/// First versions of open cards asked for at the same time.
pub const FIRSTS_AT_ONCE: usize = 8;
/// At most so many reports wait for the App.
const REPORTS_MAX: usize = 4 * PAGE as usize;

/// Refusals that end an envelope without a void record and without a hole that matters: the group is archived, or
/// this device is out.
fn is_dead(code: &Error) -> bool {
    matches!(code, Error::Gone | Error::NotMember | Error::RemovedSender | Error::NoRoom)
}

/// Refusals that mean the group moved on under a Commit or message: the log is processed, and it is built again.
fn has_moved(code: &Error) -> bool {
    matches!(code, Error::EpochTaken | Error::RoomBehind | Error::WrongEpoch | Error::StaleSession | Error::GroupBehind)
}

fn carries_commit(kind: OutboxKind) -> bool {
    !matches!(
        kind,
        OutboxKind::Message | OutboxKind::RelayMessage | OutboxKind::Envelope | OutboxKind::KeyPackages | OutboxKind::SealedKey
    )
}

/// What the engine needs from the App to act: the clock, the App's id counter and where effects go.
pub struct Ctx<'a> {
    pub now: u64,
    pub next_id: &'a mut u64,
    pub out: &'a mut Vec<Effect>,
}

impl Ctx<'_> {
    fn id(&mut self) -> IoId {
        let id = IoId(*self.next_id);
        *self.next_id += 1;
        id
    }
    fn log(&mut self, level: LogLevel, text: impl Into<String>) {
        self.out.push(Effect::Log { level, text: text.into() });
    }
}

/// How the device stands with the hub.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    /// Nothing reached the hub yet, or the last try failed.
    Offline,
    /// The hub's order is being read up to its end.
    CatchingUp,
    /// At the end of the hub's order, and the stream is open.
    Live,
}

/// Why the engine does nothing more.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// A Commit of the room group removed this device.
    Removed,
    /// Another owner wrote the device state: this App is no owner of it any more (the shell opens it again).
    Store,
    /// A write of the device state failed otherwise (the store is full, refused): memory is ahead of what is stored.
    /// The App opens the stored state again in place.
    Write,
    /// The device itself failed (its store's contents, its entropy). The App opens the stored state again in place.
    Device,
    /// An agent device: another process took its lease (13.7). Of two processes on one state the later wins.
    LeaseLost,
    /// An agent device: the hub serves an envelope this device signed beyond what its stored state signed. The
    /// state is older than what the device sent; nothing is signed with it ([`agent`]).
    RolledBack,
    /// An agent device: a Commit removed it from its main session; another connector continues the session.
    Replaced,
    /// An agent device: the hub refuses it as no member, and no Commit it still serves removed it. The state is
    /// kept.
    NotMember,
    /// An agent device: the room was told of it by another device than the one that invited it (12.1.6).
    BadInvite,
}

impl Stopped {
    /// Whether the App opens the stored state again in place: the stored state is the last good one, and nobody
    /// else holds it.
    pub fn reopens(self) -> bool {
        matches!(self, Stopped::Write | Stopped::Device)
    }
}

/// An alert for the person. The words are the App's (`views`); the engine names what happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Alert {
    /// The hub does not serve a part of a device's chain: what follows it is not shown (9.0.6).
    Withheld { group: GroupId, sender: DeviceId, change: u64 },
    /// An entry of a group's log does not process: the group is kept as it was (13.4).
    BadGroup { group: GroupId, change: u64, code: &'static str },
    /// The hub served something that is no envelope.
    NotAnEnvelope { change: u64, code: &'static str },
    /// A Welcome was refused.
    Welcome { group: GroupId, code: &'static str },
    /// The hub answered with something the protocol does not allow: nothing of it was taken.
    BadAnswer { why: &'static str },
    /// The hub says this key is no member; the device itself has seen no Commit that removed it.
    NotMember,
    /// The hub needs a newer Trommi.
    ClientTooOld,
    /// A leaf could not merge an accepted Commit (14.7): `from` refused the Commit numbered `n` in `group`, made by
    /// `committer` as the hub says. The person decides; remove the device that made it.
    Rejected { group: GroupId, n: u64, from: DeviceId, committer: Option<DeviceId> },
    /// The room was joined with the code, but the hub did not serve the link from an older code to this one: what
    /// was sealed under the older codes stays closed to this device.
    MissingLink,
    /// A newer Trommi wrote here.
    NewerVersion { from: DeviceId },
    /// A stale session waits for a repair this device could not make: nothing is written in it until then (5.2.8).
    Unrepaired { group: GroupId, code: &'static str },
    /// The core found something wrong in a sender's chain while it processed a Commit (`equivocation`: a Cut names
    /// another envelope than the one this device had accepted under that number).
    Finding { group: GroupId, sender: DeviceId, code: &'static str },
}

/// What the engine tells the App, in order. The App feeds the model from it (`Envelope`, `Log`, `Groups`).
#[derive(Debug)]
pub enum Report {
    /// The founding of the room (`Engine::found_room`) ended: taken (none), or refused with this code (the device's
    /// state is gone).
    Founded(Option<&'static str>),
    /// How a join by an invite link stands (`Engine::join_by_link`).
    LinkJoin(link_join::LinkJoinStatus),
    /// An envelope as the protocol core took it. `ordered`: in the hub's order; else read again (a repair).
    Envelope { received: Box<ReceivedEnvelope>, ordered: bool },
    /// A log entry the device processed.
    Log { change: u64, group: GroupId, processed: Box<Processed> },
    /// The device's groups may have changed: read `Engine::device().groups()` again.
    Groups,
    Alert(Alert),
    /// The hub's Desk: what it lists as open (the object, the change of its version) at which change. `truncated`:
    /// cut short, it decides nothing. The listed versions came before it as `Envelope`s (not ordered).
    Desk { at: u64, open: Vec<(ObjectId, u64)>, truncated: bool },
    /// The Desk is complete: every listed card's version, and the first version of each card revised since it was
    /// asked, was read.
    DeskComplete,
    Presence(Presence),
    /// The stream opened (again): the hub's list of who is here comes first, ended by `Here`.
    StreamOpened,
    Here,
    Connection(Connection),
    /// The hub took an outbox entry.
    Accepted { id: u64, kind: OutboxKind, change: Option<u64> },
    /// The hub refused an outbox entry for good: what it echoed is rolled back.
    Refused { id: u64, kind: OutboxKind, code: &'static str, voided: bool },
    /// The pump halts on this entry (an envelope refused without a void record), or goes on again (`None`).
    Blocked(Option<(u64, &'static str)>),
    /// The session groups of a sign-in with the code are through: how many the hub took this device into, and the
    /// ones it is not in, each with the reason. Another human device of the person adds it to those (5.2.7).
    SessionsJoined { joined: u32, not_joined: Vec<(GroupId, &'static str)> },
    /// How the join of the room with the code stands (`room_join`).
    RoomJoin(room_join::RoomJoinStatus),
    /// An action's request was answered, or given up (`requests`).
    Answer(Box<requests::Answered>),
    /// A piece the hub only passed on, as the protocol core opened it (a stroke still being drawn, 7.2).
    Relay(Box<Received>),
    /// A newcomer sent its Request for an invite of this room (12.1): the invite flow reads it.
    InviteRequest { invite: InviteId },
    /// The hub deleted a file (11.3): its cached bytes go.
    FileEvicted { file: FileId },
    Stopped(Stopped),
    /// An agent device: a human's command the gate let through, after the envelope's own report; or, after a
    /// restart, one kept from before that nobody said was done (`uncertain`).
    Command(Box<agent::AgentCommand>),
    /// An agent device: the lease this process acquired (its generation), or none once it was given up.
    Lease(Option<u64>),
}

/// A wait before the next try of something.
#[derive(Clone, Copy, Debug, Default)]
struct Retry {
    attempt: u32,
    at: Option<u64>,
}

impl Retry {
    fn fail(&mut self, now: u64, retry_after_ms: Option<u64>, jitter: &mut Jitter) {
        self.attempt = self.attempt.saturating_add(1);
        self.at = Some(now + Backoff::HUB.delay_ms(self.attempt, retry_after_ms, jitter.draw()));
    }
    fn wait(&mut self, now: u64, ms: u64) {
        self.at = Some(now + ms);
    }
    fn ok(&mut self) {
        *self = Retry::default();
    }
    fn waiting(&self, now: u64) -> bool {
        self.at.is_some_and(|at| at > now)
    }
    /// The network may be back: the wait ends, the count stays (a failure doubles on from where it was).
    fn hurry(&mut self) {
        self.at = None;
    }
}

/// What a request in flight was for.
enum Asked {
    /// A step of a join by link.
    Link { expect: Expect },
    Challenge(Expect),
    Token(Expect),
    Page(Expect),
    Post { expect: Expect, entry: u64, kind: OutboxKind, group: Option<GroupId> },
    Welcomes(Expect),
    GroupLog { expect: Expect, group: GroupId, upto: u64 },
    Chain { expect: Expect, group: GroupId, sender: DeviceId, before: u64 },
    /// A report to the hub nothing waits for (`RejectCommit`).
    Told(Expect),
    Mend { expect: Expect, mend: Mend },
    KeyPackagesHeld(Expect),
    Desk(Expect),
    /// Joins of session groups from outside, posted together: each entry's id and group, in order.
    Joins { expect: Expect, entries: Vec<(u64, Option<GroupId>)> },
    /// A read for the joins of a sign-in's sessions (`joins`).
    JoinRead { expect: Expect, what: joins::Read },
    /// The first envelope of an open object.
    First { expect: Expect, route: ObjectRoute, object: ObjectId, after: u64 },
    /// A KeyPackage claimed for a job of tending the groups (`upkeep`).
    Claim { expect: Expect, job: upkeep::Job },
    /// A page of the hub's list of the room's groups, for a job of tending (`upkeep`).
    RoomGroups { expect: Expect, job: upkeep::Job },
    /// The hub's list of wishes (`events`).
    Wishes(Expect),
    /// A read of the join of the room with the code (`room_join`).
    RoomRead { expect: Expect, what: room_join::Read },
    /// An action's request, by the id the action holds (`requests`).
    Request { id: IoId },
    /// The kept requests read at the start, and one written or deleted.
    KeptScan,
    KeptWrite,
    /// The engine's record, read at the start and written when it changed (`record`).
    Record,
    RecordWrite,
    DeviceWrite,
    /// An agent's lease (`agent`).
    Lease(Expect),
    /// The rollback guard's look at this device's own chain in a group, beyond its own head `own`.
    Guard { expect: Expect, group: GroupId, own: u64 },
    /// A page of what the hub still serves of a removal.
    Removal { expect: Expect, group: GroupId },
}

impl Asked {
    /// How the answer is to be read; none for a write to the store.
    fn expect(&self) -> Option<&Expect> {
        match self {
            Asked::DeviceWrite | Asked::Record | Asked::RecordWrite | Asked::Request { .. } | Asked::KeptScan | Asked::KeptWrite => None,
            Asked::Challenge(e) | Asked::Lease(e) | Asked::Token(e) | Asked::Page(e) | Asked::Welcomes(e) | Asked::Told(e) | Asked::KeyPackagesHeld(e) | Asked::Desk(e) | Asked::Wishes(e) => Some(e),
            Asked::Post { expect, .. }
            | Asked::Link { expect }
            | Asked::GroupLog { expect, .. }
            | Asked::Chain { expect, .. }
            | Asked::Mend { expect, .. }
            | Asked::Joins { expect, .. }
            | Asked::JoinRead { expect, .. }
            | Asked::First { expect, .. }
            | Asked::Claim { expect, .. }
            | Asked::RoomGroups { expect, .. }
            | Asked::RoomRead { expect, .. }
            | Asked::Guard { expect, .. }
            | Asked::Removal { expect, .. } => Some(expect),
        }
    }
}

/// The reading of the hub's Desk: once per run, and again when a session group was joined (the hub lists only what
/// lies in the asker's groups).
#[derive(Default)]
struct DeskRead {
    want: bool,
    asked: Option<IoId>,
    /// The first versions still to read: the object and the change just before its first envelope (a hint).
    firsts: VecDeque<(ObjectRoute, ObjectId, u64)>,
    flying: usize,
    /// A list was read and its first versions are through.
    complete: bool,
    retry: Retry,
}

/// A bounded read of what is actually missing, made when the order is at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mend {
    /// One group's envelopes after `after`, up to `upto` (`GET /v1/changes?group=`): what the device could not take
    /// before (its group was behind) is taken, what it chained without a key is read back.
    Group { group: GroupId, after: u64, upto: u64 },
    /// A sender's chain from what this device holds: a stretch the hub withheld before.
    Chain { group: GroupId, sender: DeviceId },
}

/// What the head of the order waits for before it is tried again.
enum Repair {
    /// The hub's Welcomes for this device; `after`: the last row read.
    Welcomes { after: u64, joined: Vec<GroupId>, asked: bool },
    /// The logs of groups joined late, one after another, up to the position.
    Logs { groups: VecDeque<GroupId>, after: u64, asked: bool },
    /// A stretch of a sender's chain, from what this device holds to just before `before`.
    Chain { group: GroupId, sender: DeviceId, after: u64, before: u64, asked: bool },
}

/// What was tried for the item at the head of the order.
#[derive(Clone, Copy, Default)]
struct Tried {
    welcomes: bool,
    chain: bool,
}

enum Taken {
    /// Taken, or passed for good: the position moves over it.
    Done,
    /// It waits for a repair.
    Wait(Repair),
    /// It does not process: the pass stops with the position before it.
    Halt,
}

struct Token {
    value: String,
    renew_at: u64,
}

struct StreamConn<S> {
    id: IoId,
    reader: S,
    opened: bool,
    /// The change it was opened after.
    from: u64,
    heard_at: u64,
}

/// The sync engine of one device in one room.
pub struct Engine {
    codec: Codec,
    device: Device<memstore::MemStore>,
    writes: Writes,
    room: RoomId,
    hub: String,
    address: HubAddress,
    client: String,
    jitter: Jitter,
    stopped: Option<Stopped>,
    pending: BTreeMap<IoId, Asked>,
    reports: VecDeque<Report>,

    // signing in
    token: Option<Token>,
    signing: Option<IoId>,
    auth_retry: Retry,
    /// Requests refused as `unauthorised` in a row under a fresh token.
    unauthorised: u32,

    // the hub's order
    /// Every change at or below it was taken by the device, or passed for good.
    position: u64,
    /// The items being taken, in order: a page's, or the stream's.
    todo: VecDeque<ChangeItem>,
    /// Where the position stands once `todo` is through, and whether the hub has more after it.
    todo_upto: u64,
    todo_more: bool,
    tried: Tried,
    repair: Option<Repair>,
    page: Option<IoId>,
    /// How many items the next page asks for: halved when a page was more than the shell may read.
    page_limit: u32,
    /// A pass is wanted: pages from the position until the hub has no more.
    pass: bool,
    /// A pass came to the hub's end at least once since the start.
    caught_up: bool,
    order_retry: Retry,
    /// Welcomes read with nothing joined: items that waited for a group since (`WELCOMES_AGAIN`).
    welcomes_idle: Option<u32>,
    welcomes_after: u64,
    /// A Welcome was read before the room reached its epoch (`room-behind`, `wrong-epoch`), or a group this device
    /// does not hold was founded; `.1`: the groups changed since (or the founding was seen), so the Welcomes are read
    /// once the items at hand are through.
    welcome_waits: (bool, bool),
    /// Envelopes refused as `group-behind` for a group this device does not hold that far: per group the first such
    /// change and the lowest epoch. Read again once the group is there (`past`).
    behind: BTreeMap<GroupId, (u64, u64)>,
    /// Envelopes chained without their body for want of a key (`no-key`): per group the first such change. Read back
    /// once keys were handed over.
    keyless: BTreeMap<GroupId, u64>,
    /// Chains the hub did not serve whole: per group and sender the number that could not be reached. Asked for
    /// again when the order comes to rest.
    withheld: BTreeMap<(GroupId, DeviceId), u64>,
    /// The reads that mend the above, one at a time.
    mends: VecDeque<Mend>,
    mending: Option<IoId>,
    /// From where the room's order is to be read once more (the seam to `past`): only when one of the tables above
    /// had no room for one more group, or reports were lost.
    reread_from: Option<u64>,

    desk: DeskRead,

    // the stream
    stream: Option<StreamConn<StreamReader>>,
    stream_retry: Retry,
    /// The room's head as a page said it: a hint for where to open the stream.
    head_hint: Option<u64>,
    /// Changes the stream brought while pages were read.
    live: VecDeque<ChangeItem>,
    live_bytes: usize,
    background: bool,

    // the outbox
    posting: Option<IoId>,
    pump_retry: Retry,
    blocked: Option<u64>,
    /// How many unused KeyPackages the hub holds, as it last said.
    pub key_packages_at_hub: Option<u32>,
    /// When the next round of upkeep is due (0: at once, when the order is at rest), and its question in flight.
    upkeep_at: u64,
    upkeep_asked: Option<IoId>,
    upkeep_retry: Retry,
    connection: Connection,
    /// The session groups of a sign-in with the code, being read and joined (`joins`).
    joining: Option<joins::Joining>,
    /// The hub refused joins posted together as a request: each goes alone, and gets its own answer.
    joins_alone: bool,
    /// Tending the groups: stale sessions, human devices a session lacks, the core's findings (`upkeep`).
    tending: upkeep::Tending,
    /// Where this device's reading of the order began, when not at its start (`record`).
    floor: Option<u64>,
    /// The record is being read from the store: nothing else starts before.
    restoring: Option<IoId>,
    /// The record as it was last written.
    written: Option<record::Record>,
    /// Wishes of other devices for keys, and the ones answered (`events`).
    wishes: events::Wishes,
    /// The join of the room with the code, while it runs (`room_join`): nothing else is done before it ends.
    room_join: Option<room_join::Joiner>,
    /// A room this device founds, until the hub took the founding (`found`).
    founding: Option<found::Founding>,
    /// A join of the room by an invite link, until this device is in (`link_join`).
    link_join: Option<link_join::LinkJoin>,
    /// The actions' requests (`requests`).
    requests: requests::Requests,
    /// An agent device's lease, gate and guards (`agent`); none for a human device.
    agent: Option<agent::Agent>,
}

/// Why a stored device does not open as an engine.
#[derive(Debug, PartialEq, Eq)]
pub enum OpenError {
    /// The store is empty, or its device belongs to no room yet: the sign-in and join screens.
    NoRoom,
    /// The stored state does not open.
    Device(&'static str),
    /// `Config.hub` is no canonical hub address.
    Hub,
    /// The device is in no room, and holds a join of the room with the code that was posted without its answer
    /// being read: the code is asked for again ([`room_join`]).
    Interrupted,
}

impl Engine {
    /// Opens the device the shell loaded and makes its engine. Nothing is asked of the hub yet (`drive`).
    pub fn open(hub: &str, client: &str, seed: u64, revision: u64, entries: Vec<DeviceEntry>) -> Result<Self, OpenError> {
        Self::open_with(hub, client, seed, revision, entries, Box::new(SystemEntropy))
    }

    pub fn open_with(
        hub: &str,
        client: &str,
        seed: u64,
        revision: u64,
        entries: Vec<DeviceEntry>,
        entropy: Box<dyn Entropy + Send>,
    ) -> Result<Self, OpenError> {
        if revision == 0 || entries.is_empty() {
            return Err(OpenError::NoRoom);
        }
        let address = HubAddress::parse(hub).map_err(|_| OpenError::Hub)?;
        let (store, writes) = memstore::store(revision, entries.into_iter().map(|e| (e.key.0, e.value.0)).collect());
        let device = Device::open(store, entropy).map_err(|e| OpenError::Device(e.code()))?;
        let Some(room) = device.room() else {
            // (a join with the code was posted and its answer never read: only the code goes on with it)
            return Err(if room_join::staged(&device).is_some() { OpenError::Interrupted } else { OpenError::NoRoom });
        };
        Ok(Self::over(device, writes, room, hub, address, client, seed))
    }

    /// The engine over a device that was opened or made.
    fn over(device: Device<memstore::MemStore>, writes: Writes, room: RoomId, hub: &str, address: HubAddress, client: &str, seed: u64) -> Self {
        let position = device.cursor();
        Engine {
            codec: Codec,
            device,
            writes,
            room,
            hub: hub.to_string(),
            address,
            client: client.to_string(),
            jitter: Jitter::new(seed),
            stopped: None,
            pending: BTreeMap::new(),
            reports: VecDeque::new(),
            token: None,
            signing: None,
            auth_retry: Retry::default(),
            unauthorised: 0,
            position,
            todo: VecDeque::new(),
            todo_upto: position,
            todo_more: false,
            tried: Tried::default(),
            repair: None,
            page: None,
            page_limit: PAGE,
            pass: true,
            caught_up: false,
            order_retry: Retry::default(),
            welcomes_idle: None,
            welcomes_after: 0,
            welcome_waits: (false, false),
            behind: BTreeMap::new(),
            keyless: BTreeMap::new(),
            withheld: BTreeMap::new(),
            mends: VecDeque::new(),
            mending: None,
            reread_from: None,
            desk: DeskRead { want: true, ..DeskRead::default() },
            stream: None,
            stream_retry: Retry::default(),
            head_hint: None,
            live: VecDeque::new(),
            live_bytes: 0,
            background: false,
            posting: None,
            pump_retry: Retry::default(),
            blocked: None,
            key_packages_at_hub: None,
            upkeep_at: 0,
            upkeep_asked: None,
            upkeep_retry: Retry::default(),
            connection: Connection::Offline,
            joining: None,
            joins_alone: false,
            tending: upkeep::Tending::new(),
            floor: None,
            restoring: None,
            written: None,
            wishes: events::Wishes::default(),
            room_join: None,
            founding: None,
            link_join: None,
            requests: requests::Requests::default(),
            agent: None,
        }
    }

    // ---- what the App reads

    pub fn device(&self) -> &Device<memstore::MemStore> {
        &self.device
    }
    /// The device's source of randomness, for what an action makes beside the protocol (a file's key, a share
    /// link's secret): a seeded App stays exact.
    pub fn with_entropy<T>(&self, use_it: impl FnOnce(&mut dyn Entropy) -> T) -> T {
        self.device.with_entropy(use_it)
    }
    pub fn room(&self) -> RoomId {
        self.room
    }
    pub fn position(&self) -> u64 {
        self.position
    }
    pub fn connection(&self) -> Connection {
        self.connection
    }
    pub fn stopped(&self) -> Option<Stopped> {
        self.stopped
    }
    /// A pass reached the hub's end at least once since this App started.
    pub fn caught_up(&self) -> bool {
        self.caught_up
    }
    /// From where the room's order is to be read once more, if it is (the seam to the reading of the past).
    pub fn reread_from(&self) -> Option<u64> {
        self.reread_from
    }
    /// How much the engine passed over and still has to read: groups it waits for, groups whose keys it lacks,
    /// chains the hub withholds, mends queued.
    pub fn owes(&self) -> (usize, usize, usize, usize) {
        (self.behind.len(), self.keyless.len(), self.withheld.len(), self.mends.len())
    }
    /// The next report, oldest first.
    pub fn next_report(&mut self) -> Option<Report> {
        self.reports.pop_front()
    }

    /// An action on the device (seal, commit, answer): what it wrote is stored and the pump is woken.
    pub fn act<T>(&mut self, ctx: &mut Ctx<'_>, action: impl FnOnce(&mut Device<memstore::MemStore>, u64) -> Result<T, Error>) -> Result<T, Error> {
        if self.stopped.is_some() {
            return Err(Error::Storage("the device is closed".into()));
        }
        let result = action(&mut self.device, ctx.now);
        if let Err(e) = &result {
            self.local(ctx, e);
        }
        self.drive(ctx);
        result
    }

    /// When the engine wants `Event::Wake` next; none: nothing waits for the clock.
    pub fn next_wake(&self) -> Option<u64> {
        if self.stopped.is_some() {
            return None;
        }
        let mut at: Option<u64> = None;
        let mut want = |t: Option<u64>| {
            if let Some(t) = t {
                at = Some(at.map_or(t, |a| a.min(t)));
            }
        };
        want(self.auth_retry.at);
        want(self.agent_wake());
        want(self.link_join.as_ref().and_then(|j| j.wake()));
        if self.founding.is_some() {
            want(self.pump_retry.at);
        }
        if self.token.is_some() {
            if self.desk.want || !self.desk.firsts.is_empty() {
                want(self.desk.retry.at);
            }
            if self.pass || !self.todo.is_empty() {
                want(self.order_retry.at);
            }
            if !self.background {
                want(self.stream_retry.at);
                want(self.stream.as_ref().map(|s| s.heard_at + STREAM_SILENT_MS));
            }
            if !self.device.outbox().is_empty() {
                want(self.pump_retry.at);
            }
            if let Some(joining) = &self.joining {
                want(joining.retry.at);
            }
            if let Some(j) = &self.room_join {
                want(j.retry.at);
            }
            want(self.requests.wake());
            if self.caught_up {
                want(self.readmit_wake());
            }
            want(self.tending.wake());
            if self.caught_up && self.upkeep_asked.is_none() {
                want(Some(self.upkeep_at.max(self.upkeep_retry.at.unwrap_or(0))).filter(|at| *at != 0));
            }
        }
        at
    }

    // ---- events

    /// The first step, and the one after every event: starts whatever is due and possible.
    pub fn start(&mut self, ctx: &mut Ctx<'_>) {
        self.report(Report::Groups);
        // what the engine kept across the restart comes first: where it stands, and what it owes
        self.restore(ctx);
        self.scan_kept(ctx);
        self.drive(ctx);
    }

    pub fn on_wake(&mut self, ctx: &mut Ctx<'_>) {
        // a stream that went silent is cut and opened again
        if self.stream.as_ref().is_some_and(|s| ctx.now >= s.heard_at + STREAM_SILENT_MS) {
            self.close_stream(ctx);
            self.pass = true;
        }
        self.drive(ctx);
    }

    /// The app came to the front, or the network is back: whatever waited is tried now.
    pub fn on_foreground(&mut self, ctx: &mut Ctx<'_>) {
        self.background = false;
        self.hurry();
        if self.stream.is_none() {
            self.pass = true;
        }
        self.drive(ctx);
    }

    pub fn on_background(&mut self, ctx: &mut Ctx<'_>) {
        self.background = true;
        self.close_stream(ctx);
        self.drive(ctx);
    }

    pub fn on_net(&mut self, ctx: &mut Ctx<'_>, online: bool) {
        // (only a hint: the engine's own results decide)
        if online {
            self.hurry();
            self.drive(ctx);
        }
    }

    fn hurry(&mut self) {
        self.auth_retry.hurry();
        self.desk.retry.hurry();
        self.order_retry.hurry();
        self.stream_retry.hurry();
        self.pump_retry.hurry();
        if let Some(joining) = &mut self.joining {
            joining.retry.hurry();
        }
        if let Some(j) = &mut self.room_join {
            j.retry.hurry();
        }
        self.requests.hurry();
        self.tending.retry.hurry();
    }

    /// The result of a store effect of this engine. False: the id is not the engine's.
    pub fn on_store(&mut self, ctx: &mut Ctx<'_>, id: IoId, result: &IoResult<StoreAnswer>) -> bool {
        match self.pending.remove(&id) {
            Some(Asked::DeviceWrite) => {
                if let IoResult::Err(e) = result {
                    ctx.log(LogLevel::Error, format!("device write: {:?}", e.kind));
                    let why = if e.kind == crate::app::io::IoErrorKind::Conflict { Stopped::Store } else { Stopped::Write };
                    self.stop(ctx, why);
                }
                true
            }
            Some(Asked::Record) => {
                if self.stopped.is_none() {
                    self.restored(ctx, result);
                    self.drive(ctx);
                }
                true
            }
            Some(Asked::KeptScan) => {
                self.on_kept(ctx, result);
                self.drive(ctx);
                true
            }
            Some(Asked::KeptWrite) => {
                if let IoResult::Err(e) = result {
                    ctx.log(LogLevel::Warn, format!("a kept request was not written: {:?}", e.kind));
                }
                true
            }
            Some(Asked::RecordWrite) => {
                if let IoResult::Err(e) = result {
                    // (not kept: the next step writes it again; a restart before that reads a little more)
                    ctx.log(LogLevel::Warn, format!("the engine's record was not written: {:?}", e.kind));
                    self.written = None;
                }
                true
            }
            Some(other) => {
                self.pending.insert(id, other);
                false
            }
            None => false,
        }
    }

    /// The result of an HTTP effect of this engine. False: the id is not the engine's.
    pub fn on_http(&mut self, ctx: &mut Ctx<'_>, id: IoId, result: IoResult<HttpResponse>) -> bool {
        let Some(asked) = self.pending.remove(&id) else {
            return false;
        };
        if self.stopped.is_some() {
            return true;
        }
        if let Asked::Request { id } = asked {
            self.on_request(ctx, id, result);
            self.drive(ctx);
            return true;
        }
        if matches!(asked, Asked::DeviceWrite | Asked::Record | Asked::RecordWrite | Asked::KeptScan | Asked::KeptWrite) {
            return true;
        }
        let (expect, response) = match &result {
            IoResult::Ok(response) => (asked.expect(), Some(response)),
            IoResult::Err(_) => (None, None),
        };
        let answer: Result<Answer, Failure> = match (expect, response, &result) {
            (Some(expect), Some(response), _) => self.codec.answer(expect, response),
            (_, _, IoResult::Err(error)) => match classify_io(error) {
                Some(failure) => Err(failure),
                None => Err(Failure::Offline), // cancelled by somebody else: as if it never left
            },
            _ => Err(Failure::Offline),
        };
        // a token the hub no longer takes: forgotten; whatever asked is asked again once signed in
        let refusal = match (&answer, response) {
            (Err(Failure::Refused { .. }), Some(response)) => self.codec.refusal(response),
            _ => None,
        };
        let unauthorised = refusal.as_ref().is_some_and(|r| r.code == "unauthorised") && !matches!(asked, Asked::Challenge(_) | Asked::Token(_));
        if unauthorised {
            self.token = None;
            self.unauthorised += 1;
            if self.unauthorised > 1 {
                self.auth_retry.fail(ctx.now, None, &mut self.jitter);
            }
        } else if answer.is_ok() && !matches!(asked, Asked::Challenge(_) | Asked::Token(_)) {
            self.unauthorised = 0;
        }
        if let (Some(r), false) = (&refusal, matches!(asked, Asked::Guard { .. } | Asked::Removal { .. })) {
            self.agent_refused(ctx, r.code);
        }
        let code = refusal.as_ref().map(|r| r.code);
        match asked {
            Asked::DeviceWrite | Asked::Record | Asked::RecordWrite | Asked::Request { .. } | Asked::KeptScan | Asked::KeptWrite => {}
            Asked::Lease(_) => self.on_lease(ctx, answer, code),
            Asked::Guard { group, own, .. } => self.on_guard(ctx, group, own, answer, code),
            Asked::Removal { group, .. } => self.on_removal(ctx, group, answer, code),
            Asked::Link { .. } => self.on_link_answer(ctx, answer),
            Asked::Challenge(_) => self.on_challenge(ctx, answer),
            Asked::Token(_) => self.on_token(ctx, answer),
            Asked::Page(_) => self.on_page(ctx, answer, unauthorised, refusal),
            Asked::Post { entry, kind, group, .. } => self.on_posted(ctx, entry, kind, group, answer, unauthorised, refusal),
            Asked::Welcomes(_) => self.on_welcomes(ctx, answer, unauthorised),
            Asked::GroupLog { group, upto, .. } => self.on_group_log(ctx, group, upto, answer, unauthorised),
            Asked::Chain { group, sender, before, .. } => self.on_chain(ctx, group, sender, before, answer, unauthorised),
            Asked::Told(_) => {}
            Asked::Mend { mend, .. } => self.on_mend(ctx, mend, answer, unauthorised),
            Asked::Desk(_) => self.on_desk(ctx, answer, unauthorised),
            Asked::KeyPackagesHeld(_) => self.on_key_packages_held(ctx, answer, unauthorised),
            Asked::First { route, object, after, .. } => self.on_first(ctx, route, object, after, answer, unauthorised),
            Asked::Joins { entries, .. } => self.on_joins_posted(ctx, entries, answer, unauthorised),
            Asked::JoinRead { what, .. } => self.on_join_read(ctx, what, answer, unauthorised),
            Asked::Claim { job, .. } => self.on_claimed(ctx, job, answer, unauthorised),
            Asked::RoomGroups { job, .. } => self.on_room_groups(ctx, job, answer, unauthorised),
            Asked::Wishes(_) => self.on_wishes(ctx, answer),
            Asked::RoomRead { what, .. } => self.on_room_read(ctx, what, answer, unauthorised, refusal),
        }
        self.drive(ctx);
        true
    }

    /// An item of a stream this engine opened. False: the id is not the engine's (an old stream's late item).
    pub fn on_stream(&mut self, ctx: &mut Ctx<'_>, id: IoId, item: StreamItem) -> bool {
        let Some(conn) = self.stream.as_mut().filter(|s| s.id == id) else {
            return false;
        };
        if self.stopped.is_some() {
            return true;
        }
        conn.heard_at = ctx.now;
        match item {
            StreamItem::Opened { status, headers } => {
                // (an answer that is no stream: its body follows and says why, `ended` sorts it)
                if conn.reader.opened(status, &headers).is_ok() {
                    conn.opened = true;
                    self.stream_retry.ok();
                    self.tending.stream_opened();
                    self.report(Report::StreamOpened);
                }
            }
            StreamItem::Chunk { bytes } => {
                let fed = conn.reader.chunk(&bytes);
                for event in fed.events {
                    self.on_stream_event(ctx, event);
                }
                if let Some(failure) = fed.failure {
                    // an event the protocol does not allow ends the connection; the next resumes after the position
                    ctx.log(LogLevel::Warn, "stream: an event the protocol does not allow");
                    if let Failure::BadAnswer { why } = failure {
                        self.report(Report::Alert(Alert::BadAnswer { why }));
                    }
                    self.close_stream(ctx);
                    self.stream_retry.fail(ctx.now, None, &mut self.jitter);
                    self.pass = true;
                }
            }
            StreamItem::Ended | StreamItem::Failed { .. } => {
                let failure = self.stream.take().and_then(|mut s| s.reader.ended());
                let retry_after = match &failure {
                    Some(Failure::NotNow { retry_after_ms, .. }) => *retry_after_ms,
                    Some(Failure::Refused { status: 401, .. }) => {
                        self.token = None;
                        None
                    }
                    Some(Failure::Refused { code: Some(code), .. }) => {
                        let code = code.clone();
                        self.agent_refused(ctx, &code);
                        None
                    }
                    _ => None,
                };
                self.stream_retry.fail(ctx.now, retry_after, &mut self.jitter);
                // what came between this stream's end and the next one's start is read by a page
                self.pass = true;
            }
        }
        self.drive(ctx);
        true
    }

    fn on_stream_event(&mut self, ctx: &mut Ctx<'_>, event: StreamEvent) {
        match event {
            StreamEvent::Change(item) => {
                if item.change() <= self.position {
                    return;
                }
                // While pages are read, or the stream began beyond the position, a change waits for its turn: taken
                // now it would move the position over what lies between. What does not fit is left to a page.
                let size = match &item {
                    ChangeItem::Envelope(e) => e.envelope.len(),
                    ChangeItem::Commit(l) | ChangeItem::Message(l) => l.bytes.len(),
                } + 64;
                if self.live.len() >= LIVE_MAX || self.live_bytes + size > LIVE_BYTES_MAX {
                    self.live.clear();
                    self.live_bytes = 0;
                    self.pass = true;
                    // (the stream goes on from after the page: it is opened again there)
                    self.close_stream(ctx);
                    return;
                }
                self.live_bytes += size;
                self.live.push_back(item);
            }
            StreamEvent::Welcome { .. } => {
                // a Welcome waits: read at the next item that needs one, and at once when nothing is being taken
                self.welcomes_idle = None;
                if self.repair.is_none() && self.todo.is_empty() {
                    self.repair = Some(Repair::Welcomes { after: self.welcomes_after, joined: Vec::new(), asked: false });
                }
            }
            StreamEvent::Presence(p) => {
                self.tending.presence(&p);
                self.report(Report::Presence(p))
            }
            StreamEvent::Here => self.report(Report::Here),
            StreamEvent::Relay { group, sender, message, .. } => self.on_relay(ctx, group, sender, &message),
            StreamEvent::Wish(row) => self.on_wish(ctx, row),
            StreamEvent::InviteRequest { invite } => self.report(Report::InviteRequest { invite }),
            StreamEvent::Archived { group } => self.on_archived(ctx, group),
            StreamEvent::FileEvicted { file } => self.report(Report::FileEvicted { file }),
        }
    }

    // ---- the drive: whatever is due and possible is started

    fn drive(&mut self, ctx: &mut Ctx<'_>) {
        if self.stopped.is_some() || self.restoring.is_some() {
            self.flush(ctx);
            return;
        }
        if self.founding.is_some() {
            return self.drive_founding(ctx);
        }
        if self.link_join.is_some() {
            return self.drive_link_join(ctx);
        }
        self.drive_all(ctx);
        if self.room_join.is_none() {
            self.keep(ctx);
        }
    }

    fn drive_all(&mut self, ctx: &mut Ctx<'_>) {
        self.agent_restart();
        // The order first: what is at hand is taken before anything new is asked.
        self.take(ctx);
        self.flush(ctx);
        if self.stopped.is_some() {
            return;
        }
        if !self.signed_in(ctx) {
            if self.room_join.is_none() {
                self.connect(ctx);
            }
            return;
        }
        // an agent device reads and writes nothing before it holds the lease and its own chains were checked
        if !self.drive_agent(ctx) {
            self.flush(ctx);
            self.connect(ctx);
            return;
        }
        if self.room_join.is_some() {
            // (the room is joined first: the device has nothing else to do before)
            self.drive_room_join(ctx);
            self.flush(ctx);
            if self.room_join.is_some() || self.stopped.is_some() {
                return;
            }
        }
        // the actions' requests: the person may be waiting for one
        self.drive_requests(ctx);
        // the Desk before the order: what is open now is asked for before the first page
        self.drive_desk(ctx);
        self.drive_order(ctx);
        self.drive_stream(ctx);
        self.drive_joins(ctx);
        self.drive_upkeep(ctx);
        self.drive_tending(ctx);
        self.drive_wishes(ctx);
        self.drive_pump(ctx);
        self.flush(ctx);
        self.connect(ctx);
    }

    /// Hands the shell what the device wrote: before any request that follows in this answer (the barrier).
    fn flush(&mut self, ctx: &mut Ctx<'_>) {
        if let Some(write) = self.writes.take() {
            let id = ctx.id();
            self.pending.insert(id, Asked::DeviceWrite);
            ctx.out.push(Effect::Store {
                id,
                op: StoreOp::DeviceApply {
                    expected_revision: write.expected_revision,
                    put: write.put.into_iter().map(|(k, v)| DeviceEntry { key: k.into(), value: v.into() }).collect(),
                    delete: write.delete.into_iter().map(Into::into).collect(),
                },
            });
        }
    }

    fn connect(&mut self, _ctx: &mut Ctx<'_>) {
        let now = if self.stream.as_ref().is_some_and(|s| s.opened) && !self.pass && self.todo.is_empty() && self.caught_up {
            Connection::Live
        } else if self.token.is_some() && (self.page.is_some() || !self.todo.is_empty()) {
            Connection::CatchingUp
        } else if self.token.is_some() && self.caught_up && self.background && !self.pass {
            // (in the back, at the end of the order, without a stream: as live as it gets there)
            Connection::Live
        } else if self.auth_retry.at.is_none() && self.order_retry.at.is_none() && self.stream_retry.at.is_none() {
            // (on its way: no failure stands)
            if self.caught_up { self.connection } else { Connection::CatchingUp }
        } else {
            Connection::Offline
        };
        if now != self.connection {
            self.connection = now;
            self.report(Report::Connection(now));
        }
    }

    fn report(&mut self, report: Report) {
        // (the App drains after every event; a page's worth at most stands here)
        if self.reports.len() >= REPORTS_MAX {
            self.reports.pop_front();
            self.reread_from = Some(0);
        }
        self.reports.push_back(report);
    }

    fn stop(&mut self, ctx: &mut Ctx<'_>, why: Stopped) {
        if self.stopped.is_some() {
            return;
        }
        self.stopped = Some(why);
        self.joining = None;
        self.writes.lose();
        for (id, asked) in std::mem::take(&mut self.pending) {
            if !matches!(asked, Asked::DeviceWrite | Asked::Record | Asked::RecordWrite | Asked::KeptScan | Asked::KeptWrite) {
                ctx.out.push(Effect::Cancel { id });
            }
        }
        if let Some(stream) = self.stream.take() {
            ctx.out.push(Effect::Cancel { id: stream.id });
        }
        self.report(Report::Stopped(why));
    }

    /// A failure of the device itself ends the engine: the shell opens the stored state again with a new App.
    fn local(&mut self, ctx: &mut Ctx<'_>, error: &Error) {
        if matches!(error, Error::Storage(_) | Error::Entropy | Error::Internal(_)) || !self.device.is_owner() {
            ctx.log(LogLevel::Error, format!("device: {}", error.code()));
            self.stop(ctx, if self.device.is_owner() { Stopped::Device } else { Stopped::Store });
        }
    }

    fn ask(&mut self, ctx: &mut Ctx<'_>, route: Route<'_>, token: bool, asked: impl FnOnce(Expect) -> Asked) -> Option<IoId> {
        let site = Site {
            url: &self.hub,
            token: if token { self.token.as_ref().map(|t| t.value.as_str()) } else { None },
            client: &self.client,
            lease: self.lease_header(),
        };
        match self.codec.request(&site, &route) {
            Ok((request, expect)) => {
                // (what the device wrote goes first: the request may carry what that state implies)
                self.flush(ctx);
                let id = ctx.id();
                self.pending.insert(id, asked(expect));
                ctx.out.push(Effect::Http { id, request });
                Some(id)
            }
            Err(_) => {
                ctx.log(LogLevel::Error, "a request could not be built");
                None
            }
        }
    }

    // ---- signing in (12.3)

    fn signed_in(&mut self, ctx: &mut Ctx<'_>) -> bool {
        if self.token.as_ref().is_some_and(|t| ctx.now < t.renew_at) {
            return true;
        }
        if self.signing.is_none() && !self.auth_retry.waiting(ctx.now) {
            let room = self.room;
            self.signing = self.ask(ctx, Route::Challenge { room: &room }, false, Asked::Challenge);
        }
        // (a token about to end still serves until the new one is there)
        self.token.is_some()
    }

    fn on_challenge(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>) {
        self.signing = None;
        match answer {
            Ok(Answer::Challenge(challenge)) => match self.room_join.as_ref().map_or_else(|| self.device.hub_sign_in(&self.address, challenge), |j| j.sign_in(self.room, &self.address, challenge)) {
                Ok(signed) => {
                    let room = self.room;
                    self.signing = self.ask(ctx, Route::Token { room: &room, auth: &signed.auth, signature: &signed.signature }, false, Asked::Token);
                }
                Err(e) => self.local(ctx, &e),
            },
            Ok(_) => self.auth_failed(ctx, Failure::BadAnswer { why: "challenge" }),
            Err(failure) if self.room_join.is_some() => self.room_auth_failed(ctx, failure),
            Err(failure) => self.auth_failed(ctx, failure),
        }
    }

    fn on_token(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>) {
        self.signing = None;
        match answer {
            Ok(Answer::Token { token, expires_at, role }) => {
                // The hub's clock is not this one's: the token is taken to live at least two renewal spans and at
                // most ten minutes. One that ended earlier is found out by its 401.
                let life = expires_at.saturating_sub(ctx.now).clamp(2 * TOKEN_RENEW_BEFORE_MS, TOKEN_MAX_MS);
                self.token = Some(Token { value: token, renew_at: ctx.now + life - TOKEN_RENEW_BEFORE_MS });
                self.auth_retry.ok();
                if role == Role::Removed {
                    // (the hub's word alone ends nothing: the device follows the room's Commits itself)
                    self.report(Report::Alert(Alert::NotMember));
                    self.agent_out(ctx);
                }
            }
            Ok(_) => self.auth_failed(ctx, Failure::BadAnswer { why: "token" }),
            Err(failure) if self.room_join.is_some() => self.room_auth_failed(ctx, failure),
            Err(failure) => self.auth_failed(ctx, failure),
        }
    }

    fn auth_failed(&mut self, ctx: &mut Ctx<'_>, failure: Failure) {
        let retry_after = match failure {
            Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
            _ => None,
        };
        self.auth_retry.fail(ctx.now, retry_after, &mut self.jitter);
    }

    // ---- the hub's order

    fn drive_order(&mut self, ctx: &mut Ctx<'_>) {
        // a repair the head waits for
        match &mut self.repair {
            Some(Repair::Welcomes { after, asked, .. }) if !*asked => {
                if !self.order_retry.waiting(ctx.now) {
                    let after = *after;
                    *asked = true;
                    if self.ask(ctx, Route::Welcomes { after }, true, Asked::Welcomes).is_none() {
                        self.repair = None;
                    }
                }
                return;
            }
            Some(Repair::Logs { groups, after, asked }) if !*asked => {
                if !self.order_retry.waiting(ctx.now) {
                    let Some(group) = groups.front().copied() else {
                        self.repair = None;
                        return self.drive_all(ctx);
                    };
                    let (after, upto) = (*after, self.position);
                    *asked = true;
                    let route = Route::GroupLog { group: &group, after, limit: LOG_PAGE, commits_only: false };
                    if self.ask(ctx, route, true, |expect| Asked::GroupLog { expect, group, upto }).is_none() {
                        self.repair = None;
                    }
                }
                return;
            }
            Some(Repair::Chain { group, sender, after, before, asked }) if !*asked => {
                if !self.order_retry.waiting(ctx.now) {
                    let (group, sender, after, before) = (*group, *sender, *after, *before);
                    *asked = true;
                    let route = Route::Chain { group: &group, sender: &sender, after, limit: CHAIN_PAGE };
                    if self.ask(ctx, route, true, |expect| Asked::Chain { expect, group, sender, before }).is_none() {
                        self.repair = None;
                    }
                }
                return;
            }
            Some(_) => return,
            None => {}
        }
        if !self.todo.is_empty() || self.page.is_some() || self.order_retry.waiting(ctx.now) {
            return;
        }
        if !self.pass {
            return self.drive_mend(ctx);
        }
        let after = self.position;
        // Everything the device may see, archived sessions' envelopes too: the stream brings those as well, and a
        // read that left them out would let the position pass over one the stream handed to it.
        let limit = self.page_limit;
        self.page = self.ask(ctx, Route::Changes { after, limit, only: Only::All }, true, Asked::Page);
    }

    fn on_page(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>, unauthorised: bool, refusal: Option<Refusal>) {
        self.page = None;
        match answer {
            Ok(Answer::Changes(page)) => {
                self.order_retry.ok();
                self.page_limit = (self.page_limit * 2).min(PAGE);
                self.head_hint = page.head;
                self.todo_upto = page.change;
                self.todo_more = page.more;
                self.todo = page.items.into_iter().filter(|i| i.change() > self.position).collect();
                self.tried = Tried::default();
                if self.todo.is_empty() {
                    self.page_through(ctx);
                }
            }
            Ok(_) => self.order_retry.fail(ctx.now, None, &mut self.jitter),
            Err(_) if unauthorised => {}
            Err(failure) => {
                let retry_after = match &failure {
                    Failure::NotNow { retry_after_ms, .. } => *retry_after_ms,
                    Failure::BadAnswer { why } => {
                        // a page that breaks the hub's own order, or is longer than the shell may read, is refused
                        // whole: nothing of it was taken. The next one asks for less, after a wait.
                        self.report(Report::Alert(Alert::BadAnswer { why }));
                        self.page_limit = (self.page_limit / 2).max(1);
                        None
                    }
                    Failure::Refused { .. } => {
                        match refusal.as_ref().map(|r| r.code) {
                            Some("not-member") => self.report(Report::Alert(Alert::NotMember)),
                            Some("client-too-old") => self.report(Report::Alert(Alert::ClientTooOld)),
                            _ => {}
                        }
                        None
                    }
                    Failure::Offline => None,
                };
                self.order_retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        }
    }

    /// The items at hand are through: the position stands at the hub's cursor after them.
    fn page_through(&mut self, ctx: &mut Ctx<'_>) {
        self.position = self.position.max(self.todo_upto);
        if let Some(stream) = self.stream.as_mut() {
            stream.reader.caught_up_to(self.position);
        }
        if self.todo_more {
            return; // `pass` stands: the next page
        }
        self.todo_more = false;
        // The hub has no more. What the stream brought meanwhile continues the order, if the stream began at or
        // below where the pages ended; a stream that began beyond it (the hub's `head` was a hint) left a stretch
        // nobody read, and is opened again from here.
        self.pass = false;
        self.caught_up = true;
        self.mendable(true);
        if self.stream.as_ref().is_some_and(|s| s.from > self.position) {
            self.live.clear();
            self.live_bytes = 0;
            self.close_stream(ctx);
        }
    }

    /// Takes what is at hand in the hub's order: the page's items, then, at the end of the order, the stream's.
    fn take(&mut self, ctx: &mut Ctx<'_>) {
        if self.repair.is_some() || self.order_retry.waiting(ctx.now) && self.todo.is_empty() {
            return;
        }
        let mut groups_changed = false;
        loop {
            if self.stopped.is_some() {
                break;
            }
            if self.todo.is_empty() {
                // the stream's changes continue the order only once no page is due or in flight
                if self.pass || self.page.is_some() || self.live.is_empty() || self.order_retry.waiting(ctx.now) {
                    break;
                }
                let Some(stream_from) = self.stream.as_ref().map(|s| s.from) else {
                    self.live.clear();
                    self.live_bytes = 0;
                    break;
                };
                if stream_from > self.position {
                    break;
                }
                self.todo = std::mem::take(&mut self.live).into_iter().filter(|i| i.change() > self.position).collect();
                self.live_bytes = 0;
                self.todo_upto = self.todo.back().map_or(self.position, ChangeItem::change);
                self.todo_more = false;
                self.tried = Tried::default();
                if self.todo.is_empty() {
                    break;
                }
            }
            let Some(item) = self.todo.pop_front() else { break };
            let change = item.change();
            match self.take_one(ctx, &item, &mut groups_changed) {
                Taken::Done => {
                    self.position = self.position.max(change);
                    self.tried = Tried::default();
                    if self.todo.is_empty() {
                        let paged = self.pass;
                        if paged {
                            self.page_through(ctx);
                        } else {
                            self.position = self.position.max(self.todo_upto);
                        }
                    }
                }
                Taken::Wait(repair) => {
                    self.todo.push_front(item);
                    self.repair = Some(repair);
                    break;
                }
                Taken::Halt => {
                    // 13.4: the last good state and position are kept; what was at hand is asked for again later
                    self.todo.clear();
                    self.live.clear();
                    self.live_bytes = 0;
                    self.pass = true;
                    self.order_retry.wait(ctx.now, HALTED_RETRY_MS);
                    break;
                }
            }
        }
        if groups_changed {
            self.groups_changed();
        }
        // a Welcome that came before the room Commit it builds on: read again now that the room moved
        if self.welcome_waits == (true, true) && self.repair.is_none() && self.todo.is_empty() && self.stopped.is_none() {
            self.welcome_waits = (false, false);
            self.repair = Some(Repair::Welcomes { after: self.welcomes_after, joined: Vec::new(), asked: false });
        }
    }

    /// The device's groups may have changed: the App is told, and what waited for a group is looked at again.
    fn groups_changed(&mut self) {
        self.report(Report::Groups);
        self.welcome_waits.1 = self.welcome_waits.0;
        self.tending.changed();
        if self.behind.is_empty() {
            return;
        }
        self.mendable(false);
    }

    fn queue(&mut self, mend: Mend) {
        if !self.mends.contains(&mend) && self.mends.len() < BEHIND_MAX {
            self.mends.push_back(mend);
        }
    }

    /// What can be mended now is queued: groups that came as far as what waited for them, and (`chains`) the chains
    /// the hub withheld before.
    fn mendable(&mut self, chains: bool) {
        let groups = self.device.groups().unwrap_or_default();
        let upto = self.position;
        let met: Vec<(GroupId, u64)> = self
            .behind
            .iter()
            .filter(|(group, (_, epoch))| groups.iter().any(|g| g.group == **group && g.epoch >= *epoch))
            .map(|(group, (change, _))| (*group, *change))
            .collect();
        for (group, change) in met {
            self.behind.remove(&group);
            self.queue(Mend::Group { group, after: change.saturating_sub(1), upto });
        }
        if chains {
            let withheld: Vec<(GroupId, DeviceId)> = self.withheld.keys().copied().collect();
            for (group, sender) in withheld {
                self.queue(Mend::Chain { group, sender });
            }
        }
    }

    /// Keys were handed over: what was chained without its body is read back, group by group.
    pub(super) fn keys_came(&mut self) {
        let upto = self.position;
        for (group, change) in std::mem::take(&mut self.keyless) {
            self.queue(Mend::Group { group, after: change.saturating_sub(1), upto });
        }
    }

    fn drive_mend(&mut self, ctx: &mut Ctx<'_>) {
        if self.mending.is_some() {
            return;
        }
        self.mendable(false);
        let Some(mend) = self.mends.front().copied() else { return };
        self.mending = match mend {
            Mend::Group { group, after, .. } => self.ask(ctx, Route::Changes { after, limit: PAGE, only: Only::Group(&group) }, true, |expect| Asked::Mend { expect, mend }),
            Mend::Chain { group, sender } => {
                let after = self.device.chain_head(&group, &sender).map(|h| h.seq).unwrap_or(0);
                self.ask(ctx, Route::Chain { group: &group, sender: &sender, after, limit: CHAIN_PAGE }, true, |expect| Asked::Mend { expect, mend })
            }
        };
        if self.mending.is_none() {
            self.mends.pop_front();
        }
    }

    fn on_mend(&mut self, ctx: &mut Ctx<'_>, mend: Mend, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.mending = None;
        let answer = match answer {
            Ok(answer) => answer,
            Err(_) if unauthorised => return,
            Err(failure) => {
                // tried again when the order next comes to rest after a wait; a refusal ends this one
                if matches!(failure, Failure::Refused { .. }) {
                    self.mends.pop_front();
                }
                let retry_after = match failure {
                    Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
                    _ => None,
                };
                return self.order_retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        };
        self.order_retry.ok();
        self.mends.pop_front();
        match (mend, answer) {
            (Mend::Group { group, upto, .. }, Answer::Changes(page)) => {
                for item in &page.items {
                    let ChangeItem::Envelope(e) = item else { continue };
                    if e.change > upto {
                        return; // what lies after came, or comes, in the one order
                    }
                    self.take_again(ctx, e);
                    if self.stopped.is_some() {
                        return;
                    }
                }
                if page.more && page.change < upto {
                    self.mends.push_front(Mend::Group { group, after: page.change, upto });
                }
            }
            (Mend::Chain { group, sender }, Answer::Chain { items, more }) => {
                let mut any = false;
                for link in &items {
                    any = true;
                    self.take_again(ctx, &link.item);
                    if self.stopped.is_some() {
                        return;
                    }
                }
                let held = self.device.chain_head(&group, &sender).map(|h| h.seq).unwrap_or(0);
                if self.withheld.get(&(group, sender)).is_some_and(|seq| held >= *seq) {
                    self.withheld.remove(&(group, sender));
                } else if any && more {
                    self.mends.push_front(mend);
                }
            }
            _ => {}
        }
    }

    /// An envelope read again out of the order's turn (a mend): taken if its chain takes it now; one the chain
    /// holds already is read back (its body, now that the key is there).
    fn take_again(&mut self, ctx: &mut Ctx<'_>, item: &EnvelopeItem) {
        let void = Self::void_code(item);
        let mut received = match self.device.receive_envelope(&item.envelope, item.change, true, void.as_ref(), ctx.now) {
            Ok(received) => received,
            Err(e) => return self.local(ctx, &e),
        };
        if received.outcome == EnvelopeOutcome::Refused {
            match received.code {
                Some(Error::Replay) => match self.device.receive_envelope(&item.envelope, item.change, false, void.as_ref(), ctx.now) {
                    Ok(again) => received = again,
                    Err(e) => return self.local(ctx, &e),
                },
                Some(Error::GroupBehind) => {
                    let held = self.device.group(&received.header.group).ok();
                    if !held.is_some_and(|g| g.epoch >= received.header.epoch) {
                        self.note_behind(received.header.group, item.change, received.header.epoch);
                    }
                    return;
                }
                Some(Error::Gap) => {
                    let (group, sender, seq) = (received.header.group, received.header.sender, received.header.seq);
                    self.note_withheld(group, sender, seq, item.change);
                    return;
                }
                _ => return,
            }
        }
        if received.outcome == EnvelopeOutcome::Refused {
            return;
        }
        if received.code == Some(Error::NoKey) {
            self.note_keyless(received.header.group, item.change);
        }
        self.report(Report::Envelope { received: Box::new(received), ordered: false });
    }

    /// The hub withholds a part of a chain: said, in every phase, and asked for again when the order rests.
    fn note_withheld(&mut self, group: GroupId, sender: DeviceId, seq: u64, change: u64) {
        let known = self.withheld.get(&(group, sender)).copied();
        if known.is_none() && self.withheld.len() >= BEHIND_MAX {
            self.want_reread(change.saturating_sub(1));
        } else {
            self.withheld.insert((group, sender), known.map_or(seq, |k| k.max(seq)));
        }
        if known.is_none() {
            self.report(Report::Alert(Alert::Withheld { group, sender, change }));
        }
    }

    fn note_keyless(&mut self, group: GroupId, change: u64) {
        if let Some(first) = self.keyless.get_mut(&group) {
            *first = (*first).min(change);
        } else if self.keyless.len() < BEHIND_MAX {
            self.keyless.insert(group, change);
        } else {
            self.want_reread(change.saturating_sub(1));
        }
    }

    /// Asks for the room's order to be read once more from `from` (the seam to the reading of the past).
    fn want_reread(&mut self, from: u64) {
        self.reread_from = Some(self.reread_from.map_or(from, |f| f.min(from)));
    }

    fn take_one(&mut self, ctx: &mut Ctx<'_>, item: &ChangeItem, groups_changed: &mut bool) -> Taken {
        match item {
            ChangeItem::Envelope(e) => self.take_envelope(ctx, e),
            ChangeItem::Commit(l) => self.take_log(ctx, l, true, groups_changed),
            ChangeItem::Message(l) => self.take_log(ctx, l, false, groups_changed),
        }
    }

    fn void_code(item: &EnvelopeItem) -> Option<Error> {
        item.void_code.as_deref().map(|code| Error::from_code(code).unwrap_or(Error::HubVoidedOther))
    }

    fn take_envelope(&mut self, ctx: &mut Ctx<'_>, item: &EnvelopeItem) -> Taken {
        if let Some(halt) = self.agent_guard_envelope(ctx, &item.envelope) {
            return halt;
        }
        let void = Self::void_code(item);
        let received = match self.device.receive_envelope(&item.envelope, item.change, true, void.as_ref(), ctx.now) {
            Ok(received) => received,
            Err(e) => {
                self.local(ctx, &e);
                if self.stopped.is_some() {
                    return Taken::Halt;
                }
                // not an envelope at all: nothing of it is taken
                self.report(Report::Alert(Alert::NotAnEnvelope { change: item.change, code: e.code() }));
                return Taken::Done;
            }
        };
        if received.outcome == EnvelopeOutcome::Refused {
            match received.code {
                Some(Error::GroupBehind) => {
                    let group = received.header.group;
                    let held = self.device.group(&group).ok();
                    // A group this device holds far enough refuses only what lies before its own start there.
                    if held.as_ref().is_some_and(|g| g.epoch >= received.header.epoch) {
                        return Taken::Done;
                    }
                    // A Welcome this device has not taken yet may be what it lacks.
                    if !self.tried.welcomes && self.welcomes_due() {
                        self.tried.welcomes = true;
                        return Taken::Wait(Repair::Welcomes { after: self.welcomes_after, joined: Vec::new(), asked: false });
                    }
                    // Still behind: noted, and read again once the group is there. The position passes it; the
                    // device's own cursor does not (it stays for a group it is a leaf of).
                    self.note_behind(group, item.change, received.header.epoch);
                    return Taken::Done;
                }
                Some(Error::Gap) => {
                    let (group, sender, seq) = (received.header.group, received.header.sender, received.header.seq);
                    if !self.tried.chain {
                        self.tried.chain = true;
                        let after = self.device.chain_head(&group, &sender).map(|h| h.seq).unwrap_or(0);
                        return Taken::Wait(Repair::Chain { group, sender, after, before: seq, asked: false });
                    }
                    // The hub withholds a part of the chain. Said in every phase; the envelope stays out, and the
                    // chain is asked for again from what this device holds when the order next rests.
                    self.note_withheld(group, sender, seq, item.change);
                    return Taken::Done;
                }
                _ => return Taken::Done, // a replay, a forgery: nothing of it is taken
            }
        }
        if received.code == Some(Error::NoKey) {
            // chained without its body: read back once the key was handed over
            self.note_keyless(received.header.group, item.change);
        }
        let command = self.agent_gate(ctx, &received);
        self.report(Report::Envelope { received: Box::new(received), ordered: true });
        if let Some(command) = command {
            self.report(Report::Command(Box::new(command)));
        }
        Taken::Done
    }

    fn welcomes_due(&mut self) -> bool {
        match &mut self.welcomes_idle {
            None => true,
            Some(n) => {
                *n += 1;
                if *n >= WELCOMES_AGAIN {
                    self.welcomes_idle = None;
                    true
                } else {
                    false
                }
            }
        }
    }

    fn note_behind(&mut self, group: GroupId, change: u64, epoch: u64) {
        if let Some((c, e)) = self.behind.get_mut(&group) {
            *c = (*c).min(change);
            *e = (*e).min(epoch);
        } else if self.behind.len() < BEHIND_MAX {
            self.behind.insert(group, (change, epoch));
        } else {
            // (no room for one more group: one number stands for all of them)
            self.want_reread(change.saturating_sub(1));
        }
    }

    fn take_log(&mut self, ctx: &mut Ctx<'_>, item: &LogItem, commit: bool, groups_changed: &mut bool) -> Taken {
        let entry = LogEntry {
            change: item.change,
            group: item.group,
            kind: if commit {
                LogKind::Commit { bytes: &item.bytes, recovery_auth: item.recovery_auth.as_deref() }
            } else {
                LogKind::Message { bytes: &item.bytes }
            },
        };
        let before = self.agent_before(&item.group);
        match self.device.process_log_entry(&entry, ctx.now) {
            Ok(processed) => {
                if self.agent_processed(ctx, item.group, &processed, before) {
                    self.report(Report::Log { change: item.change, group: item.group, processed: Box::new(processed) });
                    return Taken::Halt;
                }
                // The founding of a group this device does not hold: if it was added there, its Welcome waits (the
                // stream says so only to who is connected then). Read once the items at hand are through.
                if commit && item.epoch == 0 && matches!(processed, Processed::Skipped) && self.device.is_human() {
                    self.welcome_waits = (true, true);
                }
                self.processed(ctx, item.change, item.group, processed, groups_changed);
                Taken::Done
            }
            Err(e) if self.agent_log_refused(ctx, &e) => Taken::Halt,
            Err(e) => match log_finding(&e) {
                LogFinding::Duplicate => Taken::Done,
                LogFinding::Local => {
                    self.local(ctx, &e);
                    Taken::Halt
                }
                LogFinding::Early => {
                    // What it builds on may be a Welcome not taken yet. Nothing is read ahead of the hub's order
                    // for it: an entry that still comes early halts, and the next pass tries it again.
                    if !self.tried.welcomes {
                        self.tried.welcomes = true;
                        return Taken::Wait(Repair::Welcomes { after: self.welcomes_after, joined: Vec::new(), asked: false });
                    }
                    Taken::Halt
                }
                LogFinding::BadGroup => {
                    self.report(Report::Alert(Alert::BadGroup { group: item.group, change: item.change, code: e.code() }));
                    if commit {
                        let (group, n) = (item.group, item.n);
                        self.ask(ctx, Route::RejectCommit { group: &group, n }, true, Asked::Told);
                    }
                    Taken::Halt
                }
            },
        }
    }

    fn processed(&mut self, ctx: &mut Ctx<'_>, change: u64, group: GroupId, processed: Processed, groups_changed: &mut bool) {
        match &processed {
            Processed::Commit { removed, facts, .. } => {
                *groups_changed = true;
                if !group.is_room() {
                    let (adds, removes) = (facts.adds.clone(), facts.removes.clone());
                    self.note_readmit(ctx, &group, &adds, &removes);
                }
                if *removed && group.is_room() {
                    self.report(Report::Log { change, group, processed: Box::new(processed) });
                    self.stop(ctx, Stopped::Removed);
                    return;
                }
            }
            Processed::OwnCommit | Processed::Observed(_) | Processed::JoinSuperseded { .. } => *groups_changed = true,
            Processed::Message(Received::Keys { taken, .. }) if *taken > 0 => {
                // keys that open what was chained without them: those envelopes are read back, and no more
                self.keys_came();
            }
            Processed::Message(Received::NewerVersion { from }) => self.report(Report::Alert(Alert::NewerVersion { from: *from })),
            Processed::Message(_) | Processed::Skipped => {}
        }
        self.report(Report::Log { change, group, processed: Box::new(processed) });
    }

    // ---- repairs

    fn repair_failed(&mut self, ctx: &mut Ctx<'_>, failure: Failure, unauthorised: bool) {
        // the same repair is asked again after a wait; the head of the order stays where it is
        match &mut self.repair {
            Some(Repair::Welcomes { asked, .. } | Repair::Logs { asked, .. } | Repair::Chain { asked, .. }) => *asked = false,
            None => {}
        }
        if unauthorised {
            return;
        }
        let retry_after = match failure {
            Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
            _ => None,
        };
        self.order_retry.fail(ctx.now, retry_after, &mut self.jitter);
    }

    fn on_welcomes(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>, unauthorised: bool) {
        let rows = match answer {
            Ok(Answer::Welcomes(rows)) => rows,
            Ok(_) => return self.repair_failed(ctx, Failure::BadAnswer { why: "welcomes" }, false),
            Err(failure) => return self.repair_failed(ctx, failure, unauthorised),
        };
        self.order_retry.ok();
        let Some(Repair::Welcomes { mut joined, mut after, .. }) = self.repair.take() else { return };
        let expected = WelcomeExpectation { room: self.room, committer: None };
        let more = !rows.is_empty();
        // `welcomes_after` passes only what is settled: a Welcome read before the room Commit it builds on is read
        // again by the next look (the stream says a Welcome before the Commits ahead of it are taken)
        let mut settled = true;
        for row in rows {
            after = after.max(row.id);
            // (the hub keeps a Welcome until this device writes in its group: one joined before is passed over)
            if self.device.group(&row.group).is_err() {
                match self.device.join_welcome(&row.welcome, &expected, ctx.now) {
                    Ok(j) => joined.push(j.group),
                    // (not yet: the room is behind, or this device has not reached the epoch it was made in)
                    Err(Error::WrongEpoch | Error::RoomBehind) => settled = false,
                    Err(e) => {
                        self.local(ctx, &e);
                        if self.stopped.is_some() {
                            return;
                        }
                        self.report(Report::Alert(Alert::Welcome { group: row.group, code: e.code() }));
                    }
                }
            }
            if settled {
                self.welcomes_after = self.welcomes_after.max(row.id);
            }
        }
        if !settled {
            self.welcome_waits = (true, false);
        }
        if more {
            // an answer holds a bounded stretch: asked again after its last row until one is empty
            self.repair = Some(Repair::Welcomes { after, joined, asked: false });
            return;
        }
        if joined.is_empty() {
            self.welcomes_idle = Some(0);
            return;
        }
        self.welcomes_idle = None;
        self.desk.want = true;
        self.tending.look_for_archived();
        self.groups_changed();
        // A group joined late: its log is handed again from its start up to the position, so the join has its
        // place in the hub's order. What lies after the position comes at its place in the one order.
        self.repair = Some(Repair::Logs { groups: joined.into(), after: 0, asked: false });
    }

    fn on_group_log(&mut self, ctx: &mut Ctx<'_>, group: GroupId, upto: u64, answer: Result<Answer, Failure>, unauthorised: bool) {
        let (items, more) = match answer {
            Ok(Answer::GroupLog { items, more }) => (items, more),
            Ok(_) => return self.repair_failed(ctx, Failure::BadAnswer { why: "log" }, false),
            Err(failure) => return self.repair_failed(ctx, failure, unauthorised),
        };
        self.order_retry.ok();
        let Some(Repair::Logs { mut groups, mut after, .. }) = self.repair.take() else { return };
        let mut through = !more || items.is_empty();
        let mut changed = false;
        for item in &items {
            let (commit, log) = match item {
                ChangeItem::Commit(l) => (true, l),
                ChangeItem::Message(l) => (false, l),
                ChangeItem::Envelope(_) => continue,
            };
            if log.change > upto {
                through = true;
                break;
            }
            after = log.n;
            let entry = LogEntry {
                change: log.change,
                group,
                kind: if commit {
                    LogKind::Commit { bytes: &log.bytes, recovery_auth: log.recovery_auth.as_deref() }
                } else {
                    LogKind::Message { bytes: &log.bytes }
                },
            };
            match self.device.process_log_entry(&entry, ctx.now) {
                Ok(processed) => self.processed(ctx, log.change, group, processed, &mut changed),
                Err(e) => match log_finding(&e) {
                    LogFinding::Duplicate => {}
                    LogFinding::Local => {
                        self.local(ctx, &e);
                        return;
                    }
                    // not yet, or not at all: this group's reading ends here; its entries come in the one order
                    LogFinding::Early | LogFinding::BadGroup => {
                        through = true;
                        break;
                    }
                },
            }
            if self.stopped.is_some() {
                return;
            }
        }
        if changed {
            self.groups_changed();
        }
        if through {
            groups.pop_front();
            after = 0;
        }
        if !groups.is_empty() {
            self.repair = Some(Repair::Logs { groups, after, asked: false });
        }
    }

    fn on_chain(&mut self, ctx: &mut Ctx<'_>, group: GroupId, sender: DeviceId, before: u64, answer: Result<Answer, Failure>, unauthorised: bool) {
        let (items, more) = match answer {
            Ok(Answer::Chain { items, more }) => (items, more),
            Ok(_) => return self.repair_failed(ctx, Failure::BadAnswer { why: "chain" }, false),
            // (a chain the hub refuses to serve is a chain withheld: the head is tried again and says so)
            Err(Failure::Refused { .. }) if !unauthorised => {
                self.repair = None;
                return;
            }
            Err(failure) => return self.repair_failed(ctx, failure, unauthorised),
        };
        self.order_retry.ok();
        self.repair = None;
        let mut last = None;
        let mut reached = items.is_empty() || !more;
        for link in items {
            if link.seq >= before {
                reached = true;
                break;
            }
            last = Some(link.seq);
            let void = Self::void_code(&link.item);
            match self.device.receive_envelope(&link.item.envelope, link.item.change, true, void.as_ref(), ctx.now) {
                Ok(received) if received.outcome != EnvelopeOutcome::Refused => {
                    if received.code == Some(Error::NoKey) {
                        // (chained without its body, as on the order: read back once the key was handed over)
                        self.note_keyless(received.header.group, link.item.change);
                    }
                    self.report(Report::Envelope { received: Box::new(received), ordered: false })
                }
                Ok(_) => {}
                Err(e) => {
                    self.local(ctx, &e);
                    if self.stopped.is_some() {
                        return;
                    }
                }
            }
        }
        if !reached {
            if let Some(after) = last {
                self.repair = Some(Repair::Chain { group, sender, after, before, asked: false });
            }
        }
        // (else the head is tried again: taken now, or still a gap, which is then said)
    }

    // ---- the Desk: what is open now, before the order is read

    fn drive_desk(&mut self, ctx: &mut Ctx<'_>) {
        if self.desk.retry.waiting(ctx.now) || self.agent.is_some() {
            return;
        }
        if self.desk.want && self.desk.asked.is_none() {
            self.desk.asked = self.ask(ctx, Route::Desk { only: DeskOnly::Active }, true, Asked::Desk);
            if self.desk.asked.is_none() {
                self.desk.want = false;
            }
        }
        while self.desk.flying < FIRSTS_AT_ONCE {
            let Some((route, object, after)) = self.desk.firsts.pop_front() else { break };
            let asked = self.ask(ctx, Route::Object { route, object: &object, after, limit: 1 }, true, |expect| Asked::First { expect, route, object, after });
            if asked.is_some() {
                self.desk.flying += 1;
            }
        }
    }

    /// An envelope read out of the hub's order to be shown (a Desk's version): as the core reads it there, marked
    /// provisional until its chain reaches it. Nothing of the chain moves.
    fn show(&mut self, ctx: &mut Ctx<'_>, item: &EnvelopeItem) {
        let void = Self::void_code(item);
        match self.device.receive_envelope(&item.envelope, item.change, false, void.as_ref(), ctx.now) {
            Ok(received) if received.outcome != EnvelopeOutcome::Refused => self.report(Report::Envelope { received: Box::new(received), ordered: false }),
            Ok(_) => {}
            Err(e) => self.local(ctx, &e),
        }
    }

    /// Envelopes the App read out of the hub's order (a page of a Scribble Board's items): each read as `show`
    /// reads a Desk's version, and reported as `Envelope { ordered: false }`.
    pub fn show_items(&mut self, ctx: &mut Ctx<'_>, items: &[EnvelopeItem]) {
        for item in items {
            if self.stopped.is_some() {
                return;
            }
            self.show(ctx, item);
        }
    }

    fn on_desk(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.desk.asked = None;
        let desk = match answer {
            Ok(Answer::Desk(desk)) => desk,
            Err(_) if unauthorised => return,
            Ok(_) | Err(_) => {
                let retry_after = match answer {
                    Err(Failure::NotNow { retry_after_ms, .. }) => retry_after_ms,
                    Err(Failure::BadAnswer { why }) => {
                        self.report(Report::Alert(Alert::BadAnswer { why }));
                        None
                    }
                    _ => None,
                };
                return self.desk.retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        };
        self.desk.retry.ok();
        self.desk.want = false;
        self.desk.complete = false;
        self.desk.firsts.clear();
        for register in &desk.registers {
            self.show(ctx, register);
        }
        let mut open = Vec::new();
        for (route, objects) in [(ObjectRoute::Cards, &desk.cards), (ObjectRoute::PermissionRequests, &desk.permission_requests)] {
            for object in objects {
                let Some(version) = &object.version else { continue };
                self.show(ctx, version);
                open.push((object.object, version.change));
                // A card revised since it was asked: its first version says when it was asked, which the Desk's
                // order goes by. Read now, so no card moves when the past is read. (`first_change` is the hub's
                // word: a wrong one costs this read, the envelope is checked like any other.)
                if route == ObjectRoute::Cards && object.first_change < version.change {
                    self.desk.firsts.push_back((route, object.object, object.first_change.saturating_sub(1)));
                }
                if self.stopped.is_some() {
                    return;
                }
            }
        }
        self.report(Report::Desk { at: desk.change, open, truncated: desk.truncated });
        self.desk_through();
    }

    fn on_first(&mut self, ctx: &mut Ctx<'_>, route: ObjectRoute, object: ObjectId, after: u64, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.desk.flying = self.desk.flying.saturating_sub(1);
        match answer {
            Ok(Answer::Object(got)) => {
                self.desk.retry.ok();
                if let Some(first) = got.items.first() {
                    self.show(ctx, first);
                }
            }
            // asked again once signed in, or after a wait; a refusal ends it (the past brings the version)
            Err(Failure::Refused { .. }) if !unauthorised => {}
            Ok(_) => {}
            Err(failure) => {
                self.desk.firsts.push_back((route, object, after));
                if !unauthorised {
                    let retry_after = match failure {
                        Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
                        _ => None,
                    };
                    self.desk.retry.fail(ctx.now, retry_after, &mut self.jitter);
                }
            }
        }
        self.desk_through();
    }

    fn desk_through(&mut self) {
        if !self.desk.complete && !self.desk.want && self.desk.asked.is_none() && self.desk.firsts.is_empty() && self.desk.flying == 0 {
            self.desk.complete = true;
            self.report(Report::DeskComplete);
        }
    }

    // ---- the stream

    fn drive_stream(&mut self, ctx: &mut Ctx<'_>) {
        if self.background || self.stream.is_some() || self.stream_retry.waiting(ctx.now) {
            return;
        }
        // Opened beside the first page at the head the hub named (a hint: presence and Welcomes do not wait for a
        // long catch-up), else at the end of the order. `page_through` opens it again if the hint was too high.
        let from = if self.pass || !self.todo.is_empty() {
            match self.head_hint {
                Some(head) => head.max(self.todo_upto).max(self.position),
                None => return,
            }
        } else {
            self.position
        };
        let site = Site { url: &self.hub, token: self.token.as_ref().map(|t| t.value.as_str()), client: &self.client, lease: self.lease_header() };
        let Ok((request, _)) = self.codec.request(&site, &Route::Stream { after: from }) else {
            return;
        };
        self.flush(ctx);
        let id = ctx.id();
        self.stream = Some(StreamConn { id, reader: StreamReader::new(from), opened: false, from, heard_at: ctx.now });
        ctx.out.push(Effect::OpenStream { id, request });
    }

    fn close_stream(&mut self, ctx: &mut Ctx<'_>) {
        if let Some(stream) = self.stream.take() {
            ctx.out.push(Effect::Cancel { id: stream.id });
        }
    }

    // ---- upkeep: what a device does now and then without being asked

    /// A round is due once the order is at rest, then every [`UPKEEP_EVERY_MS`]: it begins with the hub's count of
    /// this device's unused KeyPackages.
    fn drive_upkeep(&mut self, ctx: &mut Ctx<'_>) {
        // (a sign-in's KeyPackages go up once its sessions are joined: a founding claims one of every human device)
        if self.joining.is_some() || !self.caught_up || self.pass || !self.todo.is_empty() || self.upkeep_asked.is_some() || ctx.now < self.upkeep_at || self.upkeep_retry.waiting(ctx.now) {
            return;
        }
        self.upkeep_asked = self.ask(ctx, Route::KeyPackagesHeld, true, Asked::KeyPackagesHeld);
        if self.upkeep_asked.is_none() {
            self.upkeep_at = ctx.now + UPKEEP_EVERY_MS;
        }
    }

    fn on_key_packages_held(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.upkeep_asked = None;
        let unused = match answer {
            Ok(Answer::KeyPackagesHeld { unused }) => unused,
            Err(_) if unauthorised => return,
            Ok(_) | Err(_) => {
                let retry_after = match answer {
                    Err(Failure::NotNow { retry_after_ms, .. }) => retry_after_ms,
                    _ => None,
                };
                return self.upkeep_retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        };
        self.upkeep_retry.ok();
        self.upkeep_at = ctx.now + UPKEEP_EVERY_MS;
        self.key_packages_at_hub = Some(unused);
        // KeyPackages (14.2): enough single-use ones, and a last-resort one now and then
        if let Err(e) = self.device.key_packages_to_upload(unused as usize, ctx.now) {
            self.local(ctx, &e);
        }
        // (the groups are looked at with every round as well: a gap whose repair failed is not forgotten, and a
        //  leaf that grew old is renewed)
        self.tending.timed();
        // `heads` (9.0.7) in each live group when the core says they are due: what a hub withholds shows there
        for group in self.device.groups().unwrap_or_default() {
            if group.archived || group.pending {
                continue;
            }
            match self.device.heads_due(&group.group, ctx.now) {
                Ok(Some(value)) => {
                    let draft = Draft::Register { group: group.group, name: "heads".into(), value: Some(SecretBytes::new(value)) };
                    if let Err(e) = self.device.seal(&draft, None, &[], ctx.now) {
                        self.local(ctx, &e);
                    }
                }
                Ok(None) => {}
                Err(e) => self.local(ctx, &e),
            }
            if self.stopped.is_some() {
                return;
            }
        }
        // the content keys this device still lacks a while after an envelope did not open for them (7.1.1)
        match self.device.key_requests(ctx.now) {
            Ok(requests) => {
                for request in requests {
                    let group = request.group;
                    self.ask(ctx, Route::Wish { kind: WishKind::Handover, group: Some(&group), key_package: None }, true, Asked::Told);
                }
            }
            Err(e) => self.local(ctx, &e),
        }
        // the wishes for keys that came while this device was away (the hub keeps them seven days)
        self.ask_wishes(ctx);
    }

    // ---- the outbox: one entry in flight, strictly in order

    fn drive_pump(&mut self, ctx: &mut Ctx<'_>) {
        if self.posting.is_some() || self.pump_retry.waiting(ctx.now) {
            return;
        }
        let entries = self.device.outbox();
        let Some(entry) = entries.first() else {
            if self.blocked.take().is_some() {
                self.report(Report::Blocked(None));
            }
            return;
        };
        // a sign-in's joins of session groups from outside at the head: together, in one request
        let together = if self.joins_alone { 0 } else { joins::joins_at(&entries) };
        if together > 1 {
            let joins = &entries[..together];
            let ids: Vec<(u64, Option<GroupId>)> = joins.iter().map(|e| (e.id, e.group)).collect();
            self.posting = self.ask(ctx, Route::Joins { entries: joins }, true, |expect| Asked::Joins { expect, entries: ids });
            if self.posting.is_some() {
                return;
            }
        }
        let room = self.room;
        let (id, kind, group) = (entry.id, entry.kind, entry.group);
        self.posting = self.post(ctx, &room, entry, |expect| Asked::Post { expect, entry: id, kind, group });
        if self.posting.is_none() {
            // an entry this build cannot post (a founding or a recovery without its extra): the pump waits
            self.pump_retry.wait(ctx.now, BLOCKED_RETRY_MS);
        }
    }

    fn post(&mut self, ctx: &mut Ctx<'_>, room: &RoomId, entry: &OutboxEntry, asked: impl FnOnce(Expect) -> Asked) -> Option<IoId> {
        self.ask(ctx, Route::Outbox { room, entry, extra: None }, true, asked)
    }

    #[allow(clippy::too_many_arguments)]
    fn on_posted(
        &mut self,
        ctx: &mut Ctx<'_>,
        entry: u64,
        kind: OutboxKind,
        group: Option<GroupId>,
        answer: Result<Answer, Failure>,
        unauthorised: bool,
        refusal: Option<Refusal>,
    ) {
        self.posting = None;
        if self.room_join.is_some() {
            return self.on_room_posted(ctx, entry, answer, unauthorised, refusal);
        }
        if self.founding.is_some() && kind == OutboxKind::RoomFounding {
            return self.on_founding_posted(ctx, entry, kind, group, answer, refusal);
        }
        let failure = match answer {
            Ok(Answer::Outbox(taken)) => {
                self.pump_retry.ok();
                return self.accepted(ctx, entry, kind, group, taken);
            }
            Ok(_) => Failure::BadAnswer { why: "an outbox answer" },
            Err(failure) => failure,
        };
        if unauthorised {
            return; // posted again, unchanged, once signed in
        }
        if let Some(retry_after) = self.not_accepted(ctx, entry, kind, group, failure, refusal) {
            self.pump_retry.fail(ctx.now, retry_after, &mut self.jitter);
        }
    }

    /// The hub took an outbox entry.
    fn accepted(&mut self, ctx: &mut Ctx<'_>, entry: u64, kind: OutboxKind, group: Option<GroupId>, taken: OutboxAnswer) {
        if self.blocked.take().is_some() {
            self.report(Report::Blocked(None));
        }
        // (`not-found`: the stream brought this device's own Commit before the POST was answered, and the
        //  core merged it and dropped the entry then)
        let merged = match self.device.outbox_accepted(entry, Accepted { change: taken.change }) {
            Ok(()) => false,
            Err(Error::NotFound) => true,
            Err(e) => return self.local(ctx, &e),
        };
        if kind == OutboxKind::KeyPackages {
            self.key_packages_at_hub = taken.unused.or(self.key_packages_at_hub);
        }
        self.report(Report::Accepted { id: entry, kind, change: taken.change });
        if carries_commit(kind) {
            // An accepted Commit is merged where the hub's order shows it. Without a stream that is the
            // next page.
            self.report(Report::Groups);
            self.tending.sent(merged);
            if !self.stream.as_ref().is_some_and(|s| s.opened) {
                self.pass = true;
            }
        } else if kind == OutboxKind::Message {
            // (a message of this device is a step of upkeep, a key handover say: the next step is looked for now)
            self.tending.want = true;
        }
        self.join_answered(kind, group, None);
    }

    /// What a failed post means for the entry. `Some`: a transient failure (not reached, 5xx, throttled, an answer
    /// the route does not allow, a refusal that is not the hub's own, a code this build does not know): the same
    /// entry again after a wait, never dropped, never reordered; inside, the hub's `Retry-After`. `None`: settled
    /// here (refused for good, or the pump halts on it and has set its own wait).
    fn not_accepted(&mut self, ctx: &mut Ctx<'_>, entry: u64, kind: OutboxKind, group: Option<GroupId>, failure: Failure, refusal: Option<Refusal>) -> Option<Option<u64>> {
        // (`code` is set only for the hub's own refusal: a proxy's page never voids or rolls back)
        let named = match &failure {
            Failure::Refused { code: Some(code), .. } => Some(code.as_str()),
            _ => None,
        };
        let code = named.and_then(Error::from_code).filter(|code| !refusal_is_passing(code));
        let Some(code) = code else {
            let retry_after = match (&failure, refusal.as_ref()) {
                (Failure::NotNow { retry_after_ms, .. }, _) => *retry_after_ms,
                (_, Some(refusal)) => refusal.retry_after_ms,
                _ => None,
            };
            if named == Some("client-too-old") {
                self.report(Report::Alert(Alert::ClientTooOld));
            }
            return Some(retry_after);
        };
        let voided = refusal.as_ref().is_some_and(|r| r.voided);
        // An envelope the hub refused WITHOUT keeping a void record, for a reason that is not the end of the group
        // or of this device: its number is not taken at the hub and may never be signed again (9.0.1), so every
        // later envelope of that chain would meet `gap`. The pump halts on it, keeps the bytes and tries again.
        if kind == OutboxKind::Envelope && !voided && !is_dead(&code) {
            if self.blocked != Some(entry) {
                self.blocked = Some(entry);
                self.report(Report::Blocked(Some((entry, code.code()))));
            }
            self.pump_retry.wait(ctx.now, BLOCKED_RETRY_MS);
            return None;
        }
        // A definitive refusal: told to the core and to the App (the echo is rolled back), and the pump goes on.
        let told = if kind != OutboxKind::Envelope {
            self.device.outbox_refused(entry, &code)
        } else if voided {
            self.device.outbox_voided(entry)
        } else {
            self.device.envelope_abandon(entry)
        };
        match told {
            Ok(()) | Err(Error::NotFound) => {}
            Err(e) => {
                self.local(ctx, &e);
                return None;
            }
        }
        self.pump_retry.ok();
        self.report(Report::Refused { id: entry, kind, code: code.code(), voided });
        self.report(Report::Groups);
        // the group moved on under it: the log is processed, and whoever asked builds it again
        if has_moved(&code) {
            self.pass = true;
        } else if carries_commit(kind) {
            // (a Commit refused for another reason: whatever of upkeep built it waits before it builds it again)
            self.tending.refused();
        }
        // 3.5: the epoch took all the envelopes it may hold. A human device moves the group on with an update of
        // its leaf, and writing goes on in the next epoch (whoever sealed the envelope seals it again).
        if code == Error::EpochFull && kind == OutboxKind::Envelope && self.device.is_human() {
            if let Some(group) = group {
                if let Err(e) = self.device.update(&group, true, ctx.now) {
                    self.local(ctx, &e);
                }
            }
        }
        self.join_answered(kind, group, Some(&code));
        None
    }
}
