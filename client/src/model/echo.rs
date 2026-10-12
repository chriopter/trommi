//! Own sends shown at once. The app calls echo…(…, local_id) before it seals, `confirm_echo` once the envelope is in
//! the outbox (several times for one echo that became several envelopes), and `rollback_echo` when sealing or the hub
//! refused it. When the hub's copy comes back through `apply_envelope`, the echo is replaced in place.

use super::apply::RegisterWrite;
use super::change::Change;
use super::types::*;
use super::{Echo, Model, SavedCard};
use serde_json::{json, Map, Value};

impl Model {
    /// A message or a board item, pending in its timeline's window under its local id.
    #[allow(clippy::too_many_arguments)]
    pub fn echo_timeline_item(
        &mut self,
        local_id: &str,
        timeline_key: &str,
        content: Obj,
        recipient: Option<String>,
        object_id: Option<String>,
        now: u64,
        change: &mut Change,
    ) -> TimelineItem {
        let me = self.my_device_id.clone().unwrap_or_default();
        let content_type = content
            .get("content_type")
            .filter(|v| !v.is_null())
            .cloned()
            .unwrap_or(json!("message"));
        let item = json!({ "envelope_number": null, "local_id": local_id, "pending": true, "envelope_hash": null, "sender_device_id": me,
            "sender_sequence": null, "recipient_device_id": recipient, "sent_at": now, "item_state": "loaded", "content_type": content_type,
            "content": Value::Object(content) });
        let item = item.as_object().cloned().unwrap_or_default();
        self.timeline_of(timeline_key)
            .items
            .insert(ItemKey::Local(local_id.to_string()), item.clone());
        self.echoes.insert(
            local_id.to_string(),
            Echo::Item {
                timeline_key: timeline_key.to_string(),
            },
        );
        change.timelines.insert(timeline_key.to_string());
        change.add_item(timeline_key, &item);
        if let Some(id) = object_id {
            change.cards.insert(id);
        }
        item
    }

    /// What an older echo of the same key would fall back to: the confirmed value under both.
    fn pending_base(&self, key: &str) -> Option<HumanRegister> {
        self.echoes.values().find_map(|e| match e {
            Echo::Registers { before, open } if open.iter().any(|k| k == key) => {
                Some(before.get(key).cloned().flatten())
            }
            _ => None,
        })?
    }

    /// Room registers, each value at once (`pending: true`); null deletes.
    pub fn echo_registers(
        &mut self,
        local_id: &str,
        values: &Map<String, Value>,
        change: &mut Change,
    ) {
        let mut before = std::collections::BTreeMap::new();
        let mut open = Vec::new();
        for (key, value) in values {
            let had = self.human.get(key).cloned();
            let base = if had.as_ref().is_some_and(|h| h.pending) {
                self.pending_base(key)
            } else {
                had
            };
            before.insert(key.clone(), base);
            open.push(key.clone());
            let me = self.my_device_id.clone();
            self.set_human_register(
                key,
                value.clone(),
                RegisterWrite {
                    envelope_number: None,
                    sender_device_id: me,
                    pending: true,
                    causal: None,
                },
                change,
            );
        }
        self.echoes
            .insert(local_id.to_string(), Echo::Registers { before, open });
    }

    /// An answer as card.answer with `pending: true`, the card in the state the answer leaves it. False without the card.
    pub fn echo_answer(
        &mut self,
        local_id: &str,
        object_id: &str,
        answer: Obj,
        object_state: &str,
        change: &mut Change,
    ) -> bool {
        let Some(card) = self.cards.get_mut(object_id) else {
            return false;
        };
        let saved = SavedCard {
            answer: card.answer.clone(),
            object_state: card.object_state.clone(),
            closed_how: card.closed_how.clone(),
        };
        let echo = Echo::Answer {
            object_id: object_id.to_string(),
            state: object_state.to_string(),
            version_hash: card.version_hash.clone(),
            saved: Box::new(saved),
        };
        let mut a = answer;
        a.insert("pending".into(), json!(true));
        a.insert("local_id".into(), json!(local_id));
        let action = a
            .get("answer_action")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        card.answer = Some(answer_of(a));
        card.object_state = object_state.to_string();
        if object_state == "closed" && action == "answer" {
            card.closed_how = Some("settled".into());
        }
        let session = card.session_id.clone();
        self.echoes.insert(local_id.to_string(), echo);
        change.cards.insert(object_id.to_string());
        if let Some(s) = session {
            change.sessions.insert(s);
        }
        change.stack = true;
        true
    }

    /// A new note or a new version of one, at once: `pending: true`, the confirmed note kept under it (`_base`). A
    /// new note stands under its local id until `confirm_echo` gives it its object id. Returns its key.
    pub fn echo_note(
        &mut self,
        local_id: &str,
        object_id: Option<&str>,
        fields: &Map<String, Value>,
        closed: bool,
        change: &mut Change,
    ) -> String {
        let key = object_id.unwrap_or(local_id).to_string();
        let prev = self.notes.get(&key).cloned();
        let prev_pending = prev
            .as_ref()
            .is_some_and(|p| p.get("pending") == Some(&Value::Bool(true)));
        let base: Option<Obj> = if prev_pending {
            prev.as_ref()
                .and_then(|p| p.get("_base"))
                .and_then(Value::as_object)
                .cloned()
        } else {
            prev.clone()
        };
        let mut note = base.clone().unwrap_or_default();
        if prev_pending {
            for (k, v) in prev.iter().flatten() {
                note.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in fields {
            note.insert(k.clone(), v.clone());
        }
        note.insert("object_id".into(), json!(key));
        note.insert("local_id".into(), json!(local_id));
        note.insert("pending".into(), json!(true));
        note.insert(
            "_base".into(),
            base.map(Value::Object).unwrap_or(Value::Null),
        );
        note.insert(
            "object_state".into(),
            json!(if closed { "closed" } else { "open" }),
        );
        note.insert(
            "by_device_id".into(),
            json!(self.my_device_id.clone().unwrap_or_default()),
        );
        self.notes.insert(key.clone(), note);
        self.echoes
            .insert(local_id.to_string(), Echo::Note { key: key.clone() });
        change.notes.insert(key.clone());
        key
    }
}

/// An answer from its JSON fields (an echo's: whatever the app put in it).
pub fn answer_of(mut a: Obj) -> Answer {
    let mut take = |k: &str| a.remove(k).unwrap_or(Value::Null);
    let answer_action = take("answer_action")
        .as_str()
        .unwrap_or("answer")
        .to_string();
    let choices = take("choices");
    let note = take("note");
    let option_notes = take("option_notes");
    let attachments = take("attachments");
    let marks = take("marks");
    let trusted = super::apply::truthy(&take("trusted"));
    let bound_version_hash = take("bound_version_hash").as_str().map(str::to_string);
    let bound_object_version = take("bound_object_version").as_i64().unwrap_or(0);
    let envelope_number = take("envelope_number").as_u64();
    let envelope_hash = take("envelope_hash").as_str().map(str::to_string);
    let by_device_id = take("by_device_id").as_str().unwrap_or("").to_string();
    let answered_at = take("answered_at").as_u64().unwrap_or(0);
    let taken_back_at = take("taken_back_at").as_u64();
    let taken_back_sent_at = take("taken_back_sent_at").as_u64();
    let pending = super::apply::truthy(&take("pending"));
    Answer {
        answer_action,
        choices,
        note,
        option_notes,
        attachments,
        marks,
        trusted,
        bound_version_hash,
        bound_object_version,
        envelope_number,
        envelope_hash,
        by_device_id,
        answered_at,
        taken_back_at,
        taken_back_sent_at,
        pending,
        extra: a,
    }
}

impl Model {
    /// The envelope of an echo is sealed: its hash will name the hub's copy; an item learns its number in the
    /// sender's chain, a new note its object id.
    pub fn confirm_echo(
        &mut self,
        local_id: &str,
        hash: &str,
        seq: u64,
        object_id: Option<&str>,
        change: &mut Change,
    ) {
        let Some(echo) = self.echoes.get(local_id).cloned() else {
            return;
        };
        self.by_hash.insert(hash.to_string(), local_id.to_string());
        match echo {
            Echo::Item { timeline_key } => {
                if let Some(item) = self
                    .timelines
                    .get_mut(&timeline_key)
                    .and_then(|t| t.items.get_mut(&ItemKey::Local(local_id.to_string())))
                {
                    item.insert("sender_sequence".into(), json!(seq));
                    item.insert("envelope_hash".into(), json!(hash));
                    let copy = item.clone();
                    change.add_item(&timeline_key, &copy);
                    change.timelines.insert(timeline_key);
                }
            }
            Echo::Note { key } => {
                let Some(id) = object_id else { return };
                if key == id {
                    return;
                }
                if let Some(mut note) = self.notes.remove(&key) {
                    note.insert("object_id".into(), json!(id));
                    self.notes.insert(id.to_string(), note);
                    change.notes.insert(key);
                    change.notes.insert(id.to_string());
                    self.echoes.insert(
                        local_id.to_string(),
                        Echo::Note {
                            key: id.to_string(),
                        },
                    );
                }
            }
            _ => {}
        }
    }

    /// Take an echo back (sealing failed, the hub refused it, its void record came): what it showed is as it was.
    pub fn rollback_echo(&mut self, local_id: &str, change: &mut Change) {
        let Some(echo) = self.echoes.remove(local_id) else {
            return;
        };
        self.by_hash.retain(|_, id| id != local_id);
        match echo {
            Echo::Item { timeline_key } => {
                if let Some(t) = self.timelines.get_mut(&timeline_key) {
                    if t.items
                        .remove(&ItemKey::Local(local_id.to_string()))
                        .is_some()
                    {
                        change.timelines.insert(timeline_key);
                    }
                }
            }
            Echo::Registers { before, open } => {
                for key in open {
                    let old = before.get(&key).cloned().flatten();
                    let (value, w) = match old {
                        Some(o) => (
                            o.value,
                            RegisterWrite {
                                envelope_number: o.envelope_number,
                                sender_device_id: o.by_device_id,
                                pending: false,
                                causal: o.causal,
                            },
                        ),
                        None => (Value::Null, RegisterWrite::default()),
                    };
                    self.set_human_register(&key, value, w, change);
                }
            }
            Echo::Answer {
                object_id,
                state,
                version_hash,
                saved,
            } => {
                let Some(card) = self.cards.get_mut(&object_id) else {
                    return;
                };
                // Only what the echo itself set; a newer state that arrived meanwhile stays. (The saved answer is read
                // again from the card's answers: what came since, a take back, shows.)
                let saved_answer = saved.answer.map(|a| match &a.envelope_hash {
                    Some(h) if !a.pending => card
                        .answers
                        .iter()
                        .find(|x| x.envelope_hash.as_deref() == Some(h.as_str()))
                        .cloned()
                        .unwrap_or(a),
                    _ => a,
                });
                let mine = card
                    .answer
                    .as_ref()
                    .and_then(|a| a.extra.get("local_id"))
                    .and_then(Value::as_str)
                    == Some(local_id);
                if mine {
                    card.answer = saved_answer.clone();
                }
                if card.object_state == state
                    && card.version_hash == version_hash
                    && card.answer == saved_answer
                {
                    card.object_state = saved.object_state;
                    card.closed_how = saved.closed_how;
                }
                let session = card.session_id.clone();
                change.cards.insert(object_id);
                if let Some(s) = session {
                    change.sessions.insert(s);
                }
                change.stack = true;
            }
            Echo::Note { key } => {
                let cur = self.notes.get(&key).cloned();
                if let Some(cur) = cur {
                    let pending = cur.get("pending") == Some(&Value::Bool(true));
                    if pending && cur.get("local_id").and_then(Value::as_str) == Some(local_id) {
                        match cur.get("_base").and_then(Value::as_object) {
                            Some(base) => {
                                self.notes.insert(key.clone(), base.clone());
                            }
                            None => {
                                self.notes.remove(&key);
                            }
                        }
                        change.notes.insert(key);
                    }
                }
            }
        }
    }
}
