//! Native share inbox items: the core decides account isolation, recipients, upload completion and fallback notes.
use super::{App, Effect};
use crate::actions::{Action, SharedFile};
use crate::views::{Board, Lens, Text};

pub(super) struct Import { action: Action, files: Vec<String>, started: bool }

impl App {
    pub(super) fn queue_import(&mut self, action: &Action, out: &mut Vec<Effect>) -> bool {
        let Action::ImportShare { id, files, .. } = action else { return false };
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) || files.len() > 32 || self.imports.len() >= 16 {
            out.push(Effect::ShareInbox { id: id.clone(), consumed: false });
        } else {
            self.imports.entry(id.clone()).or_insert_with(|| Import { action: action.clone(), files: Vec::new(), started: false });
        }
        true
    }

    pub(super) fn drive_imports(&mut self, now: u64, out: &mut Vec<Effect>) {
        let Some(engine) = self.engine.as_ref() else { return };
        let room = crate::engine::feed::hex(engine.room().as_bytes());
        for id in self.imports.keys().cloned().collect::<Vec<_>>() {
            let Some(mut import) = self.imports.remove(&id) else { continue };
            let Action::ImportShare { room: from, team, to, send, text, files, .. } = &import.action else { continue };
            // A share made for a different account must never upload or send anything here.
            if from.as_ref().is_some_and(|r| r != &room) {
                out.push(Effect::ShareInbox { id, consumed: true });
                continue;
            }
            let already = self.board.model.notes.values().any(|n| n.get("import_id").and_then(serde_json::Value::as_str) == Some(&id))
                || self.board.model.timelines.values().flat_map(|t| t.items.values()).any(|item| item.get("import_id").and_then(serde_json::Value::as_str) == Some(&id));
            if already { out.push(Effect::ShareInbox { id, consumed: true }); continue; }
            if !import.started {
                for (n, SharedFile { name, media_type, bytes }) in files.iter().enumerate() {
                    let local = format!("share-{id}-{n}");
                    self.file_action(now, &Action::Attach { local_id: local.clone(), name: name.clone(), media_type: media_type.clone(), bytes: bytes.clone() }, out);
                    import.files.push(local);
                }
                import.started = true;
            }
            if import.files.iter().any(|id| self.files.upload_failed(id)) {
                out.push(Effect::ShareInbox { id, consumed: false });
                continue;
            }
            if import.files.iter().any(|id| self.files.reference(id).is_none()) {
                self.imports.insert(id, import);
                continue;
            }
            let recipient = if *send {
                let lens = Lens { team: team.clone(), ..Default::default() };
                let board = Board::new(&self.board.model, &lens, now);
                let crown = crate::views::notes::crown_on(&board, team.as_deref().filter(|d| !d.is_empty()));
                crown.map(|a| a.row.key.clone()).or_else(|| to.as_ref().filter(|to| board.agents.iter().any(|a| &a.row.key == *to && !a.row.archived && !a.row.removed)).cloned())
            } else { None };
            let mut accepted = false;
            self.doing(now, out, |doing| match doing.import_share(&import.action, &id, text, &import.files, recipient.as_ref()) {
                Ok(()) => accepted = true,
                Err(problem) => doing.sent.toast(doing.toasts, problem.text, None),
            });
            if accepted {
                let message = if recipient.is_some() { Text::new("share.sent", "Sent from the share sheet.") } else { Text::new("share.note", "Added to the note.") };
                self.sent.toast(&mut self.next_view.toasts, message, None);
            }
            out.push(Effect::ShareInbox { id, consumed: accepted });
        }
    }
}
