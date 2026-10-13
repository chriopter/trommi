//! The hub's routes as a codec without I/O: build a request, read an answer strictly, sort a failure.
//!
//! W2 fills it from the checks of web `hub.ts` (the strictest today) and the connector's `hub.rs`. The rules the
//! contract fixes:
//!
//! - **Strict answers.** A 2xx answer whose shape, sizes, ids, base64url or change order is not what the route
//!   promises is [`Failure::BadAnswer`]: never half-applied.
//! - **Three kinds of failure**, and only the engine decides what to do with them ([`classify`]):
//!   - [`Failure::Refused`]: the hub said no for good (4xx but 408/429). Not retried; the action rolls back.
//!   - [`Failure::NotNow`]: try again later (408, 429, 5xx, and `Retry-After` when given).
//!   - [`Failure::Offline`]: no answer at all (an [`IoError`] of kind offline or timeout).

use crate::app::io::{HttpResponse, IoError, IoErrorKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The hub refused; `code` is spec section 16's when the body names one.
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

/// Sorts an HTTP answer: `Ok` for 2xx, else its failure. Reads `Retry-After` in seconds (an HTTP date is ignored).
pub fn classify(response: &HttpResponse) -> Result<(), Failure> {
    let status = response.status;
    if (200..300).contains(&status) {
        return Ok(());
    }
    let retry_after_ms = response
        .header("retry-after")
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(|s| s.saturating_mul(1000));
    if status == 408 || status == 429 || status >= 500 {
        return Err(Failure::NotNow {
            status,
            retry_after_ms,
        });
    }
    let code = serde_json::from_slice::<serde_json::Value>(&response.body)
        .ok()
        .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
    Err(Failure::Refused { status, code })
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
            classify(&answer(503, &[], "")),
            Err(Failure::NotNow {
                status: 503,
                retry_after_ms: None
            })
        );
        assert_eq!(
            classify(&answer(403, &[], r#"{"error":"not-member"}"#)),
            Err(Failure::Refused {
                status: 403,
                code: Some("not-member".into())
            })
        );
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
