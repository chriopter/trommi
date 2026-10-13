//! Deleting a desk: one rule for every app, so that a delete begun on one device finishes on any other.
//!
//! A desk holds the sessions that stand on it (their cards, Chats and Artifacts) and its Scribble Board. Deleting it
//! takes all of that away, with steps the protocol already has, one after the other:
//!
//! 1. `Mark`: the desk's register gets `deleting: { at, by, total }`. From then on every human device knows the desk
//!    is going (`desks_deleting`) and finishes the delete on its next start, whichever device began it.
//! 2. `Shred`: every open card of its sessions is shredded (an answer, as Shred on the Desk).
//! 3. `Archive`: its live sessions, helpers too, go to the archive (the register `session/<key>` and the hub's
//!    `POST /v1/groups/{group}/archive`, spec 5.2.10).
//! 4. `RemoveDevices`: the agent devices that carried only sessions of this desk leave the room (spec 5.2.8; their
//!    connector wipes its keys once it verified the removal, 13.5). A device with a live session on another desk
//!    stays, and so does every person's device.
//! 5. `LoadBoard`, `EraseBoard`: the desk's board is read, then every shape on it is erased (spec 10.5 `erase`).
//! 6. `DeleteFiles`: every file the desk's cards, answers, Chats in memory, Artifacts and board pictures name is
//!    deleted at the hub at once (`DELETE /v1/files/{file}`, a human device may delete any file; an answer
//!    `not-found` counts as done).
//! 7. `WriteSnapshot`: a snapshot of the empty board, so the hub prunes the board's items (10.9).
//! 8. `RemoveDesk`: the desk's register is taken away (`desk/<id>` null).
//!
//! The hub drops the sealed copies itself: a settled object's bodies and an archived group's messages after 30 days
//! (spec 9.4.1, 14.3), the board's items once a snapshot covers them (9.4.3).
//!
//! The planner holds no IO and no state of its own: every step is decided from the model and what the app knows of
//! the board (`Known`), so a step that was done already (on this device or another) is never asked for again, and a
//! delete broken off anywhere goes on from where it stands. The app does the step it is given and asks again.
use crate::model::Model;
use crate::sessions::{session_list, SessionRow};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// What the app knows of the desk's board and of its own deletes, at one look.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Known {
    /// The desk's board as read on this device; none while it is not read yet.
    pub board: Option<BoardKnown>,
    /// Files this device deleted at the hub (hex), or that the hub answered `not-found` for.
    pub files_gone: BTreeSet<String>,
}

/// The desk's board as the app read it.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct BoardKnown {
    /// The ids of every shape on it now.
    pub shapes: Vec<String>,
    /// The files of its pictures (and posters), hex.
    pub files: Vec<String>,
    /// How many of its shapes are notes or text.
    pub notes: u64,
    /// The newest snapshot of the board holds no shape and covers every item (written after the erase).
    pub snapshot_empty: bool,
}

/// What a desk holds, counted (the confirm names these, and the delete's progress counts them down).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Counts {
    /// Main sessions that are live, helpers that are live, and archived sessions of either kind.
    pub sessions: u64,
    pub helpers: u64,
    pub archived: u64,
    pub open_cards: u64,
    pub done_cards: u64,
    /// Agent devices that leave the room with it.
    pub agents: u64,
    /// Shapes on its board; of them pictures and notes. None: the board is not read yet.
    pub board: Option<u64>,
    pub pictures: Option<u64>,
    pub notes: Option<u64>,
    /// Files that are deleted with it (cards, Chats in memory, Artifacts, board pictures).
    pub files: u64,
    pub artifacts: u64,
    /// Lines of its goals.
    pub goals: u64,
}

/// One step of the delete.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// Write this value to the human register `register` (the desk's, with `deleting`).
    Mark { register: String, value: Value },
    /// Shred these open cards (object ids, hex).
    Shred { object_ids: Vec<String> },
    /// Archive these sessions (keys as the session list has them, with their group if known).
    Archive { sessions: Vec<ArchiveOne> },
    /// Remove these devices from the room (hex).
    RemoveDevices { device_ids: Vec<String> },
    /// Read the desk's board, then ask again with `Known::board`.
    LoadBoard { desk_id: String },
    /// Erase these shapes on the desk's board.
    EraseBoard { shape_ids: Vec<String> },
    /// Delete these files at the hub (hex); `not-found` is done too.
    DeleteFiles { file_ids: Vec<String> },
    /// Write a snapshot of the (now empty) board.
    WriteSnapshot { desk_id: String },
    /// Take the desk's register away (write null).
    RemoveDesk { register: String },
    /// Nothing left: the desk is gone.
    Done,
    /// The desk cannot be deleted: `no-desk` (no such desk), `last-desk` (every account keeps one).
    Refused { code: String },
}

/// A session to archive.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArchiveOne {
    pub key: String,
    pub session_id: Option<String>,
    pub group_id: Option<String>,
}

/// The next step, how far the delete is, and what is left.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Plan {
    #[serde(flatten)]
    pub step: Step,
    /// Units done of `total` (the total the mark keeps from the start; before the mark, what is there now).
    pub done: u64,
    pub total: u64,
    pub left: Counts,
}

/// The register of a desk.
pub fn register_of(desk_id: &str) -> String {
    format!("desk/{desk_id}")
}

/// The desks that are there now (a register with a value), with their values.
fn desks_of(m: &Model) -> BTreeMap<String, &Value> {
    m.human
        .iter()
        .filter_map(|(k, r)| k.strip_prefix("desk/").map(|id| (id.to_string(), &r.value)))
        .filter(|(_, v)| v.is_object())
        .collect()
}

/// Every file id (hex) a value names: `attachment_id` and `poster_attachment_id` of 32 hex, at any depth.
fn files_in(v: &Value, out: &mut BTreeSet<String>) {
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                if (k == "attachment_id" || k == "poster_attachment_id") && x.as_str().is_some_and(is_hex32) {
                    out.insert(x.as_str().unwrap_or_default().to_ascii_lowercase());
                } else {
                    files_in(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| files_in(x, out)),
        _ => {}
    }
}

fn is_hex32(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Model {
    /// The desks a delete was begun on (their ids): an app finishes each on its start (`desk_delete_plan`).
    pub fn desks_deleting(&self) -> Vec<String> {
        desks_of(self)
            .into_iter()
            .filter(|(_, v)| v.get("deleting").is_some_and(Value::is_object))
            .map(|(id, _)| id)
            .collect()
    }

    /// What desk `desk_id` holds, counted (for the confirm). `board`: the desk's board as read, if it is.
    pub fn desk_contents(&self, desk_id: &str, board: Option<&BoardKnown>) -> Counts {
        let rows = session_list(self, false);
        let (counts, _, _) = self.desk_left(desk_id, &rows, board, &BTreeSet::new());
        counts
    }

    /// The next step of deleting desk `desk_id`, by `by` (this device, hex) at `now`.
    pub fn desk_delete_plan(&self, desk_id: &str, known: &Known, by: &str, now: u64) -> Plan {
        let desks = desks_of(self);
        let register = register_of(desk_id);
        let rows = session_list(self, false);
        let (left, units, work) = self.desk_left(desk_id, &rows, known.board.as_ref(), &known.files_gone);
        let plan = |step: Step, total: u64| Plan {
            step,
            done: total.saturating_sub(units),
            total,
            left: left.clone(),
        };
        let Some(value) = desks.get(desk_id) else {
            // (gone: done, by this device or another; a desk that never was cannot be begun)
            return plan(
                if self.human.contains_key(&register) {
                    Step::Done
                } else {
                    Step::Refused { code: "no-desk".into() }
                },
                0,
            );
        };
        let marked = value.get("deleting").and_then(Value::as_object);
        if marked.is_none() && desks.len() <= 1 {
            return plan(
                Step::Refused {
                    code: "last-desk".into(),
                },
                units,
            );
        }
        let Some(mark) = marked else {
            let mut v = (*value).clone();
            if let Some(o) = v.as_object_mut() {
                o.insert("deleting".into(), json!({ "at": now, "by": by, "total": units }));
            }
            return plan(Step::Mark { register, value: v }, units);
        };
        let total = mark.get("total").and_then(Value::as_u64).unwrap_or(units).max(units);
        let step = if !work.shred.is_empty() {
            Step::Shred { object_ids: work.shred }
        } else if !work.archive.is_empty() {
            Step::Archive { sessions: work.archive }
        } else if !work.devices.is_empty() {
            Step::RemoveDevices {
                device_ids: work.devices,
            }
        } else if known.board.is_none() {
            Step::LoadBoard {
                desk_id: desk_id.to_string(),
            }
        } else if known.board.as_ref().is_some_and(|b| !b.shapes.is_empty()) {
            Step::EraseBoard {
                shape_ids: known.board.as_ref().map(|b| b.shapes.clone()).unwrap_or_default(),
            }
        } else if !work.files.is_empty() {
            Step::DeleteFiles { file_ids: work.files }
        } else if !known.board.as_ref().is_some_and(|b| b.snapshot_empty) {
            Step::WriteSnapshot {
                desk_id: desk_id.to_string(),
            }
        } else {
            Step::RemoveDesk { register }
        };
        plan(step, total)
    }

    /// What is left on the desk: the counts, the units of work left (for the progress) and the work itself.
    fn desk_left(
        &self,
        desk_id: &str,
        rows: &[SessionRow],
        board: Option<&BoardKnown>,
        gone: &BTreeSet<String>,
    ) -> (Counts, u64, Work) {
        let here: Vec<&SessionRow> = rows
            .iter()
            .filter(|r| r.desk == desk_id && !r.removed && !r.own)
            .collect();
        let keys: BTreeSet<&str> = here.iter().map(|r| r.key.as_str()).collect();
        let sids: BTreeSet<&str> = here.iter().filter_map(|r| r.session_id.as_deref()).collect();
        let mains: BTreeSet<&str> = here
            .iter()
            .filter(|r| r.main)
            .filter_map(|r| r.agent_device_id.as_deref())
            .collect();
        let of_desk = |sid: Option<&str>, dev: &str| match sid {
            Some(s) if !s.is_empty() => sids.contains(s) || keys.contains(s),
            _ => mains.contains(dev) || keys.contains(dev),
        };
        let mut c = Counts::default();
        let mut w = Work::default();
        let mut files = BTreeSet::new();
        for r in here.iter() {
            match (r.archived, r.parent.is_some() || !r.main) {
                (true, _) => c.archived += 1,
                (false, true) => c.helpers += 1,
                (false, false) => c.sessions += 1,
            }
            if !r.archived {
                let group = r
                    .session_id
                    .as_deref()
                    .and_then(|s| self.sessions.get(s))
                    .and_then(|s| s.group_id.clone());
                w.archive.push(ArchiveOne {
                    key: r.key.clone(),
                    session_id: r.session_id.clone(),
                    group_id: group,
                });
            }
        }
        for card in self
            .cards
            .values()
            .filter(|k| of_desk(k.session_id.as_deref(), &k.agent_device_id))
        {
            if card.object_state == "open" {
                c.open_cards += 1;
                w.shred.push(card.object_id.clone());
            } else {
                c.done_cards += 1;
            }
            files_in(&card.fields.get("attachments"), &mut files);
            for a in card.answers.iter().chain(card.answer.iter()) {
                files_in(&a.attachments, &mut files);
            }
            if let Some(t) = self.timelines.get(&card.timeline_key) {
                t.items
                    .values()
                    .for_each(|it| it.values().for_each(|v| files_in(v, &mut files)));
            }
        }
        for p in self
            .published
            .values()
            .filter(|p| of_desk(p.session_id.as_deref(), &p.agent_device_id))
        {
            c.artifacts += 1;
            files_in(&p.attachments, &mut files);
        }
        for r in here.iter() {
            if let Some(s) = r.session_id.as_deref().and_then(|s| self.sessions.get(s)) {
                if let Some(t) = self.timelines.get(&s.timeline_key) {
                    t.items
                        .values()
                        .for_each(|it| it.values().for_each(|v| files_in(v, &mut files)));
                }
            }
        }
        // (an agent device leaves only when nothing live of it stands on another desk; never a person's device)
        let elsewhere: BTreeSet<&str> = rows
            .iter()
            .filter(|r| r.desk != desk_id && !r.archived && !r.removed)
            .filter_map(|r| r.agent_device_id.as_deref())
            .collect();
        let devices: BTreeSet<String> = here
            .iter()
            .filter_map(|r| r.agent_device_id.as_deref())
            .filter(|d| !elsewhere.contains(d))
            .filter(|d| {
                self.members
                    .get(*d)
                    .is_some_and(|m| m.device_role != "human" && m.is_active && !m.is_me)
            })
            .map(String::from)
            .collect();
        c.agents = devices.len() as u64;
        w.devices = devices.into_iter().collect();
        if let Some(b) = board {
            c.board = Some(b.shapes.len() as u64);
            c.pictures = Some(b.files.len() as u64);
            c.notes = Some(b.notes);
            files.extend(b.files.iter().filter(|f| is_hex32(f)).map(|f| f.to_ascii_lowercase()));
        }
        w.files = files.into_iter().filter(|f| !gone.contains(f)).collect();
        c.files = w.files.len() as u64;
        c.goals = desks_of(self)
            .get(desk_id)
            .and_then(|v| v.get("goals"))
            .and_then(Value::as_str)
            .map_or(0, |g| g.lines().filter(|l| !l.trim().is_empty()).count() as u64);
        // Units: one per card to shred, session to archive, device to remove, shape to erase, file to delete, and one
        // each for the snapshot and the register.
        let units = (w.shred.len() + w.archive.len() + w.devices.len() + w.files.len()) as u64
            + board.map_or(0, |b| b.shapes.len() as u64)
            + u64::from(!board.is_some_and(|b| b.snapshot_empty))
            + u64::from(desks_of(self).contains_key(desk_id));
        (c, units, w)
    }
}

#[derive(Debug, Default)]
struct Work {
    shred: Vec<String>,
    archive: Vec<ArchiveOne>,
    devices: Vec<String>,
    files: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Card, HumanRegister, Member, Published};

    const ME: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
    const A1: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    const A2: &str = "a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2a2";
    const A3: &str = "a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3a3";
    const S1: &str = "51515151515151515151515151515151";
    const S2: &str = "52525252525252525252525252525252";
    const S3: &str = "53535353535353535353535353535353";
    const F1: &str = "f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1f1";
    const F2: &str = "f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2f2";
    const F3: &str = "f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3f3";

    fn member(m: &mut Model, id: &str, role: &str) {
        m.members.insert(
            id.into(),
            Member {
                device_id: id.into(),
                device_role: role.into(),
                is_active: true,
                is_me: id == ME,
                ..Default::default()
            },
        );
    }
    fn session(m: &mut Model, sid: &str, dev: &str, parent: Option<&str>, desk: &str) {
        let s = m.session_of(sid);
        s.group_id = Some(format!("g{sid}"));
        s.agent_device_id = Some(dev.into());
        s.agent_device_ids = vec![dev.into()];
        s.is_active = true;
        let mut p = serde_json::Map::new();
        p.insert("is_main".into(), json!(parent.is_none()));
        if let Some(pa) = parent {
            p.insert("parent_session".into(), json!(pa));
            s.parent_session_id = Some(pa.into());
            s.creator_device_id = Some(dev.into());
        }
        s.profile = Some(p);
        if parent.is_none() {
            set(m, &format!("session/{sid}"), json!({ "desk": desk }));
        }
    }
    fn set(m: &mut Model, key: &str, value: Value) {
        m.human.insert(
            key.into(),
            HumanRegister {
                value,
                ..Default::default()
            },
        );
    }
    fn card(m: &mut Model, id: &str, sid: &str, dev: &str, state: &str, file: Option<&str>) {
        let mut c = Card {
            object_id: id.into(),
            session_id: Some(sid.into()),
            agent_device_id: dev.into(),
            object_state: state.into(),
            ..Default::default()
        };
        if let Some(f) = file {
            c.fields.insert("attachments", json!([{ "attachment_id": f }]));
        }
        m.cards.insert(id.into(), c);
    }

    /// Two desks: "main" with S2 (on A2), "lab" with S1 (on A1) and its helper S3 (on A3), two cards, an Artifact.
    fn room() -> Model {
        let mut m = Model::new();
        member(&mut m, ME, "human");
        for a in [A1, A2, A3] {
            member(&mut m, a, "agent");
        }
        set(&mut m, "desk/main", json!({ "name": "Personal", "created_at": 1 }));
        set(
            &mut m,
            "desk/lab",
            json!({ "name": "Lab", "created_at": 2, "goals": "Ship it\n\nTest it" }),
        );
        session(&mut m, S1, A1, None, "lab");
        session(&mut m, S3, A3, Some(S1), "lab");
        session(&mut m, S2, A2, None, "main");
        card(&mut m, "c1", S1, A1, "open", Some(F1));
        card(&mut m, "c2", S1, A1, "closed", None);
        card(&mut m, "c3", S2, A2, "open", None);
        m.published.insert(
            "p1".into(),
            Published {
                object_id: "p1".into(),
                agent_device_id: A3.into(),
                session_id: Some(S3.into()),
                attachments: json!([{ "attachment_id": F2 }]),
                ..Default::default()
            },
        );
        m
    }
    fn board(shapes: usize, snapshot_empty: bool) -> BoardKnown {
        BoardKnown {
            shapes: (0..shapes).map(|i| format!("s/{i}")).collect(),
            files: if shapes > 0 { vec![F3.into()] } else { vec![] },
            notes: 0,
            snapshot_empty,
        }
    }
    fn mark(m: &mut Model, p: &Plan) {
        let Step::Mark { register, value } = &p.step else {
            panic!("not a mark: {p:?}")
        };
        set(m, register, value.clone());
    }

    #[test]
    fn counts_what_stands_on_the_desk() {
        let m = room();
        let c = m.desk_contents("lab", Some(&board(4, false)));
        assert_eq!((c.sessions, c.helpers, c.archived), (1, 1, 0));
        assert_eq!((c.open_cards, c.done_cards, c.artifacts), (1, 1, 1));
        assert_eq!(
            (c.agents, c.board, c.pictures, c.files, c.goals),
            (2, Some(4), Some(1), 3, 2)
        );
        let main = m.desk_contents("main", None);
        assert_eq!(
            (main.sessions, main.open_cards, main.agents, main.board),
            (1, 1, 1, None)
        );
    }

    #[test]
    fn the_last_desk_and_a_desk_that_is_none_are_refused() {
        let mut m = room();
        assert_eq!(
            m.desk_delete_plan("nope", &Known::default(), ME, 5).step,
            Step::Refused { code: "no-desk".into() }
        );
        m.human.remove("desk/lab");
        assert_eq!(
            m.desk_delete_plan("main", &Known::default(), ME, 5).step,
            Step::Refused {
                code: "last-desk".into()
            }
        );
    }

    #[test]
    fn a_delete_goes_step_by_step_and_counts_down() {
        let mut m = room();
        let mut k = Known::default();
        let p = m.desk_delete_plan("lab", &k, ME, 5);
        let total = p.total;
        assert_eq!(p.done, 0);
        mark(&mut m, &p);
        assert_eq!(m.desks_deleting(), vec!["lab".to_string()]);
        let p = m.desk_delete_plan("lab", &k, ME, 6);
        assert_eq!(
            p.step,
            Step::Shred {
                object_ids: vec!["c1".into()]
            }
        );
        m.cards.get_mut("c1").unwrap().object_state = "closed".into();
        let p = m.desk_delete_plan("lab", &k, ME, 7);
        let Step::Archive { sessions } = &p.step else {
            panic!("{p:?}")
        };
        assert_eq!(sessions.len(), 2);
        assert!(p.done > 0);
        for s in [S1, S3] {
            set(
                &mut m,
                &format!("session/{s}"),
                json!({ "desk": "lab", "archived": true }),
            );
        }
        let p = m.desk_delete_plan("lab", &k, ME, 8);
        assert_eq!(
            p.step,
            Step::RemoveDevices {
                device_ids: vec![A1.into(), A3.into()]
            }
        );
        for a in [A1, A3] {
            m.members.get_mut(a).unwrap().is_active = false;
        }
        assert_eq!(
            m.desk_delete_plan("lab", &k, ME, 9).step,
            Step::LoadBoard { desk_id: "lab".into() }
        );
        k.board = Some(board(3, false));
        assert_eq!(
            m.desk_delete_plan("lab", &k, ME, 9).step,
            Step::EraseBoard {
                shape_ids: vec!["s/0".into(), "s/1".into(), "s/2".into()]
            }
        );
        // (the board once erased still names its picture's file, until it is deleted)
        k.board = Some(BoardKnown {
            files: vec![F3.into()],
            ..board(0, false)
        });
        let p = m.desk_delete_plan("lab", &k, ME, 10);
        assert_eq!(
            p.step,
            Step::DeleteFiles {
                file_ids: vec![F1.into(), F2.into(), F3.into()]
            }
        );
        k.files_gone = [F1, F2, F3].map(String::from).into();
        assert_eq!(
            m.desk_delete_plan("lab", &k, ME, 11).step,
            Step::WriteSnapshot { desk_id: "lab".into() }
        );
        k.board = Some(board(0, true));
        let p = m.desk_delete_plan("lab", &k, ME, 12);
        assert_eq!(
            p.step,
            Step::RemoveDesk {
                register: "desk/lab".into()
            }
        );
        assert_eq!((p.done, p.total), (total - 1, total));
        set(&mut m, "desk/lab", Value::Null);
        let p = m.desk_delete_plan("lab", &k, ME, 13);
        assert_eq!((p.step, p.done), (Step::Done, p.total));
        assert!(m.desks_deleting().is_empty());
        // the other desk kept everything
        let main = m.desk_contents("main", None);
        assert_eq!((main.sessions, main.open_cards, main.agents), (1, 1, 1));
        assert!(m.members[A2].is_active);
    }

    /// Begun on one device, broken off after the shred: another device (a model with the same registers, its own
    /// board read) goes on from there, and asks for nothing that was done.
    #[test]
    fn a_broken_delete_goes_on_on_any_device() {
        let mut m = room();
        let p = m.desk_delete_plan("lab", &Known::default(), "other-device", 5);
        mark(&mut m, &p);
        m.cards.get_mut("c1").unwrap().object_state = "closed".into();
        assert_eq!(m.desks_deleting(), vec!["lab".to_string()]);
        let p = m.desk_delete_plan("lab", &Known::default(), ME, 50);
        assert!(matches!(p.step, Step::Archive { .. }), "{p:?}");
        // the total is the one the mark kept
        let Some(t) = m.human["desk/lab"].value["deleting"]["total"].as_u64() else {
            panic!()
        };
        assert_eq!(p.total, t);
    }

    #[test]
    fn an_agent_with_a_live_session_on_another_desk_stays() {
        let mut m = room();
        // A2 also runs a helper of S1 on "lab": it keeps its main session on "main", so it is not removed
        session(&mut m, "54545454545454545454545454545454", A2, Some(S1), "lab");
        let c = m.desk_contents("lab", None);
        assert_eq!((c.helpers, c.agents), (2, 2));
        let p = m.desk_delete_plan("lab", &Known::default(), ME, 1);
        mark(&mut m, &p);
        m.cards.get_mut("c1").unwrap().object_state = "closed".into();
        for s in [S1, S3, "54545454545454545454545454545454"] {
            set(
                &mut m,
                &format!("session/{s}"),
                json!({ "desk": "lab", "archived": true }),
            );
        }
        let p = m.desk_delete_plan("lab", &Known::default(), ME, 2);
        assert_eq!(
            p.step,
            Step::RemoveDevices {
                device_ids: vec![A1.into(), A3.into()]
            }
        );
    }

    #[test]
    fn a_desk_with_nothing_on_it_needs_its_board_read_then_goes() {
        let mut m = room();
        set(&mut m, "desk/empty", json!({ "name": "Empty", "created_at": 3 }));
        let k = Known {
            board: Some(board(0, true)),
            ..Default::default()
        };
        let p = m.desk_delete_plan("empty", &k, ME, 1);
        mark(&mut m, &p);
        assert_eq!(
            m.desk_delete_plan("empty", &k, ME, 2).step,
            Step::RemoveDesk {
                register: "desk/empty".into()
            }
        );
        assert_eq!(
            m.desk_delete_plan("empty", &Known::default(), ME, 2).step,
            Step::LoadBoard {
                desk_id: "empty".into()
            }
        );
    }
}
