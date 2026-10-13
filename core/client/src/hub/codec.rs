//! The codec's two calls: a route to its request, an answer to its typed result.

use super::json::{bad, Read};
use super::limits::{CAP_DESK, CAP_GROUPS_READ, CAP_GROUP_INFO, CAP_LIST, CAP_SMALL, CAP_WRITE};
use super::types::{Answer, ByteRange, Route, Site};
use super::{
    account, answer, changes, content, desk, files, groups, invites, outbox, push, signin, stream,
    Failure,
};
use crate::app::io::{HttpRequest, HttpResponse};
use trommi_core::ids::{GroupId, ShareId};

/// The codec's note of what was asked, kept by the engine beside the pending request and handed back with the
/// answer: the answer is checked against it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Expect(Kind);

#[derive(Clone, Debug, PartialEq, Eq)]
enum Kind {
    Changes(changes::Asked),
    Stream,
    DeskBrief {
        since: Option<u64>,
    },
    Outbox(outbox::Asked),
    KeyPackagesHeld,
    Challenge,
    Token,
    Welcomes {
        after: u64,
    },
    GroupLog(groups::LogAsked),
    GroupInfo {
        epoch: Option<u64>,
    },
    Joins {
        epochs: Vec<u64>,
    },
    GroupsRead {
        groups: Vec<GroupId>,
    },
    Claim(groups::ClaimAsked),
    RoomGroups(groups::GroupsAsked),
    Chain(content::PageAsked),
    Object(content::PageAsked),
    Wish,
    Wishes,
    RejectCommit,
    /// An answer that carries nothing but an object.
    Empty,
    Desk,
    Settled(desk::SettledAsked),
    Envelopes(content::EnvelopesAsked),
    Pruned,
    SealedKeys {
        after: u64,
        limit: u32,
    },
    Removal(groups::LogAsked),
    OpenRecovery,
    DropRecovery,
    PutFile(files::PutAsked),
    File {
        range: Option<ByteRange>,
    },
    Deleted,
    Share {
        share: ShareId,
    },
    Invite,
    Requested,
    Reveal,
    Invited,
    /// `{ "<name>": true }`.
    Said(&'static str),
    Login,
    PasskeyChallenge {
        of_account: bool,
    },
    Account,
    Usage,
    Revision,
    PasskeyAdded,
    PushState,
    Removed,
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
            Route::SignOut => (signin::sign_out_request(site)?, Kind::Empty),
            Route::Desk { only } => (desk::whole_request(site, only)?, Kind::Desk),
            Route::Settled {
                kind,
                only,
                before,
                limit,
            } => {
                let (request, asked) = desk::settled_request(site, kind, only, before, limit)?;
                (request, Kind::Settled(asked))
            }
            Route::Chat {
                timeline,
                before,
                limit,
            } => {
                let (request, asked) = content::chat_request(site, timeline, before, limit)?;
                (request, Kind::Envelopes(asked))
            }
            Route::Board {
                board,
                after,
                limit,
            } => {
                let (request, asked) = content::board_request(site, board, after, limit)?;
                (request, Kind::Envelopes(asked))
            }
            Route::BoardFrontier {
                board,
                frontier,
                files,
                snapshot,
            } => (
                content::frontier_request(site, board, frontier, files, snapshot)?,
                Kind::Pruned,
            ),
            Route::SealedKeys { after, limit } => {
                let (request, (after, limit)) = groups::sealed_keys_request(site, after, limit)?;
                (request, Kind::SealedKeys { after, limit })
            }
            Route::Removal { group, after } => {
                let (request, asked) = groups::removal_request(site, group, after)?;
                (request, Kind::Removal(asked))
            }
            Route::OpenRecovery { room } => (
                groups::open_recovery_request(site, room)?,
                Kind::OpenRecovery,
            ),
            Route::DropRecovery { room, recovery } => (
                groups::drop_recovery_request(site, room, recovery)?,
                Kind::DropRecovery,
            ),
            Route::PutFile { file, bytes } => {
                let (request, asked) = files::put_request(site, file, bytes)?;
                (request, Kind::PutFile(asked))
            }
            Route::GetFile { file, range } => {
                (files::get_request(site, file, range)?, Kind::File { range })
            }
            Route::GetShared {
                share,
                secret,
                range,
            } => (
                files::shared_request(site, share, secret, range)?,
                Kind::File { range },
            ),
            Route::DeleteFile { file } => (files::delete_file_request(site, file)?, Kind::Deleted),
            Route::DeleteShare { share } => {
                (files::delete_share_request(site, share)?, Kind::Deleted)
            }
            Route::PostShare {
                share,
                secret_hash,
                file,
                expires_at,
            } => (
                files::share_request(site, share, secret_hash, file, expires_at)?,
                Kind::Share { share: *share },
            ),
            Route::GetInvite { invite } => (invites::get_request(site, invite)?, Kind::Invite),
            Route::PostInviteRequest {
                invite,
                request,
                mac,
                signature,
            } => (
                invites::request_request(site, invite, request, mac, signature)?,
                Kind::Requested,
            ),
            Route::GetReveal { invite } => (invites::reveal_request(site, invite)?, Kind::Reveal),
            Route::PostInvite {
                offer,
                signature,
                mac,
            } => (
                invites::post_request(site, offer, signature, mac)?,
                Kind::Invited,
            ),
            Route::PutReveal {
                invite,
                reveal,
                signature,
            } => (
                invites::put_reveal_request(site, invite, reveal, signature)?,
                Kind::Said("revealed"),
            ),
            Route::DeleteInvite { invite } => {
                (invites::delete_request(site, invite)?, Kind::Said("burned"))
            }
            Route::Login { account, auth_key } => (
                account::login_request(site, false, account, auth_key)?,
                Kind::Login,
            ),
            Route::Recover { account, auth_key } => (
                account::login_request(site, true, account, auth_key)?,
                Kind::Login,
            ),
            Route::PasskeyChallenge => (
                account::passkey_challenge_request(site, false)?,
                Kind::PasskeyChallenge { of_account: false },
            ),
            Route::AccountPasskeyChallenge => (
                account::passkey_challenge_request(site, true)?,
                Kind::PasskeyChallenge { of_account: true },
            ),
            Route::PasskeyLogin { assertion } => (
                account::passkey_login_request(site, assertion)?,
                Kind::Login,
            ),
            Route::CreateAccount { account } => {
                (account::create_request(site, account)?, Kind::Account)
            }
            Route::Account => (account::view_request(site)?, Kind::Account),
            Route::AccountUsage => (account::usage_request(site)?, Kind::Usage),
            Route::PutPassword {
                password,
                revision,
                proof,
            } => (
                account::put_password_request(site, password, revision, proof)?,
                Kind::Revision,
            ),
            Route::PutKit {
                kit,
                revision,
                proof,
            } => (
                account::put_kit_request(site, kit, revision, proof)?,
                Kind::Revision,
            ),
            Route::PutEmail {
                email,
                kit,
                revision,
                proof,
            } => (
                account::put_email_request(site, email, kit, revision, proof)?,
                Kind::Revision,
            ),
            Route::AddPasskey { passkey, proof } => (
                account::add_passkey_request(site, passkey, proof)?,
                Kind::PasskeyAdded,
            ),
            Route::RemovePasskey {
                credential_id,
                proof,
            } => (
                account::remove_passkey_request(site, credential_id, proof)?,
                Kind::Said("deleted"),
            ),
            Route::DeleteAccount { proof } => {
                (account::delete_request(site, proof)?, Kind::Said("deleted"))
            }
            Route::PushRegister { registration } => (
                push::register_request(site, registration)?,
                Kind::Said("registered"),
            ),
            Route::PushState => (push::state_request(site)?, Kind::PushState),
            Route::PushRemove { endpoint } => {
                (push::remove_request(site, endpoint)?, Kind::Removed)
            }
            Route::LiveActivity { activity } => (
                push::live_activity_request(site, activity)?,
                Kind::Said("registered"),
            ),
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
            Kind::GroupsRead { groups } => {
                groups::read(groups, &answer(response, CAP_GROUPS_READ)?)?
            }
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
            Kind::Empty => {
                answer(response, CAP_WRITE)?.obj("an answer: an object")?;
                Answer::Done
            }
            Kind::Desk => Answer::Desk(desk::whole(&answer(response, CAP_DESK)?)?),
            Kind::Settled(asked) => {
                Answer::Settled(desk::settled(asked, &answer(response, CAP_LIST)?)?)
            }
            Kind::Envelopes(asked) => content::envelopes(asked, &answer(response, CAP_LIST)?)?,
            Kind::Pruned => content::pruned(&answer(response, CAP_WRITE)?)?,
            Kind::SealedKeys { after, limit } => {
                groups::sealed_keys(*after, *limit, &answer(response, CAP_LIST)?)?
            }
            Kind::Removal(asked) => groups::removal(asked, &answer(response, CAP_LIST)?)?,
            Kind::OpenRecovery => groups::recovery_opened(&answer(response, CAP_WRITE)?)?,
            Kind::DropRecovery => groups::recovery_dropped(&answer(response, CAP_WRITE)?)?,
            Kind::PutFile(asked) => files::stored(asked, &answer(response, CAP_WRITE)?)?,
            Kind::File { range } => files::bytes(*range, response)?,
            Kind::Deleted => files::deleted(&answer(response, CAP_WRITE)?)?,
            Kind::Share { share } => files::shared(share, &answer(response, CAP_WRITE)?)?,
            Kind::Invite => invites::offer(&answer(response, CAP_SMALL)?)?,
            Kind::Requested => invites::requested(&answer(response, CAP_WRITE)?)?,
            Kind::Reveal => invites::reveal(&answer(response, CAP_SMALL)?)?,
            Kind::Invited => invites::invited(&answer(response, CAP_WRITE)?)?,
            Kind::Said(name) => invites::said(name, &answer(response, CAP_WRITE)?)?,
            Kind::Login => account::login(&answer(response, CAP_SMALL)?)?,
            Kind::PasskeyChallenge { of_account } => {
                account::passkey_challenge(*of_account, &answer(response, CAP_SMALL)?)?
            }
            Kind::Account => account::view(&answer(response, CAP_SMALL)?)?,
            Kind::Usage => account::usage(&answer(response, CAP_WRITE)?)?,
            Kind::Revision => account::revised(&answer(response, CAP_WRITE)?)?,
            Kind::PasskeyAdded => account::passkey_added(&answer(response, CAP_WRITE)?)?,
            Kind::PushState => push::state(&answer(response, CAP_SMALL)?)?,
            Kind::Removed => push::removed(&answer(response, CAP_WRITE)?)?,
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
