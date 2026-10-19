//! Opt-in: a real Codex against this repository's hub and connector. It needs the `codex` CLI, logged in, and costs
//! a few model calls, so it is ignored unless asked for:
//!
//! ```text
//! cargo test -p trommi-tests --test connector_codex_real -- --ignored --nocapture
//! ```
//!
//! `CODEX_HOME` must name a Codex home of its own for tests (logged in once, `codex login` with that CODEX_HOME):
//! Codex notes a folder it ran in as trusted, and the hooks round writes hooks and their trust, so the test refuses
//! to run against `~/.codex`. `CODEX_MODEL` (default `gpt-6-luna`) and `CODEX_EFFORT` (default `low`) pick the
//! model; `TROMMI_CODEX_RUNS` repeats the whole round (default 1). Codex runs as its app server (one process, many
//! turns: an interactive session as the TUI has it) with the trommi MCP server given by `-c` overrides. Its project
//! folder is a temporary one; keys, hub and connector are throwaway.
//!
//! `a_real_codex_with_trommi_hooks` runs `trommi-connector setup codex` into that home (MCP server and hooks), trusts
//! the hooks through the app server as /hooks does (`hooks/list`, `config/value/write`), and checks the terminal
//! mirror (typed words and the final answer), the work trail, an answer under the card that started the turn, and a
//! permission prompt answered on the board; at the end `remove codex` takes it all out again. What it checks: connect by the invite link, introduce, a decision card, the
//! human's answer waking the idle session (`codex queue`), a chat message doing the same, a restart with
//! `thread/resume`, a swapped-in connector, and that no Trommi call asked for approval.
mod common;

use common::process::connector_binary;
use common::{HubProc, Human, TempDir};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
            // (what Codex's hooks pass on to the connector they run: where its doors and keys are)
            .env("XDG_RUNTIME_DIR", &s.run)
            .env("TROMMI_KEYS_DIR", &s.keys)
            .env("TROMMI_FOLDER", &s.project)
            .env("TROMMI_HUB", &s.hub_url)
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
        self.start_thread_with(project, "never").await
    }

    async fn start_thread_with(&mut self, project: &Path, approval: &str) {
        let started = self
            .request(
                "thread/start",
                json!({ "cwd": project, "approvalPolicy": approval, "sandbox": "workspace-write" }),
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

/// The Codex home for tests (`CODEX_HOME`), never the user's own `~/.codex`.
fn test_codex_home() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
    let given = std::env::var_os("CODEX_HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .expect(
            "set CODEX_HOME to a Codex home of its own for tests (never ~/.codex), logged in once",
        );
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    assert_ne!(
        canon(&given),
        canon(&home.join(".codex")),
        "CODEX_HOME must not be the user's ~/.codex"
    );
    assert!(
        given.join("auth.json").exists(),
        "{} is not logged in",
        given.display()
    );
    given
}

async fn round(n: u32) {
    eprintln!("=== real Codex round {n}");
    test_codex_home();
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let state = TempDir::new("codex-real");
    let project = state.path().join("project");
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

/// Runs the built connector's `setup codex` or `remove codex` with `home` as HOME (its installed connector there) and
/// the test's CODEX_HOME.
fn connector_cli(home: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new(home.join(".local/share/trommi/bin/trommi-connector"))
        .args(args)
        .arg("--verbose")
        .env("HOME", home)
        .output()
        .expect("the connector runs");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{args:?}: {text}");
    text
}

/// Trusts every hook of the connector in Codex, as /hooks "trust all" does; returns how many it trusted.
async fn trust_hooks(codex: &mut Codex, project: &Path) -> usize {
    let listed = codex
        .request("hooks/list", json!({ "cwds": [project] }))
        .await;
    let mut n = 0;
    for entry in listed["data"].as_array().cloned().unwrap_or_default() {
        for hook in entry["hooks"].as_array().cloned().unwrap_or_default() {
            let ours = hook["command"]
                .as_str()
                .is_some_and(|c| c.contains("trommi-connector"));
            if ours && hook["trustStatus"] != "trusted" {
                let key = hook["key"].as_str().expect("a key");
                codex
                    .request(
                        "config/value/write",
                        json!({ "keyPath": format!("hooks.state.{}.trusted_hash", Value::String(key.into())), "value": hook["currentHash"], "mergeStrategy": "replace" }),
                    )
                    .await;
                n += 1;
            }
        }
    }
    n
}

/// Waits (at most two minutes) until `seen` holds on the human's side.
async fn wait_for(human: &mut Human, what: &str, seen: impl Fn(&Human) -> bool) {
    for _ in 0..120 {
        human.sync().await;
        if seen(human) {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!(
        "never seen: {what}; items {:?}",
        human.items.iter().map(|x| &x.2).collect::<Vec<_>>()
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs a real Codex (model calls) and writes hooks into CODEX_HOME; see the file's head"]
async fn a_real_codex_with_trommi_hooks() {
    let codex_home = test_codex_home();
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let state = TempDir::new("codex-hooks");
    let project = state.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("README.md"), "# A project\n").unwrap();
    let run = std::env::temp_dir().join(format!("rc{}", trommi_connector::util::random_hex(3)));
    std::fs::create_dir_all(&run).unwrap();
    // the connector as install.sh puts it, in a home of the test's
    let home = state.path().join("home");
    let bin = home.join(".local/share/trommi/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("trommi-connector");
    std::fs::copy(connector_binary(), &program).expect("copied");
    let setup = Setup {
        hub_url: hub.url.clone(),
        program: program.clone(),
        keys: state.path().join("keys"),
        run: run.clone(),
        project: project.clone(),
    };

    // 1. setup codex: the MCP server and the hooks in CODEX_HOME; the hooks are trusted as /hooks does.
    let said = connector_cli(&home, &["setup", "codex"]);
    assert!(said.contains("hooks in"), "{said}");
    eprintln!("{said}");
    let hooks_file = codex_home.join("hooks.json");
    assert!(std::fs::read_to_string(&hooks_file)
        .unwrap()
        .contains(&program.display().to_string()));
    let mut codex = Codex::start(&setup).await;
    let trusted = trust_hooks(&mut codex, &project).await;
    eprintln!("trusted {trusted} hooks");
    codex.close().await;
    // (a Codex started after the trust runs them, and its connector reads them as trusted)
    let mut codex = Codex::start(&setup).await;
    codex.start_thread_with(&project, "on-request").await;
    let thread = codex.thread.clone();

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
    assert!(t.called("connect"), "{t:?}");
    let group = GROUP.lock().unwrap().expect("the session was founded");
    let me = human.seat(&group).await.expect("the agent");

    // 2. The terminal mirror: the typed words and the final answer reach the board without reply.
    let t = codex
        .turn("I tapped \"They match\". Now answer in plain text, without any tool: what is 6 times 7? Only the number.", &mut human)
        .await;
    assert!(!t.called("reply"), "{t:?}");
    wait_for(&mut human, "the typed words mirrored", |h| {
        h.items.iter().any(|(_, s, p)| {
            *s == me
                && p["terminal"] == "input"
                && p["text"].as_str().is_some_and(|x| x.contains("6 times 7"))
        })
    })
    .await;
    wait_for(&mut human, "the final answer mirrored", |h| {
        h.items.iter().any(|(_, s, p)| {
            *s == me
                && p["terminal"] == "answer"
                && p["text"].as_str().is_some_and(|x| x.contains("42"))
        })
    })
    .await;

    // 3. The work trail: a shell step shows as a step of the turn.
    let before = human.trail.len();
    codex
        .turn(
            "Run `ls` in this folder with the shell and tell me the file names.",
            &mut human,
        )
        .await;
    wait_for(&mut human, "a step of the work trail", |h| {
        h.trail.len() > before
    })
    .await;
    eprintln!("trail: {:?}", &human.trail[before..]);

    // 4. A card's answer starts a turn (the wake); its final text goes under that card.
    let t = codex
        .turn("File one Trommi decision card: \"Release today?\" with options yes and no. Then end your turn.", &mut human)
        .await;
    let card = t
        .tools
        .iter()
        .find(|x| x.0 == "create_decision" && !x.3)
        .and_then(|x| x.2.split_whitespace().nth(1).map(String::from))
        .unwrap_or_else(|| panic!("a card: {t:?}"));
    tokio::time::sleep(Duration::from_secs(2)).await;
    human
        .answer(&card, json!({ "answer_action": "answer", "choices": ["no"], "note": "Why not? Answer in plain text (no tool) with one sentence that contains the word FREEZE." }), &["no"], false)
        .await
        .expect("taken");
    codex
        .woken(240, &mut human)
        .await
        .expect("the answer woke Codex");
    wait_for(&mut human, "the answer under the card", |h| {
        h.card_chats.iter().any(|(c, p)| {
            *c == card
                && p["text"]
                    .as_str()
                    .is_some_and(|x| x.to_uppercase().contains("FREEZE"))
        })
    })
    .await;
    assert!(
        !human.items.iter().any(|(_, _, p)| p["terminal"] == "input"
            && p["text"]
                .as_str()
                .is_some_and(|x| x.starts_with("[Trommi]"))),
        "the wake is not mirrored as typed words"
    );

    // 5. A permission prompt reaches the board; the human allows it there and the command runs.
    // (the turn needs a Human only for an admission, which does not come here)
    let mut spare = Human::found(&hub.url).await;
    let marker = codex_home.join(format!("perm-{}", trommi_connector::util::random_hex(3)));
    let ask_text = format!("Create the file {} with `touch` in the shell. It is outside the sandbox: request the escalated permission for it, I will approve.", marker.display());
    let ask = codex.turn_with(
        &ask_text,
        &mut spare,
        |_| false,
    );
    let allow = async {
        for _ in 0..240 {
            human.sync().await;
            let ids: Vec<String> = human
                .object_ids()
                .into_iter()
                .filter(|id| human.has_request(id))
                .collect();
            if let Some(id) = ids.first() {
                human.verdict(id, true).await.expect("allowed");
                return true;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        false
    };
    let (_, asked) = tokio::join!(ask, allow);
    let made = marker.exists();
    let _ = std::fs::remove_file(&marker);
    eprintln!(
        "permission asked on the board: {asked}; file made: {made}; approvals the client saw: {:?}",
        codex.approvals
    );
    codex.close().await;
    let said = connector_cli(&home, &["remove", "codex"]);
    eprintln!("{said}");
    assert!(!std::fs::read_to_string(&hooks_file)
        .unwrap_or_default()
        .contains("trommi-connector"));
    assert!(asked, "the permission request reached the board");
    assert!(made, "allowed on the board, the command ran");
    human.sync().await;
    assert!(human.findings.is_empty(), "{:?}", human.findings);
    let _ = std::fs::remove_dir_all(&run);
    let _ = thread;
}

// ---- soak: a real Codex TUI for hours -----------------------------------------------------------------------------

/// `tmux -L <server> <args…>`.
fn tmux(server: &str, args: &[&str]) -> String {
    let out = std::process::Command::new("tmux")
        .arg("-L")
        .arg(server)
        .args(args)
        .output()
        .expect("tmux runs");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Types a line into the pane as a human does: the text, a pause, Enter (again while the text still stands in the
/// composer: a TUI may take a fast paste's Enter for a new line).
async fn type_line(server: &str, text: &str) {
    tmux(server, &["send-keys", "-t", "soak", "-l", text]);
    let head: String = text.chars().take(30).collect();
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        tmux(server, &["send-keys", "-t", "soak", "Enter"]);
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let screen = tmux(server, &["capture-pane", "-p", "-t", "soak"]);
        let waiting = screen
            .lines()
            .rev()
            .find(|l| l.trim_start().starts_with('›'))
            .is_some_and(|l| l.contains(&head));
        if !waiting {
            return;
        }
    }
}

/// The agent's chat line holding `text`, written by `me`.
async fn said_by(human: &mut Human, me: &trommi_core::ids::DeviceId, text: &str) -> bool {
    human.sync().await;
    human
        .items
        .iter()
        .any(|(_, s, p)| s == me && p["text"].as_str().is_some_and(|t| t.contains(text)))
}

/// One sample of the agent's presence on the hub: (ms since the start, online, hears).
type Sample = (u64, bool, bool);

/// What the hub says of the agents' leases now: online when a lease runs, and whether it hears.
fn presence(db: &Path) -> Option<(bool, bool)> {
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let row: Option<(i64, bool)> = c
        .query_row(
            "SELECT expires_at, hears FROM agent_leases ORDER BY expires_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    Some(match row {
        Some((exp, hears)) => (exp > now, exp > now && hears),
        None => (false, false),
    })
}

/// A real Codex TUI in tmux, its MCP servers in Codex's app-server daemon (as `codex resume` runs them on the
/// owner's desktop), against a local hub, for TROMMI_SOAK_MINUTES (default 120): idle stretches, a turn after
/// each event, one hot swap of the connector, one restart of the app-server daemon, a detach and reattach of the
/// terminal with a resize, and Codex quit and resumed. The agent's presence is sampled on the hub every 2 s and
/// written to TROMMI_SOAK_LOG; no offline stretch may last longer than TROMMI_SOAK_MAX_GAP_S (default 10).
///
/// ```text
/// CODEX_HOME=<test home> TROMMI_SOAK_MINUTES=120 cargo test -p trommi-tests --test connector_codex_real \
///   -- --ignored a_real_codex_tui_soak --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs a real Codex TUI for hours (model calls); see its comment"]
async fn a_real_codex_tui_soak() {
    let codex_home = test_codex_home();
    let minutes: u64 = std::env::var("TROMMI_SOAK_MINUTES")
        .ok()
        .and_then(|m| m.parse().ok())
        .unwrap_or(120);
    let max_gap_ms: u64 = 1000
        * std::env::var("TROMMI_SOAK_MAX_GAP_S")
            .ok()
            .and_then(|m| m.parse().ok())
            .unwrap_or(10);
    let log_file = std::env::var("TROMMI_SOAK_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("trommi-soak.log"));
    let model = std::env::var("CODEX_MODEL").unwrap_or("gpt-6-luna".into());
    let hub = HubProc::start().await;
    let mut human = Human::found(&hub.url).await;
    let state = TempDir::new("codex-soak");
    let project = state.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("README.md"), "# A project\n").unwrap();
    // the connector as install.sh puts it, in a home of the test's; keys under that home
    let home = state.path().join("home");
    let bin = home.join(".local/share/trommi/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("trommi-connector");
    let original = std::fs::read(connector_binary()).expect("the built connector");
    std::fs::write(&program, &original).unwrap();
    std::fs::set_permissions(
        &program,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&home).unwrap());
    let runtime = format!("/run/user/{uid}");
    let started = Instant::now();
    let events: Arc<std::sync::Mutex<Vec<(u64, String)>>> = Default::default();
    let note = |what: &str| {
        let at = started.elapsed().as_millis() as u64;
        eprintln!("[soak {:>7.1} min] {what}", at as f64 / 60_000.0);
        events.lock().unwrap().push((at, what.to_string()));
    };

    // setup codex (MCP server and hooks), the hooks trusted as /hooks does
    let said = connector_cli(&home, &["setup", "codex"]);
    assert!(said.contains("hooks in"), "{said}");
    {
        let run = std::env::temp_dir().join(format!("rs{}", trommi_connector::util::random_hex(3)));
        std::fs::create_dir_all(&run).unwrap();
        let setup = Setup {
            hub_url: hub.url.clone(),
            program: program.clone(),
            keys: home.join(".local/share/trommi/keys"),
            run,
            project: project.clone(),
        };
        let mut codex = Codex::start(&setup).await;
        let n = trust_hooks(&mut codex, &project).await;
        note(&format!("trusted {n} hooks"));
        codex.close().await;
    }

    // Codex's app-server daemon, started as the owner's desktop has it: HOME, XDG_RUNTIME_DIR, CODEX_HOME
    let daemon = |cmd: &str| -> Value {
        let out = std::process::Command::new("codex")
            .args(["app-server", "daemon", cmd])
            .env("HOME", &home)
            .env("CODEX_HOME", &codex_home)
            .env("XDG_RUNTIME_DIR", &runtime)
            .current_dir(&project)
            .output()
            .expect("codex runs");
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Value>(l).ok())
            .unwrap_or(Value::Null)
    };
    let _ = daemon("stop");
    // (a daemon or updater of this Codex home left from an earlier run would start the app server with its own HOME)
    let _ = std::process::Command::new("pkill")
        .args([
            "-f",
            &format!("{}/packages/app-server-daemon", codex_home.display()),
        ])
        .status();
    let up = daemon("start");
    let socket = up["socketPath"]
        .as_str()
        .expect("the daemon's socket")
        .to_string();
    // the app server must run with the test's HOME: its connectors keep their keys under it, never the user's
    let pid = up["pid"].as_u64().expect("the daemon's pid");
    let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
    let want = format!("HOME={}", home.display());
    assert!(
        environ.split(|b| *b == 0).any(|kv| kv == want.as_bytes()),
        "the app-server daemon (pid {pid}) does not run with the test's HOME"
    );
    note(&format!("daemon: {up}"));

    // the TUI in tmux, in a shell of its own pane
    let server = format!("trommi-soak-{}", trommi_connector::util::random_hex(3));
    tmux(
        &server,
        &[
            "new-session",
            "-d",
            "-s",
            "soak",
            "-x",
            "180",
            "-y",
            "50",
            "-c",
            &project.display().to_string(),
            "bash --norc --noprofile",
        ],
    );
    let codex_line = |resume: Option<&str>| {
        format!(
            "env HOME='{}' CODEX_HOME='{}' XDG_RUNTIME_DIR='{}' codex {} --remote 'unix://{}' -m '{}' -c 'approval_policy=\"never\"' -c 'projects.\"{}\".trust_level=\"trusted\"'",
            home.display(),
            codex_home.display(),
            runtime,
            resume.map(|t| format!("resume {t}")).unwrap_or_default(),
            socket,
            model,
            project.display()
        )
    };
    type_line(&server, &codex_line(None)).await;
    tokio::time::sleep(Duration::from_secs(8)).await;

    // connect by link; the human admits while the agent waits
    let invite = human.invite(None).await;
    let link = invite.link.clone();
    *PENDING.lock().unwrap() = Some(invite);
    type_line(
        &server,
        &format!("Connect to Trommi with this invite link: {link}"),
    )
    .await;
    let until = Instant::now() + Duration::from_secs(240);
    while GROUP.lock().unwrap().is_none() && Instant::now() < until {
        human_admits(&mut human).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let group = GROUP.lock().unwrap().expect("the session was founded");
    let me = human.seat(&group).await.expect("the agent");
    note("connected");

    // presence: every 2 s
    let db = hub.data.path().join("hub.db");
    let samples: Arc<std::sync::Mutex<Vec<Sample>>> = Default::default();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = {
        let (samples, stop, db) = (samples.clone(), stop.clone(), db.clone());
        tokio::spawn(async move {
            while !stop.load(std::sync::atomic::Ordering::SeqCst) {
                if let Some((online, hears)) = presence(&db) {
                    samples.lock().unwrap().push((
                        started.elapsed().as_millis() as u64,
                        online,
                        hears,
                    ));
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        })
    };

    // the thread's key, wherever under this home the connector keeps it
    let find_thread = || {
        let mut dirs = vec![home.clone()];
        while let Some(d) = dirs.pop() {
            for f in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let path = f.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.to_string_lossy().ends_with(".owner") {
                    let v: Value =
                        serde_json::from_str(&std::fs::read_to_string(&path).ok()?).ok()?;
                    if let Some(t) = v["owner"].as_str().and_then(|o| o.strip_prefix("codex:")) {
                        return Some(t.to_string());
                    }
                }
            }
        }
        None
    };
    let mut thread = None;
    for _ in 0..120 {
        thread = find_thread();
        if thread.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    let thread = thread.expect("the thread's key");
    note(&format!("thread {thread}"));

    // one turn: the human writes on the board, the agent is woken and answers there
    let mut n = 0u32;
    // A turn: the human writes on the board, the agent is woken and answers (by reply or its final text, which the
    // terminal mirror brings to the board). A request the agent read inside another turn and left may go
    // unanswered once: it is asked again, and the turn counts only if the second ask is answered.
    let mut asked_twice = 0u32;
    macro_rules! turn {
        ($why:expr) => {{
            let mut ok = false;
            for attempt in 0..2 {
                n += 1;
                let want = format!("{}", 7000 + n * 13);
                let text = format!(
                    "Rechne 7000 + {} und schreib nur das Ergebnis in den Trommi-Chat.",
                    n * 13
                );
                human
                    .say(&group, json!({ "content_type": "message", "text": text }))
                    .await
                    .expect("the human writes");
                let until = Instant::now() + Duration::from_secs(240);
                while Instant::now() < until {
                    if said_by(&mut human, &me, &want).await {
                        ok = true;
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                if ok {
                    if attempt > 0 {
                        asked_twice += 1;
                    }
                    break;
                }
            }
            note(&format!(
                "turn after {}: {}",
                $why,
                if ok { "answered" } else { "NO ANSWER" }
            ));
            assert!(ok, "the agent answered after {}", $why);
        }};
    }
    let idle_until =
        |frac: f64| started + Duration::from_secs((minutes as f64 * 60.0 * frac) as u64);
    // (the connect turn ends first: a request it reads inside its own tool calls is not this test's turn)
    tokio::time::sleep(Duration::from_secs(45)).await;
    turn!("connect");
    tokio::time::sleep_until(idle_until(0.15).into()).await;
    turn!("an idle stretch");

    // 1. hot swap
    tokio::time::sleep_until(idle_until(0.2).into()).await;
    let mut changed = original.clone();
    changed.extend_from_slice(b"\nsoak build");
    let new_file = bin.join(".new.trommi-connector");
    std::fs::write(&new_file, &changed).unwrap();
    std::fs::set_permissions(
        &new_file,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    std::fs::rename(&new_file, &program).unwrap();
    note("hot swap: a new connector binary is in place");
    tokio::time::sleep(Duration::from_secs(90)).await;
    turn!("the hot swap");

    // 2. the app-server daemon restarts (as when Codex updates itself)
    tokio::time::sleep_until(idle_until(0.4).into()).await;
    let up = daemon("restart");
    note(&format!("daemon restarted: {}", up["status"]));
    tokio::time::sleep(Duration::from_secs(30)).await;
    let screen = tmux(&server, &["capture-pane", "-p", "-t", "soak"]);
    if !screen.contains("›") {
        note("the TUI ended with the daemon: resumed");
        type_line(&server, &codex_line(Some(&thread))).await;
    }
    tokio::time::sleep(Duration::from_secs(60)).await;
    turn!("the daemon restart");

    // 3. the terminal is detached and reattached, and resized
    tokio::time::sleep_until(idle_until(0.6).into()).await;
    let client = std::process::Command::new("script")
        .args([
            "-qfc",
            &format!("tmux -L {server} attach -t soak"),
            "/dev/null",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    tokio::time::sleep(Duration::from_secs(10)).await;
    tmux(&server, &["detach-client", "-s", "soak"]);
    if let Ok(mut c) = client {
        let _ = c.kill();
    }
    tmux(
        &server,
        &["resize-window", "-t", "soak", "-x", "120", "-y", "40"],
    );
    note("terminal attached, detached and resized");
    tokio::time::sleep(Duration::from_secs(60)).await;
    turn!("the detach");

    // 4. Codex quit and resumed (`codex resume <thread>`)
    tokio::time::sleep_until(idle_until(0.8).into()).await;
    tmux(&server, &["send-keys", "-t", "soak", "C-c"]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    tmux(&server, &["send-keys", "-t", "soak", "C-c"]);
    tokio::time::sleep(Duration::from_secs(5)).await;
    type_line(&server, &codex_line(Some(&thread))).await;
    note("Codex quit and resumed");
    tokio::time::sleep(Duration::from_secs(60)).await;
    turn!("the resume");

    tokio::time::sleep_until(idle_until(1.0).into()).await;
    turn!("the last idle stretch");
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = sampler.await;

    // the presence timeline: transitions and the longest offline stretch after connecting
    let samples = samples.lock().unwrap().clone();
    let mut lines = vec![];
    let mut last: Option<(bool, bool)> = None;
    let (mut gap_from, mut worst) = (None::<u64>, 0u64);
    for (at, online, hears) in &samples {
        if last != Some((*online, *hears)) {
            lines.push(format!(
                "{:>8.1} min  {}{}",
                *at as f64 / 60_000.0,
                if *online { "online" } else { "OFFLINE" },
                if *hears { ", hears" } else { "" }
            ));
            last = Some((*online, *hears));
        }
        match (online, gap_from) {
            (false, None) => gap_from = Some(*at),
            (true, Some(from)) => {
                worst = worst.max(at - from);
                gap_from = None;
            }
            _ => {}
        }
    }
    if let (Some(from), Some((end, _, _))) = (gap_from, samples.last()) {
        worst = worst.max(end - from);
    }
    let mut report = String::new();
    report.push_str(&format!(
        "soak: {minutes} min, {} samples, longest offline stretch {:.1} s, turns asked twice: {asked_twice}\n\nevents:\n",
        samples.len(),
        worst as f64 / 1000.0
    ));
    for (at, what) in events.lock().unwrap().iter() {
        report.push_str(&format!("{:>8.1} min  {what}\n", *at as f64 / 60_000.0));
    }
    report.push_str("\npresence:\n");
    for l in &lines {
        report.push_str(l);
        report.push('\n');
    }
    let _ = std::fs::write(&log_file, &report);
    eprintln!("{report}");

    tmux(&server, &["kill-server"]);
    let _ = daemon("stop");
    connector_cli(&home, &["remove", "codex"]);
    assert!(
        worst <= max_gap_ms,
        "the agent was offline for {:.1} s:\n{report}",
        worst as f64 / 1000.0
    );
}

// ---- delivery soak: two real Codex sessions as the owner runs them, for hours -------------------------------------

/// Milliseconds since the epoch of an RFC 3339 time in UTC as Codex writes it (`2026-10-19T08:54:18.407Z`).
fn rfc3339_ms(t: &str) -> Option<u64> {
    let (date, time) = t.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut parts = time.split(':');
    let (h, mi) = (
        parts.next()?.parse::<i64>().ok()?,
        parts.next()?.parse::<i64>().ok()?,
    );
    let s: f64 = parts.next()?.parse().ok()?;
    // days from the civil date (Howard Hinnant's algorithm)
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + h * 3600 + mi * 60) as f64 * 1000.0 + s * 1000.0) as u64)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// When Codex started turns for queued wakes in `thread` (`[Trommi] …` as the user's message), from its transcript.
fn wake_times(codex_home: &Path, thread: &str) -> Vec<u64> {
    let mut dirs = vec![codex_home.join("sessions")];
    let mut out = vec![];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(&format!("{thread}.jsonl")))
            {
                for line in std::fs::read_to_string(&p).unwrap_or_default().lines() {
                    let Ok(v) = serde_json::from_str::<Value>(line) else {
                        continue;
                    };
                    let pl = &v["payload"];
                    let woke = pl["type"] == "message"
                        && pl["role"] == "user"
                        && pl["content"][0]["text"]
                            .as_str()
                            .is_some_and(|t| t.starts_with("[Trommi]"));
                    if woke {
                        if let Some(at) = v["timestamp"].as_str().and_then(rfc3339_ms) {
                            out.push(at);
                        }
                    }
                }
            }
        }
    }
    out.sort();
    out
}

/// The pids of `pid` and all its descendants.
fn family(pid: u32) -> Vec<u32> {
    let mut out = vec![pid];
    let mut i = 0;
    while i < out.len() {
        let p = out[i];
        if let Ok(kids) = std::fs::read_to_string(format!("/proc/{p}/task/{p}/children")) {
            out.extend(kids.split_whitespace().filter_map(|k| k.parse::<u32>().ok()));
        }
        i += 1;
    }
    out
}

/// The connector servers (`trommi-connector serve`) among `pids`.
fn servers_of(pids: &[u32]) -> Vec<u32> {
    pids.iter()
        .copied()
        .filter(|p| {
            std::fs::read(format!("/proc/{p}/cmdline")).is_ok_and(|c| {
                let args: Vec<String> = c
                    .split(|b| *b == 0)
                    .map(|a| String::from_utf8_lossy(a).into_owned())
                    .collect();
                args.iter().any(|a| a.ends_with("trommi-connector")) && args.iter().any(|a| a == "serve")
            })
        })
        .collect()
}

/// One delivery: what the human sent, when, and when the agent acted on it.
struct Delivery {
    session: &'static str,
    kind: String,
    sent: u64,
    done: Option<u64>,
    woke: Option<u64>,
    note: String,
}

/// One Codex session of the soak: its tmux window, project, thread and the agent's device on the board.
struct Seat2 {
    name: &'static str,
    window: String,
    project: PathBuf,
    remote: bool,
    thread: String,
    group: Option<trommi_core::ids::GroupId>,
    me: Option<trommi_core::ids::DeviceId>,
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let i = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[i.min(sorted.len() - 1)]
}

/// Two real Codex sessions as the owner runs them, against a local hub, for TROMMI_SOAK_MINUTES (default 120):
/// session A is an interactive Codex (`codex`, its MCP servers its own children), session B a Codex attached to the
/// app-server daemon (`codex --remote`, as `codex resume` runs on the owner's desktop). Every few minutes the human
/// writes in the chat, the agent files a card, and the human asks "What??", hands it back and answers it; each
/// asks for a token in the chat, and the time until the agent wrote it is measured, with the time until Codex
/// started the woken turn. Meanwhile: a hot swap of the connector, the hub gone for a minute (a network drop),
/// A's connector stopped for 90 s (SIGSTOP, a laptop's suspend), a message while A is busy in a long turn, a burst to
/// both sessions at once, and A quit and resumed after more than ten minutes while the human wrote to it. The report
/// (delivered/total, p50/p95) goes to TROMMI_SOAK_LOG; every delivery must arrive.
///
/// ```text
/// tests/codex-soak/run.sh            # or:
/// CODEX_HOME=<test home> TROMMI_SOAK_MINUTES=120 cargo test -p trommi-tests --test connector_codex_real \
///   -- --ignored a_real_codex_delivery_soak --nocapture
/// ```
#[tokio::test(flavor = "multi_thread")]
#[ignore = "runs two real Codex sessions for hours (model calls); see its comment"]
async fn a_real_codex_delivery_soak() {
    let codex_home = test_codex_home();
    let minutes: u64 = std::env::var("TROMMI_SOAK_MINUTES")
        .ok()
        .and_then(|m| m.parse().ok())
        .unwrap_or(120);
    let log_file = std::env::var("TROMMI_SOAK_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("trommi-delivery-soak.log"));
    let deadline_s: u64 = std::env::var("TROMMI_SOAK_DELIVERY_S")
        .ok()
        .and_then(|m| m.parse().ok())
        .unwrap_or(300);
    let model = std::env::var("CODEX_MODEL").unwrap_or("gpt-6-luna".into());
    let hub = HubProc::start().await;
    let mut hub = Some(hub);
    let hub_url = hub.as_ref().unwrap().url.clone();
    let mut human = Human::found(&hub_url).await;
    let state = TempDir::new("codex-dsoak");
    let home = state.path().join("home");
    let bin = home.join(".local/share/trommi/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("trommi-connector");
    let original = std::fs::read(connector_binary()).expect("the built connector");
    std::fs::write(&program, &original).unwrap();
    std::fs::set_permissions(&program, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(&home).unwrap());
    let runtime = format!("/run/user/{uid}");
    let started = Instant::now();
    let timeline: Arc<std::sync::Mutex<Vec<(u64, String)>>> = Default::default();
    let note = |what: &str| {
        let at = started.elapsed().as_millis() as u64;
        eprintln!("[dsoak {:>7.1} min] {what}", at as f64 / 60_000.0);
        timeline.lock().unwrap().push((at, what.to_string()));
    };

    // two projects; setup codex (MCP server pointing at the test's connector, with the test hub, and hooks)
    let mut seats = vec![];
    for (name, remote) in [("A", false), ("B", true)] {
        let project = state.path().join(format!("project-{}", name.to_lowercase()));
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("README.md"), format!("# Project {name}\n")).unwrap();
        seats.push(Seat2 {
            name,
            window: format!("s{}", name.to_lowercase()),
            project,
            remote,
            thread: String::new(),
            group: None,
            me: None,
        });
    }
    let said = connector_cli(&home, &["setup", "codex"]);
    assert!(said.contains("hooks in"), "{said}");
    {
        let run = std::env::temp_dir().join(format!("rd{}", trommi_connector::util::random_hex(3)));
        std::fs::create_dir_all(&run).unwrap();
        let setup = Setup {
            hub_url: hub_url.clone(),
            program: program.clone(),
            keys: home.join(".local/share/trommi/keys"),
            run,
            project: seats[0].project.clone(),
        };
        let mut codex = Codex::start(&setup).await;
        let n = trust_hooks(&mut codex, &seats[0].project).await;
        note(&format!("trusted {n} hooks"));
        codex.close().await;
    }
    // the MCP server as setup registered it, but with the test's hub and keys: Codex passes few variables to it
    // TROMMI_SOAK_RESUME_PAUSE_S (default 660) is how long A stays quit; a shorter pause shortens the window of a
    // leaving connector's note with it, so the resumed connector still finds none
    let resume_pause_s: u64 = std::env::var("TROMMI_SOAK_RESUME_PAUSE_S")
        .ok()
        .and_then(|m| m.parse().ok())
        .unwrap_or(660);
    let return_ms = (resume_pause_s * 1000 * 10 / 11).min(600_000);
    let mcp_env = format!(
        "mcp_servers.trommi.env={{TROMMI_CHANNEL_EVENTS=\"off\",TROMMI_CODEX_RETURN_MS=\"{return_ms}\",CODEX_HOME=\"{}\",TROMMI_HUB=\"{}\",TROMMI_KEYS_DIR=\"{}\",HOME=\"{}\"}}",
        codex_home.display(),
        hub_url,
        home.join(".local/share/trommi/keys").display(),
        home.display()
    );

    // the app-server daemon for B, with the test's HOME
    let daemon = |cmd: &str| -> Value {
        let out = std::process::Command::new("codex")
            .args(["app-server", "daemon", cmd])
            .env("HOME", &home)
            .env("CODEX_HOME", &codex_home)
            .env("XDG_RUNTIME_DIR", &runtime)
            .output()
            .expect("codex runs");
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .rev()
            .find_map(|l| serde_json::from_str::<Value>(l).ok())
            .unwrap_or(Value::Null)
    };
    let _ = daemon("stop");
    let _ = std::process::Command::new("pkill")
        .args(["-f", &format!("{}/packages/app-server-daemon", codex_home.display())])
        .status();
    let up = daemon("start");
    let socket = up["socketPath"].as_str().expect("the daemon's socket").to_string();
    let daemon_pid = up["pid"].as_u64().expect("the daemon's pid") as u32;
    note(&format!("daemon: {up}"));

    let server = format!("trommi-dsoak-{}", trommi_connector::util::random_hex(3));
    tmux(&server, &["new-session", "-d", "-s", "dsoak", "-n", "ctl", "-x", "180", "-y", "50", "bash --norc --noprofile"]);
    // (a pane whose Codex quit stays, to be started again)
    tmux(&server, &["set-option", "-g", "remain-on-exit", "on"]);
    let codex_line = |s: &Seat2, resume: Option<&str>| {
        format!(
            "exec env HOME='{}' CODEX_HOME='{}' XDG_RUNTIME_DIR='{}' codex {} {} -m '{}' -c 'approval_policy=\"never\"' -c 'projects.\"{}\".trust_level=\"trusted\"' -c '{}'",
            home.display(),
            codex_home.display(),
            runtime,
            resume.map(|t| format!("resume {t}")).unwrap_or_default(),
            if s.remote { format!("--remote 'unix://{socket}'") } else { String::new() },
            model,
            s.project.display(),
            mcp_env.replace('\'', "'\\''"),
        )
    };
    let start_tui = |s: &Seat2, resume: Option<&str>, fresh: bool| {
        let cmd = format!("bash --norc --noprofile -c \"{}\"", codex_line(s, resume).replace('"', "\\\""));
        if fresh {
            tmux(&server, &["new-window", "-d", "-t", "dsoak", "-n", &s.window, "-c", &s.project.display().to_string(), &cmd]);
        } else {
            tmux(&server, &["respawn-pane", "-k", "-t", &format!("dsoak:{}", s.window), "-c", &s.project.display().to_string(), &cmd]);
        }
    };
    let pane_pid = |s: &Seat2| -> u32 {
        tmux(&server, &["display-message", "-p", "-t", &format!("dsoak:{}", s.window), "#{pane_pid}"])
            .trim()
            .parse()
            .unwrap_or(0)
    };
    let type_into = |s: &Seat2, text: String| {
        let server = server.clone();
        let target = format!("dsoak:{}", s.window);
        async move {
            tmux(&server, &["send-keys", "-t", &target, "-l", &text]);
            let head: String = text.chars().take(30).collect();
            for _ in 0..6 {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                tmux(&server, &["send-keys", "-t", &target, "Enter"]);
                tokio::time::sleep(Duration::from_millis(1500)).await;
                let screen = tmux(&server, &["capture-pane", "-p", "-t", &target]);
                let waiting = screen
                    .lines()
                    .rev()
                    .find(|l| l.trim_start().starts_with('›'))
                    .is_some_and(|l| l.contains(&head));
                if !waiting {
                    return;
                }
            }
        }
    };

    // the threads that own keys, and which TUI runs which
    let owner_threads = || -> Vec<String> {
        let mut out = vec![];
        let mut dirs = vec![home.clone()];
        while let Some(d) = dirs.pop() {
            for f in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let path = f.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.to_string_lossy().ends_with(".owner") {
                    let v: Value = std::fs::read_to_string(&path)
                        .ok()
                        .and_then(|t| serde_json::from_str(&t).ok())
                        .unwrap_or(Value::Null);
                    if let Some(t) = v["owner"].as_str().and_then(|o| o.strip_prefix("codex:")) {
                        if !out.iter().any(|x| x == t) {
                            out.push(t.to_string());
                        }
                    }
                }
            }
        }
        out
    };

    // connect both, one after the other
    for i in 0..seats.len() {
        start_tui(&seats[i], None, true);
        tokio::time::sleep(Duration::from_secs(10)).await;
        let known = owner_threads();
        *GROUP.lock().unwrap() = None;
        let invite = human.invite(None).await;
        let link = invite.link.clone();
        *PENDING.lock().unwrap() = Some(invite);
        type_into(&seats[i], format!("Connect to Trommi with this invite link: {link}")).await;
        let until = Instant::now() + Duration::from_secs(240);
        while GROUP.lock().unwrap().is_none() && Instant::now() < until {
            human_admits(&mut human).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        let group = GROUP.lock().unwrap().expect("the session was founded");
        let me = human.seat(&group).await.expect("the agent");
        let mut thread = None;
        for _ in 0..120 {
            thread = owner_threads().into_iter().find(|t| !known.contains(t));
            if thread.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        seats[i].thread = thread.expect("the thread's key");
        seats[i].group = Some(group);
        seats[i].me = Some(me);
        note(&format!("{} connected: thread {}", seats[i].name, seats[i].thread));
    }
    // (the connect turns end first)
    tokio::time::sleep(Duration::from_secs(45)).await;

    let mut deliveries: Vec<Delivery> = vec![];
    let mut n = 0u32;
    let mut cards: Vec<Option<String>> = vec![None; seats.len()];
    let token = |n: u32, kind: &str| format!("{}{}Q{}", kind.to_uppercase(), n, trommi_connector::util::random_hex(2).to_uppercase());

    // Sends `body` (by `how`) to seat `i` and waits until the agent wrote `tok` anywhere on the board.
    macro_rules! deliver {
        ($i:expr, $kind:expr, $tok:expr, $send:expr) => {{
            let i: usize = $i;
            let sent = now_ms();
            let r: anyhow_like::Res = $send.await.map_err(|e| format!("{e:?}"));
            let mut d = Delivery { session: seats[i].name, kind: $kind.to_string(), sent, done: None, woke: None, note: String::new() };
            if let Err(e) = r {
                d.note = format!("not sent: {e}");
            } else {
                let me = seats[i].me.clone().unwrap();
                let until = Instant::now() + Duration::from_secs(deadline_s);
                while Instant::now() < until {
                    human.sync().await;
                    let seen = human.items.iter().any(|(_, s, p)| *s == me && p.to_string().contains(&$tok));
                    if seen {
                        d.done = Some(now_ms());
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
                d.woke = wake_times(&codex_home, &seats[i].thread).into_iter().find(|t| *t + 1500 >= sent);
                if d.done.is_none() {
                    let screen = tmux(&server, &["capture-pane", "-p", "-t", &format!("dsoak:{}", seats[i].window)]);
                    let tail: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
                    d.note = format!("NOT DELIVERED in {deadline_s} s; screen: {}", tail[tail.len().saturating_sub(6)..].join(" | "));
                }
            }
            note(&format!(
                "{} {} {}: {}",
                d.session,
                d.kind,
                $tok,
                match d.done {
                    Some(t) => format!("{:.1} s (wake {})", (t - d.sent) as f64 / 1000.0, d.woke.map(|w| format!("{:.1} s", w.saturating_sub(d.sent) as f64 / 1000.0)).unwrap_or("-".into())),
                    None => d.note.clone(),
                }
            ));
            let ok = d.done.is_some();
            deliveries.push(d);
            ok
        }};
    }
    mod anyhow_like {
        pub type Res = Result<(), String>;
    }

    // One cycle on seat i: a chat that asks for a card, "What??", a hand-back, the answer.
    macro_rules! cycle {
        ($i:expr) => {{
            let i: usize = $i;
            n += 1;
            let group = seats[i].group.clone().unwrap();
            let title = format!("Frage {n}");
            let tok = token(n, "c");
            let text = format!("Lege mit create_decision eine Entscheidungskarte mit dem Titel \"{title}\" und den Optionen ja und nein an. Schreib danach im Trommi-Chat nur das Wort {tok} und beende deinen Zug.");
            deliver!(i, "chat", tok, human.say(&group, json!({ "content_type": "message", "text": text })));
            human.sync().await;
            cards[i] = human
                .versions
                .iter()
                .rev()
                .find(|(_, p)| p["title"].as_str() == Some(title.as_str()))
                .map(|(id, _)| id.clone());
            if let Some(card) = cards[i].clone() {
                let tok = token(n, "x");
                let text = format!("Was meinst du damit? Schreib im Trommi-Chat nur das Wort {tok}.");
                deliver!(i, "explain", tok, human.card_say(&card, json!({ "content_type": "message", "text": text, "explain": true })));
                let tok = token(n, "h");
                let text = format!("Bitte überarbeiten: schreib im Trommi-Chat nur das Wort {tok}.");
                deliver!(i, "handback", tok, human.card_say(&card, json!({ "content_type": "message", "text": text, "hand_back": true })));
                let tok = token(n, "a");
                let text = format!("Schreib im Trommi-Chat nur das Wort {tok}.");
                deliver!(i, "answer", tok, human.answer(&card, json!({ "answer_action": "answer", "choices": ["ja"], "note": text }), &["ja"], false));
            } else {
                note(&format!("{} filed no card \"{title}\"", seats[i].name));
            }
        }};
    }
    macro_rules! chat {
        ($i:expr, $kind:expr) => {{
            let i: usize = $i;
            n += 1;
            let group = seats[i].group.clone().unwrap();
            let tok = token(n, "m");
            let text = format!("Schreib im Trommi-Chat nur das Wort {tok}.");
            deliver!(i, $kind, tok, human.say(&group, json!({ "content_type": "message", "text": text })))
        }};
    }

    let at = |frac: f64| started + Duration::from_secs((minutes as f64 * 60.0 * frac) as u64);
    // TROMMI_SOAK_ONLY=<n,…> runs only those disturbances (0 swap, 1 drop, 2 suspend, 3 busy, 4 burst, 5 resume),
    // one after the other, with no regular cycles between them
    let only: Option<Vec<usize>> = std::env::var("TROMMI_SOAK_ONLY")
        .ok()
        .map(|v| v.split(',').filter_map(|x| x.trim().parse().ok()).collect());
    let quick = only.is_some();
    let disturbances = [0.12, 0.25, 0.38, 0.5, 0.62, 0.72];
    let disturbances: Vec<f64> = if quick { vec![0.0; 6] } else { disturbances.to_vec() };
    let mut next = 0usize;
    let mut turn = 0usize;
    while Instant::now() < at(1.0) || (quick && next < disturbances.len()) {
        if next < disturbances.len() && Instant::now() >= at(disturbances[next]) {
            if only.as_ref().is_some_and(|o| !o.contains(&next)) {
                next += 1;
                continue;
            }
            match next {
                0 => {
                    let mut changed = original.clone();
                    changed.extend_from_slice(b"\ndelivery soak build");
                    let new_file = bin.join(".new.trommi-connector");
                    std::fs::write(&new_file, &changed).unwrap();
                    std::fs::set_permissions(&new_file, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
                    std::fs::rename(&new_file, &program).unwrap();
                    note("hot swap: a new connector binary is in place");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    chat!(0, "after-swap");
                    chat!(1, "after-swap");
                }
                1 => {
                    let (data, port) = hub.take().unwrap().stop();
                    note("network drop: the hub is gone for 60 s");
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    hub = Some(HubProc::start_in(data, port).await);
                    note("the hub is back");
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    chat!(0, "after-drop");
                    chat!(1, "after-drop");
                }
                2 => {
                    let servers = servers_of(&family(pane_pid(&seats[0])));
                    note(&format!("suspend: A's connector {servers:?} stopped for 90 s"));
                    for p in &servers {
                        let _ = std::process::Command::new("kill").args(["-STOP", &p.to_string()]).status();
                    }
                    let group = seats[0].group.clone().unwrap();
                    n += 1;
                    let tok = token(n, "s");
                    let text = format!("Schreib im Trommi-Chat nur das Wort {tok}.");
                    let sent = now_ms();
                    human.say(&group, json!({ "content_type": "message", "text": text })).await.expect("the human writes");
                    tokio::time::sleep(Duration::from_secs(90)).await;
                    for p in &servers {
                        let _ = std::process::Command::new("kill").args(["-CONT", &p.to_string()]).status();
                    }
                    note("A's connector continues");
                    let me = seats[0].me.clone().unwrap();
                    let until = Instant::now() + Duration::from_secs(deadline_s);
                    let mut done = None;
                    while Instant::now() < until {
                        human.sync().await;
                        if human.items.iter().any(|(_, s, p)| *s == me && p.to_string().contains(&tok)) {
                            done = Some(now_ms());
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    note(&format!("A during-suspend {tok}: {}", done.map(|t| format!("{:.1} s", (t - sent) as f64 / 1000.0)).unwrap_or("NOT DELIVERED".into())));
                    deliveries.push(Delivery { session: "A", kind: "during-suspend".into(), sent, done, woke: wake_times(&codex_home, &seats[0].thread).into_iter().find(|t| *t + 1500 >= sent), note: String::new() });
                }
                3 => {
                    note("busy: A runs a long turn, the human writes meanwhile");
                    type_into(&seats[0], "Run the shell command `sleep 75` and then answer only with: done".into()).await;
                    tokio::time::sleep(Duration::from_secs(10)).await;
                    chat!(0, "while-busy");
                }
                4 => {
                    note("burst: three messages to A and one to B at once");
                    let mut toks = vec![];
                    let sent = now_ms();
                    for i in [0usize, 0, 0, 1] {
                        n += 1;
                        let tok = token(n, "b");
                        let group = seats[i].group.clone().unwrap();
                        human.say(&group, json!({ "content_type": "message", "text": format!("Schreib im Trommi-Chat auch das Wort {tok}.") })).await.expect("the human writes");
                        toks.push((i, tok));
                    }
                    let until = Instant::now() + Duration::from_secs(deadline_s);
                    let mut done: Vec<Option<u64>> = vec![None; toks.len()];
                    while Instant::now() < until && done.iter().any(|d| d.is_none()) {
                        human.sync().await;
                        for (k, (i, tok)) in toks.iter().enumerate() {
                            let me = seats[*i].me.clone().unwrap();
                            if done[k].is_none() && human.items.iter().any(|(_, s, p)| *s == me && p.to_string().contains(tok)) {
                                done[k] = Some(now_ms());
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    for (k, (i, tok)) in toks.iter().enumerate() {
                        note(&format!("{} burst {tok}: {}", seats[*i].name, done[k].map(|t| format!("{:.1} s", (t - sent) as f64 / 1000.0)).unwrap_or("NOT DELIVERED".into())));
                        deliveries.push(Delivery { session: seats[*i].name, kind: "burst".into(), sent, done: done[k], woke: None, note: String::new() });
                    }
                }
                5 => {
                    // Codex quit, the human writes, Codex resumed after more than the ten minutes a leaving connector's
                    // note lasts: nobody types into it, yet it must hear the board.
                    let target = format!("dsoak:{}", seats[0].window);
                    tmux(&server, &["send-keys", "-t", &target, "C-c"]);
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    tmux(&server, &["send-keys", "-t", &target, "C-c"]);
                    note(&format!("A quit; resumed in {resume_pause_s} s"));
                    tokio::time::sleep(Duration::from_secs(resume_pause_s)).await;
                    start_tui(&seats[0], Some(&seats[0].thread.clone()), false);
                    note("A resumed (nothing typed)");
                    tokio::time::sleep(Duration::from_secs(20)).await;
                    chat!(0, "after-resume");
                }
                _ => {}
            }
            next += 1;
            continue;
        }
        if quick && next < disturbances.len() {
            tokio::time::sleep_until(at(disturbances[next]).into()).await;
            continue;
        }
        // a regular cycle, alternating, then an idle stretch of up to three minutes
        cycle!(turn % seats.len());
        turn += 1;
        let idle = 30 + (now_ms() % 150);
        tokio::time::sleep(Duration::from_secs(idle)).await;
    }

    // the report
    let mut report = String::new();
    let total = deliveries.len();
    let delivered = deliveries.iter().filter(|d| d.done.is_some()).count();
    let mut lat: Vec<u64> = deliveries.iter().filter_map(|d| d.done.map(|t| t - d.sent)).collect();
    lat.sort();
    let mut woke: Vec<u64> = deliveries.iter().filter_map(|d| d.woke.map(|w| w.saturating_sub(d.sent))).collect();
    woke.sort();
    report.push_str(&format!(
        "delivery soak: {minutes} min, Codex {} (A interactive, B on the app-server daemon)\ndelivered {delivered}/{total}; until the agent acted: p50 {:.1} s, p95 {:.1} s; until Codex started the woken turn: p50 {:.1} s, p95 {:.1} s ({} wakes)\n\nby kind:\n",
        String::from_utf8_lossy(&std::process::Command::new("codex").arg("--version").output().map(|o| o.stdout).unwrap_or_default()).trim(),
        percentile(&lat, 0.5) as f64 / 1000.0,
        percentile(&lat, 0.95) as f64 / 1000.0,
        percentile(&woke, 0.5) as f64 / 1000.0,
        percentile(&woke, 0.95) as f64 / 1000.0,
        woke.len(),
    ));
    let mut kinds: Vec<String> = deliveries.iter().map(|d| format!("{} {}", d.session, d.kind)).collect();
    kinds.sort();
    kinds.dedup();
    for k in &kinds {
        let of: Vec<&Delivery> = deliveries.iter().filter(|d| &format!("{} {}", d.session, d.kind) == k).collect();
        let mut l: Vec<u64> = of.iter().filter_map(|d| d.done.map(|t| t - d.sent)).collect();
        l.sort();
        report.push_str(&format!(
            "  {k:<22} {}/{}  p50 {:.1} s  p95 {:.1} s\n",
            l.len(),
            of.len(),
            percentile(&l, 0.5) as f64 / 1000.0,
            percentile(&l, 0.95) as f64 / 1000.0
        ));
    }
    report.push_str("\nfailures:\n");
    for d in deliveries.iter().filter(|d| d.done.is_none()) {
        report.push_str(&format!("  {:>7.1} min {} {}: {}\n", (d.sent.saturating_sub(now_ms() - started.elapsed().as_millis() as u64)) as f64 / 60_000.0, d.session, d.kind, d.note));
    }
    report.push_str("\ntimeline:\n");
    for (at, what) in timeline.lock().unwrap().iter() {
        report.push_str(&format!("{:>8.1} min  {what}\n", *at as f64 / 60_000.0));
    }
    let _ = std::fs::write(&log_file, &report);
    eprintln!("{report}");

    tmux(&server, &["kill-server"]);
    let _ = daemon("stop");
    let _ = daemon_pid;
    connector_cli(&home, &["remove", "codex"]);
    assert_eq!(delivered, total, "every delivery arrived:\n{report}");
}
