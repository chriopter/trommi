//! What the phone's notification extension gets of a device's content keys, and what it opens with them
//! (spec/v1.md 6.4, 15.2): of each session group the key of its current epoch, nothing of the room group and nothing
//! older; with it the envelope that rang opens to its header and payload, and nothing else does.

use trommi_core::device::Draft;
use trommi_core::envelope::Urgency;
use trommi_core::ids::GroupId;
use trommi_core_ffi::{
    open_notify_envelope, CoreDevice, CoreError, ErrorCode, NotifyKey, ObjectType,
};
use trommi_tests::hub::Hub;
use trommi_tests::store::MemoryStorage;
use trommi_tests::{
    enrol, found_main, found_room, json, new_device, new_device_on, now, post_ok, publish_some,
    sync_all, write, TestDevice,
};

const ZERO_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// A room with one human device (on `store`) and an agent in its main session.
struct World {
    hub: Hub,
    room: GroupId,
    group: GroupId,
    human: TestDevice,
    agent: TestDevice,
    store: MemoryStorage,
}

fn world() -> World {
    let store = MemoryStorage::new();
    let mut human = new_device_on(store.handle());
    let mut agent = new_device();
    let (mut hub, room) = found_room(&mut human);
    publish_some(&mut hub, &mut agent, 4);
    enrol(&mut hub, &mut human, &mut agent);
    let group = found_main(&mut hub, &mut human, &agent.id());
    sync_all(&hub, &mut human);
    sync_all(&hub, &mut agent);
    World {
        hub,
        room,
        group,
        human,
        agent,
        store,
    }
}

impl World {
    /// The agent writes a card that rings; the envelope as the hub serves it.
    fn card(&mut self, title: &str) -> Vec<u8> {
        let session = self.group.session_id().expect("a session group");
        let payload = format!(
            r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"{ZERO_HASH}"}}"#
        );
        write(
            &mut self.hub,
            &mut self.agent,
            &Draft::CardFirst {
                session,
                urgency: Urgency::High,
                push: true,
                payload: json(&payload),
            },
        );
        self.hub
            .content
            .envelopes
            .last()
            .expect("the hub took it")
            .bytes
            .clone()
    }

    /// The human device's state as the app opens it through the binding.
    fn app(&self) -> CoreDevice {
        CoreDevice::open_on(Box::new(self.store.handle())).expect("the device opens")
    }
}

fn code_of<T>(result: Result<T, CoreError>) -> ErrorCode {
    match result {
        Ok(_) => panic!("the call was not refused"),
        Err(error) => error.code(),
    }
}

#[test]
fn the_extension_gets_the_current_key_of_each_session_group_and_nothing_else() {
    let w = world();
    let epoch = w.human.group(&w.group).expect("the session").epoch;
    let room_epoch = w.human.group(&w.room).expect("the room").epoch;
    let keys = w.app().notify_keys().expect("the keys");
    assert_eq!(keys.len(), 1, "one session group, one epoch, no room group");
    assert_eq!(keys[0].group, w.group.as_bytes().to_vec());
    assert_eq!(keys[0].epoch, epoch);
    assert_eq!(
        keys[0].key,
        w.human
            .content_key(&w.group, epoch)
            .expect("held")
            .expose()
            .to_vec()
    );
    assert!(keys
        .iter()
        .all(|key| key.group != w.room.as_bytes().to_vec()));
    assert!(
        w.human.content_key(&w.room, room_epoch).is_ok(),
        "the room key is held, and not handed out"
    );
    // never printed
    assert_eq!(format!("{:?}", keys[0]), "NotifyKey(<redacted>)");
}

#[test]
fn after_a_new_epoch_only_the_new_key_is_handed_out() {
    let mut w = world();
    let before = w.app().notify_keys().expect("the keys");
    w.human
        .update(&w.group, true, now())
        .expect("an update Commit");
    post_ok(&mut w.hub, &mut w.human);
    sync_all(&w.hub, &mut w.agent);
    let after = w.app().notify_keys().expect("the keys");
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].epoch, before[0].epoch + 1);
    assert_ne!(after[0].key, before[0].key);
    // a card of the new epoch opens with the new key only
    let envelope = w.card("After the update");
    assert!(open_notify_envelope(after, envelope.clone()).is_ok());
    assert_eq!(
        code_of(open_notify_envelope(before, envelope)),
        ErrorCode::NoKey
    );
}

#[test]
fn the_card_that_rang_opens_to_its_header_and_title() {
    let mut w = world();
    let envelope = w.card("Ship it?");
    let keys = w.app().notify_keys().expect("the keys");
    let opened = open_notify_envelope(keys, envelope).expect("it opens");
    assert_eq!(opened.header.group, w.group.as_bytes().to_vec());
    assert_eq!(opened.header.sender, w.agent.id().as_bytes().to_vec());
    assert!(opened.header.push);
    let object = opened.header.object.as_ref().expect("an object block");
    assert_eq!(object.object_type, ObjectType::Card);
    let payload: serde_json::Value = serde_json::from_slice(&opened.payload).expect("JSON");
    assert_eq!(payload["title"], "Ship it?");
    assert_eq!(format!("{opened:?}"), "NotifyOpened(<redacted>)");
}

#[test]
fn what_is_not_the_envelope_under_its_key_does_not_open() {
    let mut w = world();
    let envelope = w.card("Ship it?");
    let keys = w.app().notify_keys().expect("the keys");
    // no key at all, another group's key, another epoch's key
    assert_eq!(
        code_of(open_notify_envelope(Vec::new(), envelope.clone())),
        ErrorCode::NoKey
    );
    let elsewhere = NotifyKey {
        group: w.room.as_bytes().to_vec(),
        ..keys[0].clone()
    };
    assert_eq!(
        code_of(open_notify_envelope(vec![elsewhere], envelope.clone())),
        ErrorCode::NoKey
    );
    let later = NotifyKey {
        epoch: keys[0].epoch + 1,
        ..keys[0].clone()
    };
    assert_eq!(
        code_of(open_notify_envelope(vec![later], envelope.clone())),
        ErrorCode::NoKey
    );
    // the right place, the wrong bytes
    let wrong = NotifyKey {
        key: vec![9; 32],
        ..keys[0].clone()
    };
    assert_eq!(
        code_of(open_notify_envelope(vec![wrong], envelope.clone())),
        ErrorCode::DecryptFailed
    );
    let short = NotifyKey {
        key: vec![9; 31],
        ..keys[0].clone()
    };
    assert_eq!(
        code_of(open_notify_envelope(vec![short], envelope.clone())),
        ErrorCode::BadFormat
    );
    // a signature that is not the sender's: refused before anything is opened
    let mut forged = envelope.clone();
    let last = forged.len() - 1;
    forged[last] ^= 1;
    assert_eq!(
        code_of(open_notify_envelope(keys.clone(), forged)),
        ErrorCode::BadSignature
    );
    // no envelope at all
    assert_eq!(
        code_of(open_notify_envelope(keys, vec![1, 2, 3])),
        ErrorCode::BadFormat
    );
}
