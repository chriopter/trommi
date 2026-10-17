//! The page Knowledge of the team in view (`Screen::Knowledge`): its entries in the order everyone shows (pinned
//! first, then the one changed last), the search, the filter All / Pinned, and every word the page says. The chip on
//! a team's Desk that leads there, and the words of the work line "reads team knowledge (12)" are here too. The
//! entries and their rules are `crate::knowledge`'s.

use super::board::Board;
use super::session::day_label;
use super::words::{next_midnight, say};
use super::{KnowledgeChipView, KnowledgeFilterView, KnowledgeRowView, KnowledgeView, KnowledgeWordsView, Text};
use crate::knowledge::{of_team, Index, ENTRIES_MAX, TEXT_MAX, TITLE_MAX};

/// What the page asks: the search's words and the filter ("" every entry, "pinned" the pinned ones).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ask {
    pub query: String,
    pub filter: String,
}

/// The team the page and the chip are about: the team in view, else the first team's id.
fn team(b: &Board) -> String {
    b.team.clone().unwrap_or_else(|| super::board::DEFAULT_TEAM.into())
}

fn count_words(n: usize) -> String {
    format!("{n} {}", if n == 1 { "entry" } else { "entries" })
}

/// The page.
pub fn page(b: &Board, ask: &Ask) -> KnowledgeView {
    let team = team(b);
    let name = b.teams.iter().find(|d| d.id == team).map(|d| d.name.clone()).unwrap_or_else(|| "Personal".into());
    let index = Index::new(of_team(b.model, &team));
    let total = index.len();
    let pinned_n = index.all().iter().filter(|e| e.pinned).count();
    let filter = if ask.filter == "pinned" { "pinned" } else { "" };
    let query = ask.query.trim().to_string();
    let found: Vec<_> = index.search(&query, usize::MAX).into_iter().filter(|e| filter.is_empty() || e.pinned).collect();
    let zone = b.lens.utc_offset_min;
    if !found.is_empty() {
        b.wake_at(Some(next_midnight(b.now, zone)));
    }
    let rows: Vec<KnowledgeRowView> = found
        .iter()
        .map(|e| {
            let when = day_label(e.changed(), b.now, zone).en;
            let by = match &e.by {
                Some(agent) => say("knowledge.by_agent", format!("proposed by {agent} · {when}"), &[("agent", agent), ("when", &when)]),
                None => say("knowledge.by_you", format!("added by you · {when}"), &[("when", &when)]),
            };
            KnowledgeRowView { id: e.id.clone(), title: e.title.clone(), text: e.text.clone(), pinned: e.pinned, by, at_ms: e.changed() }
        })
        .collect();
    let none = rows.is_empty().then(|| {
        if total == 0 {
            say("knowledge.none", "Nothing yet. Agents of this team propose what is worth keeping; it is kept on your Yes. Or add an entry yourself.", &[])
        } else if !query.is_empty() {
            say("knowledge.none_found", format!("Nothing found for “{query}”."), &[("query", &query)])
        } else {
            say("knowledge.none_pinned", "Nothing pinned yet. A pinned entry is what every agent of this team reads first.", &[])
        }
    });
    let n = total.to_string();
    KnowledgeView {
        team_id: team,
        team: say("knowledge.team", name.clone(), &[("team", &name)]),
        title: say("knowledge.title", "Knowledge", &[]),
        count: total as u32,
        lead: say("knowledge.lead", "What every agent in this team knows. New entries need your Yes.", &[]),
        query,
        filter: filter.into(),
        filters: vec![
            KnowledgeFilterView { key: String::new(), label: say("knowledge.filter.all", "All", &[]), count: total as u32, on: filter.is_empty() },
            KnowledgeFilterView { key: "pinned".into(), label: say("knowledge.filter.pinned", "Pinned", &[]), count: pinned_n as u32, on: filter == "pinned" },
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
            delete: say("knowledge.delete", "Delete", &[]),
            pin: say("knowledge.pin", "Pin", &[]),
            unpin: say("knowledge.unpin", "Unpin", &[]),
            save: say("knowledge.save", "Save", &[]),
            cancel: say("knowledge.cancel", "Cancel", &[]),
            title_hint: say("knowledge.title_hint", "Title", &[]),
            text_hint: say("knowledge.text_hint", "What every agent of this team should know", &[]),
            delete_ask: say("knowledge.delete_ask", "Delete this entry? No agent of this team reads it any more.", &[]),
            full: say("knowledge.full", format!("This team holds {ENTRIES_MAX} entries: delete one to add another."), &[("n", &ENTRIES_MAX.to_string())]),
        },
    }
}

/// The chip on a team's Desk that opens its page: "Knowledge" and how many entries.
pub fn chip(b: &Board) -> KnowledgeChipView {
    let team = team(b);
    let entries = of_team(b.model, &team);
    let n = entries.len();
    let first = Index::new(entries).search("", 4).into_iter().map(|e| e.title.clone()).collect();
    KnowledgeChipView {
        first,
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
