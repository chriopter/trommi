//! Sealed dictation in the App (spec/v1.md 18): `Action::Dictate` and the platform's and the hub's answers fed to
//! `crate::dictation::Dictation`, whose `Do`s run here as effects. The view is `ViewModel.dictation`. In the demo
//! room the microphone records nothing and the words are the demo's own.

use super::{log, App, Effect, LogLevel};
use crate::app::bytes::Bytes;
use crate::app::io::{Entry, IoId, IoResult, PlatformAnswer, PlatformOp, StoreAnswer, StoreOp};
use crate::dictation::{Attested, Do, Problem, Sent};
use crate::hub::{Answer, Codec, Expect, Failure, Route};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The words of a dictation in the demo room.
pub const DEMO_TEXT: &str = "Sounds good, go ahead with the smaller change first.";

/// A step of the chat field's microphone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum DictateStep {
    /// The microphone of the field of `session` (the shell's id of that chat) was tapped.
    Start {
        #[serde(default)]
        session: String,
    },
    /// Done speaking: seal and send.
    Stop,
    /// Throw the recording away.
    Cancel,
    /// The shell put the words of `text.id` in the field.
    Taken { id: String },
    /// The person saw the problem.
    Dismiss,
    /// Settings: dictation on or off on this device.
    Switch { on: bool },
    /// Settings: check the enclave now (it is also checked when Settings comes into view).
    Check,
    /// Security: test the sealed connection (a fresh attestation and a silent clip there and back).
    Test,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Io {
    Record,
    Stop,
    Drop,
    Attest,
    Bundle,
    Send,
    Test,
}

#[derive(Default)]
pub(super) struct Dictating {
    pub(super) machine: crate::dictation::Dictation,
    asked: BTreeMap<IoId, Io>,
    /// The cache key of the switch, once the room's cache is open.
    key: Option<String>,
    /// The clock of the last event (the view's status depends on it).
    now: u64,
    /// What a request's answer is checked against.
    expect: BTreeMap<IoId, Expect>,
}

impl Dictating {
    pub(super) fn owns(&self, id: IoId) -> bool {
        self.asked.contains_key(&id)
    }
}

impl App {
    fn dictation_available(&self) -> bool {
        self.demo || self.config.as_ref().is_some_and(|c| c.can.dictate) && self.engine.is_some()
    }

    pub(super) fn dictate_action(&mut self, now_ms: u64, step: DictateStep, out: &mut Vec<Effect>) {
        if !self.dictation_available() {
            return;
        }
        if let DictateStep::Switch { on } = step {
            let todo = self.dictation.machine.set_on(on);
            self.dictation_run(now_ms, todo, out);
            if let Some(key) = self.dictation.key.clone() {
                let id = self.io(super::Pending::DictationWrite);
                let op = if on { StoreOp::Put { put: Vec::new(), delete: vec![key] } } else { StoreOp::Put { put: vec![Entry { key, value: Bytes(b"0".to_vec()) }], delete: Vec::new() } };
                out.push(Effect::Store { id, op });
            }
            if on {
                self.dictation_check(now_ms, out);
            }
            return;
        }
        if let DictateStep::Check = step {
            return self.dictation_check(now_ms, out);
        }
        if let DictateStep::Test = step {
            if self.demo {
                return self.dictation.machine.demo_tested(now_ms);
            }
            let todo = self.dictation.machine.test(now_ms);
            return self.dictation_run(now_ms, todo, out);
        }
        if self.demo {
            let d = &mut self.dictation.machine;
            match step {
                DictateStep::Start { session } if !d.is_recording() => d.demo_recording(now_ms, &session),
                DictateStep::Stop if d.is_recording() => d.demo_done(DEMO_TEXT),
                DictateStep::Cancel => {
                    d.cancel();
                }
                DictateStep::Taken { id } => d.taken(id.parse().unwrap_or(0)),
                DictateStep::Dismiss => d.dismiss(),
                _ => {}
            }
            return;
        }
        let todo = match step {
            DictateStep::Start { session } => {
                let locale = self.config.as_ref().map(|c| c.locale.clone()).unwrap_or_default();
                self.dictation.machine.set_locale(&locale);
                self.dictation.machine.start(now_ms, &session)
            }
            DictateStep::Stop => self.dictation.machine.stop(),
            DictateStep::Cancel => self.dictation.machine.cancel(),
            DictateStep::Taken { id } => {
                self.dictation.machine.taken(id.parse().unwrap_or(0));
                Vec::new()
            }
            DictateStep::Dismiss => {
                self.dictation.machine.dismiss();
                Vec::new()
            }
            DictateStep::Switch { .. } | DictateStep::Check | DictateStep::Test => Vec::new(),
        };
        self.dictation_run(now_ms, todo, out);
    }

    /// Settings in view: the enclave checked, so its state shows (the demo shows it as checked).
    pub(super) fn dictation_check(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if !self.dictation_available() {
            return;
        }
        if self.demo {
            return self.dictation.machine.demo_connected(now_ms);
        }
        let todo = self.dictation.machine.check(now_ms);
        self.dictation_run(now_ms, todo, out);
    }

    /// The room's cache opened: the switch is read.
    pub(super) fn dictation_open(&mut self, room_hex: &str, out: &mut Vec<Effect>) {
        let key = crate::cache::key(room_hex, crate::cache::part::DICTATION, None);
        self.dictation.key = Some(key.clone());
        let id = self.io(super::Pending::DictationRead);
        out.push(Effect::Store { id, op: StoreOp::Get { keys: vec![key] } });
    }

    pub(super) fn dictation_read(&mut self, result: &IoResult<StoreAnswer>) {
        let off = matches!(result, IoResult::Ok(StoreAnswer::Values { values }) if values.first().and_then(Option::as_ref).is_some_and(|v| v.0 == b"0"));
        if off {
            self.dictation.machine.set_on(false);
        }
    }

    /// The clock: a recording stops at its longest.
    pub(super) fn dictation_tick(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        self.dictation.now = now_ms;
        if self.demo {
            return;
        }
        let todo = self.dictation.machine.tick(now_ms);
        self.dictation_run(now_ms, todo, out);
    }

    fn dictation_io(&mut self, io: Io) -> IoId {
        let id = IoId(self.next_id);
        self.next_id += 1;
        self.dictation.asked.insert(id, io);
        id
    }

    fn dictation_run(&mut self, now_ms: u64, todo: Vec<Do>, out: &mut Vec<Effect>) {
        for step in todo {
            match step {
                Do::Record { max_ms } => {
                    let id = self.dictation_io(Io::Record);
                    out.push(Effect::Platform { id, op: PlatformOp::RecordStart { max_ms }, window: 0 });
                }
                Do::Stop => {
                    let id = self.dictation_io(Io::Stop);
                    out.push(Effect::Platform { id, op: PlatformOp::RecordStop, window: 0 });
                }
                Do::Drop => {
                    // (what the recording still owes is dropped when it comes: a late recording never enters a new one)
                    self.dictation.asked.retain(|_, io| !matches!(io, Io::Record | Io::Stop));
                    let id = self.dictation_io(Io::Drop);
                    out.push(Effect::Platform { id, op: PlatformOp::RecordCancel, window: 0 });
                }
                Do::Attest { bundle, repo } => {
                    let id = self.dictation_io(Io::Attest);
                    out.push(Effect::Platform { id, op: PlatformOp::Attest { bundle: Bytes(bundle), repo: repo.to_string() }, window: 0 });
                }
                Do::FetchBundle => {
                    if !self.dictation_send(now_ms, out, Io::Bundle, None) {
                        let more = self.dictation.machine.bundle_failed(Problem::Offline);
                        self.dictation_run(now_ms, more, out);
                    }
                }
                Do::Send(sealed) => {
                    if !self.dictation_send(now_ms, out, Io::Send, Some(sealed)) {
                        let more = self.dictation.machine.sent(Sent::Offline);
                        self.dictation_run(now_ms, more, out);
                    }
                }
                Do::SendTest(sealed) => {
                    if !self.dictation_send(now_ms, out, Io::Test, Some(sealed)) {
                        self.dictation.machine.test_sent(now_ms, Sent::Offline);
                    }
                }
                Do::CancelSend => {
                    let sending: Vec<IoId> = self.dictation.asked.iter().filter(|(_, io)| **io == Io::Send).map(|(id, _)| *id).collect();
                    // (the engine's request runs out; its answer, owned by nobody now, is dropped)
                    for id in sending {
                        self.dictation.asked.remove(&id);
                        self.dictation.expect.remove(&id);
                    }
                }
            }
        }
    }

    /// A request through the engine (it adds the token and renews it); false when none could be made.
    fn dictation_send(&mut self, now_ms: u64, out: &mut Vec<Effect>, io: Io, sealed: Option<crate::hub::stt::SttSealed>) -> bool {
        let cell = std::rc::Rc::new(std::cell::RefCell::new(None));
        let put = cell.clone();
        let sent = self.with_engine(now_ms, out, move |engine, ctx| {
            engine.request(
                ctx,
                move |site| {
                    let route = match &sealed {
                        Some(sealed) => Route::Stt { sealed },
                        None => Route::SttAttestation,
                    };
                    let (request, expect) = Codec.request(site, &route)?;
                    *put.borrow_mut() = Some(expect);
                    Ok(request)
                },
                None,
            )
        });
        let expect = cell.borrow_mut().take();
        match (sent, expect) {
            (Some(Ok(id)), Some(expect)) => {
                self.dictation.asked.insert(id, io);
                self.dictation.expect.insert(id, expect);
                true
            }
            _ => false,
        }
    }

    /// The platform answered a dictation's op.
    pub(super) fn dictation_platform(&mut self, now_ms: u64, id: IoId, result: IoResult<PlatformAnswer>, out: &mut Vec<Effect>) {
        let Some(io) = self.dictation.asked.remove(&id) else { return };
        let d = &mut self.dictation.machine;
        let todo = match (io, result) {
            (Io::Record, IoResult::Ok(PlatformAnswer::Recording)) => {
                d.recording(now_ms);
                Vec::new()
            }
            (Io::Record, _) => {
                d.mic_refused();
                Vec::new()
            }
            (Io::Stop, IoResult::Ok(PlatformAnswer::Recorded { audio, ms })) => d.recorded(audio.0, ms),
            (Io::Stop, _) => d.recorded(Vec::new(), 0),
            (Io::Attest, IoResult::Ok(PlatformAnswer::Attested { hpke_key, domain })) => {
                let verdict = <[u8; 32]>::try_from(&hpke_key.0[..]).ok().map(|hpke_key| Attested { hpke_key, domain });
                d.attested(now_ms, verdict)
            }
            (Io::Attest, other) => {
                if let IoResult::Ok(PlatformAnswer::NotAttested { reason }) = &other {
                    out.push(log(LogLevel::Warn, &format!("dictation: the enclave's attestation failed: {}", reason.chars().take(200).collect::<String>())));
                }
                d.attested(now_ms, None)
            }
            _ => Vec::new(),
        };
        self.dictation_run(now_ms, todo, out);
    }

    /// The hub answered a dictation's request.
    pub(super) fn dictation_answered(&mut self, now_ms: u64, id: IoId, result: &Result<crate::app::io::HttpResponse, Failure>, out: &mut Vec<Effect>) {
        let Some(io) = self.dictation.asked.remove(&id) else { return };
        let Some(expect) = self.dictation.expect.remove(&id) else { return };
        let answer = match result {
            Ok(response) => Codec.answer(&expect, response),
            Err(failure) => Err(failure.clone()),
        };
        let sorted = |failure: &Failure| match failure {
            Failure::Offline => Sent::Offline,
            Failure::Refused { code: Some(code), .. } if code == "rate-limited" || code == "too-many" => Sent::TooMany,
            Failure::Refused { code: Some(code), .. } if code == "not-found" => Sent::Off,
            Failure::NotNow { status: 429, .. } => Sent::TooMany,
            Failure::NotNow { .. } => Sent::Upstream,
            _ => Sent::Failed,
        };
        let d = &mut self.dictation.machine;
        let todo = match (io, answer) {
            (Io::Bundle, Ok(Answer::SttBundle(bundle))) => d.bundle(bundle),
            (Io::Bundle, Err(failure)) => d.bundle_failed(match sorted(&failure) {
                Sent::Off => Problem::Off,
                Sent::TooMany => Problem::TooMany,
                Sent::Offline => Problem::Offline,
                _ => Problem::Failed,
            }),
            (Io::Send, Ok(Answer::SttSealed { nonce, body })) => d.sent(Sent::Answered { nonce, body }),
            (Io::Send, Err(failure)) => d.sent(sorted(&failure)),
            (Io::Test, Ok(Answer::SttSealed { nonce, body })) => {
                d.test_sent(now_ms, Sent::Answered { nonce, body });
                Vec::new()
            }
            (Io::Test, Err(failure)) => {
                d.test_sent(now_ms, sorted(&failure));
                Vec::new()
            }
            (_, _) => {
                out.push(log(LogLevel::Warn, "dictation: an answer of another kind"));
                Vec::new()
            }
        };
        self.dictation_run(now_ms, todo, out);
    }

    /// Signing out: a recording running or asked for ends, and nothing of the dictation stays.
    pub(super) fn dictation_signed_out(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let todo = self.dictation.machine.cancel();
        self.dictation_run(now_ms, todo, out);
        self.dictation.asked.retain(|_, io| *io == Io::Drop);
        self.dictation.expect.clear();
        self.dictation.machine = Default::default();
        self.dictation.key = None;
    }

    pub(super) fn show_dictation(&mut self) {
        let available = self.dictation_available();
        self.next_view.dictation = self.dictation.machine.view(available, self.dictation.now);
    }
}

