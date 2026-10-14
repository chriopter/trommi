//! Recovery (spec/v1.md section 8): the labels the hub checks a RecoveryAuth and a SealedKey's GroupInfo with.

/// The label of the signature in a `RecoveryAuth`.
pub const JOIN_LABEL: &str = "TrommiRecoveryJoin";
/// The label of the hash that names a GroupInfo.
pub const GROUP_INFO_LABEL: &str = "Trommi Group Info";
/// The label of the hash that names a Commit.
pub const COMMIT_LABEL: &str = "Trommi Commit";
