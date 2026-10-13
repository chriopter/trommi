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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarView {
    pub desks: Vec<DeskTab>,
    pub sessions: Vec<SessionRowView>,
    /// The number on the app icon and the tab.
    pub badge: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskTab {
    pub id: String,
    pub name: Text,
    pub waiting: u32,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRowView {
    pub id: String,
    pub name: Text,
    /// "live", "oncall", "asleep", "cut", "gone" (core/client/src/status.rs).
    pub link: String,
    /// The red hand.
    pub blocked: bool,
    pub unheard: u32,
    pub depth: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeskView {
    /// Card and request ids, in the Desk's order.
    pub stack: Vec<String>,
    pub off_your_mind: Vec<String>,
    pub later: Vec<String>,
    pub done: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<Text>,
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
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
