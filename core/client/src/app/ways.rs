//! Ways that act on more than one thing, or decide what an act means before it is done: several cards at once
//! (`Action::Batch`: the duck for all, a selection; one toast, one Undo for all) and the note that saves itself
//! (`Action::KeepNote`).

use super::{App, Effect};
use crate::actions::Action;
use crate::views::Text;
use serde_json::Value;

/// What a batch does to one card of this kind ("decision", "info"), as a card's own action; none: not to it.
fn one(way: &str, kind: &str, card_id: &str) -> Option<Action> {
    let card_id = card_id.to_string();
    Some(match (way, kind) {
        ("sweep", "decision") | ("duck", "decision") => Action::Answer {
            card_id,
            choices: Vec::new(),
            note: None,
            attachments: Vec::new(),
            option_notes: Default::default(),
            trusted: true,
            marks: Vec::new(),
        },
        ("sweep", "info") | ("read", "info") => Action::Read { card_id },
        ("later", _) => Action::Snooze { card_id, until_ms: None },
        ("wake", _) => Action::Unsnooze { card_id },
        ("shred", _) => Action::Shred { card_id, note: None, attachments: Vec::new(), marks: Vec::new() },
        ("reopen", _) => Action::DecideAgain { card_id },
        _ => return None,
    })
}

impl App {
    /// `Batch`: each card its own action, then one toast with the Undo of all (none for an Undo itself).
    pub(super) fn batch(&mut self, now_ms: u64, way: &str, card_ids: &[String], out: &mut Vec<Effect>) {
        let mut done: Vec<(String, String)> = Vec::new();
        let mut ducked = false;
        let mut seen = std::collections::BTreeSet::new();
        for id in card_ids.iter().filter(|id| seen.insert(id.as_str())) {
            // (a request is never put aside here; an unknown card is left alone)
            let Some(card) = self.board.model.cards.get(id) else { continue };
            let kind = if crate::views::board::reads_as_info(card) { "info" } else { "decision" };
            let title = card.field("title").as_str().unwrap_or("").to_string();
            let Some(action) = one(way, kind, id) else { continue };
            ducked |= matches!(action, Action::Answer { .. });
            self.action(now_ms, action, out);
            done.push((id.clone(), title));
        }
        let back = match way {
            "later" => "wake",
            "wake" | "reopen" => return,
            _ => "reopen",
        };
        if done.is_empty() {
            return;
        }
        let text = match way {
            "later" => Text::new("batch.later", "Later"),
            "shred" => Text::new("batch.shred", "Shredded"),
            "read" => Text::new("batch.read", "Read"),
            "sweep" if !ducked => Text::new("batch.read", "Read"),
            _ => Text::new("batch.duck", "Left to the agents"),
        };
        let n = done.len();
        let line = if n == 1 {
            let title = if done[0].1.is_empty() { "1 card".to_string() } else { done[0].1.clone() };
            crate::views::words::say("batch.line.one", title.clone(), &[("title", &title)])
        } else {
            crate::views::words::say("batch.line", format!("{n} cards"), &[("n", &n.to_string())])
        };
        let undo = Action::Batch { way: back.into(), card_ids: done.into_iter().map(|(id, _)| id).collect() };
        self.sent.undo_toast(&mut self.next_view.toasts, text, Some(line), undo);
    }

    /// `KeepNote`: the action the note's words as they stand come to, if any.
    pub(super) fn keep_note(&mut self, note_id: Option<&str>, text: &str, attachments: &[String]) -> Option<Action> {
        let notes = &self.board.model.notes;
        let open = |id: &str| {
            notes.get(id).is_some_and(|n| {
                let set = |k: &str| n.get(k).is_some_and(crate::model::apply::truthy);
                n.get("object_state").and_then(Value::as_str) != Some("closed") && !set("removed") && !set("held")
            })
        };
        // (the note the page shows; a page that has none yet writes into the one this device made last, so that
        //  two keeps before the first one's render do not make two notes)
        let id = match note_id {
            Some(id) => Some(id.to_string()).filter(|id| open(id)),
            None => self.made_note.clone().filter(|id| open(id)),
        };
        let words = !text.trim().is_empty();
        let ready: Vec<String> = attachments.iter().filter(|a| self.files.reference(a).is_some()).cloned().collect();
        match id {
            None if !words && ready.is_empty() => None,
            None => Some(Action::WriteNoteFiles { note_id: None, text: text.to_string(), attachments: ready }),
            Some(id) if !words && attachments.is_empty() => Some(Action::DeleteNote { note_id: id }),
            Some(id) => {
                let note = notes.get(&id)?;
                let had: Vec<&str> = note
                    .get("attachments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|a| a.get("attachment_id").and_then(Value::as_str))
                    .collect();
                let wants: Vec<&str> = ready
                    .iter()
                    .filter_map(|a| self.files.reference(a).and_then(|r| r.get("attachment_id")).and_then(Value::as_str))
                    .collect();
                let same = note.get("text").and_then(Value::as_str).unwrap_or("") == text && had == wants;
                (!same).then(|| Action::WriteNoteFiles { note_id: Some(id), text: text.to_string(), attachments: ready })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::one;
    use crate::actions::Action;

    #[test]
    fn the_sweep_ducks_a_decision_and_reads_an_info() {
        assert!(matches!(one("sweep", "decision", "c"), Some(Action::Answer { trusted: true, .. })));
        assert!(matches!(one("sweep", "info", "c"), Some(Action::Read { .. })));
        assert!(one("duck", "info", "c").is_none());
        assert!(one("read", "decision", "c").is_none());
        assert!(matches!(one("later", "info", "c"), Some(Action::Snooze { until_ms: None, .. })));
        assert!(matches!(one("reopen", "decision", "c"), Some(Action::DecideAgain { .. })));
        assert!(one("nonsense", "decision", "c").is_none());
    }
}
