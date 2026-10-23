//! Several windows on one App: the browser's tabs of one profile share one core (one device, one place that moves
//! its keys on), and each tab looks at its own page. An event names its window (`App::handle_in`); a shell with one
//! window never names one and is window 0.
//!
//! What is a window's: its lens (team, page, search, the pages of a list shown), its parts of the view model
//! (`WindowParts`: sidebar, Desk, the session and card in view, the board, …), what it was rendered last, the note
//! it is writing, whether it is in front. Everything else is the device's and the same in every window: the room, the phase and its line,
//! the account's steps, settings, files, dictation, the toasts.
//!
//! The App works on one window at a time, the current one: its lens is `App.lens`, its parts are in
//! `App.next_view`, its rendered model is `App.view`. An event of another window first makes that one current
//! (`switch_window`). After every event each other window gets its views made again when the room or the clock moved
//! them, and its own `Render` (`render_others`).
//!
//! A step a window started stays that window's through its I/O (`origin_of`, `mark_origin`): a passkey or push
//! prompt, a file to save, a link to open go to the window whose action led to them, even when another window acted
//! meanwhile; the shell refuses such a prompt when that window went.

use super::{App, Effect, Event};
use crate::views::{Lens, ViewModel, WindowParts};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// One window. Its lens, parts, view and first render are checked out into the App while it is the current one;
/// the rest is kept here for every window.
pub(super) struct Window {
    lens: Lens,
    parts: WindowParts,
    view: ViewModel,
    rendered_once: bool,
    /// The note this window is writing (`KeepNote` without an id: `App.made_note`).
    made_note: Option<String>,
    /// The Scribble Board in view (`Focus.board`, `Screen::Scribble`).
    pub(super) board: Option<trommi_core::ids::BoardId>,
    /// The window shows its page (`Focus.visible`): its chat in view counts as read.
    pub(super) looking: bool,
    /// The window is in front (`Event::Foreground` / `Background`).
    pub(super) front: bool,
    /// The files its views name, with their references.
    pub(super) shown: BTreeMap<String, Map<String, Value>>,
    /// When its views change by the clock alone.
    pub(super) wake: Option<u64>,
    /// Its views are to be made again.
    pub(super) stale: bool,
}

impl Default for Window {
    fn default() -> Self {
        Window {
            lens: Lens::default(),
            parts: WindowParts::default(),
            view: ViewModel::default(),
            rendered_once: false,
            made_note: None,
            board: None,
            looking: false,
            front: true,
            shown: BTreeMap::new(),
            wake: None,
            stale: true,
        }
    }
}

impl Window {
    /// What it looks at (for the current window, `App.lens` is the one that counts).
    pub(super) fn lens(&self) -> &Lens {
        &self.lens
    }
}

impl App {
    /// The window events are handled for now.
    pub fn window(&self) -> u32 {
        self.window
    }

    /// The windows open, in order.
    pub fn windows(&self) -> Vec<u32> {
        self.windows.keys().copied().collect()
    }

    /// A window's view model as rendered to it: what that window's shell holds after folding its `Render`s.
    pub fn window_view(&self, window: u32) -> Option<&ViewModel> {
        if window == self.window {
            return Some(&self.view);
        }
        self.windows.get(&window).map(|w| &w.view)
    }

    /// The window that is current. Called before an event of that window; a window not seen before is opened.
    pub(super) fn switch_window(&mut self, to: u32) {
        if to == self.window {
            self.windows.entry(to).or_default();
            return;
        }
        let mut incoming = self.windows.remove(&to).unwrap_or_default();
        let mut lens = std::mem::take(&mut incoming.lens);
        lens.carry_device(&self.lens);
        let lens = std::mem::replace(&mut self.lens, lens);
        let parts = self.next_view.take_window_parts();
        self.next_view.put_window_parts(std::mem::take(&mut incoming.parts));
        let view = std::mem::replace(&mut self.view, std::mem::take(&mut incoming.view));
        let rendered_once = std::mem::replace(&mut self.rendered_once, incoming.rendered_once);
        let made_note = std::mem::replace(&mut self.made_note, incoming.made_note.take());
        // (the one left keeps what it had: its lens, its parts, what it was shown)
        if let Some(left) = self.windows.get_mut(&self.window) {
            left.lens = lens;
            left.parts = parts;
            left.view = view;
            left.rendered_once = rendered_once;
            left.made_note = made_note;
        }
        self.next_view.rev = self.view.rev;
        self.seen.looking = incoming.looking;
        self.windows.insert(to, incoming);
        self.window = to;
        self.scribble_windows();
    }

    /// The Scribble Boards in view: the current window's is the focus, the other windows' are watched too.
    pub(super) fn scribble_windows(&mut self) {
        let focus = self.windows.get(&self.window).and_then(|w| w.board);
        let others = self.windows.iter().filter(|(id, _)| **id != self.window).filter_map(|(_, w)| w.board).collect();
        let thumbs = self.lens.thumbs || self.windows.iter().any(|(id, w)| *id != self.window && w.lens.thumbs);
        self.scribble.in_view(focus, others, thumbs);
    }

    /// The current window, mutable (it is always in the map).
    pub(super) fn this_window(&mut self) -> &mut Window {
        self.windows.entry(self.window).or_default()
    }

    /// Whether any window is in front.
    pub(super) fn any_front(&self) -> bool {
        self.windows.values().any(|w| w.front)
    }

    /// Whether any window shows its page.
    pub(super) fn any_looking(&self) -> bool {
        self.windows.values().any(|w| w.looking)
    }

    /// A window went: forgotten, unless it is the last.
    pub(super) fn close_window(&mut self, window: u32) {
        if self.windows.len() < 2 || !self.windows.contains_key(&window) {
            return;
        }
        if window == self.window {
            let Some(other) = self.windows.keys().copied().find(|w| *w != window) else { return };
            self.switch_window(other);
        }
        self.windows.remove(&window);
        self.scribble_windows();
        // (the files it showed may go, the clock's wake is the others')
        self.stale_views = true;
    }

    /// What the views of every window name, and the earliest moment one changes by the clock.
    pub(super) fn windows_shown(&self) -> (BTreeMap<String, Map<String, Value>>, Option<u64>) {
        let mut shown = BTreeMap::new();
        for w in self.windows.values() {
            shown.extend(w.shown.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        (shown, self.windows.values().filter_map(|w| w.wake).min())
    }

    /// After an event: every other window's views made again when the room, the clock or a lens moved them
    /// (`moved`), and its render. The current window stays current.
    pub(super) fn render_others(&mut self, now_ms: u64, moved: bool, out: &mut Vec<Effect>) {
        if self.windows.len() < 2 {
            return;
        }
        let home = self.window;
        let others: Vec<u32> = self.windows.keys().copied().filter(|w| *w != home).collect();
        for w in others {
            self.switch_window(w);
            if moved || self.windows.get(&w).is_some_and(|w| w.stale) {
                self.make_views(now_ms);
            }
            self.render_window(out);
        }
        self.switch_window(home);
    }

    /// The window an event's step belongs to: the answer to an I/O is the step of the window that started it;
    /// anything else is the window it names.
    pub(super) fn origin_of(&mut self, window: u32, event: &Event) -> u32 {
        match event {
            Event::Http { id, .. } | Event::Store { id, .. } | Event::Secret { id, .. } | Event::Platform { id, .. } => {
                self.io_windows.remove(id).unwrap_or(window)
            }
            Event::Stream { id, .. } | Event::Progress { id, .. } => self.io_windows.get(id).copied().unwrap_or(window),
            _ => window,
        }
    }

    /// After an event: its I/O is the step of `origin`, and what only a window shows (a prompt, a file to save, a
    /// link) names it. Only kept where several windows share the App; bounded (an answer that never came).
    pub(super) fn mark_origin(&mut self, origin: u32, out: &mut [Effect]) {
        const KEPT: usize = 1024;
        for effect in out.iter_mut() {
            match effect {
                Effect::Http { id, .. } | Effect::OpenStream { id, .. } | Effect::Store { id, .. } | Effect::Secret { id, .. } => {
                    if origin != 0 {
                        self.io_windows.insert(*id, origin);
                    }
                }
                Effect::Platform { id, window, .. } => {
                    *window = origin;
                    if origin != 0 {
                        self.io_windows.insert(*id, origin);
                    }
                }
                Effect::Cancel { id } => {
                    self.io_windows.remove(id);
                }
                Effect::SaveFile { window, .. } | Effect::OpenUrl { window, .. } => *window = origin,
                _ => {}
            }
        }
        while self.io_windows.len() > KEPT {
            self.io_windows.pop_first();
        }
    }
}
