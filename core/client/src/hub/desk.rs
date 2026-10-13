//! `GET /v1/desk?brief=1[&since=][&active=1]`: the Desk's open list, the open cards and permission requests as
//! ids and version numbers.

use super::json::{bad, Json, Read, MAX_NUMBER};
use super::limits::{CAP_LIST, MAX_OPEN};
use super::request::begin;
use super::types::{DeskBrief, DeskEntry, Site};
use crate::app::io::{HttpClass, HttpRequest};
use std::collections::HashSet;
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
/// no row's change number above the answer's, and with `since` none at or below it.
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
            };
            if entry.version_change > change || entry.head_change > change {
                return bad("an open object newer than the list");
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
