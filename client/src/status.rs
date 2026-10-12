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
//!   blocking (urgency critical), that stands on the Desk now (the stack: not held, not put off, not with its agent).
//!   What is not on the Desk raises no hand, so the hand and the Desk never disagree.
//!
//! Quiet (a grey hint, never a stop): connected, a status line says "working", and nothing from it for [`QUIET_MS`].
use crate::model::types::{Link, Session};
use crate::model::Model;
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};

/// No tool call for this long, in a session that hears only on its next one: it is not listening.
pub const ASLEEP_MS: u64 = 10 * 60_000;
/// A link that drops and comes back within this time (a restart of the session's bridge) is no stop.
pub const OFFLINE_GRACE_MS: u64 = 60_000;
/// Nothing from a working, connected session for this long: the quiet hint.
pub const QUIET_MS: u64 = 15 * 60_000;

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
    pub on_desk: HashMap<&'a str, Vec<&'a str>>,
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
        let mut on_desk: HashMap<&str, Vec<&str>> = HashMap::new();
        for id in &m.stack {
            if let Some(c) = m.cards.get(id) {
                if let Some(s) = c.session_id.as_deref() {
                    on_desk.entry(s).or_default().push(id.as_str());
                }
            }
        }
        let asking = m
            .open_permission_ids
            .iter()
            .filter_map(|id| m.permissions.get(id))
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
            on_desk,
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
    let working_lines: Vec<u64> = s
        .status_lines
        .iter()
        .filter(|t| t.state.as_str() == Some("working"))
        .map(|t| t.updated_at)
        .collect();
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
                .on_desk
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
                .any(|id| m.cards.get(*id).is_some_and(|c| c.urgency == "critical"))
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
}
