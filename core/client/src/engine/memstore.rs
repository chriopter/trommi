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

use std::collections::{BTreeMap, BTreeSet};
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
    /// Every key the store holds, for a store that may be wiped (a device made by a join with the code).
    keys: Option<BTreeSet<Vec<u8>>>,
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
    // (every key is followed, so a sign-out can take the whole state away in one write)
    let keys: BTreeSet<Vec<u8>> = entries.iter().map(|(k, _)| k.clone()).collect();
    let inner = Arc::new(Mutex::new(Inner {
        keys: Some(keys),
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

/// A store for a device that a join with the code makes: empty for the device (its revision 0), at `revision`
/// in the shell, and every key it is given is kept so that a join that fails can take all of it away
/// ([`Writes::wipe`]). `entries`: what the shell holds already (a join interrupted earlier), deleted by the first
/// write unless the device is opened on them.
pub fn wipeable(revision: u64, entries: Vec<(Vec<u8>, Vec<u8>)>) -> (MemStore, Writes) {
    let keys: BTreeSet<Vec<u8>> = entries.iter().map(|(k, _)| k.clone()).collect();
    let (store, writes) = store(revision, entries);
    lock(&store.0).keys = Some(keys);
    (store, writes)
}

/// A store for a new device over what the shell holds at `revision`: the device sees an empty store at its own
/// revision 0; the shell's entries `stale` are deleted with the device's first write.
pub fn fresh(revision: u64, stale: Vec<Vec<u8>>) -> (MemStore, Writes) {
    let (store, writes) = store(revision, Vec::new());
    {
        let mut inner = lock(&store.0);
        inner.loaded = Some(Loaded { revision: 0, entries: Vec::new() });
        inner.device_revision = 0;
        inner.keys = Some(BTreeSet::new());
        for key in stale {
            inner.folded.insert(key, None);
        }
    }
    (store, writes)
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
            if let Some(keys) = &mut inner.keys {
                keys.remove(&key);
            }
            inner.folded.insert(key, None);
        }
        for entry in batch.put.drain(..) {
            if let Some(keys) = &mut inner.keys {
                keys.insert(entry.key.clone());
            }
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

    /// The revision the shell's store holds once every write handed out so far is applied.
    pub fn revision(&self) -> u64 {
        self.shell_revision
    }

    /// Everything the store holds taken away, in one write, and the device may write no more: a join with the
    /// code that failed or a sign-out leaves no half device. None when it holds nothing.
    pub fn wipe(&mut self) -> Option<Write> {
        let mut inner = lock(&self.inner);
        inner.lost = true;
        let keys = inner.keys.take()?;
        let mut delete: BTreeSet<Vec<u8>> = keys;
        delete.extend(std::mem::take(&mut inner.folded).into_keys());
        drop(inner);
        if delete.is_empty() {
            return None;
        }
        let write = Write { expected_revision: self.shell_revision, put: Vec::new(), delete: delete.into_iter().collect() };
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

    #[test]
    fn a_fresh_store_is_empty_to_the_device_and_wipes_whole() {
        let (mut store, mut writes) = fresh(5, vec![vec![9]]);
        let loaded = store.load().unwrap();
        assert_eq!((loaded.revision, loaded.entries.len()), (0, 0));
        let mut a = Batch::new();
        a.put(vec![1], vec![1]);
        a.put(vec![2], vec![2]);
        store.apply(0, a).unwrap();
        let w = writes.take().unwrap();
        assert_eq!(w.expected_revision, 5);
        assert_eq!(w.delete, vec![vec![9]]);
        let mut b = Batch::new();
        b.delete(vec![1]);
        b.put(vec![3], vec![3]);
        store.apply(1, b).unwrap();
        let w = writes.wipe().unwrap();
        assert_eq!(w.expected_revision, 6);
        assert_eq!(w.delete, vec![vec![1], vec![2], vec![3]]);
        assert!(w.put.is_empty());
        assert!(matches!(store.apply(2, Batch::new()), Err(StorageError::Conflict)));
        // (a store opened as it was stored wipes whole too: a sign-out takes everything)
        let (_, mut plain) = super::store(1, vec![(vec![1], vec![1])]);
        assert_eq!(plain.wipe().map(|w| w.delete), Some(vec![vec![1]]));
    }
}
