//! This client's own limits on what a hub may answer: sizes, counts and how far a cursor may run. They follow
//! spec/v1.md section 16 and spec/hub-api.md, with room for the JSON around the bytes, and they hold whatever a
//! hub does: an answer beyond one of them is refused whole.

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

/// hub-api.md "Decided" 32: one catch-up answer looks at most so many change numbers ahead.
pub const CHANGES_WINDOW: u64 = 20_000;
/// Most items one page of a catch-up may be asked for.
pub const MAX_CHANGES: u32 = 1000;
/// Most rows the Desk's open list holds (cards, permission requests and settled ones together).
pub const MAX_OPEN: usize = 10_000;
/// Most Commits one `POST /v1/commits` and most groups one `POST /v1/groups/read` take.
pub const MAX_AT_ONCE: usize = 64;
/// Most single-use KeyPackages one upload holds (hub-api.md "Decided" 41).
pub const MAX_KEY_PACKAGES: usize = 100;
