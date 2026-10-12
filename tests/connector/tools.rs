//! Every tool of the connector, called over MCP on the built binary, against the real hub: what the agent's
//! call puts on the board, and what the human's answer brings back as a channel event.
mod common;

use common::process::Seat;
use common::{HubProc, Human};
use serde_json::{json, Value};
use trommi_connector::util::hex;
use trommi_core::ids::GroupId;

/// A hub, a human, and a connector joined by link with a main session, served over MCP.
async fn seated() -> (HubProc, Human, Seat, common::process::Mcp, GroupId) {
    seated_as(None, &[]).await
}

/// The same, the server started with `env` and, given, `hello` as its client's `initialize`.
async fn seated_as(
    hello: Option<Value>,
    env: &[(&str, std::ffi::OsString)],
) -> (HubProc, Human, Seat, common::process::Mcp, GroupId) {
    seated_on(HubProc::start().await, hello, env).await
}

/// The same on a hub of the test's own.
async fn seated_on(
    hub: HubProc,
    hello: Option<Value>,
    env: &[(&str, std::ffi::OsString)],
) -> (HubProc, Human, Seat, common::process::Mcp, GroupId) {
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
    assert!(
        log.contains("check code"),
        "the six emoji were shown:\n{log}"
    );
    if std::env::var_os("TROMMI_TEST_LOG").is_some() {
        eprintln!("--- join ---\n{log}--- server ---");
    }
    let mut mcp = if hello.is_none() && env.is_empty() {
        seat.serve().await
    } else {
        let mut command = seat.command(&[]);
        for (key, value) in env {
            command.env(key, value);
        }
        let hello = hello.unwrap_or(json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }));
        common::process::Mcp::start_as(command, hello).await
    };
    mcp.ready().await;
    (hub, human, seat, mcp, group)
}

fn payloads<'a>(human: &'a Human, key: &str) -> Vec<&'a Value> {
    human
        .items
        .iter()
        .map(|(_, _, payload)| payload)
        .filter(|payload| payload.get(key).is_some())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_tools_write_to_the_board_and_answers_come_back() {
    let (_hub, mut human, _seat, mut mcp, group) = seated().await;

    // The list of tools is the one of tools.json, in its order, each with its description.
    let listed = mcp.request("tools/list", json!({})).await;
    let names: Vec<&str> = listed["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or(""))
        .collect();
    assert_eq!(names[..3], ["reply", "create_decision", "create_info"]);
    assert!(names.contains(&"reload_connector"));

    // reply
    assert!(mcp
        .ok("reply", json!({ "text": "Build is green." }))
        .await
        .starts_with("sent"));
    // introduce, set_status
    mcp.ok(
        "introduce",
        json!({ "model": "test-model", "task": "trying every tool" }),
    )
    .await;
    mcp.ok(
        "set_status",
        json!({ "id": "tests", "label": "Tests", "state": "working", "detail": "running" }),
    )
    .await;

    // create_decision
    let made = mcp
        .ok(
            "create_decision",
            json!({
                "title": "Ship it?", "body": "Everything passes.",
                "options": [{ "key": "yes", "label": "Ship" }, { "key": "no", "label": "Wait" }],
                "recommended": "yes",
            }),
        )
        .await;
    let card = made
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    assert_eq!(card.len(), 32, "{made}");

    // list_cards shows it open
    let listed: Value = serde_json::from_str(&mcp.ok("list_cards", json!({})).await).expect("JSON");
    assert_eq!(listed[0]["id"], card.as_str());
    assert_eq!(listed[0]["status"], "open");

    // revise_card, set_urgency
    let revised = mcp
        .ok(
            "revise_card",
            json!({ "card_id": card, "body": "Everything passes, twice." }),
        )
        .await;
    assert!(revised.contains("now version 2"), "{revised}");
    mcp.ok(
        "set_urgency",
        json!({ "card_id": card, "urgency": "high", "reason": "release window" }),
    )
    .await;

    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    assert!(payloads(&human, "text")
        .iter()
        .any(|p| p["text"] == "Build is green."));
    assert!(payloads(&human, "card_type")
        .iter()
        .any(|p| p["title"] == "Ship it?" && p["body"] == "Everything passes, twice."));
    // the level stands in the body of every version (the header carries only whether it knocks)
    let levels: Vec<Value> = payloads(&human, "card_type")
        .iter()
        .filter(|p| p["title"] == "Ship it?")
        .map(|p| p["urgency"].clone())
        .collect();
    assert_eq!(levels, ["normal", "normal", "high"], "{levels:?}");
    let listed: Value = serde_json::from_str(&mcp.ok("list_cards", json!({})).await).expect("JSON");
    assert_eq!(listed[0]["urgency"], "high", "{listed}");
    assert!(payloads(&human, "name")
        .iter()
        .any(|p| p["name"] == "status_line/tests"));
    assert!(payloads(&human, "name")
        .iter()
        .any(|p| p["name"] == "profile"));

    // The human answers; the agent gets the choice as a channel event and closes the card.
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["yes"] }),
            &["yes"],
            false,
        )
        .await
        .expect("the hub takes the answer");
    let event = mcp.event("decision").await;
    assert_eq!(event["meta"]["card_id"], card.as_str(), "{event}");
    assert_eq!(event["meta"]["choice"], "yes", "{event}");
    assert_eq!(
        mcp.ok(
            "close_card",
            json!({ "card_id": card, "summary": "Live: shipped." })
        )
        .await,
        "closed"
    );

    // create_info, read by the human
    let made = mcp
        .ok(
            "create_info",
            json!({ "title": "Done", "body": "Shipped at noon." }),
        )
        .await;
    let info = made
        .split_whitespace()
        .nth(1)
        .expect("the info's id")
        .to_string();
    human.sync().await;
    human
        .answer(&info, json!({ "answer_action": "read" }), &[], true)
        .await
        .expect("the hub takes the reading");
    let event = mcp.event("info_read").await;
    assert_eq!(event["meta"]["card_id"], info.as_str(), "{event}");

    // withdraw_card, merge_cards
    let one = mcp.ok("create_decision", json!({ "title": "A?", "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] })).await;
    let two = mcp.ok("create_decision", json!({ "title": "B?", "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] })).await;
    let id = |text: &str| text.split_whitespace().nth(1).expect("an id").to_string();
    let merged = mcp
        .ok(
            "merge_cards",
            json!({ "card_ids": [id(&one), id(&two)], "title": "A or B?", "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] }),
        )
        .await;
    assert!(merged.contains("replacing"), "{merged}");
    assert_eq!(
        mcp.ok(
            "withdraw_card",
            json!({ "card_id": id(&merged), "reason": "no longer needed" })
        )
        .await,
        "withdrawn"
    );

    // A message of the human arrives with his words.
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Thanks, well done." }),
        )
        .await
        .expect("the hub takes it");
    let event = mcp.event("chat").await;
    assert!(
        event["content"]
            .as_str()
            .unwrap_or("")
            .contains("Thanks, well done."),
        "{event}"
    );

    // The Desk's goals, which a human device keeps in the session for its agent, come with a tool's result.
    human
        .set_register(&group, "goals", Some(json!({ "desk_id": "d1", "desk_name": "Launch", "goals": "Ship v2\nNo regressions" })))
        .await
        .expect("the hub takes the register");
    let mut told = String::new();
    for _ in 0..600 {
        told = mcp.ok("reply", json!({ "text": "On it." })).await;
        if told.contains("Desk goals") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        told.contains("Desk goals") && told.contains("Ship v2") && told.contains("No regressions"),
        "{told}"
    );

    mcp.ok("clear_status", json!({})).await;
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn helper_sessions_and_assets() {
    let (_hub, mut human, seat, mut mcp, _group) = seated().await;

    // open_session: the agent founds the helper's group itself, with every human device.
    let opened = mcp
        .ok(
            "open_session",
            json!({ "name": "Design", "task": "drawing the logo" }),
        )
        .await;
    assert!(opened.starts_with("child session opened"), "{opened}");
    assert!(mcp
        .ok(
            "reply",
            json!({ "session": "Design", "text": "Three drafts ready." })
        )
        .await
        .starts_with("sent"));
    mcp.ok(
        "set_status",
        json!({ "session": "Design", "id": "logo", "label": "Logo", "state": "working" }),
    )
    .await;
    let made = mcp
        .ok(
            "create_decision",
            json!({ "session": "Design", "title": "Which draft?", "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] }),
        )
        .await;
    let card = made
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();

    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    let groups = human.groups().await;
    let helper = groups
        .iter()
        .find(|g| g.session.is_some_and(|s| !s.parent.is_zero()))
        .expect("the human device is a leaf of the helper session");
    let helper_sid =
        trommi_connector::util::hex(helper.session.expect("a session").session_id.as_bytes());
    assert!(human
        .items
        .iter()
        .any(|(session, _, payload)| *session == helper_sid
            && payload["text"] == "Three drafts ready."));

    // The human's answer in the helper session arrives with its name.
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["b"] }),
            &["b"],
            false,
        )
        .await
        .expect("taken");
    let event = mcp.event("decision").await;
    assert_eq!(event["meta"]["session"], "Design", "{event}");
    assert_eq!(event["meta"]["choice"], "b", "{event}");

    let again = mcp.ok("open_session", json!({ "name": "Design" })).await;
    assert!(again.starts_with("child session already open"), "{again}");
    let closed = mcp
        .ok(
            "close_session",
            json!({ "name": "Design", "summary": "Draft B it is." }),
        )
        .await;
    assert!(closed.contains("closed"), "{closed}");

    // publish_asset, list_assets, share_asset, revoke_asset
    let page = seat.folder.join("report.html");
    std::fs::write(&page, "<h1>Report</h1><p>All good.</p>").expect("a file");
    let published = mcp
        .ok(
            "publish_asset",
            json!({ "path": page.display().to_string(), "title": "Weekly report" }),
        )
        .await;
    let asset = published
        .strip_prefix("published as ")
        .and_then(|rest| rest.split(':').next())
        .expect("the asset's id")
        .to_string();
    let listed: Value =
        serde_json::from_str(&mcp.ok("list_assets", json!({})).await).expect("JSON");
    assert_eq!(listed[0]["id"], asset.as_str());
    assert_eq!(listed[0]["title"], "Weekly report");
    let shared = mcp
        .ok("share_asset", json!({ "id": asset, "expires_hours": 2 }))
        .await;
    let link = shared
        .lines()
        .find_map(|line| line.strip_prefix("Link for the recipient: "))
        .expect("a link");
    trommi_core::files::ShareLink::parse(link).expect("a Share link of the protocol");
    let too_long = mcp
        .call(
            "share_asset",
            json!({ "id": asset, "expires_hours": 181 * 24 }),
        )
        .await;
    assert!(
        too_long.1 && too_long.0.contains("180 days"),
        "{too_long:?}"
    );
    assert!(mcp
        .ok("share_asset", json!({ "id": asset, "release": false }))
        .await
        .contains("taken back"));
    assert!(mcp
        .ok("revoke_asset", json!({ "id": asset }))
        .await
        .starts_with("revoked"));
    let listed: Value =
        serde_json::from_str(&mcp.ok("list_assets", json!({})).await).expect("JSON");
    assert_eq!(listed, json!([]));

    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

/// A hub, a human, and a connector in its session, not yet served.
async fn joined() -> (HubProc, Human, Seat, GroupId) {
    let hub = HubProc::start().await;
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
    (hub, human, seat, group)
}

#[tokio::test(flavor = "multi_thread")]
async fn the_plugins_hooks_mirror_the_terminal_and_ask_for_permission() {
    let (_hub, mut human, seat, _group) = joined().await;
    let mut mcp = seat.serve_as_plugin().await;
    mcp.ready().await;
    let me = human.seat(&_group).await.expect("the agent");

    // The human types into the terminal; the agent works; its final answer ends the turn.
    let turn =
        json!({ "session_id": "s1", "prompt_id": "p1", "cwd": seat.folder.display().to_string() });
    let with = |more: Value| {
        let mut all = turn.as_object().cloned().expect("an object");
        all.extend(more.as_object().cloned().expect("an object"));
        Value::Object(all)
    };
    seat.hook(
        "prompt",
        &with(json!({ "hook_event_name": "UserPromptSubmit", "prompt": "Please run the tests." })),
    )
    .await;
    seat.hook("trail", &with(json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": "t1", "tool_input": { "command": "cargo test", "description": "Run the tests" } }))).await;
    seat.hook("trail", &with(json!({ "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_use_id": "t1", "tool_input": { "command": "cargo test" }, "tool_response": { "stdout": "ok", "exit_code": 0 } }))).await;
    seat.hook(
        "stop",
        &with(json!({ "hook_event_name": "Stop", "last_assistant_message": "All tests pass." })),
    )
    .await;

    let mut seen = (false, false, false);
    for _ in 0..600 {
        human.sync().await;
        let terminal = |kind: &str, text: &str| {
            human.items.iter().any(|(_, sender, payload)| {
                *sender == me && payload["terminal"] == kind && payload["text"] == text
            })
        };
        seen = (
            terminal("input", "Please run the tests."),
            terminal("answer", "All tests pass."),
            human
                .trail
                .iter()
                .any(|(_, number, step)| *number >= 1 && step["tool"] == "Bash"),
        );
        if seen == (true, true, true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(
        seen,
        (true, true, true),
        "prompt, answer and trail reached the board: {:?} {:?}",
        human.trail,
        human.findings
    );
    assert!(human.findings.is_empty(), "{:?}", human.findings);

    // Claude Code asks for a permission: a request on the board, the human's verdict back to Claude Code.
    mcp.notify(
        "notifications/claude/channel/permission_request",
        json!({ "request_id": "abcde", "tool_name": "Bash", "description": "Push the branch", "input_preview": "git push" }),
    )
    .await;
    let request = loop {
        human.sync().await;
        if let Some(id) = human
            .object_ids()
            .into_iter()
            .find(|id| human.has_request(id))
        {
            break id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    assert!(human
        .items
        .iter()
        .any(|(_, _, payload)| payload["tool_name"] == "Bash"
            && payload["input_preview"] == "git push"));
    human
        .verdict(&request, true)
        .await
        .expect("the verdict is taken");
    let verdict = mcp
        .notification("notifications/claude/channel/permission")
        .await;
    assert_eq!(
        verdict,
        json!({ "request_id": "abcde", "behavior": "allow" })
    );
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_answer_of_a_turn_one_card_started_goes_into_its_thread_and_a_note_turn_stays_in_the_chat(
) {
    let (_hub, mut human, seat, group) = joined().await;
    let mut mcp = seat.serve_as_plugin().await;
    mcp.ready().await;
    let me = human.seat(&group).await.expect("the agent");
    let made = mcp
        .ok(
            "create_decision",
            json!({ "title": "Ship it?", "body": "Everything passes.",
                "options": [{ "key": "yes", "label": "Ship" }, { "key": "no", "label": "Wait" }] }),
        )
        .await;
    let card = made
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    let turn = |prompt_id: &str, more: Value| {
        let mut all = json!({ "session_id": "s1", "prompt_id": prompt_id, "cwd": seat.folder.display().to_string() })
            .as_object()
            .cloned()
            .expect("an object");
        all.extend(more.as_object().cloned().expect("an object"));
        Value::Object(all)
    };

    // A chat about the card starts the turn: its final text goes into the card's thread, one line points there.
    let about = format!(
        "<channel source=\"trommi\" kind=\"chat\" card_id=\"{card}\">\nWhy wait?\n</channel>"
    );
    seat.hook(
        "prompt",
        &turn(
            "p1",
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": about }),
        ),
    )
    .await;
    let answer = "Waiting gives the release window a day of slack.";
    seat.hook(
        "stop",
        &turn(
            "p1",
            json!({ "hook_event_name": "Stop", "last_assistant_message": answer }),
        ),
    )
    .await;
    // A note starts the next turn: its final text stays in the session's chat.
    let note =
        "<channel source=\"trommi\" kind=\"chat\" note=\"1\">\nLook at the logs.\n</channel>";
    seat.hook(
        "prompt",
        &turn(
            "p2",
            json!({ "hook_event_name": "UserPromptSubmit", "prompt": note }),
        ),
    )
    .await;
    seat.hook(
        "stop",
        &turn(
            "p2",
            json!({ "hook_event_name": "Stop", "last_assistant_message": "The logs are clean." }),
        ),
    )
    .await;

    let mut seen = (false, false, false);
    for _ in 0..600 {
        human.sync().await;
        let in_chat = |text: &str| {
            human.items.iter().any(|(_, sender, p)| {
                *sender == me && p["terminal"] == "answer" && p["text"] == text
            })
        };
        seen = (
            human
                .card_chats
                .iter()
                .any(|(c, p)| *c == card && p["text"] == answer),
            human.items.iter().any(|(_, sender, p)| {
                *sender == me
                    && p["terminal"] == "answer"
                    && p["text"] == "→ Ship it?"
                    && p["copied_cards"] == json!([card])
            }),
            in_chat("The logs are clean."),
        );
        if seen == (true, true, true) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(seen, (true, true, true), "{:?}", human.findings);
    assert!(
        !human
            .items
            .iter()
            .any(|(_, _, p)| p["text"] == answer && p["terminal"] == "answer"),
        "the card's answer is not in the session's chat as well"
    );
    assert!(
        !human
            .card_chats
            .iter()
            .any(|(_, p)| p["text"] == "The logs are clean."),
        "a note's turn stays in the chat"
    );
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_connector_killed_at_work_is_the_same_member_when_it_starts_again() {
    let (_hub, mut human, seat, group) = joined().await;
    let me = human.seat(&group).await.expect("the agent");
    let mut said = Vec::new();
    for round in 0..3 {
        let mut mcp = seat.serve().await;
        mcp.ready().await;
        for n in 0..3 {
            let text = format!("round {round}, message {n}");
            mcp.ok("reply", json!({ "text": text })).await;
            said.push(text);
        }
        // One more is on its way when the process is killed: it may have reached the hub or not, but
        // whatever was signed is either sent as it was or never existed.
        let racing = format!("round {round}, the one that raced the kill");
        let call = mcp.call("reply", json!({ "text": racing }));
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(15 * (round + 1)), call).await;
        mcp.child.start_kill().expect("kill -9");
        let _ = mcp.child.wait().await;
    }
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    mcp.ok("reply", json!({ "text": "still the same member" }))
        .await;
    said.push("still the same member".into());

    human.sync().await;
    assert!(
        human.findings.is_empty(),
        "no gap, no second envelope under one number: {:?}",
        human.findings
    );
    let texts: Vec<String> = human
        .items
        .iter()
        .filter(|(_, sender, _)| *sender == me)
        .filter_map(|(_, _, payload)| payload["text"].as_str().map(String::from))
        .filter(|text| !text.contains("raced the kill"))
        .collect();
    assert_eq!(texts, said);
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn say_and_whoami_work_without_a_running_server() {
    let (_hub, mut human, seat, group) = joined().await;
    let me = human.seat(&group).await.expect("the agent");
    let run = |args: &'static [&'static str]| {
        let mut command = seat.command(args);
        async move {
            let output = command
                .stdin(std::process::Stdio::null())
                .output()
                .await
                .expect("the command runs");
            (
                output.status.success(),
                String::from_utf8_lossy(&output.stdout).into_owned(),
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )
        }
    };
    let (ok, out, err) = run(&["say", "The build machine is out of disk."]).await;
    assert!(ok, "{out}\n{err}");
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    assert!(human.items.iter().any(|(_, sender, payload)| *sender == me
        && payload["text"] == "The build machine is out of disk."));

    let (ok, out, _) = run(&["whoami"]).await;
    assert!(ok);
    let who: Value = serde_json::from_str(&out).expect("JSON");
    assert_eq!(who["room_id"], human.room.to_base64url());
    assert_eq!(who["has_key"], true);

    // A second process cannot hold the same state: the server owns it, `say` goes through its door.
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    let (ok, out, err) = run(&["say", "Said through the running connector."]).await;
    assert!(ok, "{out}\n{err}");
    human.sync().await;
    assert!(human
        .items
        .iter()
        .any(|(_, _, payload)| payload["text"] == "Said through the running connector."));
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn after_a_takeover_the_first_connector_says_that_it_stopped() {
    let (_hub, mut human, first_seat, group) = joined().await;
    let mut first = first_seat.serve().await;
    first.ready().await;
    first
        .ok("reply", json!({ "text": "from the first machine" }))
        .await;
    let old = human.seat(&group).await.expect("the first agent device");

    // The human reconnects the session on "another machine".
    let second_seat = Seat::new(&first_seat.hub_url);
    let mut invite = human.invite(group.session_id()).await;
    let link = invite.link.clone();
    let join = second_seat.join(&link);
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.take_over(&group, old, &admitted).await;
    };
    let ((joined, log), ()) = tokio::join!(join, admit);
    assert!(joined, "the second connector joins the session:\n{log}");
    let mut second = second_seat.serve().await;
    second.ready().await;
    second
        .ok("reply", json!({ "text": "from the second machine" }))
        .await;

    // The first one answers every call with a clear word instead of sending into the void.
    let mut said = String::new();
    for _ in 0..450 {
        let (text, failed) = first.call("reply", json!({ "text": "anyone?" })).await;
        if failed && (text.contains("retired") || text.contains("stopped")) {
            said = text;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        said.contains("reconnect") || said.contains("invite"),
        "the first connector says what happened and what to do: {said}"
    );
    // ... and the way it names is there: its client is told to list the tools again, and connect is offered
    assert!(said.contains("/trommi:connect"), "{said}");
    first.notification("notifications/tools/list_changed").await;
    let listed = first.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    assert!(!human
        .items
        .iter()
        .any(|(_, _, payload)| payload["text"] == "anyone?"));
    assert!(human
        .items
        .iter()
        .any(|(_, _, payload)| payload["text"] == "from the second machine"));
    // What becomes of the first machine's state depends on what it could verify. A hub that serves a removed
    // device the Commits that removed it (`/v1/groups/{group}/removal`): the device checks them itself, says
    // that it is retired, and its state is gone. A hub that only refuses it: it stopped, and its state stays.
    fn states(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .map(|path| {
                if path.extension().is_some_and(|ext| ext == "state") {
                    // (an empty journal is no state: a slot that is looked at gets one)
                    let holds = |name: &str| {
                        std::fs::metadata(path.join(name)).is_ok_and(|meta| meta.len() > 0)
                    };
                    usize::from(holds("state.log") || holds("state.snap"))
                } else if path.is_dir() {
                    states(&path)
                } else {
                    0
                }
            })
            .sum()
    }
    let kept = states(&first_seat.home.path().join("keys"));
    if said.contains("retired") {
        assert_eq!(
            kept, 0,
            "a removal the device verified wipes its state: {said}"
        );
        assert!(
            said.contains("another connector"),
            "it knows it was a takeover: {said}"
        );
    } else {
        assert_eq!(
            kept, 1,
            "a removal the hub only claims wipes nothing: {said}"
        );
        // Set where the hub under test serves the removal route: then nothing less than verified will do.
        assert!(
            std::env::var_os("TROMMI_HUB_SERVES_REMOVAL").is_none(),
            "the hub serves the removing Commits, and the connector did not verify its removal: {said}"
        );
    }
    first.close().await;
    second.close().await;
    if said.contains("retired") {
        // Nothing at all is left of the state, not even an empty directory.
        fn state_dirs(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .flat_map(|path| {
                    if path.extension().is_some_and(|ext| ext == "state") {
                        vec![path]
                    } else if path.is_dir() {
                        state_dirs(&path)
                    } else {
                        vec![]
                    }
                })
                .collect()
        }
        let left = state_dirs(&first_seat.home.path().join("keys"));
        assert!(
            left.is_empty(),
            "a wiped slot leaves no state directory: {left:?}"
        );
    }
}

/// A host without channels and without the plugin's monitor (plain MCP: Codex, or `claude mcp add`): the tool
/// list has `inbox`, and what the human writes waits there until the agent asks.
#[tokio::test(flavor = "multi_thread")]
async fn without_channel_and_monitor_board_events_wait_for_the_inbox_tool() {
    let (_hub, mut human, seat, group) = joined().await;
    let mut mcp = seat.serve_plain().await;
    mcp.ready().await;
    let listed = mcp.request("tools/list", json!({})).await;
    assert!(
        listed["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .any(|tool| tool["name"] == "inbox"),
        "a host that hears no channel is given the inbox tool"
    );
    mcp.ok("reply", json!({ "text": "I am here." })).await;
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Please look at the plain path." }),
        )
        .await
        .expect("the human writes");
    let mut said = String::new();
    for _ in 0..450 {
        said = mcp.ok("inbox", json!({})).await;
        if said.contains("Please look at the plain path.") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        said.contains("Please look at the plain path."),
        "the human's words come out of the inbox: {said}"
    );
    // read once: the next call has nothing new
    let again = mcp.ok("inbox", json!({})).await;
    assert!(
        !again.contains("Please look at the plain path."),
        "an event is handed out once: {again}"
    );
    // (the channel notification still goes out as well: a host that does not show it drops it, and one that
    // was taken for deaf by mistake still hears it)
    mcp.close().await;
}

/// A helper reads its child session's events itself with `inbox(session)`, which claims the session: the
/// coordinator's inbox and the channel skip them, it hears of a "Done: …" card in one line, and the events come back
/// to it with a notice when the helper stops reading or the session is closed.
#[tokio::test(flavor = "multi_thread")]
async fn a_helper_claims_its_session_and_reads_its_own_events() {
    let (_hub, mut human, seat, _group) = joined().await;
    let mut mcp = seat.serve_plain_with(&[("TROMMI_CLAIM_MS", "8000")]).await;
    mcp.ready().await;
    mcp.ok("open_session", json!({ "name": "Design", "task": "logo" }))
        .await;
    let card = |made: String| {
        made.split_whitespace()
            .nth(1)
            .expect("the card's id")
            .to_string()
    };
    let ask = |title: &str| json!({ "session": "Design", "title": title, "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] });
    let first = card(mcp.ok("create_decision", ask("First?")).await);
    let second = card(mcp.ok("create_decision", ask("Second?")).await);

    // The helper's first look claims the session.
    let said = mcp.ok("inbox", json!({ "session": "design" })).await;
    assert_eq!(said, "No new board events in design.");

    human
        .answer(
            &first,
            json!({ "answer_action": "answer", "choices": ["a"] }),
            &["a"],
            false,
        )
        .await
        .expect("taken");
    let mut helper = String::new();
    for _ in 0..150 {
        helper = mcp.ok("inbox", json!({ "session": "Design" })).await;
        if helper.contains(&first) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        helper.contains(&first) && helper.contains("choice=\"a\""),
        "the helper reads its answer: {helper}"
    );
    let coordinator = mcp.ok("inbox", json!({})).await;
    assert!(
        !coordinator.contains(&first),
        "the coordinator does not see it: {coordinator}"
    );
    let sent = |mcp: &common::process::Mcp, id: &str| {
        mcp.notifications
            .iter()
            .any(|(_, p)| p["meta"]["card_id"].as_str() == Some(id))
    };
    assert!(
        !sent(&mcp, &first),
        "no channel event for a claimed session"
    );

    // A "Done: …" card of the helper: one line for the coordinator.
    mcp.ok(
        "create_info",
        json!({ "session": "Design", "title": "Done: logo drawn", "body": "Three drafts." }),
    )
    .await;
    let coordinator = mcp.ok("inbox", json!({})).await;
    assert!(
        coordinator.contains("kind=\"helper\"") && coordinator.contains("Design: filed"),
        "{coordinator}"
    );
    let line = trommi_connector::line::monitor_line(
        &json!({ "meta": { "kind": "helper", "helper": "Design", "note": "filed \"Done: logo drawn\"" } }),
    )
    .expect("a line");
    assert_eq!(
        line,
        "Trommi · Design · filed 'Done: logo drawn' · call inbox"
    );

    // The helper stops reading: the claim lapses, and the waiting answer goes to the coordinator with a notice.
    human
        .answer(
            &second,
            json!({ "answer_action": "answer", "choices": ["b"] }),
            &["b"],
            false,
        )
        .await
        .expect("taken");
    let decided = |out: &str, id: &str| {
        serde_json::from_str::<Value>(out)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .any(|c| c["id"] == id && c["status"] == "decided")
    };
    for _ in 0..150 {
        if decided(
            &mcp.ok("list_cards", json!({ "session": "Design" })).await,
            &second,
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let coordinator = mcp.ok("inbox", json!({})).await;
    assert!(
        !coordinator.contains(&second),
        "still the helper's: {coordinator}"
    );
    let mut coordinator = String::new();
    for _ in 0..150 {
        coordinator.push_str(&mcp.ok("inbox", json!({})).await);
        if coordinator.contains(&second) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        coordinator.contains("Design: helper gone, 1 event waiting")
            && coordinator.contains(&second),
        "{coordinator}"
    );

    // Claimed again, then closed: what waits goes to the coordinator.
    mcp.ok("inbox", json!({ "session": "Design" })).await;
    let third = card(mcp.ok("create_decision", ask("Third?")).await);
    human
        .answer(
            &third,
            json!({ "answer_action": "answer", "choices": ["a"] }),
            &["a"],
            false,
        )
        .await
        .expect("taken");
    for _ in 0..150 {
        if decided(
            &mcp.ok("list_cards", json!({ "session": "Design" })).await,
            &third,
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        !sent(&mcp, &third),
        "no channel event for a claimed session"
    );
    mcp.ok(
        "close_session",
        json!({ "name": "Design", "summary": "Done." }),
    )
    .await;
    let coordinator = mcp.ok("inbox", json!({})).await;
    assert!(
        coordinator.contains("Design: session closed, 1 event waiting")
            && coordinator.contains(&third),
        "{coordinator}"
    );
    mcp.close().await;
}

/// The session id in open_session's answer: `…: "<name>" (<id>), …`.
fn opened_sid(answer: &str) -> String {
    answer
        .split_once(" (")
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(sid, _)| sid.to_string())
        .unwrap_or_else(|| panic!("no session id in {answer}"))
}

/// What the owner met: closed helper sessions went on counting at the hub and in the core (the board only hid
/// them), until open_session was refused `too-many` with four on view. A closed one is archived now and frees
/// its place; one with an open question stays live until it is answered; one a human archived is no longer
/// "already open"; a closed name opens a new session. (The hub's limit is lowered to 3 here.)
#[tokio::test(flavor = "multi_thread")]
async fn closed_helper_sessions_are_archived_and_free_their_place() {
    let hub = HubProc::start_with(&[("HUB_LIMIT_HELPERS_PER_MAIN", "3")]).await;
    let (_hub, mut human, _seat, mut mcp, _group) = seated_on(hub, None, &[]).await;
    let mut sids = std::collections::BTreeMap::new();
    for name in ["A", "B", "C"] {
        let opened = mcp.ok("open_session", json!({ "name": name })).await;
        assert!(opened.starts_with("child session opened"), "{opened}");
        sids.insert(name, opened_sid(&opened));
    }
    let (refused, failed) = mcp.call("open_session", json!({ "name": "D" })).await;
    assert!(
        failed && refused.contains("live helper sessions"),
        "{refused}"
    );

    // closed without an open question: archived, its place free
    let closed = mcp.ok("close_session", json!({ "name": "A" })).await;
    assert!(closed.contains("closed and archived"), "{closed}");
    let opened = mcp.ok("open_session", json!({ "name": "D" })).await;
    assert!(opened.starts_with("child session opened"), "{opened}");
    sids.insert("D", opened_sid(&opened));

    // closed with an open question: live (and counted) until it is answered
    let made = mcp
        .ok(
            "create_decision",
            json!({ "session": "B", "title": "Which?", "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] }),
        )
        .await;
    let card = made
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    let closed = mcp.ok("close_session", json!({ "name": "B" })).await;
    assert!(closed.contains("1 open question of it stays"), "{closed}");
    let (refused, failed) = mcp.call("open_session", json!({ "name": "E" })).await;
    assert!(
        failed && refused.contains("live helper sessions"),
        "{refused}"
    );
    human.sync().await;
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["b"] }),
            &["b"],
            false,
        )
        .await
        .expect("taken");
    mcp.event("decision").await;
    let opened = mcp.ok("open_session", json!({ "name": "E" })).await;
    assert!(opened.starts_with("child session opened"), "{opened}");

    // a session a human archived is over for the connector too: the same name opens a new one
    human.sync().await;
    let c = human
        .groups()
        .await
        .into_iter()
        .find(|g| {
            g.session
                .is_some_and(|s| hex(s.session_id.as_bytes()) == sids["C"])
        })
        .expect("the human device is a leaf of C")
        .group;
    human.archive(&c).await;
    // (it hears it from the hub's stream; until then the name still finds the live C)
    let mut reopened = String::new();
    for _ in 0..300 {
        reopened = mcp.ok("open_session", json!({ "name": "C" })).await;
        if !reopened.starts_with("child session already open") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(reopened.starts_with("child session opened"), "{reopened}");
    assert_ne!(opened_sid(&reopened), sids["C"]);

    // a closed name opens a new session, not the archived one
    let closed = mcp.ok("close_session", json!({ "name": "D" })).await;
    assert!(closed.contains("closed and archived"), "{closed}");
    let opened = mcp.ok("open_session", json!({ "name": "A" })).await;
    assert!(opened.starts_with("child session opened"), "{opened}");
    assert_ne!(opened_sid(&opened), sids["A"]);
    let (refused, failed) = mcp.call("open_session", json!({ "name": "F" })).await;
    assert!(
        failed && refused.contains("live helper sessions"),
        "{refused}"
    );
    mcp.close().await;
}

/// A link past its deadline is refused by the connector itself, at once and with a plain reason, before a slot
/// is made or the hub is asked.
#[tokio::test(flavor = "multi_thread")]
async fn an_expired_link_is_refused_before_anything_is_asked() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    // an agent invite lives 15 minutes, and two more are tolerated
    let link = human.expired_link(20 * 60 * 1000).await;
    // (the connector is built before the clock starts)
    common::process::connector_binary();
    let started = std::time::Instant::now();
    let (joined, log) = seat.join(&link).await;
    assert!(!joined, "{log}");
    assert!(log.contains("expired"), "{log}");
    assert_eq!(log.matches("make a new").count(), 1, "{log}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "refused without waiting: {log}"
    );
    let keys = seat.home.path().join("keys");
    let left: Vec<_> = std::fs::read_dir(&keys)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert!(left.is_empty(), "nothing was written: {left:?}");
}

/// A restart whose old process has not ended: the new process takes the lease, and its stream (which carries
/// the lease) replaces the old one's at the hub. The old process is answered `lease-lost` and ends without
/// asking again; only the new one hears the human.
#[tokio::test(flavor = "multi_thread")]
async fn of_two_processes_of_one_device_only_the_lease_holder_streams() {
    let (_hub, mut human, seat, group) = joined().await;
    let signal = |what: &str, pid: u32| {
        let sent = std::process::Command::new("kill")
            .args([what, &pid.to_string()])
            .status()
            .expect("kill runs");
        assert!(sent.success());
    };
    let mut old = seat.serve().await;
    old.ready().await;
    let pid = old.child.id().expect("the old process");
    // the old process stands still while its state is copied, and while the new one starts
    signal("-STOP", pid);
    let twin = seat.twin(pid);
    let mut new = twin.serve().await;
    new.ready().await;
    signal("-CONT", pid);

    // the old process ends by itself, soon, and does not come back
    let ended = tokio::time::timeout(std::time::Duration::from_secs(60), old.child.wait()).await;
    assert!(
        ended.is_ok(),
        "the process that lost the lease ended (no reconnect loop)"
    );

    // the new one hears the human
    human
        .say(
            &group,
            json!({ "content_type": "message", "text": "Only the new one hears this." }),
        )
        .await
        .expect("the human writes");
    let heard = new.event("chat").await;
    assert!(
        heard.to_string().contains("Only the new one hears this."),
        "{heard}"
    );
    // and signs and sends: its state is the device's, nothing halted it
    let sent = new
        .ok("reply", json!({ "text": "The new one answers." }))
        .await;
    assert!(sent.starts_with("sent"), "{sent}");
    new.close().await;
}

fn tool_names(listed: &Value) -> Vec<String> {
    listed["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or("").to_string())
        .collect()
}

/// The join ends between two looks of the connect call (one look every 3 s here): the human's app confirmed before
/// the call saw the six emoji. The call still answers with them, and a second call says connected already.
#[tokio::test(flavor = "multi_thread")]
async fn a_join_confirmed_between_two_looks_still_answers_with_the_check_code() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut mcp = seat.serve_with(&[("TROMMI_CONNECT_POLL_MS", "3000")]).await;
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
    mcp.notification("notifications/tools/list_changed").await;
    let (said, failed) = mcp
        .call(
            "connect",
            json!({ "link": "https://example.invalid/join#x" }),
        )
        .await;
    assert!(
        failed && said.contains("connected to Trommi already"),
        "{said}"
    );
    mcp.close().await;
}

/// The connect tool's answer does not depend on how fast the human confirms: a human whose app admits the newcomer
/// at once (the join may end before the call looks at it) still gets the six emoji first, and a second call once the
/// board tools are there says connected already. Stress: 200 joins in a row against one hub (TROMMI_STRESS_RUNS
/// sets another count). Run by hand: `cargo test -p trommi-tests --test connector_tools -- --ignored connect_stress`.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "stress, by hand: 200 joins"]
async fn connect_stress_the_check_code_comes_first_however_fast_the_human_confirms() {
    let runs: usize = std::env::var("TROMMI_STRESS_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    for run in 0..runs {
        let seat = Seat::new(&hub.url);
        let mut mcp = seat.serve().await;
        let listed = mcp.request("tools/list", json!({})).await;
        assert_eq!(tool_names(&listed), ["connect"], "run {run}: {listed}");
        let mut invite = human.invite(None).await;
        let link = invite.link.clone();
        let connect = mcp.call("connect", json!({ "link": link }));
        let admit = async {
            let admitted = human.admit(&mut invite).await;
            human.found_session(&admitted).await
        };
        let ((said, failed), _group) = tokio::join!(connect, admit);
        assert!(
            !failed && said.starts_with("Check code:"),
            "run {run}: {said}"
        );
        mcp.notification("notifications/tools/list_changed").await;
        let (said, failed) = mcp
            .call(
                "connect",
                json!({ "link": "https://example.invalid/join#x" }),
            )
            .await;
        assert!(
            failed && said.contains("connected to Trommi already"),
            "run {run}: {said}"
        );
        mcp.close().await;
    }
}

/// A session whose folder is in no room starts unconnected and offers the connect tool alone. A bad or expired
/// link is a tool error; a good one answers with the six emoji, and once the human confirmed them the same
/// process goes online, says its tools changed, and lists the board tools. A restart after that ignores keys
/// of an earlier format and, with keys of two rooms, takes the room the folder joined; without that note the
/// error names both rooms.
#[tokio::test(flavor = "multi_thread")]
async fn an_unconnected_session_connects_by_the_connect_tool_without_a_restart() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut mcp = seat.serve().await;

    let listed = mcp.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");
    let (said, failed) = mcp.call("reply", json!({ "text": "hello" })).await;
    assert!(failed && said.contains("/trommi:connect"), "{said}");

    let (said, failed) = mcp.call("connect", json!({ "link": "not a link" })).await;
    assert!(failed && said.starts_with("not connected"), "{said}");
    let expired = human.expired_link(20 * 60 * 1000).await;
    let (said, failed) = mcp.call("connect", json!({ "link": expired })).await;
    assert!(failed && said.contains("expired"), "{said}");

    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let ((said, failed), _group) = tokio::join!(connect, admit);
    assert!(!failed, "{said}");
    assert!(said.starts_with("Check code:"), "{said}");
    let code: Vec<&str> = said.lines().skip(1).take_while(|l| !l.is_empty()).collect();
    assert_eq!(code.len(), 6, "{said}");
    assert!(
        code.iter().all(|l| l.split_whitespace().count() >= 2),
        "{said}"
    );
    assert!(said.contains("They match"), "{said}");

    mcp.notification("notifications/tools/list_changed").await;
    let listed = mcp.request("tools/list", json!({})).await;
    let names = tool_names(&listed);
    assert!(
        names.iter().any(|n| n == "reply") && names.iter().any(|n| n == "create_decision"),
        "{names:?}"
    );
    assert!(!names.iter().any(|n| n == "connect"), "{names:?}");
    mcp.ok("list_cards", json!({})).await;
    let (said, failed) = mcp
        .call(
            "connect",
            json!({ "link": "https://example.invalid/join#x" }),
        )
        .await;
    assert!(
        failed && said.contains("connected to Trommi already"),
        "{said}"
    );
    mcp.close().await;

    // The room this folder joined is noted in the folder.
    let noted = std::fs::read_to_string(seat.folder.join(".trommi/room"))
        .expect("the joined room is noted");
    let room = noted.trim().to_string();
    let base =
        std::fs::read_to_string(seat.folder.join(".trommi/slot-base")).expect("the slot base");
    let keys = seat.home.path().join("keys");
    // a room of an earlier format (a hex id) with a key of this folder, and a second room of this protocol
    let old = keys.join("0123456789abcdef0123456789abcdef");
    std::fs::create_dir_all(&old).expect("a folder");
    std::fs::write(old.join(format!("{}-1.key", base.trim())), "{\"old\":true}").expect("written");
    let other = trommi_core::ids::RoomId::new([7; trommi_core::ids::RoomId::LEN]).to_base64url();
    std::fs::create_dir_all(keys.join(&other)).expect("a folder");
    std::fs::write(
        keys.join(&other).join(format!("{}-1.key", base.trim())),
        trommi_connector::slotstore::KEY_MARKER,
    )
    .expect("written");

    let mut mcp = seat.serve().await;
    mcp.ready().await;
    mcp.close().await;

    std::fs::remove_file(seat.folder.join(".trommi/room")).expect("removed");
    let output = seat
        .command(&["whoami"])
        .output()
        .await
        .expect("whoami runs");
    let told = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{told}");
    let error = told
        .lines()
        .find(|l| l.contains("Trommi rooms"))
        .unwrap_or_default();
    assert!(
        error.contains("2 Trommi rooms") && error.contains(&room) && error.contains(&other),
        "{told}"
    );
    assert!(!error.contains("0123456789abcdef"), "{told}");
    assert!(
        told.lines()
            .any(|l| l.contains("ignored") && l.contains("0123456789abcdef")),
        "{told}"
    );
}

// ---- the account and usage of the program (connector/src/usage.rs) -----------------------------------------------

fn script(path: &std::path::Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).expect("written");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("executable");
}

/// The account in the main session's profile, once the human has it.
async fn account_on_the_board(human: &mut Human, group: &GroupId) -> Value {
    for _ in 0..120 {
        human.sync().await;
        if let Some(account) = human
            .register(group, "profile")
            .await
            .and_then(|p| p.get("account").cloned())
        {
            return account;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
    panic!("no account in the profile");
}

#[tokio::test(flavor = "multi_thread")]
async fn claude_codes_plan_and_windows_reach_the_board_in_the_profile() {
    let dir = common::TempDir::new("usage-claude");
    let claude = dir.path().join("claude");
    script(&claude, "#!/bin/sh\n[ \"$*\" = 'auth status --json' ] || exit 2\necho '{\"loggedIn\":true,\"authMethod\":\"claude.ai\",\"email\":\"ada@example.org\",\"subscriptionType\":\"max\"}'\n");
    let env = [
        ("TROMMI_USAGE", std::ffi::OsString::from("on")),
        ("TROMMI_USAGE_POLL_MS", "1000".into()),
        ("TROMMI_CLAUDE_BIN", claude.into_os_string()),
    ];
    let (_hub, mut human, seat, mut mcp, group) = seated_as(None, &env).await;
    // Claude Code's status line hands over the windows (rate_limits, resets in seconds)
    let mut line = seat.command(&["statusline"]);
    let mut child = line
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("runs");
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(br#"{"rate_limits":{"five_hour":{"used_percentage":62,"resets_at":4102444800},"seven_day":{"used_percentage":84,"resets_at":4102531200}}}"#).await.unwrap();
    }
    assert!(child.wait().await.unwrap().success());
    mcp.ok(
        "introduce",
        json!({ "model": "test-model", "task": "counting" }),
    )
    .await;
    let mut account = account_on_the_board(&mut human, &group).await;
    for _ in 0..40 {
        if account.get("weekly").is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        account = account_on_the_board(&mut human, &group).await;
    }
    assert_eq!(
        account,
        json!({ "kind": "claude", "plan": "max", "email": "ada@example.org",
        "five_hour": { "used": 62, "resets_at": 4102444800000u64 }, "weekly": { "used": 84, "resets_at": 4102531200000u64 } })
    );
    // the rest of the profile stays: the account is written beside it, never over it
    let profile = human.register(&group, "profile").await.unwrap();
    assert_eq!(profile["model"], "test-model");
    assert_eq!(profile["task"], "counting");
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn codexs_account_comes_from_its_app_server() {
    let dir = common::TempDir::new("usage-codex");
    let codex = dir.path().join("codex");
    script(
        &codex,
        r#"#!/bin/sh
[ "$1" = app-server ] || exit 0
while IFS= read -r line; do
  case "$line" in
    *'"id":2'*) echo '{"id":2,"result":{"account":{"type":"chatgpt","email":"ada@work.example","planType":"plus"}}}' ;;
    *'"id":3'*) echo '{"id":3,"result":{"rateLimits":{"primary":{"usedPercent":41,"windowDurationMins":10080,"resetsAt":4102444800},"secondary":null,"credits":{"hasCredits":true,"unlimited":false,"balance":"12.5"}},"rateLimitResetCredits":{"availableCount":2}}}' ;;
  esac
done
"#,
    );
    let env = [
        ("TROMMI_USAGE", std::ffi::OsString::from("on")),
        ("TROMMI_USAGE_POLL_MS", "1000".into()),
        ("TROMMI_CHANNEL_EVENTS", "off".into()),
        ("TROMMI_CODEX_BIN", codex.into_os_string()),
    ];
    let (_hub, mut human, _seat, mcp, group) =
        seated_as(Some(common::process::codex_hello()), &env).await;
    let account = account_on_the_board(&mut human, &group).await;
    assert_eq!(
        account,
        json!({ "kind": "codex", "plan": "plus", "email": "ada@work.example",
        "weekly": { "used": 41, "resets_at": 4102444800000u64 }, "credits": 12.5, "full_resets": 2 })
    );
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

/// A status line follows its question: filing a card links the session's line to it (decision), a line that
/// says in words that it waits on the human is linked too, and the human's answer turns every line on the card
/// back to working with what he chose. A session without a line gets one of the connector's own for the card,
/// which goes with close_card.
#[tokio::test(flavor = "multi_thread")]
async fn a_status_line_follows_its_question_to_the_answer() {
    let (_hub, mut human, _seat, mut mcp, group) = seated().await;
    let id = |made: &str| made.split_whitespace().nth(1).expect("an id").to_string();
    let ask = |session: Option<&str>, title: &str| {
        let mut a = json!({ "title": title, "options": [{ "key": "a", "label": "Ring" }, { "key": "b", "label": "Kreis" }] });
        if let Some(s) = session {
            a["session"] = json!(s);
        }
        a
    };

    // In a helper's session: its one working line waits on the card it files.
    mcp.ok(
        "open_session",
        json!({ "name": "Design", "task": "UNO tile" }),
    )
    .await;
    mcp.ok(
        "set_status",
        json!({ "session": "Design", "id": "uno", "label": "UNO-Kachel", "state": "working", "detail": "baut Vorschläge" }),
    )
    .await;
    let card = id(&mcp
        .ok("create_decision", ask(Some("Design"), "Ring oder Kreis?"))
        .await);
    // The helper then says it waits, as working: the connector links it to the open question all the same.
    let said = mcp
        .ok(
            "set_status",
            json!({ "session": "Design", "id": "uno", "state": "working", "detail": "2 Vorschläge, wartet auf deine Wahl" }),
        )
        .await;
    assert!(
        said.contains("is decision") && said.contains(&card),
        "{said}"
    );
    human.sync().await;
    let helper = human
        .groups()
        .await
        .into_iter()
        .find(|g| g.session.is_some_and(|s| !s.parent.is_zero()))
        .expect("the helper's session")
        .group;
    let line = human
        .register(&helper, "status_line/uno")
        .await
        .expect("the line");
    assert_eq!(line["state"], "decision", "{line}");
    assert_eq!(line["object_id"], card.as_str(), "{line}");

    // The human answers: the line is working again and says what he chose.
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["b"] }),
            &["b"],
            false,
        )
        .await
        .expect("taken");
    let mut line = Value::Null;
    for _ in 0..300 {
        human.sync().await;
        line = human
            .register(&helper, "status_line/uno")
            .await
            .unwrap_or(Value::Null);
        if line["state"] == "working" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(line["state"], "working", "{line}");
    assert_eq!(line["detail"], "Antwort da: Kreis", "{line}");
    assert_eq!(line["label"], "UNO-Kachel", "{line}");

    // In the main session with no line: the connector makes one for the card; the answer turns it to working,
    // and close_card takes it away.
    let card = id(&mcp.ok("create_decision", ask(None, "Ship it?")).await);
    let own = format!("status_line/q-{}", &card[..8]);
    human.sync().await;
    let line = human
        .register(&group, &own)
        .await
        .expect("the connector's line");
    assert_eq!(line["state"], "decision", "{line}");
    assert_eq!(line["object_id"], card.as_str(), "{line}");
    assert_eq!(line["label"], "Ship it?", "{line}");
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["a"] }),
            &["a"],
            false,
        )
        .await
        .expect("taken");
    let mut line = Value::Null;
    for _ in 0..300 {
        human.sync().await;
        line = human.register(&group, &own).await.unwrap_or(Value::Null);
        if line["state"] == "working" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(line["detail"], "Antwort da: Ring", "{line}");
    mcp.event("decision").await;
    mcp.ok(
        "close_card",
        json!({ "card_id": card, "summary": "Live: shipped." }),
    )
    .await;
    human.sync().await;
    assert_eq!(human.register(&group, &own).await, None);

    // A line the agent sets for the card itself replaces the connector's own one.
    let card = id(&mcp.ok("create_decision", ask(None, "Merge now?")).await);
    let own = format!("status_line/q-{}", &card[..8]);
    mcp.ok(
        "set_status",
        json!({ "id": "merge", "label": "Merge", "state": "decision", "card_id": card }),
    )
    .await;
    human.sync().await;
    assert_eq!(human.register(&group, &own).await, None);
    assert_eq!(
        human
            .register(&group, "status_line/merge")
            .await
            .expect("the line")["object_id"],
        card.as_str()
    );
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

/// `start_task` files the quiet "Working on" card and the status line linked to it in one step; the line keeps that
/// link through updates and a question in the session (which gets a line of its own), and turns done when the card
/// is revised to "Done: …".
#[tokio::test(flavor = "multi_thread")]
async fn start_task_files_its_card_and_line_and_done_flips_the_line() {
    let (_hub, mut human, _seat, mut mcp, _group) = seated().await;
    mcp.ok(
        "open_session",
        json!({ "name": "Release", "task": "Migration" }),
    )
    .await;
    let said = mcp
        .ok(
            "start_task",
            json!({ "session": "Release", "title": "Migration der Tabelle orders", "detail": "testet auf Staging" }),
        )
        .await;
    let card = said
        .split_whitespace()
        .nth(1)
        .expect("the card's id")
        .to_string();
    assert_eq!(card.len(), 32, "{said}");
    let line_id = "migration-der-tabelle-orders";
    assert!(said.contains(&format!("status \"{line_id}\"")), "{said}");
    let listed: Value =
        serde_json::from_str(&mcp.ok("list_cards", json!({ "session": "Release" })).await)
            .expect("JSON");
    assert_eq!(listed[0]["id"], card.as_str(), "{listed}");
    assert_eq!(
        listed[0]["title"], "Working on: Migration der Tabelle orders",
        "{listed}"
    );
    assert_eq!(listed[0]["urgency"], "low", "{listed}");
    assert_eq!(
        listed[0]["working"], true,
        "off his Desk while worked on: {listed}"
    );
    assert!(
        said.contains("not on his Desk"),
        "the agent is told where it is: {said}"
    );

    human.sync().await;
    let helper = human
        .groups()
        .await
        .into_iter()
        .find(|g| g.session.is_some_and(|s| !s.parent.is_zero()))
        .expect("the helper's session")
        .group;
    let key = format!("status_line/{line_id}");
    let line = human.register(&helper, &key).await.expect("the line");
    assert_eq!(line["state"], "working", "{line}");
    assert_eq!(line["object_id"], card.as_str(), "{line}");
    assert_eq!(line["detail"], "testet auf Staging", "{line}");

    // An update without card_id keeps the link; a question gets a line of its own.
    mcp.ok(
        "set_status",
        json!({ "session": "Release", "id": line_id, "state": "working", "detail": "läuft auf Produktion" }),
    )
    .await;
    let asked = mcp
        .ok(
            "create_decision",
            json!({ "session": "Release", "title": "Jetzt ausrollen?", "options": [{ "key": "a", "label": "Jetzt" }, { "key": "b", "label": "Später" }] }),
        )
        .await;
    let question = asked.split_whitespace().nth(1).expect("an id").to_string();
    human.sync().await;
    let line = human.register(&helper, &key).await.expect("the line");
    assert_eq!(line["state"], "working", "{line}");
    assert_eq!(line["object_id"], card.as_str(), "{line}");
    assert_eq!(line["detail"], "läuft auf Produktion", "{line}");
    let own = human
        .register(&helper, &format!("status_line/q-{}", &question[..8]))
        .await
        .expect("the question's own line");
    assert_eq!(own["state"], "decision", "{own}");

    // "Working on" → "Done": the line turns done.
    let revised = mcp
        .ok(
            "revise_card",
            json!({ "card_id": card, "title": "Done: Migration der Tabelle orders", "body": "38 Sekunden Sperre." }),
        )
        .await;
    assert!(
        revised.contains(&format!("status \"{line_id}\" done")),
        "{revised}"
    );
    human.sync().await;
    let line = human.register(&helper, &key).await.expect("the line");
    assert_eq!(line["state"], "done", "{line}");
    assert_eq!(line["object_id"], card.as_str(), "{line}");
    assert_eq!(
        line["detail"], "Done: Migration der Tabelle orders",
        "{line}"
    );
    let listed: Value =
        serde_json::from_str(&mcp.ok("list_cards", json!({ "session": "Release" })).await)
            .expect("JSON");
    let done = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == card.as_str())
        .expect("the card");
    assert_eq!(done["working"], Value::Null, "done: on his Desk: {done}");
    // What his devices read: version 1 working, the Done version without it.
    let versions: Vec<&Value> = payloads(&human, "card_type")
        .into_iter()
        .filter(|p| {
            p["title"]
                .as_str()
                .is_some_and(|t| t.ends_with("Migration der Tabelle orders"))
        })
        .collect();
    assert!(
        versions.iter().any(|p| p["working"] == true && p["title"].as_str().unwrap().starts_with("Working on")),
        "{versions:?}"
    );
    assert!(
        versions.iter().any(|p| p.get("working").is_none() && p["title"].as_str().unwrap().starts_with("Done")),
        "{versions:?}"
    );

    // A title that already says it ("Arbeite an: …") is kept, the id is given.
    let said = mcp
        .ok(
            "start_task",
            json!({ "title": "Arbeite an: Login-Seite", "id": "login" }),
        )
        .await;
    assert!(
        said.contains("\"Arbeite an: Login-Seite\"") && said.contains("status \"login\""),
        "{said}"
    );
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}

/// Two agent sessions in one folder are two members: the key a terminal's `join` made is adopted by the first
/// session that uses it, and a second session started in the same folder never takes it, also not once it is
/// free, never gets it handed over, and cannot `say` through it. Only the first session again (a resume) is
/// that member.
#[tokio::test(flavor = "multi_thread")]
async fn two_sessions_in_one_folder_never_share_a_key() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    // joined in a terminal, outside any agent session: the key belongs to no session yet
    let mut command = seat.command(&["join", &link]);
    command.env_remove("TROMMI_SESSION_KEY");
    let join = async move {
        command
            .stdin(std::process::Stdio::null())
            .output()
            .await
            .expect("the join runs")
    };
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.found_session(&admitted).await
    };
    let (joined, group) = tokio::join!(join, admit);
    assert!(
        joined.status.success(),
        "{}",
        String::from_utf8_lossy(&joined.stderr)
    );
    let me = human.seat(&group).await.expect("the agent");

    // (idle after a millisecond: an idle holder hands its key over, but only to its own session)
    let mut first = seat
        .serve_with(&[("TROMMI_UNUSED_MS", "1"), ("TROMMI_IDLE_MS", "1")])
        .await;
    first.ready().await;
    first
        .ok("reply", json!({ "text": "from the first session" }))
        .await;

    // A second Claude Code session in the same folder, while the first one runs, idle (self-join off: what is
    // tested here is that it never gets the first one's key; a_second_session_joins_by_itself_vouched_by_the_first)
    let mut second = seat
        .serve_with(&[
            ("TROMMI_SESSION_KEY", "test-other"),
            ("TROMMI_SELF_JOIN", "0"),
        ])
        .await;
    let (said, failed) = second
        .call("reply", json!({ "text": "from the second session" }))
        .await;
    assert!(failed, "the second session is not in the room: {said}");
    // it started cleanly and keeps running: Claude Code shows the plugin as connected, not as failed
    assert!(
        second.child.try_wait().expect("the process").is_none(),
        "the second session's connector is still running"
    );
    let listed = second.request("tools/list", json!({})).await;
    assert!(listed["tools"].is_array(), "it still answers: {listed}");
    assert!(
        said.contains("belongs to other") && said.contains("invite"),
        "it says the key is another session's and that it needs an invite: {said}"
    );
    // its `say` does not go through the first session's connector either
    let said = seat
        .command(&["say", "said by the second session"])
        .env("TROMMI_SESSION_KEY", "test-other")
        .env("TROMMI_SAY_MS", "3000")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .expect("say runs");
    assert!(
        !said.status.success(),
        "{}",
        String::from_utf8_lossy(&said.stdout)
    );

    // The first session ends: its key is free, and still not the second session's.
    first.close().await;
    let (said, failed) = second
        .call("reply", json!({ "text": "from the second session, later" }))
        .await;
    assert!(failed && said.contains("belongs to other"), "{said}");
    assert!(second.child.try_wait().expect("the process").is_none());
    second.close().await;

    // The first session resumed is the same member.
    let mut resumed = seat.serve().await;
    resumed.ready().await;
    resumed
        .ok(
            "reply",
            json!({ "text": "from the first session, resumed" }),
        )
        .await;

    // `claim-key`: the session's own key is its already; another session claims nothing
    let claim = |key: String| {
        let mut command = seat.command(&["claim-key"]);
        command
            .env("TROMMI_SESSION_KEY", key)
            .stdin(std::process::Stdio::null());
        async move { command.output().await.expect("claim-key runs") }
    };
    let mine = claim(seat.session_key.clone()).await;
    assert!(
        mine.status.success() && String::from_utf8_lossy(&mine.stdout).contains("already"),
        "{}{}",
        String::from_utf8_lossy(&mine.stdout),
        String::from_utf8_lossy(&mine.stderr)
    );
    let theirs = claim("test-other".into()).await;
    assert!(!theirs.status.success());

    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    let texts: Vec<String> = human
        .items
        .iter()
        .filter(|(_, sender, _)| *sender == me)
        .filter_map(|(_, _, payload)| payload["text"].as_str().map(String::from))
        .collect();
    assert!(
        texts.contains(&"from the first session".to_string())
            && texts.contains(&"from the first session, resumed".to_string()),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("second session")),
        "nothing of the second session reached the first one's chat: {texts:?}"
    );
    resumed.close().await;
}

/// Self-join (spec/v1.md 12.4): a second session on the same machine and account gets onto the board without a
/// manual invite. The running session vouches for it; the human's app answers with a link and confirms the very
/// Request whose hash the voucher passed on. The new session has its own device and its own board session, and
/// says there who vouched for it; nothing of it reaches the first session's chat; its resume is the same member.
/// A session of another account is not vouched for.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_session_joins_by_itself_vouched_by_the_first() {
    let (_hub, mut human, seat, group) = joined().await;
    let me = human.seat(&group).await.expect("the first agent");
    let mut first = seat.serve().await;
    first.ready().await;

    let other = [
        ("TROMMI_SESSION_KEY", "test-second"),
        ("TROMMI_SELF_JOIN_POLL_MS", "200"),
        ("TROMMI_SELF_JOIN", "1"),
    ];
    let mut second = seat.serve_with(&other).await;
    // its first call starts the self-join and answers at once, politely
    let (said, failed) = second.call("reply", json!({ "text": "too early" })).await;
    assert!(failed, "{said}");
    assert!(second.child.try_wait().expect("the process").is_none());
    let (admitted, new_group, ask) = human.admit_self_join(&group).await;
    assert_eq!(ask["vouched_by"], "project", "{ask}");
    assert_eq!(ask["name"], "project", "{ask}");
    assert!(admitted.device != me);
    second.ready().await;
    second
        .ok("reply", json!({ "text": "from the second session" }))
        .await;

    let in_session = |group: &GroupId| hex(group.session_id().unwrap().as_bytes());
    let texts_of = |human: &Human, group: &GroupId| -> Vec<String> {
        human
            .items
            .iter()
            .filter(|(s, _, _)| *s == in_session(group))
            .filter_map(|(_, _, payload)| payload["text"].as_str().map(String::from))
            .collect()
    };
    // the line who vouched is sent by the joining process itself, right after the join: on a slow machine it may
    // still be on its way when the session's own reply is in
    let mut new_texts = Vec::new();
    for _ in 0..50 {
        human.sync().await;
        new_texts = texts_of(&human, &new_group);
        if new_texts.contains(&"project joined by itself, vouched by project.".to_string()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    assert!(
        new_texts.contains(&"project joined by itself, vouched by project.".to_string())
            && new_texts.contains(&"from the second session".to_string()),
        "{new_texts:?}"
    );
    assert!(
        !texts_of(&human, &group)
            .iter()
            .any(|t| t.contains("second session")),
        "nothing of the second session in the first one's chat"
    );

    // its resume is the same member, without another ask
    second.close().await;
    let mut resumed = seat.serve_with(&other).await;
    resumed.ready().await;
    resumed
        .ok("reply", json!({ "text": "the second session, resumed" }))
        .await;
    human.sync().await;
    assert!(human
        .items
        .iter()
        .any(|(s, sender, payload)| *s == in_session(&new_group)
            && *sender == admitted.device
            && payload["text"] == "the second session, resumed"));
    resumed.close().await;

    // another account on the same machine: nobody vouches for it
    let asks_before = human
        .items
        .iter()
        .filter(|(_, _, p)| {
            p["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("join_ask/"))
        })
        .count();
    let foreign_config = seat.home.path().join("other-account");
    let mut third = seat
        .serve_with(&[
            ("TROMMI_SESSION_KEY", "test-third"),
            ("CLAUDE_CONFIG_DIR", foreign_config.to_str().unwrap()),
            ("TROMMI_SELF_JOIN", "1"),
        ])
        .await;
    let mut said = String::new();
    for _ in 0..50 {
        let (text, failed) = third.call("reply", json!({ "text": "third" })).await;
        assert!(failed, "{text}");
        if text.contains("needs an invite") {
            said = text;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(
        said.contains("invite"),
        "it needs an invite of its own: {said}"
    );
    assert!(third.child.try_wait().expect("the process").is_none());
    human.sync().await;
    let asks_after = human
        .items
        .iter()
        .filter(|(_, _, p)| {
            p["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("join_ask/"))
        })
        .count();
    assert_eq!(asks_before, asks_after, "no ask for another account");
    third.close().await;
    first.close().await;
}

/// Self-join in another folder, on by default (spec/v1.md 12.4): a new Claude Code session in a folder that holds no
/// key of any room starts joining by itself as soon as its client lists the tools, without a tool call. It lists
/// every board tool and connect, which says that no link is needed; the human's app gets the ask, and once in, the
/// client is told to list again and gets the board tools without connect.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_in_another_folder_joins_by_itself_without_a_tool_call() {
    let (_hub, mut human, seat, group) = joined().await;
    let mut first = seat.serve().await;
    first.ready().await;

    let elsewhere = seat.home.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a second folder");
    let mut second = seat
        .serve_with(&[
            ("TROMMI_FOLDER", elsewhere.to_str().unwrap()),
            ("TROMMI_SESSION_KEY", "test-elsewhere"),
            ("TROMMI_SELF_JOIN_POLL_MS", "200"),
        ])
        .await;
    let listed = second.request("tools/list", json!({})).await;
    let names = tool_names(&listed);
    assert!(
        names.contains(&"connect".to_string()) && names.contains(&"reply".to_string()),
        "not connect alone while it joins by itself: {names:?}"
    );
    let connect = listed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "connect")
        .unwrap();
    assert!(
        connect["description"]
            .as_str()
            .is_some_and(|d| d.contains("joining by itself") && d.contains("No invite link")),
        "{connect}"
    );

    // the ask reaches the human's app with no tool call of the new session
    let (admitted, new_group, ask) = human.admit_self_join(&group).await;
    assert_eq!(ask["name"], "elsewhere", "{ask}");
    assert_eq!(ask["vouched_by"], "project", "{ask}");
    second.ready().await;
    // once when it was vouched for (the connect tool's note changed), once when it is in
    for _ in 0..2 {
        second
            .notification("notifications/tools/list_changed")
            .await;
    }
    let listed = second.request("tools/list", json!({})).await;
    let names = tool_names(&listed);
    assert!(
        names.contains(&"reply".to_string()) && !names.contains(&"connect".to_string()),
        "{names:?}"
    );
    second
        .ok("reply", json!({ "text": "from the other folder" }))
        .await;
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    let session = hex(new_group.session_id().unwrap().as_bytes());
    assert!(human.items.iter().any(|(s, sender, p)| *s == session
        && *sender == admitted.device
        && p["text"] == "from the other folder"));
    second.close().await;
    first.close().await;
}

/// Self-join off (`TROMMI_SELF_JOIN=0`), or with nobody to vouch: a new session in a folder without a key offers
/// connect alone and asks nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_with_self_join_off_or_nobody_to_vouch_offers_connect_alone() {
    let (_hub, mut human, seat, group) = joined().await;
    let elsewhere = seat.home.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("a second folder");
    // nobody runs: nobody can vouch
    let mut alone = seat
        .serve_with(&[
            ("TROMMI_FOLDER", elsewhere.to_str().unwrap()),
            ("TROMMI_SESSION_KEY", "test-alone"),
        ])
        .await;
    let listed = alone.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");
    alone.close().await;

    let mut first = seat.serve().await;
    first.ready().await;
    let mut off = seat
        .serve_with(&[
            ("TROMMI_FOLDER", elsewhere.to_str().unwrap()),
            ("TROMMI_SESSION_KEY", "test-off"),
            ("TROMMI_SELF_JOIN", "0"),
        ])
        .await;
    let listed = off.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    human.sync().await;
    let session = hex(group.session_id().unwrap().as_bytes());
    assert!(
        !human.items.iter().any(|(s, _, p)| *s == session
            && p["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("join_ask/"))),
        "no ask with self-join off"
    );
    off.close().await;
    first.close().await;
}

/// Self-join denied (spec/v1.md 12.4): the person taps Deny on the ask; the new session stays off the board, keeps
/// running and says so, and nothing of it reaches the board.
#[tokio::test(flavor = "multi_thread")]
async fn a_self_join_the_person_denies_stays_off_the_board() {
    let (_hub, mut human, seat, group) = joined().await;
    let mut first = seat.serve().await;
    first.ready().await;
    let mut second = seat
        .serve_with(&[
            ("TROMMI_SESSION_KEY", "test-denied"),
            ("TROMMI_SELF_JOIN_POLL_MS", "200"),
            ("TROMMI_SELF_JOIN", "1"),
        ])
        .await;
    let mut said = String::new();
    for _ in 0..100 {
        let (text, failed) = second.call("reply", json!({ "text": "too early" })).await;
        assert!(failed, "{text}");
        assert!(
            text.contains("joining by itself"),
            "never 'needs an invite' while it joins: {text}"
        );
        if text.contains("one tap") {
            said = text;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        said.contains("vouched by"),
        "it says it waits for the person: {said}"
    );
    human.deny_self_join(&group).await;
    let mut said = String::new();
    for _ in 0..100 {
        let (text, failed) = second.call("reply", json!({ "text": "denied?" })).await;
        assert!(failed, "{text}");
        if text.contains("denied") {
            said = text;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(said.contains("invite"), "{said}");
    assert!(second.child.try_wait().expect("the process").is_none());
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    assert!(!human
        .items
        .iter()
        .any(|(_, _, p)| p["text"] == "denied?" || p["text"] == "too early"));
    second.close().await;
    first.close().await;
}

// ---- a new agent is online and named at once -------------------------------------------------------------------

/// Whether the human's stream says the device is online, within `ms`: the presence events of a stream opened now
/// (the hub tells a new human stream who is here first, and every change after).
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

/// The exact flow of a fresh Claude Code that joins by link while its MCP server runs: the agent runs
/// `trommi-connector connect <link>` through Bash, the human confirms, the one-shot process exits. The running
/// server takes the key and goes online by itself within two seconds, without a tool call, and the session is
/// named after its folder at once. Then the same after a reconnect (/mcp): the new server is online and named
/// without a tool call, and it stays online after the human writes to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_joined_by_the_cli_is_online_and_named_at_once() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut mcp = seat.serve().await;
    let listed = mcp.request("tools/list", json!({})).await;
    assert_eq!(tool_names(&listed), ["connect"], "{listed}");

    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = seat.join(&link);
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        let group = human.found_session(&admitted).await;
        (admitted, group)
    };
    let ((joined, log), (admitted, group)) = tokio::join!(connect, admit);
    assert!(joined, "the join failed:\n{log}");
    let device = admitted.device.to_base64url();

    assert!(
        online_within(&human, &device, 2000).await,
        "the running server went online by itself within 2 s; the join said:\n{log}"
    );
    let named = async {
        for _ in 0..40 {
            human.sync().await;
            if let Some(reg) = human.register(&group, &format!("device/{device}")).await {
                return Some(reg);
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        None
    };
    let reg = named.await.expect("the device register is written at once");
    assert_eq!(reg["device_name"], "project", "{reg}");

    // a reconnect (/mcp): the old server ends, a new one starts
    mcp.close().await;
    let mut mcp = seat.serve().await;
    assert!(
        online_within(&human, &device, 3000).await,
        "after a reconnect the new server is online without a tool call"
    );
    human
        .say(&group, json!({ "content_type": "message", "text": "Still there?" }))
        .await
        .expect("the human writes");
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(
        online_within(&human, &device, 1000).await,
        "a message of the human leaves it online"
    );
    let heard = mcp.event("chat").await;
    assert!(heard.to_string().contains("Still there?"), "{heard}");
    mcp.close().await;
}

/// The same by the connect tool (/trommi:connect): online and named once the human confirmed, then after a
/// reconnect (/mcp) online again without a tool call, and still online after the human writes to it.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_joined_by_the_connect_tool_stays_online_across_a_reconnect() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut mcp = seat.serve().await;
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let connect = mcp.call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        let group = human.found_session(&admitted).await;
        (admitted, group)
    };
    let ((said, failed), (admitted, group)) = tokio::join!(connect, admit);
    assert!(!failed, "{said}");
    let device = admitted.device.to_base64url();
    assert!(
        online_within(&human, &device, 3000).await,
        "online once the human confirmed"
    );

    mcp.close().await;
    let mut mcp = seat.serve().await;
    assert!(
        online_within(&human, &device, 3000).await,
        "after a reconnect the new server is online without a tool call"
    );
    let mut reg = None;
    for _ in 0..40 {
        human.sync().await;
        reg = human.register(&group, &format!("device/{device}")).await;
        if reg.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let reg = reg.expect("the device register is written");
    assert_eq!(reg["device_name"], "project", "{reg}");
    human
        .say(&group, json!({ "content_type": "message", "text": "Still there?" }))
        .await
        .expect("the human writes");
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(
        online_within(&human, &device, 1000).await,
        "a message of the human leaves it online"
    );
    let heard = mcp.event("chat").await;
    assert!(heard.to_string().contains("Still there?"), "{heard}");
    mcp.close().await;
}

/// A human message to a live session (no channel events: it waits for inbox) keeps the agent online on the
/// human's side, and the message reaches inbox.
#[tokio::test(flavor = "multi_thread")]
async fn a_human_message_keeps_a_live_session_online_and_reaches_inbox() {
    let (_hub, mut human, seat, group) = joined().await;
    let mut mcp = seat.serve_plain().await;
    mcp.ready().await;
    let device = human.seat(&group).await.expect("the agent's seat").to_base64url();
    assert!(online_within(&human, &device, 3000).await, "online before");
    for n in 0..3 {
        human
            .say(&group, json!({ "content_type": "message", "text": format!("Message {n}") }))
            .await
            .expect("the human writes");
        tokio::time::sleep(std::time::Duration::from_millis(700)).await;
        assert!(online_within(&human, &device, 1000).await, "online after message {n}");
    }
    let mut heard = String::new();
    for _ in 0..50 {
        heard.push_str(&mcp.ok("inbox", json!({})).await);
        if heard.contains("Message 2") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(heard.contains("Message 0") && heard.contains("Message 2"), "{heard}");
    mcp.close().await;
}
