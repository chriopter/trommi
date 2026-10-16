//! A card settled while the App was away never stands on the Desk after the next start, whatever the cache kept:
//! the hub's Desk list decides (core/client/src/desk.rs "the hub's open list decides").

mod common;

use common::{start, World};
use std::collections::BTreeMap;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::views::Phase;

fn titles(sim: &Sim) -> Vec<String> {
    sim.view.desk.rows.iter().map(|r| r.title.clone()).collect()
}

fn release(world: &World, sim: &mut Sim) {
    sim.hold = false;
    while let Some((id, request)) = sim.held.pop_front() {
        let got = world.hub.borrow_mut().http(sim.now_ms, &request);
        sim.answer(id, got);
    }
}

#[test]
fn a_card_closed_while_away_leaves_the_desk_at_the_next_start() {
    let mut world = World::new();
    let (card, hash, _) = world.agent_card("Gone?");
    world.agent_asks("Stays?");
    let mut sim = world.sim();
    sim.send(start());
    assert_eq!(titles(&sim), ["Gone?", "Stays?"]);
    sim.restart();
    world.agent_closes(card, &hash, "Gone?", 2);
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(titles(&sim), ["Stays?"]);
}

#[test]
fn a_card_closed_while_the_cache_lagged_behind_leaves_the_desk() {
    let mut world = World::new();
    let (card, hash, _) = world.agent_card("Gone?");
    world.agent_asks("Stays?");
    let mut sim = world.sim();
    sim.send(start());
    let cached: BTreeMap<String, Vec<u8>> = sim.cache.clone();
    // the close is taken and stored by the device; the process dies before the cache is written
    world.agent_closes(card, &hash, "Gone?", 2);
    assert!(world.stream(&mut sim));
    assert_eq!(titles(&sim), ["Stays?"]);
    sim.cache = cached;
    sim.restart();
    sim.send(start());
    assert_eq!(sim.view.phase, Phase::Live);
    assert_eq!(titles(&sim), ["Stays?"], "the hub's list leaves the closed card out");
}

#[test]
fn a_card_closed_while_away_is_off_the_desk_once_the_hub_lists_again_even_from_a_cache_held_back() {
    let mut world = World::new();
    let (card, hash, _) = world.agent_card("Gone?");
    world.agent_asks("Stays?");
    let mut sim = world.sim();
    sim.send(start());
    let cached = sim.cache.clone();
    world.agent_closes(card, &hash, "Gone?", 2);
    assert!(world.stream(&mut sim));
    sim.cache = cached;
    sim.restart();
    sim.hold = true;
    sim.send(start());
    release(&world, &mut sim);
    assert_eq!(titles(&sim), ["Stays?"]);
    // and again, from what this run wrote
    sim.restart();
    sim.send(start());
    assert_eq!(titles(&sim), ["Stays?"]);
}
