//! Sealed dictation (spec/v1.md 18): what the core decides between "the person taps the microphone" and "the
//! words stand in the field". The platform records, and checks Tinfoil's attestation (its SDK's verifier); this
//! module keeps the order and the one rule that matters: **nothing is sent before the enclave's attestation was
//! checked on this device, and nothing at all when it failed.**
//!
//! ```text
//! idle ──start──▶ starting ──mic live──▶ recording ──stop (or 5 min)──▶ working ──sealed──▶ sending ──▶ done
//!   ▲                 │ mic refused            │ cancel                     │ attestation failed   │ refused
//!   └──── taken ◀── done          failed ◀─────┴────────────────────────────┴──────────────────────┘
//! ```
//!
//! While the person speaks the bundle is fetched through the hub and checked by the platform, so the recording
//! is sealed the moment it ends. A checked enclave is used for [`ATTESTED_FOR_MS`]; a transcript that does not
//! open drops it. The recording is held in memory only, for one retry with the fallback model, and dropped when
//! the dictation ends. No step is kept across a restart.
//!
//! This module is pure: [`Dictation`] takes what happened and answers with [`Do`]s; `app::dictation` runs them.

pub mod ehbp;

use crate::views::{DictatedText, DictationFact, DictationView, Text};

/// The model first asked, and the one asked once more when the first fails at the enclave.
pub const MODELS: [&str; 2] = ["voxtral-small-24b", "whisper-large-v3-turbo"];
/// The code that every Tinfoil enclave of the inference router is built from (Sigstore checks the measurement
/// against its release).
pub const CONFIG_REPO: &str = "tinfoilsh/confidential-model-router";
/// The longest recording; the core stops it there.
pub const MAX_MS: u64 = 300_000;
/// A shorter one holds no words.
pub const MIN_MS: u64 = 500;
/// The most bytes of a recording (five minutes of 16 kHz mono 16-bit WAV, and room).
pub const MAX_AUDIO: usize = 11 << 20;
/// How long an enclave checked once is used without checking again.
pub const ATTESTED_FOR_MS: u64 = 600_000;
/// The languages Voxtral is told; another locale lets it find the language itself.
const LANGUAGES: [&str; 8] = ["en", "de", "fr", "es", "pt", "it", "nl", "hi"];
const BOUNDARY: &str = "trommi-dictation-7c1e";

/// What the platform's verifier found the enclave to be.
#[derive(Clone, PartialEq, Eq)]
pub struct Attested {
    /// The enclave's HPKE public key (X25519) that its attestation binds.
    pub hpke_key: [u8; 32],
    /// Its host, as the attestation names it (under `tinfoil.sh`).
    pub domain: String,
}

impl std::fmt::Debug for Attested {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Attested({})", self.domain)
    }
}

/// Why a dictation ended without words. Each has its sentence (`Problem::text`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    NoMic,
    TooShort,
    TooLong,
    /// The enclave's attestation did not check out: nothing was sent.
    NotVerified,
    Offline,
    TooMany,
    /// This hub offers no dictation.
    Off,
    NothingHeard,
    Failed,
}

impl Problem {
    pub fn text(self) -> Text {
        match self {
            Problem::NoMic => Text::new("dictate.no_mic", "The microphone was not allowed."),
            Problem::TooShort => Text::new("dictate.too_short", "Too short: hold on a little longer."),
            Problem::TooLong => Text::new("dictate.too_long", "Too long: at most five minutes."),
            Problem::NotVerified => Text::new("dictate.not_verified", "The sealed enclave could not be verified, so nothing was sent."),
            Problem::Offline => Text::new("dictate.offline", "Not sent: no connection."),
            Problem::TooMany => Text::new("dictate.too_many", "Too many dictations: wait a moment."),
            Problem::Off => Text::new("dictate.off", "Dictation is not available on this hub."),
            Problem::NothingHeard => Text::new("dictate.nothing_heard", "No words were heard."),
            Problem::Failed => Text::new("dictate.failed", "Dictation did not work. Try again."),
        }
    }
}

/// How a request of the dictation ended, as the app sorts the hub's answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sent {
    Answered { nonce: String, body: Vec<u8> },
    Offline,
    TooMany,
    Off,
    /// The enclave failed or is busy (`overloaded`): the fallback model may do.
    Upstream,
    Failed,
}

/// What the app is to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Do {
    /// Start recording (the platform asks for the microphone); stop by itself at `max_ms`.
    Record { max_ms: u64 },
    /// End the recording and hand it over.
    Stop,
    /// End the recording and drop it.
    Drop,
    /// `GET /v1/stt/attestation`.
    FetchBundle,
    /// The platform checks the bundle against `CONFIG_REPO`.
    Attest { bundle: Vec<u8>, repo: &'static str },
    /// `POST /v1/stt`.
    Send(crate::hub::stt::SttSealed),
    /// Stop waiting for the request in flight.
    CancelSend,
    /// `POST /v1/stt` of the Settings' test: a short silent clip, sealed like a dictation.
    SendTest(crate::hub::stt::SttSealed),
}

/// The Settings' test of the sealed connection: a fresh attestation and a silent clip sealed there and back.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum Test {
    #[default]
    Idle,
    Running { since: u64 },
    Passed { ms: u64 },
    Failed(Problem),
}

/// What the attestation bundle says beside what the platform checks: the release's digest and the hardware.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
struct Release {
    digest: String,
    platform: String,
}

impl Release {
    /// Read from the bundle as the hub passed it (the platform's verifier checks the same bytes): at most a
    /// 64-digit hex digest and a known platform, else nothing is shown.
    fn of(bundle: &[u8]) -> Self {
        let v: serde_json::Value = serde_json::from_slice(bundle).unwrap_or_default();
        let digest = v.get("digest").and_then(|d| d.as_str()).filter(|d| d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit())).map(str::to_ascii_lowercase).unwrap_or_default();
        let format = v.get("enclaveAttestationReport").and_then(|r| r.get("format")).and_then(|f| f.as_str()).unwrap_or_default();
        let platform = if format.contains("sev-snp") { "AMD SEV-SNP" } else if format.contains("tdx") { "Intel TDX" } else { "" };
        Release { digest, platform: platform.to_string() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Starting,
    Recording { since: u64 },
    Stopping { since: u64 },
    Working,
    Sending,
    Done { id: u64, text: String },
    Failed(Problem),
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
enum Enclave {
    #[default]
    Unknown,
    Fetching,
    Attesting,
    Verified { attested: Attested, at: u64 },
    /// The bundle did not come (offline, the hub offers none): nothing is sent.
    Unreachable(Problem),
    /// Its attestation failed: nothing is sent.
    Refused,
}

#[derive(Default)]
pub struct Dictation {
    phase: Phase,
    enclave: Enclave,
    audio: Option<Vec<u8>>,
    opener: Option<ehbp::Opener>,
    /// The model of the request in flight (an index of [`MODELS`]).
    model: usize,
    language: Option<&'static str>,
    texts: u64,
    /// The chat the dictation is for (the shell's id of its field): its words go there and nowhere else.
    session: String,
    /// The person switched dictation off on this device (Settings).
    off: bool,
    test: Test,
    test_opener: Option<ehbp::Opener>,
    /// Of the last bundle that came: shown in Settings once its enclave is verified.
    release: Release,
    pending_release: Release,
}

impl Dictation {
    /// The language to tell the enclave, from the person's locale ("de-DE" → "de").
    pub fn set_locale(&mut self, locale: &str) {
        let lang = locale.split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase();
        self.language = LANGUAGES.iter().copied().find(|l| *l == lang);
    }

    fn fresh(&self, now: u64) -> bool {
        matches!(&self.enclave, Enclave::Verified { at, .. } if now < at + ATTESTED_FOR_MS)
            || matches!(self.enclave, Enclave::Fetching | Enclave::Attesting)
    }

    fn end(&mut self, problem: Problem) {
        self.audio = None;
        self.opener = None;
        self.phase = Phase::Failed(problem);
    }

    /// Dictation on or off on this device (Settings). Off ends a dictation that runs.
    pub fn set_on(&mut self, on: bool) -> Vec<Do> {
        self.off = !on;
        if on { Vec::new() } else { self.cancel() }
    }

    pub fn is_on(&self) -> bool {
        !self.off
    }

    /// Settings in view: the enclave is checked (if not checked lately), so its state can be shown. Sends nothing.
    pub fn check(&mut self, now: u64) -> Vec<Do> {
        if self.off || self.fresh(now) || !matches!(self.phase, Phase::Idle | Phase::Done { .. } | Phase::Failed(_)) {
            return Vec::new();
        }
        self.enclave = Enclave::Fetching;
        vec![Do::FetchBundle]
    }

    /// Settings: test the sealed connection now: a fresh attestation, then a second of silence sealed to the enclave
    /// and its answer opened. Nothing else is sent; a dictation that runs keeps its own request.
    pub fn test(&mut self, now: u64) -> Vec<Do> {
        if self.off || matches!(self.test, Test::Running { .. }) {
            return Vec::new();
        }
        self.test = Test::Running { since: now };
        if matches!(self.enclave, Enclave::Fetching | Enclave::Attesting) {
            // (a check under way: its verdict starts the round trip)
            return Vec::new();
        }
        self.enclave = Enclave::Fetching;
        vec![Do::FetchBundle]
    }

    fn seal_test(&mut self, attested: &Attested) -> Vec<Do> {
        let form = form(&silence(), MODELS[0], self.language);
        let Ok(sealed) = ehbp::seal(&attested.hpke_key, &form) else {
            self.test = Test::Failed(Problem::Failed);
            return Vec::new();
        };
        self.test_opener = Some(sealed.opener.clone());
        vec![Do::SendTest(crate::hub::stt::SttSealed {
            enclave: format!("https://{}", attested.domain),
            encapsulated_key: sealed.encapsulated_key.clone(),
            content_type: format!("multipart/form-data; boundary={BOUNDARY}"),
            model: MODELS[0].to_string(),
            body: sealed.body,
        })]
    }

    /// The relay's answer to the test: passed when the sealed answer opens (whatever words it holds).
    pub fn test_sent(&mut self, now: u64, sent: Sent) {
        let Test::Running { since } = self.test else { return };
        let opener = self.test_opener.take();
        self.test = match sent {
            Sent::Answered { nonce, body } => match opener.and_then(|o| o.open(&nonce, &body).ok()) {
                Some(_) => Test::Passed { ms: now.saturating_sub(since) },
                None => {
                    self.enclave = Enclave::Unknown;
                    Test::Failed(Problem::Failed)
                }
            },
            Sent::Offline => Test::Failed(Problem::Offline),
            Sent::TooMany => Test::Failed(Problem::TooMany),
            Sent::Off => Test::Failed(Problem::Off),
            Sent::Upstream | Sent::Failed => Test::Failed(Problem::Failed),
        };
    }

    /// The microphone of the field of `session` was tapped.
    pub fn start(&mut self, now: u64, session: &str) -> Vec<Do> {
        if self.off {
            return Vec::new();
        }
        if !matches!(self.phase, Phase::Idle | Phase::Done { .. } | Phase::Failed(_)) {
            return Vec::new();
        }
        self.session = session.chars().take(200).collect();
        self.phase = Phase::Starting;
        self.audio = None;
        self.model = 0;
        let mut out = vec![Do::Record { max_ms: MAX_MS }];
        if !self.fresh(now) {
            self.enclave = Enclave::Fetching;
            out.push(Do::FetchBundle);
        }
        out
    }

    /// The microphone is live.
    pub fn recording(&mut self, now: u64) {
        if self.phase == Phase::Starting {
            self.phase = Phase::Recording { since: now };
        }
    }

    pub fn mic_refused(&mut self) {
        if matches!(self.phase, Phase::Starting | Phase::Recording { .. } | Phase::Stopping { .. }) {
            self.end(Problem::NoMic);
        }
    }

    /// The person is done speaking.
    pub fn stop(&mut self) -> Vec<Do> {
        match self.phase {
            Phase::Recording { since } => {
                self.phase = Phase::Stopping { since };
                vec![Do::Stop]
            }
            Phase::Starting => self.cancel(),
            _ => Vec::new(),
        }
    }

    /// The clock: a recording stops at [`MAX_MS`].
    pub fn tick(&mut self, now: u64) -> Vec<Do> {
        match self.phase {
            Phase::Recording { since } if now >= since + MAX_MS => self.stop(),
            _ => Vec::new(),
        }
    }

    /// When the clock matters next.
    pub fn wake(&self) -> Option<u64> {
        match self.phase {
            Phase::Recording { since } => Some(since + MAX_MS),
            _ => None,
        }
    }

    pub fn cancel(&mut self) -> Vec<Do> {
        let out = match self.phase {
            Phase::Starting | Phase::Recording { .. } | Phase::Stopping { .. } => vec![Do::Drop],
            Phase::Sending => vec![Do::CancelSend],
            _ => Vec::new(),
        };
        self.audio = None;
        self.opener = None;
        self.phase = Phase::Idle;
        out
    }

    /// The recording, ended: its bytes (WAV) and how long it ran.
    pub fn recorded(&mut self, audio: Vec<u8>, ms: u64) -> Vec<Do> {
        if !matches!(self.phase, Phase::Stopping { .. }) {
            return Vec::new();
        }
        if ms < MIN_MS || audio.len() < 64 {
            self.end(Problem::TooShort);
            return Vec::new();
        }
        if audio.len() > MAX_AUDIO {
            self.end(Problem::TooLong);
            return Vec::new();
        }
        self.audio = Some(audio);
        self.phase = Phase::Working;
        self.advance()
    }

    /// The bundle came from the hub: the platform checks it.
    pub fn bundle(&mut self, bundle: Vec<u8>) -> Vec<Do> {
        if self.enclave != Enclave::Fetching {
            return Vec::new();
        }
        self.enclave = Enclave::Attesting;
        self.pending_release = Release::of(&bundle);
        vec![Do::Attest { bundle, repo: CONFIG_REPO }]
    }

    pub fn bundle_failed(&mut self, why: Problem) -> Vec<Do> {
        if self.enclave != Enclave::Fetching {
            return Vec::new();
        }
        self.enclave = Enclave::Unreachable(why);
        if matches!(self.test, Test::Running { .. }) {
            self.test = Test::Failed(why);
        }
        self.advance()
    }

    /// The platform's verdict on the bundle. `None`: it did not check out.
    pub fn attested(&mut self, now: u64, verdict: Option<Attested>) -> Vec<Do> {
        if self.enclave != Enclave::Attesting {
            return Vec::new();
        }
        self.enclave = match verdict {
            Some(attested) if crate::hub::stt::enclave_ok(&format!("https://{}", attested.domain)) => Enclave::Verified { attested, at: now },
            _ => Enclave::Refused,
        };
        let mut out = Vec::new();
        match &self.enclave {
            Enclave::Verified { attested, .. } => {
                self.release = std::mem::take(&mut self.pending_release);
                if matches!(self.test, Test::Running { .. }) {
                    let attested = attested.clone();
                    out.extend(self.seal_test(&attested));
                }
            }
            _ => {
                self.release = Release::default();
                if matches!(self.test, Test::Running { .. }) {
                    self.test = Test::Failed(Problem::NotVerified);
                }
            }
        }
        out.extend(self.advance());
        out
    }

    /// Seals and sends once both the recording and the verdict are here: the one place a request is made.
    fn advance(&mut self) -> Vec<Do> {
        if self.phase != Phase::Working || self.audio.is_none() {
            return Vec::new();
        }
        let attested = match &self.enclave {
            Enclave::Verified { attested, .. } => attested.clone(),
            Enclave::Refused => {
                self.end(Problem::NotVerified);
                return Vec::new();
            }
            Enclave::Unreachable(why) => {
                let why = *why;
                self.end(why);
                return Vec::new();
            }
            // (still checking; the recording waits)
            Enclave::Unknown | Enclave::Fetching | Enclave::Attesting => return Vec::new(),
        };
        self.seal_and_send(&attested)
    }

    fn seal_and_send(&mut self, attested: &Attested) -> Vec<Do> {
        let Some(audio) = self.audio.as_ref() else { return Vec::new() };
        let model = MODELS[self.model];
        let form = form(audio, model, self.language);
        let Ok(sealed) = ehbp::seal(&attested.hpke_key, &form) else {
            self.enclave = Enclave::Unknown;
            self.end(Problem::Failed);
            return Vec::new();
        };
        self.opener = Some(sealed.opener.clone());
        self.phase = Phase::Sending;
        vec![Do::Send(crate::hub::stt::SttSealed {
            enclave: format!("https://{}", attested.domain),
            encapsulated_key: sealed.encapsulated_key.clone(),
            content_type: format!("multipart/form-data; boundary={BOUNDARY}"),
            model: model.to_string(),
            body: sealed.body,
        })]
    }

    /// The relay's answer.
    pub fn sent(&mut self, sent: Sent) -> Vec<Do> {
        if self.phase != Phase::Sending {
            return Vec::new();
        }
        let opener = self.opener.take();
        match sent {
            Sent::Answered { nonce, body } => {
                let text = opener
                    .and_then(|o| o.open(&nonce, &body).ok())
                    .and_then(|plain| serde_json::from_slice::<serde_json::Value>(&plain).ok())
                    .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(clean));
                match text {
                    Some(text) if !text.is_empty() => {
                        self.texts += 1;
                        self.audio = None;
                        self.phase = Phase::Done { id: self.texts, text };
                    }
                    Some(_) => self.end(Problem::NothingHeard),
                    None => {
                        // (an answer that does not open: the enclave is checked again next time)
                        self.enclave = Enclave::Unknown;
                        self.end(Problem::Failed);
                    }
                }
                Vec::new()
            }
            Sent::Upstream if self.model + 1 < MODELS.len() => {
                self.model += 1;
                match self.enclave.clone() {
                    Enclave::Verified { attested, .. } => self.seal_and_send(&attested),
                    _ => {
                        self.end(Problem::Failed);
                        Vec::new()
                    }
                }
            }
            Sent::Upstream | Sent::Failed => {
                self.end(Problem::Failed);
                Vec::new()
            }
            Sent::Offline => {
                self.end(Problem::Offline);
                Vec::new()
            }
            Sent::TooMany => {
                self.end(Problem::TooMany);
                Vec::new()
            }
            Sent::Off => {
                self.end(Problem::Off);
                Vec::new()
            }
        }
    }

    /// The shell put the words in the field.
    pub fn taken(&mut self, id: u64) {
        if matches!(self.phase, Phase::Done { id: done, .. } if done == id) {
            self.phase = Phase::Idle;
        }
    }

    /// The person saw the problem.
    pub fn dismiss(&mut self) {
        if matches!(self.phase, Phase::Failed(_)) {
            self.phase = Phase::Idle;
        }
    }

    /// Demo only: the words a recording would have brought, without platform or hub.
    pub fn demo_done(&mut self, text: &str) {
        self.texts += 1;
        self.audio = None;
        self.phase = Phase::Done { id: self.texts, text: text.to_string() };
    }

    /// Demo only: the enclave as a check that passed would leave it.
    pub fn demo_connected(&mut self, now: u64) {
        if self.release.digest.is_empty() {
            self.release = Release { digest: "3f9a0c5be1d24477a1c08e2f6b9d3e4a5c6b7d8e9f0a1b2c3d4e5f6a7b8c7c21".into(), platform: "AMD SEV-SNP".into() };
        }
        self.enclave = Enclave::Verified { attested: Attested { hpke_key: [0; 32], domain: "demo.tinfoil.sh".into() }, at: now };
    }

    /// Demo only: a test that passed, as a real one would show.
    pub fn demo_tested(&mut self, now: u64) {
        self.demo_connected(now);
        self.release = Release { digest: "3f9a0c5be1d24477a1c08e2f6b9d3e4a5c6b7d8e9f0a1b2c3d4e5f6a7b8c7c21".into(), platform: "AMD SEV-SNP".into() };
        self.test = Test::Passed { ms: 1_900 };
    }

    /// Demo only: recording at once.
    pub fn demo_recording(&mut self, now: u64, session: &str) {
        self.session = session.chars().take(200).collect();
        self.phase = Phase::Recording { since: now };
    }

    pub fn is_recording(&self) -> bool {
        matches!(self.phase, Phase::Recording { .. })
    }

    /// What Settings says of the enclave: a word for the dot (`connected` green, `checking`, `failed` red, `off` and
    /// `unknown` grey) and its sentence.
    fn status(&self, available: bool, now: u64) -> (&'static str, Text) {
        if !available {
            return ("off", Text::new("dictate.status.unavailable", "Not available on this device"));
        }
        if self.off {
            return ("off", Text::new("dictate.status.off", "Off"));
        }
        match &self.enclave {
            Enclave::Verified { at, .. } if now < at + ATTESTED_FOR_MS => ("connected", Text::new("dictate.status.connected", "Connected · sealed")),
            Enclave::Verified { .. } | Enclave::Unknown => ("unknown", Text::new("dictate.status.unknown", "Checked when you dictate")),
            Enclave::Fetching | Enclave::Attesting => ("checking", Text::new("dictate.status.checking", "Checking the enclave…")),
            Enclave::Refused => ("failed", Text::new("dictate.status.not_verified", "Enclave not verified: nothing is sent")),
            Enclave::Unreachable(Problem::Off) => ("off", Text::new("dictate.status.hub_off", "Not offered by this hub")),
            Enclave::Unreachable(Problem::Offline) => ("failed", Text::new("dictate.status.offline", "No connection to the enclave")),
            Enclave::Unreachable(_) => ("failed", Text::new("dictate.status.failed", "The enclave could not be reached")),
        }
    }

    /// Settings, Security: the facts of the sealed connection, in order, as the core says them.
    fn facts(&self, now: u64) -> Vec<DictationFact> {
        let fact = |key: &str, label: &str, value: Text| DictationFact { key: key.into(), label: Text::new(&format!("dictate.fact.{key}"), label), value, href: None, dot: String::new() };
        let verified = match &self.enclave { Enclave::Verified { at, .. } => Some(*at), _ => None };
        let enclave = if self.release.platform.is_empty() || verified.is_none() { "Tinfoil".to_string() } else { format!("Tinfoil · {}", self.release.platform) };
        let mut out = vec![
            fact("enclave", "Enclave", Text::new("dictate.fact.enclave.value", &enclave)),
            fact("model", "Model", Text::new("dictate.fact.model.value", "Voxtral Small 24B")),
        ];
        let (attestation, dot) = match &self.enclave {
            Enclave::Verified { at, .. } => (Text::new("dictate.fact.verified", &format!("verified · {}", ago(now, *at))), "connected"),
            Enclave::Fetching | Enclave::Attesting => (Text::new("dictate.fact.checking", "checking…"), "checking"),
            Enclave::Refused => (Text::new("dictate.fact.refused", "not verified: nothing is sent"), "failed"),
            Enclave::Unreachable(p) => (p.text(), "failed"),
            Enclave::Unknown => (Text::new("dictate.fact.unchecked", "not checked yet"), "unknown"),
        };
        out.push(DictationFact { dot: dot.into(), ..fact("attestation", "Attestation", attestation) });
        if verified.is_some() && !self.release.digest.is_empty() {
            let d = &self.release.digest;
            out.push(DictationFact {
                href: Some(format!("https://search.sigstore.dev/?hash={d}")),
                ..fact("code", "Code", Text::new("dictate.fact.code.value", &format!("{}…{}", &d[..4], &d[d.len() - 4..])))
            });
        }
        out.push(fact("hub", "Hub", Text::new("dictate.fact.hub.value", "relays ciphertext only")));
        out
    }

    pub fn view(&self, available: bool, now: u64) -> DictationView {
        let (status_kind, status) = self.status(available, now);
        let (state, since_ms, problem, text) = match &self.phase {
            _ if !available || self.off => ("off", 0, None, None),
            Phase::Idle => ("idle", 0, None, None),
            Phase::Starting => ("starting", 0, None, None),
            Phase::Recording { since } => ("recording", *since, None, None),
            Phase::Stopping { since } => ("working", *since, None, None),
            Phase::Working | Phase::Sending => ("working", 0, None, None),
            Phase::Done { id, text } => ("done", 0, None, Some(DictatedText { id: id.to_string(), text: text.clone() })),
            Phase::Failed(p) => ("failed", 0, Some(p.text()), None),
        };
        DictationView {
            session: if matches!(self.phase, Phase::Idle) || !available || self.off { String::new() } else { self.session.clone() },
            state: state.to_string(),
            since_ms,
            max_ms: MAX_MS,
            problem,
            text,
            hint: (state == "recording").then(|| Text::new("dictate.sealed", "Sealed: only the verified enclave hears this.")),
            facts: if available && !self.off { self.facts(now) } else { Vec::new() },
            test_state: match &self.test { Test::Idle => "idle", Test::Running { .. } => "running", Test::Passed { .. } => "passed", Test::Failed(_) => "failed" }.to_string(),
            test_result: match &self.test {
                Test::Idle => None,
                Test::Running { .. } => Some(Text::new("dictate.test.running", "Testing…")),
                Test::Passed { ms } => Some(Text::new("dictate.test.passed", &format!("✓ sealed round trip · {}", seconds(*ms)))),
                Test::Failed(p) => Some(p.text()),
            },
            available,
            on: !self.off,
            status_kind: status_kind.to_string(),
            status: Some(status),
        }
    }
}

/// "1.9 s".
fn seconds(ms: u64) -> String {
    format!("{}.{} s", ms / 1000, (ms % 1000) / 100)
}

/// "just now", "4 min ago", "2 h ago".
fn ago(now: u64, at: u64) -> String {
    let min = now.saturating_sub(at) / 60_000;
    match min {
        0 => "just now".to_string(),
        1..=59 => format!("{min} min ago"),
        _ => format!("{} h ago", min / 60),
    }
}

/// One second of silence, 16 kHz mono 16-bit WAV: what the test seals (no microphone, nothing personal).
pub fn silence() -> Vec<u8> {
    let samples: u32 = 16_000;
    let mut w = Vec::with_capacity(44 + samples as usize * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + samples * 2).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&16_000u32.to_le_bytes());
    w.extend_from_slice(&32_000u32.to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(samples * 2).to_le_bytes());
    w.resize(44 + samples as usize * 2, 0);
    w
}

/// One line of words: spaces at the ends cut, control characters but line breaks out.
fn clean(text: &str) -> String {
    text.chars().filter(|c| !c.is_control() || *c == '\n').collect::<String>().trim().to_string()
}

/// The OpenAI-style form of `/v1/audio/transcriptions`: the file, the model, the language when known.
pub fn form(audio: &[u8], model: &str, language: Option<&str>) -> Vec<u8> {
    let mut out = Vec::with_capacity(audio.len() + 512);
    let mut field = |name: &str, value: &str| {
        out.extend_from_slice(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    };
    field("model", model);
    field("response_format", "json");
    if let Some(language) = language {
        field("language", language);
    }
    out.extend_from_slice(
        format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"dictation.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes(),
    );
    out.extend_from_slice(audio);
    out.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    out
}

#[cfg(test)]
mod tests;
