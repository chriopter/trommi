//! Only active sessions are loaded (hub-api.md point 46): `active=1` leaves archived sessions out of a catch-up,
//! the Desk and the list of groups; `archived=1` and `group=` load them when they are opened. Measured on a room
//! like the owner's: one main session, 40 helper sessions of which 36 are archived.

mod common;

use common::*;
use serde_json::Value;
use trommi_hub::util::{b64, random};

const HELPERS: usize = 40;
const ARCHIVED: usize = 36;
const CHATS: usize = 6;
const REGISTERS: usize = 4;

/// A full catch-up from the start, page by page as the apps read it: (requests, bytes, envelopes, log items).
fn catch_up(w: &World, dev: &Dev, query: &str) -> (usize, usize, usize, usize) {
    let (mut after, mut requests, mut bytes, mut envelopes, mut log) = (0i64, 0, 0, 0, 0);
    loop {
        let reply = dev.get(
            &w.hub,
            &format!("/v1/changes?after={after}&limit=200{query}"),
        );
        assert_eq!(
            reply.status,
            200,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        requests += 1;
        bytes += reply.body.len();
        let page: Value = reply.json();
        for item in page["items"].as_array().unwrap() {
            if item["kind"] == "envelope" {
                envelopes += 1;
            } else {
                log += 1;
            }
        }
        after = page["change"].as_i64().unwrap();
        if page["more"] != true {
            return (requests, bytes, envelopes, log);
        }
    }
}

fn bytes(w: &World, dev: &Dev, path: &str) -> (usize, Value) {
    let reply = dev.get(&w.hub, path);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    (reply.body.len(), reply.json())
}

#[test]
fn only_active_sessions_are_loaded_and_archived_ones_when_opened() {
    // (helper sessions of before, as the owner's room still holds them: a hub serves them, it founds none since
    //  16 October 2026 but for a test of them)
    let mut w = World::on(TestHub::start_with(&[
        ("HUB_LIMIT_ENVELOPES_PER_SECOND", "100000"),
        ("HUB_HELPER_SESSIONS", "1"),
    ]));
    let mut agent = w.enrol_agent();
    let (session, _main) = w.found_main(&mut [], Some(&mut agent));
    let humans = [w.ada.id()];
    let text = "x".repeat(400);
    let mut helpers = vec![];
    for _ in 0..HELPERS {
        let (group, reply, _) = w.found_helper(&mut agent, &session, &humans, &[]);
        reply.ok();
        let helper_session: [u8; 16] = group[32..].try_into().unwrap();
        for _ in 0..CHATS {
            agent
                .send(&w.hub, &group, &chat(&helper_session, w.ada.id(), &text))
                .ok();
        }
        for _ in 0..REGISTERS {
            agent.send(&w.hub, &group, &register(&random(), &text)).ok();
        }
        helpers.push(group);
    }
    for group in &helpers[..ARCHIVED] {
        agent
            .post(
                &w.hub,
                &format!("/v1/groups/{}/archive", b64(group)),
                &serde_json::json!({}),
            )
            .ok();
    }

    // before: everything; after: the live sessions' envelopes, and every group's Commits
    let all = catch_up(&w, &w.ada, "");
    let active = catch_up(&w, &w.ada, "&active=1");
    let per = CHATS + REGISTERS;
    assert_eq!(all.2 - active.2, ARCHIVED * per, "{all:?} {active:?}");
    assert_eq!(
        all.3, active.3,
        "the log (Commits, MLS messages) stays whole"
    );
    // one archived session, when it is opened: its envelopes alone, nothing of the log
    let one = catch_up(&w, &w.ada, &format!("&group={}", b64(&helpers[0])));
    assert_eq!((one.2, one.3), (per, 0), "{one:?}");
    // a live one the same way
    assert_eq!(
        catch_up(
            &w,
            &w.ada,
            &format!("&group={}", b64(&helpers[HELPERS - 1]))
        )
        .2,
        per
    );

    // the Desk: no register of an archived session; those come with archived=1
    let (desk_all, d_all) = bytes(&w, &w.ada, "/v1/desk");
    let (desk_active, d_active) = bytes(&w, &w.ada, "/v1/desk?active=1");
    let (desk_archived, d_archived) = bytes(&w, &w.ada, "/v1/desk?archived=1");
    let registers = |d: &Value| d["registers"].as_array().unwrap().len();
    assert_eq!(
        registers(&d_all) - registers(&d_active),
        ARCHIVED * REGISTERS
    );
    assert_eq!(registers(&d_archived), ARCHIVED * REGISTERS);
    assert!(d_active["groups"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["live"] == true));
    assert!(d_archived["groups"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["live"] == false));

    // the list of groups
    let (groups_all, l_all) = bytes(&w, &w.ada, &format!("/v1/rooms/{}/groups", b64(&w.room)));
    let (groups_active, l_active) = bytes(
        &w,
        &w.ada,
        &format!("/v1/rooms/{}/groups?active=1", b64(&w.room)),
    );
    assert_eq!(
        l_all.as_array().unwrap().len() - l_active.as_array().unwrap().len(),
        ARCHIVED
    );
    let paged = bytes(
        &w,
        &w.ada,
        &format!("/v1/rooms/{}/groups?active=1&limit=5", b64(&w.room)),
    )
    .1;
    assert!(paged["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|g| g["live"] == true));

    // an agent device: the same filter over its own groups
    let agent_all = catch_up(&w, &agent, "");
    let agent_active = catch_up(&w, &agent, "&active=1");
    assert_eq!(agent_all.2 - agent_active.2, ARCHIVED * per);

    // one at most; group only on a catch-up
    w.ada
        .get(&w.hub, "/v1/changes?active=1&group=AAAA")
        .refused(400, "bad-format");
    w.ada
        .get(&w.hub, "/v1/desk?active=1&archived=1")
        .refused(400, "bad-format");
    w.ada
        .get(&w.hub, &format!("/v1/desk?group={}", b64(&helpers[0])))
        .refused(400, "bad-format");
    w.ada
        .get(&w.hub, "/v1/changes?active=2")
        .refused(400, "bad-format");

    eprintln!(
        "MEASURE {HELPERS} helper sessions, {ARCHIVED} archived, {per} envelopes each (400-byte payloads)\n\
         MEASURE catch-up from 0: {} requests, {} bytes, {} envelopes, {} log items -> active=1: {} requests, {} bytes, {} envelopes, {} log items\n\
         MEASURE desk: {desk_all} bytes -> active=1: {desk_active} bytes (archive opened: archived=1 {desk_archived} bytes)\n\
         MEASURE groups: {groups_all} bytes -> active=1: {groups_active} bytes\n\
         MEASURE one archived session opened (group=): {} requests, {} bytes",
        all.0, all.1, all.2, all.3, active.0, active.1, active.2, active.3, one.0, one.1
    );
    assert!(active.1 * 3 < all.1, "{all:?} {active:?}");
}
