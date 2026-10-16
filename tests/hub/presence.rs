//! Presence at a stream's opening: a human device whose page was loaded fresh is told at once who is here, with
//! each agent's link report while its lease runs, instead of waiting for the next change of presence; a `here`
//! event ends the list.

mod common;

use common::*;
use serde_json::json;
use std::time::Duration;
use trommi_hub::util::{b64, random};

#[test]
fn a_new_stream_of_a_human_device_is_told_who_is_here_at_once() {
    let mut w = World::on(TestHub::start());
    let mut agent = w.enrol_agent();
    let quiet = w.enrol_agent();
    w.found_main(&mut [], Some(&mut agent));
    let hub = &w.hub;
    // the agent is here: its stream is open and its connector reported its link under a lease
    let lease = agent.post(hub, "/v1/link", &json!({ "process": b64(&random::<16>()), "hears": true, "working": true, "last_call_at": 1234 }));
    assert_eq!(lease.status, 200);
    agent.lease = lease.json()["generation"].as_u64();
    let mut agent_stream = agent.events(hub, None);
    assert_eq!(agent_stream.status, 200);
    // another agent device, idle under the lease its enrolment took (hears, not working)
    let quiet_stream = quiet.events(hub, None);
    assert_eq!(quiet_stream.status, 200);

    // a page loaded fresh: its first events say who is here, nothing else had to happen
    let mut page = w.ada.events(hub, None);
    assert_eq!(page.status, 200);
    // (then one `here` ends the list: a device it did not name is not here)
    let mut told = std::collections::HashMap::new();
    let mut whole = false;
    while let Some(e) = page.next(Duration::from_secs(2)) {
        if e.name == "presence" {
            told.insert(e.data["device"].as_str().unwrap().to_string(), e.data.clone());
        }
        if e.name == "here" {
            whole = true;
            break;
        }
    }
    assert!(whole, "the list ends with a `here` event");
    let a = &told[&b64(&agent.id())];
    assert_eq!(a["online"], true);
    assert_eq!(a["hears"], true, "the lease's link report comes with it: {a}");
    assert_eq!(a["working"], true);
    assert_eq!(a["last_call_at"], 1234);
    let q = &told[&b64(&quiet.id())];
    assert_eq!(q["online"], true);
    assert_eq!((q["hears"].clone(), q["working"].clone()), (json!(true), json!(false)), "its own report: {q}");
    // (an agent device's own stream is told nothing of the others: presence is for the human devices)
    let mut others = 0;
    while let Some(e) = agent_stream.next(Duration::from_millis(500)) {
        if e.name == "presence" && e.data["device"] != b64(&agent.id()) && e.data["device"] != b64(&quiet.id()) {
            others += 1;
        }
    }
    assert_eq!(others, 0);
    drop(quiet_stream);
}

#[test]
fn public_departures_require_the_exact_devices_signature_and_replay_without_changing_membership() {
    let mut w = World::new();
    let other = w.add_human();
    let agent = w.enrol_agent();
    let path = format!("/v1/rooms/{}/departures", b64(&w.room));
    let proof = other.sign(trommi_wire::signin::LABEL_DEVICE_DEPARTURE, &trommi_wire::signin::departure_content(&w.room, &other.id()));
    let payload = json!({"device":b64(&other.id()),"signature":b64(&proof)});
    let bad = w.hub.post(&path, &json!({"device":b64(&other.id()),"signature":b64(&[0u8;64])}));
    assert_eq!(bad.json()["error"], "bad-signature");
    let agent_proof = agent.sign(trommi_wire::signin::LABEL_DEVICE_DEPARTURE, &trommi_wire::signin::departure_content(&w.room, &agent.id()));
    let bad = w.hub.post(&path, &json!({"device":b64(&agent.id()),"signature":b64(&agent_proof)}));
    assert_eq!(bad.json()["error"], "forbidden");
    let mut live = w.ada.events(&w.hub, None);
    while let Some(event) = live.next(Duration::from_secs(2)) { if event.name == "here" { break; } }
    assert_eq!(w.hub.post(&path, &payload).status, 200, "no bearer is needed for an already device-signed own request");
    let mut received = false;
    while let Some(event) = live.next(Duration::from_secs(2)) {
        if event.name == "departure" { assert_eq!(event.data, payload); received = true; break; }
    }
    assert!(received, "the current human app receives the public proof");
    assert_eq!(w.hub.post(&path, &payload).status, 200, "replay is idempotent");
    let in_room: bool = w.hub.app.db.read(|c| c.query_row("SELECT removed_epoch IS NULL FROM devices WHERE room_id=?1 AND device=?2", rusqlite::params![&w.room[..],&other.id()[..]], |r| r.get(0))).unwrap();
    assert!(in_room, "the hub relays intent; only a verified human Commit removes membership");
    drop(live);
    let mut later = w.ada.events(&w.hub, None);
    let mut replayed = 0;
    while let Some(event) = later.next(Duration::from_secs(2)) {
        if event.name == "departure" { assert_eq!(event.data, payload); replayed += 1; }
        if event.name == "here" { break; }
    }
    assert_eq!(replayed, 1, "a later human app gets one persisted proof");
    let unknown = Dev::new();
    let signature = unknown.sign(trommi_wire::signin::LABEL_DEVICE_DEPARTURE, &trommi_wire::signin::departure_content(&w.room,&unknown.id()));
    assert_eq!(w.hub.post(&path, &json!({"device":b64(&unknown.id()),"signature":b64(&signature)})).status, 200);
    let count: i64 = w.hub.app.db.read(|c| c.query_row("SELECT count(*) FROM devices WHERE room_id=?1 AND device=?2",rusqlite::params![&w.room[..],&unknown.id()[..]],|r|r.get(0))).unwrap();
    assert_eq!(count,0,"a request racing its own join never enrolls an unknown device");
}

#[test]
fn unknown_departures_are_bounded_expire_and_cannot_block_a_members_own_logout() {
    let w = World::new();
    let observer = trommi_hub::observer::MlsObserver::default();
    let unknown = Dev::new();
    let proof = unknown.sign(trommi_wire::signin::LABEL_DEVICE_DEPARTURE,&trommi_wire::signin::departure_content(&w.room,&unknown.id()));
    let own = w.ada.sign(trommi_wire::signin::LABEL_DEVICE_DEPARTURE,&trommi_wire::signin::departure_content(&w.room,&w.ada.id()));
    w.hub.app.db.write(|c| {
        for n in 0..1000u32 {
            let mut id = [0x55;32]; id[..4].copy_from_slice(&n.to_le_bytes());
            c.execute("INSERT INTO device_departures(room_id,device,signature,at) VALUES (?1,?2,?3,1000)",rusqlite::params![&w.room[..],&id[..],&[0u8;64][..]])?;
        }
        let error = trommi_hub::devices::depart(c,&observer,&w.room,&unknown.id(),&proof,2000).unwrap_err();
        assert_eq!(error.code,"busy");
        assert!(trommi_hub::devices::depart(c,&observer,&w.room,&w.ada.id(),&own,2000)?);
        assert_eq!(trommi_hub::devices::departures(c,&w.room,3_602_000)?.len(),1,"only the enrolled member's proof survives the unknown-key lifetime");
        assert!(trommi_hub::devices::depart(c,&observer,&w.room,&unknown.id(),&proof,3_602_000)?);
        assert_eq!(trommi_hub::devices::departures(c,&w.room,3_602_000)?.len(),2);
        Ok::<_,trommi_hub::error::Refused>(())
    }).unwrap();
}
