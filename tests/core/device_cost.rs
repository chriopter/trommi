//! What one entry costs a device, against how much its group already holds: receiving an entry, and writing
//! one, cost the same in a group of ten objects and in one of thousands, and the device's write carries only
//! what the entry changed.
//!
//! `a_full_account_is_read` is the measurement behind it, not a check: it prints how long a device takes to read
//! an account of about 49 000 entries and how many bytes it hands its store.
//!
//! ```text
//! cargo test --release -p trommi-tests --test core_device_cost a_full_account_is_read -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};
use trommi_core::crypto::SecretBytes;
use trommi_core::device::{Draft, EnvelopeOutcome, Sealed};
use trommi_core::envelope::Urgency;
use trommi_core::ids::{GroupId, SessionId};
use trommi_tests::hub::Hub;
use trommi_tests::hub::content::Change;
use trommi_tests::{
    enrol, found_helper, found_main, found_room, json, new_device, new_device_on, post_ok,
    publish_some, sync_all, take_envelope, write, MemoryStorage, TestDevice,
};

const ZERO_HASH: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

/// A card's payload; every one has the same length, so that two entries of one kind weigh the same.
fn card(number: usize, previous: &str) -> SecretBytes {
    json(&format!(
        r#"{{"card_type":"decision","title":"Card {number:08}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"{previous}"}}"#
    ))
}

fn line(number: usize) -> SecretBytes {
    json(&format!(r#"{{"text":"Line {number:08}"}}"#))
}

/// A room with one human device, whose store the test sees, and one agent in its main session.
struct World {
    hub: Hub,
    session: SessionId,
    group: GroupId,
    human: TestDevice,
    store: MemoryStorage,
    agent: TestDevice,
    written: usize,
}

fn world() -> World {
    let store = MemoryStorage::new();
    let mut human = new_device_on(store.handle());
    let mut agent = new_device();
    let (mut hub, _) = found_room(&mut human);
    publish_some(&mut hub, &mut agent, 4);
    enrol(&mut hub, &mut human, &mut agent);
    let group = found_main(&mut hub, &mut human, &agent.id());
    sync_all(&hub, &mut human);
    sync_all(&hub, &mut agent);
    World {
        hub,
        session: group.session_id().unwrap(),
        group,
        human,
        store,
        agent,
        written: 0,
    }
}

impl World {
    /// The agent writes a new card; both devices take it.
    fn new_card(&mut self) -> (Sealed, Cost) {
        self.written += 1;
        let draft = Draft::CardFirst {
            session: self.session,
            urgency: Urgency::Normal,
            push: false,
            payload: card(self.written, ZERO_HASH),
        };
        self.entry(&draft)
    }

    /// The agent writes the next version of `card`, still open.
    fn next_version(&mut self, first: &Sealed) -> Cost {
        self.written += 1;
        let draft = Draft::CardVersion {
            session: self.session,
            object_id: first.object_id.unwrap(),
            closed: false,
            urgency: Urgency::Normal,
            push: false,
            payload: card(self.written, &first.envelope_hash.to_base64url()),
        };
        self.entry(&draft).1
    }

    /// The agent writes a Chat line.
    fn chat(&mut self) -> Cost {
        self.written += 1;
        let draft = Draft::SessionChat {
            session: self.session,
            payload: line(self.written),
        };
        self.entry(&draft).1
    }

    /// One entry: the agent seals and posts it, the human device receives it; what each cost.
    fn entry(&mut self, draft: &Draft) -> (Sealed, Cost) {
        let start = Instant::now();
        let sealed = self
            .agent
            .seal(draft, None, &[], trommi_tests::now())
            .expect("the item seals");
        let seal = start.elapsed();
        post_ok(&mut self.hub, &mut self.agent);
        sync_all(&self.hub, &mut self.agent);
        let changes = self.hub.changes_after(self.human.cursor());
        let [Change::Envelope(stored)] = changes.as_slice() else {
            panic!("one envelope is new");
        };
        let before = self.store.written();
        let start = Instant::now();
        let received = take_envelope(&mut self.human, stored);
        let receive = start.elapsed();
        assert_eq!(received.outcome, EnvelopeOutcome::Applied);
        (
            sealed,
            Cost {
                seal,
                receive,
                bytes: self.store.written() - before,
            },
        )
    }

    fn objects(&self) -> usize {
        self.human.objects(&self.group).unwrap().len()
    }
}

/// What one entry cost: the writer's sealing, the reader's receiving, and the bytes the reader's write carried.
#[derive(Debug, Clone, Copy)]
struct Cost {
    seal: Duration,
    receive: Duration,
    bytes: u64,
}

/// The cheapest of several entries of one kind: the time the work itself takes, without what else the machine
/// did meanwhile.
fn cheapest(costs: &[Cost]) -> Cost {
    Cost {
        seal: costs.iter().map(|cost| cost.seal).min().unwrap(),
        receive: costs.iter().map(|cost| cost.receive).min().unwrap(),
        bytes: costs.iter().map(|cost| cost.bytes).max().unwrap(),
    }
}

/// One of each kind of entry, several times over, in the group as it stands: a new card, a next version of a
/// card, a Chat line.
fn sample(world: &mut World) -> [Cost; 3] {
    let (mut cards, mut versions, mut lines) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..24 {
        let (first, cost) = world.new_card();
        cards.push(cost);
        versions.push(world.next_version(&first));
        lines.push(world.chat());
    }
    [cheapest(&cards), cheapest(&versions), cheapest(&lines)]
}

#[test]
fn an_entry_costs_the_same_in_a_group_of_ten_objects_and_of_two_thousand() {
    let mut world = world();
    for _ in 0..10 {
        world.new_card();
    }
    let small = sample(&mut world);
    while world.objects() < 2000 {
        world.new_card();
    }
    let large = sample(&mut world);
    println!("kind    | objects | seal µs | receive µs | bytes written");
    for (kind, (small, large)) in ["card", "version", "chat"]
        .into_iter()
        .zip(small.iter().zip(&large))
    {
        for (objects, cost) in [(10, small), (2000, large)] {
            println!(
                "{kind:7} | {objects:7} | {:7} | {:10} | {:13}",
                cost.seal.as_micros(),
                cost.receive.as_micros(),
                cost.bytes
            );
        }
        // The device's write carries what the entry changed, whatever the group holds: its record, the object
        // it changed, the heads. (An object's version index may add a few bytes; nothing grows with the group.)
        assert!(
            large.bytes <= small.bytes + 64,
            "a {kind} writes {} bytes among 2000 objects and {} among 10",
            large.bytes,
            small.bytes
        );
        // And the work is the same. The cheapest of 24 on each side, and a wide margin: only a cost that grows
        // with the group fails this, not a busy machine.
        for (what, small, large) in [
            ("receiving", small.receive, large.receive),
            ("sealing", small.seal, large.seal),
        ] {
            assert!(
                large < small * 3 + Duration::from_micros(200),
                "{what} a {kind} takes {large:?} among 2000 objects and {small:?} among 10"
            );
        }
    }
}

/// Writes what the reading device holds before the read and the envelopes it reads, for the same read on
/// another build of the core (the browser's, under Node): JSON, bytes as hex.
fn dump(path: &str, store: &MemoryStorage, changes: &[Change]) {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let entries: Vec<serde_json::Value> = store
        .entries()
        .iter()
        .map(|entry| serde_json::json!([hex(&entry.key), hex(&entry.value)]))
        .collect();
    let envelopes: Vec<serde_json::Value> = changes
        .iter()
        .map(|change| match change {
            Change::Envelope(stored) => serde_json::json!([stored.change, hex(&stored.bytes)]),
            Change::Log(_) => panic!("only envelopes are new"),
        })
        .collect();
    let dumped = serde_json::json!({ "revision": store.revision(), "entries": entries, "envelopes": envelopes });
    std::fs::write(path, dumped.to_string()).expect("the dump is written");
}

/// How many cards, each written and then closed, and how many Chat lines a session holds.
struct Quota {
    cards: usize,
    lines: usize,
}

fn number(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// An account as months of use leave it, in the shape of the clients' full fixture (tests/client/mega/full.rs):
/// `COST_AGENTS` agents (30), each with a main session and four helper sessions; the first ten agents are the
/// live ones, whose main session weighs 20 and whose first two helpers weigh 3, every other session 1.
/// `COST_CARDS` cards (21 500) are spread by weight, each written and closed by its agent, and `COST_LINES`
/// Chat lines (4 400): about 47 000 entries. Then a device that was in every session from its founding and read
/// nothing reads it all, entry by entry in the hub's order, as a first read after a sign-in does.
#[test]
#[ignore = "a measurement: minutes"]
fn a_full_account_is_read() {
    let agents_wanted = number("COST_AGENTS", 30);
    let (cards_wanted, lines_wanted) = (number("COST_CARDS", 21_500), number("COST_LINES", 4_400));
    let start = Instant::now();
    let store = MemoryStorage::new();
    let mut human = new_device_on(store.handle());
    let (mut hub, _) = found_room(&mut human);
    publish_some(&mut hub, &mut human, agents_wanted * 4 + 8);
    let mut agents = Vec::new();
    let mut sessions: Vec<(usize, GroupId, usize)> = Vec::new();
    for index in 0..agents_wanted {
        let mut agent = new_device();
        publish_some(&mut hub, &mut agent, 4);
        enrol(&mut hub, &mut human, &mut agent);
        let main = found_main(&mut hub, &mut human, &agent.id());
        sync_all(&hub, &mut agent);
        let live = index < 10;
        sessions.push((index, main, if live { 20 } else { 1 }));
        for helper in 0..4 {
            let group = found_helper(&mut hub, &mut agent, &main, &mut []);
            sessions.push((index, group, if live && helper < 2 { 3 } else { 1 }));
        }
        agents.push(agent);
    }
    sync_all(&hub, &mut human);
    let weights: usize = sessions.iter().map(|(_, _, weight)| weight).sum();
    println!(
        "{:6.1}s {} agents, {} sessions",
        start.elapsed().as_secs_f64(),
        agents.len(),
        sessions.len()
    );
    let mut written = 0usize;
    let mut largest = 0usize;
    for (agent, group, weight) in &sessions {
        let quota = Quota {
            cards: cards_wanted * weight / weights,
            lines: lines_wanted * weight / weights,
        };
        largest = largest.max(quota.cards);
        let agent = &mut agents[*agent];
        sync_all(&hub, agent);
        let session = group.session_id().unwrap();
        for at in 0..quota.cards {
            written += 2;
            let sealed = write(
                &mut hub,
                agent,
                &Draft::CardFirst {
                    session,
                    urgency: Urgency::Normal,
                    push: false,
                    payload: card(written, ZERO_HASH),
                },
            );
            sync_all(&hub, agent);
            write(
                &mut hub,
                agent,
                &Draft::CardVersion {
                    session,
                    object_id: sealed.object_id.unwrap(),
                    closed: true,
                    urgency: Urgency::Normal,
                    push: false,
                    payload: card(written, &sealed.envelope_hash.to_base64url()),
                },
            );
            sync_all(&hub, agent);
            if at * quota.lines / quota.cards.max(1) != (at + 1) * quota.lines / quota.cards.max(1) {
                written += 1;
                write(
                    &mut hub,
                    agent,
                    &Draft::SessionChat {
                        session,
                        payload: line(written),
                    },
                );
                sync_all(&hub, agent);
            }
        }
    }
    println!(
        "{:6.1}s {written} entries written; the largest session holds {largest} cards",
        start.elapsed().as_secs_f64()
    );

    let changes = hub.changes_after(human.cursor());
    let (before, applied) = (store.written(), store.applied());
    let mut largest_write = 0u64;
    let mut tables = std::collections::BTreeMap::<u8, (usize, usize)>::new();
    for entry in store.entries() {
        let at = tables.entry(entry.key[0]).or_default();
        at.0 += 1;
        at.1 += entry.value.len();
    }
    println!("STORED before the read, per table (entries, bytes): {tables:?}");
    if let Ok(path) = std::env::var("COST_DUMP") {
        dump(&path, &store, &changes);
    }
    let read = Instant::now();
    let mut pages = Vec::new();
    for page in changes.chunks(500) {
        let page_start = Instant::now();
        for change in page {
            let Change::Envelope(stored) = change else {
                panic!("only envelopes are new");
            };
            let at = store.written();
            let received = take_envelope(&mut human, stored);
            assert_eq!(received.outcome, EnvelopeOutcome::Applied);
            largest_write = largest_write.max(store.written() - at);
        }
        pages.push(page_start.elapsed());
    }
    let took = read.elapsed();
    let entries = changes.len();
    let bytes = store.written() - before;
    pages.sort();
    println!(
        "READ {entries} entries in {:.2}s ({:.0} µs each); pages of 500: fastest {:.0} ms, median {:.0} ms, slowest {:.0} ms",
        took.as_secs_f64(),
        took.as_secs_f64() * 1e6 / entries as f64,
        pages[0].as_secs_f64() * 1e3,
        pages[pages.len() / 2].as_secs_f64() * 1e3,
        pages[pages.len() - 1].as_secs_f64() * 1e3,
    );
    println!(
        "WRITTEN {} writes, {:.1} MB in all, {:.0} bytes each, the largest {} bytes",
        store.applied() - applied,
        bytes as f64 / 1e6,
        bytes as f64 / entries as f64,
        largest_write
    );
    assert_eq!(entries, written);
}
