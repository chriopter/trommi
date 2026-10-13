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
//! waits where), and each screen's module makes its unit from it: [`sidebar`], [`desk`]. The rules are the web app's of
//! before (`app/web/public/app.mjs` `boardModel`), now here and nowhere else; [`words`] holds every sentence.
//! What this device alone chooses (the desk in view, the zone) comes in as a [`Lens`].

pub mod board;
pub mod demo;
pub mod desk;
pub mod sidebar;
#[cfg(test)]
mod testroom;
pub mod words;

pub use board::{render, Board, Lens};

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
    /// The number on the app icon and the tab: the open questions of every desk (no infos, nothing that is with
    /// its agent).
    pub badge: u32,
    /// One of the counted questions knocks (urgent, blocking, an approval): the number shows red.
    #[serde(default)]
    pub knocks: bool,
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
    /// (Placeholders until the two docked stacks land: W4 fills them from the core's `off_mind`.)
    pub off_your_mind: Vec<String>,
    pub later: Vec<String>,
    pub done: Vec<String>,
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
    /// "read" (What?? and Got it), "pair" (two answers, the lead last), "advised" (the advised answer, and
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionView {
    pub id: String,
    pub name: Text,
    pub link: String,
    pub blocked: bool,
    /// Talk rows, newest last.
    pub talk: Vec<TalkRow>,
    pub status_lines: Vec<StatusLineView>,
    pub has_more: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TalkRow {
    pub id: String,
    pub from_me: bool,
    pub text: String,
    pub time_ms: u64,
    pub heard: bool,
    /// Not confirmed by the hub yet (an echo).
    pub pending: bool,
    /// File ids (`ViewModel.files`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusLineView {
    pub id: String,
    pub label: String,
    pub detail: String,
    /// "decision", "working", "done", "later" (core/client/README.md "Status lines").
    pub state: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardView {
    pub id: String,
    pub session_id: Option<String>,
    /// "decision", "info", "request", "note".
    pub kind: String,
    /// "open", "answered", "closed".
    pub state: String,
    pub urgency: u8,
    pub title: String,
    pub body: String,
    pub options: Vec<OptionView>,
    /// "You chose …", when answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub you_chose: Option<Text>,
    pub history: u32,
    pub starred: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snoozed_until: Option<u64>,
    /// File ids (`ViewModel.files`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionView {
    pub key: String,
    pub label: String,
    pub recommended: bool,
    pub chosen: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotesView {
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsView {
    pub account: Option<String>,
    pub devices: Vec<DeviceRow>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRow {
    pub id: String,
    pub name: String,
    pub this_device: bool,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Toast {
    pub id: String,
    pub text: Text,
    /// An action the toast offers ("Undo", "Retry"), sent back as that action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<crate::actions::Action>,
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
