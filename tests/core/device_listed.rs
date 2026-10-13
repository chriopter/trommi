//! A group started from the hub's list (9.0.6): a human device that comes late reads what is open and what is
//! new, never the group's past. Its chains begin at the first envelope served after the list, the open objects
//! are taken from their signed headers, a `heads` of the person's own devices bounds where a chain may begin,
//! and an agent keeps its full check.

use trommi_core::chain::Head;
use trommi_core::crypto::SecretBytes;
use trommi_core::device::{Draft, EnvelopeOutcome, Sealed};
use trommi_core::envelope::{ObjectState, Urgency};
use trommi_core::ids::{GroupId, SessionId};
use trommi_core::Error;
use trommi_tests::hub::content::StoredEnvelope;
use trommi_tests::hub::Hub;
use trommi_tests::{
    add_human, add_to_session, enrol, found_main, found_room, json, learn, new_device_on,
    publish_some, reopen, sync_all, take_envelope, write, MemoryStorage, TestDevice,
};

const ZERO_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn card(title: &str, previous: &str) -> SecretBytes {
    json(&format!(
        r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"{previous}"}}"#
    ))
}

struct World {
    hub: Hub,
    session: SessionId,
    group: GroupId,
    human: TestDevice,
    agent: TestDevice,
    /// The late human device and its store.
    late: TestDevice,
    late_store: MemoryStorage,
    /// A card with two versions, open; a card with one version, open.
    revised: Sealed,
    plain: Sealed,
    /// The change number of the hub's list the late device starts from.
    at: u64,
}

impl World {
    fn version(&mut self, of: &Sealed, previous: &Sealed, title: &str) -> Sealed {
        let sealed = write(
            &mut self.hub,
            &mut self.agent,
            &Draft::CardVersion {
                session: self.session,
                object_id: of.object_id.unwrap(),
                closed: false,
                urgency: Urgency::Normal,
                push: false,
                payload: card(title, &previous.envelope_hash.to_base64url()),
            },
        );
        sync_all(&self.hub, &mut self.agent);
        sealed
    }

    fn stored(&self, sealed: &Sealed) -> StoredEnvelope {
        let sender = self.agent.id();
        self.hub
            .chain_of(&self.group, &sender, 0)
            .into_iter()
            .chain(self.hub.chain_of(&self.group, &self.human.id(), 0))
            .find(|stored| stored.hash == sealed.envelope_hash)
            .expect("the hub holds the envelope")
    }
}

/// A room with a human device and an agent; the agent wrote cards in the main session, the human device read
/// them and wrote its `heads`. Then a second human device comes, joins the room and the session, and reads
/// the log up to now: of the session's envelopes it holds none.
fn world() -> World {
    let store = MemoryStorage::new();
    let mut human = new_device_on(store.handle());
    let mut agent = new_device_on(MemoryStorage::new());
    let (mut hub, _room) = found_room(&mut human);
    publish_some(&mut hub, &mut agent, 4);
    enrol(&mut hub, &mut human, &mut agent);
    let group = found_main(&mut hub, &mut human, &agent.id());
    let session = group.session_id().unwrap();
    sync_all(&hub, &mut human);
    sync_all(&hub, &mut agent);
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
    let revised = first(&mut hub, &mut agent, "revised");
    let plain = first(&mut hub, &mut agent, "plain");
    let late_store = MemoryStorage::new();
    let late = new_device_on(late_store.handle());
    let mut w = World {
        hub,
        session,
        group,
        human,
        agent,
        late,
        late_store,
        revised: revised.clone(),
        plain,
        at: 0,
    };
    w.revised = w.version(&revised, &revised, "revised again");
    sync_all(&w.hub, &mut w.human);
    // The human device's `heads` of the session: the agent's chain as it read it.
    let value = w
        .human
        .heads_due(&w.group, trommi_tests::now())
        .unwrap()
        .unwrap();
    write(
        &mut w.hub,
        &mut w.human,
        &Draft::Register {
            group: w.group,
            name: "heads".into(),
            value: Some(SecretBytes::new(value)),
        },
    );
    sync_all(&w.hub, &mut w.human);
    publish_some(&mut w.hub, &mut w.late, 4);
    add_human(&mut w.hub, &mut w.human, &mut w.late);
    sync_all(&w.hub, &mut w.human);
    add_to_session(&mut w.hub, &mut w.human, &mut w.late, &w.group);
    sync_all(&w.hub, &mut w.human);
    sync_all(&w.hub, &mut w.agent);
    sync_all(&w.hub, &mut w.late);
    // It learns the session's past epochs from the hub (4.4): who wrote in them, not what.
    let room = GroupId::room(w.group.room_id());
    learn(&w.hub, &mut w.late, &room).expect("the room's past is learned");
    learn(&w.hub, &mut w.late, &w.group).expect("the past is learned");
    w.at = w.hub.change();
    w
}

#[test]
fn a_late_device_starts_from_the_list_and_reads_only_what_is_new() {
    let mut w = world();
    let group = w.group;
    assert_eq!(
        w.late.chain_head(&group, &w.agent.id()).unwrap(),
        Head::START
    );
    w.late.start_from_list(&group, w.at).unwrap();
    // Again: nothing changes.
    w.late.start_from_list(&group, w.at).unwrap();

    // The list's newest version of the revised card is taken from its header.
    let listed = w.stored(&w.revised.clone());
    let object_id = w.revised.object_id.unwrap();
    let taken = w
        .late
        .adopt_listed(&group, &object_id, &listed.bytes, listed.change)
        .unwrap();
    assert!(matches!(
        taken.outcome,
        EnvelopeOutcome::Applied | EnvelopeOutcome::Chained
    ));
    let object = w.late.object(&group, &object_id).unwrap().unwrap();
    assert_eq!(object.state, ObjectState::Open);
    assert_eq!(object.current, w.revised.envelope_hash);
    assert_eq!(object.owner, w.agent.id());
    // The same envelope again leaves the object as it stands.
    let again = w
        .late
        .adopt_listed(&group, &object_id, &listed.bytes, listed.change)
        .unwrap();
    assert!(again.object_after.is_none());
    assert_eq!(again.outcome, EnvelopeOutcome::Provisional);
    // An envelope of another object under this id is bad-format.
    let plain = w.stored(&w.plain.clone());
    assert_eq!(
        w.late
            .adopt_listed(&group, &object_id, &plain.bytes, plain.change)
            .unwrap_err(),
        Error::BadFormat
    );

    // What is new: the agent writes the next version of the revised card, the human answers the plain card
    // (which the list left out: it is taken from the answer's header).
    let revised = w.revised.clone();
    let third = w.version(&revised, &revised, "third");
    sync_all(&w.hub, &mut w.human);
    write(
        &mut w.hub,
        &mut w.human,
        &Draft::Answer {
            session: w.session,
            object_id: w.plain.object_id.unwrap(),
            choices: vec!["yes".into()],
            closes: false,
            payload: json(r#"{"answer_action":"answer","choices":["yes"]}"#),
        },
    );
    let received = sync_all(&w.hub, &mut w.late);
    assert_eq!(received.len(), 2, "{received:?}");
    for r in &received {
        assert!(
            matches!(
                r.outcome,
                EnvelopeOutcome::Applied | EnvelopeOutcome::Chained
            ),
            "{r:?}"
        );
        assert!(r.code.is_none() || r.code == Some(Error::NoKey), "{r:?}");
    }
    let object = w.late.object(&group, &object_id).unwrap().unwrap();
    assert_eq!(object.current, third.envelope_hash);
    let plain = w
        .late
        .object(&group, &w.plain.object_id.unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(plain.state, ObjectState::Answered);
    assert_eq!(plain.owner, w.agent.id());
    assert_eq!(plain.current, w.plain.envelope_hash);
    // The agent's chain began just before the version: the part before is not read and never will be.
    let began = w.late.chain_head(&group, &w.agent.id()).unwrap();
    assert_eq!(began.hash, third.envelope_hash);
    assert!(w.late.findings().unwrap().is_empty());

    // Everything stands after the store is opened again.
    let late = reopen(w.late_store.handle()).expect("the store opens");
    assert_eq!(
        late.object(&group, &object_id).unwrap().unwrap().current,
        third.envelope_hash
    );
}

#[test]
fn a_chain_begun_below_a_signed_head_is_withheld() {
    let mut w = world();
    let group = w.group;
    // The late device read the human device's `heads` out of order (the hub's list hands it).
    let heads = w
        .hub
        .chain_of(&group, &w.human.id(), 0)
        .into_iter()
        .last()
        .expect("the heads envelope");
    let shown = w
        .late
        .receive_envelope(&heads.bytes, heads.change, false, None, trommi_tests::now())
        .unwrap();
    assert_eq!(shown.code, None, "{shown:?}");
    w.late.start_from_list(&group, w.at).unwrap();
    // The hub serves the agent's second envelope (the card's second version, which the heads cover) as if it
    // were new after the list.
    let mut old = w.stored(&w.revised.clone());
    old.change = w.hub.change() + 1;
    let refused = take_envelope(&mut w.late, &old);
    assert_eq!(refused.outcome, EnvelopeOutcome::Refused);
    assert_eq!(refused.code, Some(Error::Withheld));
    assert_eq!(
        w.late.chain_head(&group, &w.agent.id()).unwrap(),
        Head::START
    );
    let findings = w.late.findings().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].sender, w.agent.id());
    assert_eq!(findings[0].code, Error::Withheld);

    // A version after what the heads name starts the chain.
    let revised = w.revised.clone();
    let third = w.version(&revised, &revised, "third");
    // (the made-up change number above took the hub's next one: the version comes under the one after)
    let mut next = w.stored(&third);
    next.change = w.late.cursor() + 1;
    let received = take_envelope(&mut w.late, &next);
    assert_ne!(received.outcome, EnvelopeOutcome::Refused, "{received:?}");
    assert_eq!(
        w.late.chain_head(&group, &w.agent.id()).unwrap().hash,
        third.envelope_hash
    );
}

#[test]
fn an_envelope_of_the_unread_prefix_is_never_read_in_order() {
    let mut w = world();
    let group = w.group;
    w.late.start_from_list(&group, w.at).unwrap();
    let revised = w.revised.clone();
    w.version(&revised, &revised, "third");
    sync_all(&w.hub, &mut w.late);
    // The hub serves the card's first version again, above the cursor: it lies before where the chain began.
    let first = w
        .hub
        .chain_of(&group, &w.agent.id(), 0)
        .into_iter()
        .next()
        .expect("the first envelope");
    let mut again = first.clone();
    again.change = w.hub.change() + 1;
    let refused = take_envelope(&mut w.late, &again);
    assert_eq!(refused.outcome, EnvelopeOutcome::Refused);
    assert_eq!(refused.code, Some(Error::Replay));
}

#[test]
fn an_agent_keeps_its_full_check() {
    let mut w = world();
    let group = w.group;
    assert_eq!(
        w.agent.start_from_list(&group, w.at).unwrap_err(),
        Error::Forbidden
    );
    // A human device that read the group's chains does not start it from the list.
    assert_eq!(
        w.human.start_from_list(&group, w.at).unwrap_err(),
        Error::Forbidden
    );
    // Without a start the list's versions are not taken.
    let listed = w.stored(&w.revised.clone());
    assert_eq!(
        w.late
            .adopt_listed(
                &group,
                &w.revised.object_id.unwrap(),
                &listed.bytes,
                listed.change
            )
            .unwrap_err(),
        Error::Forbidden
    );
    // And a chain served from its middle stays a gap.
    let revised = w.revised.clone();
    w.version(&revised, &revised, "third");
    let received = sync_all(&w.hub, &mut w.late);
    assert_eq!(received.len(), 1);
    assert_eq!(received[0].code, Some(Error::Gap));
}
