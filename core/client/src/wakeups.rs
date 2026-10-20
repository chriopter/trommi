//! Wakeups (spec/v1.md 9.3.8): an agent sets itself a time to start again (`wake_me`), once or again and again, and
//! its connector keeps the clock on the person's computer. Here, once for the connector and every app: what a wakeup
//! is (`wakeup/<id>` in the agent's session, the agent's register), the person's word on one (`wakeup_edit/<id>`: cancel
//! it, or another time), the guardrails (how many, how often, quiet hours, how many runs a day, the subscription's
//! window), the next time after a run, and the words the apps show.

use serde_json::{json, Map, Value};

/// A wakeup of a session (agent device) and the person's word on one (human devices), session group.
pub const PREFIX: &str = "wakeup/";
pub const EDIT_PREFIX: &str = "wakeup_edit/";
/// What it says to do then: at most so many characters.
pub const NOTE_MAX: usize = 300;
/// At most so many open wakeups a session, of them so many that come again.
pub const PER_SESSION_MAX: usize = 5;
pub const RECURRING_MAX: usize = 2;
/// A one-time wakeup at least so far ahead, one that comes again at least so far apart (and at most so far).
pub const MIN_AHEAD_MS: u64 = 5 * 60_000;
pub const MIN_EVERY_MS: u64 = 3_600_000;
pub const MAX_EVERY_MS: u64 = 30 * DAY_MS;
/// At most so far ahead at all.
pub const MAX_AHEAD_MS: u64 = 60 * DAY_MS;
/// At most so many runs a wakeup starts in a session on one day (the person's day).
pub const RUNS_PER_DAY: u32 = 12;
/// A run may take so long, and so many turns.
pub const RUN_MAX_MS: u64 = 30 * 60_000;
pub const RUN_MAX_TURNS: u32 = 30;
/// Above this share of the subscription's five-hour window a wakeup waits (by [`USAGE_WAIT_MS`]).
pub const USAGE_PAUSE: f64 = 0.8;
pub const USAGE_WAIT_MS: u64 = 30 * 60_000;
/// Quiet hours in the person's zone, from..to (hours): a wakeup in them is moved to their end.
pub const QUIET_FROM: u32 = 22;
pub const QUIET_TO: u32 = 7;
pub const DAY_MS: u64 = 86_400_000;
const HOUR_MS: u64 = 3_600_000;

/// What became of a wakeup's last run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LastRun {
    pub at: u64,
    /// "ran", "done" (it ended), "failed", "late" (the computer was off: run once, late), "waited" (moved by quiet
    /// hours, the day's limit or the subscription's window).
    pub result: String,
    pub detail: String,
}

/// One wakeup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wakeup {
    /// Its id: the tail of its register's name.
    pub id: String,
    /// When it runs next.
    pub at: u64,
    /// Every so many ms again (0: once).
    pub every: u64,
    pub note: String,
    /// The helper (a subagent) it is for; none: the agent.
    pub helper: Option<String>,
    /// When the agent set it.
    pub set_at: u64,
    pub last: Option<LastRun>,
    /// Runs on the day `day` (days since the epoch, in the person's zone).
    pub day: u64,
    pub day_runs: u32,
    /// The person's last word on it that was applied: its mark ([`mark`]; "" for none).
    pub edited: String,
}

/// A note as it is kept: one paragraph, at most [`NOTE_MAX`] characters.
pub fn clean_note(note: &str) -> String {
    note.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(NOTE_MAX).collect::<String>().trim_end().to_string()
}

impl Wakeup {
    pub fn read(id: &str, v: &Value) -> Option<Wakeup> {
        let o = v.as_object()?;
        let num = |k: &str| o.get(k).and_then(Value::as_u64).unwrap_or(0);
        let at = o.get("at")?.as_u64()?;
        let last = o.get("last").and_then(Value::as_object).map(|l| LastRun {
            at: l.get("at").and_then(Value::as_u64).unwrap_or(0),
            result: l.get("result").and_then(Value::as_str).unwrap_or("").chars().take(20).collect(),
            detail: clean_note(l.get("detail").and_then(Value::as_str).unwrap_or("")).chars().take(120).collect(),
        });
        Some(Wakeup {
            id: id.to_string(),
            at,
            every: num("every"),
            note: clean_note(o.get("note")?.as_str()?),
            helper: o.get("helper").and_then(Value::as_str).map(|h| h.chars().take(40).collect::<String>()).filter(|h| !h.trim().is_empty()),
            set_at: num("set_at"),
            last,
            day: num("day"),
            day_runs: num("day_runs") as u32,
            edited: o.get("edited").and_then(Value::as_str).unwrap_or("").chars().take(40).collect(),
        })
    }

    pub fn value(&self) -> Value {
        let mut o = Map::new();
        o.insert("at".into(), json!(self.at));
        if self.every > 0 {
            o.insert("every".into(), json!(self.every));
        }
        o.insert("note".into(), json!(self.note));
        if let Some(h) = &self.helper {
            o.insert("helper".into(), json!(h));
        }
        o.insert("set_at".into(), json!(self.set_at));
        if let Some(l) = &self.last {
            o.insert("last".into(), json!({ "at": l.at, "result": l.result, "detail": l.detail }));
        }
        if self.day_runs > 0 {
            o.insert("day".into(), json!(self.day));
            o.insert("day_runs".into(), json!(self.day_runs));
        }
        if !self.edited.is_empty() {
            o.insert("edited".into(), json!(self.edited));
        }
        Value::Object(o)
    }
}

/// The wakeups among a session's registers (`wakeup/<id>`), the next first.
pub fn of<'a>(registers: impl Iterator<Item = (&'a String, &'a Value)>) -> Vec<Wakeup> {
    let mut list: Vec<Wakeup> = registers.filter_map(|(k, v)| k.strip_prefix(PREFIX).and_then(|id| Wakeup::read(id, v))).collect();
    list.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
    list
}

/// The person's word on a wakeup: cancel it, or move it to another time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    Cancel,
    At(u64),
}

impl Edit {
    pub fn read(v: &Value) -> Option<Edit> {
        if v.get("cancel") == Some(&Value::Bool(true)) {
            return Some(Edit::Cancel);
        }
        v.get("at").and_then(Value::as_u64).map(Edit::At)
    }
    /// His word with its mark (`n`: new for each word, so that the connector applies each once).
    pub fn value(&self, mark: &str) -> Value {
        match self {
            Edit::Cancel => json!({ "cancel": true, "n": mark }),
            Edit::At(at) => json!({ "at": at, "n": mark }),
        }
    }
}

/// The mark of the person's word on a wakeup ("" for none).
pub fn mark(v: &Value) -> String {
    v.get("n").and_then(Value::as_str).unwrap_or("").chars().take(40).collect()
}

impl Edit {
}

// ---- times ---------------------------------------------------------------------------------------------------------

/// A span as the agent writes it: "90s", "30m", "2h", "1d", "1w" (whole numbers); none for anything else.
pub fn span(text: &str) -> Option<u64> {
    let t = text.trim().to_lowercase();
    let unit = t.chars().last()?;
    let n: u64 = t[..t.len() - unit.len_utf8()].trim().parse().ok()?;
    let ms = match unit {
        's' => 1_000,
        'm' => 60_000,
        'h' => HOUR_MS,
        'd' => DAY_MS,
        'w' => 7 * DAY_MS,
        _ => return None,
    };
    n.checked_mul(ms).filter(|ms| *ms > 0)
}

/// A time as the agent writes it, in the person's zone (`zone_min` east of UTC): "09:00" (the next 09:00),
/// "2026-10-21 09:00" or "2026-10-21T09:00", or with a zone ("…Z", "…+02:00"). None for anything else.
pub fn time(text: &str, now: u64, zone_min: i32) -> Option<u64> {
    let t = text.trim();
    let zone = zone_min as i64 * 60_000;
    if let Some((h, m)) = hm(t) {
        let local_now = now as i64 + zone;
        let day_start = local_now - local_now.rem_euclid(DAY_MS as i64);
        let mut at = day_start + (h * 60 + m) as i64 * 60_000;
        if at <= local_now {
            at += DAY_MS as i64;
        }
        return u64::try_from(at - zone).ok();
    }
    let (date, rest) = t.split_at(t.find([' ', 'T'])?);
    let rest = &rest[1..];
    let mut p = date.split('-');
    let (y, mo, d): (i64, i64, i64) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?, p.next()?.parse().ok()?);
    if p.next().is_some() || !(1..=12).contains(&mo) || !(1..=31).contains(&d) {
        return None;
    }
    let (clock, offset) = if let Some(c) = rest.strip_suffix('Z') {
        (c, Some(0i64))
    } else if let Some(i) = rest.rfind(['+', '-']).filter(|i| *i >= 4) {
        let (c, z) = rest.split_at(i);
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let (zh, zm) = hm(&z[1..])?;
        (c, Some(sign * (zh * 60 + zm) as i64 * 60_000))
    } else {
        (rest, None)
    };
    let clock = clock.get(..5).unwrap_or(clock);
    let (h, m) = hm(clock)?;
    let days = days_from_civil(y, mo, d);
    let local = days * DAY_MS as i64 + (h * 60 + m) as i64 * 60_000;
    u64::try_from(local - offset.unwrap_or(zone)).ok()
}

fn hm(t: &str) -> Option<(u64, u64)> {
    let (h, m) = t.split_once(':')?;
    let (h, m): (u64, u64) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && t.len() <= 5).then_some((h, m))
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The person's day of a moment (days since the epoch in his zone).
pub fn day_of(at: u64, zone_min: i32) -> u64 {
    ((at as i64 + zone_min as i64 * 60_000).max(0) as u64) / DAY_MS
}

/// A moment moved out of quiet hours (`QUIET_FROM`..`QUIET_TO` in the person's zone) to their end.
pub fn out_of_quiet(at: u64, zone_min: i32) -> u64 {
    let zone = zone_min as i64 * 60_000;
    let local = at as i64 + zone;
    let into_day = local.rem_euclid(DAY_MS as i64);
    let hour = (into_day / HOUR_MS as i64) as u32;
    // (the quiet hours run over midnight)
    let quiet = !(QUIET_TO..QUIET_FROM).contains(&hour);
    if !quiet {
        return at;
    }
    let day_start = local - into_day;
    let mut end = day_start + QUIET_TO as i64 * HOUR_MS as i64;
    if end <= local {
        end += DAY_MS as i64;
    }
    (end - zone).max(0) as u64
}

// ---- the guardrails --------------------------------------------------------------------------------------------------

/// Why a wakeup is not set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Neither `at` nor `in`, both, or one that does not read.
    When,
    /// Sooner than [`MIN_AHEAD_MS`] or later than [`MAX_AHEAD_MS`].
    TooSoon,
    TooLate,
    /// `every` below [`MIN_EVERY_MS`], above [`MAX_EVERY_MS`], or not a span.
    Every,
    /// [`PER_SESSION_MAX`] open already, or [`RECURRING_MAX`] that come again.
    TooMany,
    TooManyRecurring,
    /// No note: a wakeup says what to do.
    NoNote,
}

impl Refusal {
    pub fn words(self) -> String {
        match self {
            Refusal::When => "give `at` (a time: 09:00 or 2026-10-21 09:00) or `in` (30m, 2h, 1d), one of them".into(),
            Refusal::TooSoon => format!("at least {} minutes from now", MIN_AHEAD_MS / 60_000),
            Refusal::TooLate => format!("at most {} days from now", MAX_AHEAD_MS / DAY_MS),
            Refusal::Every => format!("`every` is a span of at least {} hour and at most {} days (1h, 1d, 1w)", MIN_EVERY_MS / HOUR_MS, MAX_EVERY_MS / DAY_MS),
            Refusal::TooMany => format!("this session has {PER_SESSION_MAX} wakeups already: cancel_wakeup one first"),
            Refusal::TooManyRecurring => format!("this session has {RECURRING_MAX} wakeups that come again already: cancel_wakeup one first"),
            Refusal::NoNote => "`note` is required: what to do when you wake".into(),
        }
    }
}

/// A new wakeup checked against the guardrails: when (`at` or `in`), how often (`every`), the session's open ones.
/// Its time is moved out of quiet hours. `zone_min`: the person's zone.
#[allow(clippy::too_many_arguments)]
pub fn set(id: &str, at: Option<&str>, in_: Option<&str>, every: Option<&str>, note: &str, helper: Option<&str>, open: &[Wakeup], now: u64, zone_min: i32) -> Result<Wakeup, Refusal> {
    let note = clean_note(note);
    if note.is_empty() {
        return Err(Refusal::NoNote);
    }
    let at = match (at.filter(|a| !a.trim().is_empty()), in_.filter(|i| !i.trim().is_empty())) {
        (Some(a), None) => time(a, now, zone_min).ok_or(Refusal::When)?,
        (None, Some(i)) => now.saturating_add(span(i).ok_or(Refusal::When)?),
        _ => return Err(Refusal::When),
    };
    if at < now + MIN_AHEAD_MS {
        return Err(Refusal::TooSoon);
    }
    if at > now + MAX_AHEAD_MS {
        return Err(Refusal::TooLate);
    }
    let every = match every.filter(|e| !e.trim().is_empty()) {
        None => 0,
        Some(e) => span(e).filter(|ms| (MIN_EVERY_MS..=MAX_EVERY_MS).contains(ms)).ok_or(Refusal::Every)?,
    };
    if open.len() >= PER_SESSION_MAX {
        return Err(Refusal::TooMany);
    }
    if every > 0 && open.iter().filter(|w| w.every > 0).count() >= RECURRING_MAX {
        return Err(Refusal::TooManyRecurring);
    }
    // (it carries the session's count of runs today: a run that sets the next wakeup still counts)
    let runs = session_runs(open, now, zone_min);
    Ok(Wakeup { id: id.to_string(), at: out_of_quiet(at, zone_min), every, note, helper: helper.map(str::to_string).filter(|h| !h.trim().is_empty()), set_at: now, day: if runs > 0 { day_of(now, zone_min) } else { 0 }, day_runs: runs, ..Default::default() })
}

/// What happens to a wakeup that is due at `now`.
#[derive(Clone, Debug, PartialEq)]
pub enum Due {
    /// Run it now; the wakeup as it stands after (none: it was once and is done). `late`: it was missed (the computer
    /// was off), run once now.
    Run { next: Option<Wakeup>, late: bool },
    /// Not now: wait until the wakeup's new time (quiet hours, the day's limit, the subscription's window).
    Wait(Wakeup),
}

/// The runs of a session today: each of its wakeups carries the session's count (the highest of today's).
pub fn session_runs(open: &[Wakeup], now: u64, zone_min: i32) -> u32 {
    let today = day_of(now, zone_min);
    open.iter().filter(|w| w.day == today).map(|w| w.day_runs).max().unwrap_or(0)
}

/// A wakeup at `now`: run it, or let it wait. `usage`: the share of the subscription's five-hour window in use (none:
/// not known); `runs`: the session's runs today ([`session_runs`]). A wakeup that comes again goes on to its next time
/// after now (missed ones are not made up one by one).
pub fn due(w: &Wakeup, now: u64, zone_min: i32, usage: Option<f64>, runs: u32) -> Option<Due> {
    if w.at > now {
        return None;
    }
    let today = day_of(now, zone_min);
    let waits = |at: u64, why: &str| {
        let mut next = w.clone();
        next.at = at.max(now + 60_000);
        next.last = Some(LastRun { at: now, result: "waited".into(), detail: why.into() });
        Due::Wait(next)
    };
    let quiet = out_of_quiet(now, zone_min);
    if quiet > now {
        return Some(waits(quiet, "quiet hours"));
    }
    if runs >= RUNS_PER_DAY {
        let tomorrow = out_of_quiet((today + 1) * DAY_MS - (zone_min as i64 * 60_000) as u64, zone_min);
        return Some(waits(tomorrow, "the day's limit of runs"));
    }
    if usage.is_some_and(|u| u >= USAGE_PAUSE) {
        return Some(waits(now + USAGE_WAIT_MS, "the subscription's window is almost used"));
    }
    let late = now.saturating_sub(w.at) > 15 * 60_000;
    let next = (w.every > 0).then(|| {
        let mut at = w.at;
        while at <= now {
            at = at.saturating_add(w.every);
        }
        Wakeup { at: out_of_quiet(at, zone_min), day: today, day_runs: runs + 1, last: Some(LastRun { at: now, result: if late { "late".into() } else { "ran".into() }, detail: String::new() }), ..w.clone() }
    });
    Some(Due::Run { next, late })
}

/// The words a woken run starts with: what the agent asked itself to do, and that it was a wakeup.
pub fn prompt(w: &Wakeup, late: bool, at_words: &str) -> String {
    format!(
        "[Trommi] Your wakeup{} ({at_words}{}): {}\nDo it, put what came out on the board as usual (a card for a result or a choice), and set the next wakeup only if one is needed.",
        w.helper.as_deref().map(|h| format!(" for helper \"{h}\"")).unwrap_or_default(),
        if late { ", late: the computer was off" } else { "" },
        w.note
    )
}

/// A moment as a clock shows it in the person's zone: "09:00".
pub fn clock(at: u64, zone_min: i32) -> String {
    let local = (at as i64 + zone_min as i64 * 60_000).rem_euclid(DAY_MS as i64) / 60_000;
    format!("{:02}:{:02}", local / 60, local % 60)
}

/// The person's word on a wakeup applied: none when he cancelled it; `mark` is that of his word.
pub fn apply_edit(w: &Wakeup, edit: &Edit, mark: &str, now: u64, zone_min: i32) -> Option<Wakeup> {
    match edit {
        Edit::Cancel => None,
        Edit::At(at) => Some(Wakeup { at: out_of_quiet((*at).clamp(now + 60_000, now + MAX_AHEAD_MS), zone_min), edited: mark.to_string(), ..w.clone() }),
    }
}

/// Whether the person's word (of mark `mark`) is applied: its wakeup is gone, or carries it (a word without a mark
/// is none).
pub fn edit_applied(w: Option<&Wakeup>, mark: &str) -> bool {
    mark.is_empty() || w.is_none_or(|w| w.edited == mark)
}

/// One look of the connector's clock at a session's wakeups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tick {
    /// Registers to write: a wakeup as it stands now, or none (it is gone).
    pub writes: Vec<(String, Option<Wakeup>)>,
    /// The runs to start: the wakeup's id and the words its run starts with.
    pub runs: Vec<(String, String)>,
    /// The session's runs today after this look (its connector keeps the count, also when no wakeup is left).
    pub day_runs: u32,
}

/// A session's clock at `now`: the person's words applied (`edits`: id, word, its mark), then each due wakeup run
/// or put off, at most one run per look. `running`: the wakeup a waker woke the closed session for (the guardrails
/// were its): run now, unless the person cancelled or moved it meanwhile. `counted`: the session's runs as its
/// connector kept them (the day, how many), which no wakeup taken away resets.
#[allow(clippy::too_many_arguments)]
pub fn tick(open: &[Wakeup], edits: &[(String, Edit, String)], now: u64, zone_min: i32, usage: Option<f64>, running: Option<&str>, counted: (u64, u32)) -> Tick {
    let mut out = Tick::default();
    let mut list: Vec<Wakeup> = Vec::new();
    for w in open {
        match edits.iter().find(|(id, _, mark)| *id == w.id && !edit_applied(Some(w), mark)) {
            Some((_, edit, mark)) => match apply_edit(w, edit, mark, now, zone_min) {
                Some(n) => {
                    out.writes.push((w.id.clone(), Some(n.clone())));
                    list.push(n);
                }
                None => out.writes.push((w.id.clone(), None)),
            },
            None => list.push(w.clone()),
        }
    }
    list.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
    let today = day_of(now, zone_min);
    let mut runs = session_runs(&list, now, zone_min).max(if counted.0 == today { counted.1 } else { 0 });
    let ran_before = runs;
    for w in &list {
        // (a word of his applied in this look wins over the waker's start)
        let edited = out.writes.iter().any(|(id, _)| *id == w.id);
        let started = running == Some(w.id.as_str()) && !edited && w.at <= now + MIN_AHEAD_MS;
        let step = if started { due(&Wakeup { at: w.at.min(now), ..w.clone() }, now, zone_min, None, 0).filter(|d| matches!(d, Due::Run { .. })) } else if out.runs.is_empty() { due(w, now, zone_min, usage, runs) } else { None };
        match step {
            None => {}
            Some(Due::Wait(n)) => {
                out.writes.retain(|(id, _)| *id != w.id);
                out.writes.push((w.id.clone(), Some(n)));
            }
            Some(Due::Run { next, late }) => {
                runs += 1;
                out.runs.push((w.id.clone(), prompt(w, late, &clock(w.at, zone_min))));
                out.writes.retain(|(id, _)| *id != w.id);
                out.writes.push((w.id.clone(), next));
            }
        }
    }
    out.day_runs = runs;
    // (every wakeup left carries the session's new count)
    if runs > ran_before {
        for w in &list {
            let written = out.writes.iter().position(|(id, _)| *id == w.id);
            let mut n = match written {
                Some(i) => match out.writes[i].1.clone() {
                    Some(n) => n,
                    None => continue,
                },
                None => w.clone(),
            };
            (n.day, n.day_runs) = (today, runs);
            match written {
                Some(i) => out.writes[i].1 = Some(n),
                None => out.writes.push((w.id.clone(), Some(n))),
            }
        }
    }
    out
}

/// How often, in words: "once", "every hour", "every 2 hours", "every day", "every 3 days", "every week".
pub fn every_words(every: u64) -> String {
    match every {
        0 => "once".into(),
        e if e % (7 * DAY_MS) == 0 && e / (7 * DAY_MS) == 1 => "every week".into(),
        e if e % DAY_MS == 0 => if e / DAY_MS == 1 { "every day".into() } else { format!("every {} days", e / DAY_MS) },
        e if e % HOUR_MS == 0 => if e / HOUR_MS == 1 { "every hour".into() } else { format!("every {} hours", e / HOUR_MS) },
        e => format!("every {} minutes", e / 60_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_792_490_400_000; // 2026-10-20 10:00 UTC
    const ZONE: i32 = 120; // the person's zone: UTC+2

    #[test]
    fn spans_and_times_read_as_the_agent_writes_them() {
        assert_eq!((span("30m"), span("2h"), span("1d"), span(" 1W "), span("90s")), (Some(1_800_000), Some(7_200_000), Some(DAY_MS), Some(7 * DAY_MS), Some(90_000)));
        for bad in ["", "m", "2x", "-3h", "0h", "1.5h"] {
            assert_eq!(span(bad), None, "{bad}");
        }
        // 12:00 local now; 09:00 is tomorrow, 13:00 today
        assert_eq!(time("13:00", NOW, ZONE), Some(NOW + 3_600_000));
        assert_eq!(time("09:00", NOW, ZONE), Some(NOW + 21 * 3_600_000));
        assert_eq!(time("2026-10-21 09:00", NOW, ZONE), Some(NOW + 21 * 3_600_000));
        assert_eq!(time("2026-10-21T07:00Z", NOW, ZONE), Some(NOW + 21 * 3_600_000));
        assert_eq!(time("2026-10-21T09:00+02:00", NOW, ZONE), Some(NOW + 21 * 3_600_000));
        for bad in ["", "25:00", "tomorrow", "2026-13-01 09:00", "9"] {
            assert_eq!(time(bad, NOW, ZONE), None, "{bad}");
        }
    }

    #[test]
    fn quiet_hours_move_a_wakeup_to_seven() {
        let at_23 = NOW + 11 * 3_600_000; // 23:00 local
        assert_eq!(out_of_quiet(at_23, ZONE), NOW + 19 * 3_600_000); // 07:00 local
        let at_05 = NOW + 17 * 3_600_000;
        assert_eq!(out_of_quiet(at_05, ZONE), NOW + 19 * 3_600_000);
        assert_eq!(out_of_quiet(NOW, ZONE), NOW, "noon is no quiet hour");
    }

    #[test]
    fn the_guardrails_hold() {
        let w = set("a", None, Some("30m"), None, " check   the tests ", Some("Testing"), &[], NOW, ZONE).unwrap();
        assert_eq!((w.at, w.every, w.note.as_str(), w.helper.as_deref()), (NOW + 1_800_000, 0, "check the tests", Some("Testing")));
        assert_eq!(set("a", None, Some("1m"), None, "x", None, &[], NOW, ZONE), Err(Refusal::TooSoon));
        assert_eq!(set("a", None, Some("90d"), None, "x", None, &[], NOW, ZONE), Err(Refusal::TooLate));
        assert_eq!(set("a", Some("13:00"), Some("1h"), None, "x", None, &[], NOW, ZONE), Err(Refusal::When));
        assert_eq!(set("a", None, None, None, "x", None, &[], NOW, ZONE), Err(Refusal::When));
        assert_eq!(set("a", None, Some("1h"), Some("10m"), "x", None, &[], NOW, ZONE), Err(Refusal::Every));
        assert_eq!(set("a", None, Some("1h"), None, "  ", None, &[], NOW, ZONE), Err(Refusal::NoNote));
        let open: Vec<Wakeup> = (0..PER_SESSION_MAX).map(|i| Wakeup { id: i.to_string(), ..w.clone() }).collect();
        assert_eq!(set("b", None, Some("1h"), None, "x", None, &open, NOW, ZONE), Err(Refusal::TooMany));
        let rec: Vec<Wakeup> = (0..RECURRING_MAX).map(|i| Wakeup { id: i.to_string(), every: DAY_MS, ..w.clone() }).collect();
        assert_eq!(set("b", None, Some("1h"), Some("1d"), "x", None, &rec, NOW, ZONE), Err(Refusal::TooManyRecurring));
        assert!(set("b", None, Some("1h"), None, "x", None, &rec, NOW, ZONE).is_ok(), "a once next to them is fine");
        // (one in quiet hours waits for their end)
        let night = set("c", Some("23:30"), None, None, "x", None, &[], NOW, ZONE).unwrap();
        assert_eq!(night.at, NOW + 19 * 3_600_000);
        assert_eq!(Wakeup::read("c", &night.value()), Some(night));
    }

    #[test]
    fn a_due_wakeup_runs_or_waits_and_comes_again() {
        let once = Wakeup { id: "a".into(), at: NOW, note: "x".into(), set_at: NOW - 1, ..Default::default() };
        assert_eq!(due(&once, NOW - 1, ZONE, None, 0), None, "not yet");
        assert_eq!(due(&once, NOW, ZONE, None, 0), Some(Due::Run { next: None, late: false }));
        // every hour: the next one after now, missed ones not made up one by one; counted for the day
        let hourly = Wakeup { every: 3_600_000, ..once.clone() };
        let Some(Due::Run { next: Some(n), late }) = due(&hourly, NOW + 3 * 3_600_000 + 60_000, ZONE, None, 0) else { panic!() };
        assert!(late);
        assert_eq!((n.at, n.day_runs), (NOW + 4 * 3_600_000, 1));
        // the day's limit, the subscription's window, quiet hours: it waits
        let Some(Due::Wait(w)) = due(&hourly, NOW, ZONE, None, RUNS_PER_DAY) else { panic!() };
        assert_eq!(w.at, NOW + 19 * 3_600_000, "tomorrow 07:00");
        let Some(Due::Wait(w)) = due(&hourly, NOW, ZONE, Some(0.9), 0) else { panic!() };
        assert_eq!((w.at, w.last.unwrap().detail.as_str()), (NOW + USAGE_WAIT_MS, "the subscription's window is almost used"));
        let Some(Due::Wait(w)) = due(&Wakeup { at: NOW + 11 * 3_600_000, ..hourly }, NOW + 11 * 3_600_000, ZONE, None, 0) else { panic!() };
        assert_eq!(w.at, NOW + 19 * 3_600_000);
        assert_eq!(of(std::iter::empty()), Vec::<Wakeup>::new(), "no registers, no wakeups");
    }

    #[test]
    fn the_words() {
        assert_eq!([0, 3_600_000, 7_200_000, DAY_MS, 3 * DAY_MS, 7 * DAY_MS].map(every_words), ["once", "every hour", "every 2 hours", "every day", "every 3 days", "every week"]);
        let w = Wakeup { note: "check the tests".into(), helper: Some("Testing".into()), ..Default::default() };
        assert!(prompt(&w, true, "09:00").starts_with("[Trommi] Your wakeup for helper \"Testing\" (09:00, late: the computer was off): check the tests"));
        assert_eq!((Edit::read(&Edit::At(5).value("m")), mark(&Edit::At(5).value("m"))), (Some(Edit::At(5)), "m".to_string()));
        assert_eq!(Edit::read(&Edit::Cancel.value("m")), Some(Edit::Cancel));
        assert_eq!(Edit::read(&json!({})), None);
    }

    #[test]
    fn the_clock_runs_one_at_a_time_counts_for_the_session_and_takes_his_word() {
        let a = Wakeup { id: "a".into(), at: NOW, note: "first".into(), every: DAY_MS, ..Default::default() };
        let b = Wakeup { id: "b".into(), at: NOW - 60_000, note: "second".into(), ..Default::default() };
        let c = Wakeup { id: "c".into(), at: NOW + 3_600_000, note: "later".into(), ..Default::default() };
        let t = tick(&[a.clone(), b.clone(), c.clone()], &[], NOW, ZONE, None, None, (0, 0));
        assert_eq!(t.runs.len(), 1, "one run per look");
        assert_eq!(t.runs[0].0, "b");
        assert!(t.runs[0].1.contains("(11:59): second"), "{}", t.runs[0].1);
        // b is done; a (still due) and c carry the session's count of one run today
        assert!(t.writes.contains(&("b".into(), None)));
        let counts: Vec<(String, u32)> = t.writes.iter().filter_map(|(id, w)| w.as_ref().map(|w| (id.clone(), w.day_runs))).collect();
        assert_eq!(counts, vec![("a".into(), 1), ("c".into(), 1)]);
        // nothing due, nothing written; no wakeups at all, nothing
        assert_eq!(tick(std::slice::from_ref(&c), &[], NOW, ZONE, None, None, (0, 0)), Tick::default());
        assert_eq!(tick(&[], &[], NOW, ZONE, None, None, (0, 0)), Tick::default());
        // the wakeup a waker woke the session for runs now, unless he cancelled it meanwhile
        let soon = Wakeup { at: NOW + 60_000, ..c.clone() };
        let t = tick(std::slice::from_ref(&soon), &[], NOW, ZONE, None, Some("c"), (0, 0));
        assert_eq!((t.runs.len(), t.writes.clone()), (1, vec![("c".to_string(), None)]));
        assert_eq!(tick(&[soon], &[("c".into(), Edit::Cancel, "7".into())], NOW, ZONE, None, Some("c"), (0, 0)).runs, vec![]);
        assert!(tick(std::slice::from_ref(&c), &[], NOW, ZONE, None, Some("c"), (0, 0)).runs.is_empty(), "an hour ahead on the board: not yet");
        // his word: cancelled, or moved (once: applied words are not applied again)
        let t = tick(std::slice::from_ref(&c), &[("c".into(), Edit::Cancel, "7".into())], NOW, ZONE, None, None, (0, 0));
        assert_eq!(t.writes, vec![("c".to_string(), None)]);
        let t = tick(std::slice::from_ref(&c), &[("c".into(), Edit::At(NOW + 2 * 3_600_000), "7".into())], NOW, ZONE, None, None, (0, 0));
        let moved = t.writes[0].1.clone().unwrap();
        assert_eq!((moved.at, moved.edited.as_str()), (NOW + 2 * 3_600_000, "7"));
        assert!(edit_applied(Some(&moved), "7") && edit_applied(None, "7") && !edit_applied(Some(&c), "7") && edit_applied(Some(&c), ""));
        assert_eq!(tick(&[moved], &[("c".into(), Edit::At(NOW + 2 * 3_600_000), "7".into())], NOW, ZONE, None, None, (0, 0)), Tick::default());
        assert_eq!(clock(NOW, ZONE), "12:00");
        // the count kept by the connector holds when no wakeup is left: the day's limit still stops a new one
        let t = tick(std::slice::from_ref(&b), &[], NOW, ZONE, None, None, (day_of(NOW, ZONE), RUNS_PER_DAY));
        assert!(t.runs.is_empty() && matches!(t.writes[0].1.as_ref().and_then(|w| w.last.clone()), Some(l) if l.result == "waited"));
        let t = tick(std::slice::from_ref(&b), &[], NOW, ZONE, None, None, (day_of(NOW, ZONE) - 1, RUNS_PER_DAY));
        assert_eq!((t.runs.len(), t.day_runs), (1, 1), "yesterday's count is gone");
        // a wakeup the waker started but he moved meanwhile waits for his time
        let t = tick(std::slice::from_ref(&b), &[("b".into(), Edit::At(NOW + 3_600_000), "8".into())], NOW, ZONE, None, Some("b"), (0, 0));
        assert!(t.runs.is_empty() && t.writes[0].1.as_ref().is_some_and(|w| w.at == NOW + 3_600_000));
    }

    #[test]
    fn a_new_wakeup_carries_the_days_runs() {
        let busy = Wakeup { id: "a".into(), at: NOW + 3_600_000, note: "x".into(), day: day_of(NOW, ZONE), day_runs: 4, ..Default::default() };
        let n = set("b", None, Some("1h"), None, "y", None, std::slice::from_ref(&busy), NOW, ZONE).unwrap();
        assert_eq!((n.day_runs, session_runs(&[busy, n.clone()], NOW, ZONE)), (4, 4));
        assert_eq!(session_runs(&[], NOW, ZONE), 0);
    }
}
