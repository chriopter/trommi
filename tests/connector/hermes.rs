//! The connector as the Hermes add-on runs it (connector/hermes/): one connector per Trommi chat, started with
//! `TROMMI_HOST=hermes` and channel events on. The add-on hears the board as channel events and drives the turn with
//! three notifications of its own (`trommi/working`, `trommi/preview`, `trommi/answer`); Hermes' approvals go the way
//! Claude Code's do (`permission_request`, the verdict back). Here the test is the add-on.
mod common;

use common::process::{Mcp, Seat};
use common::{HubProc, Human};
use serde_json::{json, Value};

async fn until(human: &mut Human, what: &str, check: impl Fn(&Human) -> bool) {
    for _ in 0..600 {
        human.sync().await;
        if check(human) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{what}: not seen; items {:?} findings {:?}", human.items, human.findings);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_hermes_addon_hears_the_board_and_answers_into_the_chat() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let mut command = seat.command(&[]);
    command
        .env("TROMMI_HOST", "hermes")
        .env("TROMMI_CHANNEL_EVENTS", "on")
        .env("TROMMI_AGENT_NAME", "Hermes Einkauf")
        .env("TROMMI_PROFILE_LOOK_MS", "200");
    let hello = json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "hermes-trommi", "version": "1" } });
    let mut mcp = Mcp::start_as(command, hello.clone()).await;
    let again = mcp.request("initialize", hello).await;
    let instructions = again["instructions"].as_str().unwrap_or("").to_string();
    assert!(instructions.contains("platform trommi") && instructions.contains("never repeat it with reply"), "{instructions}");
    for never in ["Trommi · ", "/mcp", "Codex", "--urgent via Bash"] {
        assert!(!instructions.contains(never), "{never:?} in {instructions}");
    }

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

    // The person writes: the add-on hears it as a channel event (it hands it to Hermes as a message of this chat).
    human.say(&group, json!({ "content_type": "message", "text": "Wie ist das Wetter?" })).await.expect("written");
    let event = mcp.notification("notifications/claude/channel").await;
    assert_eq!((event["content"].as_str(), event["meta"]["kind"].as_str()), (Some("Wie ist das Wetter?"), Some("chat")));

    // Hermes works, its answer grows, then comes whole: the agent's message in the chat.
    mcp.notify("trommi/working", json!({ "on": true })).await;
    mcp.notify("trommi/preview", json!({ "draft": "1", "text": "Sonnig" })).await;
    // (the answer is a request: answered once it is written to the board)
    assert_eq!(mcp.request("trommi/answer", json!({ "text": "Sonnig, 21 Grad." })).await, json!({}));
    assert_eq!(mcp.request("trommi/state", json!({})).await["phase"], "ready");
    mcp.notify("trommi/working", json!({ "on": false })).await;
    until(&mut human, "the answer", |h| h.items.iter().any(|(_, s, p)| *s == me && p["terminal"] == "answer" && p["text"] == "Sonnig, 21 Grad.")).await;

    // Hermes asks to run a command: a yes/no card; the person's yes goes back to the add-on.
    mcp.notify(
        "notifications/claude/channel/permission_request",
        json!({ "request_id": "ab12cd", "tool_name": "Hermes", "description": "network access", "input_preview": "curl wetter.example" }),
    )
    .await;
    let request = loop {
        human.sync().await;
        if let Some(id) = human.object_ids().into_iter().find(|id| human.has_request(id)) {
            break id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    human.verdict(&request, true).await.expect("allowed");
    let verdict = mcp.notification("notifications/claude/channel/permission").await;
    assert_eq!((verdict["request_id"].as_str(), verdict["behavior"].as_str()), (Some("ab12cd"), Some("allow")));

    // "New chat with …": the person's ask reaches the add-on as its own kind of event, not as words.
    human
        .say(&group, json!({ "content_type": "message", "text": "Start a new chat with me, please.", "new_chat": { "v": 1, "nonce": "n1" } }))
        .await
        .expect("written");
    let ask: Value = mcp.notification("notifications/claude/channel").await;
    assert_eq!((ask["meta"]["kind"].as_str(), ask["meta"]["nonce"].as_str()), (Some("new_chat"), Some("n1")));
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}
