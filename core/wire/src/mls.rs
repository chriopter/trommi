//! Trommi on MLS (spec/v1.md sections 3 to 5): the group context extensions, the note every Commit carries and
//! the room's limits that the hub checks on a Commit as the devices do.

/// The group context extension `TrommiRoom` (3.1), in the room group.
pub const EXT_ROOM: u16 = 0xF001;
/// The group context extension `TrommiSession` (3.2), in a session group.
pub const EXT_SESSION: u16 = 0xF002;
/// The version of the note in every Commit's `authenticated_data` (3.3).
pub const NOTE_VERSION: u8 = 2;
/// The length of one cut of a note: device, seq, hash.
pub const CUT_LEN: usize = 32 + 8 + 32;
/// `room_state = RefHash(ROOM_STATE_LABEL, GroupContext of the room group)`.
pub const ROOM_STATE_LABEL: &str = "Trommi Room State";
/// Most helper devices one session group holds (4.3).
pub const MAX_HELPER_DEVICES: usize = 7;

/// Most human devices outside recovery (section 16).
pub const MAX_HUMAN_DEVICES: usize = 1000;
