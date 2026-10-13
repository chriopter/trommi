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

use crate::engine::Jitter;
use crate::views::{Phase, ViewModel};
use std::collections::BTreeMap;

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
    #[allow(dead_code)] // W1: retries
    jitter: Jitter,
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
        }
    }

    /// The view model as last rendered: what a shell shows after folding every `Render` it was given.
    pub fn view(&self) -> &ViewModel {
        &self.view
    }

    /// One event in, the effects out. `now_ms` is the shell's clock (ms since 1970).
    pub fn handle(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        let _ = now_ms; // W1: every rule that needs time reads it from here
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
                    IoResult::Ok(StoreAnswer::Device { revision: 0, .. }) => {
                        self.next_view.phase = Phase::SignedOut
                    }
                    IoResult::Ok(StoreAnswer::Device { .. }) => {
                        self.next_view.phase = Phase::Opening
                    }
                    IoResult::Ok(_) => {
                        out.push(log(LogLevel::Error, "device load: not a device answer"))
                    }
                    IoResult::Err(e) => {
                        out.push(log(LogLevel::Error, &format!("device load: {:?}", e.kind)));
                        self.next_view.phase = Phase::Stopped;
                    }
                },
                None => out.push(log(LogLevel::Warn, "store: an answer to nothing asked")),
            },
            Event::Http { id, .. } | Event::Secret { id, .. } | Event::Platform { id, .. } => {
                if self.pending.remove(&id).is_none() {
                    out.push(log(LogLevel::Warn, "an answer to nothing asked"));
                }
            }
            Event::Stream { .. } | Event::Progress { .. } | Event::Finished { .. } => {}
            Event::PushToken { .. } | Event::Zone { .. } => {}
            Event::Action { .. } => {}
            Event::Focus { .. } => {}
            Event::Wake
            | Event::Foreground
            | Event::Background
            | Event::Net { .. }
            | Event::Push { .. } => {}
        }
        self.render(&mut out);
        out
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
