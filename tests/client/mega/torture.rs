//! The torture run of the mega test: every client of the person in one room on a real hub behind a misbehaving
//! network (tests/client/mega/chaos.rs), through one seeded timeline that does what broke before, again and again,
//! and checks at every checkpoint that every client agrees.
//!
//! In the room, per seed:
//! - the stand-in human device (tests/connector/common) founds it and invites;
//! - the web app: the app core in the web's executor (web-observer.mjs), several devices, signed in with the
//!   account's password, following the live stream;
//! - a fleet of iPhones: the iOS app's engine with the real core, N devices in one process (MegaFleetTests.swift),
//!   joined one by one through "Pair a device" (link and six emoji), the first with the recovery code;
//! - agents: the REAL connector binary, each an MCP server on stdio in a folder of its own, joined every way there is.
//!
//! The timeline (each step drawn by the seed):
//! - an agent joins: by the connect tool (MCP), by `trommi-connector connect` in its shell while its server runs, by
//!   asking to join on its own (a folder without a key, vouched by a session of the same machine; Allow or Deny on a
//!   human device), with the invite made on the stand-in, a web device or an iPhone; the inviter may die right after
//!   "They match" (a tab closed, an app killed): another human device must found the new session;
//! - the same folder twice (a second Claude Code session there), helper sessions opened and closed (up to 100);
//! - presence: human messages, /mcp restarts, hook, `say` and `monitor` subprocesses, killed servers (their lease
//!   runs out), archive;
//! - cards from the agents and answers from every human device;
//! - iPhones killed, offline for a while, reinstalled (paired anew), restarted warm;
//! - chaos: latency, cut streams, duplicate stream events, the hub away, the hub
//!   restarted.
//!
//! At every checkpoint it fails on:
//! - SESSIONS: two human devices show another session list (which sessions, their names, online or gone, their
//!   parent, their order: the sidebar's own projection, web `view.sidebar.sessions` of the app core, iOS `DeskModel.agents`);
//! - TRUTH: a live agent is missing from that list, shown under another name, or shown gone; a dead one shown
//!   online; a denied one shown at all;
//! - PRESENCE: a web device saw a live agent go offline outside the moments it was really away (a flicker);
//! - VARIANCE / CROSS: cards, requests, answers and the Desk differ between devices (as `every_client_shows_the_same`);
//! - BUDGET: a join, a sign-in, a pairing or a catch-up took longer than its budget (`MEGA_BUDGET_SCALE` scales them).
//!
//! Every failure names its seed; `MEGA_SEEDS=<seed>` repeats the timeline.
//!
//! ```text
//! tests/client/mega/prepare.sh .
//! MEGA_SEEDS=7 MEGA_FLEET=10 cargo test -p trommi-tests --test client_mega torture -- --ignored --nocapture
//! ```
//! Sizes: `MEGA_FLEET` iPhones (10; nightly 100), `MEGA_WEB` web devices (2), `MEGA_AGENTS` agents (8), `MEGA_STEPS`
//! steps (60), `MEGA_CARDS` cards written before the first checkpoint (200; nightly 1000), `MEGA_KILLS=0` leaves out
//! the killed servers (each waits for a lease to run out, a minute), `MEGA_CHAOS=0` the misbehaving network (`MEGA_CHAOS=cut,down`: only those of latency, cut, dup, down, restart),
//! `MEGA_IOS=0` the fleet, `MEGA_WEB=0` the web devices; `MEGA_FLEET_RUNNER` runs the fleet on another machine (tests/client/mega/fleet-remote.py).
use super::chaos::{Knobs, Proxy};
use super::common::process::{Mcp, Seat};
use super::*;
use std::time::Instant;

const LEASE_GONE_MS: u64 = 60_000 + 20_000;

fn num(name: &str, default: usize) -> usize {
    env(name).and_then(|s| s.parse().ok()).unwrap_or(default)
}

/// Whether the iPhones follow the live stream (MEGA_FLEET_LIVE=0: they catch up at each step only).
fn fleet_live() -> bool {
    env("MEGA_FLEET_LIVE").as_deref() != Some("0")
}

fn budget(ms: u64) -> u64 {
    let scale: f64 = env("MEGA_BUDGET_SCALE").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    (ms as f64 * scale) as u64
}

fn now() -> u64 {
    now_ms()
}

/// How an agent came in.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Way {
    Tool,
    Cli,
    SelfJoin,
    SameFolder,
}

/// One Claude Code session with the real connector.
struct Agent {
    /// Its folder's name: the name its session must show under.
    name: String,
    /// The machine it runs on: its own seat, or (asked to join on its own) the seat of agent `host`.
    seat: Option<Seat>,
    host: usize,
    folder: PathBuf,
    key: String,
    mcp: Option<Mcp>,
    way: Way,
    /// The session it holds, once a human device shows it.
    session: Option<String>,
    /// Its device (hex), once known.
    device: Option<String>,
    /// Joined and its server running.
    alive: bool,
    /// Never let in (self-join denied): it must show nowhere.
    denied: bool,
    /// When its server was killed (its lease runs out after).
    killed_at: Option<u64>,
    /// Moments it was really away (a restart): (from, to) in ms.
    away: Vec<(u64, u64)>,
    helpers: Vec<String>,
}

/// A human device of the person.
#[derive(Clone)]
enum HumanDev {
    Web(usize),
    Phone(usize),
    StandIn,
}

struct Torture {
    seed: u64,
    rng: Rng,
    proxy: Proxy,
    hub: Option<HubProc>,
    human: Human,
    code_hex: String,
    room_hex: String,
    out: PathBuf,
    root: PathBuf,
    web: Vec<Observer>,
    web_made: usize,
    fleet: Option<Observer>,
    /// Per iPhone: alive (its app running), and whether it ever joined.
    phones: Vec<(bool, bool)>,
    agents: Vec<Agent>,
    log: Vec<String>,
    fails: Vec<String>,
    /// Failures of a known open bug (`known_bug`): reported, not failing the run (MEGA_STRICT=1: failing).
    known: Vec<String>,
    metrics: BTreeMap<String, Vec<u64>>,
    t0: Instant,
}

impl Torture {
    fn say(&mut self, line: String) {
        let at = self.t0.elapsed().as_millis();
        eprintln!("[seed {} +{at} ms] {line}", self.seed);
        self.log.push(format!("+{at} {line}"));
    }
    fn fail(&mut self, what: String) {
        if let (Some(bug), false) = (known_bug(&what), env("MEGA_STRICT").as_deref() == Some("1")) {
            let line = format!("seed {}: [{bug}] {what}", self.seed);
            eprintln!("KNOWN {line}");
            self.log.push(format!("KNOWN [{bug}] {what}"));
            self.known.push(line);
            return;
        }
        let line = format!("seed {}: {what}", self.seed);
        eprintln!("FAIL {line}");
        self.log.push(format!("FAIL {what}"));
        self.fails.push(line);
    }
    fn metric(&mut self, name: &str, ms: u64, limit: Option<u64>) {
        self.metrics.entry(name.to_string()).or_default().push(ms);
        if let Some(l) = limit {
            if ms > budget(l) {
                self.fail(format!("BUDGET {name}: {ms} ms > {} ms", budget(l)));
            }
        }
    }

    fn humans(&self) -> Vec<HumanDev> {
        let mut v = vec![HumanDev::StandIn];
        for i in 0..self.web.len() {
            v.push(HumanDev::Web(i));
        }
        for (i, p) in self.phones.iter().enumerate() {
            if p.0 {
                v.push(HumanDev::Phone(i));
            }
        }
        v
    }

    fn live_web(&self) -> usize {
        self.web.len()
    }

    async fn phone(&mut self, cmd: Value) -> Result<Value, String> {
        match self.fleet.as_mut() {
            Some(f) => f.call(cmd).await,
            None => Err("no fleet".into()),
        }
    }

    // ---- human devices ---------------------------------------------------------------------------------------------

    async fn add_web(&mut self) {
        let k = self.web_made;
        self.web_made += 1;
        let tree = if k == 0 { "torture".to_string() } else { format!("torture{}", k + 1) };
        let mut o = Observer::spawn("web", &tree, &self.root, &self.out, self.seed);
        let t = now();
        let r = o
            .call(json!({ "op": "signin", "hub": self.proxy.url, "room_id": self.room_hex, "code": self.code_hex, "email": MEGA_EMAIL, "password": MEGA_PASSWORD, "stream": true }))
            .await;
        match r {
            Ok(_) => {
                o.signed_in = true;
                let ms = now() - t;
                self.say(format!("{} signed in in {ms} ms", o.name));
                self.metric("web_signin_ms", ms, Some(30_000));
                self.web.push(o);
            }
            Err(e) => self.fail(format!("web sign-in: {e}")),
        }
    }

    async fn start_fleet(&mut self, n: usize) {
        // (MEGA_FLEET_RUNNER: a command that runs the fleet elsewhere and relays the command files, e.g. on a Mac)
        let runner = env("MEGA_FLEET_RUNNER").map(PathBuf::from).unwrap_or_else(|| {
            self.root.join("tests/ios/.build/out/Products/Debug-linux-x86_64/TrommiCoreLiveTests-test-runner")
        });
        if !runner.is_file() {
            self.say("no iOS test runner built: the fleet is left out (tests/client/mega/prepare.sh)".into());
            return;
        }
        let dir = self.out.join("fleet");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = std::fs::File::create(dir.join("log.txt")).unwrap();
        let child = Command::new(&runner)
            .arg("TrommiCoreLiveTests.MegaFleetTests/testFleet")
            .env("TROMMI_MEGA_FLEET", &dir)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .expect("the iOS test runner");
        self.fleet = Some(Observer {
            name: "ios@fleet".into(),
            platform: "ios",
            dir,
            child,
            n: 0,
            signed_in: true,
        });
        // the first with the code, every other one paired by one that is in (the newest that is: a chain)
        let t = now();
        let r = self
            .phone(json!({ "op": "code", "dev": 0, "hub": self.proxy.url, "room_id": self.room_hex, "code": self.code_hex, "live": fleet_live() }))
            .await;
        match r {
            Ok(v) => {
                self.phones.push((true, true));
                self.metric("phone_signin_ms", now() - t, Some(30_000));
                self.say(format!("iPhone 0 signed in with the code: {v}"));
            }
            Err(e) => {
                self.fail(format!("iPhone 0 sign-in: {e}"));
                self.fleet = None;
                return;
            }
        }
        for i in 1..n {
            let by = self.rng.below(i);
            self.pair_phone(i, by, "pair").await;
        }
    }

    async fn pair_phone(&mut self, i: usize, by: usize, op: &str) {
        let t = now();
        match self.phone(json!({ "op": op, "dev": i, "by": by, "live": fleet_live() })).await {
            Ok(v) => {
                if i >= self.phones.len() {
                    self.phones.resize(i + 1, (false, false));
                }
                self.phones[i] = (true, true);
                let ms = now() - t;
                self.metric(&format!("phone_{op}_ms"), ms, Some(60_000));
                if let Some(j) = v["joined_ms"].as_u64() {
                    self.metric("phone_pair_joined_ms", j, None);
                }
                self.say(format!("iPhone {i} {op} by iPhone {by} in {ms} ms"));
            }
            Err(e) => {
                // (a reinstall wiped the phone first: it is gone until a later one works)
                if i < self.phones.len() && op == "reinstall" {
                    self.phones[i].0 = false;
                }
                self.fail(format!("iPhone {i} {op} by iPhone {by}: {e}"))
            }
        }
    }

    /// A human device that is in and alive, drawn by the seed, among those that can do `what`.
    fn pick_human(&mut self) -> HumanDev {
        let all = self.humans();
        all[self.rng.below(all.len())].clone()
    }

    // ---- agents ----------------------------------------------------------------------------------------------------

    /// The connector's command on agent `k`'s machine, in its folder and session.
    fn cmd(&self, k: usize, args: &[&str]) -> tokio::process::Command {
        let a = &self.agents[k];
        let seat = self.agents[a.host].seat.as_ref().expect("the machine's seat");
        let mut c = seat.command(args);
        c.env("TROMMI_FOLDER", &a.folder).env("TROMMI_SESSION_KEY", &a.key).current_dir(&a.folder);
        c
    }

    /// One of the plugin's hooks on agent `k`'s machine, with `input` on its stdin.
    async fn hook(&self, k: usize, kind: &str, input: &Value) {
        let seat = self.agents[self.agents[k].host].seat.as_ref().expect("the seat");
        let mut c = self.cmd(k, &[kind]);
        c.env("CLAUDE_PLUGIN_ROOT", seat.home.path()).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        let Ok(mut child) = c.spawn() else { return };
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(input.to_string().as_bytes()).await;
        }
        let _ = tokio::time::timeout(Duration::from_secs(60), child.wait()).await;
    }

    async fn serve(&self, k: usize, more: &[(&str, &str)]) -> Mcp {
        let mut c = self.cmd(k, &[]);
        for (a, b) in more {
            c.env(a, b);
        }
        // (its stderr, every server of the session after the other, in the run's output)
        let dir = self.out.join("agents");
        let _ = std::fs::create_dir_all(&dir);
        let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{}-{k}.log", self.agents[k].name))).ok();
        Mcp::start_logged(c, json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "test", "version": "0" } }), log).await
    }

    fn new_seat(&self, name: &str) -> Seat {
        let mut seat = Seat::new(&self.proxy.url);
        let folder = seat.home.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        seat.folder = folder;
        seat
    }

    /// An invite link made on `by`; with the stand-in, its invite to admit later.
    async fn make_invite(&mut self, by: &HumanDev) -> Result<(String, Option<String>, Option<common::Invite>), String> {
        match by {
            HumanDev::StandIn => {
                let inv = self.human.invite(None).await;
                Ok((inv.link.clone(), None, Some(inv)))
            }
            HumanDev::Web(i) => {
                let v = self.web[*i].call(json!({ "op": "invite_agent" })).await?;
                Ok((v["link"].as_str().unwrap_or("").to_string(), v["invite_id"].as_str().map(String::from), None))
            }
            HumanDev::Phone(i) => {
                let v = self.phone(json!({ "op": "invite_agent", "dev": i })).await?;
                Ok((v["link"].as_str().unwrap_or("").to_string(), v["invite_id"].as_str().map(String::from), None))
            }
        }
    }

    /// "They match" on `by`; `die`: the device is gone right after it.
    async fn confirm(&mut self, by: &HumanDev, id: Option<String>, inv: Option<common::Invite>, die: bool) -> Result<Option<String>, String> {
        match by {
            HumanDev::StandIn => {
                let mut inv = inv.expect("the stand-in's invite");
                let admitted = self.human.admit(&mut inv).await;
                if !die {
                    self.human.found_session(&admitted).await;
                }
                Ok(Some(trommi_connector::util::hex(admitted.device.as_bytes())))
            }
            HumanDev::Web(i) => {
                let v = self.web[*i].call(json!({ "op": "confirm_invite", "invite_id": id, "die": die })).await?;
                if die {
                    let gone = self.web.remove(*i);
                    self.say(format!("{} closed right after \"They match\"", gone.name));
                    drop(gone);
                }
                Ok(v["agent"].as_str().map(String::from))
            }
            HumanDev::Phone(i) => {
                let v = self.phone(json!({ "op": "confirm_invite", "dev": i, "invite_id": id, "die": die })).await?;
                if die {
                    self.phones[*i].0 = false;
                    self.say(format!("iPhone {i} killed right after \"They match\""));
                }
                Ok(v["agent"].as_str().map(String::from))
            }
        }
    }

    /// A new agent joins by link: the connect tool or `trommi-connector connect` while its server runs.
    async fn join_by_link(&mut self, way: Way) {
        let k = self.agents.len();
        let name = format!("agent-{k}");
        let seat = self.new_seat(&name);
        let (folder, key) = (seat.folder.clone(), seat.session_key.clone());
        self.agents.push(Agent {
            name: name.clone(),
            seat: Some(seat),
            host: k,
            folder,
            key,
            mcp: None,
            way,
            session: None,
            device: None,
            alive: false,
            denied: false,
            killed_at: None,
            away: Vec::new(),
            helpers: Vec::new(),
        });
        let mut mcp = self.serve(k, &[]).await;
        // the inviter, and whether it dies after "They match" (only when another device can take over the founding:
        // a second web device; the iOS app does not take over yet, see spec/v1.md 5.2.5)
        let mut by = self.pick_human();
        let others_web = self.live_web().saturating_sub(if matches!(by, HumanDev::Web(_)) { 1 } else { 0 });
        let die = others_web > 0 && self.rng.chance(0.35);
        if matches!(by, HumanDev::Phone(_)) && self.fleet.is_none() {
            by = HumanDev::StandIn;
        }
        let made = self.make_invite(&by).await;
        let (link, id, inv) = match made {
            Ok(x) => x,
            Err(e) => return self.fail(format!("{name}: invite on {} failed: {e}", self.dev_name(&by))),
        };
        let mut connect = self.cmd(k, &["connect", &link]);
        let t = now();
        let by_name = self.dev_name(&by);
        self.say(format!("{name} joins by {way:?}, invited on {by_name}{}", if die { ", which dies after They match" } else { "" }));
        let joining = async {
            match way {
                Way::Tool => {
                    let (said, failed) = tokio::time::timeout(Duration::from_secs(120), mcp.call("connect", json!({ "link": link })))
                        .await
                        .unwrap_or(("timed out".into(), true));
                    (!failed, said)
                }
                _ => {
                    let out = tokio::time::timeout(Duration::from_secs(120), connect.stdin(Stdio::null()).output()).await;
                    match out {
                        Ok(Ok(o)) => (o.status.success(), format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))),
                        Ok(Err(e)) => (false, e.to_string()),
                        Err(_) => (false, "timed out".into()),
                    }
                }
            }
        };
        // (the stand-in's admit borrows the world; the others answer from their own processes)
        let (joined_at, confirmed_at) = (std::cell::Cell::new(0), std::cell::Cell::new(0));
        let ((ok, said), confirmed) = {
            let joining = async {
                let r = joining.await;
                joined_at.set(now());
                r
            };
            let confirming = async {
                let r = self.confirm(&by, id, inv, die).await;
                confirmed_at.set(now());
                r
            };
            tokio::join!(joining, confirming)
        };
        self.log.push(format!(
            "{name}: its join returned after {} ms, \"They match\" after {} ms",
            joined_at.get().saturating_sub(t),
            confirmed_at.get().saturating_sub(t)
        ));
        let device = match confirmed {
            Ok(d) => d,
            Err(e) => {
                self.fail(format!("{name}: \"They match\" on {by_name} failed: {e}"));
                None
            }
        };
        if !ok {
            self.fail(format!("{name}: the join by {way:?} failed: {}", tail(&said, 600)));
        } else {
            self.log.push(format!("{name}: {way:?} said: {}", tail(&said, 400).replace('\n', " | ")));
        }
        self.agents[k].mcp = Some(mcp);
        self.agents[k].device = device;
        self.agents[k].alive = ok;
        if ok {
            self.await_online(k, t, if die { "join_after_dead_inviter_ms" } else { "join_online_ms" }).await;
        }
    }

    /// Waits until a human device shows agent `k`'s session online and named; notes the time from `t`.
    async fn await_online(&mut self, k: usize, t: u64, metric: &str) {
        let limit = if metric == "join_after_dead_inviter_ms" { 60_000 } else { 20_000 };
        let end = now() + budget(limit) + 10_000;
        let name = self.agents[k].name.clone();
        let taken: BTreeSet<String> = self.agents.iter().filter_map(|a| a.session.clone()).collect();
        while now() < end {
            if let Some(list) = self.any_shown().await {
                if let Some(e) = list.iter().find(|e| e["name"] == name.as_str() && !taken.contains(e["session"].as_str().unwrap_or("")) && e["online"] == true) {
                    self.agents[k].session = e["session"].as_str().map(String::from);
                    self.metric(metric, now() - t, Some(limit));
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        // (what the list shows of its device, if anything: a session without its name, offline, or none)
        let device = self.agents[k].device.clone();
        let seen: Vec<Value> = self
            .any_shown()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|e| device.as_deref().is_some_and(|d| e["agent"].as_str() == Some(d)))
            .collect();
        self.fail(format!("TRUTH {name}: never shown online under its folder's name within {} ms (its device: {})", end - t, short(&json!(seen))));
    }

    /// The session list of the first human device that answers (web first).
    async fn any_shown(&mut self) -> Option<Vec<Value>> {
        for o in self.web.iter_mut() {
            if let Ok(v) = o.call(json!({ "op": "shown" })).await {
                return v.as_array().cloned();
            }
        }
        let alive: Vec<usize> = self.phones.iter().enumerate().filter(|(_, p)| p.0).map(|(i, _)| i).collect();
        for i in alive {
            let _ = self.phone(json!({ "op": "sync", "dev": i })).await;
            if let Ok(v) = self.phone(json!({ "op": "shown", "dev": i })).await {
                return v.as_array().cloned();
            }
        }
        None
    }

    fn dev_name(&self, d: &HumanDev) -> String {
        match d {
            HumanDev::StandIn => "the stand-in".into(),
            HumanDev::Web(i) => self.web.get(*i).map(|o| o.name.clone()).unwrap_or_default(),
            HumanDev::Phone(i) => format!("iPhone {i}"),
        }
    }

    /// A new session asks to join on its own: a folder without a key on the machine of a running agent, or the same
    /// folder as that agent (a second Claude Code session there). A human device allows or denies.
    async fn self_join(&mut self, same_folder: bool) {
        // (only an iPhone answers the ask: the app core the web runs on shows no self-join asks yet, its
        // `view.account.asks` is never filled, so a web device cannot Allow or Deny)
        if !self.phones.iter().any(|p| p.0) {
            self.say("a self-join is left out: no iPhone runs to answer the ask (the web shows no asks)".into());
            return;
        }
        let Some(v) = self.pick_alive_agent() else { return };
        let k = self.agents.len();
        let host = self.agents[v].host;
        let (folder, name) = if same_folder {
            // (its session is named after the folder, as the first one's; the log says which is which)
            (self.agents[v].folder.clone(), self.agents[v].name.clone())
        } else {
            let name = format!("agent-{k}");
            let f = self.agents[host].seat.as_ref().unwrap().home.path().join(&name);
            std::fs::create_dir_all(&f).unwrap();
            (f, name)
        };
        let allow = self.rng.chance(0.8);
        self.agents.push(Agent {
            name: name.clone(),
            seat: None,
            host,
            folder,
            key: format!("torture-{k}"),
            mcp: None,
            way: if same_folder { Way::SameFolder } else { Way::SelfJoin },
            session: None,
            device: None,
            alive: allow,
            denied: !allow,
            killed_at: None,
            away: Vec::new(),
            helpers: Vec::new(),
        });
        let mut mcp = self.serve(k, &[("TROMMI_SELF_JOIN_POLL_MS", "200")]).await;
        if same_folder {
            // a second connector in a folder whose own one runs sleeps until its session is used (a tool call)
            let _ = tokio::time::timeout(Duration::from_secs(30), mcp.call("list_cards", json!({}))).await;
        }
        self.agents[k].mcp = Some(mcp);
        let mut by = self.pick_human();
        if matches!(by, HumanDev::StandIn | HumanDev::Web(_)) {
            by = match self.phones.iter().position(|p| p.0) {
                Some(i) => HumanDev::Phone(i),
                None => return,
            };
        }
        let by_name = self.dev_name(&by);
        self.say(format!(
            "{name} (#{k}) asks to join on its own ({}), {} on {by_name}",
            if same_folder { "same folder" } else { "folder without a key" },
            if allow { "Allow" } else { "Deny" }
        ));
        let t = now();
        // (the ask names the folder as the connector shows it, `~/…`: its name is enough)
        let folder = self.agents[k].folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let r = match &by {
            HumanDev::Web(i) => self.web[*i].call(json!({ "op": "allow_join", "allow": allow, "folder": folder })).await,
            HumanDev::Phone(i) => self.phone(json!({ "op": "allow_join", "dev": i, "allow": allow, "folder": folder })).await,
            HumanDev::StandIn => unreachable!(),
        };
        if let Err(e) = r {
            self.fail(format!("{name}: the ask never reached {by_name}: {e}"));
            self.agents[k].alive = false;
            return;
        }
        if allow {
            self.await_online(k, t, "self_join_online_ms").await;
        }
    }

    fn pick_alive_agent(&mut self) -> Option<usize> {
        let alive: Vec<usize> = self.agents.iter().enumerate().filter(|(_, a)| a.alive && a.mcp.is_some()).map(|(i, _)| i).collect();
        self.rng.pick(&alive).copied()
    }

    /// /mcp: the server ends, a new one starts in the same session.
    async fn restart_agent(&mut self, k: usize) {
        let from = now();
        if let Some(m) = self.agents[k].mcp.take() {
            m.close().await;
        }
        let mcp = self.serve(k, &[]).await;
        self.agents[k].mcp = Some(mcp);
        let name = self.agents[k].name.clone();
        self.say(format!("{name}: /mcp restart"));
        // online again without a tool call
        let end = now() + budget(10_000);
        let sid = self.agents[k].session.clone();
        loop {
            let list = self.any_shown().await.unwrap_or_default();
            if list.iter().any(|e| e["session"].as_str() == sid.as_deref() && e["online"] == true) {
                break;
            }
            if now() > end {
                self.fail(format!("TRUTH {name}: not online again within {} ms of a /mcp restart", budget(10_000)));
                break;
            }
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
        let to = now();
        self.agents[k].away.push((from, to + 1_000));
        self.metric("mcp_restart_online_ms", to - from, None);
    }

    async fn kill_agent(&mut self, k: usize) {
        if let Some(mut m) = self.agents[k].mcp.take() {
            let _ = m.child.start_kill();
            let _ = m.child.wait().await;
        }
        self.agents[k].alive = false;
        self.agents[k].killed_at = Some(now());
        let name = self.agents[k].name.clone();
        self.say(format!("{name}: its server is killed (SIGKILL), the lease runs out within a minute"));
    }

    /// A subprocess of the session's machine runs and ends: a hook, `say`, `monitor`.
    async fn subprocess(&mut self, k: usize) {
        let what = self.rng.below(3);
        // (the ask names the folder as the connector shows it, `~/…`: its name is enough)
        let folder = self.agents[k].folder.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let name = self.agents[k].name.clone();
        match what {
            0 => {
                let prompt = json!({ "session_id": "s1", "prompt_id": "p1", "cwd": folder, "hook_event_name": "UserPromptSubmit", "prompt": "Go on." });
                self.hook(k, "prompt", &prompt).await;
                let stop = json!({ "session_id": "s1", "cwd": folder, "hook_event_name": "Stop", "last_assistant_message": "Done." });
                self.hook(k, "stop", &stop).await;
                self.say(format!("{name}: prompt and stop hooks ran"));
            }
            1 => {
                let out = self.cmd(k, &["say", "from the shell"]).stdin(Stdio::null()).output().await;
                let ok = out.as_ref().map(|o| o.status.success()).unwrap_or(false);
                self.say(format!("{name}: `say` ran ({})", if ok { "ok" } else { "failed" }));
            }
            _ => {
                let mut c = self.cmd(k, &["monitor"]);
                c.env("CLAUDE_PID", std::process::id().to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
                if let Ok(mut child) = c.spawn() {
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                }
                self.say(format!("{name}: `monitor` ran 1.5 s and was killed"));
            }
        }
    }

    async fn human_message(&mut self, k: usize) {
        let Some(sid) = self.agents[k].session.clone() else { return };
        let by = self.pick_human();
        let text = format!("note {}", self.rng.next() % 10_000);
        let r = match &by {
            HumanDev::Web(i) => self.web[*i].call(json!({ "op": "say", "session": sid, "text": text })).await.map(|_| ()),
            HumanDev::Phone(i) => self.phone(json!({ "op": "say", "dev": i, "session": sid, "text": text })).await.map(|_| ()),
            HumanDev::StandIn => {
                let g = self.group_of(&sid).await;
                match g {
                    Some(g) => self.human.say(&g, json!({ "content_type": "message", "text": text })).await.map_err(|e| format!("{e:?}")),
                    None => Ok(()),
                }
            }
        };
        let who = self.dev_name(&by);
        match r {
            Ok(()) => self.say(format!("{who} writes to {}", self.agents[k].name)),
            Err(e) => self.say(format!("{who} could not write to {}: {e}", self.agents[k].name)),
        }
    }

    async fn group_of(&mut self, sid: &str) -> Option<GroupId> {
        self.human.sync().await;
        self.human
            .groups()
            .await
            .into_iter()
            .find(|g| g.session.is_some_and(|s| trommi_connector::util::hex(s.session_id.as_bytes()) == sid))
            .map(|g| g.group)
    }

    async fn cards(&mut self, k: usize, n: usize) {
        let name = self.agents[k].name.clone();
        let Some(mcp) = self.agents[k].mcp.as_mut() else { return };
        let mut made = 0;
        for i in 0..n {
            let (text, failed) = if i % 3 == 2 {
                mcp.call("create_info", json!({ "title": format!("Report {i} of {name}"), "body": "Done." })).await
            } else {
                mcp.call("create_decision", json!({ "title": format!("Question {i} of {name}"), "options": [{ "key": "a", "label": "A" }, { "key": "b", "label": "B" }] })).await
            };
            if failed {
                self.log.push(format!("{name}: card refused: {}", tail(&text, 200)));
            } else {
                made += 1;
            }
        }
        self.say(format!("{name} files {made} cards"));
    }

    async fn helpers(&mut self, k: usize, n: usize) {
        let name = self.agents[k].name.clone();
        let Some(mcp) = self.agents[k].mcp.as_mut() else { return };
        let mut opened = Vec::new();
        for i in 0..n {
            let h = format!("H{i}");
            let (text, failed) = mcp.call("open_session", json!({ "name": h })).await;
            if failed {
                self.log.push(format!("{name}: open_session {h} refused: {}", tail(&text, 200)));
                break;
            }
            opened.push(h);
        }
        let close = opened.len() / 2;
        for h in opened.iter().take(close) {
            let _ = mcp.call("close_session", json!({ "name": h })).await;
        }
        self.agents[k].helpers.extend(opened.iter().skip(close).cloned());
        self.say(format!("{name} opens {} helper sessions, closes {close}", opened.len()));
    }

    async fn answer_some(&mut self) {
        // what the first web device shows open, answered from a random human device
        let snap = match self.web.first_mut() {
            Some(o) => o.call(json!({ "op": "snapshot" })).await.unwrap_or(Value::Null),
            None => return,
        };
        let open: Vec<String> = snap["cards"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, c)| c["state"] == "open" && c["title"].as_str().is_some_and(|t| t.starts_with("Question")))
            .map(|(id, _)| id.clone())
            .collect();
        let n = (open.len() / 3).max(1).min(open.len());
        for _ in 0..n {
            let Some(card) = self.rng.pick(&open).cloned() else { return };
            let choice = if self.rng.chance(0.5) { "a" } else { "b" };
            let by = self.pick_human();
            let r = match &by {
                HumanDev::Web(i) => self.web[*i].call(json!({ "op": "answer", "card": card, "choices": [choice] })).await.map(|_| ()),
                HumanDev::Phone(i) => self.phone(json!({ "op": "answer", "dev": i, "card": card, "choices": [choice] })).await.map(|_| ()),
                HumanDev::StandIn => {
                    self.human.sync().await;
                    self.human.answer(&card, json!({ "answer_action": "answer", "choices": [choice] }), &[choice], false).await.map_err(|e| format!("{e:?}"))
                }
            };
            if let Err(e) = r {
                self.log.push(format!("answer {card}: {e}"));
            }
        }
        self.say(format!("{n} answers from the human devices"));
    }

    async fn archive_dead(&mut self) {
        let dead: Vec<usize> = self.agents.iter().enumerate().filter(|(_, a)| !a.alive && !a.denied && a.session.is_some() && a.killed_at.is_some_and(|t| now() > t + LEASE_GONE_MS)).map(|(i, _)| i).collect();
        let Some(&k) = self.rng.pick(&dead) else { return };
        let sid = self.agents[k].session.clone().unwrap();
        let r = match self.web.first_mut() {
            Some(o) => o.call(json!({ "op": "archive", "session": sid })).await,
            None => return,
        };
        let name = self.agents[k].name.clone();
        if r.is_ok() {
            self.agents[k].session = None;
            self.agents[k].denied = true; // (archived: it must show nowhere now)
            self.say(format!("{name} (dead) archived on web"));
        }
    }

    // ---- iPhones ---------------------------------------------------------------------------------------------------

    async fn phone_trouble(&mut self) {
        if self.fleet.is_none() || self.phones.len() < 2 {
            return;
        }
        let i = 1 + self.rng.below(self.phones.len() - 1);
        let alive: Vec<usize> = self.phones.iter().enumerate().filter(|(j, p)| p.0 && *j != i).map(|(j, _)| j).collect();
        let Some(&by) = self.rng.pick(&alive) else { return };
        match (self.phones[i].0, self.rng.below(4)) {
            (false, _) => self.pair_phone(i, by, "reinstall").await,
            (true, 0) => {
                let _ = self.phone(json!({ "op": "kill", "dev": i })).await;
                self.phones[i].0 = false;
                self.say(format!("iPhone {i} killed"));
            }
            (true, 1) => {
                let t = now();
                match self.phone(json!({ "op": "restart", "dev": i, "live": fleet_live() })).await {
                    Ok(v) => {
                        self.metric("phone_warm_open_ms", v["warm_ms"].as_u64().unwrap_or(0), Some(5_000));
                        self.metric("phone_restart_synced_ms", now() - t, Some(30_000));
                        self.say(format!("iPhone {i} restarted warm"));
                    }
                    Err(e) => self.fail(format!("iPhone {i} restart: {e}")),
                }
            }
            (true, 2) => {
                let _ = self.phone(json!({ "op": "live", "dev": i, "on": false })).await;
                self.say(format!("iPhone {i} offline for a while"));
                // it comes back at the next checkpoint (sync of every device)
            }
            (true, _) => self.pair_phone(i, by, "reinstall").await,
        }
    }

    // ---- chaos -----------------------------------------------------------------------------------------------------

    async fn chaos(&mut self) {
        if env("MEGA_CHAOS").as_deref() == Some("0") {
            return;
        }
        // MEGA_CHAOS=latency,cut,dup,down,restart: only these kinds (to find which one a failure needs)
        let only = env("MEGA_CHAOS").filter(|v| v != "1");
        let on = |kind: &str| only.as_deref().is_none_or(|o| o.split(',').any(|k| k == kind));
        match self.rng.below(5) {
            0 | 1 => {
                let mut k = Knobs::rough();
                if !on("latency") {
                    k.latency_ms = 0;
                }
                if !on("cut") {
                    k.cut = 0.0;
                    k.cut_after = 0.0;
                }
                if !on("dup") {
                    k.dup = 0.0;
                }
                self.proxy.set(k);
                self.say("network: rough (latency, cut streams, duplicate events)".into());
            }
            2 => {
                self.proxy.set(Knobs::calm());
                self.say("network: calm".into());
            }
            3 if on("down") => {
                self.proxy.set_down(true);
                self.say("network: the hub is away for 3 s".into());
                tokio::time::sleep(Duration::from_secs(3)).await;
                self.proxy.set_down(false);
            }
            4 if on("restart") => {
                let hub = self.hub.take().expect("the hub");
                let (data, port) = hub.stop();
                self.say("the hub restarts".into());
                tokio::time::sleep(Duration::from_millis(500)).await;
                let url = self.proxy.url.clone();
                self.hub = Some(HubProc::start_in_with(data, port, &[("HUB_URL", url.as_str())]).await);
            }
            _ => {}
        }
    }

    // ---- checking --------------------------------------------------------------------------------------------------

    async fn checkpoint(&mut self, label: &str, report: &mut Report) {
        // the network calms down and every device catches up: what must agree is what has settled
        let knobs = self.proxy.knobs();
        self.proxy.set(Knobs::calm());
        self.proxy.set_down(false);
        let t = now();
        let alive: Vec<usize> = self.phones.iter().enumerate().filter(|(_, p)| p.0).map(|(i, _)| i).collect();
        for &i in &alive {
            let _ = self.phone(json!({ "op": "live", "dev": i, "on": fleet_live() })).await;
        }
        tokio::time::sleep(Duration::from_millis(1500)).await;
        for a in self.agents.iter_mut().filter(|a| a.alive) {
            if let Some(m) = a.mcp.as_mut() {
                let _ = tokio::time::timeout(Duration::from_secs(30), m.call("list_cards", json!({}))).await;
            }
        }
        self.human.sync().await;
        // (the connectors once more after the human devices caught up, for their view below)
        tokio::time::sleep(Duration::from_millis(500)).await;
        for _ in 0..2 {
            for o in self.web.iter_mut() {
                let _ = o.call(json!({ "op": "sync" })).await;
            }
            if self.fleet.is_some() {
                let _ = self.phone(json!({ "op": "sync" })).await;
            }
        }
        self.metric("checkpoint_settle_ms", now() - t, None);

        // what every device shows
        let mut snaps = BTreeMap::new();
        let mut shown: Vec<(String, Value)> = Vec::new();
        // (the iPhones first: the cards and requests they hold are what each web device reads one by one, as its app
        // core shows one card at a time; the lists stay in the order web, iPhones)
        let mut phone_shown: Vec<(String, Value)> = Vec::new();
        for &i in &alive {
            let name = if i == 0 { "ios@fleet".to_string() } else { format!("ios@fleet{i}") };
            if let Ok(v) = self.phone(json!({ "op": "snapshot", "dev": i })).await {
                snaps.insert(name.clone(), v);
            }
            match self.phone(json!({ "op": "shown", "dev": i })).await {
                Ok(v) => phone_shown.push((name, v)),
                Err(e) => phone_shown.push((name, json!({ "failed": e }))),
            }
        }
        let ids: BTreeSet<String> = snaps
            .values()
            .flat_map(|v| ["cards", "permissions"].map(|k| v[k].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()).unwrap_or_default()))
            .flatten()
            .collect();
        for o in self.web.iter_mut() {
            if let Ok(v) = o.call(json!({ "op": "snapshot", "ids": ids })).await {
                snaps.insert(o.name.clone(), v);
            }
            match o.call(json!({ "op": "shown" })).await {
                Ok(v) => shown.push((o.name.clone(), v)),
                Err(e) => shown.push((o.name.clone(), json!({ "failed": e }))),
            }
        }
        shown.extend(phone_shown);
        let dir = self.out.join(label);
        std::fs::create_dir_all(&dir).unwrap();
        for (k, v) in &snaps {
            let _ = std::fs::write(dir.join(format!("{}.json", k.replace('@', "-"))), serde_json::to_string_pretty(v).unwrap());
        }
        let _ = std::fs::write(dir.join("shown.json"), serde_json::to_string_pretty(&json!(shown.iter().map(|(k, v)| json!({ k.clone(): v })).collect::<Vec<_>>())).unwrap());

        // SESSIONS: every device the same list
        if let Some((first, reference)) = shown.first().cloned() {
            // (`agent`, the session's agent device, is left out: the web's app core does not show it)
            let unagented = |v: &Value| -> Value {
                let mut v = v.clone();
                for e in v.as_array_mut().into_iter().flatten() {
                    if let Some(o) = e.as_object_mut() {
                        o.remove("agent");
                    }
                }
                v
            };
            for (name, list) in &shown[1..] {
                if unagented(list) != unagented(&reference) {
                    self.fail(format!("{label} SESSIONS {first} vs {name}: {}", sessions_differ(&reference, list)));
                }
            }
            // TRUTH: the agents as they really are
            let list: Vec<Value> = reference.as_array().cloned().unwrap_or_default();
            let at = now();
            let mut truths = Vec::new();
            // an agent shown only after its join's wait ran out (failed there): its session is taken from here on
            let taken: BTreeSet<String> = self.agents.iter().filter_map(|a| a.session.clone()).collect();
            for a in self.agents.iter_mut().filter(|a| a.alive && a.session.is_none()) {
                if let Some(e) = list.iter().find(|e| e["name"] == a.name.as_str() && !taken.contains(e["session"].as_str().unwrap_or("")) && a.device.as_deref().is_none_or(|d| e["agent"].as_str() == Some(d))) {
                    a.session = e["session"].as_str().map(String::from);
                }
            }
            for a in &self.agents {
                let entry = a.session.as_deref().and_then(|s| list.iter().find(|e| e["session"].as_str() == Some(s)));
                if a.denied {
                    if let Some(s) = a.session.as_deref() {
                        if list.iter().any(|e| e["session"].as_str() == Some(s)) {
                            truths.push(format!("{} (denied or archived) is shown", a.name));
                        }
                    }
                    continue;
                }
                if a.alive {
                    match entry {
                        None => truths.push(format!("{} (live) is not shown", a.name)),
                        Some(e) => {
                            if e["name"].as_str() != Some(a.name.as_str()) {
                                truths.push(format!("{} (live) is shown as {}", a.name, e["name"]));
                            }
                            if e["online"] != true {
                                truths.push(format!("{} (live) is shown gone", a.name));
                            }
                        }
                    }
                } else if let (Some(t), Some(e)) = (a.killed_at, entry) {
                    // (a live session of the same folder, a second Claude Code session there, takes the folder's key
                    // and so the killed one's place: the agent of that session is alive again, and online rightly)
                    let taken_over = self.agents.iter().any(|b| b.alive && b.folder == a.folder && b.host == a.host);
                    if !taken_over && at > t + LEASE_GONE_MS && e["online"] == true {
                        truths.push(format!("{} (killed {} s ago) is shown online", a.name, (at - t) / 1000));
                    }
                }
            }
            for t in truths {
                self.fail(format!("{label} TRUTH {first}: {t}"));
            }
        }

        // PRESENCE: no live agent seen offline outside the moments it was away
        let mut flickers = Vec::new();
        let t0 = self.t0_ms();
        for o in self.web.iter_mut() {
            let log = o.call(json!({ "op": "presence_log", "reset": true })).await.unwrap_or(Value::Null);
            for e in log.as_array().into_iter().flatten() {
                let (t, dev, on) = (e[0].as_u64().unwrap_or(0), e[1].as_str().unwrap_or(""), e[2] == true);
                if on {
                    continue;
                }
                let Some(a) = self.agents.iter().find(|a| a.device.as_deref() == Some(dev) || a.session.as_deref() == Some(dev)) else { continue };
                let really_away = !a.alive && a.killed_at.is_some_and(|k| t >= k) || a.away.iter().any(|(f, to)| t >= *f && t <= *to);
                if a.alive && !really_away && a.session.is_some() {
                    flickers.push(format!("{} saw {} go offline at +{} ms while it ran", o.name, a.name, t.saturating_sub(t0)));
                }
            }
        }
        for f in flickers {
            self.fail(format!("{label} PRESENCE {f}"));
        }

        // AGENT: each live connector's own cards (its list_cards) against the first human device
        let reference = snaps.iter().find(|(k, _)| k.starts_with("web@")).or_else(|| snaps.iter().next()).map(|(_, v)| v.clone());
        if let Some(reference) = reference {
            let mut wrong = Vec::new();
            let mut tries = 0;
            loop {
            wrong.clear();
            for a in self.agents.iter_mut().filter(|a| a.alive) {
                let Some(m) = a.mcp.as_mut() else { continue };
                let text = match tokio::time::timeout(Duration::from_secs(30), m.call("list_cards", json!({}))).await {
                    Ok((text, false)) => text,
                    Ok((said, true)) => {
                        wrong.push(format!("{}: list_cards failed: {}", a.name, tail(&said, 200)));
                        continue;
                    }
                    Err(_) => {
                        wrong.push(format!("{}: list_cards timed out", a.name));
                        continue;
                    }
                };
                let list: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                for c in list.as_array().into_iter().flatten() {
                    let id = c["id"].as_str().unwrap_or("");
                    let Some(h) = reference["cards"].get(id) else {
                        wrong.push(format!("{}: card {id} the person's devices do not show", a.name));
                        continue;
                    };
                    let theirs = match (h["state"].as_str(), h["closed_how"].as_str()) {
                        (Some("open"), _) => "open",
                        (Some("answered"), Some("answered")) => "decided",
                        (_, Some("shredded")) => "shredded",
                        _ => "done",
                    };
                    if c["status"].as_str() != Some(theirs) {
                        wrong.push(format!("{}: card {id} is {} for the agent, {theirs} for the person", a.name, c["status"]));
                    }
                }
            }
            // (a connector takes what came a moment later than the person's devices: looked at again for 10 s)
            tries += 1;
            if wrong.is_empty() || tries >= 10 {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
            }
            for w in wrong.into_iter().take(12) {
                self.fail(format!("{label} AGENT {w}"));
            }
        }

        // cards, requests, the Desk: as every_client_shows_the_same
        let before = report.failures.len();
        compare(&format!("seed {} {label}", self.seed), &snaps, report);
        let new: Vec<String> = report.failures.drain(before..).collect();
        for f in new {
            self.fail(f);
        }
        self.proxy.set(knobs);
        self.say(format!("{label}: {} devices compared, {} sessions shown", snaps.len(), shown.first().and_then(|(_, v)| v.as_array().map(|a| a.len())).unwrap_or(0)));
    }

    fn t0_ms(&self) -> u64 {
        now() - self.t0.elapsed().as_millis() as u64
    }

    /// Learns each agent's device (hex) from the session lists: the web shows the agent device of a session.
    async fn learn_devices(&mut self) {
        let Some(o) = self.web.first_mut() else { return };
        let Ok(v) = o.call(json!({ "op": "shown", "devices": true })).await else { return };
        for e in v.as_array().into_iter().flatten() {
            if let (Some(s), Some(d)) = (e["session"].as_str(), e["agent"].as_str()) {
                for a in self.agents.iter_mut() {
                    if a.session.as_deref() == Some(s) && a.device.is_none() {
                        a.device = Some(d.to_string());
                    }
                }
            }
        }
    }
}

/// The open bug a failure is a sign of, if it is one of those known and being fixed: such a failure is reported
/// under the bug's name and does not fail the run (MEGA_STRICT=1 makes it fail). Each entry goes when its fix is
/// merged; a failure that only looks like one of these is still listed, under its name, in every report.
fn known_bug(what: &str) -> Option<&'static str> {
    let parts = |w: &str| -> Vec<String> { w.split_once(": ").map(|(_, d)| d.split("; ").map(String::from).collect()).unwrap_or_default() };
    // iOS keeps closed helper sessions in its list (named after their main session); an iPhone paired after the
    // closing does not hold them
    if what.contains(" SESSIONS ") && what.contains(" vs ios@") && parts(what).iter().all(|p| p.starts_with("only second shows")) {
        return Some("ios: closed helper sessions stay listed");
    }
    if what.contains(" VARIANCE ios@") && what.contains("/sessions/") && what.contains("\"parent\":\"") && (what.ends_with(" vs null") || what.contains(": null vs {")) {
        return Some("ios: closed helper sessions stay listed");
    }
    // web: a founding of a main session that the hub refused (another human device founded it first) stays in the
    // model's sessions (never shown)
    if what.contains(" VARIANCE web@") && what.contains("/sessions/") && (what.ends_with(": {\"agent_name\":null,\"parent\":null} vs null") || what.ends_with(": null vs {\"agent_name\":null,\"parent\":null}")) {
        return Some("web: refused founding kept");
    }
    // web: a presence change missed while the stream was cut is never corrected (a new stream is told only who is
    // online)
    if what.contains(" TRUTH web@") && what.contains(" (killed ") && what.ends_with(" is shown online") {
        return Some("web: presence after a cut stream");
    }
    if what.contains(" SESSIONS web@") && parts(what).iter().all(|p| p.contains(" online: ")) {
        return Some("web: presence after a cut stream");
    }
    None
}

/// How two session lists differ, by session: missing on either side, other fields, and the order.
fn sessions_differ(a: &Value, b: &Value) -> String {
    let rows = |v: &Value| -> Vec<Value> { v.as_array().cloned().unwrap_or_default() };
    let id = |e: &Value| e["session"].as_str().unwrap_or("").to_string();
    let label = |e: &Value| format!("{} ({})", e["name"].as_str().unwrap_or("?"), &id(e)[..id(e).len().min(8)]);
    let (ra, rb) = (rows(a), rows(b));
    if !a.is_array() || !b.is_array() {
        return format!("{} vs {}", short(a), short(b));
    }
    let mut out = Vec::new();
    for e in &ra {
        match rb.iter().find(|x| id(x) == id(e)) {
            None => out.push(format!("only first shows {}", label(e))),
            Some(x) => {
                for k in ["name", "online", "parent"] {
                    if e[k] != x[k] {
                        out.push(format!("{} {k}: {} vs {}", label(e), short(&e[k]), short(&x[k])));
                    }
                }
            }
        }
    }
    for x in &rb {
        if !ra.iter().any(|e| id(e) == id(x)) {
            out.push(format!("only second shows {}", label(x)));
        }
    }
    if out.is_empty() {
        let order = |r: &[Value]| r.iter().map(label).collect::<Vec<_>>().join(", ");
        out.push(format!("order: [{}] vs [{}]", order(&ra), order(&rb)));
    }
    out.truncate(8);
    out.join("; ")
}

// ---- one seed ------------------------------------------------------------------------------------------------------

async fn run_torture(seed: u64, root: &Path, out: &Path, report: &mut Report) {
    let out = out.join(format!("torture-{seed}"));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    for (k, v) in [
        ("HUB_LOGIN_THROTTLE", "off"),
        ("HUB_LIMIT_ENVELOPES_PER_SECOND", "100000"),
        ("HUB_LIMIT_ENVELOPE_BURST", "100000"),
        ("HUB_LIMIT_HEAVY_PER_SECOND", "10000"),
        ("HUB_LIMIT_HEAVY_BURST", "10000"),
        ("HUB_LIMIT_LOGINS_PER_IP_10MIN", "100000"),
        ("HUB_LIMIT_OPEN_REQUESTS_PER_IP_MINUTE", "100000"),
        ("HUB_LIMIT_PIECES_PER_SECOND", "10000"),
        ("HUB_LIMIT_OPEN_INVITES", "64"),
    ] {
        std::env::set_var(format!("MEGA_HUB_ENV_{k}"), v);
    }
    let proxy = Proxy::start(seed).await;
    let hub = HubProc::start_in_with(TempDir::new("hub"), 0, &[("HUB_URL", proxy.url.as_str())]).await;
    proxy.set_upstream(hub.port);
    let code: Arc<Mutex<Option<Vec<u8>>>> = Arc::new(Mutex::new(None));
    let keep = code.clone();
    let human = Human::found_with(&proxy.url, move |room, c| {
        *keep.lock().unwrap() = Some(c.expose().to_vec());
        Some(account_body(room, c))
    })
    .await;
    let code_hex = trommi_connector::util::hex(&code.lock().unwrap().clone().expect("the code"));
    let room_hex = trommi_connector::util::hex(human.room.as_bytes());
    let mut t = Torture {
        seed,
        rng: Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1),
        proxy,
        hub: Some(hub),
        human,
        code_hex,
        room_hex,
        out: out.clone(),
        root: root.to_path_buf(),
        web: Vec::new(),
        web_made: 0,
        fleet: None,
        phones: Vec::new(),
        agents: Vec::new(),
        log: Vec::new(),
        fails: Vec::new(),
        known: Vec::new(),
        metrics: BTreeMap::new(),
        t0: Instant::now(),
    };
    // (the agents' logs carry ms since 1970: this is the timeline's +0)
    t.log.push(format!("+0 is {} ms since 1970", now()));
    let webs = if env("MEGA_WEB").as_deref() == Some("0") { 0 } else { num("MEGA_WEB_DEVICES", 2) };
    let fleet = if env("MEGA_IOS").as_deref() == Some("0") { 0 } else { num("MEGA_FLEET", 10) };
    let agents_n = num("MEGA_AGENTS", 8);
    let steps = num("MEGA_STEPS", 60);
    let cards = num("MEGA_CARDS", 200);
    let kills = env("MEGA_KILLS").as_deref() != Some("0");

    for _ in 0..webs {
        t.add_web().await;
    }
    if fleet > 0 {
        t.start_fleet(fleet).await;
    }
    // the first agents: one each way by link, so every way is there in every seed
    t.join_by_link(Way::Tool).await;
    t.join_by_link(Way::Cli).await;
    t.self_join(false).await;
    // the big room: cards from the first agents
    let first: Vec<usize> = t.agents.iter().enumerate().filter(|(_, a)| a.alive).map(|(i, _)| i).collect();
    for &k in &first {
        t.cards(k, cards / first.len().max(1)).await;
    }
    t.learn_devices().await;
    t.checkpoint("cp00", report).await;

    let mut cp = 1;
    for s in 0..steps {
        let step_started = Instant::now();
        let roll = t.rng.below(100);
        let live = t.agents.iter().filter(|a| a.alive).count();
        match roll {
            0..=11 if t.agents.len() < agents_n + 4 => {
                let way = if t.rng.chance(0.5) { Way::Tool } else { Way::Cli };
                t.join_by_link(way).await;
            }
            12..=17 if t.agents.len() < agents_n + 4 => {
                let same = t.rng.chance(0.3);
                t.self_join(same).await;
            }
            18..=27 => {
                if let Some(k) = t.pick_alive_agent() {
                    t.restart_agent(k).await;
                }
            }
            28..=31 if kills && live > 2 => {
                if let Some(k) = t.pick_alive_agent() {
                    t.kill_agent(k).await;
                }
            }
            32..=43 => {
                if let Some(k) = t.pick_alive_agent() {
                    t.subprocess(k).await;
                }
            }
            44..=55 => {
                if let Some(k) = t.pick_alive_agent() {
                    t.human_message(k).await;
                }
            }
            56..=67 => {
                if let Some(k) = t.pick_alive_agent() {
                    let n = 3 + t.rng.below(10);
                    t.cards(k, n).await;
                }
                t.answer_some().await;
            }
            68..=71 => {
                if let Some(k) = t.pick_alive_agent() {
                    let many = t.rng.chance(0.1);
                    let n = 2 + t.rng.below(if many { 99 } else { 8 });
                    t.helpers(k, n).await;
                }
            }
            72..=74 => t.archive_dead().await,
            75..=86 => t.phone_trouble().await,
            _ => t.chaos().await,
        }
        let took = step_started.elapsed().as_millis() as u64;
        if took > 15_000 {
            t.say(format!("SLOW step {s} (roll {roll}) took {took} ms"));
        }
        if (s + 1) % 10 == 0 {
            t.learn_devices().await;
            let label = format!("cp{cp:02}");
            t.checkpoint(&label, report).await;
            cp += 1;
        }
    }
    // killed servers: wait out their leases, so that "no online for a dead one" is checked for each
    if let Some(last) = t.agents.iter().filter_map(|a| a.killed_at).max() {
        let wait = (last + LEASE_GONE_MS).saturating_sub(now());
        if wait > 0 {
            t.say(format!("waiting {} s for the last killed lease", wait / 1000));
            tokio::time::sleep(Duration::from_millis(wait + 1000)).await;
        }
    }
    t.proxy.set(Knobs::calm());
    t.learn_devices().await;
    t.checkpoint("final", report).await;

    // per device: what it cost
    let mut stats = Map::new();
    for (i, p) in t.phones.clone().iter().enumerate() {
        if p.0 {
            if let Ok(v) = t.phone(json!({ "op": "stats", "dev": i })).await {
                stats.insert(format!("iphone{i}"), v);
            }
        }
    }
    for o in t.web.iter_mut() {
        if let Ok(v) = o.call(json!({ "op": "stats" })).await {
            stats.insert(o.name.clone(), v);
        }
    }
    let counts = t.proxy.counts();
    let summary = json!({
        "seed": seed,
        "agents": t.agents.iter().map(|a| json!({ "name": a.name, "way": format!("{:?}", a.way), "alive": a.alive, "denied": a.denied, "session": a.session })).collect::<Vec<_>>(),
        "phones": t.phones.len(),
        "web": t.web.len(),
        "metrics": t.metrics.iter().map(|(k, v)| {
            let mut s = v.clone();
            s.sort();
            (k.clone(), json!({ "n": s.len(), "median": s.get(s.len() / 2), "max": s.last() }))
        }).collect::<Map<_, _>>(),
        "devices": stats,
        "network": format!("{counts:?}"),
        "failures": t.fails,
        "known": t.known,
    });
    let _ = std::fs::write(out.join("summary.json"), serde_json::to_string_pretty(&summary).unwrap());
    let _ = std::fs::write(out.join("timeline.txt"), t.log.join("\n"));
    eprintln!("TORTURE seed {seed}: {}", serde_json::to_string(&summary["metrics"]).unwrap());
    eprintln!("TORTURE seed {seed}: network {counts:?}");
    for o in t.web.iter_mut() {
        let _ = o.call(json!({ "op": "quit" })).await;
    }
    let _ = t.phone(json!({ "op": "quit" })).await;
    for a in t.agents.iter_mut() {
        if let Some(m) = a.mcp.take() {
            m.close().await;
        }
    }
    report.failures.extend(t.fails.clone());
    report.known.extend(t.known.clone());
    report.checkpoints += cp;
}

/// The torture run, per seed (MEGA_SEEDS, default one from the clock: printed, to repeat it).
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore]
async fn torture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let seeds: Vec<u64> = env("MEGA_SEEDS")
        .map(|s| s.split(',').filter_map(|x| x.trim().parse().ok()).collect())
        .unwrap_or_else(|| vec![now_ms() % 100_000]);
    let out = PathBuf::from(env("MEGA_OUT").unwrap_or_else(|| std::env::temp_dir().join("trommi-mega").display().to_string()));
    let mut report = Report::default();
    for seed in &seeds {
        eprintln!("== torture seed {seed} (repeat: MEGA_SEEDS={seed})");
        run_torture(*seed, &root, &out, &mut report).await;
    }
    let _ = std::fs::write(out.join("torture-report.txt"), report.failures.join("\n"));
    if !report.known.is_empty() {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for k in &report.known {
            let bug = k.split_once('[').and_then(|(_, r)| r.split_once(']')).map_or("?", |(b, _)| b);
            *by.entry(bug.to_string()).or_default() += 1;
        }
        eprintln!("TORTURE known open bugs (not failing; MEGA_STRICT=1 fails on them): {by:?}");
    }
    // the seeds that failed, each line of the report starts with its own
    let failed: BTreeSet<u64> = report
        .failures
        .iter()
        .filter_map(|f| f.strip_prefix("seed ")?.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok())
        .collect();
    if !failed.is_empty() {
        let list = failed.iter().map(u64::to_string).collect::<Vec<_>>().join(",");
        eprintln!("TORTURE FAILED SEEDS: {list} (repeat: MEGA_SEEDS={list} with the same MEGA_* sizes)");
    }
    assert!(
        report.failures.is_empty(),
        "{} failures in {} checkpoints (failed seeds {:?} of {:?}; repeat one with MEGA_SEEDS=<seed>; snapshots in {}):\n{}",
        report.failures.len(),
        report.checkpoints,
        failed,
        seeds,
        out.display(),
        report.failures.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[cfg(test)]
mod known {
    use super::known_bug;

    #[test]
    fn failures_of_known_bugs_are_named() {
        let cases = [
            ("seed 1: cp01 SESSIONS web@torture vs ios@fleet: only second shows agent-2 (94873182); only second shows agent-2 (97177b42)", Some("ios: closed helper sessions stay listed")),
            ("seed 1: seed 1 cp01 VARIANCE ios@fleet vs ios@fleet1 /sessions/94873182f1e7d945e5a20e2880f6584f: {\"agent_name\":null,\"parent\":\"bb65eea195b6d48c79a579d37d9ce333\"} vs null", Some("ios: closed helper sessions stay listed")),
            ("seed 4: seed 4 cp00 VARIANCE web@torture vs web@torture2 /sessions/7559cd5444a053230bcb5366349cdbc9: null vs {\"agent_name\":null,\"parent\":null}", Some("web: refused founding kept")),
            ("seed 2: cp03 TRUTH web@torture: agent-1 (killed 147 s ago) is shown online", Some("web: presence after a cut stream")),
            ("seed 7: cp03 SESSIONS web@torture vs web@torture2: agent-0 (d1311720) online: true vs false", Some("web: presence after a cut stream")),
            // (fixed 2026-10-13, a sign-in's session keys asked in one request: no longer known)
            ("seed 103: cp03 SESSIONS web@torture vs web@torture2: H0 (1002976f) name: \"H0\" vs \"agent-7\"; only first shows agent-7 (21d272fb)", None),
            // not one of them
            ("seed 109: cp04 TRUTH web@torture: agent-7 (live) is not shown", None),
            ("seed 102: cp02 AGENT agent-1: card 0d899eb9e9c29e0190d36bcf93ba772b is \"open\" for the agent, decided for the person", None),
            ("seed 3: cp01 SESSIONS web@torture vs web@torture2: only second shows agent-0 (a8d32793)", None),
            ("seed 3: cp01 SESSIONS web@torture vs ios@fleet: only first shows agent-0 (a8d32793)", None),
        ];
        for (line, bug) in cases {
            // (as `fail` is handed it: without the "seed <n>: " the report puts in front)
            let what = line.split_once(": ").map_or(line, |(_, w)| w);
            assert_eq!(known_bug(what), bug, "{what}");
        }
    }
}
