//! The Desk's speed by itself (no binding): 1,000 open cards, then cards one at a time and in pages.
//! `cargo run --release -p trommi-client-core --example desk_speed`
use std::time::Instant;
use trommi_client_core::desk::{DeskCard, DeskState};
use trommi_client_core::State;

fn card(n: u64) -> DeskCard {
    DeskCard {
        object_id: format!("{n:032x}"),
        session_id: Some(format!("s{}", n % 16)),
        state: State::Open,
        urgency: Some("normal".into()),
        created_at: n,
        agent_device_id: Some(format!("a{}", n % 8)),
        version_change: n,
        newest: n,
        in_revision: None,
        read: true,
        working: false,
    }
}

fn main() {
    let mut d = DeskState::new();
    for n in 0..1000 {
        d.put_card(card(n));
    }
    d.project(1);
    let t = Instant::now();
    for n in 1000..4000 {
        d.put_card(card(n));
        let _ = d.project(1);
    }
    println!(
        "one at a time: {:.2} us per card",
        t.elapsed().as_secs_f64() * 1e6 / 3000.0
    );
    let (mut put, mut project) = (0.0, 0.0);
    for b in 0..15u64 {
        let t = Instant::now();
        for i in 0..200 {
            d.put_card(card(10_000 + b * 200 + i));
        }
        put += t.elapsed().as_secs_f64();
        let t = Instant::now();
        let _ = d.project(1);
        project += t.elapsed().as_secs_f64();
    }
    println!(
        "pages of 200: put {:.2} us, project {:.2} us per card",
        put * 1e6 / 3000.0,
        project * 1e6 / 3000.0
    );
}
