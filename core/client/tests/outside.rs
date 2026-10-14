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
    assert_eq!(snapshots(&sim, from), vec![WidgetSnapshot::default()]);
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
