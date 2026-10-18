//! Deleting a team with everything on it (`Action::RemoveTeam`), by the one rule every app follows
//! ([`crate::team_delete`]): the planner names the next step from the model and what this App knows of the team's
//! board; the App does it and asks again, until the team is gone. A delete begun on another device (its register
//! says `deleting`) is finished here too, once the room is live. As the web's `desks.mjs deleteDesk`.
//!
//! A step whose effect has not shown in the model yet is not done twice: the same step is asked for again only after
//! [`AGAIN_MS`], and after [`STUCK`] times the delete stops with a toast (it goes on at the next start).

use super::account_steps::Step as Asked;
use super::{App, Effect};
use crate::actions::Action;
use crate::team_delete::{Known, Step};
use crate::hub::{Codec, Route};
use crate::views::{TeamCountsView, TeamDeleteView, Phase, Text};
use std::collections::{BTreeMap, BTreeSet};
use trommi_core::ids::FileId;

/// A step whose effect has not shown is asked for again after so long.
pub const AGAIN_MS: u64 = 4_000;
/// So many times the same step without progress, and the delete stops for this run.
pub const STUCK: u32 = 8;

#[derive(Default)]
pub(super) struct Run {
    files_gone: BTreeSet<String>,
    files_asked: BTreeSet<String>,
    last: Option<(String, u64, u32)>,
    done: u64,
    total: u64,
}

#[derive(Default)]
pub(super) struct TeamDeletes {
    pub(super) running: BTreeMap<String, Run>,
    /// Teams whose delete stopped in this run (stuck): left for the next start.
    stopped: BTreeSet<String>,
    /// A step runs now (its actions come back through `after_engine`).
    driving: bool,
}

impl TeamDeletes {
    /// When to look again at a delete whose step has not shown yet.
    pub(super) fn wake(&self, now_ms: u64) -> Option<u64> {
        (!self.running.is_empty()).then_some(now_ms + AGAIN_MS)
    }
}

impl App {
    /// `RemoveTeam`: the delete begins (or goes on).
    pub(super) fn delete_team(&mut self, now_ms: u64, team_id: &str, out: &mut Vec<Effect>) {
        self.teams.stopped.remove(team_id);
        self.teams.running.entry(team_id.to_string()).or_default();
        self.drive_team_deletes(now_ms, out);
    }

    /// A file's deletion answered: gone (or the hub never had it).
    pub(super) fn team_file_gone(&mut self, team_id: &str, file: String, gone: bool) {
        if let Some(run) = self.teams.running.get_mut(team_id) {
            run.files_asked.remove(&file);
            if gone {
                run.files_gone.insert(file);
            }
        }
    }

    /// One step of every delete that runs, where its last step has shown (or waited long enough).
    pub(super) fn drive_team_deletes(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.teams.driving || self.next_view.phase != Phase::Live || self.engine.is_none() {
            return;
        }
        self.teams.driving = true;
        self.drive_teams(now_ms, out);
        self.teams.driving = false;
    }

    fn drive_teams(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        // (a delete begun elsewhere is finished here)
        for id in self.board.model.teams_deleting() {
            if !self.teams.stopped.contains(&id) {
                self.teams.running.entry(id).or_default();
            }
        }
        let by = self.engine.as_ref().map(|e| crate::engine::feed::hex(e.device().id().as_bytes())).unwrap_or_default();
        let ids: Vec<String> = self.teams.running.keys().cloned().collect();
        for team_id in ids {
            let board_id = crate::scribble::team_board(&team_id);
            let known = Known {
                board: self.board_known(&board_id),
                files_gone: self.teams.running.get(&team_id).map(|r| r.files_gone.clone()).unwrap_or_default(),
            };
            let plan = self.board.model.team_delete_plan(&team_id, &known, &by, now_ms);
            let key = serde_json::to_string(&plan.step).unwrap_or_default();
            let Some(run) = self.teams.running.get_mut(&team_id) else { continue };
            (run.done, run.total) = (plan.done, plan.total);
            match &run.last {
                Some((last, at, _)) if *last == key && now_ms < at + AGAIN_MS => continue,
                Some((last, _, n)) if *last == key && *n + 1 >= STUCK => {
                    self.teams.running.remove(&team_id);
                    self.teams.stopped.insert(team_id.clone());
                    let text = Text::new("teams.delete_stuck", "The team was not deleted yet. It goes on at the next start.");
                    self.sent.toast(&mut self.next_view.toasts, text, None);
                    continue;
                }
                Some((last, _, n)) if *last == key => run.last = Some((key, now_ms, n + 1)),
                _ => run.last = Some((key, now_ms, 0)),
            }
            self.team_step(now_ms, &team_id, &board_id, plan.step, out);
        }
        self.show_team_deletes();
    }

    fn team_step(&mut self, now_ms: u64, team_id: &str, board_id: &str, step: Step, out: &mut Vec<Effect>) {
        match step {
            Step::Mark { register, value } => self.run_action(now_ms, Action::RoomRegister { key: register, value }, out),
            Step::RemoveTeam { register } => self.run_action(now_ms, Action::RoomRegister { key: register, value: serde_json::Value::Null }, out),
            Step::ForgetKnowledge { entry_ids } => {
                for entry_id in entry_ids {
                    self.run_action(now_ms, Action::KnowledgeDelete { entry_id }, out);
                }
            }
            Step::Shred { object_ids } => {
                for card_id in object_ids {
                    self.run_action(now_ms, Action::Shred { card_id, note: None, attachments: Vec::new(), marks: Vec::new() }, out);
                }
            }
            Step::Archive { sessions } => {
                for s in sessions {
                    self.run_action(now_ms, Action::ArchiveSession { session_id: s.key, on: true }, out);
                }
            }
            Step::RemoveDevices { device_ids } => self.account(now_ms, crate::account::AccountAction::RemoveDevices { device_ids }, out),
            Step::LoadBoard { .. } => out.extend(self.board_work(now_ms, board_id, false)),
            Step::EraseBoard { shape_ids } => out.extend(self.erase_board(now_ms, board_id, &shape_ids)),
            Step::WriteSnapshot { .. } => out.extend(self.board_work(now_ms, board_id, true)),
            Step::DeleteFiles { file_ids } => {
                for file in file_ids {
                    let asked = self.teams.running.get(team_id).is_some_and(|r| r.files_asked.contains(&file));
                    let Some(id) = (!asked).then(|| unhex16(&file)).flatten() else { continue };
                    let (team, f) = (team_id.to_string(), file.clone());
                    if self.step_send(now_ms, out, move |site| Codec.request(site, &Route::DeleteFile { file: &FileId::new(id) }), move |expect| Asked::TeamFile { team, file: f, expect }) {
                        if let Some(run) = self.teams.running.get_mut(team_id) {
                            run.files_asked.insert(file);
                        }
                    }
                }
            }
            Step::Done => {
                self.teams.running.remove(team_id);
                let text = Text::new("teams.deleted", "The team is deleted.");
                self.sent.toast(&mut self.next_view.toasts, text, None);
            }
            Step::Refused { code } => {
                self.teams.running.remove(team_id);
                let text = match code.as_str() {
                    "last-team" => Text::new("action.last_team", "The last team stays."),
                    _ => Text::new("action.no_team", "That team is no longer there."),
                };
                self.sent.toast(&mut self.next_view.toasts, text, None);
            }
        }
    }

    /// The deletes as the settings show them, and what each team holds (for the confirm).
    pub(super) fn show_team_deletes(&mut self) {
        self.next_view.settings.deleting = self
            .teams
            .running
            .iter()
            .map(|(id, r)| TeamDeleteView { team_id: id.clone(), done: r.done, total: r.total })
            .collect();
    }

    /// What every team holds, its board counted where this App read it.
    pub(super) fn team_counts(&self) -> Vec<TeamCountsView> {
        let m = &self.board.model;
        m.human
            .iter()
            .filter(|(k, r)| k.starts_with("desk/") && r.value.is_object())
            .map(|(k, r)| {
                let id = k.trim_start_matches("desk/").to_string();
                let board = self.board_known(&crate::scribble::team_board(&id));
                let c = m.team_contents(&id, board.as_ref());
                let name = r.value.get("name").and_then(serde_json::Value::as_str).unwrap_or("").to_string();
                let mut view = TeamCountsView {
                    team_id: id,
                    sessions: c.sessions,
                    helpers: c.helpers,
                    archived: c.archived,
                    open_cards: c.open_cards,
                    done_cards: c.done_cards,
                    agents: c.agents,
                    board: c.board,
                    pictures: c.pictures,
                    notes: c.notes,
                    files: c.files,
                    artifacts: c.artifacts,
                    goals: c.goals,
                    knowledge: c.knowledge,
                    ..Default::default()
                };
                crate::views::settings::team_delete_ask(&mut view, &name);
                view
            })
            .collect()
    }
}

fn unhex16(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}
