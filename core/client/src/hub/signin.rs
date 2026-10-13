//! Signing in to a room (spec/v1.md 12.3): `GET /v1/rooms/{room}/challenge`, `POST /v1/rooms/{room}/tokens`.
//! Neither takes a token.

use super::json::{bad, Json, Read};
use super::limits::{CAP_WRITE, MAX_TAG};
use super::request::{b64, begin};
use super::types::{Answer, Role, Site};
use crate::app::io::HttpRequest;
use serde_json::json;
use trommi_core::ids::RoomId;

/// The longest encoded `HubAuth`: the three fixed fields and the address behind its length.
const MAX_AUTH: usize = 32 + 2 + trommi_core::hub_auth::MAX_ADDRESS_LEN + 32 + 32;

pub(crate) fn challenge_request(site: &Site<'_>, room: &RoomId) -> Read<HttpRequest> {
    let path = format!("/v1/rooms/{}/challenge", b64(room.as_bytes()));
    Ok(begin(site, "GET", &path, false, CAP_WRITE)?.done())
}

/// 32 bytes, good for two minutes and one use.
pub(crate) fn challenge(body: &Json) -> Read<Answer> {
    let o = body.obj("a challenge answer: an object")?;
    Ok(Answer::Challenge(
        o.need("challenge").fixed("challenge: 32 bytes")?,
    ))
}

pub(crate) fn token_request(
    site: &Site<'_>,
    room: &RoomId,
    auth: &[u8],
    signature: &[u8],
) -> Read<HttpRequest> {
    if auth.is_empty() || auth.len() > MAX_AUTH || signature.is_empty() || signature.len() > MAX_TAG
    {
        return bad("the request: a sign-in is a HubAuth and its signature");
    }
    let path = format!("/v1/rooms/{}/tokens", b64(room.as_bytes()));
    Ok(begin(site, "POST", &path, false, CAP_WRITE)?
        .json(&json!({ "auth": b64(auth), "signature": b64(signature) }))
        .done())
}

/// The token (16 to 200 of `[A-Za-z0-9_-]`, what a request's header takes), when the hub says it ends (its
/// clock: a hint) and what the hub takes the key for.
pub(crate) fn token(body: &Json) -> Read<Answer> {
    let o = body.obj("a token answer: an object")?;
    let token = o.need("token").text(200, "token")?;
    let plain = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-';
    if token.len() < 16 || !token.bytes().all(plain) {
        return bad("token");
    }
    Ok(Answer::Token {
        token: token.to_owned(),
        expires_at: o.need("expires_at").int("expires_at")?,
        role: o.need("role").one_of(
            &[
                ("human", Role::Human),
                ("agent", Role::Agent),
                ("helper", Role::Helper),
                ("recovery", Role::Recovery),
                ("removed", Role::Removed),
            ],
            "role",
        )?,
    })
}
