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

/// An e-mail address as the account keeps it (spec/v1.md 8.9.1), the one rule the hub and the app core both check;
/// `None` is `bad-email`. Spelled out over single characters, with no Unicode table:
///
/// 1. Leading and trailing space, tab, line feed, vertical tab, form feed and carriage return are dropped.
/// 2. Every character left is printable ASCII, U+0021 to U+007E. Anything else is refused, never mapped.
/// 3. `A` to `Z` become `a` to `z`.
/// 4. At most [`MAX_EMAIL_LEN`] characters; exactly one `@`; 1 to 64 characters before it; after it, the LAST `.`
///    has 1 to 190 characters before it and 2 to 63 after.
pub fn normalise_email(input: &str) -> Option<String> {
    let trimmed = input.trim_matches(|c: char| c == ' ' || ('\u{9}'..='\u{d}').contains(&c));
    if !trimmed.bytes().all(|b| (0x21..=0x7e).contains(&b)) {
        return None;
    }
    let email = trimmed.to_ascii_lowercase();
    let mut parts = email.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return None;
    };
    let (before, after) = domain.rsplit_once('.')?;
    let fits = email.len() <= MAX_EMAIL_LEN
        && (1..=64).contains(&local.len())
        && (1..=190).contains(&before.len())
        && (2..=63).contains(&after.len());
    fits.then_some(email)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e_mail_the_last_dot_of_the_domain_counts() {
        assert_eq!(
            normalise_email("  Ada@Example.ORG\t").as_deref(),
            Some("ada@example.org")
        );
        assert_eq!(
            normalise_email("a@example..com").as_deref(),
            Some("a@example..com")
        );
        let long = format!("x@{}.com", "a".repeat(190));
        assert_eq!(normalise_email(&long).as_deref(), Some(long.as_str()));
        for bad in [
            "a@b.example.c",
            "a@example.com.",
            "a@b",
            "a@@b.co",
            "@example.org",
            "ädä@example.org",
            &format!("x@{}.com", "a".repeat(191)),
            &format!("{}@example.org", "x".repeat(65)),
            &format!("{}@{}.org", "x".repeat(64), "a".repeat(186)),
        ] {
            assert_eq!(normalise_email(bad), None, "{bad}");
        }
    }
}
