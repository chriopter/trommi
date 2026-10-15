//! Shared engine learning of verified group history, in room/main/helper order.
use super::{Asked, Ctx, Engine, Mend, Retry};
use crate::app::io::IoId;
use crate::app::LogLevel;
use crate::hub::{Answer, ChangeItem, Failure, LogItem, Route};
use std::collections::VecDeque;
use trommi_core::ids::GroupId;
use trommi_core::recovery::ServedCommit;

/// Commits asked for in one page of a group's log while its past is learned.
const LEARN_PAGE: u32 = 1000;
/// The most a group's Commits may weigh when its past is learned (the connector's bound on a session's history).
const LEARN_BYTES_MAX: usize = 256 << 20;

/// One group's past being learned, with those it builds on.
struct Learning {
    /// The group whose past is learned (after its parents').
    group: GroupId,
    /// Still to learn, in order: the room group, a helper session's main session, the group.
    todo: VecDeque<GroupId>,
    founding: Option<Vec<u8>>,
    commits: Vec<LogItem>,
    after: u64,
    weight: usize,
    /// Its envelopes are read again whatever came of it (after a Welcome).
    reread: bool,
    /// Every group in `todo` was learned so far.
    learned: bool,
}

#[derive(Default)]
pub(super) struct Past {
    /// Groups whose past is to be learned, and whether their envelopes are read again whatever comes of it.
    queue: VecDeque<(GroupId, bool)>,
    learning: Option<Learning>,
    learn_asked: Option<IoId>,
    pub(super) learn_retry: Retry,
    /// A lease was acquired: the live groups whose past is not learned are looked for.
    pub(super) learn_due: bool,
}

impl Engine {
    // ---- the past of a group

    /// Groups joined by Welcome: their past is learned (once the log brought the join its place), and their
    /// envelopes read again.
    pub(super) fn agent_joined(&mut self, groups: &[GroupId]) {
        if self.agent.is_none() { return; }
        for group in groups {
            if !self.past.queue.iter().any(|(g, _)| g == group) {
                self.past.queue.push_back((*group, true));
            }
        }
    }

    pub(super) fn learned(&self, group: &GroupId) -> bool {
        match self.device.group(group) {
            Ok(summary) => summary.past_learned,
            Err(_) => self.device.group_past(group).ok().flatten().is_none_or(|past| past.learned),
        }
    }

    /// The next step of learning a group's past; only while the order waits for no repair.
    pub(super) fn drive_learn(&mut self, ctx: &mut Ctx<'_>) {
        if self.repair.is_some() || self.stopped.is_some() {
            return;
        }
        let way = &mut self.past;
        if way.learn_asked.is_some() || way.learn_retry.waiting(ctx.now) {
            return;
        }
        if way.learn_due {
            way.learn_due = false;
            let unlearned: Vec<GroupId> = self
                .device
                .groups()
                .unwrap_or_default()
                .into_iter()
                .filter(|s| !s.group.is_room() && !s.past_learned && !s.archived)
                .map(|s| s.group)
                .collect();
            let way = &mut self.past;
            for group in unlearned {
                if !way.queue.iter().any(|(g, _)| *g == group) && way.learning.as_ref().is_none_or(|l| l.group != group) {
                    way.queue.push_back((group, false));
                }
            }
        }
        loop {
            let way = &mut self.past;
            if way.learning.is_none() {
                let Some((group, reread)) = way.queue.pop_front() else { return };
                let parent = self.device.group(&group).ok().and_then(|s| s.session).filter(|s| !s.parent.is_zero()).map(|s| GroupId::session(s.room_id, s.parent));
                let mut todo: VecDeque<GroupId> = std::iter::once(GroupId::room(self.room)).chain(parent).collect();
                todo.push_back(group);
                let learning = Learning { group, todo, founding: None, commits: Vec::new(), after: 0, weight: 0, reread, learned: true };
                self.past.learning = Some(learning);
            }
            // (what is learned already, the core says: a group founded at epoch 0, or learned before)
            let next = loop {
                let Some(of) = self.past.learning.as_ref().and_then(|l| l.todo.front().copied()) else { break None };
                if !self.learned(&of) {
                    break Some(of);
                }
                if let Some(l) = self.past.learning.as_mut() {
                    l.todo.pop_front();
                }
            };
            let Some(of) = next else {
                self.learn_finished(ctx);
                continue;
            };
            let learning = self.past.learning.as_ref().expect("a learning");
            let (route, what) = match learning.founding {
                None => (Route::GroupInfo { group: &of, epoch: Some(0) }, of),
                Some(_) => (Route::GroupLog { group: &of, after: learning.after, limit: LEARN_PAGE, commits_only: true }, of),
            };
            let asked = self.ask(ctx, route, true, |expect| Asked::Learn { expect, group: what });
            self.past.learn_asked = asked;
            return;
        }
    }

    pub(super) fn on_learn(&mut self, ctx: &mut Ctx<'_>, of: GroupId, answer: Result<Answer, Failure>) {
        let way = &mut self.past;
        way.learn_asked = None;
        let Some(learning) = way.learning.as_mut().filter(|l| l.todo.front() == Some(&of)) else { return };
        match answer {
            Ok(Answer::GroupInfo { group_info, .. }) if learning.founding.is_none() => {
                learning.founding = Some(group_info);
                way.learn_retry.ok();
                return;
            }
            Ok(Answer::GroupLog { items, more }) if learning.founding.is_some() => {
                way.learn_retry.ok();
                let asked_after = learning.after;
                for item in items {
                    let ChangeItem::Commit(log) = item else { continue };
                    if log.group != of {
                        continue;
                    }
                    learning.after = learning.after.max(log.n);
                    learning.weight += log.bytes.len();
                    learning.commits.push(log);
                }
                // (a log that says there is more and does not move on, or that is longer than any group's, ends)
                if more && learning.after != asked_after && learning.weight <= LEARN_BYTES_MAX {
                    return;
                }
            }
            Ok(_) | Err(_) => {
                way.learn_retry.fail(ctx.now, None, &mut self.jitter);
                return;
            }
        }
        // the whole history is in hand: the core walks it
        let learning = way.learning.as_mut().expect("a learning");
        let founding = learning.founding.take().unwrap_or_default();
        let commits = std::mem::take(&mut learning.commits);
        (learning.after, learning.weight) = (0, 0);
        let served: Vec<ServedCommit<'_>> = commits.iter().map(|c| ServedCommit { change: c.change, commit: &c.bytes, recovery_auth: c.recovery_auth.as_deref() }).collect();
        match self.device.learn_history(&of, &founding, &served) {
            Ok(_) => {
                if let Some(l) = self.past.learning.as_mut() {
                    l.todo.pop_front();
                }
            }
            Err(e) => {
                self.local(ctx, &e);
                if self.stopped.is_some() {
                    return;
                }
                // the epochs before this device joined stay unknown: what was written in them is not taken
                ctx.log(LogLevel::Warn, format!("finding: {} (a session's past was not learned)", e.code()));
                if let Some(l) = self.past.learning.as_mut() {
                    l.todo.clear();
                    l.learned = false;
                }
                self.learn_finished(ctx);
            }
        }
    }

    /// The learning of one group ended: its envelopes up to the position are read again when it was learned, or
    /// joined just now.
    fn learn_finished(&mut self, _ctx: &mut Ctx<'_>) {
        let Some(learning) = self.past.learning.take() else { return };
        if learning.learned || learning.reread {
            if self.device.group(&learning.group).is_ok() {
                let upto = self.position;
                self.queue(Mend::Group { group: learning.group, after: 0, upto });
            }
            self.groups_changed();
        }
    }
}
