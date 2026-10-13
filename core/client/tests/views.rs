//! The view model of the demo room (demo/data/fixture.json), unit by unit, against the snapshots in tests/golden/:
//! reviewed once, stable. A change of a rule shows here as a diff of what a screen shows.
//!
//! After a deliberate change: `TROMMI_GOLDEN=write cargo test -p trommi-client-core --test views`, then read the diff.

use serde::Serialize;
use std::path::{Path, PathBuf};
use trommi_client_core::model::Model;
use trommi_client_core::views::{demo, desk, sidebar, Board, Lens};

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

/// The lens of the demo's first screen: the first desk, in Berlin's summer time.
fn lens(desk: Option<&str>) -> Lens {
    Lens {
        desk: desk.map(String::from),
        utc_offset_min: 120,
        ..Default::default()
    }
}

#[test]
fn the_demo_rooms_sidebar() {
    let (m, now) = demo_room();
    for (file, desk) in [("demo-sidebar.json", None), ("demo-sidebar-platform.json", Some("platform")), ("demo-sidebar-all.json", Some("all"))] {
        let lens = lens(desk);
        let b = Board::new(&m, &lens, now);
        golden(file, &sidebar::sidebar(&b));
    }
}

#[test]
fn the_demo_rooms_desk() {
    let (m, now) = demo_room();
    for (file, desk) in [("demo-desk.json", None), ("demo-desk-platform.json", Some("platform")), ("demo-desk-all.json", Some("all"))] {
        let lens = lens(desk);
        let b = Board::new(&m, &lens, now);
        golden(file, &desk::desk(&b));
    }
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
        12 => v.toasts = (0..r.below(3)).map(|i| Toast { id: format!("t{i}"), text: Text::new("toast", &r.word()), action: None }).collect(),
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
    // (real units to draw from: the demo room's sidebar and Desk on each desk)
    let pool: Vec<ViewModel> = [None, Some("platform"), Some("all")]
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
