//! The user's paths end to end, on a local hub with the real binary: what a session that cannot go on says,
//! and that the way it names gets it onto the board again.
mod common;

use common::process::Seat;
use common::{HubProc, Human, TempDir};
use serde_json::{json, Value};
use trommi_core::ids::GroupId;

fn tool_names(listed: &Value) -> Vec<String> {
    listed["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect()
}

/// A folder joined by the CLI, with its human.
async fn joined_on(hub: &HubProc) -> (Human, Seat) {
    let (human, seat, _group) = joined_with_group(hub).await;
    (human, seat)
}

async fn joined_with_group(hub: &HubProc) -> (Human, Seat, GroupId) {
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let join = seat.join(&link);
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((joined, log), group) = tokio::join!(join, admit);
    assert!(joined, "the join failed:\n{log}");
    // what to do next, for either program
    assert!(log.contains("/mcp") && log.contains("codex resume"), "{log}");
    (human, seat, group)
}

/// The hub was set up again (its data gone): the folder's key is of a room no hub knows. The session says so,
/// and the connect tool with a new link of the new room gets it onto the board, without a restart.
#[tokio::test(flavor = "multi_thread")]
async fn a_key_of_a_room_the_hub_no_longer_knows_gives_way_to_a_new_invite() {
    let hub = HubProc::start().await;
    let (_old_human, seat) = joined_on(&hub).await;
    let (_data, port) = hub.stop();
    let hub = HubProc::start_in(TempDir::new("hub"), port).await;
    let mut human = Human::found(&hub.url).await;

    let mut mcp = seat.serve().await;
    // the session learns that its key is out, says how to get back, and offers connect for that
    let mut said = String::new();
    for _ in 0..100 {
        let (text, failed) = mcp.call("list_cards", json!({})).await;
        assert!(failed, "{text}");
        said = text;
        if said.contains("/trommi:connect") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(said.contains("/trommi:connect"), "{said}");
    let listed = mcp.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");

    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), _group) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    mcp.ready().await;
    let listed = mcp.request("tools/list", json!({})).await;
    assert!(tool_names(&listed).iter().any(|n| n == "reply"), "{listed}");
    mcp.close().await;

    // after a restart the folder goes on in the new room
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    mcp.close().await;
}

/// What a hint tells the human to run exists: the connector is updated by `trommi-connector update` (the plugin
/// holds no program, and there is no connect script any more), and the join command is `connect`.
#[tokio::test(flavor = "multi_thread")]
async fn the_update_and_usage_hints_name_commands_that_exist() {
    let line = trommi_connector::host::update_line();
    assert!(line.contains("trommi-connector update"), "{line}");
    assert!(!line.contains("connect script") && !line.contains("plugin"), "{line}");

    let seat = Seat::new("http://127.0.0.1:9");
    let output = seat
        .command(&["connect"])
        .env_remove("TROMMI_INVITE")
        .output()
        .await
        .expect("connect runs");
    let told = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{told}");
    assert!(told.contains("trommi-connector connect '<invite link>'"), "{told}");
}

/// The update card is filed in the human's language: no German words are put into the agent's mouth.
#[test]
fn the_update_card_and_its_tool_are_not_in_german() {
    let mut texts = vec![trommi_connector::prompt::instructions()];
    texts.push(trommi_connector::prompt::tools_list_json(true));
    for text in texts {
        for word in ["jetzt", "später", "Neue Connector"] {
            assert!(!text.contains(word), "{word} in: {text}");
        }
    }
}

/// Whether the human's stream says the device is online, within `ms`.
async fn online_within(human: &Human, device: &str, ms: u64) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    let mut online = false;
    while std::time::Instant::now() < end {
        let Ok(mut stream) = human.hub.stream(0).await else {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            continue;
        };
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(400);
        while let Ok(Ok(Some(event))) = tokio::time::timeout_at(
            deadline,
            stream.next(std::time::Duration::from_secs(5)),
        )
        .await
        {
            if event.name == "presence" && event.data["device"] == device {
                online = event.data["online"] == Value::Bool(true);
            }
        }
        if online {
            return true;
        }
    }
    false
}

async fn test_switch(hub: &HubProc, what: &str, body: Value) {
    let test = trommi_connector::hub::Hub::new(&hub.url, None, None).unwrap();
    test.open_call(reqwest::Method::POST, &format!("/v1/__test/{what}"), Some(&body))
        .await
        .expect("the switch");
}

fn signal(pid: u32, sig: &str) {
    let ok = std::process::Command::new("kill")
        .args([sig, &pid.to_string()])
        .status()
        .expect("kill runs")
        .success();
    assert!(ok, "kill {sig} {pid}");
}

/// A laptop that sleeps: the connector stands still while the hub's clock runs on past its lease. Once it wakes,
/// it is online again by itself, hears what the human wrote meanwhile, and its tools work.
#[tokio::test(flavor = "multi_thread")]
async fn after_a_sleep_past_the_lease_the_session_is_back_by_itself() {
    let hub = HubProc::start().await;
    let (mut human, seat, group) = joined_with_group(&hub).await;
    let mut mcp = seat.serve_plain().await;
    mcp.ready().await;
    let device = human.seat(&group).await.expect("the agent's seat").to_base64url();
    assert!(online_within(&human, &device, 5000).await, "online before");
    let pid = mcp.child.id().expect("a pid");
    signal(pid, "-STOP");
    test_switch(&hub, "clock", json!({ "advance_ms": 10 * 60_000 })).await;
    test_switch(&hub, "sweep", json!({})).await;
    human
        .say(&group, json!({ "content_type": "message", "text": "Written while you slept" }))
        .await
        .expect("the human writes");
    signal(pid, "-CONT");
    assert!(online_within(&human, &device, 30_000).await, "online again after the sleep");
    let mut heard = String::new();
    for _ in 0..100 {
        heard.push_str(&mcp.ok("inbox", json!({})).await);
        if heard.contains("Written while you slept") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(heard.contains("Written while you slept"), "{heard}");
    assert!(mcp.ok("reply", json!({ "text": "Awake." })).await.starts_with("sent"));
    mcp.close().await;
}

/// Two folders whose names differ only in case or punctuation (`~/project` and `~/Project`, `my-app` and
/// `my_app`) are two folders: a session in the second never takes the first one's key.
#[tokio::test(flavor = "multi_thread")]
async fn folders_whose_names_slug_alike_do_not_share_keys() {
    let hub = HubProc::start().await;
    let (_human, seat) = joined_on(&hub).await;
    let other = seat.home.path().join("Project");
    std::fs::create_dir_all(&other).expect("a second folder");
    let mut command = seat.command(&[]);
    command
        .current_dir(&other)
        .env("TROMMI_FOLDER", &other)
        .env("TROMMI_SESSION_KEY", "another-session")
        .env("TROMMI_SELF_JOIN", "0");
    let hello = json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } });
    let mut mcp = common::process::Mcp::start_as(command, hello).await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let listed = mcp.request("tools/list", json!({})).await;
    let (said, _) = mcp.call("list_cards", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}\n{said}");
    mcp.close().await;
    let base = |f: &std::path::Path| {
        std::fs::read_to_string(f.join(".trommi/slot-base")).unwrap_or_default()
    };
    assert_ne!(base(&seat.folder), base(&other));
}

/// The largest attachment and asset the tools take (64 MiB less one byte: a padded file holds 64 MiB with its
/// mark) goes through to the hub; one byte more is refused
/// before anything is sent, with its size in the words.
#[tokio::test(flavor = "multi_thread")]
async fn the_largest_attachment_and_asset_go_through() {
    let hub = HubProc::start().await;
    let (_human, seat) = joined_on(&hub).await;
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    let big = seat.folder.join("big.bin");
    let mut bytes = vec![0u8; (64 << 20) - 1];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = (i * 7919 % 251) as u8;
    }
    std::fs::write(&big, &bytes).expect("written");
    let said = mcp
        .ok("reply", json!({ "text": "the log", "attachments": [big.display().to_string()] }))
        .await;
    assert!(said.starts_with("sent"), "{said}");
    let said = mcp.ok("publish_asset", json!({ "path": big.display().to_string() })).await;
    assert!(!said.is_empty(), "{said}");
    bytes.push(0);
    std::fs::write(&big, &bytes).expect("written");
    let (said, failed) = mcp
        .call("reply", json!({ "text": "too big", "attachments": [big.display().to_string()] }))
        .await;
    assert!(failed && said.contains("67108864 bytes") && said.contains("64 MiB"), "{said}");
    mcp.close().await;
}

/// An agent opens helper sessions up to the limit (100 live); the next one is refused in words that say what to
/// do (close one), and after close_session a new one opens.
#[tokio::test(flavor = "multi_thread")]
async fn helper_sessions_up_to_the_limit_then_close_one_to_open_another() {
    let hub = HubProc::start().await;
    let (_human, seat) = joined_on(&hub).await;
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    for n in 0..100 {
        let (said, failed) = mcp.call("open_session", json!({ "name": format!("H{n}") })).await;
        assert!(!failed, "helper {n}: {said}");
    }
    let (said, failed) = mcp.call("open_session", json!({ "name": "One more" })).await;
    eprintln!("101st: {said}");
    assert!(failed, "{said}");
    assert!(said.contains("close_session"), "{said}");
    mcp.ok("close_session", json!({ "name": "H0" })).await;
    let said = mcp.ok("open_session", json!({ "name": "One more" })).await;
    assert!(said.starts_with("child session opened"), "{said}");
    mcp.close().await;
}

/// A session that starts while the hub is away says so in plain words, and is in its room by itself once the hub
/// is back, without a restart.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_started_while_the_hub_is_away_gets_in_once_it_is_back() {
    let hub = HubProc::start().await;
    let (_human, seat) = joined_on(&hub).await;
    let (data, port) = hub.stop();
    let mut mcp = seat.serve().await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let (said, failed) = mcp.call("reply", json!({ "text": "while away" })).await;
    assert!(failed && said.contains("call the tool again"), "{said}");
    let (said, failed) = mcp.call("create_decision", json!({ "title": "Q?", "body": "b", "options": [{"key":"a","label":"A"},{"key":"b","label":"B"}] })).await;
    assert!(failed && said.contains("call the tool again"), "{said}");
    let _hub = HubProc::start_in(data, port).await;
    mcp.ready().await;
    mcp.close().await;
}

/// A binary at the connector's path that does not check against the manifest beside it (a local build copied
/// over a release, or a tampered file) is never swapped in; reload_connector says so and how to get the release
/// back, instead of saying that it was swapped in.
#[tokio::test(flavor = "multi_thread")]
async fn reload_never_claims_a_refused_binary_was_swapped_in() {
    let hub = HubProc::start().await;
    let (_human, seat) = joined_on(&hub).await;
    let bin = seat.home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("trommi-connector");
    let original = std::fs::read(common::process::connector_binary()).expect("the built connector");
    std::fs::write(&program, &original).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    // a release's manifest beside it that names another file
    std::fs::write(bin.join("manifest.json"), b"{}").unwrap();
    std::fs::write(bin.join("manifest.json.sig"), [0u8; 64]).unwrap();
    let mut mcp = seat
        .serve_launched(&program, &[("TROMMI_UPDATE_POLL_MS", "1000")])
        .await;
    mcp.ready().await;
    let mut changed = original.clone();
    changed.extend_from_slice(b"\nanother build");
    let new_file = bin.join(".new.trommi-connector");
    std::fs::write(&new_file, &changed).unwrap();
    std::fs::set_permissions(&new_file, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&new_file, &program).unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let (said, failed) = mcp.call("reload_connector", json!({})).await;
    assert!(!said.contains("swapped in now"), "{said}");
    assert!(said.contains("trommi-connector update"), "{failed} {said}");
    mcp.close().await;
}
