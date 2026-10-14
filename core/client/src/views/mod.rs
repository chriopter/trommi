//! The view model: what every screen shows, decided once. A shell draws it and nothing else.
//!
//! The core sends it as [`ViewDelta`]s inside `Effect::Render`. A shell keeps one [`ViewModel`] and folds each delta
//! into it with [`ViewModel::apply`] (or its own copy of the same rule, which the conformance scripts check).
//!
//! **Units.** A delta replaces whole units: the phase, the sidebar, the Desk, one session, one card, the notes,
//! the settings, the toasts. A unit is keyed where there are many (sessions and cards by id); `None` removes one.
//! A list that can be long (the Desk's stack, a session's talk) may later travel as [`Rows`] edits instead of
//! whole; folding gives the same model either way.
//!
//! **Texts.** Every word a screen shows that depends on state is a [`Text`]: a key, its arguments, and the
//! English sentence. Shells show `en` today; a translation later looks the key up. A shell never builds such a
//! sentence itself. Fixed labels (a button's "Send") stay the shell's.
//!
//! W4 fills the screen types; their fields here are the contract's first cut.
//!
//! **Who makes it.** [`board::Board`] reads the board model once per render (the session list, the desks, what
//! waits where), and each screen's module makes its unit from it: [`sidebar`], [`desk`], [`session`] and [`card`] (the pages in view;
//! its conversation's rows come from [`talk`]), [`notes`] and [`settings`]. The rules are the web app's of
//! before (`app/web/public/app.mjs` `boardModel`), now here and nowhere else; [`words`] holds every sentence.
//! What this device alone chooses (the desk in view, the zone) comes in as a [`Lens`].

pub mod board;
pub mod card;
pub mod demo;
pub mod files_seen;
pub mod desk;
pub mod links;
pub mod notes;
pub mod off;
pub mod session;
pub mod settings;
pub mod sidebar;
pub mod talk;
#[cfg(test)]
mod testroom;
pub mod words;

pub use board::{render, render_with_files, Board, Lens, MoreOf};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A sentence of the core: `key` names it, `args` fill it, `en` is it in English.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub key: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub args: BTreeMap<String, String>,
    pub en: String,
}

impl Text {
    pub fn new(key: &str, en: &str) -> Self {
        Self {
            key: key.into(),
            args: BTreeMap::new(),
            en: en.into(),
        }
    }
}

/// Where the app stands, for the whole window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// Before `Start`, or while the stored room is read.
    #[default]
    Starting,
    /// No room on this device: the sign-in and join screens.
    SignedOut,
    /// A room is open; the Desk is being read for the first time on this device.
    Opening,
    /// Open and current, the live stream runs.
    Live,
    /// Open, showing what it has; the hub cannot be reached right now.
    Offline,
    /// The device was removed, or another owner took its state. Nothing more is done.
    Stopped,
}

/// What the person looks at. Sent by the shell as `Event::Focus`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Focus {
    pub screen: Screen,
    /// The window (or app) is in front and visible.
    #[serde(default = "yes")]
    pub visible: bool,
    /// The desk in view: a desk's id, "all" for every desk at once, none for the first desk.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desk: Option<String>,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "screen", rename_all = "snake_case")]
pub enum Screen {
    #[default]
    Desk,
    Session {
        session_id: String,
    },
    Card {
        card_id: String,
    },
    Notes,
    /// The page Off your mind, the complete list, with the search's words (empty: all).
    Off {
        #[serde(default)]
        query: String,
    },
    Scribble {
        board_id: String,
    },
    Settings,
}

/// Everything the shell draws. Serialised, this is what the conformance scripts compare.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewModel {
    /// Counts the deltas folded in; a delta whose `base` is not this is out of order (a shell bug).
    pub rev: u64,
    pub phase: Phase,
    /// One line on where the app stands when it is not plainly live ("Catching up", "Offline: retry in 30 s").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Text>,
    pub sidebar: SidebarView,
    pub desk: DeskView,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sessions: BTreeMap<String, SessionView>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub cards: BTreeMap<String, CardView>,
    /// The page Off your mind, while it is in view (`Screen::Off`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<OffListView>,
    pub notes: NotesView,
    pub settings: SettingsView,
    /// Signing in, joining, the emoji to compare, the Emergency Kit to keep, self-join asks.
    pub account: AccountView,
    /// Files the views name (attachments, pictures), by id: uploading, ready, failed. Their bytes, when loaded,
    /// came as `Effect::FileReady` under the same id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub files: BTreeMap<String, FileView>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub toasts: Vec<Toast>,
}

/// One change of the view model, from `base` to `base + 1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewDelta {
    pub base: u64,
    pub parts: Vec<ViewPart>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "part", rename_all = "snake_case")]
// (a unit travels whole and is cloned into the model once: boxing each would only move the copy)
#[allow(clippy::large_enum_variant)]
pub enum ViewPart {
    /// Everything, replacing what the shell had (the first render, after a restart of the core).
    Whole {
        model: Box<ViewModel>,
    },
    Phase {
        phase: Phase,
        status: Option<Text>,
    },
    Sidebar {
        view: SidebarView,
    },
    Desk {
        view: DeskView,
    },
    Session {
        id: String,
        view: Option<SessionView>,
    },
    Card {
        id: String,
        view: Option<Box<CardView>>,
    },
    /// The page Off your mind; none when it is left.
    Off {
        view: Option<OffListView>,
    },
    Notes {
        view: NotesView,
    },
    Settings {
        view: SettingsView,
    },
    Account {
        view: AccountView,
    },
    File {
        id: String,
        view: Option<FileView>,
    },
    Toasts {
        toasts: Vec<Toast>,
    },
}

/// Edits of a long list, by position: at `at`, `gone` rows leave and `rows` come (the Desk's stack ops today).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowEdit<T> {
    pub at: u32,
    pub gone: u32,
    pub rows: Vec<T>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "rows", rename_all = "snake_case")]
pub enum Rows<T> {
    Whole { items: Vec<T> },
    Edits { edits: Vec<RowEdit<T>> },
}

impl<T: Clone> Rows<T> {
    /// Applies these rows to `list`. An edit past the end is cut to the end (never a panic in a shell's fold).
    pub fn fold_into(&self, list: &mut Vec<T>) {
        match self {
            Rows::Whole { items } => *list = items.clone(),
            Rows::Edits { edits } => {
                for e in edits {
                    let at = (e.at as usize).min(list.len());
                    let end = (at + e.gone as usize).min(list.len());
                    list.splice(at..end, e.rows.iter().cloned());
                }
            }
        }
    }
}

/// The sidebar (the phone: the menu's desks, the list of chats, the tab pill's numbers): the desks, and one row per
/// session of the desk in view, in the order they are drawn.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarView {
    /// The desks, in the menu's order. A room without desks has one: "Personal".
    pub desks: Vec<DeskTab>,
    /// The sessions of the desk in view, one row each, in drawing order: a main, then its helpers.
    pub sessions: Vec<SessionRowView>,
    /// The number on the app icon and the tab: the open questions of the desk in view (no infos, nothing that is
    /// with its agent). Never drawn red.
    pub badge: u32,
    /// The desk in view: its id, or "all".
    #[serde(default)]
    pub desk: String,
    /// "All Desks", the row above the desks: there with two desks or more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<DeskTab>,
    /// Where the person is, for the head: the desk's name, or "All desks".
    #[serde(default)]
    pub place: Text,
    /// The open questions of the desk in view (its lamp is lit while there are some).
    #[serde(default)]
    pub waiting: u32,
    /// How many of them knock.
    #[serde(default)]
    pub knocking: u32,
    /// How many rows of the desk in view are stopped (the red hand).
    #[serde(default)]
    pub stopped: u32,
    /// How many of them are at work.
    #[serde(default)]
    pub working: u32,
    /// How many of his answers the sessions of the desk in view have not picked up.
    #[serde(default)]
    pub unheard: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskTab {
    pub id: String,
    pub name: Text,
    /// The open questions of the desk's sessions.
    pub waiting: u32,
    pub selected: bool,
    /// One of them knocks.
    #[serde(default)]
    pub knocks: bool,
}

/// One session in the sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRowView {
    /// The session's key in the model (`Screen::Session`, the actions' `session_id`, `ViewModel.sessions`).
    pub id: String,
    pub name: Text,
    /// "live", "oncall", "asleep", "cut", "gone" (core/client/src/status.rs); "" for a helper under its main (it
    /// runs inside its main's process: the main's link says it for both).
    pub link: String,
    /// The red hand.
    pub blocked: bool,
    /// How many of his answers it has not picked up for two minutes; a main counts its helpers' too.
    pub unheard: u32,
    /// 0 for a row of its own, 1 for a helper under its main.
    pub depth: u32,
    /// Its short id on the board: its address (`/chat/<slug>`).
    #[serde(default)]
    pub slug: String,
    /// What draws it: the icon the person or the agent chose ("draw:leaf"), else its slug.
    #[serde(default)]
    pub mark: String,
    /// The desk it stands on.
    #[serde(default)]
    pub desk_id: String,
    #[serde(default)]
    pub online: bool,
    /// Connected and at work (a status line of it shows "working").
    #[serde(default)]
    pub working: bool,
    /// A main session: one that says so, or one with helpers.
    #[serde(default)]
    pub main: bool,
    /// The crowned session of its desk (the one a note goes to).
    #[serde(default)]
    pub crowned: bool,
    /// Its main's id, for a helper drawn under it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// Its helpers' ids, in drawing order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subs: Vec<String>,
    /// Its open questions: what waits for the person on the Desk (no infos, nothing that is with its agent).
    #[serde(default)]
    pub open: u32,
    /// One of them knocks.
    #[serde(default)]
    pub knocks: bool,
    /// Why it is stopped, in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<HandView>,
    /// The small line under its name: its link when it does not hear at once, else the answers it has not picked up.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap: Option<CapView>,
    /// A helper whose main is not listening, cut off or gone: drawn hushed.
    #[serde(default)]
    pub hushed: bool,
    /// Not connected, and none of its helpers is: it stands under "Disconnected".
    #[serde(default)]
    pub away: bool,
    /// The line drawn above this row: "Disconnected" over the first row that is away, a desk's name over its first
    /// row on All desks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<HeadingView>,
    /// The part of the list a main row may be dragged within (its desk, connected or not); "" for a helper.
    #[serde(default)]
    pub group: String,
    /// What its badge says to a screen reader ("Working, 2 questions open"); none without a badge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub says: Option<Text>,
    /// A main with helpers, drawn folded: what the row then shows for all of them together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folded: Option<FoldView>,
}

/// A main and its helpers as one folded row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FoldView {
    pub open: u32,
    pub knocks: bool,
    /// One of them is connected.
    pub online: bool,
    pub working: bool,
    /// The first of them that is stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<HandView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub says: Option<Text>,
    /// The helpers that are stopped: their edges show it while the row is folded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stopped: Vec<String>,
}

/// Why a session is stopped: the red hand.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandView {
    /// "cut", "offline", "permission", "blocking".
    pub why: String,
    pub text: Text,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
}

/// The small line under a session's name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapView {
    /// "oncall", "asleep", "cut", "gone", or "unheard" (it hears at once and has answers to pick up).
    pub kind: String,
    /// Two or three words: "cut off · 14:05", "1 answer waits".
    pub word: Text,
    /// The tooltip: "Cut off since 14:05".
    pub short: Text,
    /// The whole sentence about the session, for a screen reader; none for "unheard".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Text>,
}

/// A line above a row of the sidebar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadingView {
    /// "away" ("Disconnected") or "desk" (a desk's name, on All desks).
    pub kind: String,
    pub text: Text,
    /// For "desk": the desk, and its open questions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desk_id: Option<String>,
    #[serde(default)]
    pub waiting: u32,
}

/// A session's link in words: whether it hears the person, and what to do when it does not.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkWords {
    /// "live", "oncall", "asleep", "cut", "gone".
    pub state: String,
    /// Since when it is cut off or gone; for "oncall" and "asleep" its last step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<u64>,
    /// Two or three words under its name ("not listening · 12 min"); none while it hears at once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word: Option<Text>,
    /// The same as a short sentence ("Not listening for 12 min").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<Text>,
    /// The whole sentence about the session.
    pub line: Text,
    /// The way out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixView>,
}

/// What to do about a link: a sentence, and a line to type (empty when there is none).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixView {
    pub say: Text,
    pub code: String,
}

/// The Desk of the desk in view: its head, and the stack of what waits for the person as rows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskView {
    /// Card and request ids, in the Desk's order: the ids of `rows`.
    pub stack: Vec<String>,
    /// Off your mind, of the desk in view (`off.rs`, from the core's `off_mind`): the cards with their agents,
    /// starred ones too, the one that moved last first (the complete list's top part).
    pub off_your_mind: Vec<String>,
    /// The put-off cards, the last put off first.
    pub later: Vec<String>,
    /// The cards closed last (done, withdrawn, shredded), the newest first: the core's window of them
    /// (`off_mind::RECENT` for the whole room), not every closed card; the complete list has them all.
    pub done: Vec<String>,
    /// The two stacks docked at the Desk's foot; none while a stack is empty. A card has one home: a starred card
    /// is in `starred` only, also while it is being worked on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub starred: Option<DockView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<DockView>,
    /// The section Off your mind under the Desk: its first rows and how many; none while nothing is put off or
    /// closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub off: Option<OffHintView>,
    /// What stands in the list's place when nothing is open at all ("Questions land here.").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<Text>,
    /// "invite" (no session stands here yet: invite the first agent), "reading" (none yet, and the room is still
    /// being read), "desk".
    #[serde(default)]
    pub mode: String,
    /// The one big line at the top; none for "invite".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hello: Option<Text>,
    /// How far the reading of the room's past is ("Reading your history: 12,400 of 27,000"), while it is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading: Option<Text>,
    /// The desk's goals; none on All desks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goals: Option<GoalsView>,
    /// What waits, in the Desk's order: the questions and what is only to read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<DeskRowView>,
    /// The open questions (no infos): the number in the window's title.
    #[serde(default)]
    pub waiting: u32,
    /// What Blitz walks through and the duck closes ("Blitz: 3 open cards"); none when nothing waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blitz: Option<Text>,
    /// The sessions that are cut off, above the questions: one line each, with the step in its terminal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slip: Vec<SlipView>,
}

/// One of the two stacks docked at the Desk's foot (the owner's pick of 13 October 2026: two small tabs, the
/// titles shown only when a stack is fanned out).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockView {
    /// "star" or "run".
    pub key: String,
    /// How many cards it holds: the number on its tab (`rows.len()`).
    pub count: u32,
    /// What the tab says to a screen reader ("3 starred", "1 running").
    pub says: Text,
    /// Its cards in order, fanned out: the first is the top card, the one the tab shows.
    pub rows: Vec<DockRowView>,
    /// The way on to the complete list at the end of the fan ("All running and done"); the running stack only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all: Option<Text>,
}

/// A small card of a docked stack.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockRowView {
    /// The card (`Screen::Card`).
    pub id: String,
    pub title: String,
    pub from: FromView,
    /// The quiet line under the title: "Fernly · running" (a starred card being worked on), "Fernly", "Fernly ·
    /// Reworking the copy" (its status line), "Fernly · on it".
    pub line: Text,
    /// It is being worked on (the drawing's ring turns).
    pub running: bool,
    /// When it last moved (its status line's update, else when it went to its agent); when it was starred, for a
    /// starred card.
    pub at_ms: u64,
}

/// The section Off your mind under the Desk: a short hinted list that opens the complete list (`Screen::Off`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffHintView {
    /// The first rows, put off first, then the closed ones, the newest first (at most three; the shell fades them).
    pub rows: Vec<OffRowView>,
    /// "2 put off · 14 done".
    pub says: Text,
    pub put_off: u32,
    /// Every closed card of the desk in view, counted (not only the ones named).
    pub closed: u32,
}

/// A put-off or closed card, one small quiet row.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffRowView {
    pub id: String,
    /// "later", "done" or "trash".
    pub place: String,
    /// Its mark: "snooze" (put off), "shred" (shredded: the title struck through), "withdraw" (withdrawn by its
    /// agent), "tick" (done).
    pub mark: String,
    pub title: String,
    #[serde(default)]
    pub starred: bool,
    /// When it closed (ms); 0 for a put-off card and when no time is known (show none, never the epoch).
    #[serde(default)]
    pub at_ms: u64,
    /// A put-off card: until when ("until Tue 07:00", the person's zone).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Text>,
    /// What it is, to a screen reader: "Later", "Done", "Shredded", "Withdrawn".
    pub says: Text,
}

/// The page Off your mind: every card off the Desk of the desk in view. What runs as live cards, the one that moved
/// last first; then what is put off, the last put off first; then what is closed, the newest first. Each card in its
/// place by time, none lifted. Kept to the search's words.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffListView {
    /// The search's words as given (`Screen::Off`).
    #[serde(default)]
    pub query: String,
    /// The cards with their agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub works: Vec<OffWorkView>,
    /// The put-off cards, then the closed ones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rest: Vec<OffRowView>,
    /// In the list's place when nothing is shown: "Nothing yet.", "Nothing here has these words.".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub none: Option<Text>,
    /// The line under `rest` when this device holds more of it than the window shows: `rest` is the first
    /// [`board::OFF_PAGE`] rows, a page more for each time the person asked (`Lens::off_window`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub more: Option<MoreView>,
    /// Older closed cards are on the hub and not on this device yet (history on demand): the line under the list
    /// that fetches the next page. Always none until the engine pages the Done list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub older: Option<Text>,
    /// A page of older cards is being fetched: the shell shows its quiet "Loading …" line at the end.
    #[serde(default)]
    pub loading: bool,
}

/// More of a long list than its window shows: the line that asks for the next page (an action moves the window,
/// `Lens::show_more`; where this device does not hold it yet, the engine reads it from the hub).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoreView {
    /// Rows this device holds beyond the window.
    pub left: u32,
    /// The hub holds more beyond those (not read on this device yet).
    #[serde(default)]
    pub hub: bool,
    /// "Earlier messages", "Show more".
    pub says: Text,
    /// How many pages the window shows after the person asks: the number the action sets.
    pub pages: u32,
}

/// A card with its agent, on the page Off your mind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OffWorkView {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub starred: bool,
    pub from: FromView,
    /// Its status line's words, else "on it".
    pub label: Text,
    /// When it last moved (ms): the shell's "5 min".
    pub at_ms: u64,
    /// The whole row to a screen reader: "Fix the login, Fernly is on it".
    pub says: Text,
}

/// A desk's goals: a note of the person's, at most twenty lines.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalsView {
    pub desk_id: String,
    /// The lines; empty when there are none (the shell shows its faint "Goals…").
    pub lines: Vec<String>,
    /// Two lines or more: one toggle folds them to the first line. Whether they are folded is this device's.
    pub foldable: bool,
    /// What the toggle says while they are folded ("Show all 4 goals").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_all: Option<Text>,
    /// Folded to the first line on this device (`Lens::goals_folded`; the owner's pick of 13 October 2026: one
    /// toggle, folded shows the first line). Only while `foldable`.
    #[serde(default)]
    pub folded: bool,
    /// What the toggle says: "Show all 4 goals" while folded, "Show the first goal only" while open; none while not
    /// foldable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toggle: Option<Text>,
}

/// A session that is cut off, as the Desk says it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlipView {
    pub session_id: String,
    pub text: Text,
    pub fix: FixView,
}

/// One row of the Desk: a question, a request for approval, or something to read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskRowView {
    pub id: String,
    /// "decision", "info" (to read: also a card not read here yet, and one of a newer Trommi), "request".
    pub kind: String,
    /// Its body is not read on this device yet: a quiet placeholder.
    #[serde(default)]
    pub loading: bool,
    /// A card of a newer Trommi: its title where readable, nothing to answer.
    #[serde(default)]
    pub newer: bool,
    pub title: String,
    /// One or two lines under the title: its teaser, else its body as plain words.
    #[serde(default)]
    pub words: String,
    /// Small words before them: "replaces 2 questions", "revised", "Back from Later", why it is urgent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Text>,
    /// "low", "normal", "high", "critical".
    pub urgency: String,
    /// The sign before the title: "block" (blocking, or an approval), "knock" (urgent), "whenever" (low), "".
    #[serde(default)]
    pub sign: String,
    /// What the sign says ("Knock! Blocking").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign_says: Option<Text>,
    #[serde(default)]
    pub starred: bool,
    /// When it was asked.
    pub created_ms: u64,
    /// Who asked; none for a session the list does not show.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<FromView>,
    /// On the first row of a run (rows of one session that follow each other): how many rows it has. Else 0.
    #[serde(default)]
    pub run: u32,
    /// Its pictures and videos, for the small fan beside the title: the first three.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaView>,
    /// What the fan says ("Look at 4 pictures and videos").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_says: Option<Text>,
    /// The answers the row itself offers.
    pub tiles: TilesView,
    /// The ways to put it aside that apply, in order: "later", "reverse", "duck", "shred".
    pub ways: Vec<String>,
}

/// Who a card comes from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FromView {
    pub session_id: String,
    pub name: String,
    /// What draws the session (`SessionRowView::mark`).
    pub mark: String,
    /// Where it stands, for the line above a card's title ("Desk › Main › Helper"): its desk while there are two
    /// or more, its main for a helper, itself.
    pub path: Vec<String>,
    /// Its desk's name, on All desks only: the small chip before the row's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desk: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaView {
    /// The file (`ViewModel.files`, `Action::OpenFile`).
    pub file_id: String,
    /// "image" or "video".
    pub kind: String,
}

/// The answers a row offers without opening the card.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TilesView {
    /// "none" (not read on this device yet: nothing to answer), "read" (What?? and Got it), "pair" (two answers, the lead last), "advised" (the advised answer, and
    /// "+N other ways" that opens the card), "choose" (one tile that opens the card).
    pub kind: String,
    /// The answers of "pair" and "advised", in drawing order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tiles: Vec<TileView>,
    /// "advised": how many other ways there are.
    #[serde(default)]
    pub others: u32,
    /// "advised": "3 other ways: open the card"; "choose": "Choose: 4 options, several".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub says: Option<Text>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileView {
    /// The option's key (`Action::Answer`; "allow" and "deny" of a request: `Action::Verdict`).
    pub key: String,
    /// The words on the tile; empty for a tile that shows its drawing alone.
    pub label: String,
    /// Its drawing: "yes" (the lead: thumb up, or the filled tile) or "no".
    pub thumb: String,
    /// A real yes or no (thumbs); else two named tiles.
    pub thumbs: bool,
    pub lead: bool,
    /// Its agent advises it.
    pub advised: bool,
    /// Its words need the small type.
    pub small: bool,
    /// `label` is the option's short name (its full one fits no tile).
    pub short: bool,
    /// Choosing it settles the card (its agent marked it final).
    pub settles: bool,
    /// The tooltip ("Recommended", "Settles it", or the option's full name); none without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Text>,
    /// What it says to a screen reader ("Ship it (settles it)").
    pub says: Text,
}

/// One session's page: its heading, its conversation, the status lines of its running work, and what stands above
/// the field to write in. Made for the session in view (`Lens::session`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionView {
    pub id: String,
    pub name: Text,
    /// "live", "oncall", "asleep", "cut", "gone"; "" for an archived or removed session.
    pub link: String,
    pub blocked: bool,
    /// Talk rows, newest last.
    pub talk: Vec<TalkRow>,
    pub status_lines: Vec<StatusLineView>,
    /// The hub holds older messages than this device has (history on demand: asking for more reads them).
    pub has_more: bool,
    /// The line above the first row when more stands before it ("Earlier messages"): what this device holds before
    /// the window, and whether the hub holds older still. `talk` is a window of the newest [`board::TALK_PAGE`] rows,
    /// a page more for each time the person asked (`Lens::talk_window`); none when it shows everything there is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earlier: Option<MoreView>,
    /// Its short id on the board (`/chat/<slug>`).
    #[serde(default)]
    pub slug: String,
    /// What draws it (`SessionRowView::mark`).
    #[serde(default)]
    pub mark: String,
    #[serde(default)]
    pub desk_id: String,
    #[serde(default)]
    pub main: bool,
    #[serde(default)]
    pub crowned: bool,
    #[serde(default)]
    pub online: bool,
    /// Where it stands: its desk (while there are two or more), its main (for a helper), itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
    /// The model it runs on, as the heading says it ("Opus 5.5"); "" when it named none.
    #[serde(default)]
    pub model_label: String,
    /// The model's id, for the tooltip.
    #[serde(default)]
    pub model: String,
    /// The dot beside the name.
    #[serde(default)]
    pub dot: DotView,
    /// Why it is stopped (the red hand).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<HandView>,
    /// The quiet hint ("quiet for 24 min"): connected, working, and nothing from it for a while. None while stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet: Option<Text>,
    /// Above the field: why it does not hear and the step in its terminal, or that his last message waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<LinkNoteView>,
    /// What the field says while it is empty.
    #[serde(default)]
    pub compose: Text,
    /// "<name> is working": his word was the last, a short while ago.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answering: Option<Text>,
    /// Its questions that wait for the person (its infos too), and the way to the first of them.
    #[serde(default)]
    pub open: u32,
    /// "3 open".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_word: Option<Text>,
    /// "3 open questions: go to the first one".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_says: Option<Text>,
    /// The talk row of the first of them, where it was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_open: Option<String>,
    /// An empty conversation: the card of the agent that joined, and three ways to start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<EmptyTalkView>,
}

/// The dot beside a session's name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DotView {
    /// Connected (green).
    pub on: bool,
    /// How it hears when it does not hear at once: "oncall", "asleep", "cut", "gone"; else "".
    pub link: String,
    /// The tooltip: "connected", "disconnected", or the link's words.
    pub word: Text,
    /// For a screen reader: the word, and what it is at.
    pub says: Text,
}

/// The note above a field: a receipt, the sentence about the session, the way out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkNoteView {
    /// "cut", "gone", "asleep", "oncall" (the link's state), or "unheard", "sent", "heard" (a receipt alone).
    pub tone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Text>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Text>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixView>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyTalkView {
    /// Where it runs, quietly under its name ("Opus 5.5"); "" when it said nothing.
    pub meta: String,
    pub text: Text,
    /// A tap sends one.
    pub starters: Vec<Text>,
}

/// One row of a conversation. `kind` says what it is; the fields it needs are filled.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalkRow {
    pub id: String,
    pub from_me: bool,
    pub text: String,
    pub time_ms: u64,
    /// His message: its session picked it up (false also when its connector writes no receipts).
    pub heard: bool,
    /// Not confirmed by the hub yet (an echo).
    pub pending: bool,
    /// File ids (`ViewModel.files`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// "user" (his bubble), "agent" (the agent's words, light markdown), "note" (his note, taped on), "event" (one
    /// quiet line: `event`), "ask" (an open question where it was asked: `row`), "work" (a turn's trail: `work`),
    /// "artifact" (something the session published: `published`), "newer" (written by a newer Trommi).
    #[serde(default)]
    pub kind: String,
    /// Its place in the hub's order (an envelope number); 0 for what has none yet.
    #[serde(default)]
    pub number: u64,
    /// The line above it when a new day begins there ("Today", "Yesterday", "Tuesday 13 October").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<Text>,
    /// It goes on a run of the same side: no head, close to the row before.
    #[serde(default)]
    pub cont: bool,
    /// Typed into the session's terminal (his words, sent for him by the agent's connector), or the agent's answer there.
    #[serde(default)]
    pub terminal: bool,
    /// The card it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<CardRefView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventView>,
    /// What he did with a card along with these words (handed back, took back, What??): a quiet line above them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deed: Option<EventView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row: Option<Box<DeskRowView>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<WorkView>,
    /// The Artifact it announces (its object id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
    /// The agent's details, folded under its words.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
    /// The agent's layout under its words (drawn in the sandboxed frame).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    /// The cards it carries, a chip each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chips: Vec<CardRefView>,
    /// A selection of the Scribble Board (its one file is the picture).
    #[serde(default)]
    pub scribble: bool,
    /// A note: when it was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub written_ms: Option<u64>,
    /// On a card's page, the agent's words that answer his What??: that row's id (drawn as one block with it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<String>,
    /// On a card's page, his What?? that has no answer yet: "Waiting for the explanation."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waits: Option<Text>,
}

/// A card as a row of a conversation names it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardRefView {
    pub id: String,
    /// Its number ("Nr. 12"); 0 when this device does not hold it.
    pub number: u32,
    pub title: String,
}

/// One quiet line for something that happened to a card.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventView {
    /// "asked", "info", "revised", "decided", "trusted", "read", "shredded", "reopened", "done", "handed", "snoozed",
    /// "took_back", "handed_back", "explain".
    pub kind: String,
    /// "New question", "Answered", …
    pub label: Text,
    /// The card's title, when the words do not say it already.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// What became of the question is said where it was asked: this line only repeats it.
    #[serde(default)]
    pub echo: bool,
    /// On a card's page: "revised", the version it brought ("Version 3"); "decided", the version he answered when it
    /// was not the newest ("You chose in version 2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// "revised" on a card's page: it came after his hand-back ("Version 3, as you asked").
    #[serde(default)]
    pub asked: bool,
    /// "decided", "trusted", "shredded": what he wrote along, one line each (“his note”, On Label: “a note”).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub said: Vec<Text>,
    /// "decided", "trusted", "shredded": the files he sent along (`ViewModel.files`), shown under the deed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

/// A turn's trail: what the agent did between his prompt and its answer, as one block.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkView {
    pub turn: String,
    /// "running", "done", "interrupted", "failed".
    pub state: String,
    /// "Working", "Worked 2 min", "Interrupted after 12 s", "Stopped by an error after 1 h 5 min".
    pub head: Text,
    /// "7 steps · 1 helper · 2 with errors"; none without any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sum: Option<Text>,
    /// While it runs: the step it is at ("Bash · run the tests").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub now: String,
    pub lines: Vec<WorkLineView>,
    /// "and 12 more steps, not listed".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Text>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkLineView {
    pub id: String,
    /// "step", "helper", or "text" (the agent's words between two steps: `text`).
    pub kind: String,
    /// "running", "ok", "failed", "interrupted"; "" for words.
    #[serde(default)]
    pub state: String,
    /// What its sign says ("done", "exit code 2").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub says: Option<Text>,
    /// The bold word: the tool, "Helper <name>", or "Step".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool: String,
    /// What it is about ("title · subject").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub what: String,
    /// "3 steps" (a helper), "exit code 2" (a failed command).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Text>,
    /// How long it took ("0.3 s", "2 min").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub took: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub input: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub output: String,
}

/// A status line of a session's running work, as its page shows it (core/client/README.md "Status lines"): only
/// the lines that show are here.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusLineView {
    pub id: String,
    pub label: String,
    pub detail: String,
    /// "working", "decision", "later", "done" (for ten minutes), "stale" (greyed and still, for a day).
    pub state: String,
    /// "working", "waits on you", "put off", "done", "no news for a while".
    #[serde(default)]
    pub says: Text,
    /// When its agent last set it.
    #[serde(default)]
    pub updated_ms: u64,
    /// The card it stands for: the one it names, else the card with its session whose line it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<CardRefView>,
}

/// One card's page (a question, something to read, a request for approval): who it comes from, its words, its
/// answers or what was said, its history, and the comments under it. Made for the card in view (`Lens::card`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardView {
    pub id: String,
    pub session_id: Option<String>,
    /// "decision", "info", "request", "note".
    pub kind: String,
    /// "open", "answered", "closed".
    pub state: String,
    /// 0 low, 1 normal, 2 high, 3 critical.
    pub urgency: u8,
    pub title: String,
    /// Its whole text (markdown).
    pub body: String,
    /// Its options, in the tiles' order (the advised last).
    pub options: Vec<OptionView>,
    /// "You chose: Ship it", when he answered (for a screen reader; drawn from `answer`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub you_chose: Option<Text>,
    /// How many versions stood before this one.
    pub history: u32,
    pub starred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<u64>,
    /// File ids (`ViewModel.files`): every attachment that is no page of a picture.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Its number, for its address (`/card/<number>`); 0 when unknown.
    #[serde(default)]
    pub number: u32,
    /// "Nr. 12".
    #[serde(default)]
    pub nr: Text,
    /// Where it stands: "catching" (settled elsewhere, its outcome on the way), "open", "decided", "shredded", "done".
    #[serde(default)]
    pub status: String,
    /// Who it comes from, above its title: "Desk › Main › Helper" (`FromView::path`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<FromView>,
    /// "Knock! Blocking", while it is open and knocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knock: Option<Text>,
    /// The strip above the title while its session works on it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<CardWorkView>,
    /// When it was asked, or last revised.
    #[serde(default)]
    pub asked_ms: u64,
    /// Its place among what waits on the Desk in view ("3 of 7"), with its neighbours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceView>,
    /// Small words of the quiet line: "replaces 2 questions", "revised", "3 messages below".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Text>,
    /// The text that stands on the card (markdown): its paragraphs; code and tables stand in the comments.
    #[serde(default)]
    pub text: String,
    /// The comments hold more of the text ("More in the comments ↓").
    #[serde(default)]
    pub more: bool,
    /// The agent's layout under the text (drawn in the sandboxed frame).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    /// The options' own paragraphs, in the tiles' order; empty when none says more than its label.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<SectionView>,
    /// Several options may be ticked.
    #[serde(default)]
    pub multiple: bool,
    /// The answers' column: what can be done, or what was said.
    #[serde(default)]
    pub answer: AnswerView,
    /// Under the answers: whether its session hears, and whether it has his answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<LinkNoteView>,
    /// What the field under the card says while it is empty.
    #[serde(default)]
    pub ask: Text,
    /// His unsent words (the draft kept with the card).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub draft: String,
    /// The versions that stood before this one, oldest first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<VersionView>,
    /// What the star says ("Star", "Starred").
    #[serde(default)]
    pub star_says: Text,
    /// What the card does not hold, before the comments: the whole text or its layout, the links of its options,
    /// that he put it off.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lead: Vec<LeadView>,
    /// The comments and what happened to the card, oldest first: the window's rows (`earlier`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub talk: Vec<TalkRow>,
    /// The hub holds older comments than `talk` has.
    #[serde(default)]
    pub has_more: bool,
    /// The cards it replaces.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub merged_from: Vec<CardRefView>,
    /// The card that replaced it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<CardRefView>,
    /// Its pictures, in order (the stage and the small ones under it), each with the option it shows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<PictureView>,
    /// The line above the comments when more stand before them ("Load earlier (N)", "Earlier comments"): `talk`
    /// is a window of the newest [`board::COMMENTS_PAGE`] rows, a page more for each time the person asked
    /// (`Lens::comments_window`, `MoreOf::Comments`); none when it shows everything there is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub earlier: Option<MoreView>,
    /// Once it is settled (not open, not catching up): the way on that the page offers right after the person
    /// answered here (the shell shows it then): the next open question of the desk in view, or that there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<AfterView>,
}

/// A picture of a card, and the option it belongs to (the web's `pictureKeys`, `picturesOf`): a section that names
/// it decides; else its file name names one option (by key or label, the longest key winning, two equal ones
/// naming none); else, with as many pictures as options and three or more, they pair in order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PictureView {
    /// The file (`ViewModel.files`).
    pub file_id: String,
    pub name: String,
    /// Its caption, when it says more than its name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    /// The page that belongs to it (an attachment id), shown beside it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// The option it shows: pointing at that option puts it on the stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Every option a section shows it for (several may share one picture); else its `key`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub keys: Vec<String>,
}

/// The way on after an answer on a card's page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AfterView {
    /// The next open question: the next in the Desk's order after this card's place, else the first that waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<CardRefView>,
    /// "Next question", or "That was the last open question."
    pub says: Text,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionView {
    pub key: String,
    pub label: String,
    pub recommended: bool,
    /// He chose it (the answer that stands), or ticked it (his draft, while several may be ticked).
    pub chosen: bool,
    /// Its line of detail, as plain words.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    /// Choosing it settles the card (its agent marked it final).
    #[serde(default)]
    pub settles: bool,
    /// His note on it: unsent (his draft) while the card is open, else as he sent it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The tooltip ("Recommended · Settles it").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<Text>,
}

/// An option's own paragraph (a card written as sections).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionView {
    pub key: String,
    pub label: String,
    pub recommended: bool,
    /// What it says beyond its label (markdown); "" when nothing.
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
}

/// The strip above a card's title while its session works on it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardWorkView {
    pub session_id: String,
    /// What draws the session.
    pub mark: String,
    /// "Fernly Website is working on it".
    pub says: Text,
    /// Its status line ("Launch checklist · 14 of 19 done"), else "Reworking the card".
    pub words: Text,
    /// When that was said.
    pub at_ms: u64,
}

/// A card's place among what waits on the Desk in view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceView {
    pub at: u32,
    pub of: u32,
    /// "3 of 7".
    pub says: Text,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// The answers' column of a card's page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerView {
    /// "options" (the card's options answer; a decision also offers the duck, What?? and the reverse card), "read"
    /// (Got it, What??, the reverse card), "request" (Allow, Deny), "said" (what was said, and the way back),
    /// "revising" (handed back), "working" (an info still worked on), "catching", "loading", "newer".
    pub mode: String,
    /// The one still tile: what was said ("Ship it", "Read", "Shredded"), "In revision", "Being worked on".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Text>,
    /// Under it: "Your note: …", or what the mode means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Text>,
    /// The foot: "You chose", "You handed it back".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub did: Option<Text>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
    /// The way back, when there is one: "reopen" (his answer is taken back: `Action::DecideAgain`), "takeback" (his hand-back is).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub back: String,
    /// His notes on single options, as sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub option_notes: Vec<NotedView>,
    /// "<name> marked this answer as final: nothing follows from it.", when his answer settled the card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled: Option<Text>,
    /// What its agent said when it closed the card ("Done by the agent").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Its agent closed it after his answer: What?? asks about what it did.
    #[serde(default)]
    pub finished: bool,
    /// What the options say to a screen reader ("Your answer. One tap answers."), or the mode's one quiet sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub says: Option<Text>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotedView {
    pub label: String,
    pub note: Text,
}

/// A version of a card that stood before.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionView {
    pub n: u32,
    pub at_ms: u64,
    pub title: String,
    /// What its agent said changed with the version after it; "" without a word.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The version as it was, read only ("See version 2", "Open version 2 as it was"): its text as the card shows it
    /// (markdown), and its options. Empty while its body is not on this device (`loaded`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<OptionView>,
    /// Its body is on this device (history on demand: an old version may be read from the hub when asked for).
    #[serde(default)]
    pub loaded: bool,
    /// Its pictures as they were, in order (`CardView.pictures`' rule, with that version's options).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pictures: Vec<PictureView>,
    /// Its other files (`ViewModel.files`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

/// What the card itself does not hold, at the head of its comments (the web's `cardThread` lead), in this order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeadView {
    /// "whole": the whole text, when the card holds only part of it (`CardView.more`), with the agent's layout;
    /// "with": only the agent's layout, when the card holds all the words; "links": the links written into its
    /// options (a tile says them in plain words); "later": he put it off and it waits on Later.
    pub kind: String,
    /// "The whole text", "With the text", "Links of the options", "You put it off: it waits on “Later”".
    pub label: Text,
    /// "whole": the text (markdown, Artifact links by their titles).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// "whole", "with": the agent's layout (`CardView.html`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub html: String,
    /// "links": each link with the option it is in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<OptionLinkView>,
    /// "later": when he put it off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<u64>,
}

/// A link an agent wrote into an option's label or detail.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionLinkView {
    pub key: String,
    /// The option's label without the link; its key when nothing else is left.
    pub label: String,
    pub url: String,
    /// The address as it is shown: without "https://" and "www.".
    pub short: String,
}

/// The person's notes (the room's, the same on every desk) and the one in the corner (`notes.rs`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotesView {
    /// The ids of the notes that show, the last written first (the ids of `rows`). A sent note held for the toast's
    /// Undo shows nowhere.
    pub notes: Vec<String>,
    /// Those notes, in the same order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rows: Vec<NoteView>,
    /// The note in the corner: the first of them; none when there is none (the corner then makes a new one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corner: Option<String>,
    /// The corner's note holds words or files (the tab's note is drawn written).
    #[serde(default)]
    pub written: bool,
    /// What the corner's sticky says ("Your note: open it", "New note").
    #[serde(default)]
    pub says: Text,
    /// The crowned session it goes to; none when no session wears the crown here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<NoteToView>,
    /// All desks, and it goes to another desk's crown than the desk chosen last: the send names it.
    #[serde(default)]
    pub far: bool,
    /// All desks with two crowned desks or more: the person picks one (each desk's crown, in the desks' order).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choose: Vec<NoteToView>,
    /// What the send says ("Send to Fernly").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub send_says: Option<Text>,
    /// In the send's place without a crown: "Give a session the crown to send" (opens Settings · Agents).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_crown: Option<Text>,
}

/// One note.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteView {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<NoteFileView>,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// This device's version is not confirmed by the hub yet.
    #[serde(default)]
    pub pending: bool,
}

/// A file of a note: a picture as a small thumbnail, any other file by its name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteFileView {
    /// The attachment (`ViewModel.files`, `Action::OpenFile`).
    pub file_id: String,
    pub name: String,
    pub media_type: String,
    pub picture: bool,
}

/// A crowned session a note can go to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteToView {
    pub session_id: String,
    /// Its name; "Main Session" while it has none.
    pub name: String,
    /// What draws it (`SessionRowView::mark`).
    pub mark: String,
    #[serde(default)]
    pub desk_id: String,
    /// Its desk's name ("Personal" for one without), for the choice on All desks.
    #[serde(default)]
    pub desk: String,
}

/// Settings: what the list's rows say, and the person's devices (`settings.rs`). What only this device knows (the
/// theme, the Push level) is the shell's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsView {
    /// The account's e-mail, for the row Account; none until the account is read (W3).
    pub account: Option<String>,
    /// The person's devices that are in, in the order they joined (the agents are under Agents).
    pub devices: Vec<DeviceRow>,
    /// The person's removed devices, in the order they joined (the row "Removed" and its page).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<DeviceRow>,
    /// The devices that never finished signing in: offered for removal together.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unfinished: Vec<String>,
    /// "Remove the 2 devices that never finished signing in".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove_unfinished: Option<Text>,
    /// The row Agents: "2 agents · 5 sessions"; none without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<Text>,
    /// The page Agents: each program on a computer with its sessions, the most sessions first (the web's
    /// `agentsOf`, `agentsMain`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_list: Vec<AgentView>,
    /// The line under the page's title: "2 agents on 1 computer · 5 sessions"; none without an agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents_lead: Option<Text>,
    /// The row Desks: "3 desks"; none in a room without desks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desks: Option<Text>,
    /// The row Devices: "2 devices".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devices_says: Option<Text>,
    /// The room's key epoch (the version line at the list's foot).
    #[serde(default)]
    pub key_epoch: u64,
    /// The account's ways in, once read (`AccountAction::ReadAccount`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<LoginView>,
}

/// The account's ways in, for the page Account: what can be changed and how it is proved.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// The account's id as printed on the kit (a UUID).
    pub account: String,
    pub has_password: bool,
    /// The kit is salted with the e-mail ("email") or with the id ("id").
    pub kit_form: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passkeys: Vec<PasskeyView>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PasskeyView {
    /// base64url: what `RemovePasskey` names.
    pub credential_id: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<u64>,
}

/// An agent: a program on one computer ("Claude Code on MacBook Pro"), with its sessions.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentView {
    /// "Claude Code · MacBook Pro": what Remove names (the action's `key`).
    pub key: String,
    /// "Claude Code", "Codex", "Agent".
    pub program: String,
    /// The computer's name; "" when it named none.
    #[serde(default)]
    pub host: String,
    /// "Claude Code on MacBook Pro", "Codex on an unnamed computer".
    pub name: Text,
    /// One of its sessions is connected.
    #[serde(default)]
    pub online: bool,
    /// "connected", or "away · last seen" (the shell writes the time of `seen_ms` after it).
    pub state: Text,
    /// When one of its sessions last did something.
    #[serde(default)]
    pub seen_ms: u64,
    /// "2 sessions, 1 helper".
    pub count: Text,
    /// Every session of it, its helpers too.
    #[serde(default)]
    pub total: u32,
    /// Its main sessions (a helper whose main is its own counts under that main), in the list's order.
    pub sessions: Vec<AgentSessionView>,
    /// The agent devices Remove takes out of the room (never this device, never a person's).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub devices: Vec<String>,
    /// What Remove does, said before it; none when there is nothing to remove.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove: Option<Text>,
    /// Its account and how much of it is used (the connector's `account`); none without a plan or an e-mail.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageView>,
}

/// A main session of an agent on the page Agents.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSessionView {
    /// Its key (`Screen::Session`).
    pub id: String,
    pub slug: String,
    pub name: Text,
    /// What draws it (`SessionRowView::mark`).
    #[serde(default)]
    pub mark: String,
    /// "Work desk".
    pub desk: Text,
    /// Its folder on the computer; "" when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    #[serde(default)]
    pub helpers: u32,
    /// "+2 helpers"; none without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helpers_says: Option<Text>,
    #[serde(default)]
    pub online: bool,
    /// "connected", "away".
    pub state: Text,
}

/// An agent's account: its plan, e-mail and meters.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageView {
    /// "Claude Max", "ChatGPT Pro Lite"; "" without a plan.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub plan: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
    /// The 5-hour window and the week, those it said; a window that started again since is left out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meters: Vec<MeterView>,
    /// "1,200 credits", "2 full resets" (Codex).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facts: Vec<Text>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeterView {
    /// "five_hour", "weekly".
    pub key: String,
    /// "5 h", "week".
    pub label: String,
    /// "5-hour window", "This week".
    pub long: String,
    /// Percent used, 0 to 100.
    pub used: u32,
    /// At 80 % or more.
    #[serde(default)]
    pub hot: bool,
    /// When it starts again (the shell writes the time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_ms: Option<u64>,
}

/// One device of the person.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRow {
    pub id: String,
    /// Its name; "Device" while it has named itself nothing.
    pub name: String,
    pub this_device: bool,
    /// "human" or "agent".
    #[serde(default)]
    pub role: String,
    /// In the room (not removed).
    #[serde(default)]
    pub active: bool,
    /// The green dot: in, and connected (this device always is).
    #[serde(default)]
    pub online: bool,
    /// "this_device", "unfinished" (never finished signing in), "online", "away", "removed".
    #[serde(default)]
    pub state: String,
    /// The state in words ("this device", "never finished signing in").
    #[serde(default)]
    pub says: Text,
    /// The key fingerprint from the signed member list: it must look the same on every device.
    #[serde(default)]
    pub fingerprint: String,
    /// Its drawing: "phone", "laptop", "terminal".
    #[serde(default)]
    pub icon: String,
    /// When it joined, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_ms: Option<u64>,
}

/// The account's flows as a screen shows them. `step` names the screen; the fields it needs are filled.
/// `Debug` leaves out the invite link and the Kit words: they open the account.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountView {
    /// "none", "sign_in", "found", "join", "compare_emoji", "keep_kit", "invite", "recover", "working".
    pub step: String,
    /// The invite in progress (for `ConfirmEmoji`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite_id: Option<String>,
    /// An invite link to show as text or QR code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// The six emoji to compare, with their words.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emoji: Vec<EmojiView>,
    /// The Emergency Kit words to keep, shown once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kit: Vec<String>,
    /// Self-join asks waiting for "Allow" or "Deny" (for `AnswerAsk`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asks: Vec<AskView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<Text>,
}

impl std::fmt::Debug for AccountView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountView")
            .field("step", &self.step)
            .field("invite_id", &self.invite_id)
            .field("link", &self.link.as_ref().map(|_| "<redacted>"))
            .field("emoji", &self.emoji)
            .field("kit", &format_args!("<{} words redacted>", self.kit.len()))
            .field("asks", &self.asks)
            .field("problem", &self.problem)
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmojiView {
    pub emoji: String,
    pub word: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskView {
    pub nonce: String,
    /// What the vouching agent says it is: account, name, host (its word, shown as such).
    pub says: Text,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileView {
    pub name: String,
    pub media_type: String,
    pub size: u64,
    /// "uploading", "ready", "failed", "missing".
    pub state: String,
    /// The attachment's id once the hub has it (an upload's local id maps to it here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachment_id: Option<String>,
    /// 0..=100 while it moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    /// Its bytes were handed over (`Effect::FileReady`).
    pub loaded: bool,
    /// The Share links this device made for it (`Action::Share`), kept in memory only.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shares: Vec<FileShareView>,
}

/// A Share link of a file: the whole link (its key behind the `#`), for the person to copy.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileShareView {
    pub share_id: String,
    pub link: String,
    pub expires_ms: u64,
}

/// A passing line at the window's foot: what was done, and the way back. The engine and the actions make them; the
/// shell draws them and drops one when its time is up or it is pressed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toast {
    pub id: String,
    /// The bold words ("Note sent to Fernly", "Not saved").
    pub text: Text,
    /// An action the toast offers ("Undo", "Retry"), sent back as that action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<crate::actions::Action>,
    /// The quieter words after them ("the board did not take it").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Text>,
    /// The action's word on its button ("Undo", "Retry"); none: "Undo".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<Text>,
    /// Something went wrong: said at once to a screen reader (the web's role "alert"), drawn as a problem.
    #[serde(default)]
    pub alert: bool,
    /// How long it stands (ms), as the ring on its Undo runs down; none: the shell's own time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// A way to a page ("Open"): the screen, and the link's words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go: Option<ToastGo>,
}

/// A toast's way to a page.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToastGo {
    pub screen: Screen,
    pub label: Text,
}

/// Why a delta could not be folded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutOfOrder {
    pub expected: u64,
    pub got: u64,
}

impl ViewModel {
    /// Folds one delta in. Every shell does exactly this (in its own language, or by calling the core).
    pub fn apply(&mut self, delta: &ViewDelta) -> Result<(), OutOfOrder> {
        let whole = delta
            .parts
            .iter()
            .any(|p| matches!(p, ViewPart::Whole { .. }));
        if !whole && delta.base != self.rev {
            return Err(OutOfOrder {
                expected: self.rev,
                got: delta.base,
            });
        }
        for part in &delta.parts {
            match part {
                ViewPart::Whole { model } => *self = (**model).clone(),
                ViewPart::Phase { phase, status } => {
                    self.phase = *phase;
                    self.status = status.clone();
                }
                ViewPart::Sidebar { view } => self.sidebar = view.clone(),
                ViewPart::Desk { view } => self.desk = view.clone(),
                ViewPart::Session { id, view } => match view {
                    Some(v) => {
                        self.sessions.insert(id.clone(), v.clone());
                    }
                    None => {
                        self.sessions.remove(id);
                    }
                },
                ViewPart::Card { id, view } => match view {
                    Some(v) => {
                        self.cards.insert(id.clone(), (**v).clone());
                    }
                    None => {
                        self.cards.remove(id);
                    }
                },
                ViewPart::Off { view } => self.off = view.clone(),
                ViewPart::Notes { view } => self.notes = view.clone(),
                ViewPart::Settings { view } => self.settings = view.clone(),
                ViewPart::Account { view } => self.account = view.clone(),
                ViewPart::File { id, view } => match view {
                    Some(v) => {
                        self.files.insert(id.clone(), v.clone());
                    }
                    None => {
                        self.files.remove(id);
                    }
                },
                ViewPart::Toasts { toasts } => self.toasts = toasts.clone(),
            }
        }
        self.rev = delta.base + 1;
        Ok(())
    }

    /// The delta that turns `self` (as the shell has it) into `next`, unit by unit. The core keeps the last model
    /// it sent and renders with this: a unit that did not change is not sent.
    pub fn diff(&self, next: &ViewModel) -> Option<ViewDelta> {
        let mut parts = Vec::new();
        if self.phase != next.phase || self.status != next.status {
            parts.push(ViewPart::Phase {
                phase: next.phase,
                status: next.status.clone(),
            });
        }
        if self.sidebar != next.sidebar {
            parts.push(ViewPart::Sidebar {
                view: next.sidebar.clone(),
            });
        }
        if self.desk != next.desk {
            parts.push(ViewPart::Desk {
                view: next.desk.clone(),
            });
        }
        for id in self
            .sessions
            .keys()
            .filter(|k| !next.sessions.contains_key(*k))
        {
            parts.push(ViewPart::Session {
                id: id.clone(),
                view: None,
            });
        }
        for (id, v) in &next.sessions {
            if self.sessions.get(id) != Some(v) {
                parts.push(ViewPart::Session {
                    id: id.clone(),
                    view: Some(v.clone()),
                });
            }
        }
        for id in self.cards.keys().filter(|k| !next.cards.contains_key(*k)) {
            parts.push(ViewPart::Card {
                id: id.clone(),
                view: None,
            });
        }
        for (id, v) in &next.cards {
            if self.cards.get(id) != Some(v) {
                parts.push(ViewPart::Card {
                    id: id.clone(),
                    view: Some(Box::new(v.clone())),
                });
            }
        }
        if self.off != next.off {
            parts.push(ViewPart::Off {
                view: next.off.clone(),
            });
        }
        if self.notes != next.notes {
            parts.push(ViewPart::Notes {
                view: next.notes.clone(),
            });
        }
        if self.settings != next.settings {
            parts.push(ViewPart::Settings {
                view: next.settings.clone(),
            });
        }
        if self.account != next.account {
            parts.push(ViewPart::Account {
                view: next.account.clone(),
            });
        }
        for id in self.files.keys().filter(|k| !next.files.contains_key(*k)) {
            parts.push(ViewPart::File {
                id: id.clone(),
                view: None,
            });
        }
        for (id, v) in &next.files {
            if self.files.get(id) != Some(v) {
                parts.push(ViewPart::File {
                    id: id.clone(),
                    view: Some(v.clone()),
                });
            }
        }
        if self.toasts != next.toasts {
            parts.push(ViewPart::Toasts {
                toasts: next.toasts.clone(),
            });
        }
        (!parts.is_empty()).then_some(ViewDelta {
            base: self.rev,
            parts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_folds_to_the_next_model() {
        let a = ViewModel::default();
        let mut b = a.clone();
        b.phase = Phase::Live;
        b.desk.stack = vec!["c1".into(), "c2".into()];
        b.cards.insert(
            "c1".into(),
            CardView {
                id: "c1".into(),
                ..Default::default()
            },
        );
        let delta = a.diff(&b).unwrap();
        let mut shell = a.clone();
        shell.apply(&delta).unwrap();
        b.rev = 1;
        assert_eq!(shell, b);
        assert_eq!(
            shell.apply(&delta),
            Err(OutOfOrder {
                expected: 1,
                got: 0
            })
        );
        assert!(b.diff(&b).is_none());
    }

    #[test]
    fn row_edits_fold_like_the_desk_ops() {
        let mut list = vec![1, 2, 3];
        Rows::Edits {
            edits: vec![
                RowEdit {
                    at: 1,
                    gone: 1,
                    rows: vec![7, 8],
                },
                RowEdit {
                    at: 9,
                    gone: 9,
                    rows: vec![9],
                },
            ],
        }
        .fold_into(&mut list);
        assert_eq!(list, vec![1, 7, 8, 3, 9]);
    }
}
