//! Many agent sessions on one machine: the session matrix. Claude Code and Codex (a scripted client that speaks
//! as Codex 0.160 does), one folder and several, two Trommi accounts (two rooms, A and B, one human each) and two
//! accounts of the agent program (its config folder), fresh sessions and resumed ones, one after another and
//! many at once, an /mcp restart, a killed connector, the stale key of a closed session, a self-join beside
//! another join, and connect while asleep, awake or in the other account's room already.
//!
//! What every case ends with: each session on the board of the account it was let in by, with a key of its own
//! (a device no other session has), its words in its own board session. No session takes another's key, no
//! tool answers "Not connected … call the tool again" for good, and every refusal says what the human does.
//!
//! The machine is one home (keys, sockets) shared by every session. A session is its program's process (a
//! `sleep` standing for Claude Code or Codex, whose pid the connector takes for its parent) and the connector
//! it starts; Claude Code names its session in `CLAUDE_CODE_SESSION_ID`, Codex names its thread in every tool
//! call's `_meta` and nothing in the environment. Codex's app server runs every thread under one process: its
//! threads share a parent.
//!
//! Pull requests and main run the cases marked fast; `TROMMI_TESTS_FULL=1` (nightly.yml) runs every case.
mod common;

use common::process::{codex_hello, codex_meta, Mcp, Seat};
use common::{HubProc, Human};
use serde_json::json;
use std::path::PathBuf;
use trommi_connector::util::{hex, random_hex};
use trommi_core::ids::{DeviceId, GroupId};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Client {
    Claude,
    Codex,
}

/// Whether a case runs now: the fast ones always, the rest with `TROMMI_TESTS_FULL=1` (every night, and locally
/// when asked for).
fn runs(fast: bool) -> bool {
    fast || std::env::var("TROMMI_TESTS_FULL").as_deref() == Ok("1")
}

/// One machine: a hub, the two humans of accounts A and B, and the home every session shares.
struct Machine {
    _hub: HubProc,
    seat: Seat,
    a: Human,
    b: Human,
    codex_bin: PathBuf,
    /// Each conversation device's main session: (account, group). Computers are tracked separately.
    groups: Vec<(char, GroupId)>,
    computers: Vec<(char, GroupId)>,
}

impl Machine {
    async fn new() -> Machine {
        let hub = HubProc::start().await;
        let a = Human::found(&hub.url).await;
        let b = Human::found(&hub.url).await;
        let seat = Seat::new(&hub.url);
        let codex_bin = seat.home.path().join("codex");
        std::fs::write(&codex_bin, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(
            &codex_bin,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        Machine {
            _hub: hub,
            seat,
            a,
            b,
            codex_bin,
            groups: vec![],
            computers: vec![],
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        let f = self.seat.home.path().join(name);
        std::fs::create_dir_all(&f).unwrap();
        f
    }

    async fn approve_computer(&mut self, account: char) -> GroupId {
        if let Some((_, group)) = self.computers.iter().find(|(a, _)| *a == account) { return *group }
        let human = if account == 'A' { &mut self.a } else { &mut self.b };
        let group = self.seat.approve_computer(human).await;
        self.computers.push((account, group));
        group
    }

    async fn admit_any(&mut self, account: char) -> (DeviceId, GroupId) {
        let computer = self.computers.iter().find(|(a, _)| *a == account).expect("approved computer").1;
        let (admitted, group, _) = self.human(account).admit_activation(&computer).await;
        self.groups.push((account, group));
        (admitted.device, group)
    }

    fn human(&mut self, account: char) -> &mut Human {
        if account == 'A' {
            &mut self.a
        } else {
            &mut self.b
        }
    }
}

/// The program's process a session runs in.
fn program_process() -> tokio::process::Child {
    tokio::process::Command::new("sleep")
        .arg("900")
        .kill_on_drop(true)
        .spawn()
        .expect("sleep runs")
}

/// One agent session: its program, its id (Claude Code's session id, Codex's thread), its folder and the config
/// folder of its program's account; the connector it runs now, if any; where it is in (account, device, group).
struct Session {
    name: String,
    client: Client,
    id: String,
    folder: PathBuf,
    config: String,
    parent: std::sync::Arc<tokio::process::Child>,
    mcp: Option<Mcp>,
    on: Option<(char, DeviceId, GroupId)>,
}

impl Session {
    fn new(m: &Machine, name: &str, client: Client, folder: &str, config: &str) -> Session {
        Session {
            name: name.to_string(),
            client,
            id: uuid(),
            folder: m.folder(folder),
            config: config.to_string(),
            parent: std::sync::Arc::new(program_process()),
            mcp: None,
            on: None,
        }
    }

    /// Another thread of the same Codex app server (one process, one parent for every thread's connector).
    fn sibling(&self, m: &Machine, name: &str, folder: &str) -> Session {
        Session {
            name: name.to_string(),
            client: self.client,
            id: uuid(),
            folder: m.folder(folder),
            config: self.config.clone(),
            parent: self.parent.clone(),
            mcp: None,
            on: None,
        }
    }

    fn command(&self, m: &Machine) -> tokio::process::Command {
        let mut c = m.seat.command(&[]);
        let config = m.seat.home.path().join(&self.config);
        std::fs::create_dir_all(&config).unwrap();
        c.env_remove("TROMMI_SESSION_KEY")
            .env("TROMMI_FOLDER", &self.folder)
            .current_dir(&self.folder)
            .env("TROMMI_PARENT_PID", self.parent.id().unwrap().to_string())
            .env("TROMMI_CONNECT_WAIT_MS", "60000");
        match self.client {
            Client::Claude => {
                c.env("CLAUDE_CODE_SESSION_ID", &self.id)
                    .env("CLAUDE_CONFIG_DIR", &config);
            }
            Client::Codex => {
                c.env("CODEX_HOME", &config)
                    .env("TROMMI_CHANNEL_EVENTS", "off")
                    .env("TROMMI_CODEX_BIN", &m.codex_bin);
            }
        }
        c
    }

    /// Starts its connector (a new one; a running one is left as it is, as /mcp Reconnect does).
    async fn start(&mut self, m: &Machine) -> Option<Mcp> {
        let hello = match self.client {
            Client::Claude => json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "claude-code", "version": "2.1.0" } }),
            Client::Codex => codex_hello(),
        };
        // TROMMI_MATRIX_LOGS=<folder>: each connector's stderr into a file of its own there
        let log = std::env::var_os("TROMMI_MATRIX_LOGS").map(|dir| {
            let test = std::thread::current()
                .name()
                .unwrap_or("test")
                .replace("::", "-");
            let file = PathBuf::from(dir).join(format!("{test}-{}-{}.log", self.name, random_hex(2)));
            std::fs::File::create(file).expect("a log file")
        });
        let mut mcp = Mcp::start_logged(self.command(m), hello, log).await;
        if self.client == Client::Codex {
            mcp.call_meta = Some(codex_meta(&self.id));
        }
        let _ = mcp.request("tools/list", json!({})).await;
        self.mcp.replace(mcp)
    }

    fn mcp(&mut self) -> &mut Mcp {
        self.mcp.as_mut().expect("a running connector")
    }

    /// Ends the session: its connector's stdin closes, its program ends.
    async fn close(&mut self) {
        if let Some(mcp) = self.mcp.take() {
            mcp.close().await;
        }
    }

    /// Resumes the session: a new program process with the same session (Claude Code `--resume`, `codex resume`).
    async fn resume(&mut self, m: &Machine) {
        self.close().await;
        self.parent = std::sync::Arc::new(program_process());
        self.start(m).await;
    }

    /// A tool call that fails: what it says.
    async fn refused(&mut self, tool: &str) -> String {
        let (said, failed) = self.mcp().call(tool, json!({})).await;
        assert!(failed, "{}: {tool} worked: {said}", self.name);
        said
    }

    /// Joins by an invite link of `account`, typed into the session (connect tool).
    async fn connect(&mut self, m: &mut Machine, account: char) {
        let human = if account == 'A' { &mut m.a } else { &mut m.b };
        let mut invite = human.invite(None).await;
        let link = invite.link.clone();
        let mcp = self.mcp.as_mut().expect("a running connector");
        let connect = mcp.call("connect", json!({ "link": link }));
        let admit = async {
            let admitted = human.admit(&mut invite).await;
            let group = human.found_session(&admitted).await;
            (admitted.device, group)
        };
        let (device, group) = joined(&self.name, connect, admit).await;
        self.on = Some((account, device, group));
        m.groups.push((account, group));
        self.mcp().ready().await;
    }

    /// Explicitly activate on the selected board, using the approved computer even if no session runs.
    async fn activate(&mut self, m: &mut Machine, voucher: &Session) {
        let (account, _, _) = voucher.on.expect("the selected board");
        m.approve_computer(account).await;
        let room = m.human(account).room.to_base64url();
        std::fs::create_dir_all(self.folder.join(".trommi")).unwrap();
        std::fs::write(self.folder.join(".trommi/joined"), room).unwrap();
        let activation = self.mcp().call("connect", json!({}));
        let admit = m.admit_any(account);
        let ((said, failed), (device, group)) = tokio::join!(activation, admit);
        assert!(!failed, "explicit activation: {said}");
        self.on = Some((account, device, group));
        self.mcp().ready().await;
    }

    /// Says something on the board, and checks that it arrives from its own device in its own session of the
    /// right account.
    async fn speaks(&mut self, m: &mut Machine) {
        let (account, device, group) = self.on.expect("on the board");
        let text = format!("{} says {}", self.name, random_hex(3));
        self.mcp().ready().await;
        self.mcp().ok("reply", json!({ "text": text })).await;
        let session = hex(group.session_id().unwrap().as_bytes());
        let human = m.human(account);
        for _ in 0..100 {
            human.sync().await;
            if human
                .items
                .iter()
                .any(|(s, sender, p)| *s == session && *sender == device && p["text"] == text)
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!(
            "{}: {text:?} did not arrive in its session of account {account}",
            self.name
        );
    }
}

/// A connect call and the human's side of its join, together: a connect that fails ends the case at once with
/// what it said (the human would otherwise wait for a Request that never comes).
async fn joined<T>(
    name: &str,
    connect: impl std::future::Future<Output = (String, bool)>,
    admit: impl std::future::Future<Output = T>,
) -> T {
    tokio::pin!(connect);
    tokio::pin!(admit);
    let mut said = None;
    let out = loop {
        tokio::select! {
            r = &mut connect, if said.is_none() => {
                let (text, failed) = r;
                assert!(!failed && text.starts_with("Check code:"), "{name}: connect: {text}");
                said = Some(text);
            }
            out = &mut admit => break out,
        }
    };
    if said.is_none() {
        let (text, failed) = connect.await;
        assert!(!failed && text.starts_with("Check code:"), "{name}: connect: {text}");
    }
    out
}

fn uuid() -> String {
    let h = random_hex(16);
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// The end of every case: each session speaks in its own session, from a device of its own, and no human found
/// anything it could not take.
async fn all_on_board(m: &mut Machine, sessions: &mut [&mut Session]) {
    let mut devices = vec![];
    for s in sessions.iter() {
        let (_, device, _) = s.on.expect("on the board");
        assert!(
            !devices.contains(&device),
            "{} shares a device with another session",
            s.name
        );
        devices.push(device);
    }
    for s in sessions.iter_mut() {
        s.speaks(m).await;
    }
    m.a.sync().await;
    m.b.sync().await;
    assert!(m.a.findings.is_empty(), "{:?}", m.a.findings);
    assert!(m.b.findings.is_empty(), "{:?}", m.b.findings);
}

/// A refusal that says what the human does: an invite in the Trommi app and where its link goes.
fn says_what_to_do(client: Client, said: &str) {
    assert!(
        !said.contains("Call the tool again"),
        "no loop: {said}"
    );
    assert!(said.to_lowercase().contains("connect"), "{said}");
    match client {
        Client::Codex => assert!(said.contains("connect"), "{said}"),
        Client::Claude => assert!(said.contains("/trommi:connect"), "{said}"),
    }
}

macro_rules! both {
    ($fast:expr, $case:ident) => {
        mod $case {
            #[tokio::test(flavor = "multi_thread")]
            async fn claude() {
                if super::runs($fast) {
                    super::$case(super::Client::Claude).await
                }
            }
            #[tokio::test(flavor = "multi_thread")]
            async fn codex() {
                if super::runs($fast) {
                    super::$case(super::Client::Codex).await
                }
            }
        }
    };
}

// ---- the cases ------------------------------------------------------------------------------------------------

both!(true, fresh_sessions_one_after_another_in_one_folder);
/// Three fresh sessions in one folder, one after another: the first by link, the others by themselves; each
/// resumed is the same member again.
async fn fresh_sessions_one_after_another_in_one_folder(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    s2.activate(&mut m, &s1).await;
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg");
    s3.start(&m).await;
    s3.activate(&mut m, &s2).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
    s1.resume(&m).await;
    s2.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

both!(false, fresh_sessions_one_after_another_in_other_folders);
/// The same in three folders.
async fn fresh_sessions_one_after_another_in_other_folders(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "one", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "two", "cfg");
    s2.start(&m).await;
    s2.activate(&mut m, &s1).await;
    let mut s3 = Session::new(&m, "s3", client, "three", "cfg");
    s3.start(&m).await;
    s3.activate(&mut m, &s1).await;
    s1.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

both!(true, many_fresh_sessions_at_once);
/// Four fresh sessions start at once, two in the first one's folder and two elsewhere; all join by themselves.
async fn many_fresh_sessions_at_once(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut news: Vec<Session> = ["project", "project", "other", "third"]
        .iter()
        .enumerate()
        .map(|(i, f)| Session::new(&m, &format!("n{i}"), client, f, "cfg"))
        .collect();
    for s in news.iter_mut() {
        s.start(&m).await;
    }
    m.approve_computer('A').await;
    let calls = news.iter_mut().map(|s| async move { s.mcp().call("connect", json!({})).await });
    let admissions = async {
        let mut admitted = vec![];
        for _ in 0..4 { admitted.push(m.admit_any('A').await); }
        admitted
    };
    let (answers, admitted) = tokio::join!(futures_join_all(calls), admissions);
    for (said, failed) in answers { assert!(!failed, "{said}"); }
    for s in news.iter_mut() {
        s.mcp().ready().await;
    }
    // which device is whose: each session's words come from one of the admitted devices, each from another
    for s in news.iter_mut() {
        let text = format!("{} here", s.name);
        s.mcp().ok("reply", json!({ "text": text })).await;
        let mut found = None;
        for _ in 0..100 {
            m.a.sync().await;
            found = admitted.iter().copied().find(|(d, g)| {
                let session = hex(g.session_id().unwrap().as_bytes());
                m.a.items
                    .iter()
                    .any(|(x, sender, p)| *x == session && sender == d && p["text"] == text)
            });
            if found.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let (d, g) = found.unwrap_or_else(|| panic!("{} is on the board", s.name));
        s.on = Some(('A', d, g));
    }
    let mut all: Vec<&mut Session> = vec![&mut s1];
    all.extend(news.iter_mut());
    all_on_board(&mut m, &mut all).await;
}

async fn futures_join_all<F: std::future::Future>(fs: impl Iterator<Item = F>) -> Vec<F::Output> {
    let handles: Vec<_> = fs.collect();
    let mut out = vec![];
    // (run together: each waits for its own answer)
    let mut pinned: Vec<std::pin::Pin<Box<F>>> = handles.into_iter().map(Box::pin).collect();
    let mut done: Vec<Option<F::Output>> = pinned.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut all = true;
        for (i, f) in pinned.iter_mut().enumerate() {
            if done[i].is_none() {
                match f.as_mut().poll(cx) {
                    std::task::Poll::Ready(v) => done[i] = Some(v),
                    std::task::Poll::Pending => all = false,
                }
            }
        }
        if all {
            std::task::Poll::Ready(())
        } else {
            std::task::Poll::Pending
        }
    })
    .await;
    for d in done {
        out.push(d.unwrap());
    }
    out
}

both!(true, accounts_a_and_b_in_one_folder);
/// A session of account A and one of account B in the same folder, each by its link (the second one's connector
/// started asleep beside the first); each resumed is in its own account's room again; a fresh third one gets in
/// by a link of A while both run.
async fn accounts_a_and_b_in_one_folder(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    s2.connect(&mut m, 'B').await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2]).await;
    s1.resume(&m).await;
    s2.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2]).await;
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg");
    s3.start(&m).await;
    s3.connect(&mut m, 'A').await;
    s1.resume(&m).await;
    s2.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

both!(false, accounts_a_and_b_across_folders);
/// A in one folder, B in another: fresh sessions in each folder join by themselves into that folder's account;
/// a fresh session in a third folder, with two accounts on the machine, says how to get in and joins by link.
async fn accounts_a_and_b_across_folders(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "one", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "two", "cfg");
    s2.start(&m).await;
    s2.connect(&mut m, 'B').await;
    let mut s3 = Session::new(&m, "s3", client, "one", "cfg");
    s3.start(&m).await;
    s3.activate(&mut m, &s1).await;
    let mut s4 = Session::new(&m, "s4", client, "two", "cfg");
    s4.start(&m).await;
    s4.activate(&mut m, &s2).await;
    let mut s5 = Session::new(&m, "s5", client, "three", "cfg");
    s5.start(&m).await;
    let said = s5.refused("inbox").await;
    says_what_to_do(client, &said);
    s5.connect(&mut m, 'B').await;
    s1.resume(&m).await;
    s2.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3, &mut s4, &mut s5]).await;
}

both!(true, an_mcp_restart_and_a_killed_connector_keep_the_member);
/// /mcp Reconnect (a new connector while the old one still runs) and a connector killed hard: the same session
/// is the same member each time; a fresh session beside it is not.
async fn an_mcp_restart_and_a_killed_connector_keep_the_member(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    s2.activate(&mut m, &s1).await;
    // /mcp Reconnect: the old connector is still there when the new one starts
    let old = s1.start(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2]).await;
    drop(old);
    // killed hard
    let mut gone = s1.mcp.take().unwrap();
    gone.child.start_kill().unwrap();
    let _ = gone.child.wait().await;
    s1.start(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2]).await;
    // killed, and a fresh session starts before the killed one comes back: it does not take that key
    let mut gone = s2.mcp.take().unwrap();
    gone.child.start_kill().unwrap();
    let _ = gone.child.wait().await;
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg");
    s3.start(&m).await;
    s3.activate(&mut m, &s1).await;
    s2.start(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

both!(true, the_stale_key_of_a_closed_session_stays_its_own);
/// A session ends; a fresh one in its folder, without explicit activation, does not take the ended one's key:
/// it says how to get in and joins by link. The ended session resumed is itself again.
async fn the_stale_key_of_a_closed_session_stays_its_own(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    s1.close().await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    let said = s2.refused("inbox").await;
    says_what_to_do(client, &said);
    s2.connect(&mut m, 'A').await;
    s1.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2]).await;
}

both!(false, explicit_activation_beside_a_link_join);
/// Explicit activation and a direct invite proceed concurrently, into separate accounts and devices.
async fn explicit_activation_beside_a_link_join(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let computer = m.approve_computer('A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    let mut s3 = Session::new(&m, "s3", client, "other", "cfg");
    s2.start(&m).await;
    s3.start(&m).await;
    assert!(s2.refused("inbox").await.contains("connect"));
    assert_eq!(m.a.open_asks(&computer).await, 0);
    let mut invite = m.b.invite(None).await;
    let link = invite.link.clone();
    let activation = s2.mcp().call("connect", json!({}));
    let admit_activation = m.a.admit_activation(&computer);
    let connect = s3.mcp().call("connect", json!({"link":link}));
    let admit_link = async {
        let admitted = m.b.admit(&mut invite).await;
        (admitted.device, m.b.found_session(&admitted).await)
    };
    let ((said, failed), (child, group, _), (device, direct_group)) = tokio::join!(activation, admit_activation, joined("direct", connect, admit_link));
    assert!(!failed, "{said}");
    s2.on = Some(('A', child.device, group));
    s3.on = Some(('B', device, direct_group));
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

both!(true, connect_asleep_awake_and_in_the_other_accounts_room);
/// connect with a link while the connector sleeps (it started beside another session), after it woke, and
/// while the session is in the other account's room already (refused, saying what to do; the link stays
/// unused and works for a new session).
async fn connect_asleep_awake_and_in_the_other_accounts_room(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    // asleep: started beside s1, never called
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    s2.connect(&mut m, 'B').await;
    // awake: called before, then connect
    let mut s3 = Session::new(&m, "s3", client, "other", "cfg");
    s3.start(&m).await;
    let said = s3.refused("inbox").await;
    assert!(!said.contains("Call the tool again"), "{said}");
    s3.connect(&mut m, 'A').await;
    // in A's room already, a link of B
    let invite = m.b.invite(None).await;
    let (said, failed) = s1
        .mcp()
        .call("connect", json!({ "link": invite.link.clone() }))
        .await;
    assert!(failed, "{said}");
    assert!(
        said.contains("another") && said.contains("new") && !said.contains("no link is needed"),
        "it says the session is in another account's room and how to use this link: {said}"
    );
    let mut s4 = Session::new(&m, "s4", client, "project", "cfg");
    s4.start(&m).await;
    let mut invite = invite;
    let link = invite.link.clone();
    let connect = s4.mcp.as_mut().unwrap().call("connect", json!({ "link": link }));
    let admit = async {
        let admitted = m.b.admit(&mut invite).await;
        (admitted.device, m.b.found_session(&admitted).await)
    };
    let (d, g) = joined("s4", connect, admit).await;
    s4.on = Some(('B', d, g));
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3, &mut s4]).await;
}

both!(true, two_accounts_of_the_agent_program);
/// Program config folders confer no computer authority. Direct invites remain usable; later sessions
/// of either config activate through the same approved computer.
async fn two_accounts_of_the_agent_program(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg-x");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg-y");
    s2.start(&m).await;
    let said = s2.refused("inbox").await;
    says_what_to_do(client, &said);
    s2.connect(&mut m, 'A').await;
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg-y");
    s3.start(&m).await;
    s3.activate(&mut m, &s2).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

/// Claude Code and Codex in one folder, both accounts: none takes the other's key; Codex resumed is itself.
#[tokio::test(flavor = "multi_thread")]
async fn claude_and_codex_in_one_folder_with_both_accounts() {
    if !runs(true) {
        return;
    }
    let mut m = Machine::new().await;
    let mut c1 = Session::new(&m, "claude1", Client::Claude, "project", "cfg-claude");
    c1.start(&m).await;
    c1.connect(&mut m, 'A').await;
    let mut x1 = Session::new(&m, "codex1", Client::Codex, "project", "cfg-codex");
    x1.start(&m).await;
    x1.connect(&mut m, 'B').await;
    let mut c2 = Session::new(&m, "claude2", Client::Claude, "project", "cfg-claude");
    c2.start(&m).await;
    c2.activate(&mut m, &c1).await;
    let mut x2 = Session::new(&m, "codex2", Client::Codex, "project", "cfg-codex");
    x2.start(&m).await;
    x2.activate(&mut m, &x1).await;
    x1.resume(&m).await;
    c1.resume(&m).await;
    all_on_board(&mut m, &mut [&mut c1, &mut x1, &mut c2, &mut x2]).await;
}

/// Codex's app server: every thread's connector has the same parent. A second thread does not stop the first
/// one's connector nor take its key; it joins by itself; the first thread's connector started again (same
/// thread) is the same member.
#[tokio::test(flavor = "multi_thread")]
async fn codex_app_server_threads_share_a_parent() {
    if !runs(true) {
        return;
    }
    let mut m = Machine::new().await;
    let mut t1 = Session::new(&m, "t1", Client::Codex, "project", "cfg-codex");
    t1.start(&m).await;
    t1.connect(&mut m, 'A').await;
    let mut t2 = t1.sibling(&m, "t2", "project");
    t2.start(&m).await;
    t2.activate(&mut m, &t1).await;
    let mut t3 = t1.sibling(&m, "t3", "other");
    t3.start(&m).await;
    t3.activate(&mut m, &t1).await;
    all_on_board(&mut m, &mut [&mut t1, &mut t2, &mut t3]).await;
    // the app server starts t1's connector again (the thread resumed in the same process)
    t1.close().await;
    t1.start(&m).await;
    all_on_board(&mut m, &mut [&mut t1, &mut t2, &mut t3]).await;
}

// ---- ownership by the session's id (stage 1: no guessing) ------------------------------------------------------

impl Session {
    /// Who this session is to its keys' owner records.
    fn owner(&self) -> String {
        match self.client {
            Client::Claude => format!("cc:{}", self.id),
            Client::Codex => format!("codex:{}", self.id),
        }
    }
}

/// The owner records of the machine that name `owner`.
fn owner_files(m: &Machine, owner: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    for room in std::fs::read_dir(m.seat.home.path().join("keys")).into_iter().flatten().flatten() {
        for f in trommi_connector::member::room_slot_dirs(&room.path()).into_iter()
            .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten()) {
            let p = f.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".owner")
                && !name.starts_with("replaced-")
                && std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                    .is_some_and(|v| v["owner"] == owner)
            {
                out.push(p);
            }
        }
    }
    out
}

both!(true, a_key_of_no_known_session_is_never_taken);
/// A key whose owner record is missing, and one that names only a process (as keys of earlier connectors without
/// a session id): a fresh session in that folder takes neither, says how to get in, and joins by link.
async fn a_key_of_no_known_session_is_never_taken(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    s2.activate(&mut m, &s1).await;
    s1.close().await;
    s2.close().await;
    // s1's key without a record; s2's naming a process that ended
    let f1 = owner_files(&m, &s1.owner());
    let f2 = owner_files(&m, &s2.owner());
    assert_eq!((f1.len(), f2.len()), (1, 1));
    std::fs::remove_file(&f1[0]).unwrap();
    std::fs::write(&f2[0], r#"{"owner":"","claude_pid":4194300,"claude_start":"","at":0}"#).unwrap();
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg");
    s3.start(&m).await;
    let said = s3.refused("inbox").await;
    says_what_to_do(client, &said);
    s3.connect(&mut m, 'A').await;
    let (_, d1, _) = s1.on.unwrap();
    let (_, d2, _) = s2.on.unwrap();
    let (_, d3, _) = s3.on.unwrap();
    assert!(d3 != d1 && d3 != d2, "s3 has a key of its own");
    all_on_board(&mut m, &mut [&mut s3]).await;
}

/// A Claude Code session resumed in another folder: its key is found by its id where it lies.
#[tokio::test(flavor = "multi_thread")]
async fn a_session_resumed_in_another_folder_finds_its_key() {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", Client::Claude, "one", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    s1.close().await;
    s1.folder = m.folder("two");
    s1.resume(&m).await;
    all_on_board(&mut m, &mut [&mut s1]).await;
}

/// The asks the person sees in the session of `voucher` settle at `n` within `ms`; Err with what they were.
async fn asks_settle(m: &mut Machine, voucher: &GroupId, n: usize, ms: u64) -> Result<(), usize> {
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    loop {
        let open = m.a.open_asks(voucher).await;
        if open == n {
            return Ok(());
        }
        if std::time::Instant::now() > end {
            return Err(open);
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// The coordinator's session resumed under another account of its program and in another folder, beside another
/// session's key there, while its earlier process's connector still holds its key (2026-10-14): it is that member
/// and waits for its key, says so, never asks a session of the new account to let it in as a new agent, and is
/// back with its own device in its own board session once the earlier connector is gone.
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_session_waits_for_its_own_key_and_never_joins_anew() {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", Client::Claude, "trommi", "first");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    // a session of the second account, which could vouch for that account's sessions
    let mut v = Session::new(&m, "v", Client::Claude, "pspkit", "second");
    v.start(&m).await;
    v.connect(&mut m, 'A').await;
    // another session's key in the folder the coordinator goes on in
    let mut o = Session::new(&m, "o", Client::Claude, "saeckel", "third");
    o.start(&m).await;
    o.connect(&mut m, 'A').await;
    let was = s1.on;

    let old_parent = s1.parent.clone();
    let old = s1.mcp.take().expect("the earlier connector");
    s1.parent = std::sync::Arc::new(program_process());
    s1.folder = m.folder("saeckel");
    s1.config = "second".into();
    s1.start(&m).await;
    let said = s1.refused("inbox").await;
    assert!(
        said.contains("its own key") && said.contains("held by connector process"),
        "{said}"
    );
    let (_, _, vg) = v.on.unwrap();
    assert_eq!(asks_settle(&mut m, &vg, 1, 3000).await, Err(0), "no ask to let it in anew");

    // the earlier process ends: its key is free
    old.close().await;
    drop(old_parent);
    all_on_board(&mut m, &mut [&mut s1, &mut v, &mut o]).await;
    assert_eq!(s1.on, was, "the same device in the same board session");
    assert_eq!(m.a.open_asks(&vg).await, 0);
    assert_eq!(owner_files(&m, &s1.owner()).len(), 1, "one key for the session");
}

/// A session's door, as another connector of this machine knocks at it.
async fn knock_session(m: &Machine, s: &Session, req: serde_json::Value) -> serde_json::Value {
    assert!(s.on.is_some(), "{} is on the board", s.name);
    let keys = m.seat.home.path().join("keys");
    for room in std::fs::read_dir(&keys).unwrap().flatten() {
        for f in trommi_connector::member::room_slot_dirs(&room.path()).into_iter()
            .flat_map(|dir| std::fs::read_dir(dir).unwrap().flatten()) {
            let p = f.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(".owner") else { continue };
            let mine = std::fs::read_to_string(&p)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .is_some_and(|v| v["owner"] == s.owner());
            if !mine || stem.starts_with("replaced-") {
                continue;
            }
            let key = p.with_extension("key");
            let sock = m
                .seat
                .run_dir()
                .join(format!("trommi-{}", trommi_connector::slots::uid()))
                .join(format!(
                    "{}.sock",
                    &hex(&trommi_connector::util::sha256(key.display().to_string().as_bytes()))[..20]
                ));
            return trommi_connector::door::knock_at(&sock, &req, 10_000)
                .await
                .expect("the session's door answers");
        }
    }
    panic!("{}: no key of its own", s.name);
}

/// The removed running-voucher protocol never grants authority, even to a local peer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_sessions_refuse_the_removed_voucher_protocol() {
    let mut m = Machine::new().await;
    let mut session = Session::new(&m, "ordinary", Client::Claude, "one", "cfg");
    session.start(&m).await;
    session.connect(&mut m, 'A').await;
    let reply = knock_session(&m, &session, json!({"op":"vouch","owner":"cc:other","account":"claimed"})).await;
    assert_ne!(reply["ok"], true, "ordinary agents cannot admit sessions: {reply}");
}

/// The link of an invite for the coordinator's own session, given to `trommi-connector connect` in a terminal of
/// another folder (its first one) while the resumed session waits for its key: the running session takes the new
/// key at once, without a restart, and is back in its own board session.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_joined_in_a_terminal_of_another_folder_reaches_the_waiting_session() {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", Client::Claude, "trommi", "first");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let (_, old_device, group) = s1.on.unwrap();
    let first = s1.folder.clone();

    // resumed elsewhere, its key held by its earlier connector, which does not let go (hung, say)
    let old_parent = s1.parent.clone();
    let old = s1.mcp.take().expect("the earlier connector");
    s1.parent = std::sync::Arc::new(program_process());
    s1.folder = m.folder("saeckel");
    s1.config = "second".into();
    s1.start(&m).await;
    let _ = s1.refused("inbox").await;

    // the human makes an invite for that session; the agent runs connect in a terminal in its first folder
    let mut invite = m.a.invite(group.session_id()).await;
    let mut cmd = s1.command(&m);
    cmd.env("TROMMI_FOLDER", &first)
        .current_dir(&first)
        .args(["connect", invite.link.as_str()]);
    let join = cmd.output();
    let human = &mut m.a;
    let admit = async {
        let admitted = human.admit(&mut invite).await;
        human.take_over(&group, old_device, &admitted).await;
        admitted
    };
    let (out, admitted) = tokio::join!(join, admit);
    let out = out.expect("connect runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // the earlier process ends (its connector, retired by the takeover, would else look for the session's key too)
    old.close().await;
    drop(old_parent);
    s1.on = Some(('A', admitted.device, group));
    all_on_board(&mut m, &mut [&mut s1]).await;
    assert_eq!(owner_files(&m, &s1.owner()).len(), 1, "one key for the session");
}

/// Keys of one session in two rooms (copied by hand, say): refused by name, never one picked by directory order.
#[tokio::test(flavor = "multi_thread")]
async fn keys_of_one_session_in_two_rooms_are_refused_by_name() {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", Client::Claude, "one", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", Client::Claude, "two", "cfg");
    s2.start(&m).await;
    s2.connect(&mut m, 'B').await;
    s1.close().await;
    s2.close().await;
    let f1 = owner_files(&m, &s1.owner());
    let f2 = owner_files(&m, &s2.owner());
    let text = std::fs::read_to_string(&f2[0]).unwrap().replace(&s2.owner(), &s1.owner());
    std::fs::write(&f2[0], text).unwrap();
    // A manually copied session key includes its owner directory. Merely changing a record under
    // another owner's hash must not make that other owner's subtree discoverable.
    let from = f2[0].parent().unwrap();
    if from.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "s") {
        std::fs::rename(from, from.with_file_name(f1[0].parent().unwrap().file_name().unwrap())).unwrap();
    }
    s1.resume(&m).await;
    let said = s1.refused("inbox").await;
    assert!(
        said.contains("2 Trommi rooms") && said.contains("move the keys"),
        "{said}"
    );
}

/// A Codex connector serves the first thread that names itself; a call naming another thread changes nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_codex_connector_keeps_its_first_thread() {
    let mut m = Machine::new().await;
    let mut t1 = Session::new(&m, "t1", Client::Codex, "project", "cfg");
    t1.start(&m).await;
    t1.connect(&mut m, 'A').await;
    let other = uuid();
    t1.mcp().call_meta = Some(codex_meta(&other));
    all_on_board(&mut m, &mut [&mut t1]).await;
    assert!(owner_files(&m, &format!("codex:{other}")).is_empty());
    assert_eq!(owner_files(&m, &t1.owner()).len(), 1);
}

/// The public MCP path: approve a computer with emoji once, then explicitly activate another session
/// after the approving connector ended. Merely starting and listing/calling tools never asks to join.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn approved_computer_activates_only_explicit_sessions_without_a_running_voucher() {
    let mut m = Machine::new().await;
    let mut first = Session::new(&m, "approval", Client::Claude, "one", "cfg");
    first.start(&m).await;
    let mut invite = m.a.invite(None).await;
    let link = invite.link.clone();
    let human = &mut m.a;
    let connect = first.mcp().call("connect", json!({"link":link,"computer":true}));
    let admit = async {
        let machine = human.admit(&mut invite).await;
        eprintln!("computer emoji enrolment confirmed");
        let group = human.found_session(&machine).await;
        let sid = hex(group.session_id().unwrap().as_bytes());
        let room = GroupId::room(human.room);
        human.set_register(&room, &format!("session/{sid}"), Some(json!({"machine":{"device":hex(machine.device.as_bytes())}}))).await.unwrap();
        human.set_register(&group, "machine_approved", Some(json!({"device":hex(machine.device.as_bytes())}))).await.unwrap();
        eprintln!("computer grant and acknowledgement posted; waiting for first activation");
        let (child, session, _) = human.admit_activation(&group).await;
        (machine.device, group, child.device, session)
    };
    let (computer, group, device, session) = joined("computer approval", connect, admit).await;
    assert_ne!(computer, device, "computer authority and conversation are separate devices");
    first.on = Some(('A', device, session));
    m.groups.push(('A', group));
    first.mcp().ready().await;
    first.speaks(&mut m).await;
    first.close().await;
    let mut second = Session::new(&m, "activation", Client::Claude, "two", "another-program-config");
    second.start(&m).await;
    let said = second.refused("inbox").await;
    assert!(said.contains("connect"), "{said}");
    assert_eq!(m.a.open_asks(&group).await, 0, "startup, tools/list and inbox do not activate");
    let activation = second.mcp().call("connect", json!({}));
    let admit = m.a.admit_activation(&group);
    let ((said, failed), (child, session, _)) = tokio::join!(activation, admit);
    assert!(!failed, "{said}");
    assert_ne!(child.device, device);
    assert_ne!(child.device, computer);
    second.on = Some(('A', child.device, session));
    second.mcp().ready().await;
    second.speaks(&mut m).await;
    second.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_fresh_mcp_never_consumes_an_invite_from_its_environment() {
    let mut m = Machine::new().await;
    let invite = m.a.invite(None).await;
    let session = Session::new(&m, "unactivated", Client::Claude, "project", "cfg");
    let mut command = session.command(&m);
    command.env("TROMMI_INVITE", &invite.link);
    let mut mcp = Mcp::start_as(command, json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"2.1.0"}})).await;
    let _ = mcp.request("tools/list", json!({})).await;
    let (said, failed) = mcp.call("inbox", json!({})).await;
    assert!(failed && said.contains("connect"), "{said}");
    let served = m.a.hub.get(&format!("/v1/invites/{}", trommi_core::invite::InviteLink::parse(&invite.link).unwrap().invite_id().unwrap().to_base64url())).await.unwrap();
    assert_eq!(served["requests"].as_array().unwrap().len(), 0, "an environment variable is no activation command");
    mcp.close().await;
}
