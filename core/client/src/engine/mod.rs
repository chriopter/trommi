//! The sync engine: one device of one room against its hub, without I/O. Events in ([`Engine::on_http`],
//! [`Engine::on_stream`], [`Engine::on_store`], [`Engine::on_wake`]), effects out, reports for the App
//! ([`Report`]). The App holds it and feeds the model from its reports.
//!
//! # How it runs
//!
//! **Level-triggered.** After every event `drive` looks at the state and starts whatever is due and possible: a
//! sign-in, the next page, the stream, the outbox's head, a repair. Nothing is "queued to be sent": a want stays a
//! want until it is met, so a failure, a token that ended or a restart never loses one.
//!
//! **The device inside.** The protocol core's `Device` runs over [`memstore`]: what it writes leaves as one
//! `DeviceApply` per step, placed before every request of the same answer. That effect is a barrier in the shell, so
//! nothing a write implies is sent before the write is durable; if it fails the engine stops ([`Stopped::Store`]).
//!
//! **The position** is the engine's place in the hub's one order: every change at or below it was handed to the
//! device, or passed for good. It starts at the device's own stored cursor and moves only item by item, upwards. A
//! page (`GET /v1/changes` after the position) and the stream (opened after a change at or below the position) are
//! each gap-free from where they start, so an item at or below the position is one already taken.
//!
//! # What is right by construction
//!
//! - **A gap is always found and said.** An envelope whose chain misses what stands before it is never shown out
//!   of order. The sender's chain is fetched from what the device holds and the envelope tried again; if the hub
//!   does not serve the chain, `Alert::Withheld` is raised (in a catch-up and live alike), the envelope stays out,
//!   and the chain is asked for again whenever the order comes to rest.
//! - **The hub's hints cannot break the order.** `head` (where to open the stream beside a long catch-up) and a
//!   token's `expires_at` are hints. A stream that began beyond where the pages ended is closed and opened again
//!   from the position; its changes are taken only when it began at or below the position. A wrong hint costs a
//!   second stream, never an item.
//! - **Every buffer is bounded.** A page holds at most [`sync::PAGE`] items and is taken before the next is asked;
//!   the stream's changes that wait for a page are at most [`sync::LIVE_MAX`] ([`sync::LIVE_BYTES_MAX`] bytes),
//!   beyond which they are dropped and read by a page; the tables of what waits for a group, a key or a chain hold
//!   [`sync::BEHIND_MAX`] entries, beyond which one number says from where to read again; an answer is capped by
//!   its request's `max_answer`, a stream's lines and events by the codec.
//! - **A Commit is never passed over.** A log entry that does not process halts the pass with the position before
//!   it (13.4); only a duplicate is skipped. So there is no later reading that would have to "repair" one.
//! - **Repairs read what is missing, not everything.** An envelope for a group this device does not hold that far:
//!   the Welcomes once, the joined group's log up to the position, then that group's envelopes from the first one
//!   refused (`GET /v1/changes?group=`). Keys handed over: the envelopes chained without their body, per group,
//!   from the first of them. A withheld chain: that sender's chain from what the device holds. None of them reads
//!   the room's order again; [`Engine::reread_from`] is set only when one of those tables overflowed.
//!
//! # The Desk first
//!
//! Before the first page of the order the engine asks for the hub's Desk (`GET /v1/desk`): the open cards and
//! permission requests with their newest version. Each version is read by the protocol core out of the order
//! (shown as provisional until its chain reaches it), and for a card revised since it was asked the first version
//! is read as well (one small request each, [`sync::FIRSTS_AT_ONCE`] at a time), because the Desk orders cards by
//! when they were asked: no card moves when the order is read afterwards. Then [`Report::DeskComplete`]. The Desk
//! is read again only when a session group was joined. What the hub says there (`first_change`, the list itself)
//! is a hint for what to read: every envelope is checked like any other, and the list only ever HOLDS a card back
//! ([`crate::desk`]), it cannot make one.
//!
//! The hub cuts its Desk short (1000 objects of a kind, 20 000 registers, 8 MiB) and says `truncated`: such a list
//! decides nothing, what it brought is shown, and the rest comes in the hub's order. Nothing here relies on the
//! list being whole, on a version being the newest, or on the change numbers beyond their order. The Desk's
//! registers are read like its versions, out of the order; the core does not say of such a register whether it is
//! current, so the model takes a session's or a device's register only once the order reaches it (a finding, open).
//!
//! [`feed::Board`] turns the reports into the board model (`crate::model`), which the views are made from.
//!
//! # Joining the room with the code
//!
//! A slot without a device in a room joins with the recovery code ([`sync::room_join`], [`Engine::join_room`]): signed
//! in with the code's key it reads the room's sealed keys, the room group's GroupInfos, its live sessions and its
//! Commits (no content), lets the protocol core check the room from its founding, and posts the join. Until it is
//! posted a failure leaves nothing stored; once posted, the device's outbox carries it across a restart
//! ([`OpenError::Interrupted`]), and the hub's answer or the room's log decides it. Then the sessions follow.
//!
//! # A sign-in's sessions
//!
//! A device that signed in with the code joins its session groups from outside ([`sync::joins`]): what the joins
//! need is read for many groups in one request, and the joins are posted together, each answered in its place.
//!
//! # Which order the model is handed
//!
//! The engine hands the model each envelope as the protocol core took it. What is authenticated in that:
//! each sender's own order (its chain: number, hash of the one before, signature), and for an envelope that
//! changed an object the object's state after the core's replay of its versions (`object_after`: owner, state,
//! the current version's hash), which follows the versions' own hash links and not the hub. A register's
//! `current` is the core's as well. What is NOT authenticated: the order of envelopes of different senders
//! where nothing links them (the lines of a Chat, an answer against a version it does not name). There the only
//! order is the hub's change number, and the model must not let it decide a card's state where `object_after`
//! says otherwise.
//!
//! # Upkeep
//!
//! The timed round (once the order is at rest, then every [`sync::UPKEEP_EVERY_MS`]): KeyPackages stocked, `heads`
//! written, missing keys asked for. Tending the groups ([`sync::upkeep`]): a stale session cleaned (5.2.8), a human
//! device a live session lacks added (5.2.7), the own leaf renewed (5.2.9), the steps of this device's invites
//! taken, an agent without a session seated ([`crate::seats`]), the core's findings said. An envelope
//! the hub voids as `epoch-full` leads to an update of the own leaf at once, which moves the group on.
//!
//! # Across a restart
//!
//! The engine keeps one small record in the app's store ([`sync::record`]): its position, what it passed over and
//! still owes (groups it waited for, missing keys, withheld chains, queued mends), and a sign-in's session groups
//! still to join. It is written behind the device's own write of the same step, so it never runs ahead of the
//! stored device; one that lags only costs a second reading, and one of another stored state is left.
//!
//! # Not here yet
//!
//! The Desk as a delta on a warm start, the reading of the room's past (one
//! module, one entry point, behind [`Engine::reread_from`]), wishes to be let in again
//! (`readmit`) and a leaf's refusal of a Commit (`reject`).
//!
//! # Beside the order
//!
//! The stream's events that have no change number ([`sync::events`]): relayed pieces, wishes, invite Requests,
//! archived sessions, evicted files.
//!
//! The contract fixes the retry rule here, so every app waits the same way.

pub mod feed;
pub mod memstore;
pub mod sync;

pub use sync::{requests, room_join, Alert, Connection, Ctx, Engine, OpenError, Report, Stopped};

/// The device inside the engine: the protocol core's, over the store whose writes become barrier effects.
pub type Device = trommi_core::device::Device<memstore::MemStore>;

/// The hub codec the engine runs on: the routes built and their answers read strictly ([`crate::hub`]).
pub type Codec = crate::hub::Codec;

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
