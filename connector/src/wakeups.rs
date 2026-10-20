//! Wakeups on this computer (README "Wakeups", spec/v1.md 9.3.8): the clock that wakes an agent its own wakeups name.
//!
//! The wakeups themselves are registers of the agent's session (`wakeup/<id>`, sealed like everything else; the core's
//! `wakeups` module holds their rules). While the session runs, its connector's clock looks at them every half
//! minute and hands a due one to the agent as a board event. When the session ends with wakeups left, the connector
//! leaves a note of them on this computer (`<keys>/wakers/<session>.json`, private like the key beside it) and a waker:
//! a small process of its own that sleeps until the next one is due, checks the guardrails, and starts the agent's
//! program again on that conversation (`claude -p --resume`, `codex exec resume`), bounded in time and turns. The
//! connector that program starts takes the note away and runs the wakeup as it stands then (the person may have
//! cancelled or moved it meanwhile): the waker only says "a wakeup is due, read the board".
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use trommi_client_core::wakeups::{self as core, Wakeup};

/// The first words of a woken run: what is due comes from the board (as it stands then), not from the note.
pub const WOKEN: &str = "[Trommi] Woken for a wakeup you set: call the trommi inbox tool now and do what it hands you. If it hands you nothing, the human cancelled it: stop.";

/// The person's zone, in minutes east of UTC, as this computer's clock has it (`TROMMI_ZONE_MIN` in tests).
pub fn zone_min() -> i32 {
    if let Some(z) = std::env::var("TROMMI_ZONE_MIN").ok().and_then(|v| v.parse::<i32>().ok()) {
        return z;
    }
    std::process::Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| zone_of(String::from_utf8_lossy(&o.stdout).trim()))
        .unwrap_or(0)
}

/// "+0200" or "-0530" in minutes.
fn zone_of(z: &str) -> Option<i32> {
    let (sign, rest) = match z.as_bytes().first()? {
        b'+' => (1, &z[1..]),
        b'-' => (-1, &z[1..]),
        _ => return None,
    };
    if rest.len() != 4 {
        return None;
    }
    let (h, m): (i32, i32) = (rest[..2].parse().ok()?, rest[2..].parse().ok()?);
    Some(sign * (h * 60 + m))
}

/// The share of the subscription's five-hour window in use, as Claude Code's status line last kept it (none: not
/// known, or Codex).
pub fn usage(now: u64) -> Option<f64> {
    if crate::host::is_codex() {
        return None;
    }
    crate::usage::kept_windows(now).get("five_hour").and_then(|w| w.get("used")).and_then(Value::as_f64).map(|u| u / 100.0)
}

/// Where the note of a session's wakeups waits while the session is closed.
pub fn note_file(keys_dir: &Path, session: &str) -> PathBuf {
    keys_dir.join("wakers").join(format!("{}.json", session.chars().filter(char::is_ascii_alphanumeric).collect::<String>()))
}

/// The note a closed session leaves for its waker.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    pub session: String,
    pub folder: String,
    /// "claude" or "codex".
    pub program: String,
    /// The conversation to resume: Claude Code's session id, or the Codex thread.
    pub resume: String,
    /// The tool Claude Code asks the human through (`--permission-prompt-tool`).
    pub ask_tool: String,
    pub zone_min: i32,
    /// The session's runs as its connector kept them (the day, how many).
    pub counted: (u64, u32),
    pub wakeups: Vec<Wakeup>,
    /// The waker that keeps it (a newer one takes over).
    pub waker: String,
    /// A wakeup whose run the waker started: the connector that run starts runs it.
    pub running: Option<String>,
}

impl Note {
    pub fn read(file: &Path) -> Option<Note> {
        let v = crate::slots::read_json(file)?;
        let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let wakeups = v.get("wakeups").and_then(Value::as_object).map(|o| o.iter().filter_map(|(id, w)| Wakeup::read(id, w)).collect()).unwrap_or_default();
        Some(Note {
            session: s("session"),
            folder: s("folder"),
            program: s("program"),
            resume: s("resume"),
            ask_tool: s("ask_tool"),
            zone_min: v.get("zone_min").and_then(Value::as_i64).unwrap_or(0) as i32,
            counted: (v.get("day").and_then(Value::as_u64).unwrap_or(0), v.get("runs").and_then(Value::as_u64).unwrap_or(0) as u32),
            wakeups,
            waker: s("waker"),
            running: v.get("running").and_then(Value::as_str).map(str::to_string),
        })
    }
    /// Writes the note over the one there, only while it is still this waker's (never makes one anew: a connector
    /// that took it away keeps the wakeups now). Whether it was written.
    pub fn replace(&self, file: &Path, me: &str) -> bool {
        use std::io::Write;
        if Note::read(file).is_none_or(|n| n.waker != me) {
            return false;
        }
        let Ok(mut f) = std::fs::OpenOptions::new().write(true).truncate(true).open(file) else { return false };
        f.write_all(self.json().to_string().as_bytes()).is_ok()
    }
    fn json(&self) -> Value {
        let wakeups: serde_json::Map<String, Value> = self.wakeups.iter().map(|w| (w.id.clone(), w.value())).collect();
        json!({ "session": self.session, "folder": self.folder, "program": self.program, "resume": self.resume, "ask_tool": self.ask_tool, "zone_min": self.zone_min, "day": self.counted.0, "runs": self.counted.1, "wakeups": wakeups, "waker": self.waker, "running": self.running })
    }
    pub fn write(&self, file: &Path) -> std::io::Result<()> {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::slots::write_private(file, &self.json().to_string())
    }
}

/// What the waker does next with its note at `now`.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Sleep until then and look again (the note as it stands now).
    Sleep(u64, Note),
    /// Start the run of this wakeup (the note as it stands after).
    Run(String, Note),
    /// Nothing left: end.
    Done,
}

/// The waker's look at its note: the core's clock decides (quiet hours, the day's runs, the subscription's window);
/// a wakeup that waits is noted with its new time.
pub fn step(note: &Note, now: u64, usage: Option<f64>) -> Step {
    if note.wakeups.is_empty() {
        return Step::Done;
    }
    let t = core::tick(&note.wakeups, &[], now, note.zone_min, usage, None, note.counted);
    let mut next = note.clone();
    for (id, w) in &t.writes {
        next.wakeups.retain(|x| x.id != *id);
        if let Some(w) = w {
            next.wakeups.push(w.clone());
        }
    }
    next.wakeups.sort_by_key(|w| w.at);
    next.counted = (core::day_of(now, note.zone_min), t.day_runs);
    match t.runs.first() {
        Some((id, _)) => {
            next.running = Some(id.clone());
            Step::Run(id.clone(), next)
        }
        None => Step::Sleep(next.wakeups.iter().map(|w| w.at).min().unwrap_or(now + 60_000), next),
    }
}

/// The command a woken run is: the agent's program on its conversation, bounded in turns, asking the human through
/// the board (Claude Code); Codex runs as its settings say (no asking in this version).
pub fn command(note: &Note) -> Option<(String, Vec<String>)> {
    if note.resume.is_empty() {
        return None;
    }
    match note.program.as_str() {
        "claude" => {
            let mut args = vec!["-p".to_string(), WOKEN.to_string(), "--resume".into(), note.resume.clone(), "--max-turns".into(), core::RUN_MAX_TURNS.to_string()];
            if !note.ask_tool.is_empty() {
                args.extend(["--permission-prompt-tool".to_string(), note.ask_tool.clone()]);
            }
            Some((std::env::var("TROMMI_CLAUDE").unwrap_or_else(|_| "claude".into()), args))
        }
        "codex" => Some((std::env::var("TROMMI_CODEX").unwrap_or_else(|_| "codex".into()), vec!["exec".into(), "resume".into(), note.resume.clone(), WOKEN.into()])),
        _ => None,
    }
}

/// Leaves the note of a closing session and starts its waker (none when it has no wakeups).
pub fn leave(keys_dir: &Path, mut note: Note) {
    use std::os::unix::process::CommandExt;
    let file = note_file(keys_dir, &note.session);
    if note.wakeups.is_empty() {
        let _ = std::fs::remove_file(&file);
        return;
    }
    note.waker = crate::util::random_hex(8);
    note.running = None;
    if note.write(&file).is_err() {
        return;
    }
    if matches!(crate::update::verify_file(&crate::server::self_path()), crate::update::Release::Refused(_)) {
        return;
    }
    let mut cmd = std::process::Command::new(crate::server::self_path());
    cmd.args(["waker", &file.display().to_string(), &note.waker])
        .env_remove(crate::launch::ENV_ABI)
        .env_remove(crate::launch::ENV_PARENT)
        .current_dir(&note.folder)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // (a process group of its own: it outlives the session's)
    cmd.process_group(0);
    let _ = cmd.spawn();
}

/// Whether a waker started a run in `folder` that no connector took yet (its first inbox waits for the clock's look).
pub fn woken_here(keys_dir: &Path, folder: &str) -> bool {
    let Ok(dir) = std::fs::read_dir(keys_dir.join("wakers")) else { return false };
    dir.flatten().filter_map(|e| Note::read(&e.path())).any(|n| n.folder == folder && n.running.is_some())
}

/// What the connector of a session that starts takes from its note: the wakeup a waker started this run for. The
/// note goes (this connector's clock keeps the wakeups now, and its waker ends).
pub fn take(keys_dir: &Path, session: &str) -> Option<String> {
    let file = note_file(keys_dir, session);
    let note = Note::read(&file);
    let _ = std::fs::remove_file(&file);
    note.and_then(|n| n.running)
}

/// `waker <note> <waker>`: sleeps until the next wakeup of a closed session, then starts its run; ends when the note
/// is gone or another waker keeps it, or the run ended (its connector leaves a new note and waker).
pub async fn waker(file: Option<String>, me: Option<String>) -> i32 {
    let (Some(file), Some(me)) = (file.map(PathBuf::from), me) else { return 1 };
    loop {
        let Some(mut note) = Note::read(&file).filter(|n| n.waker == me) else { return 0 };
        // (this computer's zone as it is now: a change of the clocks moves the quiet hours with it)
        note.zone_min = zone_min();
        let now = crate::util::now_ms();
        match step(&note, now, usage(now)) {
            Step::Done => {
                let _ = std::fs::remove_file(&file);
                return 0;
            }
            Step::Sleep(at, next) => {
                if next != note && !next.replace(&file, &me) {
                    return 0;
                }
                let wait = at.saturating_sub(now).clamp(1_000, crate::server::env_ms("TROMMI_WAKER_LOOK_MS", 60_000));
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            }
            Step::Run(id, next) => {
                // (the note still ours, or the session opened meanwhile: then its connector runs it)
                if !next.replace(&file, &me) {
                    return 0;
                }
                let Some((program, args)) = command(&next) else {
                    let _ = std::fs::remove_file(&file);
                    return 1;
                };
                eprintln!("[trommi] waking the session for wakeup {id}");
                let mut cmd = tokio::process::Command::new(program);
                // (a process group of its own: at the end of its time the run goes with every tool it started)
                cmd.process_group(0);
                cmd.args(&args)
                    .current_dir(&next.folder)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .kill_on_drop(true);
                let Ok(mut child) = cmd.spawn() else {
                    // (the program is not there: the wakeup stays for the session's next start)
                    let _ = std::fs::remove_file(&file);
                    return 1;
                };
                let limit = std::time::Duration::from_millis(crate::server::env_ms("TROMMI_WAKE_RUN_MS", core::RUN_MAX_MS));
                if tokio::time::timeout(limit, child.wait()).await.is_err() {
                    if let Some(pid) = child.id().and_then(|p| rustix::process::Pid::from_raw(p as i32)) {
                        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::TERM);
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                    }
                    let _ = child.kill().await;
                }
                // (a run whose connector never took the note: it did not reach the board, nothing more is tried
                //  until the session opens again)
                if Note::read(&file).is_some_and(|n| n.waker == me) {
                    let _ = std::fs::remove_file(&file);
                }
                return 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const NOW: u64 = 1_792_490_400_000;

    #[test]
    fn zones_read() {
        assert_eq!((zone_of("+0200"), zone_of("-0530"), zone_of("0200"), zone_of("")), (Some(120), Some(-330), None, None));
    }

    #[test]
    fn the_waker_sleeps_runs_or_ends() {
        let w = |id: &str, at: u64| Wakeup { id: id.into(), at, note: "x".into(), ..Default::default() };
        let note = Note { session: "s".into(), program: "claude".into(), resume: "abc".into(), zone_min: 120, wakeups: vec![w("a", NOW + 60_000)], ..Default::default() };
        assert!(matches!(step(&note, NOW, None), Step::Sleep(at, _) if at == NOW + 60_000));
        let Step::Run(id, after) = step(&note, NOW + 60_000, None) else { panic!() };
        assert_eq!((id.as_str(), after.running.as_deref(), after.wakeups.len()), ("a", Some("a"), 0));
        // the subscription's window almost used: it waits half an hour
        assert!(matches!(step(&note, NOW + 60_000, Some(0.95)), Step::Sleep(at, _) if at == NOW + 60_000 + core::USAGE_WAIT_MS));
        assert_eq!(step(&Note::default(), NOW, None), Step::Done);
        let (program, args) = command(&Note { ask_tool: "mcp__trommi__ask_permission".into(), ..note.clone() }).unwrap();
        assert_eq!(program, "claude");
        assert_eq!(args[2..].join(" "), "--resume abc --max-turns 30 --permission-prompt-tool mcp__trommi__ask_permission");
        assert_eq!(command(&Note { resume: String::new(), ..note }), None, "nothing to resume, no run");
    }
}
