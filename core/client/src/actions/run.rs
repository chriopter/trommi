//! An action, done: checked against the board model, shown at once (an echo), sealed through the device into its
//! outbox, and remembered with the outbox entry ([`Sent`]) until the hub answers. What cannot be done is said as a
//! toast and changes nothing; what the device refuses to seal is rolled back at once with a toast.
//!
//! Sources: web `app/web/core/client.ts` (`answer`, `setRegisters`, `snooze`), iOS `RoomActions.swift`.

use super::sent::{Out, Sent};
use super::Action;
use crate::codec::{self, BodyKind};
use crate::model::change::Change;
use crate::model::types::Obj;
use crate::model::Model;
use crate::views::Text;
use serde_json::{json, Map, Value};
use trommi_core::crypto::SecretBytes;
use trommi_core::device::{Draft, Sealed};
use trommi_core::ids::{DeviceId, FileId, GroupId, ObjectId, RoomId, SessionId};
use trommi_core::Error;

/// The hour of the next morning a card is put off to ("Later" without a time), in the person's zone.
pub const SNOOZE_HOUR: u64 = 7;

const DAY_MS: u64 = 86_400_000;
/// A name of a desk or a session: at most so many characters.
pub const NAME_MAX: usize = 40;
/// A desk's goals: at most so many lines of at most `GOAL_LINE_MAX` characters, no blank lines.
pub const GOAL_LINES: usize = 20;
pub const GOAL_LINE_MAX: usize = 200;
/// Every account's first desk: the same id and value from every device, so two that write it at once write one.
pub const FIRST_DESK_ID: &str = "main";

fn first_desk() -> Value {
    json!({ "name": "Personal", "created_at": 0 })
}

fn desk_name(name: &str) -> String {
    let name: String = name.trim().chars().take(NAME_MAX).collect();
    if name.is_empty() { "Personal".into() } else { name }
}

/// A desk's goals as they are kept (web `cleanGoals`, iOS `deskRegister`).
pub fn clean_goals(text: &str) -> String {
    text.lines()
        .map(|l| l.trim().chars().take(GOAL_LINE_MAX).collect::<String>())
        .filter(|l| !l.is_empty())
        .take(GOAL_LINES)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Seals one draft through the device and pumps the outbox.
pub type Seal<'a> = dyn FnMut(&Draft, Option<&DeviceId>, &[FileId]) -> Result<Sealed, Error> + 'a;

/// What an action needs of the App.
pub struct Doing<'a> {
    pub model: &'a mut Model,
    pub sent: &'a mut Sent,
    pub room: RoomId,
    pub now: u64,
    pub utc_offset_min: i32,
    pub seal: &'a mut Seal<'a>,
    /// What the views must make again.
    pub change: Change,
}

/// Why an action was not done: the toast's words, and whether it offers the action again (the device refused to
/// seal it now; a check of the board's state is not offered again).
#[derive(Debug, PartialEq, Eq)]
pub struct NotDone {
    pub text: Text,
    pub again: bool,
}

fn not_done(key: &str, en: &str) -> NotDone {
    NotDone { text: Text::new(key, en), again: false }
}

/// The toast of a write the device or the hub refused, by the protocol's code.
pub fn refused(code: &str) -> Text {
    let mut t = Text::new("action.refused", &format!("Not saved: {}", reason(code)));
    t.args.insert("code".into(), code.into());
    t
}

/// The toast of a write that was lost before it was stored.
pub fn lost() -> Text {
    Text::new("action.lost", "Not saved: this device could not store it. Try again.")
}

fn reason(code: &str) -> &'static str {
    match code {
        "forbidden" => "not allowed",
        "not-member" | "removed-sender" => "this device is no longer in that session",
        "gone" => "the session is archived",
        "stale-session" | "busy" => "the session is being updated, try again in a moment",
        "too-large" => "too large",
        "not-found" => "it is no longer there",
        "no-key" => "this device lacks the key",
        _ => "refused",
    }
}

/// The next `SNOOZE_HOUR`:00 after `now` in a zone `utc_offset_min` east of UTC.
pub fn next_morning(now: u64, utc_offset_min: i32) -> u64 {
    let shift = i64::from(utc_offset_min) * 60_000;
    let local = (now as i64 + shift).max(0) as u64;
    let mut at = local - local % DAY_MS + SNOOZE_HOUR * 3_600_000;
    if at <= local {
        at += DAY_MS;
    }
    (at as i64 - shift).max(0) as u64
}

impl Doing<'_> {
    /// Does `action`; `Ok` also when nothing was to do.
    pub fn run(&mut self, action: &Action) -> Result<(), NotDone> {
        match action {
            Action::Answer { card_id, choices, note, attachments, option_notes, trusted } => {
                if !attachments.is_empty() {
                    return Err(not_done("action.attachments", "Attachments cannot be sent from here yet."));
                }
                self.answer(action, card_id, "answer", choices, note.as_deref(), option_notes, *trusted)
            }
            Action::Read { card_id } => self.answer(action, card_id, "read", &[], None, &Map::new(), false),
            Action::Shred { card_id, note } => {
                self.answer(action, card_id, "shred", &[], note.as_deref(), &Map::new(), false)
            }
            Action::Snooze { card_id, until_ms } => {
                self.card_of(card_id)?;
                let until = until_ms.unwrap_or_else(|| next_morning(self.now, self.utc_offset_min));
                self.registers(action, &[(format!("snooze/{card_id}"), json!({ "until": until }))])
            }
            // woken: a past until, so the row says "Back from snooze" (web and iOS alike)
            Action::Unsnooze { card_id } => {
                self.card_of(card_id)?;
                self.registers(action, &[(format!("snooze/{card_id}"), json!({ "until": self.now }))])
            }
            Action::Star { card_id, on } => {
                self.card_of(card_id)?;
                let value = if *on { json!({ "at": self.now }) } else { Value::Null };
                self.registers(action, &[(format!("star/{card_id}"), value)])
            }
            Action::DecideAgain { card_id } => self.decide_again(action, card_id),
            Action::Verdict { request_id, allow } => self.verdict(action, request_id, *allow),
            Action::Write { session_id, card_id, text, attachments } => {
                if !attachments.is_empty() {
                    return Err(not_done("action.attachments", "Attachments cannot be sent from here yet."));
                }
                let mut content = Map::new();
                content.insert("text".into(), json!(text));
                self.write(action, Some(session_id), card_id.as_deref(), content)
            }
            // "What??": the card goes back to its agent with the question, and comes back up awake
            Action::HandBack { card_id, note } => {
                let mut content = Map::new();
                content.insert("text".into(), json!(note.clone().unwrap_or_default()));
                content.insert("explain".into(), json!(true));
                self.write(action, None, Some(card_id), content)?;
                self.wake(card_id);
                Ok(())
            }
            Action::Draft { place, text } => {
                let value = if text.trim().is_empty() { Value::Null } else { json!({ "note": text, "ts": self.now }) };
                self.registers(action, &[(format!("draft/{place}"), value)])
            }
            Action::RenameSession { session_id, name } => {
                let name: String = name.trim().chars().take(NAME_MAX).collect();
                self.edit_session(action, session_id, "name", json!(name))
            }
            Action::ArchiveSession { session_id, on } => self.edit_session(action, session_id, "archived", json!(on)),
            Action::MoveSession { session_id, desk_id } => {
                if !self.desks().contains(desk_id) {
                    return Err(not_done("action.no_desk", "That desk is no longer there."));
                }
                self.edit_session(action, session_id, "desk", json!(desk_id))
            }
            Action::AddDesk { name } => {
                let mut values = Vec::new();
                // every account has its first desk, the same value from every device
                if self.desks().is_empty() {
                    values.push((format!("desk/{FIRST_DESK_ID}"), first_desk()));
                }
                let id = self.sent.new_id(self.now);
                values.push((format!("desk/{id}"), json!({ "name": desk_name(name), "created_at": self.now })));
                self.registers(action, &values)
            }
            Action::RenameDesk { desk_id, name } => {
                let mut desk = self.desk(desk_id)?;
                desk.insert("name".into(), json!(desk_name(name)));
                self.registers(action, &[(format!("desk/{desk_id}"), Value::Object(desk))])
            }
            Action::RemoveDesk { desk_id } => {
                let live = self.desks();
                if !live.contains(desk_id) {
                    return Err(not_done("action.no_desk", "That desk is no longer there."));
                }
                if live.len() <= 1 {
                    return Err(not_done("action.last_desk", "The last desk stays."));
                }
                self.registers(action, &[(format!("desk/{desk_id}"), Value::Null)])
            }
            // a field of the desk's register like its name; with no desk yet the first one is made for them
            Action::SetGoal { desk_id, text } => {
                let live = self.desks();
                let (id, mut desk) = if live.contains(desk_id) {
                    (desk_id.clone(), self.desk(desk_id)?)
                } else if live.is_empty() {
                    (FIRST_DESK_ID.to_string(), first_desk().as_object().cloned().unwrap_or_default())
                } else {
                    return Err(not_done("action.no_desk", "That desk is no longer there."));
                };
                let goals = clean_goals(text);
                if goals.is_empty() {
                    desk.remove("goals");
                } else {
                    desk.insert("goals".into(), json!(goals));
                }
                self.registers(action, &[(format!("desk/{id}"), Value::Object(desk))])
            }
            _ => Err(not_done("action.unknown", "This cannot be done here yet.")),
        }
    }

    fn card_of(&self, card_id: &str) -> Result<&crate::model::types::Card, NotDone> {
        self.model.cards.get(card_id).ok_or_else(|| not_done("action.not_found", "That card is no longer there."))
    }

    /// An answer of any kind ("answer", "read", "shred"): the card in the state the answer leaves it, at once.
    #[allow(clippy::too_many_arguments)]
    fn answer(
        &mut self,
        action: &Action,
        card_id: &str,
        how: &str,
        choices: &[String],
        note: Option<&str>,
        option_notes: &Map<String, Value>,
        trusted: bool,
    ) -> Result<(), NotDone> {
        let card = self.card_of(card_id)?;
        if card.object_state != "open" {
            return Err(not_done("action.card_closed", "That card is no longer open."));
        }
        // a card of a newer version is never answered by what this version guesses it means
        if card.unsupported.as_ref().is_some_and(crate::model::apply::truthy) {
            return Err(not_done("action.needs_update", "Update Trommi to answer this card."));
        }
        if card.content_state != "ok" || card.version_hash.is_none() {
            return Err(not_done("action.card_pruned", "This device holds only the header of this card: answer it on a device that shows it."));
        }
        let Some(session_id) = card.session_id.clone() else {
            return Err(not_done("action.not_found", "That card has no session."));
        };
        let options = card.fields.get("options");
        let options = options.as_array().map(Vec::as_slice).unwrap_or_default();
        let key_of = |o: &Value| o.get("key").and_then(Value::as_str).map(str::to_string);
        if how == "answer" && !trusted && choices.iter().any(|c| !options.iter().any(|o| key_of(o).as_deref() == Some(c))) {
            return Err(not_done("action.bad_choice", "That card has no such option."));
        }
        // An answer whose every choice is an option the agent marked final settles the card in the same step, as
        // read and shred close it. Anything said beside the choice is for the agent to read, so the card stays with
        // it; so does a trusted answer, where the agent still chooses.
        let said = |s: &str| !s.trim().is_empty();
        let plain = !note.is_some_and(said) && !option_notes.values().any(|v| v.as_str().is_some_and(said));
        let finals: Vec<String> =
            options.iter().filter(|o| o.get("final") == Some(&Value::Bool(true))).filter_map(key_of).collect();
        let settles = how == "answer" && !trusted && plain && !choices.is_empty() && choices.iter().all(|c| finals.contains(c));
        let closes = how != "answer" || settles;
        let state = if closes { "closed" } else { "answered" };
        let me = self.model.my_device_id.clone().unwrap_or_default();
        let recipient = self.model.holder_of(&card.agent_device_id, card.session_id.as_deref());
        let (version_hash, version) = (card.version_hash.clone(), card.object_version);

        let mut content = Map::new();
        content.insert("answer_action".into(), json!(how));
        content.insert("choices".into(), json!(choices));
        content.insert("answered_at".into(), json!(self.now));
        if let Some(n) = note {
            content.insert("note".into(), json!(n));
        }
        if !option_notes.is_empty() {
            content.insert("option_notes".into(), Value::Object(option_notes.clone()));
        }
        if trusted {
            content.insert("trusted".into(), json!(true));
        }
        let payload = codec::encode_body(BodyKind::Answer, &content).map_err(|_| not_done("action.bad", "That answer cannot be sent."))?;
        let files = file_ids(BodyKind::Answer, &content);
        let draft = Draft::Answer {
            session: id(&session_id).map(SessionId::new).ok_or_else(bad_id)?,
            object_id: id(card_id).map(ObjectId::new).ok_or_else(bad_id)?,
            choices: choices.to_vec(),
            closes,
            payload: SecretBytes::new(payload.into_bytes()),
        };
        let to = recipient.as_deref().and_then(hex32).map(DeviceId::new);

        let local_id = self.sent.local_id();
        let echo: Obj = serde_json::from_value(json!({ "answer_action": how, "choices": choices, "note": note, "option_notes": option_notes,
            "attachments": [], "marks": [], "trusted": trusted, "bound_version_hash": version_hash, "bound_object_version": version,
            "envelope_number": null, "envelope_hash": null, "by_device_id": me, "answered_at": self.now, "taken_back_at": null,
            "taken_back_sent_at": null }))
        .unwrap_or_default();
        self.model.echo_answer(&local_id, card_id, echo, state, &mut self.change);
        self.seal_echoed(action, &local_id, &draft, to.as_ref(), &files)?;
        // answered while put off: awake, so a reworked or reopened card is back on the Desk
        self.wake(card_id);
        Ok(())
    }

    /// Takes the answer in force back: the card is open again, and what was taken back becomes its draft (the ticks
    /// and notes where they were). A card put off is woken instead.
    fn decide_again(&mut self, action: &Action, card_id: &str) -> Result<(), NotDone> {
        let card = self.card_of(card_id)?;
        let snoozed = self.model.human.get(&format!("snooze/{card_id}")).and_then(|r| r.value.get("until")).and_then(Value::as_u64);
        if card.object_state == "open" && snoozed.is_some_and(|u| u > self.now) {
            return self.registers(action, &[(format!("snooze/{card_id}"), json!({ "until": self.now }))]);
        }
        let Some(answer) = card.answer.clone().filter(|a| a.envelope_hash.is_some() && !a.pending) else {
            return Err(not_done("action.nothing_to_take_back", "There is no answer to take back."));
        };
        let Some(session_id) = card.session_id.clone() else {
            return Err(not_done("action.not_found", "That card has no session."));
        };
        let recipient = self.model.holder_of(&card.agent_device_id, Some(&session_id));
        let decision = card.fields.get("card_type") == json!("decision");
        let payload = codec::encode_body(BodyKind::TakeBack, &Map::new()).map_err(|_| bad_id())?;
        let draft = Draft::TakeBack {
            session: id(&session_id).map(SessionId::new).ok_or_else(bad_id)?,
            object_id: id(card_id).map(ObjectId::new).ok_or_else(bad_id)?,
            payload: SecretBytes::new(payload.into_bytes()),
        };
        let to = recipient.as_deref().and_then(hex32).map(DeviceId::new);
        self.seal_plain(action, &draft, to.as_ref())?;
        let keys = answer.choices.as_array().cloned().unwrap_or_default();
        let note = answer.note.as_str().unwrap_or("").to_string();
        let notes = answer.option_notes.as_object().cloned().unwrap_or_default();
        if decision && (!keys.is_empty() || !note.is_empty() || !notes.is_empty()) {
            let was = json!({ "keys": keys, "note": note, "notes": notes, "ts": self.now });
            let _ = self.registers(action, &[(format!("draft/{card_id}"), was)]);
        }
        Ok(())
    }

    /// Allow or deny a permission request, addressed to its holder.
    fn verdict(&mut self, action: &Action, request_id: &str, allow: bool) -> Result<(), NotDone> {
        let Some(p) = self.model.permissions.get(request_id) else {
            return Err(not_done("action.not_found", "That request is no longer there."));
        };
        let Some(session_id) = p.session_id.clone() else {
            return Err(not_done("action.not_found", "That request has no session."));
        };
        let recipient = self.model.holder_of(&p.agent_device_id, Some(&session_id));
        let payload = codec::encode_body(BodyKind::Verdict, &Map::new()).map_err(|_| bad_id())?;
        let draft = Draft::Verdict {
            session: id(&session_id).map(SessionId::new).ok_or_else(bad_id)?,
            request_id: id(request_id).map(ObjectId::new).ok_or_else(bad_id)?,
            allow,
            payload: SecretBytes::new(payload.into_bytes()),
        };
        let to = recipient.as_deref().and_then(hex32).map(DeviceId::new);
        self.seal_plain(action, &draft, to.as_ref())
    }

    /// A Chat line to a session's agent, or on a card: shown at once in its timeline.
    fn write(&mut self, action: &Action, session_id: Option<&String>, card_id: Option<&str>, mut content: Map<String, Value>) -> Result<(), NotDone> {
        let card = match card_id {
            Some(c) => Some(self.card_of(c)?),
            None => None,
        };
        let Some(sid) = card.and_then(|c| c.session_id.clone()).or_else(|| session_id.cloned()) else {
            return Err(not_done("action.not_found", "That has no session to write to."));
        };
        if !self.model.sessions.contains_key(&sid) {
            return Err(not_done("action.not_found", "That session is no longer there."));
        }
        let recipient = match card {
            Some(c) => self.model.holder_of(&c.agent_device_id, Some(&sid)),
            None => self.model.recipient_of(&sid),
        };
        content.insert("content_type".into(), json!("message"));
        let payload = codec::encode_body(BodyKind::Message, &content).map_err(|_| not_done("action.bad", "That line cannot be sent."))?;
        let files = file_ids(BodyKind::Message, &content);
        let session = id(&sid).map(SessionId::new).ok_or_else(bad_id)?;
        let payload = SecretBytes::new(payload.into_bytes());
        let (draft, timeline) = match card_id {
            Some(c) => (Draft::CardChat { session, card: id(c).map(ObjectId::new).ok_or_else(bad_id)?, payload }, format!("chat:card/{c}")),
            None => (Draft::SessionChat { session, payload }, format!("chat:session/{sid}")),
        };
        let to = recipient.as_deref().and_then(hex32).map(DeviceId::new);
        let local_id = self.sent.local_id();
        self.model.echo_timeline_item(&local_id, &timeline, content, recipient, card_id.map(str::to_string), self.now, &mut self.change);
        self.change.sessions.insert(sid);
        self.seal_echoed(action, &local_id, &draft, to.as_ref(), &files)
    }

    /// A session's settings (its register `session/<id>`: name, icon, desk, archived), one field changed and every
    /// other kept.
    fn edit_session(&mut self, action: &Action, session_id: &str, field: &str, value: Value) -> Result<(), NotDone> {
        let Some(s) = self.model.sessions.get(session_id) else {
            return Err(not_done("action.not_found", "That session is no longer there."));
        };
        let mut settings = s
            .settings
            .clone()
            .or_else(|| self.model.human.get(&format!("session/{session_id}")).map(|r| r.value.clone()))
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        settings.insert(field.into(), value);
        self.change.sessions.insert(session_id.to_string());
        self.registers(action, &[(format!("session/{session_id}"), Value::Object(settings))])
    }

    /// The desks that are there, by id.
    fn desks(&self) -> Vec<String> {
        self.model
            .human
            .iter()
            .filter(|(_, r)| !r.value.is_null())
            .filter_map(|(k, _)| k.strip_prefix("desk/").map(str::to_string))
            .collect()
    }

    fn desk(&self, desk_id: &str) -> Result<Map<String, Value>, NotDone> {
        self.model
            .human
            .get(&format!("desk/{desk_id}"))
            .and_then(|r| r.value.as_object().cloned())
            .ok_or_else(|| not_done("action.no_desk", "That desk is no longer there."))
    }

    /// Seals a draft that shows no echo (a take back, a verdict: the hub's copy shows them).
    fn seal_plain(&mut self, action: &Action, draft: &Draft, to: Option<&DeviceId>) -> Result<(), NotDone> {
        match (self.seal)(draft, to, &[]) {
            Ok(sealed) => {
                self.sent.put(sealed.outbox_id, Out { local_id: None, action: action.clone() });
                Ok(())
            }
            Err(e) => Err(NotDone { text: refused(e.code()), again: true }),
        }
    }

    /// A card put off that the person acts on is woken: its snooze ends now. A failure here is not the action's.
    fn wake(&mut self, card_id: &str) {
        let key = format!("snooze/{card_id}");
        let until = self.model.human.get(&key).and_then(|r| r.value.get("until")).and_then(Value::as_u64);
        if until.is_some_and(|u| u > self.now) {
            let again = Action::Unsnooze { card_id: card_id.into() };
            let _ = self.registers(&again, &[(key, json!({ "until": self.now }))]);
        }
    }

    /// Room registers of the person, one echo and one envelope per name: a name whose sealing fails is taken back
    /// alone, and the names before it stay sent.
    fn registers(&mut self, action: &Action, values: &[(String, Value)]) -> Result<(), NotDone> {
        let group = GroupId::room(self.room);
        for (key, value) in values {
            let (name, wire) = codec::encode_register(key, value).map_err(|_| not_done("action.bad", "That cannot be saved."))?;
            let files = codec::file_ids_of(BodyKind::Register, &Map::from_iter([("value".to_string(), value.clone())]))
                .into_iter()
                .map(FileId::new)
                .collect::<Vec<_>>();
            let draft = Draft::Register { group, name, value: wire.map(|w| SecretBytes::new(w.into_bytes())) };
            let local_id = self.sent.local_id();
            self.model.echo_registers(&local_id, &Map::from_iter([(key.clone(), value.clone())]), &mut self.change);
            self.change.registers.insert(key.clone());
            self.seal_echoed(action, &local_id, &draft, None, &files)?;
        }
        Ok(())
    }

    /// Seals a draft whose echo stands under `local_id`: on success the echo learns its hash and waits with the
    /// outbox entry; a refusal of the device takes it back.
    fn seal_echoed(
        &mut self,
        action: &Action,
        local_id: &str,
        draft: &Draft,
        to: Option<&DeviceId>,
        files: &[FileId],
    ) -> Result<(), NotDone> {
        match (self.seal)(draft, to, files) {
            Ok(sealed) => {
                let hash = crate::engine::feed::hex(sealed.envelope_hash.as_bytes());
                let object = sealed.object_id.map(|o| crate::engine::feed::hex(o.as_bytes()));
                self.model.confirm_echo(local_id, &hash, sealed.seq, object.as_deref(), &mut self.change);
                self.sent.put(sealed.outbox_id, Out { local_id: Some(local_id.to_string()), action: action.clone() });
                Ok(())
            }
            Err(e) => {
                self.model.rollback_echo(local_id, &mut self.change);
                Err(NotDone { text: refused(e.code()), again: true })
            }
        }
    }
}

fn bad_id() -> NotDone {
    not_done("action.bad", "That cannot be sent.")
}

fn file_ids(kind: BodyKind, content: &Map<String, Value>) -> Vec<FileId> {
    codec::file_ids_of(kind, content).into_iter().map(FileId::new).collect()
}

/// A 16-byte id from the model's hex.
fn id(text: &str) -> Option<[u8; 16]> {
    unhex(text)?.try_into().ok()
}

/// A 32-byte id from the model's hex.
fn hex32(text: &str) -> Option<[u8; 32]> {
    unhex(text)?.try_into().ok()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_next_morning_in_the_persons_zone() {
        // Tue 13 Oct 2026 14:05 UTC
        let now = 1_791_900_300_000;
        assert_eq!(now % DAY_MS, 14 * 3_600_000 + 5 * 60_000);
        let day = now - now % DAY_MS;
        // in UTC: tomorrow 07:00
        assert_eq!(next_morning(now, 0), day + DAY_MS + 7 * 3_600_000);
        // in Berlin (summer, +2 h): tomorrow 07:00 local = 05:00 UTC
        assert_eq!(next_morning(now, 120), day + DAY_MS + 5 * 3_600_000);
        // at 03:00 local it is this morning still
        let early = day + 3 * 3_600_000;
        assert_eq!(next_morning(early, 0), day + 7 * 3_600_000);
        // exactly at 07:00: the next one
        assert_eq!(next_morning(day + 7 * 3_600_000, 0), day + DAY_MS + 7 * 3_600_000);
        // west of UTC (New York, -4 h): 14:05 UTC is 10:05 local, so tomorrow 07:00 local = 11:00 UTC
        assert_eq!(next_morning(now, -240), day + DAY_MS + 11 * 3_600_000);
    }

    #[test]
    fn goals_are_kept_clean() {
        assert_eq!(clean_goals("  one \r\n\n two\n"), "one\ntwo");
        assert_eq!(clean_goals(&"x\n".repeat(30)).lines().count(), GOAL_LINES);
        assert_eq!(clean_goals(&"y".repeat(300)).len(), GOAL_LINE_MAX);
        assert_eq!((desk_name("   "), desk_name(&"d".repeat(50)).len()), ("Personal".to_string(), NAME_MAX));
    }

    #[test]
    fn ids_from_hex() {
        assert_eq!(id("00112233445566778899aabbccddeeff").map(|b| b[15]), Some(0xff));
        assert_eq!(id("0011"), None);
        assert_eq!(id("zz112233445566778899aabbccddeeff"), None);
        assert_eq!(unhex("abc"), None);
    }
}
