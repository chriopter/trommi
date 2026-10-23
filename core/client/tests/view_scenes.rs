//! The view conformance scenes: what a shell receives in a scene, delta by delta, and the view model it must reach.
//!
//! Each scene runs the App against the stand-in hub (tests/common: real sealed envelopes) on a fixed clock, and
//! records every `Effect::Render` delta in order with the view model the core holds at the end, in
//! `tests/conformance/views/<scene>.json`: `{ "scene", "deltas": [ViewDelta…], "view": ViewModel }`. A shell's fold
//! (the web's `core/bindings/wasm/js/views-fold.js`, Swift's `CoreView.ViewModel.apply`, the Linux shell's) folds the
//! deltas into an empty model and must reach `view` exactly (tests/bindings/views-replay.mjs, views-replay.swift).
//!
//! Random ids (sessions, cards, devices: hex) are written as stable stand-ins, the same id the same stand-in, so a
//! recording is the same on every run. A rule that changes what a screen shows shows here as a stale recording:
//! `TROMMI_GOLDEN=write cargo test -p trommi-client-core --test view_scenes`, then read the diff.
//!
//! Answering and snoozing are recorded as the person does them on his other device (the hub brings them). The same
//! from this device, by `Event::Action`, comes with the actions stream: see the ACTIONS markers below.

mod common;

use common::{fix_clock, start, World};
use serde_json::{json, Value};
use std::path::Path;
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::Effect;
use trommi_client_core::status::ASLEEP_MS;
use trommi_client_core::views::{ViewDelta, ViewModel};

/// The scenes' clock: Tuesday 13 October 2026, 14:05 UTC.
const T0: u64 = 1_791_900_300_000;

fn world() -> World {
    fix_clock(T0);
    World::new()
}

/// Every id (a run of [0-9a-f] of 32 characters or more) as a stand-in of the same length, the n-th id met is
/// `0000000n` then `e`s; a short form of one (a run of 8 or more that begins it: a session's short id) as the same
/// beginning of its stand-in.
fn stable_ids(text: &str) -> String {
    // (a device's key fingerprint comes from its random key, and a session without an icon wears the face its random
    // id picks (faces.rs): each written as one stand-in)
    let text = &(text
        .lines()
        .map(|l| match (l.find("\"fingerprint\": \""), l.find("\"mark\": \"draw:")) {
            (Some(at), _) => format!("{}\"fingerprint\": \"(fingerprint)\"{}", &l[..at], if l.ends_with(',') { "," } else { "" }),
            (None, Some(at)) => format!("{}\"mark\": \"draw:(face)\"{}", &l[..at], if l.ends_with(',') { "," } else { "" }),
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + if text.ends_with('\n') { "\n" } else { "" });
    let runs = |text: &str| -> Vec<(usize, usize)> {
        let b = text.as_bytes();
        let (mut out, mut at) = (Vec::new(), 0);
        while at < b.len() {
            let n = b[at..].iter().take_while(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c)).count();
            if n > 0 {
                out.push((at, n));
                at += n;
            } else {
                at += 1;
            }
        }
        out
    };
    let stand_in = |n: usize, len: usize| format!("{n:08x}{}", "e".repeat(len.saturating_sub(8)));
    let mut long: Vec<&str> = Vec::new();
    for (at, n) in runs(text) {
        let id = &text[at..at + n];
        if n >= 32 && !long.contains(&id) {
            long.push(id);
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, n) in runs(text) {
        let run = &text[at..at + n];
        let whole = long.iter().position(|id| *id == run);
        let short = if (8..32).contains(&n) { long.iter().position(|id| id.starts_with(run)) } else { None };
        if let Some(i) = whole.or(short) {
            out.push_str(&text[last..at]);
            out.push_str(&stand_in(i + 1, long[i].len())[..n]);
            last = at + n;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// Records the scene: the deltas the shell was sent, and the view it must reach; compares with the file.
fn record(name: &str, sim: &Sim) {
    let deltas: Vec<&ViewDelta> = sim.log.iter().filter_map(|e| if let Effect::Render { delta, .. } = e { Some(delta) } else { None }).collect();
    // (the core's own fold reaches the same model: the recording is whole)
    let mut folded = ViewModel::default();
    for d in &deltas {
        folded.apply(d).expect("the deltas follow each other");
    }
    assert_eq!(&folded, sim.app.view(), "{name}: the deltas fold into the core's view");
    let got = stable_ids(&(serde_json::to_string_pretty(&json!({ "scene": name, "deltas": deltas, "view": folded })).unwrap() + "\n"));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/views").join(format!("{name}.json"));
    if std::env::var("TROMMI_GOLDEN").as_deref() == Ok("write") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is missing: see this file's head", path.display()));
    if want != got {
        let at = want.lines().zip(got.lines()).position(|(a, b)| a != b).unwrap_or(0);
        panic!(
            "the recording {name}.json is stale from line {} (see this file's head):\n  recorded: {}\n  now:      {}",
            at + 1,
            want.lines().nth(at).unwrap_or(""),
            got.lines().nth(at).unwrap_or("")
        );
    }
}

fn ids(sim: &Sim) -> Vec<String> {
    sim.view.desk.stack.clone()
}

#[test]
fn empty_desk() {
    let world = world();
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(sim.view.desk.empty.as_ref().map(|t| t.en.as_str()), Some("Questions land here."));
    record("empty-desk", &sim);
}

#[test]
fn a_decision_arrives() {
    let mut world = world();
    let mut sim = world.sim();
    sim.send(start());
    world.agent_asks("Ship it?");
    assert!(world.stream(&mut sim));
    assert_eq!(sim.view.desk.rows.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(), ["Ship it?"]);
    record("decision-arrives", &sim);
}

#[test]
fn answered_on_another_device() {
    let mut world = world();
    let (card, _, _) = world.agent_card("Ship it?");
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(ids(&sim).len(), 1);
    // ACTIONS: the same answer sent from this device (Event::Action { Answer }) is the scene "answer", recorded here
    // once the actions stream lands; its echo must show at once, before the hub's copy.
    world.founder_answers(card, "yes");
    assert!(world.stream(&mut sim));
    assert!(ids(&sim).is_empty(), "the answered card left the Desk");
    // (its session is not connected in the stand-in world: the answered card is done, not running)
    assert_eq!(sim.view.desk.done.len(), 1);
    assert_eq!(sim.view.desk.off.as_ref().map(|o| o.says.en.as_str()), Some("1 done"));
    record("answered-elsewhere", &sim);
}

#[test]
fn snoozed_on_another_device() {
    let mut world = world();
    world.agent_asks("Ship it?");
    let mut sim = world.sim();
    sim.send(start());
    let id = ids(&sim).pop().expect("the card stands on the Desk");
    // ACTIONS: the same snooze sent from this device (Event::Action { Snooze }) is the scene "snooze", recorded here
    // once the actions stream lands.
    let until = T0 + 60 * 60_000;
    world.founder_sets(&format!("snooze/{id}"), &json!({ "until": until, "at": T0 }).to_string());
    assert!(world.stream(&mut sim));
    assert!(ids(&sim).is_empty(), "put off: off the Desk");
    assert_eq!(sim.view.desk.later, [id]);
    assert_eq!(sim.view.desk.off.as_ref().map(|o| o.says.en.as_str()), Some("1 put off"));
    record("snoozed-elsewhere", &sim);
}

#[test]
fn a_session_goes_asleep() {
    let world = world();
    let mut sim = world.sim();
    sim.send(start());
    // its connector reports: attached, on call, its last tool call now; asleep by the clock alone after ASLEEP_MS
    let device = trommi_core::ids::base64url_encode(world.agent.id().as_bytes());
    let event = json!({ "device": device, "online": true, "hears": false, "working": false, "last_call_at": sim.now_ms });
    assert!(sim.stream(StreamItem::Chunk { bytes: format!("event: presence\ndata: {event}\n\n").into_bytes().into() }));
    let state = |sim: &Sim| sim.view.sidebar.sessions.first().map(|s| s.link.clone());
    assert_eq!(state(&sim).as_deref(), Some("oncall"));
    // (the hub's pings keep the stream open meanwhile, a minute apart)
    for _ in 0..ASLEEP_MS / 60_000 {
        sim.advance(60_000);
        sim.stream(StreamItem::Chunk { bytes: b"event: ping\ndata: {}\n\n".to_vec().into() });
    }
    // (the moment itself: the timer the core set for it)
    sim.advance(0);
    assert_eq!(sim.view.phase, trommi_client_core::views::Phase::Live);
    assert_eq!(state(&sim).as_deref(), Some("asleep"));
    record("session-asleep", &sim);
}

/// The stand-ins keep what is the same the same, and what differs apart; a short form follows its id; numbers stay.
#[test]
fn stable_ids_stand_in_by_first_meeting() {
    let (a, b) = ("aa".repeat(16), "0b".repeat(32));
    let text = format!(r#"{{"{a}": ["{b}", "{a}", "{}", "short ab12", 1791900300000]}}"#, &b[..12]);
    let one = format!("00000001{}", "e".repeat(24));
    let two = format!("00000002{}", "e".repeat(56));
    let want = format!(r#"{{"{one}": ["{two}", "{one}", "{}", "short ab12", 1791900300000]}}"#, &two[..12]);
    assert_eq!(stable_ids(&text), want);
    let _: Value = serde_json::from_str(&stable_ids(&text)).unwrap();
}
