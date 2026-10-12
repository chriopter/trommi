//! The Desk: what stands before the human, decided in one place for every app.
//!
//! [`DeskState`] is the stateful handle the apps hold (through the bindings: `ClientDesk`). The host tells it what
//! changed of its cards and permission requests ([`DeskCard`], [`DeskRequest`]), the person's snoozes, the archived
//! sessions and the hub's open list ([`Listing`]); [`DeskState::project`] answers with what changed of the Desk
//! ([`Delta`]), and [`DeskState::projection`] with the whole of it ([`Projection`]):
//!
//! - **the hub's open list decides.** A card or request this device holds as open that the list leaves out, with
//!   nothing of it here newer than the list's change, was settled elsewhere and is held back (`held`): off the Desk.
//!   While the app waits for the list's first answer of a run (`hold_all`), every open card and request is held.
//!   A hand back older than the version the hub lists is over (`hand_back_over`): the agent answered it.
//! - **requests run out.** A pending request whose deadline passed is expired; one whose body was not read here has
//!   no deadline known and counts as expired [`REQUEST_LIFE_MAX_MS`] after it was sent.
//! - **the stack**: the open cards that are not held, by urgency (critical first), then the time they were asked,
//!   then the agent device and the id; a snoozed card and a card of an archived session are left out.
//! - **each session's open cards** (not held), by the time they were asked; the open requests by their change number.
//! - **when to look again** (`next_wake`): the next snooze that ends, the next request that runs out.
//!
//! Incremental: the open cards are kept in the Desk's order as they change, and a projection after a few cards
//! changed costs a few lookups, not a sort of the Desk: its answer is the stack's edits (`stack_ops`), the held
//! marks that changed, the sessions whose lists changed. A new list, a snooze, an archived session or the clock past
//! the next wake builds the whole Desk again.

use crate::board::{Board, Kind, State};
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The longest a permission request waits for its verdict: a connector's hook waits at most an hour, and two
/// minutes more for the clocks. An unread request older than this counts as expired (the hub cannot tell, so it
/// lists such requests as open for ever).
pub const REQUEST_LIFE_MAX_MS: u64 = 62 * 60_000;

/// One object the hub lists as open.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Listed {
    pub object_id: String,
    #[serde(default = "card_kind")]
    pub kind: Kind,
    #[serde(default)]
    pub session_id: Option<String>,
    /// The change of its current version (0: the hub served none).
    #[serde(default)]
    pub version_change: u64,
    /// The change of its first envelope.
    #[serde(default)]
    pub first_change: u64,
}

fn card_kind() -> Kind {
    Kind::Card
}

/// The hub's Desk (`GET /v1/desk`) as the rules need it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Listing {
    /// The hub's change number the list stands at.
    pub at: u64,
    /// The list was cut short: it decides nothing.
    #[serde(default)]
    pub truncated: bool,
    pub open: Vec<Listed>,
}

/// A card as the Desk needs it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeskCard {
    pub object_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    pub state: State,
    /// `critical`, `high`, `normal`, `low`.
    #[serde(default)]
    pub urgency: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub agent_device_id: Option<String>,
    /// The change of its newest version known here.
    #[serde(default)]
    pub version_change: u64,
    /// The newest change of it known here (a version, an answer, a take back).
    #[serde(default)]
    pub newest: u64,
    /// The change of the hand back (or What??) in force.
    #[serde(default)]
    pub in_revision: Option<u64>,
    /// Its newest version's body was read here.
    #[serde(default = "yes")]
    pub read: bool,
    /// An info its agent is still working on (spec 9.1.3): open, but on neither the stack nor its session's list
    /// until a version without it.
    #[serde(default)]
    pub working: bool,
}

/// A permission request as the Desk needs it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeskRequest {
    pub object_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Waiting for a verdict (as this device holds it).
    pub pending: bool,
    #[serde(default)]
    pub envelope_number: u64,
    /// The newest change of it known here (the request, a verdict).
    #[serde(default)]
    pub newest: u64,
    #[serde(default)]
    pub sent_at: u64,
    /// Its deadline from the bind; 0 when not known (its body not read).
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default = "yes")]
    pub read: bool,
}

fn yes() -> bool {
    true
}

/// The whole Desk.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Projection {
    /// The open cards on the Desk, in its order.
    pub stack: Vec<String>,
    /// The pending requests on the Desk, by change number.
    pub permissions_open: Vec<String>,
    /// Open here, settled at the hub (or held while the list is awaited): off the Desk.
    pub held: Vec<String>,
    /// Pending requests whose time ran out.
    pub expired: Vec<String>,
    /// Cards whose hand back is older than the version the hub lists.
    pub hand_back_over: Vec<String>,
    /// Per session: its open cards that are not held, by the time they were asked.
    pub session_open: BTreeMap<String, Vec<String>>,
    /// When the Desk changes by the clock alone (a snooze ends, a request runs out), ms; 0 for never.
    pub next_wake: u64,
}

/// What changed of the Desk since the last answer. `whole`: everything is in it (the first answer, a new list, a
/// snooze, an archived session, the clock past the next wake); then `stack` is the whole stack and `held` every
/// held object, `session_open` every session's list. Otherwise `stack_ops` edit the stack held from the last answer,
/// in order: at `at`, `gone` ids leave and `ids` come in their place; `session_ops` edit the sessions' lists the same
/// way; `held` and `unheld` name what changed; `permissions_open` is there when it changed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Delta {
    pub whole: bool,
    pub stack: Vec<String>,
    pub stack_ops: Vec<StackOp>,
    pub permissions_open: Option<Vec<String>>,
    pub held: Vec<String>,
    pub unheld: Vec<String>,
    pub expired: Vec<String>,
    pub hand_back_over: Vec<String>,
    pub session_open: BTreeMap<String, Vec<String>>,
    /// Not whole: the edits of the sessions' lists, in order.
    pub session_ops: Vec<SessionOp>,
    pub next_wake: u64,
}

/// One edit of a session's list of open cards: at `at`, `gone` ids leave and `ids` come.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionOp {
    pub session_id: String,
    pub at: u32,
    pub gone: u32,
    pub ids: Vec<String>,
}

/// One edit of the stack: at `at`, `gone` ids leave and `ids` come.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StackOp {
    pub at: u32,
    pub gone: u32,
    pub ids: Vec<String>,
}

/// A card's place on the Desk: urgency (critical first), the time it was asked, its agent device, its id.
type StackKey = (Reverse<u8>, u64, String, String);

fn rank(urgency: Option<&str>) -> u8 {
    match urgency {
        Some("critical") => 3,
        Some("high") => 2,
        Some("low") => 0,
        _ => 1,
    }
}

fn stack_key(c: &DeskCard) -> StackKey {
    (
        Reverse(rank(c.urgency.as_deref())),
        c.created_at,
        c.agent_device_id.clone().unwrap_or_default(),
        c.object_id.clone(),
    )
}

/// An object id as the model's 32 hex digits, from either form a register name carries it in: hex, or the wire's
/// base64url of its 16 bytes.
pub fn object_hex(id: &str) -> String {
    if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return id.to_ascii_lowercase();
    }
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32)
    };
    if id.len() != 22 {
        return id.to_string();
    }
    let (mut bits, mut n, mut out) = (0u32, 0, String::with_capacity(32));
    for c in id.bytes() {
        let Some(v) = value(c) else {
            return id.to_string();
        };
        bits = (bits << 6) | v;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push_str(&format!("{:02x}", (bits >> n) & 0xff));
        }
    }
    if out.len() == 32 {
        out
    } else {
        id.to_string()
    }
}

/// Whether a snooze value hides its card at `now`, and until when (None inside: for good).
fn snoozed(value: &serde_json::Value, now: u64) -> Option<Option<u64>> {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(false) => None,
        serde_json::Value::Object(o) => match o.get("until") {
            None | Some(serde_json::Value::Null) => Some(None),
            Some(u) => {
                let until = u.as_f64().unwrap_or(0.0);
                if until > now as f64 {
                    Some(Some(until.min(u64::MAX as f64) as u64))
                } else {
                    None
                }
            }
        },
        // (a value of another form: a web of before wrote `true`)
        _ => Some(None),
    }
}

/// The stateful Desk. See the module's documentation.
#[derive(Clone, Debug, Default)]
pub struct DeskState {
    cards: HashMap<String, DeskCard>,
    requests: HashMap<String, DeskRequest>,
    /// Snooze registers by the name they came under (hex or base64url), with the change of the write.
    snoozes: BTreeMap<String, (serde_json::Value, u64)>,
    /// The snoozes by card (hex): the newer write of a card named both ways.
    snooze_of: HashMap<String, serde_json::Value>,
    archived: BTreeSet<String>,
    listing: Option<Listing>,
    listed: HashMap<String, u64>,
    hold_all: bool,

    // what the last answer said (the incremental state)
    answered: bool,
    whole_due: bool,
    dirty_cards: BTreeSet<String>,
    dirty_requests: bool,
    /// The stack as last answered, in order, and each id's key in it.
    visible: Vec<StackKey>,
    visible_key: HashMap<String, StackKey>,
    /// Each session's open, not held cards by (time asked, id), as last answered.
    session_sets: HashMap<String, Vec<(u64, String)>>,
    /// Where each card stands in `session_sets`.
    session_of_card: HashMap<String, (String, u64)>,
    held: BTreeSet<String>,
    permissions_open: Vec<String>,
    next_wake: u64,
    wake_from_cards: u64,
}

impl DeskState {
    pub fn new() -> DeskState {
        DeskState::default()
    }

    pub fn put_card(&mut self, card: DeskCard) {
        if self.requests.remove(&card.object_id).is_some() {
            self.dirty_requests = true;
        }
        self.dirty_cards.insert(card.object_id.clone());
        self.cards.insert(card.object_id.clone(), card);
    }
    pub fn put_request(&mut self, request: DeskRequest) {
        if self.cards.remove(&request.object_id).is_some() {
            self.dirty_cards.insert(request.object_id.clone());
        }
        self.dirty_requests = true;
        self.requests.insert(request.object_id.clone(), request);
    }
    /// Forgets an object (a card or a request).
    pub fn remove(&mut self, object_id: &str) {
        if self.cards.remove(object_id).is_some() {
            self.dirty_cards.insert(object_id.to_string());
        }
        if self.requests.remove(object_id).is_some() {
            self.dirty_requests = true;
        }
    }
    /// Forgets every card and request (a model built again from nothing); the list, the snoozes, the archived
    /// sessions and the waiting stay.
    pub fn clear(&mut self) {
        self.cards.clear();
        self.requests.clear();
        self.whole_due = true;
    }
    /// A snooze register's value (`snooze/<card>`) written at change `at` (a write not taken yet: `u64::MAX`); null
    /// deletes it. The card may be named as the model names it (hex) or as the wire does (base64url): the two are
    /// two registers at the hub; of a card named both ways the newer write counts.
    pub fn set_snooze(&mut self, card: &str, value: serde_json::Value, at: u64) {
        if value.is_null() {
            self.snoozes.remove(card);
        } else {
            self.snoozes.insert(card.to_string(), (value, at));
        }
        let mut best: HashMap<String, (&serde_json::Value, u64)> = HashMap::new();
        for (name, (value, at)) in &self.snoozes {
            let id = object_hex(name);
            if best.get(&id).is_none_or(|(_, had)| *at >= *had) {
                best.insert(id, (value, *at));
            }
        }
        self.snooze_of = best.into_iter().map(|(k, (v, _))| (k, v.clone())).collect();
        self.whole_due = true;
    }
    pub fn set_archived(&mut self, sessions: impl IntoIterator<Item = String>) {
        let archived: BTreeSet<String> = sessions.into_iter().collect();
        if archived != self.archived {
            self.archived = archived;
            self.whole_due = true;
        }
    }
    /// The hub's open list as last read; none (or a truncated one) forgets it: the device's own view stands.
    pub fn set_listing(&mut self, listing: Option<Listing>) {
        self.listing = listing.filter(|l| !l.truncated);
        self.listed = self
            .listing
            .iter()
            .flat_map(|l| {
                l.open
                    .iter()
                    .map(|o| (o.object_id.clone(), o.version_change))
            })
            .collect();
        self.whole_due = true;
    }
    pub fn listing(&self) -> Option<&Listing> {
        self.listing.as_ref()
    }
    /// The app waits for the hub's first list of a run: every open card and request is held meanwhile.
    pub fn set_hold_all(&mut self, hold: bool) {
        if hold != self.hold_all {
            self.hold_all = hold;
            self.whole_due = true;
        }
    }

    fn held_by_list(&self, id: &str, newest: u64) -> bool {
        self.hold_all
            || matches!(&self.listing, Some(l) if !self.listed.contains_key(id) && newest <= l.at)
    }
    fn card_held(&self, c: &DeskCard) -> bool {
        c.state == State::Open && self.held_by_list(&c.object_id, c.newest.max(c.version_change))
    }
    /// On the Desk now: open, not held, not snoozed, not in an archived session. The snooze's end, when one.
    fn card_visible(&self, c: &DeskCard, now: u64) -> (bool, Option<u64>) {
        if c.state != State::Open || c.working || self.card_held(c) {
            return (false, None);
        }
        if let Some(until) = self
            .snooze_of
            .get(&c.object_id)
            .and_then(|v| snoozed(v, now))
        {
            return (false, until);
        }
        (
            !c.session_id
                .as_ref()
                .is_some_and(|s| self.archived.contains(s)),
            None,
        )
    }
    fn hand_back_over(&self, c: &DeskCard) -> bool {
        matches!((c.in_revision, self.listed.get(&c.object_id)), (Some(h), Some(v)) if h < *v)
    }
    fn session_list(&self, session: &str) -> Vec<String> {
        let mut list: Vec<(u64, &str)> = self
            .cards
            .values()
            .filter(|c| {
                c.state == State::Open
                    && !c.working
                    && c.session_id.as_deref() == Some(session)
                    && !self.card_held(c)
            })
            .map(|c| (c.created_at, c.object_id.as_str()))
            .collect();
        list.sort();
        list.into_iter().map(|(_, id)| id.to_string()).collect()
    }
    /// The pending requests on the Desk, the expired ones, the held ones, and the next one to run out.
    fn requests_now(&self, now: u64) -> (Vec<String>, Vec<String>, Vec<String>, u64) {
        let (mut open, mut expired, mut held, mut wake) =
            (Vec::new(), Vec::new(), Vec::new(), u64::MAX);
        let mut pending: Vec<&DeskRequest> = self.requests.values().filter(|r| r.pending).collect();
        pending.sort_by(|a, b| {
            a.envelope_number
                .cmp(&b.envelope_number)
                .then_with(|| a.object_id.cmp(&b.object_id))
        });
        for r in pending {
            if self.held_by_list(&r.object_id, r.newest.max(r.envelope_number)) {
                held.push(r.object_id.clone());
                continue;
            }
            let ends = if r.expires_at != 0 {
                r.expires_at
            } else if !r.read && r.sent_at != 0 {
                r.sent_at + REQUEST_LIFE_MAX_MS
            } else {
                0
            };
            if ends != 0 && now > ends {
                expired.push(r.object_id.clone());
                continue;
            }
            if ends != 0 {
                wake = wake.min(ends);
            }
            open.push(r.object_id.clone());
        }
        (open, expired, held, wake)
    }

    /// The whole Desk at `now` (nothing of the incremental state is touched).
    pub fn projection(&self, now: u64) -> Projection {
        let mut p = Projection::default();
        let (open, expired, held_requests, mut wake) = self.requests_now(now);
        p.permissions_open = open;
        p.expired = expired;
        let mut keys: Vec<StackKey> = Vec::new();
        let mut held_cards = Vec::new();
        for c in self.cards.values() {
            if self.card_held(c) {
                held_cards.push(c.object_id.clone());
            } else if self.hand_back_over(c) && self.listing.is_some() {
                p.hand_back_over.push(c.object_id.clone());
            }
            match self.card_visible(c, now) {
                (true, _) => keys.push(stack_key(c)),
                (false, Some(until)) => wake = wake.min(until),
                _ => {}
            }
        }
        keys.sort();
        p.stack = keys.into_iter().map(|k| k.3).collect();
        let mut held: Vec<String> = held_cards.into_iter().chain(held_requests).collect();
        held.sort();
        p.held = held;
        p.hand_back_over.sort();
        let sessions: BTreeSet<&str> = self
            .cards
            .values()
            .filter_map(|c| c.session_id.as_deref())
            .collect();
        for s in sessions {
            let list = self.session_list(s);
            if !list.is_empty() {
                p.session_open.insert(s.to_string(), list);
            }
        }
        p.next_wake = if wake == u64::MAX { 0 } else { wake };
        p
    }

    /// What changed of the Desk at `now` since the last answer (see [`Delta`]).
    pub fn project(&mut self, now: u64) -> Delta {
        let clock = self.next_wake != 0 && now >= self.next_wake;
        if !self.answered || self.whole_due || clock {
            return self.whole(now);
        }
        let mut d = Delta::default();
        // (per session, its last edit: the next one right behind it is joined to it)
        let mut last_op: HashMap<String, usize> = HashMap::new();
        // the cards that changed: their place in the stack, their hold, their sessions
        let dirty = std::mem::take(&mut self.dirty_cards);
        for id in &dirty {
            // where it stood
            if let Some(old) = self.visible_key.remove(id) {
                let pos = self.visible.partition_point(|k| k < &old);
                if self.visible.get(pos) == Some(&old) {
                    self.visible.remove(pos);
                    push_stack_op(&mut d.stack_ops, pos as u32, 1, None);
                }
            }
            if let Some((s, at)) = self.session_of_card.remove(id) {
                if let Some(set) = self.session_sets.get_mut(&s) {
                    let entry = (at, id.clone());
                    let pos = set.partition_point(|e| e < &entry);
                    if set.get(pos) == Some(&entry) {
                        set.remove(pos);
                    }
                    push_session_op(&mut d.session_ops, &mut last_op, s, pos as u32, 1, None);
                }
            }
            let was_held = self.held.contains(id);
            let Some(c) = self.cards.get(id) else {
                if was_held {
                    self.held.remove(id);
                    d.unheld.push(id.clone());
                }
                continue;
            };
            // (what the card says, read before anything changes: the card itself is not copied)
            let held_now = self.card_held(c);
            let hand_back = !held_now && self.listing.is_some() && self.hand_back_over(c);
            let in_session = match (
                &c.session_id,
                c.state == State::Open && !c.working && !held_now,
            ) {
                (Some(s), true) => Some((s.clone(), c.created_at)),
                _ => None,
            };
            let visible = self.card_visible(c, now);
            let key = matches!(visible, (true, _)).then(|| stack_key(c));
            if held_now != was_held {
                if held_now {
                    self.held.insert(id.clone());
                    d.held.push(id.clone());
                } else {
                    self.held.remove(id);
                    d.unheld.push(id.clone());
                }
            }
            if hand_back {
                d.hand_back_over.push(id.clone());
            }
            if let Some((s, created_at)) = in_session {
                let set = self.session_sets.entry(s.clone()).or_default();
                let entry = (created_at, id.clone());
                let pos = set.partition_point(|e| e < &entry);
                set.insert(pos, entry);
                push_session_op(
                    &mut d.session_ops,
                    &mut last_op,
                    s.clone(),
                    pos as u32,
                    0,
                    Some(id.clone()),
                );
                self.session_of_card.insert(id.clone(), (s, created_at));
            }
            match (visible, key) {
                ((true, _), Some(key)) => {
                    let pos = self.visible.partition_point(|k| k < &key);
                    self.visible.insert(pos, key.clone());
                    self.visible_key.insert(id.clone(), key);
                    push_stack_op(&mut d.stack_ops, pos as u32, 0, Some(id.clone()));
                }
                ((false, Some(until)), _) => self.wake_from_cards = self.wake_from_cards.min(until),
                _ => {}
            }
        }
        self.next_wake = min_wake(self.next_wake, self.wake_from_cards);
        if std::mem::take(&mut self.dirty_requests) {
            let (open, expired, held_requests, wake) = self.requests_now(now);
            for id in &held_requests {
                if self.held.insert(id.clone()) {
                    d.held.push(id.clone());
                }
            }
            let held_now: BTreeSet<&String> = held_requests.iter().collect();
            let gone: Vec<String> = self
                .held
                .iter()
                .filter(|id| self.requests.contains_key(*id) && !held_now.contains(id))
                .cloned()
                .collect();
            for id in gone {
                self.held.remove(&id);
                d.unheld.push(id);
            }
            d.expired = expired;
            if open != self.permissions_open {
                self.permissions_open = open.clone();
                d.permissions_open = Some(open);
            }
            self.next_wake = min_wake(wake, self.wake_from_cards);
        }
        d.next_wake = self.next_wake;
        d
    }

    fn whole(&mut self, now: u64) -> Delta {
        let p = self.projection(now);
        self.answered = true;
        self.whole_due = false;
        self.dirty_cards.clear();
        self.dirty_requests = false;
        self.visible = p
            .stack
            .iter()
            .map(|id| stack_key(&self.cards[id]))
            .collect();
        self.visible_key = self
            .visible
            .iter()
            .map(|k| (k.3.clone(), k.clone()))
            .collect();
        self.session_sets.clear();
        self.session_of_card.clear();
        for (s, ids) in &p.session_open {
            for id in ids {
                let at = self.cards[id].created_at;
                // (the lists come sorted by the time each card was asked: kept as they are)
                self.session_sets
                    .entry(s.clone())
                    .or_default()
                    .push((at, id.clone()));
                self.session_of_card.insert(id.clone(), (s.clone(), at));
            }
        }
        self.held = p.held.iter().cloned().collect();
        self.permissions_open = p.permissions_open.clone();
        self.next_wake = p.next_wake;
        // (the snoozes' ends are in it; an early wake only builds the Desk again)
        self.wake_from_cards = if p.next_wake == 0 {
            u64::MAX
        } else {
            p.next_wake
        };
        Delta {
            whole: true,
            stack: p.stack,
            stack_ops: Vec::new(),
            permissions_open: Some(p.permissions_open),
            held: p.held,
            unheld: Vec::new(),
            expired: p.expired,
            hand_back_over: p.hand_back_over,
            session_open: p.session_open,
            session_ops: Vec::new(),
            next_wake: p.next_wake,
        }
    }
}

fn min_wake(a: u64, b: u64) -> u64 {
    let m = [a, b]
        .into_iter()
        .filter(|w| *w != 0 && *w != u64::MAX)
        .min();
    m.unwrap_or(0)
}

// ---- the Desk of a host that keeps a `Board` (the Linux app) ---------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    /// Read here: it can be answered.
    Ready,
    /// Listed, not read yet: a quiet "Loading …" with nothing to answer.
    Placeholder,
}

/// One row of the Desk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeskEntry {
    pub object_id: String,
    pub kind: Kind,
    pub session_id: Option<String>,
    pub readiness: Readiness,
    /// The change of the version shown (or listed, for a placeholder).
    pub change: u64,
    /// The body to render, when read.
    pub body: Option<serde_json::Value>,
}

/// The Desk of a [`Board`], in the order of the projection (cards, then requests), with a placeholder for every
/// listed object the board holds nothing of.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Desk {
    pub entries: Vec<DeskEntry>,
    pub held: Vec<String>,
    pub expired: Vec<String>,
    pub hand_back_over: Vec<String>,
    /// Built from a hub list (false: no list yet, or a truncated one; the Desk is this device's own view).
    pub complete: bool,
    pub next_expiry: u64,
}

impl Desk {
    /// The Desk at `now`, from the board and the hub's list (if one was read).
    pub fn build(board: &Board, listing: Option<&Listing>, now_ms: u64) -> Desk {
        let mut state = DeskState::new();
        for o in board.objects.values() {
            let newest = o.state_change.max(o.version_change);
            match o.kind {
                Kind::Card => state.put_card(DeskCard {
                    object_id: o.object_id.clone(),
                    session_id: o.session_id.clone(),
                    state: o.state,
                    urgency: None,
                    created_at: o.version_change,
                    agent_device_id: None,
                    version_change: o.version_change,
                    newest,
                    in_revision: o.hand_back,
                    read: o.is_read(),
                    working: false,
                }),
                Kind::Request => state.put_request(DeskRequest {
                    object_id: o.object_id.clone(),
                    session_id: o.session_id.clone(),
                    pending: o.state == State::Open,
                    envelope_number: o.version_change,
                    newest,
                    sent_at: o.sent_at,
                    expires_at: o.expires_at,
                    read: o.is_read(),
                }),
            }
        }
        state.set_listing(listing.cloned());
        let p = state.projection(now_ms);
        let mut desk = Desk {
            held: p.held,
            expired: p.expired,
            hand_back_over: p.hand_back_over,
            complete: state.listing().is_some(),
            next_expiry: p.next_wake,
            entries: Vec::new(),
        };
        let listed: BTreeMap<&str, &Listed> = state
            .listing()
            .iter()
            .flat_map(|l| l.open.iter().map(|o| (o.object_id.as_str(), o)))
            .collect();
        for id in p.stack.iter().chain(p.permissions_open.iter()) {
            let Some(o) = board.get(id) else { continue };
            let want = listed
                .get(id.as_str())
                .map(|l| l.version_change)
                .unwrap_or(0);
            let read = o.is_read() && o.version_change >= want;
            desk.entries.push(DeskEntry {
                object_id: id.clone(),
                kind: o.kind,
                session_id: o.session_id.clone(),
                readiness: if read {
                    Readiness::Ready
                } else {
                    Readiness::Placeholder
                },
                change: o.version_change.max(want),
                body: if read { o.body.clone() } else { None },
            });
        }
        for (id, l) in &listed {
            if board.get(id).is_none() {
                desk.entries.push(DeskEntry {
                    object_id: id.to_string(),
                    kind: l.kind,
                    session_id: l.session_id.clone(),
                    readiness: Readiness::Placeholder,
                    change: l.version_change.max(l.first_change),
                    body: None,
                });
            }
        }
        desk
    }
}

/// One edit of the stack, joined to the edit before when it starts right behind what that one put in (a page of
/// new cards in a row crosses as one edit, not one per card).
fn push_stack_op(ops: &mut Vec<StackOp>, at: u32, gone: u32, id: Option<String>) {
    if let Some(last) = ops.last_mut() {
        if at == last.at + last.ids.len() as u32 {
            last.gone += gone;
            last.ids.extend(id);
            return;
        }
    }
    ops.push(StackOp {
        at,
        gone,
        ids: id.into_iter().collect(),
    });
}

/// One edit of a session's list, joined to that session's edit before as [`push_stack_op`] does (the edits of
/// different sessions do not touch each other).
fn push_session_op(
    ops: &mut Vec<SessionOp>,
    last_op: &mut HashMap<String, usize>,
    session_id: String,
    at: u32,
    gone: u32,
    id: Option<String>,
) {
    if let Some(&i) = last_op.get(&session_id) {
        let last = &mut ops[i];
        if at == last.at + last.ids.len() as u32 {
            last.gone += gone;
            last.ids.extend(id);
            return;
        }
    }
    last_op.insert(session_id.clone(), ops.len());
    ops.push(SessionOp {
        session_id,
        at,
        gone,
        ids: id.into_iter().collect(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::{Fact, Source};

    fn card(id: &str, change: u64) -> DeskCard {
        DeskCard {
            object_id: id.into(),
            session_id: Some("s".into()),
            state: State::Open,
            urgency: None,
            created_at: change,
            agent_device_id: None,
            version_change: change,
            newest: change,
            in_revision: None,
            read: true,
            working: false,
        }
    }
    fn request(id: &str, change: u64) -> DeskRequest {
        DeskRequest {
            object_id: id.into(),
            session_id: Some("s".into()),
            pending: true,
            envelope_number: change,
            newest: change,
            sent_at: 1_000,
            expires_at: 0,
            read: true,
        }
    }
    fn listing(at: u64, ids: &[(&str, u64)]) -> Option<Listing> {
        Some(Listing {
            at,
            truncated: false,
            open: ids
                .iter()
                .map(|(id, v)| Listed {
                    object_id: id.to_string(),
                    kind: Kind::Card,
                    session_id: None,
                    version_change: *v,
                    first_change: *v,
                })
                .collect(),
        })
    }

    #[test]
    fn the_stack_by_urgency_then_time() {
        let mut d = DeskState::new();
        d.put_card(DeskCard {
            urgency: Some("low".into()),
            ..card("low", 1)
        });
        d.put_card(card("b", 3));
        d.put_card(card("a", 2));
        d.put_card(DeskCard {
            urgency: Some("critical".into()),
            ..card("crit", 9)
        });
        d.put_card(DeskCard {
            state: State::Answered,
            ..card("answered", 4)
        });
        assert_eq!(d.projection(0).stack, ["crit", "a", "b", "low"]);
        assert_eq!(d.projection(0).session_open["s"], ["low", "a", "b", "crit"]);
    }

    #[test]
    fn snoozed_and_archived_leave_the_stack() {
        let mut d = DeskState::new();
        d.put_card(card("a", 1));
        d.put_card(card("b", 2));
        d.put_card(DeskCard {
            session_id: Some("old".into()),
            ..card("c", 3)
        });
        d.set_snooze("a", serde_json::json!({ "until": 5_000 }), 1);
        d.set_snooze("b", serde_json::json!({}), 1);
        d.set_archived(["old".to_string()]);
        let p = d.projection(1_000);
        assert!(p.stack.is_empty());
        assert_eq!(p.next_wake, 5_000);
        assert_eq!(d.projection(6_000).stack, ["a"]);
        assert_eq!(p.session_open["s"], ["a", "b"]);
    }

    #[test]
    fn settled_elsewhere_is_held_unless_newer_here() {
        let mut d = DeskState::new();
        d.put_card(card("old", 50));
        d.put_card(card("new", 150));
        d.put_card(card("listed", 60));
        d.put_request(request("r", 40));
        d.set_listing(listing(100, &[("listed", 60)]));
        let p = d.projection(2_000);
        assert_eq!(p.held, ["old", "r"]);
        assert_eq!(p.stack, ["listed", "new"]);
        assert!(p.permissions_open.is_empty());
        d.set_listing(Some(Listing {
            at: 100,
            truncated: true,
            open: vec![],
        }));
        assert!(d.projection(2_000).held.is_empty());
    }

    #[test]
    fn waiting_for_the_list_holds_everything_open() {
        let mut d = DeskState::new();
        d.put_card(card("a", 1));
        d.put_card(DeskCard {
            state: State::Closed,
            ..card("closed", 2)
        });
        d.put_request(request("r", 3));
        d.set_hold_all(true);
        let p = d.projection(0);
        assert_eq!(p.held, ["a", "r"]);
        assert!(p.stack.is_empty() && p.permissions_open.is_empty());
        d.set_hold_all(false);
        assert_eq!(d.projection(0).stack, ["a"]);
    }

    #[test]
    fn an_unread_request_past_62_minutes_expired() {
        let mut d = DeskState::new();
        d.put_request(DeskRequest {
            read: false,
            ..request("r", 50)
        });
        let soon = d.projection(1_000 + REQUEST_LIFE_MAX_MS - 1);
        assert_eq!(soon.permissions_open, ["r"]);
        assert_eq!(soon.next_wake, 1_000 + REQUEST_LIFE_MAX_MS);
        let late = d.projection(1_000 + REQUEST_LIFE_MAX_MS + 1);
        assert!(late.permissions_open.is_empty());
        assert_eq!(late.expired, ["r"]);
        d.put_request(DeskRequest {
            expires_at: 5_000,
            ..request("r2", 51)
        });
        assert_eq!(d.projection(4_000).permissions_open, ["r", "r2"]);
        assert_eq!(d.projection(6_000).expired, ["r2"]);
    }

    #[test]
    fn a_snooze_written_in_either_form_hides_the_card() {
        let mut d = DeskState::new();
        let id = "2cbdd22674edfb9c0f31ae1edfef7863";
        d.put_card(card(id, 1));
        assert_eq!(object_hex("LL3SJnTt-5wPMa4e3-94Yw"), id);
        d.set_snooze(
            "LL3SJnTt-5wPMa4e3-94Yw",
            serde_json::json!({ "until": 9_000 }),
            5,
        );
        assert!(d.projection(1_000).stack.is_empty());
        d.set_snooze(id, serde_json::json!({ "until": 500 }), 3);
        assert!(d.projection(1_000).stack.is_empty());
        d.set_snooze(id, serde_json::json!({ "until": 500 }), 7);
        assert_eq!(d.projection(1_000).stack, [id]);
    }

    #[test]
    fn an_old_hand_back_is_over() {
        let mut d = DeskState::new();
        d.put_card(DeskCard {
            in_revision: Some(60),
            ..card("a", 50)
        });
        d.set_listing(listing(100, &[("a", 70)]));
        assert_eq!(d.projection(0).hand_back_over, ["a"]);
        d.set_listing(listing(100, &[("a", 50)]));
        assert!(d.projection(0).hand_back_over.is_empty());
    }

    /// The deltas, applied to what the last answers said, always give the whole projection: many random edits.
    #[test]
    fn an_info_still_worked_on_stands_nowhere() {
        let mut d = DeskState::new();
        d.put_card(card("a", 1));
        d.put_card(DeskCard {
            working: true,
            ..card("b", 2)
        });
        let p = d.projection(5);
        assert_eq!(p.stack, vec!["a".to_string()]);
        assert_eq!(p.session_open.get("s"), Some(&vec!["a".to_string()]));
        d.project(5);
        d.put_card(DeskCard {
            working: false,
            ..card("b", 3)
        });
        let delta = d.project(5);
        assert_eq!(
            delta.stack_ops.len(),
            1,
            "its Done version brings it to the Desk"
        );
    }

    #[test]
    fn deltas_give_the_whole_desk() {
        for seed in [7u64, 11, 13, 101, 997, 4_242, 65_537, 1_000_003] {
            deltas_with(seed);
        }
    }

    fn deltas_with(seed: u64) {
        let mut rng = seed;
        let mut next = |n: u64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng % n
        };
        let mut d = DeskState::new();
        let mut stack: Vec<String> = Vec::new();
        let mut held: BTreeSet<String> = BTreeSet::new();
        let mut sessions: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut perms: Vec<String> = Vec::new();
        let mut now = 1_000u64;
        for step in 0..4_000 {
            let id = format!("{:032x}", next(60));
            match next(20) {
                0..=9 => d.put_card(DeskCard {
                    session_id: Some(format!("s{}", next(4))),
                    state: [State::Open, State::Open, State::Answered, State::Closed]
                        [next(4) as usize],
                    urgency: Some(["low", "normal", "high", "critical"][next(4) as usize].into()),
                    created_at: next(500),
                    agent_device_id: Some(format!("a{}", next(3))),
                    newest: next(300),
                    version_change: next(300),
                    in_revision: if next(5) == 0 { Some(next(300)) } else { None },
                    working: next(6) == 0,
                    ..card(&id, 0)
                }),
                10..=11 => d.remove(&id),
                12..=13 => d.put_request(DeskRequest {
                    pending: next(3) != 0,
                    envelope_number: next(300),
                    expires_at: if next(2) == 0 { now + next(400) } else { 0 },
                    read: next(4) != 0,
                    sent_at: now - 1,
                    ..request(&format!("{:031x}r", next(20)), 0)
                }),
                14 => d.set_snooze(
                    &id,
                    if next(2) == 0 {
                        serde_json::json!({ "until": now + next(300) })
                    } else {
                        serde_json::Value::Null
                    },
                    step,
                ),
                15 => d.set_listing(if next(3) == 0 {
                    None
                } else {
                    listing(next(300), &[(&id, next(300))])
                }),
                16 => d.set_hold_all(next(4) == 0),
                17 => d.set_archived(if next(2) == 0 {
                    vec![format!("s{}", next(4))]
                } else {
                    vec![]
                }),
                _ => now += next(200),
            }
            if next(3) != 0 {
                continue;
            }
            let delta = d.project(now);
            if delta.whole {
                stack = delta.stack.clone();
                held = delta.held.iter().cloned().collect();
                sessions = delta.session_open.clone();
            } else {
                for op in &delta.stack_ops {
                    let at = op.at as usize;
                    stack.splice(at..at + op.gone as usize, op.ids.iter().cloned());
                }
                for id in &delta.held {
                    held.insert(id.clone());
                }
                for id in &delta.unheld {
                    held.remove(id);
                }
                for op in &delta.session_ops {
                    let list = sessions.entry(op.session_id.clone()).or_default();
                    let at = op.at as usize;
                    list.splice(at..at + op.gone as usize, op.ids.iter().cloned());
                    if list.is_empty() {
                        sessions.remove(&op.session_id);
                    }
                }
            }
            if let Some(p) = &delta.permissions_open {
                perms = p.clone();
            }
            let whole = d.projection(now);
            assert_eq!(stack, whole.stack, "the stack at step {step} (seed {seed})");
            assert_eq!(
                held,
                whole.held.iter().cloned().collect::<BTreeSet<_>>(),
                "held at step {step}"
            );
            assert_eq!(sessions, whole.session_open, "the sessions at step {step}");
            assert_eq!(perms, whole.permissions_open, "the requests at step {step}");
        }
    }

    fn fact(id: &str, kind: Kind, change: u64, state: State, body: bool) -> Fact {
        Fact {
            object_id: id.into(),
            kind,
            session_id: Some("s".into()),
            change,
            state,
            is_version: true,
            body: body.then(|| serde_json::json!({ "title": id })),
            sent_at: 1_000,
            expires_at: 0,
            hand_back: None,
            source: Source::Live,
        }
    }

    #[test]
    fn a_board_desk_with_placeholders() {
        let mut board = Board::new();
        board.apply(&fact("a", Kind::Card, 50, State::Open, true));
        board.apply(&fact("old", Kind::Card, 20, State::Open, true));
        let desk = Desk::build(
            &board,
            listing(100, &[("a", 80), ("unknown", 90)]).as_ref(),
            2_000,
        );
        assert!(desk.complete);
        assert_eq!(desk.held, ["old"]);
        let rows: Vec<(&str, Readiness)> = desk
            .entries
            .iter()
            .map(|e| (e.object_id.as_str(), e.readiness))
            .collect();
        assert_eq!(
            rows,
            [
                ("a", Readiness::Placeholder),
                ("unknown", Readiness::Placeholder)
            ]
        );
        let own = Desk::build(&board, None, 0);
        assert!(!own.complete);
        assert_eq!(own.entries.len(), 2);
    }
}
