//! The cache on a board the size of the large fixture (FULL_REVISED=1 FULL_CARDS=21500: 48,318 changes, 21,500
//! cards of which 60 stand open, all revised): what a full write costs, what a live line costs, how long the read
//! takes to the first picture. A made-up board of that shape, not the fixture itself (the fixture is a hub's
//! encrypted order; the App on main does not read a real hub yet).
//!
//! `cargo test --release -p trommi-client-core --test cache_large -- --ignored --nocapture`

use serde_json::json;
use std::time::Instant;
use trommi_client_core::app::io::{Entry, IoId, IoResult, StoreAnswer, StoreOp};
use trommi_client_core::app::Effect;
use trommi_client_core::cache::{Keeper, Took};
use trommi_client_core::model::change::Change;
use trommi_client_core::model::types::ItemKey;
use trommi_client_core::model::Model;
use std::collections::BTreeMap;

type Store = BTreeMap<String, Vec<u8>>;

fn run(store: &mut Store, op: &StoreOp) -> IoResult<StoreAnswer> {
    match op {
        StoreOp::Get { keys } => IoResult::Ok(StoreAnswer::Values { values: keys.iter().map(|k| store.get(k).map(|v| v.clone().into())).collect() }),
        StoreOp::Scan { prefix } => IoResult::Ok(StoreAnswer::Entries {
            entries: store.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(prefix.as_str())).map(|(k, v)| Entry { key: k.clone(), value: v.clone().into() }).collect(),
        }),
        StoreOp::Put { put, delete } => {
            for k in delete {
                store.remove(k);
            }
            for e in put {
                store.insert(e.key.clone(), e.value.0.clone());
            }
            IoResult::Ok(StoreAnswer::Done)
        }
        _ => unreachable!(),
    }
}

/// Runs the keeper's effects against the store until it waits for nothing; the bytes it put.
fn drive(keeper: &mut Keeper, model: &mut Model, store: &mut Store, next: &mut u64, mut out: Vec<Effect>) -> (usize, usize) {
    let (mut bytes, mut keys) = (0, 0);
    while let Some(effect) = out.pop() {
        if let Effect::Store { id, op } = effect {
            if let StoreOp::Put { put, delete } = &op {
                bytes += put.iter().map(|e| e.key.len() + e.value.0.len()).sum::<usize>();
                keys += put.len() + delete.len();
            }
            let answer = run(store, &op);
            assert_ne!(keeper.on_store(id, &answer, model, 0, next, &mut out), Took::No);
        }
    }
    (bytes, keys)
}

fn big_model() -> Model {
    let mut m = Model::new();
    m.my_device_id = Some("d1".into());
    let sessions = 40;
    for s in 0..sessions {
        m.sessions.insert(format!("s{s:030}"), serde_json::from_value(json!({ "session_id": format!("s{s:030}"), "profile": { "is_main": s == 0 } })).unwrap());
    }
    for c in 0..21_500u64 {
        let id = format!("{c:032x}");
        let open = c % 358 == 0; // 60 open
        let version = |v: u64| json!({ "object_version": v, "envelope_number": c * 2 + v, "content": { "card_type": "decision", "title": format!("Card {c} v{v}"), "options": [{ "key": "yes", "label": "Yes" }, { "key": "no", "label": "No" }], "sections": [{ "text": "Some context for the question, a few lines long, as an agent writes it." }] } });
        let card = json!({
            "object_id": id, "session_id": format!("s{:030}", c % sessions), "title": format!("Card {c}"), "object_version": 2,
            "object_state": if open { "open" } else { "closed" }, "card_type": "decision",
            "options": [{ "key": "yes", "label": "Yes" }, { "key": "no", "label": "No" }],
            "versions": [version(1), version(2)], "envelope_number": c * 2 + 2, "first_envelope_number": c * 2 + 1,
        });
        m.cards.insert(id, serde_json::from_value(card).unwrap());
    }
    let mut n = 50_000u64;
    for s in 0..sessions {
        let key = format!("chat:session/s{s:030}");
        let mut t = trommi_client_core::model::types::Timeline { timeline_key: key.clone(), ..Default::default() };
        for _ in 0..(5_000 / sessions as usize * if s == 0 { 4 } else { 1 }) {
            n += 1;
            let item = json!({ "envelope_number": n, "content": { "content_type": "message", "text": "A line of an agent's chat, about this long, with a little more to it." }, "sender_device_id": "a".repeat(64), "sent_at": 1_791_000_000_000u64 + n }).as_object().unwrap().clone();
            t.items.insert(ItemKey::of(&item), item);
        }
        t.item_count = t.items.len() as u64;
        t.newest_envelope_number = n;
        t.window_open = true;
        m.timelines.insert(key, t);
    }
    m
}

#[test]
#[ignore]
fn the_cache_of_a_large_board() {
    let model = big_model();
    let mut store = Store::new();
    // a first write of the whole board (a cache written anew)
    let (mut next, mut out) = (1, Vec::new());
    let mut empty = Model::new();
    let mut keeper = Keeper::open("ab", "d1", 1, &mut next, &mut out);
    drive(&mut keeper, &mut empty, &mut store, &mut next, std::mem::take(&mut out));
    let mut m = model;
    let mut all = Change::new();
    m.touch_all(&mut all);
    keeper.note(&all);
    keeper.step(0, &mut next, &mut out);
    let t = Instant::now();
    let (bytes, keys) = drive(&mut keeper, &mut m, &mut store, &mut next, std::mem::take(&mut out));
    let size: usize = store.iter().map(|(k, v)| k.len() + v.len()).sum();
    println!("whole board: {keys} keys, {:.1} MB put in {:?} (serialised; a store adds its own time); {:.1} MB in the store", bytes as f64 / 1e6, t.elapsed(), size as f64 / 1e6);
    println!("  the model's snapshot as one text: {:.1} MB", m.snapshot_text().len() as f64 / 1e6);
    // a live line in the biggest chat
    let mut change = Change::new();
    let key = "chat:session/s000000000000000000000000000000".to_string();
    let item = json!({ "envelope_number": 99_999, "content": { "content_type": "message", "text": "A new line." }, "sender_device_id": "a".repeat(64), "sent_at": 1_791_000_099_999u64 }).as_object().unwrap().clone();
    let t = m.timelines.get_mut(&key).unwrap();
    t.items.insert(ItemKey::of(&item), item.clone());
    t.item_count += 1;
    change.add_item(&key, &item);
    change.timelines.insert(key.clone());
    change.sessions.insert("s000000000000000000000000000000".into());
    keeper.note(&change);
    keeper.saw(&[Effect::Store { id: IoId(0), op: StoreOp::DeviceApply { expected_revision: 1, put: vec![], delete: vec![] } }]);
    keeper.step(0, &mut next, &mut out);
    let t = Instant::now();
    let (bytes, keys) = drive(&mut keeper, &mut m, &mut store, &mut next, std::mem::take(&mut out));
    println!("a live line: {keys} keys, {bytes} bytes in three puts, {:?}", t.elapsed());
    // the next start: the read until the board is loaded and the first picture made
    let t = Instant::now();
    let (mut next, mut out) = (1, Vec::new());
    let mut back = Model::new();
    back.my_device_id = Some("d1".into());
    let mut keeper = Keeper::open("ab", "d1", 2, &mut next, &mut out);
    let mut read = None;
    for e in out {
        if let Effect::Store { id, op } = e {
            let answer = run(&mut store, &op);
            if let Took::Read(r) = keeper.on_store(id, &answer, &mut back, 0, &mut next, &mut Vec::new()) {
                read = Some(r);
            }
        }
    }
    let loaded = t.elapsed();
    let mut c = Change::new();
    back.project(&mut c, 1_791_000_100_000);
    let mut view = trommi_client_core::views::ViewModel::default();
    let lens = trommi_client_core::views::Lens::default();
    trommi_client_core::views::render(&back, &lens, 1_791_000_100_000, &mut view);
    println!("start: {read:?} after {loaded:?} (scan answered by memory, records read into the model); first picture at {:?}", t.elapsed());
    assert_eq!(back.cards.len(), 21_500);
    assert_eq!(back.timelines[&key].items.len(), m.timelines[&key].items.len());
}
