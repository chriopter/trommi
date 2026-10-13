//! A fake I/O executor: runs an App's effects in memory, with a clock the test moves. The smallest shell there is,
//! and the seed of W6's simulated network (drops, 5xx, reordering, cuts, kill and restart from the store).
//!
//! - **Stores** are maps, kept across a restart of the App (`restart` makes a new App over the same stores).
//! - **HTTP and streams** go to a function the test gives (`hub`): the stand-in hub. Without one every request
//!   is `offline`.
//! - **Timers**: one, as the contract says; `advance` moves the clock and fires it when due.
//! - **Renders** are folded into `view`, exactly as a shell does.

use super::{
    App, DeviceEntry, Effect, Entry, Event, HttpRequest, HttpResponse, IoError, IoErrorKind, IoId,
    IoResult, StoreAnswer, StoreOp,
};
use crate::views::ViewModel;
use std::collections::{BTreeMap, VecDeque};

pub type Hub = Box<dyn FnMut(u64, &HttpRequest) -> Result<HttpResponse, IoError>>;

pub struct Sim {
    pub app: App,
    pub now_ms: u64,
    pub view: ViewModel,
    pub cache: BTreeMap<String, Vec<u8>>,
    pub device: (u64, BTreeMap<Vec<u8>, Vec<u8>>),
    pub timer: Option<u64>,
    pub hub: Option<Hub>,
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
            log: Vec::new(),
            queue: VecDeque::new(),
        }
    }

    /// Sends one event and runs everything that follows from it until nothing is left to do now.
    pub fn send(&mut self, event: Event) {
        self.queue.push_back(event);
        while let Some(event) = self.queue.pop_front() {
            let effects = self.app.handle(self.now_ms, event);
            self.execute(effects);
        }
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
    }

    /// Runs effects as a shell does: in order, a failed device write ending the rest. Results are queued.
    pub fn execute(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            self.log.push(effect.clone());
            match effect {
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
                    let result = match self.hub.as_mut() {
                        Some(hub) => hub(self.now_ms, &request).into(),
                        None => IoResult::Err(IoError::new(IoErrorKind::Offline)),
                    };
                    self.queue.push_back(Event::Http { id, result });
                }
                Effect::OpenStream { id, .. } => self.queue.push_back(Event::Stream {
                    id,
                    item: super::StreamItem::Failed {
                        error: IoError::new(IoErrorKind::Offline),
                    },
                }),
                Effect::Secret { id, .. } => self.queue.push_back(Event::Secret {
                    id,
                    result: IoResult::Err(IoError::new(IoErrorKind::Denied)),
                }),
                Effect::Timer { at_ms } => self.timer = at_ms,
                Effect::Cancel { .. }
                | Effect::Notify { .. }
                | Effect::Snapshot { .. }
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
            StoreOp::Put { put, delete } => {
                for k in delete {
                    self.cache.remove(&k);
                }
                for e in put {
                    self.cache.insert(e.key, e.value.0);
                }
                IoResult::Ok(StoreAnswer::Done)
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
                | Effect::Secret { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    }
}
