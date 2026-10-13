//! The store interface: the host keeps bytes under keys; the client core says what to keep.
//!
//! The protocol core's device state has its own store (`trommi_core::store::Storage`). This one holds the
//! client's model (the board, the registers, the plan's cursor): a cache that can always be built again from
//! the hub, so it needs no transactions, only "last write wins" per key.

use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;

pub trait Store {
    fn load(&self, key: &str) -> Option<Vec<u8>>;
    fn save(&mut self, key: &str, bytes: Vec<u8>);
}

/// Saves a value as JSON.
pub fn put<T: Serialize>(store: &mut dyn Store, key: &str, value: &T) {
    if let Ok(bytes) = serde_json::to_vec(value) {
        store.save(key, bytes);
    }
}

/// Loads a value saved by [`put`]; `None` when absent or of another shape (the cache is built again).
pub fn get<T: DeserializeOwned>(store: &dyn Store, key: &str) -> Option<T> {
    serde_json::from_slice(&store.load(key)?).ok()
}

#[derive(Default)]
pub struct MemoryStore(pub BTreeMap<String, Vec<u8>>);

impl Store for MemoryStore {
    fn load(&self, key: &str) -> Option<Vec<u8>> {
        self.0.get(key).cloned()
    }
    fn save(&mut self, key: &str, bytes: Vec<u8>) {
        self.0.insert(key.to_string(), bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Board;

    #[test]
    fn a_board_round_trips() {
        let mut store = MemoryStore::default();
        put(&mut store, "board", &Board::new());
        assert_eq!(get::<Board>(&store, "board"), Some(Board::new()));
        store.save("board", b"junk".to_vec());
        assert_eq!(get::<Board>(&store, "board"), None);
    }
}
