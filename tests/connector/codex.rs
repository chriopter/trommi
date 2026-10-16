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
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nprintf '%s\\n' \"$CODEX_HOME\" >> '{}.home'\n",
            calls.display(),
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
    let env = [
        ("TROMMI_UPDATE_POLL_MS", "1000"),
        ("TROMMI_CODEX_REWAKE_MS", "4000"),
    ];
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
        json!({ "model": "GPT-6", "task": "Codex test drive", "icon": "bell" }),
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

    // A woken turn that never read the board does not leave the session deaf: a later event wakes it once more,
    // but not before TROMMI_CODEX_REWAKE_MS, so a busy turn is not flooded with queued messages.
    let before = wakes(&codex_calls, thread);
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Erste Nachricht." }),
        )
        .await
        .expect("the human writes");
    woken(&codex_calls, thread, before).await;
    let first_wake = std::time::Instant::now();
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Zweite Nachricht." }),
        )
        .await
        .expect("the human writes");
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert_eq!(
        wakes(&codex_calls, thread),
        before + 1,
        "no second wake while the first is fresh"
    );
    tokio::time::sleep(std::time::Duration::from_millis(4500).saturating_sub(first_wake.elapsed()))
        .await;
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Dritte Nachricht." }),
        )
        .await
        .expect("the human writes");
    let again = woken(&codex_calls, thread, before + 1).await;
    assert!(again.contains("3 new board events"), "{again}");
    let said = inbox_holds(&mut mcp, "Dritte Nachricht.").await;
    assert!(
        said.contains("Erste Nachricht.") && said.contains("Zweite Nachricht."),
        "{said}"
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

    // Codex receives no channel events and caches its first tool list. A changed contract must tell
    // the agent through a normal tool result to restart Codex and reload both definitions and words.
    std::fs::write(&new_file, b"#!/bin/sh\nif [ \"$1\" = launch-abi ]; then echo '2:changed-contract'; exit 0; fi\nexit 99\n").unwrap();
    std::fs::set_permissions(&new_file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    std::fs::rename(&new_file, &program).unwrap();
    let notice = tokio::time::timeout(std::time::Duration::from_secs(15), async {
        loop {
            let said = mcp.ok("inbox", json!({})).await;
            if said.contains("reload its tools and instructions") { break said; }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }).await.expect("Codex sees the stale-contract notice in its tool result");
    assert!(notice.contains("Tell the human") && notice.contains("Quit Codex"), "{notice}");
    assert!(!notice.contains("/mcp"), "{notice}");
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
    // without Trommi's hooks in Codex (no `setup codex` here) the human never sees the terminal
    assert!(
        instructions.contains("never sees this terminal"),
        "{instructions}"
    );
    // A card's event is answered under the card (Claude Code's mirror does that by itself).
    assert!(instructions.contains("card_id"), "{instructions}");
    // No tool text tells Codex about Claude Code's /mcp.
    let listed = mcp.request("tools/list", json!({})).await;
    for tool in listed["tools"].as_array().expect("tools") {
        let text = tool["description"].as_str().unwrap_or("");
        assert!(!text.contains("/mcp"), "{}: {text}", tool["name"]);
    }
    mcp.close().await;
}

/// Codex with the hooks `setup codex` writes into its hooks.json, trusted by the human: the terminal mirror works as
/// in Claude Code. The instructions say so; the turn a queued wake started for a card puts its final text under
/// that card (the chat gets one line pointing there); his typed words are mirrored; a hook of another thread of the
/// same Codex is not this session's.
#[tokio::test(flavor = "multi_thread")]
async fn codex_with_trusted_hooks_mirrors_its_turns_and_answers_under_the_card() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let (codex, codex_calls) = fake_codex(seat.home.path());
    let thread = "01a129c9-b755-7380-8769-c46a0f75a6a1";

    // what `setup codex` writes, and the trust Codex notes once the human accepted the hooks
    let codex_home = seat.home.path().join(".codex");
    std::fs::create_dir_all(&codex_home).unwrap();
    let hooks_file = codex_home.join("hooks.json");
    let program = connector_binary().display().to_string();
    let hooks = trommi_connector::setup::with_codex_hooks(Value::Null, &program).expect("hooks");
    std::fs::write(&hooks_file, hooks.to_string()).unwrap();
    std::fs::write(
        codex_home.join("config.toml"),
        format!(
            "[hooks.state.\"{f}:user_prompt_submit:0:0\"]\ntrusted_hash = \"sha256:00\"\n\n[hooks.state.\"{f}:stop:0:0\"]\ntrusted_hash = \"sha256:00\"\n",
            f = hooks_file.display()
        ),
    )
    .unwrap();

    // (the Codex home as `setup codex` registers it: its `codex queue` must go there, not to ~/.codex)
    let home_text = codex_home.display().to_string();
    let mut mcp = seat
        .serve_codex(&codex, thread, None, &[("CODEX_HOME", &home_text)])
        .await;
    let again = mcp
        .request("initialize", common::process::codex_hello())
        .await;
    let instructions = again["instructions"].as_str().unwrap_or("").to_string();
    assert!(
        instructions.contains("reach the board by themselves"),
        "{instructions}"
    );
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), group) = tokio::join!(connect, admit);
    assert!(!failed, "{said}");
    mcp.ready().await;
    let me = human.seat(&group).await.expect("the agent");

    let made = mcp
        .ok(
            "create_decision",
            json!({ "title": "Ship it?", "options": [{ "key": "yes", "label": "Ship" }, { "key": "no", "label": "Wait" }] }),
        )
        .await;
    let card = made
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    let codex_input = |turn: &str, thread: &str, more: Value| {
        let mut all = json!({ "session_id": thread, "turn_id": turn, "transcript_path": null, "cwd": seat.folder.display().to_string(), "model": "gpt-6-luna", "permission_mode": "default" });
        all.as_object_mut()
            .unwrap()
            .extend(more.as_object().cloned().unwrap());
        all
    };

    // The human asks on the card; the session is woken, the wake starts a turn, inbox hands over the question.
    let before = wakes(&codex_calls, thread);
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["no"], "note": "Warum warten?" }),
            &["no"],
            false,
        )
        .await
        .expect("taken");
    let wake = woken(&codex_calls, thread, before).await;
    let wake_text = wake.split("--message ").nth(1).unwrap_or("").to_string();
    let homes =
        std::fs::read_to_string(format!("{}.home", codex_calls.display())).unwrap_or_default();
    assert_eq!(
        homes.lines().last(),
        Some(home_text.as_str()),
        "the wake goes to this Codex home"
    );
    seat.hook(
        "prompt",
        &codex_input(
            "t1",
            thread,
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": wake_text }),
        ),
    )
    .await;
    inbox_holds(&mut mcp, "Warum warten?").await;
    let answer = "Warten gibt dem Release einen Tag Luft.";
    seat.hook(
        "stop",
        &codex_input("t1", thread, json!({ "hook_event_name": "Stop", "stop_hook_active": false, "last_assistant_message": answer })),
    )
    .await;
    // A hook of another thread of this Codex is not this session's.
    seat.hook(
        "stop",
        &codex_input("t9", "02b239d9-c866-7480-9879-d57a1f86b7b2", json!({ "hook_event_name": "Stop", "stop_hook_active": false, "last_assistant_message": "Fremder Thread." })),
    )
    .await;
    // His typed words, and the answer of that turn, go to the chat.
    seat.hook(
        "prompt",
        &codex_input(
            "t2",
            thread,
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": "Und die Logs?" }),
        ),
    )
    .await;
    seat.hook(
        "stop",
        &codex_input("t2", thread, json!({ "hook_event_name": "Stop", "stop_hook_active": false, "last_assistant_message": "Die Logs sind sauber." })),
    )
    .await;

    let mut seen = (false, false, false, false);
    for _ in 0..600 {
        human.sync().await;
        let in_chat = |kind: &str, text: &str| {
            human
                .items
                .iter()
                .any(|(_, sender, p)| *sender == me && p["terminal"] == kind && p["text"] == text)
        };
        seen = (
            human
                .card_chats
                .iter()
                .any(|(c, p)| *c == card && p["text"] == answer),
            human.items.iter().any(|(_, sender, p)| {
                *sender == me && p["terminal"] == "answer" && p["copied_cards"] == json!([card])
            }),
            in_chat("input", "Und die Logs?"),
            in_chat("answer", "Die Logs sind sauber."),
        );
        if seen == (true, true, true, true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(seen, (true, true, true, true), "{:?}", human.findings);
    assert!(
        !human
            .items
            .iter()
            .any(|(_, _, p)| p["text"] == "Fremder Thread."),
        "another thread's hook is not mirrored"
    );
    assert!(
        !human.items.iter().any(|(_, _, p)| p["terminal"] == "input"
            && p["text"]
                .as_str()
                .is_some_and(|t| t.starts_with("[Trommi]"))),
        "the wake is not his typing"
    );
    mcp.close().await;
}

/// A stand-in for one Codex process: a process that lives as long as the test needs it, whose pid the connector
/// takes for its parent (`TROMMI_PARENT_PID`, as under the launcher). Two Codex sessions are two Codex processes;
/// in a test every connector is a child of the test itself.
fn codex_process() -> tokio::process::Child {
    tokio::process::Command::new("sleep")
        .arg("600")
        .kill_on_drop(true)
        .spawn()
        .expect("sleep runs")
}

/// The connector as Codex starts it after `trommi-connector setup codex`: Codex's `initialize`, no channel events,
/// `CODEX_HOME` of the account, and no session key (Codex names no session in the environment of its MCP servers).
async fn serve_bare_codex(
    seat: &Seat,
    codex: &Path,
    codex_home: &Path,
    parent: u32,
    thread: &str,
) -> common::process::Mcp {
    let mut command = seat.command(&[]);
    command
        .env_remove("TROMMI_SESSION_KEY")
        .env("TROMMI_CHANNEL_EVENTS", "off")
        .env("TROMMI_CODEX_BIN", codex)
        .env("TROMMI_CODEX_WAKE_MS", "300")
        .env("CODEX_HOME", codex_home)
        .env("TROMMI_PARENT_PID", parent.to_string());
    let mut mcp = common::process::Mcp::start_as(command, common::process::codex_hello()).await;
    mcp.call_meta = Some(common::process::codex_meta(thread));
    mcp
}

/// Two Claude Code sessions joined by separate explicit invites in one folder,
/// both running. Returns the seat, the first session's group and both servers.
async fn two_claude_sessions(
    human: &mut Human,
    seat: &Seat,
) -> (
    trommi_core::ids::GroupId,
    common::process::Mcp,
    common::process::Mcp,
) {
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let join = seat.join(&link);
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((joined, log), group) = tokio::join!(join, admit);
    assert!(joined, "the join failed:\n{log}");
    let mut first = seat.serve().await;
    first.ready().await;
    let mut second = seat
        .serve_with(&[
            ("TROMMI_SESSION_KEY", "test-second"),
        ])
        .await;
    let (said, failed) = second.call("reply", json!({ "text": "too early" })).await;
    assert!(failed, "{said}");
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = second.call("connect", json!({"link":link}));
    let admit = async { let device = human.admit(&mut invite).await; human.found_session(&device).await; };
    let ((said, failed), _) = tokio::join!(connect, admit);
    assert!(!failed, "{said}");
    second.ready().await;
    (group, first, second)
}

/// A new Codex session in a folder whose two keys belong to two running Claude Code sessions (the owner's case of
/// 2026-10-13): its very first Trommi call is connect with a fresh invite link, and it joins with a key of its own,
/// next to theirs. Before, connect answered "Not connected to Trommi. Call the tool again." on every try: a
/// connector that starts beside other sessions waits ("asleep") until its session is used, and connect never woke
/// it. Before a link is there, what the tools say sends the human to the one step that works: an invite, its link
/// typed into Codex, connect.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_session_connects_by_link_in_a_folder_with_other_sessions_keys() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let (_group, mut first, mut second) = two_claude_sessions(&mut human, &seat).await;
    let (codex, _calls) = fake_codex(seat.home.path());
    let codex_home = seat.home.path().join("codex-home");
    std::fs::create_dir_all(&codex_home).unwrap();
    let parent = codex_process();
    let thread = "01a129c9-b755-7380-8769-c46a0f75a6b2";
    let mut mcp = serve_bare_codex(&seat, &codex, &codex_home, parent.id().unwrap(), thread).await;
    let listed = mcp.request("tools/list", json!({})).await;
    assert!(tool_names(&listed).iter().any(|n| n == "connect"));

    // connect before any link: it says what to do, and never "call the tool again"
    let (said, failed) = mcp.call("connect", json!({})).await;
    assert!(failed, "{said}");
    assert!(!said.contains("Call the tool again"), "{said}");
    assert!(
        said.contains("command") && said.contains("Approve a computer"),
        "it asks for the link of an invite: {said}"
    );

    // the very first call with a link joins (the case of a fresh Codex: connect is its first Trommi call)
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        let group = human.found_session(&admitted).await;
        (admitted, group)
    };
    let ((said, failed), (admitted, codex_group)) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    mcp.ready().await;
    mcp.ok("reply", json!({ "text": "from Codex" })).await;
    human.sync().await;
    let codex_session = trommi_connector::util::hex(codex_group.session_id().unwrap().as_bytes());
    assert!(
        human.items.iter().any(|(s, sender, p)| *s == codex_session
            && *sender == admitted.device
            && p["text"] == "from Codex"),
        "Codex speaks in its own session"
    );
    // the Claude Code sessions keep their keys
    first
        .ok("reply", json!({ "text": "first still here" }))
        .await;
    second
        .ok("reply", json!({ "text": "second still here" }))
        .await;
    mcp.close().await;
    first.close().await;
    second.close().await;
    drop(parent);
}

/// Codex explicitly activates on the same approved computer as other programs. Each session has a
/// separate key, and inbox never joins a fresh session. A direct invite still works beside activation.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_session_activates_on_the_approved_computer_across_program_accounts() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let (_group, mut first, mut second) = two_claude_sessions(&mut human, &seat).await;
    let (codex, _calls) = fake_codex(seat.home.path());
    let codex_home = seat.home.path().join("codex-home");
    std::fs::create_dir_all(&codex_home).unwrap();
    // the asks made so far (each written once with a value, later taken back with null)
    let asks = |human: &Human| {
        human
            .items
            .iter()
            .filter(|(_, _, p)| !p["value"].is_null())
            .filter_map(|(_, _, p)| p["name"].as_str().filter(|n| n.starts_with("join_ask/")))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    };
    human.sync().await;
    let asks_before = asks(&human);

    // The first Codex: no Claude Code session vouches for it, and what it is told leads to an invite and connect.
    let parent_a = codex_process();
    let thread_a = "01a129c9-b755-7380-8769-c46a0f75a6c3";
    let mut a =
        serve_bare_codex(&seat, &codex, &codex_home, parent_a.id().unwrap(), thread_a).await;
    let mut said = String::new();
    for _ in 0..100 {
        let (text, failed) = a.call("inbox", json!({})).await;
        assert!(failed, "{text}");
        said = text;
        if !said.contains("joining by itself") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        said.contains("connect"),
        "it says how this Codex gets in: {said}"
    );
    assert!(!said.contains("Call the tool again"), "{said}");
    human.sync().await;
    assert_eq!(
        asks(&human),
        asks_before,
        "no Claude Code session asked the human for a Codex session"
    );
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = a.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), group_a) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    a.ready().await;

    let computer = seat.approve_computer(&mut human).await;
    // A second Codex explicitly activates through the approved computer.
    let parent_b = codex_process();
    let thread_b = "01a129c9-b755-7380-8769-c46a0f75a6d4";
    let mut b =
        serve_bare_codex(&seat, &codex, &codex_home, parent_b.id().unwrap(), thread_b).await;
    let (said, failed) = b.call("inbox", json!({})).await;
    assert!(failed, "{said}");
    let activation = b.call("connect", json!({}));
    let admit = human.admit_activation(&computer);
    let ((said, failed), (admitted_b, _group_b, ask)) = tokio::join!(activation, admit);
    assert!(!failed, "{said}");
    assert!(
        ask["account_label"]
            .as_str()
            .is_some_and(|l| l.eq_ignore_ascii_case("codex")),
        "{ask}"
    );
    b.ready().await;
    b.ok("reply", json!({ "text": "second Codex" })).await;
    human.sync().await;
    assert!(human
        .items
        .iter()
        .any(|(_, sender, p)| *sender == admitted_b.device && p["text"] == "second Codex"));

    // A third Codex stays off the board until given a direct invite.
    let parent_c = codex_process();
    let thread_c = "01a129c9-b755-7380-8769-c46a0f75a6e5";
    let mut c =
        serve_bare_codex(&seat, &codex, &codex_home, parent_c.id().unwrap(), thread_c).await;
    let (said, failed) = c.call("inbox", json!({})).await;
    assert!(failed, "{said}");
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = c.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await;
        admitted
    };
    let ((said, failed), admitted_c) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    c.ready().await;
    c.ok("reply", json!({ "text": "third Codex" })).await;
    // its ask is taken back
    let session_a = trommi_connector::util::hex(group_a.session_id().unwrap().as_bytes());
    let mut open = vec![];
    for _ in 0..100 {
        human.sync().await;
        let names: Vec<String> = human
            .items
            .iter()
            .filter(|(s, _, _)| *s == session_a)
            .filter_map(|(_, _, p)| p["name"].as_str().map(String::from))
            .filter(|n| n.starts_with("join_ask/"))
            .collect();
        open.clear();
        for name in names {
            if human
                .register(&group_a, &name)
                .await
                .is_some_and(|v| !v.is_null())
            {
                open.push(name);
            }
        }
        if open.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(open.is_empty(), "asks still open: {open:?}");
    human.sync().await;
    assert!(human
        .items
        .iter()
        .any(|(_, sender, p)| *sender == admitted_c.device && p["text"] == "third Codex"));
    c.close().await;
    drop(parent_c);

    // every session keeps its own key
    a.ok("reply", json!({ "text": "first Codex still here" }))
        .await;
    first
        .ok("reply", json!({ "text": "first still here" }))
        .await;
    second
        .ok("reply", json!({ "text": "second still here" }))
        .await;
    for m in [a, b, first, second] {
        m.close().await;
    }
    drop((parent_a, parent_b));
}

/// Codex as it really starts the connector: through the launcher, with no session key in the environment (Codex
/// names its thread only in the `_meta` of a tool call).
async fn serve_launched_codex(
    seat: &Seat,
    codex: &Path,
    program: &Path,
    thread: &str,
    env: &[(&str, &str)],
) -> common::process::Mcp {
    let mut command = seat.command_of(program, &[]);
    command
        .env_remove("TROMMI_LAUNCH")
        .env_remove("TROMMI_SESSION_KEY")
        .env("TROMMI_CHANNEL_EVENTS", "off")
        .env("TROMMI_CODEX_BIN", codex)
        .env("TROMMI_CODEX_WAKE_MS", "300");
    for (key, value) in env {
        command.env(key, value);
    }
    let mut mcp = common::process::Mcp::start_as(command, common::process::codex_hello()).await;
    mcp.call_meta = Some(common::process::codex_meta(thread));
    mcp
}

/// The owner's case of 2026-10-16: a Codex agent showed as gone again and again. A connector that replaced the
/// one of a running Codex session (a hot swap, or Codex restarting its app server, which starts its MCP servers
/// anew) did not know the session's thread: Codex names it only with its next Trommi call, so the new connector
/// stayed asleep, held no key, and the agent was off the board until the human wrote to it in the terminal. Now
/// the swapped-in connector takes over the thread of the one before it under the same launcher at once, and a
/// connector started after the old one ended wakes that thread, whose next call puts it back on the board.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_session_stays_on_the_board_across_a_hot_swap_and_a_restart() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let (codex, codex_calls) = fake_codex(seat.home.path());
    let thread = "01a13951-b78c-77a2-b72e-88f496583c3d";
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
    let mut mcp = serve_launched_codex(&seat, &codex, &program, thread, &env).await;
    let _ = mcp.request("tools/list", json!({})).await;
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), group) = tokio::join!(connect, admit);
    assert!(!failed && said.starts_with("Check code:"), "{said}");
    mcp.ready().await;
    let before = wakes(&codex_calls, thread);
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Vor dem Tausch." }),
        )
        .await
        .expect("the human writes");
    woken(&codex_calls, thread, before).await;
    inbox_holds(&mut mcp, "Vor dem Tausch.").await;

    // Hot swap while the session is idle: no tool call follows, yet the new connector is in the room and a board
    // event wakes the thread.
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

    // Codex restarts its app server: the connector's input ends, a new one starts for the same thread and is
    // never called. It wakes the thread, and the thread's next call brings it back with what came meanwhile.
    mcp.close().await;
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Während des Neustarts." }),
        )
        .await
        .expect("the human writes");
    let before = wakes(&codex_calls, thread);
    let mut mcp = serve_launched_codex(&seat, &codex, &program, thread, &env).await;
    let _ = mcp.request("tools/list", json!({})).await;
    let wake = woken(&codex_calls, thread, before).await;
    assert!(
        wake.contains("[Trommi]") && wake.contains("inbox"),
        "{wake}"
    );
    inbox_holds(&mut mcp, "Während des Neustarts.").await;
    // Only once: a second connector of the folder later wakes nobody.
    let after = wakes(&codex_calls, thread);
    let other = serve_launched_codex(
        &seat,
        &codex,
        &program,
        "01a13951-0000-7000-8000-000000000001",
        &env,
    )
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
    assert_eq!(wakes(&codex_calls, thread), after, "no second wake");
    other.close().await;
    mcp.ok("reply", json!({ "text": "Wieder da." })).await;
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}
