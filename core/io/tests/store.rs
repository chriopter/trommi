//! The two spaces on disk: the device space keeps the journal's promises (one owner, a revision, durable), the
//! cache space shares its atomic journal under a separate namespace.

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

#[test]
fn in_one_journal_the_cache_lives_beside_the_device_and_the_owners_own_entries() {
    use trommi_io::journal::{Journal, SIDE};
    let dir = folder("in-journal");
    let journal = Journal::open(&dir).unwrap();
    // an entry of the journal's owner of its own (another side tag) is not the cache's
    journal.put(vec![SIDE, b'k', 1, b'x'], b"mine".to_vec());
    journal.commit().unwrap();
    let mut stores = Stores::in_journal(journal.clone());
    assert_eq!(device(&mut stores), (0, vec![]));
    ok(apply(&mut stores, 0, vec![entry(b"\x01dev", b"one")], vec![]));
    put(&mut stores, &[("engine/ab", b"rec"), ("c1/ab/meta", b"m")], &[]);
    assert_eq!(get(&mut stores, &["engine/ab", "nope"]), vec![Some(b"rec".to_vec()), None]);
    assert_eq!(scan(&mut stores, "c1/"), vec![("c1/ab/meta".to_string(), b"m".to_vec())]);
    put(&mut stores, &[], &["engine/ab"]);
    assert_eq!(get(&mut stores, &["engine/ab"]), vec![None]);
    // the device space holds only the device's entries; the owner's entry is as it was
    assert_eq!(device(&mut stores), (1, vec![(b"\x01dev".to_vec(), b"one".to_vec())]));
    assert_eq!(journal.get(&[SIDE, b'k', 1, b'x']).map(|v| v.to_vec()), Some(b"mine".to_vec()));
    // all of it is on disk: opened again, the same
    drop(stores);
    drop(journal);
    let mut stores = Stores::in_journal(Journal::open(&dir).unwrap());
    assert_eq!(get(&mut stores, &["c1/ab/meta"]), vec![Some(b"m".to_vec())]);
    assert_eq!(device(&mut stores).0, 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_cache_batch_leaves_every_key_unchanged() {
    let dir = folder("cache-atomic");
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    put(&mut stores, &[("existing", b"before"), ("keep", b"yes")], &[]);
    let result = stores.run(StoreOp::Put {
        put: vec![
            Entry { key: "existing".into(), value: b"after".as_slice().into() },
            Entry { key: "k".repeat(300), value: b"refused".as_slice().into() },
        ],
        delete: vec!["keep".into()],
    });
    assert!(matches!(result, IoResult::Err(_)));
    drop(stores);
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert_eq!(get(&mut stores, &["existing", "keep"]), vec![Some(b"before".to_vec()), Some(b"yes".to_vec())]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn poisoned_cache_state_cannot_be_read_as_if_a_failed_write_had_committed() {
    use trommi_io::journal::Journal;
    let dir = folder("cache-poison");
    let journal = Journal::open(&dir).unwrap();
    let mut stores = Stores::in_journal(journal.clone());
    put(&mut stores, &[("engine/requests", b"durable")], &[]);
    journal.put([&[trommi_io::journal::SIDE, b'c'][..], b"engine/requests"].concat(), b"uncommitted".to_vec());
    journal.poison();
    assert!(matches!(stores.run(StoreOp::Get { keys: vec!["engine/requests".into()] }), IoResult::Err(_)));
    assert!(matches!(stores.run(StoreOp::Scan { prefix: "engine/".into() }), IoResult::Err(_)));
    drop(stores); drop(journal);
    let mut stores = Stores::in_journal(Journal::open(&dir).unwrap());
    assert_eq!(get(&mut stores, &["engine/requests"]), vec![Some(b"durable".to_vec())]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn legacy_cache_migration_keeps_pending_work_and_never_reimports_stale_files() {
    let dir = folder("cache-migration");
    let cache = dir.join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("engine%2Froom"), b"room identity").unwrap();
    std::fs::write(cache.join("engine%2Frequests"), b"pending request").unwrap();
    std::fs::create_dir(cache.join("unreadable")).unwrap();
    // A failed read leaves no migration marker or partial import.
    assert!(Stores::open(&dir.join("device"), &cache).is_err());
    std::fs::remove_dir(cache.join("unreadable")).unwrap();
    let mut stores = Stores::open(&dir.join("device"), &cache).unwrap();
    assert_eq!(get(&mut stores, &["engine/room", "engine/requests"]), vec![Some(b"room identity".to_vec()), Some(b"pending request".to_vec())]);
    assert_eq!(device(&mut stores), (0, vec![]), "cache migration changes no device revision");
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    put(&mut stores, &[("engine/room", b"current identity")], &["engine/requests"]);
    drop(stores);
    // A crash after the durable migration but before cleanup leaves stale files.
    std::fs::write(cache.join("engine%2Froom"), b"stale identity").unwrap();
    std::fs::write(cache.join("engine%2Frequests"), b"already answered").unwrap();
    let mut stores = Stores::open(&dir.join("device"), &cache).unwrap();
    assert_eq!(get(&mut stores, &["engine/room", "engine/requests"]), vec![Some(b"current identity".to_vec()), None]);
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    drop(stores);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sixty_four_cache_puts_are_one_durable_journal_record() {
    let dir = folder("cache-cost");
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    let log = dir.join("device/state.log");
    let before = std::fs::metadata(&log).unwrap().len();
    let entries = (0..64).map(|n| Entry {
        key: format!("c1/aa/card/{n}"), value: vec![n; 1024].into(),
    }).collect();
    ok(stores.run(StoreOp::Put { put: entries, delete: vec![] }));
    let bytes = std::fs::read(&log).unwrap();
    let added = &bytes[usize::try_from(before).unwrap()..];
    let payload = u32::from_le_bytes(added[..4].try_into().unwrap()) as usize;
    assert_eq!(added.len(), payload + 4 + 32, "all64puts form exactly one journal record");
    assert_eq!(std::fs::read_dir(dir.join("cache")).unwrap().count(), 0);
    println!("64 cache puts: one synced journal record, {} appended bytes, no per-key files", added.len());
    drop(stores);
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert_eq!(scan(&mut stores, "c1/aa/card/").len(), 64);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_durable_public_notice_survives_private_device_erasure_and_its_ack_survives_reopen() {
    let dir = folder("public-departure");
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    ok(apply(&mut stores, 0, vec![entry(b"key", b"private signing key")], vec![]));
    ok(stores.run(StoreOp::PutDurable { put:vec![Entry { key:"public-departure/room.device".into(), value:b"public signed notice".as_slice().into() }], delete:vec![] }));
    ok(apply(&mut stores, 1, vec![], vec![b"key"]));
    drop(stores);
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert!(device(&mut stores).1.is_empty());
    assert_eq!(get(&mut stores, &["public-departure/room.device"]), vec![Some(b"public signed notice".to_vec())]);
    ok(stores.run(StoreOp::PutDurable { put:vec![], delete:vec!["public-departure/room.device".into()] }));
    drop(stores);
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    assert!(scan(&mut stores, "public-departure/").is_empty());
    drop(stores);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cache_names_are_ordered_and_never_include_device_entries_or_cached_values() {
    let dir = folder("cache-names");
    let mut stores = Stores::open(&dir.join("device"), &dir.join("cache")).unwrap();
    ok(apply(&mut stores, 0, vec![entry(b"private-device-key", b"secret")], vec![]));
    put(&mut stores, &[("private/b", b"big value"), ("private/a", b"another value"), ("other", b"unrelated")], &[]);
    assert_eq!(ok(stores.run(StoreOp::Keys { prefix:"private/".into() })), StoreAnswer::Keys { keys:vec!["private/a".into(),"private/b".into()] });
    assert_eq!(ok(stores.run(StoreOp::Keys { prefix:String::new() })), StoreAnswer::Keys { keys:vec!["other".into(),"private/a".into(),"private/b".into()] });
    drop(stores);
    let _ = std::fs::remove_dir_all(dir);
}
