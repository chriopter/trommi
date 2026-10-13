//! `Effect::Store` on disk: the two spaces of the contract (core/client/README.md, "What is stored").
//!
//! ```text
//! <device dir>/   the device space: the journal (journal.rs: lock, state.snap, state.log), mode 0700. Opening it
//!                 takes the lock: one owner. A write is one record, synced before it is answered; the revision
//!                 is kept beside the entries and compared on every `DeviceApply`.
//! <cache dir>/    the cache space: one file per key, mode 0600, written beside its place and renamed onto it.
//!                 Not synced: a lost write costs a catch-up, never data.
//! ```
//!
//! A cache key becomes a file name by escaping every byte that is not a letter, a digit, `-` or `_` as `%XX`
//! (so `/` and `.` are escaped and no key leaves the folder); a key whose name would pass 255 bytes is refused.

use crate::journal::{CoreStore, Journal, StoreError};
use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use trommi_client_core::app::{
    Bytes, DeviceEntry, Entry, IoError, IoErrorKind, IoResult, StoreAnswer, StoreOp,
};
use trommi_core::store::{Batch, Storage, StorageError};

/// Why the stores of a slot could not be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Another process owns this device state (the app runs twice).
    Locked,
    /// The device state does not read.
    Damaged(String),
    /// The file system failed.
    Io(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Locked => f.write_str("another process holds this state"),
            OpenError::Damaged(what) => write!(f, "the stored state does not read: {what}"),
            OpenError::Io(what) => write!(f, "the state could not be opened: {what}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Both spaces of one slot. Holding it is holding the slot's lock.
pub struct Stores {
    journal: Journal,
    device: CoreStore,
    cache: Cache,
}

fn failed(kind: IoErrorKind, detail: &str) -> IoError {
    IoError {
        kind,
        detail: detail.to_string(),
    }
}

/// A file system error as the contract sorts it; the detail is the kind's name, never a path.
fn io_error(e: &std::io::Error) -> IoError {
    let kind = match e.kind() {
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => IoErrorKind::Full,
        ErrorKind::PermissionDenied => IoErrorKind::Denied,
        _ => IoErrorKind::Failed,
    };
    failed(kind, &e.kind().to_string())
}

impl Stores {
    /// Opens the slot: takes the device lock (`OpenError::Locked` while another process has it) and makes the
    /// folders, each mode 0700, where there are none.
    pub fn open(device_dir: &Path, cache_dir: &Path) -> Result<Stores, OpenError> {
        let journal = Journal::open(device_dir).map_err(|e| match e {
            StoreError::Locked => OpenError::Locked,
            StoreError::Damaged(what) => OpenError::Damaged(what),
            StoreError::Io(what) => OpenError::Io(what),
            StoreError::Poisoned => OpenError::Io("an earlier write failed".into()),
        })?;
        let cache = Cache::open(cache_dir).map_err(|e| OpenError::Io(e.kind().to_string()))?;
        Ok(Stores {
            device: journal.core_store(),
            journal,
            cache,
        })
    }

    /// Runs one store op to its end (for `DeviceApply`: durable on return).
    pub fn run(&mut self, op: StoreOp) -> IoResult<StoreAnswer> {
        match op {
            StoreOp::Get { keys } => self.cache.get(&keys),
            StoreOp::Scan { prefix } => self.cache.scan(&prefix),
            StoreOp::Put { put, delete } => self.cache.put(put, delete),
            StoreOp::DeviceLoad => self.device_load(),
            StoreOp::DeviceApply {
                expected_revision,
                put,
                delete,
            } => self.device_apply(expected_revision, put, delete),
        }
        .into()
    }

    fn device_load(&mut self) -> Result<StoreAnswer, IoError> {
        let loaded = self
            .device
            .load()
            .map_err(|_| failed(IoErrorKind::Failed, "the device state is not readable"))?;
        Ok(StoreAnswer::Device {
            revision: loaded.revision,
            entries: loaded
                .entries
                .iter()
                .map(|e| DeviceEntry {
                    key: e.key.clone().into(),
                    value: e.value.clone().into(),
                })
                .collect(),
        })
    }

    fn device_apply(
        &mut self,
        expected_revision: u64,
        put: Vec<DeviceEntry>,
        delete: Vec<Bytes>,
    ) -> Result<StoreAnswer, IoError> {
        let mut batch = Batch::new();
        for key in delete {
            batch.delete(key.0);
        }
        for entry in put {
            batch.put(entry.key.0, entry.value.0);
        }
        self.device
            .apply(expected_revision, batch)
            .map_err(|e| match e {
                StorageError::Conflict => failed(IoErrorKind::Conflict, "another owner wrote"),
                StorageError::Failed(_) => {
                    failed(IoErrorKind::Failed, "the device state was not written")
                }
            })?;
        // the journal stages a batch; `commit` writes and syncs it. After a failure it stays poisoned: every
        // later op fails, and the shell opens the slot again.
        self.journal.commit().map_err(|e| match e {
            StoreError::Io(what) if what.contains("No space") => {
                failed(IoErrorKind::Full, "no space")
            }
            _ => failed(IoErrorKind::Failed, "the device state was not written"),
        })?;
        Ok(StoreAnswer::Done)
    }
}

/// The cache space: a folder of files.
struct Cache {
    dir: PathBuf,
    /// Every key stored, read from the folder at the first scan and kept current from then on.
    keys: Option<BTreeSet<String>>,
    /// Counts the temporary files of this process.
    writes: u64,
}

/// The longest file name a key may give.
const MAX_NAME: usize = 255;

fn file_name(key: &str) -> Option<String> {
    let mut name = String::with_capacity(key.len() + 8);
    for b in key.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            name.push(b as char);
        } else {
            name.push_str(&format!("%{b:02X}"));
        }
    }
    (!name.is_empty() && name.len() <= MAX_NAME).then_some(name)
}

fn key_of(name: &str) -> Option<String> {
    let bytes = name.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = name.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let key = String::from_utf8(out).ok()?;
    // only names this module writes count (a temporary file starts with a dot, which a key's name never does)
    (file_name(&key).as_deref() == Some(name)).then_some(key)
}

impl Cache {
    fn open(dir: &Path) -> std::io::Result<Cache> {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
        Ok(Cache {
            dir: dir.to_path_buf(),
            keys: None,
            writes: 0,
        })
    }

    fn path(&self, key: &str) -> Result<PathBuf, IoError> {
        file_name(key)
            .map(|name| self.dir.join(name))
            .ok_or_else(|| failed(IoErrorKind::Failed, "a key no file can have"))
    }

    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, IoError> {
        match std::fs::read(self.path(key)?) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(&e)),
        }
    }

    fn keys(&mut self) -> Result<&mut BTreeSet<String>, IoError> {
        if self.keys.is_none() {
            let mut keys = BTreeSet::new();
            for entry in std::fs::read_dir(&self.dir).map_err(|e| io_error(&e))? {
                let name = entry.map_err(|e| io_error(&e))?.file_name();
                if let Some(key) = name.to_str().and_then(key_of) {
                    keys.insert(key);
                }
            }
            self.keys = Some(keys);
        }
        Ok(self.keys.as_mut().expect("just read"))
    }

    fn get(&mut self, keys: &[String]) -> Result<StoreAnswer, IoError> {
        let values = keys
            .iter()
            .map(|key| Ok(self.read(key)?.map(Bytes)))
            .collect::<Result<_, IoError>>()?;
        Ok(StoreAnswer::Values { values })
    }

    fn scan(&mut self, prefix: &str) -> Result<StoreAnswer, IoError> {
        let found: Vec<String> = self
            .keys()?
            .range(prefix.to_string()..)
            .take_while(|key| key.starts_with(prefix))
            .cloned()
            .collect();
        let mut entries = Vec::with_capacity(found.len());
        for key in found {
            // (a file that went since the folder was read is a key that is not stored)
            if let Some(value) = self.read(&key)? {
                entries.push(Entry {
                    key,
                    value: value.into(),
                });
            }
        }
        Ok(StoreAnswer::Entries { entries })
    }

    fn put(&mut self, put: Vec<Entry>, delete: Vec<String>) -> Result<StoreAnswer, IoError> {
        for key in delete {
            match std::fs::remove_file(self.path(&key)?) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(io_error(&e)),
                _ => {}
            }
            if let Some(keys) = self.keys.as_mut() {
                keys.remove(&key);
            }
        }
        for entry in put {
            let path = self.path(&entry.key)?;
            self.writes += 1;
            let tmp = self
                .dir
                .join(format!(".new.{}.{}", std::process::id(), self.writes));
            let written = (|| {
                use std::io::Write;
                let _ = std::fs::remove_file(&tmp);
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(&tmp)?;
                file.write_all(&entry.value)?;
                drop(file);
                std::fs::rename(&tmp, &path)
            })();
            if let Err(e) = written {
                let _ = std::fs::remove_file(&tmp);
                return Err(io_error(&e));
            }
            if let Some(keys) = self.keys.as_mut() {
                keys.insert(entry.key);
            }
        }
        Ok(StoreAnswer::Done)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_file_name_and_back() {
        for key in [
            "c1/ab12/desk",
            "c1/ab/card/00ff",
            "a.b",
            "..",
            "ä/%",
            "A_b-9",
        ] {
            let name = file_name(key).unwrap();
            assert!(!name.contains('/') && !name.contains('.'), "{name}");
            assert_eq!(key_of(&name).as_deref(), Some(key));
        }
        assert_eq!(file_name("c1/x").unwrap(), "c1%2Fx");
        assert!(file_name("").is_none());
        assert!(file_name(&"/".repeat(86)).is_none(), "258 bytes of name");
        // names this module did not write are no keys
        assert_eq!(key_of(".new.1.2"), None);
        assert_eq!(key_of("c1%2fx"), None);
        assert_eq!(key_of("a%4"), None);
    }
}
