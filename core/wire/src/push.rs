//! Push (spec/v1.md section 15): the sealed payload's fixed length and associated data.

/// The longest sealed APNs payload: every sealed payload is this long.
pub const MAX_APNS_LEN: usize = 512;
/// The length every sealed APNs plaintext is padded to: the payload less a nonce of 12 and a tag of 16.
pub const APNS_PLAIN_LEN: usize = MAX_APNS_LEN - 12 - 16;
/// The associated data of a sealed APNs payload.
pub const APNS_AAD: &[u8] = b"trommi apns v1";
/// The associated data of a Live Activity's sealed counts.
pub const LIVE_AAD: &[u8] = b"trommi live v1";
