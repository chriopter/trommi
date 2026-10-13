//! Invites (spec/v1.md 12.1): the three signed messages, by invite id. What they mean and whether their
//! signatures and MACs hold is the protocol core's; here only that they are small and whole.

use super::json::{bad, Json, Read};
use super::limits::{CAP_SMALL, CAP_WRITE, MAX_KEY_PACKAGE, MAX_SMALL_STRUCT, MAX_TAG};
use super::request::{b64, begin};
use super::types::{Answer, InviteOffer, InviteRequestRow, Site};
use crate::app::io::HttpRequest;
use serde_json::json;
use trommi_core::ids::InviteId;

/// Most Requests one invite takes.
const MAX_REQUESTS: usize = 4;

fn path(invite: &InviteId, rest: &str) -> String {
    format!("/v1/invites/{}{rest}", b64(invite.as_bytes()))
}

fn small(bytes: &[u8], max: usize) -> Read<String> {
    if bytes.is_empty() || bytes.len() > max {
        return bad("the request: a part of an invite is missing or too large");
    }
    Ok(b64(bytes))
}

/// With a token when the site has one: a human device of the invite's room also gets the Requests.
pub(crate) fn get_request(site: &Site<'_>, invite: &InviteId) -> Read<HttpRequest> {
    Ok(begin(
        site,
        "GET",
        &path(invite, ""),
        site.token.is_some(),
        CAP_SMALL,
    )?
    .done())
}

pub(crate) fn offer(body: &Json) -> Read<Answer> {
    let o = body.obj("an invite: an object")?;
    let requests = match o.maybe("requests") {
        None => None,
        Some(v) => {
            let mut rows = Vec::new();
            for v in v.list(MAX_REQUESTS, "requests: at most 4")? {
                let r = v.obj("a Request: an object")?;
                rows.push(InviteRequestRow {
                    request: r.need("request").bytes(1, MAX_KEY_PACKAGE, "request")?.0,
                    mac: r.need("mac").bytes(1, MAX_TAG, "a Request's mac")?.0,
                    signature: r
                        .need("signature")
                        .bytes(1, MAX_TAG, "a Request's signature")?
                        .0,
                });
            }
            Some(rows)
        }
    };
    Ok(Answer::Invite(InviteOffer {
        offer: o.need("offer").bytes(1, MAX_SMALL_STRUCT, "offer")?.0,
        signature: o
            .need("signature")
            .bytes(1, MAX_TAG, "an Offer's signature")?
            .0,
        mac: o
            .maybe("mac")
            .map(|v| v.bytes(1, MAX_TAG, "an Offer's mac").map(|b| b.0))
            .transpose()?,
        expires_at: o.need("expires_at").int("an invite's expires_at")?,
        requests,
    }))
}

pub(crate) fn request_request(
    site: &Site<'_>,
    invite: &InviteId,
    request: &[u8],
    mac: &[u8],
    signature: &[u8],
) -> Read<HttpRequest> {
    let body = json!({
        "request": small(request, MAX_KEY_PACKAGE)?,
        "mac": small(mac, MAX_TAG)?,
        "signature": small(signature, MAX_TAG)?,
    });
    Ok(
        begin(site, "POST", &path(invite, "/request"), false, CAP_WRITE)?
            .json(&body)
            .done(),
    )
}

pub(crate) fn requested(body: &Json) -> Read<Answer> {
    let o = body.obj("a Request answer: an object")?;
    Ok(Answer::Requested {
        request_hash: o.need("request_hash").fixed("request_hash: 32 bytes")?,
    })
}

pub(crate) fn reveal_request(site: &Site<'_>, invite: &InviteId) -> Read<HttpRequest> {
    Ok(begin(site, "GET", &path(invite, "/reveal"), false, CAP_SMALL)?.done())
}

pub(crate) fn reveal(body: &Json) -> Read<Answer> {
    let o = body.obj("a Reveal: an object")?;
    Ok(Answer::Reveal {
        reveal: o.need("reveal").bytes(1, MAX_SMALL_STRUCT, "reveal")?.0,
        signature: o
            .need("signature")
            .bytes(1, MAX_TAG, "a Reveal's signature")?
            .0,
    })
}

pub(crate) fn post_request(
    site: &Site<'_>,
    offer: &[u8],
    signature: &[u8],
    mac: &[u8],
) -> Read<HttpRequest> {
    let body = json!({
        "offer": small(offer, MAX_SMALL_STRUCT)?,
        "signature": small(signature, MAX_TAG)?,
        "mac": small(mac, MAX_TAG)?,
    });
    Ok(begin(site, "POST", "/v1/invites", true, CAP_WRITE)?
        .json(&body)
        .done())
}

pub(crate) fn invited(body: &Json) -> Read<Answer> {
    let o = body.obj("an invite answer: an object")?;
    Ok(Answer::Invited {
        invite: o.need("invite_id").id(InviteId::from_slice, "invite_id")?,
    })
}

pub(crate) fn put_reveal_request(
    site: &Site<'_>,
    invite: &InviteId,
    reveal: &[u8],
    signature: &[u8],
) -> Read<HttpRequest> {
    let body = json!({ "reveal": small(reveal, MAX_SMALL_STRUCT)?, "signature": small(signature, MAX_TAG)? });
    Ok(
        begin(site, "PUT", &path(invite, "/reveal"), true, CAP_WRITE)?
            .json(&body)
            .done(),
    )
}

pub(crate) fn delete_request(site: &Site<'_>, invite: &InviteId) -> Read<HttpRequest> {
    Ok(begin(site, "DELETE", &path(invite, ""), true, CAP_WRITE)?.done())
}

/// An answer that says one thing happened: `{ "<name>": true }`.
pub(crate) fn said(name: &'static str, body: &Json) -> Read<Answer> {
    match body.obj("an answer: an object")?.need(name) {
        Json::Bool(true) => Ok(Answer::Done),
        _ => bad("an answer that does not say it was done"),
    }
}
