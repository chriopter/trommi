//! The App: one object per room on a device, and the whole of the app's logic. Sans-I/O: [`App::handle`] takes one
//! [`Event`] and the shell's clock and answers with [`Effect`]s. It does no I/O, reads no clock, starts no thread
//! and holds no callback. Every shell (web worker, iOS, Linux, a test) drives it the same way:
//!
//! ```text
//! shell                                 App
//!   Event::Start{config}         ──▶   handle(now, event) ──▶ [Render, Store{DeviceLoad}, …]
//!   runs the effects, in order
//!   Event::Store{id, result}     ──▶   handle(now, event) ──▶ [Render, Http{…}, OpenStream{…}, Timer{…}]
//!   Event::Http / Stream / Wake  ──▶   …
//!   Event::Action{action}        ──▶   …  (the person did something)
//! ```
//!
//! core/client/README.md, "The app core", is the contract in words: the order of effects, errors and retries,
//! threads per platform, the stored format, and what a shell may and may not do.
//!
//! This is W0's skeleton: it starts, reads the device state, and shows `SignedOut` or `Opening`. W1 (engine), W3
//! (actions, account) and W4 (views) fill it in behind this same surface.

pub mod bytes;
pub mod edge;
pub mod effect;
pub mod event;
pub mod io;
pub mod script;
pub mod sim;

pub use bytes::Bytes;
pub use effect::{Effect, LogLevel, Notice, WidgetSnapshot};
pub use event::{Abilities, Config, Event};
pub use io::{
    DeviceEntry, Entry, Header, HttpClass, HttpRequest, HttpResponse, IoError, IoErrorKind, IoId,
    IoResult, PlatformAnswer, PlatformOp, SecretOp, StoreAnswer, StoreOp, StreamItem,
};

use crate::engine::{self, Connection, Engine, Jitter, OpenError, Report};
use crate::views::{Lens, Phase, Screen, ViewModel};
use std::collections::BTreeMap;

/// The hub of a shell that names none.
pub const DEFAULT_HUB: &str = "https://hub.trommi.com";
/// At most so many of the engine's alerts are kept for the views.
const ALERTS_MAX: usize = 200;

/// A device that closed itself is opened again in place at most so many times in a row (the count starts anew once
/// it is live again, or the app comes to the front).
pub const REOPEN_TRIES: u32 = 3;
/// At most so many invites with a Request wait for the invite flow.
const INVITE_REQUESTS_MAX: usize = 64;

/// What a piece of I/O in flight was for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    DeviceLoad,
    /// A cache entry deleted (a file the hub evicted): nothing waits for it.
    CacheDelete,
}

pub struct App {
    config: Option<Config>,
    next_id: u64,
    pending: BTreeMap<IoId, Pending>,
    /// The view model as the core decides it now.
    next_view: ViewModel,
    /// The view model as the shell has it (what was rendered so far).
    view: ViewModel,
    rendered_once: bool,
    /// The App's own draws (the waits before a closed device is opened again); the engine draws its own.
    jitter: Jitter,
    /// The sync engine, once the stored device opened.
    engine: Option<Engine>,
    /// The timer the shell holds.
    timer: Option<u64>,
    /// What the engine reported for the person (until the views show them: W4).
    alerts: Vec<engine::Alert>,
    sessions_not_joined: Option<Vec<(trommi_core::ids::GroupId, &'static str)>>,
    /// The board model, fed from the engine's reports.
    board: engine::feed::Board,
    /// What this device alone chooses: the desk and the pages in view, the zone (`Event::Focus`, `Event::Zone`).
    lens: Lens,
    /// The board's units are to be made again: the model, the lens or the clock moved.
    stale_views: bool,
    /// When the board's units change by the clock alone.
    view_wake: Option<u64>,
    /// The device's source of randomness when it is not the system's (a seeded test).
    entropy: Option<Box<dyn trommi_core::crypto::Entropy + Send>>,
    /// Invites of this room a newcomer sent its Request for, oldest first (the invite flow reads them: W3).
    invite_requests: Vec<trommi_core::ids::InviteId>,
    /// The device closed itself (a failed write, a failure of its own): when the stored state is read again, and
    /// how often it was in a row.
    reopen_at: Option<u64>,
    reopen_tries: u32,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        App {
            config: None,
            next_id: 1,
            pending: BTreeMap::new(),
            next_view: ViewModel::default(),
            view: ViewModel::default(),
            rendered_once: false,
            jitter: Jitter::new(0),
            engine: None,
            timer: None,
            alerts: Vec::new(),
            sessions_not_joined: None,
            board: engine::feed::Board::default(),
            lens: Lens::default(),
            stale_views: false,
            view_wake: None,
            entropy: None,
            invite_requests: Vec::new(),
            reopen_at: None,
            reopen_tries: 0,
        }
    }

    /// An App whose device draws its randomness from `entropy` and not from the system: for a test that must
    /// repeat exactly. A shipped app uses [`App::new`].
    pub fn with_entropy(entropy: Box<dyn trommi_core::crypto::Entropy + Send>) -> Self {
        App {
            entropy: Some(entropy),
            ..Self::new()
        }
    }

    /// The board model as the engine's reports left it (the views are made from it: W4).
    pub fn board(&self) -> &engine::feed::Board {
        &self.board
    }

    /// The sync engine, once the stored device opened.
    pub fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }

    /// An action on the device (seal, commit, answer) at the shell's clock: what it wrote is stored before anything
    /// it implies is sent, and the outbox is pumped. The actions (W3) go through this; `None` without a device.
    pub fn act<T>(
        &mut self,
        now_ms: u64,
        action: impl FnOnce(&mut engine::Device, u64) -> Result<T, trommi_core::Error>,
    ) -> (Option<Result<T, trommi_core::Error>>, Vec<Effect>) {
        let mut out = Vec::new();
        let result = self.with_engine(now_ms, &mut out, |engine, ctx| engine.act(ctx, action));
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        (result, out)
    }

    /// Joins the live session groups of a sign-in with the code (`engine::sync::joins`): read and posted together.
    /// The sign-in's flow calls this once the device is in the room; nothing happens without a device.
    pub fn join_sessions(&mut self, now_ms: u64, joins: engine::sync::joins::SessionJoins) -> Vec<Effect> {
        let mut out = Vec::new();
        self.with_engine(now_ms, &mut out, |engine, ctx| engine.join_sessions(ctx, joins));
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        out
    }

    /// Once a sign-in's sessions are through: the ones this device is not in, each with the reason. None before.
    pub fn sessions_not_joined(&self) -> Option<&[(trommi_core::ids::GroupId, &'static str)]> {
        self.sessions_not_joined.as_deref()
    }

    /// The invites a newcomer sent its Request for since they were last taken, oldest first, each once.
    pub fn take_invite_requests(&mut self) -> Vec<trommi_core::ids::InviteId> {
        std::mem::take(&mut self.invite_requests)
    }

    /// The engine's alerts so far, oldest first.
    pub fn alerts(&self) -> &[engine::Alert] {
        &self.alerts
    }

    /// The view model as last rendered: what a shell shows after folding every `Render` it was given.
    pub fn view(&self) -> &ViewModel {
        &self.view
    }

    /// One event in, the effects out. `now_ms` is the shell's clock (ms since 1970).
    pub fn handle(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        let mut out = Vec::new();
        match event {
            Event::Start { config } => {
                if self.config.is_some() {
                    out.push(log(LogLevel::Warn, "start: already started"));
                } else {
                    self.jitter = Jitter::new(config.seed);
                    self.lens.utc_offset_min = config.utc_offset_min;
                    // (the Desk's greeting: one for the whole run)
                    self.lens.dice = (config.seed ^ (config.seed >> 32)) as u32;
                    self.config = Some(config);
                    self.next_view.phase = Phase::Starting;
                    let id = self.io(Pending::DeviceLoad);
                    out.push(Effect::Store {
                        id,
                        op: StoreOp::DeviceLoad,
                    });
                }
            }
            _ if self.config.is_none() => out.push(log(LogLevel::Warn, "an event before start")),
            Event::Store { id, result } => match self.pending.remove(&id) {
                Some(Pending::DeviceLoad) => match result {
                    IoResult::Ok(StoreAnswer::Device { revision, entries }) => {
                        self.open(now_ms, revision, entries, &mut out)
                    }
                    IoResult::Ok(_) => {
                        out.push(log(LogLevel::Error, "device load: not a device answer"))
                    }
                    IoResult::Err(e) => {
                        out.push(log(LogLevel::Error, &format!("device load: {:?}", e.kind)));
                        self.closed(now_ms, &mut out);
                    }
                },
                // (a cache entry that stays is only a cache entry: the hub no longer serves the file)
                Some(Pending::CacheDelete) => {}
                None => {
                    let mine = self.with_engine(now_ms, &mut out, |engine, ctx| {
                        engine.on_store(ctx, id, &result)
                    });
                    if mine != Some(true) {
                        out.push(log(LogLevel::Warn, "store: an answer to nothing asked"));
                    }
                }
            },
            Event::Http { id, result } => {
                let mine = self.with_engine(now_ms, &mut out, |engine, ctx| {
                    engine.on_http(ctx, id, result)
                });
                if mine != Some(true) && self.pending.remove(&id).is_none() {
                    out.push(log(LogLevel::Warn, "an answer to nothing asked"));
                }
            }
            Event::Secret { id, .. } | Event::Platform { id, .. } => {
                if self.pending.remove(&id).is_none() {
                    out.push(log(LogLevel::Warn, "an answer to nothing asked"));
                }
            }
            Event::Stream { id, item } => {
                // (an item of a stream the engine closed is dropped: the id is forgotten)
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_stream(ctx, id, item));
            }
            Event::Progress { .. } | Event::Finished { .. } => {}
            Event::PushToken { .. } => {}
            Event::Zone { utc_offset_min } => {
                self.lens.utc_offset_min = utc_offset_min;
                self.stale_views = true;
            }
            Event::Action { .. } => {}
            Event::Focus { focus } => {
                // (the single desk chosen last keeps the note and the Scribble Board on All desks)
                if let Some(desk) = focus.desk.as_deref().filter(|d| *d != crate::views::board::ALL_DESKS) {
                    self.lens.home_desk = Some(desk.to_string());
                }
                self.lens.desk = focus.desk;
                let (session, card, off) = match focus.screen {
                    Screen::Session { session_id } => (Some(session_id), None, None),
                    Screen::Card { card_id } => (None, Some(card_id), None),
                    Screen::Off { query } => (None, None, Some(query)),
                    _ => (None, None, None),
                };
                self.lens.session = session;
                self.lens.card = card;
                self.lens.off = off;
                self.stale_views = true;
            }
            Event::Wake => {
                self.timer = None;
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_wake(ctx));
                if self.reopen_at.is_some_and(|at| now_ms >= at) {
                    self.reopen(&mut out);
                }
            }
            Event::Foreground => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_foreground(ctx));
                // (one that gave up on its stored state tries again when the person comes back)
                if self.next_view.phase == Phase::Stopped && self.reopen_tries > REOPEN_TRIES {
                    self.reopen_tries = 0;
                    self.reopen(&mut out);
                }
            }
            Event::Background => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_background(ctx));
            }
            Event::Net { online } => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_net(ctx, online));
            }
            Event::Push { .. } => {}
        }
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        out
    }

    /// The stored device as the shell loaded it: an engine over it, or the sign-in screens.
    fn open(&mut self, now_ms: u64, revision: u64, entries: Vec<DeviceEntry>, out: &mut Vec<Effect>) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let hub = if config.hub.is_empty() {
            DEFAULT_HUB
        } else {
            config.hub.as_str()
        };
        let client = format!("{}/{}", config.platform, config.version);
        let entropy = self
            .entropy
            .take()
            .unwrap_or_else(|| Box::new(trommi_core::crypto::SystemEntropy));
        // (opened again in place: the board stays as it was shown, the new engine brings what is new)
        let again = self.engine.is_some();
        match Engine::open_with(hub, &client, config.seed, revision, entries, entropy) {
            Ok(engine) if again && engine.device().id() != self.board_device() => {
                out.push(log(LogLevel::Error, "device open: another device is stored now"));
                self.next_view.phase = Phase::Stopped;
            }
            Ok(engine) => {
                if !again {
                    self.board = engine::feed::Board::new(engine.device());
                }
                self.stale_views = true;
                self.engine = Some(engine);
                self.next_view.phase = Phase::Opening;
                self.with_engine(now_ms, out, |engine, ctx| engine.start(ctx));
            }
            Err(OpenError::NoRoom) if !again => self.next_view.phase = Phase::SignedOut,
            Err(OpenError::NoRoom) => {
                out.push(log(LogLevel::Error, "device open: the stored state holds no room now"));
                self.next_view.phase = Phase::Stopped;
            }
            Err(OpenError::Device(code)) => {
                out.push(log(LogLevel::Error, &format!("device open: {code}")));
                self.closed(now_ms, out);
            }
            Err(OpenError::Hub) => {
                out.push(log(LogLevel::Error, "start: the hub's address is not canonical"));
                self.next_view.phase = Phase::Stopped;
            }
        }
    }

    /// The id of the device the board is of (the one opened first).
    fn board_device(&self) -> trommi_core::ids::DeviceId {
        self.engine.as_ref().map_or(trommi_core::ids::DeviceId::new([0; 32]), |e| e.device().id())
    }

    /// The device closed itself, or its stored state did not open again: read again after a wait, at most
    /// [`REOPEN_TRIES`] times in a row; then the App stops. Only a device that ran here is opened again in place.
    fn closed(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.engine.is_none() {
            self.next_view.phase = Phase::Stopped;
            return;
        }
        self.reopen_tries += 1;
        if self.reopen_tries > REOPEN_TRIES {
            out.push(log(LogLevel::Error, "the device does not open again: stopped"));
            self.reopen_at = None;
            self.next_view.phase = Phase::Stopped;
            return;
        }
        let wait = engine::Backoff::HUB.delay_ms(self.reopen_tries, None, self.jitter.draw());
        self.reopen_at = Some(now_ms + wait);
        self.next_view.phase = Phase::Opening;
    }

    /// Reads the stored state again, to open it in place of the closed device.
    fn reopen(&mut self, out: &mut Vec<Effect>) {
        self.reopen_at = None;
        let id = self.io(Pending::DeviceLoad);
        out.push(Effect::Store { id, op: StoreOp::DeviceLoad });
    }

    fn with_engine<T>(
        &mut self,
        now_ms: u64,
        out: &mut Vec<Effect>,
        step: impl FnOnce(&mut Engine, &mut engine::Ctx<'_>) -> T,
    ) -> Option<T> {
        let engine = self.engine.as_mut()?;
        let mut ctx = engine::Ctx {
            now: now_ms,
            next_id: &mut self.next_id,
            out,
        };
        Some(step(engine, &mut ctx))
    }

    /// What the engine reported since the last event: the phase, the alerts, the timer.
    fn after_engine(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let mut fed = false;
        let mut evicted = Vec::new();
        let mut stopped = false;
        while let Some(report) = engine.next_report() {
            self.board.take(&report, engine.device(), now_ms);
            fed = true;
            match report {
                Report::Connection(Connection::Live) => {
                    self.next_view.phase = Phase::Live;
                    self.reopen_tries = 0;
                }
                Report::Connection(Connection::Offline) => self.next_view.phase = Phase::Offline,
                Report::Connection(Connection::CatchingUp) => {
                    if self.next_view.phase != Phase::Live {
                        self.next_view.phase = Phase::Opening
                    }
                }
                Report::Stopped(why) if why.reopens() => stopped = true,
                Report::Stopped(_) => self.next_view.phase = Phase::Stopped,
                Report::SessionsJoined { not_joined, .. } => self.sessions_not_joined = Some(not_joined),
                Report::InviteRequest { invite } => {
                    if !self.invite_requests.contains(&invite) {
                        if self.invite_requests.len() >= INVITE_REQUESTS_MAX {
                            self.invite_requests.remove(0);
                        }
                        self.invite_requests.push(invite);
                    }
                }
                Report::FileEvicted { file } => evicted.push(file),
                Report::Alert(alert) => {
                    if self.alerts.len() >= ALERTS_MAX {
                        self.alerts.remove(0);
                    }
                    self.alerts.push(alert);
                }
                _ => {}
            }
        }
        if !evicted.is_empty() {
            // the cached bytes of a file the hub deleted (11.3) go
            let room = crate::engine::feed::hex(engine.room().as_bytes());
            let delete = evicted.iter().map(|file| crate::cache::key(&room, crate::cache::part::FILE, Some(&crate::engine::feed::hex(file.as_bytes())))).collect();
            let id = IoId(self.next_id);
            self.next_id += 1;
            self.pending.insert(id, Pending::CacheDelete);
            out.push(Effect::Store { id, op: StoreOp::Put { put: Vec::new(), delete } });
        }
        if fed || (self.board.model.desk_wake != 0 && now_ms >= self.board.model.desk_wake) {
            let _ = self.board.finish(now_ms);
            self.stale_views = true;
        }
        // The board's units: made again when the model, the lens or the clock moved (every unit whole; a unit that
        // did not change is not sent, `ViewModel::diff`).
        if self.stale_views || self.view_wake.is_some_and(|at| now_ms >= at) {
            self.stale_views = false;
            self.lens.reading = self.board.model.reading;
            self.view_wake = crate::views::render(&self.board.model, &self.lens, now_ms, &mut self.next_view);
        }
        let engine_wake = engine.next_wake();
        if stopped {
            self.closed(now_ms, out);
        }
        let model_wake = Some(self.board.model.desk_wake).filter(|at| *at != 0);
        let wake = [engine_wake, model_wake, self.view_wake, self.reopen_at].into_iter().flatten().min();
        if wake != self.timer {
            self.timer = wake;
            out.push(Effect::Timer { at_ms: wake });
        }
    }

    fn io(&mut self, what: Pending) -> IoId {
        let id = IoId(self.next_id);
        self.next_id += 1;
        self.pending.insert(id, what);
        id
    }

    /// Puts a `Render` first when the view changed: the shell draws before it starts the I/O.
    fn render(&mut self, out: &mut Vec<Effect>) {
        let delta = if !self.rendered_once {
            self.rendered_once = true;
            let mut whole = self.next_view.clone();
            whole.rev = 0;
            Some(crate::views::ViewDelta {
                base: 0,
                parts: vec![crate::views::ViewPart::Whole {
                    model: Box::new(whole),
                }],
            })
        } else {
            self.view.diff(&self.next_view)
        };
        if let Some(delta) = delta {
            // the core's own fold, so `view` is exactly what a shell holds
            if self.view.apply(&delta).is_ok() {
                self.next_view.rev = self.view.rev;
                out.insert(0, Effect::Render { delta });
            }
        }
    }
}

fn log(level: LogLevel, text: &str) -> Effect {
    Effect::Log {
        level,
        text: text.to_string(),
    }
}

#[cfg(test)]
mod tests;
