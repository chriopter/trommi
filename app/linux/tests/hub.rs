//! The Linux app on the app core against the real hub (this repository's `hub/`, as a child process): a room with
//! an agent's session and a card, this app's device in it, and the window handed the Desk with that card, drawn
//! from the App's renders alone. Then the app ends and starts again over the same folders.
//!
//! The device is placed in the app's slot as a sign-in would leave it: the App has no sign-in of its own yet (its
//! `join_room` comes with the account flows); until then the test lets a human device of the room in by link, as
//! the connector's tests do, and hands its stored state to the app.

#[path = "../../../tests/connector/common/mod.rs"]
mod common;

use common::{HubProc, Human, TempDir};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use trommi_connector::client::Client;
use trommi_connector::join::join_room;
use trommi_connector::store::Journal;
use trommi_linux::shell;

/// The hub's binary: `TROMMI_HUB_BIN`, else the main workspace's, built once here.
fn hub_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("TROMMI_HUB_BIN") {
        return PathBuf::from(path);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = std::process::Command::new(cargo)
        .args(["build", "--locked", "-p", "trommi-hub", "--manifest-path"])
        .arg(root.join("Cargo.toml"))
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the hub builds");
    let target = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("target"));
    target.join("debug/trommi-hub")
}

/// The temporary folders of human devices the connector's stand-in made so far.
fn human_dirs() -> Vec<PathBuf> {
    let prefix = format!("trommi-connector-test-human-{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .map(|dir| {
            dir.filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default()
}

/// The stored state of a device (its journal's files, not its lock) copied into `to`.
fn copy_state(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("state") {
            std::fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
}

struct Room {
    hub: HubProc,
    /// The agent's connector, kept so its session stays served.
    _agent: std::sync::Arc<Client>,
    _events: tokio::sync::mpsc::UnboundedReceiver<trommi_connector::client::ClientEvent>,
    _agent_dir: TempDir,
}

/// A hub, a human's room with a second human device (this app's: its state copied to `slot_device`), an agent in
/// a main session of its own, and one card of the agent waiting.
fn room(runtime: &tokio::runtime::Runtime, slot_device: &Path) -> Room {
    std::env::set_var("TROMMI_HUB_BIN", hub_binary());
    runtime.block_on(async {
        let hub = HubProc::start().await;
        let mut human = Human::found(&hub.url).await;
        // this app's device: let in by link, with KeyPackages, so the agent's session takes it in at its founding
        let before = human_dirs();
        let mut me = human.add_human().await;
        let me_dir = human_dirs().into_iter().find(|d| !before.contains(d)).expect("the new device's folder");
        human.follow_invites().await;
        me.publish_key_packages().await;
        // the agent, by link, and its session
        let agent_dir = TempDir::new("agent");
        let journal = Journal::open(agent_dir.path()).expect("a journal");
        let mut invite = human.invite(None).await;
        let link = invite.link.clone();
        let for_join = journal.clone();
        let joining = tokio::spawn(async move { join_room(&link, for_join, |_| {}, 50, 30_000).await });
        let admitted = human.admit(&mut invite).await;
        joining.await.expect("the join ran").expect("joined");
        let (agent, events) = Client::open(journal).await.expect("the slot opens");
        agent.start().await.expect("it goes online");
        human.found_session(&admitted).await;
        common::eventually("the agent is in its session", || async { agent.core.lock().await.session_id().is_some() }).await;
        let fields = json!({
            "card_type": "decision", "title": "Blue or green?", "body": "", "urgency": "normal",
            "options": [{ "key": "blue", "label": "Blue" }, { "key": "green", "label": "Green" }],
            "allows_multiple": false,
        });
        agent.send_card(fields.as_object().cloned().unwrap(), None).await.expect("a card");
        // this device takes its Welcome to the session, then its state goes to the app
        me.sync().await;
        copy_state(&me_dir, slot_device);
        drop(me);
        Room { hub, _agent: agent, _events: events, _agent_dir: agent_dir }
    })
}

/// Waits until the window was handed a view that `want` accepts; the last view seen is in the message otherwise.
fn wait_for(drawn: &mpsc::Receiver<String>, what: &str, want: impl Fn(&Value) -> bool) -> Value {
    let end = Instant::now() + Duration::from_secs(60);
    let mut last = Value::Null;
    while Instant::now() < end {
        match drawn.recv_timeout(Duration::from_millis(200)) {
            Ok(json) => {
                last = serde_json::from_str(&json).expect("the window reads JSON");
                if want(&last) {
                    return last;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    panic!("no view with {what}; the last: phase {} desk {}", last["phase"], last["desk"]);
}

/// The titles of the Desk's rows, in the order given.
fn desk_titles(view: &Value) -> Vec<String> {
    view["desk"]["rows"]
        .as_array()
        .map(|rows| rows.iter().filter_map(|r| r["title"].as_str().map(String::from)).collect())
        .unwrap_or_default()
}

/// Starts the shell over the folders under `TROMMI_LINUX_DIR`, retried while the last run still holds the slot.
fn start(runtime: &tokio::runtime::Runtime, hub: &str) -> (trommi_io::Handle, mpsc::Receiver<String>) {
    for _ in 0..250 {
        let (draw, drawn) = mpsc::channel();
        let mut config = shell::config(120);
        config.hub = hub.to_string();
        match shell::start(runtime, config, move |json| {
            let _ = draw.send(json);
        }) {
            Ok(handle) => return (handle, drawn),
            Err(_) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    panic!("the slot stayed held");
}

#[test]
fn the_desk_comes_from_the_real_hub_and_again_after_a_restart() {
    let dir = std::env::temp_dir().join(format!("trommi-linux-hub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("TROMMI_LINUX_DIR", &dir);
    let scenario = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let room = room(&scenario, &dir.join(shell::SLOT).join("device"));
    let app = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();

    let started = Instant::now();
    let (handle, drawn) = start(&app, &room.hub.url);
    let view = wait_for(&drawn, "the card on the Desk", |v| desk_titles(v).iter().any(|t| t == "Blue or green?"));
    eprintln!("the Desk after {} ms, phase {}", started.elapsed().as_millis(), view["phase"]);
    // the sidebar names the agent's session, and the card's row says who sent it
    let sessions = view["sidebar"]["sessions"].as_array().cloned().unwrap_or_default();
    assert_eq!(sessions.len(), 1, "one session: {sessions:?}");
    let row = &view["desk"]["rows"][0];
    assert_eq!(row["from"]["session_id"], sessions[0]["id"], "{row}");
    wait_for(&drawn, "the phase live", |v| v["phase"] == "live");

    // the app ends and starts again over the same folders: the same Desk
    drop(handle);
    let (handle, drawn) = start(&app, &room.hub.url);
    let view = wait_for(&drawn, "the card after the restart", |v| v["phase"] == "live" && desk_titles(v).iter().any(|t| t == "Blue or green?"));
    assert_eq!(desk_titles(&view), vec!["Blue or green?".to_string()]);
    drop(handle);
    drop(app);
    drop(room);
    drop(scenario);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Pictures of the window over the real hub's room, light and dark (Qt offscreen), into `TROMMI_LINUX_SHOTS`.
/// Not part of the run: `cargo test --test hub -- --ignored pictures`.
#[test]
#[ignore]
fn pictures_of_the_window_over_the_real_hub() {
    let shots = PathBuf::from(std::env::var("TROMMI_LINUX_SHOTS").expect("TROMMI_LINUX_SHOTS names the folder"));
    std::fs::create_dir_all(&shots).unwrap();
    let dir = std::env::temp_dir().join(format!("trommi-linux-pictures-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let scenario = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let room = room(&scenario, &dir.join(shell::SLOT).join("device"));
    let scene = std::env::var("TROMMI_LINUX_SCENE").unwrap_or_default();
    for theme in ["light", "dark"] {
        let name = if scene.is_empty() { format!("desk-{theme}.png") } else { format!("{scene}-{theme}.png") };
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_trommi-linux"))
            .env("TROMMI_LINUX_APP_CORE", "1")
            .env("QT_QPA_PLATFORM", "offscreen")
            .env("TROMMI_LINUX_DIR", &dir)
            .env("TROMMI_HUB", &room.hub.url)
            .env("TROMMI_LINUX_THEME", theme)
            .env("TROMMI_LINUX_SHOT", shots.join(&name))
            .env("TROMMI_LINUX_SHOT_AFTER_MS", "3000")
            .status()
            .expect("the app runs");
        assert!(status.success(), "{name}: {status}");
        assert!(std::fs::metadata(shots.join(&name)).map(|m| m.len() > 8000).unwrap_or(false), "{name} is empty");
    }
    drop(room);
    drop(scenario);
    let _ = std::fs::remove_dir_all(&dir);
}
