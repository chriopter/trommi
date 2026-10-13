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
//! The code's keys live in the engine only while this runs and are dropped with it. Nothing of it is stored: after
//! a restart the joins already in the outbox are posted, and the groups not reached are left to another human
//! device of the person (5.2.7), or to the shell asking again with the code.

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
    keys: RecoveryKeys,
    later: Option<Later>,
    /// Still to be read, together.
    together: VecDeque<GroupId>,
    /// Rows of an answer not looked at yet.
    rows: VecDeque<GroupRead>,
    /// To be read alone.
    alone: VecDeque<GroupId>,
    one: Option<One>,
    asked: Option<IoId>,
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

impl<H: HubCodec> Engine<H> {
    /// Joins the live session groups of a sign-in with the code (see the module). A second call while one runs adds
    /// its groups and replaces the keys.
    pub fn join_sessions(&mut self, ctx: &mut Ctx<'_>, joins: SessionJoins) {
        if self.stopped.is_some() {
            return;
        }
        let mut joining = match self.joining.take() {
            Some(mut joining) => {
                joining.keys = joins.keys;
                joining.later = joins.later;
                joining
            }
            None => Joining {
                keys: joins.keys,
                later: joins.later,
                together: VecDeque::new(),
                rows: VecDeque::new(),
                alone: VecDeque::new(),
                one: None,
                asked: None,
                retry: Retry::default(),
                tries: BTreeMap::new(),
                posted: BTreeSet::new(),
                joined: 0,
                not_joined: Vec::new(),
            },
        };
        for group in joins.groups {
            if !group.is_room() && !joining.together.contains(&group) && !joining.alone.contains(&group) && !joining.posted.contains(&group) {
                joining.together.push_back(group);
            }
        }
        self.joining = Some(joining);
        self.drive(ctx);
    }

    /// Whether the session groups of a sign-in are still being joined.
    pub fn joining_sessions(&self) -> bool {
        self.joining.is_some()
    }

    pub(super) fn drive_joins(&mut self, ctx: &mut Ctx<'_>) {
        // (the core judges a session's Commits against the room states this device holds: the order first)
        if !self.caught_up || self.pass || !self.todo.is_empty() || self.repair.is_some() {
            return;
        }
        let Some(mut j) = self.joining.take() else { return };
        if j.asked.is_none() && !j.retry.waiting(ctx.now) {
            self.joins_step(ctx, &mut j);
        }
        if self.stopped.is_some() {
            return;
        }
        let through = j.asked.is_none() && j.one.is_none() && j.rows.is_empty() && j.together.is_empty() && j.alone.is_empty() && j.posted.is_empty();
        if through {
            // every session's keys are at hand now: the Desk of the joined groups, and the room's past once more
            self.desk.want = true;
            self.want_reread(0);
            self.report(Report::Groups);
            self.report(Report::SessionsJoined { joined: j.joined, not_joined: j.not_joined });
            return self.drive(ctx);
        }
        self.joining = Some(j);
    }

    fn joins_step(&mut self, ctx: &mut Ctx<'_>, j: &mut Joining) {
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
        let built = match &j.later {
            Some(later) => self.device.join_session_checked_keys(&j.keys, &one.group, current, &later.rows, &later.links, ctx.now),
            None => self.device.join_session_checked(&j.keys, &one.group, current, ctx.now),
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
            Ok(false) if row.more => j.one = Some(one),
            // (the log ends before the epoch named: the check and the GroupInfo decide)
            Ok(false) => self.join_build(ctx, j, one),
            Err(e) => self.join_failed(ctx, j, group, &e),
        }
    }

    pub(super) fn on_join_read(&mut self, ctx: &mut Ctx<'_>, what: Read, answer: Result<Answer, Failure>, unauthorised: bool) {
        let Some(mut j) = self.joining.take() else { return };
        j.asked = None;
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
                Err(failure) => {
                    let refusal = match &failure {
                        Failure::Refused { status, code: Some(code) } => {
                            Some(Refusal { code: code.clone(), status: *status, voided: false, retry_after_ms: None, epoch: None, seq: None, device: None, group })
                        }
                        _ => None,
                    };
                    if let Some(retry_after) = self.not_accepted(ctx, entry, OutboxKind::ExternalCommit, group, failure, refusal) {
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
