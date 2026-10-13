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

use crate::engine::{self, Codec, Connection, Engine, Jitter, OpenError, Report};
use crate::views::{Phase, ViewModel};
use std::collections::BTreeMap;

/// The hub of a shell that names none.
pub const DEFAULT_HUB: &str = "https://hub.trommi.com";
/// At most so many of the engine's alerts are kept for the views.
const ALERTS_MAX: usize = 200;

/// What a piece of I/O in flight was for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    DeviceLoad,
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
    #[allow(dead_code)] // (the engine draws its own from the same seed)
    jitter: Jitter,
    /// The sync engine, once the stored device opened.
    engine: Option<Engine<Codec>>,
    /// The timer the shell holds.
    timer: Option<u64>,
    /// What the engine reported for the person (until the views show them: W4).
    alerts: Vec<engine::Alert>,
    /// The board model, fed from the engine's reports.
    board: engine::feed::Board,
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
            board: engine::feed::Board::default(),
        }
    }

    /// The board model as the engine's reports left it (the views are made from it: W4).
    pub fn board(&self) -> &engine::feed::Board {
        &self.board
    }

    /// The sync engine, once the stored device opened.
    pub fn engine(&self) -> Option<&Engine<Codec>> {
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
                        self.next_view.phase = Phase::Stopped;
                    }
                },
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
            Event::PushToken { .. } | Event::Zone { .. } => {}
            Event::Action { .. } => {}
            Event::Focus { .. } => {}
            Event::Wake => {
                self.timer = None;
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_wake(ctx));
            }
            Event::Foreground => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_foreground(ctx));
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
        match Engine::open(Codec::default(), hub, &client, config.seed, revision, entries) {
            Ok(engine) => {
                self.board = engine::feed::Board::new(engine.device());
                self.engine = Some(engine);
                self.next_view.phase = Phase::Opening;
                self.with_engine(now_ms, out, |engine, ctx| engine.start(ctx));
            }
            Err(OpenError::NoRoom) => self.next_view.phase = Phase::SignedOut,
            Err(OpenError::Device(code)) => {
                out.push(log(LogLevel::Error, &format!("device open: {code}")));
                self.next_view.phase = Phase::Stopped;
            }
            Err(OpenError::Hub) => {
                out.push(log(LogLevel::Error, "start: the hub's address is not canonical"));
                self.next_view.phase = Phase::Stopped;
            }
        }
    }

    fn with_engine<T>(
        &mut self,
        now_ms: u64,
        out: &mut Vec<Effect>,
        step: impl FnOnce(&mut Engine<Codec>, &mut engine::Ctx<'_>) -> T,
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
        while let Some(report) = engine.next_report() {
            self.board.take(&report, engine.device(), now_ms);
            fed = true;
            match report {
                Report::Connection(Connection::Live) => self.next_view.phase = Phase::Live,
                Report::Connection(Connection::Offline) => self.next_view.phase = Phase::Offline,
                Report::Connection(Connection::CatchingUp) => {
                    if self.next_view.phase != Phase::Live {
                        self.next_view.phase = Phase::Opening
                    }
                }
                Report::Stopped(_) => self.next_view.phase = Phase::Stopped,
                Report::Alert(alert) => {
                    if self.alerts.len() >= ALERTS_MAX {
                        self.alerts.remove(0);
                    }
                    self.alerts.push(alert);
                }
                _ => {}
            }
        }
        if fed || (self.board.model.desk_wake != 0 && now_ms >= self.board.model.desk_wake) {
            // (W4: the views are made from what this says changed)
            let _ = self.board.finish(now_ms);
        }
        let model_wake = Some(self.board.model.desk_wake).filter(|at| *at != 0);
        let wake = match (engine.next_wake(), model_wake) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
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
