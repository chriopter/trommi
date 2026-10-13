//! Real devices on OpenMLS (sections 4 to 9) against the hub in memory: a room with two human devices and an
//! agent in its main session. The fuzzer drives what they write (cards, versions, answers, take backs,
//! requests, verdicts, notes, registers, board items, Chat), when each one reads the hub, and the Commits of
//! the groups (updates, a forced update); and between that, what a device is handed that the hub never sent
//! in that order: a log entry twice, an entry before the one in front of it, an entry with a flipped byte,
//! an envelope twice or flipped, and a device of no room that is handed everything.
//!
//! Properties:
//! - nothing panics, and a device that was handed something odd still reads the hub's log to its end
//!   afterwards (a refusal changes nothing; a replay is a duplicate);
//! - the outsider never opens a body;
//! - at the end both human devices hold the same objects and registers of the room and the session, and
//!   each device's cursor is at the hub's last change.

use super::Bytes;
use crate::content::Dice;
use crate::hub::content::Change;
use crate::hub::Hub;
use crate::{
    add_human, enrol, found_main, found_room, json, new_device, now, post_all, process,
    publish_some, take_welcomes, TestDevice,
};
use trommi_core::device::{log_finding, Draft, LogFinding};
use trommi_core::envelope::{ObjectState, ObjectType, Urgency};
use trommi_core::ids::{BoardId, GroupId, Hash32, SessionId};

struct World {
    hub: Hub,
    room: GroupId,
    session: SessionId,
    group: GroupId,
    /// Human 1, human 2, the agent.
    devices: Vec<TestDevice>,
    /// A device of no room.
    outsider: TestDevice,
}

fn world() -> World {
    let mut human = new_device();
    let mut other = new_device();
    let mut agent = new_device();
    let (mut hub, room) = found_room(&mut human);
    publish_some(&mut hub, &mut agent, 4);
    add_human(&mut hub, &mut human, &mut other);
    publish_some(&mut hub, &mut other, 4);
    enrol(&mut hub, &mut human, &mut agent);
    let group = found_main(&mut hub, &mut human, &agent.id());
    let mut devices = vec![human, other, agent];
    for device in &mut devices {
        sync(&hub, device);
    }
    World {
        hub,
        room,
        session: group.session_id().expect("a session"),
        group,
        devices,
        outsider: new_device(),
    }
}

/// Reads the hub's changes after the device's cursor, in order. A refusal of the log is allowed only as a
/// duplicate (it was handed the entry before); an envelope is never refused with `Err`.
fn sync(hub: &Hub, device: &mut TestDevice) {
    for change in hub.changes_after(device.cursor()) {
        match change {
            Change::Log(item) => {
                if let Err(error) = process(device, &item) {
                    assert_eq!(
                        log_finding(&error),
                        LogFinding::Duplicate,
                        "a log entry of the hub's own order does not process: {error:?}"
                    );
                }
                take_welcomes(hub, device, item.change);
            }
            Change::Envelope(stored) => {
                device
                    .receive_envelope(
                        &stored.bytes,
                        stored.change,
                        true,
                        stored.void_code.as_ref(),
                        now(),
                    )
                    .expect("the device takes the envelope the hub serves");
            }
        }
    }
}

fn payload(title: &str, previous: &Hash32, version: u64) -> trommi_core::crypto::SecretBytes {
    json(&format!(
        r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":{version},"previous_version_hash":"{}","text":"{title}"}}"#,
        previous.to_base64url()
    ))
}

const NAMES: [&str; 8] = [
    "crown",
    "desk/a",
    "session/x",
    "snooze/y",
    "goals",
    "profile",
    "status_line/s",
    "heard",
];

pub fn run(data: &[u8]) {
    let mut dice = Dice(Bytes::new(data));
    let mut w = world();
    for _ in 0..24 {
        let Ok(op) = dice.below(20) else { break };
        let who = dice.below(3).unwrap_or(0) as usize;
        let in_group = if dice.one_in(2).unwrap_or(false) {
            w.group
        } else {
            w.room
        };
        let objects = w.devices[who].objects(&w.group).unwrap_or_default();
        let notes = w.devices[who].objects(&w.room).unwrap_or_default();
        let pick =
            |dice: &mut Dice<Bytes>,
             list: &[(trommi_core::ids::ObjectId, trommi_core::objects::Object)]| {
                (!list.is_empty())
                    .then(|| list[dice.below(list.len() as u64).unwrap_or(0) as usize].clone())
            };
        let urgency = Urgency::from_byte(dice.below(4).unwrap_or(0) as u8).expect("0..4");
        let debug = std::env::var_os("FUZZ_DEBUG").is_some();
        if debug {
            eprintln!(
                "devices: op {op} by {who} in {}",
                if in_group == w.group {
                    "session"
                } else {
                    "room"
                }
            );
        }
        let draft = match op {
            0 | 1 => Some(Draft::CardFirst {
                session: w.session,
                urgency,
                push: dice.one_in(2).unwrap_or(false),
                payload: payload("a", &Hash32::ZERO, 1),
            }),
            2 => pick(&mut dice, &objects).map(|(id, o)| Draft::CardVersion {
                session: w.session,
                object_id: id,
                closed: dice.one_in(3).unwrap_or(false),
                urgency,
                push: false,
                payload: payload("b", &o.current, 2),
            }),
            3 | 4 => pick(&mut dice, &objects).map(|(id, o)| Draft::Answer {
                session: w.session,
                object_id: id,
                choices: vec![if dice.one_in(2).unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                }
                .to_owned()],
                closes: dice.one_in(2).unwrap_or(false),
                payload: json(&format!(
                    r#"{{"version_hash":"{}"}}"#,
                    o.current.to_base64url()
                )),
            }),
            5 => pick(&mut dice, &objects)
                .filter(|(_, o)| o.state != ObjectState::Open)
                .map(|(id, _)| Draft::TakeBack {
                    session: w.session,
                    object_id: id,
                    payload: json("{}"),
                }),
            6 => Some(Draft::PermissionRequest {
                session: w.session,
                urgency,
                expires_at: now() + dice.below(600_000).unwrap_or(0),
                push: false,
                payload: json(r#"{"tool_name":"Bash","description":"ls"}"#),
            }),
            7 => pick(&mut dice, &objects)
                .filter(|(_, o)| o.object_type == ObjectType::Request)
                .map(|(id, _)| Draft::Verdict {
                    session: w.session,
                    request_id: id,
                    allow: dice.one_in(2).unwrap_or(false),
                    payload: json("{}"),
                }),
            8 => Some(Draft::NoteFirst {
                payload: payload("n", &Hash32::ZERO, 1),
            }),
            9 => pick(&mut dice, &notes).map(|(id, o)| Draft::NoteVersion {
                object_id: id,
                closed: dice.one_in(4).unwrap_or(false),
                payload: payload("m", &o.current, 2),
            }),
            10 | 11 => {
                let name = NAMES[dice.below(NAMES.len() as u64).unwrap_or(0) as usize].to_owned();
                let value = (!dice.one_in(5).unwrap_or(false)).then(|| {
                    json(&format!(
                        r#"{{"v":{},"goals":"g"}}"#,
                        dice.below(9).unwrap_or(0)
                    ))
                });
                Some(Draft::Register {
                    group: in_group,
                    name,
                    value,
                })
            }
            12 => Some(Draft::BoardItem {
                board: BoardId::new([dice.below(2).unwrap_or(0) as u8; 16]),
                payload: json(
                    r#"{"schema_version":2,"content_type":"erase","shape_ids":["AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA/3/0"]}"#,
                ),
            }),
            13 => Some(Draft::SessionChat {
                session: w.session,
                payload: json(r#"{"text":"hi"}"#),
            }),
            14 => {
                // a device reads the hub
                sync(&w.hub, &mut w.devices[who]);
                None
            }
            15 => {
                // a human device updates its leaf in a group
                let forced = dice.one_in(2).unwrap_or(false);
                // (the device reads the hub first: after an accepted Commit, post_all hands it the log up to the
                // Commit, and an envelope in between would be passed by)
                sync(&w.hub, &mut w.devices[who]);
                if who < 2 && w.devices[who].update(&in_group, forced, now()).is_ok() {
                    let _ = post_all(&mut w.hub, &mut w.devices[who]);
                }
                None
            }
            _ => {
                odd(&mut w, &mut dice, who);
                None
            }
        };
        if let Some(draft) = draft {
            match w.devices[who].seal(&draft, None, &[], now()) {
                Ok(_) => {
                    let posted = post_all(&mut w.hub, &mut w.devices[who]);
                    if debug {
                        eprintln!("devices:   posted {posted:?}");
                    }
                }
                Err(e) if debug => eprintln!("devices:   not sealed: {e:?}"),
                Err(_) => {}
            }
        }
    }
    for device in &mut w.devices {
        sync(&w.hub, device);
    }
    let last = w.hub.changes_after(0).last().map_or(0, Change::change);
    for (i, device) in w.devices.iter().enumerate() {
        assert_eq!(device.cursor(), last, "device {i} did not read to the end");
    }
    let (a, b) = (&w.devices[0], &w.devices[1]);
    for group in [w.room, w.group] {
        assert_eq!(
            a.objects(&group).ok(),
            b.objects(&group).ok(),
            "the human devices hold other objects"
        );
        for name in NAMES {
            assert_eq!(
                a.register(&group, name)
                    .ok()
                    .flatten()
                    .map(|v| v.expose().to_vec()),
                b.register(&group, name)
                    .ok()
                    .flatten()
                    .map(|v| v.expose().to_vec()),
                "the human devices hold another {name}"
            );
        }
    }
}

/// Hands device `who` something the hub never sent in that order, and the outsider whatever there is.
fn odd(w: &mut World, dice: &mut Dice<Bytes>, who: usize) {
    let changes = w.hub.changes_after(0);
    if changes.is_empty() {
        return;
    }
    let at = dice.below(changes.len() as u64).unwrap_or(0) as usize;
    let flip_at = dice.below(1 << 16).unwrap_or(0) as usize;
    let flip = 1 + dice.below(255).unwrap_or(0) as u8;
    let flipped = |bytes: &[u8]| {
        let mut b = bytes.to_vec();
        if !b.is_empty() {
            let i = flip_at % b.len();
            b[i] ^= flip;
        }
        b
    };
    let device = &mut w.devices[who];
    let cursor = device.cursor();
    // what the device has read is read again, in any order; what it has not read comes only flipped (the
    // order of what is new is the caller's: 5.4.1)
    let read = changes[at].change() <= cursor;
    match (&changes[at], dice.below(4).unwrap_or(0)) {
        (Change::Log(item), 0) if read => {
            if let Err(error) = process(device, item) {
                assert_eq!(
                    log_finding(&error),
                    LogFinding::Duplicate,
                    "a replay is no duplicate: {error:?}"
                );
            }
        }
        (Change::Log(item), 1) => {
            let mut item = item.clone();
            item.bytes = flipped(&item.bytes);
            match process(device, &item) {
                Ok(_) if cursor < item.change => {
                    panic!("a log entry with a flipped byte was processed")
                }
                Ok(_) => {}
                Err(_) => assert_eq!(
                    device.cursor(),
                    cursor,
                    "a refused log entry moved the cursor"
                ),
            }
        }
        (Change::Envelope(stored), 0) => {
            let _ = device.receive_envelope(
                &stored.bytes,
                stored.change,
                read,
                stored.void_code.as_ref(),
                now(),
            );
            assert_eq!(
                device.cursor(),
                cursor,
                "an envelope read again moved the cursor"
            );
        }
        (Change::Envelope(stored), 1) => {
            let bytes = flipped(&stored.bytes);
            if let Ok(received) = device.receive_envelope(&bytes, stored.change, read, None, now())
            {
                assert!(received.body.is_none(), "a flipped envelope was opened");
            }
            assert_eq!(
                device.cursor(),
                cursor,
                "a flipped envelope moved the cursor"
            );
        }
        (_, 2) => {
            // the outsider is handed the room's log and its content
            for change in &changes {
                match change {
                    Change::Log(item) => {
                        let _ = process(&mut w.outsider, item);
                    }
                    Change::Envelope(stored) => {
                        if let Ok(received) = w.outsider.receive_envelope(
                            &stored.bytes,
                            stored.change,
                            false,
                            None,
                            now(),
                        ) {
                            assert!(received.body.is_none(), "a device of no room opened a body");
                        }
                    }
                }
            }
        }
        _ => {}
    }
}
