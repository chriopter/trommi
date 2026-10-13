//! The mega test of the clients: one long, seeded, random timeline against a real hub, with every client of the
//! person as a device of the same room, and the question whether all of them end up showing exactly the same.
//!
//! In the room, per seed:
//! - the stand-in human device (tests/connector/common) founds it and keeps the recovery code;
//! - agents: the connector's own client, in process, each in a main session (one also opens a helper session);
//! - the web app's client layer with the real core (tests/client/mega/web-observer.mjs), twice;
//! - the iOS app's engine with the real core (MegaObserverTests.swift, its test runner on Linux), twice.
//!
//! The timeline: cards (decision and info), versions, answers, answers taken back, hand backs, What??, an agent
//! putting a card before the human again, snoozes and wake-ups, closings and withdrawals, session renames,
//! permission requests (some that expire within seconds) and verdicts, Chat lines. The human acts from every
//! device in turn. The web and iOS devices sign in with the code after the first part (a fresh login with
//! history, the hub's Desk read before the history), in an order the seed picks, see faults (failed and slow
//! requests), go away and come back while the history route is down (the Desk first, the history out of order).
//!
//! At every checkpoint each device shows its snapshot (Desk order, open requests, every card's state, title,
//! answer, hand back, hold, every request's state, the sessions' names, the snoozes). The test fails on:
//! - VARIANCE: two devices of the same app differ (a race in the app);
//! - CROSS: web and iOS differ on what both show;
//! - AGENT: an agent holds another state of a card or request, or another answer, than the person's devices;
//! - REGRESSION (only with a tree named `branch` in MEGA_TREES): the branch differs from every device of the
//!   other trees of its platform.
//!
//! ```text
//! tests/client/mega/prepare.sh .        # the core for the browser and for Swift, the iOS test runner
//! MEGA_SEEDS=1,2,3 MEGA_STEPS=50 cargo test -p trommi-tests --test client_mega every_client -- --ignored --nocapture
//! ```
//! `MEGA_TREES="branch=<repo>,main=<worktree>"` runs the devices of several checkouts in one room (default: this
//! one); `MEGA_TWINS=0` leaves the second device of each app out; `MEGA_IOS=0`, `MEGA_WEB=0`; `MEGA_OUT` is
//! where the snapshots and the report go. `speed_of_every_client` measures the same clients with a big Desk, and
//! how fast a reopened connector holds the human's answers (`connector_answers_lag_ms`: per answer, from the hub's
//! taking it to the agent's holding it).
#[path = "../connector/common/mod.rs"]
mod common;
#[path = "mega/chaos.rs"]
mod chaos;
#[path = "mega/torture.rs"]
mod torture;

use common::{HubProc, Human, TempDir};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use trommi_connector::client::{Client, ClientEvent};
use trommi_connector::join::join_room;
use trommi_connector::store::Journal;
use trommi_connector::util::now_ms;
use trommi_core::crypto::SecretBytes;
use trommi_core::device::Draft;
use trommi_core::ids::{GroupId, ObjectId, SessionId};

// ---- randomness, seeded --------------------------------------------------------------------------------------------

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() % 10_000) as f64 / 10_000.0 < p
    }
    fn pick<'a, T>(&mut self, list: &'a [T]) -> Option<&'a T> {
        if list.is_empty() {
            None
        } else {
            Some(&list[self.below(list.len())])
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn hexid(h: &str) -> ObjectId {
    ObjectId::from_slice(&trommi_connector::util::unhex(h).expect("hex")).expect("an id")
}

fn sid(h: &str) -> SessionId {
    SessionId::from_slice(&trommi_connector::util::unhex(h).expect("hex")).expect("a session id")
}

fn fields(v: Value) -> Map<String, Value> {
    v.as_object().cloned().expect("an object")
}

// ---- a device in another process, driven by command files ---------------------------------------------------------

struct Observer {
    /// e.g. "web@branch"
    name: String,
    platform: &'static str,
    dir: PathBuf,
    child: Child,
    n: u64,
    signed_in: bool,
}

impl Observer {
    fn spawn(platform: &'static str, tree: &str, root: &Path, work: &Path, seed: u64) -> Observer {
        let name = format!("{platform}@{tree}");
        let dir = work.join(format!("{platform}-{tree}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the observer's folder");
        let log = std::fs::File::create(dir.join("log.txt")).expect("a log");
        let branch_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let child = match platform {
            "web" => Command::new("node")
                .arg(branch_root.join("tests/client/mega/web-observer.mjs"))
                .arg(&dir)
                .arg(root)
                .env("MEGA_SEED", seed.to_string())
                .stdin(Stdio::null())
                .stdout(log.try_clone().unwrap())
                .stderr(log)
                .spawn()
                .expect("node"),
            "ios" => {
                // the built test runner itself (tests/client/mega/prepare.sh builds it): no SwiftPM, so several
                // devices of one tree run side by side without its lock on .build
                let runner = root.join("tests/ios/.build/out/Products/Debug-linux-x86_64/TrommiCoreLiveTests-test-runner");
                Command::new(&runner)
                    .arg("TrommiCoreLiveTests.MegaObserverTests/testObserve")
                    .env("TROMMI_MEGA_OBSERVER", &dir)
                    .stdin(Stdio::null())
                    .stdout(log.try_clone().unwrap())
                    .stderr(log)
                    .spawn()
                    .expect("the iOS test runner")
            }
            _ => unreachable!(),
        };
        Observer {
            name,
            platform,
            dir,
            child,
            n: 0,
            signed_in: false,
        }
    }

    async fn call(&mut self, cmd: Value) -> Result<Value, String> {
        let n = self.n;
        self.n += 1;
        let tmp = self.dir.join(format!("cmd-{n}.json.tmp"));
        std::fs::write(&tmp, cmd.to_string()).unwrap();
        std::fs::rename(&tmp, self.dir.join(format!("cmd-{n}.json"))).unwrap();
        let res = self.dir.join(format!("res-{n}.json"));
        let started = std::time::Instant::now();
        let _slow = SlowCall(started, format!("{} {}", self.name, cmd["op"]));
        let limit: u64 = env("MEGA_CALL_S")
            .and_then(|s| s.parse().ok())
            .unwrap_or(240);
        for _ in 0..(limit * 20) {
            if let Ok(text) = std::fs::read_to_string(&res) {
                let v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                return if v["ok"] == true {
                    Ok(v["result"].clone())
                } else {
                    Err(format!("{}: {} {}", self.name, v["error"], v["message"]))
                };
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                let log = std::fs::read_to_string(self.dir.join("log.txt")).unwrap_or_default();
                return Err(format!(
                    "{} exited ({status}) at {cmd}: {}",
                    self.name,
                    tail(&log, 3000)
                ));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err(format!("{}: no answer to {cmd} in {limit} s", self.name))
    }
}

/// Says when a call to a device took longer than 5 s (for the reader of a slow run).
struct SlowCall(std::time::Instant, String);
impl Drop for SlowCall {
    fn drop(&mut self) {
        let ms = self.0.elapsed().as_millis();
        if ms > 5_000 {
            eprintln!("SLOW {} took {ms} ms", self.1);
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tail(s: &str, n: usize) -> &str {
    let at = s.len().saturating_sub(n);
    let at = (at..s.len())
        .find(|i| s.is_char_boundary(*i))
        .unwrap_or(s.len());
    &s[at..]
}

// ---- agents --------------------------------------------------------------------------------------------------------

struct Agent {
    client: Arc<Client>,
    _events: tokio::task::JoinHandle<()>,
    _dir: TempDir,
    session: String,
    helper: Option<String>,
}

async fn join_agent(
    human: &mut Human,
) -> (
    Arc<Client>,
    tokio::task::JoinHandle<()>,
    TempDir,
    common::Admitted,
) {
    let dir = TempDir::new("agent");
    let journal = Journal::open(dir.path()).expect("a journal");
    let mut invite = human.invite(None).await;
    let link = invite.link.clone();
    let for_join = journal.clone();
    let joining = tokio::spawn(async move { join_room(&link, for_join, |_| {}, 50, 60_000).await });
    let admitted = human.admit(&mut invite).await;
    joining.await.expect("the join ran").expect("joined");
    let (client, mut events) = Client::open(journal).await.expect("the slot opens");
    client.start().await.expect("online");
    // (the human's commands: taken and noted, as the connector hands them to its agent)
    let c = client.clone();
    let drain = tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            if let ClientEvent::Command(command) = event {
                if !command.envelope_hash.is_empty() {
                    let _ = c.ledger_mark(&command.envelope_hash).await;
                }
            }
        }
    });
    (client, drain, dir, admitted)
}

// ---- the world of one seed -----------------------------------------------------------------------------------------

struct World {
    rng: Rng,
    human: Human,
    agents: Vec<Agent>,
    observers: Vec<Observer>,
    n: u64,
    room_hex: String,
    code_hex: String,
    hub_url: String,
    log: Vec<String>,
    /// Requests that run out within seconds: never given a verdict (a race with the clock).
    short_requests: BTreeSet<String>,
}

#[derive(Clone)]
struct CardView {
    id: String,
    agent: usize,
    state: String,
    in_revision: bool,
    info: bool,
}

impl World {
    async fn cards(&self) -> Vec<CardView> {
        let mut out = Vec::new();
        for (i, a) in self.agents.iter().enumerate() {
            let core = a.client.core.lock().await;
            for c in core.model.cards.values() {
                out.push(CardView {
                    id: c.object_id.clone(),
                    agent: i,
                    state: c.object_state.clone(),
                    in_revision: c.in_revision.is_some(),
                    info: c.content.get("card_type").and_then(Value::as_str) == Some("info"),
                });
            }
        }
        out
    }

    async fn pending_requests(&self) -> Vec<String> {
        let mut out = Vec::new();
        for a in &self.agents {
            let core = a.client.core.lock().await;
            for p in core.model.permissions.values() {
                if p.permission_state == "pending"
                    && p.expires_at > now_ms() + 60_000
                    && !self.short_requests.contains(&p.object_id)
                {
                    out.push(p.object_id.clone());
                }
            }
        }
        out
    }

    async fn settle_agents(&self) {
        for a in &self.agents {
            let _ = a.client.settle(60_000).await;
        }
    }

    fn humans(&self) -> Vec<Option<usize>> {
        let mut v = vec![None];
        for (i, o) in self.observers.iter().enumerate() {
            if o.signed_in {
                v.push(Some(i));
            }
        }
        v
    }

    /// One random step. Returns what it did, for the log.
    async fn step(&mut self) -> String {
        self.n += 1;
        let n = self.n;
        let cards = self.cards().await;
        let open: Vec<CardView> = cards
            .iter()
            .filter(|c| c.state == "open" && !c.in_revision)
            .cloned()
            .collect();
        let open_decisions: Vec<CardView> = open.iter().filter(|c| !c.info).cloned().collect();
        let answered: Vec<CardView> = cards
            .iter()
            .filter(|c| c.state == "answered")
            .cloned()
            .collect();
        let revision: Vec<CardView> = cards
            .iter()
            .filter(|c| c.state == "open" && c.in_revision)
            .cloned()
            .collect();
        let a = self.rng.below(self.agents.len());
        let pending_n = self.pending_requests().await.len();
        // (only what can be done now, weighted: the ranges of `roll` below)
        let ranges: [(usize, usize, bool); 12] = [
            (0, 17, true),
            (18, 25, !open.is_empty()),
            (26, 42, !open_decisions.is_empty()),
            (43, 47, !answered.is_empty()),
            (48, 55, !answered.is_empty()),
            (56, 58, !open.is_empty()),
            (59, 63, !open_decisions.is_empty()),
            (64, 67, !revision.is_empty()),
            (68, 72, !open.is_empty()),
            (73, 78, true),
            (79, 86, true),
            (87, 93, pending_n > 0),
        ];
        let total: usize = ranges
            .iter()
            .filter(|r| r.2)
            .map(|r| r.1 - r.0 + 1)
            .sum::<usize>()
            + 6;
        let mut at = self.rng.below(total);
        let mut roll = 94;
        for (lo, hi, ok) in ranges {
            if !ok {
                continue;
            }
            let width = hi - lo + 1;
            if at < width {
                roll = lo + at;
                break;
            }
            at -= width;
        }
        let done = match roll {
            0..=17 => {
                let info = self.rng.chance(0.2);
                let session = if self.rng.chance(0.25) {
                    self.agents[a]
                        .helper
                        .clone()
                        .or(Some(self.agents[a].session.clone()))
                } else {
                    Some(self.agents[a].session.clone())
                };
                let urgency = ["low", "normal", "high", "critical"][self.rng.below(4)];
                let card = if info {
                    json!({ "card_type": "info", "title": format!("c{n} info"), "body": format!("info {n}"), "urgency": urgency })
                } else {
                    json!({ "card_type": "decision", "title": format!("c{n} decide"), "body": format!("decide {n}"), "urgency": urgency,
                            "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }, { "key": "c", "label": "C" }], "allows_multiple": false })
                };
                match self.agents[a].client.send_card(fields(card), session).await {
                    Ok(id) => format!(
                        "agent{a} asks {id} ({})",
                        if info { "info" } else { "decision" }
                    ),
                    Err(e) => format!("agent{a} ask refused: {}", e.text()),
                }
            }
            18..=25 => match self.rng.pick(&open) {
                Some(c) => {
                    let r = self.agents[c.agent].client.revise(&c.id, fields(json!({ "body": format!("revised at {n}"), "change_note": format!("v at {n}") })), None).await;
                    format!(
                        "agent{} revises {}: {:?}",
                        c.agent,
                        c.id,
                        r.err().map(|e| e.code)
                    )
                }
                None => "nothing to revise".into(),
            },
            26..=42 => match self.rng.pick(&open_decisions).cloned() {
                Some(c) => {
                    let choice = ["a", "b", "c"][self.rng.below(3)];
                    self.human_act(json!({ "op": "answer", "card": c.id, "choices": [choice] }))
                        .await
                }
                None => "nothing to answer".into(),
            },
            43..=47 => match self.rng.pick(&answered).cloned() {
                Some(c) => {
                    self.human_act(json!({ "op": "take_back", "card": c.id }))
                        .await
                }
                None => "nothing to take back".into(),
            },
            48..=55 => match self.rng.pick(&answered) {
                Some(c) => {
                    let r = self.agents[c.agent]
                        .client
                        .close(&c.id, &format!("done at {n}"))
                        .await;
                    format!(
                        "agent{} closes {}: {:?}",
                        c.agent,
                        c.id,
                        r.err().map(|e| e.code)
                    )
                }
                None => "nothing to close".into(),
            },
            56..=58 => match self.rng.pick(&open) {
                Some(c) => {
                    let r = self.agents[c.agent]
                        .client
                        .withdraw(&c.id, "not needed")
                        .await;
                    format!(
                        "agent{} withdraws {}: {:?}",
                        c.agent,
                        c.id,
                        r.err().map(|e| e.code)
                    )
                }
                None => "nothing to withdraw".into(),
            },
            59..=63 => match self.rng.pick(&open_decisions).cloned() {
                Some(c) => {
                    let op = if self.rng.chance(0.5) {
                        "explain"
                    } else {
                        "hand_back"
                    };
                    self.human_act(json!({ "op": op, "card": c.id })).await
                }
                None => "nothing to hand back".into(),
            },
            64..=67 => {
                match self.rng.pick(&revision) {
                    Some(c) => {
                        let r = self.agents[c.agent]
                        .client
                        .send_message(fields(json!({ "text": format!("look again {n}"), "present_card": true })), Some(c.id.clone()), None)
                        .await;
                        format!(
                            "agent{} presents {}: {:?}",
                            c.agent,
                            c.id,
                            r.err().map(|e| e.code)
                        )
                    }
                    None => "nothing to present".into(),
                }
            }
            68..=72 => match self.rng.pick(&open).cloned() {
                Some(c) => {
                    // a snooze for an hour, or a wake-up (none)
                    let until = if self.rng.chance(0.7) {
                        json!(now_ms() + 3_600_000)
                    } else {
                        Value::Null
                    };
                    self.human_act(json!({ "op": "snooze", "card": c.id, "until": until }))
                        .await
                }
                None => "nothing to snooze".into(),
            },
            73..=78 => {
                let target = if self.rng.chance(0.3) {
                    self.agents[a].helper.clone()
                } else {
                    None
                }
                .unwrap_or_else(|| self.agents[a].session.clone());
                let name = format!("S{a} named at {n}");
                let r = self.agents[a]
                    .client
                    .set_status(fields(json!({ "profile": { "agent_name": name, "task": format!("task {n}") } })), Some(target.clone()))
                    .await;
                format!(
                    "agent{a} renames {target} to {name}: {:?}",
                    r.err().map(|e| e.code)
                )
            }
            79..=86 => {
                let short = self.rng.chance(0.3);
                let ms = if short { 1_500 } else { 40 * 60_000 };
                match self.agents[a]
                    .client
                    .request_permission(
                        &format!("p{n}"),
                        &format!("tool call {n}"),
                        "ls -la",
                        ms,
                        Some(self.agents[a].session.clone()),
                    )
                    .await
                {
                    Ok(id) => {
                        if short {
                            self.short_requests.insert(id.clone());
                        }
                        format!(
                            "agent{a} asks permission {id}{}",
                            if short { " (short)" } else { "" }
                        )
                    }
                    Err(e) => format!("agent{a} permission refused: {}", e.text()),
                }
            }
            87..=93 => {
                let pending = self.pending_requests().await;
                match self.rng.pick(&pending).cloned() {
                    Some(id) => {
                        let allow = self.rng.chance(0.6);
                        self.human_act(json!({ "op": "verdict", "request": id, "allow": allow }))
                            .await
                    }
                    None => "no request waits".into(),
                }
            }
            _ => {
                let r = self.agents[a]
                    .client
                    .send_message(
                        fields(json!({ "text": format!("line {n}") })),
                        None,
                        Some(self.agents[a].session.clone()),
                    )
                    .await;
                format!("agent{a} says line {n}: {:?}", r.err().map(|e| e.code))
            }
        };
        self.settle_agents().await;
        done
    }

    /// A human's deed, from one of the person's devices picked at random (the stand-in, or a signed-in observer).
    async fn human_act(&mut self, cmd: Value) -> String {
        let humans = self.humans();
        let who = *self.rng.pick(&humans).unwrap();
        match who {
            Some(i) => {
                let o = &mut self.observers[i];
                let name = o.name.clone();
                match o.call(cmd.clone()).await {
                    Ok(_) => format!("{name} {cmd}"),
                    Err(e) => format!("{name} {cmd} REFUSED {e}"),
                }
            }
            None => {
                self.human.sync().await;
                let card = cmd["card"].as_str().unwrap_or_default().to_string();
                let object = cmd["card"]
                    .as_str()
                    .or(cmd["request"].as_str())
                    .unwrap_or_default()
                    .to_string();
                if !self.human.object_ids().contains(&object) {
                    return format!("stand-in {cmd} SKIPPED: it never read that object");
                }
                let r = match cmd["op"].as_str().unwrap() {
                    "answer" => {
                        let choices: Vec<String> = cmd["choices"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|c| c.as_str().unwrap().to_string())
                            .collect();
                        let refs: Vec<&str> = choices.iter().map(String::as_str).collect();
                        self.human.answer(&card, json!({ "schema_version": 2, "answer_action": "answer", "choices": choices }), &refs, false).await
                    }
                    "take_back" => self.human.take_back(&card).await,
                    "verdict" => {
                        self.human
                            .verdict(
                                cmd["request"].as_str().unwrap(),
                                cmd["allow"].as_bool().unwrap(),
                            )
                            .await
                    }
                    "snooze" => {
                        let group = GroupId::room(self.human.room);
                        let value =
                            (!cmd["until"].is_null()).then(|| json!({ "until": cmd["until"] }));
                        self.human
                            .set_register(&group, &format!("snooze/{card}"), value)
                            .await
                    }
                    op @ ("explain" | "hand_back") => {
                        let session = self.session_of_card(&card).await;
                        let mut body = json!({ "schema_version": 2, "content_type": "message", "text": if op == "explain" { "What??" } else { "back to you" } });
                        body[op] = json!(true);
                        match session {
                            Some(s) => {
                                self.human
                                    .write(Draft::CardChat {
                                        session: sid(&s),
                                        card: hexid(&card),
                                        payload: SecretBytes::new(body.to_string().into_bytes()),
                                    })
                                    .await
                            }
                            None => Ok(()),
                        }
                    }
                    _ => Ok(()),
                };
                format!(
                    "stand-in {cmd}{}",
                    r.err()
                        .map(|e| format!(" REFUSED {}", e.code))
                        .unwrap_or_default()
                )
            }
        }
    }

    async fn session_of_card(&self, card: &str) -> Option<String> {
        for a in &self.agents {
            if let Some(c) = a.client.core.lock().await.model.cards.get(card) {
                return c.session_id.clone();
            }
        }
        None
    }

    /// Everybody catches up; then every device's snapshot.
    async fn checkpoint(&mut self, label: &str, out: &Path) -> BTreeMap<String, Value> {
        self.settle_agents().await;
        self.human.sync().await;
        let mut snaps = BTreeMap::new();
        for o in self.observers.iter_mut().filter(|o| o.signed_in) {
            if let Err(e) = o.call(json!({ "op": "sync" })).await {
                self.log.push(format!("{label}: {e}"));
            }
        }
        // a second round: what one device wrote while another caught up
        for o in self.observers.iter_mut().filter(|o| o.signed_in) {
            let _ = o.call(json!({ "op": "sync" })).await;
        }
        for o in self.observers.iter_mut().filter(|o| o.signed_in) {
            match o.call(json!({ "op": "snapshot" })).await {
                Ok(v) => {
                    snaps.insert(o.name.clone(), v);
                }
                Err(e) => {
                    self.log.push(format!("{label}: snapshot {e}"));
                    snaps.insert(o.name.clone(), json!({ "snapshot_failed": tail(&e, 400) }));
                }
            }
        }
        for (i, a) in self.agents.iter().enumerate() {
            let core = a.client.core.lock().await;
            let mut cards = Map::new();
            for c in core.model.cards.values() {
                let answer = c.answer.as_ref().map(|x| {
                    let mut ch: Vec<String> = x
                        .choices
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                    ch.sort();
                    json!({ "action": x.answer_action, "choices": ch })
                });
                cards.insert(
                    c.object_id.clone(),
                    json!({ "state": c.object_state, "answer": answer }),
                );
            }
            let mut perms = Map::new();
            for p in core.model.permissions.values() {
                perms.insert(p.object_id.clone(), json!({ "state": p.permission_state }));
            }
            snaps.insert(
                format!("agent{i}"),
                json!({ "cards": cards, "permissions": perms }),
            );
        }
        let dir = out.join(label);
        std::fs::create_dir_all(&dir).unwrap();
        for (k, v) in &snaps {
            std::fs::write(
                dir.join(format!("{}.json", k.replace('@', "-"))),
                serde_json::to_string_pretty(v).unwrap(),
            )
            .unwrap();
        }
        snaps
    }
}

// ---- comparing -----------------------------------------------------------------------------------------------------

/// Every path where two JSON values differ, at most `max`.
fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<String>, max: usize) {
    if out.len() >= max {
        return;
    }
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            // (`debug`: what a device says of itself for the reader of a failure, never compared)
            let keys: BTreeSet<&String> = x
                .keys()
                .chain(y.keys())
                .filter(|k| k.as_str() != "debug")
                .collect();
            for k in keys {
                diff(
                    x.get(k).unwrap_or(&Value::Null),
                    y.get(k).unwrap_or(&Value::Null),
                    &format!("{path}/{k}"),
                    out,
                    max,
                );
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {} vs {}", short(a), short(b))),
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 {
        format!("{}…", &s[..160])
    } else {
        s
    }
}

/// The fields every platform has, for comparing platforms with each other.
fn common_view(snap: &Value) -> Value {
    let mut cards = Map::new();
    for (id, c) in snap["cards"].as_object().into_iter().flatten() {
        cards.insert(id.clone(), json!({ "state": c["state"], "answer": c["answer"], "in_revision": c["in_revision"], "held": c["held"], "title": c["title"] }));
    }
    let mut perms = Map::new();
    for (id, p) in snap["permissions"].as_object().into_iter().flatten() {
        perms.insert(
            id.clone(),
            json!({ "state": p["state"], "held": p["held"] }),
        );
    }
    let mut names = Map::new();
    for (id, s) in snap["sessions"].as_object().into_iter().flatten() {
        names.insert(id.clone(), s["agent_name"].clone());
    }
    json!({ "desk": snap["desk"], "permissions_open": snap["permissions_open"], "cards": cards, "permissions": perms, "sessions": names, "snoozed": snap["snoozed"] })
}

#[derive(Default)]
struct Report {
    failures: Vec<String>,
    checkpoints: usize,
    /// (the torture run) failures of known open bugs, not failing it
    known: Vec<String>,
}

/// The platform and tree of a device's name ("web@main2" → ("web", "main")).
fn platform_tree(name: &str) -> (&str, &str) {
    let (p, t) = name.split_once('@').unwrap_or((name, ""));
    (p, t.trim_end_matches(|c: char| c.is_ascii_digit()))
}

fn compare(label: &str, snaps: &BTreeMap<String, Value>, report: &mut Report) {
    report.checkpoints += 1;
    for (k, v) in snaps {
        if let Some(e) = v.get("snapshot_failed") {
            report
                .failures
                .push(format!("{label} SNAPSHOT {k} failed: {e}"));
        }
    }
    let devices: Vec<(&String, &Value)> = snaps
        .iter()
        .filter(|(k, v)| k.contains('@') && v.get("snapshot_failed").is_none())
        .collect();
    // VARIANCE: devices of the same app and tree, each against what most of them show (with many devices one
    // that differs is named once, not once per other device)
    let groups: BTreeSet<(&str, &str)> = devices.iter().map(|(k, _)| platform_tree(k)).collect();
    for group in groups {
        let mine: Vec<(&String, &Value)> = devices.iter().copied().filter(|(k, _)| platform_tree(k) == group).collect();
        let shown = |v: &Value| {
            let mut v = v.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("debug");
            }
            v.to_string()
        };
        let mut counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        for (i, (_, v)) in mine.iter().enumerate() {
            counts.entry(shown(v)).or_insert((0, i)).0 += 1;
        }
        // the most shown; on a tie the one of the first device
        let Some(&(_, at)) = counts.values().max_by_key(|(n, i)| (*n, std::cmp::Reverse(*i))) else {
            continue;
        };
        let (a, va) = mine[at];
        for (b, vb) in &mine {
            if b == &a {
                continue;
            }
            let mut d = Vec::new();
            diff(va, vb, "", &mut d, 8);
            for line in d {
                report
                    .failures
                    .push(format!("{label} VARIANCE {a} vs {b} {line}"));
            }
        }
    }
    // CROSS: web and iOS of the same tree, on the fields both show
    let trees: BTreeSet<&str> = devices.iter().map(|(k, _)| platform_tree(k).1).collect();
    for tree in &trees {
        let first = |p: &str| {
            devices
                .iter()
                .find(|(k, _)| platform_tree(k) == (p, *tree))
                .map(|(_, v)| *v)
        };
        if let (Some(w), Some(i)) = (first("web"), first("ios")) {
            let mut d = Vec::new();
            diff(&common_view(w), &common_view(i), "", &mut d, 12);
            for line in d {
                report
                    .failures
                    .push(format!("{label} CROSS web@{tree} vs ios@{tree} {line}"));
            }
        }
    }
    // REGRESSION: the branch against every other tree's devices of its platform
    for platform in ["web", "ios"] {
        let Some(b) = snaps.get(&format!("{platform}@branch")) else {
            continue;
        };
        let others: Vec<(&String, &Value)> = devices
            .iter()
            .copied()
            .filter(|(k, _)| platform_tree(k).0 == platform && platform_tree(k).1 != "branch")
            .collect();
        if others.is_empty() {
            continue;
        }
        let paths = |v: &Value| -> BTreeSet<String> {
            let mut d = Vec::new();
            diff(v, b, "", &mut d, 200);
            d.iter()
                .map(|l| l.split(": ").next().unwrap_or("").to_string())
                .collect()
        };
        let mut common = paths(others[0].1);
        for (_, v) in &others[1..] {
            common = common.intersection(&paths(v)).cloned().collect();
        }
        for p in common.iter().take(12) {
            report.failures.push(format!(
                "{label} REGRESSION {platform}@branch differs from every other device at {p}"
            ));
        }
    }
    // AGENT: what the agents hold against the person's devices (the first web device)
    let Some((_, r)) = devices
        .iter()
        .find(|(k, _)| k.starts_with("web@"))
        .or_else(|| devices.first())
    else {
        return;
    };
    for (k, v) in snaps.iter().filter(|(k, _)| k.starts_with("agent")) {
        for (id, c) in v["cards"].as_object().into_iter().flatten() {
            let Some(theirs) = r["cards"].get(id) else {
                continue;
            };
            if theirs["state"] != c["state"] {
                report.failures.push(format!(
                    "{label} AGENT {k} card {id}: {} vs {}",
                    c["state"], theirs["state"]
                ));
            }
            if !c["answer"].is_null()
                && !theirs["answer"].is_null()
                && theirs["answer"] != c["answer"]
            {
                report.failures.push(format!(
                    "{label} AGENT {k} card {id}: answer {} vs {}",
                    short(&c["answer"]),
                    short(&theirs["answer"])
                ));
            }
        }
        for (id, p) in v["permissions"].as_object().into_iter().flatten() {
            let Some(rp) = r["permissions"].get(id) else {
                continue;
            };
            let (m, t) = (
                p["state"].as_str().unwrap_or(""),
                rp["state"].as_str().unwrap_or(""),
            );
            // (an agent does not mark a request it never heard of again as expired: pending there is expired here)
            if m != t && !(m == "pending" && t == "expired") {
                report
                    .failures
                    .push(format!("{label} AGENT {k} request {id}: {m} vs {t}"));
            }
        }
    }
}

// ---- the hub -------------------------------------------------------------------------------------------------------

async fn start_hub() -> HubProc {
    // the hub of tests/connector/common, with the limits of the iOS mega run lifted
    for (k, v) in [
        ("HUB_LOGIN_THROTTLE", "off"),
        ("HUB_LIMIT_ENVELOPES_PER_SECOND", "100000"),
        ("HUB_LIMIT_ENVELOPE_BURST", "100000"),
        ("HUB_LIMIT_HEAVY_PER_SECOND", "10000"),
        ("HUB_LIMIT_HEAVY_BURST", "10000"),
        ("HUB_LIMIT_LOGINS_PER_IP_10MIN", "10000"),
        ("HUB_LIMIT_OPEN_REQUESTS_PER_IP_MINUTE", "100000"),
        ("HUB_LIMIT_PIECES_PER_SECOND", "10000"),
    ] {
        std::env::set_var(format!("MEGA_HUB_ENV_{k}"), v);
    }
    HubProc::start().await
}

// ---- one seed ------------------------------------------------------------------------------------------------------

async fn run_seed(seed: u64, trees: &[(String, PathBuf)], out: &Path, report: &mut Report) {
    let steps: usize = env("MEGA_STEPS").and_then(|s| s.parse().ok()).unwrap_or(60);
    let history: usize = env("MEGA_HISTORY")
        .and_then(|s| s.parse().ok())
        .unwrap_or(steps / 3);
    let agents_n: usize = env("MEGA_AGENTS").and_then(|s| s.parse().ok()).unwrap_or(2);
    let out = out.join(format!("seed-{seed}"));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let hub = start_hub().await;
    let code: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let keep = code.clone();
    let mut human = Human::found_with(&hub.url, move |_, c| {
        *keep.lock().unwrap() = Some(c.expose().to_vec());
        None
    })
    .await;
    let code_hex = trommi_connector::util::hex(&code.lock().unwrap().clone().expect("the code"));
    let room_hex = trommi_connector::util::hex(human.room.as_bytes());

    let mut agents = Vec::new();
    for i in 0..agents_n {
        let (client, drain, dir, admitted) = join_agent(&mut human).await;
        let group = human.found_session(&admitted).await;
        let session = trommi_connector::util::hex(group.session_id().unwrap().as_bytes());
        let c = client.clone();
        common::eventually("the agent is in its session", || async {
            c.core.lock().await.session_id().is_some()
        })
        .await;
        let _ = client
            .set_status(
                fields(json!({ "profile": { "agent_name": format!("S{i}"), "task": "start" } })),
                Some(session.clone()),
            )
            .await;
        agents.push(Agent {
            client,
            _events: drain,
            _dir: dir,
            session,
            helper: None,
        });
    }
    // a helper session of the first agent
    if let Ok(h) = agents[0]
        .client
        .open_child_session(fields(json!({ "agent_name": "helper of S0" })))
        .await
    {
        agents[0].helper = Some(h);
    }

    let mut world = World {
        rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
        human,
        agents,
        observers: Vec::new(),
        n: 0,
        room_hex,
        code_hex,
        hub_url: hub.url.clone(),
        log: Vec::new(),
        short_requests: BTreeSet::new(),
    };
    for _ in 0..history {
        let line = world.step().await;
        world.log.push(line);
    }

    // the person's other devices sign in now: a fresh login with history
    // (a second device of every tree but a branch: MEGA_TWINS=0 leaves it out)
    let mut devices: Vec<(String, PathBuf)> = trees.to_vec();
    if env("MEGA_TWINS").as_deref() != Some("0") {
        for (tree, root) in trees.iter().filter(|t| t.0 != "branch") {
            devices.push((format!("{tree}2"), root.clone()));
        }
    }
    for (tree, root) in &devices {
        if env("MEGA_WEB").as_deref() != Some("0") {
            world
                .observers
                .push(Observer::spawn("web", tree, root, &out, seed));
        }
        if env("MEGA_IOS").as_deref() != Some("0") {
            world
                .observers
                .push(Observer::spawn("ios", tree, root, &out, seed));
        }
    }
    // (the order of the sign-ins is the seed's: the devices that are in first answer the later ones' key requests)
    let mut order: Vec<usize> = (0..world.observers.len()).collect();
    for i in (1..order.len()).rev() {
        let j = world.rng.below(i + 1);
        order.swap(i, j);
    }
    for i in order {
        let fault = world.rng.chance(0.5);
        let (hub_url, room, code) = (
            world.hub_url.clone(),
            world.room_hex.clone(),
            world.code_hex.clone(),
        );
        let o = &mut world.observers[i];
        // (both trees of a platform get the same faults)
        if fault {
            let _ = o
                .call(json!({ "op": "faults", "fail": 0.05, "delay_ms": 120, "seed": seed }))
                .await;
        }
        match o
            .call(json!({ "op": "signin", "hub": hub_url, "room_id": room, "code": code }))
            .await
        {
            Ok(r) => {
                o.signed_in = true;
                world.log.push(format!("{} signed in: {r}", o.name));
            }
            Err(e) => {
                world.log.push(format!("SIGN-IN FAILED {e}"));
                report
                    .failures
                    .push(format!("seed {seed}: sign-in failed: {e}"));
            }
        }
    }
    let snaps = world.checkpoint("cp00-signed-in", &out).await;
    compare(&format!("seed {seed} cp00"), &snaps, report);

    // the rest, with every device acting; now and then one platform goes away and comes back with the history down
    let mut cp = 1;
    for s in 0..(steps - history) {
        let line = world.step().await;
        world.log.push(line);
        if world.rng.chance(0.08) && !world.observers.is_empty() {
            let platform = if world.rng.chance(0.5) { "web" } else { "ios" };
            world.log.push(format!(
                "{platform} goes away; the history route goes down when it comes back"
            ));
            for o in world
                .observers
                .iter_mut()
                .filter(|o| o.platform == platform && o.signed_in)
            {
                let _ = o.call(json!({ "op": "restart" })).await;
                let _ = o
                    .call(json!({ "op": "faults", "changes_down": true }))
                    .await;
            }
            for _ in 0..3 {
                let line = world.step().await;
                world.log.push(line);
            }
            for o in world
                .observers
                .iter_mut()
                .filter(|o| o.platform == platform && o.signed_in)
            {
                let _ = o.call(json!({ "op": "sync" })).await;
                let _ = o
                    .call(json!({ "op": "faults", "changes_down": false }))
                    .await;
            }
        }
        if (s + 1) % 10 == 0 {
            let label = format!("cp{cp:02}");
            let snaps = world.checkpoint(&label, &out).await;
            compare(&format!("seed {seed} {label}"), &snaps, report);
            cp += 1;
        }
    }
    // the short requests have run out by now
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    for o in world.observers.iter_mut().filter(|o| o.signed_in) {
        let _ = o
            .call(json!({ "op": "faults", "fail": 0.0, "delay_ms": 0, "changes_down": false }))
            .await;
    }
    let snaps = world.checkpoint("final", &out).await;
    compare(&format!("seed {seed} final"), &snaps, report);
    for o in world.observers.iter_mut() {
        let _ = o.call(json!({ "op": "quit" })).await;
    }
    std::fs::write(out.join("timeline.txt"), world.log.join("\n")).unwrap();
    for a in &world.agents {
        a.client.stop().await;
    }
    drop(hub);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn every_client_shows_the_same() {
    let trees: Vec<(String, PathBuf)> = env("MEGA_TREES")
        .unwrap_or_else(|| {
            format!(
                "this={}",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .display()
            )
        })
        .split(',')
        .map(|t| {
            let (name, path) = t.split_once('=').expect("name=path");
            (name.to_string(), PathBuf::from(path))
        })
        .collect();
    let seeds: Vec<u64> = env("MEGA_SEEDS")
        .unwrap_or_else(|| "1".into())
        .split(',')
        .map(|s| s.trim().parse().expect("a seed"))
        .collect();
    let out = PathBuf::from(env("MEGA_OUT").unwrap_or_else(|| {
        std::env::temp_dir()
            .join("trommi-mega-clients")
            .display()
            .to_string()
    }));
    std::fs::create_dir_all(&out).unwrap();
    let mut report = Report::default();
    for seed in &seeds {
        eprintln!("MEGA seed {seed} …");
        let before = report.failures.len();
        run_seed(*seed, &trees, &out, &mut report).await;
        eprintln!(
            "MEGA seed {seed}: {} differences",
            report.failures.len() - before
        );
    }
    let text = format!(
        "seeds {:?}, {} checkpoints\n\nDIFFERENCES ({}):\n{}\n",
        seeds,
        report.checkpoints,
        report.failures.len(),
        report.failures.join("\n")
    );
    std::fs::write(out.join("report.txt"), &text).unwrap();
    eprintln!("{}", tail(&text, 6000));
    assert!(
        report.failures.is_empty(),
        "{} differences: see {}",
        report.failures.len(),
        out.join("report.txt").display()
    );
}

// ---- speed --------------------------------------------------------------------------------------------------------

/// The speed of every client with a big Desk (MEGA_SPEED_CARDS, 1000): a fresh login (sign-in with the code, until
/// the whole Desk stands), a warm start (from the device's own store and cache), cards coming in (MEGA_SPEED_MORE
/// at once, per card), and the memory each client holds. The Linux engine (on the client core) catches up from
/// the start; the connector is timed reopening with its cards and taking the human's answers. Every tree in
/// MEGA_TREES is measured in the same room. Results: MEGA_OUT/speed.json and the test's output.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn speed_of_every_client() {
    let trees: Vec<(String, PathBuf)> = env("MEGA_TREES")
        .unwrap_or_else(|| {
            format!(
                "this={}",
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("..")
                    .display()
            )
        })
        .split(',')
        .map(|t| {
            let (name, path) = t.split_once('=').expect("name=path");
            (name.to_string(), PathBuf::from(path))
        })
        .collect();
    let n: usize = env("MEGA_SPEED_CARDS")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let more: usize = env("MEGA_SPEED_MORE")
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);
    let out = PathBuf::from(env("MEGA_OUT").unwrap_or_else(|| {
        std::env::temp_dir()
            .join("trommi-mega-clients")
            .display()
            .to_string()
    }));
    let work = out.join("speed");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let mut results = Map::new();
    let ms = |t: std::time::Instant| t.elapsed().as_millis() as u64;

    let hub = start_hub().await;
    let code: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let keep = code.clone();
    let mut human = Human::found_with(&hub.url, move |_, c| {
        *keep.lock().unwrap() = Some(c.expose().to_vec());
        None
    })
    .await;
    let code_hex = trommi_connector::util::hex(&code.lock().unwrap().clone().unwrap());
    let room_hex = trommi_connector::util::hex(human.room.as_bytes());
    // MEGA_SPEED_AGENTS agents (each its main session and a helper session: many groups, many Commits in the
    // history, as a real account has), the cards spread over their sessions
    let agents_n: usize = env("MEGA_SPEED_AGENTS")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let mut agents = Vec::new();
    for i in 0..agents_n {
        let (client, drain, dir, admitted) = join_agent(&mut human).await;
        let group = human.found_session(&admitted).await;
        let session = trommi_connector::util::hex(group.session_id().unwrap().as_bytes());
        let a = client.clone();
        common::eventually("the agent is in its session", || async {
            a.core.lock().await.session_id().is_some()
        })
        .await;
        let _ = client
            .set_status(
                fields(json!({ "profile": { "agent_name": format!("S{i}") } })),
                Some(session.clone()),
            )
            .await;
        let mut sessions = vec![session];
        if agents_n > 1 {
            if let Ok(h) = client
                .open_child_session(fields(json!({ "agent_name": format!("helper of S{i}") })))
                .await
            {
                sessions.push(h);
            }
        }
        agents.push((client, drain, dir, sessions));
    }
    eprintln!("SPEED {agents_n} agents in");
    let t = std::time::Instant::now();
    for i in 0..n {
        let (a, _, _, sessions) = &agents[i % agents_n];
        let card = json!({ "card_type": "decision", "title": format!("bulk {i}"), "body": "one of many", "urgency": "normal",
                           "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }], "allows_multiple": false });
        a.send_card(
            fields(card),
            Some(sessions[(i / agents_n) % sessions.len()].clone()),
        )
        .await
        .expect("a card");
    }
    for (a, _, _, _) in &agents {
        let _ = a.settle(120_000).await;
    }
    let (agent, _drain, _adir, sessions) = agents.remove(0);
    let session = sessions[0].clone();
    results.insert(
        "connector_writes_ms_per_card".into(),
        json!(ms(t) as f64 / n as f64),
    );
    eprintln!("SPEED {n} cards written in {} ms", ms(t));

    let mut observers = Vec::new();
    for (tree, root) in &trees {
        for platform in ["web", "ios"] {
            if env(&format!("MEGA_{}", platform.to_uppercase())).as_deref() == Some("0") {
                continue;
            }
            observers.push(Observer::spawn(platform, tree, root, &work, 1));
        }
    }
    // (the devices that sign in first answer the later ones' key requests: MEGA_SPEED_REVERSE=1 turns the order)
    if env("MEGA_SPEED_REVERSE").as_deref() == Some("1") {
        observers.reverse();
    }
    let stats = |v: &Value| json!({ "rss_mb": v["rss"].as_f64().map(|r| (r / 1_048_576.0 * 10.0).round() / 10.0), "desk": v["desk"], "cards": v["cards"] });
    for o in observers.iter_mut() {
        // (one at a time: each has the machine to itself)
        let t = std::time::Instant::now();
        let signed = o
            .call(json!({ "op": "signin", "hub": hub.url, "room_id": room_hex, "code": code_hex }))
            .await;
        let fresh = ms(t);
        let s = o
            .call(json!({ "op": "stats", "reset": true }))
            .await
            .unwrap_or(Value::Null);
        let mut r = Map::new();
        r.insert("fresh_login_ms".into(), json!(fresh));
        r.insert("fresh_login_requests".into(), s["requests"].clone());
        r.insert("after_login".into(), stats(&s));
        if let Err(e) = signed {
            r.insert("error".into(), json!(e));
        }
        let t = std::time::Instant::now();
        let _ = o.call(json!({ "op": "restart" })).await;
        let restored = ms(t);
        let s = o
            .call(json!({ "op": "stats" }))
            .await
            .unwrap_or(Value::Null);
        r.insert("warm_open_ms".into(), json!(restored));
        r.insert("desk_after_warm_open".into(), s["desk"].clone());
        let t = std::time::Instant::now();
        let _ = o.call(json!({ "op": "sync" })).await;
        r.insert("warm_sync_ms".into(), json!(ms(t)));
        let s = o
            .call(json!({ "op": "stats", "reset": true }))
            .await
            .unwrap_or(Value::Null);
        r.insert("warm_requests".into(), s["requests"].clone());
        results.insert(o.name.clone(), Value::Object(r));
        eprintln!("SPEED {}: {}", o.name, results[&o.name]);
    }
    // cards coming in
    for i in 0..more {
        let card = json!({ "card_type": "decision", "title": format!("more {i}"), "body": "new", "urgency": "normal",
                           "options": [{ "key": "a", "label": "A" }], "allows_multiple": false });
        agent
            .send_card(fields(card), Some(session.clone()))
            .await
            .expect("a card");
    }
    let _ = agent.settle(60_000).await;
    for o in observers.iter_mut() {
        let t = std::time::Instant::now();
        let _ = o.call(json!({ "op": "sync" })).await;
        let took = ms(t);
        let s = o
            .call(json!({ "op": "stats", "reset": true }))
            .await
            .unwrap_or(Value::Null);
        let r = results.get_mut(&o.name).unwrap().as_object_mut().unwrap();
        r.insert("incoming_requests".into(), s["requests"].clone());
        r.insert(
            "incoming_ms_per_card".into(),
            json!(took as f64 / more as f64),
        );
        r.insert("after_incoming".into(), stats(&s));
    }
    // what is measured so far is written before the connector's part
    std::fs::write(
        out.join("speed.json"),
        serde_json::to_string_pretty(&Value::Object(results.clone())).unwrap(),
    )
    .unwrap();
    // the connector: reopened with its cards, then the human's answers coming in. (Its event drain holds the client:
    // it goes first, or the journal's lock is never let go.)
    agent.stop().await;
    _drain.abort();
    let _ = _drain.await;
    let weak = Arc::downgrade(&agent);
    drop(agent);
    common::eventually("the old client is gone", || async {
        weak.strong_count() == 0
    })
    .await;
    let journal = loop {
        match Journal::open(_adir.path()) {
            Ok(j) => break j,
            Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    };
    let t = std::time::Instant::now();
    let (agent, mut events) = Client::open(journal).await.expect("reopened");
    agent.start().await.expect("online");
    let _ = agent.settle(60_000).await;
    results.insert("connector_reopen_ms".into(), json!(ms(t)));
    let ids: Vec<String> = agent
        .core
        .lock()
        .await
        .model
        .cards
        .values()
        .filter(|c| c.object_state == "open")
        .take(more)
        .map(|c| c.object_id.clone())
        .collect();
    human.sync().await;
    let known = human.object_ids();
    let ids: Vec<String> = ids.into_iter().filter(|id| known.contains(id)).collect();
    // The human answers them one by one (each a catch-up and a write of the stand-in); meanwhile a watcher notes
    // when the agent first holds each one answered. The lag of an answer: from the hub's taking it to the agent's
    // holding it. (Before: the time after the last answer was posted, when the agent held most of them already.)
    let taken_at: Arc<Mutex<BTreeMap<String, std::time::Instant>>> = Arc::default();
    let watch = {
        let (agent, ids, taken_at) = (agent.clone(), ids.clone(), taken_at.clone());
        tokio::spawn(async move {
            loop {
                let open: Vec<String> = {
                    let core = agent.core.lock().await;
                    let now = std::time::Instant::now();
                    let mut taken = taken_at.lock().unwrap();
                    for id in &ids {
                        if !taken.contains_key(id)
                            && core
                                .model
                                .cards
                                .get(id)
                                .is_some_and(|c| c.object_state != "open")
                        {
                            taken.insert(id.clone(), now);
                        }
                    }
                    ids.iter()
                        .filter(|id| !taken.contains_key(*id))
                        .cloned()
                        .collect()
                };
                if open.is_empty() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
    };
    let t = std::time::Instant::now();
    let mut sent_at = BTreeMap::new();
    let mut refused = 0;
    for id in &ids {
        match human
            .answer(
                id,
                json!({ "schema_version": 2, "answer_action": "answer", "choices": ["a"] }),
                &["a"],
                false,
            )
            .await
        {
            Ok(()) => {
                sent_at.insert(id.clone(), std::time::Instant::now());
            }
            Err(e) => {
                if refused == 0 {
                    eprintln!("SPEED the stand-in's answer was refused: {e:?}");
                }
                refused += 1;
            }
        }
    }
    // (until the agent holds every answer that was posted, at most two minutes)
    let wait = std::time::Instant::now();
    while wait.elapsed() < Duration::from_secs(120)
        && !sent_at
            .keys()
            .all(|id| taken_at.lock().unwrap().contains_key(id))
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    watch.abort();
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(1), events.recv()).await {}
    let taken_at = taken_at.lock().unwrap().clone();
    // (an answer the agent held before the stand-in had its write back counts as 0 ms)
    let mut lags: Vec<u64> = sent_at
        .iter()
        .filter_map(|(id, sent)| {
            taken_at
                .get(id)
                .map(|taken| taken.saturating_duration_since(*sent).as_millis() as u64)
        })
        .collect();
    lags.sort();
    let pct = |p: usize| {
        lags.get((lags.len() * p / 100).min(lags.len().max(1) - 1))
            .copied()
    };
    results.insert("connector_answers_sent".into(), json!(sent_at.len()));
    results.insert(
        "connector_answers_taken".into(),
        json!(sent_at
            .keys()
            .filter(|id| taken_at.contains_key(*id))
            .count()),
    );
    results.insert(
        "connector_answers_lag_ms".into(),
        json!({ "median": pct(50), "p90": pct(90), "max": lags.last() }),
    );
    // from the first answer to the agent holding the last one
    results.insert(
        "connector_answers_all_ms".into(),
        json!(taken_at
            .values()
            .max()
            .map(|last| last.saturating_duration_since(t).as_millis() as u64)),
    );
    if !human.findings.is_empty() {
        // the stand-in could not follow the room: its answers measure nothing
        results.insert("stand_in_findings".into(), json!(human.findings));
    }
    for o in observers.iter_mut() {
        let _ = o.call(json!({ "op": "quit" })).await;
    }
    let text = serde_json::to_string_pretty(&Value::Object(results)).unwrap();
    std::fs::write(out.join("speed.json"), &text).unwrap();
    eprintln!("SPEED RESULTS\n{text}");
    agent.stop().await;
    drop(hub);
}

// ---- helper names after a fresh sign-in ----------------------------------------------------------------------------

/// The names of an agent's helper sessions on a device that signs in fresh, with the code, long after the helpers
/// were opened (the person's report of 2026-10-13: after a new login on the web every helper showed its folder's name
/// instead of its own, CI, Mac, Web …). The helpers write cards and status lines after their founding, and a first web
/// device and the iPhones (when built) are in before the second web device signs in. Each device must show every
/// helper under the name it was opened with, at once and after a sync.
///
/// ```text
/// tests/client/mega/prepare.sh .     # (core/wasm/build.sh is enough with MEGA_IOS=0)
/// cargo test -p trommi-tests --test client_mega helper_names -- --ignored --nocapture
/// ```
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn helper_names_after_a_fresh_signin() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let helpers_n: usize = num_env("MEGA_HELPERS", 12);
    let out = std::env::temp_dir().join("trommi-mega-helper-names");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let hub = start_hub().await;
    let code: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let keep = code.clone();
    let mut human = Human::found_with(&hub.url, move |_, c| {
        *keep.lock().unwrap() = Some(c.expose().to_vec());
        None
    })
    .await;
    let code_hex = trommi_connector::util::hex(&code.lock().unwrap().clone().expect("the code"));
    let room_hex = trommi_connector::util::hex(human.room.as_bytes());
    let (client, _drain, _dir, admitted) = join_agent(&mut human).await;
    let group = human.found_session(&admitted).await;
    let main = trommi_connector::util::hex(group.session_id().unwrap().as_bytes());
    let c = client.clone();
    common::eventually("the agent is in its session", || async {
        c.core.lock().await.session_id().is_some()
    })
    .await;
    client
        .set_status(
            fields(json!({ "profile": { "agent_name": "trommi", "task": "lead" } })),
            Some(main.clone()),
        )
        .await
        .expect("the main session's profile");
    let mut want: BTreeMap<String, String> = BTreeMap::new();
    want.insert(main.clone(), "trommi".into());
    for i in 0..helpers_n {
        let name = format!("Helper{i}");
        let sid = client
            .open_child_session(fields(json!({ "agent_name": name, "task": "work" })))
            .await
            .expect("a helper session");
        // what a helper does after its founding: a status line, a card
        let _ = client
            .set_status(
                fields(
                    json!({ format!("status_line/{i}"): { "label": "work", "state": "working" } }),
                ),
                Some(sid.clone()),
            )
            .await;
        want.insert(sid, name);
    }
    human.sync().await;
    let _ = client.settle(60_000).await;

    let shown_names = |list: &Value| -> BTreeMap<String, String> {
        list.as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|e| {
                (
                    e["session"].as_str().unwrap_or("").to_string(),
                    e["name"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    };
    let mut observers = vec![Observer::spawn("web", "first", &root, &out, 1)];
    let ios = root
        .join("tests/ios/.build/out/Products/Debug-linux-x86_64/TrommiCoreLiveTests-test-runner")
        .exists()
        && env("MEGA_IOS").as_deref() != Some("0");
    if ios {
        observers.push(Observer::spawn("ios", "phone", &root, &out, 1));
    }
    observers.push(Observer::spawn("web", "second", &root, &out, 1));
    let mut failures = Vec::new();
    for o in observers.iter_mut() {
        // the last device's requests are slow: the devices that are in add it to the helper sessions (5.2.7)
        // before its own joins with the code reach them
        if o.name.ends_with("@second") {
            let delay: u64 = num_env("MEGA_SLOW_MS", 400) as u64;
            let _ = o
                .call(json!({ "op": "faults", "delay_ms": delay, "seed": 1 }))
                .await;
        }
        o.call(json!({ "op": "signin", "hub": hub.url, "room_id": room_hex, "code": code_hex, "stream": true }))
            .await
            .unwrap_or_else(|e| panic!("{} signs in: {e}", o.name));
        o.signed_in = true;
        // the helpers go on working while the device is in
        for (n, sid) in want.keys().filter(|s| **s != main).enumerate().take(3) {
            let _ = client
                .set_status(
                    fields(
                        json!({ format!("status_line/{n}"): { "label": "work", "state": "done" } }),
                    ),
                    Some(sid.clone()),
                )
                .await;
        }
    }
    for o in observers.iter_mut() {
        let _ = o.call(json!({ "op": "sync" })).await;
        let got = shown_names(&o.call(json!({ "op": "shown" })).await.expect("shown"));
        for (sid, name) in &want {
            match got.get(sid) {
                Some(n) if n == name => {}
                other => failures.push(format!(
                    "{}: {} shows as {:?}, want {name:?}",
                    o.name,
                    &sid[..8],
                    other
                )),
            }
        }
    }
    for o in observers.iter_mut() {
        let _ = o.call(json!({ "op": "quit" })).await;
    }
    client.stop().await;
    drop(hub);
    assert!(
        failures.is_empty(),
        "helper names lost:\n{}",
        failures.join("\n")
    );
}

fn num_env(name: &str, default: usize) -> usize {
    env(name).and_then(|s| s.parse().ok()).unwrap_or(default)
}
