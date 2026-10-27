//! The page Wakeups of the team in view (`Screen::Wakeups`): every wakeup its sessions' agents set themselves, the
//! next first, with the note, how often, who set it, its last run, and what the person asked of it (cancel, another
//! time) while that is on its way. And the chat's quiet line "09:00 · …". The rules are `crate::wakeups`'s.

use super::board::Board;
use super::session::day_label;
use super::words::{clock, local, say};
use super::{WakeLineView, WakeupRowView, WakeupWordsView, WakeupsView};
use crate::wakeups::{every_words, of, Edit, Wakeup, EDIT_PREFIX};

/// "Today 09:00", "Tomorrow 07:00", "Thursday 22 October 09:00".
fn when(b: &Board, at: u64) -> String {
    let zone = b.lens.utc_offset_min;
    let day = day_label(at, b.now, zone).en;
    let time = clock(at, at, zone);
    format!("{day} {time}")
}

/// A session's wakeups as the person's device holds them, with his word on each (none: nothing asked).
pub fn of_session(b: &Board, key: &str) -> Vec<(Wakeup, Option<Edit>)> {
    let Some(s) = b.model.sessions.get(key) else {
        return Vec::new();
    };
    of(s.registers.iter().map(|(k, r)| (k, &r.value)))
        .into_iter()
        .map(|w| {
            // (his word until the connector applied it)
            let edit = s
                .registers
                .get(&format!("{EDIT_PREFIX}{}", w.id))
                .filter(|r| {
                    !crate::wakeups::edit_applied(Some(&w), &crate::wakeups::mark(&r.value))
                })
                .and_then(|r| Edit::read(&r.value));
            (w, edit)
        })
        .collect()
}

fn row(
    b: &Board,
    session_key: &str,
    name: &str,
    mark: &str,
    w: &Wakeup,
    edit: Option<&Edit>,
) -> WakeupRowView {
    let next = when(b, w.at);
    let last = w.last.as_ref().map(|l| {
        let at = when(b, l.at);
        match l.result.as_str() {
            "waited" => say(
                "wakeups.last.waited",
                format!("Waited: {} ({at})", l.detail),
                &[("why", &l.detail), ("when", &at)],
            ),
            "late" => say(
                "wakeups.last.late",
                format!("Ran late {at}: the computer was off"),
                &[("when", &at)],
            ),
            "failed" => say(
                "wakeups.last.failed",
                format!("Did not start {at}: {}", l.detail),
                &[("why", &l.detail), ("when", &at)],
            ),
            _ => say("wakeups.last.ran", format!("Ran {at}"), &[("when", &at)]),
        }
    });
    let pending = edit.map(|e| match e {
        Edit::Cancel => say(
            "wakeups.pending.cancel",
            "Cancelling… (when its computer is on)",
            &[],
        ),
        Edit::At(at) => {
            let to = when(b, *at);
            say(
                "wakeups.pending.move",
                format!("Moving to {to}… (when its computer is on)"),
                &[("when", &to)],
            )
        }
    });
    WakeupRowView {
        id: w.id.clone(),
        session_id: session_key.to_string(),
        who: say("wakeups.who", name.to_string(), &[("name", name)]),
        mark: mark.to_string(),
        helper: w.helper.clone(),
        note: w.note.clone(),
        next: say("wakeups.next", next.clone(), &[("when", &next)]),
        at_ms: w.at,
        how_often: say(
            &format!(
                "wakeups.every.{}",
                if w.every == 0 { "once" } else { "again" }
            ),
            every_words(w.every),
            &[],
        ),
        recurring: w.every > 0,
        last,
        pending,
    }
}

/// How many wakeups the agents of the team in view set (as many as its page lists): the group's ⋯ menu says it.
pub fn count(b: &Board) -> usize {
    let team = super::knowledge::team(b);
    b.agents
        .iter()
        .filter(|a| a.team.as_deref().is_none_or(|t| t == team) && !a.row.archived)
        .map(|a| of_session(b, &a.row.key).len())
        .sum()
}

/// The page.
pub fn page(b: &Board) -> WakeupsView {
    let team = super::knowledge::team(b);
    let name = b
        .teams
        .iter()
        .find(|d| d.id == team)
        .map(|d| d.name.clone())
        .unwrap_or_else(|| "Personal".into());
    let mut rows: Vec<WakeupRowView> = Vec::new();
    for a in b
        .agents
        .iter()
        .filter(|a| a.team.as_deref().is_none_or(|t| t == team) && !a.row.archived)
    {
        for (w, edit) in of_session(b, &a.row.key) {
            rows.push(row(
                b,
                &a.row.key,
                &a.row.name,
                &a.row.mark(),
                &w,
                edit.as_ref(),
            ));
        }
    }
    rows.sort_by(|x, y| x.at_ms.cmp(&y.at_ms).then(x.id.cmp(&y.id)));
    if let Some(first) = rows.first() {
        b.wake_at(Some(first.at_ms));
    }
    b.wake_at(Some(super::words::next_midnight(
        b.now,
        b.lens.utc_offset_min,
    )));
    let n = rows.len().to_string();
    WakeupsView {
        team_id: team,
        team: say("wakeups.team", name.clone(), &[("team", &name)]),
        title: say("wakeups.title", "Wakeups", &[]),
        count: rows.len() as u32,
        lead: say(
            "wakeups.lead",
            "When the agents of this team start again on their own, and what they will do then.",
            &[],
        ),
        none: rows.is_empty().then(|| {
            say(
                "wakeups.none",
                "No wakeups. An agent sets one with wake_me when something should happen later.",
                &[],
            )
        }),
        summary: say(
            "wakeups.count",
            format!("{n} {}", if rows.len() == 1 { "wakeup" } else { "wakeups" }),
            &[("n", &n)],
        ),
        rows,
        words: WakeupWordsView {
            cancel: say("wakeups.cancel", "Cancel", &[]),
            move_to: say("wakeups.move", "Other time", &[]),
            save: say("wakeups.save", "Save", &[]),
            back: say("wakeups.back", "Back", &[]),
            cancel_ask: say(
                "wakeups.cancel_ask",
                "Cancel this wakeup? Its agent will not start for it.",
                &[],
            ),
        },
    }
}

/// The chat's quiet line of a session: its next wakeup, when short ("09:00", "Thu 09:00") and its note beside it,
/// "and 2 more" behind; none without one.
pub fn line(b: &Board, key: &str) -> Option<WakeLineView> {
    let list = of_session(b, key);
    let (w, _) = list
        .iter()
        .find(|(_, e)| !matches!(e, Some(Edit::Cancel)))?;
    let zone = b.lens.utc_offset_min;
    let today = day_label(w.at, b.now, zone).key == "day.today";
    let at = if today {
        clock(w.at, w.at, zone)
    } else {
        when(b, w.at)
    };
    // (short for its column: the time today, the weekday within the week, else the date)
    let days = local(w.at, zone).days - local(b.now, zone).days;
    let short = if today {
        at.clone()
    } else if (1..7).contains(&days) {
        super::off::weekday_time(w.at, zone)
    } else {
        clock(w.at, b.now, zone)
    };
    b.wake_at(Some(w.at));
    let more = list.len() - 1;
    let n = more.to_string();
    let tail = if more > 0 {
        format!(", and {more} more")
    } else {
        String::new()
    };
    Some(WakeLineView {
        at: say("wakeups.line.at", short.clone(), &[("when", &short)]),
        note: w.note.clone(),
        more: (more > 0).then(|| {
            say(
                "wakeups.line.more",
                format!("and {more} more"),
                &[("n", &n)],
            )
        }),
        says: say(
            "wakeups.line",
            format!("Wakes at {at} · {}{tail}", w.note),
            &[("when", &at), ("note", &w.note), ("more", &n)],
        ),
    })
}
