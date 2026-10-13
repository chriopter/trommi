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
