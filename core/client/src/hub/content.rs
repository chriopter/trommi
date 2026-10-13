//! Reads of stored content beside the catch-up: a sender's chain and every envelope of one object.

use super::items::envelope_item;
use super::json::{bad, Json, Read, MAX_NUMBER};
use super::limits::{
    CAP_LIST, CAP_WRITE, MAX_CHAT_PAGE, MAX_FRONTIER_FILES, MAX_FRONTIER_WRITERS, MAX_PAGE,
};
use super::request::{b64, begin};
use super::types::{Answer, ChainItem, ObjectEnvelopes, ObjectRoute, Site, Timeline};
use crate::app::io::{HttpClass, HttpRequest};
use serde_json::json;
use trommi_core::ids::{BoardId, DeviceId, FileId, GroupId, Hash32, ObjectId};

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

// ---- a Chat's page, a board's items, a board's frontier

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EnvelopesAsked {
    /// Where the page starts: every change lies beyond it, in the page's direction.
    pub from: u64,
    pub limit: u32,
    /// A Chat reads newest first.
    pub descending: bool,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `GET /v1/chats/{session|card}/{id}/items?before=&limit=` (a timeline is named by 32 hex digits here).
pub(crate) fn chat_request(
    site: &Site<'_>,
    timeline: Timeline<'_>,
    before: Option<u64>,
    limit: u32,
) -> Read<(HttpRequest, EnvelopesAsked)> {
    if before.is_some_and(|b| b > MAX_NUMBER) {
        return bad("the request: a cursor beyond any number of the protocol");
    }
    let path = match timeline {
        Timeline::Session(session) => {
            format!("/v1/chats/session/{}/items", hex(session.as_bytes()))
        }
        Timeline::Card(card) => format!("/v1/chats/card/{}/items", hex(card.as_bytes())),
    };
    let limit = limit.clamp(1, MAX_CHAT_PAGE);
    let mut build = begin(site, "GET", &path, true, CAP_LIST)?;
    if let Some(before) = before {
        build = build.query("before", before);
    }
    let asked = EnvelopesAsked {
        from: before.unwrap_or(MAX_NUMBER + 1),
        limit,
        descending: true,
    };
    Ok((
        build
            .query("limit", limit)
            .class(HttpClass::Background)
            .done(),
        asked,
    ))
}

/// `GET /v1/boards/{board}?after_change=&limit=`.
pub(crate) fn board_request(
    site: &Site<'_>,
    board: &BoardId,
    after: u64,
    limit: u32,
) -> Read<(HttpRequest, EnvelopesAsked)> {
    if after > MAX_NUMBER {
        return bad("the request: a cursor beyond any number of the protocol");
    }
    let limit = limit.clamp(1, MAX_PAGE);
    let request = begin(
        site,
        "GET",
        &format!("/v1/boards/{}", b64(board.as_bytes())),
        true,
        CAP_LIST,
    )?
    .query("after_change", after)
    .query("limit", limit)
    .class(HttpClass::Background)
    .done();
    let asked = EnvelopesAsked {
        from: after,
        limit,
        descending: false,
    };
    Ok((request, asked))
}

/// A page of envelopes whose change numbers run strictly one way from where the page starts.
pub(crate) fn envelopes(asked: &EnvelopesAsked, body: &Json) -> Read<Answer> {
    let o = body.obj("a page of envelopes: an object")?;
    let more = o.need("more").bool("more")?;
    let list = o
        .need("items")
        .list(asked.limit as usize, "items: at most the number asked for")?;
    let mut items = Vec::with_capacity(list.len());
    let mut at = asked.from;
    for v in list {
        let item = envelope_item(v.obj("an envelope item: an object")?, false)?;
        let onward = match asked.descending {
            true => item.change < at,
            false => item.change > at,
        };
        if !onward {
            return bad("envelopes out of order");
        }
        at = item.change;
        items.push(item);
    }
    if more && items.is_empty() {
        return bad("more to come, and no step forward");
    }
    Ok(Answer::Envelopes { items, more })
}

/// `POST /v1/boards/{board}/frontier` (10.9).
pub(crate) fn frontier_request(
    site: &Site<'_>,
    board: &BoardId,
    frontier: &[(DeviceId, u64, Hash32)],
    files: &[FileId],
    snapshot: Option<(u64, Hash32)>,
) -> Read<HttpRequest> {
    if frontier.len() > MAX_FRONTIER_WRITERS || files.len() > MAX_FRONTIER_FILES {
        return bad("the request: a frontier of at most 1000 writers and 20000 files");
    }
    let mut writers = serde_json::Map::new();
    for (writer, seq, hash) in frontier {
        if *seq > MAX_NUMBER
            || writers
                .insert(b64(writer.as_bytes()), json!([seq, b64(hash.as_bytes())]))
                .is_some()
        {
            return bad("the request: a frontier names a writer once");
        }
    }
    let mut body = json!({
        "frontier": writers,
        "files": files.iter().map(|f| b64(f.as_bytes())).collect::<Vec<_>>(),
    });
    if let Some((seq, hash)) = snapshot {
        if seq > MAX_NUMBER {
            return bad("the request: a chain number beyond any number of the protocol");
        }
        body["snapshot"] = json!([seq, b64(hash.as_bytes())]);
    }
    let path = format!("/v1/boards/{}/frontier", b64(board.as_bytes()));
    Ok(begin(site, "POST", &path, true, CAP_WRITE)?
        .json(&body)
        .done())
}

pub(crate) fn pruned(body: &Json) -> Read<Answer> {
    let o = body.obj("a frontier answer: an object")?;
    Ok(Answer::Pruned {
        pruned: o.need("pruned").int("pruned: a count")?,
    })
}
