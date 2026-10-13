//! What the app keeps on disk, under one folder per room (`~/.local/share/trommi-linux/<room>/`, mode 0700):
//!
//! ```text
//! device/       the device's state: the connector's journal (keys, groups, chains, outbox), one owner at a time
//! room.json     the hub, the room, this device
//! model.json    the board model as the client core writes it, with the change it reaches: the next start shows
//!               it at once and reads only what came after
//! ```
//!
//! The device's state is the core's to judge; the model is a cache and may be thrown away at any time.

use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use trommi_connector::store::{CoreStore, Journal};
use trommi_core::store::{Batch, Loaded, Storage, StorageError};

/// The core's store over the journal, durable when `apply` returns: the journal stages a batch and writes it with
/// `commit`, which is done here at once (the connector commits per operation; this app has nothing of its own to
/// write beside the core's batches).
pub struct DurableStore {
    core: CoreStore,
    journal: Journal,
}

impl DurableStore {
    pub fn new(journal: Journal) -> Self {
        Self {
            core: journal.core_store(),
            journal,
        }
    }
}

impl Storage for DurableStore {
    fn load(&mut self) -> Result<Loaded, StorageError> {
        self.core.load()
    }

    fn apply(&mut self, expected_revision: u64, batch: Batch) -> Result<(), StorageError> {
        self.core.apply(expected_revision, batch)?;
        self.journal
            .commit()
            .map_err(|e| StorageError::Failed(e.to_string()))
    }
}

/// Where the app keeps its rooms: `TROMMI_LINUX_DIR`, else `$XDG_DATA_HOME/trommi-linux`, else
/// `~/.local/share/trommi-linux`.
pub fn base_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TROMMI_LINUX_DIR") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("trommi-linux");
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/trommi-linux")
}

/// A folder of the app's own, created with mode 0700.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

/// Writes a file whole or not at all (a temporary file renamed over it), mode 0600.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(tmp, path)
}

/// The room a folder holds.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomRecord {
    pub hub: String,
    /// The room id, hex.
    pub room: String,
    /// This device, hex.
    pub device: String,
    /// A sign-in with the code whose session joins are not all through yet.
    pub code_join: bool,
}

impl RoomRecord {
    pub fn to_json(&self) -> Value {
        json!({ "hub": self.hub, "room": self.room, "device": self.device, "code_join": self.code_join })
    }

    pub fn of(v: &Value) -> Option<RoomRecord> {
        Some(RoomRecord {
            hub: v["hub"].as_str()?.to_string(),
            room: v["room"].as_str()?.to_string(),
            device: v["device"].as_str()?.to_string(),
            code_join: v["code_join"].as_bool().unwrap_or(false),
        })
    }

    pub fn load(dir: &Path) -> Option<RoomRecord> {
        let text = std::fs::read_to_string(dir.join("room.json")).ok()?;
        Self::of(&serde_json::from_str(&text).ok()?)
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        write_atomic(&dir.join("room.json"), self.to_json().to_string().as_bytes())
    }
}

/// The rooms this machine holds, newest first: the folders with a room record.
pub fn rooms() -> Vec<PathBuf> {
    let mut out: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(base_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("room.json").is_file())
        .map(|p| {
            let at = std::fs::metadata(p.join("room.json"))
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            (at, p)
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// The cached model: its snapshot and the change it reaches.
pub fn load_model(dir: &Path) -> Option<(String, u64)> {
    let text = std::fs::read_to_string(dir.join("model.json")).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    Some((v["snapshot"].as_str()?.to_string(), v["cursor"].as_u64()?))
}

pub fn save_model(dir: &Path, snapshot: &str, cursor: u64) -> std::io::Result<()> {
    let v = json!({ "cursor": cursor, "snapshot": snapshot });
    write_atomic(&dir.join("model.json"), v.to_string().as_bytes())
}
