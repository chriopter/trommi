//! Opt-in: a real Codex against this repository's hub and connector. It needs the `codex` CLI, logged in, and costs
//! a few model calls, so it is ignored unless asked for:
//!
//! ```text
//! cargo test -p trommi-tests --test connector_codex_real -- --ignored --nocapture
//! ```
//!
//! `CODEX_MODEL` (default `gpt-6-luna`) and `CODEX_EFFORT` (default `low`) pick the model; `TROMMI_CODEX_RUNS`
//! repeats the whole round (default 1). Codex runs as its app server (one process, many turns: an interactive
//! session as the TUI has it) with the trommi MCP server given by `-c` overrides, never in the user's config. Its
//! project folder is `~/.cache/trommi-codex-smoke/project` (Codex notes a folder it ran in as trusted, once); keys,
//! hub and connector are throwaway. What it checks: connect by the invite link, introduce, a decision card, the
//! human's answer waking the idle session (`codex queue`), a chat message doing the same, a restart with
//! `thread/resume`, a swapped-in connector, and that no Trommi call asked for approval.
mod common;

use common::process::connector_binary;
use common::{HubProc, Human, TempDir};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

/// A Codex app server over stdio.
struct Codex {
    child: Child,
    stdin: ChildStdin,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
    thread: String,
    /// Server requests that asked for approval of anything.
    approvals: Vec<String>,
}

/// One finished turn: the agent's messages and its trommi tool calls `(tool, arguments, result text, failed)`.
#[derive(Default, Debug)]
struct Turn {
    said: Vec<String>,
    tools: Vec<(String, Value, String, bool)>,
}

impl Turn {
    fn called(&self, tool: &str) -> bool {
        self.tools.iter().any(|t| t.0 == tool)
    }
}

struct Setup {
    hub_url: String,
    program: PathBuf,
    keys: PathBuf,
    run: PathBuf,
    project: PathBuf,
}

impl Codex {
    async fn start(s: &Setup) -> Codex {
        let model = std::env::var("CODEX_MODEL").unwrap_or("gpt-6-luna".into());
        let env = format!(
            "{{TROMMI_HUB=\"{}\",TROMMI_KEYS_DIR=\"{}\",XDG_RUNTIME_DIR=\"{}\",TROMMI_FOLDER=\"{}\",TROMMI_UPDATE_POLL_MS=\"1000\",TROMMI_UPDATE_CHECK_MS=\"0\"}}",
            s.hub_url,
            s.keys.display(),
            s.run.display(),
            s.project.display()
        );
        let mut child = Command::new("codex")
            .args(["app-server", "-c"])
            .arg(format!(
                "mcp_servers.trommi.command=\"{}\"",
                s.program.display()
            ))
            .args(["-c", "mcp_servers.trommi.args=[]", "-c"])
            .arg(format!("mcp_servers.trommi.env={env}"))
            .args(["-c"])
            .arg(format!("model=\"{model}\""))
            .current_dir(&s.project)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("codex runs (is the Codex CLI installed?)");
        let stdin = child.stdin.take().unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut codex = Codex {
            child,
            stdin,
            lines,
            next_id: 1,
            thread: String::new(),
            approvals: Vec::new(),
        };
        codex
            .request(
                "initialize",
                json!({ "clientInfo": { "name": "trommi-test", "title": "trommi test", "version": "0" } }),
            )
            .await;
        codex
            .send(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }))
            .await;
        codex
    }

    async fn send(&mut self, message: &Value) {
        self.stdin
            .write_all(format!("{message}\n").as_bytes())
            .await
            .expect("codex reads");
        self.stdin.flush().await.unwrap();
    }

    async fn next(&mut self, until: Instant) -> Option<Value> {
        let left = until.saturating_duration_since(Instant::now());
        let line = tokio::time::timeout(left, self.lines.next_line())
            .await
            .ok()?;
        let line = line.expect("codex's stdout")?;
        let message: Value = serde_json::from_str(&line).ok()?;
        // a request of the server: approvals are noted (and declined), anything else is answered empty
        if message.get("id").is_some() && message.get("method").is_some() {
            let method = message["method"].as_str().unwrap_or("").to_string();
            let answer = if method.to_lowercase().contains("approval") {
                self.approvals
                    .push(format!("{method}: {}", message["params"]));
                json!({ "decision": "decline" })
            } else if method.to_lowercase().contains("elicitation") {
                self.approvals
                    .push(format!("{method}: {}", message["params"]));
                json!({ "action": "decline" })
            } else {
                json!({})
            };
            self.send(&json!({ "jsonrpc": "2.0", "id": message["id"], "result": answer }))
                .await;
            return Some(json!({}));
        }
        Some(message)
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        let until = Instant::now() + Duration::from_secs(120);
        loop {
            let message = self
                .next(until)
                .await
                .unwrap_or_else(|| panic!("no answer to {method}"));
            if message.get("id").and_then(Value::as_u64) == Some(id)
                && message.get("method").is_none()
            {
                assert!(message.get("error").is_none(), "{method}: {message}");
                return message["result"].clone();
            }
        }
    }

    fn effort() -> String {
        std::env::var("CODEX_EFFORT").unwrap_or("low".into())
    }

    async fn start_thread(&mut self, project: &Path) {
        let started = self
            .request(
                "thread/start",
                json!({ "cwd": project, "approvalPolicy": "never", "sandbox": "workspace-write" }),
            )
            .await;
        self.thread = started["thread"]["id"]
            .as_str()
            .expect("a thread")
            .to_string();
    }

    async fn resume(&mut self, thread: &str, project: &Path) {
        self.request(
            "thread/resume",
            json!({ "threadId": thread, "cwd": project, "approvalPolicy": "never", "sandbox": "workspace-write", "excludeTurns": true }),
        )
        .await;
        self.thread = thread.to_string();
    }

    /// Reads until a turn completes; `on_tool` runs when a trommi tool call starts (the test's human acts there).
    async fn finish_turn(
        &mut self,
        secs: u64,
        mut on_tool: impl FnMut(&str) -> bool,
        human: &mut Human,
    ) -> Option<Turn> {
        let until = Instant::now() + Duration::from_secs(secs);
        let mut turn = Turn::default();
        let mut started = false;
        while let Some(message) = self.next(until).await {
            match message["method"].as_str().unwrap_or("") {
                "turn/started" => started = true,
                "item/started" if message["params"]["item"]["type"] == "mcpToolCall" => {
                    let tool = message["params"]["item"]["tool"]
                        .as_str()
                        .unwrap_or("")
                        .to_string();
                    if on_tool(&tool) {
                        human_admits(human).await;
                    }
                }
                "item/completed" => {
                    let item = &message["params"]["item"];
                    match item["type"].as_str() {
                        Some("agentMessage") => {
                            let text = item["text"].as_str().unwrap_or("").to_string();
                            eprintln!("  codex: {}", text.chars().take(300).collect::<String>());
                            turn.said.push(text);
                        }
                        Some("mcpToolCall") => {
                            let text = item["result"]["content"][0]["text"]
                                .as_str()
                                .or(item["error"]["message"].as_str())
                                .unwrap_or("")
                                .to_string();
                            let failed =
                                item["status"] != "completed" || item["result"]["isError"] == true;
                            eprintln!(
                                "  tool {} {} -> {}{}",
                                item["tool"].as_str().unwrap_or(""),
                                item["arguments"]
                                    .to_string()
                                    .chars()
                                    .take(200)
                                    .collect::<String>(),
                                if failed { "FAILED " } else { "" },
                                text.chars().take(200).collect::<String>()
                            );
                            turn.tools.push((
                                item["tool"].as_str().unwrap_or("").to_string(),
                                item["arguments"].clone(),
                                text,
                                failed,
                            ));
                        }
                        Some("commandExecution") => eprintln!(
                            "  shell: {}",
                            item["command"]
                                .as_str()
                                .unwrap_or("")
                                .chars()
                                .take(200)
                                .collect::<String>()
                        ),
                        _ => {}
                    }
                }
                "turn/completed" if started => return Some(turn),
                _ => {}
            }
        }
        None
    }

    async fn turn(&mut self, text: &str, human: &mut Human) -> Turn {
        self.turn_with(text, human, |_| false).await
    }

    async fn turn_with(
        &mut self,
        text: &str,
        human: &mut Human,
        on_tool: impl FnMut(&str) -> bool,
    ) -> Turn {
        eprintln!("> {}", text.chars().take(200).collect::<String>());
        let params = json!({ "threadId": self.thread, "effort": Self::effort(), "input": [{ "type": "text", "text": text, "text_elements": [] }] });
        self.request("turn/start", params).await;
        self.finish_turn(600, on_tool, human)
            .await
            .expect("the turn ends")
    }

    /// A turn the connector started by itself (the queued wake), within `secs`.
    async fn woken(&mut self, secs: u64, human: &mut Human) -> Option<Turn> {
        eprintln!("(waiting for a wake)");
        self.finish_turn(secs, |_| false, human).await
    }

    async fn close(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(Duration::from_secs(20), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

/// The admission the test's human does while Codex's connect waits; set by `round`.
static PENDING: std::sync::Mutex<Option<common::Invite>> = std::sync::Mutex::new(None);
static GROUP: std::sync::Mutex<Option<trommi_core::ids::GroupId>> = std::sync::Mutex::new(None);

async fn human_admits(human: &mut Human) {
    let Some(mut invite) = PENDING.lock().unwrap().take() else {
        return;
    };
    let admitted = human.admit(&mut invite).await;
    let group = human.found_session(&admitted).await;
    *GROUP.lock().unwrap() = Some(group);
}

/// Waits (at most two minutes) until the agent wrote `text` into the chat.
async fn wait_said(human: &mut Human, text: &str) {
    for _ in 0..120 {
        if agent_said(human, text).await {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("the agent never wrote {text:?} on the board");
}

/// Whether the human's side holds a chat message of the agent with `text` (any case).
async fn agent_said(human: &mut Human, text: &str) -> bool {
    human.sync().await;
    let wanted = text.to_lowercase();
    human.items.iter().any(|(_, _, p)| {
        p["text"]
            .as_str()
            .is_some_and(|t| t.to_lowercase().contains(&wanted))
    })
}

async fn round(n: u32) {
    eprintln!("=== real Codex round {n}");
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let state = TempDir::new("codex-real");
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let project = home.join(".cache/trommi-codex-smoke/project");
    let _ = std::fs::remove_dir_all(project.join(".trommi"));
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("README.md"), "# A project\n").unwrap();
    let run = std::env::temp_dir().join(format!("rc{}", trommi_connector::util::random_hex(3)));
    std::fs::create_dir_all(&run).unwrap();
    let bin = state.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("trommi-connector");
    let original = std::fs::read(connector_binary()).expect("the built connector");
    std::fs::write(&program, &original).unwrap();
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let setup = Setup {
        hub_url: hub.url.clone(),
        program: program.clone(),
        keys: state.path().join("keys"),
        run: run.clone(),
        project: project.clone(),
    };

    let mut codex = Codex::start(&setup).await;
    codex.start_thread(&project).await;
    let thread = codex.thread.clone();

    // 1. "Connect to Trommi with this link": Codex calls connect, the human admits while it waits.
    let invite = human.invite(None).await;
    let link = invite.link.clone();
    *PENDING.lock().unwrap() = Some(invite);
    let t = codex
        .turn_with(
            &format!("Connect to Trommi with this invite link: {link}"),
            &mut human,
            |tool| tool == "connect",
        )
        .await;
    assert!(t.called("connect"), "Codex called connect: {t:?}");
    assert!(
        t.tools
            .iter()
            .any(|x| x.0 == "connect" && x.2.contains("Check code")),
        "the check code came back: {t:?}"
    );
    let group = GROUP.lock().unwrap().expect("the session was founded");

    // 2. Introduce: the board tools are there without a new tool list.
    let t = codex
        .turn(
            "I tapped \"They match\" in the app. Introduce yourself on the Trommi board.",
            &mut human,
        )
        .await;
    assert!(
        t.tools
            .iter()
            .any(|x| ["introduce", "reply"].contains(&x.0.as_str()) && !x.3),
        "a board tool worked in the same session, without a new tool list: {t:?}"
    );

    // 3. A decision card, then the turn ends; the human's answer wakes the session.
    let t = codex
        .turn(
            "Ask me on the Trommi board whether the README should be in English or German: one decision card, options en and de. Then end your turn; I answer on the board.",
            &mut human,
        )
        .await;
    let card = t
        .tools
        .iter()
        .find(|x| x.0 == "create_decision" && !x.3)
        .and_then(|x| x.2.split_whitespace().nth(1).map(String::from))
        .unwrap_or_else(|| panic!("a decision card was filed: {t:?}"));
    tokio::time::sleep(Duration::from_secs(2)).await;
    human
        .answer(
            &card,
            json!({ "answer_action": "answer", "choices": ["de"], "note": "Auf Deutsch, bitte (äöü ß)." }),
            &["de"],
            false,
        )
        .await
        .expect("the answer is taken");
    let t = codex
        .woken(180, &mut human)
        .await
        .expect("the answer woke Codex");
    assert!(t.called("inbox"), "the woken turn read the board: {t:?}");
    assert!(
        t.tools
            .iter()
            .any(|x| x.0 == "inbox" && x.2.contains("choice=\"de\"")),
        "inbox held the answer: {t:?}"
    );

    // 4. A chat message wakes it too, and the answer reaches the board.
    human
        .say(&group, json!({ "content_type": "message", "text": "Bitte antworte im Chat nur mit dem Wort HALLO." }))
        .await
        .expect("the human writes");
    codex
        .woken(180, &mut human)
        .await
        .expect("the chat woke Codex");
    wait_said(&mut human, "hallo").await;

    // 5. Restart: Codex quits, the human writes meanwhile, Codex resumes the thread and is woken before its first
    // call.
    codex.close().await;
    human
        .say(&group, json!({ "content_type": "message", "text": "Während du weg warst: antworte nur mit dem Wort ZURUECK." }))
        .await
        .expect("the human writes");
    let mut codex = Codex::start(&setup).await;
    codex.resume(&thread, &project).await;
    codex
        .woken(240, &mut human)
        .await
        .expect("the resumed thread was woken");
    wait_said(&mut human, "zurueck").await;

    // 6. A new connector binary is swapped in under the running Codex; the next message wakes it as before.
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
    tokio::time::sleep(Duration::from_secs(15)).await;
    human
        .say(&group, json!({ "content_type": "message", "text": "Nach dem Tausch: antworte nur mit dem Wort GETAUSCHT." }))
        .await
        .expect("the human writes");
    codex
        .woken(180, &mut human)
        .await
        .expect("the swapped-in connector woke Codex");
    wait_said(&mut human, "getauscht").await;

    assert!(
        codex.approvals.is_empty(),
        "no Trommi call asked for approval: {:?}",
        codex.approvals
    );
    codex.close().await;
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    let _ = std::fs::remove_dir_all(&run);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs a real Codex (model calls); see the file's head"]
async fn a_real_codex_session_round_trip() {
    let runs: u32 = std::env::var("TROMMI_CODEX_RUNS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    for n in 1..=runs {
        round(n).await;
    }
}
