//! The hub's refusal, `{ "error": code, "message": text, … }`, and what is not one.
//!
//! Only a body that names a code of spec/v1.md section 16 **under that code's own status** is the hub's refusal.
//! Everything else under an error status (a proxy's page, a code this protocol does not have, a known code under
//! another status) is somebody else's answer: the engine must not void an outbox entry or roll an action back on
//! it. The hub's `message` is never kept: a hub could echo a secret there.

use super::json::Json;
use crate::app::io::HttpResponse;
use trommi_core::ids::{DeviceId, GroupId};

/// Most a refusal's body may hold.
pub(crate) const CAP_REFUSAL: usize = 64 << 10;
/// The longest wait a hub may ask for: a day.
const MAX_RETRY_AFTER_S: u64 = 86_400;

use trommi_wire::codes::known;
/// The codes a hub refuses with and the status each comes with: one table with the hub's (core/wire).
pub use trommi_wire::codes::status_of;

/// A refusal of this protocol, with the members the hub adds to some of them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    /// A code of spec/v1.md section 16.
    pub code: &'static str,
    /// The hub stored a void record for the envelope: its chain number is used up (9.0.11).
    pub voided: bool,
    /// The wait the hub names (`rate-limited`, `overloaded`): the `retry-after` header, else the body's
    /// `retry_after`; whole seconds from 1 to a day, anything else counts as absent.
    pub retry_after_ms: Option<u64>,
    /// `epoch-taken`, `wrong-epoch`: the group's epoch now.
    pub epoch: Option<u64>,
    /// `gap`: the sender's last chain number the hub holds.
    pub seq: Option<u64>,
    /// `incomplete`: a device the Commit leaves out.
    pub device: Option<DeviceId>,
    /// The group a refusal is about, where the hub names it.
    pub group: Option<GroupId>,
}

fn wait_ms(seconds: u64) -> Option<u64> {
    (1..=MAX_RETRY_AFTER_S)
        .contains(&seconds)
        .then(|| seconds.saturating_mul(1000))
}

/// The refusal in a refusal's members, when `status` is its code's own.
pub(crate) fn from_members(body: &Json, status: u16, header_wait: Option<&str>) -> Option<Refusal> {
    let o = body.obj("").ok()?;
    let code = known(o.need("error").text(40, "").ok()?)?;
    o.need("message").text(CAP_REFUSAL, "").ok()?;
    if status_of(code) != Some(status) {
        return None;
    }
    let from_header = header_wait.and_then(|v| v.trim().parse::<u64>().ok());
    let seconds = match header_wait {
        Some(_) => from_header,
        None => o.maybe("retry_after").and_then(|v| v.int("").ok()),
    };
    Some(Refusal {
        status,
        code,
        voided: matches!(o.need("voided"), Json::Bool(true)),
        retry_after_ms: seconds.and_then(wait_ms),
        epoch: o.maybe("epoch").and_then(|v| v.int("").ok()),
        seq: o.maybe("seq").and_then(|v| v.int("").ok()),
        device: o
            .maybe("device")
            .and_then(|v| v.id(DeviceId::from_slice, "").ok()),
        group: o.maybe("group_id").and_then(|v| v.group("").ok()),
    })
}

/// The hub's refusal in an answer with an error status; `None` for a 2xx answer and for an error that is no
/// refusal of this protocol.
pub fn refusal(response: &HttpResponse) -> Option<Refusal> {
    if (200..300).contains(&response.status) {
        return None;
    }
    let body = Json::parse(&response.body, CAP_REFUSAL).ok()?;
    from_members(&body, response.status, response.header("retry-after"))
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
    fn a_refusal_is_a_known_code_under_its_own_status() {
        let r = refusal(&answer(
            409,
            &[],
            r#"{"error":"epoch-taken","message":"x","epoch":7}"#,
        ))
        .unwrap();
        assert_eq!(
            (r.code, r.status, r.epoch, r.voided),
            ("epoch-taken", 409, Some(7), false)
        );
        let r = refusal(&answer(
            409,
            &[],
            r#"{"error":"gap","message":"x","seq":3,"voided":true}"#,
        ))
        .unwrap();
        assert_eq!((r.code, r.seq, r.voided), ("gap", Some(3), true));
        // a known code under another status, an unknown code, no message, a page, an oversize body, a 2xx
        assert_eq!(
            refusal(&answer(
                403,
                &[],
                r#"{"error":"epoch-taken","message":"x"}"#
            )),
            None
        );
        assert_eq!(
            refusal(&answer(400, &[], r#"{"error":"made-up","message":"x"}"#)),
            None
        );
        assert_eq!(refusal(&answer(404, &[], r#"{"error":"not-found"}"#)), None);
        assert_eq!(refusal(&answer(404, &[], "<html>not found</html>")), None);
        assert_eq!(
            refusal(&answer(200, &[], r#"{"error":"not-found","message":"x"}"#)),
            None
        );
        let long = format!(
            r#"{{"error":"not-found","message":"{}"}}"#,
            "x".repeat(CAP_REFUSAL)
        );
        assert_eq!(refusal(&answer(404, &[], &long)), None);
    }

    #[test]
    fn the_wait_is_the_headers_else_the_bodys_and_at_most_a_day() {
        let body = r#"{"error":"rate-limited","message":"x","retry_after":9}"#;
        let wait = |headers: &[(&str, &str)], body: &str| {
            refusal(&answer(429, headers, body)).unwrap().retry_after_ms
        };
        assert_eq!(wait(&[("Retry-After", "4")], body), Some(4000));
        assert_eq!(wait(&[], body), Some(9000));
        assert_eq!(
            wait(&[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")], body),
            None
        );
        assert_eq!(wait(&[("retry-after", "0")], body), None);
        assert_eq!(wait(&[("retry-after", "86401")], body), None);
        assert_eq!(
            wait(
                &[],
                r#"{"error":"rate-limited","message":"x","retry_after":1.5}"#
            ),
            None
        );
    }
}
