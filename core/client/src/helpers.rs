//! Helpers: an agent's subagents. A helper has no session of its own: it writes through its agent's connector into
//! its agent's session group, and what it writes there carries its name (`helper`): its cards, its status lines and
//! its Chat messages (spec/v1.md 9.1). The board shows a session's helpers inside that session.
//!
//! The rules, for every app alike:
//! - a helper's name is one line of at most [`NAME_MAX`] characters; anything else names none;
//! - a card's helper is its `helper`, else the helper session of before it was filed in (`sessions::LegacyHelper`,
//!   spec/v1.md 5.2.3): those have no entry in the session list, and their cards count under their main;
//! - a session's helpers are the names on its status lines that show and on its cards that are not closed (and its
//!   helper sessions of before that are still on the Desk), in the order they first showed, each name once
//!   (letter case aside);
//! - a helper works while one of its status lines shows "working"; its open questions are its cards that wait for
//!   the person on the Desk.

use crate::model::types::{Card, StatusLine};
use serde_json::Value;
use std::collections::BTreeMap;

/// The longest name a helper has (characters).
pub const NAME_MAX: usize = 40;

/// A helper's name as an agent wrote it: one line, trimmed, at most [`NAME_MAX`] characters; none for anything else.
pub fn name_of(v: Option<&Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    if s.is_empty() || s.chars().count() > NAME_MAX || s.chars().any(char::is_control) {
        return None;
    }
    Some(s.to_string())
}

/// The helper a card comes from: its `helper`, else the name of the helper session of before it was filed in
/// (`legacy`: the helper session's key to its name).
pub fn of_card(c: &Card, legacy: &BTreeMap<String, String>) -> Option<String> {
    name_of(Some(&c.field("helper"))).or_else(|| c.session_id.as_ref().and_then(|s| legacy.get(s)).cloned())
}

/// One helper of a session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Helper {
    pub name: String,
    /// One of its status lines shows "working".
    pub working: bool,
    /// Its cards that wait for the person on the Desk.
    pub waiting: u32,
    /// When it first showed: its oldest line's or card's time.
    pub since: u64,
}

/// A card of a session with its helper's name and whether it waits for the person on the Desk.
pub type HelperCard<'c> = (&'c Card, Option<String>, bool);

/// The helpers of one session, from its status lines (`shown`: what each line shows now, by line id, status.rs
/// `line_state`; `recent`: whether a done or stale line still shows) and its cards (with `waits`: the card waits for
/// the person on the Desk), in the order they first showed.
pub fn helpers_of<'c>(
    lines: &[StatusLine],
    shown: impl Fn(&StatusLine) -> Option<&'c str>,
    cards: impl IntoIterator<Item = HelperCard<'c>>,
) -> Vec<Helper> {
    let mut out: Vec<Helper> = Vec::new();
    let mut add = |name: &str, at: u64, working: bool, waiting: bool| {
        let i = match out.iter().position(|h| h.name.to_lowercase() == name.to_lowercase()) {
            Some(i) => i,
            None => {
                out.push(Helper { name: name.to_string(), since: at, ..Default::default() });
                out.len() - 1
            }
        };
        let h = &mut out[i];
        h.since = h.since.min(at);
        h.working |= working;
        h.waiting += u32::from(waiting);
    };
    for t in lines {
        let (Some(name), Some(state)) = (t.helper.as_deref(), shown(t)) else { continue };
        add(name, t.updated_at, state == "working", false);
    }
    for (c, name, waits) in cards {
        let Some(name) = name else { continue };
        if c.object_state == "closed" {
            continue;
        }
        add(&name, c.created_at, false, waits);
    }
    out.sort_by_key(|h| h.since);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_name_is_one_short_line() {
        assert_eq!(name_of(Some(&json!("  Icons "))).as_deref(), Some("Icons"));
        assert_eq!(name_of(Some(&json!(""))), None);
        assert_eq!(name_of(Some(&json!("a\nb"))), None);
        assert_eq!(name_of(Some(&json!("x".repeat(41)))), None);
        assert_eq!(name_of(Some(&json!(3))), None);
        assert_eq!(name_of(None), None);
    }

    #[test]
    fn helpers_come_from_lines_and_cards_once_each_in_order() {
        let line = |id: &str, helper: Option<&str>, at: u64| StatusLine {
            id: id.into(),
            helper: helper.map(String::from),
            updated_at: at,
            ..Default::default()
        };
        let lines = vec![line("a", Some("Icons"), 30), line("b", None, 10), line("c", Some("Tokens"), 20), line("d", Some("Gone"), 5)];
        let card = |state: &str, at: u64| Card { object_state: state.into(), created_at: at, ..Default::default() };
        let (open, closed, other) = (card("open", 40), card("closed", 1), card("answered", 50));
        let cards = vec![(&open, Some("icons".to_string()), true), (&closed, Some("Old".to_string()), false), (&other, Some("Dark Mode".to_string()), false)];
        let shown = |t: &StatusLine| match t.id.as_str() {
            "a" => Some("working"),
            "d" => None,
            _ => Some("done"),
        };
        let h = helpers_of(&lines, shown, cards);
        let got: Vec<(&str, bool, u32)> = h.iter().map(|h| (h.name.as_str(), h.working, h.waiting)).collect();
        assert_eq!(got, [("Tokens", false, 0), ("Icons", true, 1), ("Dark Mode", false, 0)]);
    }
}
