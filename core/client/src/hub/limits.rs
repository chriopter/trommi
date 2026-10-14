//! This client's own limits on what a hub may answer: sizes, counts and how far a cursor may run. They follow
//! spec/v1.md section 16 and spec/hub-api.md, with room for the JSON around the bytes, and they hold whatever a
//! hub does: an answer beyond one of them is refused whole.
//!
//! The pages, windows and counts the hub's routes keep to are the hub's and this client's at once: they come from
//! core/wire, which the hub reads them from too.

/// A sealed copy of the recovery code; a passkey's credential id; an e-mail address, and the one field that names
/// an account.
pub use trommi_wire::account::{
    MAX_CREDENTIAL_ID_LEN as MAX_CREDENTIAL_ID, MAX_EMAIL_LEN as MAX_EMAIL,
    SEALED_COPY_LEN as SEALED_COPY,
};
use trommi_wire::limits::ANSWER_BYTES;
/// Most devices one claim of KeyPackages names: the leaves of a group's tree.
pub use trommi_wire::limits::MAX_LEAVES as MAX_CLAIM;
pub use trommi_wire::limits::{
    CHANGES_WINDOW, LOG_PAGE, MAX_AT_ONCE, MAX_CHANGES, MAX_CHAT_PAGE, MAX_FRONTIER_FILES,
    MAX_FRONTIER_WRITERS, MAX_GROUPS_PAGE, MAX_LOG, MAX_PAGE, MAX_REMOVAL, MAX_SEALED_KEYS,
};

const MIB: usize = 1 << 20;

/// Most the answer to a write may hold: a few numbers and ids, or a refusal.
pub const CAP_WRITE: usize = 64 << 10;
/// Most a list may hold: the hub's 8 MiB of envelopes (hub-api.md "Decided" 22), the one item that may exceed
/// it, and the JSON around them.
pub const CAP_LIST: usize = 16 * MIB;
/// Most the Desk may hold: the same budget, the registers and the room's groups.
pub const CAP_DESK: usize = 32 * MIB;
/// Most one event of the stream may hold, from its first line to the empty line that ends it.
pub const CAP_EVENT: usize = 4 * MIB;

/// An envelope: a padded body of 64 KiB, a header with 255 file ids, a signature.
pub const MAX_ENVELOPE: usize = 128 << 10;
/// An MLS message, a GroupInfo or a Welcome: each within the 1 MiB of a Commit.
pub const MAX_MLS: usize = MIB;
/// A KeyPackage.
pub const MAX_KEY_PACKAGE: usize = 16 << 10;
/// The small structs: a SealedKey, a RecoveryLink, a RecoveryAuth.
pub const MAX_SMALL_STRUCT: usize = 1024;

/// Most rows the Desk's open list holds (cards, permission requests and settled ones together).
pub const MAX_OPEN: usize = 10_000;
/// Most single-use KeyPackages one upload holds (hub-api.md "Decided" 41).
pub const MAX_KEY_PACKAGES: usize = 100;
/// Most one GroupInfo answer may hold: a GroupInfo of 1 MiB in base64url and the JSON around it.
pub const CAP_GROUP_INFO: usize = 2 * MIB;
/// A signature or a MAC: 64 and 32 bytes in v1.md; their exact form is the protocol core's to check.
pub const MAX_TAG: usize = 128;
/// Most Welcomes one answer holds (the hub's 8 MiB of them).
pub const MAX_WELCOMES: usize = 10_000;
/// Most leaves a group's row lists (the tree's 2 047 nodes hold 1 024; with room).
pub const MAX_LEAVES: usize = 4096;
/// Most wishes `GET /v1/requests` lists.
pub const MAX_WISHES: usize = 100_000;
/// Most open objects of one kind the Desk holds (the hub cuts it short there).
pub const MAX_DESK_OBJECTS: usize = 1000;
/// Most register values the Desk holds.
pub const MAX_DESK_REGISTERS: usize = 20_000;
/// Most groups a hub lists with the Desk.
pub const MAX_DESK_GROUPS: usize = 10_000;
/// Most stored bytes of a file (spec/v1.md 16: 64 MiB of plaintext).
pub const CAP_FILE: usize = 67_125_269;
/// How long the shell waits for a file to go up or come down.
pub const TRANSFER_TIMEOUT_MS: u32 = 600_000;
/// Most the answer of `POST /v1/groups/read` may hold. The hub keeps one budget of 8 MiB for the whole answer and
/// answers every group that does not fit `too-large` in its place; only the first group always goes in, and alone
/// it may exceed the budget: its GroupInfo and founding GroupInfo, and a log page whose Commits stop at 8 MiB of
/// base64 (checked below, with this client's own limits on each part).
pub const CAP_GROUPS_READ: usize = 12 * MIB;

/// Base64url of `n` bytes.
const fn b64(n: usize) -> usize {
    (n * 4).div_ceil(3)
}

/// The first group of a groups/read alone, at the largest every part of it may be, and a refusal row in the place
/// of each other group: under [`CAP_GROUPS_READ`]. (A log entry's other members: its numbers, the group id, the
/// sender and the names, under 400 bytes, and its RecoveryAuth.)
const _: () = assert!(
    2 * b64(MAX_MLS)
        + ANSWER_BYTES
        + LOG_PAGE * (400 + b64(MAX_SMALL_STRUCT))
        + MAX_AT_ONCE * 512
        + 4096
        <= CAP_GROUPS_READ
);
/// Most a small answer that is more than a few numbers may hold (an account, a login, an invite).
pub const CAP_SMALL: usize = MIB;
/// Most rooms an account's answers list, and most passkeys.
pub const MAX_ROOMS: usize = 16;
pub const MAX_PASSKEYS: usize = 64;
/// Most push registrations `GET /v1/push` lists, and the longest endpoint.
pub const MAX_PUSH: usize = 64;
pub const MAX_ENDPOINT: usize = 2048;
