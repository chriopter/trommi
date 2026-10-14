//! The invite (spec/v1.md section 12.2): its lifetimes and the labels of its signatures and hashes.

/// How long a human device's invite lives.
pub const HUMAN_INVITE_LIFE_MS: u64 = 10 * 60 * 1000;
/// How long an agent device's invite lives.
pub const AGENT_INVITE_LIFE_MS: u64 = 15 * 60 * 1000;
/// How far a clock may be off when an invite's deadline is checked.
pub const CLOCK_TOLERANCE_MS: u64 = 2 * 60 * 1000;
/// The length of an Offer's MAC.
pub const MAC_LEN: usize = 32;

/// The labels of the Offer's, the InviteRequest's and the Reveal's signatures.
pub const SIGN_OFFER: &str = "TrommiInviteOffer";
pub const SIGN_REQUEST: &str = "TrommiInviteRequest";
pub const SIGN_REVEAL: &str = "TrommiInviteReveal";
/// The labels of the hashes that name an Offer and an InviteRequest, and of the commitment to the Reveal.
pub const LABEL_OFFER_HASH: &str = "Trommi Invite Offer";
pub const LABEL_REQUEST_HASH: &str = "Trommi Invite Request";
pub const LABEL_COMMITMENT: &str = "Trommi Invite Commitment";
