//! Requests the App's actions send to the hub through the engine ([`Engine::request`]): the engine holds the token,
//! so a request that needs it is signed here, renewed on a `401` and sent again, and retried by the engine's one
//! rule. The App sends nothing with a token on its own.
//!
//! # How it runs
//!
//! The action builds its request with the codec's builder for the route (which sets the route's `max_answer`),
//! once, against a [`Site`] that carries the token the engine holds. The engine keeps the built request and owns it
//! until it is answered:
//!
//! - **a `401`** (`unauthorised`): the token is dropped, the engine signs in again, and the kept request goes again
//!   with the new token in its `authorization` header (not built again);
//! - **not reached, not now, a 5xx**: tried again after [`Backoff::HUB`]'s wait (the hub's `Retry-After` honoured).
//!   A request the person waits for ([`HttpClass::Interactive`]) is tried at most [`INTERACTIVE_TRIES`] times and
//!   then reported as failed; one of the background, an upload or a download until it is answered;
//! - **anything else** (a 2xx, the hub's refusal, a proxy's page): reported as [`Report::Answer`], the raw
//!   response, for the action to read with its own codec answer.
//!
//! # Kept requests
//!
//! A request with side effects at the hub may be given a name (`keep`): it is written to the app store
//! (`engine/req/<name>`, without its token) before it is sent, deleted once answered, and sent again after a
//! restart, answered under the same name. So such a route must be idempotent at the hub: a second arrival of the
//! same request has the effect of one. The store write is not a barrier: a request that left just before the
//! process died and was not written yet is lost to the restart, and the action that asked again repeats it.

use super::*;
use crate::app::io::{Entry, Header, HttpClass, HttpRequest};

/// A request the person waits for is tried at most so many times.
pub const INTERACTIVE_TRIES: u32 = 2;
/// At most so many requests wait at once; one more is refused (`busy`).
pub const REQUESTS_MAX: usize = 256;
/// At most so many kept ones.
pub const KEPT_MAX: usize = 64;
/// The prefix of a kept request's key in the app store.
pub const KEPT_PREFIX: &str = "engine/req/";

/// One answered request (the App's [`crate::app::App::take_answers`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    /// The id [`Engine::request`] gave; a kept request sent again after a restart gets a new one.
    pub id: IoId,
    pub keep: Option<String>,
    /// The hub's response (2xx or a refusal), or why none came (`Failure::Offline`, `NotNow`, a bad answer).
    pub result: Result<HttpResponse, Failure>,
}

struct Out {
    request: HttpRequest,
    keep: Option<String>,
    /// The id it is sent under now.
    flying: Option<IoId>,
    tries: u32,
    retry: Retry,
}

#[derive(Default)]
pub(super) struct Requests {
    outs: BTreeMap<IoId, Out>,
    /// The kept ones of the store are being read.
    scanning: Option<IoId>,
}

impl Requests {
    pub(super) fn wake(&self) -> Option<u64> {
        self.outs.values().filter(|o| o.flying.is_none()).filter_map(|o| o.retry.at).min()
    }
    pub(super) fn hurry(&mut self) {
        for out in self.outs.values_mut() {
            out.retry.hurry();
        }
    }
}

fn kept_key(name: &str) -> String {
    format!("{KEPT_PREFIX}{name}")
}

fn needs_token(request: &HttpRequest) -> bool {
    request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("authorization"))
}

/// The request as it is kept: without its token.
fn without_token(request: &HttpRequest) -> HttpRequest {
    let mut kept = request.clone();
    for h in &mut kept.headers {
        if h.name.eq_ignore_ascii_case("authorization") {
            h.value = String::new();
        }
    }
    kept
}

impl Engine {
    /// Sends a request an action built with the codec's builder for its route (`build` gets the hub, the token
    /// and the client header). Returns the id its [`Report::Answer`] comes under. `keep`: a name for a request
    /// with side effects at the hub, kept across a restart (see the module); at most 64 characters of
    /// `[a-z0-9/_-]`. Refused: `bad-request` (it does not build, or a bad name), `busy` (too many wait),
    /// `stopped`.
    pub fn request(&mut self, ctx: &mut Ctx<'_>, build: impl FnOnce(&Site<'_>) -> Result<HttpRequest, Failure>, keep: Option<String>) -> Result<IoId, &'static str> {
        if self.stopped.is_some() {
            return Err("stopped");
        }
        let plain = |n: &str| !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"/_-".contains(&b));
        if keep.as_deref().is_some_and(|n| !plain(n)) {
            return Err("bad-request");
        }
        if self.requests.outs.len() >= REQUESTS_MAX || (keep.is_some() && self.requests.outs.values().filter(|o| o.keep.is_some()).count() >= KEPT_MAX) {
            return Err("busy");
        }
        let site = Site { url: &self.hub, token: Some(self.token.as_ref().map_or("waits-for-a-sign-in", |t| t.value.as_str())), client: &self.client };
        let request = build(&site).map_err(|_| "bad-request")?;
        if let Some(name) = &keep {
            let Ok(value) = serde_json::to_vec(&without_token(&request)) else { return Err("bad-request") };
            let id = ctx.id();
            self.pending.insert(id, Asked::KeptWrite);
            ctx.out.push(Effect::Store { id, op: StoreOp::Put { put: vec![Entry { key: kept_key(name), value: value.into() }], delete: Vec::new() } });
        }
        let id = ctx.id();
        self.requests.outs.insert(id, Out { request, keep, flying: None, tries: 0, retry: Retry::default() });
        self.drive(ctx);
        Ok(id)
    }

    /// Sends what waits and may go now: a request that needs the token only with one.
    pub(super) fn drive_requests(&mut self, ctx: &mut Ctx<'_>) {
        let token = self.token.as_ref().map(|t| t.value.clone());
        let due: Vec<IoId> = self.requests.outs.iter().filter(|(_, o)| o.flying.is_none() && !o.retry.waiting(ctx.now) && (token.is_some() || !needs_token(&o.request))).map(|(id, _)| *id).collect();
        if !due.is_empty() {
            // (what the device wrote goes first, as for every request)
            self.flush(ctx);
        }
        for id in due {
            let Some(out) = self.requests.outs.get_mut(&id) else { continue };
            let mut request = out.request.clone();
            if let Some(token) = &token {
                for h in &mut request.headers {
                    if h.name.eq_ignore_ascii_case("authorization") {
                        *h = Header { name: "authorization".into(), value: format!("Bearer {token}") };
                    }
                }
            }
            let flying = ctx.id();
            out.flying = Some(flying);
            out.tries += 1;
            self.pending.insert(flying, Asked::Request { id });
            ctx.out.push(Effect::Http { id: flying, request });
        }
    }

    /// The result of one sending of a request.
    pub(super) fn on_request(&mut self, ctx: &mut Ctx<'_>, id: IoId, result: IoResult<HttpResponse>) {
        let Some(mut out) = self.requests.outs.remove(&id) else { return };
        out.flying = None;
        let (result, unauthorised, retry_after) = match result {
            IoResult::Ok(response) => {
                let refusal = self.codec.refusal(&response);
                let unauthorised = refusal.as_ref().is_some_and(|r| r.code == "unauthorised") && needs_token(&out.request);
                match crate::hub::classify(&response) {
                    Err(Failure::NotNow { retry_after_ms, .. }) => (Err(Failure::NotNow { status: response.status, retry_after_ms }), false, retry_after_ms),
                    _ => (Ok(response), unauthorised, None),
                }
            }
            IoResult::Err(error) => (Err(classify_io(&error).unwrap_or(Failure::Offline)), false, None),
        };
        if unauthorised {
            // the token ended: signed in again, and the same request goes with the new one
            self.token = None;
            self.unauthorised += 1;
            if self.unauthorised > 1 {
                self.auth_retry.fail(ctx.now, None, &mut self.jitter);
            }
            if out.tries < 8 {
                self.requests.outs.insert(id, out);
                return;
            }
        }
        let transient = result.is_err();
        let interactive = out.request.class == HttpClass::Interactive;
        if transient && !(interactive && out.tries >= INTERACTIVE_TRIES) {
            out.retry.fail(ctx.now, retry_after, &mut self.jitter);
            self.requests.outs.insert(id, out);
            return;
        }
        if let Some(name) = &out.keep {
            if !transient {
                let key = kept_key(name);
                let write = ctx.id();
                self.pending.insert(write, Asked::KeptWrite);
                ctx.out.push(Effect::Store { id: write, op: StoreOp::Put { put: Vec::new(), delete: vec![key] } });
            }
        }
        self.report(Report::Answer(Box::new(requests::Answered { id, keep: out.keep, result })));
    }

    /// At the start: the kept requests of an earlier run are read, to be sent again.
    pub(super) fn scan_kept(&mut self, ctx: &mut Ctx<'_>) {
        let id = ctx.id();
        self.requests.scanning = Some(id);
        self.pending.insert(id, Asked::KeptScan);
        ctx.out.push(Effect::Store { id, op: StoreOp::Scan { prefix: KEPT_PREFIX.into() } });
    }

    pub(super) fn on_kept(&mut self, ctx: &mut Ctx<'_>, result: &IoResult<StoreAnswer>) {
        self.requests.scanning = None;
        let IoResult::Ok(StoreAnswer::Entries { entries }) = result else {
            ctx.log(LogLevel::Warn, "the kept requests were not read");
            return;
        };
        for entry in entries.iter().take(KEPT_MAX) {
            let name = entry.key.strip_prefix(KEPT_PREFIX).unwrap_or_default().to_string();
            let Ok(request) = serde_json::from_slice::<HttpRequest>(&entry.value.0) else { continue };
            // (only to this engine's hub: a kept request never names another address)
            if name.is_empty() || !request.url.starts_with(&format!("{}/", self.hub)) || self.requests.outs.values().any(|o| o.keep.as_deref() == Some(&name)) {
                continue;
            }
            let id = ctx.id();
            self.requests.outs.insert(id, Out { request, keep: Some(name), flying: None, tries: 0, retry: Retry::default() });
        }
    }
}
