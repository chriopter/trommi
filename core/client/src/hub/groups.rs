//! The MLS delivery service's reads and the writes beside the outbox: Welcomes, a group's log, GroupInfos, the
//! room's groups, KeyPackages claimed, several joins in one request, wishes to the human devices.

use super::items::log_item;
use super::json::{bad, Json, Obj, Read, MAX_NUMBER};
use super::limits::{
    CAP_DESK, CAP_GROUPS_READ, CAP_GROUP_INFO, CAP_LIST, CAP_WRITE, LOG_PAGE, MAX_AT_ONCE,
    MAX_CLAIM, MAX_GROUPS_PAGE, MAX_KEY_PACKAGE, MAX_LEAVES, MAX_LOG, MAX_MLS, MAX_REMOVAL,
    MAX_SEALED_KEYS, MAX_SMALL_STRUCT, MAX_WELCOMES, MAX_WISHES,
};
use super::outbox::{commit_body, committed};
use super::refusal;
use super::request::{b64, begin};
use super::stream::wish_row;
use super::types::{
    Answer, ChangeItem, GroupRead, GroupRow, GroupRowKind, LogItem, OutboxAnswer, RecoveryLinkRow,
    SealedKeysPage, Site, WelcomeRow, WishKind,
};
use super::Failure;
use crate::app::io::{HttpClass, HttpRequest};
use serde_json::{json, Value};
use std::collections::HashSet;
use trommi_core::ids::{DeviceId, GroupId, RoomId, SessionId};
use trommi_core::store::{OutboxEntry, OutboxKind};

fn cursor(n: u64) -> Read<u64> {
    if n > MAX_NUMBER {
        return bad("the request: a cursor beyond any number of the protocol");
    }
    Ok(n)
}

fn group_path(group: &GroupId, rest: &str) -> String {
    format!("/v1/groups/{}/{rest}", b64(group.as_bytes()))
}

// ---- GET /v1/welcomes?after=

pub(crate) fn welcomes_request(site: &Site<'_>, after: u64) -> Read<HttpRequest> {
    Ok(begin(site, "GET", "/v1/welcomes", true, CAP_LIST)?
        .query("after", cursor(after)?)
        .class(HttpClass::Background)
        .done())
}

/// The Welcomes for the asking device, oldest first: their numbers rise strictly and lie above the one asked
/// from. The device asks again after the last one's `id` until an answer is empty.
pub(crate) fn welcomes(after: u64, body: &Json) -> Read<Answer> {
    let list = body.list(MAX_WELCOMES, "welcomes: a list")?;
    let mut out = Vec::with_capacity(list.len());
    let mut at = after;
    for v in list {
        let o = v.obj("a welcome: an object")?;
        let row = WelcomeRow {
            id: o.need("id").count("a welcome's id")?,
            group: o.need("group_id").group("a welcome's group_id")?,
            welcome: o.need("welcome").bytes(1, MAX_MLS, "welcome")?.0,
            at: o.need("at").int("a welcome's at")?,
        };
        if row.id <= at {
            return bad("Welcomes out of order");
        }
        at = row.id;
        out.push(row);
    }
    Ok(Answer::Welcomes(out))
}

// ---- GET /v1/groups/{group}/log?after=&limit=[&kind=commit]

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LogAsked {
    pub group: GroupId,
    pub after: u64,
    pub limit: u32,
    pub commits_only: bool,
}

pub(crate) fn log_request(
    site: &Site<'_>,
    group: &GroupId,
    after: u64,
    limit: u32,
    commits_only: bool,
) -> Read<(HttpRequest, LogAsked)> {
    let limit = limit.clamp(1, MAX_LOG);
    let mut build = begin(site, "GET", &group_path(group, "log"), true, CAP_LIST)?
        .query("after", cursor(after)?)
        .query("limit", limit);
    if commits_only {
        build = build.query("kind", "commit");
    }
    let asked = LogAsked {
        group: *group,
        after,
        limit,
        commits_only,
    };
    Ok((build.class(HttpClass::Background).done(), asked))
}

/// The entries of one log page: the group's own, their numbers rising strictly above `after` (and their change
/// numbers with them), Commits only where only those were asked for, at most `limit`.
fn log_page(
    v: &Json,
    group: &GroupId,
    after: u64,
    limit: usize,
    commits_only: bool,
) -> Read<(Vec<(LogItem, bool)>, bool)> {
    let o = v.obj("a log page: an object")?;
    let more = o.need("more").bool("a log page's more")?;
    let list = o
        .need("items")
        .list(limit, "a log page: at most the entries asked for")?;
    let mut items: Vec<(LogItem, bool)> = Vec::with_capacity(list.len());
    let (mut n, mut change) = (after, 0);
    for v in list {
        let (item, commit) = log_item(v.obj("a log entry: an object")?)?;
        if item.group != *group || item.n <= n || item.change <= change || (commits_only && !commit)
        {
            return bad("a log entry out of place");
        }
        (n, change) = (item.n, item.change);
        items.push((item, commit));
    }
    if more && items.is_empty() {
        return bad("more to come, and no step forward");
    }
    Ok((items, more))
}

pub(crate) fn log(asked: &LogAsked, body: &Json) -> Read<Answer> {
    let (items, more) = log_page(
        body,
        &asked.group,
        asked.after,
        asked.limit as usize,
        asked.commits_only,
    )?;
    let items = items
        .into_iter()
        .map(|(item, commit)| match commit {
            true => ChangeItem::Commit(item),
            false => ChangeItem::Message(item),
        })
        .collect();
    Ok(Answer::GroupLog { items, more })
}

// ---- GET /v1/groups/{group}/info[?epoch=]

pub(crate) fn info_request(
    site: &Site<'_>,
    group: &GroupId,
    epoch: Option<u64>,
) -> Read<HttpRequest> {
    let mut build = begin(
        site,
        "GET",
        &group_path(group, "info"),
        true,
        CAP_GROUP_INFO,
    )?;
    if let Some(epoch) = epoch {
        build = build.query("epoch", cursor(epoch)?);
    }
    Ok(build.class(HttpClass::Background).done())
}

/// `{ epoch, group_info }`; of an epoch asked for, exactly that one.
fn info_of(o: Obj<'_>, asked: Option<u64>) -> Read<(u64, Vec<u8>)> {
    let epoch = o.need("epoch").int("a GroupInfo's epoch")?;
    if asked.is_some_and(|asked| asked != epoch) {
        return bad("another epoch than the one asked for");
    }
    Ok((
        epoch,
        o.need("group_info").bytes(1, MAX_MLS, "group_info")?.0,
    ))
}

pub(crate) fn info(asked: Option<u64>, body: &Json) -> Read<Answer> {
    let (epoch, group_info) = info_of(body.obj("a GroupInfo answer: an object")?, asked)?;
    Ok(Answer::GroupInfo { epoch, group_info })
}

// ---- an answer in the place of one of several (POST /v1/commits, POST /v1/groups/read)

/// The refusal the hub put in the place of one answer, as the failure a request of its own would have met. A
/// member `error` that is no refusal of this protocol under its own status makes the whole answer a bad one.
fn refused_in_place(v: &Json, o: Obj<'_>) -> Read<Option<Failure>> {
    if !o.has("error") {
        return Ok(None);
    }
    let status = match o.need("status").int("a refusal's status")? {
        s @ 400..=599 => s as u16,
        _ => return bad("a refusal's status"),
    };
    let Some(refusal) = refusal::from_members(v, status, None) else {
        return bad("a refusal in an answer's place that is none of this protocol");
    };
    Ok(Some(if status == 408 || status == 429 || status >= 500 {
        Failure::NotNow {
            status,
            retry_after_ms: refusal.retry_after_ms,
        }
    } else {
        Failure::Refused {
            status,
            code: Some(refusal.code.to_owned()),
        }
    }))
}

// ---- POST /v1/commits

/// Joins of session groups from outside, one per group, in one request. Checked against: how many, and the
/// epoch each was built on.
pub(crate) fn joins_request(
    site: &Site<'_>,
    entries: &[OutboxEntry],
) -> Read<(HttpRequest, Vec<u64>)> {
    if entries.is_empty() || entries.len() > MAX_AT_ONCE {
        return bad("the request: 1 to 64 joins in one request");
    }
    let mut groups = HashSet::new();
    let mut commits = Vec::with_capacity(entries.len());
    for entry in entries {
        let Some(group) = entry
            .group
            .filter(|_| entry.kind == OutboxKind::ExternalCommit && entry.parts.len() == 4)
        else {
            return bad("the request: joins of groups from outside only");
        };
        if !groups.insert(group) {
            return bad("the request: one join per group");
        }
        let mut body = commit_body(entry, 0, 1, 2, None, Some(3))?;
        if !body.contains_key("recovery_auth") {
            return bad("the request: an outbox entry lacks a part");
        }
        body.insert("group".to_owned(), json!(b64(group.as_bytes())));
        commits.push(Value::Object(body));
    }
    let request = begin(site, "POST", "/v1/commits", true, CAP_WRITE)?
        .json(&json!({ "commits": commits }))
        .done();
    Ok((request, entries.iter().map(|e| e.epoch).collect()))
}

/// One answer per join, in the order sent: the Commit's answer, or the hub's refusal of it.
pub(crate) fn joins(epochs: &[u64], body: &Json) -> Read<Answer> {
    let o = body.obj("the answers to Commits: an object")?;
    let list = o
        .need("answers")
        .list(epochs.len(), "answers: one per Commit")?;
    if list.len() != epochs.len() {
        return bad("answers: one per Commit");
    }
    let mut out: Vec<Result<OutboxAnswer, Failure>> = Vec::with_capacity(list.len());
    for (v, built_on) in list.iter().zip(epochs) {
        let o = v.obj("an answer to a Commit: an object")?;
        out.push(match refused_in_place(v, o)? {
            Some(failure) => Err(failure),
            None => Ok(committed(o, *built_on)?),
        });
    }
    Ok(Answer::Joins(out))
}

// ---- POST /v1/groups/read

pub(crate) fn read_request(
    site: &Site<'_>,
    groups: &[GroupId],
) -> Read<(HttpRequest, Vec<GroupId>)> {
    if groups.is_empty() || groups.len() > MAX_AT_ONCE {
        return bad("the request: 1 to 64 groups in one request");
    }
    let distinct: HashSet<&GroupId> = groups.iter().collect();
    if distinct.len() != groups.len() {
        return bad("the request: a group once");
    }
    let ids: Vec<String> = groups.iter().map(|g| b64(g.as_bytes())).collect();
    let request = begin(site, "POST", "/v1/groups/read", true, CAP_GROUPS_READ)?
        .json(&json!({ "groups": ids }))
        .class(HttpClass::Background)
        .done();
    Ok((request, groups.to_vec()))
}

/// One answer per group, in the order asked (so no group twice, none that was not asked for): its current
/// GroupInfo, its founding one, and its Commits from the first: one per epoch, none at or beyond the current
/// epoch. Where the first page does not reach the current epoch the log must say `more`, and the group is
/// marked not `complete`. The whole answer is bounded by its size cap, whatever a hub puts in.
pub(crate) fn read(asked: &[GroupId], body: &Json) -> Read<Answer> {
    let o = body.obj("groups read: an object")?;
    let list = o
        .need("groups")
        .list(asked.len(), "groups: one per group asked")?;
    if list.len() != asked.len() {
        return bad("groups: one per group asked");
    }
    let mut out: Vec<Result<GroupRead, Failure>> = Vec::with_capacity(list.len());
    for (v, group) in list.iter().zip(asked) {
        let o = v.obj("a group read: an object")?;
        if o.need("group").group("a group read's group")? != *group {
            return bad("the groups in another order than asked");
        }
        if let Some(failure) = refused_in_place(v, o)? {
            out.push(Err(failure));
            continue;
        }
        let (epoch, info) = info_of(o.need("info").obj("a GroupInfo answer: an object")?, None)?;
        let founding = match (epoch, o.maybe("founding")) {
            (0, None) => None,
            (0, Some(_)) => return bad("a founding GroupInfo beside the one of epoch 0"),
            (_, None) => return bad("no founding GroupInfo"),
            (_, Some(f)) => Some(info_of(f.obj("a GroupInfo answer: an object")?, Some(0))?.1),
        };
        let (items, more) = log_page(o.need("log"), group, 0, LOG_PAGE, true)?;
        let mut log = Vec::with_capacity(items.len());
        for (at, (item, _)) in items.into_iter().enumerate() {
            if item.epoch != at as u64 || item.epoch >= epoch {
                return bad("a log that is not one Commit per epoch up to the current one");
            }
            log.push(item);
        }
        let complete = log.len() as u64 == epoch;
        if !complete && !more {
            return bad("a log that does not reach the current epoch");
        }
        out.push(Ok(GroupRead {
            group: *group,
            info,
            epoch,
            founding,
            log,
            complete,
        }));
    }
    Ok(Answer::GroupsRead(out))
}

// ---- POST /v1/key-packages/claim

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClaimAsked {
    pub devices: Vec<DeviceId>,
    pub partial: bool,
}

/// NOT repeatable: a claim that was not answered used its KeyPackages up.
pub(crate) fn claim_request(
    site: &Site<'_>,
    devices: &[DeviceId],
    partial: bool,
) -> Read<(HttpRequest, ClaimAsked)> {
    if devices.is_empty() || devices.len() > MAX_CLAIM {
        return bad("the request: 1 to 1024 devices in one claim");
    }
    let distinct: HashSet<&DeviceId> = devices.iter().collect();
    if distinct.len() != devices.len() {
        return bad("the request: a claim names a device once");
    }
    let ids: Vec<String> = devices.iter().map(|d| b64(d.as_bytes())).collect();
    let body = match partial {
        true => json!({ "devices": ids, "partial": true }),
        false => json!({ "devices": ids }),
    };
    let request = begin(site, "POST", "/v1/key-packages/claim", true, CAP_DESK)?
        .json(&body)
        .done();
    let asked = ClaimAsked {
        devices: devices.to_vec(),
        partial,
    };
    Ok((request, asked))
}

/// One KeyPackage of each device asked, in the order asked, and of no other. With `partial`, the devices the
/// hub holds none of are named in `missing` instead: every device asked is in exactly one of the two.
pub(crate) fn claimed(asked: &ClaimAsked, body: &Json) -> Read<Answer> {
    let o = body.obj("a claim answer: an object")?;
    let got = o.need("key_packages").obj("key_packages: an object")?;
    let mut missing = Vec::new();
    let mut gone = HashSet::new();
    if asked.partial {
        let of_asked: HashSet<&DeviceId> = asked.devices.iter().collect();
        for v in o
            .need("missing")
            .list(asked.devices.len(), "missing: device ids")?
        {
            let device = v.id(DeviceId::from_slice, "missing: device ids")?;
            if !of_asked.contains(&device) || !gone.insert(device) {
                return bad("missing: a device that was not asked for, or twice");
            }
            missing.push(device);
        }
    } else if o.maybe("missing").is_some() {
        return bad("missing: only a partial claim has it");
    }
    if got.members().len() + missing.len() != asked.devices.len() {
        return bad("KeyPackages of other devices than the ones asked for");
    }
    let mut key_packages = Vec::with_capacity(got.members().len());
    for device in &asked.devices {
        // (the name is the device's id in its one spelling: any other name is no device asked for)
        let name = b64(device.as_bytes());
        match (got.has(&name), gone.contains(device)) {
            (true, false) => {
                let bytes = got.need(&name).bytes(1, MAX_KEY_PACKAGE, "a KeyPackage")?.0;
                key_packages.push((*device, bytes));
            }
            (false, true) => {}
            _ => return bad("KeyPackages of other devices than the ones asked for"),
        }
    }
    Ok(Answer::Claimed {
        key_packages,
        missing,
    })
}

// ---- GET /v1/rooms/{room}/groups?after=&limit=

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GroupsAsked {
    pub room: RoomId,
    pub after: u64,
    pub limit: u32,
}

pub(crate) fn groups_request(
    site: &Site<'_>,
    room: &RoomId,
    after: u64,
    limit: u32,
) -> Read<(HttpRequest, GroupsAsked)> {
    let limit = limit.clamp(1, MAX_GROUPS_PAGE);
    let path = format!("/v1/rooms/{}/groups", b64(room.as_bytes()));
    let request = begin(site, "GET", &path, true, CAP_LIST)?
        .query("after", cursor(after)?)
        .query("limit", limit)
        .class(HttpClass::Background)
        .done();
    let asked = GroupsAsked {
        room: *room,
        after,
        limit,
    };
    Ok((request, asked))
}

pub(crate) fn group_row(v: &Json, room: Option<&RoomId>) -> Read<GroupRow> {
    let o = v.obj("a group: an object")?;
    let group = o.need("group_id").group("a group's group_id")?;
    let kind = o.need("kind").one_of(
        &[
            ("room", GroupRowKind::Room),
            ("main", GroupRowKind::Main),
            ("helper", GroupRowKind::Helper),
        ],
        "a group's kind",
    )?;
    let session = o
        .maybe("session_id")
        .map(|v| v.id(SessionId::from_slice, "a group's session_id"))
        .transpose()?;
    let parent = o
        .maybe("parent")
        .map(|v| v.id(SessionId::from_slice, "a group's parent"))
        .transpose()?
        .filter(|p| !p.is_zero());
    // the row is of the room asked for, and what it says of itself is what its id says
    if room.is_some_and(|room| group.room_id() != *room)
        || (kind == GroupRowKind::Room) != group.is_room()
        || session.is_some_and(|s| group.session_id() != Some(s))
        || (kind == GroupRowKind::Helper) != parent.is_some()
    {
        return bad("a group that is not what its id says");
    }
    let mut seen = HashSet::new();
    let mut leaves = Vec::new();
    for v in o.need("leaves").list(MAX_LEAVES, "a group's leaves")? {
        let leaf = v.id(DeviceId::from_slice, "a leaf")?;
        if !seen.insert(leaf) {
            return bad("a leaf twice");
        }
        leaves.push(leaf);
    }
    Ok(GroupRow {
        group,
        kind,
        parent,
        epoch: o.need("epoch").int("a group's epoch")?,
        room_epoch: o.need("room_epoch").int("a group's room_epoch")?,
        live: o.need("live").bool("a group's live")?,
        stale: o.need("stale").bool("a group's stale")?,
        leaves,
    })
}

/// One page of the room's groups, in the order of founding: at most `limit` rows, no group twice, the cursor
/// for the next page not behind the one asked from and moved when there is `more`.
pub(crate) fn groups(asked: &GroupsAsked, body: &Json) -> Read<Answer> {
    let o = body.obj("a page of groups: an object")?;
    let more = o.need("more").bool("more")?;
    let after = o.need("after").int("after: the cursor")?;
    if after < asked.after || (more && after == asked.after) {
        return bad("a cursor that does not move forward");
    }
    let list = o
        .need("items")
        .list(asked.limit as usize, "items: at most the number asked for")?;
    let mut seen = HashSet::new();
    let mut items = Vec::with_capacity(list.len());
    for v in list {
        let row = group_row(v, Some(&asked.room))?;
        if !seen.insert(row.group) {
            return bad("a group twice");
        }
        items.push(row);
    }
    Ok(Answer::RoomGroups { items, more, after })
}

// ---- POST /v1/requests, GET /v1/requests, POST /v1/groups/{group}/reject

/// An unsigned wish of this device to the human devices. Not repeatable: a second arrival is a second wish.
pub(crate) fn wish_request(
    site: &Site<'_>,
    kind: WishKind,
    group: Option<&GroupId>,
    key_package: Option<&[u8]>,
) -> Read<HttpRequest> {
    let mut body = json!({ "kind": match kind {
        WishKind::Readmit => "readmit",
        WishKind::Handover => "handover",
        WishKind::Session => "session",
    } });
    if let Some(group) = group {
        body["group"] = json!(b64(group.as_bytes()));
    }
    if let Some(key_package) = key_package {
        if key_package.is_empty() || key_package.len() > MAX_KEY_PACKAGE {
            return bad("the request: a KeyPackage of at most 16 KiB");
        }
        body["key_package"] = json!(b64(key_package));
    }
    Ok(begin(site, "POST", "/v1/requests", true, CAP_WRITE)?
        .json(&body)
        .done())
}

pub(crate) fn wished(body: &Json) -> Read<u64> {
    body.obj("a request answer: an object")?
        .need("id")
        .int("a request's id")
}

pub(crate) fn wishes_request(site: &Site<'_>) -> Read<HttpRequest> {
    Ok(begin(site, "GET", "/v1/requests", true, CAP_LIST)?
        .class(HttpClass::Background)
        .done())
}

/// Every wish the asker may see: no id twice.
pub(crate) fn wishes(body: &Json) -> Read<Answer> {
    let list = body.list(MAX_WISHES, "requests: a list")?;
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for v in list {
        let o = v.obj("a request: an object")?;
        let row = wish_row(o)?;
        if row.at.is_none() || !seen.insert(row.id) {
            return bad("a request without its time, or twice");
        }
        out.push(row);
    }
    Ok(Answer::Wishes(out))
}

/// `POST /v1/groups/{group}/reject` (14.7): this leaf cannot merge the accepted Commit numbered `n`.
pub(crate) fn reject_request(site: &Site<'_>, group: &GroupId, n: u64) -> Read<HttpRequest> {
    if n == 0 || n > MAX_NUMBER {
        return bad("the request: the log number of a Commit");
    }
    Ok(
        begin(site, "POST", &group_path(group, "reject"), true, CAP_WRITE)?
            .json(&json!({ "n": n }))
            .done(),
    )
}

// ---- GET /v1/sealed-keys?after=&limit=

pub(crate) fn sealed_keys_request(
    site: &Site<'_>,
    after: u64,
    limit: u32,
) -> Read<(HttpRequest, (u64, u32))> {
    let limit = limit.clamp(1, MAX_SEALED_KEYS);
    let request = begin(site, "GET", "/v1/sealed-keys", true, CAP_LIST)?
        .query("after", cursor(after)?)
        .query("limit", limit)
        .class(HttpClass::Background)
        .done();
    Ok((request, (after, limit)))
}

/// The SealedKeys and RecoveryLinks above `after`, each list ascending by change. The cursor is the newest
/// change served and no further (one that ran ahead would skip keys), and moves when there is `more`.
pub(crate) fn sealed_keys(after: u64, limit: u32, body: &Json) -> Read<Answer> {
    let o = body.obj("sealed keys: an object")?;
    let change = o.need("change").int("change: the cursor")?;
    let more = o.need("more").bool("more")?;
    let mut rows = Vec::new();
    let mut at = after;
    for v in o
        .need("rows")
        .list(limit as usize, "rows: at most the number asked for")?
    {
        let r = v.obj("a row: an object")?;
        let row_change = r.need("change").count("a row's change")?;
        if row_change <= at {
            return bad("sealed keys out of order");
        }
        at = row_change;
        rows.push((
            row_change,
            r.need("sealed_key")
                .bytes(1, MAX_SMALL_STRUCT, "sealed_key")?
                .0,
        ));
    }
    let mut links = Vec::new();
    let mut link_at = after;
    for v in o
        .need("links")
        .list(limit as usize, "links: at most the number asked for")?
    {
        let r = v.obj("a link: an object")?;
        let row = RecoveryLinkRow {
            room_epoch: r.need("room_epoch").int("a link's room_epoch")?,
            change: r.need("change").count("a link's change")?,
            recovery_link: r
                .need("recovery_link")
                .bytes(1, MAX_SMALL_STRUCT, "recovery_link")?
                .0,
        };
        if row.change <= link_at {
            return bad("recovery links out of order");
        }
        link_at = row.change;
        links.push(row);
    }
    if change != at.max(link_at) || (more && change == after) {
        return bad("a cursor that is not where the lists end");
    }
    Ok(Answer::SealedKeys(SealedKeysPage {
        rows,
        links,
        change,
        more,
    }))
}

// ---- GET /v1/groups/{group}/removal?after=

pub(crate) fn removal_request(
    site: &Site<'_>,
    group: &GroupId,
    after: u64,
) -> Read<(HttpRequest, LogAsked)> {
    let request = begin(site, "GET", &group_path(group, "removal"), true, CAP_LIST)?
        .query("after", cursor(after)?)
        .class(HttpClass::Background)
        .done();
    let asked = LogAsked {
        group: *group,
        after,
        limit: MAX_REMOVAL as u32,
        commits_only: true,
    };
    Ok((request, asked))
}

/// The group's Commits after `after`, up to and including the one that removed the asking key, whose log
/// number is `removed_at`: none beyond it, and the last page ends on it.
pub(crate) fn removal(asked: &LogAsked, body: &Json) -> Read<Answer> {
    let removed_at = body
        .obj("a removal page: an object")?
        .need("removed_at")
        .count("removed_at")?;
    let (items, more) = log_page(body, &asked.group, asked.after, MAX_REMOVAL, true)?;
    let items: Vec<LogItem> = items.into_iter().map(|(item, _)| item).collect();
    let last = items.last().map_or(asked.after, |i| i.n);
    if last > removed_at || (!more && last != removed_at) {
        return bad("a removal that does not end on the removing Commit");
    }
    Ok(Answer::Removal {
        items,
        more,
        removed_at,
    })
}

// ---- POST /v1/rooms/{room}/recovery, DELETE /v1/rooms/{room}/recovery/{id}

pub(crate) fn open_recovery_request(site: &Site<'_>, room: &RoomId) -> Read<HttpRequest> {
    let path = format!("/v1/rooms/{}/recovery", b64(room.as_bytes()));
    Ok(begin(site, "POST", &path, true, CAP_WRITE)?
        .json(&json!({}))
        .done())
}

pub(crate) fn recovery_opened(body: &Json) -> Read<Answer> {
    let o = body.obj("a recovery: an object")?;
    Ok(Answer::Recovery {
        id: o.need("recovery_id").fixed("recovery_id: 16 bytes")?,
        expires_at: o.need("expires_at").int("a recovery's expires_at")?,
    })
}

pub(crate) fn drop_recovery_request(
    site: &Site<'_>,
    room: &RoomId,
    recovery: &[u8; 16],
) -> Read<HttpRequest> {
    let path = format!(
        "/v1/rooms/{}/recovery/{}",
        b64(room.as_bytes()),
        b64(recovery)
    );
    Ok(begin(site, "DELETE", &path, true, CAP_WRITE)?.done())
}

pub(crate) fn recovery_dropped(body: &Json) -> Read<Answer> {
    let o = body.obj("a dropped recovery: an object")?;
    Ok(Answer::Dropped {
        dropped: o.need("dropped").bool("dropped")?,
    })
}
