//! The sync engine: catch-up in the hub's order, the live stream, the outbox pump, retries, upkeep, the login
//! rebuild, recovery of a closed device, and when to wake. W1 builds it (start: Linux `engine/sync.rs`, the
//! specification of the login rebuild, iOS `Room.swift`, web `engine.ts`).
//!
//! The contract fixes the retry rule here, so every app waits the same way.

/// How long to wait before the next try of something that failed with `NotNow` or `Offline`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Backoff {
    pub first_ms: u64,
    pub max_ms: u64,
}

impl Backoff {
    /// The engine's rule for hub requests: 1 s, doubling, at most 60 s.
    pub const HUB: Backoff = Backoff {
        first_ms: 1_000,
        max_ms: 60_000,
    };

    /// The wait before try number `attempt + 1` (`attempt` failed tries so far, from 1): doubling from `first_ms`,
    /// capped at `max_ms`, then jittered down by up to a quarter with `jitter` (0..=u64::MAX, from the App's
    /// seeded generator), and never shorter than the hub's `retry_after_ms`.
    pub fn delay_ms(&self, attempt: u32, retry_after_ms: Option<u64>, jitter: u64) -> u64 {
        let exp = attempt.saturating_sub(1).min(32);
        let base = self.first_ms.saturating_mul(1u64 << exp).min(self.max_ms);
        let cut = (base / 4).saturating_mul(jitter >> 32) >> 32;
        (base - cut).max(retry_after_ms.unwrap_or(0))
    }
}

/// A small deterministic generator (SplitMix64) for jitter: seeded from `Config.seed`, so a torture run repeats.
#[derive(Clone, Debug)]
pub struct Jitter(u64);

impl Jitter {
    pub fn new(seed: u64) -> Self {
        Jitter(seed)
    }
    pub fn draw(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_caps_and_honours_retry_after() {
        let b = Backoff::HUB;
        assert_eq!(b.delay_ms(1, None, 0), 1_000);
        assert_eq!(b.delay_ms(3, None, 0), 4_000);
        assert_eq!(b.delay_ms(30, None, 0), 60_000);
        assert!(b.delay_ms(30, None, u64::MAX) >= 45_000);
        assert_eq!(b.delay_ms(1, Some(30_000), 0), 30_000);
        let (mut a, mut c) = (Jitter::new(7), Jitter::new(7));
        assert_eq!(a.draw(), c.draw());
    }
}
