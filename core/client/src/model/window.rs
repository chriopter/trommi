//! A timeline's window as the app reads it: pages of older items, a board's tail, items from the app's cache.

use super::change::Change;
use super::received::{Outcome, Received};
use super::types::*;
use super::Model;

impl Model {
    /// Envelopes of a page of one timeline, read out of the hub's order: applied to this model (its window opened)
    /// or, `scratch`, to a model of their own that shares only the members (a read that must not grow the window).
    /// Returns the items they gave, in the page's order (those the window holds).
    pub fn apply_page(
        &mut self,
        timeline_key: &str,
        received: &[Received],
        scratch: bool,
        now: u64,
        change: &mut Change,
    ) -> Vec<TimelineItem> {
        let mut other = Model {
            members: self.members.clone(),
            my_device_id: self.my_device_id.clone(),
            ..Model::default()
        };
        let mut other_change = Change::new();
        let (m, ch) = if scratch {
            (&mut other, &mut other_change)
        } else {
            (self, change)
        };
        m.timeline_of(timeline_key).window_open = true;
        for r in received.iter().filter(|r| r.outcome != Outcome::Refused) {
            m.apply_envelope(r, ch, now);
        }
        let t = m.timeline_of(timeline_key);
        received
            .iter()
            .filter_map(|r| t.items.get(&ItemKey::Number(r.change)).cloned())
            .collect()
    }

    /// Items into a timeline's window where it holds none with a body, and its window as the app read it.
    pub fn put_items(
        &mut self,
        timeline_key: &str,
        items: Vec<TimelineItem>,
        loaded_down_to: Option<u64>,
        has_more: Option<bool>,
        change: &mut Change,
    ) {
        let t = self.timeline_of(timeline_key);
        t.window_open = true;
        let mut added = Vec::new();
        for it in items {
            let k = ItemKey::of(&it);
            let has_body = t
                .items
                .get(&k)
                .is_some_and(|x| x.get("content").is_some_and(|c| !c.is_null()));
            if !has_body {
                t.items.insert(k.clone(), it);
            }
            added.push(t.items[&k].clone());
        }
        if let Some(d) = loaded_down_to {
            t.loaded_down_to = Some(t.loaded_down_to.map_or(d, |x| x.min(d)));
        }
        if let Some(m) = has_more {
            t.has_more = m;
        }
        for it in &added {
            change.add_item(timeline_key, it);
        }
        change.timelines.insert(timeline_key.to_string());
    }
}
