//! A session's state as every app shows it: its link (whether it hears the person), whether it is stopped (the red
//! hand), whether it is quiet. One rule for every app, decided at the model's clock; each answer says when it may
//! change by the clock alone (`recheck_at`), so an app asks again then and never decides by itself.
//!
//! The link, from what its connector reports (README "The link"):
//! - `gone`: no stream, and no word that Claude Code lives on;
//! - `cut`: no stream but its Claude Code lives (`exit.claude` alive); or a stream, and a session of its folder lost
//!   its connector (`cut_since`); or a stream with nobody attached;
//! - `live`: hears at once (also: an agent that reports nothing);
//! - `oncall`: hears on its next tool call;
//! - `asleep`: on call, and no tool call for [`ASLEEP_MS`] (a session in Codex is never asleep: it reads on its next
//!   turn).
//!
//! The red hand: a session is stopped when it is not archived and
//! - it is cut off;
//! - it is disconnected while a status line of its says "working", for longer than [`OFFLINE_GRACE_MS`] (and longer
//!   than that since this app started: every session is away until it links again);
//! - it is connected and waits for the person: an open approval request of it, or an open card of it marked as
//!   blocking (urgency critical), that stands on the Desk now (the stack: not held, not put off, not with its agent)
//!   and is read on this device (one whose body has not come yet waits quietly, on the Desk and here).
//!   What is not on the Desk raises no hand, so the hand and the Desk never disagree.
//!
//! Quiet (a grey hint, never a stop): connected, a status line says "working", and nothing from it for [`QUIET_MS`].
//!
//! A session works while one of its status lines shows "working" ([`line_state`]): the chip and the line never
//! disagree.
//!
//! What a status line shows ([`line_state`]), so that it never says more than the Desk's "Off your mind" does
//! (core/client/README.md "Status lines"): `gone` in an archived session; `done` once its agent said so; linked to a card,
//! the card's place decides ([`card_place`]): put off is `later`, open is as set, answered and still worked on is
//! `working` at most, else `done`; unlinked and not updated for [`LINE_STALE_MS`]: `stale`.
use crate::model::types::{Card, Link, Session, StatusLine};
use crate::model::Model;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// No tool call for this long, in a session that hears only on its next one: it is not listening.
pub const ASLEEP_MS: u64 = 10 * 60_000;
/// A link that drops and comes back within this time (a restart of the session's bridge) is no stop.
pub const OFFLINE_GRACE_MS: u64 = 60_000;
/// Nothing from a working, connected session for this long: the quiet hint.
pub const QUIET_MS: u64 = 15 * 60_000;
/// A status line its agent has not updated for this long, and whose card does not hold it, is stale: it no longer
/// says that its session works (the apps show it greyed and still for a day).
pub const LINE_STALE_MS: u64 = 2 * 60 * 60_000;
/// An answered card counts as worked on by its session for so long (while the session runs, or has not heard it).
pub const ACTING_MS: u64 = 6 * 60 * 60_000;

/// Where a card lies at the foot of the Desk, as far as its status line needs it: `later` (put off), `works` (with
/// its agent: handed back, an info it still works on, or answered within [`ACTING_MS`] while its session runs or has
/// not heard the answer), `done`, `trash` (shredded, or closed without his answer), or None (open on the Desk, or an
/// answered card without his answer). `runs`: its session is online or cut off (it still runs); `heard_up_to`: the
/// session's receipt mark. With the moment the place changes by the clock, if it will.
pub fn card_place(c: &Card, runs: bool, heard_up_to: Option<u64>, now: u64) -> (Option<&'static str>, Option<u64>) {
    let info = c.field("card_type").as_str() == Some("info") || c.is_unsupported();
    let a = c.answer.as_ref();
    let answered_by = a.is_some_and(|a| {
        if info {
            a.answer_action == "read"
        } else {
            a.trusted || a.choices.as_array().is_some_and(|l| !l.is_empty())
        }
    });
    if c.object_state == "open" {
        if c.snoozed_until.is_some() {
            return (Some("later"), None);
        }
        let working = info && c.field("working") == Value::Bool(true);
        return ((c.in_revision.is_some() || working).then_some("works"), None);
    }
    if c.closed_how.as_deref() == Some("shredded") {
        return (Some("trash"), None);
    }
    if c.closed_how.as_deref() == Some("answered") && c.object_state == "answered" {
        if !answered_by {
            return (None, None);
        }
        let a = a.expect("answered by him");
        let heard = match (a.pending, a.envelope_number, heard_up_to) {
            (false, Some(said), Some(mark)) => Some(said <= mark),
            _ => None,
        };
        let until = a.answered_at.saturating_add(ACTING_MS);
        return if now < until && (runs || heard == Some(false)) {
            (Some("works"), Some(until))
        } else {
            (Some("done"), None)
        };
    }
    (Some(if answered_by { "done" } else { "trash" }), None)
}

/// What a status line shows now (see the module's rules): `gone`, `done`, `later`, `stale` or the state its agent set
/// (`working`, `decision`, …). `card`: the card it names, if this device holds it. With the moment it changes by the
/// clock, if it will.
pub fn line_state(
    t: &StatusLine,
    archived: bool,
    card: Option<&Card>,
    runs: bool,
    heard_up_to: Option<u64>,
    now: u64,
) -> (String, Option<u64>) {
    let set = t.state.as_str().unwrap_or("working");
    if archived {
        return ("gone".into(), None);
    }
    if set == "done" {
        return ("done".into(), None);
    }
    if let Some(c) = card {
        let (at, until) = card_place(c, runs, heard_up_to, now);
        let shown = match at {
            Some("later") => "later",
            _ if c.object_state == "open" => set,
            Some("works") if set == "decision" => "working",
            Some("works") => set,
            _ => "done",
        };
        return (shown.into(), until);
    }
    if now.saturating_sub(t.updated_at) > LINE_STALE_MS {
        ("stale".into(), None)
    } else {
        (set.into(), Some(t.updated_at + LINE_STALE_MS + 1))
    }
}

/// A session's link at a moment.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct LinkView {
    /// `live`, `oncall`, `asleep`, `cut` or `gone`.
    pub state: String,
    /// When it was cut off or went; for `oncall` and `asleep` its last tool call (None: not known).
    pub since: Option<u64>,
    /// `gone` and `cut`: the connector's exit reason ("" when it left without a word), or `folder` / `detached`.
    pub reason: String,
}

/// Why a session is stopped.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Blocked {
    /// `cut`, `offline`, `permission` or `blocking`.
    pub why: String,
    pub since: Option<u64>,
}

/// The link of a subject (a member or a session) at `now`; the moment it turns asleep by the clock, if it will.
pub fn link_state(
    online: bool,
    offline_since: Option<u64>,
    link: Option<&Link>,
    codex: bool,
    now: u64,
) -> (LinkView, Option<u64>) {
    let view = |state: &str, since: Option<u64>, reason: &str| LinkView {
        state: state.into(),
        since,
        reason: reason.into(),
    };
    if !online {
        let alive = link
            .and_then(|l| l.exit.as_ref())
            .is_some_and(|e| e.claude == "alive");
        let reason = link
            .and_then(|l| l.exit.as_ref())
            .map(|e| e.reason.as_str())
            .unwrap_or("");
        return (view(if alive { "cut" } else { "gone" }, offline_since, reason), None);
    }
    let Some(l) = link else {
        return (view("live", None, ""), None);
    };
    if l.cut_since.is_some() {
        return (view("cut", l.cut_since, "folder"), None);
    }
    if !l.attached {
        return (view("cut", l.since, "detached"), None);
    }
    if l.hears != "oncall" {
        return (view("live", None, ""), None);
    }
    let last = l.last_call_at.or(l.since);
    match last {
        Some(t) if !codex && now.saturating_sub(t) >= ASLEEP_MS => (view("asleep", last, ""), None),
        Some(t) if !codex => (view("oncall", last, ""), Some(t + ASLEEP_MS)),
        _ => (view("oncall", last, ""), None),
    }
}

/// What the status of one session reads besides the session itself.
pub struct Context<'a> {
    pub model: &'a Model,
    /// The cards on the Desk now, by session.
    pub on_team: HashMap<&'a str, Vec<&'a str>>,
    /// The open permission requests on the Desk, by session.
    pub asking: BTreeSet<&'a str>,
    /// When the agent process behind each agent device last said anything, in any of its sessions.
    pub device_active: HashMap<&'a str, u64>,
    pub now: u64,
    /// When this app started (every session is away until it links again).
    pub started_at: u64,
}

impl<'a> Context<'a> {
    pub fn new(m: &'a Model, now: u64) -> Self {
        let mut on_team: HashMap<&str, Vec<&str>> = HashMap::new();
        for id in &m.stack {
            if let Some(c) = m.cards.get(id) {
                if let Some(s) = c.session_id.as_deref() {
                    on_team.entry(s).or_default().push(id.as_str());
                }
            }
        }
        let asking = m
            .open_permission_ids
            .iter()
            .filter_map(|id| m.permissions.get(id))
            // (one not read on this device yet waits quietly, as on the Desk: no hand points at what he cannot see)
            .filter(|p| p.content_state.is_empty() || p.content_state == "ok")
            .filter_map(|p| p.session_id.as_deref())
            .collect();
        let mut device_active: HashMap<&str, u64> = HashMap::new();
        for s in m.sessions.values() {
            if let Some(d) = s.agent_device_id.as_deref() {
                let e = device_active.entry(d).or_default();
                *e = (*e).max(s.last_activity_at);
            }
        }
        Context {
            model: m,
            on_team,
            asking,
            device_active,
            now,
            started_at: if m.started_at == 0 { now } else { m.started_at },
        }
    }
}

/// One session's status at `ctx.now`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Status {
    pub link: Option<LinkView>,
    pub blocked: Option<Blocked>,
    /// A status line of it says "working".
    pub working: bool,
    /// Quiet: its last sign of life, when that was [`QUIET_MS`] or more ago.
    pub quiet_since: Option<u64>,
    /// When the agent process behind it last said anything, in any of its sessions.
    pub device_active: u64,
    /// When any of this may change by the clock alone.
    pub recheck_at: Option<u64>,
    /// What each of its status lines shows now, by line id ([`line_state`]).
    pub lines: std::collections::BTreeMap<String, String>,
}

fn earliest(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// The status of a session as listed (`archived`, `removed`, `online`, `has_parent` as the session list decided).
pub fn status_of(
    ctx: &Context,
    s: &Session,
    archived: bool,
    removed: bool,
    online: bool,
    has_parent: bool,
) -> Status {
    let m = ctx.model;
    let now = ctx.now;
    let codex = s
        .agent_device_id
        .as_deref()
        .and_then(|d| m.members.get(d))
        .is_some_and(|d| d.platform.as_str() == Some("codex"));
    let mut recheck = None;
    let link = if archived || removed {
        None
    } else {
        let (l, at) = link_state(online, s.offline_since, s.link.as_ref(), codex, now);
        recheck = earliest(recheck, at);
        Some(l)
    };
    // (what each line shows: its card and its session decide; a session cut off still runs. The session works while
    //  one of its lines shows "working": the chip and the line always say the same, core/client/README.md "Status lines")
    let runs = online || link.as_ref().is_some_and(|l| l.state == "cut");
    let mut lines = std::collections::BTreeMap::new();
    let mut working_lines: Vec<u64> = vec![];
    for t in &s.status_lines {
        let card = t.object_id.as_str().and_then(|id| m.cards.get(id));
        let (shown, at) = line_state(t, archived, card, runs, s.heard_up_to, now);
        recheck = earliest(recheck, at);
        if shown == "working" {
            working_lines.push(t.updated_at);
        }
        lines.insert(t.id.clone(), shown);
    }
    let working = !working_lines.is_empty();
    let device_active = s
        .agent_device_id
        .as_deref()
        .and_then(|d| ctx.device_active.get(d).copied())
        .unwrap_or(0);
    let mut blocked = None;
    let mut quiet_since = None;
    if !archived {
        if let Some(l) = link.as_ref().filter(|l| l.state == "cut") {
            blocked = Some(Blocked {
                why: "cut".into(),
                since: l.since,
            });
        } else if !online {
            if working {
                let from = s
                    .offline_since
                    .unwrap_or(s.last_activity_at)
                    .max(ctx.started_at);
                if now.saturating_sub(from) >= OFFLINE_GRACE_MS {
                    blocked = Some(Blocked {
                        why: "offline".into(),
                        since: s.offline_since,
                    });
                } else {
                    recheck = earliest(recheck, Some(from + OFFLINE_GRACE_MS));
                }
            }
        } else {
            let sid = s.session_id.as_str();
            let waiting: Vec<&str> = ctx
                .on_team
                .get(sid)
                .into_iter()
                .flatten()
                .copied()
                .filter(|id| m.cards.get(*id).is_some_and(|c| c.in_revision.is_none()))
                .collect();
            if ctx.asking.contains(sid) {
                blocked = Some(Blocked {
                    why: "permission".into(),
                    since: None,
                });
            } else if waiting
                .iter()
                // (a card whose body is not read here yet stands on the Desk as a quiet "Loading …": no hand for it
                //  until it is read; the owner, 13 October 2026: "Still, ohne Hand und Knöpfe")
                .any(|id| m.cards.get(*id).is_some_and(|c| c.urgency == "critical" && !matches!(c.content_state.as_str(), "undecryptable" | "header")))
            {
                blocked = Some(Blocked {
                    why: "blocking".into(),
                    since: None,
                });
            }
            if working {
                let mut last = s.last_activity_at;
                if has_parent {
                    last = last.max(device_active);
                }
                last = working_lines.iter().copied().fold(last, u64::max);
                if now.saturating_sub(last) >= QUIET_MS {
                    quiet_since = Some(last);
                } else {
                    recheck = earliest(recheck, Some(last + QUIET_MS));
                }
            }
        }
    }
    Status {
        link,
        blocked,
        working,
        quiet_since,
        device_active,
        recheck_at: recheck,
        lines,
    }
}

#[cfg(test)]
mod tests {
    use crate::model::change::Change;
    use crate::model::types::{Card, HumanRegister, Member, StatusLine};
    use crate::model::Model;
    use crate::sessions::{session_list, SessionRow};
    use serde_json::json;

    const D0: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";
    const S0: &str = "50505050505050505050505050505050";
    const C0: &str = "c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0c0";

    fn room(online: bool) -> Model {
        let mut m = Model::new();
        m.my_device_id = Some("ee".repeat(16));
        m.members.insert(
            D0.into(),
            Member {
                device_id: D0.into(),
                device_role: "agent".into(),
                is_active: true,
                is_online: online,
                ..Default::default()
            },
        );
        let s = m.session_of(S0);
        s.group_id = Some("g".into());
        s.agent_device_id = Some(D0.into());
        s.agent_device_ids = vec![D0.into()];
        s.is_online = online;
        s.offline_since = (!online).then_some(1_000);
        m
    }
    fn card(m: &mut Model, urgency: &str, version: u64) {
        m.cards.insert(
            C0.into(),
            Card {
                object_id: C0.into(),
                agent_device_id: D0.into(),
                session_id: Some(S0.into()),
                object_state: "open".into(),
                urgency: urgency.into(),
                envelope_number: version,
                state_envelope_number: version,
                created_at: 10,
                content_state: "ok".into(),
                ..Default::default()
            },
        );
    }
    fn at(m: &mut Model, now: u64, cards: bool) -> SessionRow {
        let mut c = Change::new();
        if cards {
            c.cards.insert(C0.into());
        }
        m.project(&mut c, now);
        session_list(m, false).remove(0)
    }
    fn why(r: &SessionRow) -> Option<&str> {
        r.blocked.as_ref().map(|b| b.why.as_str())
    }

    /// The red hand stands exactly while the blocking question stands on the Desk: put off (Later), it goes; a newer
    /// version of the question brings both back.
    #[test]
    fn the_red_hand_follows_the_desk() {
        let mut m = room(true);
        card(&mut m, "critical", 5);
        let r = at(&mut m, 100_000, true);
        assert_eq!(m.stack, [C0]);
        assert_eq!(why(&r), Some("blocking"));
        m.human.insert(
            format!("snooze/{C0}"),
            HumanRegister {
                value: json!({ "until": 900_000, "at": 100_000 }),
                envelope_number: Some(7),
                ..Default::default()
            },
        );
        let mut c = Change::new();
        c.registers.insert(format!("snooze/{C0}"));
        m.project(&mut c, 100_000);
        assert!(m.stack.is_empty());
        assert_eq!(m.cards[C0].snoozed_until, Some(900_000));
        assert_eq!(why(&session_list(&m, false)[0]), None, "put off: no hand");
        card(&mut m, "critical", 9);
        let r = at(&mut m, 100_000, true);
        assert_eq!(m.stack, [C0], "a newer version wakes it");
        assert_eq!(m.cards[C0].snoozed_until, None);
        assert_eq!(m.cards[C0].unsnoozed, Some(900_000));
        assert_eq!(why(&r), Some("blocking"));
    }

    /// A question the hub's list says was settled elsewhere is off the Desk, and raises no hand.
    #[test]
    fn a_question_settled_elsewhere_raises_no_hand() {
        let mut m = room(true);
        card(&mut m, "critical", 5);
        m.set_desk_listing(Some((50, vec![])));
        let r = at(&mut m, 100_000, true);
        assert!(m.stack.is_empty());
        assert_eq!(why(&r), None);
    }

    /// Disconnected while working: the hand after the grace, and the list says when to look again.
    #[test]
    fn offline_while_working_after_the_grace() {
        let mut m = room(false);
        m.sessions.get_mut(S0).unwrap().status_lines.push(StatusLine {
            id: "t".into(),
            state: json!("working"),
            updated_at: 500,
            ..Default::default()
        });
        let mut c = Change::new();
        m.project(&mut c, 30_000);
        let r = session_list(&m, false).remove(0);
        assert_eq!(why(&r), None);
        assert_eq!(r.recheck_at, Some(90_000), "a minute after this run started");
        let mut c = Change::new();
        m.project(&mut c, 90_000);
        let r = session_list(&m, false).remove(0);
        assert_eq!(why(&r), Some("offline"));
        assert_eq!(r.link.unwrap().state, "gone");
    }

    /// A working line nobody updated for LINE_STALE_MS says nothing any more: no hand while disconnected; one linked to
    /// an open card holds as long as that card is open and not put off.
    #[test]
    fn a_stale_line_is_no_work() {
        let stale = super::LINE_STALE_MS + 1_000;
        let all = |m: &mut Model, now: u64| {
            let mut c = Change::new();
            c.cards.insert(C0.into());
            c.sessions.insert(S0.into());
            m.project(&mut c, now);
            session_list(m, false).remove(0)
        };
        let mut m = room(false);
        m.sessions.get_mut(S0).unwrap().status_lines.push(StatusLine {
            id: "t".into(),
            state: json!("working"),
            updated_at: 500,
            ..Default::default()
        });
        // (this run started long ago: past the grace, a working line raises the hand)
        all(&mut m, 1_000);
        assert_eq!(why(&all(&mut m, stale)), None);
        card(&mut m, "normal", 1);
        m.sessions.get_mut(S0).unwrap().status_lines[0].object_id = json!(C0);
        assert_eq!(why(&all(&mut m, stale)), Some("offline"));
        m.cards.get_mut(C0).unwrap().snoozed_until = Some(stale * 2);
        assert_eq!(why(&all(&mut m, stale)), None);
    }

    /// The chip says what the line says: a fresh working line whose card was shredded shows done, and its session
    /// does not work; one whose card was answered just now works while the session runs.
    #[test]
    fn the_chip_follows_the_line() {
        let mut m = room(true);
        card(&mut m, "normal", 1);
        m.sessions.get_mut(S0).unwrap().status_lines.push(StatusLine {
            id: "t".into(),
            state: json!("working"),
            object_id: json!(C0),
            updated_at: 1_000,
            ..Default::default()
        });
        let all = |m: &mut Model| {
            let mut c = Change::new();
            c.cards.insert(C0.into());
            c.sessions.insert(S0.into());
            m.project(&mut c, 2_000);
            session_list(m, false).remove(0)
        };
        let r = all(&mut m);
        assert!(r.working);
        assert_eq!(r.lines["t"], "working");
        {
            let c = m.cards.get_mut(C0).unwrap();
            c.object_state = "closed".into();
            c.closed_how = Some("shredded".into());
        }
        let r = all(&mut m);
        assert_eq!(r.lines["t"], "done");
        assert!(!r.working);
        {
            let c = m.cards.get_mut(C0).unwrap();
            c.object_state = "answered".into();
            c.closed_how = Some("answered".into());
            c.answer = Some(serde_json::from_value(json!({ "answer_action": "answer", "choices": ["a"], "answered_at": 1_500 })).unwrap());
        }
        let r = all(&mut m);
        assert_eq!(r.lines["t"], "working");
        assert!(r.working);
    }

    /// Hears on its next step, then asleep by the clock; a session in Codex is never asleep.
    #[test]
    fn on_call_then_asleep() {
        let mut m = room(true);
        let link = crate::model::types::Link {
            hears: "oncall".into(),
            attached: true,
            last_call_at: Some(1_000),
            ..Default::default()
        };
        m.sessions.get_mut(S0).unwrap().link = Some(link);
        let mut c = Change::new();
        m.project(&mut c, 2_000);
        let r = session_list(&m, false).remove(0);
        assert_eq!(r.link.as_ref().unwrap().state, "oncall");
        assert_eq!(r.recheck_at, Some(1_000 + super::ASLEEP_MS));
        m.project(&mut Change::new(), 1_000 + super::ASLEEP_MS);
        assert_eq!(session_list(&m, false)[0].link.as_ref().unwrap().state, "asleep");
        m.members.get_mut(D0).unwrap().platform = json!("codex");
        assert_eq!(session_list(&m, false)[0].link.as_ref().unwrap().state, "oncall");
    }

    /// The patch sends the list again when the clock passes a status change, with nothing else changed.
    #[test]
    fn the_clock_sends_the_list_again() {
        let mut m = room(true);
        let link = crate::model::types::Link {
            hears: "oncall".into(),
            attached: true,
            last_call_at: Some(1_000),
            ..Default::default()
        };
        m.sessions.get_mut(S0).unwrap().link = Some(link);
        let mut c = Change::new();
        c.sessions.insert(S0.into());
        m.project(&mut c, 2_000);
        let p = m.patch(&c);
        assert_eq!(p["wake_at"], json!(1_000 + super::ASLEEP_MS));
        let mut c = Change::new();
        m.project(&mut c, 1_500);
        assert!(c.is_empty());
        m.project(&mut c, 1_000 + super::ASLEEP_MS);
        assert!(c.status);
        let p = m.patch(&c);
        assert_eq!(p["session_list"][0]["link"]["state"], json!("asleep"));
    }

    mod lines {
        use super::super::{line_state, ACTING_MS, LINE_STALE_MS};
        use crate::model::types::{Answer, Card, InRevision, StatusLine};
        use serde_json::json;

        const NOW: u64 = 100 * 3_600_000;
        fn line(state: &str, updated: u64) -> StatusLine {
            StatusLine { id: "t".into(), state: json!(state), updated_at: updated, ..Default::default() }
        }
        fn open() -> Card {
            Card { object_id: "c".into(), object_state: "open".into(), ..Default::default() }
        }
        fn answered(at: u64) -> Card {
            Card {
                object_state: "answered".into(),
                closed_how: Some("answered".into()),
                answer: Some(Answer { answer_action: "answer".into(), choices: json!(["a"]), answered_at: at, envelope_number: Some(9), ..Default::default() }),
                ..open()
            }
        }
        fn shown(t: &StatusLine, archived: bool, c: Option<&Card>) -> String {
            line_state(t, archived, c, true, None, NOW).0
        }

        #[test]
        fn a_line_of_an_archived_session_is_gone() {
            assert_eq!(shown(&line("working", NOW - 60_000), true, None), "gone");
        }

        #[test]
        fn a_line_whose_card_is_put_off_is_later() {
            let put_off = Card { snoozed_until: Some(NOW + 3_600_000), ..open() };
            assert_eq!(shown(&line("decision", NOW - 60_000), false, Some(&put_off)), "later");
            assert_eq!(shown(&line("working", NOW - 60_000), false, Some(&put_off)), "later");
        }

        #[test]
        fn an_open_card_holds_its_line_as_set_however_old() {
            assert_eq!(shown(&line("decision", NOW - 50 * 3_600_000), false, Some(&open())), "decision");
            let handed = Card { in_revision: Some(InRevision { envelope_number: 3, ..Default::default() }), ..open() };
            assert_eq!(shown(&line("working", NOW - 50 * 3_600_000), false, Some(&handed)), "working");
        }

        #[test]
        fn answered_working_while_its_session_is_at_it_then_done() {
            let a = answered(NOW - 60_000);
            assert_eq!(shown(&line("decision", NOW - 60_000), false, Some(&a)), "working");
            assert_eq!(shown(&line("working", NOW - 60_000), false, Some(&a)), "working");
            // (and it says when that ends)
            assert_eq!(line_state(&line("working", NOW - 60_000), false, Some(&a), true, None, NOW).1, Some(NOW - 60_000 + ACTING_MS));
            assert_eq!(shown(&line("working", NOW - 60_000), false, Some(&answered(NOW - ACTING_MS - 1))), "done");
            let shredded = Card { object_state: "closed".into(), closed_how: Some("shredded".into()), ..open() };
            assert_eq!(shown(&line("working", NOW - 60_000), false, Some(&shredded)), "done");
            // a session that is gone: done, unless it has not heard the answer yet
            assert_eq!(line_state(&line("working", NOW - 60_000), false, Some(&a), false, Some(9), NOW).0, "done");
            assert_eq!(line_state(&line("working", NOW - 60_000), false, Some(&a), false, Some(8), NOW).0, "working");
        }

        #[test]
        fn an_unlinked_line_nobody_updated_is_stale_and_done_stays_done() {
            assert_eq!(shown(&line("working", NOW - LINE_STALE_MS + 1000), false, None), "working");
            assert_eq!(shown(&line("working", NOW - LINE_STALE_MS - 1000), false, None), "stale");
            assert_eq!(shown(&line("decision", NOW - LINE_STALE_MS - 1000), false, None), "stale");
            assert_eq!(shown(&line("done", NOW - LINE_STALE_MS - 1000), false, None), "done");
        }
    }
}
