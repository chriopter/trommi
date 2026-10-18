//! The page Knowledge of the team in view (`Screen::Knowledge`): its entries in the order everyone shows (pinned
//! first, then the one changed last), the search, the filter All / New / Pinned / Forgotten, what is new to the
//! person, the history of the entry he opened with the way back from each change, and every word the page says. The
//! chip on a team's Desk that leads there, and the words of the work line "reads team knowledge (12)" are here too.
//! The entries and their rules are `crate::knowledge`'s.

use super::board::Board;
use super::session::day_label;
use super::words::{next_midnight, say};
use super::{KnowledgeChipView, KnowledgeFilterView, KnowledgeKindView, KnowledgeLogView, KnowledgeRowView, KnowledgeView, KnowledgeWordsView, Text};
use crate::knowledge::{forgotten_of_team, is_new, logs_of, of_team, seen_of, Entry, Index, Log, ENTRIES_MAX, FORGOTTEN_DAYS, TEXT_MAX, TITLE_MAX};

/// What the page asks: the search's words, the filter ("" every entry, "new", "pinned", "forgotten"), the entry
/// open (its history shows), and when the person had last looked before this visit (what changed since is new).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ask {
    pub query: String,
    pub filter: String,
    pub open: String,
    /// The team the visit began on, and the moment he had looked before it (none: not read yet).
    pub team: String,
    pub seen_before: Option<u64>,
}

/// The team the page and the chip are about: the team in view, else the first team's id.
pub fn team(b: &Board) -> String {
    b.team.clone().unwrap_or_else(|| super::board::DEFAULT_TEAM.into())
}

fn count_words(n: usize) -> String {
    format!("{n} {}", if n == 1 { "entry" } else { "entries" })
}

/// A kind in words: "Rule", "Decision", "Fact", "Where".
pub fn kind_label(kind: &str) -> Text {
    let kind = crate::knowledge::clean_kind(kind);
    let en = match kind.as_str() {
        "rule" => "Rule",
        "decision" => "Decision",
        "where" => "Where",
        _ => "Fact",
    };
    say(&format!("knowledge.kind.{kind}"), en, &[])
}

fn who(by: &Option<String>) -> String {
    by.clone().unwrap_or_else(|| "you".into())
}

/// A row's line: who did the last thing to it, and when ("updated by Codex · Yesterday", "added by you · Today").
fn by_line(e: &Entry, when: &str) -> Text {
    let (key, verb, by) = if e.forgotten() {
        ("knowledge.by.forgot", "forgotten", &e.deleted_by)
    } else if e.updated_at > 0 {
        ("knowledge.by.updated", "updated", &e.updated_by)
    } else {
        ("knowledge.by.added", "added", &e.by)
    };
    let name = who(by);
    say(key, format!("{verb} by {name} · {when}"), &[("who", &name), ("when", when)])
}

/// One change of the history: "Codex updated it · Yesterday", the words it replaced, and Undo.
fn log_view(b: &Board, l: &Log) -> KnowledgeLogView {
    let when = day_label(l.at, b.now, b.lens.utc_offset_min).en;
    let name = who(&l.by);
    let verb = match l.op.as_str() {
        "add" => "added it",
        "update" => "changed it",
        "forget" => "forgot it",
        _ => "brought it back",
    };
    KnowledgeLogView {
        id: l.id.clone(),
        says: say(&format!("knowledge.log.{}", l.op), format!("{name} {verb} · {when}"), &[("who", &name), ("when", &when)]),
        title: l.title.clone().unwrap_or_default(),
        text: l.text.clone().unwrap_or_default(),
        before: if l.title.is_some() { say("knowledge.log.before", "Before:", &[]) } else { say("knowledge.log.new", "It was new.", &[]) },
        by_agent: l.by.is_some(),
        at_ms: l.at,
    }
}

/// The page.
pub fn page(b: &Board, ask: &Ask) -> KnowledgeView {
    let team = team(b);
    let name = b.teams.iter().find(|d| d.id == team).map(|d| d.name.clone()).unwrap_or_else(|| "Personal".into());
    let index = Index::new(of_team(b.model, &team));
    let mut forgotten = forgotten_of_team(b.model, &team);
    forgotten.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at).then(a.id.cmp(&b.id)));
    // (what changed since he last looked before this visit; while that is not read yet, since he last looked)
    let seen = ask.seen_before.unwrap_or_else(|| seen_of(b.model, &team));
    let total = index.len();
    let pinned_n = index.all().iter().filter(|e| e.pinned).count();
    let new_n = index.all().iter().filter(|e| is_new(e, seen)).count();
    let filter = match ask.filter.as_str() {
        f @ ("pinned" | "new" | "forgotten") => f,
        _ => "",
    };
    let query = ask.query.trim().to_string();
    let found: Vec<&Entry> = if filter == "forgotten" {
        forgotten.iter().collect()
    } else {
        index.search(&query, usize::MAX).into_iter().filter(|e| match filter {
            "pinned" => e.pinned,
            "new" => is_new(e, seen),
            _ => true,
        }).collect()
    };
    let zone = b.lens.utc_offset_min;
    if !found.is_empty() {
        b.wake_at(Some(next_midnight(b.now, zone)));
    }
    let logs = logs_of(b.model);
    let open = (!ask.open.is_empty()).then_some(ask.open.as_str());
    let rows: Vec<KnowledgeRowView> = found
        .iter()
        .map(|e| {
            let when = day_label(e.changed(), b.now, zone).en;
            let mine = logs.get(&e.id).map(Vec::as_slice).unwrap_or_default();
            // (the way back from the last change, when an agent made it: one tap)
            let undo = mine.first().filter(|l| l.by.is_some()).map(|l| l.id.clone());
            KnowledgeRowView {
                id: e.id.clone(),
                title: e.title.clone(),
                text: e.text.clone(),
                pinned: e.pinned,
                kind: crate::knowledge::clean_kind(&e.kind),
                kind_label: kind_label(&e.kind),
                confirmed: (e.confirmed() > e.changed()).then(|| {
                    let day = day_label(e.confirmed(), b.now, zone).en;
                    say("knowledge.confirmed", format!("confirmed {day}"), &[("when", &day)])
                }),
                by: by_line(e, &when),
                at_ms: e.changed(),
                new: is_new(e, seen),
                forgotten: e.forgotten(),
                undo,
                restore: e.forgotten().then(|| mine.iter().find(|l| l.op == "forget").map(|l| l.id.clone())).flatten(),
                gone_in: e.forgotten().then(|| {
                    let left = ((e.deleted_at + FORGOTTEN_DAYS * crate::knowledge::DAY_MS).saturating_sub(b.now) / crate::knowledge::DAY_MS).max(1);
                    say("knowledge.gone_in", format!("Goes for good in {left} {}", if left == 1 { "day" } else { "days" }), &[("n", &left.to_string())])
                }),
                open: open == Some(e.id.as_str()),
                history: if open == Some(e.id.as_str()) { mine.iter().map(|l| log_view(b, l)).collect() } else { Vec::new() },
            }
        })
        .collect();
    let none = rows.is_empty().then(|| {
        if filter == "forgotten" {
            say("knowledge.none_forgotten", "Nothing forgotten. A forgotten entry can be brought back here for 30 days.", &[])
        } else if total == 0 {
            say("knowledge.none", "Nothing yet. Agents of this team keep here what lasts: rules, decisions, preferences, corrections. Or add an entry yourself.", &[])
        } else if !query.is_empty() {
            say("knowledge.none_found", format!("Nothing found for “{query}”."), &[("query", &query)])
        } else if filter == "new" {
            say("knowledge.none_new", "Nothing new since you last looked.", &[])
        } else {
            say("knowledge.none_pinned", "Nothing pinned yet. A pinned entry is what every agent of this team reads first.", &[])
        }
    });
    let n = total.to_string();
    let filter_view = |key: &str, label: Text, count: usize| KnowledgeFilterView { key: key.into(), label, count: count as u32, on: filter == key };
    KnowledgeView {
        team_id: team,
        team: say("knowledge.team", name.clone(), &[("team", &name)]),
        title: say("knowledge.title", "Knowledge", &[]),
        count: total as u32,
        lead: say("knowledge.lead", "What every agent in this team knows. Agents keep it up to date; every change can be undone.", &[]),
        query,
        filter: filter.into(),
        filters: vec![
            filter_view("", say("knowledge.filter.all", "All", &[]), total),
            filter_view("new", say("knowledge.filter.new", "New", &[]), new_n),
            filter_view("pinned", say("knowledge.filter.pinned", "Pinned", &[]), pinned_n),
            filter_view("forgotten", say("knowledge.filter.forgotten", "Forgotten", &[]), forgotten.len()),
        ],
        search: say("knowledge.search", "Search knowledge…", &[]),
        summary: say("knowledge.count", count_words(total), &[("n", &n)]),
        rows,
        none,
        full: total >= ENTRIES_MAX,
        title_max: TITLE_MAX as u32,
        text_max: TEXT_MAX as u32,
        words: KnowledgeWordsView {
            add: say("knowledge.add", "Add entry", &[]),
            edit: say("knowledge.edit", "Edit", &[]),
            delete: say("knowledge.delete", "Forget", &[]),
            pin: say("knowledge.pin", "Pin", &[]),
            unpin: say("knowledge.unpin", "Unpin", &[]),
            save: say("knowledge.save", "Save", &[]),
            cancel: say("knowledge.cancel", "Cancel", &[]),
            title_hint: say("knowledge.title_hint", "Title", &[]),
            text_hint: say("knowledge.text_hint", "What every agent of this team should know", &[]),
            delete_ask: say("knowledge.delete_ask", "Forget this entry? No agent of this team reads it any more; you can bring it back for 30 days.", &[]),
            full: say("knowledge.full", format!("This team holds {ENTRIES_MAX} entries: forget one to add another."), &[("n", &ENTRIES_MAX.to_string())]),
            new_mark: say("knowledge.new", "New", &[]),
            undo: say("knowledge.undo", "Undo", &[]),
            restore: say("knowledge.restore", "Bring back", &[]),
            history: say("knowledge.history", "History", &[]),
        },
        kinds: crate::knowledge::KINDS.iter().map(|k| KnowledgeKindView { key: (*k).to_string(), label: kind_label(k) }).collect(),
    }
}

/// The chip on a team's Desk that opens its page: "Knowledge" and how many entries.
pub fn chip(b: &Board) -> KnowledgeChipView {
    let team = team(b);
    let entries = of_team(b.model, &team);
    let n = entries.len();
    let seen = seen_of(b.model, &team);
    let fresh = entries.iter().filter(|e| is_new(e, seen)).count() as u32;
    let first = Index::new(entries).search("", 4).into_iter().map(|e| e.title.clone()).collect();
    KnowledgeChipView {
        first,
        fresh,
        count: n as u32,
        label: say("knowledge.chip", "Knowledge", &[]),
        says: say("knowledge.chip.says", format!("Team knowledge: {}", count_words(n)), &[("n", &n.to_string())]),
        team_id: team,
    }
}

/// The work line of an agent's step that read its team's knowledge: "reads team knowledge (12)", with what it
/// searched for. `held`: the entries its session holds.
pub fn work_words(held: usize, query: &str) -> Text {
    let n = held.to_string();
    if query.trim().is_empty() {
        say("work.knowledge", format!("reads team knowledge ({n})"), &[("n", &n)])
    } else {
        let q = query.trim();
        say("work.knowledge.query", format!("reads team knowledge ({n}) · “{q}”"), &[("n", &n), ("query", q)])
    }
}
