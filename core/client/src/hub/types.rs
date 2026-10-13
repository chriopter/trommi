//! The typed data that crosses between the codec and whoever asks (the engine): what is asked ([`Route`], at a
//! [`Site`]), what a checked answer holds ([`Answer`]), what the live stream brings ([`StreamEvent`]).
//!
//! Ids are the protocol core's types, so a value of the wrong length cannot be asked with and is never handed
//! on. Byte strings are `Vec<u8>`; `Debug` of anything that holds content, a key or a token shows its kind only.

use super::Failure;
use trommi_core::ids::{
    BoardId, DeviceId, FileId, GroupId, Hash32, InviteId, ObjectId, RoomId, SessionId, ShareId,
};
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
    /// `DELETE /v1/token`: ends the token the site carries, at once; the hub cuts the device's streams.
    SignOut,
    /// `GET /v1/desk[?active=1|?archived=1]`: the open objects with their current version, every writer's
    /// newest value per register.
    Desk { only: DeskOnly },
    /// `GET /v1/chats/{session|card}/{id}/items?before=&limit=`: a Chat's envelopes, newest first.
    Chat {
        timeline: Timeline<'a>,
        before: Option<u64>,
        limit: u32,
    },
    /// `GET /v1/boards/{board}?after_change=&limit=`: the board's items after that change, oldest first.
    Board {
        board: &'a BoardId,
        after: u64,
        limit: u32,
    },
    /// `POST /v1/boards/{board}/frontier` (10.9): what this device's snapshot covers, per writer the chain
    /// number and hash, and the files its shapes still name; `snapshot`: the register value the hub took.
    BoardFrontier {
        board: &'a BoardId,
        frontier: &'a [(DeviceId, u64, Hash32)],
        files: &'a [FileId],
        snapshot: Option<(u64, Hash32)>,
    },
    /// `GET /v1/sealed-keys?after=&limit=`: the SealedKeys and RecoveryLinks above a change.
    SealedKeys { after: u64, limit: u32 },
    /// `GET /v1/groups/{group}/removal?after=`: for a key the hub took out of the room, the Commits up to the
    /// one that removed it (hub-api.md 42).
    Removal { group: &'a GroupId, after: u64 },
    /// `POST /v1/rooms/{room}/recovery` (8.7): locks the room for ten minutes; the recovery key's token.
    OpenRecovery { room: &'a RoomId },
    /// `DELETE /v1/rooms/{room}/recovery/{id}`: gives the room back without publishing.
    DropRecovery {
        room: &'a RoomId,
        recovery: &'a [u8; 16],
    },
    /// `PUT /v1/files/{file}`: the stored (encrypted) bytes.
    PutFile { file: &'a FileId, bytes: &'a [u8] },
    /// `GET /v1/files/{file}`, whole or one inclusive range of bytes.
    GetFile {
        file: &'a FileId,
        range: Option<ByteRange>,
    },
    /// `DELETE /v1/files/{file}`.
    DeleteFile { file: &'a FileId },
    /// `POST /v1/shares` (11.5): registers a Share link for a file.
    PostShare {
        share: &'a ShareId,
        secret_hash: &'a [u8; 32],
        file: &'a FileId,
        expires_at: u64,
    },
    /// `DELETE /v1/shares/{share}`: revokes the link.
    DeleteShare { share: &'a ShareId },
    /// `GET /v1/shares/{share}`: the shared file's bytes for whoever holds the link's secret; no token. The
    /// secret travels in a header, never in the address.
    GetShared {
        share: &'a ShareId,
        secret: &'a [u8; 32],
        range: Option<ByteRange>,
    },
}

/// Which sessions the Desk covers (hub-api.md point 46).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeskOnly {
    #[default]
    All,
    /// The live sessions: what a start loads.
    Active,
    /// The archived ones alone: when one is opened.
    Archived,
}

/// A Chat's timeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Timeline<'a> {
    Session(&'a SessionId),
    Card(&'a ObjectId),
}

/// An inclusive range of a file's bytes; `last: None`: to the file's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub first: u64,
    pub last: Option<u64>,
}

/// An open object of the Desk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeskObject {
    pub object: ObjectId,
    pub group: GroupId,
    /// 1 open, 2 answered, 3 closed (v1.md 9).
    pub state: u8,
    /// The knock bit (and the level in headers of before 11 October 2026).
    pub urgency: u8,
    pub answered_at: u64,
    pub owner: DeviceId,
    pub first_change: u64,
    pub head_change: u64,
    /// The envelope of its current version; none when the Desk was cut short before it.
    pub version: Option<EnvelopeItem>,
}

/// `GET /v1/desk`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Desk {
    pub cards: Vec<DeskObject>,
    pub notes: Vec<DeskObject>,
    pub permission_requests: Vec<DeskObject>,
    pub artifacts: Vec<DeskObject>,
    /// Every writer's newest value per register.
    pub registers: Vec<EnvelopeItem>,
    /// The room's groups, from a hub that still sends them with the Desk.
    pub groups: Option<Vec<GroupRow>>,
    /// The Desk was cut short: the rest comes by the catch-up.
    pub truncated: bool,
    pub change: u64,
}

/// `GET /v1/sealed-keys`: both lists ascend by change number and are complete up to `change`, the cursor for
/// the next call.
#[derive(Clone, PartialEq, Eq)]
pub struct SealedKeysPage {
    /// `(change, SealedKey)`.
    pub rows: Vec<(u64, Vec<u8>)>,
    pub links: Vec<RecoveryLinkRow>,
    pub change: u64,
    pub more: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RecoveryLinkRow {
    pub room_epoch: u64,
    pub change: u64,
    pub recovery_link: Vec<u8>,
}

/// A file's bytes as one answer brought them.
#[derive(Clone, PartialEq, Eq)]
pub struct FileBytes {
    pub bytes: Vec<u8>,
    /// Where `bytes` begin in the file.
    pub first: u64,
    /// The whole file's size: the answer's own length, or what a range's answer says the file has.
    pub size: u64,
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
    /// A route whose answer carries nothing (`RejectCommit`, `SignOut`, a deletion).
    Done,
    Desk(Desk),
    /// A page of a Chat (newest first) or of a board's items (oldest first).
    Envelopes {
        items: Vec<EnvelopeItem>,
        more: bool,
    },
    /// `BoardFrontier`: how many item bodies the hub pruned now.
    Pruned {
        pruned: u64,
    },
    SealedKeys(SealedKeysPage),
    /// `Removal`: the Commits up to the removing one, whose log number is `removed_at`.
    Removal {
        items: Vec<LogItem>,
        more: bool,
        removed_at: u64,
    },
    /// `OpenRecovery`.
    Recovery {
        id: [u8; 16],
        expires_at: u64,
    },
    /// `DropRecovery`.
    Dropped {
        dropped: bool,
    },
    /// `PutFile`: the size and hash the hub stored (the caller compares the hash with its own).
    FileStored {
        size: u64,
        sha256: [u8; 32],
    },
    /// `GetFile`, `GetShared`.
    File(FileBytes),
    /// `PostShare`: when the hub ends the link.
    Shared {
        expires_at: u64,
    },
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
            Answer::Desk(_) => "Desk",
            Answer::Envelopes { .. } => "Envelopes",
            Answer::Pruned { .. } => "Pruned",
            Answer::SealedKeys(_) => "SealedKeys",
            Answer::Removal { .. } => "Removal",
            Answer::Recovery { .. } => "Recovery",
            Answer::Dropped { .. } => "Dropped",
            Answer::FileStored { .. } => "FileStored",
            Answer::File(_) => "File",
            Answer::Shared { .. } => "Shared",
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
