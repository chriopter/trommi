//! An agent device's lease (spec/v1.md 13.7): `POST /v1/link`.
//!
//! A new random process id acquires the lease with a generation one above the last, valid 60 s; the same process
//! renews it with its generation, which stays; a renewal under an older generation is refused `lease-lost`. Every
//! other write of the device carries the generation as `Trommi-Lease` ([`Site::lease`]); this route does not.

use super::json::{bad, Json, Read};
use super::limits::CAP_WRITE;
use super::request::{b64, begin};
use super::types::{Answer, LinkReport, Site};
use crate::app::io::HttpRequest;
use serde_json::json;

/// The most a renewal's `last_call_at` may be: every number of the contract stays below 2^53.
const MAX_TIME: u64 = (1 << 53) - 1;

pub(crate) fn link_request(
    site: &Site<'_>,
    process: &[u8; 16],
    generation: Option<u64>,
    report: &LinkReport,
    leaving: bool,
) -> Read<HttpRequest> {
    if generation == Some(0) || (leaving && generation.is_none()) || report.last_call_at > MAX_TIME {
        return bad("the request: a lease is acquired without a generation, renewed or left with its own");
    }
    // (the route that takes the lease carries none)
    let site = Site { lease: None, ..*site };
    let mut body = json!({
        "process": b64(process),
        "hears": report.hears,
        "working": report.working,
        "last_call_at": report.last_call_at,
    });
    if let Some(generation) = generation {
        body["generation"] = json!(generation);
    }
    if leaving {
        body["leaving"] = json!(true);
    }
    Ok(begin(&site, "POST", "/v1/link", true, CAP_WRITE)?.json(&body).done())
}

/// The lease's generation (never 0) and when it ends by the hub's clock (a hint).
pub(crate) fn linked(asked: Option<u64>, body: &Json) -> Read<Answer> {
    let o = body.obj("a lease: an object")?;
    let generation = o.need("generation").count("generation")?;
    if generation == 0 {
        return bad("a lease of generation 0");
    }
    // a renewal keeps its generation
    if asked.is_some_and(|asked| asked != generation) {
        return bad("a renewal that changed the lease's generation");
    }
    Ok(Answer::Linked {
        generation,
        expires_at: o.need("expires_at").int("expires_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::super::request::tests::SITE;
    use super::super::{Codec, Route};
    use super::*;
    use crate::app::io::HttpResponse;

    fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
        request.headers.iter().find(|h| h.name == name).map(|h| h.value.as_str())
    }

    fn body(request: &HttpRequest) -> serde_json::Value {
        serde_json::from_slice(&request.body.as_ref().expect("a body").0).expect("JSON")
    }

    const REPORT: LinkReport = LinkReport { hears: true, working: false, last_call_at: 1234 };

    #[test]
    fn acquiring_names_the_process_and_no_generation() {
        let site = Site { lease: Some(4), ..SITE };
        let route = Route::Link { process: &[7; 16], generation: None, report: &REPORT, leaving: false };
        let (request, _) = Codec.request(&site, &route).expect("a request");
        assert_eq!((request.method.as_str(), request.url.as_str()), ("POST", "https://hub.example/v1/link"));
        assert_eq!(header(&request, "trommi-lease"), None, "the lease route carries no lease");
        assert!(header(&request, "authorization").is_some());
        let body = body(&request);
        assert_eq!(body["process"], b64(&[7; 16]));
        assert_eq!((body["hears"].as_bool(), body["working"].as_bool(), body["last_call_at"].as_u64()), (Some(true), Some(false), Some(1234)));
        assert!(body.get("generation").is_none() && body.get("leaving").is_none());
    }

    #[test]
    fn renewing_and_leaving_carry_the_generation() {
        let route = Route::Link { process: &[7; 16], generation: Some(3), report: &REPORT, leaving: true };
        let (request, _) = Codec.request(&SITE, &route).expect("a request");
        let body = body(&request);
        assert_eq!((body["generation"].as_u64(), body["leaving"].as_bool()), (Some(3), Some(true)));
        for (generation, leaving) in [(Some(0), false), (None, true)] {
            let route = Route::Link { process: &[7; 16], generation, report: &REPORT, leaving };
            assert!(Codec.request(&SITE, &route).is_err(), "{generation:?} {leaving}");
        }
    }

    #[test]
    fn the_answer_is_a_generation_that_a_renewal_keeps() {
        let answer = |generation: Option<u64>, body: serde_json::Value| {
            let route = Route::Link { process: &[1; 16], generation, report: &REPORT, leaving: false };
            let (_, expect) = Codec.request(&SITE, &route).expect("a request");
            Codec.answer(&expect, &HttpResponse { status: 200, headers: Vec::new(), body: body.to_string().into_bytes().into() })
        };
        assert!(matches!(answer(None, json!({ "generation": 5, "expires_at": 9 })), Ok(Answer::Linked { generation: 5, expires_at: 9 })));
        assert!(matches!(answer(Some(5), json!({ "generation": 5, "expires_at": 9 })), Ok(Answer::Linked { generation: 5, .. })));
        assert!(answer(Some(5), json!({ "generation": 6, "expires_at": 9 })).is_err());
        assert!(answer(None, json!({ "generation": 0, "expires_at": 9 })).is_err());
        assert!(answer(None, json!({ "expires_at": 9 })).is_err());
    }

    #[test]
    fn writes_and_the_stream_carry_the_lease_reads_do_not() {
        let site = Site { lease: Some(9), ..SITE };
        let (stream, _) = Codec.request(&site, &Route::Stream { after: 3 }).expect("a request");
        assert_eq!(header(&stream, "trommi-lease"), Some("9"));
        let (page, _) = Codec.request(&site, &Route::Changes { after: 0, limit: 5, only: Default::default() }).expect("a request");
        assert_eq!(header(&page, "trommi-lease"), None);
        let (wish, _) = Codec.request(&site, &Route::Wishes).expect("a request");
        assert_eq!(header(&wish, "trommi-lease"), None);
        let (sign_out, _) = Codec.request(&site, &Route::SignOut).expect("a request");
        assert_eq!(header(&sign_out, "trommi-lease"), Some("9"));
        let (none, _) = Codec.request(&SITE, &Route::SignOut).expect("a request");
        assert_eq!(header(&none, "trommi-lease"), None);
    }
}
