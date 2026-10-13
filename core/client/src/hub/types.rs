//! The typed data that crosses between the codec and whoever asks (the engine): what is asked ([`Route`], at a
//! [`Site`]), what a checked answer holds ([`Answer`]), what the live stream brings ([`StreamEvent`]).
//!
//! Ids are the protocol core's types, so a value of the wrong length cannot be asked with and is never handed
//! on. Byte strings are `Vec<u8>`; `Debug` of anything that holds content, a key or a token shows its kind only.

use super::Failure;
use trommi_core::ids::{DeviceId, FileId, GroupId, InviteId, ObjectId, RoomId, SessionId};
use trommi_core::store::OutboxEntry;

/// Where and as whom a request goes. The codec adds nothing of its own: no token it remembers, no clock.
#[derive(Clone, Copy)]
pub struct Site<'a> {
    /// The hub's address without a trailing slash ("https://hub.trommi.com").
    pub url: &'a str,
    /// The bearer token the engine holds; none for the routes that take none (`Challenge`, `Token`).
    pub token: Option<&'a str>,
    /// `Config.platform` and `Config.version`, for the `trommi-client` header.
    pub client: &'a str,
}

impl std::fmt::Debug for Site<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Site({}, {})", self.url, self.client)
    }
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
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OutboxExtra {
    /// `RoomFounding`: the account made with the room (the body of `POST /v1/account`, JSON), and the found token.
    pub account: Option<Vec<u8>>,
    pub found_token: Option<String>,
    /// `RecoveryCommit`, `RecoveryFinish`: the recovery they belong to.
    pub recovery_id: Option<Vec<u8>>,
}

impl std::fmt::Debug for OutboxExtra {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // (an account's body holds login keys, the found token is a secret)
        f.write_str("OutboxExtra(<redacted>)")
    }
}

/// One request to the hub, by its meaning. Borrowed: an outbox entry may hold megabytes and is not copied to ask.
#[derive(Clone, Copy, Debug)]
pub enum Route<'a> {
    /// `GET /v1/rooms/{room}/challenge` (no token).
    Challenge { room: &'a RoomId },
    /// `POST /v1/rooms/{room}/tokens` (no token): the device's `HubAuth` and its signature.
    Token {
        room: &'a RoomId,
        auth: &'a [u8],
        signature: &'a [u8],
    },
    /// `GET /v1/changes?after=&limit=`: a page of the room's one order.
    Changes {
        after: u64,
        limit: u32,
        only: Only<'a>,
    },
    /// `GET /v1/stream?after=`: the request of an `OpenStream` effect. It has no answer to read: its events go
    /// through [`HubCodec::stream_event`].
    Stream { after: u64 },
    /// `GET /v1/welcomes?after=`.
    Welcomes { after: u64 },
    /// `GET /v1/groups/{group}/log?after=&limit=[&kind=commit]`.
    GroupLog {
        group: &'a GroupId,
        after: u64,
        limit: u32,
        commits_only: bool,
    },
    /// `GET /v1/groups/{group}/chains/{sender}?after=&limit=`.
    Chain {
        group: &'a GroupId,
        sender: &'a DeviceId,
        after: u64,
        limit: u32,
    },
    /// `GET /v1/desk?brief=1[&since=][&active=1]`.
    DeskBrief { since: Option<u64>, active: bool },
    /// `GET /v1/cards/{object}?after=&limit=` and its siblings: every envelope of one object.
    Object {
        route: ObjectRoute,
        object: &'a ObjectId,
        after: u64,
        limit: u32,
    },
    /// One outbox entry exactly as the protocol core made it, to the route of its kind.
    Outbox {
        room: &'a RoomId,
        entry: &'a OutboxEntry,
        extra: Option<&'a OutboxExtra>,
    },
    /// `POST /v1/commits`: joins of several session groups from outside in one request (1 to 64, one per group).
    Joins { entries: &'a [OutboxEntry] },
    /// `POST /v1/groups/read`: what joining several groups from outside needs (1 to 64).
    GroupsRead { groups: &'a [GroupId] },
    /// `PUT /v1/key-packages` with nothing to add: how many unused ones the hub holds.
    KeyPackagesHeld,
    /// `POST /v1/key-packages/claim`.
    ClaimKeyPackages {
        devices: &'a [DeviceId],
        partial: bool,
    },
    /// `GET /v1/rooms/{room}/groups?after=&limit=`.
    RoomGroups {
        room: &'a RoomId,
        after: u64,
        limit: u32,
    },
    /// `GET /v1/groups/{group}/info[?epoch=]`.
    GroupInfo {
        group: &'a GroupId,
        epoch: Option<u64>,
    },
    /// `POST /v1/requests`.
    Wish {
        kind: WishKind,
        group: Option<&'a GroupId>,
        key_package: Option<&'a [u8]>,
    },
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
    /// When the hub took it, by the hub's clock.
    pub at: u64,
    pub bytes: Vec<u8>,
    /// The device that posted it.
    pub sender: DeviceId,
    /// A Commit's `RecoveryAuth`, where the hub stores one beside it.
    pub recovery_auth: Option<Vec<u8>>,
}

/// An envelope as a read route serves it (pruned form when its body is gone).
#[derive(Clone, PartialEq, Eq)]
pub struct EnvelopeItem {
    pub change: u64,
    pub envelope: Vec<u8>,
    /// When the hub took it, by the hub's clock.
    pub received_at: u64,
    /// Beyond its sender's Cut: told on the chain route only, false everywhere else.
    pub cut: bool,
    /// The hub's void record, as spec section 16 names the code; a code this build does not know stays text.
    pub void_code: Option<String>,
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
    /// A Commit (`Commit`, `ExternalCommit`, `RecoveryCode`): the group's epoch after it, which is the epoch it
    /// was built on plus one (checked).
    pub epoch: Option<u64>,
    /// `Message`: its number in the group's log; none for a relayed one.
    pub n: Option<u64>,
    /// `RoomFounding`: the room the hub founded, which is the entry's own when it names one (checked).
    pub room: Option<RoomId>,
    /// `GroupFounding`: the session group the hub founded, which is the entry's own when it names one (checked).
    pub group: Option<GroupId>,
    /// `RecoveryCommit`: whether the part was kept (false: the same part came before).
    pub kept: Option<bool>,
    /// `RecoveryFinish`: what the finish published.
    pub finished: Option<Finished>,
}

/// What `POST …/recovery/{id}/finish` answers (8.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    pub published: bool,
    /// The first of the consecutive change numbers the parts were published under.
    pub first_change: u64,
    pub device: DeviceId,
}

/// One group of `GroupsRead`.
#[derive(Clone, PartialEq, Eq)]
pub struct GroupRead {
    pub group: GroupId,
    /// The current GroupInfo and its epoch.
    pub info: Vec<u8>,
    pub epoch: u64,
    /// The GroupInfo of epoch 0; `None` when the group is at epoch 0 (then `info` is it).
    pub founding: Option<Vec<u8>>,
    /// The group's Commits, oldest first: one per epoch from 0, at most up to `epoch` (checked).
    pub log: Vec<LogItem>,
    /// `log` reaches `epoch`. False: the Commits did not fit the first page; that group is read alone.
    pub complete: bool,
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
    /// A helper session: its main session. `None` for the room group and a main session.
    pub parent: Option<SessionId>,
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
    /// When the hub took it; told by `GET /v1/requests`, not by the stream.
    pub at: Option<u64>,
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
    Token {
        token: String,
        expires_at: u64,
        role: Role,
    },
    Changes(ChangesPage),
    Welcomes(Vec<WelcomeRow>),
    /// Commits and messages of one group, as `ChangeItem`s (never an envelope).
    GroupLog {
        items: Vec<ChangeItem>,
        more: bool,
    },
    Chain {
        items: Vec<ChainItem>,
        more: bool,
    },
    DeskBrief(DeskBrief),
    Object(ObjectEnvelopes),
    Outbox(OutboxAnswer),
    /// One per entry asked, in order.
    Joins(Vec<Result<OutboxAnswer, Failure>>),
    /// One per group asked, in order.
    GroupsRead(Vec<Result<GroupRead, Failure>>),
    KeyPackagesHeld {
        unused: u32,
    },
    Claimed {
        key_packages: Vec<(DeviceId, Vec<u8>)>,
        missing: Vec<DeviceId>,
    },
    RoomGroups {
        items: Vec<GroupRow>,
        more: bool,
        after: u64,
    },
    GroupInfo {
        epoch: u64,
        group_info: Vec<u8>,
    },
    Wished {
        id: u64,
    },
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
    Relay {
        group: GroupId,
        epoch: u64,
        sender: DeviceId,
        message: Vec<u8>,
    },
    /// A Welcome waits for this device.
    Welcome {
        group: GroupId,
    },
    Wish(WishRow),
    InviteRequest {
        invite: InviteId,
    },
    Presence(Presence),
    /// The end of the list of who is here that a human device's new stream is told first.
    Here,
    Archived {
        group: GroupId,
    },
    FileEvicted {
        file: FileId,
    },
}

/// A presence report (hub-api.md "Decided" 20, 31). Without `hears` it says nothing of the link: the report a
/// client holds stands while the device is online.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub device: DeviceId,
    pub online: bool,
    /// An agent's lease ran out and no stream of it is open.
    pub lost: bool,
    pub hears: Option<bool>,
    pub working: Option<bool>,
    pub last_call_at: Option<u64>,
}

/// `Debug` that names the type and what orders it, never the bytes it holds (content, a Welcome, a KeyPackage).
macro_rules! quiet_debug {
    ($type:ty, |$v:ident| $($arg:tt)+) => {
        impl std::fmt::Debug for $type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let $v = self;
                write!(f, $($arg)+)
            }
        }
    };
}

quiet_debug!(
    LogItem,
    |v| "LogItem(n {} at {}, {} bytes)",
    v.n,
    v.change,
    v.bytes.len()
);
quiet_debug!(
    EnvelopeItem,
    |v| "EnvelopeItem(at {}, {} bytes)",
    v.change,
    v.envelope.len()
);
quiet_debug!(ChainItem, |v| "ChainItem(seq {}, {:?})", v.seq, v.item);
quiet_debug!(
    ChangesPage,
    |v| "ChangesPage({:?}, change {}, more {}, head {:?})",
    v.items,
    v.change,
    v.more,
    v.head
);
quiet_debug!(
    WelcomeRow,
    |v| "WelcomeRow({}, {} bytes)",
    v.id,
    v.welcome.len()
);
quiet_debug!(
    ObjectEnvelopes,
    |v| "ObjectEnvelopes({:?}, more {})",
    v.items,
    v.more
);
quiet_debug!(
    GroupRead,
    |v| "GroupRead(epoch {}, {:?}, complete {})",
    v.epoch,
    v.log,
    v.complete
);

impl std::fmt::Debug for StreamEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamEvent::Change(item) => write!(f, "{item:?}"),
            StreamEvent::Relay { message, .. } => write!(f, "Relay({} bytes)", message.len()),
            StreamEvent::Welcome { .. } => f.write_str("Welcome"),
            StreamEvent::Wish(row) => write!(f, "Wish({:?}, {})", row.kind, row.id),
            StreamEvent::InviteRequest { .. } => f.write_str("InviteRequest"),
            StreamEvent::Presence(p) => write!(f, "{p:?}"),
            StreamEvent::Here => f.write_str("Here"),
            StreamEvent::Archived { .. } => f.write_str("Archived"),
            StreamEvent::FileEvicted { .. } => f.write_str("FileEvicted"),
        }
    }
}
