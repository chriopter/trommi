//! Presence at a stream's opening: a human device whose page was loaded fresh is told at once who is here, with
//! each agent's link report while its lease runs, instead of waiting for the next change of presence.

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
    let mut told = std::collections::HashMap::new();
    while let Some(e) = page.next(Duration::from_secs(2)) {
        if e.name == "presence" {
            told.insert(e.data["device"].as_str().unwrap().to_string(), e.data.clone());
        }
        if told.contains_key(&b64(&agent.id())) && told.contains_key(&b64(&quiet.id())) {
            break;
        }
    }
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
