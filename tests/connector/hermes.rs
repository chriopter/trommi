//! The connector with Hermes Agent: the chat bridge (connector/src/chat_api.rs). Hermes starts the connector as its
//! MCP server with `TROMMI_HOST=hermes` (`setup hermes`); the connector hands the board's messages to Hermes' API
//! server (an OpenAI-compatible chat server on this computer) as the turns of one conversation and puts the answer
//! into the board's chat. Here a fake Hermes API server stands for Hermes: it notes every request and answers as
//! Hermes does (chat.completion.chunk frames, `hermes.tool.progress`, `approval.request`, `[DONE]`).
//!
//! One session: connect by the tool; the human writes; the turn streams with a step and a question for approval
//! (the yes/no card on the board, the yes back to Hermes); the answer lands in the chat; /stop ends a long turn.
mod common;

use common::process::{Mcp, Seat};
use common::{HubProc, Human};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the fake Hermes saw: each request's head and body, and whether a stream was cut off by its client.
#[derive(Default)]
struct Seen {
    requests: Vec<(String, String)>,
    cut: bool,
}

/// Reads one request: its head (lower-cased header names kept as sent) and its body.
async fn request(c: &mut tokio::net::TcpStream) -> Option<(String, String)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = c.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(i) = text.find("\r\n\r\n") {
            let len = text[..i]
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                .unwrap_or(0);
            if buf.len() >= i + 4 + len {
                return Some((text[..i].to_string(), text[i + 4..i + 4 + len].to_string()));
            }
        }
    }
}

fn chunk(words: &str) -> String {
    format!("data: {}\n\n", json!({ "object": "chat.completion.chunk", "choices": [{ "index": 0, "delta": { "content": words }, "finish_reason": null }] }))
}

/// A fake Hermes API server. The first turn asks for an approval and goes on once it came; the second one says a word
/// and then works on until its client goes.
async fn fake_hermes(seen: Arc<Mutex<Seen>>) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    let approved = Arc::new(tokio::sync::Notify::new());
    tokio::spawn(async move {
        let mut turns = 0;
        while let Ok((mut c, _)) = l.accept().await {
            let Some((head, body)) = request(&mut c).await else { continue };
            seen.lock().unwrap().requests.push((head.clone(), body));
            let (seen, approved) = (seen.clone(), approved.clone());
            if head.starts_with("POST /v1/runs/") {
                approved.notify_one();
                let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await;
                continue;
            }
            turns += 1;
            let turn = turns;
            tokio::spawn(async move {
                let _ = c.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\nX-Hermes-Session-Id: s\r\n\r\n").await;
                if turn == 1 {
                    let first = format!(
                        "{}event: hermes.tool.progress\ndata: {}\n\nevent: approval.request\ndata: {}\n\n",
                        chunk("Ich schaue nach."),
                        json!({ "tool": "web_search", "emoji": "🔎", "label": "Wetter Berlin", "toolCallId": "t1", "status": "running" }),
                        json!({ "event": "approval.request", "run_id": "chatcmpl-turn1", "timestamp": 1.0, "command": "curl wetter.example", "description": "network access", "pattern_key": "net", "request_id": "r1", "choices": ["once", "deny"] })
                    );
                    let _ = c.write_all(first.as_bytes()).await;
                    approved.notified().await;
                    let rest = format!(
                        "{}{}data: {}\n\ndata: [DONE]\n\n",
                        chunk("Sonnig, "),
                        chunk("21 Grad."),
                        json!({ "object": "chat.completion.chunk", "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }] })
                    );
                    let _ = c.write_all(rest.as_bytes()).await;
                } else {
                    let _ = c.write_all(chunk("Fange an.").as_bytes()).await;
                    // (keep-alives until the client goes)
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        if c.write_all(b": keepalive\n\n").await.is_err() {
                            seen.lock().unwrap().cut = true;
                            return;
                        }
                    }
                }
                let _ = c.shutdown().await;
            });
        }
    });
    url
}

/// Waits until `check` holds on the human's view, syncing it.
async fn until(human: &mut Human, what: &str, check: impl Fn(&Human) -> bool) {
    for _ in 0..600 {
        human.sync().await;
        if check(human) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("{what}: not seen; items {:?} trail {:?} findings {:?}", human.items, human.trail, human.findings);
}

#[tokio::test(flavor = "multi_thread")]
async fn hermes_answers_in_the_chat_through_the_bridge() {
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let seat = Seat::new(&hub.url);
    let seen = Arc::new(Mutex::new(Seen::default()));
    let url = fake_hermes(seen.clone()).await;
    // what `setup hermes` keeps
    trommi_connector::chat_api::Endpoint { url, key: "test-key".into(), model: "hermes-agent".into() }
        .save(&seat.home.path().join("keys"), "hermes")
        .expect("kept");

    let mut command = seat.command(&[]);
    command
        .env("TROMMI_HOST", "hermes")
        .env("TROMMI_CHANNEL_EVENTS", "off")
        .env("TROMMI_CHAT_GATHER_MS", "100");
    let hello = json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "hermes", "version": "0" } });
    let mut mcp = Mcp::start_as(command, hello.clone()).await;
    let again = mcp.request("initialize", hello).await;
    let instructions = again["instructions"].as_str().unwrap_or("").to_string();
    assert!(instructions.contains("final text reaches the board's chat by itself"), "{instructions}");
    assert!(instructions.contains("Hermes gateway"), "{instructions}");
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

    // The human writes: the words go to Hermes as a turn of the session's conversation.
    human.say(&group, json!({ "content_type": "message", "text": "Hallo Hermes, wie ist das Wetter?" })).await.expect("written");
    let request = loop {
        if let Some(r) = seen.lock().unwrap().requests.first().cloned() {
            break r;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    };
    let head = request.0.to_ascii_lowercase();
    assert!(head.starts_with("post /v1/chat/completions "), "{head}");
    assert!(head.contains("authorization: bearer test-key"), "{head}");
    let conversation = head.lines().find_map(|l| l.strip_prefix("x-hermes-session-id: ")).expect("the conversation").to_string();
    assert!(conversation.starts_with("trommi-") && conversation.len() > 12, "{conversation}");
    let body: Value = serde_json::from_str(&request.1).expect("json");
    assert_eq!(body["messages"], json!([{ "role": "user", "content": "Hallo Hermes, wie ist das Wetter?" }]));
    assert_eq!(body["stream"], json!(true));

    // Hermes asks for an approval: a request on the board; the human allows it, and Hermes hears "once".
    let asked = loop {
        human.sync().await;
        if let Some(id) = human.object_ids().into_iter().find(|id| human.has_request(id)) {
            break id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    };
    assert!(human.items.iter().any(|(_, _, p)| p["tool_name"] == "Hermes" && p["input_preview"] == "curl wetter.example"));
    human.verdict(&asked, true).await.expect("allowed");

    // The answer, with the turn's steps before it.
    until(&mut human, "the answer", |h| h.items.iter().any(|(_, s, p)| *s == me && p["terminal"] == "answer" && p["text"] == "Sonnig, 21 Grad.")).await;
    let approval = seen.lock().unwrap().requests.iter().find(|(h, _)| h.starts_with("POST /v1/runs/")).cloned().expect("the approval went back");
    assert!(approval.0.starts_with("POST /v1/runs/chatcmpl-turn1/approval "), "{}", approval.0);
    assert_eq!(serde_json::from_str::<Value>(&approval.1).unwrap(), json!({ "choice": "once", "request_id": "r1" }));
    until(&mut human, "the steps", |h| {
        let steps: Vec<&Value> = h.trail.iter().map(|(_, _, s)| s).collect();
        steps.iter().any(|s| s["text"] == "Ich schaue nach.") && steps.iter().any(|s| s["tool"] == "web_search" && s["text"].as_str().is_some_and(|t| t.contains("Wetter Berlin")))
    })
    .await;

    // A long turn, and /stop: the request ends (Hermes interrupts its agent), the chat says so.
    human.say(&group, json!({ "content_type": "message", "text": "Mach eine lange Aufgabe." })).await.expect("written");
    for _ in 0..300 {
        if seen.lock().unwrap().requests.iter().filter(|(h, _)| h.starts_with("POST /v1/chat/")).count() == 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let second = seen.lock().unwrap().requests.iter().filter(|(h, _)| h.starts_with("POST /v1/chat/")).nth(1).cloned().expect("the second turn");
    assert!(second.0.to_ascii_lowercase().contains(&format!("x-hermes-session-id: {conversation}")), "the same conversation");
    human.say(&group, json!({ "content_type": "message", "text": "/stop" })).await.expect("written");
    until(&mut human, "the stopped answer", |h| h.items.iter().any(|(_, s, p)| *s == me && p["terminal"] == "answer" && p["text"] == "Fange an.\n\n(stopped)")).await;
    for _ in 0..100 {
        if seen.lock().unwrap().cut {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(seen.lock().unwrap().cut, "the stream was ended");
    assert_eq!(seen.lock().unwrap().requests.iter().filter(|(h, _)| h.starts_with("POST /v1/chat/")).count(), 2, "/stop is no turn of its own");
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    mcp.close().await;
}
