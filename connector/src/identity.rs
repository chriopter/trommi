//! Stable public session identity and display name, independent of the computer approval key.
use crate::member::Cfg;
use crate::util::{hex, sha256};

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
    v.len() == 16
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The name a new session proposes for itself: the agent's name its setup gave (`TROMMI_AGENT_NAME`), else its folder's
/// name. (The human device names it after the session it
/// was before when the ask's `agent` identity matches one: [`agent_identity`].)
pub fn session_name(cfg: &Cfg) -> String {
    // (a chat agent's name as its setup gave it: "Hermes Einkauf")
    if let Some(name) = std::env::var("TROMMI_AGENT_NAME").ok().map(|n| n.trim().chars().take(40).collect::<String>()).filter(|n| !n.is_empty()) {
        return name;
    }
    cfg.folder
        .file_name()
        .map(|x| x.to_string_lossy().to_string())
        .filter(|x| !x.is_empty())
        .unwrap_or_else(|| cfg.shown.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One session, one identity, on every device and account of this machine; none without an id.
    #[test]
    fn the_identity_follows_the_session() {
        let a = agent_identity("cc:3ddd0415-2915-4605-9b83-f261e3335711").unwrap();
        assert!(is_identity(&a));
        assert_eq!(
            agent_identity("cc:3ddd0415-2915-4605-9b83-f261e3335711").unwrap(),
            a
        );
        assert_ne!(agent_identity("cc:another").unwrap(), a);
        assert_eq!(agent_identity(""), None);
        assert!(!is_identity("ABCDEF0123456789") && !is_identity("abc"));
    }
}
