//! The session groups of a sign-in with the code (8.4, 8.5): a device that joined the room group with the recovery
//! code joins each live session group from outside. The fast path of it: what the joins need is read for many
//! groups in one request (`POST /v1/groups/read`), and the joins the device built are posted together
//! (`POST /v1/commits`), each answered in its place.
//!
//! # How it runs
//!
//! [`Engine::join_sessions`] takes the list (main sessions first) and the code's keys. Once the hub's order is at
//! rest, the groups are read [`READ_AT_ONCE`] at a time. Each row is handed to the protocol core at once: the
//! check from the founding GroupInfo through the group's Commits, then the join on the GroupInfo served as
//! current. The core holds one check at a time, so a row whose log goes on (`log.more`) is read on before the next
//! row is looked at. What the hub says of a row decides what is read, never what is believed: every Commit is
//! judged by the core against the room states this device holds.
//!
//! - a row refused as `too-large` did not fit the answer: that group is read alone (its two GroupInfos, then its
//!   Commits page by page);
//! - a row refused otherwise (`not-found`: the asker may not see the group) is not joined, for good;
//! - a request that fails as a whole is asked again after a wait; one the hub refuses as a whole leaves every group
//!   of it to a reading of its own.
//!
//! A join is built on the GroupInfo of one epoch. If the group moved on meanwhile (the check does not agree with
//! the GroupInfo, or the hub answers the posted join `epoch-taken`), the group is read alone once more, at most
//! [`JOIN_TRIES`] times in all. When every group is joined or given up, [`Report::SessionsJoined`] says which.
//!
//! # What is kept
//!
//! The code's keys live in the engine only while this runs and are dropped with it; they are never stored. The
//! engine's record ([`super::record`]) keeps which groups are still to join. After a restart the joins already in
//! the outbox are posted; a session whose join was not built yet is reported as `interrupted`, and is left to
//! another human device of the person (5.2.7), or to the shell asking again with the code.

use super::*;
use std::collections::BTreeSet;
use trommi_core::recovery::{RecoveryKeys, ServedCommit};

/// Groups asked for in one `POST /v1/groups/read` (the hub takes 64).
pub const READ_AT_ONCE: usize = 32;
/// Joins posted in one `POST /v1/commits`: at most so many, and so many bytes of them (the hub takes a body of
/// 1.5 MiB).
pub const JOINS_AT_ONCE: usize = 32;
pub const JOINS_BYTES: usize = 1 << 20;
/// A group is read and its join built at most so many times.
pub const JOIN_TRIES: u32 = 3;
/// Commits handed to the core's check in one slice; halved when the core says `too-large`.
const SLICE: usize = 64;

/// Every `SealedKey` and every `RecoveryLink` of the room, as the hub served them to the sign-in.
pub struct Later {
    pub rows: Vec<Vec<u8>>,
    pub links: Vec<Vec<u8>>,
}

/// What [`Engine::join_sessions`] takes.
pub struct SessionJoins {
    /// The keys of the code in hand.
    pub keys: RecoveryKeys,
    /// For sessions whose check was deferred at the join of the room: every `SealedKey` and every `RecoveryLink`
    /// of the room, from which each session's content keys are selected when it is checked. None: the room's
    /// check selected them.
    pub later: Option<Later>,
    /// The live session groups, main sessions before helper sessions.
    pub groups: Vec<GroupId>,
}

/// A read in flight.
#[derive(Clone, Debug)]
pub(super) enum Read {
    Together(Vec<GroupId>),
    Founding(GroupId),
    Current(GroupId),
    Log(GroupId),
}

/// One group being read on, or alone. The core holds its check once `started`.
struct One {
    group: GroupId,
    started: bool,
    /// The GroupInfo served as current, and its epoch: the group's Commits are read up to it.
    current: Option<(Vec<u8>, u64)>,
    /// The log's number of the last Commit handed to the check.
    after: u64,
    /// The epoch the last Commit handed to the check led to.
    reached: u64,
}

pub(super) struct Joining {
    /// The keys of the code; none after a restart, when only the joins already built go on.
    keys: Option<RecoveryKeys>,
    later: Option<Later>,
    room_keys_recovered: bool,
    /// The first Desk may still need the code to recover old room keys.
    room_pending: bool,
    reported: bool,
    /// Still to be read, together.
    together: VecDeque<GroupId>,
    /// Rows of an answer not looked at yet.
    rows: VecDeque<GroupRead>,
    /// To be read alone.
    alone: VecDeque<GroupId>,
    one: Option<One>,
    asked: Option<IoId>,
    /// The groups of the read in flight that are in none of the lists above.
    reading: Vec<GroupId>,
    pub(super) retry: Retry,
    tries: BTreeMap<GroupId, u32>,
    /// Joins built and in the outbox: the hub's answer is awaited.
    posted: BTreeSet<GroupId>,
    joined: u32,
    not_joined: Vec<(GroupId, &'static str)>,
}

/// How many entries at the outbox's head are joins of session groups from outside, one per group, that go in one
/// request.
pub(super) fn joins_at(entries: &[OutboxEntry]) -> usize {
    let mut seen = BTreeSet::new();
    let mut bytes = 0usize;
    for (at, entry) in entries.iter().enumerate() {
        let Some(group) = entry.group.filter(|g| entry.kind == OutboxKind::ExternalCommit && !g.is_room()) else { return at };
        let size = entry.parts.iter().map(Vec::len).sum::<usize>() / 3 * 4 + 200;
        if at >= JOINS_AT_ONCE || (at > 0 && bytes + size > JOINS_BYTES) || !seen.insert(group) {
            return at;
        }
        bytes += size;
    }
    entries.len()
}

/// A refusal's code as this build names it; one it does not know is `refused`.
fn code_of(failure: &Failure) -> &'static str {
    match failure {
        Failure::Refused { code: Some(code), .. } => Error::from_code(code).map_or("refused", |e| e.code()),
        _ => "refused",
    }
}

impl Joining {
    /// After a restart: the joins that were built wait for the hub's answer; nothing more is read.
    pub(super) fn resumed(posted: BTreeSet<GroupId>, joined: u32, not_joined: Vec<(GroupId, &'static str)>, room_pending: bool) -> Self {
        Joining {
            keys: None,
            later: None,
            room_keys_recovered: false,
            room_pending,
            reported: false,
            together: VecDeque::new(),
            rows: VecDeque::new(),
            alone: VecDeque::new(),
            one: None,
            asked: None,
            reading: Vec::new(),
            retry: Retry::default(),
            tries: BTreeMap::new(),
            posted,
            joined,
            not_joined,
        }
    }

    /// What a restart needs of it: the groups still to join, and how the others ended.
    pub(super) fn record(&self) -> super::record::JoinsRecord {
        let waiting = self.together.iter().chain(&self.alone).copied().chain(self.rows.iter().map(|row| row.group)).chain(self.one.iter().map(|one| one.group)).chain(self.posted.iter().copied()).chain(self.reading.iter().copied());
        super::record::JoinsRecord::new(waiting, self.joined, &self.not_joined, self.room_pending && !self.room_keys_recovered)
    }
}

impl Engine {
    /// Joins the live session groups of a sign-in with the code (see the module). A second call while one runs adds
    /// its groups and replaces the keys.
    pub fn join_sessions(&mut self, ctx: &mut Ctx<'_>, joins: SessionJoins) {
        if self.stopped.is_some() {
            return;
        }
        let room_pending = joins.later.is_some();
        let mut joining = match self.joining.take() {
            Some(mut joining) => {
                joining.keys = Some(joins.keys);
                joining.later = joins.later;
                joining.room_keys_recovered = false;
                joining.room_pending = room_pending;
                joining.reported = false;
                joining.not_joined.retain(|(_, code)| *code != "interrupted");
                joining
            }
            None => Joining {
                keys: Some(joins.keys),
                later: joins.later,
                room_keys_recovered: false,
                room_pending,
                reported: false,
                together: VecDeque::new(),
                rows: VecDeque::new(),
                alone: VecDeque::new(),
                one: None,
                asked: None,
                reading: Vec::new(),
                retry: Retry::default(),
                tries: BTreeMap::new(),
                posted: BTreeSet::new(),
                joined: 0,
                not_joined: Vec::new(),
            },
        };
        for group in joins.groups {
            if !group.is_room() && self.device.group(&group).is_err() && !joining.together.contains(&group) && !joining.alone.contains(&group) && !joining.posted.contains(&group) {
                joining.together.push_back(group);
            }
        }
        self.joining = Some(joining);
        self.drive(ctx);
    }

    /// The first Desk needs old room keys but a restart lost the in-memory code.
    pub(super) fn note_room_key_need(&mut self) {
        let room = GroupId::room(self.room);
        if let Some(j) = self.joining.as_mut().filter(|j| j.room_pending && j.keys.is_none()) {
            if !j.not_joined.iter().any(|(g, _)| *g == room) {
                j.not_joined.push((room, "interrupted"));
            }
        }
    }

    /// Whether the session groups of a sign-in are still being joined.
    pub fn joining_sessions(&self) -> bool {
        self.joining.as_ref().is_some_and(|j| !j.reported)
    }

    pub(super) fn drive_joins(&mut self, ctx: &mut Ctx<'_>) {
        // (the core judges a session's Commits against the room states this device holds: the order first)
        if !self.caught_up || self.pass || !self.todo.is_empty() || self.repair.is_some() {
            return;
        }
        let Some(mut j) = self.joining.take() else { return };
        if j.reported { self.joining = Some(j); return; }
        if j.asked.is_none() && !j.retry.waiting(ctx.now) {
            self.joins_step(ctx, &mut j);
        }
        if self.stopped.is_some() {
            return;
        }
        // A join that left the outbox without its answer being read (the stream brought the Commit and the core
        // merged it, then the answer was lost): settled by what the device holds, or nobody would ever say it.
        let outbox = self.device.outbox();
        let gone: Vec<GroupId> = j.posted.iter().filter(|g| !outbox.iter().any(|e| e.kind == OutboxKind::ExternalCommit && e.group == Some(**g))).copied().collect();
        for group in gone {
            j.posted.remove(&group);
            if self.device.group(&group).is_ok() {
                j.joined += 1;
                self.desk.want = true;
            } else {
                j.not_joined.push((group, "refused"));
            }
        }
        let room_waits = j.room_pending && !j.room_keys_recovered && !self.desk.complete
            || j.keys.is_some() && self.anchored_past_missing() && self.room_past_pending();
        let through = !room_waits && j.asked.is_none() && j.one.is_none() && j.rows.is_empty() && j.together.is_empty() && j.alone.is_empty() && j.posted.is_empty();
        if through {
            // every session's keys are at hand now: the Desk of the joined groups, and what came without its key
            // since the sign-in is read back group by group, from the first such envelope. The room's order is
            // not read again: the past comes on demand (history), and catch-up resumes at the position.
            self.desk.want = true;
            self.tending.look_for_archived();
            self.keys_came();
            self.report(Report::Groups);
            let interrupted = j.not_joined.iter().any(|(_, code)| *code == "interrupted");
            self.report(Report::SessionsJoined { joined: j.joined, not_joined: j.not_joined.clone() });
            if interrupted { j.reported = true; self.joining = Some(j); }
            return self.drive_all(ctx);
        }
        self.joining = Some(j);
    }

    fn joins_step(&mut self, ctx: &mut Ctx<'_>, j: &mut Joining) {
        let room = GroupId::room(self.room);
        // Keep the code until the first Desk has revealed whether old room registers need it.
        // An empty Desk does not trigger history; an old room envelope does.
        if j.room_pending && !j.room_keys_recovered && !self.desk.complete
            || j.keys.is_some() && j.together.is_empty() && j.alone.is_empty() && j.rows.is_empty() && j.one.is_none()
                && self.anchored_past_missing() && self.room_past_pending() {
            return;
        }
        if self.past_failed(&room) {
            let waiting: Vec<_> = j.together.drain(..).chain(j.alone.drain(..))
                .chain(j.rows.drain(..).map(|row| row.group)).chain(j.one.take().map(|one| one.group)).collect();
            j.not_joined.extend(waiting.into_iter().map(|g| (g, "bad-group")));
            return;
        }
        if (!j.together.is_empty() || !j.alone.is_empty() || !j.rows.is_empty() || j.one.is_some()) && self.need_past(room) {
            return;
        }
        if !j.room_keys_recovered && self.learned(&room) {
            if let (Some(keys), Some(later)) = (&j.keys, &j.later) {
                match self.device.recover_room_keys_after_history(keys, &later.rows, &later.links) {
                    Ok(missing) => {
                        if missing.is_some() { self.report(Report::Alert(Alert::MissingLink)); }
                        j.room_keys_recovered = true;
                    }
                    Err(e) => { self.local(ctx, &e); return; }
                }
            }
        }
        loop {
            if self.stopped.is_some() {
                return;
            }
            if let Some(one) = &j.one {
                let group = one.group;
                let (route, what) = if !one.started {
                    (Route::GroupInfo { group: &group, epoch: Some(0) }, Read::Founding(group))
                } else if one.current.is_none() {
                    (Route::GroupInfo { group: &group, epoch: None }, Read::Current(group))
                } else {
                    (Route::GroupLog { group: &group, after: one.after, limit: LOG_PAGE, commits_only: true }, Read::Log(group))
                };
                j.asked = self.ask(ctx, route, true, |expect| Asked::JoinRead { expect, what });
                if j.asked.is_none() {
                    j.one = None;
                    j.not_joined.push((group, "bad-format"));
                    continue;
                }
                return;
            }
            if let Some(row) = j.rows.pop_front() {
                self.join_row(ctx, j, row);
                continue;
            }
            if !j.together.is_empty() {
                let groups: Vec<GroupId> = j.together.iter().take(READ_AT_ONCE).copied().collect();
                j.asked = self.ask(ctx, Route::GroupsRead { groups: &groups }, true, |expect| Asked::JoinRead { expect, what: Read::Together(groups.clone()) });
                if j.asked.is_some() {
                    j.together.drain(..groups.len());
                    j.reading = groups;
                    return;
                }
                // (a request that cannot be built: each alone)
                j.alone.extend(j.together.drain(..));
                continue;
            }
            let Some(group) = j.alone.pop_front() else { return };
            if self.device.group(&group).is_ok() {
                continue; // a Welcome came first
            }
            j.one = Some(One { group, started: false, current: None, after: 0, reached: 0 });
        }
    }

    /// A group does not join this time: read alone once more if the group may only have moved on, else given up.
    fn join_failed(&mut self, ctx: &mut Ctx<'_>, j: &mut Joining, group: GroupId, error: &Error) {
        self.local(ctx, error);
        let tries = j.tries.entry(group).or_default();
        *tries += 1;
        let moved = has_moved(error) || matches!(error, Error::Busy | Error::BadGroup | Error::NotFound);
        if moved && *tries < JOIN_TRIES {
            j.alone.push_back(group);
        } else {
            j.not_joined.push((group, error.code()));
        }
    }

    /// Hands the check its next Commits, those below the epoch of the GroupInfo to join on. True: the check
    /// reached that epoch.
    fn join_slices(&mut self, one: &mut One, log: &[LogItem]) -> Result<bool, Error> {
        let upto = one.current.as_ref().map_or(0, |(_, epoch)| *epoch);
        let commits: Vec<&LogItem> = log.iter().filter(|c| c.n > one.after && c.epoch < upto).collect();
        let mut at = 0;
        let mut size = SLICE;
        while at < commits.len() {
            let slice: Vec<ServedCommit<'_>> = commits[at..(at + size).min(commits.len())]
                .iter()
                .map(|c| ServedCommit { change: c.change, commit: &c.bytes, recovery_auth: c.recovery_auth.as_deref() })
                .collect();
            match self.device.session_check_slice(&one.group, &slice) {
                Ok(()) => {
                    at += slice.len();
                    let last = commits[at - 1];
                    (one.after, one.reached) = (last.n, last.epoch + 1);
                }
                Err(Error::TooLarge) if size > 1 => size /= 2,
                Err(e) => return Err(e),
            }
        }
        Ok(one.reached >= upto)
    }

    /// The check is through: the join is built on the GroupInfo served as current, and waits in the outbox.
    fn join_build(&mut self, ctx: &mut Ctx<'_>, j: &mut Joining, one: One) {
        let Some((current, _)) = &one.current else { return };
        // (after a restart the code's keys are gone: nothing is read then, so nothing comes here)
        let Some(keys) = &j.keys else { return j.not_joined.push((one.group, "interrupted")) };
        let built = match &j.later {
            Some(later) => self.device.join_session_checked_keys(keys, &one.group, current, &later.rows, &later.links, ctx.now),
            None => self.device.join_session_checked(keys, &one.group, current, ctx.now),
        };
        match built {
            Ok(_) => {
                j.posted.insert(one.group);
            }
            Err(e) => self.join_failed(ctx, j, one.group, &e),
        }
    }

    /// One good row of a read: checked from its founding, and joined, or read on.
    fn join_row(&mut self, ctx: &mut Ctx<'_>, j: &mut Joining, row: GroupRead) {
        let group = row.group;
        if self.device.group(&group).is_ok() {
            return; // a member already: a Welcome came first, or an earlier run's join went through
        }
        let founding = row.founding.as_deref().unwrap_or(&row.info);
        match self.device.session_check_start(founding) {
            Ok(checked) if checked == group => {}
            // (a founding GroupInfo of another group than the one asked: not this session)
            Ok(_) => return self.join_failed(ctx, j, group, &Error::BadGroup),
            Err(e) => return self.join_failed(ctx, j, group, &e),
        }
        let mut one = One { group, started: true, current: Some((row.info, row.epoch)), after: 0, reached: 0 };
        match self.join_slices(&mut one, &row.log) {
            Ok(true) => self.join_build(ctx, j, one),
            Ok(false) if !row.complete => j.one = Some(one),
            // (the log ends before the epoch named: the check and the GroupInfo decide)
            Ok(false) => self.join_build(ctx, j, one),
            Err(e) => self.join_failed(ctx, j, group, &e),
        }
    }

    pub(super) fn on_join_read(&mut self, ctx: &mut Ctx<'_>, what: Read, answer: Result<Answer, Failure>, unauthorised: bool) {
        let Some(mut j) = self.joining.take() else { return };
        j.asked = None;
        j.reading.clear();
        self.join_read(ctx, &mut j, what, answer, unauthorised);
        if self.stopped.is_none() {
            self.joining = Some(j);
        }
    }

    fn join_read(&mut self, ctx: &mut Ctx<'_>, j: &mut Joining, what: Read, answer: Result<Answer, Failure>, unauthorised: bool) {
        let answer = match answer {
            Ok(answer) => answer,
            Err(failure) => {
                // The hub's own refusal ends this read: of many groups, each is read alone; of one, it is not
                // joined. Anything else is the same read again (after a wait, or once signed in).
                let refused = matches!(failure, Failure::Refused { .. }) && !unauthorised;
                match what {
                    Read::Together(groups) if refused => j.alone.extend(groups),
                    Read::Together(groups) => {
                        for group in groups.into_iter().rev() {
                            j.together.push_front(group);
                        }
                    }
                    Read::Founding(group) | Read::Current(group) | Read::Log(group) if refused => {
                        j.one = None;
                        j.not_joined.push((group, code_of(&failure)));
                    }
                    _ => {}
                }
                if !refused && !unauthorised {
                    let retry_after = match failure {
                        Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
                        Failure::BadAnswer { why } => {
                            self.report(Report::Alert(Alert::BadAnswer { why }));
                            None
                        }
                        _ => None,
                    };
                    j.retry.fail(ctx.now, retry_after, &mut self.jitter);
                }
                return;
            }
        };
        j.retry.ok();
        match (what, answer) {
            (Read::Together(groups), Answer::GroupsRead(rows)) => {
                for (group, row) in groups.into_iter().zip(rows) {
                    match row {
                        Ok(read) if read.group == group => j.rows.push_back(read),
                        Ok(_) => j.not_joined.push((group, "bad-group")),
                        // it did not fit the answer, or the hub could not serve it now: alone
                        Err(Failure::Refused { code: Some(code), .. }) if code == "too-large" => j.alone.push_back(group),
                        Err(failure @ Failure::Refused { .. }) => j.not_joined.push((group, code_of(&failure))),
                        Err(_) => j.alone.push_back(group),
                    }
                }
            }
            (Read::Founding(group), Answer::GroupInfo { group_info, .. }) => {
                let Some(mut one) = j.one.take().filter(|o| o.group == group) else { return };
                match self.device.session_check_start(&group_info) {
                    Ok(checked) if checked == group => {
                        one.started = true;
                        j.one = Some(one);
                    }
                    Ok(_) => self.join_failed(ctx, j, group, &Error::BadGroup),
                    Err(e) => self.join_failed(ctx, j, group, &e),
                }
            }
            (Read::Current(group), Answer::GroupInfo { epoch, group_info }) => {
                let Some(mut one) = j.one.take().filter(|o| o.group == group) else { return };
                one.current = Some((group_info, epoch));
                if epoch == 0 {
                    self.join_build(ctx, j, one);
                } else {
                    j.one = Some(one);
                }
            }
            (Read::Log(group), Answer::GroupLog { items, more }) => {
                let Some(mut one) = j.one.take().filter(|o| o.group == group) else { return };
                let log: Vec<LogItem> = items.into_iter().filter_map(|i| if let ChangeItem::Commit(l) = i { Some(l) } else { None }).collect();
                let before = one.after;
                match self.join_slices(&mut one, &log) {
                    Ok(true) => self.join_build(ctx, j, one),
                    // (a page that brought nothing new would be asked for ever: the check and the GroupInfo decide)
                    Ok(false) if more && one.after > before => j.one = Some(one),
                    Ok(false) => self.join_build(ctx, j, one),
                    Err(e) => self.join_failed(ctx, j, group, &e),
                }
            }
            (Read::Together(groups), _) => j.alone.extend(groups),
            (Read::Founding(group) | Read::Current(group) | Read::Log(group), _) => {
                j.one = None;
                j.not_joined.push((group, "bad-format"));
            }
        }
    }

    /// The hub answered a posted join of a session group: taken (`refused` none), or refused for good.
    pub(super) fn join_answered(&mut self, kind: OutboxKind, group: Option<GroupId>, refused: Option<&Error>) {
        let (Some(j), Some(group)) = (self.joining.as_mut(), group) else { return };
        if kind != OutboxKind::ExternalCommit || !j.posted.remove(&group) {
            return;
        }
        match refused {
            None => {
                j.joined += 1;
                // (the joined group's open cards: the Desk is read again)
                self.desk.want = true;
            }
            Some(code) => {
                let tries = j.tries.entry(group).or_default();
                *tries += 1;
                if has_moved(code) && *tries < JOIN_TRIES {
                    j.alone.push_back(group);
                } else {
                    j.not_joined.push((group, code.code()));
                }
            }
        }
    }

    /// The answer to joins posted together: each row is taken as the answer to its entry posted alone.
    pub(super) fn on_joins_posted(&mut self, ctx: &mut Ctx<'_>, entries: Vec<(u64, Option<GroupId>)>, answer: Result<Answer, Failure>, unauthorised: bool) {
        self.posting = None;
        let rows = match answer {
            Ok(Answer::Joins(rows)) if rows.len() == entries.len() => rows,
            Ok(_) => return self.pump_retry.fail(ctx.now, None, &mut self.jitter),
            Err(_) if unauthorised => return, // posted again, unchanged, once signed in
            Err(Failure::Refused { .. }) => {
                // the request itself was refused: each join goes alone and gets its own answer
                self.joins_alone = true;
                return;
            }
            Err(failure) => {
                let retry_after = match failure {
                    Failure::NotNow { retry_after_ms, .. } => retry_after_ms,
                    _ => None,
                };
                return self.pump_retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        };
        // Every answer the hub gave for good is told, in order; one that means "not now" leaves its entry at the
        // outbox's head for the next post (joins of different groups do not wait for each other).
        let mut wait: Option<Option<u64>> = None;
        for ((entry, group), row) in entries.into_iter().zip(rows) {
            match row {
                Ok(taken) => self.accepted(ctx, entry, OutboxKind::ExternalCommit, group, taken),
                // (a row's refusal names its code alone: no void record, no wait of its own)
                Err(failure) => {
                    if let Some(retry_after) = self.not_accepted(ctx, entry, OutboxKind::ExternalCommit, group, failure, None) {
                        wait = Some(wait.flatten().max(retry_after));
                    }
                }
            }
            if self.stopped.is_some() {
                return;
            }
        }
        match wait {
            Some(retry_after) => self.pump_retry.fail(ctx.now, retry_after, &mut self.jitter),
            None => self.pump_retry.ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trommi_core::ids::{RoomId, SessionId};

    fn join(n: u8, bytes: usize) -> OutboxEntry {
        let group = GroupId::session(RoomId::new([1; 32]), SessionId::new([n; 16]));
        OutboxEntry { id: u64::from(n), kind: OutboxKind::ExternalCommit, group: Some(group), epoch: 1, parts: vec![vec![0; bytes]] }
    }

    #[test]
    fn joins_posted_together_stop_at_the_byte_cap_of_a_post() {
        // three of 400 KiB (base64: ~533 KiB each): two would pass 1 MiB, so one goes alone
        let big: Vec<OutboxEntry> = (1..=3).map(|n| join(n, 400 << 10)).collect();
        assert_eq!(joins_at(&big), 1);
        // of 200 KiB (~267 KiB each), three fit below 1 MiB and the fourth does not
        let mid: Vec<OutboxEntry> = (1..=5).map(|n| join(n, 200 << 10)).collect();
        assert_eq!(joins_at(&mid), 3);
        // a first one beyond the cap still goes, alone
        let huge = vec![join(1, 2 << 20), join(2, 10)];
        assert_eq!(joins_at(&huge), 1);
        // small ones stop at the count, and at a second join of the same group
        let many: Vec<OutboxEntry> = (1..=40).map(|n| join(n, 100)).collect();
        assert_eq!(joins_at(&many), JOINS_AT_ONCE);
        let twice = vec![join(1, 100), join(2, 100), join(1, 100)];
        assert_eq!(joins_at(&twice), 2);
    }
}
