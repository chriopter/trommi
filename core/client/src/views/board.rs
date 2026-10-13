//! The board as the views read it, worked out once per render from the board model: the sessions with their desk
//! and crown, the desks, the desk in view, the open questions and where they wait, the answers a session has not
//! picked up. Every screen's unit is made from this, so that the sidebar, the Desk, a session and a card never
//! disagree. The rules are the web app's `boardModel` of before (app/web/public/app.mjs).
//!
//! What is decided elsewhere in this crate is taken, not derived again: the session list with each session's link,
//! red hand and status lines (`sessions.rs`, `status.rs`), the Desk's stack and its order (`desk.rs`, the model's
//! `stack`, `open_permission_ids`, `desk_order`).

use super::words::earliest;
use crate::model::types::{Card, PermissionRequest};
use crate::model::Model;
use crate::sessions::{desks_in_order, session_list, SessionRow};
use serde_json::Value;
use std::cell::Cell;
use std::collections::HashMap;

/// The desk id that stands for every desk at once. Nothing is stored for it: it is this device's choice.
pub const ALL_DESKS: &str = "all";
/// The desk a room without desks shows in its menu.
pub const DEFAULT_DESK: &str = "main";
/// An answer its session has not picked up for this long is worth a word.
pub const UNHEARD_MS: u64 = 2 * 60_000;

/// What this device alone chooses and knows: none of it is the room's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lens {
    /// The desk in view: a desk's id, [`ALL_DESKS`], or none (the first desk).
    pub desk: Option<String>,
    /// The single desk chosen last: on All desks the note and the Scribble Board belong to it.
    pub home_desk: Option<String>,
    /// Minutes east of UTC (`Config.utc_offset_min`): a time of day in a sentence is the person's.
    pub utc_offset_min: i32,
    /// The room is still being read on this device (a sign-in, the past read again).
    pub reading: bool,
    /// How far the reading of the room's past is: done, of how many.
    pub reading_progress: Option<(u64, u64)>,
    /// Picks the Desk's greeting, once per run of the app (from `Config.seed`).
    pub dice: u32,
}

/// A desk of the room.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeskFacts {
    pub id: String,
    pub name: String,
    pub goals: String,
    /// The desk's own crown, when its register names one (`Some(Null)`: nobody on this desk).
    pub crown: Option<Value>,
}

/// A session of the list, with what the views add to the core's row.
#[derive(Clone, Debug)]
pub struct Agent {
    pub row: SessionRow,
    /// The desk it stands on: its own when that desk is there, else the first; none in a room without desks.
    pub desk: Option<String>,
    /// The crowned session of its desk (one per desk, never one for all).
    pub crowned: bool,
    /// It stands on the desk in view (archived or not).
    pub on_desk: bool,
    /// Its device says platform "codex".
    pub codex: bool,
    /// Its connector said a last word before its stream closed.
    pub said_goodbye: bool,
}

/// An open question: a card or an approval request that waits for the person on the Desk now (no info, nothing
/// that is with its agent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fresh {
    pub id: String,
    /// Its session among [`Board::agents`]; none for a session the list does not show.
    pub agent: Option<usize>,
    /// Urgent, blocking, or an approval request.
    pub knocks: bool,
    pub permission: bool,
}

pub struct Board<'a> {
    pub model: &'a Model,
    pub lens: &'a Lens,
    pub now: u64,
    /// Every session of the list, in its order (archived ones too).
    pub agents: Vec<Agent>,
    /// By session key.
    pub by_key: HashMap<String, usize>,
    /// By the board's id of a session (`SessionRow::id`).
    pub by_id: HashMap<String, usize>,
    /// The desks in the menu's order; empty in a room without desks.
    pub desks: Vec<DeskFacts>,
    /// The desk in view: an id of `desks`, [`ALL_DESKS`], or none in a room without desks.
    pub desk: Option<String>,
    /// Every desk at once.
    pub all: bool,
    /// The open questions of the whole board, in the stack's order, the approval requests last.
    pub all_fresh: Vec<Fresh>,
    /// How many of his answers each session has not picked up for [`UNHEARD_MS`], by index of `agents`.
    pub unheard: HashMap<usize, u32>,
    wake: Cell<Option<u64>>,
}

/// A card that reads as an info: one that is, one of a newer Trommi, and one whose body is not read here yet (a
/// quiet placeholder: nothing to answer and no knock until it is read).
pub fn reads_as_info(c: &Card) -> bool {
    c.field("card_type").as_str() == Some("info") || c.is_unsupported() || is_loading(c)
}

/// Open, and its body is not read on this device yet.
pub fn is_loading(c: &Card) -> bool {
    !c.is_unsupported() && matches!(c.content_state.as_str(), "undecryptable" | "header")
}

/// The session a card or request belongs to: its session, else its agent device.
pub fn key_of<'c>(session: &'c Option<String>, agent: &'c str) -> &'c str {
    session.as_deref().unwrap_or(agent)
}

fn request_is_read(p: &PermissionRequest) -> bool {
    p.content_state.is_empty() || p.content_state == "ok"
}

impl<'a> Board<'a> {
    pub fn new(model: &'a Model, lens: &'a Lens, now: u64) -> Self {
        let desks: Vec<DeskFacts> = desks_in_order(model)
            .into_iter()
            .map(|(id, name)| {
                let v = model.human.get(&format!("desk/{id}")).map(|r| &r.value);
                DeskFacts {
                    goals: v.and_then(|v| v.get("goals")).and_then(Value::as_str).map(clean_goals).unwrap_or_default(),
                    crown: v.and_then(|v| v.get("crown")).cloned(),
                    id,
                    name,
                }
            })
            .collect();
        let has = |id: &str| desks.iter().any(|d| d.id == id);
        let all = desks.len() > 1 && lens.desk.as_deref() == Some(ALL_DESKS);
        let desk: Option<String> = if all {
            Some(ALL_DESKS.into())
        } else {
            desks.first().map(|first| match lens.desk.as_deref() {
                Some(d) if has(d) => d.to_string(),
                _ => first.id.clone(),
            })
        };
        let rows = session_list(model, lens.reading);
        let agents: Vec<Agent> = rows
            .into_iter()
            .map(|row| {
                let own = desks.first().map(|first| if has(&row.desk) { row.desk.clone() } else { first.id.clone() });
                // The crown: a desk's own (its register `desk/<id>`, `crown`) decides on that desk; a desk without
                // one keeps the room's single crown, if that session stands on it.
                let crowned = match own.as_ref().and_then(|d| desks.iter().find(|x| &x.id == d)).and_then(|d| d.crown.as_ref()) {
                    None => row.starred,
                    Some(c) => match c.get("session_id").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                        Some(s) => row.session_id.as_deref() == Some(s),
                        None => {
                            let dev = c.get("agent_device_id").and_then(Value::as_str).filter(|s| !s.is_empty());
                            dev.is_some() && dev == row.agent_device_id.as_deref() && row.parent.is_none()
                        }
                    },
                };
                let session = model.sessions.get(&row.key);
                Agent {
                    on_desk: desks.is_empty() || all || own == desk,
                    codex: row
                        .agent_device_id
                        .as_deref()
                        .and_then(|d| model.members.get(d))
                        .is_some_and(|d| d.platform.as_str() == Some("codex")),
                    said_goodbye: session.and_then(|s| s.link.as_ref()).is_some_and(|l| l.exit.is_some()),
                    desk: own,
                    crowned,
                    row,
                }
            })
            .collect();
        let by_key: HashMap<String, usize> = agents.iter().enumerate().map(|(i, a)| (a.row.key.clone(), i)).collect();
        let by_id: HashMap<String, usize> = agents.iter().enumerate().map(|(i, a)| (a.row.id.clone(), i)).collect();

        let mut all_fresh = Vec::new();
        for c in model.stack.iter().filter_map(|id| model.cards.get(id)) {
            if c.object_state != "open" || c.catching_up || c.in_revision.is_some() || reads_as_info(c) {
                continue;
            }
            all_fresh.push(Fresh {
                id: c.object_id.clone(),
                agent: by_key.get(key_of(&c.session_id, &c.agent_device_id)).copied(),
                knocks: matches!(c.urgency.as_str(), "high" | "critical"),
                permission: false,
            });
        }
        // (a request not read here yet says nothing of what it asks to run: it waits quietly, as a card does)
        for p in model.open_permission_ids.iter().filter_map(|id| model.permissions.get(id)) {
            if p.permission_state != "pending" || p.catching_up || !request_is_read(p) {
                continue;
            }
            all_fresh.push(Fresh {
                id: p.object_id.clone(),
                agent: by_key.get(key_of(&p.session_id, &p.agent_device_id)).copied(),
                knocks: true,
                permission: true,
            });
        }

        let mut board = Board {
            model,
            lens,
            now,
            agents,
            by_key,
            by_id,
            desks,
            desk,
            all,
            all_fresh,
            unheard: HashMap::new(),
            wake: Cell::new(None),
        };
        board.unheard = board.count_unheard();
        board
    }

    /// Whether what belongs to this session shows on the desk in view. A session the list does not show counts as
    /// standing on the first desk.
    pub fn in_view(&self, agent: Option<usize>) -> bool {
        match agent {
            Some(i) => self.agents[i].on_desk,
            None => self.desks.is_empty() || self.all || self.desk.as_deref() == self.desks.first().map(|d| d.id.as_str()),
        }
    }

    /// The desk a card of this session counts on, in a room with desks.
    pub fn desk_of(&self, agent: Option<usize>) -> Option<&str> {
        match agent {
            Some(i) => self.agents[i].desk.as_deref(),
            None => self.desks.first().map(|d| d.id.as_str()),
        }
    }

    /// The open questions of the desk in view.
    pub fn fresh(&self) -> impl Iterator<Item = &Fresh> {
        self.all_fresh.iter().filter(|f| self.in_view(f.agent))
    }

    /// The screen is to be made again at `at` by the clock alone (a count of minutes, an answer that turns late).
    pub fn wake_at(&self, at: Option<u64>) {
        self.wake.set(earliest(self.wake.get(), at));
    }

    /// When any unit made from this board so far changes by the clock alone.
    pub fn wake(&self) -> Option<u64> {
        self.wake.get()
    }

    /// Whether the card's session has the person's last word on it (his answer, or the hand back): none when there
    /// is nothing to hear. `heard`: none when its connector writes no receipts.
    pub fn heard_of(&self, c: &Card) -> Option<Heard> {
        if c.catching_up {
            return None;
        }
        let a = c.answer.as_ref();
        let open = c.object_state == "open";
        let decided = c.closed_how.as_deref() == Some("answered") && c.object_state == "answered";
        // (when he said it: the hand back's message, or his answer)
        let at = if open {
            let r = c.in_revision.as_ref()?;
            self.model
                .timelines
                .get(&c.timeline_key)
                .and_then(|t| t.items.get(&crate::model::types::ItemKey::Number(r.envelope_number)))
                .and_then(|i| i.get("sent_at"))
                .and_then(Value::as_u64)
                .unwrap_or(c.updated_at)
        } else if decided {
            a?.answered_at
        } else {
            return None;
        };
        if at == 0 || a.is_some_and(|a| a.pending) {
            return None;
        }
        let said = if open { c.in_revision.as_ref().map(|r| r.envelope_number) } else { a.and_then(|a| a.envelope_number) };
        let mark = self.model.sessions.get(key_of(&c.session_id, &c.agent_device_id)).and_then(|s| s.heard_up_to);
        let heard = match (said, mark) {
            (Some(said), Some(mark)) => Some(said <= mark),
            _ => None,
        };
        let waiting = if heard == Some(true) { 0 } else { self.now.saturating_sub(at) };
        Some(Heard {
            heard,
            waiting,
            late: heard == Some(false) && waiting >= UNHEARD_MS,
            late_at: (heard == Some(false) && waiting < UNHEARD_MS).then_some(at + UNHEARD_MS),
        })
    }

    fn count_unheard(&self) -> HashMap<usize, u32> {
        let mut out: HashMap<usize, u32> = HashMap::new();
        for c in self.model.cards.values() {
            let agent = self.by_key.get(key_of(&c.session_id, &c.agent_device_id)).copied();
            if !self.in_view(agent) {
                continue;
            }
            let Some(h) = self.heard_of(c) else { continue };
            self.wake_at(h.late_at);
            if let (true, Some(i)) = (h.late, agent) {
                *out.entry(i).or_default() += 1;
            }
        }
        out
    }
}

/// Whether a session picked up the person's last word on a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heard {
    pub heard: Option<bool>,
    /// How long his word has waited, 0 once it was heard.
    pub waiting: u64,
    /// Not heard for [`UNHEARD_MS`].
    pub late: bool,
    /// When it turns late, if it has not yet.
    pub late_at: Option<u64>,
}

pub const GOALS_LINES: usize = 20;
pub const GOALS_LINE_MAX: usize = 200;

/// A desk's goals as they are kept: at most [`GOALS_LINES`] lines of at most [`GOALS_LINE_MAX`] characters (UTF-16
/// units, never half a character), no blank lines at either end, no trailing spaces.
pub fn clean_goals(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<String> = unified
        .split('\n')
        .map(|l| {
            let l = l.trim_end();
            let mut units = 0;
            l.chars().take_while(|c| { units += c.len_utf16(); units <= GOALS_LINE_MAX }).collect()
        })
        .collect();
    let joined = lines.join("\n");
    joined.trim_matches('\n').split('\n').take(GOALS_LINES).collect::<Vec<_>>().join("\n")
}
