//! What this device sent and the hub has not answered yet: each outbox entry with the echoes it carries and the
//! action that made it, so a refusal rolls the echo back with a toast that offers the same action again.
//!
//! An entry leaves this list three ways: the hub took it (`Report::Accepted`: the hub's copy replaces the echo when
//! it comes back), the hub refused it (`Report::Refused`: rolled back, a toast), or it is no longer in the device's
//! outbox although neither was said (the write that held it failed and the stored state was opened again without
//! it: rolled back, a toast). Nothing here is stored: after a restart the outbox sends what it holds, once, and the
//! hub's copy shows it.

use super::Action;
use crate::views::{Text, Toast};
use std::collections::{BTreeMap, BTreeSet};

/// At most so many toasts stand at once; the oldest goes first.
pub const TOASTS_MAX: usize = 5;

/// One echo of an outbox entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Out {
    /// The model's local id of the echo (`Model::rollback_echo`); none for a write without an echo.
    pub local_id: Option<String>,
    /// What the person did, offered again by the toast of a refusal.
    pub action: Action,
}

/// The bookkeeping of the actions in flight.
#[derive(Debug, Default)]
pub struct Sent {
    /// Seeds the ids this device makes (a desk's): `Config.seed`, random in a shipped app.
    pub seed: u64,
    next_local: u64,
    next_toast: u64,
    /// By outbox entry id.
    outbox: BTreeMap<u64, Vec<Out>>,
}

impl Sent {
    /// A new id of 16 bytes as hex (a desk's), from the seed, the clock and a count: never the same twice in an App,
    /// and two devices' ids differ by their seeds.
    pub fn new_id(&mut self, now: u64) -> String {
        self.next_local += 1;
        let mut x = self.seed ^ now.rotate_left(17) ^ self.next_local.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut out = String::with_capacity(32);
        for _ in 0..2 {
            // splitmix64
            x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = x;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            out.push_str(&format!("{:016x}", z ^ (z >> 31)));
        }
        out
    }

    /// A local id for an echo: counted up per App, never reused by it.
    pub fn local_id(&mut self) -> String {
        self.next_local += 1;
        format!("local-{}", self.next_local)
    }

    /// The entry `outbox_id` carries this echo.
    pub fn put(&mut self, outbox_id: u64, out: Out) {
        self.outbox.entry(outbox_id).or_default().push(out);
    }

    pub fn is_empty(&self) -> bool {
        self.outbox.is_empty()
    }

    /// The entries waiting for the hub's answer.
    pub fn entries(&self) -> impl Iterator<Item = u64> + '_ {
        self.outbox.keys().copied()
    }

    /// Whether an echo of this local id waits for the hub.
    pub fn waits(&self, local_id: &str) -> bool {
        self.outbox.values().flatten().any(|o| o.local_id.as_deref() == Some(local_id))
    }

    /// The hub took the entry, or it is gone: what it carried.
    pub fn take(&mut self, outbox_id: u64) -> Vec<Out> {
        self.outbox.remove(&outbox_id).unwrap_or_default()
    }

    /// The entries that are no longer in the device's outbox (`held`): what each carried.
    pub fn lost(&mut self, held: &BTreeSet<u64>) -> Vec<Out> {
        let gone: Vec<u64> = self.outbox.keys().filter(|id| !held.contains(id)).copied().collect();
        gone.into_iter().flat_map(|id| self.take(id)).collect()
    }

    /// A toast in `toasts`, the oldest going when there are too many. `again`: offered as its action.
    pub fn toast(&mut self, toasts: &mut Vec<Toast>, text: Text, again: Option<Action>) {
        self.next_toast += 1;
        toasts.push(Toast {
            id: format!("toast-{}", self.next_toast),
            text,
            // (every toast of the actions says a refusal or a loss: a problem, and its action is the same again)
            label: again.as_ref().map(|_| Text::new("action.retry", "Retry")),
            action: again,
            line: None,
            alert: true,
            ms: None,
            go: None,
        });
        if toasts.len() > TOASTS_MAX {
            toasts.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn star(id: &str) -> Action {
        Action::Star { card_id: id.into(), on: true }
    }

    #[test]
    fn an_entry_is_taken_once_and_a_lost_one_is_found() {
        let mut s = Sent::default();
        let (a, b) = (s.local_id(), s.local_id());
        assert_ne!(a, b);
        s.put(4, Out { local_id: Some(a.clone()), action: star("x") });
        s.put(5, Out { local_id: Some(b.clone()), action: star("y") });
        assert!(s.waits(&a));
        assert_eq!(s.take(4).len(), 1);
        assert!(s.take(4).is_empty() && !s.waits(&a));
        assert_eq!(s.lost(&BTreeSet::from([5])), []);
        assert_eq!(s.lost(&BTreeSet::new()).len(), 1);
        assert!(s.is_empty());
    }

    #[test]
    fn toasts_keep_the_newest() {
        let mut s = Sent::default();
        let mut t = Vec::new();
        for i in 0..7 {
            s.toast(&mut t, Text::new("k", &i.to_string()), None);
        }
        assert_eq!(t.len(), TOASTS_MAX);
        assert_eq!((t[0].id.as_str(), t[4].id.as_str()), ("toast-3", "toast-7"));
    }
}
