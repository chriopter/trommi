//! Push for this device (spec/v1.md 15): its level (`Action::Push`: off, all, only knocking), the platform's
//! registration (`PlatformOp::PushRegister`, a new token by `Event::PushToken`), the hub's registration
//! (`POST /v1/push`, `DELETE /v1/push`), and what the settings show (`view.settings.push`). As the web's `startPush`
//! and iOS `PushRegistration.swift` did it.
//!
//! - **Web Push**: the platform's token is the subscription's JSON (`{endpoint, keys: {p256dh, auth}}`); the hub's
//!   VAPID key is read first (`GET /v1/push`) and given to the platform.
//! - **APNs**: the token is `{token, environment, topic}` as JSON; the core makes the notify key (32 random bytes)
//!   that the hub seals each notification under, keeps it as the secret [`NOTIFY_KEY`] (the notification extension
//!   reads it there) and registers it with the token.
//!
//! The level of now is the hub's: read once the room is live (`GET /v1/push`), it names this device's registrations.

use super::account_steps::Step;
use super::{log, App, Effect, LogLevel};
use crate::app::bytes::Bytes;
use crate::app::io::{IoId, IoResult, PlatformAnswer, PlatformOp, SecretOp};
use crate::hub::{Answer, Codec, Failure, PushLevel, PushRegistration, PushTarget, Route};
use crate::views::{PushView, Text};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The secret that holds the APNs notify key (the notification extension opens pushes with it).
pub const NOTIFY_KEY: &str = "push-notify-key";

/// What the person chose for this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushChoice {
    Off,
    All,
    Knocking,
}

/// An HTTP step of push.
pub(super) enum Ask {
    /// `GET /v1/push`: the level of now, and the VAPID key for a registration that waits for it.
    State,
    Register,
    Remove,
}

#[derive(Default)]
pub(super) struct Push {
    /// The level asked for and not yet registered.
    want: Option<PushChoice>,
    /// The hub's VAPID key, once read.
    vapid: Option<Vec<u8>>,
    /// The platform's registration in flight.
    platform: BTreeMap<IoId, ()>,
    /// The platform's token of now (kind, bytes).
    token: Option<(String, Vec<u8>)>,
    /// The APNs notify key of this device.
    pub(super) notify_key: Option<[u8; 32]>,
    /// The level the hub has, once read.
    pub(super) level: Option<PushChoice>,
    read: bool,
}

impl Push {
    pub(super) fn owns_platform(&self, id: IoId) -> bool {
        self.platform.contains_key(&id)
    }
}

fn level_of(choice: PushChoice) -> PushLevel {
    match choice {
        PushChoice::All => PushLevel::All,
        _ => PushLevel::Knocking,
    }
}

/// The platform's token as the hub's target.
fn target_of(kind: &str, token: &[u8], notify_key: Option<[u8; 32]>) -> Option<PushTarget> {
    let v: serde_json::Value = serde_json::from_slice(token).ok()?;
    let b64 = |s: &serde_json::Value| s.as_str().and_then(|t| trommi_core::ids::base64url_decode(t.trim_end_matches('=')).ok());
    match kind {
        "web_push" => Some(PushTarget::WebPush {
            endpoint: v.get("endpoint")?.as_str()?.to_string(),
            p256dh: b64(v.get("keys")?.get("p256dh")?)?,
            auth: b64(v.get("keys")?.get("auth")?)?,
        }),
        "apns" => Some(PushTarget::Apns {
            token: v.get("token")?.as_str()?.to_string(),
            key: notify_key?.to_vec(),
            environment: v.get("environment")?.as_str()?.to_string(),
            topic: v.get("topic")?.as_str()?.to_string(),
        }),
        _ => None,
    }
}

impl App {
    /// `Action::Push`: the level for this device.
    pub(super) fn push_action(&mut self, now_ms: u64, choice: PushChoice, out: &mut Vec<Effect>) {
        let can = self.config.as_ref().map(|c| c.can.push.clone()).unwrap_or_default();
        if choice != PushChoice::Off && can.is_empty() {
            let text = Text::new("push.cannot", "This device cannot get push notifications.");
            self.sent.toast(&mut self.next_view.toasts, text, None);
            return;
        }
        if choice == PushChoice::Off {
            self.push.want = None;
            self.push_send(now_ms, out, Ask::Remove);
            let id = self.push_io();
            out.push(Effect::Platform { id, op: PlatformOp::PushUnregister });
            self.push.token = None;
            self.push.level = Some(PushChoice::Off);
            self.show_push();
            return;
        }
        self.push.want = Some(choice);
        if self.push.token.is_some() {
            return self.push_send(now_ms, out, Ask::Register);
        }
        if can == "web_push" && self.push.vapid.is_none() {
            return self.push_send(now_ms, out, Ask::State);
        }
        self.push_platform(out);
    }

    fn push_io(&mut self) -> IoId {
        let id = IoId(self.next_id);
        self.next_id += 1;
        self.push.platform.insert(id, ());
        id
    }

    /// The platform asked to register (it may ask the person first).
    fn push_platform(&mut self, out: &mut Vec<Effect>) {
        let id = self.push_io();
        out.push(Effect::Platform { id, op: PlatformOp::PushRegister { server_key: self.push.vapid.clone().map(Bytes) } });
    }

    fn push_send(&mut self, now_ms: u64, out: &mut Vec<Effect>, ask: Ask) {
        let route_of = |push: &Push| -> Option<PushRegistration> {
            let (kind, token) = push.token.as_ref()?;
            let target = target_of(kind, token, push.notify_key)?;
            Some(PushRegistration { target, level: level_of(push.want?) })
        };
        match ask {
            Ask::State => {
                self.step_send(now_ms, out, |site| Codec.request(site, &Route::PushState), |expect| Step::Push(Ask::State, expect));
            }
            Ask::Remove => {
                self.step_send(now_ms, out, |site| Codec.request(site, &Route::PushRemove { endpoint: None }), |expect| Step::Push(Ask::Remove, expect));
            }
            Ask::Register => {
                let Some(registration) = route_of(&self.push) else {
                    out.push(log(LogLevel::Warn, "push: a token the hub cannot take"));
                    let text = Text::new("push.failed", "Push could not be turned on here.");
                    self.sent.toast(&mut self.next_view.toasts, text, None);
                    return;
                };
                self.step_send(now_ms, out, move |site| Codec.request(site, &Route::PushRegister { registration: &registration }), |expect| Step::Push(Ask::Register, expect));
            }
        }
    }

    /// The platform's answer to a registration: the token, or no.
    pub(super) fn push_platform_answered(&mut self, now_ms: u64, id: IoId, result: IoResult<PlatformAnswer>, out: &mut Vec<Effect>) {
        self.push.platform.remove(&id);
        match result {
            IoResult::Ok(PlatformAnswer::Push { kind, token }) => self.push_token(now_ms, kind, Some(token.0), out),
            IoResult::Ok(PlatformAnswer::Done) => {}
            _ if self.push.want.is_some() => {
                self.push.want = None;
                let text = Text::new("push.refused", "Notifications were not allowed.");
                self.sent.toast(&mut self.next_view.toasts, text, None);
            }
            _ => {}
        }
    }

    /// A token from the platform (`Event::PushToken`, or the answer to a registration): registered with the hub
    /// at the level asked for, or the level the hub had.
    pub(super) fn push_token(&mut self, now_ms: u64, kind: String, token: Option<Vec<u8>>, out: &mut Vec<Effect>) {
        let Some(token) = token else {
            self.push.token = None;
            return self.push_send(now_ms, out, Ask::Remove);
        };
        if kind == "apns" && self.push.notify_key.is_none() {
            let Ok(key) = trommi_core::crypto::random::<32>(&mut trommi_core::crypto::SystemEntropy) else { return };
            self.push.notify_key = Some(key);
            let id = IoId(self.next_id);
            self.next_id += 1;
            out.push(Effect::Secret { id, op: SecretOp::Put { name: NOTIFY_KEY.into(), value: Bytes(key.to_vec()) } });
        }
        self.push.token = Some((kind, token));
        if self.push.want.is_none() {
            self.push.want = self.push.level.filter(|l| *l != PushChoice::Off);
        }
        if self.push.want.is_some() {
            self.push_send(now_ms, out, Ask::Register);
        }
    }

    /// Reads the hub's push state once the room is live.
    pub(super) fn push_look(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let can = self.config.as_ref().is_some_and(|c| !c.can.push.is_empty());
        if self.push.read || self.engine.is_none() || !can {
            return;
        }
        self.push.read = true;
        self.push_send(now_ms, out, Ask::State);
    }

    pub(super) fn push_answered(&mut self, now_ms: u64, ask: Ask, answer: Result<Answer, Failure>, out: &mut Vec<Effect>) {
        match (ask, answer) {
            (Ask::State, Ok(Answer::Push(state))) => {
                self.push.vapid = Some(state.vapid_public_key.clone());
                self.push.level = Some(match state.subscriptions.first().map(|s| s.level) {
                    Some(PushLevel::All) => PushChoice::All,
                    Some(PushLevel::Knocking) => PushChoice::Knocking,
                    None => PushChoice::Off,
                });
                if self.push.want.is_some() && self.push.token.is_none() {
                    self.push_platform(out);
                }
            }
            (Ask::Register, Ok(_)) => {
                self.push.level = self.push.want.take();
            }
            (Ask::Remove, Ok(_)) => {}
            (_, Err(failure)) => {
                out.push(log(LogLevel::Warn, &format!("push: {failure:?}")));
                if self.push.want.take().is_some() {
                    let text = Text::new("push.failed", "Push could not be turned on here.");
                    self.sent.toast(&mut self.next_view.toasts, text, None);
                }
            }
            (_, Ok(_)) => {}
        }
        let _ = now_ms;
        self.show_push();
    }

    pub(super) fn show_push(&mut self) {
        self.outside.due = true;
        let can = self.config.as_ref().map(|c| c.can.push.clone()).unwrap_or_default();
        self.next_view.settings.push = (!can.is_empty()).then(|| PushView {
            level: match self.push.level.unwrap_or(PushChoice::Off) {
                PushChoice::Off => "off",
                PushChoice::All => "all",
                PushChoice::Knocking => "knocking",
            }
            .into(),
            busy: self.push.want.is_some(),
        });
    }
}
