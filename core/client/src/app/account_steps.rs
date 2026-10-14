//! The account's steps of a device that is in its room: removing devices (`RemoveDevices`: human devices by a Cut
//! each, agent devices out of `agents`), signing out (`SignOut`: the token ends at the hub, the device's state and
//! the cache are wiped here; the device stays a leaf until another device removes it, as on the web), and deleting
//! the account (`DeleteAccount`: the password proves it, the hub deletes the account and its rooms, then the device
//! signs out). Web `client.ts removeDevices`, `leaveRoom`, `account.ts deleteAccount`; iOS `RoomDevices.swift`.

use super::{log, App, Effect, LogLevel, Pending, StoreOp};
use crate::account::AccountAction;
use crate::app::io::IoId;
use crate::hub::{AccountProof, Answer, Codec, Expect, Failure, Route};
use crate::views::{AccountView, Phase, Text};
use std::collections::BTreeMap;
use trommi_core::ids::{DeviceId, GroupId};

/// What an account step's request was.
pub(super) enum Step {
    /// `GET /v1/account` of a deletion: its e-mail derives the password's key.
    ReadAccount { password: crate::account::Secret, expect: Expect },
    Delete(Expect),
    SignOut,
    /// A change of the account's ways in (`account_change`).
    Change(super::account_change::Ask, Expect),
    /// Nothing waits for the answer.
    Quiet,
    /// A file of a desk being deleted (`app::desks`).
    DeskFile { desk: String, file: String, expect: Expect },
}

#[derive(Default)]
pub(super) struct Steps {
    pub(super) asked: BTreeMap<IoId, Step>,
}

fn unhex32(text: &str) -> Option<[u8; 32]> {
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

impl App {
    /// `RemoveDevices`, `SignOut`, `DeleteAccount`: true when `action` was one of them.
    pub(super) fn account_step(&mut self, now_ms: u64, action: &AccountAction, out: &mut Vec<Effect>) -> bool {
        match action {
            AccountAction::RemoveDevices { device_ids } => self.remove_devices(now_ms, device_ids, out),
            AccountAction::SignOut => self.sign_out(now_ms, out),
            AccountAction::DeleteAccount { password } => {
                let password = password.clone();
                self.step_send(now_ms, out, |site| Codec.request(site, &Route::Account), move |expect| Step::ReadAccount { password, expect });
                self.next_view.account = AccountView { step: "working".into(), ..Default::default() };
            }
            _ => return false,
        }
        true
    }

    pub(super) fn step_send(
        &mut self,
        now_ms: u64,
        out: &mut Vec<Effect>,
        build: impl FnOnce(&crate::hub::Site<'_>) -> Result<(crate::app::io::HttpRequest, Expect), Failure> + 'static,
        step: impl FnOnce(Expect) -> Step,
    ) -> bool {
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
                self.steps.asked.insert(id, step(expect));
                true
            }
            _ => {
                self.next_view.account = AccountView { step: "none".into(), problem: Some(Text::new("account.not_sent", "That could not be sent. Try again.")), ..Default::default() };
                false
            }
        }
    }

    fn remove_devices(&mut self, now_ms: u64, device_ids: &[String], out: &mut Vec<Effect>) {
        let Some(engine) = self.engine.as_ref() else { return };
        let (humans, agents) = engine.roles();
        let me = engine.device().id();
        let ids: Vec<DeviceId> = device_ids.iter().filter_map(|d| unhex32(d)).map(DeviceId::new).filter(|d| *d != me).collect();
        let cut_humans: Vec<DeviceId> = ids.iter().filter(|d| humans.contains(d)).copied().collect();
        let gone_agents: Vec<DeviceId> = ids.iter().filter(|d| agents.contains(d)).copied().collect();
        let room = GroupId::room(engine.room());
        let mut failed = None;
        if !cut_humans.is_empty() {
            let done = self.with_engine(now_ms, out, |engine, ctx| {
                engine.act(ctx, |d, now| {
                    let cuts = cut_humans.iter().map(|h| d.cut_of(&room, h)).collect::<Result<Vec<_>, _>>()?;
                    d.remove_human_devices(&cuts, now)
                })
            });
            if let Some(Err(e)) = done {
                failed = Some(e.code());
            }
        }
        if failed.is_none() && !gone_agents.is_empty() {
            if let Some(Err(e)) = self.with_engine(now_ms, out, |engine, ctx| engine.act(ctx, |d, now| d.remove_agents(&gone_agents, now))) {
                failed = Some(e.code());
            }
        }
        if let Some(code) = failed {
            let text = Text::new("devices.not_removed", &format!("The device was not removed ({code})."));
            self.sent.toast(&mut self.next_view.toasts, text, None);
        }
    }

    /// Signs out: the token ends at the hub, and everything this slot stored goes.
    fn sign_out(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.engine.is_none() {
            return;
        }
        self.step_send(now_ms, out, |site| Codec.request(site, &Route::SignOut), |_| Step::SignOut);
        let wiped = self.with_engine(now_ms, out, |engine, ctx| engine.wipe(ctx));
        if wiped != Some(true) {
            out.push(log(LogLevel::Warn, "sign out: the device's state was not wiped"));
        }
        // (the engine goes; what it still has in flight is only logged when it comes)
        if let Some(engine) = self.engine.as_mut() {
            for id in engine.forget() {
                self.pending.insert(id, Pending::Dropped);
            }
        }
        self.engine = None;
        self.cache = None;
        self.board = crate::engine::feed::Board::default();
        self.stale_views = true;
        self.next_view.phase = Phase::SignedOut;
        self.next_view.account = AccountView { step: "sign_in".into(), ..Default::default() };
        // the cache space too: every key, read first
        let id = self.io(Pending::Wipe);
        out.push(Effect::Store { id, op: StoreOp::Scan { prefix: String::new() } });
    }

    /// The keys the cache space holds, all to go.
    pub(super) fn wipe_cache(&mut self, keys: Vec<String>, out: &mut Vec<Effect>) {
        if keys.is_empty() {
            return;
        }
        let id = self.io(Pending::CacheDelete);
        out.push(Effect::Store { id, op: StoreOp::Put { put: Vec::new(), delete: keys } });
    }

    pub(super) fn step_answered(&mut self, now_ms: u64, id: IoId, result: &Result<crate::app::io::HttpResponse, Failure>, out: &mut Vec<Effect>) {
        let Some(step) = self.steps.asked.remove(&id) else { return };
        let answer = |expect: &Expect| match result {
            Ok(response) => Codec.answer(expect, response),
            Err(failure) => Err(failure.clone()),
        };
        match step {
            Step::SignOut | Step::Quiet => {}
            Step::DeskFile { desk, file, expect } => {
                let gone = match answer(&expect) {
                    Ok(_) => true,
                    Err(Failure::Refused { code: Some(code), .. }) => code == "not-found",
                    Err(_) => false,
                };
                self.desk_file_gone(&desk, file, gone);
            }
            Step::Change(ask, expect) => {
                let answer = answer(&expect);
                self.change_answered(now_ms, ask, answer, out);
            }
            Step::ReadAccount { password, expect } => {
                let email = match answer(&expect) {
                    Ok(Answer::Account(view)) => view.email,
                    _ => None,
                };
                let Some(email) = email else {
                    self.next_view.account = AccountView { step: "none".into(), problem: Some(Text::new("account.no_email", "This account has no e-mail and password: delete it on a device with its passkey.")), ..Default::default() };
                    return;
                };
                let Ok(keys) = trommi_core::account::password_keys(&email, &password.0, None) else {
                    self.next_view.account = AccountView { step: "none".into(), problem: Some(Text::new("account.wrong_login", "The e-mail or the password is wrong.")), ..Default::default() };
                    return;
                };
                let proof = AccountProof::Password { auth_key: *keys.auth_key.expose() };
                drop(keys);
                self.step_send(now_ms, out, move |site| Codec.request(site, &Route::DeleteAccount { proof: &proof }), Step::Delete);
            }
            Step::Delete(expect) => match answer(&expect) {
                Ok(_) => {
                    self.sign_out(now_ms, out);
                    self.next_view.account = AccountView { step: "found".into(), ..Default::default() };
                }
                Err(Failure::Refused { code: Some(code), .. }) if code == "wrong-login" => {
                    self.next_view.account = AccountView { step: "none".into(), problem: Some(Text::new("account.wrong_login", "The e-mail or the password is wrong.")), ..Default::default() };
                }
                Err(_) => {
                    self.next_view.account = AccountView { step: "none".into(), problem: Some(Text::new("account.not_deleted", "The account was not deleted. Try again.")), ..Default::default() };
                }
            },
        }
    }
}
