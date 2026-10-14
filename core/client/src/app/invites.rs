//! Letting a new device in by link, on the inviter's side (spec/v1.md 12.1; web `client.ts` `createInvite`,
//! `checkInvite`, `confirmInvite`, `invitesDone`; iOS `RoomDevices.swift`): `AccountAction::InviteDevice` (a human
//! device of the person) and `InviteAgent` (an agent, which founds its main session) open an invite and publish its
//! Offer; the link shows in `view.account` (`step: "invite"`). When the newcomer's Request comes (the stream's
//! `invite_request`, or a look every few seconds) it is accepted and its Reveal published, and the six emoji show
//! (`step: "compare_emoji"`). `ConfirmEmoji` adds the newcomer by a Commit (the engine takes the invite's further
//! steps, also after a restart) or burns the invite at the hub. Once the newcomer is in, an agent's new session gets
//! the name and desk the invite was made with, and the step is `none` again.
//!
//! The link holds the invite's secret: it is in the view only (whose `Debug` leaves it out), never in a log or a
//! stored entry of the App.

use super::{log, App, Effect, LogLevel};
use crate::account::AccountAction;
use crate::app::io::IoId;
use crate::hub::{Answer, Codec, Expect, Failure, Route};
use crate::views::{AccountView, EmojiView, Text};
use serde_json::json;
use std::collections::BTreeMap;
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{DeviceId, InviteId};
use trommi_core::invite::{CheckCode, Role, SignedRequest};

/// How often an open invite looks for its Request when the stream did not say.
const LOOK_MS: u64 = 3_000;
/// The app's origin an invite link points at.
pub const INVITE_APP: &str = "https://app.trommi.com";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Stage {
    /// The Offer is being published.
    Publishing,
    /// Waits for the newcomer's Request.
    Open,
    /// The Request was accepted: the person compares the emoji.
    Compare { code: [u8; 6], request_hash: [u8; 32], newcomer: DeviceId },
    /// Confirmed: the engine adds the newcomer.
    Adding { newcomer: DeviceId },
}

pub(super) struct Inviting {
    role: Role,
    label: String,
    desk: Option<String>,
    expires_at: u64,
    stage: Stage,
    next_look: u64,
    /// A request of this invite in flight.
    asked: Option<IoId>,
}

/// What an invite's request was.
pub(super) enum InviteAsk {
    Publish(InviteId, Expect),
    Look(InviteId, Expect),
    Reveal(InviteId, Expect),
    Burn,
}

#[derive(Default)]
pub(super) struct Invites {
    open: BTreeMap<InviteId, Inviting>,
    asked: BTreeMap<IoId, InviteAsk>,
}

impl Invites {
    pub(super) fn wake(&self) -> Option<u64> {
        self.open.values().filter(|i| i.stage == Stage::Open && i.asked.is_none()).map(|i| i.next_look.min(i.expires_at)).min()
    }
    pub(super) fn owns(&self, id: IoId) -> bool {
        self.asked.contains_key(&id)
    }
}

fn problem(key: &str, en: &str) -> AccountView {
    AccountView { step: "invite".into(), problem: Some(Text::new(key, en)), ..Default::default() }
}

impl App {
    /// `InviteDevice`, `InviteAgent`, `ConfirmEmoji`: true when `action` was one of them.
    pub(super) fn invite_action(&mut self, now_ms: u64, action: &AccountAction, out: &mut Vec<Effect>) -> bool {
        match action {
            AccountAction::InviteDevice => self.invite_open(now_ms, Role::Human, String::new(), None, out),
            AccountAction::InviteAgent { label, desk } => self.invite_open(now_ms, Role::Agent, label.clone(), desk.clone(), out),
            AccountAction::ConfirmEmoji { invite_id, matches } => self.invite_confirm(now_ms, invite_id, *matches, out),
            _ => return false,
        }
        true
    }

    fn invite_open(&mut self, now_ms: u64, role: Role, label: String, desk: Option<String>, out: &mut Vec<Effect>) {
        let Some(engine) = self.engine.as_ref() else {
            self.next_view.account = problem("action.no_room", "Sign in first.");
            return;
        };
        let Ok(hub) = HubAddress::parse(engine.hub()) else { return };
        let opened = self.with_engine(now_ms, out, |engine, ctx| engine.act(ctx, |d, now| d.invite_open(role, None, INVITE_APP, &hub, now)));
        let opened = match opened {
            Some(Ok(opened)) => opened,
            Some(Err(e)) => {
                self.next_view.account = match e.code() {
                    "too-many" => problem("invite.too_many", "Too many invites are open. Wait until one is used or ends."),
                    "forbidden" => problem("invite.forbidden", "Only a person's device can invite."),
                    code => problem("invite.failed", &format!("No invite could be made ({code}).")),
                };
                return;
            }
            None => return,
        };
        // (one agent link at a time: a newer one ends this device's earlier one nobody answered yet)
        if role == Role::Agent {
            let old: Vec<InviteId> = self.invites.open.iter().filter(|(_, i)| i.role == Role::Agent && matches!(i.stage, Stage::Open | Stage::Publishing)).map(|(id, _)| *id).collect();
            for id in old {
                self.invites.open.remove(&id);
                self.invite_send(now_ms, out, move |site| Codec.request(site, &Route::DeleteInvite { invite: &id }), |_| InviteAsk::Burn);
            }
        }
        let invite = opened.invite_id;
        let link = String::from_utf8(opened.link.expose().to_vec()).unwrap_or_default();
        let offer = opened.signed_offer;
        self.invites.open.insert(invite, Inviting { role, label, desk, expires_at: opened.expires_at, stage: Stage::Publishing, next_look: now_ms + LOOK_MS, asked: None });
        let asked = self.invite_send(
            now_ms,
            out,
            move |site| Codec.request(site, &Route::PostInvite { offer: &offer.offer, signature: &offer.signature, mac: &offer.mac }),
            |expect| InviteAsk::Publish(invite, expect),
        );
        if let Some(i) = self.invites.open.get_mut(&invite) {
            i.asked = asked;
        }
        self.next_view.account = AccountView { step: "invite".into(), invite_id: Some(hex(invite.as_bytes())), link: Some(link), ..Default::default() };
    }

    /// Sends a request of the invites through the engine; the id it went under.
    fn invite_send(
        &mut self,
        now_ms: u64,
        out: &mut Vec<Effect>,
        build: impl FnOnce(&crate::hub::Site<'_>) -> Result<(crate::app::io::HttpRequest, Expect), Failure> + 'static,
        ask: impl FnOnce(Expect) -> InviteAsk,
    ) -> Option<IoId> {
        let cell = std::rc::Rc::new(std::cell::RefCell::new(None));
        let put = cell.clone();
        let sent = self.with_engine(now_ms, out, |engine, ctx| {
            engine.request(
                ctx,
                move |site| {
                    let (request, expect) = build(site)?;
                    *put.borrow_mut() = Some(expect);
                    Ok(request)
                },
                None,
            )
        });
        let expect = cell.borrow_mut().take();
        match (sent, expect) {
            (Some(Ok(id)), Some(expect)) => {
                self.invites.asked.insert(id, ask(expect));
                Some(id)
            }
            _ => {
                out.push(log(LogLevel::Warn, "invite: a request was not sent"));
                None
            }
        }
    }

    /// The invites' timer, and the stream saying a Request came: look at the hub for it.
    pub(super) fn invites_look(&mut self, now_ms: u64, came: Option<InviteId>, out: &mut Vec<Effect>) {
        let mut expired = Vec::new();
        let mut look = Vec::new();
        for (id, i) in &mut self.invites.open {
            if matches!(i.stage, Stage::Open | Stage::Publishing | Stage::Compare { .. }) && now_ms > i.expires_at {
                expired.push(*id);
            } else if i.stage == Stage::Open && i.asked.is_none() && (came == Some(*id) || now_ms >= i.next_look) {
                look.push(*id);
            }
        }
        for id in expired {
            self.invites.open.remove(&id);
            if self.next_view.account.invite_id.as_deref() == Some(hex(id.as_bytes()).as_str()) {
                self.next_view.account = problem("invite.expired", "The link ended unused. Make a new one.");
            }
        }
        for id in look {
            let asked = self.invite_send(now_ms, out, move |site| Codec.request(site, &Route::GetInvite { invite: &id }), |expect| InviteAsk::Look(id, expect));
            if let Some(i) = self.invites.open.get_mut(&id) {
                i.asked = asked;
                i.next_look = now_ms + LOOK_MS;
            }
        }
    }

    /// An answer to one of the invites' requests.
    pub(super) fn invite_answered(&mut self, now_ms: u64, id: IoId, result: &Result<crate::app::io::HttpResponse, Failure>, out: &mut Vec<Effect>) {
        let Some(ask) = self.invites.asked.remove(&id) else { return };
        let answer = |expect: &Expect| match result {
            Ok(response) => Codec.answer(expect, response),
            Err(failure) => Err(failure.clone()),
        };
        match ask {
            InviteAsk::Burn => {}
            InviteAsk::Publish(invite, expect) => {
                let Some(i) = self.invites.open.get_mut(&invite) else { return };
                i.asked = None;
                match answer(&expect) {
                    Ok(_) => i.stage = Stage::Open,
                    Err(_) => {
                        self.invites.open.remove(&invite);
                        self.next_view.account = problem("invite.not_published", "The invite did not reach the hub. Try again.");
                    }
                }
            }
            InviteAsk::Look(invite, expect) => {
                let Some(i) = self.invites.open.get_mut(&invite) else { return };
                i.asked = None;
                let Ok(Answer::Invite(offer)) = answer(&expect) else { return };
                for row in offer.requests.unwrap_or_default() {
                    let request = SignedRequest { request: row.request, mac: row.mac, signature: row.signature };
                    let accepted = self.with_engine(now_ms, out, |engine, ctx| engine.act(ctx, |d, now| d.invite_accept(&invite, &request, now)));
                    match accepted {
                        Some(Ok(accepted)) => {
                            let reveal = accepted.signed_reveal;
                            let asked = self.invite_send(
                                now_ms,
                                out,
                                move |site| Codec.request(site, &Route::PutReveal { invite: &invite, reveal: &reveal.reveal, signature: &reveal.signature }),
                                |expect| InviteAsk::Reveal(invite, expect),
                            );
                            let code = accepted.code.numbers();
                            if let Some(i) = self.invites.open.get_mut(&invite) {
                                i.asked = asked;
                                i.stage = Stage::Compare { code, request_hash: *accepted.request_hash.as_bytes(), newcomer: accepted.new_device };
                            }
                            self.next_view.account = AccountView {
                                step: "compare_emoji".into(),
                                invite_id: Some(hex(invite.as_bytes())),
                                emoji: emoji_of(code),
                                ..Default::default()
                            };
                            return;
                        }
                        Some(Err(e)) if e.code() == "invite-expired" => {
                            self.invites.open.remove(&invite);
                            self.next_view.account = problem("invite.expired", "The link ended unused. Make a new one.");
                            return;
                        }
                        _ => continue,
                    }
                }
            }
            InviteAsk::Reveal(invite, expect) => {
                if let Some(i) = self.invites.open.get_mut(&invite) {
                    i.asked = None;
                }
                if answer(&expect).is_err() {
                    out.push(log(LogLevel::Warn, "invite: the Reveal was not published; the next look tries again"));
                    if let Some(i) = self.invites.open.get_mut(&invite) {
                        i.stage = Stage::Open;
                    }
                }
            }
        }
    }

    fn invite_confirm(&mut self, now_ms: u64, invite_id: &str, matches: bool, out: &mut Vec<Effect>) {
        let Some(invite) = unhex(invite_id).map(InviteId::new) else { return };
        let Some(Stage::Compare { code, request_hash, newcomer }) = self.invites.open.get(&invite).map(|i| i.stage.clone()) else {
            self.next_view.account = problem("invite.not_waiting", "This invite waits for no emoji.");
            return;
        };
        let Ok(check) = CheckCode::from_numbers(code) else { return };
        let hash = trommi_core::ids::Hash32::new(request_hash);
        let confirmed = self.with_engine(now_ms, out, |engine, ctx| engine.act(ctx, |d, now| d.invite_confirm(&invite, &check, &hash, matches, now)));
        if !matches {
            self.invites.open.remove(&invite);
            self.invite_send(now_ms, out, move |site| Codec.request(site, &Route::DeleteInvite { invite: &invite }), |_| InviteAsk::Burn);
            self.next_view.account = problem("invite.mismatch", "The emoji do not match: nobody was added, and the link is spent.");
            return;
        }
        match confirmed {
            Some(Ok(_)) => {
                if let Some(i) = self.invites.open.get_mut(&invite) {
                    i.stage = Stage::Adding { newcomer };
                }
                self.next_view.account = AccountView { step: "working".into(), invite_id: Some(invite_id.to_string()), ..Default::default() };
            }
            Some(Err(e)) => {
                self.invites.open.remove(&invite);
                self.next_view.account = problem("invite.failed", &format!("The device was not added ({}).", e.code()));
            }
            None => {}
        }
    }

    /// After the engine's reports: an invite being added whose steps are through is done once its newcomer is in.
    pub(super) fn invites_settle(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let adding: Vec<(InviteId, DeviceId)> =
            self.invites.open.iter().filter_map(|(id, i)| if let Stage::Adding { newcomer } = &i.stage { Some((*id, *newcomer)) } else { None }).collect();
        if adding.is_empty() {
            return;
        }
        let Some(engine) = self.engine.as_mut() else { return };
        let steps: Vec<InviteId> = engine.device_mut_steps();
        let (humans, agents) = engine.roles();
        for (invite, newcomer) in adding {
            // (in the room is done for the person: its sessions follow as the engine takes the invite's steps)
            let inside = humans.contains(&newcomer) || agents.contains(&newcomer);
            if !inside && steps.contains(&invite) {
                continue;
            }
            let Some(i) = self.invites.open.remove(&invite) else { continue };
            if !inside {
                self.next_view.account = problem("invite.removed", "The new device is not in the room.");
                continue;
            }
            if i.role == Role::Agent && (!i.label.is_empty() || i.desk.is_some()) {
                let device = hex(newcomer.as_bytes());
                let session = self.board.model.sessions.iter().find(|(_, s)| s.agent_device_id.as_deref() == Some(device.as_str()) && s.parent_session_id.is_none()).map(|(k, _)| k.clone());
                if let Some(session) = session {
                    let mut place = serde_json::Map::new();
                    if !i.label.is_empty() {
                        place.insert("name".into(), json!(i.label));
                    }
                    if let Some(desk) = &i.desk {
                        place.insert("desk".into(), json!(desk));
                    }
                    self.doing(now_ms, out, |doing| doing.session_settings(&session, place));
                }
            }
            self.next_view.account = AccountView { step: "none".into(), ..Default::default() };
        }
    }
}

fn emoji_of(code: [u8; 6]) -> Vec<EmojiView> {
    code.iter()
        .map(|n| {
            let (emoji, word) = trommi_core::invite::CHECK_EMOJI.get(usize::from(*n)).copied().unwrap_or_default();
            EmojiView { emoji: emoji.into(), word: word.into() }
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}
