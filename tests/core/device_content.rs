//! Stored content through the device (section 9): what a device writes reaches the hub from its outbox, comes
//! back in the hub's order, and leaves the same chains, objects and registers on every device.

use trommi_core::chain::HeadStanding;
use trommi_core::crypto::SecretBytes;
use trommi_core::device::{Draft, EnvelopeOutcome, ReceivedEnvelope, HEADS_EVERY_MS};
use trommi_core::envelope::{Bind, ObjectState, Urgency, Verdict};
use trommi_core::ids::{DeviceId, GroupId, SessionId};
use trommi_core::objects::{AnswerAction, Command, Decision, Refusal};
use trommi_core::Error;
use trommi_tests::hub::Hub;
use trommi_tests::{
    add_human, enrol, found_main, found_room, json, new_device, now, publish_some, sync_all, write,
    TestDevice,
};

/// A room with one human device and an agent in its main session.
struct World {
    hub: Hub,
    room: GroupId,
    session: SessionId,
    group: GroupId,
    human: TestDevice,
    agent: TestDevice,
}

fn world() -> World {
    world_with(None)
}

/// The same room with a second human device, which is in the session from its founding.
fn world_of_two() -> (World, TestDevice) {
    let mut other = new_device();
    let world = world_with(Some(&mut other));
    (world, other)
}

fn world_with(other: Option<&mut TestDevice>) -> World {
    let mut human = new_device();
    let mut agent = new_device();
    let (mut hub, room) = found_room(&mut human);
    publish_some(&mut hub, &mut agent, 4);
    if let Some(other) = other {
        add_human(&mut hub, &mut human, other);
        publish_some(&mut hub, other, 4);
        enrol(&mut hub, &mut human, &mut agent);
        let _ = found_main(&mut hub, &mut human, &agent.id());
        sync_all(&hub, other);
    } else {
        enrol(&mut hub, &mut human, &mut agent);
        let _ = found_main(&mut hub, &mut human, &agent.id());
    }
    sync_all(&hub, &mut human);
    sync_all(&hub, &mut agent);
    let group = human
        .groups()
        .unwrap()
        .into_iter()
        .find(|summary| summary.session.is_some())
        .unwrap()
        .group;
    World {
        hub,
        room,
        session: group.session_id().unwrap(),
        group,
        human,
        agent,
    }
}

const ZERO_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

fn card(title: &str, previous: &str) -> SecretBytes {
    json(&format!(
        r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"{previous}"}}"#
    ))
}

/// Both devices take everything new; returns what the first got.
fn both(world: &mut World) -> (Vec<ReceivedEnvelope>, Vec<ReceivedEnvelope>) {
    (
        sync_all(&world.hub, &mut world.human),
        sync_all(&world.hub, &mut world.agent),
    )
}

#[test]
fn a_card_is_answered_taken_back_and_answered_again_and_each_passes_the_gate_once() {
    let mut w = world();
    let sealed = write(
        &mut w.hub,
        &mut w.agent,
        &Draft::CardFirst {
            session: w.session,
            urgency: Urgency::High,
            push: true,
            payload: card("Ship it?", ZERO_HASH),
        },
    );
    let card_id = sealed.object_id.unwrap();
    let (at_human, at_agent) = both(&mut w);
    assert_eq!(at_human.len(), 1);
    assert_eq!(at_human[0].outcome, EnvelopeOutcome::Applied);
    assert_eq!(at_human[0].envelope_hash, sealed.envelope_hash);
    assert!(at_human[0].header.push);
    assert_eq!(at_agent[0].outcome, EnvelopeOutcome::Applied);
    let seen = w.human.object(&w.group, &card_id).unwrap().unwrap();
    assert_eq!(seen.state, ObjectState::Open);
    assert_eq!(seen.owner, w.agent.id());
    assert_eq!(seen.current, sealed.envelope_hash);
    assert_eq!(
        w.human.object_owner(&w.group, &card_id).unwrap(),
        Some(w.agent.id())
    );

    // The human answers; the answer is addressed to the card's owner by the device.
    let answer = Draft::Answer {
        session: w.session,
        object_id: card_id,
        choices: vec!["yes".into()],
        closes: false,
        payload: json(r#"{"answer_action":"answer","choices":["yes"]}"#),
    };
    let answered = write(&mut w.hub, &mut w.human, &answer);
    let (_, at_agent) = both(&mut w);
    assert_eq!(at_agent.len(), 1);
    assert!(at_agent[0].command);
    assert_eq!(at_agent[0].header.recipient, w.agent.id());
    assert_eq!(
        at_agent[0].object_after.as_ref().unwrap().1.state,
        ObjectState::Answered
    );
    assert_eq!(
        w.agent.commands_pending().unwrap(),
        vec![answered.envelope_hash]
    );
    // The gate lets it through exactly once.
    let decision = w.agent.command(&answered.envelope_hash, now()).unwrap();
    assert_eq!(
        decision,
        Decision::Act(Command::Answer {
            action: AnswerAction::Answer,
            choices: vec!["yes".into()],
        })
    );
    assert!(w.agent.commands_pending().unwrap().is_empty());
    assert_eq!(
        w.agent.command(&answered.envelope_hash, now()).unwrap(),
        Decision::Uncertain
    );
    w.agent.command_finished(&answered.envelope_hash).unwrap();
    assert_eq!(
        w.agent.command(&answered.envelope_hash, now()).unwrap(),
        Decision::Done
    );
    // A human device has no gate.
    assert_eq!(
        w.human.command(&answered.envelope_hash, now()),
        Err(Error::Forbidden)
    );

    // Taken back: the card is open again, and the agent is told to stop.
    let back = write(
        &mut w.hub,
        &mut w.human,
        &Draft::TakeBack {
            session: w.session,
            object_id: card_id,
            payload: json("{}"),
        },
    );
    both(&mut w);
    assert_eq!(
        w.agent.command(&back.envelope_hash, now()).unwrap(),
        Decision::Act(Command::TakeBack)
    );
    w.agent.command_finished(&back.envelope_hash).unwrap();
    for device in [&w.human, &w.agent] {
        let card = device.object(&w.group, &card_id).unwrap().unwrap();
        assert_eq!(card.state, ObjectState::Open);
        assert_eq!(card.answer, None);
    }

    // Answered again, this time closing the card; a choice the card does not offer does not pass the gate.
    let wrong = write(
        &mut w.hub,
        &mut w.human,
        &Draft::Answer {
            session: w.session,
            object_id: card_id,
            choices: vec!["maybe".into()],
            closes: true,
            payload: json(r#"{"answer_action":"answer"}"#),
        },
    );
    both(&mut w);
    assert_eq!(
        w.agent.command(&wrong.envelope_hash, now()).unwrap(),
        Decision::Refused(Refusal::BadChoice)
    );
    for device in [&w.human, &w.agent] {
        let card = device.object(&w.group, &card_id).unwrap().unwrap();
        assert_eq!(card.state, ObjectState::Closed);
    }
    assert_eq!(
        w.hub.objects(&w.group).get(&card_id),
        w.human.object(&w.group, &card_id).unwrap().as_ref()
    );
    // A closed card takes no answer: the device refuses to write it.
    assert_eq!(
        w.human.seal(&answer, None, &[], now()).map(|_| ()),
        Err(Error::Forbidden)
    );
}

fn text(received: &ReceivedEnvelope) -> String {
    String::from_utf8(received.body.as_ref().unwrap().payload().to_vec()).unwrap()
}

#[test]
fn a_permission_request_takes_one_verdict_before_it_expires() {
    let mut w = world();
    let session = w.session;
    let request = move |expires_at| Draft::PermissionRequest {
        session,
        urgency: Urgency::Critical,
        expires_at,
        push: true,
        payload: json(r#"{"tool_name":"Bash","description":"run","input_preview":"ls"}"#),
    };
    let expires_at = now() + 60_000;
    let first = write(&mut w.hub, &mut w.agent, &request(expires_at));
    let request_id = first.object_id.unwrap();
    let (at_human, _) = both(&mut w);
    let Bind::Request(bind) = at_human[0].body.as_ref().unwrap().bind() else {
        panic!("a request binds its expiry");
    };
    assert_eq!((bind.request_id, bind.expires_at), (request_id, expires_at));

    let verdict = move |request_id, allow| Draft::Verdict {
        session,
        request_id,
        allow,
        payload: json("{}"),
    };
    let allowed = write(&mut w.hub, &mut w.human, &verdict(request_id, true));
    let (_, at_agent) = both(&mut w);
    let Bind::Verdict(bound) = at_agent[0].body.as_ref().unwrap().bind() else {
        panic!("a verdict binds the request");
    };
    assert_eq!(bound.request_hash, first.envelope_hash);
    assert_eq!(bound.expires_at, expires_at);
    assert_eq!(
        w.agent.command(&allowed.envelope_hash, now()).unwrap(),
        Decision::Act(Command::Verdict(Verdict::Allow))
    );
    w.agent.command_finished(&allowed.envelope_hash).unwrap();
    let closed = w.human.object(&w.group, &request_id).unwrap().unwrap();
    assert_eq!(closed.state, ObjectState::Closed);
    // A second verdict on it is not written.
    assert_eq!(
        w.human
            .seal(&verdict(request_id, false), None, &[], now())
            .map(|_| ()),
        Err(Error::Forbidden)
    );

    // A verdict that the agent reads after the request ran out lets nothing happen.
    let short = write(&mut w.hub, &mut w.agent, &request(now() + 5_000));
    both(&mut w);
    let late = write(
        &mut w.hub,
        &mut w.human,
        &verdict(short.object_id.unwrap(), true),
    );
    both(&mut w);
    assert_eq!(
        w.agent.command(&late.envelope_hash, now() + 6_000).unwrap(),
        Decision::Refused(Refusal::Expired)
    );
    // A verdict on a request this device never opened cannot be written.
    assert_eq!(
        w.human
            .seal(&verdict(first.object_id.unwrap(), true), None, &[], now())
            .map(|_| ()),
        Err(Error::Forbidden)
    );
}

#[test]
fn chat_runs_on_a_session_and_on_a_card_and_only_a_humans_reaches_the_gate() {
    let mut w = world();
    let said = write(
        &mut w.hub,
        &mut w.human,
        &Draft::SessionChat {
            session: w.session,
            payload: json(r#"{"content_type":"message","text":"go on"}"#),
        },
    );
    let (at_human, at_agent) = both(&mut w);
    assert_eq!(at_human[0].outcome, EnvelopeOutcome::Applied);
    assert!(!at_human[0].command);
    assert!(at_agent[0].command);
    assert_eq!(at_agent[0].header.recipient, w.agent.id());
    assert!(text(&at_agent[0]).contains("go on"));
    assert_eq!(
        w.agent.command(&said.envelope_hash, now()).unwrap(),
        Decision::Act(Command::Chat)
    );

    // The agent's answer is addressed to nobody and is no command for anyone.
    let reply = write(
        &mut w.hub,
        &mut w.agent,
        &Draft::SessionChat {
            session: w.session,
            payload: json(r#"{"content_type":"message","text":"done","terminal":"answer"}"#),
        },
    );
    let (at_human, at_agent) = both(&mut w);
    assert_eq!(at_human[0].header.recipient, DeviceId::ZERO);
    assert!(text(&at_human[0]).contains("done"));
    assert!(!at_agent[0].command);
    assert_eq!(
        w.agent.command(&reply.envelope_hash, now()),
        Err(Error::NotFound)
    );

    // A message the agent addresses to itself is not a human's: the gate refuses it.
    let own = w
        .agent
        .seal(
            &Draft::SessionChat {
                session: w.session,
                payload: json(r#"{"text":"rm -rf"}"#),
            },
            Some(&w.agent.id()),
            &[],
            now(),
        )
        .unwrap();
    trommi_tests::post_ok(&mut w.hub, &mut w.agent);
    let (_, at_agent) = both(&mut w);
    assert!(at_agent[0].command);
    assert_eq!(
        w.agent.command(&own.envelope_hash, now()).unwrap(),
        Decision::Refused(Refusal::NotHuman)
    );

    // A card's own Chat exists once the card does.
    let session = w.session;
    let chat = move |card| Draft::CardChat {
        session,
        card,
        payload: json(r#"{"text":"why?"}"#),
    };
    let made = write(
        &mut w.hub,
        &mut w.agent,
        &Draft::CardFirst {
            session: w.session,
            urgency: Urgency::Normal,
            push: false,
            payload: card("Why", ZERO_HASH),
        },
    );
    let unknown = trommi_core::ids::ObjectId::new([9; 16]);
    assert_eq!(
        w.human.seal(&chat(unknown), None, &[], now()).map(|_| ()),
        Err(Error::Forbidden)
    );
    both(&mut w);
    let asked = write(&mut w.hub, &mut w.human, &chat(made.object_id.unwrap()));
    both(&mut w);
    assert_eq!(
        w.agent.command(&asked.envelope_hash, now()).unwrap(),
        Decision::Act(Command::Chat)
    );
    // A human may not write a card, nor an agent an answer.
    let forged = Draft::CardFirst {
        session: w.session,
        urgency: Urgency::Normal,
        push: false,
        payload: card("Mine", ZERO_HASH),
    };
    assert_eq!(
        w.human.seal(&forged, None, &[], now()).map(|_| ()),
        Err(Error::Forbidden)
    );
}

#[test]
fn a_note_takes_versions_of_two_writers() {
    let (mut w, mut other) = world_of_two();
    let note = |previous: &str, lamport: u64| {
        json(&format!(
            r#"{{"text":"n","lamport":{lamport},"previous_version_hash":"{previous}"}}"#
        ))
    };
    let first = write(
        &mut w.hub,
        &mut w.human,
        &Draft::NoteFirst {
            payload: note(ZERO_HASH, 1),
        },
    );
    let id = first.object_id.unwrap();
    assert_eq!(first.group, w.room);
    sync_all(&w.hub, &mut w.human);
    sync_all(&w.hub, &mut other);
    // Both write a version on the first, neither knowing the other's.
    let follows = first.envelope_hash.to_base64url();
    let version = |lamport| Draft::NoteVersion {
        object_id: id,
        closed: false,
        payload: note(&follows, lamport),
    };
    let mine = write(&mut w.hub, &mut w.human, &version(2));
    let theirs = write(&mut w.hub, &mut other, &version(2));
    for device in [&mut w.human, &mut other] {
        let got = sync_all(&w.hub, device);
        assert_eq!(got.len(), 2);
        assert!(got.iter().all(|r| r.outcome == EnvelopeOutcome::Applied));
        // The hub keeps the newest by arrival, and so does every device.
        let note = device.object(&w.room, &id).unwrap().unwrap();
        assert_eq!(note.current, theirs.envelope_hash);
        assert_ne!(mine.envelope_hash, theirs.envelope_hash);
    }
    assert_eq!(
        w.hub.objects(&w.room).get(&id),
        other.object(&w.room, &id).unwrap().as_ref()
    );
    // An agent device is no leaf of the room group and writes no Note.
    assert_eq!(
        w.agent.seal(&version(3), None, &[], now()).map(|_| ()),
        Err(Error::NotMember)
    );
}

#[test]
fn registers_follow_the_lamport_rule_across_devices() {
    let (mut w, mut other) = world_of_two();
    let (room, group) = (w.room, w.group);
    let desk = move |name: &str| Draft::Register {
        group: room,
        name: "desk/AAAAAAAAAAAAAAAAAAAAAA".into(),
        value: Some(json(&format!(r#"{{"name":"{name}"}}"#))),
    };
    // Two devices write the same name without having seen each other: the same lamport, and the higher
    // sender wins on both.
    write(&mut w.hub, &mut w.human, &desk("mine"));
    write(&mut w.hub, &mut other, &desk("theirs"));
    let winner = if w.human.id() > other.id() {
        "mine"
    } else {
        "theirs"
    };
    let read = move |device: &TestDevice| {
        let value = device
            .register(&room, "desk/AAAAAAAAAAAAAAAAAAAAAA")
            .unwrap()
            .unwrap();
        String::from_utf8(value.expose().to_vec()).unwrap()
    };
    let got = sync_all(&w.hub, &mut w.human);
    assert_eq!(got.len(), 2);
    assert_eq!(
        got[0].register.as_ref().unwrap().name,
        "desk/AAAAAAAAAAAAAAAAAAAAAA"
    );
    assert!(got[0].register.as_ref().unwrap().current);
    assert_eq!(
        got[1].register.as_ref().unwrap().current,
        winner == "theirs"
    );
    sync_all(&w.hub, &mut other);
    assert!(read(&w.human).contains(winner));
    assert_eq!(read(&w.human), read(&other));
    // Whoever writes next has seen both: its lamport is one above, and it wins whatever its id.
    let loser = if winner == "mine" {
        &mut other
    } else {
        &mut w.human
    };
    write(&mut w.hub, loser, &desk("later"));
    sync_all(&w.hub, &mut w.human);
    sync_all(&w.hub, &mut other);
    assert!(read(&w.human).contains("later"));
    assert_eq!(read(&w.human), read(&other));
    // A deletion is a value too.
    write(
        &mut w.hub,
        &mut w.human,
        &Draft::Register {
            group: w.room,
            name: "desk/AAAAAAAAAAAAAAAAAAAAAA".into(),
            value: None,
        },
    );
    sync_all(&w.hub, &mut other);
    assert_eq!(
        other
            .register(&w.room, "desk/AAAAAAAAAAAAAAAAAAAAAA")
            .unwrap(),
        None
    );

    // Each name has its owners: the agent's registers in its session, the humans' in the room.
    let session_register = move |name: &str| Draft::Register {
        group,
        name: name.into(),
        value: Some(json(r#"{"label":"x"}"#)),
    };
    for name in ["profile", "status_line/1", "heard", "alert/abc"] {
        write(&mut w.hub, &mut w.agent, &session_register(name));
        assert_eq!(
            w.human
                .seal(&session_register(name), None, &[], now())
                .map(|_| ()),
            Err(Error::Forbidden)
        );
    }
    assert_eq!(
        w.agent
            .seal(&session_register("goals"), None, &[], now())
            .map(|_| ()),
        Err(Error::Forbidden)
    );
    assert_eq!(
        w.human
            .seal(&session_register("no such name"), None, &[], now())
            .map(|_| ()),
        Err(Error::Forbidden)
    );
    write(&mut w.hub, &mut w.human, &session_register("goals"));
    // What the agent is handed of the room group, of which it is no leaf, is not for it.
    for received in sync_all(&w.hub, &mut w.agent) {
        if received.header.group == w.group {
            assert_eq!(received.outcome, EnvelopeOutcome::Applied);
        } else {
            assert_eq!(received.code, Some(Error::GroupBehind));
        }
    }
    assert!(w.agent.register(&w.group, "goals").unwrap().is_some());
    sync_all(&w.hub, &mut w.human);
    assert!(w.human.register(&w.group, "profile").unwrap().is_some());
    assert!(w
        .human
        .register(&w.group, "status_line/1")
        .unwrap()
        .is_some());
}

#[test]
fn heads_are_due_when_a_head_changed_and_show_what_a_reader_lacks() {
    let (mut w, mut other) = world_of_two();
    let start = now();
    // Nothing accepted yet: nothing to say.
    assert_eq!(w.human.heads_due(&w.room, start).unwrap(), None);
    let note = Draft::NoteFirst {
        payload: json(&format!(
            r#"{{"text":"n","previous_version_hash":"{ZERO_HASH}"}}"#
        )),
    };
    let first = write(&mut w.hub, &mut w.human, &note);
    sync_all(&w.hub, &mut w.human);
    let value = w.human.heads_due(&w.room, start).unwrap().unwrap();
    let heads = Draft::Register {
        group: w.room,
        name: "heads".into(),
        value: Some(SecretBytes::new(value)),
    };
    write(&mut w.hub, &mut w.human, &heads);
    sync_all(&w.hub, &mut w.human);
    // The heads register itself moved the own chain, and that calls for no other one.
    let written = now();
    assert_eq!(w.human.heads_due(&w.room, written + 1).unwrap(), None);
    assert_eq!(
        w.human
            .heads_due(&w.room, written + HEADS_EVERY_MS)
            .unwrap(),
        None
    );

    // The other device is handed the heads but not the Note: it holds less than the head names.
    let served = w.hub.changes_after(other.cursor());
    for change in &served {
        if let trommi_tests::hub::content::Change::Envelope(stored) = change {
            if stored.hash != first.envelope_hash {
                let got = trommi_tests::take_envelope(&mut other, stored);
                assert_eq!(got.code, Some(Error::Gap));
            }
        }
    }
    // It reads the chain of the writer from its own head on, and compares.
    for stored in w.hub.chain_of(&w.room, &w.human.id(), 0) {
        other
            .receive_envelope(&stored.bytes, stored.change, true, None, now())
            .unwrap();
    }
    let standings = other.compare_heads(&w.room, &w.human.id()).unwrap();
    assert_eq!(standings, vec![(w.human.id(), HeadStanding::Held)]);
    assert_eq!(
        other.cut_of(&w.room, &w.human.id()).unwrap().seq,
        2,
        "the last envelope accepted"
    );
}

// ---- every outcome, a removed device, a hostile hub ----

use trommi_core::chain::Role;
use trommi_core::device::Confirmation;
use trommi_core::envelope::{self, Envelope, ObjectFields, ObjectType, Subject};
use trommi_core::ids::{BoardId, Hash32, ObjectId};
use trommi_core::objects::Objects;
use trommi_tests::forge::Forger;
use trommi_tests::forge_content::{posting, Claim, Pen};
use trommi_tests::hub::content::{Change, StoredEnvelope};
use trommi_tests::{add_forger, post_ok, take_envelope};

/// A room whose founder reads, with a second human leaf whose key the test holds.
struct Forged {
    hub: Hub,
    room: GroupId,
    reader: TestDevice,
    pen: Pen,
}

fn forged() -> Forged {
    let mut reader = new_device();
    let (mut hub, room) = found_room(&mut reader);
    let forger = Forger::new();
    add_forger(&mut hub, &mut reader, &forger);
    sync_all(&hub, &mut reader);
    Forged {
        hub,
        room,
        reader,
        pen: Pen::new(&forger.key),
    }
}

impl Forged {
    /// What the pen claims: a human leaf of the room group in the epoch the reader stands in.
    fn claim(&self) -> Claim {
        let epoch = self.reader.group(&self.room).unwrap().epoch;
        self.claim_in(epoch)
    }

    fn claim_in(&self, epoch: u64) -> Claim {
        Claim {
            group: self.room,
            epoch,
            role: Role::Human,
            seat: None,
            key: self.reader.content_key(&self.room, epoch).unwrap(),
        }
    }

    fn stroke(&mut self) -> Envelope {
        let claim = self.claim();
        self.pen.sign(&claim, &board_item(), &Objects::new(), now())
    }

    /// The hub takes the envelope; returns it as served.
    fn post(&mut self, envelope: &Envelope) -> StoredEnvelope {
        self.hub
            .post(&self.pen.id(), &posting(envelope))
            .expect("the hub takes it");
        self.hub.content.envelopes.last().unwrap().clone()
    }

    /// Hands the reader an envelope the hub never took, read back at the reader's cursor.
    fn slip(&mut self, envelope: &Envelope) -> ReceivedEnvelope {
        let change = self.reader.cursor();
        self.reader
            .receive_envelope(&envelope.encode().unwrap(), change, true, None, now())
            .unwrap()
    }
}

fn board_item() -> envelope::Draft {
    envelope::Draft::board_item(
        BoardId::ALL_DESKS,
        br#"{"content_type":"erase","shape_ids":["AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA/1/0"]}"#,
    )
}

fn note_draft() -> envelope::Draft {
    envelope::Draft::first_version(
        ObjectType::Note,
        Urgency::Normal,
        format!(r#"{{"text":"n","previous_version_hash":"{ZERO_HASH}"}}"#).as_bytes(),
    )
    .unwrap()
}

#[test]
fn every_refusal_of_checks_one_to_six_consumes_nothing() {
    let mut f = forged();
    let head = |f: &Forged| f.reader.chain_head(&f.room, &f.pen.id()).unwrap();
    // Check 1: not an envelope at all; another room.
    assert_eq!(
        f.reader.receive_envelope(&[], 0, true, None, now()),
        Err(Error::BadFormat)
    );
    let mut pen = f.pen.fork();
    let mut elsewhere = pen.sign(&f.claim(), &board_item(), &Objects::new(), now());
    elsewhere.header.group = GroupId::room(trommi_core::ids::RoomId::new([7; 32]));
    pen.resign(&mut elsewhere);
    assert_eq!(f.slip(&elsewhere).code, Some(Error::WrongRoom));
    // Check 2: an epoch the reader has not reached; with the cursor held back when it comes in order.
    let mut pen = f.pen.fork();
    let mut ahead = pen.sign(&f.claim(), &board_item(), &Objects::new(), now());
    ahead.header.epoch += 1;
    pen.resign(&mut ahead);
    assert_eq!(f.slip(&ahead).code, Some(Error::GroupBehind));
    let cursor = f.reader.cursor();
    let early = f
        .reader
        .receive_envelope(&ahead.encode().unwrap(), cursor + 5, true, None, now())
        .unwrap();
    assert_eq!(early.outcome, EnvelopeOutcome::Refused);
    assert_eq!(early.code, Some(Error::GroupBehind));
    assert_eq!(f.reader.cursor(), cursor, "the log is processed first");
    // Check 3: a signer that is no leaf.
    let stranger =
        trommi_core::crypto::SigningKey::generate(&mut trommi_core::crypto::SystemEntropy).unwrap();
    let outside = Pen::new(&stranger).sign(&f.claim(), &board_item(), &Objects::new(), now());
    assert_eq!(f.slip(&outside).code, Some(Error::NotMember));
    // Check 5: a signature that is not the sender's.
    let mut unsigned = f
        .pen
        .fork()
        .sign(&f.claim(), &board_item(), &Objects::new(), now());
    unsigned.signature[0] ^= 1;
    assert_eq!(f.slip(&unsigned).code, Some(Error::BadSignature));
    // Check 6: a number beyond the next, a wrong link, and after the first envelope a replay and a fork.
    let mut pen = f.pen.fork();
    let first = pen.sign(&f.claim(), &board_item(), &Objects::new(), now());
    let second = pen.sign(&f.claim(), &board_item(), &Objects::new(), now());
    assert_eq!(f.slip(&second).code, Some(Error::Gap));
    assert_eq!(head(&f).seq, 0, "nothing was consumed");
    assert_eq!(f.slip(&first).outcome, EnvelopeOutcome::Applied);
    assert_eq!(f.slip(&first).code, Some(Error::Replay));
    let other_first = f
        .pen
        .fork()
        .sign(&f.claim(), &board_item(), &Objects::new(), now());
    assert_eq!(f.slip(&other_first).code, Some(Error::Equivocation));
    let mut broken = second.clone();
    broken.header.prev = Hash32::new([3; 32]);
    pen.resign(&mut broken);
    assert_eq!(f.slip(&broken).code, Some(Error::ChainBreak));
    assert_eq!(head(&f).seq, 1);
    assert_eq!(f.slip(&second).outcome, EnvelopeOutcome::Applied);
    assert_eq!(head(&f).hash, second.hash().unwrap());
}

#[test]
fn what_fails_checks_seven_to_nine_is_chained_and_never_applied() {
    let mut f = forged();
    let claim = f.claim();
    // Check 7: a version of an object nobody began.
    let mut forbidden = f.pen.sign(&claim, &note_draft(), &Objects::new(), now());
    let Subject::Version(fields) = forbidden.header.subject else {
        panic!("a version");
    };
    forbidden.header.subject = Subject::Version(ObjectFields {
        object_id: ObjectId::new([5; 16]),
        object_ref: Hash32::new([6; 32]),
        ..fields
    });
    f.pen.resign(&mut forbidden);
    let got = f.slip(&forbidden);
    assert_eq!(
        (got.outcome, got.code.clone()),
        (EnvelopeOutcome::Chained, Some(Error::Forbidden))
    );
    assert!(got.body.is_none() && got.object_after.is_none());
    assert!(f.reader.objects(&f.room).unwrap().is_empty());
    // Check 9: a body sealed under another header does not open; the envelope still counts for its object.
    let mut unreadable = f.pen.sign(&claim, &note_draft(), &Objects::new(), now());
    unreadable.header.time += 1;
    f.pen.resign(&mut unreadable);
    let got = f.slip(&unreadable);
    assert_eq!(
        (got.outcome, got.code.clone()),
        (EnvelopeOutcome::Chained, Some(Error::DecryptFailed))
    );
    assert_eq!(got.object_after.unwrap().1.state, ObjectState::Open);
    // A pruned envelope verifies and carries the chain and the object.
    let note = f.pen.sign(&claim, &note_draft(), &Objects::new(), now());
    let got = f.slip(&note.prune().unwrap());
    assert_eq!(
        (got.outcome, got.code.clone()),
        (EnvelopeOutcome::Chained, Some(Error::Pruned))
    );
    assert!(got.object_after.is_some());
    // Fetched in full later, its body is taken after all.
    let full = f
        .reader
        .receive_envelope(&note.encode().unwrap(), 0, false, None, now())
        .unwrap();
    assert_eq!(
        (full.outcome, full.code.clone()),
        (EnvelopeOutcome::Applied, None)
    );
    assert!(full.body.is_some() && full.object_after.is_some());
    // An envelope the chain holds is read back under the number of its place there, whatever number the fetch
    // names: that one is the hub's word alone.
    let ahead = f
        .reader
        .receive_envelope(&note.encode().unwrap(), got.change + 1_000_000, false, None, now())
        .unwrap();
    assert_eq!((full.change, ahead.change), (got.change, got.change));
    // A kind of a newer Trommi is chained and nothing else.
    let mut newer = f.pen.sign(&claim, &note_draft(), &Objects::new(), now());
    newer.header.subject = Subject::Reserved {
        kind: 9,
        block: [0; envelope::OBJECT_BLOCK_LEN],
    };
    f.pen.resign(&mut newer);
    let got = f.slip(&newer);
    assert_eq!(
        (got.outcome, got.code.clone()),
        (EnvelopeOutcome::Chained, Some(Error::NewerVersion))
    );
    assert_eq!(f.reader.chain_head(&f.room, &f.pen.id()).unwrap().seq, 4);
    assert_eq!(f.reader.objects(&f.room).unwrap().len(), 2);
}

#[test]
fn an_envelope_of_an_ended_epoch_is_taken_for_two_minutes() {
    let mut f = forged();
    let old = f.claim();
    let in_time = f.pen.sign(&old, &board_item(), &Objects::new(), now());
    let late = f.pen.sign(&old, &board_item(), &Objects::new(), now());
    // The reader ends the epoch at a moment of its own clock.
    let ended = now();
    f.reader.update(&f.room, true, ended).unwrap().unwrap();
    post_ok(&mut f.hub, &mut f.reader);
    sync_all(&f.hub, &mut f.reader);
    assert_eq!(f.reader.group(&f.room).unwrap().epoch, old.epoch + 1);
    let take = |f: &mut Forged, envelope: &Envelope, at: u64| {
        let stored = f.post(envelope);
        f.reader
            .receive_envelope(&stored.bytes, stored.change, true, None, at)
            .unwrap()
    };
    let got = take(&mut f, &in_time, ended + 119_000);
    assert_eq!(got.outcome, EnvelopeOutcome::Applied);
    let got = take(&mut f, &late, ended + 121_000);
    assert_eq!(
        (got.outcome, got.code),
        (EnvelopeOutcome::Chained, Some(Error::WrongEpoch))
    );
}

#[test]
fn a_void_record_is_chained_and_a_missing_or_unfounded_marker_shows() {
    let mut f = forged();
    let claim = f.claim();
    // The hub voids what check 7 forbids: it keeps the number and says so.
    let mut forbidden = f.pen.sign(&claim, &note_draft(), &Objects::new(), now());
    let Subject::Version(fields) = forbidden.header.subject else {
        panic!("a version");
    };
    forbidden.header.subject = Subject::Version(ObjectFields {
        object_id: ObjectId::new([5; 16]),
        object_ref: Hash32::new([6; 32]),
        ..fields
    });
    f.pen.resign(&mut forbidden);
    let entry = posting(&forbidden);
    assert_eq!(f.hub.post(&f.pen.id(), &entry), Err(Error::Forbidden));
    assert!(f.hub.voided(&entry));
    let stored = f.hub.content.envelopes.last().unwrap().clone();
    assert!(Envelope::decode(&stored.bytes).unwrap().is_pruned());
    // Served with its marker: a void record whose reason the reader checks again and finds.
    let got = take_envelope(&mut f.reader, &stored);
    assert_eq!(
        (got.outcome, got.code.clone(), got.finding.clone()),
        (EnvelopeOutcome::Void, Some(Error::Forbidden), None)
    );

    // A hub that removes the marker changes nothing: the reader's own check 7 refuses the envelope.
    let mut second = forged();
    let claim = second.claim();
    let mut forbidden = second
        .pen
        .sign(&claim, &note_draft(), &Objects::new(), now());
    forbidden.header.subject = Subject::Version(ObjectFields {
        object_id: ObjectId::new([5; 16]),
        object_ref: Hash32::new([6; 32]),
        ..fields
    });
    second.pen.resign(&mut forbidden);
    let _ = second.hub.post(&second.pen.id(), &posting(&forbidden));
    let mut stored = second.hub.content.envelopes.last().unwrap().clone();
    stored.void_code = None;
    let got = take_envelope(&mut second.reader, &stored);
    assert_eq!(
        (got.outcome, got.code),
        (EnvelopeOutcome::Chained, Some(Error::Forbidden))
    );
    assert!(second.reader.objects(&second.room).unwrap().is_empty());

    // A hub that voids another sender's good envelope is found out: the reason cannot be checked again.
    let good = f
        .pen
        .sign(&f.claim(), &note_draft(), &Objects::new(), now());
    let change = f.reader.cursor() + 1;
    let got = f
        .reader
        .receive_envelope(
            &good.prune().unwrap().encode().unwrap(),
            change,
            true,
            Some(&Error::Forbidden),
            now(),
        )
        .unwrap();
    assert_eq!(got.outcome, EnvelopeOutcome::Void);
    assert_eq!(got.finding, Some(Error::HubVoidedOther));
    assert!(f.reader.objects(&f.room).unwrap().is_empty());
}

#[test]
fn a_provisional_envelope_is_confirmed_or_dropped_when_its_chain_arrives() {
    let mut f = forged();
    let first = f.stroke();
    let second = f.stroke();
    let fetch = |f: &mut Forged, envelope: &Envelope| {
        f.reader
            .receive_envelope(&envelope.encode().unwrap(), 0, false, None, now())
            .unwrap()
    };
    // A page of the board brings the second before its chain: shown, not accepted.
    let got = fetch(&mut f, &second);
    assert_eq!(got.outcome, EnvelopeOutcome::Provisional);
    assert!(got.body.is_some() && !got.command);
    assert_eq!(f.reader.chain_head(&f.room, &f.pen.id()).unwrap().seq, 0);
    assert_eq!(f.slip(&first).provisional, None);
    assert_eq!(f.slip(&second).provisional, Some(Confirmation::Confirmed));
    // Fetched again, it is what its chain made of it.
    assert_eq!(fetch(&mut f, &second).outcome, EnvelopeOutcome::Applied);

    // Another envelope under a number the chain holds is a hash mismatch at once.
    let mut pen = f.pen.fork();
    let third = pen.sign(&f.claim(), &board_item(), &Objects::new(), now());
    let third_again = f
        .pen
        .fork()
        .sign(&f.claim(), &note_draft(), &Objects::new(), now());
    assert_eq!(fetch(&mut f, &third).outcome, EnvelopeOutcome::Provisional);
    // The chain reaches the number with the other one: what was shown is dropped.
    let got = f.slip(&third_again);
    assert_eq!(got.outcome, EnvelopeOutcome::Applied);
    assert_eq!(
        got.provisional,
        Some(Confirmation::Dropped(Error::HashMismatch))
    );
    let got = fetch(&mut f, &third);
    assert_eq!(
        (got.outcome, got.code),
        (EnvelopeOutcome::Refused, Some(Error::HashMismatch))
    );
}

#[test]
fn a_cut_ends_a_removed_devices_chain_for_everyone() {
    let mut a = new_device();
    let mut b = new_device();
    let (mut hub, room) = found_room(&mut a);
    add_human(&mut hub, &mut a, &mut b);
    let forger = Forger::new();
    add_forger(&mut hub, &mut a, &forger);
    sync_all(&hub, &mut a);
    sync_all(&hub, &mut b);
    let mut pen = Pen::new(&forger.key);
    let epoch = a.group(&room).unwrap().epoch;
    let claim = Claim {
        group: room,
        epoch,
        role: Role::Human,
        seat: None,
        key: a.content_key(&room, epoch).unwrap(),
    };
    let mut write = |hub: &mut Hub, draft: &envelope::Draft| {
        let envelope = pen.sign(&claim, draft, &Objects::new(), now());
        hub.post(&pen.id(), &posting(&envelope)).unwrap();
        hub.content.envelopes.last().unwrap().clone()
    };
    let one = write(&mut hub, &board_item());
    let two = write(&mut hub, &note_draft());
    let note_id = two.header.subject.object().unwrap().object_id;
    // The remover has accepted the first only; the other device both.
    take_envelope(&mut a, &one);
    take_envelope(&mut b, &one);
    assert_eq!(
        take_envelope(&mut b, &two).outcome,
        EnvelopeOutcome::Applied
    );
    assert!(b.object(&room, &note_id).unwrap().is_some());
    let cut = a.cut_of(&room, &pen.id()).unwrap();
    assert_eq!((cut.seq, cut.hash), (1, one.hash));
    a.remove_human_devices(&[cut], now()).unwrap();
    post_ok(&mut hub, &mut a);
    // The hub marks what lies beyond the Cut and serves it no more; its objects are as if it never came.
    assert!(hub.chain_of(&room, &pen.id(), 1)[0].cut);
    assert!(hub.objects(&room).get(&note_id).is_none());
    assert!(!hub
        .changes_after(0)
        .iter()
        .any(|change| matches!(change, Change::Envelope(stored) if stored.hash == two.hash)));
    sync_all(&hub, &mut a);
    sync_all(&hub, &mut b);
    for device in [&mut a, &mut b] {
        assert_eq!(device.chain_cut(&room, &pen.id()).unwrap().unwrap().seq, 1);
        assert_eq!(device.chain_head(&room, &pen.id()).unwrap().hash, one.hash);
        assert!(device.object(&room, &note_id).unwrap().is_none());
        // What lies beyond is refused by everyone, by whichever route it comes.
        let again = device
            .receive_envelope(&two.bytes, two.change, true, None, now())
            .unwrap();
        assert_eq!(again.code, Some(Error::RemovedSender));
        let fetched = device
            .receive_envelope(&two.bytes, two.change, false, None, now())
            .unwrap();
        assert_eq!(fetched.code, Some(Error::RemovedSender));
        assert!(device.findings().unwrap().is_empty());
    }
    // And by the hub.
    let three = pen.sign(&claim, &board_item(), &Objects::new(), now());
    assert_eq!(
        hub.post(&pen.id(), &posting(&three)),
        Err(Error::RemovedSender)
    );
}

#[test]
fn a_hostile_hub_ends_in_the_named_finding() {
    let mut a = new_device();
    let mut b = new_device();
    let mut c = new_device();
    let (mut hub, room) = found_room(&mut a);
    add_human(&mut hub, &mut a, &mut b);
    add_human(&mut hub, &mut a, &mut c);
    sync_all(&hub, &mut a);
    sync_all(&hub, &mut b);
    sync_all(&hub, &mut c);
    let note = |n: u32| Draft::NoteFirst {
        payload: json(&format!(
            r#"{{"text":"{n}","previous_version_hash":"{ZERO_HASH}"}}"#
        )),
    };
    let first = write(&mut hub, &mut b, &note(1));
    let second = write(&mut hub, &mut b, &note(2));
    let third = write(&mut hub, &mut b, &note(3));
    sync_all(&hub, &mut b);
    let served: Vec<StoredEnvelope> = hub.chain_of(&room, &b.id(), 0);

    // It reorders within a chain: nothing is taken out of turn, and in turn everything is.
    let got = take_envelope(&mut a, &served[1]);
    assert_eq!(got.code, Some(Error::Gap));
    assert_eq!(
        take_envelope(&mut a, &served[0]).outcome,
        EnvelopeOutcome::Applied
    );
    assert_eq!(
        take_envelope(&mut a, &served[1]).outcome,
        EnvelopeOutcome::Applied
    );
    assert_eq!(
        a.object(&room, &second.object_id.unwrap()).unwrap(),
        b.object(&room, &second.object_id.unwrap()).unwrap()
    );
    let _ = first;

    // It drops the newest envelope of a sender: the heads of a device that got it show what is missing.
    sync_all(&hub, &mut c);
    let value = c.heads_due(&room, now()).unwrap().unwrap();
    let heads = Draft::Register {
        group: room,
        name: "heads".into(),
        value: Some(SecretBytes::new(value)),
    };
    write(&mut hub, &mut c, &heads);
    let told = hub.chain_of(&room, &c.id(), 0);
    assert_eq!(
        take_envelope(&mut a, &told[0]).outcome,
        EnvelopeOutcome::Applied
    );
    let standings = a.compare_heads(&room, &c.id()).unwrap();
    assert_eq!(standings, vec![(b.id(), HeadStanding::Behind { have: 2 })]);
    // The reader asks for the chain after number 2, the hub has nothing, and the comparison stands.
    let _ = third;
    let again = a.compare_heads(&room, &c.id()).unwrap();
    assert_eq!(again[0].1.after_fetch(), Err(Error::Withheld));

    // It serves a fork: another envelope under a number the reader holds.
    let forger = Forger::new();
    add_forger(&mut hub, &mut a, &forger);
    sync_all(&hub, &mut a);
    let epoch = a.group(&room).unwrap().epoch;
    let claim = Claim {
        group: room,
        epoch,
        role: Role::Human,
        seat: None,
        key: a.content_key(&room, epoch).unwrap(),
    };
    let pen = Pen::new(&forger.key);
    let one = pen
        .fork()
        .sign(&claim, &board_item(), &Objects::new(), now());
    let other = pen
        .fork()
        .sign(&claim, &note_draft(), &Objects::new(), now());
    let cursor = a.cursor();
    let take = |a: &mut TestDevice, envelope: &Envelope| {
        a.receive_envelope(&envelope.encode().unwrap(), cursor, true, None, now())
            .unwrap()
    };
    assert_eq!(take(&mut a, &one).outcome, EnvelopeOutcome::Applied);
    let got = take(&mut a, &other);
    assert_eq!(
        (got.outcome, got.code),
        (EnvelopeOutcome::Refused, Some(Error::Equivocation))
    );
    let forked = other.header.subject.object().unwrap().object_id;
    assert!(a.object(&room, &forked).unwrap().is_none());
}

#[test]
fn a_board_is_loaded_from_its_snapshot_and_the_items_after_it() {
    use trommi_core::board::{self, ServedItem, Snapshot};
    use trommi_core::board_items::{ItemBody, Shape};
    use trommi_core::crypto::SystemEntropy;
    use trommi_core::device::{board_reduce, BoardItem, BoardSnapshot};
    use trommi_tests::content::{shape, Dice};

    let mut a = new_device();
    let mut b = new_device();
    let (mut hub, room) = found_room(&mut a);
    add_human(&mut hub, &mut a, &mut b);
    sync_all(&hub, &mut a);
    sync_all(&hub, &mut b);
    let board_id = BoardId::ALL_DESKS;
    let mut dice = Dice(SystemEntropy);
    let drawn: Shape = shape(&mut dice).unwrap();
    let strokes = |shapes: Vec<Shape>| Draft::BoardItem {
        board: board_id,
        payload: SecretBytes::new(
            ItemBody::Strokes(shapes)
                .encode()
                .unwrap()
                .expose()
                .to_vec(),
        ),
    };
    let item = |device: &TestDevice, sealed: &trommi_core::device::Sealed, hub: &Hub| BoardItem {
        sender: device.id(),
        seq: sealed.seq,
        payload: SecretBytes::new(
            Envelope::decode(&hub.chain_of(&room, &device.id(), sealed.seq - 1)[0].bytes)
                .unwrap()
                .open(
                    &device
                        .content_key(&room, device.group(&room).unwrap().epoch)
                        .unwrap(),
                )
                .unwrap()
                .payload()
                .to_vec(),
        ),
    };
    // One item, then the writer's snapshot of the board as it stands, then another item.
    let first = write(&mut hub, &mut a, &strokes(vec![drawn]));
    sync_all(&hub, &mut a);
    let frontier = vec![(a.id(), a.chain_head(&room, &a.id()).unwrap())];
    let file = board_reduce(None, &[item(&a, &first, &hub)], &frontier).unwrap();
    let snapshot = Snapshot {
        attachment: r#"{"file_id":"AAAAAAAAAAAAAAAAAAAAAA"}"#.into(),
        frontier: frontier.clone(),
        change: a.cursor(),
    };
    write(
        &mut hub,
        &mut a,
        &Draft::Register {
            group: room,
            name: board::snapshot_name(&board_id),
            value: Some(json(&snapshot.value().unwrap())),
        },
    );
    let second = write(&mut hub, &mut a, &strokes(vec![shape(&mut dice).unwrap()]));

    // The other device has no snapshot before it read the register.
    assert_eq!(b.board_load(&board_id, &[]), Err(Error::NotFound));
    sync_all(&hub, &mut b);
    let served = [ServedItem {
        sender: a.id(),
        seq: second.seq,
        hash: second.envelope_hash,
    }];
    // A hub that leaves out the item after the snapshot is found out by the writer's chain.
    assert_eq!(b.board_load(&board_id, &[]), Err(Error::Withheld));
    let loaded = b.board_load(&board_id, &served).unwrap();
    assert_eq!(loaded.fresh, vec![0]);
    assert_eq!(
        loaded.frontier,
        vec![(a.id(), b.chain_head(&room, &a.id()).unwrap())]
    );
    // The board is the snapshot with the fresh item merged in.
    let whole = board_reduce(
        Some(&BoardSnapshot {
            file: file.expose(),
            frontier: &frontier,
        }),
        &[item(&a, &second, &hub)],
        &loaded.frontier,
    )
    .unwrap();
    let direct = board_reduce(
        None,
        &[item(&a, &second, &hub), item(&a, &first, &hub)],
        &loaded.frontier,
    )
    .unwrap();
    assert_eq!(whole.expose(), direct.expose());
    // The same snapshot loads again, with the items after it: the board is built anew from it.
    assert_eq!(b.board_load(&board_id, &served), Ok(loaded.clone()));
    let third = write(&mut hub, &mut a, &strokes(vec![shape(&mut dice).unwrap()]));
    sync_all(&hub, &mut b);
    let both = [
        served[0],
        ServedItem {
            sender: a.id(),
            seq: third.seq,
            hash: third.envelope_hash,
        },
    ];
    assert_eq!(b.board_load(&board_id, &both).unwrap().fresh, vec![0, 1]);
    assert_eq!(b.board_load(&board_id, &both).unwrap().fresh, vec![0, 1]);
    // A newer snapshot written by a device that had read the board only up to the second item is taken,
    // though this device applied the third item already.
    let newer = Snapshot {
        attachment: r#"{"file_id":"AAAAAAAAAAAAAAAAAAAAAA"}"#.into(),
        frontier: vec![(
            a.id(),
            trommi_core::chain::Head {
                seq: second.seq,
                hash: second.envelope_hash,
            },
        )],
        change: a.cursor(),
    };
    let register = |snapshot: &Snapshot| Draft::Register {
        group: room,
        name: board::snapshot_name(&board_id),
        value: Some(json(&snapshot.value().unwrap())),
    };
    write(&mut hub, &mut a, &register(&newer));
    sync_all(&hub, &mut b);
    assert_eq!(b.board_load(&board_id, &both[1..]).unwrap().fresh, vec![0]);
    // An older snapshot is not loaded over the frontier of the one loaded last.
    let older = Snapshot {
        frontier: Vec::new(),
        ..newer
    };
    write(&mut hub, &mut a, &register(&older));
    sync_all(&hub, &mut b);
    assert_eq!(b.board_load(&board_id, &both), Err(Error::Replay));
}

#[test]
fn an_envelope_that_comes_behind_later_ones_is_judged_at_its_own_place() {
    let (mut w, mut other) = world_of_two();
    let made = write(
        &mut w.hub,
        &mut w.agent,
        &Draft::CardFirst {
            session: w.session,
            urgency: Urgency::Normal,
            push: false,
            payload: card("Late", ZERO_HASH),
        },
    );
    let card_id = made.object_id.unwrap();
    sync_all(&w.hub, &mut w.human);
    sync_all(&w.hub, &mut w.agent);
    let answered = write(
        &mut w.hub,
        &mut w.human,
        &Draft::Answer {
            session: w.session,
            object_id: card_id,
            choices: vec!["no".into()],
            closes: false,
            payload: json(r#"{"answer_action":"answer"}"#),
        },
    );
    // The hub hands the other device the answer before the card it answers.
    let card_stored = w.hub.chain_of(&w.group, &w.agent.id(), 0).remove(0);
    let answer_stored = w.hub.chain_of(&w.group, &w.human.id(), 0).remove(0);
    let early = take_envelope(&mut other, &answer_stored);
    assert_eq!(early.envelope_hash, answered.envelope_hash);
    assert_eq!(
        (early.outcome, early.code),
        (EnvelopeOutcome::Chained, Some(Error::Forbidden))
    );
    // The card comes behind it: it is judged where it stands in the hub's order, and the group's state is
    // built again, with the answer in its place.
    let late = take_envelope(&mut other, &card_stored);
    assert_eq!(late.outcome, EnvelopeOutcome::Applied);
    assert!(late.replayed);
    let state = other.object(&w.group, &card_id).unwrap().unwrap();
    assert_eq!(state.state, ObjectState::Answered);
    assert_eq!(Some(&state), w.hub.objects(&w.group).get(&card_id));
    assert_eq!(late.object_after.unwrap().1, state);
}

#[test]
fn a_relayed_stroke_piece_is_taken_without_the_log() {
    use trommi_core::device::Received;
    let (mut w, mut other) = world_of_two();
    let piece = br#"{"stroke":"AAAAAAAAAAAAAAAAAAAAAA","number":1}"#;
    w.human
        .send_stroke_piece(&BoardId::ALL_DESKS, piece)
        .unwrap();
    post_ok(&mut w.hub, &mut w.human);
    let (group, relayed) = w.hub.relayed.last().unwrap().clone();
    let cursor = other.cursor();
    let got = other.receive_relay(&group, &relayed, now()).unwrap();
    assert_eq!(
        got,
        Some(Received::StrokePiece {
            from: w.human.id(),
            board: BoardId::ALL_DESKS,
            piece: piece.to_vec(),
        })
    );
    assert_eq!(other.cursor(), cursor);
    // It opens once, and an agent device, no leaf of the room group, gets nothing.
    assert_eq!(other.receive_relay(&group, &relayed, now()).unwrap(), None);
    assert_eq!(
        w.agent.receive_relay(&group, &relayed, now()).unwrap(),
        None
    );
}

/// 7.0: an application message is padded to its bucket: pieces of different lengths in one bucket travel alike,
/// and still open to what was sent.
#[test]
fn application_messages_are_padded_to_buckets() {
    use trommi_core::device::Received;
    let (mut w, mut other) = world_of_two();
    let piece = |pad: usize| {
        format!(
            r#"{{"stroke":"AAAAAAAAAAAAAAAAAAAAAA","number":1,"x":"{}"}}"#,
            "a".repeat(pad)
        )
        .into_bytes()
    };
    let mut sizes = Vec::new();
    for pad in [0usize, 200, 2000, 2500] {
        let sent = piece(pad);
        w.human
            .send_stroke_piece(&BoardId::ALL_DESKS, &sent)
            .unwrap();
        post_ok(&mut w.hub, &mut w.human);
        let (group, relayed) = w.hub.relayed.last().unwrap().clone();
        sizes.push(relayed.len());
        let got = other.receive_relay(&group, &relayed, now()).unwrap();
        assert_eq!(
            got,
            Some(Received::StrokePiece {
                from: w.human.id(),
                board: BoardId::ALL_DESKS,
                piece: sent,
            })
        );
    }
    // 0 and 200 bytes more fit the bucket of 512; 2000 and 2500 that of 4096
    assert_eq!(sizes[0], sizes[1]);
    assert_eq!(sizes[2], sizes[3]);
    assert!(sizes[2] > sizes[1] + 3000);
    assert_eq!(trommi_core::mls::message::bucket(0), Some(512));
    assert_eq!(
        trommi_core::mls::message::bucket(40_000),
        Some(trommi_core::mls::message::TOP_BUCKET)
    );
    assert_eq!(trommi_core::mls::message::bucket(48_769), None);
}

#[test]
fn the_heads_envelope_itself_calls_for_no_other_heads() {
    let store = trommi_tests::MemoryStorage::new();
    let mut handle = store.handle();
    let (mut human, mut other) = (trommi_tests::new_device_on(store), new_device());
    let (mut hub, room) = found_room(&mut human);
    add_human(&mut hub, &mut human, &mut other);
    sync_all(&hub, &mut human);
    sync_all(&hub, &mut other);
    let note = Draft::NoteFirst {
        payload: json(&format!(
            r#"{{"text":"n","previous_version_hash":"{ZERO_HASH}"}}"#
        )),
    };
    let heads_of = |device: &mut TestDevice, at: u64| {
        device
            .heads_due(&room, at)
            .unwrap()
            .map(|value| Draft::Register {
                group: room,
                name: "heads".into(),
                value: Some(SecretBytes::new(value)),
            })
    };
    let mut restart = |device: TestDevice| {
        drop(device);
        let store = handle.reopened();
        handle = store.handle();
        trommi_tests::reopen(store).unwrap()
    };
    let start = now();
    write(&mut hub, &mut human, &note);
    sync_all(&hub, &mut human);
    let heads = heads_of(&mut human, start).unwrap();
    write(&mut hub, &mut human, &heads);
    sync_all(&hub, &mut human);

    // The device comes online again: its own chain moved by the `heads` envelope alone, nothing is due.
    let mut human = restart(human);
    assert!(heads_of(&mut human, start + 1).is_none());
    assert!(heads_of(&mut human, start + HEADS_EVERY_MS).is_none());
    let mut human = restart(human);
    assert!(heads_of(&mut human, start + 2 * HEADS_EVERY_MS).is_none());

    // Another sender writes: a head changed. The device says so when it next comes online.
    write(&mut hub, &mut other, &note);
    sync_all(&hub, &mut human);
    assert!(heads_of(&mut human, start + 1).is_none());
    let mut human = restart(human);
    let heads = heads_of(&mut human, start + 1).unwrap();
    // The value lists every chain, the device's own too.
    let Draft::Register {
        value: Some(value), ..
    } = &heads
    else {
        unreachable!()
    };
    let listed = String::from_utf8(value.expose().to_vec()).unwrap();
    assert!(listed.contains(&human.id().to_base64url()));
    assert!(listed.contains(&other.id().to_base64url()));
    write(&mut hub, &mut human, &heads);
    sync_all(&hub, &mut human);
    assert!(heads_of(&mut human, start + 3 * HEADS_EVERY_MS).is_none());

    // What the device writes itself that is no `heads` is a changed head too.
    write(&mut hub, &mut human, &note);
    assert!(heads_of(&mut human, start + 3 * HEADS_EVERY_MS).is_some());
}

// ---- register values read out of the hub's order: the overlay of a human device (9.3.6) ----

use trommi_client_core::engine::feed::received_of;
use trommi_client_core::model::change::Change as ModelChange;
use trommi_client_core::model::Model;
use trommi_core::ids::RegisterId;

const NAME: &str = "desk/AAAAAAAAAAAAAAAAAAAAAA";
const OTHER: &str = "desk/BBBBBBBBBBBBBBBBBBBBBB";
const J: u64 = 1 << 24;

/// What a human device shows: the board model, fed with every envelope the device reports, as the apps feed it.
struct Shown(Model, ModelChange, Vec<(String, Option<String>)>);

impl Shown {
    fn new() -> Self {
        Self(Model::new(), ModelChange::new(), Vec::new())
    }

    fn take(&mut self, received: &ReceivedEnvelope) {
        self.0
            .apply_envelope(&received_of(received), &mut self.1, now());
        for name in [NAME, OTHER] {
            let now = self.get(name);
            if self.2.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v) != Some(&now) {
                self.2.push((name.into(), now));
            }
        }
    }

    /// The `name` field of the value shown for a room register.
    fn get(&self, name: &str) -> Option<String> {
        self.0
            .human
            .get(&trommi_client_core::codec::register_key_of(name))
            .and_then(|held| held.value.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_owned)
    }

    /// Every value `name` showed, in turn.
    fn history(&self, name: &str) -> Vec<Option<String>> {
        self.2
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.clone())
            .collect()
    }
}

/// The `name` field of the value the device's chains hold for a room register.
fn held(device: &TestDevice, room: &GroupId, name: &str) -> Option<String> {
    let value = device.register(room, name).unwrap()?;
    let value: serde_json::Value = serde_json::from_slice(value.expose()).unwrap();
    value
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

/// A register envelope with a lamport of the test's choice, under the register id `id` of its writer.
fn register_draft(id: u8, name: &str, value: Option<&str>, lamport: u64) -> envelope::Draft {
    let value = value.map_or(
        serde_json::Value::Null,
        |v| serde_json::json!({ "name": v }),
    );
    let payload = serde_json::json!({ "name": name, "value": value, "lamport": lamport });
    envelope::Draft::register(RegisterId::new([id; 16]), payload.to_string().as_bytes())
}

impl Forged {
    fn register(&mut self, id: u8, name: &str, value: Option<&str>, lamport: u64) -> Envelope {
        let claim = self.claim();
        self.pen.sign(
            &claim,
            &register_draft(id, name, value, lamport),
            &Objects::new(),
            now(),
        )
    }

    /// The reader fetches an envelope out of the hub's order (the Desk's registers).
    fn fetch(&mut self, stored: &StoredEnvelope) -> ReceivedEnvelope {
        self.reader
            .receive_envelope(&stored.bytes, stored.change, false, None, now())
            .unwrap()
    }

    /// The reader takes an envelope in the hub's order.
    fn read(&mut self, bytes: &[u8], change: u64, void: Option<&Error>) -> ReceivedEnvelope {
        self.reader
            .receive_envelope(bytes, change, true, void, now())
            .unwrap()
    }
}

/// Every kind of register the Desk serves stands on a human device before its chain is read: a desk, a session's
/// name (the person's settings of it), a star, a snooze (the card off the Desk), a device's name; and an agent's
/// profile in its session.
#[test]
fn every_kind_of_register_read_out_of_the_order_stands_on_a_human_device() {
    let mut f = forged();
    let room = f.room;
    let pen_id = f.pen.id().to_base64url();
    let card = "CCCCCCCCCCCCCCCCCCCCCA";
    let kinds = [
        (NAME.to_string(), r#"{"name":"Lab"}"#),
        (
            "session/SSSSSSSSSSSSSSSSSSSSSA".to_string(),
            r#"{"name":"Named"}"#,
        ),
        (format!("star/{card}"), r#"{"at":5}"#),
        (format!("snooze/{card}"), r#"{"until":99999999999999}"#),
        (format!("device/{pen_id}"), r#"{"device_name":"pen"}"#),
    ];
    let mut shown = Shown::new();
    for (i, (name, value)) in kinds.iter().enumerate() {
        let claim = f.claim();
        let payload = format!(r#"{{"name":"{name}","value":{value},"lamport":{}}}"#, i + 1);
        let draft =
            envelope::Draft::register(RegisterId::new([i as u8 + 1; 16]), payload.as_bytes());
        let envelope = f.pen.sign(&claim, &draft, &Objects::new(), now());
        let stored = f.post(&envelope);
        let got = f.fetch(&stored);
        assert_eq!(got.outcome, EnvelopeOutcome::Provisional, "{name}");
        let change = got.register.clone().expect("a register change");
        assert!(change.current, "{name}");
        // the chains hold nothing of it
        assert_eq!(f.reader.register(&room, name).unwrap(), None, "{name}");
        shown.take(&got);
    }
    let model = &shown.0;
    assert_eq!(shown.get(NAME).as_deref(), Some("Lab"));
    let key = trommi_client_core::codec::register_key_of;
    assert!(model
        .human
        .contains_key(&key("session/SSSSSSSSSSSSSSSSSSSSSA")));
    assert!(model.human.contains_key(&key(&format!("star/{card}"))));
    assert!(model.human.contains_key(&key(&format!("snooze/{card}"))));
    assert_eq!(
        model
            .device_registers
            .get(&hex(f.pen.id().as_bytes()))
            .and_then(|d| d.get("device_name"))
            .and_then(|v| v.as_str()),
        Some("pen")
    );

    // An agent's profile in its session, on a human device that has not read the session yet.
    let (mut w, mut other) = world_of_two();
    sync_all(&w.hub, &mut w.agent);
    sync_all(&w.hub, &mut other);
    write(
        &mut w.hub,
        &mut w.agent,
        &Draft::Register {
            group: w.group,
            name: "profile".into(),
            value: Some(json(r#"{"agent_name":"Probe"}"#)),
        },
    );
    let profile = w.hub.chain_of(&w.group, &w.agent.id(), 0).pop().unwrap();
    let got = other
        .receive_envelope(&profile.bytes, profile.change, false, None, now())
        .unwrap();
    assert_eq!(got.outcome, EnvelopeOutcome::Provisional);
    assert!(got.register.as_ref().expect("a register change").current);
    let mut shown = Shown::new();
    shown.take(&got);
    let session = hex(w.session.as_bytes());
    assert!(shown
        .0
        .sessions
        .get(&session)
        .is_some_and(|s| s.registers.contains_key("profile")));
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A device that holds a value never shows an older one: an older value read out of the order is not current,
/// and a value of the chain older than the one shown from the overlay does not replace it.
#[test]
fn an_overlay_value_never_rolls_back_what_a_device_holds() {
    let mut f = forged();
    let room = f.room;
    let old = f.register(1, NAME, Some("old"), 1);
    let mid = f.register(1, NAME, Some("mid"), 2);
    let new = f.register(1, NAME, Some("new"), 3);
    let (old, mid, new) = (f.post(&old), f.post(&mid), f.post(&new));
    let mut shown = Shown::new();
    // the newest first, out of the order: it stands
    let got = f.fetch(&new);
    assert!(got.register.as_ref().unwrap().current);
    shown.take(&got);
    // an older one out of the order after it: not current
    let got = f.fetch(&mid);
    assert!(!got.register.as_ref().unwrap().current);
    shown.take(&got);
    // the chain brings the oldest and the middle one: they count there, but are not shown over the newer one
    for stored in [&old, &mid] {
        let got = f.read(&stored.bytes, stored.change, None);
        assert_eq!(got.outcome, EnvelopeOutcome::Applied);
        assert!(
            !got.register.as_ref().unwrap().current,
            "masked by the newer value read out of the order"
        );
        shown.take(&got);
    }
    assert_eq!(held(&f.reader, &room, NAME).as_deref(), Some("mid"));
    // the chain reaches the newest: it stands there too
    let got = f.read(&new.bytes, new.change, None);
    assert!(got.register.as_ref().unwrap().current);
    assert!(got.shown.is_empty());
    shown.take(&got);
    assert_eq!(held(&f.reader, &room, NAME).as_deref(), Some("new"));
    assert_eq!(shown.history(NAME), vec![Some("new".into())]);
    // a value the chains hold is never shown over by an older one read out of the order
    let older = f.register(1, NAME, Some("older"), 2);
    let older = f.post(&older);
    let got = f.fetch(&older);
    assert!(!got.register.as_ref().unwrap().current);
    shown.take(&got);
    assert_eq!(shown.get(NAME).as_deref(), Some("new"));
}

/// Codex finding 1: the chain reaches a value of the overlay without its body (pruned as served): it takes the
/// overlay's body, and a rebuild of the group's registers keeps the newer value.
#[test]
fn a_bodiless_chain_record_takes_the_overlays_body_and_a_rebuild_keeps_it() {
    let mut f = forged();
    let room = f.room;
    let old = f.register(1, NAME, Some("old"), 1);
    let new = f.register(1, NAME, Some("new"), 2);
    let (old, new_stored) = (f.post(&old), f.post(&new));
    let mut shown = Shown::new();
    shown.take(&f.fetch(&new_stored));
    shown.take(&f.read(&old.bytes, old.change, None));
    let pruned = new.prune().unwrap().encode().unwrap();
    let got = f.read(&pruned, new_stored.change, None);
    assert_eq!(
        got.outcome,
        EnvelopeOutcome::Applied,
        "the overlay's body: the very envelope"
    );
    assert!(got.register.as_ref().unwrap().current);
    shown.take(&got);
    // an envelope behind later ones builds the group's registers again
    let late = f.register(2, OTHER, Some("late"), 3);
    let got = f.read(&late.encode().unwrap(), 1, None);
    assert!(got.replayed);
    shown.take(&got);
    assert_eq!(held(&f.reader, &room, NAME).as_deref(), Some("new"));
    assert_eq!(shown.get(NAME).as_deref(), Some("new"));
    assert_eq!(shown.history(NAME), vec![Some("new".into())]);
}

/// Codex finding 2: a Cut drops what the overlay holds of its sender beyond it; nothing of it stays current or
/// keeps a later value of the chains from standing.
#[test]
fn a_cut_drops_the_overlay_values_beyond_it() {
    let mut a = new_device();
    let mut b = new_device();
    let (mut hub, room) = found_room(&mut a);
    add_human(&mut hub, &mut a, &mut b);
    let forger = Forger::new();
    add_forger(&mut hub, &mut a, &forger);
    sync_all(&hub, &mut a);
    sync_all(&hub, &mut b);
    let mut pen = Pen::new(&forger.key);
    let epoch = a.group(&room).unwrap().epoch;
    let claim = Claim {
        group: room,
        epoch,
        role: Role::Human,
        seat: None,
        key: a.content_key(&room, epoch).unwrap(),
    };
    let envelope = pen.sign(
        &claim,
        &register_draft(1, NAME, Some("cut"), 5),
        &Objects::new(),
        now(),
    );
    hub.post(&pen.id(), &posting(&envelope)).unwrap();
    let stored = hub.content.envelopes.last().unwrap().clone();
    let mut shown = Shown::new();
    let got = a
        .receive_envelope(&stored.bytes, stored.change, false, None, now())
        .unwrap();
    assert!(got.register.as_ref().unwrap().current);
    shown.take(&got);
    // a removes the pen's device with its Cut at 0: nothing of it was accepted
    let cut = a.cut_of(&room, &pen.id()).unwrap();
    assert_eq!(cut.seq, 0);
    a.remove_human_devices(&[cut], now()).unwrap();
    post_ok(&mut hub, &mut a);
    sync_all(&hub, &mut a);
    assert_eq!(held(&a, &room, NAME), None);
    // b writes the name with a lower lamport than the dropped value: it stands, nothing masks it
    write(
        &mut hub,
        &mut b,
        &Draft::Register {
            group: room,
            name: NAME.into(),
            value: Some(json(r#"{"name":"b"}"#)),
        },
    );
    for got in sync_all(&hub, &mut a) {
        if got.register.as_ref().is_some_and(|r| r.name == NAME) {
            assert!(
                got.register.as_ref().unwrap().current,
                "no overlay value of the cut sender stands above it"
            );
        }
        shown.take(&got);
    }
    assert_eq!(held(&a, &room, NAME).as_deref(), Some("b"));
    assert_eq!(shown.get(NAME).as_deref(), Some("b"));
}

/// Codex finding 3: an overlay value whose number the chain fills otherwise (another envelope there, or a void
/// record) is dropped, and what stands for the name then is shown again.
#[test]
fn a_dropped_overlay_value_gives_way_to_what_stands() {
    // another envelope under its number
    let mut f = forged();
    let room = f.room;
    let mut fork = f.pen.fork();
    let shown_value = f.register(1, NAME, Some("shown"), 5);
    let stored = f.post(&shown_value);
    let mut shown = Shown::new();
    shown.take(&f.fetch(&stored));
    assert_eq!(shown.get(NAME).as_deref(), Some("shown"));
    let other = fork.sign(&f.claim(), &board_item(), &Objects::new(), now());
    let got = f.slip(&other);
    assert_eq!(
        got.provisional,
        Some(Confirmation::Dropped(Error::HashMismatch))
    );
    assert_eq!(got.shown.len(), 1);
    assert!(got.shown[0].now.is_none());
    shown.take(&got);
    assert_eq!(shown.get(NAME), None);
    assert_eq!(held(&f.reader, &room, NAME), None);

    // a void record under its number, with an older value of the chain to fall back to
    let mut f = forged();
    let room = f.room;
    let old = f.register(1, NAME, Some("old"), 1);
    let voided = f.register(1, NAME, Some("voided"), 5);
    let (old, voided_stored) = (f.post(&old), f.post(&voided));
    let mut shown = Shown::new();
    shown.take(&f.fetch(&voided_stored));
    let got = f.read(&old.bytes, old.change, None);
    assert!(!got.register.as_ref().unwrap().current);
    shown.take(&got);
    assert_eq!(shown.get(NAME).as_deref(), Some("voided"));
    let pruned = voided.prune().unwrap().encode().unwrap();
    let got = f.read(&pruned, voided_stored.change, Some(&Error::TooLarge));
    assert_eq!(got.outcome, EnvelopeOutcome::Void);
    assert!(matches!(got.provisional, Some(Confirmation::Dropped(_))));
    assert_eq!(got.shown.len(), 1);
    assert!(got.shown[0].now.is_some());
    shown.take(&got);
    assert_eq!(shown.get(NAME).as_deref(), Some("old"));
    assert_eq!(held(&f.reader, &room, NAME).as_deref(), Some("old"));
}

/// Codex finding 4: an overlay value counts its lamport against the chains' largest one, never against values
/// of the overlay; what it shows is what the chain then decides, and nothing shown is rolled back.
#[test]
fn an_overlay_value_counts_its_lamport_against_the_chains() {
    let mut f = forged();
    let room = f.room;
    let old = f.register(1, NAME, Some("old"), 1);
    let high = f.register(1, NAME, Some("high"), 2 * J);
    let booster = f.register(2, OTHER, Some("booster"), J);
    let (old, high, booster) = (f.post(&old), f.post(&high), f.post(&booster));
    let mut shown = Shown::new();
    shown.take(&f.read(&old.bytes, old.change, None));
    // the booster first, then the high one, out of the order: the high one counts as 0 against the chains
    shown.take(&f.fetch(&booster));
    let got = f.fetch(&high);
    assert!(!got.register.as_ref().unwrap().current);
    shown.take(&got);
    // the chain: the high one counts 0 at its place, the booster after it
    for stored in [&high, &booster] {
        let got = f.read(&stored.bytes, stored.change, None);
        shown.take(&got);
    }
    // an envelope behind later ones builds the registers again
    let late = f.register(3, "desk/DDDDDDDDDDDDDDDDDDDDDA", Some("late"), 4);
    let got = f.read(&late.encode().unwrap(), 1, None);
    assert!(got.replayed);
    shown.take(&got);
    assert_eq!(held(&f.reader, &room, NAME).as_deref(), Some("old"));
    assert_eq!(
        shown.history(NAME),
        vec![Some("old".into())],
        "never high, never back"
    );
}

/// Codex finding 5: an overlay value binds no register id of its writer: a writer's id used for two names out of
/// the order does not make its chain's value bad-format.
#[test]
fn an_overlay_value_binds_no_register_id() {
    let mut f = forged();
    let room = f.room;
    let first = f.register(1, NAME, Some("first"), 1);
    let clash = f.register(1, OTHER, Some("clash"), 2);
    let (first, clash) = (f.post(&first), f.post(&clash));
    let mut shown = Shown::new();
    let got = f.fetch(&clash);
    assert!(got.register.as_ref().unwrap().current);
    shown.take(&got);
    assert_eq!(shown.get(OTHER).as_deref(), Some("clash"));
    let got = f.read(&first.bytes, first.change, None);
    assert_eq!(got.outcome, EnvelopeOutcome::Applied);
    assert!(got.register.as_ref().unwrap().current);
    shown.take(&got);
    // the chain refuses the second use of the id: the clash goes from what is shown
    let got = f.read(&clash.bytes, clash.change, None);
    assert_eq!(got.code, Some(Error::BadFormat));
    shown.take(&got);
    assert_eq!(held(&f.reader, &room, NAME).as_deref(), Some("first"));
    assert_eq!(held(&f.reader, &room, OTHER), None);
    assert_eq!(shown.get(NAME).as_deref(), Some("first"));
    assert_eq!(shown.get(OTHER), None);
}

/// An agent device keeps no overlay: a register of its session read out of the order is shown provisional
/// without a register change, and counts only when its chain reaches it.
#[test]
fn an_agent_device_takes_a_register_only_from_its_chain() {
    let mut w = world();
    let session = w.group;
    sync_all(&w.hub, &mut w.agent);
    let cursor = w.agent.cursor();
    write(
        &mut w.hub,
        &mut w.human,
        &Draft::Register {
            group: session,
            name: "goals".into(),
            value: Some(json(r#"{"goals":"ship"}"#)),
        },
    );
    let stored = w
        .hub
        .changes_after(cursor)
        .into_iter()
        .find_map(|c| match c {
            Change::Envelope(stored) => Some(stored),
            _ => None,
        })
        .unwrap();
    let got = w
        .agent
        .receive_envelope(&stored.bytes, stored.change, false, None, now())
        .unwrap();
    assert_eq!(got.outcome, EnvelopeOutcome::Provisional);
    assert_eq!(got.register, None);
    assert!(got.shown.is_empty());
    assert_eq!(w.agent.register(&session, "goals").unwrap(), None);
    for got in sync_all(&w.hub, &mut w.agent) {
        assert!(got.shown.is_empty());
    }
    assert!(w.agent.register(&session, "goals").unwrap().is_some());
}
