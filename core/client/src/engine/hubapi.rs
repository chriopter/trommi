//! What the engine asks of the hub's codec (`crate::hub`, W2), and the typed data that crosses between them.
//!
//! The split: the engine decides WHAT to ask and WHEN (order, retries, the token it holds); the codec knows HOW a
//! route looks on the wire: it builds the whole [`HttpRequest`], reads an answer strictly (shape, sizes, ids,
//! base64url, the order of change numbers) and frames the event stream (untrusted bytes, bounded there), so that the
//! engine only ever sees values the protocol allows.
//!
//! An answer is checked against what was asked: [`HubCodec::request`] hands back an `Expect` of the codec's own
//! making (the `after` and `limit` of a page, the kind of an outbox entry, how many joins went together), the engine
//! keeps it beside the pending id and hands it to [`HubCodec::answer`]. The engine never looks inside.
//!
//! Until W2's codec lands, [`super::stub::StubCodec`] implements this with the checks the Linux app had.

use crate::app::io::{Header, HttpRequest, HttpResponse};
use crate::hub::Failure;
use trommi_core::ids::{DeviceId, GroupId, ObjectId, RoomId};
use trommi_core::store::OutboxEntry;

/// Where and as whom a request goes. The codec adds nothing of its own: no token it remembers, no clock.
#[derive(Clone, Copy, Debug)]
pub struct Site<'a> {
    /// The hub's address without a trailing slash ("https://hub.trommi.com").
    pub url: &'a str,
    /// The bearer token the engine holds; none for the routes that take none (`Challenge`, `Token`).
    pub token: Option<&'a str>,
    /// `Config.platform` and `Config.version`, for the `trommi-client` header.
    pub client: &'a str,
}

/// Which envelopes a read of the hub's order brings (hub-api.md point 46).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Only<'a> {
    /// Everything the asker may see.
    #[default]
    All,
    /// No envelope of an archived session (its Commits still come).
    Active,
    /// One group's envelopes alone.
    Group(&'a GroupId),
}

/// The object routes (`GET /v1/cards/{object}` and its siblings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectRoute {
    Cards,
    Notes,
    PermissionRequests,
    Artifacts,
}

/// A wish to the human devices (`POST /v1/requests`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WishKind {
    Readmit,
    Handover,
    Session,
}

/// What an outbox entry needs beside its own bytes (kept by the engine until the entry is answered).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutboxExtra {
    /// `RoomFounding`: the account made with the room (the body of `POST /v1/account`, JSON), and the found token.
    pub account: Option<Vec<u8>>,
    pub found_token: Option<String>,
    /// `RecoveryCommit`, `RecoveryFinish`: the recovery they belong to.
    pub recovery_id: Option<Vec<u8>>,
}

/// One request to the hub, by its meaning. Borrowed: an outbox entry may hold megabytes and is not copied to ask.
#[derive(Clone, Copy, Debug)]
pub enum Route<'a> {
    /// `GET /v1/rooms/{room}/challenge` (no token).
    Challenge { room: &'a RoomId },
    /// `POST /v1/rooms/{room}/tokens` (no token): the device's `HubAuth` and its signature.
    Token { room: &'a RoomId, auth: &'a [u8], signature: &'a [u8] },
    /// `GET /v1/changes?after=&limit=`: a page of the room's one order.
    Changes { after: u64, limit: u32, only: Only<'a> },
    /// `GET /v1/stream?after=`: the request of an `OpenStream` effect. It has no answer to read: its events go
    /// through [`HubCodec::stream_event`].
    Stream { after: u64 },
    /// `GET /v1/welcomes?after=`.
    Welcomes { after: u64 },
    /// `GET /v1/groups/{group}/log?after=&limit=[&kind=commit]`.
    GroupLog { group: &'a GroupId, after: u64, limit: u32, commits_only: bool },
    /// `GET /v1/groups/{group}/chains/{sender}?after=&limit=`.
    Chain { group: &'a GroupId, sender: &'a DeviceId, after: u64, limit: u32 },
    /// `GET /v1/desk?brief=1[&since=][&active=1]`.
    DeskBrief { since: Option<u64>, active: bool },
    /// `GET /v1/desk[?active=1]`: the open objects with their newest version, and every writer's newest value per
    /// register.
    Desk { active: bool },
    /// `GET /v1/cards/{object}?after=&limit=` and its siblings: every envelope of one object.
    Object { route: ObjectRoute, object: &'a ObjectId, after: u64, limit: u32 },
    /// One outbox entry exactly as the protocol core made it, to the route of its kind.
    Outbox { room: &'a RoomId, entry: &'a OutboxEntry, extra: Option<&'a OutboxExtra> },
    /// `POST /v1/commits`: joins of several session groups from outside in one request (1 to 64, one per group).
    Joins { entries: &'a [OutboxEntry] },
    /// `POST /v1/groups/read`: what joining several groups from outside needs (1 to 64, each group once).
    GroupsRead { groups: &'a [GroupId] },
    /// `PUT /v1/key-packages` with nothing to add: how many unused ones the hub holds.
    KeyPackagesHeld,
    /// `POST /v1/key-packages/claim`.
    ClaimKeyPackages { devices: &'a [DeviceId], partial: bool },
    /// `GET /v1/rooms/{room}/groups?after=&limit=`.
    RoomGroups { room: &'a RoomId, after: u64, limit: u32 },
    /// `GET /v1/groups/{group}/info[?epoch=]`.
    GroupInfo { group: &'a GroupId, epoch: Option<u64> },
    /// `POST /v1/requests`.
    Wish { kind: WishKind, group: Option<&'a GroupId>, key_package: Option<&'a [u8]> },
    /// `GET /v1/requests`.
    Wishes,
    /// `POST /v1/groups/{group}/reject` (14.7): the Commit numbered `n` in the group's log does not process.
    RejectCommit { group: &'a GroupId, n: u64 },
}

/// An entry of the room's one order.
#[derive(Clone, PartialEq, Eq)]
pub enum ChangeItem {
    Commit(LogItem),
    Message(LogItem),
    Envelope(EnvelopeItem),
}

impl ChangeItem {
    pub fn change(&self) -> u64 {
        match self {
            ChangeItem::Commit(l) | ChangeItem::Message(l) => l.change,
            ChangeItem::Envelope(e) => e.change,
        }
    }
}

impl std::fmt::Debug for ChangeItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = match self {
            ChangeItem::Commit(_) => "commit",
            ChangeItem::Message(_) => "message",
            ChangeItem::Envelope(_) => "envelope",
        };
        write!(f, "ChangeItem({kind} at {})", self.change())
    }
}

/// A Commit or an application message of a group's log.
#[derive(Clone, PartialEq, Eq)]
pub struct LogItem {
    pub change: u64,
    pub group: GroupId,
    /// Its number in its group's log (the `after` of `GroupLog`, the `n` of `RejectCommit`).
    pub n: u64,
    /// The epoch a Commit builds on, or a message was sent in.
    pub epoch: u64,
    /// When the hub took it (the hub's clock).
    pub at: u64,
    /// The device that posted it, as the hub says.
    pub sender: DeviceId,
    pub bytes: Vec<u8>,
    /// A Commit's `RecoveryAuth`, where the hub stores one beside it.
    pub recovery_auth: Option<Vec<u8>>,
}

/// An envelope as a read route serves it (pruned form when its body is gone).
#[derive(Clone, PartialEq, Eq)]
pub struct EnvelopeItem {
    pub change: u64,
    pub envelope: Vec<u8>,
    /// When the hub took it (the hub's clock).
    pub received_at: u64,
    /// The hub's void record, as spec section 16 names the code; a code this build does not know stays text.
    pub void_code: Option<String>,
    /// Only on the chain route: the envelope lies beyond its sender's Cut.
    pub cut: bool,
}

/// An envelope of a sender's chain (`Chain`).
#[derive(Clone, PartialEq, Eq)]
pub struct ChainItem {
    pub seq: u64,
    pub item: EnvelopeItem,
}

/// A page of the hub's order. The codec has checked: the items ascend strictly by `change`, all above the `after`
/// asked and none above `change`; `more` with `change == after` is refused.
#[derive(Clone, PartialEq, Eq)]
pub struct ChangesPage {
    pub items: Vec<ChangeItem>,
    /// The cursor for the next page.
    pub change: u64,
    pub more: bool,
    /// The room's newest change as the hub says it: a HINT (how far there is to read). The engine never lets it
    /// decide what was read.
    pub head: Option<u64>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct WelcomeRow {
    /// The row's number (the `after` of the next read).
    pub id: u64,
    pub group: GroupId,
    pub welcome: Vec<u8>,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskEntry {
    pub object: ObjectId,
    pub version_change: u64,
    pub head_change: u64,
}

/// `GET /v1/desk?brief=1`: the open cards and permission requests by id and version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskBrief {
    pub cards: Vec<DeskEntry>,
    pub permission_requests: Vec<DeskEntry>,
    /// With `since`: what changed after it and is open no more. None for a whole list.
    pub settled: Option<Vec<ObjectId>>,
    pub truncated: bool,
    pub change: u64,
}

/// An open object of the Desk.
#[derive(Clone, PartialEq, Eq)]
pub struct DeskObject {
    pub object: ObjectId,
    pub group: GroupId,
    /// The change of its first envelope and of its newest, as the hub says: hints for what to read.
    pub first_change: u64,
    pub head_change: u64,
    /// Its newest version; none when the hub holds none to serve.
    pub version: Option<EnvelopeItem>,
}

/// `GET /v1/desk`.
#[derive(Clone, PartialEq, Eq)]
pub struct Desk {
    pub cards: Vec<DeskObject>,
    pub permission_requests: Vec<DeskObject>,
    pub notes: Vec<DeskObject>,
    pub artifacts: Vec<DeskObject>,
    pub registers: Vec<EnvelopeItem>,
    /// The Desk was cut short: it decides nothing, the rest comes in the hub's order.
    pub truncated: bool,
    pub change: u64,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ObjectEnvelopes {
    pub object: ObjectId,
    pub group: GroupId,
    pub items: Vec<EnvelopeItem>,
    pub more: bool,
}

/// What the hub answered to an accepted outbox entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OutboxAnswer {
    /// The room's change number the hub gave the write; none for a kind that takes none.
    pub change: Option<u64>,
    /// `KeyPackages`: how many unused single-use ones the hub holds now.
    pub unused: Option<u32>,
    /// A Commit: the epoch it made. A message: its number in the group's log.
    pub epoch: Option<u64>,
    pub n: Option<u64>,
    /// A founding: what the hub founded.
    pub room: Option<RoomId>,
    pub group: Option<GroupId>,
}

/// One group of `GroupsRead`.
#[derive(Clone, PartialEq, Eq)]
pub struct GroupRead {
    pub group: GroupId,
    /// The current GroupInfo and its epoch.
    pub info: Vec<u8>,
    pub epoch: u64,
    /// The GroupInfo of epoch 0; none for a group still at epoch 0 (`info` is it).
    pub founding: Option<Vec<u8>>,
    /// The group's first Commits, oldest first.
    pub log: Vec<LogItem>,
    /// The log goes on after them: the engine reads on (`GroupLog`, Commits only).
    pub more: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupRowKind {
    Room,
    Main,
    Helper,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupRow {
    pub group: GroupId,
    pub kind: GroupRowKind,
    pub epoch: u64,
    pub room_epoch: u64,
    pub live: bool,
    pub stale: bool,
    pub leaves: Vec<DeviceId>,
}

/// What the hub takes a signed-in key for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Human,
    Agent,
    Helper,
    Recovery,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WishRow {
    pub id: u64,
    pub device: DeviceId,
    pub kind: WishRowKind,
    pub group: Option<GroupId>,
    pub key_package: Option<Vec<u8>>,
    pub n: Option<u64>,
    pub committer: Option<DeviceId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WishRowKind {
    Readmit,
    Handover,
    Session,
    Reject,
}

/// A checked answer.
#[derive(Clone, PartialEq, Eq)]
pub enum Answer {
    Challenge([u8; 32]),
    /// `expires_at` is the hub's clock, a hint: the engine bounds the life it assumes.
    Token { token: String, expires_at: u64, role: Role },
    Changes(ChangesPage),
    Welcomes(Vec<WelcomeRow>),
    /// Commits and messages of one group, as `ChangeItem`s (never an envelope).
    GroupLog { items: Vec<ChangeItem>, more: bool },
    Chain { items: Vec<ChainItem>, more: bool },
    DeskBrief(DeskBrief),
    Desk(Desk),
    Object(ObjectEnvelopes),
    Outbox(OutboxAnswer),
    /// One per entry asked, in order.
    Joins(Vec<Result<OutboxAnswer, Failure>>),
    /// One per group asked, in order. A row refused as `too-large` did not fit the answer (the engine reads that
    /// group alone); `not-found`: the asker may not see the group.
    GroupsRead(Vec<Result<GroupRead, Failure>>),
    KeyPackagesHeld { unused: u32 },
    Claimed { key_packages: Vec<(DeviceId, Vec<u8>)>, missing: Vec<DeviceId> },
    RoomGroups { items: Vec<GroupRow>, more: bool, after: u64 },
    GroupInfo { epoch: u64, group_info: Vec<u8> },
    Wished { id: u64 },
    Wishes(Vec<WishRow>),
    /// A route whose answer carries nothing (`RejectCommit`).
    Done,
}

impl std::fmt::Debug for Answer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // (an answer may hold a token or a Welcome: its kind is all a log needs)
        f.write_str(match self {
            Answer::Challenge(_) => "Challenge",
            Answer::Token { .. } => "Token",
            Answer::Changes(_) => "Changes",
            Answer::Welcomes(_) => "Welcomes",
            Answer::GroupLog { .. } => "GroupLog",
            Answer::Chain { .. } => "Chain",
            Answer::DeskBrief(_) => "DeskBrief",
            Answer::Desk(_) => "Desk",
            Answer::Object(_) => "Object",
            Answer::Outbox(_) => "Outbox",
            Answer::Joins(_) => "Joins",
            Answer::GroupsRead(_) => "GroupsRead",
            Answer::KeyPackagesHeld { .. } => "KeyPackagesHeld",
            Answer::Claimed { .. } => "Claimed",
            Answer::RoomGroups { .. } => "RoomGroups",
            Answer::GroupInfo { .. } => "GroupInfo",
            Answer::Wished { .. } => "Wished",
            Answer::Wishes(_) => "Wishes",
            Answer::Done => "Done",
        })
    }
}

/// One event of the live stream. Only `Change` has a number and is replayed on resume.
#[derive(Clone, PartialEq, Eq)]
pub enum StreamEvent {
    Change(ChangeItem),
    /// A piece that was only passed on (7.2).
    Relay { group: GroupId, message: Vec<u8> },
    /// A Welcome waits for this device.
    Welcome { group: GroupId },
    Wish(WishRow),
    InviteRequest { invite: Vec<u8> },
    Presence(Presence),
    /// The end of the list of who is here that a human device's new stream is told first.
    Here,
    Archived { group: GroupId },
    FileEvicted { file: Vec<u8> },
}

/// A presence report of the stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub device: DeviceId,
    pub online: bool,
    pub lost: bool,
    pub hears: Option<bool>,
    pub working: Option<bool>,
    pub last_call_at: Option<u64>,
}

/// What a refusal of the hub says beside its code. Only for an answer that IS the hub's refusal: a code of spec
/// section 16 under its own status. Anything else (a proxy's page) is no refusal, and the engine never voids or
/// rolls back on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub status: u16,
    /// The hub keeps the refused envelope as a void record (9.0.8).
    pub voided: bool,
    pub retry_after_ms: Option<u64>,
    /// `epoch-taken` and its kin: where the group stands.
    pub epoch: Option<u64>,
    pub seq: Option<u64>,
    pub device: Option<DeviceId>,
    pub group: Option<GroupId>,
}

/// What a chunk of the stream gave: the events it completed, in order, and what ended the reading. Events before
/// a bad one are still handed over; with a `failure` the engine closes the connection.
pub struct Fed {
    pub events: Vec<StreamEvent>,
    pub failure: Option<Failure>,
}

/// The reading of one open stream: framing (bounded lines and events), and the checks of its events (a change's
/// event id is its number; numbers rise above `after` and above the last one). One per connection.
pub trait StreamReader {
    /// The answer's head. `Err`: not a stream to read (the engine still feeds the body, and `ended` classifies it).
    fn opened(&mut self, status: u16, headers: &[Header]) -> Result<(), Failure>;
    /// The next bytes, cut anywhere.
    fn chunk(&mut self, bytes: &[u8]) -> Fed;
    /// The connection ended: the failure a non-200 answer's body stands for, if it was one.
    fn ended(&mut self) -> Option<Failure>;
    /// A catch-up beside the stream took the order up to `change`: nothing at or below it is handed over.
    fn caught_up_to(&mut self, change: u64);
}

/// The hub's routes without I/O. Stateless: everything a call needs is in its arguments.
pub trait HubCodec {
    /// What `answer` needs to know of the request to check its answer. The codec's own; the engine only keeps it.
    type Expect: Clone + std::fmt::Debug;

    /// The whole request of a route: method, absolute URL, headers (the bearer and `trommi-client` among them), body,
    /// timeout and class. `Err` only for a route that cannot be asked as given (more joins than one request takes,
    /// an outbox kind without its extra): [`Failure::BadAnswer`] with the reason.
    fn request(&self, site: &Site<'_>, route: &Route<'_>) -> Result<(HttpRequest, Self::Expect), Failure>;

    /// Reads an answer: [`crate::hub::classify`] first (so a status that is no 2xx is its failure), then the strict
    /// check of the body against what was asked. Never half an answer: `Err(Failure::BadAnswer)` instead.
    fn answer(&self, expect: &Self::Expect, response: &HttpResponse) -> Result<Answer, Failure>;

    /// The reader of one stream opened with `Route::Stream { after }`.
    type Stream: StreamReader;
    fn stream(&self, after: u64) -> Self::Stream;

    /// What a refusal says beside its code (`voided`, the epoch of `epoch-taken`). `None`: the answer is not the
    /// hub's refusal ([`Failure::Refused`] with `code: None`): never void or roll back on it.
    fn refusal(&self, response: &HttpResponse) -> Option<Refusal>;
}
