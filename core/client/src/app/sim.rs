//! A fake I/O executor: runs an App's effects in memory, with a clock the test moves. The smallest shell there is,
//! and the seed of W6's simulated network (drops, 5xx, reordering, cuts, kill and restart from the store).
//!
//! - **Stores** are maps, kept across a restart of the App (`restart` makes a new App over the same stores).
//! - **HTTP and streams** go to a function the test gives (`hub`): the stand-in hub. Without one every request
//!   is `offline`.
//!   An answer longer than the request's `max_answer` is cut off as a shell must: `too_large`, no body. With
//!   `hold` the requests wait in `held` for the test to answer by hand (`answer`).
//! - **Streams**: with `stream_hub` an `OpenStream` stays open (`streams`) and gets what that function answers at
//!   once; the test sends what follows (`stream`). Without one a stream fails `offline`.
//! - **Timers**: one, as the contract says; `advance` moves the clock and fires it when due.
//! - **Renders** are folded into `view`, exactly as a shell does.

use super::{
    App, DeviceEntry, Effect, Entry, Event, HttpRequest, HttpResponse, IoError, IoErrorKind, IoId,
    IoResult, StoreAnswer, StoreOp, StreamItem,
};
use crate::views::ViewModel;
use std::collections::{BTreeMap, VecDeque};

pub type Hub = Box<dyn FnMut(u64, &HttpRequest) -> Result<HttpResponse, IoError>>;
/// What a stream brings the moment it is opened (its head, what the hub replays).
pub type StreamHub = Box<dyn FnMut(u64, &HttpRequest) -> Vec<StreamItem>>;
/// The platform's services (passkeys, push); without one every `Platform` effect is `Refused`.
pub type Platform = Box<dyn FnMut(&super::PlatformOp) -> IoResult<super::PlatformAnswer>>;

pub struct Sim {
    pub app: App,
    pub now_ms: u64,
    pub view: ViewModel,
    pub cache: BTreeMap<String, Vec<u8>>,
    pub device: (u64, BTreeMap<Vec<u8>, Vec<u8>>),
    pub timer: Option<u64>,
    pub hub: Option<Hub>,
    pub stream_hub: Option<StreamHub>,
    pub platform: Option<Platform>,
    /// The platform's secret store; without one every `Secret` effect is `denied`.
    pub secrets: Option<BTreeMap<String, Vec<u8>>>,
    /// The streams that are open, oldest first.
    pub streams: Vec<(IoId, HttpRequest)>,
    /// Requests wait in `held` to be answered by hand.
    pub hold: bool,
    /// The process dies once so many more effects were run (`restart` brings a new one): the effect it dies before
    /// and everything behind it never happens, and no event reaches the App any more.
    pub die_in: Option<usize>,
    pub dead: bool,
    pub held: VecDeque<(IoId, HttpRequest)>,
    /// So many of the next device writes fail as a full store does (`full`), and so many device loads (`failed`).
    pub device_full: u32,
    pub load_fails: u32,
    /// Every effect the App asked for, in order (for assertions).
    pub log: Vec<Effect>,
    queue: VecDeque<Event>,
}

impl Sim {
    pub fn new(now_ms: u64) -> Self {
        Sim {
            app: App::new(),
            now_ms,
            view: ViewModel::default(),
            cache: BTreeMap::new(),
            device: (0, BTreeMap::new()),
            timer: None,
            hub: None,
            stream_hub: None,
            platform: None,
            secrets: None,
            streams: Vec::new(),
            hold: false,
            die_in: None,
            dead: false,
            held: VecDeque::new(),
            device_full: 0,
            load_fails: 0,
            log: Vec::new(),
            queue: VecDeque::new(),
        }
    }

    /// Sends one event and runs everything that follows from it until nothing is left to do now.
    pub fn send(&mut self, event: Event) {
        if self.dead {
            return;
        }
        self.queue.push_back(event);
        while let Some(event) = self.queue.pop_front() {
            let effects = self.app.handle(self.now_ms, event);
            self.execute(effects);
        }
    }

    /// Runs effects the test got from the App by hand (`App::act`), and everything that follows from them.
    pub fn run(&mut self, effects: Vec<Effect>) {
        self.execute(effects);
        while let Some(event) = self.queue.pop_front() {
            let effects = self.app.handle(self.now_ms, event);
            self.execute(effects);
        }
    }

    /// Answers a held request.
    pub fn answer(&mut self, id: IoId, result: Result<HttpResponse, IoError>) {
        self.held.retain(|(held, _)| *held != id);
        self.send(Event::Http {
            id,
            result: result.into(),
        });
    }

    /// Sends an item on the newest open stream; `ended` and `failed` close it. False: no stream is open.
    pub fn stream(&mut self, item: StreamItem) -> bool {
        let Some((id, _)) = self.streams.last().cloned() else {
            return false;
        };
        if matches!(item, StreamItem::Ended | StreamItem::Failed { .. }) {
            self.streams.pop();
        }
        self.send(Event::Stream { id, item });
        true
    }

    /// Moves the clock; fires the timer when it is due.
    pub fn advance(&mut self, ms: u64) {
        self.now_ms += ms;
        if self.timer.is_some_and(|at| at <= self.now_ms) {
            self.timer = None;
            self.send(Event::Wake);
        }
    }

    /// The process died: a new App over the same stores; the view is gone with the process.
    pub fn restart(&mut self) {
        self.app = App::new();
        self.view = ViewModel::default();
        self.timer = None;
        self.queue.clear();
        self.streams.clear();
        self.held.clear();
        self.die_in = None;
        self.dead = false;
    }

    /// Runs effects as a shell does: in order, a failed device write ending the rest. Results are queued.
    pub fn execute(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            if self.dead {
                return;
            }
            if let Some(left) = self.die_in.as_mut() {
                if *left == 0 {
                    self.dead = true;
                    self.queue.clear();
                    return;
                }
                *left -= 1;
            }
            self.log.push(effect.clone());
            match effect {
                Effect::ShareInbox { .. } | Effect::ForgetShared => {},
                Effect::Render { delta } => self.view.apply(&delta).expect("renders come in order"),
                Effect::Store { id, op } => {
                    let failed = matches!(op, StoreOp::DeviceApply { .. });
                    let result = self.store(op);
                    let stop = failed && matches!(result, IoResult::Err(_));
                    self.queue.push_back(Event::Store { id, result });
                    if stop {
                        return; // the barrier: drop the rest of this answer
                    }
                }
                Effect::Http { id, request } => {
                    if self.hold {
                        self.held.push_back((id, request));
                        continue;
                    }
                    let result = match self.hub.as_mut() {
                        // (a shell stops reading at the cap and hands over no body cut short)
                        Some(hub) => match hub(self.now_ms, &request) {
                            Ok(answer) if answer.body.len() > request.max_answer as usize => {
                                IoResult::Err(IoError::new(IoErrorKind::TooLarge))
                            }
                            other => other.into(),
                        },
                        None => IoResult::Err(IoError::new(IoErrorKind::Offline)),
                    };
                    self.queue.push_back(Event::Http { id, result });
                }
                Effect::OpenStream { id, request } => match self.stream_hub.as_mut() {
                    Some(open) => {
                        let items = open(self.now_ms, &request);
                        let closed = items
                            .iter()
                            .any(|i| matches!(i, StreamItem::Ended | StreamItem::Failed { .. }));
                        if !closed {
                            self.streams.push((id, request));
                        }
                        for item in items {
                            self.queue.push_back(Event::Stream { id, item });
                        }
                    }
                    None => self.queue.push_back(Event::Stream {
                        id,
                        item: StreamItem::Failed {
                            error: IoError::new(IoErrorKind::Offline),
                        },
                    }),
                },
                Effect::Cancel { id } => {
                    self.streams.retain(|(open, _)| *open != id);
                    self.held.retain(|(held, _)| *held != id);
                }
                Effect::Secret { id, op } => self.queue.push_back(Event::Secret {
                    id,
                    result: match (self.secrets.as_mut(), op) {
                        (None, _) => IoResult::Err(IoError::new(IoErrorKind::Denied)),
                        (Some(s), super::SecretOp::Get { name }) => IoResult::Ok(s.get(&name).cloned().map(Into::into)),
                        (Some(s), super::SecretOp::Put { name, value }) => {
                            s.insert(name, value.0);
                            IoResult::Ok(None)
                        }
                        (Some(s), super::SecretOp::Delete { name }) => {
                            s.remove(&name);
                            IoResult::Ok(None)
                        }
                    },
                }),
                Effect::Platform { id, op } => self.queue.push_back(Event::Platform {
                    id,
                    result: match self.platform.as_mut() {
                        Some(platform) => platform(&op),
                        None => IoResult::Ok(super::PlatformAnswer::Refused),
                    },
                }),
                Effect::Timer { at_ms } => self.timer = at_ms,
                Effect::FileReady { .. }
                | Effect::FileGone { .. }
                | Effect::Notify { .. }
                | Effect::Snapshot { .. }
                | Effect::NotifyContext { .. }
                | Effect::SaveFile { .. }
                | Effect::OpenUrl { .. }
                | Effect::Log { .. } => {}
            }
        }
    }

    fn store(&mut self, op: StoreOp) -> IoResult<StoreAnswer> {
        match op {
            StoreOp::Get { keys } => IoResult::Ok(StoreAnswer::Values {
                values: keys
                    .iter()
                    .map(|k| self.cache.get(k).map(|v| v.clone().into()))
                    .collect(),
            }),
            StoreOp::Scan { prefix } => IoResult::Ok(StoreAnswer::Entries {
                entries: self
                    .cache
                    .range(prefix.clone()..)
                    .take_while(|(k, _)| k.starts_with(&prefix))
                    .map(|(k, v)| Entry {
                        key: k.clone(),
                        value: v.clone().into(),
                    })
                    .collect(),
            }),
            StoreOp::Keys { prefix } => IoResult::Ok(StoreAnswer::Keys {
                keys:self.cache.range(prefix.clone()..).take_while(|(key,_)| key.starts_with(&prefix)).map(|(key,_)| key.clone()).collect(),
            }),
            StoreOp::Put { put, delete } | StoreOp::PutDurable { put, delete } => {
                for k in delete {
                    self.cache.remove(&k);
                }
                for e in put {
                    self.cache.insert(e.key, e.value.0);
                }
                IoResult::Ok(StoreAnswer::Done)
            }
            StoreOp::DeviceLoad if self.load_fails > 0 => {
                self.load_fails -= 1;
                IoResult::Err(IoError::new(IoErrorKind::Failed))
            }
            StoreOp::DeviceLoad => IoResult::Ok(StoreAnswer::Device {
                revision: self.device.0,
                entries: self
                    .device
                    .1
                    .iter()
                    .map(|(k, v)| DeviceEntry {
                        key: k.clone().into(),
                        value: v.clone().into(),
                    })
                    .collect(),
            }),
            StoreOp::DeviceApply {
                expected_revision,
                put,
                delete,
            } => {
                if expected_revision != self.device.0 {
                    return IoResult::Err(IoError::new(IoErrorKind::Conflict));
                }
                if self.device_full > 0 {
                    self.device_full -= 1;
                    return IoResult::Err(IoError::new(IoErrorKind::Full));
                }
                for k in delete {
                    self.device.1.remove(&k.0);
                }
                for e in put {
                    self.device.1.insert(e.key.0, e.value.0);
                }
                self.device.0 += 1;
                IoResult::Ok(StoreAnswer::Done)
            }
        }
    }

    /// The ids of the I/O in the log, for tests that answer by hand.
    pub fn ids(&self) -> Vec<IoId> {
        self.log
            .iter()
            .filter_map(|e| match e {
                Effect::Http { id, .. }
                | Effect::OpenStream { id, .. }
                | Effect::Store { id, .. }
                | Effect::Secret { id, .. }
                | Effect::Platform { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }
}

/// An agent device's engine run in memory ([`crate::engine::Engine::become_agent`]): the same stores, stand-in
/// hub, streams and clock as [`Sim`], with the engine driven directly (an agent has no App and no views). Every
/// report the engine made is kept in `reports`.
pub struct AgentSim {
    /// The stores, the hub, the streams and the clock (its `app` is not used).
    pub io: Sim,
    pub engine: Option<crate::engine::Engine>,
    pub reports: Vec<crate::engine::Report>,
    /// The hub's address the engine is opened with.
    pub hub: String,
    /// What the connector stored when the device answered its invite: the engine is opened with it
    /// ([`crate::engine::Engine::open_agent`]).
    pub invite: Option<crate::engine::AgentInvite>,
    next_id: u64,
}

impl AgentSim {
    /// An engine over the device state in `io.device`, made an agent's; nothing is asked before [`AgentSim::start`].
    pub fn new(io: Sim, hub: &str) -> Self {
        let mut sim = AgentSim { io, engine: None, reports: Vec::new(), hub: hub.to_string(), invite: None, next_id: 1 };
        sim.open();
        sim
    }

    /// An engine over the device state in `io.device` of a device that answered an invite, opened with what its
    /// connector stored ([`crate::engine::Engine::open_agent`]).
    pub fn invited(io: Sim, hub: &str, invite: crate::engine::AgentInvite) -> Self {
        let mut sim = AgentSim { io, engine: None, reports: Vec::new(), hub: hub.to_string(), invite: Some(invite), next_id: 1 };
        sim.open();
        sim
    }

    /// The process died: a new engine over the same stores.
    pub fn restart(&mut self) {
        self.io.timer = None;
        self.io.queue.clear();
        self.io.streams.clear();
        self.io.held.clear();
        self.open();
    }

    fn open(&mut self) {
        let entries = self.io.device.1.iter().map(|(k, v)| DeviceEntry { key: k.clone().into(), value: v.clone().into() }).collect();
        let engine = match &self.invite {
            Some(invite) => crate::engine::Engine::open_agent(&self.hub, "test/0.0.0", 7, self.io.device.0, entries, invite.clone()).expect("the stored device opens"),
            None => {
                let mut engine = crate::engine::Engine::open(&self.hub, "test/0.0.0", 7, self.io.device.0, entries).expect("the stored device opens");
                engine.become_agent().expect("an agent device");
                engine
            }
        };
        self.engine = Some(engine);
    }

    pub fn engine(&mut self) -> &mut crate::engine::Engine {
        self.engine.as_mut().expect("an engine")
    }

    /// Calls the engine with a context and runs everything that follows.
    pub fn with<T>(&mut self, call: impl FnOnce(&mut crate::engine::Engine, &mut crate::engine::Ctx<'_>) -> T) -> T {
        let mut out = Vec::new();
        let result = {
            let mut ctx = crate::engine::Ctx { now: self.io.now_ms, next_id: &mut self.next_id, out: &mut out };
            call(self.engine.as_mut().expect("an engine"), &mut ctx)
        };
        self.run(out);
        result
    }

    pub fn start(&mut self) {
        self.with(|engine, ctx| engine.start(ctx));
    }

    /// Runs effects and the events that follow from them, until nothing is left to do now.
    pub fn run(&mut self, effects: Vec<Effect>) {
        self.io.execute(effects);
        while let Some(event) = self.io.queue.pop_front() {
            let mut out = Vec::new();
            {
                let engine = self.engine.as_mut().expect("an engine");
                let mut ctx = crate::engine::Ctx { now: self.io.now_ms, next_id: &mut self.next_id, out: &mut out };
                match event {
                    Event::Http { id, result } => {
                        engine.on_http(&mut ctx, id, result);
                    }
                    Event::Stream { id, item } => {
                        engine.on_stream(&mut ctx, id, item);
                    }
                    Event::Store { id, result } => {
                        engine.on_store(&mut ctx, id, &result);
                    }
                    Event::Wake => engine.on_wake(&mut ctx),
                    _ => {}
                }
            }
            self.io.execute(out);
            self.drain();
        }
        self.drain();
    }

    fn drain(&mut self) {
        let engine = self.engine.as_mut().expect("an engine");
        while let Some(report) = engine.next_report() {
            self.reports.push(report);
        }
        self.io.timer = engine.next_wake();
    }

    /// Moves the clock; wakes the engine when its timer is due.
    pub fn advance(&mut self, ms: u64) {
        self.io.now_ms += ms;
        if self.io.timer.is_some_and(|at| at <= self.io.now_ms) {
            self.io.timer = None;
            self.io.queue.push_back(Event::Wake);
            self.run(Vec::new());
        }
    }

    /// Sends an item on the newest open stream, and runs what follows.
    pub fn stream(&mut self, item: StreamItem) -> bool {
        let Some((id, _)) = self.io.streams.last().cloned() else { return false };
        if matches!(item, StreamItem::Ended | StreamItem::Failed { .. }) {
            self.io.streams.pop();
        }
        self.io.queue.push_back(Event::Stream { id, item });
        self.run(Vec::new());
        true
    }

    /// The commands the engine reported, in order.
    pub fn commands(&self) -> Vec<crate::engine::AgentCommand> {
        self.reports.iter().filter_map(|r| match r {
            crate::engine::Report::Command(c) => Some((**c).clone()),
            _ => None,
        }).collect()
    }
}
