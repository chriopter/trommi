//! The Desk of the model: the stack, each session's open cards, the open permission requests, from the client
//! core's Desk rules (`crate::desk`). What a batch touched goes to the Desk; what changed of it comes back.

use super::change::Change;
use super::types::*;
use super::Model;
use crate::desk::{DeskCard, DeskRequest, Listed, Listing};
use crate::{Kind, State};
use serde_json::Value;

const MAX_SAFE: u64 = 9_007_199_254_740_991;

fn desk_card(c: &Card) -> DeskCard {
    DeskCard {
        object_id: c.object_id.clone(),
        session_id: c.session_id.clone(),
        state: match c.object_state.as_str() {
            "answered" => State::Answered,
            "closed" => State::Closed,
            _ => State::Open,
        },
        urgency: Some(c.urgency.clone()),
        first_change: c.first_envelope_number,
        agent_device_id: Some(c.agent_device_id.clone()),
        version_change: c.envelope_number,
        newest: c.state_envelope_number,
        in_revision: c.in_revision.as_ref().map(|r| r.envelope_number),
        read: c.content_state == "ok",
        working: c.field("working") == Value::Bool(true),
    }
}

fn desk_request(p: &PermissionRequest) -> DeskRequest {
    DeskRequest {
        object_id: p.object_id.clone(),
        session_id: p.session_id.clone(),
        pending: p.permission_state == "pending",
        envelope_number: p.envelope_number,
        newest: p.envelope_number,
        sent_at: p.sent_at,
        expires_at: p.expires_at,
        read: p.content_state == "ok",
    }
}

impl Model {
    /// The hub's Desk as last read: what it lists as open (by id, the change of its version) and at which change;
    /// none forgets it. It decides the Desk from the next projection on.
    pub fn set_desk_listing(&mut self, listing: Option<(u64, Vec<(String, u64)>)>) {
        self.set_desk_rows(listing.map(|(at, open)| {
            (
                at,
                open.into_iter()
                    .map(|(id, version)| (id, version, 0))
                    .collect(),
            )
        }));
    }

    /// [`Model::set_desk_listing`] with the change each listed object was first filed at (the hub's row; 0: not
    /// said). A card stands on the Desk, in its session's list and in the walk by that change, so a device that
    /// knows a card only by its newest version (a sign-in: the Desk is read, the past is not) puts it where every
    /// other device has it, and nothing moves when the card's first version is read later.
    #[allow(clippy::type_complexity)]
    pub fn set_desk_rows(&mut self, listing: Option<(u64, Vec<(String, u64, u64)>)>) {
        self.desk_first.clear();
        if let Some((_, open)) = &listing {
            for (id, _, first) in open {
                if *first > 0 {
                    self.desk_first.insert(id.clone(), *first);
                }
            }
        }
        self.desk_first_new = !self.desk_first.is_empty();
        self.desk.set_listing(listing.map(|(at, open)| {
            Listing {
                at,
                truncated: false,
                open: open
                    .into_iter()
                    .map(|(object_id, version_change, first_change)| Listed {
                        object_id,
                        kind: Kind::Card,
                        session_id: None,
                        version_change,
                        first_change,
                    })
                    .collect(),
            }
        }));
        self.desk_dirty = true;
    }

    /// The hub's row says a card was first filed before anything of it this device read: the card takes that
    /// change as its first. Only ever lowered (the first envelope of an object is its lowest), and only while
    /// the device does not hold the card's first version: once it does, that version's number stands
    /// (apply.rs), whatever a row said before or says later. Asked of the cards a batch touched (a card made
    /// from its newest version; one whose first number the batch moved) and of every listed card after a new
    /// list.
    fn first_from_desk(&mut self, change: &mut Change) {
        if self.desk_first.is_empty() {
            return;
        }
        let ids: Vec<String> = if std::mem::take(&mut self.desk_first_new) {
            self.desk_first.keys().cloned().collect()
        } else {
            change.cards.iter().cloned().collect()
        };
        for id in ids {
            let (Some(first), Some(card)) = (self.desk_first.get(&id), self.cards.get_mut(&id))
            else {
                continue;
            };
            if *first < card.first_envelope_number && !card.holds_first_version() {
                card.first_envelope_number = *first;
                change.cards.insert(id);
                self.desk_rebuild = true;
            }
        }
    }

    /// The app waits for the hub's first list of a run: every open card and request is held meanwhile.
    pub fn set_desk_hold(&mut self, hold: bool) {
        self.desk.set_hold_all(hold);
        self.desk_dirty = true;
    }

    fn snooze_at(&self, id: &str) -> u64 {
        self.human
            .get(&format!("snooze/{id}"))
            .and_then(|r| r.envelope_number)
            .unwrap_or(MAX_SAFE)
    }
    fn snooze_value(&self, id: &str) -> Value {
        self.human
            .get(&format!("snooze/{id}"))
            .map(|r| r.value.clone())
            .unwrap_or(Value::Null)
    }

    /// Stack, per-session card lists, open permission requests, from the Desk rules. A change that touches nothing of
    /// the Desk, before the clock reached its next wake, costs nothing here.
    pub fn project(&mut self, change: &mut Change, now: u64) {
        self.clock = now;
        if self.started_at == 0 {
            self.started_at = now;
        }
        if self.status_wake != 0 && now >= self.status_wake {
            self.status_wake = 0;
            change.status = true;
        }
        self.first_from_desk(change);
        let relevant = !change.cards.is_empty()
            || !change.permissions.is_empty()
            || change
                .registers
                .iter()
                .any(|k| k.starts_with("snooze/") || k.starts_with("session/"));
        if self.desk_rebuild {
            self.desk_rebuild = false;
            self.desk_whole = false;
        }
        let wake = self.desk_wake != 0 && now >= self.desk_wake;
        if self.desk_whole && !relevant && !self.desk_dirty && !wake {
            return;
        }
        self.desk_dirty = false;
        let before = self.stack.clone();
        if !self.desk_whole {
            self.desk.clear();
            let cards: Vec<DeskCard> = self.cards.values().map(desk_card).collect();
            for c in cards {
                self.desk.put_card(c);
            }
            let requests: Vec<DeskRequest> = self.permissions.values().map(desk_request).collect();
            for r in requests {
                self.desk.put_request(r);
            }
            let snoozes: Vec<(String, Value, u64)> = self
                .human
                .iter()
                .filter_map(|(k, r)| {
                    k.strip_prefix("snooze/")
                        .map(|id| (id.to_string(), r.value.clone(), self.snooze_at(id)))
                })
                .collect();
            for (id, v, at) in snoozes {
                self.desk.set_snooze(&id, v, at);
            }
            let cards = &self.cards;
            for s in self.sessions.values_mut() {
                s.card_ids.sort_by(|a, b| {
                    let ca = cards.get(a).map_or(0, |c| c.first_envelope_number);
                    let cb = cards.get(b).map_or(0, |c| c.first_envelope_number);
                    ca.cmp(&cb).then_with(|| a.cmp(b))
                });
            }
            self.desk_whole = true;
            self.desk_archived = None;
        } else {
            for id in change.cards.clone() {
                match self.cards.get(&id) {
                    Some(c) => {
                        let d = desk_card(c);
                        self.desk.put_card(d);
                    }
                    None if !self.permissions.contains_key(&id) => self.desk.remove(&id),
                    None => {}
                }
            }
            for id in change.permissions.clone() {
                match self.permissions.get(&id) {
                    Some(p) => {
                        let d = desk_request(p);
                        self.desk.put_request(d);
                    }
                    None if !self.cards.contains_key(&id) => self.desk.remove(&id),
                    None => {}
                }
            }
            for k in change.registers.clone() {
                if let Some(id) = k.strip_prefix("snooze/") {
                    let (v, at) = (self.snooze_value(id), self.snooze_at(id));
                    self.desk.set_snooze(id, v, at);
                }
            }
        }
        // (a session is archived as the session list shows it (sessions.rs): the person's choice, a closed helper, a
        // replaced pairing; a main whose agent is back is not. One rule: a card stands on the Desk exactly when its
        // session stands in the list)
        let archived: Vec<String> = crate::sessions::archived_sessions(self);
        let joined = archived.join(",");
        if self.desk_archived.as_deref() != Some(joined.as_str()) {
            self.desk_archived = Some(joined);
            self.desk.set_archived(archived);
        }
        let p = self.desk.project(now);
        self.desk_wake = p.next_wake;
        let mut moved = false;
        let mark = |model: &mut Model, id: &str, hold: bool, change: &mut Change| {
            if let Some(c) = model.cards.get_mut(id) {
                if c.catching_up == hold {
                    return;
                }
                c.catching_up = hold;
                change.cards.insert(id.to_string());
                if let Some(s) = &c.session_id {
                    change.sessions.insert(s.clone());
                }
            } else if let Some(r) = model.permissions.get_mut(id) {
                if r.catching_up == hold {
                    return;
                }
                r.catching_up = hold;
                change.permissions.insert(id.to_string());
                if let Some(s) = &r.session_id {
                    change.sessions.insert(s.clone());
                }
            }
        };
        if p.whole {
            let held: std::collections::BTreeSet<String> = p.held.iter().cloned().collect();
            let was: Vec<String> = self
                .cards
                .values()
                .filter(|c| c.catching_up)
                .map(|c| c.object_id.clone())
                .chain(
                    self.permissions
                        .values()
                        .filter(|r| r.catching_up)
                        .map(|r| r.object_id.clone()),
                )
                .collect();
            for id in was {
                if !held.contains(&id) {
                    mark(self, &id, false, change)
                }
            }
            for id in &held {
                mark(self, id, true, change)
            }
            moved = p.stack != before;
            self.stack = p.stack.clone();
            for s in self.sessions.values_mut() {
                s.open_card_ids.clear()
            }
        } else {
            for id in &p.held {
                mark(self, id, true, change)
            }
            for id in &p.unheld {
                mark(self, id, false, change)
            }
            if !p.stack_ops.is_empty() {
                for op in &p.stack_ops {
                    let (at, gone) = (op.at as usize, op.gone as usize);
                    self.stack.splice(at..at + gone, op.ids.iter().cloned());
                }
                moved = true;
            }
        }
        for (sid, ids) in &p.session_open {
            if let Some(s) = self.sessions.get_mut(sid) {
                s.open_card_ids = ids.clone();
                change.sessions.insert(sid.clone());
            }
        }
        for op in &p.session_ops {
            if let Some(s) = self.sessions.get_mut(&op.session_id) {
                let (at, gone) = (op.at as usize, op.gone as usize);
                s.open_card_ids
                    .splice(at..at + gone, op.ids.iter().cloned());
                change.sessions.insert(op.session_id.clone());
            }
        }
        if let Some(open) = &p.permissions_open {
            if *open != self.open_permission_ids {
                moved = true
            }
            self.open_permission_ids = open.clone();
        }
        for id in &p.expired {
            if let Some(r) = self.permissions.get_mut(id) {
                if r.permission_state != "pending" {
                    continue;
                }
                r.permission_state = "expired".into();
                change.permissions.insert(id.clone());
                if let Some(s) = &r.session_id {
                    change.sessions.insert(s.clone());
                }
            }
        }
        for id in &p.hand_back_over {
            if let Some(c) = self.cards.get_mut(id) {
                if c.in_revision.is_some() {
                    c.in_revision = None;
                    change.cards.insert(id.clone());
                }
            }
        }
        if moved {
            change.stack = true
        }
        self.project_walk(change);
        self.mark_snoozes(change, now);
    }

    /// Each open card's snooze as the views show it, from the Desk's own rule (`DeskState::snooze_holds`): held off
    /// until its time (`snoozed_until`, `snoozed_at`), or back from it (`unsnoozed`). Over the snoozed cards alone.
    fn mark_snoozes(&mut self, change: &mut Change, now: u64) {
        let ids: std::collections::BTreeSet<String> = self
            .desk
            .snoozed_ids()
            .cloned()
            .chain(self.snooze_marked.iter().cloned())
            .collect();
        for id in ids {
            let (mut until, mut at, mut woken) = (None, None, None);
            let open = self.cards.get(&id).is_some_and(|c| c.object_state == "open");
            if let (true, Some(d), Some((value, _))) =
                (open, self.desk.card(&id), self.desk.snooze_of(&id))
            {
                let end = value
                    .get("until")
                    .and_then(Value::as_f64)
                    .filter(|u| u.is_finite() && *u > 0.0)
                    .map(|u| u.min(MAX_SAFE as f64) as u64);
                if self.desk.snooze_holds(d, now).is_some() {
                    until = end;
                    at = value
                        .get("at")
                        .and_then(Value::as_f64)
                        .filter(|u| u.is_finite() && *u >= 0.0)
                        .map(|u| u.min(MAX_SAFE as f64) as u64)
                        .or(Some(0));
                } else {
                    woken = end;
                }
            }
            let Some(c) = self.cards.get_mut(&id) else {
                self.snooze_marked.remove(&id);
                continue;
            };
            if (c.snoozed_until, c.snoozed_at, c.unsnoozed) != (until, at, woken) {
                c.snoozed_until = until;
                c.snoozed_at = at;
                c.unsnoozed = woken;
                change.cards.insert(id.clone());
                if let Some(s) = &c.session_id {
                    change.sessions.insert(s.clone());
                }
            }
            if until.is_some() || at.is_some() || woken.is_some() {
                self.snooze_marked.insert(id);
            } else {
                self.snooze_marked.remove(&id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A card as a device holds it that read only its newest version, at change `seen`.
    fn card(id: &str, seen: u64, sent: u64) -> Card {
        serde_json::from_value(json!({
            "object_id": id, "agent_device_id": "a1", "session_id": "s1", "object_state": "open", "urgency": "normal",
            "created_at": sent, "first_envelope_number": seen, "envelope_number": seen, "state_envelope_number": seen,
            "content_state": "ok", "card_type": "decision",
        }))
        .unwrap()
    }
    fn model(cards: Vec<Card>) -> Model {
        let mut m = Model::new();
        m.sessions.insert(
            "s1".into(),
            serde_json::from_value(json!({ "session_id": "s1" })).unwrap(),
        );
        for c in cards {
            m.sessions
                .get_mut("s1")
                .unwrap()
                .card_ids
                .push(c.object_id.clone());
            m.cards.insert(c.object_id.clone(), c);
        }
        m
    }
    fn lists(m: &Model) -> (Vec<String>, Vec<String>, Vec<String>, Vec<String>) {
        let s = &m.sessions["s1"];
        (
            m.stack.clone(),
            m.desk_order.clone(),
            s.open_card_ids.clone(),
            s.card_ids.clone(),
        )
    }

    /// The rule of the Desk's order: a card stands by the change it was first filed at, which the hub's row says.
    /// A device that knows its cards only by their newest versions orders them as one that read everything, and
    /// reading a first version later moves nothing.
    #[test]
    fn a_card_stands_by_the_hubs_first_change_before_its_past_is_read() {
        // (filed in the order c, b, a; each revised later, in the order a, b, c; the senders' clocks say b, a, c)
        let mut m = model(vec![
            card("a", 50, 200),
            card("b", 60, 100),
            card("c", 70, 300),
        ]);
        let mut ch = Change::default();
        m.set_desk_rows(Some((
            70,
            vec![
                ("a".into(), 50, 3),
                ("b".into(), 60, 2),
                ("c".into(), 70, 1),
            ],
        )));
        m.project(&mut ch, 1_000);
        let want: Vec<String> = ["c", "b", "a"].map(String::from).to_vec();
        assert_eq!(
            lists(&m),
            (want.clone(), want.clone(), want.clone(), want.clone())
        );
        assert_eq!(m.cards["a"].first_envelope_number, 3);
        assert!(ch.cards.contains("a") && ch.stack);
        // the past is read: every first number is the one the row said, nothing moves
        let mut ch = Change::default();
        for (id, first) in [("a", 3), ("b", 2), ("c", 1)] {
            let c = m.cards.get_mut(id).unwrap();
            c.first_envelope_number = c.first_envelope_number.min(first);
            c.created_at = first * 10;
            ch.cards.insert(id.into());
        }
        m.project(&mut ch, 1_001);
        assert_eq!(lists(&m), (want.clone(), want.clone(), want.clone(), want));
    }

    /// The hub's word only lowers a card's first change, and one the row does not say (0) leaves it.
    #[test]
    fn a_first_change_is_never_raised_on_the_hubs_word() {
        let mut m = model(vec![card("a", 5, 1), card("b", 6, 2), card("c", 7, 3)]);
        let mut ch = Change::default();
        m.set_desk_rows(Some((
            9,
            vec![("a".into(), 5, 90), ("b".into(), 6, 0), ("c".into(), 7, 4)],
        )));
        m.project(&mut ch, 1_000);
        assert_eq!(m.stack, ["c", "a", "b"]);
        assert_eq!(
            (
                m.cards["a"].first_envelope_number,
                m.cards["b"].first_envelope_number,
                m.cards["c"].first_envelope_number
            ),
            (5, 6, 4)
        );
        // a number a later batch moves up again (apply.rs: an envelope took its place in the hub's order) is
        // lowered again by the row still in force
        m.cards.get_mut("c").unwrap().first_envelope_number = 8;
        let mut ch = Change::default();
        ch.cards.insert("c".into());
        m.project(&mut ch, 1_001);
        assert_eq!(m.cards["c"].first_envelope_number, 4);
        assert_eq!(m.stack, ["c", "a", "b"]);
        // a card whose first version the device holds stands by that version's number, whatever a row says
        let held: CardVersion = serde_json::from_value(json!({ "version_hash": "v1", "envelope_number": 6 })).unwrap();
        m.cards.get_mut("b").unwrap().versions.push(held);
        m.set_desk_rows(Some((10, vec![("a".into(), 5, 5), ("b".into(), 6, 1), ("c".into(), 7, 4)])));
        m.project(&mut Change::default(), 1_002);
        assert_eq!(m.cards["b"].first_envelope_number, 6);
        assert_eq!(m.stack, ["c", "a", "b"]);
    }
}
