//! The approved computer's local I/O. Authority, quotas and Request binding stay in the shared app core.
//! A filesystem lock serializes brief opens; no process or state lock is kept between activation polls.
use crate::error::{Fault, Result};
use crate::member::{self, Cfg, Member};
use crate::util::now_ms;
use serde_json::{json, Value};
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn directory(cfg: &Cfg, room: &str) -> PathBuf {
    cfg.keys_dir.join(room).join("machine")
}

fn config(cfg: &Cfg, room: &str) -> Cfg {
    let mut machine = cfg.clone();
    machine.owner = format!("machine:{room}");
    machine.session = format!("machine-operation:{}", crate::slots::pid());
    machine.room = room.to_string();
    machine
}

fn has_key(cfg: &Cfg, room: &str) -> bool {
    let machine = config(cfg, room);
    !member::own_slots(&machine, &machine.owner).is_empty()
}

/// The folder's chosen board, otherwise the only approved computer board; never a directory-order guess.
pub fn room_for(cfg: &Cfg) -> Result<String> {
    if let Some(room) = (!cfg.room.is_empty())
        .then(|| cfg.room.clone())
        .or_else(|| member::joined_room(cfg))
    {
        return if has_key(cfg, &room) {
            Ok(room)
        } else {
            Err(Fault::new("computer-required", "The human chooses Approve a computer in Trommi for this folder's board and gives you its command; then call connect. Nothing joined the board"))
        };
    }
    let mut rooms: Vec<String> = std::fs::read_dir(&cfg.keys_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|r| trommi_core::ids::RoomId::from_base64url(r).is_ok() && has_key(cfg, r))
        .collect();
    rooms.sort();
    if rooms.len() == 1 {
        return Ok(rooms.remove(0));
    }
    if rooms.is_empty() {
        Err(Fault::new("computer-required", "This computer is not approved for Trommi. The human chooses Approve a computer in Trommi and gives you its command. Then call connect with that command. Nothing joined the board."))
    } else {
        Err(Fault::new("ambiguous-room", format!("This computer is approved for several Trommi boards ({}). Connect with the intended board's link in this folder first, or set TROMMI_ROOM.", rooms.join(", "))))
    }
}

async fn lock(cfg: &Cfg, room: &str) -> Result<File> {
    let dir = directory(cfg, room);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    if !std::fs::symlink_metadata(&dir)?.is_dir() {
        return Err(Fault::new(
            "computer-state",
            "The computer state path is not a directory",
        ));
    }
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(dir.join("operation.lock"))?;
    let deadline = now_ms() + 15_000;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if now_ms() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(Fault::new(
                    "computer-busy",
                    "Another activation is using the computer key; call connect again in a moment",
                ))
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.into()),
        }
    }
}

/// Separate member so the MCP session can show this computer approval's emoji without taking its key.
pub fn approval_member(cfg: &Cfg, link: &str) -> Result<Arc<Member>> {
    let (room, _) = member::room_of_link(link)?;
    Ok(Member::new(config(cfg, &room)))
}

/// One emoji approval. Only the human app's authenticated room grant enables subsequent activations.
pub async fn approve(cfg: &Cfg, link: &str, machine: &Arc<Member>) -> Result<()> {
    let (room, _) = member::room_of_link(link)?;
    let _lock = lock(cfg, &room).await?;
    let result = async {
        if has_key(cfg, &room) {
            machine.open(false).await?;
            if machine.phase() == "ready" {
                return Err(Fault::new("already-approved", "This computer already has a Trommi key for this board. Activate a session with connect without a link. To approve a replacement, remove this computer in Trommi first."));
            }
        }
        machine.join(link, false, true).await?;
        let client = machine.client().ok_or_else(|| Fault::new("computer-state", "The computer did not finish joining"))?;
        let deadline = now_ms() + 30_000;
        loop {
            let approved = client.session_register("machine_approved").await?.and_then(|s| serde_json::from_str::<Value>(&s).ok());
            if approved.as_ref().and_then(|v| v.get("device")).and_then(Value::as_str) == Some(client.me_hex().as_str()) { return Ok(()) }
            if now_ms() >= deadline { return Err(Fault::new("computer-required", "The human app has not approved this computer. Use a current Trommi app and its Approve a computer command.")) }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }.await;
    machine.release().await;
    result
}

enum Operation {
    Read(String),
    Write(String, Value),
}

async fn exchange(cfg: &Cfg, room: &str, operation: Operation) -> Result<Option<Value>> {
    let _lock = lock(cfg, room).await?;
    let machine = Member::new(config(cfg, room));
    let result = async {
        machine.open(false).await?;
        let client = machine.client().ok_or_else(|| {
            Fault::new(
                "computer-required",
                "The computer key is no longer usable; approve this computer in Trommi again",
            )
        })?;
        let online_until = tokio::time::Instant::now() + Duration::from_secs(5);
        while !client.is_online() && tokio::time::Instant::now() < online_until {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !client.is_online() {
            return Err(Fault::new(
                "offline",
                "The computer is waiting for the Trommi hub",
            ));
        }
        match operation {
            Operation::Read(key) => client
                .session_register(&key)
                .await?
                .map(|s| serde_json::from_str(&s).map_err(Fault::from))
                .transpose(),
            Operation::Write(key, value) => {
                client
                    .set_status([(key, value)].into_iter().collect(), None)
                    .await?;
                client.settle(3_000).await?;
                Ok(None)
            }
        }
    }
    .await;
    machine.release().await;
    result
}

/// Explicit per-session activation. Requests and polls briefly open the approved computer, then release it.
pub async fn activate(cfg: &Cfg, member: &Arc<Member>) -> Result<()> {
    if member.phase() == "ready" {
        return Ok(());
    }
    if !cfg.owner().is_empty() && !member::own_slots(cfg, &cfg.owner()).is_empty() {
        member.claim(false).await;
        return if member.phase() == "ready" {
            Ok(())
        } else {
            Err(Fault::new(
                "own-key",
                "This session already has a key; waiting for that key, without joining again",
            ))
        };
    }
    let room = room_for(cfg)?;
    let nonce = crate::util::random_hex(16);
    let key = format!("join_ask/{nonce}");
    let mut ask = json!({"name":crate::identity::session_name(cfg),"folder":cfg.shown,"host":cfg.host,
        "account_label":crate::host::name(),"agent":crate::identity::agent_identity(&cfg.owner())});
    exchange(cfg, &room, Operation::Write(key.clone(), ask.clone())).await?;
    let deadline = now_ms() + trommi_core::invite::AGENT_INVITE_LIFE_MS;
    let result = async {
        let link = loop {
            if now_ms() >= deadline { return Err(Fault::new("activation-timeout", "Activation is waiting for a current human Trommi app to be online. Open it, then call connect again.")) }
            if let Some(answer) = exchange(cfg, &room, Operation::Read(format!("join_link/{nonce}"))).await? {
                if let Some(why) = answer.get("refused").and_then(Value::as_str) {
                    return Err(Fault::new("activation-refused", format!("The human app refused activation ({why}); no session was added")));
                }
                if let Some(link) = answer.get("link").and_then(Value::as_str) { break link.to_string() }
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
        };
        let joining = member.join(&link, false, false);
        tokio::pin!(joining);
        let mut bound = false;
        loop {
            tokio::select! {
                result = &mut joining => return result,
                _ = tokio::time::sleep(Duration::from_millis(deadline.saturating_sub(now_ms()))) => {
                    return Err(Fault::new("activation-timeout", "Activation did not finish; open the human Trommi app and call connect again"));
                }
                _ = tokio::time::sleep(Duration::from_millis(100)), if !bound => {
                    let hash = member.me.lock().unwrap().request_hash.clone();
                    if let Some(hash) = hash {
                        ask["request_hash"] = json!(hash);
                        exchange(cfg, &room, Operation::Write(key.clone(), ask.clone())).await?;
                        bound = true;
                    }
                }
            }
        }
    }.await;
    let _ = exchange(cfg, &room, Operation::Write(key, Value::Null)).await;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(Cfg);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("trommi-computer-{}", crate::util::random_hex(16)));
            Self(Cfg {
                keys_dir: root.join("keys"),
                folder: root.join("project"),
                shown: "~/project".into(),
                host: "test".into(),
                base: "project".into(),
                session: "cc:test".into(),
                owner: "cc:test".into(),
                takeover_ms: 1,
                hub_url: "http://127.0.0.1:9".into(),
                room: String::new(),
            })
        }
        fn key(&self, byte: u8) -> String {
            let room = trommi_core::ids::RoomId::new([byte; 32]).to_base64url();
            let dir = directory(&self.0, &room);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("session-1.key"), crate::slotstore::KEY_MARKER).unwrap();
            std::fs::write(
                dir.join("session-1.owner"),
                json!({"owner":format!("machine:{room}")}).to_string(),
            )
            .unwrap();
            room
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(self.0.keys_dir.parent().unwrap());
        }
    }

    #[test]
    fn selected_boards_never_fall_back_to_another_computers_board() {
        let mut f = Fixture::new();
        assert!(room_for(&f.0).is_err());
        let first = f.key(1);
        assert_eq!(room_for(&f.0).unwrap(), first);
        let second = f.key(2);
        assert!(room_for(&f.0).is_err(), "several boards need a choice");
        member::note_joined_room(&f.0, &second);
        assert_eq!(room_for(&f.0).unwrap(), second);
        f.0.room = trommi_core::ids::RoomId::new([3; 32]).to_base64url();
        assert!(
            room_for(&f.0).is_err(),
            "an explicit unapproved board cannot use another's authority"
        );
        f.0.room = first.clone();
        assert_eq!(room_for(&f.0).unwrap(), first);
    }

    #[tokio::test]
    async fn computer_operations_release_the_private_lock_and_refuse_symlinks() {
        let f = Fixture::new();
        let room = f.key(1);
        let held = lock(&f.0, &room).await.unwrap();
        let path = directory(&f.0, &room).join("operation.lock");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let probe = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        assert!(matches!(
            probe.try_lock(),
            Err(std::fs::TryLockError::WouldBlock)
        ));
        drop(held);
        assert!(probe.try_lock().is_ok());
        drop(probe);
        std::fs::remove_file(&path).unwrap();
        let target = directory(&f.0, &room).join("untouched");
        std::fs::write(&target, "private").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(lock(&f.0, &room).await.is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "private");
    }
}
