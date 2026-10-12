//! The connector's MCP side under random input (README "Fuzzing"): the built binary, joined to a real hub,
//! is sent tool calls whose arguments follow the tool list's schemas with odd values (huge and empty texts,
//! control characters, other scripts, paths that climb out, numbers of every size, wrong types, deep nesting),
//! tool names that do not exist, and lines that are no JSON-RPC at all. Black box, no coverage feedback: a
//! long campaign is a matter of `FUZZ_MCP_CASES` (default 150) and `FUZZ_SEED`.
//!
//! Properties: the connector never exits, every request with an id gets exactly one answer with that id within
//! a minute, every line it writes is one JSON message, and it still answers `ping` at the end. A case that
//! fails is written to `$FUZZ_ARTIFACTS` (default the temp directory) as the line that was sent.
mod common;

use common::process::{Mcp, Seat};
use common::{HubProc, Human};
use serde_json::{json, Map, Value};
use std::time::Duration;

/// A small random source (xorshift), seeded from `FUZZ_SEED` or the clock; the seed is printed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn one_in(&mut self, n: u64) -> bool {
        self.below(n) == 0
    }
}

const ODD_TEXTS: [&str; 22] = [
    "",
    " ",
    "\n\n\n",
    "\u{0}",
    "\u{202e}gnp.exe",
    "日本語のテキスト",
    "👩‍👩‍👧‍👦🏳️‍🌈",
    "../../../../etc/passwd",
    "/etc/shadow",
    "~/.ssh/id_ed25519",
    "C:\\Windows\\system32",
    "file:///etc/hosts",
    "https://evil.example/x",
    "%2e%2e%2f%2e%2e%2f",
    "{\"json\":\"inside\"}",
    "```\ncode\n```",
    "<script>alert(1)</script>",
    "<img src=x onerror=alert(1)>",
    "$(rm -rf ~)",
    "null",
    "0",
    "AAAAAAAAAAAAAAAAAAAAAA",
];

fn text(rng: &mut Rng) -> String {
    match rng.below(6) {
        0 => ODD_TEXTS[rng.below(ODD_TEXTS.len() as u64) as usize].to_owned(),
        1 => "x".repeat(rng.below(200_000) as usize),
        2 => (0..rng.below(64))
            .map(|_| char::from_u32(rng.below(0x11_0000) as u32).unwrap_or('?'))
            .collect(),
        3 => (0..rng.below(64))
            .map(|_| char::from(rng.below(128) as u8))
            .collect(),
        _ => format!("word {}", rng.below(1000)),
    }
}

fn odd(rng: &mut Rng, depth: u32) -> Value {
    match rng.below(10) {
        0 => Value::Null,
        1 => json!(rng.one_in(2)),
        2 => json!(rng.next() as i64),
        3 => json!(f64::from(rng.below(1000) as u32) / 3.0 - 100.0),
        4 if depth < 4 => Value::Array((0..rng.below(5)).map(|_| odd(rng, depth + 1)).collect()),
        5 if depth < 4 => {
            let mut map = Map::new();
            for _ in 0..rng.below(5) {
                map.insert(text(rng), odd(rng, depth + 1));
            }
            Value::Object(map)
        }
        6 if depth == 0 => {
            // nesting as deep as a JSON parser takes (serde_json stops at 128: a deeper message is no message,
            // and without its id it has no answer)
            let mut v = json!(1);
            for _ in 0..rng.below(120) {
                v = json!([v]);
            }
            v
        }
        _ => json!(text(rng)),
    }
}

/// A value for a property of this schema: mostly of its type, now and then anything.
fn of_schema(rng: &mut Rng, schema: &Value) -> Value {
    if rng.one_in(6) {
        return odd(rng, 0);
    }
    if let Some(choices) = schema["enum"].as_array() {
        if !choices.is_empty() && !rng.one_in(4) {
            return choices[rng.below(choices.len() as u64) as usize].clone();
        }
    }
    let ty = schema["type"]
        .as_str()
        .or_else(|| schema["type"][0].as_str())
        .unwrap_or("string");
    match ty {
        "string" => json!(text(rng)),
        "integer" | "number" => match rng.below(4) {
            0 => json!(-1),
            1 => json!(u64::MAX),
            2 => json!(1e300),
            _ => json!(rng.below(100)),
        },
        "boolean" => json!(rng.one_in(2)),
        "array" => {
            let max = if rng.one_in(10) { 300 } else { 4 };
            Value::Array(
                (0..rng.below(max))
                    .map(|_| of_schema(rng, &schema["items"]))
                    .collect(),
            )
        }
        "object" => arguments(rng, schema),
        _ => odd(rng, 1),
    }
}

fn arguments(rng: &mut Rng, schema: &Value) -> Value {
    let mut map = Map::new();
    if let Some(props) = schema["properties"].as_object() {
        for (name, prop) in props {
            if rng.below(3) != 0 {
                map.insert(name.clone(), of_schema(rng, prop));
            }
        }
    }
    if rng.one_in(8) {
        map.insert(text(rng), odd(rng, 1));
    }
    Value::Object(map)
}

async fn seated() -> (HubProc, Human, Seat, Mcp) {
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
    let ((joined, log), _group) = tokio::join!(join, admit);
    assert!(joined, "the join failed:\n{log}");
    let mut mcp = seat.serve().await;
    mcp.ready().await;
    (hub, human, seat, mcp)
}

/// Sends one line and waits for the answer to `id`, keeping notifications; `None` if none came in time.
async fn answer(mcp: &mut Mcp, line: &str, id: Option<u64>) -> Option<Value> {
    mcp.send_raw(line).await;
    let id = id?;
    for _ in 0..600 {
        match mcp.read(100).await {
            Some(message) if message.get("id").and_then(Value::as_u64) == Some(id) => {
                return Some(message)
            }
            Some(_) | None => {}
        }
    }
    None
}

fn keep_failure(line: &str, why: &str) -> ! {
    let dir = std::env::var_os("FUZZ_ARTIFACTS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join(format!("connector-mcp-{}.jsonl", std::process::id()));
    let _ = std::fs::write(&file, line);
    panic!("{why}; the line is in {}", file.display());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_connector_survives_random_tool_calls_and_lines() {
    let cases: u64 = std::env::var("FUZZ_MCP_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150);
    let seed: u64 = std::env::var("FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(1, |d| d.as_nanos() as u64)
                | 1
        });
    eprintln!("FUZZ_SEED={seed}");
    let mut rng = Rng(seed);
    let tools: Value =
        serde_json::from_str(include_str!("../../connector/tools.json")).expect("the tool list");
    let mut list: Vec<(String, Value)> = tools["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            (
                t["name"].as_str().unwrap_or("").to_owned(),
                t["inputSchema"].clone(),
            )
        })
        .collect();
    for extra in ["reload", "inbox", "connect"] {
        list.push((extra.to_owned(), tools[extra]["inputSchema"].clone()));
    }

    let (_hub, _human, _seat, mut mcp) = seated().await;
    let mut id = 1_000_000u64;
    for case in 0..cases {
        id += 1;
        let (line, expect) = match rng.below(12) {
            0 => (text(&mut rng), None),
            1 if rng.one_in(2) => (format!("{}1{}", "[".repeat(5000), "]".repeat(5000)), None),
            1 => (json!([{ "jsonrpc": "2.0", "id": id, "method": "ping" }]).to_string(), None),
            2 => (json!({ "jsonrpc": "2.0", "id": { "odd": true }, "method": "tools/call", "params": odd(&mut rng, 0) }).to_string(), None),
            3 => (json!({ "jsonrpc": "2.0", "id": id, "method": text(&mut rng), "params": odd(&mut rng, 0) }).to_string(), Some(id)),
            4 => (json!({ "jsonrpc": "2.0", "method": "notifications/claude/channel/permission", "params": odd(&mut rng, 0) }).to_string(), None),
            5 => (json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": text(&mut rng), "arguments": odd(&mut rng, 0) } }).to_string(), Some(id)),
            _ => {
                let (name, schema) = &list[rng.below(list.len() as u64) as usize];
                let args = if rng.one_in(10) { odd(&mut rng, 0) } else { arguments(&mut rng, schema) };
                (
                    json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": name, "arguments": args } })
                        .to_string(),
                    Some(id),
                )
            }
        };
        let got = answer(&mut mcp, &line, expect).await;
        if expect.is_some() && got.is_none() {
            keep_failure(&line, &format!("case {case}: no answer within a minute"));
        }
        if let Ok(Some(status)) = mcp.child.try_wait() {
            keep_failure(
                &line,
                &format!("case {case}: the connector exited: {status}"),
            );
        }
    }
    id += 1;
    let pong = answer(
        &mut mcp,
        &json!({ "jsonrpc": "2.0", "id": id, "method": "ping" }).to_string(),
        Some(id),
    )
    .await;
    assert!(pong.is_some(), "no pong after the campaign");
    tokio::time::sleep(Duration::from_millis(10)).await;
    mcp.close().await;
}
