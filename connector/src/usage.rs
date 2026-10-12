//! The account and usage of the program that runs this connector, for the board: which plan, which e-mail, how much
//! of the 5-hour window and of the week is used and when each starts again; for Codex also its credits and free full
//! resets. They travel like the rest of the agent's profile: in the main session's `profile` register (`account`),
//! end-to-end encrypted; the hub sees only that a register changed. Never logged.
//!
//! Where the numbers come from (no credential file of another program is read):
//!
//! - **Claude Code**: `claude auth status --json` names the e-mail and the plan (`subscriptionType`). The windows
//!   Claude Code hands only to a status line command (`rate_limits` in the JSON on its stdin). So `setup claude`
//!   (and the installed connector once at its start) puts `trommi-connector statusline` in as the status line of
//!   Claude Code's user settings ([`ensure_statusline`]): it keeps the windows in a file of this user
//!   (`~/.local/share/trommi/usage/claude-<config>.json`, 0600) and runs a status line that was there before with the
//!   same input, its output passed on unchanged. `trommi-connector remove claude` puts the one before back.
//! - **Codex**: its app server (`codex app-server`, JSON-RPC on stdio): `account/read` and `account/rateLimits/read`.
//!   Without Codex nothing is sent.
//!
//! What is sent changes only when a value does; it is looked at about once a minute ([`Poller`]).
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The status line command is ours when it holds this.
const MARK: &str = "exec \"$p\" statusline";
const PROGRAM: &str = "p=\"$HOME/.local/share/trommi/bin/trommi-connector\"";

// ---- the account, as it travels -----------------------------------------------------------------------------------

/// A window: its used percent (0-100) and when it starts again (ms), or None when it already did.
fn window(used: Option<f64>, resets_at_ms: Option<u64>, now_ms: u64) -> Option<Value> {
    let used = used.filter(|u| u.is_finite())?;
    if resets_at_ms.is_some_and(|at| at <= now_ms) {
        return None;
    }
    let mut w = Map::new();
    w.insert("used".into(), json!(used.clamp(0.0, 100.0).round() as u64));
    if let Some(at) = resets_at_ms {
        w.insert("resets_at".into(), json!(at));
    }
    Some(Value::Object(w))
}

/// Claude Code's e-mail and plan from `claude auth status --json`: only for a claude.ai subscription.
pub fn claude_account(status: &Value) -> Option<(String, String)> {
    if status["loggedIn"] != json!(true) || status["authMethod"].as_str() != Some("claude.ai") {
        return None;
    }
    let plan = status["subscriptionType"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let email = status["email"].as_str().unwrap_or("").to_string();
    (!plan.is_empty() || !email.is_empty()).then_some((plan, email))
}

/// The windows of a status line's input (`rate_limits`: `five_hour`, `seven_day`, each `used_percentage` and
/// `resets_at` in seconds), as the board keeps them.
pub fn claude_windows(rate_limits: &Value, now_ms: u64) -> Map<String, Value> {
    let mut out = Map::new();
    for (from, to) in [("five_hour", "five_hour"), ("seven_day", "weekly")] {
        let w = &rate_limits[from];
        let at = w["resets_at"]
            .as_f64()
            .filter(|s| *s > 0.0)
            .map(|s| (s * 1000.0) as u64);
        if let Some(v) = window(w["used_percentage"].as_f64(), at, now_ms) {
            out.insert(to.into(), v);
        }
    }
    out
}

/// The account of Claude Code: what `auth status` said and the windows the status line kept.
pub fn claude(plan: &str, email: &str, windows: Map<String, Value>) -> Value {
    let mut a = Map::new();
    a.insert("kind".into(), json!("claude"));
    a.insert("plan".into(), json!(plan));
    a.insert("email".into(), json!(email));
    a.extend(windows);
    Value::Object(a)
}

/// The account of Codex from its app server's `account/read` and `account/rateLimits/read` results. A window of up
/// to six hours is the 5-hour one, one of six days or more the week.
pub fn codex(account: &Value, limits: &Value, now_ms: u64) -> Option<Value> {
    let acc = &account["account"];
    if acc["type"].as_str() != Some("chatgpt") {
        return None;
    }
    let rl = &limits["rateLimits"];
    let plan = acc["planType"]
        .as_str()
        .or(rl["planType"].as_str())
        .unwrap_or("");
    let email = acc["email"].as_str().unwrap_or("");
    let mut a = Map::new();
    a.insert("kind".into(), json!("codex"));
    a.insert("plan".into(), json!(plan));
    a.insert("email".into(), json!(email));
    for w in [&rl["primary"], &rl["secondary"]] {
        let Some(mins) = w["windowDurationMins"].as_u64() else {
            continue;
        };
        let to = if mins <= 360 {
            "five_hour"
        } else if mins >= 6 * 1440 {
            "weekly"
        } else {
            continue;
        };
        let at = w["resetsAt"]
            .as_f64()
            .filter(|s| *s > 0.0)
            .map(|s| (s * 1000.0) as u64);
        if let Some(v) = window(w["usedPercent"].as_f64(), at, now_ms) {
            a.entry(to).or_insert(v);
        }
    }
    let credits = &rl["credits"];
    if credits["hasCredits"] == json!(true) && credits["unlimited"] != json!(true) {
        let balance = credits["balance"]
            .as_f64()
            .or_else(|| credits["balance"].as_str().and_then(|s| s.parse().ok()));
        if let Some(b) = balance.filter(|b| b.is_finite()) {
            a.insert("credits".into(), json!((b * 100.0).round() / 100.0));
        }
    }
    let resets = &limits["rateLimitResetCredits"];
    let free = resets["availableCount"].as_u64().or_else(|| {
        resets["credits"].as_array().map(|c| {
            c.iter()
                .filter(|x| x["status"].as_str() == Some("available"))
                .count() as u64
        })
    });
    if let Some(n) = free {
        a.insert("full_resets".into(), json!(n));
    }
    (!plan.is_empty() || !email.is_empty()).then_some(Value::Object(a))
}

// ---- where things are ---------------------------------------------------------------------------------------------

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .filter(|h| h.is_absolute())
}

/// `~/.local/share/trommi/usage`, this user's own (0700).
pub fn data_dir() -> Option<PathBuf> {
    home().map(|h| h.join(".local/share/trommi/usage"))
}

/// Claude Code's folder of this process: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn claude_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".claude")))
}

/// The file the status line keeps the windows of this Claude Code's account in: one per Claude Code folder (an
/// account each), named by a hash of the folder.
pub fn windows_file() -> Option<PathBuf> {
    let dir = claude_dir()?;
    let mut x: u64 = 0xcbf29ce484222325;
    for b in dir.as_os_str().as_encoded_bytes() {
        x = (x ^ u64::from(*b)).wrapping_mul(0x100000001b3);
    }
    Some(data_dir()?.join(format!("claude-{x:016x}.json")))
}

/// Claude Code's user settings: `<folder>/settings.json`.
pub fn settings_file() -> Option<PathBuf> {
    claude_dir().map(|d| d.join("settings.json"))
}

/// Writes a file whole (a file beside it, then renamed), 0600, in a folder made 0700 when it is new.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let dir = path.parent().unwrap_or(Path::new("."));
    if !dir.exists() {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let tmp = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

// ---- the status line ----------------------------------------------------------------------------------------------

/// `trommi-connector statusline [-- <command>]`: Claude Code's status line. Keeps the windows of its input, then runs
/// `<command>` (the status line that was there before) with the same input and passes on what it prints and its exit
/// code. Without one it prints nothing.
pub fn statusline(args: &[String]) -> i32 {
    let mut input = Vec::new();
    let _ = std::io::stdin().take(4 << 20).read_to_end(&mut input);
    if let Ok(v) = serde_json::from_slice::<Value>(&input) {
        let rl = &v["rate_limits"];
        if rl.is_object() && (rl["five_hour"].is_object() || rl["seven_day"].is_object()) {
            if let Some(file) = windows_file() {
                let kept = json!({ "rate_limits": rl, "at": crate::util::now_ms() });
                let _ = write_private(&file, kept.to_string().as_bytes());
            }
        }
    }
    let Some(command) = args
        .iter()
        .position(|a| a == "--")
        .and_then(|i| args.get(i + 1))
    else {
        return 0;
    };
    let child = std::process::Command::new("sh")
        .args(["-c", command])
        .stdin(std::process::Stdio::piped())
        .spawn();
    let Ok(mut child) = child else { return 1 };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&input);
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

/// The windows the status line kept for this Claude Code's account, as the board keeps them.
pub fn kept_windows(now_ms: u64) -> Map<String, Value> {
    windows_file()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| claude_windows(&v["rate_limits"], now_ms))
        .unwrap_or_default()
}

/// A word for `sh` in single quotes.
fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
/// The single-quoted word at the start of `s`, unquoted (as [`quote`] wrote it).
fn unquote(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = s.strip_prefix('\'')?;
    loop {
        let end = rest.find('\'')?;
        out.push_str(&rest[..end]);
        rest = &rest[end + 1..];
        match rest.strip_prefix("\\''") {
            Some(r) => {
                out.push('\'');
                rest = r;
            }
            None => return Some(out),
        }
    }
}

/// Our status line command; with the one before, it runs that one (also when the connector is gone).
pub fn our_command(before: Option<&str>) -> String {
    match before.filter(|b| !b.trim().is_empty()) {
        None => format!("{PROGRAM}; [ -x \"$p\" ] || exit 0; {MARK}"),
        Some(b) => format!(
            "{PROGRAM}; if [ -x \"$p\" ]; then {MARK} -- {q}; else exec sh -c {q}; fi",
            q = quote(b)
        ),
    }
}
/// The command before, out of ours.
fn before_of(ours: &str) -> Option<String> {
    let at = ours.find(&format!("{MARK} -- "))? + MARK.len() + 4;
    unquote(&ours[at..])
}

/// Where in a JSON object's text its top-level member `key` stands: (start of the key, end of the value).
fn member_span(text: &str, key: &str) -> Option<(usize, usize)> {
    let b = text.as_bytes();
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while *i < b.len() && (b[*i] as char).is_ascii_whitespace() {
            *i += 1;
        }
    };
    let skip_string = |i: &mut usize| -> Option<()> {
        // at the opening quote
        *i += 1;
        while *i < b.len() {
            match b[*i] {
                b'\\' => *i += 2,
                b'"' => {
                    *i += 1;
                    return Some(());
                }
                _ => *i += 1,
            }
        }
        None
    };
    skip_ws(&mut i);
    if b.get(i) != Some(&b'{') {
        return None;
    }
    i += 1;
    loop {
        skip_ws(&mut i);
        match b.get(i)? {
            b'}' => return None,
            b',' => {
                i += 1;
                continue;
            }
            b'"' => {}
            _ => return None,
        }
        let key_start = i;
        skip_string(&mut i)?;
        let name: String = serde_json::from_str(&text[key_start..i]).ok()?;
        skip_ws(&mut i);
        if b.get(i) != Some(&b':') {
            return None;
        }
        i += 1;
        skip_ws(&mut i);
        // the value: up to the next comma or brace at this depth
        let mut depth = 0i32;
        while i < b.len() {
            match b[i] {
                b'"' => {
                    skip_string(&mut i)?;
                    continue;
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' if depth == 0 => break,
                b'}' | b']' => depth -= 1,
                b',' if depth == 0 => break,
                _ => {}
            }
            i += 1;
        }
        let mut end = i;
        while end > key_start && (b[end - 1] as char).is_ascii_whitespace() {
            end -= 1;
        }
        if name == key {
            return Some((key_start, end));
        }
    }
}

/// A value as it stands in a settings file indented by two: pretty, its inner lines indented once more.
fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v)
        .unwrap()
        .replace('\n', "\n  ")
}

/// The settings text with `key` set to `value` (None: taken out); every other byte stays as it was.
pub fn set_member(text: &str, key: &str, value: Option<&Value>) -> Option<String> {
    let parsed: Value = serde_json::from_str(text).ok()?;
    let obj = parsed.as_object()?;
    let out = match (member_span(text, key), value) {
        (Some((start, end)), Some(v)) => format!(
            "{}{}: {}{}",
            &text[..start],
            serde_json::to_string(key).unwrap(),
            pretty(v),
            &text[end..]
        ),
        (Some((start, end)), None) => {
            // the member and the comma after it, else the comma before it
            let rest = &text[end..];
            let after = rest.trim_start();
            if let Some(next) = after.strip_prefix(',') {
                let skip = rest.len() - next.trim_start().len();
                format!("{}{}", &text[..start], &text[end + skip..])
            } else {
                let head = text[..start].trim_end();
                let head = head.strip_suffix(',').unwrap_or(head);
                format!("{head}{rest}")
            }
        }
        (None, Some(v)) => {
            let open = text.find('{')?;
            let member = format!("\n  {}: {}", serde_json::to_string(key).unwrap(), pretty(v));
            if obj.is_empty() {
                format!("{{{member}\n}}\n")
            } else {
                format!("{}{member},{}", &text[..=open], &text[open + 1..])
            }
        }
        (None, None) => return Some(text.to_string()),
    };
    serde_json::from_str::<Value>(&out).ok()?;
    Some(out)
}

/// The settings file to write: the file a link names (one settings file for several Claude Code folders stays one).
fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write_settings(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let target = real(path);
    let mode = std::fs::metadata(&target)
        .map(|m| m.permissions().mode() & 0o777)
        .unwrap_or(0o600);
    write_private(&target, text.as_bytes())
        .and_then(|()| std::fs::set_permissions(&target, std::fs::Permissions::from_mode(mode)))
        .map_err(|e| format!("{} not written ({e})", target.display()))
}

/// Puts our status line into Claude Code's settings at `path`, around the one there (kept, its output passed on).
/// Returns what was done. A settings file that is not a JSON object is left alone.
pub fn ensure_statusline(path: &Path) -> Result<String, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "{}\n".to_string(),
        Err(e) => return Err(format!("{} not read ({e})", path.display())),
    };
    let v: Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "{} is not JSON; the status line was left alone",
            path.display()
        )
    })?;
    let Some(obj) = v.as_object() else {
        return Err(format!(
            "{} is not a JSON object; the status line was left alone",
            path.display()
        ));
    };
    let now = obj.get("statusLine");
    let command = now.and_then(|s| s["command"].as_str());
    if command.is_some_and(|c| c.contains(MARK)) {
        return Ok("status line: Trommi's there already".into());
    }
    let mut line = match now {
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    };
    line.insert("type".into(), json!("command"));
    line.insert("command".into(), json!(our_command(command)));
    let out = set_member(&text, "statusLine", Some(&Value::Object(line)))
        .ok_or_else(|| format!("{} could not be changed", path.display()))?;
    write_settings(path, &out)?;
    Ok(match command {
        Some(_) => "status line: Trommi's put around yours (it shows as before), for the plan's usage on the board".into(),
        None => "status line: Trommi's put in (it prints nothing), for the plan's usage on the board".into(),
    })
}

/// Takes our status line out of the settings at `path`: the one before stands there again, or none.
pub fn remove_statusline(path: &Path) -> Result<String, String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok("status line: no settings file".into());
    };
    let v: Value =
        serde_json::from_str(&text).map_err(|_| format!("{} is not JSON", path.display()))?;
    let now = &v["statusLine"];
    let Some(command) = now["command"].as_str().filter(|c| c.contains(MARK)) else {
        return Ok("status line: not Trommi's, left as it is".into());
    };
    let restored = before_of(command).map(|b| {
        let mut o = now.as_object().cloned().unwrap_or_default();
        o.insert("command".into(), json!(b));
        Value::Object(o)
    });
    let out = set_member(&text, "statusLine", restored.as_ref())
        .ok_or_else(|| format!("{} could not be changed", path.display()))?;
    write_settings(path, &out)?;
    Ok(match restored {
        Some(_) => "status line: yours stands there again".into(),
        None => "status line: Trommi's taken out".into(),
    })
}

/// Whether the user took it out (`remove claude`): then the connector does not put it in again by itself.
fn declined_file() -> Option<PathBuf> {
    data_dir().map(|d| d.join("statusline-removed"))
}
pub fn mark_declined(on: bool) {
    if let Some(f) = declined_file() {
        if on {
            let _ = write_private(
                &f,
                b"removed by `trommi-connector remove claude`; `setup claude` puts it in again\n",
            );
        } else {
            let _ = std::fs::remove_file(f);
        }
    }
}
pub fn declined() -> bool {
    declined_file().is_some_and(|f| f.exists())
}

// ---- asking the programs ------------------------------------------------------------------------------------------

/// `claude auth status --json` (`TROMMI_CLAUDE_BIN`, else `claude` on the PATH).
async fn claude_status() -> Option<Value> {
    let bin = std::env::var_os("TROMMI_CLAUDE_BIN")
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "claude".into());
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["auth", "status", "--json"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .ok()?
        .ok()?;
    serde_json::from_slice(&out.stdout).ok()
}

/// Codex's app server: `account/read` and `account/rateLimits/read`, then it is ended.
async fn codex_ask() -> Option<(Value, Value)> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut cmd = tokio::process::Command::new(crate::host::codex_program());
    cmd.arg("app-server")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let mut child = cmd.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let mut lines = BufReader::new(child.stdout.take()?).lines();
    let asked = async {
        for req in [
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "clientInfo": { "name": "trommi", "version": crate::server::SERVER_VERSION } } }),
            json!({ "jsonrpc": "2.0", "method": "initialized" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "account/read", "params": {} }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "account/rateLimits/read" }),
        ] {
            stdin.write_all(format!("{req}\n").as_bytes()).await.ok()?;
        }
        stdin.flush().await.ok()?;
        let (mut account, mut limits) = (None, None);
        while account.is_none() || limits.is_none() {
            let line = lines.next_line().await.ok()??;
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v["id"].as_u64() {
                Some(2) => account = Some(v["result"].clone()),
                Some(3) => limits = Some(v["result"].clone()),
                _ => {}
            }
        }
        Some((account?, limits?))
    };
    let got = tokio::time::timeout(Duration::from_secs(20), asked)
        .await
        .ok()
        .flatten();
    let _ = child.kill().await;
    got
}

/// Looks now and then and says what to send: the account of this program, when it changed since the last time.
#[derive(Default)]
pub struct Poller {
    /// Claude Code's plan and e-mail, and when they were asked (they change rarely: every 10 minutes).
    claude: Option<(u64, Option<(String, String)>)>,
    /// Codex's account, and when it was asked (every 2 minutes: each time a process).
    codex: Option<(u64, Option<Value>)>,
    sent: Option<Value>,
}

impl Poller {
    /// The account as it stands now, or None when there is nothing to say of it.
    pub async fn now(&mut self, is_codex: bool, now_ms: u64) -> Option<Value> {
        if is_codex {
            if self
                .codex
                .as_ref()
                .is_none_or(|(at, _)| now_ms.saturating_sub(*at) >= 120_000)
            {
                let got = codex_ask().await.and_then(|(a, l)| codex(&a, &l, now_ms));
                self.codex = Some((now_ms, got));
            }
            return self.codex.as_ref().and_then(|(_, v)| v.clone());
        }
        if self
            .claude
            .as_ref()
            .is_none_or(|(at, _)| now_ms.saturating_sub(*at) >= 600_000)
        {
            let got = claude_status().await.as_ref().and_then(claude_account);
            self.claude = Some((now_ms, got));
        }
        let (plan, email) = self.claude.as_ref()?.1.clone()?;
        Some(claude(&plan, &email, kept_windows(now_ms)))
    }
    /// The account to send now: when it differs from what was sent.
    pub async fn changed(&mut self, is_codex: bool, now_ms: u64) -> Option<Value> {
        let now = self.now(is_codex, now_ms).await?;
        (self.sent.as_ref() != Some(&now)).then_some(now)
    }
    pub fn sent(&mut self, v: Value) {
        self.sent = Some(v);
    }
}

/// Whether the board gets the account and usage (`TROMMI_USAGE=off` turns it off).
pub fn on() -> bool {
    !matches!(std::env::var("TROMMI_USAGE").as_deref(), Ok("off" | "0"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_code_names_plan_and_email_only_for_a_subscription() {
        let s = json!({ "loggedIn": true, "authMethod": "claude.ai", "email": "ada@example.org", "subscriptionType": "max" });
        assert_eq!(
            claude_account(&s),
            Some(("max".into(), "ada@example.org".into()))
        );
        assert_eq!(
            claude_account(&json!({ "loggedIn": true, "authMethod": "apiKey" })),
            None
        );
        assert_eq!(
            claude_account(&json!({ "loggedIn": false, "authMethod": "claude.ai" })),
            None
        );
    }

    #[test]
    fn the_status_line_windows_become_the_boards() {
        let now = 1_791_713_000_000;
        let rl = json!({ "five_hour": { "used_percentage": 12.4, "resets_at": 1791729000 }, "seven_day": { "used_percentage": 53, "resets_at": 1792224000 } });
        let w = claude_windows(&rl, now);
        assert_eq!(
            w["five_hour"],
            json!({ "used": 12, "resets_at": 1_791_729_000_000u64 })
        );
        assert_eq!(
            w["weekly"],
            json!({ "used": 53, "resets_at": 1_792_224_000_000u64 })
        );
        // a window that started again since is left out: what was said of it is old
        assert!(claude_windows(&rl, 1_791_729_000_001)
            .get("five_hour")
            .is_none());
        assert!(claude_windows(&json!(null), now).is_empty());
    }

    #[test]
    fn codex_has_its_week_credits_and_full_resets() {
        let now = 1_791_713_000_000;
        let account = json!({ "account": { "type": "chatgpt", "email": "ada@work.example", "planType": "prolite" } });
        let limits = json!({ "rateLimits": { "primary": { "usedPercent": 41, "windowDurationMins": 10080, "resetsAt": 1792304061 }, "secondary": null,
            "credits": { "hasCredits": true, "unlimited": false, "balance": "62498.3051380000" }, "planType": "prolite" },
            "rateLimitResetCredits": { "availableCount": 3, "credits": [] } });
        let a = codex(&account, &limits, now).unwrap();
        assert_eq!(a["kind"], "codex");
        assert_eq!(a["plan"], "prolite");
        assert_eq!(
            a["weekly"],
            json!({ "used": 41, "resets_at": 1_792_304_061_000u64 })
        );
        assert!(a.get("five_hour").is_none());
        assert_eq!(a["credits"], json!(62498.31));
        assert_eq!(a["full_resets"], json!(3));
        assert!(codex(&json!({ "account": { "type": "apiKey" } }), &limits, now).is_none());
    }

    #[test]
    fn our_command_runs_the_one_before_and_gives_it_back() {
        let before = "printf '%s\\n' \"it's me\"";
        let ours = our_command(Some(before));
        assert!(ours.contains(MARK));
        assert_eq!(before_of(&ours).as_deref(), Some(before));
        assert_eq!(before_of(&our_command(None)), None);
        // the command runs: without the connector at its path, the one before still prints
        let out = std::process::Command::new("sh")
            .args(["-c", &ours])
            .env("HOME", "/nonexistent-trommi-home")
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "it's me");
    }

    #[test]
    fn a_member_is_set_and_taken_out_and_the_rest_stays_as_it_was() {
        let text = "{\n  \"model\": \"opus\",\n  \"hooks\": {\"a\": [1, \"}\"]}\n}\n";
        let v = json!({ "type": "command", "command": "x" });
        let set = set_member(text, "statusLine", Some(&v)).unwrap();
        assert!(set.starts_with("{\n  \"statusLine\": {\n    \"command\": \"x\",\n    \"type\": \"command\"\n  },\n  \"model\": \"opus\","), "{set}");
        let changed = set_member(&set, "statusLine", Some(&json!({ "command": "y" }))).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&changed).unwrap()["statusLine"]["command"],
            "y"
        );
        assert_eq!(set_member(&set, "statusLine", None).unwrap(), text);
        let last = "{\"a\": 1, \"statusLine\": {\"command\": \"z\"}}";
        assert_eq!(set_member(last, "statusLine", None).unwrap(), "{\"a\": 1}");
        assert_eq!(
            set_member("{}", "statusLine", Some(&json!(1))).unwrap(),
            "{\n  \"statusLine\": 1\n}\n"
        );
        assert!(set_member("[1]", "statusLine", Some(&json!(1))).is_none());
    }
}
