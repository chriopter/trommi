//! `GET /v1/changes?after=&limit=[&active=1|&group=]`: catch-up, everything the asker may see above a change
//! number, in the hub's one order (hub-api.md "Decided" 19, 32, 46).

use super::items::change_item;
use super::json::{bad, Json, Read, MAX_NUMBER};
use super::limits::{CAP_LIST, CHANGES_WINDOW, MAX_CHANGES};
use super::request::{b64, begin};
use super::types::{ChangeItem, ChangesPage, Only, Site};
use crate::app::io::{HttpClass, HttpRequest};

/// What a page was asked with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    pub after: u64,
    pub limit: u32,
    /// `group=`: one group's envelopes alone.
    pub one_group: bool,
}

pub(crate) fn request(
    site: &Site<'_>,
    after: u64,
    limit: u32,
    only: Only<'_>,
) -> Read<(HttpRequest, Asked)> {
    if after > MAX_NUMBER {
        return bad("the request: a cursor beyond any change number");
    }
    let limit = limit.clamp(1, MAX_CHANGES);
    let build = begin(site, "GET", "/v1/changes", true, CAP_LIST)?
        .query("after", after)
        .query("limit", limit);
    let build = match only {
        Only::All => build,
        Only::Active => build.query("active", 1),
        Only::Group(group) => build.query("group", b64(group.as_bytes())),
    };
    let asked = Asked {
        after,
        limit,
        one_group: matches!(only, Only::Group(_)),
    };
    Ok((build.class(HttpClass::Background).done(), asked))
}

/// The page, checked against what was asked: the cursor moves forward by at most the hub's window, the items
/// ascend strictly, all above the cursor asked from and none above the answer's, at most the number asked for.
pub(crate) fn read(asked: &Asked, body: &Json) -> Read<ChangesPage> {
    let o = body.obj("a catch-up answer: an object")?;
    let change = o.need("change").int("change: the cursor")?;
    let more = o.need("more").bool("more: true or false")?;
    if change < asked.after || change - asked.after > CHANGES_WINDOW {
        return bad("a cursor outside the stretch asked for");
    }
    if more && change == asked.after {
        return bad("more to come, and no step forward");
    }
    let list = o
        .need("items")
        .list(asked.limit as usize, "items: at most the number asked for")?;
    let mut items = Vec::with_capacity(list.len());
    let mut at = asked.after;
    for v in list {
        let item = change_item(v)?;
        if item.change() <= at || item.change() > change {
            return bad("changes out of order, or outside the stretch answered");
        }
        if asked.one_group && !matches!(item, ChangeItem::Envelope(_)) {
            return bad("a log entry among one group's envelopes");
        }
        at = item.change();
        items.push(item);
    }
    let head = o
        .maybe("head")
        .map(|v| v.int("head: a change number"))
        .transpose()?;
    if head.is_some_and(|head| head < change) {
        return bad("head: below the cursor");
    }
    Ok(ChangesPage {
        items,
        change,
        more,
        head,
    })
}
