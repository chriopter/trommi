//! The limits of the hub's routes that a client asks within (spec/hub-api.md): pages, windows and counts. The hub
//! clamps or refuses beyond them; the client never asks for more and refuses an answer that holds more.
//!
//! What a client allows itself beyond these (room for the JSON around the bytes, its own caps on whole answers)
//! is the client's own and stays in `core/client`.

/// Most items one page of a catch-up (`GET /v1/changes`) holds.
pub const MAX_CHANGES: u32 = 1000;
/// hub-api.md "Decided" 32: one catch-up answer looks at most so many change numbers ahead.
pub const CHANGES_WINDOW: u64 = 20_000;
/// Most entries one page of a group's log (`GET /v1/groups/{id}/log`) holds.
pub const MAX_LOG: u32 = 1000;
/// The page of Commits `POST /v1/groups/read` gives per group.
pub const LOG_PAGE: usize = 256;
/// Most envelopes one page of a chain or an object holds.
pub const MAX_PAGE: u32 = 2000;
/// Most envelopes one page of a Chat holds.
pub const MAX_CHAT_PAGE: u32 = 200;
/// Most rows one page of the room's groups holds.
pub const MAX_GROUPS_PAGE: u32 = 1000;
/// Most rows one page of sealed keys holds.
pub const MAX_SEALED_KEYS: u32 = 2000;
/// Most Commits one `POST /v1/commits` and most groups one `POST /v1/groups/read` take.
pub const MAX_AT_ONCE: usize = 64;
/// Most leaves of a group's ratchet tree (2 047 nodes), and so most devices one claim of KeyPackages names.
pub const MAX_LEAVES: usize = 1024;
/// Most Commits one page of a removal holds.
pub const MAX_REMOVAL: usize = 200;
/// Most writers and files one board frontier names (hub-api.md "Decided" 45).
pub const MAX_FRONTIER_WRITERS: usize = 1000;
pub const MAX_FRONTIER_FILES: usize = 20_000;
/// hub-api.md "Decided" 22: the budget of one answer that lists envelopes, Commits or Welcomes; only its first
/// item may go beyond it.
pub const ANSWER_BYTES: usize = 8 << 20;
