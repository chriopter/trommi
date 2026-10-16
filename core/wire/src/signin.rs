//! Signing in at a hub (spec/v1.md section 12.1): the challenge, the token and the label of `HubAuth`'s
//! signature.

/// The length of a challenge.
pub const CHALLENGE_LEN: usize = 32;
/// How long a challenge may be answered.
pub const CHALLENGE_LIFE_MS: u64 = 2 * 60 * 1000;
/// How long a token lives.
pub const TOKEN_LIFE_MS: u64 = 10 * 60 * 1000;
/// The label of `HubAuth`'s signature.
pub const LABEL_HUB_AUTH: &str = "TrommiHubAuth";

/// A device's irrevocable request to retire its own key from one room. It is not a membership Commit.
pub const LABEL_DEVICE_DEPARTURE: &str = "TrommiDeviceDeparture";

/// Fixed-width canonical payload; the signature label separates it from every other signed operation.
pub fn departure_content(room: &[u8; 32], device: &[u8; 32]) -> [u8; 64] {
    let mut content = [0; 64];
    content[..32].copy_from_slice(room);
    content[32..].copy_from_slice(device);
    content
}
