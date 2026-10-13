//! The device's store inside the App: the protocol core writes synchronously, the shell stores asynchronously.
//!
//! [`trommi_core::store::Storage::apply`] must return only once a batch is durable. Inside a sans-I/O core it
//! cannot wait, so the promise is kept from the other side: a batch is taken here at once, leaves as a
//! `StoreOp::DeviceApply` effect, and that effect is a **barrier** in the shell (core/client/README.md "Order"):
//! no request made after it starts before it is durable, and if it fails every effect behind it is dropped and the
//! App stops. So nothing that a batch implies ever leaves the device before the batch is stored.
//!
//! Batches of one step are folded into one write ([`Writes::take`]): a page of a catch-up is one durable step, not
//! one per item. The shell's revision counts those writes; the device's own count of its `apply` calls is never
//! shown to the shell.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use trommi_core::store::{Batch, Entry, Loaded, Storage, StorageError};

#[derive(Default)]
struct Inner {
    /// What `load` hands the device, once.
    loaded: Option<Loaded>,
    /// The device's own revision: what it names in `apply`.
    device_revision: u64,
    /// The writes not yet handed to the shell, folded: a key's last state (`None`: deleted).
    folded: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    /// The device is no owner any more (a write failed in the shell): every `apply` is refused.
    lost: bool,
}

/// The store the device is opened on.
pub struct MemStore(Arc<Mutex<Inner>>);

/// The engine's end: takes what the device wrote.
pub struct Writes {
    inner: Arc<Mutex<Inner>>,
    /// The revision the shell's store holds once every write handed out so far is applied.
    shell_revision: u64,
}

/// A store over what the shell loaded, and the engine's end of it.
pub fn store(revision: u64, entries: Vec<(Vec<u8>, Vec<u8>)>) -> (MemStore, Writes) {
    let inner = Arc::new(Mutex::new(Inner {
        loaded: Some(Loaded {
            revision,
            entries: entries.into_iter().map(|(k, v)| Entry::new(k, v)).collect(),
        }),
        device_revision: revision,
        ..Inner::default()
    }));
    (
        MemStore(inner.clone()),
        Writes {
            inner,
            shell_revision: revision,
        },
    )
}

fn lock(inner: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    inner.lock().unwrap_or_else(|e| e.into_inner())
}

impl Storage for MemStore {
    fn load(&mut self) -> Result<Loaded, StorageError> {
        lock(&self.0)
            .loaded
            .take()
            .ok_or_else(|| StorageError::Failed("the stored state was read already".into()))
    }

    fn apply(&mut self, expected_revision: u64, mut batch: Batch) -> Result<(), StorageError> {
        let mut inner = lock(&self.0);
        if inner.lost || expected_revision != inner.device_revision {
            return Err(StorageError::Conflict);
        }
        inner.device_revision += 1;
        // deletions first, then puts; of two puts of one key the later counts (the store's contract)
        for key in batch.delete.drain(..) {
            inner.folded.insert(key, None);
        }
        for entry in batch.put.drain(..) {
            // (an `Entry` wipes its value when dropped: the bytes are copied out, not moved)
            inner.folded.insert(entry.key.clone(), Some(entry.value.clone()));
        }
        Ok(())
    }
}

/// One write for the shell: `StoreOp::DeviceApply` with these values.
pub struct Write {
    pub expected_revision: u64,
    pub put: Vec<(Vec<u8>, Vec<u8>)>,
    pub delete: Vec<Vec<u8>>,
}

impl Writes {
    /// Everything the device wrote since the last call, as one write; none when it wrote nothing.
    pub fn take(&mut self) -> Option<Write> {
        let folded = std::mem::take(&mut lock(&self.inner).folded);
        if folded.is_empty() {
            return None;
        }
        let mut write = Write {
            expected_revision: self.shell_revision,
            put: Vec::new(),
            delete: Vec::new(),
        };
        for (key, value) in folded {
            match value {
                Some(value) => write.put.push((key, value)),
                None => write.delete.push(key),
            }
        }
        self.shell_revision += 1;
        Some(write)
    }

    /// A write failed in the shell: the device's memory is ahead of what is stored, and it may write no more.
    pub fn lose(&mut self) {
        lock(&self.inner).lost = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_of_one_step_fold_into_one() {
        let (mut store, mut writes) = store(7, vec![(vec![1], vec![1])]);
        assert_eq!(store.load().unwrap().revision, 7);
        assert!(store.load().is_err());
        assert!(writes.take().is_none());
        let mut a = Batch::new();
        a.put(vec![1], vec![2]);
        a.put(vec![2], vec![2]);
        store.apply(7, a).unwrap();
        let mut b = Batch::new();
        b.delete(vec![2]);
        b.put(vec![3], vec![3]);
        store.apply(8, b).unwrap();
        // the device's revision is its own
        assert!(matches!(store.apply(7, Batch::new()), Err(StorageError::Conflict)));
        let w = writes.take().unwrap();
        assert_eq!(w.expected_revision, 7);
        assert_eq!(w.put, vec![(vec![1], vec![2]), (vec![3], vec![3])]);
        assert_eq!(w.delete, vec![vec![2]]);
        let mut c = Batch::new();
        c.put(vec![4], vec![4]);
        store.apply(9, c).unwrap();
        assert_eq!(writes.take().unwrap().expected_revision, 8);
        writes.lose();
        assert!(matches!(store.apply(10, Batch::new()), Err(StorageError::Conflict)));
    }
}
