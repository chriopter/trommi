//! The codec's two calls: a route to its request, an answer to its typed result.

use super::json::{bad, Read};
use super::limits::{CAP_LIST, CAP_WRITE};
use super::types::{Answer, Route, Site};
use super::{answer, changes, desk, outbox, stream, Failure};
use crate::app::io::{HttpRequest, HttpResponse};

/// The codec's note of what was asked, kept by the engine beside the pending request and handed back with the
/// answer: the answer is checked against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expect(Kind);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Changes(changes::Asked),
    Stream,
    DeskBrief { since: Option<u64> },
    Outbox(outbox::Asked),
    KeyPackagesHeld,
}

/// The hub's routes without I/O. Stateless: everything a call needs is in its arguments.
#[derive(Clone, Copy, Debug, Default)]
pub struct Codec;

impl Codec {
    /// Whether this codec builds `route` yet (the rest follow in the order the engine needs them).
    pub fn builds(route: &Route<'_>) -> bool {
        matches!(
            route,
            Route::Changes { .. }
                | Route::Stream { .. }
                | Route::DeskBrief { .. }
                | Route::Outbox { .. }
                | Route::KeyPackagesHeld
        )
    }

    /// The whole request of a route: method, absolute URL, headers (the bearer and `trommi-client` among them),
    /// body, timeout, class and the most its answer may hold. `Err` ([`Failure::BadAnswer`], "the request: …")
    /// only for a route that cannot be asked as given.
    pub fn request(
        &self,
        site: &Site<'_>,
        route: &Route<'_>,
    ) -> Result<(HttpRequest, Expect), Failure> {
        let (request, kind) = match *route {
            Route::Changes { after, limit, only } => {
                let (request, asked) = changes::request(site, after, limit, only)?;
                (request, Kind::Changes(asked))
            }
            Route::Stream { after } => (stream::request(site, after)?, Kind::Stream),
            Route::DeskBrief { since, active } => {
                let (request, since) = desk::request(site, since, active)?;
                (request, Kind::DeskBrief { since })
            }
            Route::Outbox { room, entry, extra } => {
                let (request, asked) = outbox::request(site, room, entry, extra)?;
                (request, Kind::Outbox(asked))
            }
            Route::KeyPackagesHeld => (outbox::held_request(site)?, Kind::KeyPackagesHeld),
            _ => return bad("the request: a route this codec does not build yet"),
        };
        Ok((request, Expect(kind)))
    }

    /// Reads an answer: [`classify`](super::classify) first (a status that is no 2xx is its failure), then the
    /// strict check of the body against what was asked. Never half an answer.
    pub fn answer(&self, expect: &Expect, response: &HttpResponse) -> Result<Answer, Failure> {
        Ok(match &expect.0 {
            Kind::Changes(asked) => {
                Answer::Changes(changes::read(asked, &answer(response, CAP_LIST)?)?)
            }
            Kind::DeskBrief { since } => {
                Answer::DeskBrief(desk::read(*since, &answer(response, CAP_LIST)?)?)
            }
            Kind::Outbox(asked) => {
                Answer::Outbox(outbox::read(asked, &answer(response, CAP_WRITE)?)?)
            }
            Kind::KeyPackagesHeld => {
                let body = answer(response, CAP_WRITE)?;
                Answer::KeyPackagesHeld {
                    unused: outbox::unused(body.obj("a KeyPackage answer: an object")?)?,
                }
            }
            Kind::Stream => return stream_has_no_answer(),
        })
    }

    /// The hub's refusal in an answer with an error status, with what the hub adds to some (`voided` of a
    /// refused envelope, the `epoch` of `epoch-taken`, the wait); `None` for an error that is no refusal of this
    /// protocol.
    pub fn refusal(&self, response: &HttpResponse) -> Option<super::Refusal> {
        super::refusal(response)
    }

    /// Whether a refusal says the hub keeps the envelope as a void record (9.0.11): its chain number is used up.
    pub fn voided(&self, response: &HttpResponse) -> bool {
        super::refusal(response).is_some_and(|r| r.voided)
    }
}

fn stream_has_no_answer() -> Read<Answer> {
    bad("a stream has no answer: its bytes go to a StreamReader")
}
