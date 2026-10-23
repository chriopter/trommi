//! The loop with a stand-in core that asks for exactly what a test wants, and with the App itself.

mod common;

use common::{closed, request, server, write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use trommi_client_core::app::{
    App, Config, DeviceEntry, Effect, Entry, Event, IoErrorKind, IoId, IoResult, StoreAnswer,
    StoreOp, StreamItem,
};
use trommi_client_core::views::{Phase, ViewModel};
use trommi_io::{start, Core, OpenError, Shell, Stores};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trommi-io-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn stores(dir: &Path) -> Stores {
    Stores::open(&dir.join("device"), &dir.join("cache")).unwrap()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// A core that answers each event as the test says and tells the test every event it got.
struct Scripted<F> {
    answer: F,
    heard: mpsc::UnboundedSender<Event>,
}

impl<F: FnMut(&Event) -> Vec<Effect> + Send + 'static> Core for Scripted<F> {
    fn handle(&mut self, _now_ms: u64, event: Event) -> Vec<Effect> {
        let effects = (self.answer)(&event);
        let _ = self.heard.send(event);
        effects
    }
}

struct Quiet;
impl Shell for Quiet {
    fn show(&mut self, _effect: Effect) {}
}

fn scripted<F: FnMut(&Event) -> Vec<Effect> + Send + 'static>(
    answer: F,
) -> (Scripted<F>, mpsc::UnboundedReceiver<Event>) {
    let (heard, events) = mpsc::unbounded_channel();
    (Scripted { answer, heard }, events)
}

async fn next(events: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .expect("an event in time")
        .expect("the loop runs")
}

async fn nothing(events: &mut mpsc::UnboundedReceiver<Event>, ms: u64) {
    if let Ok(event) = tokio::time::timeout(Duration::from_millis(ms), events.recv()).await {
        panic!("nothing was expected, got {event:?}");
    }
}

fn start_event() -> Event {
    Event::Start {
        config: Config {
            platform: "test".into(),
            version: "0".into(),
            seed: 1,
            ..Default::default()
        },
    }
}

fn device_apply(id: u64, expected_revision: u64) -> Effect {
    Effect::Store {
        id: IoId(id),
        op: StoreOp::DeviceApply {
            expected_revision,
            put: vec![DeviceEntry {
                key: b"\x01k".as_slice().into(),
                value: b"v".as_slice().into(),
            }],
            delete: vec![],
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_leaves_before_the_device_state_is_durable() {
    let dir = folder("barrier");
    let log = dir.join("device/state.log");
    // when the request arrives, the hub looks at the journal: the record is there already
    let durable = Arc::new(AtomicBool::new(false));
    let (d, l) = (durable.clone(), log.clone());
    let hub = server(move |mut stream| {
        let (d, l) = (d.clone(), l.clone());
        async move {
            d.store(
                std::fs::metadata(&l).is_ok_and(|m| m.len() > 0),
                Ordering::SeqCst,
            );
            write(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
            )
            .await;
        }
    })
    .await;
    let url = hub.url.clone();
    let (core, mut events) = scripted(move |event| match event {
        Event::Start { .. } => vec![
            device_apply(1, 0),
            Effect::Http {
                id: IoId(2),
                request: request("POST", &url),
            },
        ],
        _ => vec![],
    });
    let (handle, task) = start(core, Quiet, stores(&dir)).unwrap();
    handle.send(start_event());
    assert!(matches!(next(&mut events).await, Event::Start { .. }));
    assert!(matches!(
        next(&mut events).await,
        Event::Store {
            id: IoId(1),
            result: IoResult::Ok(StoreAnswer::Done)
        }
    ));
    assert!(
        matches!(next(&mut events).await, Event::Http { id: IoId(2), result: IoResult::Ok(r) } if r.status == 200)
    );
    assert!(
        durable.load(Ordering::SeqCst),
        "the request left before the record was written"
    );
    drop(handle);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_device_write_drops_what_waited_behind_it() {
    let dir = folder("barrier-failed");
    let hub = server(|mut stream| async move {
        write(&mut stream, b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await;
    })
    .await;
    let url = hub.url.clone();
    let shown = Arc::new(Mutex::new(0usize));
    struct Counting(Arc<Mutex<usize>>);
    impl Shell for Counting {
        fn show(&mut self, _effect: Effect) {
            *self.0.lock().unwrap() += 1;
        }
    }
    let (core, mut events) = scripted(move |event| match event {
        // the stored revision is 0: this write meets a conflict
        Event::Start { .. } => vec![
            device_apply(1, 7),
            Effect::Http {
                id: IoId(2),
                request: request("POST", &url),
            },
            Effect::OpenStream {
                id: IoId(3),
                request: request("GET", &url),
            },
            Effect::Log {
                level: trommi_client_core::app::LogLevel::Info,
                text: "after".into(),
            },
        ],
        // an event handled while the write runs: its effects wait behind the barrier too
        Event::Foreground => vec![Effect::Http {
            id: IoId(4),
            request: request("POST", &url),
        }],
        _ => vec![],
    });
    let (handle, task) = start(core, Counting(shown.clone()), stores(&dir)).unwrap();
    handle.send(start_event());
    handle.send(Event::Foreground);
    let mut heard = vec![
        next(&mut events).await,
        next(&mut events).await,
        next(&mut events).await,
    ];
    // The loop picks between a new event and a result at random (tokio::select!): the write may fail before
    // Foreground is handled, and then its request is no longer behind the barrier and leaves (the order the core
    // heard them in says which it was; asserting only the first case failed now and then on CI).
    let waited = heard.iter().position(|e| matches!(e, Event::Foreground))
        < heard.iter().position(|e| matches!(e, Event::Store { .. }));
    heard.retain(|e| !matches!(e, Event::Start { .. } | Event::Foreground));
    assert!(
        matches!(&heard[..], [Event::Store { id: IoId(1), result: IoResult::Err(e) }] if e.kind == IoErrorKind::Conflict),
        "{heard:?}"
    );
    if !waited {
        let after = next(&mut events).await;
        assert!(
            matches!(
                after,
                Event::Http {
                    id: IoId(4),
                    result: IoResult::Ok(_)
                }
            ),
            "{after:?}"
        );
    }
    nothing(&mut events, 300).await;
    assert_eq!(
        hub.count(),
        usize::from(!waited),
        "a request of the failed write's answer, or one that waited behind it, left"
    );
    assert_eq!(*shown.lock().unwrap(), 0);
    drop(handle);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn store_ops_run_in_the_order_asked() {
    let dir = folder("order");
    let (core, mut events) = scripted(|event| match event {
        Event::Start { .. } => vec![
            Effect::Store {
                id: IoId(1),
                op: StoreOp::Put {
                    put: vec![Entry {
                        key: "c1/r/desk".into(),
                        value: b"1".as_slice().into(),
                    }],
                    delete: vec![],
                },
            },
            Effect::Store {
                id: IoId(2),
                op: StoreOp::Get {
                    keys: vec!["c1/r/desk".into()],
                },
            },
            Effect::Store {
                id: IoId(3),
                op: StoreOp::Put {
                    put: vec![],
                    delete: vec!["c1/r/desk".into()],
                },
            },
            Effect::Store {
                id: IoId(4),
                op: StoreOp::Scan {
                    prefix: "c1/".into(),
                },
            },
        ],
        _ => vec![],
    });
    let (handle, task) = start(core, Quiet, stores(&dir)).unwrap();
    handle.send(start_event());
    next(&mut events).await;
    assert!(matches!(
        next(&mut events).await,
        Event::Store {
            id: IoId(1),
            result: IoResult::Ok(StoreAnswer::Done)
        }
    ));
    assert!(
        matches!(next(&mut events).await, Event::Store { id: IoId(2), result: IoResult::Ok(StoreAnswer::Values { values }) } if values[0].as_deref() == Some(b"1".as_slice()))
    );
    assert!(matches!(
        next(&mut events).await,
        Event::Store { id: IoId(3), .. }
    ));
    assert!(
        matches!(next(&mut events).await, Event::Store { id: IoId(4), result: IoResult::Ok(StoreAnswer::Entries { entries }) } if entries.is_empty())
    );
    drop(handle);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_stream_closes_and_brings_nothing_more() {
    let dir = folder("cancel");
    let hung_up = Arc::new(AtomicBool::new(false));
    let h = hung_up.clone();
    let hub = server(move |mut stream| {
        let h = h.clone();
        async move {
            write(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nx\r\n",
            )
            .await;
            h.store(closed(&mut stream).await, Ordering::SeqCst);
        }
    })
    .await;
    let url = hub.url.clone();
    let (core, mut events) = scripted(move |event| match event {
        Event::Start { .. } => vec![Effect::OpenStream {
            id: IoId(1),
            request: request("GET", &url),
        }],
        Event::Stream {
            item: StreamItem::Chunk { .. },
            ..
        } => vec![
            Effect::Cancel { id: IoId(1) },
            Effect::Cancel { id: IoId(99) },
        ],
        _ => vec![],
    });
    let (handle, task) = start(core, Quiet, stores(&dir)).unwrap();
    handle.send(start_event());
    next(&mut events).await;
    assert!(matches!(
        next(&mut events).await,
        Event::Stream {
            id: IoId(1),
            item: StreamItem::Opened { status: 200, .. }
        }
    ));
    assert!(matches!(
        next(&mut events).await,
        Event::Stream {
            id: IoId(1),
            item: StreamItem::Chunk { .. }
        }
    ));
    // no `failed`, no `ended`: the core forgot the id before it said so
    nothing(&mut events, 500).await;
    for _ in 0..60 {
        if hung_up.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        hung_up.load(Ordering::SeqCst),
        "the stream's connection stayed open"
    );
    drop(handle);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn one_timer_replacing_the_last() {
    let dir = folder("timer");
    let (core, mut events) = scripted(|event| match event {
        Event::Start { .. } => vec![
            Effect::Timer {
                at_ms: Some(now() + 150),
            },
            Effect::Timer {
                at_ms: Some(now() + 60_000),
            },
            Effect::Timer {
                at_ms: Some(now() + 120),
            },
        ],
        // the second wake is asked for and taken away again
        Event::Wake => vec![
            Effect::Timer {
                at_ms: Some(now() + 50),
            },
            Effect::Timer { at_ms: None },
        ],
        // a time that passed already is due at once
        Event::Foreground => vec![Effect::Timer { at_ms: Some(1) }],
        _ => vec![],
    });
    let (handle, task) = start(core, Quiet, stores(&dir)).unwrap();
    let began = std::time::Instant::now();
    handle.send(start_event());
    next(&mut events).await;
    assert!(matches!(next(&mut events).await, Event::Wake));
    assert!(
        began.elapsed() >= Duration::from_millis(119),
        "the timer was early: {:?}",
        began.elapsed()
    );
    nothing(&mut events, 400).await;
    handle.send(Event::Foreground);
    assert!(matches!(next(&mut events).await, Event::Foreground));
    assert!(matches!(next(&mut events).await, Event::Wake));
    drop(handle);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// The window of a test: folds every render, as a shell does.
struct Window(Arc<Mutex<ViewModel>>);
impl Shell for Window {
    fn show(&mut self, effect: Effect) {
        if let Effect::Render { delta, .. } = effect {
            self.0
                .lock()
                .unwrap()
                .apply(&delta)
                .expect("renders come in order");
        }
    }
}

async fn phase(view: &Arc<Mutex<ViewModel>>, want: Phase) {
    for _ in 0..100 {
        if view.lock().unwrap().phase == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "the phase is {:?}, not {want:?}",
        view.lock().unwrap().phase
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_app_starts_reads_its_device_and_starts_again_after_a_kill() {
    let dir = folder("app");
    let view = Arc::new(Mutex::new(ViewModel::default()));
    let (handle, task) = start(App::new(), Window(view.clone()), stores(&dir)).unwrap();
    handle.send(start_event());
    // an empty device: the sign-in
    phase(&view, Phase::SignedOut).await;
    // the slot has one owner while the loop runs
    assert_eq!(
        Stores::open(&dir.join("device"), &dir.join("cache")).err(),
        Some(OpenError::Locked)
    );
    // the process ends: every handle goes, the loop's task ends, and the slot is free at that moment
    drop(handle);
    task.await.unwrap();
    let again = stores(&dir);
    let view = Arc::new(Mutex::new(ViewModel::default()));
    let (handle, task) = start(App::new(), Window(view.clone()), again).unwrap();
    handle.send(start_event());
    phase(&view, Phase::SignedOut).await;
    drop(handle);
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
