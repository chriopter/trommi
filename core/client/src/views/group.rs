//! The group page: the page of one group (a team) with its head (the name, the ⋯ menu) and its open cards in one
//! block per chat, each block's head the way into its chat. There is no page of every group: Blitz walks every
//! group's open cards and is the only view of everything. A shell draws the blocks one after the other and decides
//! nothing.
//!
//! The rules (the owner, 27 and 28 October 2026: the Desk taken away, then "Nach Chat gruppiert"):
//! - the rows are the walk: the stack's cards that are not with their agent, and the open approval requests, of the
//!   group in view, in the core's order (`Model::desk_order`); what the order does not name yet stands last;
//! - the rows stand in one block per chat, the blocks in the sidebar's order of the chats (a block keeps its place while
//!   the person answers; Blitz walks by urgency); in a block its questions (decisions, requests) first, then what is
//!   only to read, each in the walk's order;
//! - the page names no chat that has nothing open (the sidebar lists them) and filters nothing: a block's head opens
//!   its chat;
//! - the chats on no team have no group page: their cards stand in their chat and in Blitz;
//! - nothing open: a calm word, who is at work, where else a question waits, and what the person answered here lately;
//! - a card not read on this device yet and a card of a newer Trommi stand as something to read, with nothing to
//!   answer;
//! - a row offers its answers itself when they are two (a yes and a no as thumbs, two named ways as tiles), or
//!   when its agent advises exactly one way; else one tile opens the card.

use super::board::{is_loading, key_of, reads_as_info, Board, DEFAULT_TEAM};
use super::links::plain;
use super::sidebar::units_of;
use super::words::{age_short, say};
use super::{
    AnsweredView, ChatBlockView, DeskRowView, ElsewhereView, FromView, GlobalBlitzView, GoalsView,
    GroupEmptyView, GroupHeadView, GroupMenuItem, GroupView, MediaView, Screen, SessionRowView,
    SlipView, Text, TileView, TilesView,
};
use crate::model::types::{Card, PermissionRequest};
use serde_json::Value;

pub const NEEDS_NEWER: &str = "This needs a newer version of Trommi. Update Trommi to see it.";

/// So many answered questions stand under "You answered lately" on a page with nothing open.
const LATELY: usize = 3;

/// The walk of the sessions `on` takes: what waits there (the stack's cards that are not with their agent, and the
/// open requests), in the core's order; a card it does not name yet, between two projections, stands last until it
/// does. The group page's rows, a chat's Blitz and Blitz over every group (`walk_all`) walk this.
pub(super) fn walk_of<'b>(b: &'b Board, on: impl Fn(Option<usize>) -> bool) -> Vec<&'b str> {
    let mut waiting: Vec<&str> = b
        .all_cards
        .iter()
        .filter(|f| on(f.agent))
        .map(|f| f.id.as_str())
        .collect();
    let mut walk: Vec<&str> = Vec::with_capacity(waiting.len());
    for id in &b.model.desk_order {
        if let Some(at) = waiting.iter().position(|w| w == id) {
            walk.push(waiting.remove(at));
        }
    }
    walk.append(&mut waiting);
    walk
}

/// Blitz over every group: every group's open cards in their order (a card's page walks on in it, `Focus.walk_all`).
pub fn walk_all(b: &Board) -> Vec<String> {
    walk_of(b, |_| true).into_iter().map(String::from).collect()
}

/// What Blitz walks: "Blitz: 3 open cards".
pub(super) fn blitz_words(n: u32) -> Text {
    if n == 1 {
        say("blitz.one", "Blitz: 1 open card", &[("n", "1")])
    } else {
        let n = n.to_string();
        say("blitz", format!("Blitz: {n} open cards"), &[("n", &n)])
    }
}

/// Blitz over every group, whatever group is in view: its first card is the first of the walk that this device
/// holds; none while nothing is open anywhere.
pub(super) fn blitz(b: &Board) -> Option<GlobalBlitzView> {
    let all = walk_of(b, |_| true);
    let held = |id: &str| b.model.cards.contains_key(id) || b.model.permissions.contains_key(id);
    all.iter().find(|id| held(id)).map(|id| GlobalBlitzView {
        card: id.to_string(),
        says: blitz_words(all.len() as u32),
        scope: say("blitz.scope", "all groups", &[]),
    })
}

/// The group page of the group in view.
pub fn group(b: &Board) -> GroupView {
    let m = b.model;
    let units = units_of(b);
    let agent_of =
        |session: &Option<String>, device: &str| b.by_key.get(key_of(session, device)).copied();
    let chats = super::sidebar::rows_of(b, &units, &super::sidebar::asked_last(b));

    // ---- the rows ----
    let walk = walk_of(b, |agent| b.on_page(agent));
    let rows: Vec<DeskRowView> = walk
        .iter()
        .filter_map(|id| match (m.cards.get(*id), m.permissions.get(*id)) {
            (Some(c), _) => Some(card_row(b, c, agent_of(&c.session_id, &c.agent_device_id))),
            (None, Some(p)) => Some(request_row(
                b,
                p,
                agent_of(&p.session_id, &p.agent_device_id),
            )),
            _ => None,
        })
        // (a row whose session the list does not show has no place on a page)
        .filter(|r| r.from.is_some())
        // (how long ago, beside who asked; the page is made again when it reads otherwise)
        .map(|mut r| {
            let (age, next) = age_short(r.created_ms, b.now);
            b.wake_at(Some(next));
            r.age = Some(age);
            r
        })
        .collect();
    let (asks, reads) = (
        rows.iter().filter(|r| r.kind != "info").count(),
        rows.len() - rows.iter().filter(|r| r.kind != "info").count(),
    );
    let blocks = blocks_of(rows, &chats);

    // ---- the head ----
    let waiting = b.all_fresh.iter().filter(|f| b.on_page(f.agent)).count() as u32;
    let cut: Vec<SlipView> = units
        .iter()
        .filter_map(|u| u.link.as_ref().filter(|l| l.state == "cut").map(|l| (u, l)))
        .map(|(u, l)| {
            let a = &b.agents[u.agent];
            let at = l
                .word
                .as_ref()
                .and_then(|w| w.args.get("at"))
                .cloned()
                .unwrap_or_default();
            let since = if at.is_empty() {
                String::new()
            } else {
                format!(" since {at}")
            };
            SlipView {
                session_id: a.row.key.clone(),
                text: say(
                    "group.slip.cut",
                    format!(
                        "{} is cut off{since}: it cannot hear you and cannot write to you.",
                        a.row.name
                    ),
                    &[("name", &a.row.name), ("at", &at)],
                ),
                fix: l.fix.clone().unwrap_or_default(),
            }
        })
        .collect();
    let reading = b.lens.reading;
    let progress = b
        .lens
        .reading_progress
        .filter(|(_, total)| *total > 0)
        .map(|(done, total)| {
            let (d, t) = (grouped(done), grouped(total));
            say(
                "group.reading",
                format!("Reading your history: {d} of {t}"),
                &[("done", &d), ("total", &t)],
            )
        });
    // (no chat stands here yet: invite the first agent; the head stays while there is a group to name)
    let mode = match (units.is_empty(), reading) {
        (true, true) => "reading",
        (true, false) => "invite",
        _ => "group",
    };
    let team = b.team.clone().unwrap_or_else(|| DEFAULT_TEAM.into());
    let head = (!units.is_empty() || !b.teams.is_empty()).then(|| head(b, &team));
    let goals = (!units.is_empty()).then(|| goals_of(b, &team));
    // Nothing waits here (what is with its agent waits for the agent, not for him): the list's place says so.
    let empty = blocks.is_empty().then(|| {
        if reading {
            GroupEmptyView {
                title: say("group.empty.reading", "Catching up: reading your sessions on this device. Their open questions show here as they are read.", &[]),
                ..Default::default()
            }
        } else {
            let name = head
                .as_ref()
                .map(|h| h.name.en.clone())
                .unwrap_or_else(|| "Personal".into());
            empty_of(b, &team, &name, &chats)
        }
    });
    let summary = summary_of(asks, reads);
    let mut view = GroupView {
        stack: blocks
            .iter()
            .flat_map(|k| k.questions.iter().chain(k.reads.iter()))
            .map(|r| r.id.clone())
            .collect(),
        empty,
        mode: mode.into(),
        reading: progress.filter(|_| reading),
        head,
        goals,
        summary,
        blocks,
        waiting,
        blitz: blitz(b),
        slip: cut,
        duck: None,
    };
    view.duck = duck_all(view.rows().into_iter());
    view
}

/// The rows in one block per chat, in each its questions, then what is only to read, each part in the walk's order.
/// The blocks stand in the list's order (`chats`: the group's rows, their names and marks too), so a block keeps its
/// place while the person answers; what knocks says so on its row (and the page's knock strips point to it).
fn blocks_of(rows: Vec<DeskRowView>, chats: &[SessionRowView]) -> Vec<ChatBlockView> {
    let mut blocks: Vec<ChatBlockView> = Vec::new();
    for r in rows {
        let Some(from) = r.from.clone() else { continue };
        let at = match blocks.iter().position(|k| k.session_id == from.session_id) {
            Some(at) => at,
            None => {
                let chat = chats.iter().find(|c| c.id == from.session_id);
                blocks.push(ChatBlockView {
                    session_id: from.session_id.clone(),
                    slug: chat.map(|c| c.slug.clone()).unwrap_or_default(),
                    name: chat.map(|c| c.name.clone()).unwrap_or_else(|| {
                        say("agent.name", from.name.clone(), &[("name", &from.name)])
                    }),
                    mark: chat
                        .map(|c| c.mark.clone())
                        .unwrap_or_else(|| from.mark.clone()),
                    working: chat
                        .filter(|c| c.working)
                        .map(|_| say("group.block.working", "working", &[])),
                    open_chat: say("group.block.open_chat", "Open chat ›", &[]),
                    ..Default::default()
                });
                blocks.len() - 1
            }
        };
        let k = &mut blocks[at];
        if r.kind == "info" {
            k.reads.push(r);
        } else {
            k.questions.push(r);
        }
    }
    // (the chats in the list's order: answering a card never moves a block, a block only leaves when it is empty)
    let place = |k: &ChatBlockView| {
        chats
            .iter()
            .position(|c| c.id == k.session_id)
            .unwrap_or(usize::MAX)
    };
    blocks.sort_by_key(place);
    for k in &mut blocks {
        let (name, n) = (k.name.en.clone(), k.questions.len());
        let ns = n.to_string();
        k.open = (n > 0).then(|| say("group.block.open", format!("{ns} open"), &[("n", &ns)]));
        k.says = if n > 0 {
            say(
                "group.block.says",
                format!("{name}: {ns} open. Open the chat"),
                &[("name", &name), ("n", &ns)],
            )
        } else {
            say(
                "group.block.says.read",
                format!("{name}: open the chat"),
                &[("name", &name)],
            )
        };
    }
    blocks
}

/// The quiet line over the blocks: "4 wait for you · 3 to read"; none while nothing is open.
fn summary_of(asks: usize, reads: usize) -> Option<Text> {
    let (a, r) = (asks.to_string(), reads.to_string());
    let args: &[(&str, &str)] = &[("n", &a), ("reads", &r)];
    match (asks, reads) {
        (0, 0) => None,
        (0, _) => Some(say("group.summary.reads", format!("{r} to read"), args)),
        (1, 0) => Some(say("group.summary.one", "1 waits for you", args)),
        (_, 0) => Some(say("group.summary.asks", format!("{a} wait for you"), args)),
        (1, _) => Some(say(
            "group.summary.one_reads",
            format!("1 waits for you · {r} to read"),
            args,
        )),
        _ => Some(say(
            "group.summary",
            format!("{a} wait for you · {r} to read"),
            args,
        )),
    }
}

/// Names in a line: "a", "a and b", "a, b and c", "a, b and 2 more".
fn names_line(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, c] => format!("{a}, {b} and {c}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    }
}

/// The page with nothing open: the calm word, who is at work, where else a question waits, what he answered lately.
fn empty_of(b: &Board, team: &str, name: &str, chats: &[SessionRowView]) -> GroupEmptyView {
    let working: Vec<String> = chats
        .iter()
        .filter(|c| c.working)
        .map(|c| c.name.en.clone())
        .collect();
    let line = if chats.is_empty() {
        None
    } else if working.is_empty() {
        Some(say(
            "group.empty.line",
            "What your chats ask lands here.",
            &[],
        ))
    } else {
        let who = names_line(&working);
        let are = if working.len() == 1 { "is" } else { "are" };
        Some(say(
            if working.len() == 1 {
                "group.empty.working.one"
            } else {
                "group.empty.working"
            },
            format!("{who} {are} at work. What they ask lands here."),
            &[("names", &who)],
        ))
    };
    // (another team where questions wait, the first in the menu's order; the chats on no team are no group)
    let elsewhere = b.teams.iter().filter(|t| t.id != team).find_map(|t| {
        let on = |agent: Option<usize>| {
            agent.is_some_and(|i| {
                let a = &b.agents[i];
                a.team.as_deref() == Some(t.id.as_str()) && !a.row.archived
            })
        };
        let n = b.all_fresh.iter().filter(|f| on(f.agent)).count();
        (n > 0).then(|| {
            let tn = super::sidebar::team_name(&t.name).en;
            let ns = n.to_string();
            let mark = b
                .agents
                .iter()
                .find(|a| a.team.as_deref() == Some(t.id.as_str()) && !a.row.archived)
                .map(|a| a.row.mark())
                .unwrap_or_default();
            ElsewhereView {
                team_id: t.id.clone(),
                mark,
                says: if n == 1 {
                    say(
                        "group.empty.elsewhere.one",
                        format!("1 question waits in {tn}"),
                        &[("n", &ns), ("name", &tn)],
                    )
                } else {
                    say(
                        "group.empty.elsewhere",
                        format!("{ns} questions wait in {tn}"),
                        &[("n", &ns), ("name", &tn)],
                    )
                },
            }
        })
    });
    let lately = lately_of(b, chats);
    GroupEmptyView {
        title: say(
            "group.empty",
            format!("Nothing waits for you in {name}."),
            &[("name", name)],
        ),
        line,
        elsewhere,
        lately_says: (!lately.is_empty())
            .then(|| say("group.empty.lately", "You answered lately", &[])),
        lately,
    }
}

/// The questions of the group's chats the person answered last, the newest first: what he chose, how long ago.
fn lately_of(b: &Board, chats: &[SessionRowView]) -> Vec<AnsweredView> {
    let mut answered: Vec<(&Card, &crate::model::types::Answer, &SessionRowView)> = b
        .model
        .cards
        .values()
        .filter(|c| c.object_state != "open" && c.field("card_type").as_str() != Some("info"))
        .filter_map(|c| {
            let a = c
                .answer
                .as_ref()
                .filter(|a| a.taken_back_at.is_none() && a.answered_at > 0)?;
            let agent = b.by_key.get(key_of(&c.session_id, &c.agent_device_id))?;
            let key = &b.agents[*agent].row.key;
            let chat = chats.iter().find(|r| &r.id == key)?;
            Some((c, a, chat))
        })
        .collect();
    answered.sort_by(|x, y| {
        y.1.answered_at
            .cmp(&x.1.answered_at)
            .then(x.0.object_id.cmp(&y.0.object_id))
    });
    answered
        .into_iter()
        .take(LATELY)
        .map(|(c, a, chat)| {
            let options = c.field("options");
            let labels: Vec<String> = super::talk::choices_of(&a.choices)
                .into_iter()
                .map(|k| {
                    options
                        .as_array()
                        .and_then(|o| {
                            o.iter()
                                .find(|o| o.get("key").and_then(Value::as_str) == Some(k))
                        })
                        .and_then(|o| o.get("label").and_then(Value::as_str))
                        .unwrap_or(k)
                        .to_string()
                })
                .collect();
            let choice = if a.trusted {
                say("card.said.duck", "I don’t give a duck", &[])
            } else if labels.is_empty() {
                say("group.lately.answered", "Answered", &[])
            } else {
                let l = labels.join(", ");
                say("card.said", l.as_str(), &[])
            };
            let (age, next) = age_short(a.answered_at, b.now);
            b.wake_at(Some(next));
            AnsweredView {
                id: c.object_id.clone(),
                session_id: chat.id.clone(),
                slug: chat.slug.clone(),
                name: chat.name.clone(),
                mark: chat.mark.clone(),
                title: shown_title(c),
                choice,
                answered_ms: a.answered_at,
                age,
            }
        })
        .collect()
}

/// The head of the group in view: its name and its ⋯ menu.
fn head(b: &Board, team: &str) -> GroupHeadView {
    let name = super::sidebar::team_name(
        b.teams
            .iter()
            .find(|d| d.id == team)
            .map(|d| d.name.as_str())
            .unwrap_or(""),
    );
    let mut menu = Vec::new();
    {
        let (n, fresh) = super::knowledge::counts(b);
        let entries = |n: u32| if n == 1 { "entry" } else { "entries" };
        let (ns, fs) = (n.to_string(), fresh.to_string());
        menu.push(GroupMenuItem {
            key: "knowledge".into(),
            label: say("group.menu.knowledge", "Knowledge", &[]),
            count: match (n, fresh) {
                (0, _) => None,
                (_, 0) => Some(say(
                    "group.menu.knowledge.count",
                    format!("{ns} {}", entries(n)),
                    &[("n", &ns)],
                )),
                _ => Some(say(
                    "group.menu.knowledge.fresh",
                    format!("{ns} {} · {fs} new", entries(n)),
                    &[("n", &ns), ("new", &fs)],
                )),
            },
            opens: Some(Screen::Knowledge {
                query: String::new(),
                filter: if fresh > 0 {
                    "new".into()
                } else {
                    String::new()
                },
                open: String::new(),
            }),
            apart: false,
        });
        let w = super::wakeups::count(b).to_string();
        menu.push(GroupMenuItem {
            key: "wakeups".into(),
            label: say("group.menu.wakeups", "Wakeups", &[]),
            count: (w != "0")
                .then(|| say("group.menu.wakeups.count", format!("{w} set"), &[("n", &w)])),
            opens: Some(Screen::Wakeups),
            apart: false,
        });
        menu.push(GroupMenuItem {
            key: "board".into(),
            label: say("group.menu.board", "Scribble Board", &[]),
            count: None,
            opens: Some(Screen::Scribble {
                board_id: crate::scribble::team_board(team),
            }),
            apart: false,
        });
        // (a room without teams has its one list, no team to rename)
        if !b.teams.is_empty() {
            menu.push(GroupMenuItem {
                key: "rename".into(),
                label: say("group.menu.rename", "Rename group", &[]),
                count: None,
                opens: None,
                apart: true,
            });
        }
    }
    let menu_says = say(
        "group.menu.says",
        format!("More of {}", name.en),
        &[("name", &name.en)],
    );
    GroupHeadView {
        team_id: team.into(),
        name,
        menu,
        menu_says,
    }
}

/// A team's goals, foldable to their first line on this device.
fn goals_of(b: &Board, team: &str) -> GoalsView {
    let text = b
        .teams
        .iter()
        .find(|d| d.id == team)
        .map(|d| d.goals.as_str())
        .unwrap_or("");
    let lines: Vec<String> = if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').map(String::from).collect()
    };
    let n = lines.len().to_string();
    let foldable = lines.len() > 1;
    let folded = foldable && b.lens.goals_folded.iter().any(|t| t == team);
    let show_all = foldable.then(|| {
        say(
            "group.goals.show_all",
            format!("Show all {n} goals"),
            &[("n", &n)],
        )
    });
    GoalsView {
        toggle: if folded {
            show_all.clone()
        } else {
            foldable.then(|| say("group.goals.fold", "Show the first goal only", &[]))
        },
        team_id: team.into(),
        foldable,
        show_all,
        folded,
        lines,
    }
}

/// The duck for all over the page's rows: a decision goes to its agent, a card to read is closed; a request, and a
/// card not read here yet, are never touched (the web's `duckAll`, the iPhone's `DeskWays`).
fn duck_all<'r>(rows: impl Iterator<Item = &'r DeskRowView> + Clone) -> Option<super::DuckAllView> {
    let mine = |kind: &str| {
        rows.clone()
            .filter(|r| r.kind == kind && !r.loading && !r.newer)
            .count()
    };
    let (asks, reads) = (mine("decision"), mine("info"));
    let n = asks + reads;
    if n == 0 {
        return None;
    }
    let card_ids = rows
        .clone()
        .filter(|r| matches!(r.kind.as_str(), "decision" | "info") && !r.loading && !r.newer)
        .map(|r| r.id.clone())
        .collect();
    let (ns, a, r) = (n.to_string(), asks.to_string(), reads.to_string());
    let args: &[(&str, &str)] = &[("n", &ns), ("decisions", &a), ("reads", &r)];
    let goes = if asks == 1 {
        "1 decision goes".to_string()
    } else {
        format!("{asks} decisions go")
    };
    let closed = if reads == 1 {
        "1 card to read is".to_string()
    } else {
        format!("{reads} cards to read are")
    };
    let (ask, yes) = match (asks > 0, n == 1) {
        (true, true) => (
            say("duck.ask.one", "Duck it?", args),
            say("duck.yes.one", "Yes, duck it", args),
        ),
        (true, false) => (
            say("duck.ask", format!("Duck all {n}?"), args),
            say("duck.yes", "Yes, duck them all", args),
        ),
        (false, true) => (
            say("duck.ask.read_one", "Close it as read?", args),
            say("duck.yes.read_one", "Yes, read", args),
        ),
        (false, false) => (
            say("duck.ask.read", format!("Close all {n} as read?"), args),
            say("duck.yes.read", "Yes, all read", args),
        ),
    };
    let what = if reads == 0 {
        say("duck.what.go", format!("{goes} to the agents"), args)
    } else if asks == 0 {
        say("duck.what.read", format!("{closed} closed"), args)
    } else {
        say(
            "duck.what",
            format!("{goes} to the agents, {closed} closed"),
            args,
        )
    };
    let tip = match (reads == 0, asks == 0, n == 1) {
        (true, _, true) => say(
            "duck.tip.one",
            "I don’t give a duck: for the one open decision",
            args,
        ),
        (true, _, false) => say(
            "duck.tip",
            format!("I don’t give a duck: for all {n} open decisions"),
            args,
        ),
        (false, true, true) => say(
            "duck.tip.read_one",
            "I don’t give a duck: close the one card to read",
            args,
        ),
        (false, true, false) => say(
            "duck.tip.read",
            format!("I don’t give a duck: close all {n} cards to read"),
            args,
        ),
        _ => say(
            "duck.tip.all",
            format!("I don’t give a duck: for all {n} open cards"),
            args,
        ),
    };
    let short = say("duck.short", "IDGAD", &[]);
    Some(super::DuckAllView {
        card_ids,
        tip,
        short,
        what,
        ask,
        yes,
        no: say("duck.no", "Cancel", &[]),
    })
}

/// A count with a comma between the thousands, as the group page writes it: 12,400.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Who a card or a request comes from: its session, and for a card of a helper that helper ("Helper Icons").
pub(super) fn from_of(b: &Board, agent: Option<usize>, card: Option<&Card>) -> Option<FromView> {
    let a = &b.agents[agent?];
    Some(FromView {
        session_id: a.row.key.clone(),
        name: a.row.name.clone(),
        mark: a.row.mark(),
        path: a.row.path.clone(),
        helper: card
            .and_then(|c| b.helper_of(c))
            .map(|h| say("from.helper", format!("Helper {h}"), &[("name", &h)])),
    })
}

pub(super) fn text_of(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// The sign before a title and its words: what knocks, and what may wait.
fn sign_of(permission: bool, info: bool, urgency: &str) -> (String, Option<Text>) {
    if permission {
        (
            "block".into(),
            Some(say("row.knock.permission", "Knock! Permission", &[])),
        )
    } else if urgency == "critical" {
        (
            "block".into(),
            Some(say("row.knock.blocking", "Knock! Blocking", &[])),
        )
    } else if urgency == "high" {
        ("knock".into(), Some(say("row.knock", "Knock", &[])))
    } else if !info && urgency == "low" {
        (
            "whenever".into(),
            Some(say("row.whenever", "Whenever", &[])),
        )
    } else {
        (String::new(), None)
    }
}

/// A card's title as every screen shows it: "Loading …" while its body is not read on this device, a line for a
/// card of a newer Trommi that names none.
/// A title without the status word an agent put in front ("Working on: …", "Arbeite an: …", "Paused: …"); the title
/// itself when it has none or when nothing would be left.
pub fn short_title(title: &str) -> String {
    const WORDS: [&str; 12] = [
        "working on",
        "arbeite an",
        "arbeitet an",
        "on it",
        "done",
        "fertig",
        "paused",
        "pausiert",
        "waiting",
        "wartet",
        "blocked",
        "blockiert",
    ];
    let Some((head, rest)) = title.split_once(':') else {
        return title.to_string();
    };
    let rest = rest.trim();
    if rest.is_empty() || !WORDS.contains(&head.trim().to_lowercase().as_str()) {
        return title.to_string();
    }
    rest.to_string()
}

pub fn shown_title(c: &Card) -> String {
    let title = text_of(&c.field("title"));
    if is_loading(c) {
        "Loading …".to_string()
    } else if c.is_unsupported() && title.is_empty() {
        "A card from a newer Trommi".into()
    } else {
        title
    }
}

pub(super) fn card_row(b: &Board, c: &Card, agent: Option<usize>) -> DeskRowView {
    let loading = is_loading(c);
    let newer = c.is_unsupported();
    let info = reads_as_info(c);
    let urgency = if loading {
        "normal".to_string()
    } else if c.urgency.is_empty() {
        "normal".into()
    } else {
        c.urgency.clone()
    };
    let title = shown_title(c);
    let teaser = if loading {
        "Still being decrypted on this device.".to_string()
    } else if newer {
        NEEDS_NEWER.into()
    } else {
        text_of(&c.field("teaser"))
    };
    let words = if teaser.is_empty() {
        plain(&text_of(&c.field("body")), b.assets())
    } else {
        teaser.clone()
    };
    let mut notes = Vec::new();
    if !loading {
        let merged = c
            .field("merged_from_object_ids")
            .as_array()
            .map_or(0, Vec::len);
        if merged == 1 {
            notes.push(say(
                "row.replaces.one",
                "replaces 1 question",
                &[("n", "1")],
            ));
        } else if merged > 1 {
            let n = merged.to_string();
            notes.push(say(
                "row.replaces",
                format!("replaces {n} questions"),
                &[("n", &n)],
            ));
        }
        if c.history.len() > 1 {
            notes.push(say("row.revised", "revised", &[]));
        }
    }
    if c.unsnoozed.is_some() && c.snoozed_until.is_none() {
        notes.push(say("row.back_from_later", "Back from Later", &[]));
    }
    let reason = text_of(&c.field("urgency_reason"));
    if teaser.is_empty() && !reason.is_empty() && !loading {
        notes.push(say("row.urgency_reason", reason.as_str(), &[]));
    }
    let (sign, sign_says) = sign_of(false, info, &urgency);
    // (pictures first, then videos; a page that belongs to a picture is no file of its own)
    let attachments: Vec<Value> = if loading || newer {
        Vec::new()
    } else {
        c.field("attachments")
            .as_array()
            .cloned()
            .unwrap_or_default()
    };
    let pages: Vec<&str> = attachments
        .iter()
        .filter_map(|a| a.get("page").and_then(Value::as_str))
        .filter_map(|p| p.strip_prefix("attachment:"))
        .collect();
    let kind_of = |a: &Value| {
        let t = a.get("media_type").and_then(Value::as_str).unwrap_or("");
        if t.starts_with("image/") {
            "image"
        } else if t.starts_with("video/") {
            "video"
        } else {
            ""
        }
    };
    let own: Vec<&Value> = attachments
        .iter()
        .filter(|a| !pages.contains(&a.get("attachment_id").and_then(Value::as_str).unwrap_or("")))
        .collect();
    let media: Vec<MediaView> = ["image", "video"]
        .iter()
        .flat_map(|k| {
            own.iter()
                .filter(move |a| kind_of(a) == *k)
                .map(move |a| MediaView {
                    file_id: a
                        .get("attachment_id")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    kind: k.to_string(),
                })
        })
        .collect();
    // (the fan's pictures are files the person may open: the App lets them be fetched)
    for a in own.iter().filter(|a| {
        media
            .iter()
            .any(|m| Some(m.file_id.as_str()) == a.get("attachment_id").and_then(Value::as_str))
    }) {
        super::files_seen::saw(a);
    }
    let media_says = match media.len() {
        0 => None,
        1 => Some(say("row.media.one", "Look at the picture", &[("n", "1")])),
        n => Some(say(
            "row.media",
            format!("Look at {n} pictures and videos"),
            &[("n", &n.to_string())],
        )),
    };
    let starred = b
        .model
        .human
        .get(&format!("star/{}", c.object_id))
        .is_some_and(|r| !r.value.is_null() && r.value != Value::Bool(false));
    let options: Vec<Opt> = if info {
        Vec::new()
    } else {
        options_of(&c.field("options"))
    };
    let advised: Vec<String> = match c.field("recommended") {
        Value::String(s) => vec![s],
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        _ => Vec::new(),
    };
    let multiple = c.field("allows_multiple") == Value::Bool(true);
    DeskRowView {
        id: c.object_id.clone(),
        kind: if info { "info" } else { "decision" }.into(),
        loading,
        newer,
        title,
        words,
        notes,
        urgency,
        sign,
        sign_says,
        starred,
        created_ms: c.created_at,
        age: None,
        from: from_of(b, agent, Some(c)),
        run: 0,
        media_says,
        media: media.into_iter().take(3).collect(),
        // (not read here yet: nothing to answer, not even "Got it", until it is read)
        tiles: if loading {
            no_tiles()
        } else if info {
            read_tiles()
        } else {
            tiles(&options, &advised, multiple, false)
        },
        ways: if loading {
            vec![]
        } else if info {
            vec!["later", "reverse", "shred"]
        } else {
            vec!["later", "reverse", "duck", "shred"]
        }
        .into_iter()
        .map(String::from)
        .collect(),
    }
}

pub(super) fn request_row(b: &Board, p: &PermissionRequest, agent: Option<usize>) -> DeskRowView {
    let from = from_of(b, agent, None);
    // (not read here yet: what it asks to run is unknown, so it is not offered to allow; it waits quietly)
    if !(p.content_state.is_empty() || p.content_state == "ok") {
        return DeskRowView {
            id: p.object_id.clone(),
            kind: "info".into(),
            loading: true,
            title: "Loading …".into(),
            words: "Still being decrypted on this device.".into(),
            urgency: "normal".into(),
            created_ms: p.sent_at,
            from,
            tiles: no_tiles(),
            ways: Vec::new(),
            ..Default::default()
        };
    }
    let (sign, sign_says) = sign_of(true, false, "critical");
    let options = vec![
        Opt {
            key: "allow".into(),
            label: "Allow".into(),
            ..Default::default()
        },
        Opt {
            key: "deny".into(),
            label: "Deny".into(),
            ..Default::default()
        },
    ];
    DeskRowView {
        id: p.object_id.clone(),
        kind: "request".into(),
        title: format!("Approval: {}", p.tool_name),
        words: plain(
            &format!("{}\n\n{}", p.description, p.input_preview),
            b.assets(),
        ),
        urgency: "critical".into(),
        sign,
        sign_says,
        created_ms: p.sent_at,
        from,
        tiles: tiles(&options, &[], false, true),
        ways: vec!["later".into()],
        ..Default::default()
    }
}

/// An option of a card as a row reads it.
#[derive(Clone, Debug, Default)]
pub struct Opt {
    pub key: String,
    pub label: String,
    /// Its short name, when it has one of at most 18 characters.
    pub short: String,
    pub settles: bool,
}

pub(crate) fn options_of(v: &Value) -> Vec<Opt> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|o| {
            let short = o
                .get("short")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            Opt {
                key: text_of(o.get("key").unwrap_or(&Value::Null)),
                label: text_of(o.get("label").unwrap_or(&Value::Null)),
                short: if short.encode_utf16().count() <= 18 {
                    short
                } else {
                    String::new()
                },
                settles: o.get("final") == Some(&Value::Bool(true)),
            }
        })
        .collect()
}

/// Whether a label stands in a tile `width` characters wide on at most `lines` lines (a hyphen may break a word).
fn fits(label: &str, width: usize, lines: usize) -> bool {
    let mut broken = String::new();
    let mut chars = label.trim().chars().peekable();
    while let Some(c) = chars.next() {
        broken.push(c);
        if c == '-' && chars.peek().is_some_and(|n| !n.is_whitespace()) {
            broken.push(' ');
        }
    }
    let (mut n, mut used) = (1, 0);
    for word in broken.split_whitespace() {
        let len = word.encode_utf16().count();
        if len > width {
            return false;
        }
        if used > 0 && used + 1 + len > width {
            n += 1;
            used = len;
        } else {
            used += usize::from(used > 0) + len;
        }
    }
    n <= lines
}

/// A plain yes or no.
fn bare(label: &str) -> bool {
    matches!(
        label.trim().to_lowercase().as_str(),
        "yes" | "no" | "ok" | "okay" | "allow" | "deny" | "ja" | "nein"
    )
}

fn no_tiles() -> TilesView {
    TilesView {
        kind: "none".into(),
        ..Default::default()
    }
}

fn read_tiles() -> TilesView {
    TilesView {
        kind: "read".into(),
        ..Default::default()
    }
}

/// The answers a row offers for these options. `advised`: the keys its agent advises. `request`: an approval
/// request (Allow and Deny, thumbs, "allow" leads).
pub fn tiles(options: &[Opt], advised: &[String], multiple: bool, request: bool) -> TilesView {
    let all_bare = options.iter().all(|o| bare(&o.label));
    // (how the labels stand under the thumbs: in the usual type, in the small one, or not at all)
    let size = if all_bare {
        "none"
    } else if options.iter().all(|o| fits(&o.label, 14, 2)) {
        "usual"
    } else if options.iter().all(|o| fits(&o.label, 17, 3)) {
        "small"
    } else {
        "none"
    };
    let all_short = options.iter().all(|o| !o.short.is_empty());
    let quick = !multiple && (request || options.len() == 2);
    if quick && (all_bare || size != "none" || all_short) {
        // Thumbs only for a real yes or no (bare words, or a request): the option its agent leads with (its first,
        // or "allow") is the up. Two named options are two tiles with their words; the advised one is the filled
        // one. The lead always stands last (at the right).
        let thumbs = all_bare || request;
        let worded = !all_bare && size == "none";
        let is_lead = |i: usize, o: &Opt| {
            if request {
                o.key == "allow"
            } else if thumbs {
                i == 0
            } else {
                advised.contains(&o.key)
            }
        };
        let mut order: Vec<(usize, &Opt)> = options.iter().enumerate().collect();
        order.sort_by_key(|(i, o)| is_lead(*i, o));
        let tiles = order
            .into_iter()
            .map(|(i, o)| {
                let lead = is_lead(i, o);
                let is_advised = advised.contains(&o.key);
                let unlabelled = size == "none" && !all_bare;
                TileView {
                    key: o.key.clone(),
                    label: if worded {
                        o.short.clone()
                    } else if unlabelled {
                        String::new()
                    } else {
                        o.label.clone()
                    },
                    thumb: if lead { "yes" } else { "no" }.into(),
                    thumbs,
                    lead,
                    advised: is_advised,
                    small: size == "small",
                    short: worded,
                    settles: o.settles,
                    tip: if is_advised {
                        Some(say("tile.recommended", "Recommended", &[]))
                    } else if o.settles {
                        Some(say("tile.settles", "Settles it", &[]))
                    } else if unlabelled {
                        Some(say("tile.label", o.label.as_str(), &[]))
                    } else {
                        None
                    },
                    says: if o.settles {
                        say(
                            "tile.says.settles",
                            format!("{} (settles it)", o.label),
                            &[("label", &o.label)],
                        )
                    } else {
                        say("tile.says", o.label.as_str(), &[("label", &o.label)])
                    },
                }
            })
            .collect();
        return TilesView {
            kind: "pair".into(),
            tiles,
            ..Default::default()
        };
    }
    // More than two ways, one of them advised: that one is the lead tile, one press takes it; "+N other ways"
    // opens the card.
    let one = (!request && !multiple && advised.len() == 1)
        .then(|| options.iter().find(|o| o.key == advised[0]))
        .flatten();
    if let Some(o) = one {
        let others = options.len() as u32 - 1;
        let n = others.to_string();
        return TilesView {
            kind: "advised".into(),
            tiles: vec![TileView {
                key: o.key.clone(),
                label: o.label.clone(),
                thumb: "yes".into(),
                thumbs: false,
                lead: true,
                advised: true,
                small: false,
                short: false,
                settles: o.settles,
                tip: Some(say("tile.label", o.label.as_str(), &[])),
                says: if o.settles {
                    say(
                        "tile.says.advised_settles",
                        format!("{}, advised (settles it)", o.label),
                        &[("label", &o.label)],
                    )
                } else {
                    say(
                        "tile.says.advised",
                        format!("{}, advised", o.label),
                        &[("label", &o.label)],
                    )
                },
            }],
            others,
            says: Some(say(
                "tile.others",
                format!("{n} other ways: open the card"),
                &[("n", &n)],
            )),
        };
    }
    let n = options.len().to_string();
    TilesView {
        kind: "choose".into(),
        says: Some(if multiple {
            say(
                "tile.choose.several",
                format!("Choose: {n} options, several"),
                &[("n", &n)],
            )
        } else {
            say("tile.choose", format!("Choose: {n} options"), &[("n", &n)])
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{dev, Room, MIN, NOW};
    use crate::views::Lens;
    use serde_json::json;

    fn view_with(r: &Room, lens: &Lens) -> GroupView {
        let m = r.model(NOW);
        group(&Board::new(&m, lens, NOW))
    }
    fn view(r: &Room, team: Option<&str>) -> GroupView {
        view_with(
            r,
            &Lens {
                team: team.map(String::from),
                ..Default::default()
            },
        )
    }
    fn opt(key: &str, label: &str) -> Opt {
        Opt {
            key: key.into(),
            label: label.into(),
            ..Default::default()
        }
    }
    fn shown(t: &TilesView) -> Vec<(&str, &str, &str)> {
        t.tiles
            .iter()
            .map(|x| (x.key.as_str(), x.label.as_str(), x.thumb.as_str()))
            .collect()
    }
    fn en(t: &Option<Text>) -> &str {
        t.as_ref().map(|t| t.en.as_str()).unwrap_or("")
    }
    /// Every row of the page, block after block.
    fn rows(v: &GroupView) -> Vec<&DeskRowView> {
        v.rows()
    }
    /// The calm word when nothing is open.
    fn calm(v: &GroupView) -> &str {
        v.empty.as_ref().map(|e| e.title.en.as_str()).unwrap_or("")
    }
    fn blitz_en(v: &GroupView) -> &str {
        v.blitz.as_ref().map(|b| b.says.en.as_str()).unwrap_or("")
    }

    /// A card that arrives in an archived session stays off the group page (web and iPhone: the core's stack); a main whose
    /// agent connects again is no longer archived, and its card stands there.
    #[test]
    fn a_card_of_an_archived_session_stays_off_the_page() {
        let mut r = Room::default();
        r.session("main", None, false);
        r.session("helper", Some("main"), false);
        r.settings.insert(dev("main"), json!({ "archived": true }));
        r.settings
            .insert(dev("helper"), json!({ "archived": true }));
        r.card("m1", "main", "decision", "normal")["session_id"] = json!(dev("main"));
        r.card("h1", "helper", "decision", "normal")["session_id"] = json!(dev("helper"));
        assert!(view(&r, None).stack.is_empty());
        r.sessions[0]["is_online"] = json!(true);
        r.sessions[0]["offline_since"] = Value::Null;
        assert_eq!(view(&r, None).stack, ["m1"]);
    }

    /// The web's `tiles` (app/web/public/ui.mjs), case by case.
    #[test]
    fn a_yes_or_no_is_two_thumbs_the_first_option_up_and_last() {
        let t = tiles(
            &[opt("y", "Yes"), opt("n", "No")],
            &["n".into()],
            false,
            false,
        );
        assert_eq!(
            (t.kind.as_str(), shown(&t)),
            ("pair", vec![("n", "No", "no"), ("y", "Yes", "yes")])
        );
        assert!(t.tiles.iter().all(|x| x.thumbs && !x.small && !x.short));
        // (the advised one is marked, whichever thumb it is)
        assert_eq!(
            t.tiles
                .iter()
                .map(|x| (x.advised, x.lead, en(&x.tip)))
                .collect::<Vec<_>>(),
            [(true, false, "Recommended"), (false, true, "")]
        );
        // an approval request: allow is the up, whatever the order
        let t = tiles(
            &[opt("deny", "Deny"), opt("allow", "Allow")],
            &[],
            false,
            true,
        );
        assert_eq!(
            shown(&t),
            [("deny", "Deny", "no"), ("allow", "Allow", "yes")]
        );
    }

    #[test]
    fn two_named_ways_are_two_tiles_the_advised_one_leads() {
        let t = tiles(
            &[opt("split", "Split"), opt("centered", "Centered")],
            &["split".into()],
            false,
            false,
        );
        assert_eq!(
            shown(&t),
            [("centered", "Centered", "no"), ("split", "Split", "yes")]
        );
        assert!(t.tiles.iter().all(|x| !x.thumbs));
        // (none advised: none leads, the order given)
        let t = tiles(&[opt("a", "Alpha"), opt("b", "Beta")], &[], false, false);
        assert_eq!(shown(&t), [("a", "Alpha", "no"), ("b", "Beta", "no")]);
        // words that need three lines of 17 stand in the small type
        let t = tiles(
            &[
                opt("a", "Keep the old importer running"),
                opt("b", "On step 3"),
            ],
            &[],
            false,
            false,
        );
        assert!(t.kind == "pair" && t.tiles.iter().all(|x| x.small));
        // a choice that settles the card says so
        let mut last = opt("b", "Drop it");
        last.settles = true;
        let t = tiles(&[opt("a", "Keep it"), last], &[], false, false);
        assert_eq!(
            (
                en(&t.tiles[1].tip),
                t.tiles[1].says.en.as_str(),
                t.tiles[0].says.en.as_str()
            ),
            ("Settles it", "Drop it (settles it)", "Keep it")
        );
    }

    #[test]
    fn labels_that_fit_no_tile_need_short_names_or_the_card() {
        let long = |key: &str, short: &str| Opt {
            key: key.into(),
            label: format!("{key}: a label that is far too long for any tile to hold"),
            short: short.into(),
            settles: false,
        };
        let t = tiles(&[long("a", "Keep"), long("b", "Drop")], &[], false, false);
        assert_eq!(
            (t.kind.as_str(), shown(&t)),
            ("pair", vec![("a", "Keep", "no"), ("b", "Drop", "no")])
        );
        assert!(t
            .tiles
            .iter()
            .all(|x| x.short && en(&x.tip).starts_with(&x.key)));
        // (one without a short name: the card)
        let t = tiles(&[long("a", "Keep"), long("b", "")], &[], false, false);
        assert_eq!(
            (t.kind.as_str(), en(&t.says)),
            ("choose", "Choose: 2 options")
        );
        // (a word longer than the tile is wide fits nowhere; a hyphen may break one)
        assert!(
            !fits("Internationalisation", 17, 3)
                && fits("Server-side-rendering", 14, 2)
                && !fits("one two three four five six", 5, 2)
        );
    }

    #[test]
    fn more_than_two_ways_the_advised_one_or_the_card() {
        let three = [opt("a", "Outline"), opt("b", "Filled"), opt("c", "Duotone")];
        let t = tiles(&three, &["b".into()], false, false);
        assert_eq!(
            (t.kind.as_str(), shown(&t), t.others, en(&t.says)),
            (
                "advised",
                vec![("b", "Filled", "yes")],
                2,
                "2 other ways: open the card"
            )
        );
        assert_eq!(t.tiles[0].says.en, "Filled, advised");
        // (two advised, none advised, an advised key that is no option, several may be ticked: the card)
        for (advised, multiple, says) in [
            (vec!["a", "b"], false, "Choose: 3 options"),
            (vec![], false, "Choose: 3 options"),
            (vec!["zz"], false, "Choose: 3 options"),
            (vec!["b"], true, "Choose: 3 options, several"),
        ] {
            let advised: Vec<String> = advised.into_iter().map(String::from).collect();
            let t = tiles(&three, &advised, multiple, false);
            assert_eq!(
                (t.kind.as_str(), en(&t.says), t.tiles.len()),
                ("choose", says, 0)
            );
        }
        // (two ways of which several may be ticked are no pair)
        assert_eq!(
            tiles(&[opt("y", "Yes"), opt("n", "No")], &[], true, false).kind,
            "choose"
        );
    }

    #[test]
    fn the_rows_are_the_walk_of_the_group_in_view_the_questions_first() {
        let mut r = Room {
            teams: json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } }),
            ..Default::default()
        };
        r.session("a", None, true);
        r.session("b-lab", None, true);
        r.on_team("b-lab", "lab");
        r.card("q-old", "a", "decision", "normal")["first_envelope_number"] = json!(1);
        r.card("q-new", "a", "decision", "normal");
        r.card("q-read", "a", "info", "normal");
        r.card("q-block", "a", "decision", "critical");
        r.card("q-back", "a", "decision", "normal")["in_revision"] =
            json!({ "by": "me", "envelope_number": 500 });
        r.card("q-lab", "b-lab", "decision", "high");
        r.request("p1", "a", "ok");
        let v = view(&r, Some("main"));
        // (the questions in the core's order, what blocks first, the oldest first; then what is only to read)
        assert_eq!(v.stack, ["q-block", "p1", "q-old", "q-new", "q-read"]);
        assert_eq!(
            rows(&v).iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            v.stack
        );
        assert_eq!(en(&v.summary), "4 wait for you · 1 to read");
        assert_eq!(v.summary.as_ref().unwrap().args["n"], "4");
        assert_eq!(v.blocks.len(), 1, "one chat, one block");
        // (Blitz is every group's: the other team's question too)
        assert_eq!((v.waiting, blitz_en(&v)), (4, "Blitz: 6 open cards"));
        assert_eq!(
            v.blitz
                .as_ref()
                .map(|b| (b.card.as_str(), b.scope.en.as_str())),
            Some(("q-block", "all groups"))
        );
        assert!(rows(&v)
            .iter()
            .all(|x| x.run == 0 && x.from.as_ref().is_some_and(|f| f.path == ["Work", "a"])));
        // one group at a time: the other team's question stands on its own page only ("all" is no team: the first)
        assert_eq!(view(&r, Some("lab")).stack, ["q-lab"]);
        assert_eq!(view(&r, Some("all")).stack, v.stack);
        assert_eq!(view(&r, None).stack, v.stack);
        assert_eq!(
            walk_all(&Board::new(&r.model(NOW), &Lens::default(), NOW)).len(),
            6
        );
    }

    /// One block per chat that has something open, in the list's order; in a block its questions, then what is only to
    /// read, in the walk's order; its head opens the chat. No chat without open cards, no filter.
    #[test]
    fn the_rows_stand_in_one_block_per_chat_in_the_list_order() {
        let mut r = Room::default();
        for name in ["a", "b", "c", "d", "e", "f"] {
            r.session(name, None, true);
        }
        r.card("q-b", "b", "decision", "normal");
        r.card("q-e", "e", "decision", "normal");
        r.card("i-f", "f", "info", "normal");
        r.card("q-d", "d", "decision", "high");
        r.card("i-d", "d", "info", "normal");
        r.card("q-b2", "b", "decision", "normal");
        let v = view(&r, None);
        let names: Vec<&str> = v.blocks.iter().map(|k| k.name.en.as_str()).collect();
        let (m, lens) = (r.model(NOW), Lens::default());
        let b = Board::new(&m, &lens, NOW);
        let list: Vec<String> = super::super::sidebar::rows_of(
            &b,
            &units_of(&b),
            &super::super::sidebar::asked_last(&b),
        )
        .iter()
        .map(|c| c.name.en.clone())
        .filter(|n| names.contains(&n.as_str()))
        .collect();
        assert_eq!(names, list, "the list's order, whatever knocks");
        let mut all = names.clone();
        all.sort();
        assert_eq!(
            all,
            ["b", "d", "e", "f"],
            "no block for a chat with nothing open"
        );
        let d = v.blocks.iter().find(|k| k.name.en == "d").unwrap();
        assert_eq!(
            (
                d.session_id.as_str(),
                d.questions
                    .iter()
                    .map(|x| x.id.as_str())
                    .collect::<Vec<_>>(),
                d.reads.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
                en(&d.open),
                d.open_chat.en.as_str(),
                d.says.en.as_str()
            ),
            (
                dev("d").as_str(),
                vec!["q-d"],
                vec!["i-d"],
                "1 open",
                "Open chat ›",
                "d: 1 open. Open the chat"
            )
        );
        assert!(!d.slug.is_empty() && d.mark.starts_with("draw:"));
        let b = v.blocks.iter().find(|k| k.name.en == "b").unwrap();
        assert_eq!((b.questions.len(), en(&b.open)), (2, "2 open"));
        let f = v.blocks.iter().find(|k| k.name.en == "f").unwrap();
        assert_eq!(
            (f.open.is_none(), f.says.en.as_str()),
            (true, "f: open the chat")
        );
        // (the stack is the blocks' rows in their order)
        assert_eq!(
            v.stack,
            v.blocks
                .iter()
                .flat_map(|k| k.questions.iter().chain(k.reads.iter()))
                .map(|x| x.id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(en(&v.summary), "4 wait for you · 2 to read");
        assert_eq!(v.waiting, 4);
    }

    /// The chats on no team have no group page: their cards stand in their chat and in Blitz; a team's page never
    /// shows them, and the address of the group of before ("none") is the first team's page.
    #[test]
    fn the_chats_on_no_team_have_no_group_page() {
        let mut r = Room {
            teams: json!({ "main": { "name": "Work", "created_at": 1 } }),
            ..Default::default()
        };
        r.session("a", None, true);
        r.session("loose", None, true);
        r.settings.insert(dev("loose"), json!({ "desk": null }));
        r.card("q-a", "a", "decision", "normal");
        r.card("q-loose", "loose", "decision", "normal");
        let work = view(&r, Some("main"));
        assert_eq!(work.stack, ["q-a"]);
        assert_eq!(blitz_en(&work), "Blitz: 2 open cards");
        assert_eq!(view(&r, Some("none")).stack, ["q-a"]);
        assert_eq!(view(&r, Some("none")).head.unwrap().team_id, "main");
    }

    /// The head: the team's name and the ⋯ menu that holds what the Desk's tiles held (no faces: the sidebar has them).
    #[test]
    fn the_head_names_the_group_and_its_menu() {
        let mut r = Room {
            teams: json!({ "main": { "name": "Fernly", "created_at": 1 } }),
            ..Default::default()
        };
        for name in ["a", "b", "c", "d"] {
            r.session(name, None, true);
        }
        let v = view(&r, None);
        let head = v.head.unwrap();
        assert_eq!(
            (head.team_id.as_str(), head.name.en.as_str()),
            ("main", "Fernly")
        );
        assert_eq!(
            head.menu
                .iter()
                .map(|m| (m.key.as_str(), m.label.en.as_str(), en(&m.count), m.apart))
                .collect::<Vec<_>>(),
            [
                ("knowledge", "Knowledge", "", false),
                ("wakeups", "Wakeups", "", false),
                ("board", "Scribble Board", "", false),
                ("rename", "Rename group", "", true)
            ]
        );
        assert!(matches!(head.menu[0].opens, Some(Screen::Knowledge { .. })));
        assert_eq!(head.menu[1].opens, Some(Screen::Wakeups));
        assert!(matches!(head.menu[2].opens, Some(Screen::Scribble { .. })));
        assert_eq!(head.menu[3].opens, None);
        // (a room without teams: its one list has no team to rename)
        let mut r = Room::default();
        r.session("a", None, true);
        let head = view(&r, None).head.unwrap();
        assert_eq!(head.name.en, "Personal");
        assert!(head.menu.iter().all(|m| m.key != "rename"));
        // (a new account: no group to name yet)
        assert!(view(&Room::default(), None).head.is_none());
    }

    #[test]
    fn a_row_says_what_knocks_and_what_it_went_through() {
        let mut r = Room::default();
        r.session("a", None, true);
        let c = r.card("q1", "a", "decision", "critical");
        c["options"] = json!([{ "key": "y", "label": "Yes" }, { "key": "n", "label": "No" }]);
        c["body"] = json!("**Disk** at `91%`.\n\n```sh\ndf -h\n```\nAdd # space?");
        c["urgency_reason"] = json!("the backfill stops in an hour");
        c["merged_from_object_ids"] = json!(["x", "y"]);
        c["versions"] = json!([{ "object_version": 1, "envelope_number": 1, "object_state": "open" }, { "object_version": 2, "envelope_number": 2, "object_state": "open" }]);
        c["attachments"] = json!([
            { "attachment_id": "v1", "media_type": "video/mp4" }, { "attachment_id": "p1", "media_type": "image/png", "page": "attachment:h1" },
            { "attachment_id": "h1", "media_type": "text/html" }, { "attachment_id": "p2", "media_type": "image/png" },
            { "attachment_id": "p3", "media_type": "image/jpeg" }, { "attachment_id": "doc", "media_type": "application/pdf" },
        ]);
        r.card("q2", "a", "decision", "high")["teaser"] = json!("A teaser.");
        r.cards[1]["urgency_reason"] = json!("not shown beside a teaser");
        r.card("q3", "a", "decision", "low");
        r.card("q4", "a", "info", "low");
        r.stars = json!({ "q3": { "at": 5 } });
        let v = view(&r, None);
        let all = rows(&v);
        let row = |id: &str| *all.iter().find(|x| x.id == id).unwrap();
        let q1 = row("q1");
        assert_eq!(
            (q1.sign.as_str(), en(&q1.sign_says), q1.words.as_str()),
            ("block", "Knock! Blocking", "Disk at 91%. Add space?")
        );
        assert_eq!(
            q1.notes.iter().map(|n| n.en.as_str()).collect::<Vec<_>>(),
            [
                "replaces 2 questions",
                "revised",
                "the backfill stops in an hour"
            ]
        );
        // (pictures before videos, three at most; the page of a picture and a document are none)
        assert_eq!(
            q1.media
                .iter()
                .map(|x| (x.file_id.as_str(), x.kind.as_str()))
                .collect::<Vec<_>>(),
            [("p1", "image"), ("p2", "image"), ("p3", "image")]
        );
        assert_eq!(en(&q1.media_says), "Look at 4 pictures and videos");
        assert_eq!(
            (q1.tiles.kind.as_str(), q1.ways.join(" ")),
            ("pair", "later reverse duck shred".into())
        );
        let q2 = row("q2");
        assert_eq!(
            (
                q2.sign.as_str(),
                en(&q2.sign_says),
                q2.words.as_str(),
                q2.notes.len()
            ),
            ("knock", "Knock", "A teaser.", 0)
        );
        assert_eq!(
            (
                row("q3").sign.as_str(),
                en(&row("q3").sign_says),
                row("q3").starred,
                q1.starred
            ),
            ("whenever", "Whenever", true, false)
        );
        // (something to read: no sign for a low one, What?? and Got it, no duck)
        let q4 = row("q4");
        assert_eq!(
            (
                q4.kind.as_str(),
                q4.sign.as_str(),
                q4.tiles.kind.as_str(),
                q4.ways.join(" ")
            ),
            ("info", "", "read", "later reverse shred".into())
        );
    }

    #[test]
    fn what_is_not_read_here_yet_waits_quietly() {
        let mut r = Room::default();
        r.session("a", None, true);
        let c = r.card("q-loading", "a", "decision", "high");
        c["content_state"] = json!("undecryptable");
        c["options"] = json!([{ "key": "y", "label": "Yes" }, { "key": "n", "label": "No" }]);
        let c = r.card("q-newer", "a", "decision", "normal");
        c["unsupported"] = json!("schema");
        c["title"] = json!("");
        r.request("p-unread", "a", "header");
        r.request("p-read", "a", "ok");
        let v = view(&r, None);
        let all = rows(&v);
        let row = |id: &str| *all.iter().find(|x| x.id == id).unwrap();
        let l = row("q-loading");
        assert_eq!(
            (
                l.kind.as_str(),
                l.loading,
                l.title.as_str(),
                l.words.as_str(),
                l.sign.as_str(),
                l.urgency.as_str(),
                l.tiles.kind.as_str()
            ),
            (
                "info",
                true,
                "Loading …",
                "Still being decrypted on this device.",
                "",
                "normal",
                "none"
            )
        );
        assert!(
            l.ways.is_empty()
                && row("p-unread").ways.is_empty()
                && row("p-unread").tiles.kind == "none"
        );
        let n = row("q-newer");
        assert_eq!(
            (
                n.kind.as_str(),
                n.newer,
                n.title.as_str(),
                n.words.as_str(),
                n.tiles.kind.as_str()
            ),
            (
                "info",
                true,
                "A card from a newer Trommi",
                NEEDS_NEWER,
                "read"
            )
        );
        let p = row("p-unread");
        assert_eq!(
            (
                p.kind.as_str(),
                p.loading,
                p.title.as_str(),
                p.sign.as_str()
            ),
            ("info", true, "Loading …", "")
        );
        let p = row("p-read");
        assert_eq!(
            (
                p.kind.as_str(),
                p.title.as_str(),
                p.sign.as_str(),
                en(&p.sign_says),
                p.ways.join(" ")
            ),
            (
                "request",
                "Approval: Bash",
                "block",
                "Knock! Permission",
                "later".into()
            )
        );
        assert_eq!(
            shown(&p.tiles),
            [("deny", "Deny", "no"), ("allow", "Allow", "yes")]
        );
        assert_eq!(v.waiting, 1, "only the request that is read is a question");
    }

    #[test]
    fn the_page_invites_reads_and_says_who_is_cut_off() {
        let empty = Room::default();
        let v = view(&empty, None);
        assert_eq!(
            (v.mode.as_str(), calm(&v), v.goals.is_none()),
            ("invite", "Nothing waits for you in Personal.", true)
        );
        let v = view_with(
            &empty,
            &Lens {
                reading: true,
                reading_progress: Some((12_400, 27_000)),
                ..Default::default()
            },
        );
        assert_eq!(
            (v.mode.as_str(), en(&v.reading)),
            ("reading", "Reading your history: 12,400 of 27,000")
        );
        assert_eq!(calm(&v), "Catching up: reading your sessions on this device. Their open questions show here as they are read.");

        let mut r = Room::default();
        r.session("a", None, true)["heard_up_to"] = json!(40);
        assert_eq!(view(&r, None).mode, "group");
        assert_eq!(view(&r, None).blitz, None);
        // a session that is cut off has its line above the questions
        r.session("b", None, false)["link"] = json!({ "hears": "live", "attached": true, "exit": { "reason": "stdin", "claude": "alive" } });
        let v = view(&r, None);
        assert_eq!(
            v.slip
                .iter()
                .map(|s| (
                    s.session_id.clone(),
                    s.text.en.as_str(),
                    s.fix.say.en.as_str(),
                    s.fix.code.as_str()
                ))
                .collect::<Vec<_>>(),
            [(
                dev("b"),
                "b is cut off since 13:27: it cannot hear you and cannot write to you.",
                "In its terminal:",
                "/mcp → trommi → Reconnect"
            )]
        );
        // something to read is no question: Blitz walks it
        r.card("q-read", "a", "info", "normal");
        let v = view(&r, None);
        assert_eq!(
            (v.waiting, blitz_en(&v), v.empty.is_none(), en(&v.summary)),
            (0, "Blitz: 1 open card", true, "1 to read")
        );
    }

    /// Nothing waits here: the calm word, who is at work, where else a question waits, what he answered lately.
    #[test]
    fn nothing_open_says_so_calmly_and_where_else_something_waits() {
        let mut r = Room {
            teams: json!({ "main": { "name": "Work", "created_at": 1 }, "lab": { "name": "Lab", "created_at": 2 } }),
            ..Default::default()
        };
        r.session("a", None, true);
        r.session("b", None, true);
        r.session("c-lab", None, true);
        r.on_team("c-lab", "lab");
        let v = view(&r, Some("main"));
        let e = v.empty.as_ref().unwrap();
        assert_eq!(
            (
                e.title.en.as_str(),
                en(&e.line),
                e.elsewhere.is_none(),
                e.lately.is_empty(),
                e.lately_says.is_none()
            ),
            (
                "Nothing waits for you in Work.",
                "What your chats ask lands here.",
                true,
                true,
                true
            )
        );
        // (a card that is with its agent waits for the agent: nothing waits for him)
        r.card("q-back", "a", "decision", "normal")["in_revision"] =
            json!({ "by": "me", "envelope_number": 500 });
        assert_eq!(
            calm(&view(&r, Some("main"))),
            "Nothing waits for you in Work."
        );
        // (what is put off is not open here)
        r.card("q-later", "a", "decision", "normal");
        r.snoozes = json!({ "q-later": { "until": NOW + 60 * MIN } });
        assert_eq!(
            calm(&view(&r, Some("main"))),
            "Nothing waits for you in Work."
        );
        // another team's question: where it waits
        r.card("q-lab", "c-lab", "decision", "normal");
        let v = view(&r, Some("main"));
        let at = v.empty.as_ref().unwrap().elsewhere.as_ref().unwrap();
        assert_eq!(
            (
                at.team_id.as_str(),
                at.says.en.as_str(),
                at.says.args["name"].as_str()
            ),
            ("lab", "1 question waits in Lab", "Lab")
        );
        assert!(at.mark.starts_with("draw:"));
        // what he answered here lately, the newest first, with what he chose
        let c = r.card("q-done", "a", "decision", "normal");
        c["object_state"] = json!("answered");
        c["options"] =
            json!([{ "key": "ship", "label": "Ship today" }, { "key": "wait", "label": "Wait" }]);
        c["answer"] = json!({ "answer_action": "answer", "choices": ["ship"], "answered_at": NOW - 2 * 60 * MIN });
        let c = r.card("q-duck", "b", "decision", "normal");
        c["object_state"] = json!("answered");
        c["answer"] = json!({ "answer_action": "answer", "choices": [], "trusted": true, "answered_at": NOW - 5 * MIN });
        let v = view(&r, Some("main"));
        let e = v.empty.as_ref().unwrap();
        assert_eq!(en(&e.lately_says), "You answered lately");
        assert_eq!(
            e.lately
                .iter()
                .map(|l| (
                    l.id.as_str(),
                    l.name.en.as_str(),
                    l.choice.en.as_str(),
                    l.age.en.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                ("q-duck", "b", "I don’t give a duck", "5m"),
                ("q-done", "a", "Ship today", "2h")
            ]
        );
        // (something open: no calm word)
        r.card("q-open", "a", "decision", "normal");
        assert!(view(&r, Some("main")).empty.is_none());
    }

    /// Who is at work, in one line.
    #[test]
    fn the_names_at_work_in_one_line() {
        let n = |v: &[&str]| names_line(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(
            (
                n(&["a"]),
                n(&["a", "b"]),
                n(&["a", "b", "c"]),
                n(&["a", "b", "c", "d", "e"])
            ),
            (
                "a".to_string(),
                "a and b".to_string(),
                "a, b and c".to_string(),
                "a, b and 3 more".to_string()
            )
        );
    }

    /// A count of one is a count too: the shell draws the number from `args.n` (the Linux shell's empty bubble).
    #[test]
    fn a_count_of_one_names_its_number() {
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("q", "a", "decision", "normal");
        let blitz = view(&r, None).blitz.unwrap().says;
        assert_eq!(
            (blitz.key.as_str(), blitz.args.get("n").map(String::as_str)),
            ("blitz.one", Some("1"))
        );
    }

    #[test]
    fn the_goals_fold_with_two_lines_or_more() {
        let mut r = Room {
            teams: json!({ "main": { "name": "Work", "goals": "  \nShip it   \r\nTest it\n\n" } }),
            ..Default::default()
        };
        r.session("a", None, true);
        let g = view(&r, None).goals.unwrap();
        assert_eq!(
            (
                g.team_id.as_str(),
                g.lines.clone(),
                g.foldable,
                en(&g.show_all)
            ),
            (
                "main",
                vec!["Ship it".to_string(), "Test it".into()],
                true,
                "Show all 2 goals"
            )
        );
        assert_eq!(
            (g.folded, en(&g.toggle)),
            (false, "Show the first goal only")
        );
        // folded on this device: the toggle shows them all again
        let lens = Lens {
            goals_folded: vec!["main".into()],
            ..Default::default()
        };
        let g = view_with(&r, &lens).goals.unwrap();
        assert_eq!((g.folded, en(&g.toggle)), (true, "Show all 2 goals"));
        r.teams = json!({ "main": { "name": "Work", "goals": "One goal" } });
        let g = view_with(&r, &lens).goals.unwrap();
        assert_eq!(
            (g.lines.len(), g.foldable, g.show_all, g.folded, g.toggle),
            (1, false, None, false, None)
        );
        r.teams = json!({ "main": { "name": "Work" } });
        assert_eq!(view(&r, None).goals.unwrap().lines.len(), 0);
        // (a room without teams keeps its goals on the first team it will have)
        let mut r = Room::default();
        r.session("a", None, true);
        assert_eq!(view(&r, None).goals.unwrap().team_id, "main");
    }

    #[test]
    fn the_duck_for_all_leaves_the_decisions_to_the_agents_and_reads_the_rest() {
        let mut r = Room::default();
        r.session("a", None, true);
        let v = view(&r, None);
        assert!(v.duck.is_none(), "nothing waits: no duck");
        r.card("q1", "a", "decision", "normal");
        r.request("p-read", "a", "ok");
        let v = view(&r, None);
        let d = v.duck.clone().unwrap();
        assert_eq!(d.card_ids, ["q1"], "a request is never ducked");
        assert_eq!(
            (
                d.ask.en.as_str(),
                d.yes.en.as_str(),
                d.tip.en.as_str(),
                d.what.en.as_str(),
                d.no.en.as_str()
            ),
            (
                "Duck it?",
                "Yes, duck it",
                "I don’t give a duck: for the one open decision",
                "1 decision goes to the agents",
                "Cancel"
            )
        );
        assert_eq!(
            (d.short.en.as_str(), d.short.key.as_str()),
            ("IDGAD", "duck.short"),
            "the short name, the question follows"
        );
        r.card("i1", "a", "info", "normal");
        r.card("i2", "a", "info", "normal");
        let c = r.card("q-loading", "a", "decision", "normal");
        c["content_state"] = json!("header");
        let d = view(&r, None).duck.unwrap();
        assert_eq!(
            d.card_ids.len(),
            3,
            "not what is not read here yet: {:?}",
            d.card_ids
        );
        assert_eq!(
            (
                d.ask.en.as_str(),
                d.yes.en.as_str(),
                d.tip.en.as_str(),
                d.what.en.as_str()
            ),
            (
                "Duck all 3?",
                "Yes, duck them all",
                "I don’t give a duck: for all 3 open cards",
                "1 decision goes to the agents, 2 cards to read are closed"
            )
        );
        assert_eq!(d.ask.args.get("n").map(String::as_str), Some("3"));
        // only cards to read
        let mut r = Room::default();
        r.session("a", None, true);
        r.card("i1", "a", "info", "normal");
        r.card("i2", "a", "info", "normal");
        let d = view(&r, None).duck.unwrap();
        assert_eq!(
            (d.ask.en.as_str(), d.yes.en.as_str(), d.what.en.as_str()),
            (
                "Close all 2 as read?",
                "Yes, all read",
                "2 cards to read are closed"
            )
        );
    }
}
