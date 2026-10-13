//! Reads of stored content beside the catch-up: a sender's chain and every envelope of one object.

use super::items::envelope_item;
use super::json::{bad, Json, Read, MAX_NUMBER};
use super::limits::{CAP_LIST, MAX_PAGE};
use super::request::{b64, begin};
use super::types::{Answer, ChainItem, ObjectEnvelopes, ObjectRoute, Site};
use crate::app::io::{HttpClass, HttpRequest};
use trommi_core::ids::{DeviceId, GroupId, ObjectId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PageAsked {
    pub after: u64,
    pub limit: u32,
    /// The object asked for; none for a chain.
    pub object: Option<ObjectId>,
}

fn page(
    site: &Site<'_>,
    path: &str,
    after: u64,
    limit: u32,
    object: Option<ObjectId>,
) -> Read<(HttpRequest, PageAsked)> {
    if after > MAX_NUMBER {
        return bad("the request: a cursor beyond any number of the protocol");
    }
    let limit = limit.clamp(1, MAX_PAGE);
    let request = begin(site, "GET", path, true, CAP_LIST)?
        .query("after", after)
        .query("limit", limit)
        .class(HttpClass::Background)
        .done();
    Ok((
        request,
        PageAsked {
            after,
            limit,
            object,
        },
    ))
}

/// `GET /v1/groups/{group}/chains/{sender}?after=&limit=`.
pub(crate) fn chain_request(
    site: &Site<'_>,
    group: &GroupId,
    sender: &DeviceId,
    after: u64,
    limit: u32,
) -> Read<(HttpRequest, PageAsked)> {
    let path = format!(
        "/v1/groups/{}/chains/{}",
        b64(group.as_bytes()),
        b64(sender.as_bytes())
    );
    page(site, &path, after, limit, None)
}

/// A sender's envelopes in pruned form: their chain numbers rise strictly above the one asked from; those
/// beyond the sender's Cut are marked `cut`.
pub(crate) fn chain(asked: &PageAsked, body: &Json) -> Read<Answer> {
    let o = body.obj("a chain page: an object")?;
    let more = o.need("more").bool("more")?;
    let list = o
        .need("items")
        .list(asked.limit as usize, "items: at most the number asked for")?;
    let mut items = Vec::with_capacity(list.len());
    let mut at = asked.after;
    for v in list {
        let o = v.obj("a chain item: an object")?;
        let seq = o.need("seq").count("seq")?;
        if seq <= at {
            return bad("a chain out of order");
        }
        at = seq;
        items.push(ChainItem {
            seq,
            item: envelope_item(o, true)?,
        });
    }
    if more && items.is_empty() {
        return bad("more to come, and no step forward");
    }
    Ok(Answer::Chain { items, more })
}

/// `GET /v1/cards/{object}?after=&limit=` and its siblings.
pub(crate) fn object_request(
    site: &Site<'_>,
    route: ObjectRoute,
    object: &ObjectId,
    after: u64,
    limit: u32,
) -> Read<(HttpRequest, PageAsked)> {
    let word = match route {
        ObjectRoute::Cards => "cards",
        ObjectRoute::Notes => "notes",
        ObjectRoute::PermissionRequests => "permission-requests",
        ObjectRoute::Artifacts => "artifacts",
    };
    let path = format!("/v1/{word}/{}", b64(object.as_bytes()));
    page(site, &path, after, limit, Some(*object))
}

/// Every envelope of the object asked for (and of no other), their change numbers rising strictly above the
/// one asked from.
pub(crate) fn object(asked: &PageAsked, body: &Json) -> Read<Answer> {
    let o = body.obj("an object: an object")?;
    let object = o.need("object_id").id(ObjectId::from_slice, "object_id")?;
    if Some(object) != asked.object {
        return bad("another object than the one asked for");
    }
    let more = o.need("more").bool("more")?;
    let list = o
        .need("items")
        .list(asked.limit as usize, "items: at most the number asked for")?;
    let mut items = Vec::with_capacity(list.len());
    let mut at = asked.after;
    for v in list {
        let item = envelope_item(v.obj("an envelope item: an object")?, false)?;
        if item.change <= at {
            return bad("envelopes out of order");
        }
        at = item.change;
        items.push(item);
    }
    if more && items.is_empty() {
        return bad("more to come, and no step forward");
    }
    Ok(Answer::Object(ObjectEnvelopes {
        object,
        group: o.need("group_id").group("an object's group_id")?,
        items,
        more,
    }))
}
