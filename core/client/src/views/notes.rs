//! The person's notes: the one yellow note in the corner (the newest unsent one), what it holds, and whom it goes to.
//!
//! The rules are the web's of before (`notes.mjs` `boardNotes`, `sidebar.mjs` `cornerNote`, `deskNotesOf`, `app.mjs`
//! `crownOf`):
//! - a note shows while it is open, not removed, not held (a sent note waits a moment for the toast's Undo: every page
//!   hides it), and not a new one still under its local id (the page that made it gets it by its object id);
//! - the last written first; the corner shows the first of them, else an empty one;
//! - it goes to the crowned session of the desk in view; on All desks to the crown of the single desk chosen last,
//!   else the first desk's crown that there is, and with two crowned desks or more the person picks one.
//!
//! Notes are the room's, not a desk's: every desk shows the same ones.

use super::board::{Agent, Board};
use super::words::say;
use super::{NoteFileView, NoteToView, NoteView, NotesView};
use crate::model::types::Note;
use serde_json::Value;

fn text_of<'n>(n: &'n Note, key: &str) -> &'n str {
    n.get(key).and_then(Value::as_str).unwrap_or("")
}

fn set(n: &Note, key: &str) -> bool {
    n.get(key).is_some_and(crate::model::apply::truthy)
}

/// Whether a note shows on any page.
fn shows(n: &Note) -> bool {
    let id = text_of(n, "object_id");
    text_of(n, "object_state") != "closed"
        && !set(n, "removed")
        && !set(n, "held")
        && !(set(n, "pending") && !id.is_empty() && n.get("local_id").and_then(Value::as_str) == Some(id))
}

/// A file of a note: a picture shows as a small thumbnail, any other file by its name.
fn file_of(a: &Value) -> NoteFileView {
    super::files_seen::saw(a);
    let s = |k: &str| a.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let name = a.get("file_name").and_then(Value::as_str).unwrap_or("file").to_string();
    let media_type = s("media_type");
    let lower = name.to_ascii_lowercase();
    let picture = media_type.starts_with("image/")
        || [".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg"].iter().any(|e| lower.ends_with(e));
    NoteFileView { file_id: s("attachment_id"), name, media_type, picture }
}

fn note_of(n: &Note) -> NoteView {
    let num = |k: &str| n.get(k).and_then(Value::as_u64).unwrap_or(0);
    NoteView {
        id: text_of(n, "object_id").to_string(),
        text: text_of(n, "text").to_string(),
        files: n.get("attachments").and_then(Value::as_array).map(|l| l.iter().map(file_of).collect()).unwrap_or_default(),
        created_ms: num("created_at"),
        updated_ms: num("updated_at"),
        pending: set(n, "pending"),
    }
}

fn to_of(a: &Agent, b: &Board) -> NoteToView {
    let name = a.row.name.trim();
    let desk = a.desk.as_deref().and_then(|d| b.desks.iter().find(|x| x.id == d));
    NoteToView {
        session_id: a.row.key.clone(),
        // (never an empty word: a session without a name yet is the desk's Main Session)
        name: if name.is_empty() { "Main Session".into() } else { name.to_string() },
        mark: if a.row.icon.is_empty() { a.row.id.clone() } else { a.row.icon.clone() },
        desk_id: a.desk.clone().unwrap_or_default(),
        desk: desk.map(|d| if d.name.is_empty() { "Personal".to_string() } else { d.name.clone() }).unwrap_or_default(),
    }
}

/// The crowned session a desk's note goes to (none without a crown on it).
fn crown_on<'b>(b: &'b Board, desk: Option<&str>) -> Option<&'b Agent> {
    b.agents.iter().find(|a| a.crowned && !a.row.archived && !a.row.removed && (desk.is_none() || a.desk.as_deref() == desk))
}

pub fn notes(b: &Board) -> NotesView {
    let mut shown: Vec<&Note> = b.model.notes.values().filter(|n| shows(n)).collect();
    // (the last written first; of two written at once, the one the hub filed last)
    shown.sort_by_key(|n| {
        let num = |k: &str| n.get(k).and_then(Value::as_u64).unwrap_or(0);
        std::cmp::Reverse((num("updated_at"), num("envelope_number")))
    });
    let rows: Vec<NoteView> = shown.into_iter().map(note_of).collect();

    // Whom it goes to.
    let (to, choose, far) = if b.all {
        let crowns: Vec<&Agent> = b.desks.iter().filter_map(|d| crown_on(b, Some(&d.id))).collect();
        let home = b.lens.home_desk.as_deref().filter(|h| b.desks.iter().any(|d| d.id == *h));
        let own = home.and_then(|h| crown_on(b, Some(h)));
        let to = own.or(crowns.first().copied());
        // (All desks, and the desk chosen last has no crown: the note still goes to another desk's crowned session)
        let far = to.is_some() && own.is_none() && crowns.len() == 1;
        let choose = if crowns.len() > 1 { crowns.iter().map(|a| to_of(a, b)).collect() } else { Vec::new() };
        (to, choose, far)
    } else {
        let desk = if b.desks.is_empty() { None } else { b.desk.as_deref() };
        (crown_on(b, desk).filter(|a| a.on_desk), Vec::new(), false)
    };
    let to = to.map(|a| to_of(a, b));
    let first = rows.first();
    let written = first.is_some_and(|n| !n.text.trim().is_empty() || !n.files.is_empty());
    NotesView {
        notes: rows.iter().map(|n| n.id.clone()).collect(),
        corner: first.map(|n| n.id.clone()),
        written,
        says: if written { say("note.open", "Your note: open it", &[]) } else { say("note.new", "New note", &[]) },
        send_says: to.as_ref().map(|t| say("note.send_to", format!("Send to {}", t.name), &[("name", &t.name)])),
        no_crown: to.is_none().then(|| say("note.no_crown", "Give a session the crown to send", &[])),
        to,
        far,
        choose,
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{dev, Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::json;

    fn view(r: &Room, lens: &Lens) -> NotesView {
        let m = r.model(NOW);
        notes(&Board::new(&m, lens, NOW))
    }
    fn note(id: &str, text: &str, updated: u64) -> Value {
        json!({ "object_id": id, "text": text, "object_state": "open", "created_at": NOW - 60 * MIN, "updated_at": updated, "envelope_number": 1 })
    }
    fn lens(desk: Option<&str>) -> Lens {
        Lens { desk: desk.map(String::from), ..Default::default() }
    }

    #[test]
    fn the_newest_unsent_note_stands_in_the_corner() {
        let mut r = Room::default();
        r.notes.push(note("old", "first", NOW - 30 * MIN));
        r.notes.push(note("new", "second", NOW - 5 * MIN));
        let mut held = note("held", "sent a moment ago", NOW - MIN);
        held["held"] = json!({ "to": dev("a"), "until": NOW + 3000 });
        r.notes.push(held);
        let mut closed = note("closed", "thrown away", NOW);
        closed["object_state"] = json!("closed");
        r.notes.push(closed);
        let mut made = note("l1", "being sealed", NOW);
        made["pending"] = json!(true);
        made["local_id"] = json!("l1");
        r.notes.push(made);
        let v = view(&r, &lens(None));
        assert_eq!(v.notes, ["new", "old"]);
        assert_eq!(v.corner.as_deref(), Some("new"));
        assert!(v.written);
        assert_eq!(v.says.en, "Your note: open it");
        // an empty corner note is not written
        r.notes = vec![note("e", "  ", NOW)];
        let v = view(&r, &lens(None));
        assert!(!v.written);
        assert_eq!(v.says.en, "New note");
        // a note of pictures alone is written
        r.notes[0]["attachments"] = json!([{ "attachment_id": "p", "file_name": "shot.PNG", "media_type": "" }, { "attachment_id": "d", "file_name": "a.pdf", "media_type": "application/pdf" }]);
        let v = view(&r, &lens(None));
        assert!(v.written);
        assert_eq!((v.rows[0].files[0].picture, v.rows[0].files[1].picture), (true, false));
    }

    #[test]
    fn it_goes_to_the_crown_of_the_desk_in_view() {
        let mut r = Room { desks: json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "", "created_at": 2 } }), ..Default::default() };
        r.session("a", None, true);
        r.session("b", None, true);
        r.on_desk("b", "lab");
        let v = view(&r, &lens(None));
        assert!(v.to.is_none());
        assert_eq!(v.no_crown.unwrap().en, "Give a session the crown to send");
        r.desks["main"]["crown"] = json!({ "agent_device_id": dev("a") });
        let v = view(&r, &lens(None));
        assert_eq!(v.to.as_ref().unwrap().name, "a");
        assert_eq!(v.send_says.unwrap().en, "Send to a");
        // the other desk has no crown
        assert!(view(&r, &lens(Some("lab"))).to.is_none());
        // All desks: one crowned desk, not the one chosen last: it goes there, named
        let all = Lens { desk: Some("all".into()), home_desk: Some("lab".into()), ..Default::default() };
        let v = view(&r, &all);
        assert_eq!(v.to.as_ref().map(|t| t.name.as_str()), Some("a"));
        assert!(v.far);
        assert!(v.choose.is_empty());
        // two crowned desks: the person picks; the one chosen last is the default
        r.desks["lab"]["crown"] = json!({ "agent_device_id": dev("b") });
        let v = view(&r, &all);
        assert_eq!(v.to.as_ref().map(|t| t.name.as_str()), Some("b"));
        assert!(!v.far);
        assert_eq!(v.choose.iter().map(|t| (t.desk.as_str(), t.name.as_str())).collect::<Vec<_>>(), [("Work", "a"), ("Personal", "b")]);
    }
}
