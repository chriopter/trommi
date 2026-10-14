//! The one object the QML window holds: the app core's view as JSON text (`view_json`), the files the core handed
//! over for drawing (`files_json`), and the window's own settings; the person's events go in through `send_event`.
//! The app core runs in its loop (shell.rs, trommi-io); its renders come back to Qt's thread through cxx-qt's queue.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, view_json)]
        #[qproperty(QString, files_json)]
        #[qproperty(QString, error)]
        #[qproperty(QString, theme_mode)]
        #[qproperty(QString, open_request)]
        #[qproperty(QString, shot_path)]
        #[qproperty(i32, shot_after_ms)]
        #[qproperty(QString, scene)]
        type Trommi = super::TrommiRust;

        /// Opens the slot and starts the app core's loop.
        #[qinvokable]
        fn start(self: Pin<&mut Trommi>);
        /// The person's event for the app core, as JSON (`shell::person_event`).
        #[qinvokable]
        fn send_event(self: Pin<&mut Trommi>, json: &QString);
        /// A file the person picked: read here, sent as `Action::Attach` under the window's id for it.
        #[qinvokable]
        fn attach_file(self: Pin<&mut Trommi>, url: &QString, local_id: &QString);
        /// Light or dark, kept for the next start (the web's rule: light unless the person picks dark).
        #[qinvokable]
        fn set_theme(self: Pin<&mut Trommi>, mode: &QString);
        /// A picture of the window was written to `shot_path` (QML): ends a scene run.
        #[qinvokable]
        fn shot_taken(self: Pin<&mut Trommi>);
    }

    impl cxx_qt::Threading for Trommi {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use std::sync::{Arc, Mutex, OnceLock};

type Thread = cxx_qt::CxxQtThread<qobject::Trommi>;

pub struct TrommiRust {
    view_json: QString,
    files_json: QString,
    error: QString,
    theme_mode: QString,
    open_request: QString,
    shot_path: QString,
    shot_after_ms: i32,
    scene: QString,
}

impl Default for TrommiRust {
    fn default() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        Self {
            view_json: QString::from("{}"),
            files_json: QString::from("{}"),
            error: QString::default(),
            theme_mode: QString::from(
                Some(env("TROMMI_LINUX_THEME"))
                    .filter(|t| !t.is_empty())
                    .or_else(|| std::fs::read_to_string(crate::shell::base_dir().join("theme")).ok())
                    .unwrap_or_default()
                    .trim(),
            ),
            open_request: QString::default(),
            shot_path: QString::from(env("TROMMI_LINUX_SHOT").as_str()),
            shot_after_ms: env("TROMMI_LINUX_SHOT_AFTER_MS").parse().unwrap_or(1500),
            scene: QString::from(env("TROMMI_LINUX_SCENE").as_str()),
        }
    }
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("a tokio runtime")
    })
}

/// The app core's loop, while it runs.
fn core_slot() -> &'static Mutex<Option<trommi_io::Handle>> {
    static CORE: OnceLock<Mutex<Option<trommi_io::Handle>>> = OnceLock::new();
    CORE.get_or_init(Mutex::default)
}

fn tell_error(thread: &Thread, text: String) {
    let _ = thread.queue(move |mut o: Pin<&mut qobject::Trommi>| o.as_mut().set_error(QString::from(text.as_str())));
}

/// The desktop's services of the window: the files' map and a saved file's place to the window, the notifications
/// to the session bus (a click opens what it names).
fn desktop_for(thread: Thread) -> crate::platform::Desktop {
    let mut desktop = crate::platform::Desktop::new();
    let on = thread.clone();
    desktop.on_files = Box::new(move |json| {
        let _ = on.queue(move |mut o: Pin<&mut qobject::Trommi>| o.as_mut().set_files_json(QString::from(json.as_str())));
    });
    let on = thread.clone();
    desktop.on_saved = Box::new(move |path| {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        tell_error(&on, format!("Saved to Downloads: {name}"));
    });
    desktop.on_notice = Box::new(move |notice| {
        if std::env::var("TROMMI_LINUX_NOTIFY").is_ok_and(|v| v == "0") {
            return;
        }
        let note = crate::notify::Note {
            card: notice.target.clone().unwrap_or_default(),
            summary: notice.title.en.clone(),
            body: notice.body.en.clone(),
            urgent: notice.urgent,
        };
        let thread = thread.clone();
        runtime().spawn(async move {
            static NOTIFIER: tokio::sync::OnceCell<Option<crate::notify::Notifier>> = tokio::sync::OnceCell::const_new();
            let notifier = NOTIFIER
                .get_or_init(|| async move {
                    let open: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |target: String| {
                        let _ = thread.queue(move |mut o: Pin<&mut qobject::Trommi>| {
                            // (set twice, so that the same card asked for again still changes the property)
                            o.as_mut().set_open_request(QString::default());
                            o.as_mut().set_open_request(QString::from(if target.is_empty() { "desk" } else { target.as_str() }));
                        });
                    });
                    crate::notify::Notifier::connect(open).await.ok()
                })
                .await;
            if let Some(n) = notifier {
                let _ = n.show(&note).await;
            }
        });
    });
    desktop
}

impl qobject::Trommi {
    fn start(self: Pin<&mut Self>) {
        let thread = self.qt_thread();
        let offset = cxx_qt_lib::QDateTime::current_date_time().offset_from_utc() / 60;
        let draw_on = thread.clone();
        let started = crate::shell::start_with(
            runtime(),
            crate::shell::config(offset),
            move |json| {
                let _ = draw_on.queue(move |mut o: Pin<&mut qobject::Trommi>| {
                    o.as_mut().set_view_json(QString::from(json.as_str()));
                });
            },
            desktop_for(thread.clone()),
        );
        match started {
            Ok(handle) => *core_slot().lock().unwrap_or_else(|e| e.into_inner()) = Some(handle),
            Err(why) => tell_error(&thread, format!("Could not open: {why}")),
        }
    }

    fn send_event(self: Pin<&mut Self>, json: &QString) {
        match crate::shell::person_event(&json.to_string()) {
            Ok(event) => {
                if let Some(handle) = core_slot().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    handle.send(event);
                }
            }
            Err(why) => eprintln!("[trommi] the window sent {why}"),
        }
    }

    fn attach_file(self: Pin<&mut Self>, url: &QString, local_id: &QString) {
        let thread = self.qt_thread();
        let (url, local_id) = (url.to_string(), local_id.to_string());
        runtime().spawn_blocking(move || match crate::platform::read_picked(&url) {
            Ok((name, media_type, bytes)) => {
                let action = trommi_client_core::actions::Action::Attach { local_id, name, media_type, bytes: bytes.into() };
                if let Some(handle) = core_slot().lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    handle.send(trommi_client_core::app::Event::Action { action });
                }
            }
            Err(why) => tell_error(&thread, format!("The file could not be read: {why}")),
        });
    }

    fn set_theme(mut self: Pin<&mut Self>, mode: &QString) {
        let mode = if mode.to_string() == "dark" { "dark" } else { "light" };
        let base = crate::shell::base_dir();
        if std::fs::DirBuilder::new().recursive(true).create(&base).is_ok() {
            let _ = std::fs::write(base.join("theme"), mode);
        }
        self.as_mut().set_theme_mode(QString::from(mode));
    }

    fn shot_taken(self: Pin<&mut Self>) {
        // (the loop ends with its handle; the slot is free for the next run)
        core_slot().lock().unwrap_or_else(|e| e.into_inner()).take();
    }
}
