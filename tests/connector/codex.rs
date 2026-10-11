//! The connector with Codex as its MCP client: a scripted client that behaves as Codex 0.160 does. Codex shows no
//! channel events, never asks for the tool list again (it ignores `notifications/tools/list_changed`), names its
//! thread in every tool call's `_meta`, and starts a turn in an idle session when a message is queued for it
//! (`codex queue --thread <id> --message <text>`). The script that stands for `codex` here notes every such call.
//!
//! One session from the invite link to a restart and a hot swap: connect by the tool, the board tools listed from
//! the start, a card filed, the human's answer waking the session (the queued message) and read with `inbox`,
//! reply, revise_card, a helper session, a restart that wakes the same thread before its first call, and a
//! swapped-in connector that does the same.
mod common;

use common::process::{connector_binary, Seat};
use common::{eventually, HubProc, Human};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn tool_names(listed: &Value) -> Vec<String> {
    listed["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(String::from))
        .collect()
}

/// A `codex` that notes each call's arguments, one call per line, in `calls`.
fn fake_codex(dir: &Path) -> (PathBuf, PathBuf) {
    let calls = dir.join("codex-calls");
    let program = dir.join("codex");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\n",
            calls.display()
        ),
    )
    .expect("written");
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .expect("executable");
    (program, calls)
}

fn calls(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

/// Waits until `codex queue` was called for `thread` more than `before` times.
async fn woken(path: &Path, thread: &str, before: usize) -> String {
    let wanted = format!("queue --thread {thread} --message ");
    eventually("the Codex session is woken", || {
        let n = calls(path)
            .iter()
            .filter(|c| c.starts_with(&wanted))
            .count();
        async move { n > before }
    })
    .await;
    calls(path)
        .into_iter()
        .rfind(|c| c.starts_with(&wanted))
        .unwrap_or_default()
}

fn wakes(path: &Path, thread: &str) -> usize {
    let wanted = format!("queue --thread {thread} --message ");
    calls(path)
        .iter()
        .filter(|c| c.starts_with(&wanted))
        .count()
}

/// Calls inbox until it holds `text` (a woken Codex calls it once; the event may still be on its way).
async fn inbox_holds(mcp: &mut common::process::Mcp, text: &str) -> String {
    let mut said = String::new();
    for _ in 0..300 {
        said = mcp.ok("inbox", json!({})).await;
        if said.contains(text) {
            return said;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    panic!("inbox never held {text:?}: {said}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_codex_session_connects_files_cards_and_is_woken_by_the_board() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let (codex, codex_calls) = fake_codex(seat.home.path());
    let thread = "01a129c9-b755-7380-8769-c46a0f75a6a1";

    // The connector as installed: a copy behind the launcher, which the test replaces later.
    let bin = seat.home.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("trommi-connector");
    let original = std::fs::read(connector_binary()).expect("the built connector");
    std::fs::write(&program, &original).unwrap();
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let env = [("TROMMI_UPDATE_POLL_MS", "1000")];
    let mut mcp = seat.serve_codex(&codex, thread, Some(&program), &env).await;

    // Codex lists the tools once, at its start: in a folder in no room yet that is every board tool and connect
    // (a list that grew later would never be seen).
    let listed = mcp.request("tools/list", json!({})).await;
    let names = tool_names(&listed);
    for wanted in [
        "connect",
        "inbox",
        "reply",
        "create_decision",
        "revise_card",
        "open_session",
    ] {
        assert!(names.iter().any(|n| n == wanted), "{wanted} in {names:?}");
    }
    // Every tool says what it does to the world, so that Codex does not ask for approval of each board call.
    for tool in listed["tools"].as_array().unwrap() {
        assert!(tool["annotations"].is_object(), "{tool}");
    }
    let reply = listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "reply")
        .unwrap();
    assert_eq!(reply["annotations"]["destructiveHint"], false);
    assert_eq!(reply["annotations"]["openWorldHint"], false);

    // A board tool before connecting says what to do, in Codex's words, and never to run the connector.
    let (said, failed) = mcp.call("reply", json!({ "text": "hello" })).await;
    assert!(failed && said.contains("connect tool"), "{said}");
    assert!(
        !said.contains("/trommi:connect") && !said.contains("/mcp"),
        "{said}"
    );

    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), group) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    // No new list needed: the board tools work in the same session.
    mcp.ready().await;
    mcp.ok(
        "introduce",
        json!({ "model": "GPT-6", "task": "Codex test drive", "icon": "drum" }),
    )
    .await;
    // Between turns it hears the board at once (it is woken): the lease says so, and the apps show it as there,
    // never as gone or asleep.
    let db = hub.data.path().join("hub.db");
    eventually("the lease reports a session that hears", || {
        let db = db.clone();
        async move {
            rusqlite::Connection::open_with_flags(&db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .ok()
                .and_then(|c| {
                    c.query_row("SELECT hears FROM agent_leases", [], |row| {
                        row.get::<_, bool>(0)
                    })
                    .ok()
                })
                == Some(true)
        }
    })
    .await;
    human.sync().await;
    assert!(
        human
            .items
            .iter()
            .any(|(_, _, p)| p["value"]["platform"] == "codex"),
        "the device says it is a Codex session: {:?}",
        human.items
    );

    // A card; its answer wakes the idle session, and inbox hands it over with the human's note.
    let made = mcp
        .ok(
            "create_decision",
            json!({ "title": "README language?", "options": [{ "key": "en", "label": "English" }, { "key": "de", "label": "German" }] }),
        )
        .await;
    assert!(
        made.contains("inbox") && !made.contains("channel event"),
        "{made}"
    );
    let card = made
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    let before = wakes(&codex_calls, thread);
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["de"], "note": "Auf Deutsch, bitte: äöü ß 🥁" }),
            &["de"],
            false,
        )
        .await
        .expect("taken");
    let wake = woken(&codex_calls, thread, before).await;
    assert!(
        wake.contains("[Trommi]") && wake.contains("inbox"),
        "{wake}"
    );
    let said = inbox_holds(&mut mcp, "choice=\"de\"").await;
    assert!(said.contains("äöü ß 🥁"), "the note comes along: {said}");
    assert!(
        !said.contains("--dangerously-load-development-channels"),
        "{said}"
    );
    mcp.ok("reply", json!({ "card_id": card, "text": "Wird gemacht." }))
        .await;
    mcp.ok(
        "close_card",
        json!({ "card_id": card, "summary": "German." }),
    )
    .await;

    // An ongoing matter stays one card.
    let info = mcp
        .ok(
            "create_info",
            json!({ "title": "Job: running", "body": "Step 1 of 3" }),
        )
        .await;
    let info_id = info
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    let revised = mcp
        .ok(
            "revise_card",
            json!({ "card_id": info_id, "title": "Done: job finished", "body": "All 3 steps." }),
        )
        .await;
    assert!(revised.contains("version 2"), "{revised}");

    // A helper session.
    mcp.ok("open_session", json!({ "name": "Docs", "task": "docs" }))
        .await;
    mcp.ok(
        "set_status",
        json!({ "session": "Docs", "id": "docs", "label": "Docs", "state": "working" }),
    )
    .await;
    mcp.ok("reply", json!({ "session": "Docs", "text": "Ready." }))
        .await;
    mcp.ok(
        "close_session",
        json!({ "name": "Docs", "summary": "done" }),
    )
    .await;

    // Long words, not ASCII, come through whole.
    let long = "Grüße aus Köln – ÄÖÜäöüß€✓🥁 ".repeat(400);
    mcp.ok("reply", json!({ "text": long })).await;
    human.sync().await;
    assert!(
        human
            .items
            .iter()
            .any(|(_, _, p)| p["text"] == long.as_str()),
        "the long reply arrives as it was"
    );

    // Restart (`codex resume`): Codex ends, the human writes meanwhile; the new connector wakes the same thread
    // before Codex called anything.
    mcp.close().await;
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Während du weg warst." }),
        )
        .await
        .expect("the human writes");
    let before = wakes(&codex_calls, thread);
    let mut mcp = seat.serve_codex(&codex, thread, Some(&program), &env).await;
    let _ = mcp.request("tools/list", json!({})).await;
    woken(&codex_calls, thread, before).await;
    inbox_holds(&mut mcp, "Während du weg warst.").await;

    // Hot swap: a new binary at the connector's path is swapped in; the session is woken as before.
    let mut changed = original.clone();
    changed.extend_from_slice(b"\nanother build");
    let new_file = bin.join(".new.trommi-connector");
    std::fs::write(&new_file, &changed).unwrap();
    std::fs::set_permissions(
        &new_file,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    std::fs::rename(&new_file, &program).unwrap();
    mcp.notification("notifications/tools/list_changed").await;
    let before = wakes(&codex_calls, thread);
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Nach dem Tausch." }),
        )
        .await
        .expect("the human writes");
    woken(&codex_calls, thread, before).await;
    inbox_holds(&mut mcp, "Nach dem Tausch.").await;
    mcp.ok("reply", json!({ "text": "Gehört." })).await;
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

/// Codex's instructions: no monitor line, no channel flag, no `say` through the shell; inbox at every turn, and a
/// restart of Codex when the tools are missing.
#[tokio::test(flavor = "multi_thread")]
async fn codex_is_told_how_it_hears_the_board_and_never_to_script_the_connector() {
    let seat = Seat::new("http://127.0.0.1:9");
    let (codex, _) = fake_codex(seat.home.path());
    let mut command = seat.command(&[]);
    command
        .env("TROMMI_CHANNEL_EVENTS", "off")
        .env("TROMMI_CODEX_BIN", &codex);
    let hello = common::process::codex_hello();
    let mut mcp = common::process::Mcp::start_as(command, hello.clone()).await;
    // (initialize once more, to read its answer)
    let again = mcp.request("initialize", hello).await;
    let instructions = again["instructions"].as_str().unwrap_or("").to_string();
    assert!(
        instructions.contains("No board events show in Codex"),
        "{instructions}"
    );
    assert!(instructions.contains("codex resume"), "{instructions}");
    for never in ["Trommi · ", "--urgent via Bash", "/mcp", "Claude Code"] {
        assert!(!instructions.contains(never), "{never:?} in {instructions}");
    }
    mcp.close().await;
}
