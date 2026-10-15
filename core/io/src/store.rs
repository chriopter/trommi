//! `Effect::Store` on disk: device and cache are separate namespaces in one durable journal.
//! Each write is one synced record, holding every put/delete or none. The slot lock owns both
//! namespaces; device revisions count only device writes. Legacy per-key cache files are
//! imported once, with a durable marker in the same record, before they are removed.

use crate::journal::{CoreStore, Journal, StoreError};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
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
    /// Preserve the former file-backed cache key limits without narrowing connector keys.
    file_keys: bool,
}

/// The tag of the cache space's side keys in a journal ([`Stores::in_journal`]).
pub const CACHE_TAG: u8 = b'c';

fn journal_key(key: &str) -> Vec<u8> {
    let mut out = vec![crate::journal::SIDE, CACHE_TAG];
    out.extend_from_slice(key.as_bytes());
    out
}

fn failed(kind: IoErrorKind, detail: &str) -> IoError {
    IoError {
        kind,
        detail: detail.to_string(),
    }
}

fn journal_error(error: StoreError, detail: &str) -> IoError {
    let kind = match error {
        StoreError::Full => IoErrorKind::Full,
        StoreError::Denied => IoErrorKind::Denied,
        _ => IoErrorKind::Failed,
    };
    failed(kind, detail)
}

impl Stores {
    /// Opens the slot: takes the device lock (`OpenError::Locked` while another process has it) and makes the
    /// folders, each mode 0700, where there are none.
    pub fn open(device_dir: &Path, cache_dir: &Path) -> Result<Stores, OpenError> {
        let journal = Journal::open(device_dir).map_err(|e| match e {
            StoreError::Locked => OpenError::Locked,
            StoreError::Damaged(what) => OpenError::Damaged(what),
            StoreError::Full => OpenError::Io("the device is full".into()),
            StoreError::Denied => OpenError::Io("the file system denied access".into()),
            StoreError::Io(what) => OpenError::Io(what),
            StoreError::Poisoned => OpenError::Io("an earlier write failed".into()),
        })?;
        migrate_cache(&journal, cache_dir)?;
        let mut stores = Self::in_journal(journal);
        stores.file_keys = true;
        Ok(stores)
    }

    /// Both spaces of a slot in a journal the caller opened (and so holds the lock of): the device's entries and,
    /// under the side keys `[SIDE, b'c', key…]`, the cache space. For the connector, whose slot is one journal.
    pub fn in_journal(journal: Journal) -> Stores {
        Stores {
            device: journal.core_store(),
            journal,
            file_keys: false,
        }
    }

    /// Runs one store op to its end (for `DeviceApply`: durable on return).
    pub fn run(&mut self, op: StoreOp) -> IoResult<StoreAnswer> {
        if self.journal.is_poisoned() {
            return IoResult::Err(failed(IoErrorKind::Failed, "an earlier store write failed; reopen the slot"));
        }
        match op {
            StoreOp::Get { keys } => Ok(StoreAnswer::Values {
                values: keys
                    .iter()
                    .map(|key| self.journal.get(&journal_key(key)).map(|v| Bytes(v.to_vec())))
                    .collect(),
            }),
            StoreOp::Scan { prefix } => {
                let entries = self
                    .journal
                    .scan(&journal_key(&prefix))
                    .into_iter()
                    .filter_map(|(key, value)| {
                        let key = String::from_utf8(key.get(2..)?.to_vec()).ok()?;
                        Some(Entry { key, value: value.to_vec().into() })
                    })
                    .collect();
                Ok(StoreAnswer::Entries { entries })
            }
            StoreOp::Put { put, delete } => self.journal_put(put, delete),
            StoreOp::DeviceLoad => self.device_load(),
            StoreOp::DeviceApply { expected_revision, put, delete } => self.device_apply(expected_revision, put, delete),
        }
        .into()
    }

    fn journal_put(&mut self, put: Vec<Entry>, delete: Vec<String>) -> Result<StoreAnswer, IoError> {
        // Validate the entire batch before staging any mutation. Keep the legacy key contract.
        for key in put.iter().map(|entry| &entry.key).chain(delete.iter()) {
            if self.file_keys && file_name(key).is_none() {
                return Err(failed(IoErrorKind::Failed, "a cache key is too long or empty"));
            }
        }
        for key in delete {
            self.journal.delete(journal_key(&key));
        }
        for entry in put {
            self.journal.put(journal_key(&entry.key), entry.value.0);
        }
        self.journal
            .commit()
            .map_err(|error| journal_error(error, "the cache was not written"))?;
        Ok(StoreAnswer::Done)
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
        self.journal.commit().map_err(|error| journal_error(error, "the device state was not written"))?;
        Ok(StoreAnswer::Done)
    }
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

/// This marker shares the migration's commit, so stale legacy files are never imported again.
const CACHE_MIGRATED: &[u8] = &[crate::journal::SIDE, b'm', b'c', 1];

fn migrate_cache(journal: &Journal, dir: &Path) -> Result<(), OpenError> {
    let io = |error: std::io::Error| OpenError::Io(error.kind().to_string());
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir).map_err(io)?;
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(io)? {
        let entry = entry.map_err(io)?;
        if let Some(key) = entry.file_name().to_str().and_then(key_of) {
            files.push((key, entry.path()));
        }
    }
    match journal.get(CACHE_MIGRATED) {
        Some(marker) if marker.as_slice() == [1] => {}
        Some(_) => return Err(OpenError::Damaged("the cache migration marker does not read".into())),
        None => {
            // Read everything before staging anything. A failed read cannot commit a partial import.
            let entries = files.iter().map(|(key, path)| {
                std::fs::read(path).map(|value| (journal_key(key), value)).map_err(io)
            }).collect::<Result<Vec<_>, _>>()?;
            for (key, value) in entries { journal.put(key, value); }
            journal.put(CACHE_MIGRATED.to_vec(), vec![1]);
            journal.commit().map_err(|error| OpenError::Io(error.to_string()))?;
        }
    }
    // Cleanup is after the durable marker. If interrupted, reopening repeats cleanup, never import.
    for (_, path) in files { std::fs::remove_file(path).map_err(io)?; }
    Ok(())
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
