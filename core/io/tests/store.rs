//! The two spaces on disk: the device space keeps the journal's promises (one owner, a revision, durable), the
//! cache space is a plain key-value folder.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use trommi_client_core::app::{DeviceEntry, Entry, IoErrorKind, IoResult, StoreAnswer, StoreOp};
use trommi_io::{OpenError, Stores};

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("trommi-io-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn ok(result: IoResult<StoreAnswer>) -> StoreAnswer {
    match result {
        IoResult::Ok(answer) => answer,
        IoResult::Err(e) => panic!("{e:?}"),
    }
}

fn entry(key: &[u8], value: &[u8]) -> DeviceEntry {
    DeviceEntry {
        key: key.into(),
        value: value.into(),
    }
}

fn apply(
    stores: &mut Stores,
    expected_revision: u64,
    put: Vec<DeviceEntry>,
    delete: Vec<&[u8]>,
) -> IoResult<StoreAnswer> {
    stores.run(StoreOp::DeviceApply {
        expected_revision,
        put,
        delete: delete.into_iter().map(Into::into).collect(),
    })
}

type Pairs = Vec<(Vec<u8>, Vec<u8>)>;

fn device(stores: &mut Stores) -> (u64, Pairs) {
    match ok(stores.run(StoreOp::DeviceLoad)) {
        StoreAnswer::Device { revision, entries } => {
            let mut all: Vec<_> = entries.into_iter().map(|e| (e.key.0, e.value.0)).collect();
            all.sort();
            (revision, all)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_device_space_is_whole_durable_and_has_one_owner() {
    let dir = folder("device");
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert_eq!(device(&mut stores), (0, vec![]));
    // a second owner is refused while the first holds the slot
    assert_eq!(
        Stores::open(&dir.join("device"), &dir.join("cache")).err(),
        Some(OpenError::Locked)
    );

    assert_eq!(
        ok(apply(
            &mut stores,
            0,
            vec![entry(b"\x01a", b"one"), entry(b"\x02b", b"two")],
            vec![]
        )),
        StoreAnswer::Done
    );
    // deletions first, then puts: a key both deleted and put stays
    assert_eq!(
        ok(apply(
            &mut stores,
            1,
            vec![entry(b"\x02b", b"2")],
            vec![b"\x02b", b"\x01a", b"\x09none"]
        )),
        StoreAnswer::Done
    );
    assert_eq!(
        device(&mut stores),
        (2, vec![(b"\x02b".to_vec(), b"2".to_vec())])
    );

    // another revision than the stored one: nothing is written
    for wrong in [0, 1, 3] {
        match apply(&mut stores, wrong, vec![entry(b"\x01x", b"x")], vec![]) {
            IoResult::Err(e) => assert_eq!(e.kind, IoErrorKind::Conflict),
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(device(&mut stores).0, 2);

    // closed and opened again (the process died): what was answered is there
    drop(stores);
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert_eq!(
        device(&mut stores),
        (2, vec![(b"\x02b".to_vec(), b"2".to_vec())])
    );
    assert_eq!(
        std::fs::metadata(dir.join("device"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn put(stores: &mut Stores, put: &[(&str, &[u8])], delete: &[&str]) {
    let op = StoreOp::Put {
        put: put
            .iter()
            .map(|(k, v)| Entry {
                key: k.to_string(),
                value: (*v).into(),
            })
            .collect(),
        delete: delete.iter().map(|k| k.to_string()).collect(),
    };
    assert_eq!(ok(stores.run(op)), StoreAnswer::Done);
}

fn get(stores: &mut Stores, keys: &[&str]) -> Vec<Option<Vec<u8>>> {
    match ok(stores.run(StoreOp::Get {
        keys: keys.iter().map(|k| k.to_string()).collect(),
    })) {
        StoreAnswer::Values { values } => values.into_iter().map(|v| v.map(|b| b.0)).collect(),
        other => panic!("{other:?}"),
    }
}

fn scan(stores: &mut Stores, prefix: &str) -> Vec<(String, Vec<u8>)> {
    match ok(stores.run(StoreOp::Scan {
        prefix: prefix.into(),
    })) {
        StoreAnswer::Entries { entries } => {
            entries.into_iter().map(|e| (e.key, e.value.0)).collect()
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_cache_space_gets_scans_and_puts() {
    let dir = folder("cache");
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert_eq!(get(&mut stores, &["c1/aa/desk"]), vec![None]);
    assert_eq!(scan(&mut stores, ""), vec![]);
    put(
        &mut stores,
        &[
            ("c1/aa/card/2", b"two"),
            ("c1/aa/card/10", b"ten"),
            ("c1/aa/desk", b"{}"),
            ("c1/ab/desk", b"other"),
            ("c1/aa/card/2", b"TWO"),
        ],
        &[],
    );
    // one answer per key asked, in the order asked; the later put of a key counts
    assert_eq!(
        get(&mut stores, &["c1/aa/desk", "nothing", "c1/aa/card/2"]),
        vec![Some(b"{}".to_vec()), None, Some(b"TWO".to_vec())]
    );
    // a scan is in key order (the keys', not the files')
    let cards = scan(&mut stores, "c1/aa/card/");
    assert_eq!(
        cards.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["c1/aa/card/10", "c1/aa/card/2"]
    );
    assert_eq!(scan(&mut stores, "c1/aa/").len(), 3);
    assert_eq!(scan(&mut stores, "c1/a").len(), 4);
    // deletions first, then puts
    put(
        &mut stores,
        &[("c1/aa/desk", b"new")],
        &["c1/aa/desk", "c1/aa/card/10", "never there"],
    );
    assert_eq!(
        get(&mut stores, &["c1/aa/desk", "c1/aa/card/10"]),
        vec![Some(b"new".to_vec()), None]
    );
    assert_eq!(scan(&mut stores, "c1/aa/card/").len(), 1);

    // no key leaves the folder, and none becomes a hidden file
    put(
        &mut stores,
        &[
            ("../../escape", b"x"),
            (".hidden", b"y"),
            ("c1/ä ö/%", b"z"),
        ],
        &[],
    );
    assert!(!dir.join("escape").exists() && !dir.parent().unwrap().join("escape").exists());
    assert_eq!(
        get(&mut stores, &["../../escape", ".hidden", "c1/ä ö/%"]),
        vec![
            Some(b"x".to_vec()),
            Some(b"y".to_vec()),
            Some(b"z".to_vec())
        ]
    );
    for file in std::fs::read_dir(dir.join("cache")).unwrap() {
        let file = file.unwrap();
        assert!(
            !file.file_name().to_string_lossy().starts_with('.'),
            "{:?} was left",
            file.file_name()
        );
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    }
    // a key no file can have is an error, not a panic and not another key
    let long = "k".repeat(300);
    match stores.run(StoreOp::Put {
        put: vec![Entry {
            key: long,
            value: b"v".as_slice().into(),
        }],
        delete: vec![],
    }) {
        IoResult::Err(e) => assert_eq!(e.kind, IoErrorKind::Failed),
        other => panic!("{other:?}"),
    }

    // opened again: everything is there, and a scan finds it from the folder
    drop(stores);
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert_eq!(
        scan(&mut stores, "c1/aa/"),
        vec![
            ("c1/aa/card/2".to_string(), b"TWO".to_vec()),
            ("c1/aa/desk".to_string(), b"new".to_vec())
        ]
    );
    assert_eq!(scan(&mut stores, "").len(), 6);
    let _ = std::fs::remove_dir_all(&dir);
}
