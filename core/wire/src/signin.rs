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
