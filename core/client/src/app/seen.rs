//! What the Chat list counts as read on this device: for each session, the newest word of its agent when its chat
//! was last in view (`Lens::seen`; the dot of `SessionRowView.unread`). Kept in the room's cache under
//! `c1/<room>/seen` (only numbers: no words, no names), read when the room opens, written when a mark moves. The
//! first time there is none, everything said up to the first look counts as read (no dot on every old chat).

use super::{App, Effect, IoId, IoResult, Pending, StoreAnswer, StoreOp};
use crate::app::io::Entry;
use crate::views::Phase;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Seen {
    /// The cache key, once the room is open.
    key: Option<String>,
    /// Nothing was kept: the first look marks everything read.
    baseline: bool,
    /// The marks as the cache holds them (or is being given them).
    stored: BTreeMap<String, u64>,
    /// A write is on its way; `again`: the marks moved meanwhile.
    writing: bool,
    again: bool,
    /// The chat in view is shown (`Focus.visible`, the app in front).
    pub(super) looking: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Stored {
    v: u32,
    marks: BTreeMap<String, u64>,
}

impl App {
    /// The room's cache opened: its marks are read.
    pub(super) fn seen_open(&mut self, room_hex: &str, out: &mut Vec<Effect>) {
        let key = crate::cache::key(room_hex, crate::cache::part::SEEN, None);
        self.seen = super::seen::Seen { key: Some(key.clone()), looking: self.seen.looking, ..Default::default() };
        self.lens.seen = None;
        let id = self.io(Pending::SeenRead);
        out.push(Effect::Store { id, op: StoreOp::Get { keys: vec![key] } });
    }

    /// The demo room: nothing is kept, nothing counts as read before it is looked at.
    pub(super) fn seen_demo(&mut self) {
        self.seen = super::seen::Seen { looking: self.seen.looking, ..Default::default() };
        self.lens.seen = Some(BTreeMap::new());
    }

    pub(super) fn seen_read(&mut self, result: &IoResult<StoreAnswer>) {
        let kept: Option<Stored> = match result {
            IoResult::Ok(StoreAnswer::Values { values }) => values.first().and_then(Option::as_ref).and_then(|v| serde_json::from_slice(&v.0).ok()),
            _ => None,
        };
        let kept = kept.filter(|k| k.v == 1);
        self.seen.baseline = kept.is_none();
        let marks = kept.map(|k| k.marks).unwrap_or_default();
        self.seen.stored = marks.clone();
        self.lens.seen = Some(marks);
        self.stale_views = true;
    }

    pub(super) fn seen_written(&mut self, id: IoId, result: &IoResult<StoreAnswer>, out: &mut Vec<Effect>) {
        let _ = id;
        self.seen.writing = false;
        if !matches!(result, IoResult::Ok(StoreAnswer::Done)) {
            // (only a cache: the marks are written again with the next move)
            self.seen.stored.clear();
        }
        if std::mem::take(&mut self.seen.again) {
            self.seen_write(out);
        }
    }

    /// Before the views are made: the chat in view is read up to its newest word; the first look reads everything.
    pub(super) fn seen_tend(&mut self, out: &mut Vec<Effect>) {
        // (the chats in view of every window that shows its page)
        let current = self.window;
        let looked: Vec<String> = self.seen.looking.then(|| self.lens.session.clone()).flatten().into_iter()
            .chain(self.windows.iter().filter(|(id, w)| **id != current && w.looking).filter_map(|(_, w)| w.lens().session.clone()))
            .collect();
        let Some(marks) = self.lens.seen.as_mut() else { return };
        let model = &self.board.model;
        let mut moved = false;
        if self.seen.baseline && self.next_view.phase == Phase::Live && !model.reading {
            for (key, n) in crate::views::sidebar::newest_words(model) {
                moved |= marks.insert(key, n) != Some(n);
            }
            self.seen.baseline = false;
        }
        for shown in looked.iter().map(String::as_str) {
            // (the page's address may name the session by its short id)
            let key = model.sessions.keys().find(|k| *k == shown).cloned().or_else(|| {
                crate::sessions::session_list(model, false).into_iter().find(|r| r.id == shown).map(|r| r.key)
            });
            if let Some(key) = key {
                if let Some(n) = crate::views::sidebar::newest_word(model, &key) {
                    if marks.get(&key).is_none_or(|m| *m < n) {
                        marks.insert(key, n);
                        moved = true;
                    }
                }
            }
        }
        if moved {
            self.stale_views = true;
        }
        if self.seen.key.is_some() && self.lens.seen.as_ref() != Some(&self.seen.stored) {
            self.seen_write(out);
        }
    }

    fn seen_write(&mut self, out: &mut Vec<Effect>) {
        let (Some(key), Some(marks)) = (self.seen.key.clone(), self.lens.seen.clone()) else { return };
        if self.seen.writing {
            self.seen.again = true;
            return;
        }
        let Ok(value) = serde_json::to_vec(&Stored { v: 1, marks: marks.clone() }) else { return };
        self.seen.writing = true;
        self.seen.stored = marks;
        let id = self.io(Pending::SeenWrite);
        out.push(Effect::Store { id, op: StoreOp::Put { put: vec![Entry { key, value: super::Bytes(value) }], delete: Vec::new() } });
    }
}
