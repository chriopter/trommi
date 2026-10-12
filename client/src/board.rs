//! The board: every card and permission request this device knows, as plain facts the protocol core accepted.
//!
//! A host turns each `ReceivedEnvelope` of a Version, Answer, TakeBack, Request or Verdict into one [`Fact`] and
//! names where it came from ([`Source`]). The board keeps, per object, the newest state it knows and the newest
//! version it read, and enforces the one rule of reading newest to oldest: **what history brings may only close**.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What an object is. Notes and Artifacts never stand on the Desk and are not kept here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Card,
    Request,
}

/// The state an envelope gives its object (spec/v1.md 9.2.1; the header's byte 1, 2, 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Open,
    Answered,
    Closed,
}

impl State {
    /// From the header's byte; `None` for any other.
    pub fn from_byte(byte: u8) -> Option<State> {
        match byte {
            1 => Some(State::Open),
            2 => Some(State::Answered),
            3 => Some(State::Closed),
            _ => None,
        }
    }
}

/// Where a fact came from, which decides what it may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The current version the hub's Desk lists, read out of the hub's order: the newest there is.
    Desk,
    /// In the hub's order at its head (the live stream, a catch-up above the Desk's cursor, an own write back).
    Live,
    /// Older history, read in the background after the Desk was complete: it may only close.
    History,
}

/// One envelope's effect on one object, as the protocol core accepted it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    /// The object id, lowercase hex.
    pub object_id: String,
    pub kind: Kind,
    /// The session the object lies in, lowercase hex.
    #[serde(default)]
    pub session_id: Option<String>,
    /// The hub's change number of this envelope.
    pub change: u64,
    /// The state the envelope gives the object (from its header: readable without the body).
    pub state: State,
    /// This envelope is a version of the object (a card's version, a request): its body is the object's body.
    #[serde(default)]
    pub is_version: bool,
    /// The opened body (JSON) of a version; `None` when it did not open here (no key yet).
    #[serde(default)]
    pub body: Option<serde_json::Value>,
    /// The sender's clock (header time), ms.
    #[serde(default)]
    pub sent_at: u64,
    /// A request's deadline from its bind, ms; 0 when unknown.
    #[serde(default)]
    pub expires_at: u64,
    /// The agent handed the card back to the human at this change (a `present` in its Chat).
    #[serde(default)]
    pub hand_back: Option<u64>,
    pub source: Source,
}

/// What the board holds of one object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub object_id: String,
    pub kind: Kind,
    pub session_id: Option<String>,
    pub state: State,
    /// The change of the envelope that gave the current state.
    pub state_change: u64,
    /// The change of the newest version known (its header, if not its body).
    pub version_change: u64,
    /// The body of that version, once it opened here.
    pub body: Option<serde_json::Value>,
    pub sent_at: u64,
    pub expires_at: u64,
    pub hand_back: Option<u64>,
}

impl Object {
    /// Its body was read here (a placeholder until then).
    pub fn is_read(&self) -> bool {
        self.body.is_some()
    }
}

/// What applying a fact did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applied {
    /// The object is new to the board.
    Added,
    /// Its state or version moved.
    Changed,
    /// Only the body of the version known arrived.
    Filled,
    /// History closed (or answered) it.
    Closed,
    /// Nothing changed: older, the same, or history that would have opened or replaced.
    Ignored,
}

/// Every card and permission request this device knows, by object id.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Board {
    pub objects: BTreeMap<String, Object>,
}

impl Board {
    pub fn new() -> Board {
        Board::default()
    }

    pub fn get(&self, object_id: &str) -> Option<&Object> {
        self.objects.get(object_id)
    }

    /// Takes one fact. The rules:
    /// - Desk and Live facts are the newest there is: the higher change wins, whatever the state.
    /// - History may only close: it moves Open to Answered or Closed and Answered to Closed when it is not older
    ///   than the state known, it fills the body of the version known, and it never opens, never adds an open
    ///   object, never replaces a newer version.
    pub fn apply(&mut self, fact: &Fact) -> Applied {
        let Some(object) = self.objects.get_mut(&fact.object_id) else {
            if fact.source == Source::History && fact.state == State::Open {
                // an old question read again is never put on anyone's Desk by history
                return Applied::Ignored;
            }
            self.objects
                .insert(fact.object_id.clone(), Self::fresh(fact));
            return Applied::Added;
        };
        if object.session_id.is_none() {
            object.session_id.clone_from(&fact.session_id);
        }
        match fact.source {
            Source::Desk | Source::Live => {
                let mut changed = false;
                if fact.change > object.state_change {
                    object.state = fact.state;
                    object.state_change = fact.change;
                    changed = true;
                }
                if fact.is_version && fact.change > object.version_change {
                    object.version_change = fact.change;
                    object.body.clone_from(&fact.body);
                    object.sent_at = fact.sent_at;
                    if fact.expires_at != 0 {
                        object.expires_at = fact.expires_at;
                    }
                    changed = true;
                } else if fact.is_version
                    && fact.change == object.version_change
                    && object.body.is_none()
                    && fact.body.is_some()
                {
                    object.body.clone_from(&fact.body);
                    if fact.expires_at != 0 {
                        object.expires_at = fact.expires_at;
                    }
                    if !changed {
                        return Applied::Filled;
                    }
                }
                if let Some(at) = fact.hand_back {
                    if object.hand_back.is_none_or(|h| at > h) {
                        object.hand_back = Some(at);
                        changed = true;
                    }
                }
                if changed {
                    Applied::Changed
                } else {
                    Applied::Ignored
                }
            }
            Source::History => {
                if fact.is_version
                    && fact.change == object.version_change
                    && object.body.is_none()
                    && fact.body.is_some()
                {
                    object.body.clone_from(&fact.body);
                    if fact.expires_at != 0 {
                        object.expires_at = fact.expires_at;
                    }
                    return Applied::Filled;
                }
                if fact.state > object.state && fact.change >= object.state_change {
                    object.state = fact.state;
                    object.state_change = fact.change;
                    return Applied::Closed;
                }
                Applied::Ignored
            }
        }
    }

    fn fresh(fact: &Fact) -> Object {
        Object {
            object_id: fact.object_id.clone(),
            kind: fact.kind,
            session_id: fact.session_id.clone(),
            state: fact.state,
            state_change: fact.change,
            version_change: if fact.is_version { fact.change } else { 0 },
            body: if fact.is_version {
                fact.body.clone()
            } else {
                None
            },
            sent_at: fact.sent_at,
            expires_at: fact.expires_at,
            hand_back: fact.hand_back,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fact(id: &str, change: u64, state: State, source: Source) -> Fact {
        Fact {
            object_id: id.into(),
            kind: Kind::Card,
            session_id: Some("s1".into()),
            change,
            state,
            is_version: state == State::Open,
            body: Some(serde_json::json!({ "title": format!("card {id} at {change}") })),
            sent_at: change * 1000,
            expires_at: 0,
            hand_back: None,
            source,
        }
    }

    #[test]
    fn history_never_opens_or_replaces() {
        let mut board = Board::new();
        assert_eq!(
            board.apply(&fact("a", 10, State::Open, Source::History)),
            Applied::Ignored
        );
        assert!(board.get("a").is_none());
        board.apply(&fact("a", 50, State::Open, Source::Desk));
        // an older version read in the background does not replace the newer body
        assert_eq!(
            board.apply(&fact("a", 20, State::Open, Source::History)),
            Applied::Ignored
        );
        assert_eq!(board.get("a").unwrap().version_change, 50);
    }

    #[test]
    fn history_closes_only_forward() {
        let mut board = Board::new();
        board.apply(&fact("a", 50, State::Open, Source::Desk));
        // an answer older than the version known is the answer to an older version: it closes nothing
        assert_eq!(
            board.apply(&fact("a", 40, State::Closed, Source::History)),
            Applied::Ignored
        );
        assert_eq!(
            board.apply(&fact("a", 60, State::Answered, Source::History)),
            Applied::Closed
        );
        assert_eq!(board.get("a").unwrap().state, State::Answered);
        // and never back to open
        assert_eq!(
            board.apply(&fact("a", 70, State::Open, Source::History)),
            Applied::Ignored
        );
        assert_eq!(
            board.apply(&fact("a", 80, State::Closed, Source::History)),
            Applied::Closed
        );
    }

    #[test]
    fn live_is_newest_wins() {
        let mut board = Board::new();
        board.apply(&fact("a", 50, State::Open, Source::Desk));
        board.apply(&fact("a", 60, State::Answered, Source::Live));
        assert_eq!(
            board.apply(&fact("a", 70, State::Open, Source::Live)),
            Applied::Changed
        );
        assert_eq!(board.get("a").unwrap().state, State::Open);
        assert_eq!(
            board.apply(&fact("a", 65, State::Closed, Source::Live)),
            Applied::Ignored
        );
    }

    #[test]
    fn a_header_first_then_its_body() {
        let mut board = Board::new();
        let mut f = fact("a", 50, State::Open, Source::Desk);
        f.body = None;
        board.apply(&f);
        assert!(!board.get("a").unwrap().is_read());
        let mut again = fact("a", 50, State::Open, Source::History);
        again.expires_at = 9;
        assert_eq!(board.apply(&again), Applied::Filled);
        assert!(board.get("a").unwrap().is_read());
    }
}
