//! Shared engine learning of verified group history, in room/main/helper order.
use super::{Asked, Ctx, Engine, Mend, Retry};
use crate::app::io::IoId;
use crate::app::LogLevel;
use crate::hub::{Answer, ChangeItem, Failure, Route};
use std::collections::{BTreeSet, VecDeque};
use trommi_core::ids::GroupId;
use trommi_core::recovery::{ServedCommit, MAX_SLICE_COMMITS, MAX_SLICE_LEN};

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
    started: bool,
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
    failed: BTreeSet<GroupId>,
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
            if !self.past.failed.contains(group) && !self.past.queue.iter().any(|(g, _)| g == group) {
                self.past.queue.push_back((*group, true));
            }
        }
    }

    pub(super) fn anchored_past_missing(&self) -> bool {
        self.device.room_past_required().unwrap_or(true)
    }

    pub(super) fn room_past_pending(&self) -> bool {
        let room = GroupId::room(self.room);
        self.past.queue.iter().any(|(g, _)| *g == room)
            || self.past.learning.as_ref().is_some_and(|l| l.todo.contains(&room))
    }

    pub(super) fn need_anchored_past(&mut self) -> bool {
        self.anchored_past_missing() && self.need_past(GroupId::room(self.room))
    }

    /// Queue a verified backward walk only when a caller needs the missing epochs.
    pub(super) fn need_past(&mut self, group: GroupId) -> bool {
        if self.learned(&group) { return false; }
        if !self.past.failed.contains(&group) && !self.past.queue.iter().any(|(g, _)| *g == group)
            && self.past.learning.as_ref().is_none_or(|l| l.group != group) {
            self.past.queue.push_back((group, false));
        }
        true
    }

    pub(super) fn past_failed(&self, group: &GroupId) -> bool { self.past.failed.contains(group) }

    pub(super) fn learned(&self, group: &GroupId) -> bool {
        match self.device.group(group) {
            Ok(summary) => summary.past_learned,
            Err(_) => self.device.group_past(group).ok().flatten().is_none_or(|past| past.learned),
        }
    }

    /// The next step of learning a group's past, including when a repair needs those missing epochs.
    pub(super) fn drive_learn(&mut self, ctx: &mut Ctx<'_>) {
        if self.stopped.is_some() {
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
                if !group.is_room() { todo.push_back(group); }
                let learning = Learning { group, todo, started: false, after: 0, weight: 0, reread, learned: true };
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
            let (route, what) = match learning.started {
                false => (Route::GroupInfo { group: &of, epoch: Some(0) }, of),
                true => (Route::GroupLog { group: &of, after: learning.after, limit: LEARN_PAGE, commits_only: true }, of),
            };
            let asked = self.ask(ctx, route, true, |expect| Asked::Learn { expect, group: what });
            self.past.learn_asked = asked;
            return;
        }
    }

    pub(super) fn on_learn(&mut self, ctx: &mut Ctx<'_>, of: GroupId, answer: Result<Answer, Failure>) {
        self.past.learn_asked = None;
        if self.past.learning.as_ref().is_none_or(|l| l.todo.front() != Some(&of)) { return; }
        let result = self.take_past_answer(of, answer);
        match result {
            Ok(Some(false)) => (),
            Ok(None) => {
                self.past.learn_retry.fail(ctx.now, None, &mut self.jitter);
            }
            Ok(Some(true)) => {
                self.past.learn_retry.ok();
                if let Some(l) = self.past.learning.as_mut() {
                    l.todo.pop_front();
                    l.started = false;
                    l.after = 0;
                    l.weight = 0;
                }
            }
            Err(e) => {
                self.device.learn_abandon(&of);
                self.past.failed.insert(of);
                self.report(super::Report::Alert(super::Alert::BadGroup { group: of, change: 0, code: e.code() }));
                self.local(ctx, &e);
                if self.stopped.is_some() { return; }
                ctx.log(LogLevel::Warn, format!("finding: {} (a session's past was not learned)", e.code()));
                if let Some(l) = self.past.learning.as_mut() {
                    l.todo.clear();
                    l.learned = false;
                }
                self.learn_finished(ctx);
            }
        }
    }

    /// Verify each bounded slice as it arrives; no page or founding bytes survive this callback.
    fn take_past_answer(&mut self, of: GroupId, answer: Result<Answer, Failure>) -> Result<Option<bool>, trommi_core::Error> {
        let l = self.past.learning.as_mut().expect("a learning");
        let progress = match answer {
            Ok(Answer::GroupInfo { group_info, .. }) if !l.started => {
                let progress = self.device.learn_start(&of, &group_info)?;
                l.started = true;
                progress
            }
            Ok(Answer::GroupLog { items, more }) if l.started => {
                let old_after = l.after;
                let commits: Vec<_> = items.iter().filter_map(|item| match item {
                    ChangeItem::Commit(log) if log.group == of => Some(log),
                    _ => None,
                }).collect();
                let mut rest = commits.as_slice();
                let mut progress = None;
                while !rest.is_empty() {
                    let mut bytes = 0usize;
                    let count = rest.iter().take(MAX_SLICE_COMMITS).take_while(|c| {
                        bytes = bytes.saturating_add(c.bytes.len());
                        bytes <= MAX_SLICE_LEN
                    }).count();
                    if count == 0 { return Err(trommi_core::Error::TooLarge); }
                    let (slice, later) = rest.split_at(count);
                    let served: Vec<_> = slice.iter().map(|c| ServedCommit {
                        change: c.change, commit: &c.bytes, recovery_auth: c.recovery_auth.as_deref(),
                    }).collect();
                    l.weight = l.weight.saturating_add(slice.iter().map(|c| c.bytes.len()).sum::<usize>());
                    if l.weight > LEARN_BYTES_MAX { return Err(trommi_core::Error::TooLarge); }
                    l.after = slice.iter().fold(l.after, |n, c| n.max(c.n));
                    let reached = self.device.learn_slice(&of, &served)?;
                    progress = Some(reached);
                    if reached.epoch >= reached.upto { break; }
                    rest = later;
                }
                if let Some(progress) = progress.filter(|p| p.epoch >= p.upto) {
                    progress
                } else if more && l.after > old_after {
                    self.past.learn_retry.ok();
                    return Ok(Some(false));
                } else {
                    // Finishing a truncated or stalled log refuses it against the held origin.
                    self.device.learn_finish(&of)?;
                    return Ok(Some(true));
                }
            }
            Ok(_) | Err(_) => return Ok(None),
        };
        self.past.learn_retry.ok();
        if progress.epoch < progress.upto { return Ok(Some(false)); }
        self.device.learn_finish(&of)?;
        Ok(Some(true))
    }

    /// The learning of one group ended: its envelopes up to the position are read again when it was learned, or
    /// joined just now.
    fn learn_finished(&mut self, _ctx: &mut Ctx<'_>) {
        let Some(learning) = self.past.learning.take() else { return };
        if learning.learned || learning.reread {
            if !learning.group.is_room() && self.device.group(&learning.group).is_ok() {
                let upto = self.position;
                self.queue(Mend::Group { group: learning.group, after: 0, upto });
            }
            self.groups_changed();
        }
    }
}
