//! What the App hands the platform outside its window (`app::outside`): the widgets' snapshot, local notifications
//! for a card that newly waits while the app is in the back, the notification extension's context and keys.

mod common;

use common::{now, World};
use trommi_client_core::account::AccountAction;
use trommi_client_core::actions::Action;
use trommi_client_core::app::io::{HttpRequest, HttpResponse};
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Abilities, Config, Effect, Event, Notice, NotifyContext, WidgetSnapshot};
use trommi_client_core::views::Phase;

fn start(can: Abilities) -> Event {
    Event::Start { config: Config { platform: "test".into(), version: "0".into(), hub: common::hub::HUB_URL.into(), seed: 7, can, ..Default::default() } }
}

fn snapshots(sim: &Sim, from: usize) -> Vec<WidgetSnapshot> {
    sim.log[from..].iter().filter_map(|e| if let Effect::Snapshot { snapshot } = e { Some(snapshot.clone()) } else { None }).collect()
}

fn notices(sim: &Sim, from: usize) -> Vec<Notice> {
    sim.log[from..].iter().filter_map(|e| if let Effect::Notify { notice } = e { Some(notice.clone()) } else { None }).collect()
}

fn contexts(sim: &Sim, from: usize) -> Vec<Option<NotifyContext>> {
    sim.log[from..].iter().filter_map(|e| if let Effect::NotifyContext { context } = e { Some(context.clone()) } else { None }).collect()
}

#[test]
fn the_snapshot_follows_the_board_and_is_not_sent_again_unchanged() {
    let mut world = World::new();
    world.agent_asks("Ship it?");
    let mut sim = world.sim();
    sim.send(start(Abilities { snapshots: true, ..Default::default() }));
    assert_eq!(sim.view.phase, Phase::Live);
    let sent = snapshots(&sim, 0);
    assert_eq!(sent.len(), 1, "one snapshot once the room was read: {sent:?}");
    let first = &sent[0];
    assert_eq!((first.waiting, first.urgent, first.cards.len()), (1, false, 1));
    let card = &first.cards[0];
    assert_eq!((card.title.as_str(), card.kind.as_str(), card.urgency.as_str()), ("Ship it?", "decision", sim.view.desk.rows[0].urgency.as_str()));
    assert_eq!(card.options.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(), ["yes", "no"]);
    assert!(!card.session.is_empty() && !card.multiple);

    // nothing changed: nothing is sent
    let from = sim.log.len();
    sim.send(Event::Wake);
    sim.send(Event::Foreground);
    assert!(snapshots(&sim, from).is_empty());

    // a second card: one snapshot with both
    let from = sim.log.len();
    world.agent_asks("And this?");
    assert!(world.stream(&mut sim));
    let sent = snapshots(&sim, from);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].waiting, 2);
    assert_eq!(sent[0].cards.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), ["Ship it?", "And this?"], "the one that waits longest first");

    // answered here: it waits no more
    let from = sim.log.len();
    let id = card.id.clone();
    sim.send(Event::Action { action: Action::Answer { card_id: id, choices: vec!["yes".into()], note: None, attachments: vec![], option_notes: Default::default(), trusted: false, marks: vec![] } });
    let sent = snapshots(&sim, from);
    assert_eq!(sent.last().map(|s| s.waiting), Some(1), "{sent:?}");
}

#[test]
fn without_the_abilities_nothing_goes_outside() {
    let mut world = World::new();
    world.agent_asks("Ship it?");
    let mut sim = world.sim();
    sim.secrets = Some([("push-notify-key".to_string(), vec![9; 32])].into_iter().collect());
    sim.send(start(Abilities::default()));
    sim.send(Event::Background);
    world.agent_asks("Later");
    sim.send(Event::Push { payload: vec![1].into() });
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert!(!sim.log.iter().any(|e| matches!(e, Effect::Snapshot { .. } | Effect::Notify { .. } | Effect::NotifyContext { .. })));
}

#[test]
fn a_notice_only_for_a_card_that_newly_waits_while_the_app_is_in_the_back() {
    let mut world = World::new();
    world.agent_asks("Read at the start");
    let mut sim = world.sim();
    sim.send(start(Abilities { notify: true, ..Default::default() }));
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(notices(&sim, 0).is_empty(), "nothing read at the start rings");

    // in front: a new card does not ring
    let from = sim.log.len();
    world.agent_asks("Seen in front");
    assert!(world.stream(&mut sim));
    assert_eq!(sim.view.desk.rows.len(), 2);
    assert!(notices(&sim, from).is_empty());

    // in the back, a push: the new card rings, the ones before do not
    sim.send(Event::Background);
    let from = sim.log.len();
    world.agent_asks("Came while away");
    sim.send(Event::Push { payload: vec![1].into() });
    let rung = notices(&sim, from);
    assert_eq!(rung.len(), 1, "{rung:?}");
    let card = sim.view.desk.rows.iter().find(|r| r.title == "Came while away").expect("on the desk").id.clone();
    assert_eq!((rung[0].id.as_str(), rung[0].body.en.as_str(), rung[0].target.as_deref(), rung[0].urgent), (card.as_str(), "Came while away", Some(card.as_str()), false));
    assert!(!rung[0].title.en.is_empty(), "the session's name");

    // the same push again: it rang already
    let from = sim.log.len();
    sim.send(Event::Push { payload: vec![1].into() });
    assert!(notices(&sim, from).is_empty());
}

#[test]
fn an_app_launched_by_a_push_does_not_ring_for_what_it_reads_first() {
    let mut world = World::new();
    world.agent_asks("Old");
    world.agent_asks("New");
    let mut sim = world.sim();
    sim.send(start(Abilities { notify: true, ..Default::default() }));
    sim.send(Event::Push { payload: vec![1].into() });
    assert_eq!(sim.view.phase, Phase::Live);
    assert!(notices(&sim, 0).is_empty());
}

#[test]
fn the_extensions_keys_follow_an_epoch_and_go_at_sign_out() {
    let mut world = World::new();
    world.hub.borrow_mut().fault = Some(Box::new(|_, request: &HttpRequest| {
        (request.method == "GET" && request.url.ends_with("/v1/push")).then(|| {
            let body = format!(
                r#"{{"subscriptions":[{{"kind":"apns","endpoint":"e","level":"all","created_at":1}}],"vapid_public_key":"{}","apns":true}}"#,
                trommi_core::ids::base64url_encode(&[4; 65])
            );
            Ok(HttpResponse { status: 200, headers: vec![], body: body.into_bytes().into() })
        })
    }));
    let mut sim = world.sim();
    sim.secrets = Some([("push-notify-key".to_string(), vec![9; 32])].into_iter().collect());
    sim.send(start(Abilities { notify: true, push: "apns".into(), ..Default::default() }));
    assert_eq!(sim.view.phase, Phase::Live);
    let sent = contexts(&sim, 0);
    assert_eq!(sent.len(), 1, "once the push level is known");
    let first = sent[0].clone().expect("a context");
    assert_eq!((first.push_key.0.clone(), first.client.as_str(), first.hub.as_str()), (vec![9; 32], "test/0", common::hub::HUB_URL));
    assert_eq!(first.room, hex(world.room.room_id().as_bytes()));
    assert_eq!(first.keys.len(), 1, "the one session group, no room group");
    assert_eq!(first.keys[0].group, hex(world.group.as_bytes()));
    assert_eq!(first.keys[0].key.0.len(), 32);
    assert!(!format!("{first:?}").contains(&format!("{:?}", first.keys[0].key.0)), "its Debug shows no key");

    // nothing moved: nothing again
    let from = sim.log.len();
    sim.send(Event::Wake);
    assert!(contexts(&sim, from).is_empty());

    // the session's epoch moves: the keys of the new one
    let from = sim.log.len();
    {
        let hub = &mut *world.hub.borrow_mut();
        common::sync(hub, &mut world.founder);
        world.founder.update(&world.group, true, now()).expect("an update").expect("a Commit");
        assert!(common::post_all(hub, &mut world.founder).is_empty());
    }
    assert!(world.stream(&mut sim));
    let moved = contexts(&sim, from);
    assert_eq!(moved.len(), 1, "{}", moved.len());
    let moved = moved[0].clone().expect("a context");
    assert_eq!(moved.keys.len(), 1);
    assert_eq!(moved.keys[0].epoch, first.keys[0].epoch + 1);
    assert_ne!(moved.keys[0].key, first.keys[0].key);

    // to the back: the cursor goes along
    world.agent_says("one more");
    assert!(world.stream(&mut sim));
    let from = sim.log.len();
    sim.send(Event::Background);
    let back = contexts(&sim, from);
    assert_eq!(back.len(), 1);
    assert!(back[0].as_ref().is_some_and(|c| c.cursor > moved.cursor));

    // push off here: the keys go, the context stays
    let from = sim.log.len();
    sim.send(Event::Action { action: Action::Push { level: trommi_client_core::app::push::PushChoice::Off } });
    let off = contexts(&sim, from);
    assert!(off.last().is_some_and(|c| c.as_ref().is_some_and(|c| c.keys.is_empty())), "{}", off.len());

    // signed out: all of it goes
    let from = sim.log.len();
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(contexts(&sim, from), vec![None]);
}

#[test]
fn signing_out_empties_the_widgets() {
    let mut world = World::new();
    world.agent_asks("Ship it?");
    let mut sim = world.sim();
    sim.send(start(Abilities { snapshots: true, ..Default::default() }));
    let from = sim.log.len();
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(snapshots(&sim, from), vec![WidgetSnapshot { live_activity: Some(false), ..Default::default() }]);
}

#[test]
fn the_demo_sends_its_snapshot() {
    let fixture: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../demo/data/fixture.json")).expect("the demo")).expect("json");
    let mut sim = Sim::new(now());
    sim.send(Event::Start { config: Config { platform: "test".into(), version: "0".into(), seed: 7, can: Abilities { snapshots: true, ..Default::default() }, demo: Some(fixture), ..Default::default() } });
    let sent = snapshots(&sim, 0);
    assert_eq!(sent.len(), 1);
    assert!(sent[0].waiting > 0 && sent[0].waiting as usize >= sent[0].cards.len() && !sent[0].cards.is_empty());
    let from = sim.log.len();
    sim.send(Event::Wake);
    assert!(snapshots(&sim, from).is_empty());
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn native_share_imports_wait_for_uploads_and_never_cross_accounts() {
    use trommi_client_core::actions::SharedFile;
    use trommi_client_core::app::Bytes;
    let world = World::new();
    let mut sim = world.sim(); sim.send(common::start());
    let action = |id: &str, room| Action::ImportShare { id: id.into(), room, desk: None, to: None, send: false, text: "From another app".into(), files: vec![SharedFile { name: "note.txt".into(), media_type: "text/plain".into(), bytes: Bytes(b"private file".to_vec()) }] };
    let wrong = "ab".repeat(16);
    let from = sim.log.len();
    sim.send(Event::Action { action: action(&wrong, Some("00".repeat(32))) });
    assert!(world.hub.borrow().files.is_empty());
    assert!(sim.log[from..].iter().any(|e| matches!(e, Effect::ShareInbox { id, consumed: true } if id == &wrong)));

    let id = "cd".repeat(16); let import = action(&id, None);
    sim.hold = true; let from = sim.log.len();
    sim.send(Event::Action { action: import.clone() });
    assert!(!sim.log[from..].iter().any(|e| matches!(e, Effect::ShareInbox { consumed: true, .. })));
    assert!(sim.view.notes.rows.is_empty(), "no note points to a file before it is stored");
    sim.hold = false;
    while let Some((io, request)) = sim.held.pop_front() { let got = world.hub.borrow_mut().http(sim.now_ms, &request); sim.answer(io, got); }
    assert_eq!(world.hub.borrow().files.len(), 1);
    assert!(sim.log[from..].iter().any(|e| matches!(e, Effect::ShareInbox { id: got, consumed: true } if got == &id)));
    assert_eq!(sim.view.notes.rows.len(), 1);
    assert_eq!(sim.view.notes.rows[0].files.len(), 1);
    let notes = sim.view.notes.rows.len();
    sim.send(Event::Action { action: import.clone() });
    assert_eq!(sim.view.notes.rows.len(), notes, "the same inbox item is not imported twice");
    sim.restart(); sim.send(common::start()); world.stream(&mut sim);
    sim.send(Event::Action { action: import });
    assert_eq!(sim.view.notes.rows.len(), notes, "the note's import identity survives a restart");
}

#[test]
fn live_activity_tokens_use_the_cores_room_tag_and_do_not_replace_the_apns_registration() {
    use std::{rc::Rc, cell::RefCell};
    use trommi_client_core::app::Bytes;
    let world = World::new();
    let sent = Rc::new(RefCell::new(Vec::<serde_json::Value>::new())); let got = sent.clone();
    world.hub.borrow_mut().fault = Some(Box::new(move |_, request: &HttpRequest| {
        if request.method == "GET" && request.url.ends_with("/v1/push") {
            return Some(Ok(HttpResponse { status: 200, headers: vec![], body: format!(r#"{{"subscriptions":[{{"kind":"apns","endpoint":"e","level":"all","created_at":1}}],"vapid_public_key":"{}","apns":true}}"#, trommi_core::ids::base64url_encode(&[4; 65])).into_bytes().into() }));
        }
        if request.url.ends_with("/v1/live-activity") {
            got.borrow_mut().push(serde_json::from_slice(&request.body.as_ref().unwrap().0).unwrap());
            return Some(Ok(HttpResponse { status: 200, headers: vec![], body: b"{}".to_vec().into() }));
        }
        None
    }));
    let mut sim = world.sim(); sim.send(start(Abilities { push: "apns".into(), ..Default::default() }));
    let token = |kind: &str, tag: Option<&str>| {
        let mut body = serde_json::json!({"kind":kind,"token":"test-token","environment":"sandbox","topic":"com.trommi.ios"});
        if let Some(tag) = tag { body["tag"] = tag.into(); }
        Event::PushToken { kind: "apns_live_activity".into(), token: Some(Bytes(serde_json::to_vec(&body).unwrap())) }
    };
    sim.send(token("start", None)); assert_eq!(sent.borrow().len(), 1);
    let tag = sent.borrow()[0]["tag"].as_str().unwrap().to_string(); assert_eq!(tag.len(), 16);
    sim.send(token("start", None)); assert_eq!(sent.borrow().len(), 1, "one registration per token");
    sim.send(token("activity", Some("another-room"))); assert_eq!(sent.borrow().len(), 1);
    sim.send(token("activity", Some(&tag))); assert_eq!(sent.borrow().len(), 2);
    sim.send(Event::Action { action: Action::Push { level: trommi_client_core::app::push::PushChoice::Off } });
    sim.send(token("activity", Some(&tag))); assert_eq!(sent.borrow().len(), 2, "off never hands over a live token");
}

#[test]
fn a_verified_human_removal_clears_the_device_cache_and_notification_keys() {
    let mut world = World::new(); world.agent_asks("Private question");
    let mut sim = world.sim();
    sim.secrets = Some([("push-notify-key".to_string(), vec![9;32])].into_iter().collect());
    sim.send(start(Abilities { snapshots: true, notify: true, push: "apns".into(), secrets: true, ..Default::default() }));
    world.peers_sync();
    let cut = world.founder.cut_of(&world.room, &world.me).unwrap();
    world.founder.remove_human_devices(&[cut], now()).unwrap();
    assert!(common::post_all(&mut world.hub.borrow_mut(), &mut world.founder).is_empty());
    let from = sim.log.len(); assert!(world.stream(&mut sim));
    assert!(sim.device.1.is_empty(), "a signed removal wipes the protocol state");
    assert!(sim.log[from..].iter().any(|e| matches!(e, Effect::ForgetShared)));
    assert!(sim.cache.is_empty(), "the removed device's cache goes too");
    assert!(contexts(&sim, from).contains(&None), "the extension's copy must go");
    assert!(sim.secrets.as_ref().unwrap().get("push-notify-key").is_none());
}

#[test]
fn an_empty_native_slot_clears_a_stale_extension_context() {
    let mut sim = Sim::new(now());
    sim.secrets = Some([("push-notify-key".to_string(), vec![9;32])].into_iter().collect());
    sim.send(start(Abilities { snapshots: true, notify: true, push: "apns".into(), secrets: true, ..Default::default() }));
    assert_eq!(sim.view.phase, Phase::SignedOut);
    assert!(contexts(&sim, 0).contains(&None));
    assert!(sim.secrets.as_ref().unwrap().get("push-notify-key").is_none());
}

#[test]
fn a_hubs_removal_claim_alone_never_erases_a_human_device_or_shared_inbox() {
    let world = World::new();
    world.hub.borrow_mut().removed_keys.insert(world.me, Default::default());
    let mut sim = world.sim();
    let stored = sim.device.clone();
    sim.send(start(Abilities { snapshots: true, notify: true, ..Default::default() }));
    assert!(sim.app.engine().is_some());
    assert_eq!(sim.device, stored, "only a verified removal may erase this device");
    assert!(!sim.log.iter().any(|e| matches!(e, Effect::ForgetShared)));
}

#[test]
fn a_failed_sign_out_wipe_still_drops_plaintext_and_extension_keys_and_reports_the_failure() {
    let mut world = World::new(); world.agent_asks("Private question");
    let mut sim = world.sim();
    sim.secrets = Some(Default::default());
    sim.send(start(Abilities { snapshots: true, notify: true, push: "apns".into(), secrets: true, ..Default::default() }));
    let stored = sim.device.clone(); let from = sim.log.len();
    sim.device_full = 1;
    sim.send(Event::Action { action: Action::Account { action: AccountAction::SignOut } });
    assert_eq!(sim.view.phase, Phase::Stopped);
    assert_eq!(sim.view.status.as_ref().unwrap().key, "account.wipe_failed");
    assert_eq!(sim.device, stored, "the disk refused deletion");
    assert!(sim.app.engine().is_none() && sim.view.cards.is_empty() && sim.view.desk.rows.is_empty());
    assert!(contexts(&sim, from).contains(&None));
    assert!(sim.secrets.as_ref().unwrap().get("push-notify-key").is_none());
}
