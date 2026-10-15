//! The hub's routes as a codec without I/O: build a request, read an answer strictly, sort a failure.
//!
//! The engine decides what to ask and when (order, retries, the token it holds). The codec knows how a route looks
//! on the wire: [`Codec::request`] makes the whole [`HttpRequest`](crate::app::io::HttpRequest) of a [`Route`] at
//! a [`Site`] and hands back an [`Expect`], its own note of what was asked; [`Codec::answer`] takes that note and
//! the raw answer and gives the typed [`Answer`] or a [`Failure`]. Nothing here does I/O, reads a clock or keeps
//! state between calls, but the [`stream::StreamReader`], which holds the unfinished event of one connection and
//! the last change it brought.
//!
//! The rules:
//!
//! - **The hub is not trusted.** A 2xx answer whose shape, sizes, counts, ids, base64url or order is not what the
//!   route promises is [`Failure::BadAnswer`] as a whole: no part of it is handed on. Every answer has a size
//!   cap (also told to the shell as `max_answer`), every list a count, every byte string a length; a list that
//!   must not repeat an entry is refused when it does; change, log and chain numbers must run the way the route
//!   says and stay inside what was asked for. These limits are this client's own, whatever a hub does
//!   ([`limits`], and `json.rs` for how the JSON itself is bounded).
//! - **Three kinds of failure**, and only the engine decides what to do with them ([`classify`]):
//!   - [`Failure::Refused`]: an error status that is no "not now" (4xx except 408/429, or a checked capacity refusal). `code` is set only for the
//!     hub's own refusal ([`refusal`]: a code of spec/v1.md 16 under its own status); without one the answer came
//!     from somewhere else (a proxy), and nothing may be voided or rolled back on it.
//!   - [`Failure::NotNow`]: try again later (408, throttling 429, 5xx, and `Retry-After` when given).
//!   - [`Failure::Offline`]: no answer at all (an [`IoError`] of kind offline or timeout).
//! - **A redirect is never followed** (the token would travel with it): a 3xx answer is a bad answer.
//! - **A request the codec cannot build** (an address that is not canonical, a token of other than plain
//!   characters, an outbox entry without its parts) is also [`Failure::BadAnswer`], its text beginning
//!   "the request:"; nothing is sent.
//!
//! Every route a human device's app uses is a [`Route`]: sign-in, catch-up, the live stream, the Desk, the
//! outbox, the delivery service's reads and joins, content, files and Share links, invites, the account, push;
//! and an agent device's lease (`POST /v1/link`, with `Trommi-Lease` on its writes and stream: [`Site::lease`]).
//! Not here, because the app core never asks them: the two routes of the notification extension that go by a
//! push ticket (`GET /v1/push-envelope`, `GET /v1/push-changes`).

mod account;
mod changes;
mod codec;
mod content;
mod desk;
mod files;
mod groups;
mod invites;
mod items;
mod link;
mod json;
pub mod limits;
mod outbox;
mod push;
mod refusal;
mod request;
mod signin;
pub mod stream;
mod types;

pub use account::{account_copies_body, new_account_body};
pub use codec::{Codec, Expect};
pub use refusal::{refusal, status_of, Refusal};
pub use request::TIMEOUT_MS;
pub use stream::{Fed, StreamReader};
pub use types::*;
pub(crate) use groups::{archive_answer, archive_request};

use crate::app::io::{HttpResponse, IoError, IoErrorKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// An error status that is no "not now". `code` is spec section 16's when the answer is the hub's own
    /// refusal ([`refusal`] gives its further members); `None`: not this protocol's refusal (a proxy).
    Refused { status: u16, code: Option<String> },
    /// Try again; not before `retry_after_ms` when the hub said so.
    NotNow {
        status: u16,
        retry_after_ms: Option<u64>,
    },
    /// No answer.
    Offline,
    /// An answer the route does not allow (W2's checks).
    BadAnswer { why: &'static str },
}

/// Sorts an HTTP answer: `Ok` for 2xx, else its failure. Reads `Retry-After` in seconds (an HTTP date is ignored);
/// an answer without that header may name the wait as `retry_after` in the hub's own refusal.
pub fn classify(response: &HttpResponse) -> Result<(), Failure> {
    let status = response.status;
    if (200..300).contains(&status) {
        return Ok(());
    }
    if status < 400 {
        return Err(Failure::BadAnswer {
            why: "a redirect, or a status that is no answer",
        });
    }
    if status == 408 || status == 429 || status >= 500 {
        // Capacity is a refusal of this request; retrying it cannot free a helper's place. Only a checked
        // protocol code under its own status is final: throttling and proxy responses still wait and retry.
        if status == 429 && refusal(response).is_some_and(|r| r.code == "too-many") {
            return Err(Failure::Refused { status, code: Some("too-many".into()) });
        }
        // (the header; without one, the wait the hub's own refusal names in its body)
        let retry_after_ms = match response.header("retry-after") {
            Some(v) => v.trim().parse::<u64>().ok().map(|s| s.saturating_mul(1000)),
            None => refusal(response).and_then(|r| r.retry_after_ms),
        };
        return Err(Failure::NotNow {
            status,
            retry_after_ms,
        });
    }
    let code = refusal(response).map(|r| r.code.to_owned());
    Err(Failure::Refused { status, code })
}

/// A JSON route's answer: its failure when the status is none of 2xx, else the body read within `cap` bytes.
pub(crate) fn answer(response: &HttpResponse, cap: usize) -> Result<json::Json, Failure> {
    classify(response)?;
    json::Json::parse(&response.body, cap)
}

/// Sorts an I/O error: offline, timeout and failed are all [`Failure::Offline`]; `cancelled` is no failure; an
/// answer longer than the request's `max_answer` is a bad answer (the hub answered, with what no route allows).
pub fn classify_io(error: &IoError) -> Option<Failure> {
    match error.kind {
        IoErrorKind::Cancelled => None,
        IoErrorKind::TooLarge => Some(Failure::BadAnswer {
            why: "an answer larger than its route allows",
        }),
        _ => Some(Failure::Offline),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::io::Header;

    fn answer(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
        HttpResponse {
            status,
            headers: headers
                .iter()
                .map(|(n, v)| Header {
                    name: n.to_string(),
                    value: v.to_string(),
                })
                .collect(),
            body: body.as_bytes().to_vec().into(),
        }
    }

    #[test]
    fn a_named_capacity_refusal_is_final_while_throttling_is_retried() {
        assert_eq!(classify(&answer(429, &[], r#"{"error":"too-many","message":"at the limit"}"#)),
            Err(Failure::Refused { status: 429, code: Some("too-many".into()) }));
        assert!(matches!(classify(&answer(429, &[], r#"{"error":"rate-limited","message":"wait"}"#)), Err(Failure::NotNow { .. })));
        assert!(matches!(classify(&answer(503, &[], r#"{"error":"too-many","message":"wrong status"}"#)), Err(Failure::NotNow { .. })));
    }

    #[test]
    fn failures_sort_into_three_kinds() {
        assert_eq!(classify(&answer(204, &[], "")), Ok(()));
        assert_eq!(
            classify(&answer(429, &[("Retry-After", "7")], "")),
            Err(Failure::NotNow {
                status: 429,
                retry_after_ms: Some(7000)
            })
        );
        assert_eq!(
            classify(&answer(
                429,
                &[],
                r#"{"error":"rate-limited","message":"x","retry_after":3}"#
            )),
            Err(Failure::NotNow {
                status: 429,
                retry_after_ms: Some(3000)
            })
        );
        assert_eq!(
            classify(&answer(503, &[], "")),
            Err(Failure::NotNow {
                status: 503,
                retry_after_ms: None
            })
        );
        assert_eq!(
            classify(&answer(
                403,
                &[],
                r#"{"error":"not-member","message":"no"}"#
            )),
            Err(Failure::Refused {
                status: 403,
                code: Some("not-member".into())
            })
        );
        // a code this protocol does not have, a known one under another status, a proxy's page: not the hub's
        for body in [
            r#"{"error":"made-up"}"#,
            r#"{"error":"gone","message":"x"}"#,
            "<html>",
        ] {
            assert_eq!(
                classify(&answer(403, &[], body)),
                Err(Failure::Refused {
                    status: 403,
                    code: None
                })
            );
        }
        assert!(matches!(
            classify(&answer(
                302,
                &[("location", "https://elsewhere.example/")],
                ""
            )),
            Err(Failure::BadAnswer { .. })
        ));
        assert_eq!(
            classify_io(&IoError::new(IoErrorKind::Timeout)),
            Some(Failure::Offline)
        );
        assert_eq!(classify_io(&IoError::new(IoErrorKind::Cancelled)), None);
        assert!(matches!(
            classify_io(&IoError::new(IoErrorKind::TooLarge)),
            Some(Failure::BadAnswer { .. })
        ));
    }
}
