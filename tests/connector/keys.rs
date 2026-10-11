//! Key requests (spec/v1.md 7.1.1) against the real hub: a human device let in by link that did not take its key
//! handover before the hub swept it (application messages are kept 30 days, hub-api.md point 10) asks the human
//! devices for the keys; the other human device answers with a handover in the room group, and the newcomer reads
//! what was written before it came.

mod common;

use common::*;
use serde_json::json;
use trommi_core::device::KEY_REQUEST_GRACE_MS;
use trommi_core::ids::GroupId;

const DAY: i64 = 86_400_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_device_that_missed_its_handover_asks_and_reads_what_came_before() {
    let hub = HubProc::start().await;
    let mut ada = Human::found(&hub.url).await;
    let room = GroupId::room(ada.room);
    let desk = json!({ "name": "Work", "created_at": 1 });
    ada.set_register(&room, "desk/work", Some(desk.clone()))
        .await
        .expect("a desk is written");

    // The phone is let in by link; the invite's handover goes out, the phone does not come online.
    let mut phone = ada.add_human().await;
    ada.follow_invites().await;
    // 31 days on, the hub sweeps the old application messages: the handover is gone, the Commits stay.
    let test = trommi_connector::hub::Hub::new(&hub.url, None, None).unwrap();
    test.open_call(
        reqwest::Method::POST,
        "/v1/__test/clock",
        Some(&json!({ "advance_ms": 31 * DAY })),
    )
    .await
    .expect("the clock moves");
    test.open_call(
        reqwest::Method::POST,
        "/v1/__test/retention",
        Some(&json!({})),
    )
    .await
    .expect("the hub sweeps");

    phone.sync().await;
    phone.learn(&room).await;
    assert!(
        phone.read_again().await > 0,
        "the desk does not open without its key"
    );
    assert_eq!(phone.register(&room, "desk/work").await, None);

    // Five minutes later the key is still missing: the phone asks, once.
    let at = trommi_connector::util::now_ms() + KEY_REQUEST_GRACE_MS;
    assert_eq!(phone.ask_for_keys(at).await, vec![room]);
    assert!(phone.ask_for_keys(at + 1000).await.is_empty());

    // Ada answers from the hub's list; the phone takes the keys and reads again.
    ada.sync().await;
    assert!(ada.answer_key_requests().await > 0, "a handover is sent");
    // (the same wish seen again within the hour is not answered twice)
    assert_eq!(ada.answer_key_requests().await, 0);
    phone.sync().await;
    assert_eq!(phone.read_again().await, 0, "everything opens");
    assert_eq!(phone.register(&room, "desk/work").await, Some(desk));
}
