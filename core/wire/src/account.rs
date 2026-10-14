//! The account (spec/v1.md section 16): the pinned key derivation record and the lengths the hub checks.
//!
//! The derivation itself and the sealing of the copies are the protocol core's (`core/protocol/src/account.rs`);
//! the hub only checks that what a client stores is in these forms.

/// The key derivation record is pinned: Argon2id, record version 1, 64 MiB, 3 passes, 1 lane.
pub const KDF_ALG: &str = "argon2id";
pub const KDF_RECORD_VERSION: u32 = 1;
pub const KDF_MEMORY_KIB: u32 = 65536;
pub const KDF_PASSES: u32 = 3;
pub const KDF_LANES: u32 = 1;

/// A sealed copy of a way in: its version byte, a nonce of 12, a key of 32, a tag of 16.
pub const SEALED_COPY_LEN: usize = 1 + 12 + 32 + 16;
/// The version byte a sealed copy begins with.
pub const SEALED_COPY_VERSION: u8 = 2;
/// The longest passkey credential id.
pub const MAX_CREDENTIAL_ID_LEN: usize = 1023;
/// The longest e-mail address.
pub const MAX_EMAIL_LEN: usize = 254;
