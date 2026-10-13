//! What this device sent and the hub has not answered yet: each outbox entry with the echoes it carries and the
//! action that made it, so a refusal rolls the echo back with a toast that offers the same action again.
//!
//! An entry leaves this list three ways: the hub took it (`Report::Accepted`: the hub's copy replaces the echo when
//! it comes back), the hub refused it (`Report::Refused`: rolled back, a toast), or it is no longer in the device's
//! outbox although neither was said (the write that held it failed and the stored state was opened again without
//! it: rolled back, a toast). Nothing here is stored: after a restart the outbox sends what it holds, once, and the
//! hub's copy shows it.
//!
//! A Note has at most one version on its way (web `noteBusy`): the device takes a later version only on one it
//! holds, and it holds an own version once that came back from the hub. A further edit meanwhile is shown at once and
//! waits here, the newest only; it is sealed when the version before it is back, refused or lost. (Like the web's, a
//! waiting edit is in memory only: it is lost if the app ends before the version before it came back.)

use super::Action;
use crate::views::{Text, Toast};
use serde_json::{Map, Value};
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

/// An edit of a Note that waits for the version before it.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteNext {
    /// The note's own fields it changes (every waiting edit's, the newest last).
    pub own: Map<String, Value>,
    /// It deletes the note.
    pub closed: bool,
    /// Its echo.
    pub local_id: String,
    /// The action that made it, offered again when it is refused.
    pub action: Action,
}

/// A Note's version on its way: its hash and outbox entry, and the edit waiting behind it.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteBusy {
    pub hash: String,
    pub outbox: u64,
    pub next: Option<NoteNext>,
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
    /// The Notes with a version on its way, by object id.
    pub notes: BTreeMap<String, NoteBusy>,
    /// The highest lamport this App gave a Note's version (9.3.2 chooses the current version by it).
    pub lamport: u64,
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

    /// The entries that are no longer in the device's outbox (`held`).
    pub fn gone(&self, held: &BTreeSet<u64>) -> Vec<u64> {
        self.outbox.keys().filter(|id| !held.contains(id)).copied().collect()
    }

    /// The Note whose version on its way has this hash, or this outbox entry, is free again: its id and the edit
    /// that waited behind it.
    pub fn note_back(&mut self, hash: Option<&str>, outbox: Option<u64>) -> Option<(String, Option<NoteNext>)> {
        let id = self
            .notes
            .iter()
            .find(|(_, b)| hash == Some(b.hash.as_str()) || outbox == Some(b.outbox))
            .map(|(id, _)| id.clone())?;
        let busy = self.notes.remove(&id)?;
        Some((id, busy.next))
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
        assert_eq!(s.gone(&BTreeSet::from([5])), Vec::<u64>::new());
        assert_eq!(s.gone(&BTreeSet::new()), vec![5]);
        assert_eq!(s.take(5).len(), 1);
        assert!(s.is_empty());
    }

    #[test]
    fn a_note_is_free_again_by_its_hash_or_its_entry() {
        let mut s = Sent::default();
        let next = NoteNext { own: Map::new(), closed: true, local_id: "local-9".into(), action: star("n") };
        s.notes.insert("n1".into(), NoteBusy { hash: "aa".into(), outbox: 3, next: Some(next.clone()) });
        s.notes.insert("n2".into(), NoteBusy { hash: "bb".into(), outbox: 4, next: None });
        assert_eq!(s.note_back(Some("cc"), Some(9)), None);
        assert_eq!(s.note_back(Some("aa"), None), Some(("n1".into(), Some(next))));
        assert_eq!(s.note_back(None, Some(4)), Some(("n2".into(), None)));
        assert!(s.notes.is_empty());
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
