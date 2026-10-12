//! Joining a room by an agent invite link (spec/v1.md 12.1): the link names the hub, the room and a secret; this
//! device answers the inviter's Offer with a KeyPackage of its own, shows the six emoji of the check code, and is
//! in once the human confirmed them in the app and the inviter enrolled it.
//!
//! The link is used once and never stored. What is stored is what the device must hold the room to afterwards:
//! which device invited it, and the room epoch and state it starts following the room group at.
use crate::client::{hub_for, put_room_record, sleep, RoomRecord};
use crate::error::{Fault, Result};
use crate::hub::{b64, unb64, Hub};
use crate::keeper::Keeper;
use crate::store::Journal;
use crate::util::{hex, now_ms};
use crate::vault::Vault;
use serde_json::{json, Value};
use std::sync::Arc;
use trommi_core::invite::{CheckCode, InviteLink, Offer, Role, SignedOffer, SignedReveal};

/// The room and hub an invite link names, without using it: `(room id as base64url, hub address)`.
pub fn room_of_link(link: &str) -> Result<(String, String)> {
    let link = InviteLink::parse(link.trim())?;
    Ok((link.room_id.to_base64url(), link.hub.as_str().to_string()))
}

/// Checks a pasted link before anything is asked of a hub or written: its form, and its deadline by this
/// machine's clock (with the protocol's two minutes of tolerance). An expired link is `invite-expired`, a
/// deadline further ahead than any invite lives is `bad-invite`.
pub fn check_link(link: &str, now_ms: u64) -> Result<()> {
    let link = InviteLink::parse(link.trim())?;
    link.check_deadline(now_ms).map_err(|error| {
        let fault = Fault::from(error);
        let message = match fault.code.as_str() {
            "invite-expired" => format!(
                "this invite link expired {} ago; make a new invite in the Trommi app",
                ago(now_ms.saturating_sub(link.expires_at))
            ),
            "bad-invite" => {
                "this invite link's deadline is further ahead than an invite lives (is this machine's clock right?)"
                    .to_string()
            }
            _ => return fault,
        };
        Fault::new(fault.code, message)
    })
}

/// The longest waits before a join step that did not reach the hub is asked again; after them the join gives up.
const JOIN_RETRY_MS: [u64; 3] = [1000, 2000, 4000];

/// Runs a join step, asking again with the same bytes while the hub is not reached or busy, a few times
/// and never past `until`: a network blip never burns the invite. `posted`: the step writes, so an
/// `invite-used` after a lost answer may be this device's own Request (the Reveal decides). What is still
/// `offline` after the last try says so, and that the same link may be tried again.
async fn steady<F, Fut>(until: u64, poll_ms: u64, posted: bool, step: F) -> Result<Value>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<Value>>,
{
    let mut maybe_in = false;
    for tried in 0.. {
        match step().await {
            Ok(v) => return Ok(v),
            Err(fault) if posted && maybe_in && fault.code == "invite-used" => {
                return Ok(Value::Null)
            }
            Err(fault) if crate::hub::is_transient(&fault) => {
                maybe_in = true;
                let Some(wait) = JOIN_RETRY_MS.get(tried) else {
                    return Err(unreached(fault));
                };
                if now_ms() > until {
                    return Err(unreached(fault));
                }
                // (the pace of the join's own polls, doubling: a test's short one keeps it short)
                sleep((*wait).min(poll_ms.max(1) << tried)).await;
            }
            Err(fault) => return Err(fault),
        }
    }
    unreachable!()
}

/// A join step that never got through: the link is not used up.
fn unreached(fault: Fault) -> Fault {
    if fault.code != "offline" {
        return fault;
    }
    Fault::new(
        "offline",
        format!(
            "{}; the invite link is not used up, try the same link again",
            fault.message
        ),
    )
}

fn ago(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0 => "just now".to_string(),
        1..=119 => format!("{minutes} min"),
        _ => format!("{} h", minutes / 60),
    }
}

/// Answers the invite `link` with a new device in the empty `journal` and waits until the inviter enrolled it.
/// `on_code` is handed the check code as soon as the inviter revealed it; the human compares it in the app.
/// `poll_ms` is the pause between questions to the hub, `timeout_ms` how long the whole join may take.
///
/// On success the journal holds the device and the room record, and [`crate::client::Client::open`] takes over.
/// On any failure the journal is wiped: a half-made device is of no use.
pub async fn join_room(
    link: &str,
    journal: Journal,
    on_code: impl Fn(&CheckCode) + Send,
    poll_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    join_room_with(link, journal, on_code, |_| {}, poll_ms, timeout_ms).await
}

/// [`join_room`], and `on_request` is handed the hash of this device's Request before it leaves (12.1; a self-join
/// passes it on to the human devices through its voucher, 12.4).
pub async fn join_room_with(
    link: &str,
    journal: Journal,
    on_code: impl Fn(&CheckCode) + Send,
    on_request: impl Fn(&trommi_core::ids::Hash32) + Send,
    poll_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let outcome = answer_invite(link, &journal, on_code, on_request, poll_ms, timeout_ms).await;
    if outcome.is_err() {
        let _ = journal.wipe();
    }
    outcome
}

async fn answer_invite(
    link: &str,
    journal: &Journal,
    on_code: impl Fn(&CheckCode) + Send,
    on_request: impl Fn(&trommi_core::ids::Hash32) + Send,
    poll_ms: u64,
    timeout_ms: u64,
) -> Result<()> {
    let link_text = link.trim().to_string();
    let link = InviteLink::parse(&link_text)?;
    let until = now_ms() + timeout_ms;
    let open = Hub::new(link.hub.as_str(), None, None)?;
    // The new device: its key, its KeyPackage and its Request are written before the Request leaves (13.2).
    let for_vault = journal.clone();
    let vault = Arc::new(Keeper::spawn(move || Vault::create(for_vault))?);
    let outcome = async {
        // The link as the device reads it, before the Offer is fetched: its deadline, and that this device
        // holds no room yet.
        let for_link = link_text.clone();
        let joining = vault
            .call(move |v: &mut Vault| v.device.join_link(&for_link, now_ms()))
            .await??;
        let invite_id = joining.invite_id.to_base64url();
        let offer_path = format!("/v1/invites/{invite_id}");
        let offer = steady(until, poll_ms, false, || {
            open.open_call(reqwest::Method::GET, &offer_path, None)
        })
        .await?;
        // The MAC binds the Offer to this link; a hub that serves none leaves it empty and the device
        // refuses the Offer (bad-invite).
        let offer = SignedOffer {
            offer: unb64(&offer, "offer")?,
            signature: unb64(&offer, "signature")?,
            mac: unb64(&offer, "mac").unwrap_or_default(),
        };
        // What the Offer says is checked by the device below; it is read here only for what this side stores.
        let told = Offer::decode(&offer.offer)?;
        let now = now_ms();
        let request = vault
            .call(move |v: &mut Vault| -> Result<_> {
                let request = v.device.join_request(&link_text, &offer, now)?;
                Ok(request)
            })
            .await??;
        if request.role != Role::Agent {
            return Err(Fault::new(
                "bad-invite",
                "this link invites a human device; the connector needs an agent invite",
            ));
        }
        let record = RoomRecord {
            hub: link.hub.as_str().to_string(),
            app: link.app().to_string(),
            room: link.room_id.to_base64url(),
            invited_session: if told.session_id.is_zero() {
                String::new()
            } else {
                hex(told.session_id.as_bytes())
            },
            room_epoch: told.room_epoch,
            enrolled: false,
        };
        put_room_record(journal, &record);
        journal.commit()?;

        let signed = &request.signed_request;
        on_request(&trommi_core::invite::request_hash(signed)?);
        let body = json!({
            "request": b64(&signed.request), "mac": b64(&signed.mac),
            "signature": b64(&signed.signature),
        });
        // The same bytes again after a lost answer: the hub answers a repeated Request as the first one, or
        // calls the invite used once the app revealed it (then the Reveal below says whose it took).
        let request_path = format!("/v1/invites/{invite_id}/request");
        steady(until, poll_ms, true, || {
            open.open_call(reqwest::Method::POST, &request_path, Some(&body))
        })
        .await?;

        // The inviter's Reveal: the code both sides show.
        let reveal = loop {
            match open
                .open_call(
                    reqwest::Method::GET,
                    &format!("/v1/invites/{invite_id}/reveal"),
                    None,
                )
                .await
            {
                Ok(reveal) => break reveal,
                Err(fault) if fault.code == "not-found" || crate::hub::is_transient(&fault) => {}
                Err(fault) => return Err(fault),
            }
            if now_ms() > until {
                return Err(Fault::new(
                    "invite-expired",
                    "the app did not answer this invite in time",
                ));
            }
            sleep(poll_ms).await;
        };
        let reveal = SignedReveal {
            reveal: unb64(&reveal, "reveal")?,
            signature: unb64(&reveal, "signature")?,
        };
        let code = vault
            .call(move |v: &mut Vault| -> Result<CheckCode> {
                let code = v.device.join_reveal(&reveal)?;
                v.commit()?;
                Ok(code)
            })
            .await??;
        on_code(&code);

        // The hub lets the device sign in once the inviter's Commit names its key (12.3.2): until then it
        // answers `not-member`. That the inviter made that Commit is the device's check when the room
        // group's log brings it (12.1.6).
        let hub = hub_for(&vault, link.hub.as_str(), link.room_id)?;
        loop {
            match hub.get("/v1/welcomes").await {
                Ok(_) => return Ok(()),
                Err(fault) if fault.code == "not-member" || crate::hub::is_transient(&fault) => {}
                Err(fault) => return Err(fault),
            }
            if now_ms() > until {
                return Err(Fault::new(
                    "invite-expired",
                    "the human did not confirm the six emoji in time; make a new invite in the Trommi app",
                ));
            }
            sleep(poll_ms).await;
        }
    }
    .await;
    // The device object goes before the client opens the same state.
    match Arc::try_unwrap(vault) {
        Ok(keeper) => keeper.close(),
        Err(_) => return Err(Fault::new("internal", "the joining device is still in use")),
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn offline() -> Fault {
        Fault::new("offline", "the hub could not be reached")
    }

    #[tokio::test]
    async fn a_step_that_did_not_reach_the_hub_is_asked_again() {
        let calls = AtomicUsize::new(0);
        let got = steady(now_ms() + 60_000, 5, false, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err(offline())
                } else {
                    Ok(json!({"offer": "x"}))
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(got["offer"], "x");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_request_whose_answer_was_lost_may_find_the_invite_used() {
        let calls = AtomicUsize::new(0);
        let step = || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(offline())
                } else {
                    Err(Fault::new("invite-used", "revealed"))
                }
            }
        };
        assert_eq!(
            steady(now_ms() + 60_000, 5, true, step).await.unwrap(),
            Value::Null
        );
        // a first answer that says used is a refusal, and a read is never taken for a post
        let first = steady(now_ms() + 60_000, 5, true, || async {
            Err::<Value, _>(Fault::new("invite-used", "used"))
        })
        .await;
        assert_eq!(first.unwrap_err().code, "invite-used");
        let calls = AtomicUsize::new(0);
        let read = steady(now_ms() + 60_000, 5, false, || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            async move {
                if n == 0 {
                    Err(offline())
                } else {
                    Err(Fault::new("invite-used", "used"))
                }
            }
        })
        .await;
        assert_eq!(read.unwrap_err().code, "invite-used");
    }

    #[tokio::test]
    async fn a_hub_never_reached_says_so_and_that_the_link_is_not_used_up() {
        let calls = AtomicUsize::new(0);
        let err = steady(now_ms() + 60_000, 5, true, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<Value, _>(offline()) }
        })
        .await
        .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 1 + JOIN_RETRY_MS.len());
        assert_eq!(err.code, "offline");
        assert!(err.message.contains("not used up"), "{}", err.message);
        // a refusal is never asked again
        let calls = AtomicUsize::new(0);
        let err = steady(now_ms() + 60_000, 5, false, || {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Err::<Value, _>(Fault::new("invite-expired", "ran out")) }
        })
        .await
        .unwrap_err();
        assert_eq!(
            (err.code.as_str(), calls.load(Ordering::SeqCst)),
            ("invite-expired", 1)
        );
    }
}
