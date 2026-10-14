//! A group's objects in the device's store: one entry per object, read and written one at a time. A device
//! that holds the record of the form before (every object of a group in one value) reads it as it is, turns it
//! into entries in the write of the next envelope it receives in that group, and loses nothing on the way, also
//! when that write fails or the device is cut off in the middle.

use trommi_core::codec::{Reader, Writer};
use trommi_core::device::{Draft, EnvelopeOutcome, Sealed};
use trommi_core::envelope::{ObjectState, Urgency};
use trommi_core::ids::{GroupId, ObjectId, SessionId};
use trommi_core::store::{table, Entry};
use trommi_core::Error;
use trommi_tests::hub::content::Change;
use trommi_tests::hub::Hub;
use trommi_tests::{
    add_human, enrol, found_main, found_room, join_room, json, new_device_on, post_ok, publish_some,
    reopen, sync_all, take_envelope, test_keys, write, MemoryStorage, TestDevice,
};

const ZERO_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
/// The parts of the table of objects.
const WHOLE: u8 = 0;
const OBJECT: u8 = 1;
const NOTE_VERSION: u8 = 2;
const REVISION: u8 = 3;

fn objects_key(part: u8, group: &GroupId, rest: &[u8]) -> Vec<u8> {
    let id = group.as_bytes();
    [&[table::OBJECT, part, id.len() as u8][..], id, rest].concat()
}

/// The stored entries of the objects of `group`.
fn objects_of(store: &MemoryStorage, group: &GroupId) -> Vec<Entry> {
    let of_group = |entry: &Entry| {
        [WHOLE, OBJECT, NOTE_VERSION, REVISION]
            .iter()
            .any(|part| entry.key.starts_with(&objects_key(*part, group, &[])))
    };
    store.entries().into_iter().filter(of_group).collect()
}

/// Puts the objects of `group` in `store` back into the form a build before this one left: one record with
/// every object, every Note version and the count, exactly the bytes it wrote (the hub's own state of the
/// group, which took the same envelopes in the same order). Checked against the entries it replaces.
fn as_before(store: &MemoryStorage, hub: &Hub, group: &GroupId) {
    let whole = hub.objects(group);
    let held = objects_of(store, group);
    let count = |part: u8| {
        held.iter()
            .filter(|entry| entry.key.starts_with(&objects_key(part, group, &[])))
            .count()
    };
    assert_eq!(count(WHOLE), 0, "the group is in entries");
    assert_eq!(count(OBJECT), whole.iter().count());
    assert_eq!(count(NOTE_VERSION), whole.note_versions().count());
    assert_eq!(
        store.get(&objects_key(REVISION, group, &[])),
        Some(whole.revision().to_be_bytes().to_vec())
    );
    store.rewrite(|entries| {
        for entry in &held {
            entries.remove(&entry.key);
        }
        entries.insert(objects_key(WHOLE, group, &[]), whole.to_bytes().unwrap());
    });
}

fn card(title: &str, previous: &str) -> trommi_core::crypto::SecretBytes {
    json(&format!(
        r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"{previous}"}}"#
    ))
}

fn note(previous: &str, lamport: u64) -> trommi_core::crypto::SecretBytes {
    json(&format!(
        r#"{{"text":"n","lamport":{lamport},"previous_version_hash":"{previous}"}}"#
    ))
}

/// A room with two human devices and an agent in its main session, in which cards and a Note were written:
/// an open card, a closed one, an answered one, and a Note with two versions in the room group.
struct World {
    hub: Hub,
    room: GroupId,
    session: SessionId,
    group: GroupId,
    human: TestDevice,
    store: MemoryStorage,
    other: TestDevice,
    other_store: MemoryStorage,
    agent: TestDevice,
    agent_store: MemoryStorage,
    open: Sealed,
    note: Sealed,
    /// The session's objects as every device holds them now, in the form before.
    before_late: Vec<u8>,
}

fn world() -> World {
    let store = MemoryStorage::new();
    let agent_store = MemoryStorage::new();
    let mut human = new_device_on(store.handle());
    let mut agent = new_device_on(agent_store.handle());
    let other_store = MemoryStorage::new();
    let mut other = new_device_on(other_store.handle());
    let (mut hub, room) = found_room(&mut human);
    publish_some(&mut hub, &mut agent, 4);
    add_human(&mut hub, &mut human, &mut other);
    publish_some(&mut hub, &mut other, 4);
    enrol(&mut hub, &mut human, &mut agent);
    let group = found_main(&mut hub, &mut human, &agent.id());
    let session = group.session_id().unwrap();
    for device in [&mut human, &mut other, &mut agent] {
        sync_all(&hub, device);
    }
    let first = |hub: &mut Hub, agent: &mut TestDevice, title: &str| {
        let sealed = write(
            hub,
            agent,
            &Draft::CardFirst {
                session,
                urgency: Urgency::Normal,
                push: false,
                payload: card(title, ZERO_HASH),
            },
        );
        sync_all(hub, agent);
        sealed
    };
    let open = first(&mut hub, &mut agent, "open");
    let closed = first(&mut hub, &mut agent, "closed");
    let answered = first(&mut hub, &mut agent, "answered");
    write(
        &mut hub,
        &mut agent,
        &Draft::CardVersion {
            session,
            object_id: closed.object_id.unwrap(),
            closed: true,
            urgency: Urgency::Normal,
            push: false,
            payload: card("closed", &closed.envelope_hash.to_base64url()),
        },
    );
    sync_all(&hub, &mut human);
    write(
        &mut hub,
        &mut human,
        &Draft::Answer {
            session,
            object_id: answered.object_id.unwrap(),
            choices: vec!["yes".into()],
            closes: false,
            payload: json(r#"{"answer_action":"answer","choices":["yes"]}"#),
        },
    );
    let note_first = write(
        &mut hub,
        &mut human,
        &Draft::NoteFirst {
            payload: note(ZERO_HASH, 1),
        },
    );
    sync_all(&hub, &mut human);
    write(
        &mut hub,
        &mut human,
        &Draft::NoteVersion {
            object_id: note_first.object_id.unwrap(),
            closed: false,
            payload: note(&note_first.envelope_hash.to_base64url(), 2),
        },
    );
    for device in [&mut human, &mut other, &mut agent] {
        sync_all(&hub, device);
    }
    World {
        before_late: hub.objects(&group).to_bytes().unwrap(),
        hub,
        room,
        session,
        group,
        human,
        store,
        other,
        other_store,
        agent,
        agent_store,
        open,
        note: note_first,
    }
}

impl World {
    /// The agent writes the next version of the open card; returns the envelope as the hub serves it.
    fn next_version(&mut self) -> trommi_tests::hub::content::StoredEnvelope {
        write(
            &mut self.hub,
            &mut self.agent,
            &Draft::CardVersion {
                session: self.session,
                object_id: self.open.object_id.unwrap(),
                closed: false,
                urgency: Urgency::Normal,
                push: false,
                payload: card("open, again", &self.open.envelope_hash.to_base64url()),
            },
        );
        self.newest()
    }

    /// The other human device writes a version of the Note; returns the envelope as the hub serves it.
    fn next_note_version(&mut self) -> trommi_tests::hub::content::StoredEnvelope {
        write(
            &mut self.hub,
            &mut self.other,
            &Draft::NoteVersion {
                object_id: self.note.object_id.unwrap(),
                closed: false,
                payload: note(&self.note.envelope_hash.to_base64url(), 3),
            },
        );
        self.newest()
    }

    fn newest(&self) -> trommi_tests::hub::content::StoredEnvelope {
        match self.hub.changes_after(self.human.cursor()).pop() {
            Some(Change::Envelope(stored)) => *stored,
            _ => panic!("an envelope is new"),
        }
    }

    /// A second store with what the human device's store holds, its objects in the form before.
    fn store_of_before(&self) -> MemoryStorage {
        let store = self.store.reopened();
        as_before(&store, &self.hub, &self.group);
        as_before(&store, &self.hub, &self.room);
        store
    }
}

/// What a device says of the objects of `group`.
fn said(device: &TestDevice, group: &GroupId) -> Vec<(ObjectId, ObjectState, Option<trommi_core::ids::DeviceId>)> {
    device
        .objects(group)
        .unwrap()
        .into_iter()
        .map(|(id, object)| {
            assert_eq!(device.object(group, &id).unwrap(), Some(object.clone()));
            (id, object.state, device.object_owner(group, &id).unwrap())
        })
        .collect()
}

#[test]
fn the_objects_are_stored_one_entry_each() {
    let w = world();
    let count = |group: &GroupId, part: u8| {
        objects_of(&w.store, group)
            .iter()
            .filter(|entry| entry.key.starts_with(&objects_key(part, group, &[])))
            .count()
    };
    assert_eq!(
        (count(&w.group, WHOLE), count(&w.group, OBJECT), count(&w.group, NOTE_VERSION)),
        (0, 3, 0)
    );
    assert_eq!(
        (count(&w.room, WHOLE), count(&w.room, OBJECT), count(&w.room, NOTE_VERSION)),
        (0, 1, 2)
    );
    // The count of transitions: three first versions, a closing version, an answer; a Note and its version.
    assert_eq!(
        w.store.get(&objects_key(REVISION, &w.group, &[])),
        Some(5u64.to_be_bytes().to_vec())
    );
    assert_eq!(
        w.store.get(&objects_key(REVISION, &w.room, &[])),
        Some(2u64.to_be_bytes().to_vec())
    );
    assert_eq!(said(&w.human, &w.group).len(), 3);
    assert_eq!(w.human.objects(&w.group).unwrap(), w.agent.objects(&w.group).unwrap());
    // What a restart reads is what was held.
    let again = reopen(w.store.reopened()).unwrap();
    assert_eq!(said(&again, &w.group), said(&w.human, &w.group));
    assert_eq!(said(&again, &w.room), said(&w.human, &w.room));
}

#[test]
fn a_record_of_before_is_read_as_it_is_and_becomes_entries_with_the_next_envelope() {
    let mut w = world();
    let store = w.store_of_before();
    let mut before = reopen(store.handle()).expect("a device of before opens");
    // It answers as the device that never held the record does, and reading writes nothing.
    for group in [w.group, w.room] {
        assert_eq!(said(&before, &group), said(&w.human, &group));
        assert_eq!(objects_of(&store, &group).len(), 1);
    }
    assert_eq!(store.applied(), 0);

    // The next envelope of the session: its write holds the group's entries and takes the record away.
    let stored = w.next_version();
    let (here, there) = (
        take_envelope(&mut w.human, &stored),
        take_envelope(&mut before, &stored),
    );
    assert_eq!(there.outcome, EnvelopeOutcome::Applied);
    assert_eq!(there.object_after, here.object_after);
    assert_eq!(store.applied(), 1, "one write");
    assert_eq!(objects_of(&store, &w.group), objects_of(&w.store, &w.group));
    // The room group was not touched: its record stands, and still reads.
    assert_eq!(objects_of(&store, &w.room).len(), 1);
    assert_eq!(said(&before, &w.room), said(&w.human, &w.room));

    // An envelope of the room group, a Note version that follows a version the record held.
    let stored = w.next_note_version();
    let (here, there) = (
        take_envelope(&mut w.human, &stored),
        take_envelope(&mut before, &stored),
    );
    assert_eq!(there.outcome, EnvelopeOutcome::Applied);
    assert_eq!(there.object_after, here.object_after);
    assert_eq!(objects_of(&store, &w.room), objects_of(&w.store, &w.room));
    // Nothing else differs either: the two stores hold the same entries.
    assert_eq!(store.entries(), w.store.entries());
    let again = reopen(store.reopened()).unwrap();
    for group in [w.group, w.room] {
        assert_eq!(said(&again, &group), said(&w.human, &group));
    }
}

#[test]
fn an_envelope_that_changes_no_object_turns_the_record_into_entries_too() {
    let mut w = world();
    let store = w.store_of_before();
    let mut before = reopen(store.handle()).unwrap();
    write(
        &mut w.hub,
        &mut w.agent,
        &Draft::SessionChat {
            session: w.session,
            payload: json(r#"{"text":"a line"}"#),
        },
    );
    let stored = w.newest();
    take_envelope(&mut w.human, &stored);
    assert_eq!(take_envelope(&mut before, &stored).outcome, EnvelopeOutcome::Applied);
    assert_eq!(objects_of(&store, &w.group), objects_of(&w.store, &w.group));
    assert_eq!(said(&before, &w.group), said(&w.human, &w.group));
}

#[test]
fn a_write_that_fails_or_is_cut_off_leaves_the_record_of_before_whole() {
    let mut w = world();
    let store = w.store_of_before();
    let held = store.entries();
    let mut before = reopen(store.handle()).unwrap();
    let stored = w.next_version();
    // The write that would have held the entries fails: nothing of it is stored, and the device is back at
    // what is stored.
    store.fail_apply(1);
    let failed = before.receive_envelope(
        &stored.bytes,
        stored.change,
        true,
        stored.void_code.as_ref(),
        trommi_tests::now(),
    );
    assert!(matches!(failed, Err(Error::Storage(_))));
    assert_eq!(store.entries(), held);
    assert_eq!(said(&before, &w.group), said(&w.human, &w.group));
    // A device cut off there starts again from the record, takes the envelope, and holds what the device
    // that never held a record holds.
    let restarted = store.reopened();
    let mut again = reopen(restarted.handle()).unwrap();
    take_envelope(&mut w.human, &stored);
    for device in [&mut before, &mut again] {
        assert_eq!(take_envelope(device, &stored).outcome, EnvelopeOutcome::Applied);
        assert_eq!(said(device, &w.group), said(&w.human, &w.group));
    }
    assert_eq!(objects_of(&store, &w.group), objects_of(&w.store, &w.group));
    assert_eq!(objects_of(&restarted, &w.group), objects_of(&w.store, &w.group));
}

#[test]
fn a_device_of_before_writes_against_its_record_and_an_envelope_behind_builds_the_entries() {
    let mut w = world();
    // The agent's own store in the form before: it seals a version of its card against the record.
    let store = w.agent_store.reopened();
    as_before(&store, &w.hub, &w.group);
    let mut agent = reopen(store.handle()).unwrap();
    let sealed = agent
        .seal(
            &Draft::CardVersion {
                session: w.session,
                object_id: w.open.object_id.unwrap(),
                closed: true,
                urgency: Urgency::Normal,
                push: false,
                payload: card("open, closed", &w.open.envelope_hash.to_base64url()),
            },
            None,
            &[],
            trommi_tests::now(),
        )
        .expect("it seals against the record");
    assert_eq!(objects_of(&store, &w.group).len(), 1, "sealing changes no object");
    post_ok(&mut w.hub, &mut agent);
    let got = sync_all(&w.hub, &mut agent);
    assert_eq!(got[0].envelope_hash, sealed.envelope_hash);
    assert_eq!(
        agent
            .object(&w.group, &w.open.object_id.unwrap())
            .unwrap()
            .unwrap()
            .state,
        ObjectState::Closed
    );
    sync_all(&w.hub, &mut w.human);
    assert_eq!(agent.objects(&w.group).unwrap(), w.human.objects(&w.group).unwrap());
    assert_eq!(objects_of(&store, &w.group), objects_of(&w.store, &w.group));

}

#[test]
fn an_envelope_behind_later_ones_builds_the_entries_from_the_record_of_before() {
    let mut w = world();
    let made = write(
        &mut w.hub,
        &mut w.agent,
        &Draft::CardFirst {
            session: w.session,
            urgency: Urgency::Normal,
            push: false,
            payload: card("late", ZERO_HASH),
        },
    );
    sync_all(&w.hub, &mut w.human);
    sync_all(&w.hub, &mut w.agent);
    write(
        &mut w.hub,
        &mut w.human,
        &Draft::Answer {
            session: w.session,
            object_id: made.object_id.unwrap(),
            choices: vec!["no".into()],
            closes: false,
            payload: json(r#"{"answer_action":"answer"}"#),
        },
    );
    // The other device has neither. One copy of it holds the record of before; the hub's state of the
    // group as that device last saw it is what the record held.
    let store = w.other_store.reopened();
    let whole = objects_of(&store, &w.group);
    store.rewrite(|entries| {
        for entry in &whole {
            entries.remove(&entry.key);
        }
        entries.insert(objects_key(WHOLE, &w.group, &[]), w.before_late.clone());
    });
    let mut before = reopen(store.handle()).unwrap();
    assert_eq!(said(&before, &w.group), said(&w.other, &w.group));
    // The hub hands both the answer before the card it answers: the card is judged at its own place, and
    // the group's state is built again (9.2.1).
    let card_stored = w.hub.chain_of(&w.group, &w.agent.id(), 0).pop().unwrap();
    let answer_stored = w.hub.chain_of(&w.group, &w.human.id(), 0).pop().unwrap();
    for device in [&mut w.other, &mut before] {
        let early = take_envelope(device, &answer_stored);
        assert_eq!(
            (early.outcome, early.code),
            (EnvelopeOutcome::Chained, Some(Error::Forbidden))
        );
        let late = take_envelope(device, &card_stored);
        assert_eq!(late.outcome, EnvelopeOutcome::Applied);
        assert!(late.replayed);
        assert_eq!(late.object_after.unwrap().1.state, ObjectState::Answered);
    }
    assert_eq!(said(&before, &w.group), said(&w.other, &w.group));
    assert_eq!(objects_of(&store, &w.group), objects_of(&w.other_store, &w.group));
    assert_eq!(
        w.other.objects(&w.group).unwrap(),
        w.hub
            .objects(&w.group)
            .iter()
            .map(|(id, object)| (*id, object.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn objects_that_contradict_each_other_do_not_open() {
    let w = world();
    let damaged = |change: &dyn Fn(&MemoryStorage)| {
        let store = w.store.reopened();
        change(&store);
        reopen(store).err()
    };
    assert_eq!(damaged(&|_| {}), None);
    let is_storage = |error: Option<Error>| matches!(error, Some(Error::Storage(_)));
    // Both forms for one group.
    assert!(is_storage(damaged(&|store| {
        let whole = w.hub.objects(&w.group).to_bytes().unwrap();
        store.rewrite(|entries| {
            entries.insert(objects_key(WHOLE, &w.group, &[]), whole);
        });
    })));
    // A Note version without its Note, and one of an object that is no Note.
    let note_id = w.note.object_id.unwrap();
    assert!(is_storage(damaged(&|store| {
        store.rewrite(|entries| {
            entries.remove(&objects_key(OBJECT, &w.room, note_id.as_bytes()));
        });
    })));
    let card_id = w.open.object_id.unwrap();
    assert!(is_storage(damaged(&|store| {
        let version = [&card_id.as_bytes()[..], w.open.envelope_hash.as_bytes()].concat();
        store.rewrite(|entries| {
            entries.insert(objects_key(NOTE_VERSION, &w.group, &version), vec![1]);
        });
    })));
    // An entry that is none of the four, a key that is too long, values that are not what their keys hold.
    for (key, value) in [
        (objects_key(4, &w.group, &[]), vec![1]),
        (objects_key(OBJECT, &w.group, &[0; 17]), vec![0; 98]),
        (objects_key(OBJECT, &w.group, card_id.as_bytes()), vec![0; 97]),
        (objects_key(REVISION, &w.group, &[]), vec![0; 7]),
        (objects_key(REVISION, &w.group, &[1]), vec![0; 8]),
        (
            objects_key(NOTE_VERSION, &w.room, &[&note_id.as_bytes()[..], w.note.envelope_hash.as_bytes()].concat()),
            vec![2],
        ),
        (objects_key(WHOLE, &w.room, &[]), vec![0; 3]),
    ] {
        assert!(
            is_storage(damaged(&|store| {
                store.rewrite(|entries| {
                    entries.insert(key.clone(), value.clone());
                });
            })),
            "{key:02x?}"
        );
    }
}

/// `staged`, a join from outside as a device stores it beside its state, with one more entry to put.
fn staged_with(staged: &[u8], key: Vec<u8>, value: Vec<u8>) -> Vec<u8> {
    let mut reader = Reader::new(staged);
    let entries = reader.opaque().unwrap().to_vec();
    let mut put = Writer::new();
    put.fixed(reader.opaque().unwrap());
    put.opaque(&key).unwrap();
    put.opaque(&value).unwrap();
    let rest = reader.take(reader.remaining()).unwrap().to_vec();
    let mut writer = Writer::new();
    writer.opaque(&entries).unwrap();
    writer.opaque(&put.into_bytes()).unwrap();
    writer.fixed(&rest);
    writer.into_bytes()
}

#[test]
fn a_join_staged_before_with_a_record_of_objects_becomes_entries_when_it_is_adopted() {
    let mut w = world();
    // A device builds a join of the room from outside, kept beside its state until the hub takes it. Its
    // copy holds the room group's objects in the record of before (as a build before this one staged it),
    // while the device holds the same group's objects as entries already.
    let store = MemoryStorage::new();
    let mut joiner = new_device_on(store.handle());
    join_room(&w.hub, &mut joiner, &test_keys()).unwrap();
    drop(joiner);
    let held = objects_of(&w.store, &w.room);
    let record = w.hub.objects(&w.room).to_bytes().unwrap();
    store.rewrite(|entries| {
        for entry in &held {
            entries.insert(entry.key.clone(), entry.value.clone());
        }
        let staged = entries
            .keys()
            .find(|key| key.starts_with(&[table::DEVICE, 4]))
            .cloned()
            .expect("a staged join");
        let value = staged_with(&entries[&staged], objects_key(WHOLE, &w.room, &[]), record);
        entries.insert(staged, value);
    });
    let mut joiner = reopen(store.handle()).expect("it opens with its staged join");
    post_ok(&mut w.hub, &mut joiner);
    // The copy is the real state now, the objects as entries, and the state opens again.
    assert_eq!(objects_of(&store, &w.room), held);
    let again = reopen(store.reopened()).expect("the adopted state opens");
    assert_eq!(said(&again, &w.room), said(&w.human, &w.room));
}
