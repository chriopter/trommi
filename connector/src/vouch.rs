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

/// Whether self-join is on: on by default (the web and iPhone apps answer asks since v225); TROMMI_SELF_JOIN=0
/// turns it off.
pub fn enabled() -> bool {
    env("TROMMI_SELF_JOIN").as_deref() != Some("0")
}

/// Who this agent session is on the board, the same across its devices (a resume that joins again, a session under
/// another account of this machine): the first 16 hex of SHA-256 of `trommi agent:<owner>`, none for a session
/// without an id. It names no secret (the owner is `cc:<session id>`, `key:…`, `codex:<thread>`); a human device
/// reads it from the agent's `device/<id>` register and from an ask, to keep one entry and one name per session.
pub fn agent_identity(owner: &str) -> Option<String> {
    (!owner.is_empty())
        .then(|| hex(&sha256(format!("trommi agent:{owner}").as_bytes()))[..16].to_string())
}

/// Whether `v` is an agent identity as [`agent_identity`] makes it (16 lower-case hex digits).
pub fn is_identity(v: &str) -> bool {
    v.len() == 16 && v.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The name a new session proposes for itself: its folder's name. (The human device names it after the session it
/// was before when the ask's `agent` identity matches one: [`agent_identity`].)
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

/// The rooms a keyless session may self-join, in the order it asks: the room of its folder first, then every
/// other room this folder has keys in (a folder of sessions of two accounts: a running session of this program
/// and account may be in either), else the only room under the keys folder.
pub fn rooms_for(cfg: &Cfg) -> Vec<String> {
    let mut rooms: Vec<String> = room_for(cfg).into_iter().collect();
    let names: Vec<String> = std::fs::read_dir(&cfg.keys_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| trommi_core::ids::RoomId::from_base64url(n).is_ok())
                .collect()
        })
        .unwrap_or_default();
    for n in names {
        if !rooms.contains(&n) && !crate::member::slots_in(cfg, &cfg.keys_dir.join(&n)).is_empty() {
            rooms.push(n);
        }
    }
    rooms
}

/// Who could vouch for a keyless session: the held slots of its rooms (`rooms_for`), in that order.
pub fn vouchers(cfg: &Cfg) -> Vec<SlotPaths> {
    rooms_for(cfg)
        .iter()
        .flat_map(|room| held_slots(&cfg.keys_dir.join(room)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One session, one identity, on every device and account of this machine; none without an id.
    #[test]
    fn the_identity_follows_the_session() {
        let a = agent_identity("cc:3ddd0415-2915-4605-9b83-f261e3335711").unwrap();
        assert!(is_identity(&a));
        assert_eq!(agent_identity("cc:3ddd0415-2915-4605-9b83-f261e3335711").unwrap(), a);
        assert_ne!(agent_identity("cc:another").unwrap(), a);
        assert_eq!(agent_identity(""), None);
        assert!(!is_identity("ABCDEF0123456789") && !is_identity("abc"));
    }
}
