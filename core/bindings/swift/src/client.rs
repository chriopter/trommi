//! The client core (`trommi-client-core`): what every app decides the same way, above the protocol. Here: the
//! Desk (core/client/src/desk.rs), as one object per device, `ClientDesk`, that the app tells what changed and asks what
//! the Desk is now. Plain records cross the edge, no JSON: a card that came costs one small call each way.

use crate::CoreError;
use std::sync::Mutex;
use trommi_client_core::desk::{DeskCard, DeskRequest, DeskState, Listed, Listing};
use trommi_client_core::{Kind, State};

record! {
    /// A card as the Desk needs it (core/client/src/desk.rs `DeskCard`). `state`: 1 open, 2 answered, 3 closed.
    /// `urgency`: 3 critical, 2 high, 1 normal, 0 low.
    pub struct DeskCardIn {
        pub object_id: String,
        pub session_id: Option<String>,
        pub state: u8,
        pub urgency: u8,
        pub created_at: u64,
        pub agent_device_id: String,
        /// The change of its newest version known here.
        pub version_change: u64,
        /// The newest change of it known here (a version, an answer, a take back).
        pub newest: u64,
        /// The change of the hand back (or What??) in force.
        pub in_revision: Option<u64>,
        /// Its newest version's body was read here.
        pub read: bool,
        /// An info its agent is still working on (spec 9.1.3): on neither the stack nor its session's list.
        pub working: bool,
    }
}

record! {
    /// A permission request as the Desk needs it (core/client/src/desk.rs `DeskRequest`).
    pub struct DeskRequestIn {
        pub object_id: String,
        pub session_id: Option<String>,
        pub pending: bool,
        pub envelope_number: u64,
        pub newest: u64,
        pub sent_at: u64,
        /// Its deadline; 0 when not known (its body not read).
        pub expires_at: u64,
        pub read: bool,
    }
}

record! {
    /// One object the hub's Desk lists as open, with the change of its current version.
    pub struct DeskListed {
        pub object_id: String,
        pub version_change: u64,
    }
}

record! {
    /// One edit of the stack: at `at`, `gone` ids leave and `ids` come.
    pub struct DeskStackOp {
        pub at: u32,
        pub gone: u32,
        pub ids: Vec<String>,
    }
}

record! {
    /// A session's open cards (not held), by the time they were asked; empty: none any more.
    pub struct DeskSessionOpen {
        pub session_id: String,
        pub card_ids: Vec<String>,
    }
}

record! {
    /// One edit of a session's list of open cards: at `at`, `gone` ids leave and `ids` come.
    pub struct DeskSessionOp {
        pub session_id: String,
        pub at: u32,
        pub gone: u32,
        pub ids: Vec<String>,
    }
}

record! {
    /// What changed of the Desk since the last answer (core/client/src/desk.rs `Delta`). `whole`: everything is in it,
    /// `stack` is the whole stack and `held` every held object; otherwise `stack_ops` edit the last stack in order,
    /// `session_ops` edit the sessions' lists the same way (`session_open`: every list, whole only), `held`/`unheld`
    /// name what changed, `permissions_open` is there when it changed. `expired`: requests whose time ran out (the app marks them); `hand_back_over`: hand backs
    /// older than the version the hub lists (the app ends them); `next_wake`: when to ask again by the clock.
    pub struct DeskDelta {
        pub whole: bool,
        pub stack: Vec<String>,
        pub stack_ops: Vec<DeskStackOp>,
        pub permissions_open: Option<Vec<String>>,
        pub held: Vec<String>,
        pub unheld: Vec<String>,
        pub expired: Vec<String>,
        pub hand_back_over: Vec<String>,
        pub session_open: Vec<DeskSessionOpen>,
        pub session_ops: Vec<DeskSessionOp>,
        pub next_wake: u64,
    }
}

fn state_of(byte: u8) -> Result<State, CoreError> {
    State::from_byte(byte).ok_or_else(|| CoreError::bad_format("a card's state is 1, 2 or 3"))
}

/// The Desk of one device, kept up to date by its app: the app tells it what changed, and asks it what the Desk is.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct ClientDesk(Mutex<DeskState>);

impl ClientDesk {
    fn with<T>(
        &self,
        f: impl FnOnce(&mut DeskState) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut d = self
            .0
            .lock()
            .map_err(|_| CoreError::internal("the Desk failed before"))?;
        f(&mut d)
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl ClientDesk {
    /// An empty Desk.
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Mutex::new(DeskState::new()))
    }

    /// Cards that are new or changed.
    pub fn put_cards(&self, cards: Vec<DeskCardIn>) -> Result<(), CoreError> {
        self.with(|d| {
            for c in cards {
                let urgency = ["low", "normal", "high", "critical"]
                    .get(c.urgency as usize)
                    .map(|s| s.to_string());
                d.put_card(DeskCard {
                    object_id: c.object_id,
                    session_id: c.session_id,
                    state: state_of(c.state)?,
                    urgency,
                    created_at: c.created_at,
                    agent_device_id: Some(c.agent_device_id),
                    version_change: c.version_change,
                    newest: c.newest,
                    in_revision: c.in_revision,
                    read: c.read,
                    working: c.working,
                });
            }
            Ok(())
        })
    }

    /// Permission requests that are new or changed.
    pub fn put_requests(&self, requests: Vec<DeskRequestIn>) -> Result<(), CoreError> {
        self.with(|d| {
            for r in requests {
                d.put_request(DeskRequest {
                    object_id: r.object_id,
                    session_id: r.session_id,
                    pending: r.pending,
                    envelope_number: r.envelope_number,
                    newest: r.newest,
                    sent_at: r.sent_at,
                    expires_at: r.expires_at,
                    read: r.read,
                });
            }
            Ok(())
        })
    }

    /// Cards and requests that are gone.
    pub fn remove(&self, object_ids: Vec<String>) -> Result<(), CoreError> {
        self.with(|d| {
            for id in object_ids {
                d.remove(&id);
            }
            Ok(())
        })
    }

    /// Forgets every card and request (a model built again from nothing).
    pub fn clear(&self) -> Result<(), CoreError> {
        self.with(|d| {
            d.clear();
            Ok(())
        })
    }

    /// A snooze register `snooze/<card>` as its JSON value (none: deleted), written at change `at` (one not taken
    /// yet: any number above every change). The card may be named in hex or in base64url.
    pub fn set_snooze(
        &self,
        card: String,
        value: Option<String>,
        at: u64,
    ) -> Result<(), CoreError> {
        let value = match value {
            Some(text) => serde_json::from_str(&text)
                .map_err(|_| CoreError::bad_format("a snooze is JSON"))?,
            None => serde_json::Value::Null,
        };
        self.with(|d| {
            d.set_snooze(&card, value, at);
            Ok(())
        })
    }

    /// The archived sessions, all of them.
    pub fn set_archived(&self, session_ids: Vec<String>) -> Result<(), CoreError> {
        self.with(|d| {
            d.set_archived(session_ids);
            Ok(())
        })
    }

    /// The hub's Desk as last read: what it lists as open, at which change.
    pub fn set_listing(&self, at: u64, listed: Vec<DeskListed>) -> Result<(), CoreError> {
        let open = listed
            .into_iter()
            .map(|l| Listed {
                object_id: l.object_id,
                kind: Kind::Card,
                session_id: None,
                version_change: l.version_change,
                first_change: 0,
            })
            .collect();
        self.with(|d| {
            d.set_listing(Some(Listing {
                at,
                truncated: false,
                open,
            }));
            Ok(())
        })
    }

    /// No list of the hub: the device's own view stands.
    pub fn forget_listing(&self) -> Result<(), CoreError> {
        self.with(|d| {
            d.set_listing(None);
            Ok(())
        })
    }

    /// The app waits for the hub's first list of a run: every open card and request is held meanwhile.
    pub fn set_hold_all(&self, hold: bool) -> Result<(), CoreError> {
        self.with(|d| {
            d.set_hold_all(hold);
            Ok(())
        })
    }

    /// Cards that are new or changed, as one JSON list of core/client/src/desk.rs `DeskCard` (snake case, `state` as
    /// text): the browser's way, one text across instead of a record per card.
    pub fn put_cards_json(&self, cards: String) -> Result<(), CoreError> {
        let cards: Vec<DeskCard> = serde_json::from_str(&cards)
            .map_err(|_| CoreError::bad_format("a list of Desk cards"))?;
        self.with(|d| {
            for c in cards {
                d.put_card(c);
            }
            Ok(())
        })
    }

    /// Permission requests that are new or changed, as one JSON list of core/client/src/desk.rs `DeskRequest`.
    pub fn put_requests_json(&self, requests: String) -> Result<(), CoreError> {
        let requests: Vec<DeskRequest> = serde_json::from_str(&requests)
            .map_err(|_| CoreError::bad_format("a list of Desk requests"))?;
        self.with(|d| {
            for r in requests {
                d.put_request(r);
            }
            Ok(())
        })
    }

    /// What changed of the Desk at `now_ms`, as the JSON of core/client/src/desk.rs `Delta` (snake case).
    pub fn project_json(&self, now_ms: u64) -> Result<String, CoreError> {
        self.with(|d| {
            serde_json::to_string(&d.project(now_ms))
                .map_err(|_| CoreError::internal("the Desk could not be said"))
        })
    }

    /// What changed of the Desk at `now_ms` since the last answer.
    pub fn project(&self, now_ms: u64) -> Result<DeskDelta, CoreError> {
        self.with(|d| {
            let delta = d.project(now_ms);
            Ok(DeskDelta {
                whole: delta.whole,
                stack: delta.stack,
                stack_ops: delta
                    .stack_ops
                    .into_iter()
                    .map(|o| DeskStackOp {
                        at: o.at,
                        gone: o.gone,
                        ids: o.ids,
                    })
                    .collect(),
                permissions_open: delta.permissions_open,
                held: delta.held,
                unheld: delta.unheld,
                expired: delta.expired,
                hand_back_over: delta.hand_back_over,
                session_open: delta
                    .session_open
                    .into_iter()
                    .map(|(session_id, card_ids)| DeskSessionOpen {
                        session_id,
                        card_ids,
                    })
                    .collect(),
                session_ops: delta
                    .session_ops
                    .into_iter()
                    .map(|o| DeskSessionOp {
                        session_id: o.session_id,
                        at: o.at,
                        gone: o.gone,
                        ids: o.ids,
                    })
                    .collect(),
                next_wake: delta.next_wake,
            })
        })
    }
}
