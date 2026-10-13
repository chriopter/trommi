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
    /// The main session of every agent on a board: (account, group). A running one may vouch for a new session.
    groups: Vec<(char, GroupId)>,
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
        }
    }

    fn folder(&self, name: &str) -> PathBuf {
        let f = self.seat.home.path().join(name);
        std::fs::create_dir_all(&f).unwrap();
        f
    }

    /// The human of `account` allows the next ask to let a session in, whichever running session vouched.
    async fn admit_any(&mut self, account: char) -> (DeviceId, GroupId) {
        let groups: Vec<GroupId> = self
            .groups
            .iter()
            .filter(|(a, _)| *a == account)
            .map(|(_, g)| *g)
            .collect();
        for _ in 0..300 {
            for g in &groups {
                if self.human(account).open_ask(g).await.is_some() {
                    let (admitted, group, _) = self.human(account).admit_self_join(g).await;
                    self.groups.push((account, group));
                    return (admitted.device, group);
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        panic!("no session of account {account} asked to let a session in");
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
            .env("TROMMI_SELF_JOIN_POLL_MS", "200")
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

    /// Joins by itself, vouched by `voucher`, whose human allows it.
    async fn self_join(&mut self, m: &mut Machine, voucher: &Session) {
        let (account, _, _) = voucher.on.expect("a session of the account is on the board");
        let said = self.refused("inbox").await;
        assert!(
            !said.contains("Call the tool again"),
            "{}: {said}",
            self.name
        );
        let (device, new_group) = m.admit_any(account).await;
        self.on = Some((account, device, new_group));
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
    assert!(said.to_lowercase().contains("invite an agent"), "{said}");
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
    s2.self_join(&mut m, &s1).await;
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg");
    s3.start(&m).await;
    s3.self_join(&mut m, &s2).await;
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
    s2.self_join(&mut m, &s1).await;
    let mut s3 = Session::new(&m, "s3", client, "three", "cfg");
    s3.start(&m).await;
    s3.self_join(&mut m, &s1).await;
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
    // every one is used at once: each asks to be let in
    let calls = news
        .iter_mut()
        .map(|s| async move { s.mcp().call("inbox", json!({})).await });
    for (said, failed) in futures_join_all(calls).await {
        assert!(failed && !said.contains("Call the tool again"), "{said}");
    }
    let mut admitted = vec![];
    for _ in 0..news.len() {
        admitted.push(m.admit_any('A').await);
    }
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
    s3.self_join(&mut m, &s1).await;
    let mut s4 = Session::new(&m, "s4", client, "two", "cfg");
    s4.start(&m).await;
    s4.self_join(&mut m, &s2).await;
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
    s2.self_join(&mut m, &s1).await;
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
    s3.self_join(&mut m, &s1).await;
    s2.start(&m).await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3]).await;
}

both!(true, the_stale_key_of_a_closed_session_stays_its_own);
/// A session ends; a fresh one in its folder, with nobody running to vouch, does not take the ended one's key:
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

both!(false, a_self_join_beside_a_pending_link_join);
/// A fresh session waits for its self-join while the human gives it a link: the link wins. Another fresh session
/// joins by itself while a third joins by link at the same time.
async fn a_self_join_beside_a_pending_link_join(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg");
    s2.start(&m).await;
    let said = s2.refused("inbox").await;
    assert!(said.contains("joining by itself"), "{said}");
    s2.connect(&mut m, 'A').await;
    let (_, _, group) = s1.on.unwrap();
    for _ in 0..100 {
        if m.a.open_ask(&group).await.is_none() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    assert!(m.a.open_ask(&group).await.is_none(), "the ask is taken back");
    // one by itself, one by link, at once
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg");
    let mut s4 = Session::new(&m, "s4", client, "other", "cfg");
    s3.start(&m).await;
    s4.start(&m).await;
    let said = s3.refused("inbox").await;
    assert!(said.contains("joining by itself"), "{said}");
    let mut invite = m.b.invite(None).await;
    let link = invite.link.clone();
    let connect = s4.mcp.as_mut().unwrap().call("connect", json!({ "link": link }));
    let link_join = async {
        let admitted = m.b.admit(&mut invite).await;
        let g4 = m.b.found_session(&admitted).await;
        (admitted.device, g4)
    };
    let (d4, g4) = joined("s4", connect, link_join).await;
    let (d3, g3) = m.admit_any('A').await;
    s4.on = Some(('B', d4, g4));
    s3.on = Some(('A', d3, g3));
    s3.mcp().ready().await;
    s4.mcp().ready().await;
    all_on_board(&mut m, &mut [&mut s1, &mut s2, &mut s3, &mut s4]).await;
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
/// Two accounts of the agent program on the machine (two config folders): a session of the second is not
/// vouched for by the first (spec/v1.md 12.4.1), says how to get in, joins by link; later sessions of the
/// second account join by themselves.
async fn two_accounts_of_the_agent_program(client: Client) {
    let mut m = Machine::new().await;
    let mut s1 = Session::new(&m, "s1", client, "project", "cfg-x");
    s1.start(&m).await;
    s1.connect(&mut m, 'A').await;
    let mut s2 = Session::new(&m, "s2", client, "project", "cfg-y");
    s2.start(&m).await;
    let mut said = String::new();
    for _ in 0..50 {
        said = s2.refused("inbox").await;
        if !said.contains("joining by itself") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    says_what_to_do(client, &said);
    s2.connect(&mut m, 'A').await;
    let mut s3 = Session::new(&m, "s3", client, "project", "cfg-y");
    s3.start(&m).await;
    s3.self_join(&mut m, &s2).await;
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
    c2.self_join(&mut m, &c1).await;
    let mut x2 = Session::new(&m, "codex2", Client::Codex, "project", "cfg-codex");
    x2.start(&m).await;
    x2.self_join(&mut m, &x1).await;
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
    t2.self_join(&mut m, &t1).await;
    let mut t3 = t1.sibling(&m, "t3", "other");
    t3.start(&m).await;
    t3.self_join(&mut m, &t1).await;
    all_on_board(&mut m, &mut [&mut t1, &mut t2, &mut t3]).await;
    // the app server starts t1's connector again (the thread resumed in the same process)
    t1.close().await;
    t1.start(&m).await;
    all_on_board(&mut m, &mut [&mut t1, &mut t2, &mut t3]).await;
}
