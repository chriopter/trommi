//! Stored content end to end (section 9): devices 1 and 2 (human), 3 (the agent; opener of the helper
//! session) and 4 (a helper) write cards, notes, answers, take backs, requests, verdicts, registers, board items
//! and Chat lines into the room, the main session and the helper session, through the hub's checks. Device 9
//! is no member. Between the honest writes come forgeries: flipped bytes, envelopes signed by a key that is not
//! the sender's, a non-member's envelopes, replays, epochs that move on.
//!
//! Properties:
//! - nothing panics;
//! - what the hub refuses is in no log; what it takes, every member device takes too, in the hub's order;
//! - a flipped byte, a foreign signature or a non-member's envelope is never taken, by the hub or a device;
//! - the two human devices, one of them lagging behind and catching up later, end with the same chains,
//!   objects, registers and boards, and the hub's chains and objects equal theirs.

use super::Bytes;
use crate::content::{shape, Dice};
use crate::room::{
    device, helper_session, key, room, session, signer, World, HELPER_SESSION, SESSION,
};
use crate::seal;
use std::collections::BTreeMap;
use trommi_core::board_items::{Board, ItemBody};
use trommi_core::chain::GroupFacts;
use trommi_core::chain::{Mode, Outcome, Receipt, Served};
use trommi_core::envelope::{
    AnswerBind, Draft, Envelope, ObjectType, Subject, TakeBackBind, Timeline, Urgency, Verdict,
    VerdictBind,
};
use trommi_core::ids::{BoardId, DeviceId, GroupId, RegisterId, SessionId};
use trommi_core::registers::Registers;
use trommi_core::Error;

/// One device's view beyond what [`World`] keeps: registers and boards from the opened bodies.
struct Device {
    world: World,
    registers: BTreeMap<GroupId, Registers>,
    boards: BTreeMap<(GroupId, BoardId), Board>,
    /// How far into the log it has read.
    read: usize,
}

impl Device {
    fn new(n: u8) -> Self {
        let mut world = World::new();
        world.me = device(n);
        Self {
            world,
            registers: BTreeMap::new(),
            boards: BTreeMap::new(),
            read: 0,
        }
    }

    fn take(&mut self, bytes: &[u8]) -> Result<Receipt, Error> {
        let receipt = self.world.take_as(bytes, &Served::Stored, Mode::InOrder)?;
        let header = &receipt.envelope().header;
        if let Outcome::Taken { body: Ok(body), .. } = receipt.outcome() {
            match header.subject {
                Subject::Register(_) => {
                    let registers = self.registers.entry(header.group).or_default();
                    if let Ok(update) = registers.judge(&self.world.fake, header, body.payload()) {
                        registers.apply(&update).expect("a judged update applies");
                    }
                }
                Subject::Item(Timeline::Board(board)) => {
                    if let Ok(item) = ItemBody::decode(body.payload()) {
                        let _ = self.boards.entry((header.group, board)).or_default().apply(
                            header.sender,
                            header.seq,
                            item,
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(receipt)
    }
}

enum Logged {
    Envelope(Vec<u8>),
    /// A Commit of the group: its next epoch, the same leaves.
    Commit(GroupId),
}

struct Flow {
    hub: World,
    log: Vec<Logged>,
    devices: Vec<Device>,
}

impl Flow {
    /// Hands the log to device `i` up to its end.
    fn catch_up(&mut self, i: usize) {
        while self.devices[i].read < self.log.len() {
            let at = self.devices[i].read;
            match &self.log[at] {
                Logged::Envelope(bytes) => {
                    let me = self.devices[i].world.me;
                    let group = Envelope::decode(bytes).expect("logged").header.group;
                    let member = self.devices[i]
                        .world
                        .fake
                        .groups
                        .get(&group)
                        .and_then(|g| g.epochs.last())
                        .is_some_and(|e| e.leaves.contains_key(&me));
                    let taken = self.devices[i].take(bytes);
                    if member {
                        if let Err(e) = taken {
                            panic!(
                                "the hub took an envelope that member device {i} refuses: {e:?}"
                            );
                        }
                    }
                }
                Logged::Commit(group) => {
                    let now = self.devices[i].world.now;
                    self.devices[i].world.fake.commit(group, now, now, |_| {});
                }
            }
            self.devices[i].read += 1;
        }
    }

    /// The hub takes `bytes` from signed-in device `n`; logged when taken.
    fn post(&mut self, n: u8, bytes: &[u8]) -> Result<(), Error> {
        let envelope = Envelope::decode(bytes)?;
        let receipt = self.hub.hub_take(n, &envelope)?;
        let _ = receipt;
        self.log.push(Logged::Envelope(bytes.to_vec()));
        // device 0 (human 1) follows the hub at once: it is the writers' view of the rules
        self.catch_up(0);
        Ok(())
    }
}

fn group_of(pick: u64) -> GroupId {
    match pick % 3 {
        0 => room(),
        1 => session(),
        _ => helper_session(),
    }
}

fn writer_of(pick: u64) -> u8 {
    [1, 2, 3, 4, 1, 2, 9][(pick % 7) as usize]
}

const NAMES: [&str; 12] = [
    "heads",
    "crown",
    "desk/a",
    "goals",
    "session/x",
    "snooze/y",
    "profile",
    "status_line/s",
    "heard",
    "join_ask/n",
    "draft/q",
    "nobody/z",
];

fn register_id(n: u8, name: &str) -> RegisterId {
    let mut bytes = [n; 16];
    for (to, from) in bytes.iter_mut().zip(name.bytes().rev()) {
        *to ^= from;
    }
    RegisterId::new(bytes)
}

fn payload<E: trommi_core::crypto::Entropy>(
    dice: &mut Dice<E>,
    extra: serde_json::Value,
) -> Vec<u8> {
    let mut value = serde_json::json!({ "schema_version": 2, "title": "t", "text": "x" });
    if let (Some(to), Some(from)) = (value.as_object_mut(), extra.as_object()) {
        for (k, v) in from {
            to.insert(k.clone(), v.clone());
        }
    }
    if dice.one_in(16).unwrap_or(false) {
        // something no writer makes
        let junk: [u8; 8] = dice.bytes().unwrap_or_default();
        return junk.to_vec();
    }
    serde_json::to_vec(&value).expect("json")
}

pub fn run(data: &[u8]) {
    let mut dice = Dice(Bytes::new(data));
    let mut hub = World::new();
    hub.fake.hub = true;
    let mut flow = Flow {
        hub,
        log: Vec::new(),
        devices: vec![Device::new(1), Device::new(2), Device::new(3)],
    };
    // nonces of the writers' seals
    let mut writer = World::new();

    for _ in 0..64 {
        let Ok(op) = dice.below(16) else { break };
        let mut n = writer_of(dice.below(64).unwrap_or(0));
        let mut group = group_of(dice.below(3).unwrap_or(0));
        if !dice.one_in(4).unwrap_or(true) {
            // mostly a writer and a group the rules let write this
            let human = 1 + dice.below(2).unwrap_or(0) as u8;
            let in_session = if dice.one_in(3).unwrap_or(false) {
                helper_session()
            } else {
                session()
            };
            (n, group) = match op {
                0 | 5 => (
                    if in_session == session() {
                        3
                    } else {
                        3 + dice.below(2).unwrap_or(0) as u8
                    },
                    in_session,
                ),
                1 => (human, room()),
                3 | 4 | 6 => (human, in_session),
                9 | 10 => (human, room()),
                11 => (
                    if dice.one_in(2).unwrap_or(false) {
                        human
                    } else {
                        3
                    },
                    in_session,
                ),
                _ => (n, group),
            };
        }
        let objects = flow.devices[0].world.objects(&group);
        let listed: Vec<_> = objects.iter().map(|(id, o)| (*id, o.clone())).collect();
        let some_object = |dice: &mut Dice<Bytes>| {
            (!listed.is_empty())
                .then(|| listed[dice.below(listed.len() as u64).unwrap_or(0) as usize].clone())
        };
        let urgency = Urgency::from_byte(dice.below(4).unwrap_or(0) as u8).expect("0..4");
        let draft = match op {
            0 | 1 => {
                let kind = if op == 0 {
                    ObjectType::Card
                } else {
                    ObjectType::Note
                };
                let zero = trommi_core::ids::Hash32::ZERO.to_base64url();
                let extra =
                    serde_json::json!({ "previous_version_hash": zero, "object_version": 1 });
                Draft::first_version(kind, urgency, &payload(&mut dice, extra)).ok()
            }
            2 => some_object(&mut dice).and_then(|(id, o)| {
                if !dice.one_in(4).unwrap_or(true) {
                    n = (1..=4).find(|k| device(*k) == o.owner).unwrap_or(n);
                }
                Draft::later_version(
                    id,
                    o.object_type,
                    dice.one_in(3).unwrap_or(false),
                    urgency,
                    o.current,
                    &payload(
                        &mut dice,
                        serde_json::json!({ "previous_version_hash": o.current.to_base64url() }),
                    ),
                )
                .ok()
            }),
            3 => some_object(&mut dice).map(|(id, o)| {
                let bind = AnswerBind {
                    object_id: id,
                    version_hash: o.current,
                    choices: vec![b"yes".to_vec()],
                };
                Draft::answer(
                    bind,
                    dice.one_in(2).unwrap_or(false),
                    urgency,
                    o.owner,
                    &payload(&mut dice, serde_json::json!({})),
                )
            }),
            4 => some_object(&mut dice).and_then(|(id, o)| {
                o.answer.map(|answer| {
                    let bind = TakeBackBind {
                        object_id: id,
                        previous_hash: answer,
                        version_hash: o.current,
                    };
                    Draft::take_back(
                        bind,
                        urgency,
                        o.owner,
                        &payload(&mut dice, serde_json::json!({})),
                    )
                })
            }),
            5 => Some(Draft::request(
                urgency,
                writer.now + dice.below(120_000).unwrap_or(0),
                &payload(&mut dice, serde_json::json!({})),
            )),
            6 => some_object(&mut dice)
                .filter(|(_, o)| o.object_type == ObjectType::Request)
                .map(|(id, o)| {
                    let bind = VerdictBind {
                        request_id: id,
                        request_hash: o.current,
                        expires_at: writer.now + 60_000,
                        verdict: if dice.one_in(2).unwrap_or(false) {
                            Verdict::Allow
                        } else {
                            Verdict::Deny
                        },
                    };
                    Draft::verdict(
                        bind,
                        urgency,
                        o.owner,
                        &payload(&mut dice, serde_json::json!({})),
                    )
                }),
            7 | 8 => {
                let name = NAMES[dice.below(NAMES.len() as u64).unwrap_or(0) as usize];
                let largest = flow.devices[0]
                    .registers
                    .get(&group)
                    .map_or(0, Registers::largest_lamport);
                let lamport = match dice.below(8).unwrap_or(0) {
                    0 => dice.below(u64::MAX).unwrap_or(0),
                    1 => largest + (1 << 24) + 1,
                    2 => 0,
                    _ => largest + 1,
                };
                let value = match dice.below(4).unwrap_or(0) {
                    0 => serde_json::Value::Null,
                    1 => serde_json::json!({ "goals": "a\nb" }),
                    _ => serde_json::json!({ "v": dice.below(100).unwrap_or(0) }),
                };
                let body = serde_json::json!({ "name": name, "value": value, "lamport": lamport });
                Some(Draft::register(
                    register_id(n, name),
                    &serde_json::to_vec(&body).expect("json"),
                ))
            }
            9 | 10 => {
                let body = match dice.below(3).unwrap_or(0) {
                    0 => shape(&mut dice).ok().map(|s| ItemBody::Strokes(vec![s])),
                    _ => {
                        let target = trommi_core::board::ShapeId {
                            sender: device(1 + dice.below(2).unwrap_or(0) as u8),
                            seq: 1 + dice.below(8).unwrap_or(0),
                            index: 0,
                        };
                        Some(if dice.one_in(2).unwrap_or(false) {
                            ItemBody::Erase(vec![target])
                        } else {
                            ItemBody::Move {
                                shapes: vec![target],
                                offset: [3, -4],
                            }
                        })
                    }
                };
                body.and_then(|b| b.encode().ok()).map(|bytes| {
                    Draft::board_item(
                        BoardId::new([dice.below(2).unwrap_or(0) as u8; 16]),
                        bytes.expose(),
                    )
                })
            }
            11 => {
                let session_id: SessionId = if group == helper_session() {
                    HELPER_SESSION
                } else {
                    SESSION
                };
                let to = if n <= 2 { device(3) } else { DeviceId::ZERO };
                Some(Draft::session_chat(
                    session_id,
                    to,
                    &payload(&mut dice, serde_json::json!({})),
                ))
            }
            12 => {
                // a forgery: a byte of a logged envelope flipped
                let logged: Vec<&Vec<u8>> = flow
                    .log
                    .iter()
                    .filter_map(|l| match l {
                        Logged::Envelope(b) => Some(b),
                        Logged::Commit(_) => None,
                    })
                    .collect();
                if let Some(original) = (!logged.is_empty())
                    .then(|| logged[dice.below(logged.len() as u64).unwrap_or(0) as usize].clone())
                {
                    let mut bytes = original.clone();
                    let at = dice.below(bytes.len() as u64).unwrap_or(0) as usize;
                    let flip = 1 + dice.below(255).unwrap_or(0) as u8;
                    bytes[at] ^= flip;
                    let sender = Envelope::decode(&original).expect("logged").header.sender;
                    let n = (1..=4).find(|n| device(*n) == sender).unwrap_or(9);
                    assert!(
                        flow.hub
                            .hub_take(
                                n,
                                &match Envelope::decode(&bytes) {
                                    Ok(e) => e,
                                    Err(_) => continue,
                                }
                            )
                            .is_err(),
                        "the hub took a flipped byte at {at}"
                    );
                    assert!(
                        flow.devices[0]
                            .world
                            .take_as(&bytes, &Served::Stored, Mode::ReadingBack)
                            .is_err(),
                        "a device took a flipped byte at {at}"
                    );
                }
                None
            }
            13 => {
                // a forgery: device n's next envelope, signed by another key
                if let Ok(sealed) = writer.seal_next(
                    n,
                    group,
                    &Draft::session_chat(SESSION, DeviceId::ZERO, b"{}"),
                ) {
                    let mut envelope = sealed.envelope;
                    let other = if n == 1 { 2 } else { 1 };
                    seal::sign(&mut envelope, &signer(other)).expect("signs");
                    assert!(
                        flow.hub.hub_take(n, &envelope).is_err(),
                        "the hub took a foreign signature"
                    );
                    assert!(
                        flow.hub.hub_take(other, &envelope).is_err(),
                        "the hub took a foreign signature"
                    );
                    let bytes = envelope.encode().expect("encodes");
                    assert!(
                        flow.devices[0]
                            .world
                            .take_as(&bytes, &Served::Stored, Mode::ReadingBack)
                            .is_err(),
                        "a device took a foreign signature"
                    );
                }
                None
            }
            14 => {
                // a replay of a logged envelope
                let logged: Vec<Vec<u8>> = flow
                    .log
                    .iter()
                    .filter_map(|l| match l {
                        Logged::Envelope(b) => Some(b.clone()),
                        Logged::Commit(_) => None,
                    })
                    .collect();
                if !logged.is_empty() {
                    let bytes = &logged[dice.below(logged.len() as u64).unwrap_or(0) as usize];
                    let envelope = Envelope::decode(bytes).expect("logged");
                    let n = (1..=4)
                        .find(|n| device(*n) == envelope.header.sender)
                        .unwrap_or(9);
                    assert!(
                        flow.hub.hub_take(n, &envelope).is_err(),
                        "the hub took a replay"
                    );
                }
                None
            }
            _ => {
                // a device catches up, or an epoch ends
                match dice.below(3).unwrap_or(0) {
                    0 => flow.catch_up(1),
                    1 => flow.catch_up(2),
                    _ => {
                        let now = flow.hub.now;
                        flow.hub.fake.commit(&group, now, now, |_| {});
                        writer.fake.commit(&group, now, now, |_| {});
                        flow.log.push(Logged::Commit(group));
                        flow.catch_up(0);
                    }
                }
                None
            }
        };
        let Some(draft) = draft else { continue };
        let before = writer.own.get(&(group, n)).copied();
        // the writer's view of the rules is device 1's, which follows the hub
        writer.chains = flow.devices[0].world.chains.clone();
        writer.objects = flow.devices[0].world.objects.clone();
        let sealed = if n == 9 {
            // a non-member signs without asking the rules
            let epoch = writer
                .fake
                .processed_epoch(&group)
                .ok()
                .flatten()
                .unwrap_or(0);
            let slot = writer.slot(n, group, epoch);
            if let Ok(sealed) =
                seal::seal_at(&draft, &slot, &key(epoch), &signer(n), &mut writer.entropy)
            {
                let bytes = sealed.envelope.encode().expect("encodes");
                assert!(
                    flow.hub.hub_take(n, &sealed.envelope).is_err(),
                    "the hub took a non-member's envelope"
                );
                assert!(
                    flow.devices[0]
                        .world
                        .take_as(&bytes, &Served::Stored, Mode::ReadingBack)
                        .is_err(),
                    "a device took a non-member's envelope"
                );
            }
            Err(Error::Forbidden)
        } else {
            writer.seal_next(n, group, &draft)
        };
        let posted = match sealed {
            Ok(sealed) => {
                let bytes = sealed.envelope.encode().expect("encodes");
                flow.post(n, &bytes)
            }
            Err(e) => Err(e),
        };
        if let (Err(e), true) = (&posted, std::env::var_os("FUZZ_DEBUG").is_some()) {
            eprintln!(
                "flow: op {op} by {n} in {} refused: {e:?}",
                if group == room() {
                    "room"
                } else if group == session() {
                    "session"
                } else {
                    "helper"
                }
            );
        }
        if posted.is_err() {
            // what is not taken uses up no number
            match before {
                Some(head) => writer.own.insert((group, n), head),
                None => writer.own.remove(&(group, n)),
            };
        }
    }

    flow.catch_up(1);
    if std::env::var_os("FUZZ_DEBUG").is_some() {
        let objects: usize = flow.devices[0]
            .world
            .objects
            .values()
            .map(|o| o.iter().count())
            .sum();
        let registers = flow.devices[0].registers.len();
        let boards = flow.devices[0].boards.len();
        eprintln!(
            "flow: log {} objects {objects} register groups {registers} boards {boards}",
            flow.log.len()
        );
    }
    let (a, b) = (&flow.devices[0], &flow.devices[1]);
    assert_eq!(
        a.world.chains, b.world.chains,
        "the human devices hold other chains"
    );
    assert_eq!(
        a.world.objects, b.world.objects,
        "the human devices hold other objects"
    );
    assert_eq!(
        a.registers, b.registers,
        "the human devices hold other registers"
    );
    assert_eq!(a.boards, b.boards, "the human devices hold other boards");
    assert_eq!(
        flow.hub.chains, a.world.chains,
        "the hub's chains are not the devices'"
    );
    assert_eq!(
        flow.hub.objects, a.world.objects,
        "the hub's objects are not the devices'"
    );
}
