//! Refusals: the stable codes of spec/v1.md section 16 and the status each comes with (hub-api.md "Decided" 28).
//!
//! The hub answers a refusal as `{ "error": code, "message": text, … }` under its code's status; a client counts a
//! body as the hub's refusal only when it names one of these codes under that same status.

/// The codes a hub refuses with and the status each comes with.
pub const CODES: &[(u16, &[&str])] = &[
    (
        400,
        &[
            "bad-format",
            "newer-version",
            "bad-commit",
            "bad-signature",
            "bad-invite",
            "bad-key-package",
            "wrong-room",
            "incomplete",
            "chain-break",
            "bad-email",
            "bad-passkey",
        ],
    ),
    (
        401,
        &[
            "unauthorised",
            "bad-challenge",
            "wrong-login",
            "wrong-recovery",
        ],
    ),
    (
        403,
        &["forbidden", "not-member", "removed-sender", "wrong-sender"],
    ),
    (404, &["not-found", "no-room"]),
    // (no route of the hub answers it today: an unknown method is `not-found`)
    (405, &["method-not-allowed"]),
    (
        409,
        &[
            "epoch-taken",
            "wrong-epoch",
            "room-behind",
            "group-behind",
            "stale-session",
            "epoch-full",
            "replay",
            "gap",
            "equivocation",
            "room-exists",
            "invite-used",
            "lease-lost",
            "account-exists",
            "last-way-in",
            "account-changed",
        ],
    ),
    (410, &["gone", "invite-expired", "invite-burned"]),
    (413, &["too-large", "quota-exceeded"]),
    (416, &["range"]),
    (426, &["client-too-old"]),
    (429, &["too-many", "rate-limited"]),
    (500, &["internal"]),
    (503, &["overloaded"]),
];

/// The status a code comes with; `None` for a text that is no code of this protocol.
pub fn status_of(code: &str) -> Option<u16> {
    CODES
        .iter()
        .find(|(_, codes)| codes.contains(&code))
        .map(|(status, _)| *status)
}

/// The code as the protocol's own text, when `code` is one.
pub fn known(code: &str) -> Option<&'static str> {
    CODES
        .iter()
        .flat_map(|(_, codes)| codes.iter())
        .find(|c| **c == code)
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_code_has_one_status() {
        let mut seen = std::collections::HashSet::new();
        for (_, codes) in CODES {
            for code in *codes {
                assert!(seen.insert(*code), "{code} twice");
            }
        }
        assert_eq!(status_of("overloaded"), Some(503));
        assert_eq!(status_of("internal"), Some(500));
        assert_eq!(status_of("nope"), None);
        assert_eq!(known("gap"), Some("gap"));
        assert_eq!(known("nope"), None);
    }
}
