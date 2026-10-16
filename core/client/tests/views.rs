//! The view model of the demo room (demo/data/fixture.json), unit by unit, against the snapshots in tests/golden/:
//! reviewed once, stable. A change of a rule shows here as a diff of what a screen shows.
//!
//! After a deliberate change: `TROMMI_GOLDEN=write cargo test -p trommi-client-core --test views`, then read the diff.

use serde::Serialize;
use std::path::{Path, PathBuf};
use trommi_client_core::model::Model;
use trommi_client_core::views::{card, demo, desk, notes, session, settings, sidebar, Board, Lens};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The demo room at the moment it was made (no time moved), and that moment.
fn demo_room() -> (Model, u64) {
    let text = std::fs::read_to_string(root().join("demo/data/fixture.json")).expect("demo/data/fixture.json");
    let fixture: serde_json::Value = serde_json::from_str(&text).unwrap();
    let now = fixture["made_at"].as_u64().unwrap();
    (demo::model_of(&fixture, now).unwrap(), now)
}

fn golden(name: &str, view: &impl Serialize) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    let got = serde_json::to_string_pretty(view).unwrap() + "\n";
    if std::env::var("TROMMI_GOLDEN").as_deref() == Ok("write") {
        std::fs::write(&path, &got).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is missing: see this file's head", path.display()));
    if want != got {
        let at = want.lines().zip(got.lines()).position(|(a, b)| a != b).unwrap_or(0);
        panic!(
            "{name} differs from line {}:\n  golden: {}\n  now:    {}",
            at + 1,
            want.lines().nth(at).unwrap_or(""),
            got.lines().nth(at).unwrap_or("")
        );
    }
}

/// The lens of the demo's first screen: the first team, in Berlin's summer time.
fn lens(team: Option<&str>) -> Lens {
    Lens {
        team: team.map(String::from),
        utc_offset_min: 120,
        ..Default::default()
    }
}

#[test]
fn the_demo_rooms_sidebar() {
    let (m, now) = demo_room();
    for (file, team) in [("demo-sidebar.json", None), ("demo-sidebar-platform.json", Some("platform"))] {
        let lens = lens(team);
        let b = Board::new(&m, &lens, now);
        golden(file, &sidebar::sidebar(&b));
    }
}

#[test]
fn the_demo_rooms_desk() {
    let (m, now) = demo_room();
    for (file, team) in [("demo-desk.json", None), ("demo-desk-platform.json", Some("platform"))] {
        let lens = lens(team);
        let b = Board::new(&m, &lens, now);
        golden(file, &desk::desk(&b));
    }
}

/// A session's page: the main with the longest conversation, and one of the other team.
#[test]
fn the_demo_rooms_sessions() {
    let (m, now) = demo_room();
    let lens = lens(None);
    let b = Board::new(&m, &lens, now);
    for slug in ["fernly-web", "payments-service"] {
        let view = session::session(&b, slug).unwrap_or_else(|| panic!("no session {slug}"));
        golden(&format!("demo-session-{slug}.json"), &view);
    }
}

/// A card's page: an open decision with pictures, one that was answered, and one that is with its agent.
#[test]
fn the_demo_rooms_cards() {
    let (m, now) = demo_room();
    let lens = lens(None);
    let b = Board::new(&m, &lens, now);
    let walk = desk::desk(&b).stack;
    for (file, id) in [("demo-card-hero.json", "a38afef6dbdd056db8042980f2b83336"), ("demo-card-answered.json", "cac2dd6c8c804cbff53f50e0bf8c239e"), ("demo-card-with-agent.json", "0b48fb2a02a4fcc2a22da6f5a3b7bb49")] {
        let view = card::card(&b, id, &walk).unwrap_or_else(|| panic!("no card {id}"));
        golden(file, &view);
    }
}

/// The notes (the corner's note and whom it goes to) and Settings (its rows' words, the person's devices), on the
/// first team and on All teams.
#[test]
fn the_demo_rooms_notes_and_settings() {
    let (m, now) = demo_room();
    for (file, team) in [("demo-notes.json", None)] {
        let lens = lens(team);
        golden(file, &notes::notes(&Board::new(&m, &lens, now)));
    }
    let lens = lens(None);
    golden("demo-settings.json", &settings::settings(&Board::new(&m, &lens, now)));
}

/// Every text of one in the demo's views carries its number (`args.n`), as the texts of many do.
#[test]
fn texts_of_one_carry_their_number() {
    fn walk(v: &serde_json::Value, bad: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(o) => {
                if let (Some(key), Some(_)) = (o.get("key").and_then(|k| k.as_str()), o.get("en")) {
                    let one = key.ends_with(".one") || key.ends_with("_one");
                    // (not a count: "One tap answers")
                    if one && key != "card.answer.one" && o.get("args").and_then(|a| a.get("n")).and_then(|n| n.as_str()) != Some("1") {
                        bad.push(key.to_string());
                    }
                }
                o.values().for_each(|x| walk(x, bad));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, bad)),
            _ => {}
        }
    }
    let mut bad = Vec::new();
    for e in std::fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")).unwrap() {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(e.unwrap().path()).unwrap()).unwrap();
        walk(&v, &mut bad);
    }
    assert!(bad.is_empty(), "texts of one without args.n: {bad:?}");
}

/// A small generator for the property test below (xorshift: the same edits for the same seed).
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn word(&mut self) -> String {
        format!("w{}", self.below(7))
    }
}

/// One random edit of a view model: any unit may change, appear or go; long lists gain, lose and swap rows.
fn edit(v: &mut trommi_client_core::views::ViewModel, pool: &[trommi_client_core::views::ViewModel], r: &mut Rng) {
    use trommi_client_core::views::*;
    let other = &pool[r.below(pool.len())];
    match r.below(14) {
        0 => v.phase = [Phase::Starting, Phase::SignedOut, Phase::Opening, Phase::Live, Phase::Offline, Phase::Stopped][r.below(6)],
        1 => v.status = (r.below(2) == 0).then(|| Text::new("status", &r.word())),
        2 => v.sidebar = other.sidebar.clone(),
        3 => v.desk = other.desk.clone(),
        4 if !v.desk.rows.is_empty() => {
            let at = r.below(v.desk.rows.len());
            match r.below(3) {
                0 => {
                    v.desk.rows.remove(at);
                }
                1 => {
                    let to = r.below(v.desk.rows.len());
                    v.desk.rows.swap(at, to);
                }
                _ => v.desk.rows[at].title = r.word(),
            }
            v.desk.stack = v.desk.rows.iter().map(|x| x.id.clone()).collect();
        }
        5 if !v.sidebar.sessions.is_empty() => {
            let at = r.below(v.sidebar.sessions.len());
            v.sidebar.sessions[at].open = r.below(9) as u32;
            v.sidebar.badge = r.below(99) as u32;
        }
        6 => {
            let id = r.word();
            v.sessions.insert(id.clone(), SessionView { id, has_more: r.below(2) == 0, link: r.word(), ..Default::default() });
        }
        7 => {
            let id = r.word();
            v.sessions.remove(&id);
        }
        8 => {
            let id = r.word();
            v.cards.insert(id.clone(), CardView { id, title: r.word(), history: r.below(4) as u32, ..Default::default() });
        }
        9 => {
            let id = r.word();
            v.cards.remove(&id);
        }
        10 => v.notes.notes = (0..r.below(3)).map(|_| r.word()).collect(),
        11 => {
            let id = r.word();
            if r.below(3) == 0 {
                v.files.remove(&id);
            } else {
                v.files.insert(id, FileView { name: r.word(), percent: Some(r.below(100) as u8), ..Default::default() });
            }
        }
        12 => v.toasts = (0..r.below(3)).map(|i| Toast {
                id: format!("t{i}"),
                text: Text::new("toast", &r.word()),
                action: None,
                line: (r.below(2) == 0).then(|| Text::new("toast.line", &r.word())),
                label: None,
                alert: r.below(2) == 0,
                ms: (r.below(2) == 0).then(|| r.below(9000) as u64),
                go: None,
            }).collect(),
        13 => {
            v.settings.account = (r.below(2) == 0).then(|| r.word());
            v.account.step = r.word();
        }
        _ => {}
    }
}

/// `diff` then `apply` gives the next model, over long runs of seeded random edits: a shell that folds every delta
/// holds exactly what the core decided, and a model that did not change sends nothing.
#[test]
fn diff_then_apply_is_the_next_model() {
    use trommi_client_core::views::ViewModel;
    let (m, now) = demo_room();
    // (real units to draw from: the demo room's sidebar and Desk on each team)
    let pool: Vec<ViewModel> = [None, Some("platform")]
        .into_iter()
        .map(|d| {
            let lens = lens(d);
            let b = Board::new(&m, &lens, now);
            ViewModel { sidebar: sidebar::sidebar(&b), desk: desk::desk(&b), ..Default::default() }
        })
        .collect();
    for seed in 1..=40u64 {
        let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut shell = ViewModel::default();
        let mut core = ViewModel::default();
        for step in 0..300 {
            for _ in 0..1 + r.below(3) {
                edit(&mut core, &pool, &mut r);
            }
            match shell.diff(&core) {
                Some(delta) => {
                    // (across the edge: a delta is what its JSON says)
                    let wire: trommi_client_core::views::ViewDelta = serde_json::from_str(&serde_json::to_string(&delta).unwrap()).unwrap();
                    shell.apply(&wire).unwrap_or_else(|e| panic!("seed {seed} step {step}: {e:?}"));
                    core.rev = shell.rev;
                }
                None => assert_eq!(shell, core, "seed {seed} step {step}: nothing sent, so nothing changed"),
            }
            assert_eq!(shell, core, "seed {seed} step {step}");
            assert!(shell.diff(&core).is_none());
        }
    }
}

/// `render` fills the board's units and leaves the engine's alone.
#[test]
fn render_makes_the_boards_units_and_keeps_the_rest() {
    use trommi_client_core::views::{render, Phase, ViewModel};
    let (m, now) = demo_room();
    let lens = lens(None);
    let mut view = ViewModel { phase: Phase::Live, ..Default::default() };
    let wake = render(&m, &lens, now, &mut view);
    let b = Board::new(&m, &lens, now);
    assert_eq!((&view.sidebar, &view.desk, view.phase), (&sidebar::sidebar(&b), &desk::desk(&b), Phase::Live));
    assert!(view.sessions.is_empty(), "no page in view: no session's unit");
    // the session in view (by the short id of its address) is the one unit made; leaving it takes it away
    let on = Lens { session: Some("fernly-web".into()), ..lens.clone() };
    render(&m, &on, now, &mut view);
    let b = Board::new(&m, &on, now);
    let page = session::session(&b, "fernly-web").unwrap();
    assert_eq!(view.sessions.get(&page.id), Some(&page));
    assert_eq!(view.sessions.len(), 1);
    // the card in view, by its number of the address
    let on = Lens { card: Some("46".into()), ..lens.clone() };
    render(&m, &on, now, &mut view);
    assert_eq!(view.cards.values().map(|c| (c.number, c.title.as_str())).collect::<Vec<_>>(), [(46, "Launch on Monday at 09:00, or on Tuesday?")]);
    assert!(view.sessions.is_empty());
    render(&m, &lens, now, &mut view);
    assert!(view.sessions.is_empty() && view.cards.is_empty());
    assert!(wake.is_none_or(|w| w > now), "a wake lies ahead: {wake:?} at {now}");
}

/// What one render costs on a large room: the demo grown to the mega account's size (21,500 cards, most of them
/// closed; 150 sessions; a talk of 3,000 messages), with a session's page and a card's page in view. Prints the
/// median of 15 renders. Not a gate: run when a rule's cost is in question, in release:
/// `cargo test -p trommi-client-core --release --test views -- --ignored --nocapture render_cost`.
#[test]
#[ignore]
fn render_cost_on_a_large_room() {
    use serde_json::{json, Value};
    use trommi_client_core::views::{render, ViewModel};
    let text = std::fs::read_to_string(root().join("demo/data/fixture.json")).unwrap();
    let mut f: Value = serde_json::from_str(&text).unwrap();
    let now = f["made_at"].as_u64().unwrap();
    let cards: Vec<Value> = f["cards"].as_array().unwrap().clone();
    let sessions: Vec<Value> = f["sessions"].as_array().unwrap().clone();
    let id = |n: usize| format!("{n:032x}");
    // (more sessions: copies of the demo's, archived, as the mega account's 120 archived ones)
    let mut grown_sessions = sessions.clone();
    let mut settings = f["human"]["session_settings"].as_object().cloned().unwrap_or_default();
    for n in 0..(150 - sessions.len()) {
        let mut s = sessions[n % sessions.len()].clone();
        let dev = format!("{:064x}", 0xdead_0000_u64 + n as u64);
        s["agent_device_id"] = json!(dev);
        s["agent_session_id"] = json!(format!("copy-{n}"));
        s["is_online"] = json!(false);
        settings.insert(dev, json!({ "archived": true }));
        grown_sessions.push(s);
    }
    f["human"]["session_settings"] = Value::Object(settings);
    f["sessions"] = json!(grown_sessions);
    // (more cards: copies of the demo's, closed, each its own id and number)
    let mut grown = cards.clone();
    for n in 0..(21_500 - cards.len()) {
        let mut c = cards[n % cards.len()].clone();
        c["object_id"] = json!(id(n + 1));
        c["object_state"] = json!("closed");
        c["closed_how"] = json!(if n % 3 == 0 { "answered" } else { "done" });
        c["first_envelope_number"] = json!(10_000 + n);
        c["envelope_number"] = json!(10_000 + n);
        c["created_at"] = json!(now - 1000 * n as u64);
        c["updated_at"] = json!(now - 1000 * n as u64);
        grown.push(c);
    }
    f["cards"] = json!(grown);
    // (a long talk in the main's session)
    let dev = "54ddca7af4029f0526984da1e346d8331b288c038a7dadd153fa3079f750d8e8";
    let key = format!("chat:session/{dev}");
    let talk: Vec<Value> = (0..3_000u64)
        .map(|n| json!({ "envelope_number": 900_000 + n, "local_id": null, "pending": false, "sender_device_id": if n % 2 == 0 { dev } else { "me" },
            "sent_at": now - (3_000 - n) * 60_000, "item_state": "loaded", "content_type": "message", "content": { "text": format!("Message {n}: see https://example.com/page/{n} and **this**.") } }))
        .collect();
    f["timelines"][&key] = json!(talk);
    let t = std::time::Instant::now();
    let m = demo::model_of(&f, now).unwrap();
    let built = t.elapsed();
    let median = |lens: &Lens, view: &mut ViewModel| {
        let mut times = Vec::new();
        for _ in 0..15 {
            let t = std::time::Instant::now();
            render(&m, lens, now, view);
            times.push(t.elapsed());
        }
        times.sort();
        times
    };
    {
        let lens = Lens { utc_offset_min: 120, ..Default::default() };
        let t = std::time::Instant::now();
        let b = Board::new(&m, &lens, now);
        let t_board = t.elapsed();
        let t = std::time::Instant::now();
        let _ = sidebar::sidebar(&b);
        let t_side = t.elapsed();
        let t = std::time::Instant::now();
        let off = trommi_client_core::views::off::of_board(&b);
        let t_off = t.elapsed();
        let t = std::time::Instant::now();
        let _ = desk::desk_with(&b, &off);
        let t_desk = t.elapsed();
        let t = std::time::Instant::now();
        let _ = notes::notes(&b);
        let _ = settings::settings(&b);
        println!("render_cost: board {t_board:?}, sidebar {t_side:?}, off_mind {t_off:?}, team {t_desk:?}, notes+settings {:?}", t.elapsed());
    }
    let mut view = ViewModel::default();
    let desk_only = median(&Lens { utc_offset_min: 120, ..Default::default() }, &mut view);
    let off = median(&Lens { utc_offset_min: 120, off: Some(String::new()), ..Default::default() }, &mut view);
    println!("render_cost: the Desk alone {:?}; with the page Off your mind {:?} ({} KB)", desk_only[7], off[7], serde_json::to_string(&view.off).unwrap().len() / 1024);
    let lens = Lens { utc_offset_min: 120, session: Some("fernly-web".into()), card: Some("a38afef6dbdd056db8042980f2b83336".into()), ..Default::default() };
    let times = median(&lens, &mut view);
    let talk_len = view.sessions.values().next().map(|s| s.talk.len()).unwrap_or(0);
    let json = serde_json::to_string(&view).unwrap().len();
    println!(
        "render_cost: {} cards, {} sessions, talk {talk_len} rows; model built in {built:?}; render median {:?} (min {:?}, max {:?}); view {} KB as JSON",
        m.cards.len(),
        m.sessions.len(),
        times[times.len() / 2],
        times[0],
        times[times.len() - 1],
        json / 1024
    );
    assert!(m.cards.len() >= 21_500);
}
