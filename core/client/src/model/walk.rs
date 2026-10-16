//! The walk of the model: every open card and request that waits for the person, in the Desk's order
//! (`crate::desk::walk_order`), and the next open question after a card. The apps render it; none decides it.

use super::change::Change;
use super::types::*;
use super::Model;
use crate::desk::{next_in_walk, walk_order, WalkCard};

/// A card as the walk needs it. One whose body is not read here yet stands as a quiet info (nothing to answer, no
/// knock) until it is read; one from a newer Trommi is read as an info.
fn walk_card(c: &Card) -> WalkCard {
    let newer = c.is_unsupported();
    let loading = !newer && matches!(c.content_state.as_str(), "undecryptable" | "header");
    WalkCard {
        object_id: c.object_id.clone(),
        info: loading || newer || c.field("card_type").as_str() == Some("info"),
        permission: false,
        urgency: Some(if loading { "normal".into() } else { c.urgency.clone() }),
        first_change: c.first_envelope_number,
    }
}

/// A permission request as the walk needs it.
fn walk_request(p: &PermissionRequest) -> WalkCard {
    WalkCard {
        object_id: p.object_id.clone(),
        info: false,
        permission: true,
        urgency: Some("critical".into()),
        first_change: p.envelope_number,
    }
}

/// The session a card or request belongs to, as the apps name it: its session, else its agent device.
fn key_of<'a>(session: &'a Option<String>, agent: &'a str) -> &'a str {
    session.as_deref().unwrap_or(agent)
}

impl Model {
    /// The walk, in the Desk's order: the Desk's stack (open, not held, not put off, not of an archived session) less
    /// what is with its agent (handed back, What??), and the pending permission requests. `within`: only the cards of
    /// these sessions (the team in view, by session key); None: every team.
    fn walk_cards(&self, within: Option<&[String]>) -> Vec<WalkCard> {
        let mine = |key: &str| within.is_none_or(|w| w.iter().any(|k| k == key));
        let mut out: Vec<WalkCard> = self
            .stack
            .iter()
            .filter_map(|id| self.cards.get(id))
            .filter(|c| c.in_revision.is_none() && mine(key_of(&c.session_id, &c.agent_device_id)))
            .map(walk_card)
            .chain(
                self.open_permission_ids
                    .iter()
                    .filter_map(|id| self.permissions.get(id))
                    .filter(|p| mine(key_of(&p.session_id, &p.agent_device_id)))
                    .map(walk_request),
            )
            .collect();
        walk_order(&mut out);
        out
    }

    /// The walk of every team, by id.
    pub fn walk(&self, within: Option<&[String]>) -> Vec<String> {
        self.walk_cards(within).into_iter().map(|c| c.object_id).collect()
    }

    /// The open question to offer after `card` (just decided, or still open): the next in the Desk's order among the
    /// cards of `within` (None: every team), else the first that waits; None when nothing else waits.
    pub fn next_open_after(&self, card: &str, within: Option<&[String]>) -> Option<String> {
        let me = self
            .cards
            .get(card)
            .map(walk_card)
            .or_else(|| self.permissions.get(card).map(walk_request))?;
        let walk = self.walk_cards(within);
        next_in_walk(&walk, &me).map(str::to_string)
    }

    /// The walk after a projection: named changed (`stack`) when it moved.
    pub(super) fn project_walk(&mut self, change: &mut Change) {
        let walk = self.walk(None);
        if walk != self.desk_order {
            self.desk_order = walk;
            change.stack = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn card(id: &str, urgency: &str, created: u64, info: bool) -> Card {
        serde_json::from_value(json!({
            "object_id": id, "agent_device_id": "a1", "session_id": if id.starts_with('x') { "s2" } else { "s1" },
            "object_state": "open", "urgency": urgency, "created_at": created, "first_envelope_number": created,
            "card_type": if info { "info" } else { "decision" },
        }))
        .unwrap()
    }
    fn model(cards: Vec<Card>) -> Model {
        let mut m = Model::new();
        for c in cards {
            m.stack.push(c.object_id.clone());
            m.cards.insert(c.object_id.clone(), c);
        }
        m
    }

    #[test]
    fn the_walk_by_band_then_infos_newest_then_decisions_oldest() {
        let mut m = model(vec![
            card("old", "normal", 1, false),
            card("new", "low", 5, false),
            card("note1", "normal", 2, true),
            card("note2", "normal", 4, true),
            card("knock", "high", 6, false),
            card("block", "critical", 9, false),
            card("hinfo", "high", 3, true),
        ]);
        let p: PermissionRequest = serde_json::from_value(json!({ "object_id": "perm", "agent_device_id": "a1", "sent_at": 7, "envelope_number": 7, "permission_state": "pending" })).unwrap();
        m.permissions.insert("perm".into(), p);
        m.open_permission_ids.push("perm".into());
        assert_eq!(m.walk(None), ["perm", "block", "hinfo", "knock", "note2", "note1", "old", "new"]);
    }

    #[test]
    fn with_its_agent_or_off_the_stack_is_not_in_the_walk() {
        let mut m = model(vec![card("a", "normal", 1, false), card("b", "normal", 2, false)]);
        m.cards.get_mut("a").unwrap().in_revision = Some(serde_json::from_value(json!({ "envelope_number": 3 })).unwrap());
        m.cards.insert("snoozed".into(), card("snoozed", "critical", 0, false));
        assert_eq!(m.walk(None), ["b"]);
    }

    #[test]
    fn the_next_question_after_one_decided_here() {
        let mut m = model(vec![
            card("a", "normal", 1, false),
            card("b", "normal", 2, false),
            card("c", "normal", 3, false),
            card("x", "normal", 4, false),
        ]);
        // b decided: it left the stack, the one after it in the Desk's order comes next
        m.stack.retain(|id| id != "b");
        m.cards.get_mut("b").unwrap().object_state = "answered".into();
        assert_eq!(m.next_open_after("b", None).as_deref(), Some("c"));
        // the last one decided: the walk goes round to the first
        m.stack.retain(|id| id != "x");
        assert_eq!(m.next_open_after("x", None).as_deref(), Some("a"));
        // on a team: only its sessions' cards (x is of s2)
        let s1 = vec!["s1".to_string()];
        m.stack.push("x".into());
        assert_eq!(m.next_open_after("c", Some(&s1)).as_deref(), Some("a"));
        assert_eq!(m.next_open_after("c", None).as_deref(), Some("x"));
        // still open: never itself
        assert_eq!(m.next_open_after("a", Some(&s1)).as_deref(), Some("c"));
        // nothing else waits
        let only = model(vec![card("a", "normal", 1, false)]);
        assert_eq!(only.next_open_after("a", None), None);
        assert_eq!(only.next_open_after("none", None), None);
    }

    #[test]
    fn a_more_urgent_card_decided_leads_to_the_top_of_the_rest() {
        let mut m = model(vec![card("crit", "critical", 9, false), card("n1", "normal", 1, false), card("n2", "normal", 2, false)]);
        m.stack.retain(|id| id != "crit");
        assert_eq!(m.next_open_after("crit", None).as_deref(), Some("n1"));
        // an info decided (read) leads to the decisions of its band, oldest first
        let mut m = model(vec![card("i", "normal", 5, true), card("n1", "normal", 1, false)]);
        m.stack.retain(|id| id != "i");
        assert_eq!(m.next_open_after("i", None).as_deref(), Some("n1"));
    }

    #[test]
    fn a_card_not_read_yet_waits_as_a_quiet_info() {
        let mut m = model(vec![card("n1", "normal", 1, false), card("dark", "critical", 2, false)]);
        m.cards.get_mut("dark").unwrap().content_state = "header".into();
        assert_eq!(m.walk(None), ["dark", "n1"]);
        m.cards.get_mut("dark").unwrap().content_state = "ok".into();
        assert_eq!(m.walk(None), ["dark", "n1"]);
        m.cards.get_mut("n1").unwrap().urgency = "high".into();
        assert_eq!(m.walk(None), ["dark", "n1"]);
        m.cards.get_mut("dark").unwrap().content_state = "undecryptable".into();
        assert_eq!(m.walk(None), ["n1", "dark"]);
    }

    #[test]
    fn a_null_unsupported_is_a_card_as_any() {
        let mut m = model(vec![card("i", "normal", 5, true), card("d", "normal", 1, false)]);
        m.cards.get_mut("d").unwrap().unsupported = Some(Value::Null);
        assert_eq!(m.walk(None), ["i", "d"]);
        m.cards.get_mut("d").unwrap().unsupported = Some(Value::from("card_type"));
        assert_eq!(m.walk(None), ["i", "d"]);
        m.cards.get_mut("i").unwrap().unsupported = Some(Value::Null);
        m.cards.get_mut("d").unwrap().first_envelope_number = 9;
        assert_eq!(m.walk(None), ["d", "i"]);
    }

    #[test]
    fn a_projection_names_the_walk_changed() {
        let mut m = model(vec![card("a", "normal", 1, false)]);
        let mut c = Change::new();
        m.project_walk(&mut c);
        assert!(c.stack);
        assert_eq!(m.desk_order, ["a"]);
        let mut c = Change::new();
        m.project_walk(&mut c);
        assert!(!c.stack);
    }
}
