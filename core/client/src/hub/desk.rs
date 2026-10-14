//! `GET /v1/desk`: whole (the open objects with their current versions and the registers), and with `brief=1`
//! the open list, the open cards and permission requests as ids and version numbers.

use super::groups::group_row;
use super::items::envelope_item;
use super::json::{bad, Json, Read, MAX_NUMBER};
use super::limits::{
    CAP_DESK, CAP_LIST, MAX_DESK_GROUPS, MAX_DESK_OBJECTS, MAX_DESK_REGISTERS, MAX_OPEN,
    MAX_SETTLED_PAGE,
};
use super::request::b64;
use super::request::begin;
use super::types::{
    Desk, DeskBrief, DeskEntry, DeskObject, DeskOnly, GroupCounts, ObjectRoute, Only,
    SettledObject, SettledPage, Site, StateCounts,
};
use crate::app::io::{HttpClass, HttpRequest};
use std::collections::HashSet;
use trommi_core::envelope::{Envelope, Header, Subject};
use trommi_core::ids::DeviceId;
use trommi_core::ids::GroupId;
use trommi_core::ids::ObjectId;

pub(crate) fn request(
    site: &Site<'_>,
    since: Option<u64>,
    active: bool,
) -> Read<(HttpRequest, Option<u64>)> {
    if since.is_some_and(|s| s > MAX_NUMBER) {
        return bad("the request: since beyond any change number");
    }
    let mut build = begin(site, "GET", "/v1/desk", true, CAP_LIST)?.query("brief", 1);
    if let Some(since) = since {
        build = build.query("since", since);
    }
    if active {
        build = build.query("active", 1);
    }
    Ok((build.class(HttpClass::Background).done(), since))
}

/// The list, checked: at most [`MAX_OPEN`] rows in all, no object named twice (as open, as settled or as both),
/// no row's change number above the answer's, a row's first, version and head change in that order, and with
/// `since` none at or below it.
pub(crate) fn read(since: Option<u64>, body: &Json) -> Read<DeskBrief> {
    let o = body.obj("the open list: an object")?;
    let change = o.need("change").int("change")?;
    let truncated = o.need("truncated").bool("truncated: true or false")?;
    let mut seen: HashSet<ObjectId> = HashSet::new();
    let mut left = MAX_OPEN;
    let mut rows = |v: &Json| -> Read<Vec<DeskEntry>> {
        let list = v.list(left, "the open list: more rows than it may hold")?;
        left -= list.len();
        let mut out = Vec::with_capacity(list.len());
        for v in list {
            let r = v.obj("an open object")?;
            let entry = DeskEntry {
                object: r.need("object_id").id(ObjectId::from_slice, "object_id")?,
                version_change: r.need("version_change").count("version_change")?,
                head_change: r.need("head_change").count("head_change")?,
                first_change: match r.maybe("first_change") {
                    None => None,
                    Some(v) => Some(v.count("first_change")?),
                },
            };
            if entry.version_change > change || entry.head_change > change {
                return bad("an open object newer than the list");
            }
            if entry.version_change > entry.head_change
                || entry.first_change.is_some_and(|f| f > entry.version_change)
            {
                return bad("an open object whose change numbers are out of order");
            }
            if since.is_some_and(|since| entry.head_change <= since) {
                return bad("an open object that did not change since");
            }
            if !seen.insert(entry.object) {
                return bad("an object named twice");
            }
            out.push(entry);
        }
        Ok(out)
    };
    let cards = rows(o.need("cards"))?;
    let permission_requests = rows(o.need("permission_requests"))?;
    let settled = match since {
        None => None,
        Some(_) => {
            let list = o
                .need("settled")
                .list(left, "settled: more rows than the list may hold")?;
            let mut out = Vec::with_capacity(list.len());
            for v in list {
                let object = v.id(ObjectId::from_slice, "settled: object ids")?;
                if !seen.insert(object) {
                    return bad("an object named twice");
                }
                out.push(object);
            }
            Some(out)
        }
    };
    Ok(DeskBrief {
        cards,
        permission_requests,
        settled,
        truncated,
        change,
    })
}

// ---- GET /v1/desk[?active=1|?archived=1]: the whole Desk

pub(crate) fn whole_request(site: &Site<'_>, only: DeskOnly) -> Read<HttpRequest> {
    let build = begin(site, "GET", "/v1/desk", true, CAP_DESK)?;
    Ok(match only {
        DeskOnly::All => build,
        DeskOnly::Active => build.query("active", 1),
        DeskOnly::Archived => build.query("archived", 1),
    }
    .class(HttpClass::Background)
    .done())
}

/// What an envelope's readable header says, when these bytes are an envelope this build reads (one of a newer
/// format is the protocol core's to refuse, not a reason to drop the Desk).
fn header_of(envelope: &[u8]) -> Option<Header> {
    Envelope::decode(envelope).ok().map(|e| e.header)
}

/// One object's row as the Desk and the settled list give it, checked: its change numbers in order and not
/// above the answer's, its version an envelope of that object and group by its own (signed) header; `open`: the
/// Desk's rows are open, the settled list's are answered or closed.
pub(crate) fn object_row(v: &Json, change: u64, open: bool) -> Read<DeskObject> {
    let o = v.obj("an object of the Desk")?;
    let small = |name: &str, why: &'static str| -> Read<u8> {
        u8::try_from(o.need(name).int(why)?).or(bad(why))
    };
    let version = match o.maybe("version") {
        None => None,
        Some(v) => Some(envelope_item(v.obj("a version: an envelope item")?, false)?),
    };
    let row = DeskObject {
        object: o.need("object_id").id(ObjectId::from_slice, "object_id")?,
        group: o.need("group_id").group("an object's group_id")?,
        state: small("state", "an object's state")?,
        urgency: small("urgency", "an object's urgency")?,
        answered_at: o.need("answered_at").int("answered_at")?,
        owner: o
            .need("owner")
            .id(DeviceId::from_slice, "an object's owner")?,
        first_change: o.need("first_change").count("first_change")?,
        head_change: o.need("head_change").count("head_change")?,
        version,
    };
    if open && row.state != 1 {
        return bad("an object on the Desk that is not open");
    }
    if !open && !(row.state == 2 || row.state == 3) {
        return bad("a settled object that is not answered or closed");
    }
    if row.first_change > row.head_change || row.head_change > change {
        return bad("an object whose change numbers do not fit the Desk's");
    }
    if let Some(version) = &row.version {
        if version.change < row.first_change || version.change > row.head_change {
            return bad("a version outside its object's changes");
        }
        // the version is an envelope of this object in this group, by its own (signed) header
        if let Some(header) = header_of(&version.envelope) {
            let of_object = header
                .subject
                .object()
                .is_some_and(|f| f.object_id == row.object);
            if header.group != row.group || !of_object {
                return bad("a version that is another object's, or another group's");
            }
        }
    }
    Ok(row)
}

fn objects(v: &Json, change: u64, seen: &mut HashSet<ObjectId>) -> Read<Vec<DeskObject>> {
    let list = v.list(
        MAX_DESK_OBJECTS,
        "the Desk: more objects of a kind than it may hold",
    )?;
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        let row = object_row(v, change, true)?;
        if !seen.insert(row.object) {
            return bad("an object named twice");
        }
        out.push(row);
    }
    Ok(out)
}

fn state_counts(v: &Json) -> Read<StateCounts> {
    let o = v.obj("a kind's counts: an object")?;
    Ok(StateCounts {
        open: o.need("open").int("open: a count")?,
        answered: o.need("answered").int("answered: a count")?,
        closed: o.need("closed").int("closed: a count")?,
    })
}

/// The Desk's `counts`: at most [`MAX_DESK_GROUPS`] rows, strictly ascending by group id (so none twice), each
/// kind's three numbers. Only the hub's word beyond that: the client shows them, nothing follows from them.
fn counts(v: &Json) -> Read<Vec<GroupCounts>> {
    let list = v.list(
        MAX_DESK_GROUPS,
        "counts: more groups than the Desk may hold",
    )?;
    let mut out: Vec<GroupCounts> = Vec::with_capacity(list.len());
    for v in list {
        let o = v.obj("a group's counts: an object")?;
        let row = GroupCounts {
            group: o.need("group_id").group("counts: a group_id")?,
            cards: state_counts(o.need("cards"))?,
            permission_requests: state_counts(o.need("permission_requests"))?,
            notes: state_counts(o.need("notes"))?,
            artifacts: state_counts(o.need("artifacts"))?,
        };
        if out
            .last()
            .is_some_and(|last| last.group.as_bytes() >= row.group.as_bytes())
        {
            return bad("counts out of order, or a group twice");
        }
        out.push(row);
    }
    Ok(out)
}

/// The Desk, checked: at most [`MAX_DESK_OBJECTS`] objects of a kind, each open, none twice, its change numbers
/// in order and not above the Desk's, its version an envelope of that object and group; the registers ascending
/// by change, each a register value, no writer's register twice.
///
/// What stays the hub's claim, and nothing to rely on: that the list is every open object (`truncated` says
/// when it is not, but a hub can also just leave one out), an object's `state`, `urgency`, `owner` and its
/// change numbers beyond their order, and that a version is the newest. The protocol core decides what an
/// envelope is; the catch-up brings everything again in the hub's order.
pub(crate) fn whole(body: &Json) -> Read<Desk> {
    let o = body.obj("the Desk: an object")?;
    let change = o.need("change").int("the Desk's change")?;
    let truncated = o.need("truncated").bool("truncated: true or false")?;
    let mut seen = HashSet::new();
    let cards = objects(o.need("cards"), change, &mut seen)?;
    let notes = objects(o.need("notes"), change, &mut seen)?;
    let permission_requests = objects(o.need("permission_requests"), change, &mut seen)?;
    let artifacts = objects(o.need("artifacts"), change, &mut seen)?;
    let list = o.need("registers").list(MAX_DESK_REGISTERS, "registers")?;
    let mut registers = Vec::with_capacity(list.len());
    let mut values = HashSet::new();
    let mut at = 0;
    for v in list {
        let item = envelope_item(v.obj("a register: an envelope item")?, false)?;
        if item.change <= at || item.change > change {
            return bad("registers out of order, or newer than the Desk");
        }
        at = item.change;
        if let Some(header) = header_of(&item.envelope) {
            let Subject::Register(register) = header.subject else {
                return bad("a register that is no register value");
            };
            if !values.insert((header.group, header.sender, register)) {
                return bad("two values of one writer's register");
            }
        }
        registers.push(item);
    }
    let groups = match o.maybe("groups") {
        None => None,
        Some(v) => {
            let list = v.list(MAX_DESK_GROUPS, "the Desk's groups")?;
            let mut seen = HashSet::new();
            let mut rows = Vec::with_capacity(list.len());
            for v in list {
                let row = group_row(v, None)?;
                if !seen.insert(row.group) {
                    return bad("a group twice");
                }
                rows.push(row);
            }
            Some(rows)
        }
    };
    let counts = match o.maybe("counts") {
        None => None,
        Some(v) => Some(counts(v)?),
    };
    // a whole Desk lists every open object it counts, group by group (cut short, it lists fewer)
    if let (Some(counts), false) = (&counts, truncated) {
        let mut listed: std::collections::HashMap<&[u8], [u64; 4]> =
            std::collections::HashMap::new();
        for (kind, rows) in [&cards, &permission_requests, &notes, &artifacts]
            .into_iter()
            .enumerate()
        {
            for row in rows {
                listed.entry(row.group.as_bytes()).or_default()[kind] += 1;
            }
        }
        for row in counts {
            let open = [
                row.cards.open,
                row.permission_requests.open,
                row.notes.open,
                row.artifacts.open,
            ];
            if listed.remove(row.group.as_bytes()).unwrap_or_default() != open {
                return bad("counts that do not match the open objects listed");
            }
        }
        if !listed.is_empty() {
            return bad("counts that do not match the open objects listed");
        }
    }
    Ok(Desk {
        cards,
        notes,
        permission_requests,
        artifacts,
        registers,
        groups,
        counts,
        truncated,
        change,
    })
}

// ---- GET /v1/objects?state=settled: the objects no longer open, a page at a time

/// What a page of the settled objects was asked for: its answer is checked against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SettledAsked {
    kind: Option<ObjectRoute>,
    group: Option<GroupId>,
    before: Option<u64>,
    limit: u32,
}

const KINDS: [(&str, ObjectRoute); 4] = [
    ("cards", ObjectRoute::Cards),
    ("notes", ObjectRoute::Notes),
    ("permission_requests", ObjectRoute::PermissionRequests),
    ("artifacts", ObjectRoute::Artifacts),
];

fn kind_word(kind: ObjectRoute) -> &'static str {
    KINDS
        .iter()
        .find(|(_, k)| *k == kind)
        .map(|(w, _)| *w)
        .unwrap_or("cards")
}

pub(crate) fn settled_request(
    site: &Site<'_>,
    kind: Option<ObjectRoute>,
    only: Only<'_>,
    before: Option<u64>,
    limit: u32,
) -> Read<(HttpRequest, SettledAsked)> {
    if before.is_some_and(|b| b == 0 || b > MAX_NUMBER) {
        return bad("the request: before is a change number");
    }
    let limit = limit.clamp(1, MAX_SETTLED_PAGE);
    let mut build = begin(site, "GET", "/v1/objects", true, CAP_LIST)?
        .query("state", "settled")
        .query("limit", limit);
    if let Some(kind) = kind {
        build = build.query("kind", kind_word(kind));
    }
    if let Some(before) = before {
        build = build.query("before", before);
    }
    let group = match only {
        Only::All => None,
        Only::Active => {
            build = build.query("active", 1);
            None
        }
        Only::Group(group) => {
            build = build.query("group", b64(group.as_bytes()));
            Some(*group)
        }
    };
    Ok((
        build.class(HttpClass::Background).done(),
        SettledAsked {
            kind,
            group,
            before,
            limit,
        },
    ))
}

/// A page of the settled objects, checked: at most the rows asked for, each answered or closed, of the kind and
/// group asked for, strictly newest first by `head_change` and below the `before` asked, none twice, each row
/// checked as the Desk's; with `more` the next `before` lies below the one asked and not above the last row's
/// `head_change`, without it is 0. That a row is the newest version, and that no row was left out, stay the
/// hub's word.
pub(crate) fn settled(asked: &SettledAsked, body: &Json) -> Read<SettledPage> {
    let o = body.obj("the settled objects: an object")?;
    let change = o.need("change").int("change")?;
    let more = o.need("more").bool("more: true or false")?;
    let before = o.need("before").int("before")?;
    let list = o
        .need("items")
        .list(asked.limit as usize, "items: at most the number asked for")?;
    let mut items = Vec::with_capacity(list.len());
    let mut seen = HashSet::new();
    let mut below = asked.before.unwrap_or(u64::MAX);
    for v in list {
        let kind = v
            .obj("a settled object")?
            .need("kind")
            .one_of(&KINDS, "a settled object's kind")?;
        let row = object_row(v, change, false)?;
        if asked.kind.is_some_and(|k| k != kind) {
            return bad("a settled object of another kind than asked");
        }
        if asked.group.is_some_and(|g| g != row.group) {
            return bad("a settled object of another group than asked");
        }
        if row.head_change >= below {
            return bad("settled objects out of order");
        }
        below = row.head_change;
        if !seen.insert(row.object) {
            return bad("an object named twice");
        }
        items.push(SettledObject { kind, row });
    }
    let next_ok = if more {
        before >= 1 && before <= below && asked.before.is_none_or(|b| before < b)
    } else {
        before == 0
    };
    if !next_ok {
        return bad("a cursor that does not lie where the page ends");
    }
    Ok(SettledPage {
        items,
        more,
        before,
        change,
    })
}
