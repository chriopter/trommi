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
    fn ids_from_hex() {
        assert_eq!(id("00112233445566778899aabbccddeeff").map(|b| b[15]), Some(0xff));
        assert_eq!(id("0011"), None);
        assert_eq!(id("zz112233445566778899aabbccddeeff"), None);
        assert_eq!(unhex("abc"), None);
    }
}
