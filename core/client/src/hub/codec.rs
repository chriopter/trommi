//! The codec's two calls: a route to its request, an answer to its typed result.

use super::json::{bad, Read};
use super::limits::{CAP_DESK, CAP_GROUP_INFO, CAP_LIST, CAP_WRITE};
use super::types::{Answer, Route, Site};
use super::{answer, changes, content, desk, groups, outbox, signin, stream, Failure};
use crate::app::io::{HttpRequest, HttpResponse};
use trommi_core::ids::GroupId;

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
    Challenge,
    Token,
    Welcomes { after: u64 },
    GroupLog(groups::LogAsked),
    GroupInfo { epoch: Option<u64> },
    Joins { epochs: Vec<u64> },
    GroupsRead { groups: Vec<GroupId> },
    Claim(groups::ClaimAsked),
    RoomGroups(groups::GroupsAsked),
    Chain(content::PageAsked),
    Object(content::PageAsked),
    Wish,
    Wishes,
    RejectCommit,
}

/// The hub's routes without I/O. Stateless: everything a call needs is in its arguments.
#[derive(Clone, Copy, Debug, Default)]
pub struct Codec;

impl Codec {
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
            Route::Challenge { room } => (signin::challenge_request(site, room)?, Kind::Challenge),
            Route::Token {
                room,
                auth,
                signature,
            } => (
                signin::token_request(site, room, auth, signature)?,
                Kind::Token,
            ),
            Route::Welcomes { after } => (
                groups::welcomes_request(site, after)?,
                Kind::Welcomes { after },
            ),
            Route::GroupLog {
                group,
                after,
                limit,
                commits_only,
            } => {
                let (request, asked) =
                    groups::log_request(site, group, after, limit, commits_only)?;
                (request, Kind::GroupLog(asked))
            }
            Route::GroupInfo { group, epoch } => (
                groups::info_request(site, group, epoch)?,
                Kind::GroupInfo { epoch },
            ),
            Route::Joins { entries } => {
                let (request, epochs) = groups::joins_request(site, entries)?;
                (request, Kind::Joins { epochs })
            }
            Route::GroupsRead { groups } => {
                let (request, groups) = groups::read_request(site, groups)?;
                (request, Kind::GroupsRead { groups })
            }
            Route::ClaimKeyPackages { devices, partial } => {
                let (request, asked) = groups::claim_request(site, devices, partial)?;
                (request, Kind::Claim(asked))
            }
            Route::RoomGroups { room, after, limit } => {
                let (request, asked) = groups::groups_request(site, room, after, limit)?;
                (request, Kind::RoomGroups(asked))
            }
            Route::Chain {
                group,
                sender,
                after,
                limit,
            } => {
                let (request, asked) = content::chain_request(site, group, sender, after, limit)?;
                (request, Kind::Chain(asked))
            }
            Route::Object {
                route,
                object,
                after,
                limit,
            } => {
                let (request, asked) = content::object_request(site, route, object, after, limit)?;
                (request, Kind::Object(asked))
            }
            Route::Wish {
                kind,
                group,
                key_package,
            } => (
                groups::wish_request(site, kind, group, key_package)?,
                Kind::Wish,
            ),
            Route::Wishes => (groups::wishes_request(site)?, Kind::Wishes),
            Route::RejectCommit { group, n } => {
                (groups::reject_request(site, group, n)?, Kind::RejectCommit)
            }
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
            Kind::Challenge => signin::challenge(&answer(response, CAP_WRITE)?)?,
            Kind::Token => signin::token(&answer(response, CAP_WRITE)?)?,
            Kind::Welcomes { after } => groups::welcomes(*after, &answer(response, CAP_LIST)?)?,
            Kind::GroupLog(asked) => groups::log(asked, &answer(response, CAP_LIST)?)?,
            Kind::GroupInfo { epoch } => groups::info(*epoch, &answer(response, CAP_GROUP_INFO)?)?,
            Kind::Joins { epochs } => groups::joins(epochs, &answer(response, CAP_WRITE)?)?,
            Kind::GroupsRead { groups } => groups::read(groups, &answer(response, CAP_LIST)?)?,
            Kind::Claim(asked) => groups::claimed(asked, &answer(response, CAP_DESK)?)?,
            Kind::RoomGroups(asked) => groups::groups(asked, &answer(response, CAP_LIST)?)?,
            Kind::Chain(asked) => content::chain(asked, &answer(response, CAP_LIST)?)?,
            Kind::Object(asked) => content::object(asked, &answer(response, CAP_LIST)?)?,
            Kind::Wish => Answer::Wished {
                id: groups::wished(&answer(response, CAP_WRITE)?)?,
            },
            Kind::Wishes => groups::wishes(&answer(response, CAP_LIST)?)?,
            Kind::RejectCommit => {
                groups::wished(&answer(response, CAP_WRITE)?)?;
                Answer::Done
            }
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
