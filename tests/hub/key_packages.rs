//! A session founded while a human device has no KeyPackage at the hub (spec/v1.md 5.2.5): the hub's clock is
//! moved past the 90 days a KeyPackage is handed out. A test binary of its own: the clock belongs to the whole
//! process.

mod common;

use common::*;
use serde_json::json;
use trommi_hub::util::{b64, unb64};

const DAY: i64 = 86_400_000;

#[test]
fn a_session_is_founded_without_a_human_device_the_hub_holds_no_key_package_of() {
    let mut w = World::new();
    let mut bea = w.add_human();
    let (mut first, mut second) = (w.enrol_agent(), w.enrol_agent());
    w.catch_up(&mut bea, &w.room.clone());
    // bea is away for longer than a KeyPackage is handed out (90 days); the others come back
    w.hub.clock(91 * DAY);
    let room = w.room;
    for d in [&mut w.ada, &mut first, &mut second] {
        d.sign_in(&w.hub, &room).ok();
    }
    for agent in [&mut first, &mut second] {
        agent.link(&w.hub).ok();
    }
    first.upload_key_packages(&w.hub, 1).ok();
    second.upload_key_packages(&w.hub, 1).ok();
    // 5.2.5: a partial claim names bea in `missing`; the founding leaves bea out and is taken
    let claimed = w
        .ada
        .post(
            &w.hub,
            "/v1/key-packages/claim",
            &json!({ "devices": [b64(&bea.id()), b64(&first.id())], "partial": true }),
        )
        .ok();
    assert_eq!(claimed["missing"], json!([b64(&bea.id())]));
    let of_agent = unb64(claimed["key_packages"][b64(&first.id())].as_str().unwrap()).unwrap();
    let (group, reply, _) = w.found_main_adding(vec![of_agent]);
    reply.ok();
    // once bea has a KeyPackage again, a founding that leaves bea out is refused: claim again
    bea.sign_in(&w.hub, &room).ok();
    bea.upload_key_packages(&w.hub, 1).ok();
    let of_agent = w.claim(&w.ada, &[second.id()]);
    let (_, reply, _) = w.found_main_adding(of_agent);
    reply.refused(400, "incomplete");
    // and a human device of the group adds bea (5.2.7)
    let adds = w.claim(&w.ada, &[bea.id()]);
    let now = w.ada.room_now();
    let out = w.ada.commit(
        &group,
        &Change {
            adds,
            ..Default::default()
        },
        now,
    );
    let key = w.ada.sealed_key(&group, out.epoch + 1, &out.group_info, now.0, &w.recovery.hpke_public, true);
    w.ada.post_commit(&w.hub, &out, &key).ok();
}

