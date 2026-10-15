//! The commands beside the MCP server: join, say, the plugin's hooks, the monitor, the witness, whoami.
use crate::bridge::{About, Bridge};
use crate::driver::BoxFut;
use crate::door::{knock, ring};
use crate::error::{Fault, Result};
use crate::hooks::{hook_output, hook_request};
use crate::member::{self, command_slots, connector_config, paths_of, resolve_room, Member};
use crate::server::{env_ms, say_with};
use crate::slots::*;
use crate::util::now_ms;
use serde_json::{json, Value};
use std::sync::Arc;

/// `join <link>`: join a room with an agent invite link (or TROMMI_INVITE), then exit.
pub async fn join(given: Option<String>) -> Result<String> {
    let link = given
        .or_else(|| member::env("TROMMI_INVITE"))
        .ok_or_else(|| {
            Fault::plain(
                "usage: trommi-connector connect '<invite link>' (the link from \"Invite an agent\" in the Trommi app)",
            )
        })?;
    let m = Member::new(connector_config(None, true));
    eprintln!("[trommi] joining; confirm this session in the Trommi app");
    let replace_held = std::env::var("TROMMI_JOIN_OTHER").as_deref() != Ok("1");
    if let Err(e) = m.join(&link, replace_held, true).await {
        m.stop().await;
        let spent = [
            "invite-expired",
            "invite-used",
            "invite-burned",
            "invite-contested",
            "bad-invite",
        ]
        .contains(&e.code.as_str());
        return Err(Fault::plain(format!(
            "not joined: {}{}",
            e.text(),
            if spent && !e.text().contains("make a new") {
                " (an invite link works once and for a limited time: make a new one in the app)"
            } else {
                ""
            }
        )));
    }
    let c = m.client().unwrap();
    let (room, sid) = {
        let core = c.lock();
        (c.room_id().to_base64url(), core.session_id())
    };
    let sid = sid
        .or_else(|| {
            m.me.lock()
                .unwrap()
                .session
                .as_ref()
                .and_then(|s| s["agent_session_id"].as_str().map(String::from))
        })
        .unwrap_or_else(|| "undefined".into());
    let kf = m
        .paths()
        .map(|p| p.key_file.display().to_string())
        .unwrap_or_default();
    let out = format!(
        "joined room {room} as device {}, session {sid}; key file {kf}",
        c.me()
    );
    // What this process wrote (the device's name) goes out before it ends; then the key is handed to the MCP
    // server of the Claude Code (or Codex) session whose shell ran this, which goes online at once.
    let _ = c.settle(3000).await;
    // (the ring is not waited for: this process holds the device's state until it ends, and the server takes
    // it once this process is gone)
    let up = crate::hooks::ancestors(8);
    let running = up.iter().any(|p| crate::door::bell_path(*p).exists());
    if !running {
        // no server takes over: the lease is given up, the device shows offline until a session starts here
        let _ = tokio::time::timeout(
            std::time::Duration::from_millis(1500),
            c.report_link(json!({ "hears": false, "working": false, "leaving": true })),
        )
        .await;
    }
    m.stop().await;
    if running {
        let _ = ring(&up, 300).await;
    }
    Ok(if running {
        format!("{out}\nThe Trommi MCP server of this session goes online with this key now; its board tools appear in a moment.")
    } else {
        format!("{out}\nNow start claude (or codex) in this folder. A Claude Code that runs here already: /mcp, then trommi, then Reconnect; a Codex that runs here already: quit it and start it again (`codex resume` keeps the conversation).")
    })
}

fn not_in_room(phase: &str) -> String {
    match phase {
        "waiting-session" => {
            "the agent is not assigned to a session yet (the human does that in the Trommi app)"
                .into()
        }
        "needs-invite" => "no free key of this folder".into(),
        p => format!("not in the room ({p})"),
    }
}

/// `say`: the emergency side channel, never a second writer on a key.
#[allow(unused_assignments)] // (as sayCli: a door's error is kept until the own try replaces it)
pub async fn say(argv: &[String]) -> Result<String> {
    let mut urgent = false;
    let mut session: Option<String> = None;
    let mut words = vec![];
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--urgent" => urgent = true,
            "--session" => {
                i += 1;
                session = argv.get(i).cloned();
            }
            w => words.push(w.to_string()),
        }
        i += 1;
    }
    let text = crate::util::js_trim(&words.join(" ")).to_string();
    if text.is_empty() {
        return Err(Fault::plain(
            "usage: trommi-connector say \"<text>\" [--session <name>] [--urgent]",
        ));
    }
    let mut cfg = connector_config(None, true);
    cfg.invite = String::new();
    cfg.session = format!("say:{}", pid());
    let Some(room_id) = resolve_room(&cfg)? else {
        return Err(Fault::plain(format!(
            "this folder ({}) has no Trommi key: it is not in a room",
            cfg.shown
        )));
    };
    let until = now_ms() + env_ms("TROMMI_SAY_MS", 30_000).max(1);
    // (the owner: a running connector of another agent session in this folder refuses, so that these words never
    // go into that session's chat)
    let request = json!({ "op": "say", "text": text, "session": session, "urgent": urgent, "owner": cfg.owner() });
    let mut last: Option<Fault>;
    loop {
        let mut silent = vec![];
        for p in command_slots(&cfg, &room_id) {
            let holders = holders_of(&p);
            if holders.is_empty() {
                continue;
            }
            match knock(&p, &request, until.saturating_sub(now_ms()).max(1000)).await {
                Ok(r) if r["ok"] == Value::Bool(true) => {
                    return Ok(format!(
                        "said through the running connector (pid {}): {}",
                        holders
                            .iter()
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        crate::util::js_string(&r["said"])
                    ))
                }
                Ok(r) => last = Some(Fault::plain(crate::util::js_string(&r["error"]))),
                Err(e) => {
                    silent.extend(holders);
                    last = Some(Fault::plain(e));
                }
            }
        }
        let m = Member::new(cfg.clone());
        let res: Result<String> = async {
            let left = until.saturating_sub(now_ms()).max(1000);
            let m2 = m.clone();
            let h = tokio::spawn(async move { m2.open(false).await });
            let _ = tokio::time::timeout(std::time::Duration::from_millis(left), h).await;
            if m.phase() == "ready" {
                let (client, storage, cache) = {
                    let me = m.me.lock().unwrap();
                    (me.client.clone().unwrap(), me.storage.clone().unwrap(), me.paths.clone().unwrap().cache)
                };
                let state = storage.get("channel").and_then(|v| v.as_object().cloned()).unwrap_or_default();
                let st2 = storage.clone();
                let notify: crate::bridge::Notify = Arc::new(|_: String, _: Value, _: Option<About>| -> BoxFut<'static, ()> { Box::pin(async {}) });
                let b = Bridge::new(client.clone(), notify, cache, state, Arc::new(move |v| st2.set("channel", v)));
                let said = say_with(b, request.clone()).await?;
                client.settle(until.saturating_sub(now_ms()).max(5000)).await?;
                return Ok(format!("said as this folder's agent (slot {}): {said}", m.paths().map(|p| p.slot).unwrap_or(0)));
            }
            let p = m.paths();
            if p.as_ref().is_some_and(|p| !p.has_key) && !silent.is_empty() {
                let list = silent.iter().map(|x| format!("pid {x}")).collect::<Vec<_>>().join(", ");
                let kill = silent.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" ");
                return Err(Fault::plain(format!("the key is held by connector process {list}, which does not answer (an older connector without the side channel, or a hung one). It is never overridden (two writers on one key break its chain): run `kill {kill}`, then say again")));
            }
            if p.as_ref().is_some_and(|p| !p.has_key && !p.foreign.is_empty()) {
                return Err(Fault::plain("this folder's Trommi key belongs to another agent session: this session is a member of its own and needs an invite of its own (Trommi app: \"invite an agent\")"));
            }
            let e = m.me.lock().unwrap().error.clone();
            Err(Fault::plain(e.unwrap_or_else(|| not_in_room(&m.phase()))))
        }
        .await;
        m.stop().await;
        match res {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
        if now_ms() >= until {
            return Err(last.unwrap_or_else(|| Fault::plain("not said")));
        }
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
}

/// The plugin's hooks: reads the hook's JSON on stdin, asks the running connector of this Claude Code session.
pub async fn hook(kind: &str) -> String {
    let mut text = String::new();
    let _ = tokio::io::AsyncReadExt::read_to_string(&mut tokio::io::stdin(), &mut text).await;
    let input: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let Some(request) = hook_request(kind, &input) else {
        return String::new();
    };
    let folder = member::env("TROMMI_FOLDER")
        .or_else(|| member::env("CLAUDE_PROJECT_DIR"))
        .or_else(|| {
            input
                .get("cwd")
                .and_then(|v| v.as_str())
                .filter(|x| !x.is_empty())
                .map(String::from)
        });
    let mut cfg = connector_config(folder, false);
    cfg.invite = String::new();
    let Ok(Some(room_id)) = resolve_room(&cfg) else {
        return String::new();
    };
    let pids: Vec<u32> = request["ancestors"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().map(|v| v as u32))
                .collect()
        })
        .unwrap_or_default();
    ring(&pids, 15_000).await;
    let wait = request.get("wait_ms").and_then(|v| v.as_u64()).unwrap_or(0);
    for p in command_slots(&cfg, &room_id) {
        let n = p.slot;
        if holders_of(&p).is_empty() {
            continue;
        }
        match knock(&p, &request, wait + 15_000).await {
            Ok(a) if a["ok"] == Value::Bool(true) => return hook_output(kind, &a),
            Ok(a) => eprintln!(
                "[trommi] hook {kind}: slot {n}: {}",
                crate::util::js_string(&a["error"])
            ),
            Err(e) => eprintln!("[trommi] hook {kind}: slot {n}: {e}"),
        }
    }
    String::new()
}

/// The hooks of the terminal mirror (`prompt`: UserPromptSubmit, `stop`: Stop): one line to the connector of this
/// Claude Code process through its bell, which answers at once. Prints nothing, fails silently, never waits long.
pub async fn mirror_hook(kind: &str) {
    if !crate::mirror::enabled() {
        return;
    }
    let mut bytes = vec![];
    let _ = tokio::io::AsyncReadExt::read_to_end(
        &mut tokio::io::AsyncReadExt::take(tokio::io::stdin(), 8 * 1024 * 1024),
        &mut bytes,
    )
    .await;
    let input: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let pids = crate::hooks::ancestors(8);
    let Some(request) = crate::mirror::hook_request(kind, &input, &pids) else {
        return;
    };
    for p in &pids {
        let f = crate::door::bell_path(*p);
        if f.exists() {
            let _ = crate::door::knock_at(&f, &request, crate::mirror::HOOK_MS).await;
            return;
        }
    }
}

/// The hooks of a turn's trail (PreToolUse, PostToolUse, PostToolUseFailure, MessageDisplay, SubagentStart,
/// SubagentStop, SessionEnd, StopFailure; trail.rs): one line to the connector of this Claude Code process through
/// its bell. Prints nothing, fails silently, never waits long.
pub async fn trail_hook() {
    let level = crate::mirror::level();
    if level < crate::mirror::Level::Steps {
        return;
    }
    let mut bytes = vec![];
    let _ = tokio::io::AsyncReadExt::read_to_end(
        &mut tokio::io::AsyncReadExt::take(tokio::io::stdin(), 8 * 1024 * 1024),
        &mut bytes,
    )
    .await;
    let input: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    let pids = crate::hooks::ancestors(8);
    let Some(request) = crate::trail::hook_request(&input, &pids, level) else {
        return;
    };
    for p in &pids {
        let f = crate::door::bell_path(*p);
        if f.exists() {
            let _ = crate::door::knock_at(&f, &request, crate::mirror::HOOK_MS).await;
            return;
        }
    }
}

/// "Cut off", said once in the name of a key nobody holds (the folder watch).
pub async fn last_word(cfg: &member::Cfg, room_id: &str, t: &Value) -> bool {
    let Some(slot) = t.get("slot").and_then(|v| v.as_u64()) else {
        return false;
    };
    let owner = t.get("owner").and_then(Value::as_str).unwrap_or("");
    let p = if owner.is_empty() {
        paths_of(cfg, room_id, slot as u32)
    } else {
        let Some((_, p)) = member::own_slots(cfg, owner).into_iter().find(|(r, p)| r == room_id && u64::from(p.slot) == slot) else { return false };
        p
    };
    if !p.key_file.exists() || !lock_slot(&p, &format!("witness:{}", pid())) {
        return false;
    }
    let r: Result<()> = async {
        // The hub itself notices a connector that is gone: its lease runs out after 60 s (spec/v1.md 13.7), and
        // the board shows the agent as lost. What is left to the witness is to note that the loss was seen.
        let mut mark = t.as_object().cloned().unwrap_or_default();
        let file = mark.remove("file").and_then(|f| f.as_str().map(String::from)).unwrap_or_default();
        mark.insert("told".into(), json!(true));
        let _ = write_private(std::path::Path::new(&file), &Value::Object(mark).to_string());
        eprintln!("[trommi] said for the session of Claude Code pid {}: cut off (its connector, pid {}, is gone)", t["claude_pid"], t["pid"]);
        Ok(())
    }
    .await;
    unlock_slot(&p);
    match r {
        Ok(()) => true,
        Err(e) => {
            eprintln!("[trommi] cut off not said: {}", e.text());
            false
        }
    }
}

/// `witness <session>`: waits out the grace; if the session is then cut off and nobody holds its key, says so.
pub async fn witness(session: Option<String>) {
    let mut cfg = connector_config(None, false);
    cfg.invite = String::new();
    let (Ok(Some(room_id)), Some(session)) = (resolve_room(&cfg), session) else {
        return;
    };
    let dir = cfg.keys_dir.join(&room_id);
    let grace = env_ms("TROMMI_CUT_GRACE_MS", 20_000);
    let idle = env_ms("TROMMI_IDLE_MS", 1_800_000);
    let until = now_ms() + grace + 60_000;
    loop {
        let w = folder_watch(&dir, &cfg.base, now_ms(), grace, idle);
        let in_cut = w
            .cut
            .iter()
            .find(|m| crate::util::js_string(&m["session"]) == session)
            .cloned();
        let in_pending = w
            .pending
            .iter()
            .any(|m| crate::util::js_string(&m["session"]) == session);
        match in_cut {
            Some(t) => {
                if t.get("slot").is_some_and(|s| !s.is_null())
                    && !t.get("told").is_some_and(crate::util::truthy)
                {
                    last_word(&cfg, &room_id, &t).await;
                }
                return;
            }
            None if !in_pending => return,
            None => {}
        }
        if now_ms() > until {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(
            (grace / 4).clamp(50, 1000),
        ))
        .await;
    }
}

/// `whoami`: room, key file and folder.
pub fn whoami() -> Result<String> {
    let cfg = connector_config(None, true);
    let room = resolve_room(&cfg)?;
    let p = room.as_ref().map(|r| command_slots(&cfg, r).into_iter().next().unwrap_or_else(|| paths_of(&cfg, r, 1)));
    Ok(serde_json::to_string_pretty(&json!({
        "room_id": room, "key_file": p.as_ref().map(|p| p.key_file.display().to_string()), "has_key": p.as_ref().is_some_and(|p| p.key_file.exists()),
        "folder": cfg.shown, "host": cfg.host,
    })).unwrap())
}

/// `allow-tools`: the plugin's board tools allowed in this folder (.claude/settings.local.json), as the connect
/// script asks for.
pub fn allow_tools() -> std::io::Result<()> {
    let f = std::path::Path::new(".claude/settings.local.json");
    std::fs::create_dir_all(".claude")?;
    let mut j: Value = std::fs::read_to_string(f)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.is_object())
        .unwrap_or_else(|| json!({}));
    if !j["permissions"].is_object() {
        j["permissions"] = json!({});
    }
    if !j["permissions"]["allow"].is_array() {
        j["permissions"]["allow"] = json!([]);
    }
    let rule = json!("mcp__plugin_trommi_trommi");
    let a = j["permissions"]["allow"].as_array_mut().unwrap();
    if !a.contains(&rule) {
        a.push(rule);
    }
    std::fs::write(f, serde_json::to_string_pretty(&j).unwrap() + "\n")
}

/// `claim-key`: the key that the running connector of this agent session holds becomes this session's
/// (`cc:<CLAUDE_CODE_SESSION_ID>` or `key:<TROMMI_SESSION_KEY>`), so that no other session takes it while this
/// one's connector restarts. Only a key nobody owns yet; never one of another session, never a takeover.
pub async fn claim_key() -> Result<String> {
    let mut cfg = connector_config(None, false);
    cfg.invite = String::new();
    if cfg.owner().is_empty() {
        return Err(Fault::plain(
            "no agent session here: run it from the Claude Code session whose key it is (CLAUDE_CODE_SESSION_ID) or with TROMMI_SESSION_KEY",
        ));
    }
    let Some(room_id) = resolve_room(&cfg)? else {
        return Err(Fault::plain(format!(
            "this folder ({}) has no Trommi key",
            cfg.shown
        )));
    };
    let Some(p) = member::own_held_slot(&cfg, &room_id).await else {
        return Err(Fault::plain(
            "no connector of this session holds a key of this folder: call a Trommi tool first, then run this again",
        ));
    };
    let record = owner_of(&p);
    if owned_by(record.as_ref(), &cfg.owner(), 0) {
        return Ok(format!("slot {} is this session's already", p.slot));
    }
    let unowned = record.as_ref().is_none_or(|r| {
        !r.get("owner").is_some_and(crate::util::truthy)
            && r.get("claude_pid").is_none_or(|v| v.is_null())
    });
    if !unowned {
        return Err(Fault::plain(format!(
            "slot {} has an owner already; it stays as it is",
            p.slot
        )));
    }
    write_owner(&p, &owner_record(&cfg.owner(), None));
    Ok(format!(
        "slot {} belongs to this session now ({})",
        p.slot,
        p.key_file.display()
    ))
}

/// Dispatch a command line; Some(exit code) when it was one of the commands.
pub async fn run(argv: &[String]) -> i32 {
    let cmd = argv.first().map(|s| s.as_str()).unwrap_or("");
    let given = argv.get(1).cloned();
    match cmd {
        "join" | "connect" => match join(given).await {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("[trommi] {}", e.text());
                1
            }
        },
        "claim-key" => match claim_key().await {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("[trommi] {}", e.text());
                1
            }
        },
        "say" => match say(&argv[1..]).await {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("[trommi] {}", e.text());
                1
            }
        },
        "permission" | "notice" | "denied" | "resolved" => {
            let out = hook(cmd).await;
            if !out.is_empty() {
                println!("{out}");
            }
            0
        }
        "prompt" | "stop" => {
            mirror_hook(cmd).await;
            0
        }
        "trail" => {
            trail_hook().await;
            0
        }
        "monitor" => {
            crate::door::run_monitor().await;
            0
        }
        "witness" => {
            witness(given).await;
            0
        }
        "whoami" => match whoami() {
            Ok(s) => {
                println!("{s}");
                0
            }
            Err(e) => {
                eprintln!("[trommi] {}", e.text());
                1
            }
        },
        "allow-tools" => match allow_tools() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("[trommi] {e}");
                1
            }
        },
        "setup" => crate::setup::run_cli(given.as_deref(), argv.get(2..).unwrap_or_default()),
        "remove" => crate::setup::remove_cli(given.as_deref(), argv.get(2..).unwrap_or_default()),
        "statusline" => crate::usage::statusline(&argv[1..]),
        "update" => match crate::update::update(given.as_deref() == Some("--check")).await {
            Ok(said) => {
                println!("{said}");
                0
            }
            Err(why) => {
                eprintln!("[trommi] {why}");
                1
            }
        },
        "--version" | "version" => {
            println!(
                "trommi-connector {} ({}, {}; {})",
                crate::server::SERVER_VERSION,
                crate::CLIENT,
                crate::server::disk_version(),
                crate::update::standing(&crate::update::verify_file(&crate::server::self_path()))
            );
            0
        }
        other => {
            eprintln!("[trommi] unknown command {other}; use connect <link>, say \"<text>\" [--session <name>] [--urgent], whoami, setup claude|codex, remove claude|codex, update [--check] or --version, or no argument for the MCP server");
            1
        }
    }
}
