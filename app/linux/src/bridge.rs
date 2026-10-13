//! The one object the QML window holds: state as properties (the views as JSON text), actions as invokables. The
//! engine runs on a tokio runtime of its own; what it learns comes back to Qt's thread through cxx-qt's queue.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, status)]
        #[qproperty(bool, signed_in)]
        #[qproperty(bool, starting)]
        #[qproperty(bool, busy)]
        #[qproperty(bool, live)]
        #[qproperty(QString, title)]
        #[qproperty(QString, desk_json)]
        #[qproperty(QString, sessions_json)]
        #[qproperty(QString, chat_json)]
        #[qproperty(QString, selected)]
        #[qproperty(QString, hub_url)]
        #[qproperty(QString, error)]
        #[qproperty(QString, shot_path)]
        #[qproperty(i32, shot_after_ms)]
        #[qproperty(QString, scene)]
        #[qproperty(QString, scene_session)]
        #[qproperty(QString, theme_mode)]
        #[qproperty(QString, greeting)]
        type Trommi = super::TrommiRust;

        /// Opens the room this machine holds, if any, from its cache, and starts catching up and listening.
        #[qinvokable]
        fn start(self: Pin<&mut Trommi>);
        /// Signs in with the account's e-mail and password.
        #[qinvokable]
        fn sign_in_password(self: Pin<&mut Trommi>, hub: &QString, email: &QString, password: &QString);
        /// Signs in with the Emergency Kit: the account (e-mail or id) and its words.
        #[qinvokable]
        fn sign_in_kit(self: Pin<&mut Trommi>, hub: &QString, account: &QString, words: &QString);
        /// Answers a card with one option, or a permission request (`allow` / `deny`).
        #[qinvokable]
        fn answer(self: Pin<&mut Trommi>, id: &QString, key: &QString);
        /// Answers a card that allows several options: the keys as a JSON list.
        #[qinvokable]
        fn answer_many(self: Pin<&mut Trommi>, id: &QString, keys_json: &QString);
        /// Marks an info card read.
        #[qinvokable]
        fn mark_read(self: Pin<&mut Trommi>, id: &QString);
        /// Shows a session's Chat; empty: the Desk.
        #[qinvokable]
        fn select_session(self: Pin<&mut Trommi>, id: &QString);
        /// Writes to the selected session's agent.
        #[qinvokable]
        fn send(self: Pin<&mut Trommi>, text: &QString);
        /// Light or dark, kept for the next start (the web's rule: light unless the person picks dark).
        #[qinvokable]
        fn set_theme(self: Pin<&mut Trommi>, mode: &QString);
        /// Writes a picture of the window to `shot_path` was done (QML): ends a scene run.
        #[qinvokable]
        fn shot_taken(self: Pin<&mut Trommi>);
    }

    impl cxx_qt::Threading for Trommi {}
}

use crate::engine::{signin, Engine, Sink, Update};
use core::pin::Pin;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::sync::{Arc, Mutex, OnceLock};

pub struct TrommiRust {
    status: QString,
    signed_in: bool,
    starting: bool,
    busy: bool,
    live: bool,
    title: QString,
    desk_json: QString,
    sessions_json: QString,
    chat_json: QString,
    selected: QString,
    hub_url: QString,
    error: QString,
    shot_path: QString,
    shot_after_ms: i32,
    scene: QString,
    scene_session: QString,
    theme_mode: QString,
    greeting: QString,
}

impl Default for TrommiRust {
    fn default() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let hub = std::env::var("TROMMI_HUB").unwrap_or_else(|_| "https://hub.trommi.com".into());
        Self {
            status: QString::from("Starting …"),
            signed_in: false,
            starting: true,
            busy: false,
            live: false,
            title: QString::default(),
            desk_json: QString::from("[]"),
            sessions_json: QString::from("[]"),
            chat_json: QString::from("[]"),
            selected: QString::default(),
            hub_url: QString::from(hub.as_str()),
            error: QString::default(),
            shot_path: QString::from(env("TROMMI_LINUX_SHOT").as_str()),
            shot_after_ms: env("TROMMI_LINUX_SHOT_AFTER_MS").parse().unwrap_or(1500),
            scene: QString::from(env("TROMMI_LINUX_SCENE").as_str()),
            scene_session: QString::from(env("TROMMI_LINUX_SESSION").as_str()),
            theme_mode: QString::from(
                Some(env("TROMMI_LINUX_THEME"))
                    .filter(|t| !t.is_empty())
                    .or_else(|| std::fs::read_to_string(crate::store::base_dir().join("theme")).ok())
                    .unwrap_or_default()
                    .trim(),
            ),
            greeting: QString::from(env("TROMMI_LINUX_GREETING").as_str()),
        }
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(3)
            .enable_all()
            .build()
            .expect("a runtime")
    })
}

fn engine_slot() -> &'static Mutex<Option<Arc<Engine>>> {
    static SLOT: OnceLock<Mutex<Option<Arc<Engine>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

fn engine() -> Option<Arc<Engine>> {
    engine_slot().lock().unwrap_or_else(|e| e.into_inner()).clone()
}

type Thread = cxx_qt::CxxQtThread<qobject::Trommi>;

/// The engine's updates, onto Qt's thread.
fn sink_for(thread: Thread) -> Sink {
    Arc::new(move |u: Update| {
        let _ = thread.queue(move |mut o: Pin<&mut qobject::Trommi>| {
            if let Some(s) = u.status {
                o.as_mut().set_status(QString::from(s.as_str()));
            }
            if let Some(d) = u.desk {
                o.as_mut().set_desk_json(QString::from(d.as_str()));
            }
            if let Some(s) = u.sessions {
                o.as_mut().set_sessions_json(QString::from(s.as_str()));
            }
            if let Some(c) = u.chat {
                o.as_mut().set_chat_json(QString::from(c.as_str()));
            }
            if let Some(l) = u.live {
                o.as_mut().set_live(l);
            }
            if let Some(t) = u.title {
                o.as_mut().set_title(QString::from(t.as_str()));
            }
            if let Some(s) = u.signed_in {
                o.as_mut().set_signed_in(s);
                o.as_mut().set_starting(false);
            }
        });
    })
}

fn tell_error(thread: &Thread, text: String) {
    let _ = thread.queue(move |mut o: Pin<&mut qobject::Trommi>| {
        o.as_mut().set_error(QString::from(text.as_str()));
        o.as_mut().set_busy(false);
    });
}

/// The engine is in: kept, followed live.
fn run(engine: Arc<Engine>) {
    *engine_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(engine.clone());
    runtime().spawn(engine.run_live());
}

impl qobject::Trommi {
    fn start(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        let sink = sink_for(thread.clone());
        runtime().spawn(async move {
            match Engine::open_newest(sink) {
                Ok(Some(engine)) => run(engine),
                Ok(None) => {
                    let _ = thread.queue(|mut o: Pin<&mut qobject::Trommi>| {
                        o.as_mut().set_starting(false);
                        o.as_mut().set_status(QString::from("Not signed in"));
                    });
                    // (a scene signs in on its own: TROMMI_LINUX_EMAIL and TROMMI_LINUX_PASSWORD, at TROMMI_HUB)
                    // (or with the Emergency Kit: TROMMI_LINUX_KIT_ACCOUNT and TROMMI_LINUX_KIT_WORDS)
                    if let (Ok(account), Ok(words)) = (std::env::var("TROMMI_LINUX_KIT_ACCOUNT"), std::env::var("TROMMI_LINUX_KIT_WORDS")) {
                        let hub = std::env::var("TROMMI_HUB").unwrap_or_default();
                        match signin::with_kit(&hub, &account, &words, sink_for(thread.clone())).await {
                            Ok(engine) => run(engine),
                            Err(f) => tell_error(&thread, f.text()),
                        }
                    } else if let (Ok(email), Ok(password)) = (std::env::var("TROMMI_LINUX_EMAIL"), std::env::var("TROMMI_LINUX_PASSWORD")) {
                        let hub = std::env::var("TROMMI_HUB").unwrap_or_default();
                        let sink = sink_for(thread.clone());
                        match signin::with_password(&hub, &email, &password, sink).await {
                            Ok(engine) => run(engine),
                            Err(f) => tell_error(&thread, f.text()),
                        }
                    }
                }
                Err(f) => {
                    let text = f.text();
                    let _ = thread.queue(move |mut o: Pin<&mut qobject::Trommi>| {
                        o.as_mut().set_starting(false);
                        o.as_mut().set_status(QString::from(format!("Could not open: {text}").as_str()));
                    });
                }
            }
        });
    }

    fn sign_in_password(mut self: Pin<&mut Self>, hub: &QString, email: &QString, password: &QString) {
        self.as_mut().set_busy(true);
        self.as_mut().set_error(QString::default());
        let thread = self.qt_thread();
        let (hub, email, password) = (hub.to_string(), email.to_string(), password.to_string());
        runtime().spawn(async move {
            match signin::with_password(&hub, &email, &password, sink_for(thread.clone())).await {
                Ok(engine) => run(engine),
                Err(f) => tell_error(&thread, f.text()),
            }
        });
    }

    fn sign_in_kit(mut self: Pin<&mut Self>, hub: &QString, account: &QString, words: &QString) {
        self.as_mut().set_busy(true);
        self.as_mut().set_error(QString::default());
        let thread = self.qt_thread();
        let (hub, account, words) = (hub.to_string(), account.to_string(), words.to_string());
        runtime().spawn(async move {
            match signin::with_kit(&hub, &account, &words, sink_for(thread.clone())).await {
                Ok(engine) => run(engine),
                Err(f) => tell_error(&thread, f.text()),
            }
        });
    }

    fn answer(self: Pin<&mut Self>, id: &QString, key: &QString) {
        let thread = self.qt_thread();
        let (id, key) = (id.to_string(), key.to_string());
        let Some(engine) = engine() else { return };
        runtime().spawn(async move {
            let is_request = engine
                .mirror
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .permission(&id)
                .is_some();
            let done = if is_request {
                engine.verdict(&id, key == "allow").await
            } else {
                engine.answer(&id, vec![key]).await
            };
            if let Err(f) = done {
                tell_error(&thread, format!("Not sent: {}", f.text()));
            }
        });
    }

    fn answer_many(self: Pin<&mut Self>, id: &QString, keys_json: &QString) {
        let thread = self.qt_thread();
        let id = id.to_string();
        let keys: Vec<String> = serde_json::from_str(&keys_json.to_string()).unwrap_or_default();
        let Some(engine) = engine() else { return };
        runtime().spawn(async move {
            if let Err(f) = engine.answer(&id, keys).await {
                tell_error(&thread, format!("Not sent: {}", f.text()));
            }
        });
    }

    fn mark_read(self: Pin<&mut Self>, id: &QString) {
        let thread = self.qt_thread();
        let id = id.to_string();
        let Some(engine) = engine() else { return };
        runtime().spawn(async move {
            if let Err(f) = engine.answer(&id, Vec::new()).await {
                tell_error(&thread, format!("Not sent: {}", f.text()));
            }
        });
    }

    fn select_session(mut self: Pin<&mut Self>, id: &QString) {
        self.as_mut().set_selected(id.clone());
        let id = id.to_string();
        if id.is_empty() {
            self.as_mut().set_chat_json(QString::from("[]"));
        }
        let Some(engine) = engine() else { return };
        runtime().spawn(async move {
            engine.select((!id.is_empty()).then_some(id));
        });
    }

    fn send(self: Pin<&mut Self>, text: &QString) {
        let thread = self.qt_thread();
        let session = self.rust().selected.to_string();
        let text = text.to_string();
        let Some(engine) = engine() else { return };
        runtime().spawn(async move {
            if let Err(f) = engine.say_to(&session, &text).await {
                tell_error(&thread, format!("Not sent: {}", f.text()));
            }
        });
    }

    fn set_theme(mut self: Pin<&mut Self>, mode: &QString) {
        let mode = if mode.to_string() == "dark" { "dark" } else { "light" };
        let _ = crate::store::private_dir(&crate::store::base_dir());
        let _ = std::fs::write(crate::store::base_dir().join("theme"), mode);
        self.as_mut().set_theme_mode(QString::from(mode));
    }

    fn shot_taken(self: Pin<&mut Self>) {
        if let Some(engine) = engine() {
            engine.close();
        }
    }
}
