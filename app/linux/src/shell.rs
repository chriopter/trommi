//! The shell: this app as the app core sees it (core/client/README.md, "The app core"). It draws and runs the
//! platform's services; it decides nothing.
//!
//! ```text
//! Qt thread                         tokio (trommi-io)                    core/client
//!   taps, focus      ── Handle ──▶  the loop that owns the App     ──▶   App::handle
//!   view_json (QML)  ◀── draw ────  Window::show(Render): the fold ◀──   Effect::Render { delta }
//! ```
//!
//! - [`View`] is the fold: the one `ViewModel` this app keeps, each delta applied with the core's own rule, and
//!   its JSON text, which is all the QML reads.
//! - [`Window`] is the part of the shell that is not I/O (`trommi_io::Shell`): renders to the window, log lines to
//!   standard error.
//! - [`start`] opens the slot's folders (the device lock with them) and starts the loop.
//!
//! On disk, under the app's folder (`TROMMI_LINUX_DIR`, else `$XDG_DATA_HOME/trommi-linux`, else
//! `~/.local/share/trommi-linux`): `<slot>/device/` the device space, `<slot>/cache/` the cache space.

use std::path::PathBuf;
use trommi_client_core::app::{Abilities, App, Config, Effect, Event, LogLevel};
use trommi_client_core::views::{ViewDelta, ViewModel};
use trommi_io::{Handle, Shell, Stores};
use crate::platform::Desktop;

/// The view model as this app holds it.
#[derive(Default)]
pub struct View {
    model: ViewModel,
}

impl View {
    /// Folds one render in. `false`: the delta does not follow the last one (it is left out).
    pub fn fold(&mut self, delta: &ViewDelta) -> bool {
        self.model.apply(delta).is_ok()
    }

    /// What the window reads.
    pub fn json(&self) -> String {
        serde_json::to_string(&self.model).unwrap_or_else(|_| "{}".into())
    }
}

/// The window's side of the loop: `draw` is given the view's JSON after every render; the desktop's services
/// (`platform::Desktop`) do the files, notifications and links.
pub struct Window<F> {
    view: View,
    draw: F,
    desktop: Desktop,
}

impl<F: FnMut(String) + Send + 'static> Window<F> {
    pub fn new(draw: F) -> Self {
        Self::with_desktop(draw, Desktop::new())
    }

    pub fn with_desktop(draw: F, desktop: Desktop) -> Self {
        Self {
            view: View::default(),
            draw,
            desktop,
        }
    }
}

impl<F: FnMut(String) + Send + 'static> Shell for Window<F> {
    fn show(&mut self, effect: Effect) {
        match effect {
            Effect::Render { delta, .. } => {
                if self.view.fold(&delta) {
                    (self.draw)(self.view.json());
                } else {
                    eprintln!("[trommi] a render came out of order and was left out");
                }
            }
            Effect::Log { level, text } => {
                if level != LogLevel::Debug || std::env::var_os("TROMMI_LINUX_DEBUG").is_some() {
                    eprintln!("[trommi] {text}");
                }
            }
            Effect::FileReady { file_id, bytes, .. } => {
                if let Err(e) = self.desktop.file_ready(&file_id, &bytes) {
                    eprintln!("[trommi] a file for drawing could not be kept: {}", e.kind());
                }
            }
            Effect::FileGone { file_id } => self.desktop.file_gone(&file_id),
            Effect::Notify { notice } => self.desktop.notify(notice),
            Effect::SaveFile { name, bytes, .. } => {
                if let Err(e) = self.desktop.save_file(&name, &bytes) {
                    eprintln!("[trommi] the file could not be saved: {}", e.kind());
                }
            }
            Effect::OpenUrl { url, .. } => {
                if !self.desktop.open_url(&url) {
                    eprintln!("[trommi] a link of another kind was not opened");
                }
            }
            // (no widgets on this desktop, no notification extension)
            Effect::Snapshot { .. } | Effect::NotifyContext { .. } | Effect::ShareInbox { .. } | Effect::ForgetShared => {}
            // (I/O: never handed to the window)
            Effect::Http { .. }
            | Effect::OpenStream { .. }
            | Effect::Cancel { .. }
            | Effect::Store { .. }
            | Effect::Secret { .. }
            | Effect::Platform { .. }
            | Effect::Timer { .. } => {}
        }
    }
}

/// What the window may send, as JSON in the contract's spelling (`Event`): what the person did (`action`), what
/// they look at (`focus`), the window coming to the front or going to the back, the time zone. Anything else (an
/// I/O result, a start) is the loop's alone and refused here.
pub fn person_event(json: &str) -> Result<Event, String> {
    let event: Event = serde_json::from_str(json).map_err(|e| format!("not an event: {e}"))?;
    match event {
        Event::Action { .. } | Event::Focus { .. } | Event::Foreground | Event::Background | Event::Zone { .. } => Ok(event),
        _ => Err("not the person's event".into()),
    }
}

/// Where the app keeps its slots.
pub fn base_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TROMMI_LINUX_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("trommi-linux");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/trommi-linux")
}

/// The slot this app owns: one room per installation.
pub const SLOT: &str = "default";

/// What the core cannot find out itself. `utc_offset_min` is the desktop's (Qt's) word.
pub fn config(utc_offset_min: i32) -> Config {
    use std::hash::{BuildHasher, Hasher};
    // the system's randomness, as the standard library seeds its hash maps with it; below 2^53
    let seed = std::collections::hash_map::RandomState::new().build_hasher().finish() >> 11;
    let locale = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .ok()
        .and_then(|l| l.split('.').next().map(|l| l.replace('_', "-")))
        .unwrap_or_default();
    Config {
        platform: "linux".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        hub: std::env::var("TROMMI_HUB").unwrap_or_default(),
        slot: SLOT.into(),
        seed,
        utc_offset_min,
        locale,
        can: Abilities {
            notify: true,
            ..Abilities::default()
        },
        demo: None,
    }
}

/// Opens the slot and starts the loop on `runtime`; `draw` gets the view's JSON after every render (on the
/// loop's task). The handle is the app's: dropping it ends the loop and frees the slot.
pub fn start(
    runtime: &tokio::runtime::Runtime,
    config: Config,
    draw: impl FnMut(String) + Send + 'static,
) -> Result<Handle, String> {
    start_with(runtime, config, draw, Desktop::new())
}

/// The same, with the desktop's services as given (the window's, or a test's).
pub fn start_with(
    runtime: &tokio::runtime::Runtime,
    config: Config,
    draw: impl FnMut(String) + Send + 'static,
    desktop: Desktop,
) -> Result<Handle, String> {
    let slot = base_dir().join(&config.slot);
    let stores = Stores::open(&slot.join("device"), &slot.join("cache")).map_err(|e| e.to_string())?;
    let _on = runtime.enter();
    let (handle, _task) = trommi_io::start(App::new(), Window::with_desktop(draw, desktop), stores)?;
    handle.send(Event::Start { config });
    Ok(handle)
}
