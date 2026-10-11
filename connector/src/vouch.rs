//! Self-join (spec/v1.md 12.4): a new agent session on this machine and account gets onto the board without a
//! manual invite. The connector of a running session of the same account (the voucher) asks the human devices
//! in its own session group (`join_ask/<nonce>`); a human device answers there with an invite link for it
//! (`join_link/<nonce>`), which only that session group reads; the new session joins with it as with any link, and
//! the voucher passes on the hash of its Request, which the human device checks before it confirms on its own.
//! What travels between the two connectors goes through the voucher's door (a Unix socket of this user).
use crate::member::{env, home, Cfg};
use crate::slots::SlotPaths;
use crate::util::{hex, sha256};
use std::path::{Path, PathBuf};

/// How long a new session waits for the human's app to answer its ask (an app device must be online).
pub const LINK_WAIT_MS: u64 = 15 * 60_000;
/// The most auto-joined live sessions per account; the human's app refuses more (`join_link` = `{ refused }`).
pub const MAX_PER_ACCOUNT: usize = 8;

/// The account this connector runs under: the client and its config folder. Claude Code: `CLAUDE_CONFIG_DIR`,
/// else `~/.claude`; Codex: `CODEX_HOME`, else `~/.codex`. Returns (id, label): the id is the first 16 hex of
/// SHA-256 of `<client>:<folder>` (what the human's app counts by), the label the folder's last part.
pub fn account() -> (String, String) {
    let (kind, folder): (&str, PathBuf) = if crate::host::is_codex() || env("CODEX_HOME").is_some()
    {
        (
            "codex",
            crate::host::codex_home()
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".codex")),
        )
    } else {
        (
            "claude",
            env("CLAUDE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| home().join(".claude")),
        )
    };
    let folder = crate::member::resolve(&folder.display().to_string());
    let id = hex(&sha256(format!("{kind}:{}", folder.display()).as_bytes()))[..16].to_string();
    let label = folder
        .file_name()
        .map(|n| n.to_string_lossy().trim_start_matches('.').to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| kind.to_string());
    (id, format!("{kind} {label}"))
}

/// Whether self-join is on (TROMMI_SELF_JOIN=0 turns it off).
pub fn enabled() -> bool {
    env("TROMMI_SELF_JOIN").as_deref() != Some("0")
}

/// The name a new session proposes for itself: its folder's name.
pub fn session_name(cfg: &Cfg) -> String {
    cfg.folder
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .filter(|x| !x.is_empty())
        .unwrap_or_else(|| cfg.shown.clone())
}

/// The keyed slots of a room held by a running connector, of every folder: who could vouch.
pub fn held_slots(room_dir: &Path) -> Vec<SlotPaths> {
    let Ok(rd) = std::fs::read_dir(room_dir) else {
        return vec![];
    };
    let mut out: Vec<SlotPaths> = vec![];
    for e in rd.filter_map(|e| e.ok()) {
        let f = e.file_name().to_string_lossy().to_string();
        let Some((name, pid)) = f.rsplit_once(".lock.") else {
            continue;
        };
        if f.starts_with("replaced-") || !pid.bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(pid) = pid.parse::<u32>() else {
            continue;
        };
        if pid == crate::slots::pid() || !crate::slots::alive(pid) {
            continue;
        }
        let key_file = room_dir.join(format!("{name}.key"));
        if !key_file.exists() || out.iter().any(|p| p.key_file == key_file) {
            continue;
        }
        out.push(SlotPaths {
            dir: room_dir.to_path_buf(),
            lock_file: room_dir.join(format!("{name}.lock")),
            prefix: format!("{name}."),
            cache: room_dir.join(format!("{name}.files")),
            key_file,
            ..Default::default()
        });
    }
    out
}

/// The room a keyless session may self-join: the one of its folder, else the only room under the keys folder.
pub fn room_for(cfg: &Cfg) -> Option<String> {
    if let Ok(Some(room)) = crate::member::resolve_room(cfg) {
        return Some(room);
    }
    let rooms: Vec<String> = std::fs::read_dir(&cfg.keys_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| trommi_core::ids::RoomId::from_base64url(n).is_ok())
        .collect();
    (rooms.len() == 1).then(|| rooms[0].clone())
}
