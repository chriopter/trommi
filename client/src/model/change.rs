//! What one batch touched: the records an app must read again (the web app's `Change`).

use super::types::{ItemKey, TimelineItem};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Change {
    pub cards: BTreeSet<String>,
    pub sessions: BTreeSet<String>,
    pub permissions: BTreeSet<String>,
    pub notes: BTreeSet<String>,
    pub published: BTreeSet<String>,
    pub timelines: BTreeSet<String>,
    pub registers: BTreeSet<String>,
    pub members: bool,
    pub alerts: bool,
    pub stack: bool,
    pub room: bool,
    /// The session list's status changed by the clock (a link asleep, a red hand after the grace, a quiet hint).
    pub status: bool,
    /// The items added or replaced in this batch, by timeline key, in the order they came.
    pub items: BTreeMap<String, Vec<ItemKey>>,
}

impl Change {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add_item(&mut self, key: &str, item: &TimelineItem) {
        let k = ItemKey::of(item);
        let list = self.items.entry(key.to_string()).or_default();
        if !list.contains(&k) {
            list.push(k);
        }
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
            && !self.members
            && !self.alerts
            && !self.stack
            && !self.room
            && !self.status
            && self.cards.is_empty()
            && self.sessions.is_empty()
            && self.permissions.is_empty()
            && self.notes.is_empty()
            && self.published.is_empty()
            && self.timelines.is_empty()
            && self.registers.is_empty()
    }
}
