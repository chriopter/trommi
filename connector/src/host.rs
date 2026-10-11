//! Which program runs this connector: Claude Code or Codex. Codex is known by its MCP client name in
//! `initialize` (`codex-mcp-client`), or by the program that started this process. What differs:
//!
//! - Codex shows no channel events and does not ask for the tool list again when it changes: a session that is
//!   in no room yet lists every board tool from the start, beside `connect` (`server.rs`).
//! - Codex can be woken: `codex queue --thread <id> --message <text>` starts a turn in an idle session (or after
//!   the running one). Every tool call carries the thread's id in `_meta`; with it, a new board event wakes the
//!   session ([`wake`]), so it hears the board at once, as Claude Code with channels does.
//! - The words for a restart differ: Codex is restarted (or a new session started), Claude Code reconnects in
//!   /mcp.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static CODEX: AtomicBool = AtomicBool::new(false);
static THREAD: Mutex<Option<String>> = Mutex::new(None);

/// Whether Codex runs this connector.
pub fn is_codex() -> bool {
    CODEX.load(Ordering::SeqCst)
}

pub fn set_codex() {
    CODEX.store(true, Ordering::SeqCst);
}

/// Whether an MCP client's `initialize` names Codex (`clientInfo.name` "codex-mcp-client", or a title "Codex").
pub fn client_is_codex(params: &serde_json::Value) -> bool {
    let info = &params["clientInfo"];
    ["name", "title"].iter().any(|k| {
        info[k]
            .as_str()
            .is_some_and(|v| v.to_ascii_lowercase().starts_with("codex"))
    })
}

/// Whether a command line (the parent's) is Codex's.
pub fn args_are_codex(args: &[String]) -> bool {
    let codex = regex::Regex::new(r"(^|[\\/])codex(\.exe)?$").unwrap();
    args.iter().take(2).any(|a| codex.is_match(a))
}

/// The id of the Codex thread this connector serves, from a tool call's `_meta` (`threadId`, or
/// `x-codex-turn-metadata.thread_id`). Returns whether it is new.
pub fn note_call_meta(meta: &serde_json::Value) -> bool {
    let id = meta["threadId"]
        .as_str()
        .or_else(|| meta["x-codex-turn-metadata"]["thread_id"].as_str())
        .filter(|id| valid_thread(id));
    let Some(id) = id else { return false };
    set_codex();
    let mut t = THREAD.lock().unwrap();
    if t.as_deref() == Some(id) {
        return false;
    }
    *t = Some(id.to_string());
    true
}

/// A thread id as Codex writes it: a UUID (hex and dashes), nothing a command line could take for an option.
fn valid_thread(id: &str) -> bool {
    (8..=64).contains(&id.len())
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && !id.starts_with('-')
}

/// The Codex thread this connector serves, once a tool call named it.
pub fn thread() -> Option<String> {
    THREAD.lock().unwrap().clone()
}

/// Whether waking is off (`TROMMI_CODEX_WAKE=off`).
pub fn wake_off() -> bool {
    matches!(
        std::env::var("TROMMI_CODEX_WAKE").as_deref(),
        Ok("off" | "0")
    )
}

/// The Codex program to queue a message with: `TROMMI_CODEX_BIN`, the parent's program when it is Codex, or
/// `codex` on the PATH.
pub fn codex_program() -> std::path::PathBuf {
    if let Some(p) = std::env::var_os("TROMMI_CODEX_BIN").filter(|p| !p.is_empty()) {
        return p.into();
    }
    let parent = crate::slots::ppid();
    if let Ok(exe) = std::fs::read_link(format!("/proc/{parent}/exe")) {
        let exe =
            std::path::PathBuf::from(exe.display().to_string().trim_end_matches(" (deleted)"));
        if exe
            .file_name()
            .is_some_and(|n| n == "codex" || n == "codex.exe")
        {
            return exe;
        }
    }
    let args = crate::slots::args_of(parent);
    if let Some(first) = args.first().filter(|_| args_are_codex(&args)) {
        if first.contains('/') {
            return first.into();
        }
    }
    "codex".into()
}

/// The Codex home of the Codex that runs this connector: `CODEX_HOME` as `setup codex` registered it with the
/// server, else the one in the environment of the Codex process (Linux), else none (Codex's default, ~/.codex).
pub fn codex_home() -> Option<std::ffi::OsString> {
    if let Some(h) = std::env::var_os("CODEX_HOME").filter(|h| !h.is_empty()) {
        return Some(h);
    }
    let environ = std::fs::read(format!("/proc/{}/environ", crate::slots::ppid())).ok()?;
    environ
        .split(|b| *b == 0)
        .find_map(|kv| kv.strip_prefix(b"CODEX_HOME="))
        .filter(|v| !v.is_empty())
        .map(|v| std::ffi::OsString::from(String::from_utf8_lossy(v).into_owned()))
}

/// Whether this session can be woken now: Codex, a thread known, waking not turned off.
pub fn can_wake() -> bool {
    is_codex() && !wake_off() && thread().is_some()
}

/// Wakes the Codex session: queues `text` as a message for its thread. Returns whether Codex took it.
pub async fn wake(text: &str) -> bool {
    let Some(thread) = thread() else { return false };
    let mut cmd = tokio::process::Command::new(codex_program());
    // (Codex starts MCP servers with few of its variables: the queue must go to the home of the Codex that runs
    // this session, not to ~/.codex)
    if let Some(home) = codex_home() {
        cmd.env("CODEX_HOME", home);
    }
    cmd.args(["queue", "--thread", &thread, "--message", text])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let run = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output()).await;
    match run {
        Ok(Ok(out)) if out.status.success() => true,
        Ok(Ok(out)) => {
            eprintln!(
                "[trommi] codex queue failed ({}): {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            );
            false
        }
        Ok(Err(e)) => {
            eprintln!("[trommi] codex queue did not start: {e}");
            false
        }
        Err(_) => {
            eprintln!("[trommi] codex queue did not answer in 30 s");
            false
        }
    }
}

/// The program's name, for texts the human reads.
pub fn name() -> &'static str {
    if is_codex() {
        "Codex"
    } else {
        "Claude Code"
    }
}

/// How the human restarts this session's connector.
pub fn restart_line() -> &'static str {
    if is_codex() {
        "Quit Codex and start it again in this folder (`codex resume` keeps the conversation)."
    } else {
        "In the terminal of this Claude Code session: /mcp, then trommi, then Reconnect."
    }
}

/// How a newer connector is installed and loaded.
pub fn update_line() -> &'static str {
    if is_codex() {
        "run `trommi-connector update` in a terminal, then quit Codex and start it again (`codex resume` keeps the conversation)"
    } else {
        "update the trommi plugin (or run the connect script again), then /mcp → trommi → Reconnect"
    }
}
