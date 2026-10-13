//! The App's cache (core/client/src/cache) against the stand-in hub: the board it keeps is drawn at the next start
//! before the hub answers anything, a cache that is not whole or not this device's is never shown, and a process
//! killed before any effect leaves a cache that loads as a board the App had (or is dropped), never a wrong one.

mod common;

use common::{start, World};
use serde_json::Value;
use std::collections::BTreeMap;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Effect, Event};
use trommi_client_core::views::Phase;

/// Answers every request the App holds out, and those that follow, until none is left.
fn release(world: &World, sim: &mut Sim) {
    sim.hold = false;
    while let Some((id, request)) = sim.held.pop_front() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
}

/// The board as a person reads it: every card, session, register and timeline item, by id (presence and alerts
/// left out: they follow the clock and the stream).
fn board(sim: &Sim) -> BTreeMap<String, Value> {
    let snap = sim.app.board().model.snapshot();
    let mut out = BTreeMap::new();
    for part in ["cards", "sessions", "permissions", "notes", "published", "human"] {
        for pair in snap[part].as_array().into_iter().flatten() {
            // (a field that is null and one left out read the same)
            let mut record = pair[1].clone();
            if let Some(o) = record.as_object_mut() {
                o.retain(|_, v| !v.is_null());
            }
            out.insert(format!("{part}/{}", pair[0].as_str().unwrap_or("")), record);
        }
    }
    for t in snap["timelines"].as_array().into_iter().flatten() {
        for item in t[1]["items"].as_array().into_iter().flatten() {
            out.insert(format!("item/{}/{}", t[0].as_str().unwrap_or(""), item[1]["envelope_number"]), item[1].clone());
        }
    }
    for id in snap["members"].as_array().into_iter().flatten().filter_map(|m| m[0].as_str()) {
        out.insert(format!("member/{id}"), Value::Null);
    }
    out
}

fn items(board: &BTreeMap<String, Value>) -> usize {
    board.keys().filter(|k| k.starts_with("item/")).count()
}

/// The cache's `meta`, as stored.
fn meta(sim: &Sim) -> Option<Value> {
    sim.cache.iter().find(|(k, _)| k.ends_with("/meta")).and_then(|(_, v)| serde_json::from_slice(v).ok())
}

fn answered_http(sim: &Sim, from: usize) -> usize {
    // (with `hold` no request is answered: every Http effect since `from` is still held)
    sim.log[from..].iter().filter(|e| matches!(e, Effect::Http { .. })).count() - sim.held.len()
}

/// A room with two open cards and three lines. What the App would post (its KeyPackages, its `heads`) the hub
/// does not take now, so every Sim of the App's device stored at the start reads the same room (two Apps of one
/// device that both posted would be two forks of it).
fn scene() -> World {
    let mut world = World::new();
    world.hub.borrow_mut().fault = Some(Box::new(|_, r| {
        (r.method != "GET" && !r.url.contains("/tokens")).then(|| Ok(common::hub::later(503, 1)))
    }));
    world.agent_says("one");
    world.agent_asks("First?");
    world.agent_says("two");
    world.agent_asks("Second?");
    world.agent_says("three");
    world
}

#[test]
fn a_start_with_a_filled_cache_draws_the_desk_before_the_hub_answers() {
    let world = scene();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(sim.view.desk.rows.len(), 2);
    let before = board(&sim);
    assert!(items(&before) >= 3, "the lines are on the board: {before:?}");
    assert_eq!(meta(&sim).unwrap()["rev"], Value::from(sim.device.0), "the cache stands at the device's revision");
    // the next start: nothing is answered by the hub
    sim.restart();
    sim.hold = true;
    let from = sim.log.len();
    sim.send(start());
    assert_eq!(answered_http(&sim, from), 0);
    assert!(!sim.held.is_empty(), "the engine asked the hub only after the cache was read");
    let titles: Vec<&str> = sim.view.desk.rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["First?", "Second?"], "the cached Desk is drawn before any answer");
    assert_eq!(board(&sim), before, "the whole board, the chat lines too, is the cached one");
    // the hub answers: nothing moves, nothing is written but the revision
    let writes = sim.cache.clone();
    release(&world, &mut sim);
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(board(&sim), before);
    let changed: Vec<&String> = sim.cache.iter().filter(|(k, v)| writes.get(*k) != Some(v)).map(|(k, _)| k).collect();
    assert!(changed.iter().all(|k| k.ends_with("/meta") || k.ends_with("/desk") || k.contains("/model/sess/") || k.contains("/model/members")), "{changed:?}");
}

#[test]
fn a_live_line_writes_its_own_records_and_not_the_board() {
    let mut world = scene();
    let mut sim = world.sim();
    sim.send(start());
    let before = sim.cache.clone();
    let from = sim.log.len();
    world.agent_says("a live line");
    assert!(world.stream(&mut sim));
    let mut put = Vec::new();
    for e in &sim.log[from..] {
        if let Effect::Store { op: trommi_client_core::app::io::StoreOp::Put { put: entries, delete }, .. } = e {
            assert!(delete.is_empty());
            put.extend(entries.iter().map(|e| (e.key.clone(), e.value.0.len())));
        }
    }
    let keys: Vec<&str> = put.iter().map(|(k, _)| k.rsplit_once("/model/").map_or("meta", |(_, t)| t)).collect();
    assert!(keys.iter().filter(|k| k.starts_with("item/")).count() == 1, "one item: {keys:?}");
    assert!(keys.len() <= 6, "the line, its timeline, its session, the meta twice: {keys:?}");
    let bytes: usize = put.iter().map(|(_, n)| n).sum();
    let whole: usize = before.values().map(Vec::len).sum();
    assert!(bytes * 2 < whole, "a live line writes {bytes} bytes of a cache of {whole}");
}

#[test]
fn a_cache_behind_the_device_keeps_the_cards_and_drops_the_lines_it_may_lack() {
    let world = scene();
    let mut sim = world.sim();
    sim.send(start());
    let before = board(&sim);
    // the process died after the device stored and before the cache was written: the meta names an older revision
    let (key, mut m) = sim.cache.iter().find(|(k, _)| k.ends_with("/meta")).map(|(k, v)| (k.clone(), serde_json::from_slice::<Value>(v).unwrap())).unwrap();
    m["rev"] = Value::from(sim.device.0 - 1);
    sim.cache.insert(key, serde_json::to_vec(&m).unwrap());
    sim.restart();
    sim.hold = true;
    sim.send(start());
    let titles: Vec<&str> = sim.view.desk.rows.iter().map(|r| r.title.as_str()).collect();
    assert_eq!(titles, ["First?", "Second?"]);
    let shown = board(&sim);
    assert_eq!(items(&shown), 0, "no line is shown that may have one missing before it");
    assert!(shown.iter().all(|(k, v)| before.get(k) == Some(v)), "everything shown is what the App had");
    for t in sim.app.board().model.timelines.values() {
        assert!(t.has_more && !t.window_open, "the timeline says it has more to read");
    }
    release(&world, &mut sim);
    assert_eq!(meta(&sim).unwrap()["rev"], Value::from(sim.device.0));
    assert!(sim.cache.keys().all(|k| !k.contains("/model/item/")), "the dropped items are deleted from the cache");
}

#[test]
fn a_cache_not_whole_or_not_this_devices_is_never_shown_and_is_built_again() {
    let world = scene();
    let mut sim = world.sim();
    sim.send(start());
    let before = board(&sim);
    let room = sim.cache.keys().next().unwrap().split('/').nth(1).unwrap().to_string();
    let meta_key = format!("c1/{room}/meta");
    let good = sim.cache[&meta_key].clone();
    let card = r#"{"object_id":"ff","title":"Not here","object_state":"open","object_version":1}"#;
    for (why, stored) in [
        ("another layout", r#"{"v":99,"device":"x","rev":0}"#.to_string()),
        ("another device", String::from_utf8(good.clone()).unwrap().replace(r#""device":""#, r#""device":"00"#)),
        ("a newer revision than the device's", r#"{"v":1,"device":"D","rev":999999}"#.replace('D', serde_json::from_slice::<Value>(&good).unwrap()["device"].as_str().unwrap())),
        ("a write under way", String::from_utf8(good.clone()).unwrap().replace(r#""writing":false"#, r#""writing":true"#)),
        ("a record of an unknown kind", String::from_utf8(good.clone()).unwrap()),
        ("no meta", String::new()),
    ] {
        let mut sim = world.sim();
        sim.send(start());
        sim.cache.insert(format!("c1/{room}/model/card/ff"), card.as_bytes().to_vec());
        if why == "a record of an unknown kind" {
            sim.cache.insert(format!("c1/{room}/model/what/1"), b"1".to_vec());
        }
        if stored.is_empty() {
            sim.cache.remove(&meta_key);
        } else {
            sim.cache.insert(meta_key.clone(), stored.into_bytes());
        }
        sim.restart();
        sim.hold = true;
        sim.send(start());
        assert!(sim.view.desk.rows.iter().all(|r| r.title != "Not here"), "{why}: never shown");
        assert!(sim.app.board().model.timelines.values().all(|t| t.items.is_empty()), "{why}: nothing of it shown");
        release(&world, &mut sim);
        let logs: Vec<String> = sim.log.iter().filter_map(|e| if let Effect::Log { text, .. } = e { Some(text.clone()) } else { None }).collect();
        assert_eq!(sim.view.phase, Phase::Live, "{why}: {logs:?}");
        assert!(!sim.cache.contains_key(&format!("c1/{room}/model/card/ff")), "{why}: its records are deleted");
        assert!(!sim.cache.contains_key(&format!("c1/{room}/model/what/1")), "{why}");
        assert!(board(&sim).keys().all(|k| before.contains_key(k)), "{why}: built from the hub");
        assert_eq!(meta(&sim).unwrap()["rev"], Value::from(sim.device.0), "{why}: written anew");
        // and the cache written anew is taken at the next start
        sim.restart();
        sim.hold = true;
        sim.send(start());
        assert_eq!(board(&sim), board(&{
            let mut s = world.sim();
            s.cache = sim.cache.clone();
            s.device = sim.device.clone();
            s.hold = true;
            s.send(start());
            s
        }));
    }
}

#[test]
fn killed_before_any_effect_the_next_start_shows_a_board_the_app_had_and_ends_where_it_would() {
    let world = scene();
    let reference = {
        let mut sim = world.sim();
        sim.send(start());
        assert_eq!(sim.view.phase, Phase::Live);
        (board(&sim), sim.log.len())
    };
    let (mut behind, mut wholes) = (0, 0);
    for n in 0..reference.1 + 4 {
        let mut sim = world.sim();
        sim.die_in = Some(n);
        sim.send(start());
        let died = sim.dead;
        sim.restart();
        let device_rev = sim.device.0;
        let stamped = meta(&sim).map(|m| (m["rev"].as_u64().unwrap(), m["writing"].as_bool().unwrap()));
        if let Some((rev, _)) = stamped {
            assert!(rev <= device_rev, "kill {n}: the cache never runs ahead of the device ({rev} > {device_rev})");
        }
        sim.hold = true;
        sim.send(start());
        // what the cache shows before the hub answers is what the App had
        let shown = board(&sim);
        // (cards and lines do not change in this room once read; a session's list of cards grows meanwhile)
        for (k, v) in shown.iter().filter(|(k, _)| k.starts_with("cards/") || k.starts_with("item/")) {
            assert_eq!(reference.0.get(k), Some(v), "kill {n}: {k} is not what the App had");
        }
        let lagging = stamped.is_some_and(|(rev, writing)| !writing && rev < device_rev);
        // (a cache dropped, or none, while the device took lines: those lines are replays for it now)
        let whole = stamped.is_some_and(|(rev, writing)| !writing && rev == device_rev) || (stamped.is_none() && device_rev == 0);
        behind += usize::from(lagging);
        wholes += usize::from(whole);
        if lagging {
            assert_eq!(items(&shown), 0, "kill {n}: a cache behind shows no line");
        }
        release(&world, &mut sim);
        assert_eq!(sim.view.phase, Phase::Live, "kill {n}");
        let end = board(&sim);
        // the cards, sessions and registers end as in the run without a kill; the lines too unless the cache was
        // behind (then a timeline is read when it is opened, and until then a session's last activity is the last
        // one this App saw)
        let without_items = |b: &BTreeMap<String, Value>| {
            b.iter()
                .filter(|(k, _)| !k.starts_with("item/"))
                .map(|(k, v)| {
                    let mut v = v.clone();
                    if let (true, Some(o)) = (!whole && k.starts_with("sessions/"), v.as_object_mut()) {
                        o.remove("last_activity_at");
                    }
                    (k.clone(), v)
                })
                .collect::<BTreeMap<_, _>>()
        };
        let (a, b) = (without_items(&end), without_items(&reference.0));
        let differ: Vec<(&String, Option<&Value>, Option<&Value>)> = a.keys().chain(b.keys()).filter(|k| a.get(*k) != b.get(*k)).map(|k| (k, a.get(k), b.get(k))).collect();
        assert!(differ.is_empty(), "kill {n}: {differ:?}");
        if whole {
            assert_eq!(end, reference.0, "kill {n} (died: {died}): the lines too");
        }
        assert_eq!(meta(&sim).unwrap()["rev"], Value::from(sim.device.0), "kill {n}: written up to the device");
    }
    eprintln!("{} kills: {wholes} left a whole cache, {behind} one behind the device", reference.1 + 4);
    assert!(behind > 0, "some kill falls between the device's write and the cache's");
    // (the Sim runs every write at once, so a write is under way at most kills of a read; a shell's store takes its
    // time between the App's events)
    assert!(wholes > 0, "a kill outside a write leaves a whole cache");
}

#[test]
fn a_store_that_does_not_answer_the_read_leaves_the_cache_unwritten() {
    let world = scene();
    let mut sim = world.sim();
    sim.send(start());
    let kept = sim.cache.clone();
    sim.restart();
    sim.hold = true;
    sim.send(start());
    // (by hand: a new App whose cache read fails)
    let mut app = trommi_client_core::app::App::new();
    let effects = app.handle(sim.now_ms, start());
    let load = effects.iter().find_map(|e| if let Effect::Store { id, .. } = e { Some(*id) } else { None }).unwrap();
    let effects = app.handle(sim.now_ms, Event::Store { id: load, result: trommi_client_core::app::io::IoResult::Ok(trommi_client_core::app::io::StoreAnswer::Device { revision: sim.device.0, entries: sim.device.1.iter().map(|(k, v)| trommi_client_core::app::io::DeviceEntry { key: k.clone().into(), value: v.clone().into() }).collect() }) });
    let mut later = Vec::new();
    for e in effects {
        if let Effect::Store { id, op } = e {
            if matches!(op, trommi_client_core::app::io::StoreOp::Get { .. } | trommi_client_core::app::io::StoreOp::Scan { .. }) {
                later.extend(app.handle(sim.now_ms, Event::Store { id, result: trommi_client_core::app::io::IoResult::Err(trommi_client_core::app::io::IoError::new(trommi_client_core::app::io::IoErrorKind::Failed)) }));
            }
        }
    }
    assert!(later.iter().any(|e| matches!(e, Effect::Http { .. })), "the engine starts without the cache");
    assert!(!later.iter().any(|e| matches!(e, Effect::Store { op: trommi_client_core::app::io::StoreOp::Put { .. }, .. })), "and nothing is written over it");
    assert_eq!(sim.cache, kept);
}

