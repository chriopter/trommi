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
use crate::views::{AccountView, EmojiView, InviteView, Text};
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

/// A link the person allowed for an agent's ask to let in a new session of its machine (12.4): its Request is let
/// in without the emoji when the ask names its hash and the voucher still stands.
#[derive(Clone, Debug)]
pub(super) struct Vouch {
    pub(super) nonce: String,
    pub(super) session_id: String,
    pub(super) account: String,
    pub(super) account_label: String,
    pub(super) by_name: String,
    pub(super) machine: Option<String>,
}

pub(super) struct Inviting {
    role: Role,
    computer: bool,
    pub(super) vouch: Option<Vouch>,
    /// The link, kept for a vouched one until it is written for the asking agent.
    link: Option<String>,
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

/// An invite of this device whose newcomer came in: shown as joined until it is dropped or another is made.
#[derive(Clone, Debug)]
struct Joined {
    invite: InviteId,
    role: Role,
    newcomer: DeviceId,
    label: String,
    computer: bool,
}

#[derive(Default)]
pub(super) struct Invites {
    open: BTreeMap<InviteId, Inviting>,
    asked: BTreeMap<IoId, InviteAsk>,
    joined: Option<Joined>,
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
            AccountAction::InviteDevice => {
                self.invite_open(now_ms, Role::Human, String::new(), None, None, out);
            }
            AccountAction::InviteAgent { label, desk } => {
                self.invite_open(now_ms, Role::Agent, label.clone(), desk.clone(), None, out);
            }
            AccountAction::ApproveComputer { label } => {
                if self.invite_open(now_ms, Role::Agent, label.clone(), None, None, out) {
                    if let Some(id) = self.next_view.account.invite_id.as_deref().and_then(unhex).map(InviteId::new) {
                        if let Some(invite) = self.invites.open.get_mut(&id) { invite.computer = true; }
                    }
                    if let Some(link) = self.next_view.account.link.as_deref() {
                        let (prompt, command) = connect_words(link, true);
                        self.next_view.account.connect_prompt = Some(prompt);
                        self.next_view.account.connect_command = Some(command);
                    }
                }
            }
            AccountAction::ConfirmEmoji { invite_id, matches } => self.invite_confirm(now_ms, invite_id, *matches, out),
            AccountAction::DropInvite { invite_id } => self.invite_drop(now_ms, invite_id, out),
            _ => return false,
        }
        true
    }

    pub(super) fn invite_open(&mut self, now_ms: u64, role: Role, label: String, desk: Option<String>, vouch: Option<Vouch>, out: &mut Vec<Effect>) -> bool {
        let shown = vouch.is_none();
        if self.demo {
            // A made-up invite for /screens; no protocol state, usable secret, hub or storage is created.
            let invite = InviteId::new([0; 16]);
            let link = "https://app.trommi.com/#demo-invite".to_string();
            self.invites.open.clear();
            self.invites.joined = None;
            self.invites.open.insert(invite, Inviting { role, computer:false, vouch:None, link:None, label, desk,
                expires_at:now_ms + role.invite_life_ms(), stage:Stage::Open,
                next_look:u64::MAX, asked:None });
            let (prompt, command) = connect_words(&link, false);
            self.next_view.account = AccountView { step:"invite".into(), invite_id:Some(hex(invite.as_bytes())),
                link:Some(link), connect_prompt:(role == Role::Agent).then_some(prompt),
                connect_command:(role == Role::Agent).then_some(command), ..Default::default() };
            return true;
        }
        let Some(engine) = self.engine.as_ref() else {
            self.next_view.account = problem("action.no_room", "Sign in first.");
            return false;
        };
        let Ok(hub) = HubAddress::parse(engine.hub()) else { return false };
        let opened = self.with_engine(now_ms, out, |engine, ctx| engine.act(ctx, |d, now| d.invite_open(role, None, INVITE_APP, &hub, now)));
        let opened = match opened {
            Some(Ok(opened)) => opened,
            Some(Err(_)) if !shown => return false,
            Some(Err(e)) => {
                self.next_view.account = match e.code() {
                    "too-many" => problem("invite.too_many", "Too many invites are open. Wait until one is used or ends."),
                    "forbidden" => problem("invite.forbidden", "Only a person's device can invite."),
                    code => problem("invite.failed", &format!("No invite could be made ({code}).")),
                };
                return false;
            }
            None => return false,
        };
        // (one agent link at a time: a newer one ends this device's earlier one nobody answered yet)
        if role == Role::Agent && shown {
            let old: Vec<InviteId> = self.invites.open.iter().filter(|(_, i)| i.role == Role::Agent && i.vouch.is_none() && matches!(i.stage, Stage::Open | Stage::Publishing)).map(|(id, _)| *id).collect();
            for id in old {
                self.invites.open.remove(&id);
                self.invite_send(now_ms, out, move |site| Codec.request(site, &Route::DeleteInvite { invite: &id }), |_| InviteAsk::Burn);
            }
        }
        let invite = opened.invite_id;
        if shown {
            self.invites.joined = None;
        }
        let link = String::from_utf8(opened.link.expose().to_vec()).unwrap_or_default();
        let offer = opened.signed_offer;
        self.invites.open.insert(invite, Inviting { role, computer: false, vouch, link: (!shown).then(|| link.clone()), label, desk, expires_at: opened.expires_at, stage: Stage::Publishing, next_look: now_ms + LOOK_MS, asked: None });
        let asked = self.invite_send(
            now_ms,
            out,
            move |site| Codec.request(site, &Route::PostInvite { offer: &offer.offer, signature: &offer.signature, mac: &offer.mac }),
            |expect| InviteAsk::Publish(invite, expect),
        );
        if let Some(i) = self.invites.open.get_mut(&invite) {
            i.asked = asked;
        }
        if shown {
            let (prompt, command) = connect_words(&link, false);
            self.next_view.account = AccountView {
                step: "invite".into(), invite_id: Some(hex(invite.as_bytes())), link: Some(link),
                connect_prompt: (role == Role::Agent).then_some(prompt),
                connect_command: (role == Role::Agent).then_some(command), ..Default::default()
            };
        }
        true
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
                    Ok(_) => {
                        i.stage = Stage::Open;
                        // (a vouched link goes to the asking agent now that the hub holds its Offer)
                        if let (Some(v), Some(link)) = (i.vouch.clone(), i.link.take()) {
                            let value = json!({ "link": link, "invite_id": hex(invite.as_bytes()) });
                            self.doing(now_ms, out, |doing| {
                                doing.session_register(&v.session_id, &format!("join_link/{}", v.nonce), &value);
                            });
                        }
                    }
                    Err(_) => {
                        let vouch = self.invites.open.remove(&invite).and_then(|i| i.vouch);
                        match vouch {
                            Some(v) => self.doing(now_ms, out, |doing| {
                                doing.session_register(&v.session_id, &format!("join_link/{}", v.nonce), &json!({ "refused": "busy" }));
                            }),
                            None => self.next_view.account = problem("invite.not_published", "The invite did not reach the hub. Try again."),
                        }
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
                            let mut vouched = false;
                            if let Some(i) = self.invites.open.get_mut(&invite) {
                                i.asked = asked;
                                i.stage = Stage::Compare { code, request_hash: *accepted.request_hash.as_bytes(), newcomer: accepted.new_device };
                                vouched = i.vouch.is_some();
                            }
                            // (a vouched one is let in by the ask's word, not the emoji: `selfjoin_tend`)
                            if vouched {
                                return self.selfjoin_tend(now_ms, out);
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

    pub(super) fn invite_confirm(&mut self, now_ms: u64, invite_id: &str, matches: bool, out: &mut Vec<Effect>) {
        let vouched = unhex(invite_id).map(InviteId::new).and_then(|id| self.invites.open.get(&id)).is_some_and(|i| i.vouch.is_some());
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
            if !vouched {
                self.next_view.account = problem("invite.mismatch", "The emoji do not match: nobody was added, and the link is spent.");
            }
            return;
        }
        match confirmed {
            Some(Ok(_)) => {
                if let Some(i) = self.invites.open.get_mut(&invite) {
                    i.stage = Stage::Adding { newcomer };
                }
                if !vouched {
                    self.next_view.account = AccountView { step: "working".into(), invite_id: Some(invite_id.to_string()), ..Default::default() };
                }
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
            let device = hex(newcomer.as_bytes());
            let session = self.board.model.sessions.iter().find(|(_, s)| s.agent_device_id.as_deref() == Some(device.as_str()) && s.parent_session_id.is_none()).map(|(k, _)| k.clone());
            let needs_session = self.invites.open.get(&invite).is_some_and(|i| i.role == Role::Agent && (i.computer || !i.label.is_empty() || i.desk.is_some() || i.vouch.is_some()));
            // Keep the human's approval until the authenticated main session is available to bind it to.
            if inside && needs_session && session.is_none() { continue }
            let Some(i) = self.invites.open.remove(&invite) else { continue };
            if !inside {
                self.next_view.account = problem("invite.removed", "The new device is not in the room.");
                continue;
            }
            if needs_session {
                if let Some(session) = session {
                    let mut place = serde_json::Map::new();
                    if !i.label.is_empty() {
                        place.insert("name".into(), json!(i.label));
                    }
                    if let Some(desk) = &i.desk {
                        place.insert("desk".into(), json!(desk));
                    }
                    if let Some(v) = &i.vouch {
                        let mut vouched = json!({ "by": v.session_id, "by_name": v.by_name, "account": v.account, "account_label": v.account_label });
                        if let Some(machine) = &v.machine {
                            vouched["machine"] = json!(machine);
                            vouched["device"] = json!(device);
                        }
                        place.insert("vouched".into(), vouched);
                    }
                    if i.computer {
                        place.insert("machine".into(), json!({ "device": device }));
                    }
                    self.doing(now_ms, out, |doing| {
                        doing.session_settings(&session, place);
                        if i.computer {
                            doing.session_register(&session, "machine_approved", &json!({ "device": device }));
                        }
                    });
                }
            }
            if i.vouch.is_none() {
                self.invites.joined = Some(Joined { invite, role: i.role, newcomer, label: i.label.clone(), computer: i.computer });
                self.next_view.account = AccountView { step: "none".into(), ..Default::default() };
            }
        }
    }

    /// `DropInvite`: an open invite of this device is burned at the hub; a joined one leaves the view.
    fn invite_drop(&mut self, now_ms: u64, invite_id: &str, out: &mut Vec<Effect>) {
        let Some(invite) = unhex(invite_id).map(InviteId::new) else { return };
        if self.invites.joined.as_ref().is_some_and(|j| j.invite == invite) {
            self.invites.joined = None;
        }
        // (one being added is past the hub's part: the engine finishes it)
        let open = self.invites.open.get(&invite).is_some_and(|i| i.vouch.is_none() && !matches!(i.stage, Stage::Adding { .. }));
        if open {
            self.invites.open.remove(&invite);
            self.invite_send(now_ms, out, move |site| Codec.request(site, &Route::DeleteInvite { invite: &invite }), |_| InviteAsk::Burn);
        }
        if self.next_view.account.invite_id.as_deref() == Some(invite_id) || self.next_view.account.step == "invite" {
            self.next_view.account = AccountView { step: "none".into(), ..Default::default() };
        }
    }

    /// The invite the account's screen shows (`AccountView.invite`): the one in its step, else the one that joined.
    pub(super) fn invite_view(&self) -> Option<InviteView> {
        let model = &self.board.model;
        let name = |newcomer: &DeviceId, role: Role, label: &str| {
            let device = hex(newcomer.as_bytes());
            let named = crate::sessions::device_name_of(model, Some(&device));
            if role == Role::Agent && !label.is_empty() {
                label.to_string()
            } else {
                named
            }
        };
        let role_of = |role: Role| if role == Role::Agent { "agent" } else { "human" }.to_string();
        let words = |role, computer| {
            if computer {
                (Text::new("invite.computer", "Approve a computer"), Text::new("invite.computer_notice", &format!("Approving this computer lets its local agent sessions connect without another tap, up to {} at once. Remove this computer to remove those sessions too.", crate::selfjoin::MAX_VOUCHED)))
            } else if role == Role::Agent {
                (Text::new("invite.agent", "Invite an agent"), Text::new("invite.agent_notice", "Connect this one session by comparing six emoji."))
            } else {
                (Text::new("invite.human", "Pair a device"), Text::new("invite.human_notice", "Compare the six emoji on both devices."))
            }
        };
        let shown = self.next_view.account.invite_id.as_deref().and_then(unhex).map(InviteId::new);
        if let Some((id, i)) = shown.and_then(|id| self.invites.open.get(&id).map(|i| (id, i))).filter(|(_, i)| i.vouch.is_none()) {
            let (state, newcomer) = match &i.stage {
                Stage::Publishing | Stage::Open => ("open", String::new()),
                Stage::Compare { newcomer, .. } => ("confirm_code", name(newcomer, i.role, &i.label)),
                Stage::Adding { newcomer } => ("adding", name(newcomer, i.role, &i.label)),
            };
            let (title, notice) = words(i.role, i.computer);
            return Some(InviteView { invite_id: hex(id.as_bytes()), title, notice, computer: i.computer, role: role_of(i.role), state: state.into(), expires_ms: i.expires_at, newcomer });
        }
        let j = self.invites.joined.as_ref()?;
        let (title, notice) = words(j.role, j.computer);
        Some(InviteView { invite_id: hex(j.invite.as_bytes()), title, notice, computer: j.computer, role: role_of(j.role), state: "joined".into(), expires_ms: 0, newcomer: name(&j.newcomer, j.role, &j.label) })
    }

    /// The state of each vouched link open here, by its invite's id (hex): a self-join that is under way.
    pub(super) fn vouched_states(&self) -> BTreeMap<String, String> {
        self.invites
            .open
            .iter()
            .filter(|(_, i)| i.vouch.is_some())
            .map(|(id, i)| {
                let state = match i.stage {
                    Stage::Publishing | Stage::Open => "open",
                    Stage::Compare { .. } => "confirm_code",
                    Stage::Adding { .. } => "adding",
                };
                (hex(id.as_bytes()), state.to_string())
            })
            .collect()
    }

    /// The vouched links open here: the accounts they are for (the bounds count them), and those whose Request waits
    /// for the ask's word, with its hash (base64url).
    pub(super) fn vouched_open(&self) -> (Vec<String>, Vec<(String, Vouch, String)>) {
        let mut accounts = Vec::new();
        let mut waiting = Vec::new();
        for (id, i) in &self.invites.open {
            let Some(v) = &i.vouch else { continue };
            accounts.push(v.account.clone());
            if let Stage::Compare { request_hash, .. } = &i.stage {
                waiting.push((hex(id.as_bytes()), v.clone(), trommi_core::ids::base64url_encode(request_hash)));
            }
        }
        (accounts, waiting)
    }
}

pub(super) fn emoji_of(code: [u8; 6]) -> Vec<EmojiView> {
    code.iter()
        .map(|n| {
            let (emoji, word) = trommi_core::invite::CHECK_EMOJI.get(usize::from(*n)).copied().unwrap_or_default();
            EmojiView { emoji: emoji.into(), word: word.into() }
        })
        .collect()
}

fn connect_words(link: &str, computer: bool) -> (String, String) {
    let quoted = format!("'{}'", link.replace('\'', "'\\''"));
    if computer {
        (
            format!("Approve this computer in Trommi with this invite link: {quoted}. Call the Trommi connect tool with this link and computer=true."),
            format!("/trommi:connect --computer {quoted}"),
        )
    } else {
        (format!("Connect to Trommi with this invite link: {quoted}"), format!("/trommi:connect {quoted}"))
    }
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

#[cfg(test)]
mod computer_tests {
    use super::*;

    #[test]
    fn copyable_invite_instructions_are_redacted_with_the_link() {
        let (prompt, command) = connect_words("private-invite-secret", true);
        let view = AccountView { link: Some("private-invite-secret".into()), connect_prompt: Some(prompt), connect_command: Some(command), ..Default::default() };
        assert!(!format!("{view:?}").contains("private-invite-secret"));
        let json = serde_json::to_value(view).unwrap();
        assert!(json["connect_prompt"].as_str().unwrap().contains("computer=true"));
        assert!(json["connect_command"].as_str().unwrap().contains("--computer"));
    }
}
