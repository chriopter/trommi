//! Push (spec/v1.md 15), for human devices: registering for Web Push or APNs, the registrations, a Live
//! Activity's token. An endpoint and a token are credentials: they go in bodies only.

use super::json::{bad, Json, Read};
use super::limits::{CAP_SMALL, CAP_WRITE, MAX_ENDPOINT, MAX_PUSH};
use super::request::{b64, begin};
use super::types::{
    Answer, LiveActivity, PushKind, PushLevel, PushRegistration, PushState, PushSubscription,
    PushTarget, Site,
};
use crate::app::io::HttpRequest;
use serde_json::json;

const WRONG: &str = "the request: a part of the push registration is missing or too large";

/// A text of the platform's (a token, a topic): printable ASCII without spaces.
fn word(text: &str, max: usize) -> Read<&str> {
    if text.is_empty() || text.len() > max || !text.bytes().all(|b| b.is_ascii_graphic()) {
        return bad(WRONG);
    }
    Ok(text)
}

fn key(bytes: &[u8], max: usize) -> Read<String> {
    if bytes.is_empty() || bytes.len() > max {
        return bad(WRONG);
    }
    Ok(b64(bytes))
}

fn level(level: PushLevel) -> &'static str {
    match level {
        PushLevel::All => "all",
        PushLevel::Knocking => "knocking",
    }
}

pub(crate) fn register_request(
    site: &Site<'_>,
    registration: &PushRegistration,
) -> Read<HttpRequest> {
    let mut body = json!({ "level": level(registration.level) });
    match &registration.target {
        PushTarget::WebPush {
            endpoint,
            p256dh,
            auth,
        } => {
            body["web_push"] = json!({
                "endpoint": word(endpoint, MAX_ENDPOINT)?,
                "keys": { "p256dh": key(p256dh, 128)?, "auth": key(auth, 64)? },
            });
        }
        PushTarget::Apns {
            token,
            key: sealing,
            environment,
            topic,
        } => {
            body["apns"] = json!({
                "token": word(token, 256)?,
                "key": key(sealing, 64)?,
                "environment": word(environment, 32)?,
                "topic": word(topic, 256)?,
            });
        }
    }
    Ok(begin(site, "POST", "/v1/push", true, CAP_WRITE)?
        .json(&body)
        .done())
}

pub(crate) fn state_request(site: &Site<'_>) -> Read<HttpRequest> {
    Ok(begin(site, "GET", "/v1/push", true, CAP_SMALL)?.done())
}

/// This device's registrations (no endpoint twice) and the hub's Web Push key.
pub(crate) fn state(body: &Json) -> Read<Answer> {
    let o = body.obj("the push state: an object")?;
    let mut subscriptions: Vec<PushSubscription> = Vec::new();
    for v in o.need("subscriptions").list(MAX_PUSH, "subscriptions")? {
        let s = v.obj("a registration: an object")?;
        let row = PushSubscription {
            kind: s.need("kind").one_of(
                &[("web_push", PushKind::WebPush), ("apns", PushKind::Apns)],
                "a registration's kind",
            )?,
            endpoint: s
                .need("endpoint")
                .text(MAX_ENDPOINT, "endpoint")?
                .to_owned(),
            level: s.need("level").one_of(
                &[("all", PushLevel::All), ("knocking", PushLevel::Knocking)],
                "level",
            )?,
            created_at: s.need("created_at").int("created_at")?,
        };
        if subscriptions
            .iter()
            .any(|known| known.endpoint == row.endpoint)
        {
            return bad("a registration twice");
        }
        subscriptions.push(row);
    }
    Ok(Answer::Push(PushState {
        subscriptions,
        vapid_public_key: o
            .need("vapid_public_key")
            .bytes(65, 65, "vapid_public_key: 65 bytes")?
            .0,
        apns: o.need("apns").bool("apns")?,
    }))
}

pub(crate) fn remove_request(site: &Site<'_>, endpoint: Option<&str>) -> Read<HttpRequest> {
    let body = match endpoint {
        Some(endpoint) => json!({ "endpoint": word(endpoint, MAX_ENDPOINT)? }),
        None => json!({}),
    };
    Ok(begin(site, "DELETE", "/v1/push", true, CAP_WRITE)?
        .json(&body)
        .done())
}

pub(crate) fn removed(body: &Json) -> Read<Answer> {
    let o = body.obj("a deletion: an object")?;
    Ok(Answer::Removed {
        deleted: o.need("deleted").int("deleted: a count")?,
    })
}

pub(crate) fn live_activity_request(site: &Site<'_>, activity: &LiveActivity) -> Read<HttpRequest> {
    let body = json!({
        "kind": if activity.start { "start" } else { "activity" },
        "token": word(&activity.token, 512)?,
        "tag": word(&activity.tag, 64)?,
        "environment": word(&activity.environment, 32)?,
        "topic": word(&activity.topic, 256)?,
    });
    Ok(begin(site, "POST", "/v1/live-activity", true, CAP_WRITE)?
        .json(&body)
        .done())
}
