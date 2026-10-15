//! A slot's state as the connector's shell sees it: the journal (`store.rs`), opened under the slot's claim, and
//! the few values of the shell's own that live in it (the bridge's state, what waited for another session). The
//! driver (`driver.rs`) works on the same journal; a value of the shell's own is written as a record of its own.
use crate::driver::{kv_get, kv_set, room_record, RoomRecord};
use crate::error::Result;
use crate::store::Journal;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The marker a keyed slot's key file holds. The device's key itself is in the slot's state (13.1).
pub const KEY_MARKER: &str = "trommi-state v2\n";

/// The open state of one slot.
pub struct SlotStore {
    journal: Journal,
}

/// Where a slot's state lives: beside its key file, `<name>.state/`.
pub fn state_dir(key_file: &Path) -> PathBuf {
    key_file.with_extension("state")
}

impl SlotStore {
    /// Opens the state beside `key_file`, taking its lock. `state-locked` while another process holds it.
    pub fn open(key_file: &Path) -> Result<Arc<SlotStore>> {
        Ok(Arc::new(SlotStore {
            journal: Journal::open(&state_dir(key_file))?,
        }))
    }

    /// The journal.
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// The room this slot was joined to, if it was.
    pub fn room(&self) -> Option<RoomRecord> {
        room_record(&self.journal)
    }

    /// A value of the shell's own.
    pub fn get(&self, name: &str) -> Option<Value> {
        kv_get(&self.journal, name)
    }

    /// Stores a value of the shell's own, written at once.
    pub fn set(&self, name: &str, value: Value) {
        if let Err(fault) = kv_set(&self.journal, name, &value) {
            eprintln!("[trommi] state not written: {}", fault.code);
        }
    }
}
