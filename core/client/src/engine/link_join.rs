//! Joining a room by an invite link, on the new device's side (spec/v1.md 12.1; web `room.ts joinRoom`): the
//! Offer is read (no token), this device's Request posted (no token), and the inviter's Reveal waited for; then
//! both screens show the same six emoji ([`LinkJoinStatus::Code`]). Once the person confirmed them on the inviter's
//! device, the hub lets this device sign in (until then `not-member`, retried by the engine's back-off), the
//! room's Welcome is read and taken, and the engine runs as for any device. Human devices only: an agent device
//! joins by the connector.
//!
//! The link holds the invite's secret: it stays in this module, never in a `Debug` or a log. A join cut off by a
//! restart is begun again by the person with the same link (the Request is answered as before).

use super::*;

/// How a join by link stands. `Joined` comes once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkJoinStatus {
    /// Reading the Offer, posting the Request, waiting for the Reveal.
    Asking,
    /// The six numbers both screens show.
    Code([u8; 6]),
    /// The person confirmed on the other device: this one is let in.
    Joined,
    /// Given up, with the code of why (`invite-expired`, `invite-used`, `code-mismatch`, …).
    Failed(&'static str),
}

pub(super) struct LinkJoin {
    link: String,
    invite: InviteId,
    step: LinkStep,
    retry: Retry,
    /// When it gives up waiting.
    until: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkStep {
    Offer,
    Request,
    Reveal,
    SignIn,
    Welcome,
}

/// How long the new device waits for the person to confirm, at most.
const WAIT_MS: u64 = 15 * 60_000;
/// How often the Reveal is looked for.
const REVEAL_EVERY_MS: u64 = 1_000;

impl LinkJoin {
    /// When the join wants the clock: its next look, and the end of its wait.
    pub(super) fn wake(&self) -> Option<u64> {
        Some(self.retry.at.map_or(self.until, |at| at.min(self.until)))
    }
}

impl Engine {
    /// A new device in an empty slot that joins the room an invite link names. Refused: the protocol core's code
    /// (`bad-invite`, `invite-expired`, `room-exists`), `bad-hub`.
    #[allow(clippy::too_many_arguments)]
    pub fn join_by_link(client: &str, seed: u64, revision: u64, entries: Vec<DeviceEntry>, link: &str, now_ms: u64, entropy: Box<dyn Entropy + Send>) -> Result<Self, &'static str> {
        let entries: Vec<(Vec<u8>, Vec<u8>)> = entries.into_iter().map(|e| (e.key.0, e.value.0)).collect();
        if revision != 0 && !entries.is_empty() {
            let (store, _) = memstore::store(revision, entries.clone());
            if Device::open(store, Box::new(SystemEntropy)).is_ok_and(|d| d.room().is_some()) {
                return Err("room-exists");
            }
        }
        let stale: Vec<Vec<u8>> = entries.into_iter().map(|(k, _)| k).collect();
        let (store, writes) = memstore::fresh(revision, stale);
        let device = Device::create(store, entropy).map_err(|e| e.code())?;
        let named = device.join_link(link, now_ms).map_err(|e| e.code())?;
        let hub = named.hub.as_str().to_string();
        let address = HubAddress::parse(&hub).map_err(|_| "bad-hub")?;
        let mut engine = Self::over(device, writes, named.room_id, &hub, address, client, seed);
        engine.link_join = Some(LinkJoin { link: link.to_string(), invite: named.invite_id, step: LinkStep::Offer, retry: Retry::default(), until: now_ms + WAIT_MS });
        Ok(engine)
    }

    /// Whether a join by link runs.
    pub fn joining_by_link(&self) -> bool {
        self.link_join.is_some()
    }

    pub(super) fn drive_link_join(&mut self, ctx: &mut Ctx<'_>) {
        self.flush(ctx);
        let Some(j) = self.link_join.as_ref() else { return };
        if ctx.now > j.until {
            return self.link_failed(ctx, "invite-expired");
        }
        if self.pending.values().any(|a| matches!(a, Asked::Link { .. })) || j.retry.waiting(ctx.now) {
            return;
        }
        let invite = j.invite;
        match j.step {
            LinkStep::Offer => {
                self.ask(ctx, Route::GetInvite { invite: &invite }, false, |expect| Asked::Link { expect });
            }
            LinkStep::Request => {}
            LinkStep::Reveal => {
                self.ask(ctx, Route::GetReveal { invite: &invite }, false, |expect| Asked::Link { expect });
            }
            LinkStep::SignIn => {
                if self.signed_in(ctx) {
                    if let Some(j) = self.link_join.as_mut() {
                        j.step = LinkStep::Welcome;
                    }
                    self.drive_link_join(ctx);
                }
            }
            LinkStep::Welcome => {
                self.ask(ctx, Route::Welcomes { after: 0 }, true, |expect| Asked::Link { expect });
            }
        }
    }

    pub(super) fn on_link_answer(&mut self, ctx: &mut Ctx<'_>, answer: Result<Answer, Failure>) {
        let Some(step) = self.link_join.as_ref().map(|j| j.step) else { return };
        let wait = |e: &mut Engine, ctx: &mut Ctx<'_>, ms: Option<u64>| {
            if let Some(j) = e.link_join.as_mut() {
                match ms {
                    Some(ms) => j.retry.wait(ctx.now, ms),
                    None => j.retry.fail(ctx.now, None, &mut e.jitter),
                }
            }
        };
        let refused = |answer: &Result<Answer, Failure>| match answer {
            Err(Failure::Refused { code: Some(code), .. }) => Some(Error::from_code(code).map_or("refused", |e| e.code())),
            Err(Failure::Refused { .. }) => Some("refused"),
            _ => None,
        };
        match (step, answer) {
            (LinkStep::Offer, Ok(Answer::Invite(offer))) => {
                let Some(link) = self.link_join.as_ref().map(|j| j.link.clone()) else { return };
                let signed = trommi_core::invite::SignedOffer { offer: offer.offer, signature: offer.signature, mac: offer.mac.unwrap_or_default() };
                match self.device.join_request(&link, &signed, ctx.now) {
                    Ok(request) => {
                        let invite = request.invite_id;
                        let r = request.signed_request;
                        if let Some(j) = self.link_join.as_mut() {
                            j.step = LinkStep::Request;
                        }
                        self.ask(ctx, Route::PostInviteRequest { invite: &invite, request: &r.request, mac: &r.mac, signature: &r.signature }, false, |expect| Asked::Link { expect });
                    }
                    Err(e) => self.link_failed(ctx, e.code()),
                }
            }
            (LinkStep::Request, Ok(_)) => {
                if let Some(j) = self.link_join.as_mut() {
                    j.step = LinkStep::Reveal;
                }
            }
            (LinkStep::Reveal, Ok(Answer::Reveal { reveal, signature })) => match self.device.join_reveal(&trommi_core::invite::SignedReveal { reveal, signature }) {
                Ok(code) => {
                    if let Some(j) = self.link_join.as_mut() {
                        j.step = LinkStep::SignIn;
                    }
                    self.report(Report::LinkJoin(LinkJoinStatus::Code(code.numbers())));
                }
                Err(e) => self.link_failed(ctx, e.code()),
            },
            (LinkStep::Reveal, Err(Failure::Refused { code: Some(code), .. })) if code == "not-found" => wait(self, ctx, Some(REVEAL_EVERY_MS)),
            (LinkStep::Welcome, Ok(Answer::Welcomes(rows))) => {
                let room = GroupId::room(self.room);
                let mut joined = false;
                for row in rows.iter().filter(|r| r.group == room) {
                    if self.device.join_invited(&row.welcome, ctx.now).is_ok() {
                        joined = true;
                        break;
                    }
                }
                if !joined {
                    return wait(self, ctx, Some(REVEAL_EVERY_MS));
                }
                self.link_join = None;
                self.position = self.device.cursor();
                self.todo_upto = self.position;
                self.report(Report::LinkJoin(LinkJoinStatus::Joined));
                self.report(Report::Groups);
            }
            (_, answer) => match refused(&answer) {
                // (a burned invite: the other device said the emoji do not match)
                Some("invite-burned") => self.link_failed(ctx, "code-mismatch"),
                Some(code) if step != LinkStep::Welcome => self.link_failed(ctx, code),
                _ => wait(self, ctx, None),
            },
        }
    }

    /// Given up: the device's state goes in one write.
    fn link_failed(&mut self, ctx: &mut Ctx<'_>, code: &'static str) {
        self.link_join = None;
        self.flush(ctx);
        if let Some(write) = self.writes.wipe() {
            let id = ctx.id();
            self.pending.insert(id, Asked::DeviceWrite);
            ctx.out.push(Effect::Store {
                id,
                op: StoreOp::DeviceApply { expected_revision: write.expected_revision, put: Vec::new(), delete: write.delete.into_iter().map(Into::into).collect() },
            });
        }
        ctx.log(LogLevel::Warn, format!("join by link: {code}"));
        self.report(Report::LinkJoin(LinkJoinStatus::Failed(code)));
    }
}
