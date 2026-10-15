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
use trommi_connector::driver::Driver as Client;
use trommi_connector::join::join_room;
use trommi_connector::store::Journal;
use trommi_linux::shell;

/// The tests set `TROMMI_LINUX_DIR` for the process: one at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    events: tokio::sync::mpsc::UnboundedReceiver<trommi_connector::driver::DriverEvent>,
    _agent_dir: TempDir,
}

/// The account the room is founded with: the e-mail and the password a sign-in types.
const EMAIL: &str = "ada@example.org";
const PASSWORD: &str = "correct horse battery staple";

/// The body of an account made with the room's founding (`POST /v1/rooms` with `account`), as the apps sign up:
/// copies of the code sealed under the password and under a kit.
fn account_body(room: &trommi_core::ids::RoomId, code: &trommi_core::crypto::Secret<32>) -> Value {
    use trommi_connector::driver::b64;
    use trommi_core::account::{self, Way};
    use trommi_core::crypto::SystemEntropy;
    let pw = account::password_keys(EMAIL, PASSWORD, Some(account::KDF_RECORD)).expect("the password's keys");
    let words = account::generate_kit_words(&mut SystemEntropy).expect("words");
    let words = String::from_utf8(words.expose().to_vec()).expect("text");
    let kit = account::kit_keys(EMAIL, &words).expect("the kit's keys");
    let sealed_password = account::seal_code(&pw.wrap_key, room, Way::Password, code, &mut SystemEntropy).expect("sealed");
    let sealed_kit = account::seal_code(&kit.wrap_key, room, Way::Kit, code, &mut SystemEntropy).expect("sealed");
    json!({
        "email": EMAIL,
        "password": {
            "auth_key": b64(pw.auth_key.expose()), "sealed_copy": b64(&sealed_password),
            "kdf": { "alg": "argon2id", "v": 1, "m": 65536, "t": 3, "p": 1 },
        },
        "kit": { "auth_key": b64(kit.auth_key.expose()), "sealed_copy": b64(&sealed_kit) },
    })
}

/// A hub, a human's room with an account (`EMAIL`, `PASSWORD`), an agent in a main session of its own, and one
/// card of the agent waiting. With `slot_device`: a second human device, let in by link before the session was
/// founded, its state copied there (this app's, as a sign-in would leave it).
fn room(runtime: &tokio::runtime::Runtime, slot_device: Option<&Path>) -> Room {
    std::env::set_var("TROMMI_HUB_BIN", hub_binary());
    runtime.block_on(async {
        let hub = HubProc::start().await;
        let mut human = Human::found_with(&hub.url, |room, code| Some(account_body(room, code))).await;
        // this app's device: let in by link, with KeyPackages, so the agent's session takes it in at its founding
        let mut me = None;
        if slot_device.is_some() {
            let before = human_dirs();
            let mut device = human.add_human().await;
            let dir = human_dirs().into_iter().find(|d| !before.contains(d)).expect("the new device's folder");
            human.follow_invites().await;
            device.publish_key_packages().await;
            me = Some((device, dir));
        }
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
        common::eventually("the agent is in its session", || async { agent.lock().session_id().is_some() }).await;
        let fields = json!({
            "card_type": "decision", "title": "Blue or green?", "body": "", "urgency": "normal",
            "options": [{ "key": "blue", "label": "Blue" }, { "key": "green", "label": "Green" }],
            "allows_multiple": false,
        });
        agent.send_card(fields.as_object().cloned().unwrap(), None).await.expect("a card");
        // this device takes its Welcome to the session, then its state goes to the app
        if let (Some((mut device, dir)), Some(slot_device)) = (me, slot_device) {
            device.sync().await;
            copy_state(&dir, slot_device);
        }
        Room { hub, _agent: agent, events, _agent_dir: agent_dir }
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
    panic!("no view with {what}; the last: phase {} account {} desk {}", last["phase"], last["account"], last["desk"]);
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
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("trommi-linux-hub-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("TROMMI_LINUX_DIR", &dir);
    let scenario = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let mut room = room(&scenario, Some(&dir.join(shell::SLOT).join("device")));
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
    if view["phase"] != "live" {
        wait_for(&drawn, "the phase live", |v| v["phase"] == "live");
    }

    // the window's pages, as the window asks for them (`shell::person_event`): the card's, then the session's
    let card = row["id"].as_str().unwrap().to_string();
    let session = sessions[0]["id"].as_str().unwrap().to_string();
    let send = |json: Value| assert!(handle.send(shell::person_event(&json.to_string()).expect("the person's event")));
    send(json!({ "type": "focus", "focus": { "screen": { "screen": "card", "card_id": card } } }));
    let view = wait_for(&drawn, "the card's page", |v| v["cards"][&card].is_object());
    let page = &view["cards"][&card];
    assert_eq!((page["title"].as_str(), page["answer"]["mode"].as_str()), (Some("Blue or green?"), Some("options")), "{page}");
    let keys: Vec<&str> = page["options"].as_array().unwrap().iter().filter_map(|o| o["key"].as_str()).collect();
    assert_eq!(keys.len(), 2, "{page}");
    assert!(keys.contains(&"blue") && keys.contains(&"green"), "{page}");
    send(json!({ "type": "focus", "focus": { "screen": { "screen": "session", "session_id": session } } }));
    let view = wait_for(&drawn, "the session's page", |v| v["sessions"][&session].is_object());
    let page = &view["sessions"][&session];
    assert!(page["talk"].as_array().is_some_and(|t| t.iter().any(|r| r["kind"] == "ask" || r["card"]["id"] == card.as_str())), "the question stands where it was asked: {page}");
    assert!(view["cards"].as_object().is_none_or(|c| c.is_empty()), "the card's page is gone with its focus");
    send(json!({ "type": "focus", "focus": { "screen": { "screen": "desk" } } }));
    wait_for(&drawn, "the Desk alone", |v| v["sessions"].as_object().is_none_or(|s| s.is_empty()));

    // the app ends and starts again over the same folders: the same Desk
    drop(handle);
    let (handle, drawn) = start(&app, &room.hub.url);
    let view = wait_for(&drawn, "the card after the restart", |v| v["phase"] == "live" && desk_titles(v).iter().any(|t| t == "Blue or green?"));
    assert_eq!(desk_titles(&view), vec!["Blue or green?".to_string()]);

    // a tile on the Desk: the answer leaves the Desk at once, and reaches the agent as the person's answer
    let send = |json: Value| assert!(handle.send(shell::person_event(&json.to_string()).expect("the person's event")));
    send(json!({ "type": "action", "action": { "do": "answer", "card_id": card, "choices": ["green"] } }));
    wait_for(&drawn, "the Desk without the answered card", |v| !desk_titles(v).iter().any(|t| t == "Blue or green?"));
    let command = scenario.block_on(async {
        loop {
            let event = tokio::time::timeout(Duration::from_secs(90), room.events.recv()).await.expect("the agent hears in time").expect("the agent lives");
            if let trommi_connector::driver::DriverEvent::Command(command) = event {
                if command.command == "answer" {
                    return command;
                }
            }
        }
    });
    assert_eq!(command.object_id.as_deref(), Some(card.as_str()), "the answer is to the card");
    assert_eq!(command.choices, vec!["green".to_string()], "the answer chose green");

    // and again after a restart: the card stays answered, nothing is sent twice
    drop(handle);
    let (handle, drawn) = start(&app, &room.hub.url);
    let view = wait_for(&drawn, "the Desk after the second restart", |v| v["phase"] == "live");
    assert!(!desk_titles(&view).iter().any(|t| t == "Blue or green?"), "the answered card stays off the Desk: {}", view["desk"]);
    std::thread::sleep(Duration::from_secs(2));
    let again = scenario.block_on(async {
        while let Ok(event) = room.events.try_recv() {
            if let trommi_connector::driver::DriverEvent::Command(command) = event {
                if command.command == "answer" {
                    return true;
                }
            }
        }
        false
    });
    assert!(!again, "the answer was sent once");
    drop(handle);
    drop(app);
    drop(room);
    drop(scenario);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Signing in as the window does it (ViewSignIn: `Action::Account` with the e-mail and the password) on an empty
/// slot: the core opens the code, joins the room and its session, and the Desk shows the agent's card.
#[test]
fn signing_in_with_the_password_brings_the_desk() {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("trommi-linux-signin-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::env::set_var("TROMMI_LINUX_DIR", &dir);
    let scenario = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let room = room(&scenario, None);
    let app = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();

    let (handle, drawn) = start(&app, &room.hub.url);
    wait_for(&drawn, "the sign-in", |v| v["phase"] == "signed_out");
    let send = |json: Value| assert!(handle.send(shell::person_event(&json.to_string()).expect("the person's event")));
    // a wrong password: the core says so on the sign-in, nothing is joined
    send(json!({ "type": "action", "action": { "do": "account", "action": { "step": "sign_in", "email": EMAIL, "password": "wrong" } } }));
    let view = wait_for(&drawn, "the problem of a wrong password", |v| v["account"]["problem"].is_object());
    assert_eq!(view["phase"], "signed_out", "{}", view["account"]);
    // the right one, as ViewSignIn sends it; the hub makes the next try from here wait a second after a wrong one,
    // which the core says as such (not as offline), and the person tries again
    let started = Instant::now();
    let mut view = Value::Null;
    for _ in 0..3 {
        send(json!({ "type": "action", "action": { "do": "account", "action": { "step": "sign_in", "email": EMAIL, "password": PASSWORD } } }));
        view = wait_for(&drawn, "the card after the sign-in, or the throttle", |v| {
            desk_titles(v).iter().any(|t| t == "Blue or green?") || v["account"]["problem"].is_object()
        });
        match view["account"]["problem"]["key"].as_str() {
            None => break,
            Some("account.throttled") => std::thread::sleep(Duration::from_millis(2_100)),
            Some(other) => panic!("the sign-in failed: {other}"),
        }
    }
    assert!(desk_titles(&view).iter().any(|t| t == "Blue or green?"), "{}", view["account"]);
    eprintln!("signed in, the Desk after {} ms", started.elapsed().as_millis());
    assert!(view["account"]["problem"].is_null(), "{}", view["account"]);
    if view["phase"] != "live" {
        wait_for(&drawn, "the phase live", |v| v["phase"] == "live");
    }

    // the app starts again: the device it joined with is stored, the Desk comes without a sign-in
    drop(handle);
    let (handle, drawn) = start(&app, &room.hub.url);
    wait_for(&drawn, "the card after the restart", |v| v["phase"] == "live" && desk_titles(v).iter().any(|t| t == "Blue or green?"));
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
    let room = room(&scenario, Some(&dir.join(shell::SLOT).join("device")));
    let scene = std::env::var("TROMMI_LINUX_SCENE").unwrap_or_default();
    for theme in ["light", "dark"] {
        let name = if scene.is_empty() { format!("desk-{theme}.png") } else { format!("{scene}-{theme}.png") };
        let status = std::process::Command::new(env!("CARGO_BIN_EXE_trommi-linux"))
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
