//! The loop: one tokio task owns the core and one queue of events; everything the core asks for is started here
//! and its result comes back through the same queue, one event at a time, in the order things happened.
//!
//! ```text
//! Handle::send (the window)  ─┐
//! results of I/O             ─┼─▶ queue ─▶ core.handle(now, event) ─▶ effects, started in order
//! the timer                  ─┘
//! ```
//!
//! **The barrier.** A `Store` with `DeviceApply` holds back every effect after it, of the same answer and of later
//! ones, until it is durable; events are handled meanwhile and their effects queue up behind it. When it failed,
//! what queued up is dropped and the core hears only the failure.
//!
//! **Stores** run on one thread of their own, in the order asked: a write to disk never stalls the loop, and a
//! `Get` after a `Put` reads what was put.
//!
//! **Results that nobody waits for** are dropped here already: of a request or stream that was cancelled, and a
//! timer that was replaced.

use crate::http::Http;
use crate::store::Stores;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use trommi_client_core::app::{
    App, Bytes, Effect, Event, IoError, IoErrorKind, IoId, IoResult, PlatformAnswer, PlatformOp,
    SecretOp, StoreAnswer, StoreOp, StreamItem,
};

/// What the loop drives: the app core, or a stand-in in a test.
pub trait Core: Send + 'static {
    fn handle(&mut self, now_ms: u64, event: Event) -> Vec<Effect>;
}

impl Core for App {
    fn handle(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        App::handle(self, now_ms, event)
    }
}

/// The part of a shell that is not I/O: the window and the desktop. Called on the loop's task, in order.
pub trait Shell: Send + 'static {
    /// `Render`, `FileReady`, `FileGone`, `Notify`, `Snapshot`, `SaveFile`, `OpenUrl`, `Log`: what only the
    /// window or the desktop can do. It must not block.
    fn show(&mut self, effect: Effect);

    /// The platform's secret store. Without one (`Config.can.secrets` false) the core does not ask.
    fn secret(&mut self, _op: SecretOp) -> IoResult<Option<Bytes>> {
        IoResult::Err(IoError::new(IoErrorKind::Denied))
    }

    /// A platform service (a passkey, push registration). Without one: refused.
    fn platform(&mut self, _op: PlatformOp) -> IoResult<PlatformAnswer> {
        IoResult::Ok(PlatformAnswer::Refused)
    }
}

/// The shell's end of the queue. Dropping every handle ends the loop: requests in flight are aborted and the
/// stores are closed (the device lock is free again).
#[derive(Clone)]
pub struct Handle {
    events: mpsc::UnboundedSender<Event>,
}

impl Handle {
    /// Queues an event (`Start` first, then the person's actions, focus, the platform's news). `false`: the loop
    /// is gone.
    pub fn send(&self, event: Event) -> bool {
        self.events.send(event).is_ok()
    }
}

/// What comes back into the loop from its own tasks.
enum Back {
    Event(Event),
    Wake(u64),
}

/// How many results may wait for the loop before the tasks that bring them wait themselves (a stream's chunks).
const BACKLOG: usize = 64;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

struct Loop<C, S> {
    core: C,
    shell: S,
    http: Http,
    back: mpsc::Sender<Back>,
    store: std::sync::mpsc::Sender<(IoId, StoreOp)>,
    /// The requests and streams in flight.
    flying: HashMap<IoId, JoinHandle<()>>,
    /// The `DeviceApply` everything waits for, and what waits.
    barrier: Option<IoId>,
    held: VecDeque<Effect>,
    timer: Option<JoinHandle<()>>,
    /// Counts the timers set: a `Wake` of an earlier one is dropped.
    timers: u64,
}

/// Starts the loop on the current tokio runtime. The caller sends `Event::Start` through the handle; the stores
/// are the slot's, opened (and so locked) before. The task that is returned ends once every handle is dropped and
/// the stores are closed.
pub fn start<C: Core, S: Shell>(
    core: C,
    shell: S,
    stores: Stores,
) -> Result<(Handle, JoinHandle<()>), String> {
    let http = Http::new()?;
    let (events_tx, mut events) = mpsc::unbounded_channel::<Event>();
    let (back_tx, mut back) = mpsc::channel::<Back>(BACKLOG);
    let (store_tx, store_rx) = std::sync::mpsc::channel::<(IoId, StoreOp)>();
    let answers = back_tx.clone();
    let (closed_tx, closed) = tokio::sync::oneshot::channel::<()>();
    std::thread::Builder::new()
        .name("trommi-store".into())
        .spawn(move || {
            let mut stores = stores;
            while let Ok((id, op)) = store_rx.recv() {
                let result = stores.run(op);
                if answers
                    .blocking_send(Back::Event(Event::Store { id, result }))
                    .is_err()
                {
                    break;
                }
            }
            // the lock goes with the stores; only then is the loop's task over
            drop(stores);
            let _ = closed_tx.send(());
        })
        .map_err(|_| "no thread for the stores".to_string())?;
    let mut l = Loop {
        core,
        shell,
        http,
        back: back_tx,
        store: store_tx,
        flying: HashMap::new(),
        barrier: None,
        held: VecDeque::new(),
        timer: None,
        timers: 0,
    };
    let task = tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                biased;
                b = back.recv() => match b {
                    Some(Back::Event(event)) => match l.arrived(event) { Some(event) => event, None => continue },
                    Some(Back::Wake(n)) if n == l.timers => Event::Wake,
                    Some(Back::Wake(_)) => continue,
                    None => break,
                },
                e = events.recv() => match e { Some(event) => event, None => break },
            };
            let effects = l.core.handle(now_ms(), event);
            l.run(effects);
        }
        for (_, task) in l.flying.drain() {
            task.abort();
        }
        if let Some(timer) = l.timer.take() {
            timer.abort();
        }
        drop(l);
        drop(back);
        let _ = closed.await;
    });
    Ok((Handle { events: events_tx }, task))
}

impl<C: Core, S: Shell> Loop<C, S> {
    /// A result came back: whether the core hears of it. Ends the barrier it was.
    fn arrived(&mut self, event: Event) -> Option<Event> {
        match &event {
            Event::Http { id, .. } => {
                self.flying.remove(id)?;
            }
            Event::Stream { id, item } => {
                if !self.flying.contains_key(id) {
                    return None;
                }
                if matches!(item, StreamItem::Ended | StreamItem::Failed { .. }) {
                    self.flying.remove(id);
                }
            }
            Event::Store { id, result } if self.barrier == Some(*id) => {
                self.barrier = None;
                let held = std::mem::take(&mut self.held);
                if matches!(result, IoResult::Ok(_)) {
                    self.run(held.into());
                }
                // (failed: what waited behind it is dropped; the core hears only the failure)
            }
            _ => {}
        }
        Some(event)
    }

    /// Starts effects in the order given; behind a barrier they wait.
    fn run(&mut self, effects: Vec<Effect>) {
        let mut effects = VecDeque::from(effects);
        while let Some(effect) = effects.pop_front() {
            if self.barrier.is_some() {
                self.held.push_back(effect);
                continue;
            }
            match effect {
                Effect::Http { id, request } => {
                    let (http, back) = (self.http.clone(), self.back.clone());
                    let task = tokio::spawn(async move {
                        let result = http.fetch(&request).await.into();
                        let _ = back.send(Back::Event(Event::Http { id, result })).await;
                    });
                    self.flying.insert(id, task);
                }
                Effect::OpenStream { id, request } => {
                    let (http, back) = (self.http.clone(), self.back.clone());
                    let task = tokio::spawn(async move {
                        http.stream(&request, |item| {
                            let back = back.clone();
                            async move {
                                back.send(Back::Event(Event::Stream { id, item }))
                                    .await
                                    .is_ok()
                            }
                        })
                        .await;
                    });
                    self.flying.insert(id, task);
                }
                Effect::Cancel { id } => {
                    if let Some(task) = self.flying.remove(&id) {
                        task.abort();
                    }
                }
                Effect::Store { id, op } => {
                    if matches!(op, StoreOp::DeviceApply { .. }) {
                        self.barrier = Some(id);
                    }
                    if self.store.send((id, op)).is_err() {
                        // the stores' thread is gone: the core hears it as a failure, in the loop's order
                        self.barrier = self.barrier.filter(|b| *b != id);
                        let result: IoResult<StoreAnswer> =
                            IoResult::Err(IoError::new(IoErrorKind::Failed));
                        let back = self.back.clone();
                        tokio::spawn(async move {
                            let _ = back.send(Back::Event(Event::Store { id, result })).await;
                        });
                    }
                }
                Effect::Secret { id, op } => {
                    let result = self.shell.secret(op);
                    self.answer(Event::Secret { id, result });
                }
                Effect::Platform { id, op } => {
                    let result = self.shell.platform(op);
                    self.answer(Event::Platform { id, result });
                }
                Effect::Timer { at_ms } => {
                    if let Some(timer) = self.timer.take() {
                        timer.abort();
                    }
                    self.timers += 1;
                    if let Some(at) = at_ms {
                        let (n, back) = (self.timers, self.back.clone());
                        self.timer = Some(tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(at.saturating_sub(now_ms())))
                                .await;
                            let _ = back.send(Back::Wake(n)).await;
                        }));
                    }
                }
                effect @ (Effect::Render { .. }
                | Effect::FileReady { .. }
                | Effect::FileGone { .. }
                | Effect::Notify { .. }
                | Effect::Snapshot { .. }
                | Effect::SaveFile { .. }
                | Effect::OpenUrl { .. }
                | Effect::Log { .. }) => self.shell.show(effect),
            }
        }
    }

    /// An answer the shell gave at once: through the queue, like every other result.
    fn answer(&mut self, event: Event) {
        let back = self.back.clone();
        tokio::spawn(async move {
            let _ = back.send(Back::Event(event)).await;
        });
    }
}
