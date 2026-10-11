//! Where one device could end up without the key of an epoch another device wrote in (6.2, 7.1, 8.4, 4.6): a
//! human device that comes later must read what an agent wrote before it came — above all the agent's own name
//! (`device/<id>`) and `profile` in its session, which the apps show the session by. Each test plays one path
//! by which a device joins or comes back, and asks whether it reads the agent's registers.

use trommi_core::device::Draft;
use trommi_core::device::{EnvelopeOutcome, Processed, Received, ReceivedEnvelope};
use trommi_core::ids::GroupId;
use trommi_core::Error;
use trommi_tests::hub::Hub;
use trommi_tests::{
    add_human, add_to_session, enrol, found_main, found_room, join_room, join_session, json, learn,
    new_device, now, post_ok, publish_some, settle, settle_joining, sync_all, test_keys, write,
    TestDevice,
};

/// A room with one human device `a` and an agent `agent` in the main session `main`; the agent has
/// introduced itself (`device/<id>`) in the session's first epoch.
struct Room {
    hub: Hub,
    a: TestDevice,
    agent: TestDevice,
    room: GroupId,
    main: GroupId,
}

fn device_name(agent: &TestDevice) -> String {
    format!("device/{}", agent.id().to_base64url())
}

fn introduce(hub: &mut Hub, agent: &mut TestDevice, main: &GroupId, name: &str) {
    sync_all(hub, agent);
    let draft = Draft::Register {
        group: *main,
        name: device_name(agent),
        value: Some(json(&format!(
            r#"{{"device_name":"{name}","platform":"linux"}}"#
        ))),
    };
    write(hub, agent, &draft);
}

fn profile(hub: &mut Hub, agent: &mut TestDevice, main: &GroupId, name: &str) {
    sync_all(hub, agent);
    let draft = Draft::Register {
        group: *main,
        name: "profile".into(),
        value: Some(json(&format!(
            r#"{{"agent_name":"{name}","model":"m","task":"t"}}"#
        ))),
    };
    write(hub, agent, &draft);
}

fn room() -> Room {
    let (mut a, mut agent) = (new_device(), new_device());
    let (mut hub, room) = found_room(&mut a);
    publish_some(&mut hub, &mut a, 8);
    enrol(&mut hub, &mut a, &mut agent);
    publish_some(&mut hub, &mut agent, 1);
    let main = found_main(&mut hub, &mut a, &agent.id());
    settle(&hub, &mut agent);
    introduce(&mut hub, &mut agent, &main, "build-bot");
    sync_all(&hub, &mut a);
    Room {
        hub,
        a,
        agent,
        room,
        main,
    }
}

/// `a` commits an update in `group`, everyone else follows.
fn update(r: &mut Room, group: &GroupId) {
    sync_all(&r.hub, &mut r.a);
    r.a.update(group, true, now()).unwrap().unwrap();
    post_ok(&mut r.hub, &mut r.a);
    sync_all(&r.hub, &mut r.agent);
}

/// What a client does once it holds a group's past and whatever keys came: every envelope of `group` in the
/// hub's order, then each again out of order for its body (a device keeps no ciphertext, 9.0.6).
fn read_group(hub: &Hub, device: &mut TestDevice, group: &GroupId) -> Vec<ReceivedEnvelope> {
    let served: Vec<_> = hub
        .content
        .envelopes
        .iter()
        .filter(|stored| stored.header.group == *group && !stored.cut)
        .cloned()
        .collect();
    for stored in &served {
        device
            .receive_envelope(&stored.bytes, stored.change, true, None, now())
            .unwrap();
    }
    served
        .iter()
        .map(|stored| {
            device
                .receive_envelope(&stored.bytes, stored.change, false, None, now())
                .unwrap()
        })
        .collect()
}

/// The agent's name and profile as `device` reads them in `main`; none where the body never opened.
fn reads(
    device: &TestDevice,
    agent: &TestDevice,
    main: &GroupId,
) -> (Option<String>, Option<String>) {
    let text = |name: &str| {
        device
            .register(main, name)
            .unwrap()
            .map(|value| String::from_utf8(value.expose().to_vec()).unwrap())
    };
    (text(&device_name(agent)), text("profile"))
}

fn no_key_left(received: &[ReceivedEnvelope]) -> usize {
    received
        .iter()
        .filter(|got| got.code == Some(Error::NoKey))
        .count()
}

/// A human device added by link: the inviter's steps (12.1.5) are the room Add, the room handover, then an
/// Add into every live session group. Here the session group moved on between the handover and the Add
/// (another Commit there: an update, a takeover, a second human device's Add), and the agent wrote its
/// profile in that epoch. Before the fix the newcomer never got that epoch's key.
#[test]
fn a_device_added_by_link_reads_what_the_agent_wrote_between_the_handover_and_its_session_add() {
    let mut r = room();
    let mut phone = new_device();
    add_human(&mut r.hub, &mut r.a, &mut phone);
    publish_some(&mut r.hub, &mut phone, 4);
    // The room handover, as the invite's step sends it: keys of every group up to now.
    r.a.send_handover(&r.room, &phone.id()).unwrap();
    post_ok(&mut r.hub, &mut r.a);
    // The session moves on before the phone is added to it, and the agent writes in that epoch.
    let main = r.main;
    update(&mut r, &main);
    profile(&mut r.hub, &mut r.agent, &main, "Builder");
    sync_all(&r.hub, &mut r.a);
    add_to_session(&mut r.hub, &mut r.a, &mut phone, &main);
    sync_all(&r.hub, &mut r.a);

    let joined = settle_joining(&r.hub, &mut phone);
    assert_eq!(joined.len(), 1);
    learn(&r.hub, &mut phone, &r.room).unwrap();
    learn(&r.hub, &mut phone, &main).unwrap();
    sync_all(&r.hub, &mut phone);
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0, "every envelope of the session opens");
    let (name, profile) = reads(&phone, &r.agent, &main);
    assert!(name.unwrap().contains("build-bot"));
    assert!(profile.unwrap().contains("Builder"));
}

/// A human device signs in with the code (8.4) and joins the room group; its join of the session did not
/// happen (the hub did not answer, the session was refused): another human device adds it (5.2.7). A device
/// that joined with the code forgot the code: it cannot open the session's SealedKeys any more. Before the fix
/// it never read anything the agent wrote before that Add — the session showed the raw id.
#[test]
fn a_device_that_signed_in_and_was_added_to_a_session_later_reads_the_agents_name() {
    let mut r = room();
    profile(&mut r.hub, &mut r.agent, &r.main.clone(), "Builder");
    let mut phone = new_device();
    let keys = test_keys();
    join_room(&r.hub, &mut phone, &keys).unwrap();
    post_ok(&mut r.hub, &mut phone);
    publish_some(&mut r.hub, &mut phone, 4);
    sync_all(&r.hub, &mut phone);
    // The other human device sees a live session without the phone and adds it.
    sync_all(&r.hub, &mut r.a);
    let main = r.main;
    add_to_session(&mut r.hub, &mut r.a, &mut phone, &main);
    sync_all(&r.hub, &mut r.a);

    let joined = settle_joining(&r.hub, &mut phone);
    assert_eq!(joined.len(), 1);
    learn(&r.hub, &mut phone, &main).unwrap();
    sync_all(&r.hub, &mut phone);
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0, "every envelope of the session opens");
    let (name, profile) = reads(&phone, &r.agent, &main);
    assert!(name.unwrap().contains("build-bot"));
    assert!(profile.unwrap().contains("Builder"));
}

/// The room handover of an invite meets `wrong-epoch`: another human device's Commit ended its epoch before
/// it was posted (7.0: process the log, encrypt again). Before the fix the message was dropped, the invite
/// counted it as handed, and the newcomer never got a key older than its join.
#[test]
fn a_handover_refused_with_wrong_epoch_is_sent_again() {
    let mut r = room();
    let mut b = new_device();
    add_human(&mut r.hub, &mut r.a, &mut b);
    r.a.send_handover(&r.room, &b.id()).unwrap();
    post_ok(&mut r.hub, &mut r.a);
    publish_some(&mut r.hub, &mut b, 4);
    sync_all(&r.hub, &mut b);
    sync_all(&r.hub, &mut r.a);

    let mut phone = new_device();
    add_human(&mut r.hub, &mut r.a, &mut phone);
    sync_all(&r.hub, &mut b);
    r.a.send_handover(&r.room, &phone.id()).unwrap();
    // b's Commit takes the room's next epoch before the handover is posted.
    b.update(&r.room, true, now()).unwrap().unwrap();
    post_ok(&mut r.hub, &mut b);
    let entry = r.a.outbox().remove(0);
    assert_eq!(r.hub.post(&r.a.id(), &entry), Err(Error::WrongEpoch));
    // 7.0: the client processes the log and reports the refusal; the core makes the message again in the
    // epoch the group has now, and the client posts it.
    settle(&r.hub, &mut r.a);
    r.a.outbox_refused(entry.id, &Error::WrongEpoch).unwrap();
    let again = r.a.outbox();
    assert_eq!(again.len(), 1);
    assert_ne!(again[0].parts, entry.parts);
    post_ok(&mut r.hub, &mut r.a);
    // Taken: nothing is owed any more, a restart sends nothing again.
    assert!(r.a.outbox().is_empty());

    let processed = settle(&r.hub, &mut phone);
    let keys: usize = processed
        .iter()
        .map(|done| match done {
            Processed::Message(Received::Keys { taken, .. }) => *taken,
            _ => 0,
        })
        .sum();
    assert!(keys > 0, "the old keys reached the newcomer");
    let room_group = r.room;
    let first = phone.group(&room_group).unwrap().own_from;
    for epoch in 0..first {
        assert!(
            phone.content_key(&room_group, epoch).is_ok(),
            "room epoch {epoch}"
        );
    }
}

/// Control: a device that was a leaf of the session when the agent introduced itself, and was offline while
/// the session went through many epochs, reads the name when it comes back: it derives every key itself.
#[test]
fn a_device_offline_for_many_epochs_reads_the_agents_name() {
    let mut r = room();
    let mut phone = new_device();
    add_human(&mut r.hub, &mut r.a, &mut phone);
    publish_some(&mut r.hub, &mut phone, 4);
    let main = r.main;
    add_to_session(&mut r.hub, &mut r.a, &mut phone, &main);
    settle_joining(&r.hub, &mut phone);
    introduce(&mut r.hub, &mut r.agent, &main, "build-bot-2");
    for _ in 0..12 {
        update(&mut r, &main);
        let room = r.room;
        update(&mut r, &room);
    }
    profile(&mut r.hub, &mut r.agent, &main, "Builder");
    sync_all(&r.hub, &mut phone);
    // The agent's chain began before the phone came into the session: it is read from number 1 once the
    // session's past is learned (9.0.6), as the apps do.
    learn(&r.hub, &mut phone, &r.room).unwrap();
    learn(&r.hub, &mut phone, &main).unwrap();
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    let (name, profile) = reads(&phone, &r.agent, &main);
    assert!(name.unwrap().contains("build-bot-2"));
    assert!(profile.unwrap().contains("Builder"));
}

/// Control: the phone is reinstalled — a new device that signs in with the code (8.4) into the room and the
/// session — and reads what the agent wrote long before, from the SealedKeys.
#[test]
fn a_reinstalled_device_that_signs_in_reads_the_agents_name() {
    let mut r = room();
    let main = r.main;
    for _ in 0..3 {
        update(&mut r, &main);
    }
    profile(&mut r.hub, &mut r.agent, &main, "Builder");
    let mut phone = new_device();
    let keys = test_keys();
    join_room(&r.hub, &mut phone, &keys).unwrap();
    post_ok(&mut r.hub, &mut phone);
    join_session(&r.hub, &mut phone, &keys, &main).unwrap();
    post_ok(&mut r.hub, &mut phone);
    sync_all(&r.hub, &mut phone);
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    assert!(got
        .iter()
        .all(|got| got.outcome == EnvelopeOutcome::Applied));
    let (name, profile) = reads(&phone, &r.agent, &main);
    assert!(name.unwrap().contains("build-bot"));
    assert!(profile.unwrap().contains("Builder"));
}

/// The same across a restart: the session handover owed by an Add is stored with the merge, and a
/// `wrong-epoch` after the device was opened again still makes it again.
#[test]
fn an_owed_session_handover_survives_a_restart() {
    use trommi_tests::{new_device_on, reopen, MemoryStorage};
    let store = MemoryStorage::new();
    let handle = store.handle();
    let mut a = new_device_on(store);
    let mut agent = new_device();
    let (mut hub, room_group) = found_room(&mut a);
    publish_some(&mut hub, &mut a, 8);
    enrol(&mut hub, &mut a, &mut agent);
    publish_some(&mut hub, &mut agent, 1);
    let main = found_main(&mut hub, &mut a, &agent.id());
    settle(&hub, &mut agent);
    introduce(&mut hub, &mut agent, &main, "build-bot");
    sync_all(&hub, &mut a);
    let mut b = new_device();
    add_human(&mut hub, &mut a, &mut b);
    sync_all(&hub, &mut a);
    // A second human device in the session, whose Commit will end the epoch the handover is made in.
    add_to_session(&mut hub, &mut a, &mut b, &main);
    sync_all(&hub, &mut a);
    settle_joining(&hub, &mut b);
    // The phone's Add is posted alone; the device merges it and stops before it posts the handover.
    let mut phone = new_device();
    add_human(&mut hub, &mut a, &mut phone);
    sync_all(&hub, &mut a);
    let package = phone.key_package(now()).unwrap();
    a.add_to_session(&main, &phone.id(), &package, now())
        .unwrap();
    let entry = a.outbox().remove(0);
    let accepted = hub.post(&a.id(), &entry).unwrap();
    a.outbox_accepted(entry.id, accepted).unwrap();
    trommi_tests::process_up_to(&hub, &mut a, accepted.change.unwrap());
    let owed = a.outbox();
    assert_eq!(owed.len(), 1, "the session handover waits in the outbox");
    // The device ends; b commits in the session meanwhile.
    drop(a);
    let mut a = reopen(handle.reopened()).unwrap();
    sync_all(&hub, &mut b);
    b.update(&main, true, now()).unwrap().unwrap();
    post_ok(&mut hub, &mut b);
    let entry = a.outbox().remove(0);
    assert_eq!(hub.post(&a.id(), &entry), Err(Error::WrongEpoch));
    settle(&hub, &mut a);
    a.outbox_refused(entry.id, &Error::WrongEpoch).unwrap();
    post_ok(&mut hub, &mut a);
    assert!(a.outbox().is_empty());

    settle_joining(&hub, &mut phone);
    learn(&hub, &mut phone, &room_group).unwrap();
    learn(&hub, &mut phone, &main).unwrap();
    sync_all(&hub, &mut phone);
    let got = read_group(&hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    assert!(reads(&phone, &agent, &main)
        .0
        .unwrap()
        .contains("build-bot"));
}

/// The handover owed by an Add cannot go while its session is stale (5.2.8: the room revoked the agent before
/// the message was posted). It waits, and goes out after the Commit that cleans the session.
#[test]
fn a_handover_owed_to_a_stale_session_goes_out_after_it_is_cleaned() {
    let mut r = room();
    let mut phone = new_device();
    add_human(&mut r.hub, &mut r.a, &mut phone);
    r.a.send_handover(&r.room, &phone.id()).unwrap();
    post_ok(&mut r.hub, &mut r.a);
    publish_some(&mut r.hub, &mut phone, 4);
    let main = r.main;
    let package = phone.key_package(now()).unwrap();
    r.a.add_to_session(&main, &phone.id(), &package, now())
        .unwrap();
    let add = r.a.outbox().remove(0);
    let accepted = r.hub.post(&r.a.id(), &add).unwrap();
    r.a.outbox_accepted(add.id, accepted).unwrap();
    trommi_tests::process_up_to(&r.hub, &mut r.a, accepted.change.unwrap());
    let owed = r.a.outbox().remove(0);
    // The room revokes the agent: the session is stale from that Commit on.
    r.a.remove_agents(&[r.agent.id()], now()).unwrap();
    let revoke =
        r.a.outbox()
            .into_iter()
            .find(|entry| entry.id != owed.id)
            .unwrap();
    let accepted = r.hub.post(&r.a.id(), &revoke).unwrap();
    r.a.outbox_accepted(revoke.id, accepted).unwrap();
    trommi_tests::process_up_to(&r.hub, &mut r.a, accepted.change.unwrap());
    let refused = r.hub.post(&r.a.id(), &owed).unwrap_err();
    assert!(
        matches!(refused, Error::StaleSession | Error::WrongEpoch),
        "{refused:?}"
    );
    r.a.outbox_refused(owed.id, &refused).unwrap();
    assert!(r.a.outbox().is_empty(), "nothing goes into a stale session");
    // The session is cleaned: the agent's leaf goes, with its Cut. The handover follows that Commit.
    let cut = r.a.cut_of(&main, &r.agent.id()).unwrap();
    r.a.clean_session(&main, &[cut], None, now()).unwrap();
    post_ok(&mut r.hub, &mut r.a);
    assert!(r.a.outbox().is_empty());

    settle_joining(&r.hub, &mut phone);
    learn(&r.hub, &mut phone, &r.room).unwrap();
    learn(&r.hub, &mut phone, &main).unwrap();
    sync_all(&r.hub, &mut phone);
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    assert!(reads(&phone, &r.agent, &main)
        .0
        .unwrap()
        .contains("build-bot"));
}

/// The same, when another human device cleans the session and this one processed that Commit before it
/// reported the hub's `stale-session`: the handover goes out at once, it waits for no further Commit.
#[test]
fn a_handover_refused_as_stale_after_the_cleanup_was_seen_goes_out_at_once() {
    let mut r = room();
    let mut b = new_device();
    add_human(&mut r.hub, &mut r.a, &mut b);
    let main = r.main;
    add_to_session(&mut r.hub, &mut r.a, &mut b, &main);
    sync_all(&r.hub, &mut r.a);
    settle_joining(&r.hub, &mut b);
    let mut phone = new_device();
    add_human(&mut r.hub, &mut r.a, &mut phone);
    sync_all(&r.hub, &mut r.a);
    let package = phone.key_package(now()).unwrap();
    r.a.add_to_session(&main, &phone.id(), &package, now())
        .unwrap();
    let add = r.a.outbox().remove(0);
    let accepted = r.hub.post(&r.a.id(), &add).unwrap();
    r.a.outbox_accepted(add.id, accepted).unwrap();
    trommi_tests::process_up_to(&r.hub, &mut r.a, accepted.change.unwrap());
    let owed = r.a.outbox().remove(0);
    // b revokes the agent and cleans the session before a posts its handover. (It read the session from
    // number 1 first, as the apps do: its Cut keeps what the agent wrote.)
    sync_all(&r.hub, &mut b);
    learn(&r.hub, &mut b, &r.room).unwrap();
    learn(&r.hub, &mut b, &main).unwrap();
    read_group(&r.hub, &mut b, &main);
    b.remove_agents(&[r.agent.id()], now()).unwrap();
    post_ok(&mut r.hub, &mut b);
    let cut = b.cut_of(&main, &r.agent.id()).unwrap();
    b.clean_session(&main, &[cut], None, now()).unwrap();
    post_ok(&mut r.hub, &mut b);
    let refused = r.hub.post(&r.a.id(), &owed).unwrap_err();
    // a processes the revocation and the cleanup, then reports the refusal.
    settle(&r.hub, &mut r.a);
    r.a.outbox_refused(owed.id, &refused).unwrap();
    assert_eq!(
        r.a.outbox().len(),
        1,
        "made again in the cleaned session's epoch"
    );
    post_ok(&mut r.hub, &mut r.a);

    settle_joining(&r.hub, &mut phone);
    learn(&r.hub, &mut phone, &r.room).unwrap();
    learn(&r.hub, &mut phone, &main).unwrap();
    sync_all(&r.hub, &mut phone);
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    assert!(reads(&phone, &r.agent, &main)
        .0
        .unwrap()
        .contains("build-bot"));
}

// ---- 7.1.1: a device that missed its handover asks for the keys, and another human device answers

/// The hub sweeps application messages after 30 days (`hub-api.md` point 10): every key handover in the log
/// goes, Commits stay.
fn sweep_messages(hub: &mut Hub) {
    hub.log.retain(|item| item.commit);
}

/// The phone was added by link to the room and to the session, and the agent had written its name and profile
/// before; the phone stayed away until the hub had swept both handovers.
fn phone_that_missed_its_handovers(r: &mut Room) -> TestDevice {
    phone_that_missed_its_handovers_as(r, new_device())
}

fn phone_that_missed_its_handovers_as(r: &mut Room, mut phone: TestDevice) -> TestDevice {
    let main = r.main;
    profile(&mut r.hub, &mut r.agent, &main, "Builder");
    sync_all(&r.hub, &mut r.a);
    add_human(&mut r.hub, &mut r.a, &mut phone);
    r.a.send_handover(&r.room, &phone.id()).unwrap();
    post_ok(&mut r.hub, &mut r.a);
    publish_some(&mut r.hub, &mut phone, 4);
    add_to_session(&mut r.hub, &mut r.a, &mut phone, &main);
    sync_all(&r.hub, &mut r.a);
    sweep_messages(&mut r.hub);
    settle_joining(&r.hub, &mut phone);
    learn(&r.hub, &mut phone, &r.room).unwrap();
    learn(&r.hub, &mut phone, &main).unwrap();
    sync_all(&r.hub, &mut phone);
    phone
}

/// What `device` posts as its key requests at `at`: the groups named.
fn asks(device: &mut TestDevice, at: u64) -> Vec<GroupId> {
    device
        .key_requests(at)
        .unwrap()
        .into_iter()
        .map(|request| request.group)
        .collect()
}

#[test]
fn a_device_that_missed_its_handover_asks_and_another_human_device_answers() {
    use trommi_core::device::{KEY_REQUEST_FIRST_PAUSE_MS, KEY_REQUEST_GRACE_MS};
    let mut r = room();
    let main = r.main;
    let mut phone = phone_that_missed_its_handovers(&mut r);
    let t = now();
    let got = read_group(&r.hub, &mut phone, &main);
    assert!(
        no_key_left(&got) > 0,
        "the agent's name does not open without its key"
    );
    assert_eq!(reads(&phone, &r.agent, &main), (None, None));

    // Not at once: the handover behind an Add normally comes within seconds.
    assert!(asks(&mut phone, t).is_empty());
    let request = phone.key_requests(t + KEY_REQUEST_GRACE_MS).unwrap();
    assert_eq!(request.len(), 1);
    assert_eq!(request[0].group, main);
    assert!(request[0].epochs.contains(&1), "{:?}", request[0].epochs);
    // Asked: not again before the pause.
    assert!(asks(&mut phone, t + KEY_REQUEST_GRACE_MS + 1000).is_empty());
    // Not answered (the other devices were away): asked again after the pause.
    let again = t + KEY_REQUEST_GRACE_MS + KEY_REQUEST_FIRST_PAUSE_MS;
    assert_eq!(asks(&mut phone, again), vec![main]);

    // `a` answers in the room group, with the session's keys.
    sync_all(&r.hub, &mut r.a);
    let sent =
        r.a.answer_key_request(Some(&main), &phone.id(), now())
            .unwrap();
    assert!(!sent.is_empty());
    // The same request seen again (the stream, then the hub's list) is answered once.
    assert!(r
        .a
        .answer_key_request(Some(&main), &phone.id(), now())
        .unwrap()
        .is_empty());
    post_ok(&mut r.hub, &mut r.a);

    let processed = settle(&r.hub, &mut phone);
    let taken: usize = processed
        .iter()
        .map(|done| match done {
            Processed::Message(Received::Keys { taken, .. }) => *taken,
            _ => 0,
        })
        .sum();
    assert!(taken > 0, "the keys reached the phone");
    // The client reads back what it could not open.
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    let (name, profile) = reads(&phone, &r.agent, &main);
    assert!(name.unwrap().contains("build-bot"));
    assert!(profile.unwrap().contains("Builder"));
    // Nothing is missing any more: nothing is asked.
    assert!(asks(&mut phone, again + 10 * KEY_REQUEST_FIRST_PAUSE_MS).is_empty());
}

/// A request without a group is answered with every key of the room; the requester takes the session's keys
/// from the room group as from the handover of an invite.
#[test]
fn a_request_without_a_group_is_answered_with_every_key() {
    let mut r = room();
    let main = r.main;
    let mut phone = phone_that_missed_its_handovers(&mut r);
    sync_all(&r.hub, &mut r.a);
    assert!(!r
        .a
        .answer_key_request(None, &phone.id(), now())
        .unwrap()
        .is_empty());
    post_ok(&mut r.hub, &mut r.a);
    settle(&r.hub, &mut phone);
    let got = read_group(&r.hub, &mut phone, &main);
    assert_eq!(no_key_left(&got), 0);
    assert!(reads(&phone, &r.agent, &main)
        .0
        .unwrap()
        .contains("build-bot"));
    for epoch in 0..r.hub.epoch(&r.room).unwrap() {
        assert!(
            phone.content_key(&r.room, epoch).is_ok(),
            "room epoch {epoch}"
        );
    }
}

/// Who asks and who answers: human devices only. An agent device lacks the keys before its takeover when the
/// human chose "without history" (5.3.2): it is not answered, and it does not ask.
#[test]
fn only_human_devices_ask_and_only_human_devices_are_answered() {
    use trommi_core::device::KEY_REQUEST_GRACE_MS;
    let mut r = room();
    let main = r.main;
    let mut phone = phone_that_missed_its_handovers(&mut r);
    sync_all(&r.hub, &mut r.a);
    sync_all(&r.hub, &mut r.agent);
    // An agent device is not answered, and answers nobody.
    assert_eq!(
        r.a.answer_key_request(Some(&main), &r.agent.id(), now()),
        Err(Error::Forbidden)
    );
    assert_eq!(
        r.agent.answer_key_request(Some(&main), &phone.id(), now()),
        Err(Error::Forbidden)
    );
    // Its own request is no request.
    assert!(r
        .a
        .answer_key_request(None, &r.a.id(), now())
        .unwrap()
        .is_empty());
    // A device that holds none of the keys sends nothing.
    read_group(&r.hub, &mut phone, &main);
    let other = GroupId::room(trommi_core::ids::RoomId::new([7; 32]));
    assert_eq!(
        phone.answer_key_request(Some(&other), &r.a.id(), now()),
        Err(Error::NotFound)
    );
    // An agent device never asks.
    assert!(r
        .agent
        .key_requests(now() + KEY_REQUEST_GRACE_MS * 10)
        .unwrap()
        .is_empty());
}

/// What is wanted outlives a restart: the device that could not open the envelope and stopped before it asked
/// asks when it is opened again, without reading anything again.
#[test]
fn a_wanted_key_is_asked_for_after_a_restart() {
    use trommi_core::device::KEY_REQUEST_GRACE_MS;
    use trommi_tests::{new_device_on, reopen, MemoryStorage};
    let mut r = room();
    let main = r.main;
    let store = MemoryStorage::new();
    let handle = store.handle();
    let mut phone = phone_that_missed_its_handovers_as(&mut r, new_device_on(store));
    assert!(no_key_left(&read_group(&r.hub, &mut phone, &main)) > 0);
    drop(phone);
    let mut phone = reopen(handle.reopened()).unwrap();
    // (the grace begins again with the new run)
    let t = now();
    assert!(asks(&mut phone, t).is_empty());
    assert_eq!(asks(&mut phone, t + KEY_REQUEST_GRACE_MS), vec![main]);
    // Answered, the keys come; what is wanted is forgotten, also across the next restart.
    sync_all(&r.hub, &mut r.a);
    r.a.answer_key_request(Some(&main), &phone.id(), now())
        .unwrap();
    post_ok(&mut r.hub, &mut r.a);
    settle(&r.hub, &mut phone);
    assert!(asks(&mut phone, t + 100 * KEY_REQUEST_GRACE_MS).is_empty());
    assert_eq!(no_key_left(&read_group(&r.hub, &mut phone, &main)), 0);
}
