//! Sealed dictation (spec/v1.md 18): the hub relays a recording it cannot read to an attested Tinfoil enclave.
//!
//! The device checks the enclave's attestation itself and seals the recording to the enclave's HPKE key (EHBP,
//! the Encrypted HTTP Body Protocol: the body is ciphertext, the headers stay readable). The hub only:
//!
//! - checks the sign-in (a human device), the size, and the account's rate (10 a minute, `stt_per_day` a day);
//! - passes the attestation bundle on (`GET /v1/stt/attestation`): public data, which the device verifies, so the
//!   hub can withhold it but not forge it;
//! - forwards the sealed body to the enclave the device names in `X-Tinfoil-Enclave-Url` (HTTPS under
//!   `tinfoil.sh` only), with the one Tinfoil API key it holds (`HUB_TINFOIL_API_KEY`) added, and passes the
//!   sealed answer back with its `Ehbp-Response-Nonce`.
//!
//! It never reads, keeps or logs a body: a log line has the size rounded to 64 KiB, the duration and the status.
//! No key: dictation is off (`not-found`).

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use hyper::Response;
use serde_json::json;
use std::collections::HashMap;
use tokio::sync::{Mutex, Semaphore};

use crate::app::App;
use crate::error::{refuse, Res};
use crate::http::{self, Answer, Body, ReqBody};
use crate::limits::Window;

/// The request headers of EHBP and Tinfoil that are passed on; nothing else of the device's request is.
const PASSED_ON: &[&str] = &["ehbp-encapsulated-key", "content-type", "accept", "x-tinfoil-model"];
/// The one path of the enclave the relay calls.
pub const UPSTREAM_PATH: &str = "/v1/audio/transcriptions";
/// The most an answer of the enclave (a transcript, sealed) may hold.
const ANSWER_LIMIT: usize = 1 << 20;
/// The most an attestation bundle may hold.
const BUNDLE_LIMIT: usize = 4 << 20;
/// How long a bundle fetched is passed on again before it is fetched anew.
const BUNDLE_FOR: Duration = Duration::from_secs(60);

/// The relay's settings and state; `None` in `App` when the hub holds no Tinfoil key.
pub struct Stt {
    key: String,
    attestation_url: String,
    /// origins allowed besides `https://*.tinfoil.sh` (a test's stand-in enclave); empty in production
    extra_origins: Vec<String>,
    client: reqwest::Client,
    /// requests at the enclave at once, over all accounts
    running: Semaphore,
    /// recordings arriving at once, over all accounts (a slow sender holds one of these, not the enclave's)
    arriving: Semaphore,
    /// recordings arriving or at the enclave per account
    per_account: std::sync::Mutex<HashMap<[u8; 32], usize>>,
    per_minute: Window,
    per_day: Window,
    bundle: Mutex<Option<(Instant, Bytes)>>,
}

impl std::fmt::Debug for Stt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stt(<key redacted>)")
    }
}

impl Stt {
    pub fn new(
        key: String,
        attestation_url: String,
        extra_origins: Vec<String>,
        per_day: usize,
        at_once: usize,
    ) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(180))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Stt {
            key,
            attestation_url,
            extra_origins,
            client,
            running: Semaphore::new(at_once.max(1)),
            arriving: Semaphore::new(at_once.max(1) * 2),
            per_account: std::sync::Mutex::new(HashMap::new()),
            per_minute: Window::new(10, 60_000),
            per_day: Window::new(per_day.max(1), 86_400_000),
            bundle: Mutex::new(None),
        })
    }

    pub fn sweep(&self, now: u64) {
        self.per_minute.sweep(now);
        self.per_day.sweep(now);
    }

    /// The enclave origin the device named, if the relay may call it: HTTPS, the host `tinfoil.sh` or one under
    /// it, no user, no port but 443, no path, query or fragment (or exactly one of the extra origins).
    pub fn enclave(&self, named: Option<&str>) -> Res<String> {
        let named = named.ok_or_else(|| refuse("bad-format", "x-tinfoil-enclave-url: missing"))?;
        let named = named.trim_end_matches('/');
        if self.extra_origins.iter().any(|o| o == named) {
            return Ok(named.to_string());
        }
        let host = named
            .strip_prefix("https://")
            .ok_or_else(|| refuse("bad-format", "x-tinfoil-enclave-url: not an enclave"))?;
        let host = host.strip_suffix(":443").unwrap_or(host);
        let fine = !host.is_empty()
            && host.len() <= 253
            && host
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.')
            && !host.contains("..")
            && !host.starts_with('.')
            && (host == "tinfoil.sh" || host.ends_with(".tinfoil.sh"));
        if !fine {
            return Err(refuse("bad-format", "x-tinfoil-enclave-url: not an enclave"));
        }
        Ok(format!("https://{host}"))
    }
}

/// Requests of one account arriving or at the enclave at once.
const PER_ACCOUNT: usize = 2;

/// One account's place among the requests in flight; given back when dropped.
struct InFlight<'a>(&'a Stt, [u8; 32]);

impl<'a> InFlight<'a> {
    fn enter(stt: &'a Stt, account: [u8; 32]) -> Option<Self> {
        let mut held = stt.per_account.lock().unwrap_or_else(|e| e.into_inner());
        let n = held.entry(account).or_insert(0);
        if *n >= PER_ACCOUNT {
            return None;
        }
        *n += 1;
        Some(InFlight(stt, account))
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut held = self.0.per_account.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = held.get_mut(&self.1) {
            *n -= 1;
            if *n == 0 {
                held.remove(&self.1);
            }
        }
    }
}

fn off() -> crate::error::Refused {
    refuse("not-found", "this hub offers no dictation")
}

/// One log line per relayed request: no account, no address, no body; the size to 64 KiB.
fn logged(event: &str, size: usize, started: Instant, status: u16) {
    crate::log::info(
        event,
        json!({ "kib64": size.div_ceil(65_536), "ms": started.elapsed().as_millis() as u64, "status": status }),
    );
}

async fn signed_in_human(app: &Arc<App>, bearer: Option<String>) -> Res<crate::store::Auth> {
    let a = app.clone();
    let auth = tokio::task::spawn_blocking(move || a.read(|x| a.sessions.authorise(x.c, bearer.as_deref(), x.now)))
        .await
        .unwrap_or_else(|_| Err(refuse("internal", "internal error")))?;
    auth.human()?;
    Ok(auth)
}

/// `GET /v1/stt/attestation`: Tinfoil's attestation bundle, as fetched (at most a minute old).
pub async fn attestation(app: &Arc<App>, bearer: Option<&str>) -> Res<Answer> {
    let stt = app.stt.as_ref().ok_or_else(off)?;
    signed_in_human(app, bearer.map(str::to_string)).await?;
    let mut held = stt.bundle.lock().await;
    if let Some((at, bytes)) = held.as_ref() {
        if at.elapsed() < BUNDLE_FOR {
            return Ok(bundle_answer(bytes.clone()));
        }
    }
    let started = Instant::now();
    let failed = |status: u16| {
        logged("stt_attestation", 0, started, status);
        refuse("overloaded", "the enclave's attestation could not be read").retry(5)
    };
    let mut answer = stt
        .client
        .get(&stt.attestation_url)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|_| failed(0))?;
    let status = answer.status().as_u16();
    if status != 200 {
        return Err(failed(status));
    }
    let mut out = Vec::new();
    while let Some(chunk) = answer.chunk().await.map_err(|_| failed(status))? {
        if out.len() + chunk.len() > BUNDLE_LIMIT {
            return Err(failed(413));
        }
        out.extend_from_slice(&chunk);
    }
    let bytes = Bytes::from(out);
    *held = Some((Instant::now(), bytes.clone()));
    Ok(bundle_answer(bytes))
}

fn bundle_answer(bytes: Bytes) -> Answer {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .header("content-length", bytes.len())
        .body(Body::full(bytes))
        .expect("a valid response")
}

/// `POST /v1/stt`: the sealed recording to the enclave, its sealed transcript back.
pub async fn relay(
    app: &Arc<App>,
    bearer: Option<&str>,
    headers: &hyper::HeaderMap,
    body: &mut ReqBody,
) -> Res<Answer> {
    let stt = app.stt.as_ref().ok_or_else(off)?;
    let auth = signed_in_human(app, bearer.map(str::to_string)).await?;
    let enclave = stt.enclave(http::header(headers, "x-tinfoil-enclave-url"))?;
    let key = http::header(headers, "ehbp-encapsulated-key").unwrap_or_default();
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(refuse("bad-format", "ehbp-encapsulated-key: 32 bytes as hex"));
    }
    let limit = app.cfg.stt_limit;
    let announced: Option<u64> = http::header(headers, "content-length").and_then(|v| v.parse().ok());
    if announced.is_some_and(|n| n > limit as u64) {
        return Err(refuse("too-large", "a dictation is at most five minutes"));
    }
    let account = &auth.room[..];
    // (a look first, so that a refused request does not read its body; spent below, once it arrived)
    let now = crate::util::now();
    if let Err(wait) = stt.per_minute.check(account, now, false) {
        return Err(refuse("rate-limited", "ten dictations a minute").retry(wait));
    }
    if let Err(wait) = stt.per_day.check(account, now, false) {
        return Err(refuse("rate-limited", "the dictations of this day are used up").retry(wait));
    }
    let Some(_in_flight) = InFlight::enter(stt, auth.room) else {
        return Err(refuse("rate-limited", "one dictation at a time").retry(2));
    };
    let sealed = {
        let Ok(_arriving) = stt.arriving.try_acquire() else {
            return Err(refuse("overloaded", "dictation is busy: try again in a moment").retry(2));
        };
        body.read(limit, Duration::from_secs(120)).await?
    };
    if sealed.is_empty() {
        return Err(refuse("bad-format", "a dictation without a recording"));
    }
    // spent now, at the time it arrived, each limit checked and counted in one step
    let now = crate::util::now();
    if let Err(wait) = stt.per_day.check(account, now, true) {
        return Err(refuse("rate-limited", "the dictations of this day are used up").retry(wait));
    }
    if let Err(wait) = stt.per_minute.check(account, now, true) {
        return Err(refuse("rate-limited", "ten dictations a minute").retry(wait));
    }
    let Ok(_running) = stt.running.try_acquire() else {
        return Err(refuse("overloaded", "dictation is busy: try again in a moment").retry(2));
    };
    let started = Instant::now();
    let size = sealed.len();
    let mut call = stt
        .client
        .post(format!("{enclave}{UPSTREAM_PATH}"))
        .header("authorization", format!("Bearer {}", stt.key))
        .body(sealed);
    for name in PASSED_ON {
        if let Some(value) = http::header(headers, name) {
            call = call.header(*name, value);
        }
    }
    let failed = |status: u16| {
        logged("stt", size, started, status);
        if status == 401 || status == 403 {
            crate::log::warn("stt_key_refused", json!({ "status": status }));
        }
        match status {
            429 | 503 => refuse("overloaded", "the enclave is busy: try again in a moment").retry(5),
            _ => refuse("overloaded", "the enclave did not answer").retry(2),
        }
    };
    let mut answer = call.send().await.map_err(|_| failed(0))?;
    let status = answer.status().as_u16();
    let nonce = answer
        .headers()
        .get("ehbp-response-nonce")
        .and_then(|v| v.to_str().ok())
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_string);
    let (200, Some(nonce)) = (status, nonce) else {
        return Err(failed(status));
    };
    let content_type = answer
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let mut out = Vec::new();
    while let Some(chunk) = answer.chunk().await.map_err(|_| failed(status))? {
        if out.len() + chunk.len() > ANSWER_LIMIT {
            return Err(failed(413));
        }
        out.extend_from_slice(&chunk);
    }
    logged("stt", size, started, 200);
    Ok(Response::builder()
        .status(200)
        .header("content-type", content_type)
        .header("cache-control", "no-store")
        .header("ehbp-response-nonce", nonce)
        .header("content-length", out.len())
        .body(Body::full(Bytes::from(out)))
        .expect("a valid response"))
}
