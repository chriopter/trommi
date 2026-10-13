//! What the connector's end-to-end tests share: the real hub (this workspace's `hub/`) as a child process, and a stand-in for the
//! human's device. The stand-in is the core's `Device` over the connector's journal, driven as an app drives it,
//! and speaks to the hub through the connector's hub client; everything a human's app does in these tests is
//! here, step by step.
#![allow(dead_code)]

pub mod process;

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use trommi_connector::client::hub_for;
use trommi_connector::error::{Fault, Result};
use trommi_connector::hub::{b64, unb64, Hub};
use trommi_connector::keeper::Keeper;
use trommi_connector::store::Journal;
use trommi_connector::util::{hex, now_ms};
use trommi_connector::vault::Vault;
use trommi_core::crypto::{SecretBytes, SystemEntropy};
use trommi_core::device::{
    Accepted, Draft, EnvelopeOutcome, GroupSummary, InviteStep, LogEntry, LogKind, Processed,
    Received, WelcomeExpectation,
};
use trommi_core::envelope::Subject;
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{DeviceId, GroupId, InviteId, ObjectId, RoomId, SessionId};
use trommi_core::invite::{Role, SignedRequest};
use trommi_core::store::{OutboxEntry, OutboxKind};
use trommi_core::Error;

/// A directory under the test run's temporary directory, removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "trommi-connector-test-{name}-{}-{}",
            std::process::id(),
            trommi_connector::util::random_hex(4)
        ));
        std::fs::create_dir_all(&dir).expect("a temporary directory");
        TempDir(dir)
    }
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The hub's binary: `TROMMI_HUB_BIN`, or the hub of this workspace, built beside the tests.
pub fn hub_binary() -> PathBuf {
    match std::env::var_os("TROMMI_HUB_BIN") {
        Some(path) => PathBuf::from(path),
        None => process::built("trommi-hub", "trommi-hub"),
    }
}

/// The real hub, running for one test.
pub struct HubProc {
    child: Child,
    pub url: String,
    pub port: u16,
    pub data: TempDir,
}

impl HubProc {
    /// Starts the hub on a free port with an empty data directory and waits until it answers.
    pub async fn start() -> HubProc {
        let data = TempDir::new("hub");
        Self::start_in(data, 0).await
    }

    /// Starts the hub with `env` on top of the test's own (`HUB_LIMIT_…` for a limit a test reaches).
    pub async fn start_with(env: &[(&str, &str)]) -> HubProc {
        Self::start_in_with(TempDir::new("hub"), 0, env).await
    }

    /// Starts the hub over `data` (a restart keeps its database); `port` 0 picks a free one.
    pub async fn start_in(data: TempDir, port: u16) -> HubProc {
        Self::start_in_with(data, port, &[]).await
    }

    /// The same with `env` on top (applied last: `HUB_URL` here names the address the hub takes as its own, e.g. a
    /// proxy in front of it).
    pub async fn start_in_with(data: TempDir, port: u16, env: &[(&str, &str)]) -> HubProc {
        let binary = hub_binary();
        assert!(
            binary.is_file(),
            "no hub binary at {}: cargo build -p trommi-hub, or set TROMMI_HUB_BIN",
            binary.display()
        );
        // Port 0: the hub binds a port the system picks and announces it on its first line of stdout. Probing a
        // free port here and handing it over would race the other tests' hubs for it.
        let mut command = Command::new(&binary);
        command
            .env_clear()
            .env("HUB_HOST", "127.0.0.1")
            .env("HUB_PORT", port.to_string())
            .env("HUB_DATA", data.path())
            .env("HUB_QUIET", "1")
            .env("HUB_TEST_CONTROL", "1")
            .env("HUB_LIMIT_FOUND_PER_IP_HOUR", "100000")
            .env("HUB_PUSH_HOSTS", "127.0.0.1:9")
            // (the mega test of the clients lifts the limits: MEGA_HUB_ENV_<name>=<value>)
            .envs(
                std::env::vars().filter_map(|(k, v)| {
                    k.strip_prefix("MEGA_HUB_ENV_").map(|k| (k.to_string(), v))
                }),
            )
            .stdin(Stdio::null())
            .stdout(if port == 0 {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::inherit());
        if port != 0 {
            command.env("HUB_URL", format!("http://127.0.0.1:{port}"));
        }
        command.envs(env.iter().copied());
        let mut child = command.spawn().expect("the hub starts");
        let port = if port != 0 {
            port
        } else {
            announced_port(&mut child)
        };
        let url = format!("http://127.0.0.1:{port}");
        let mut hub = HubProc {
            child,
            url,
            port,
            data,
        };
        let client = Hub::new(&hub.url, None, None).expect("a client");
        for _ in 0..1200 {
            if let Ok(Some(status)) = hub.child.try_wait() {
                panic!("the hub exited before it answered: {status}");
            }
            if client
                .open_call(reqwest::Method::GET, "/healthz", None)
                .await
                .is_ok()
            {
                return hub;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("the hub did not come up");
    }

    /// Stops the hub and returns its data directory and port, to start it again.
    pub fn stop(mut self) -> (TempDir, u16) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let data = std::mem::replace(&mut self.data, TempDir(PathBuf::new()));
        (data, self.port)
    }
}

/// The port a hub started with `HUB_PORT=0` announces on its first line of stdout; the rest of its stdout is read
/// and dropped on a thread of its own, so that the hub never writes into a full pipe.
fn announced_port(child: &mut Child) -> u16 {
    use std::io::{BufRead, BufReader};
    let stdout = child.stdout.take().expect("the hub's stdout");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut lines = BufReader::new(stdout).lines();
        let first = lines.next().and_then(|l| l.ok());
        let _ = tx.send(first);
        for _ in lines {}
    });
    let line = rx
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("the hub announces its port")
        .unwrap_or_else(|| {
            panic!(
                "the hub exited before it announced its port: {:?}",
                child.wait()
            )
        });
    let value: Value = serde_json::from_str(&line).expect("the hub's port line is JSON");
    assert_eq!(value["event"], "port", "the hub's first line: {line}");
    value["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .expect("a port")
}

impl Drop for HubProc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// An invite the stand-in made, until its device is in.
pub struct Invite {
    id: InviteId,
    /// The link to hand to the connector.
    pub link: String,
}

/// A device the stand-in admitted.
pub struct Admitted {
    pub device: DeviceId,
    /// The six numbers of the check code, as the app shows them.
    pub code: [u8; 6],
}

/// The human's device: the core's `Device` over a journal, as an app drives it.
pub struct Human {
    vault: Arc<Keeper<Vault>>,
    pub hub: Hub,
    id: DeviceId,
    pub room: RoomId,
    address: HubAddress,
    /// The account made with the room's founding, if any.
    founding_account: Option<Value>,
    _dir: TempDir,
    /// The group of every card, Artifact and request seen, by object id as hex.
    objects: BTreeMap<String, GroupId>,
    requests: BTreeSet<String>,
    /// What the stand-in could not take, by code: a test asserts that it stays empty.
    pub findings: Vec<String>,
    /// The work trail steps that opened: (turn, number, step).
    pub trail: Vec<(String, u32, Value)>,
    /// The Chat messages, versions and register values that opened: (session hex, sender, payload).
    pub items: Vec<(String, DeviceId, Value)>,
    /// The Chat messages on a card's own Chat that opened: (card's object id as hex, payload).
    pub card_chats: Vec<(String, Value)>,
    /// Further human devices a session founded by this device adds at once (their KeyPackages claimed then).
    pub also_add: Vec<DeviceId>,
}

pub fn fault_of(error: Error) -> Fault {
    Fault::from(error)
}

fn payload(value: &Value) -> SecretBytes {
    SecretBytes::new(value.to_string().into_bytes())
}

impl Human {
    /// Runs `job` on the device.
    async fn v<R: Send + 'static>(&self, job: impl FnOnce(&mut Vault) -> R + Send + 'static) -> R {
        self.vault.call(job).await.expect("the device's thread")
    }

    /// A new device that founds a room at the hub and publishes its KeyPackages.
    pub async fn found(url: &str) -> Human {
        Self::found_with(url, |_, _| None).await
    }

    /// The same, with an account made in the founding's request (`POST /v1/rooms` with `account`, as the apps
    /// sign up): `account` is handed the room id and the recovery code and gives the account's body.
    pub async fn found_with(
        url: &str,
        account: impl FnOnce(&RoomId, &trommi_core::crypto::Secret<32>) -> Option<Value>
            + Send
            + 'static,
    ) -> Human {
        let dir = TempDir::new("human");
        let journal = Journal::open(dir.path()).expect("a journal");
        let vault = Arc::new(Keeper::spawn(move || Vault::create(journal)).expect("a device"));
        let (id, room, founding_account) = vault
            .call(move |v: &mut Vault| {
                // The recovery code a human's app shows once; the stand-in forgets it.
                let (code, keys) =
                    trommi_core::recovery::RecoveryKeys::generate(&mut SystemEntropy)
                        .expect("a code");
                let room = v
                    .device
                    .found_room(&keys, now_ms())
                    .expect("the room is founded");
                v.commit().expect("stored");
                let body = account(&room, &code);
                (v.me(), room, body)
            })
            .await
            .expect("the device's thread");
        let hub = hub_for(&vault, url, room).expect("a client");
        let mut human = Human {
            vault,
            hub,
            id,
            room,
            address: HubAddress::parse(url).expect("the hub's address"),
            founding_account,
            _dir: dir,
            objects: BTreeMap::new(),
            requests: BTreeSet::new(),
            findings: Vec::new(),
            trail: Vec::new(),
            items: Vec::new(),
            card_chats: Vec::new(),
            also_add: Vec::new(),
        };
        human.post_all().await.expect("the founding is taken");
        human
            .v(|v| {
                v.device
                    .key_packages_to_upload(0, now_ms())
                    .expect("key packages");
                v.commit().expect("stored");
            })
            .await;
        human.post_all().await.expect("the key packages are taken");
        human
    }

    pub fn id(&self) -> DeviceId {
        self.id
    }

    /// The device a human addresses in a session group: its agent leaf.
    pub async fn seat(&self, group: &GroupId) -> Option<DeviceId> {
        let group = *group;
        self.v(move |v| v.seat(&group)).await
    }

    /// Archives a session group (5.2.10): at the hub, then on the device.
    pub async fn archive(&self, group: &GroupId) {
        self.hub
            .post(
                &format!("/v1/groups/{}/archive", b64(group.as_bytes())),
                &serde_json::json!({}),
            )
            .await
            .expect("archived at the hub");
        let group = *group;
        self.v(move |v| {
            v.device.archive(&group).expect("archived");
            v.commit().expect("stored");
        })
        .await;
    }

    /// The groups the device is a leaf of.
    pub async fn groups(&self) -> Vec<GroupSummary> {
        self.v(|v| v.device.groups().expect("groups")).await
    }

    async fn post(&self, entry: &OutboxEntry) -> Result<Value> {
        let part = |i: usize| entry.parts.get(i).map(|p| b64(p)).unwrap_or_default();
        let has = |i: usize| entry.parts.get(i).is_some_and(|p| !p.is_empty());
        let group = entry.group.map(|g| b64(g.as_bytes())).unwrap_or_default();
        match entry.kind {
            OutboxKind::RoomFounding => {
                let open = Hub::new(self.address.as_str(), None, None)?;
                open.open_call(
                    reqwest::Method::POST,
                    "/v1/rooms",
                    Some(&{
                        let mut body = json!({ "group_info": part(0), "sealed_key": part(1) });
                        if let Some(account) = &self.founding_account {
                            body["account"] = account.clone();
                        }
                        body
                    }),
                )
                .await
            }
            OutboxKind::GroupFounding => {
                let mut body = json!({
                    "group_info_0": part(0), "sealed_key_0": part(1), "commit": part(2),
                    "group_info": part(3), "sealed_key": part(5),
                });
                if has(4) {
                    body["welcome"] = json!(part(4));
                }
                self.hub.post("/v1/groups", &body).await
            }
            OutboxKind::Commit => {
                let mut body = json!({
                    "epoch": entry.epoch, "commit": part(0), "group_info": part(1),
                    "sealed_key": part(3),
                });
                if has(2) {
                    body["welcome"] = json!(part(2));
                }
                self.hub
                    .post(&format!("/v1/groups/{group}/commits"), &body)
                    .await
            }
            OutboxKind::Message => {
                self.hub
                    .post(
                        &format!("/v1/groups/{group}/messages"),
                        &json!({ "epoch": entry.epoch, "message": part(0) }),
                    )
                    .await
            }
            OutboxKind::KeyPackages => {
                let single: Vec<String> = entry.parts.iter().skip(1).map(|p| b64(p)).collect();
                let mut body = json!({ "single_use": single });
                if has(0) {
                    body["last_resort"] = json!(part(0));
                }
                self.hub.put("/v1/key-packages", &body).await
            }
            OutboxKind::Envelope => {
                self.hub
                    .post("/v1/envelopes", &json!({ "envelope": part(0) }))
                    .await
            }
            OutboxKind::SealedKey => {
                self.hub
                    .put("/v1/sealed-keys", &json!({ "sealed_key": part(0) }))
                    .await
            }
            other => panic!("the stand-in posts no {other:?}"),
        }
    }

    /// Posts the device's outbox and reports each answer; the first refusal is returned.
    pub async fn post_all(&mut self) -> Result<()> {
        let entries = self.v(|v| v.device.outbox()).await;
        for entry in entries {
            let id = entry.id;
            let envelope = entry.kind == OutboxKind::Envelope;
            match self.post(&entry).await {
                Ok(answer) => {
                    let accepted = Accepted {
                        change: answer.get("change").and_then(Value::as_u64),
                    };
                    self.v(move |v| -> Result<()> {
                        v.device.outbox_accepted(id, accepted).map_err(fault_of)?;
                        v.commit()
                    })
                    .await?;
                }
                Err(fault) => {
                    let code = fault.as_core().unwrap_or(Error::Internal("refused"));
                    let voided = fault.extra.get("voided") == Some(&Value::Bool(true));
                    self.v(move |v| -> Result<()> {
                        if voided {
                            v.device.outbox_voided(id).map_err(fault_of)?;
                        } else if envelope {
                            v.device.envelope_abandon(id).map_err(fault_of)?;
                        } else {
                            v.device.outbox_refused(id, &code).map_err(fault_of)?;
                        }
                        v.commit()
                    })
                    .await?;
                    return Err(fault);
                }
            }
        }
        Ok(())
    }

    /// Makes an agent invite (for a new session, or to take `session` over) and publishes its Offer.
    pub async fn invite(&mut self, session: Option<SessionId>) -> Invite {
        self.invite_as(Role::Agent, session).await
    }

    /// Makes an invite for a device of `role` and publishes its Offer.
    pub async fn invite_as(&mut self, role: Role, session: Option<SessionId>) -> Invite {
        self.sync().await;
        let address = self.address.clone();
        let opened = self
            .v(move |v| {
                let opened = v
                    .device
                    .invite_open(
                        role,
                        session.as_ref(),
                        "https://app.trommi.example",
                        &address,
                        now_ms(),
                    )
                    .expect("an invite");
                v.commit().expect("stored");
                opened
            })
            .await;
        let offer = &opened.signed_offer;
        self.hub
            .post(
                "/v1/invites",
                &json!({ "offer": b64(&offer.offer), "signature": b64(&offer.signature), "mac": b64(&offer.mac) }),
            )
            .await
            .expect("the hub takes the Offer");
        Invite {
            id: opened.invite_id,
            link: String::from_utf8_lossy(opened.link.expose()).into_owned(),
        }
    }

    /// The link of an agent invite opened `ago_ms` before now and never published: by now it has expired.
    pub async fn expired_link(&mut self, ago_ms: u64) -> String {
        let address = self.address.clone();
        self.v(move |v| {
            let opened = v
                .device
                .invite_open(
                    Role::Agent,
                    None,
                    "https://app.trommi.example",
                    &address,
                    now_ms() - ago_ms,
                )
                .expect("an invite");
            String::from_utf8_lossy(opened.link.expose()).into_owned()
        })
        .await
    }

    /// Waits for the Request to an invite, reveals, "confirms the emoji" and commits the new device: the
    /// change of `agents` in the room group.
    pub async fn admit(&mut self, invite: &mut Invite) -> Admitted {
        let path = format!("/v1/invites/{}", invite.id.to_base64url());
        let request = loop {
            let served = self.hub.get(&path).await.expect("the invite");
            if let Some(first) = served["requests"].as_array().and_then(|list| list.first()) {
                break SignedRequest {
                    request: unb64(first, "request").expect("request"),
                    mac: unb64(first, "mac").expect("mac"),
                    signature: unb64(first, "signature").expect("signature"),
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        let id = invite.id;
        let accepted = self
            .v(move |v| {
                let accepted = v
                    .device
                    .invite_accept(&id, &request, now_ms())
                    .expect("the Request is accepted");
                v.commit().expect("stored");
                accepted
            })
            .await;
        self.hub
            .put(
                &format!("{path}/reveal"),
                &json!({
                    "reveal": b64(&accepted.signed_reveal.reveal),
                    "signature": b64(&accepted.signed_reveal.signature),
                }),
            )
            .await
            .expect("the Reveal is published");
        // The human compares the six emoji and taps "They match".
        let (code, request_hash) = (accepted.code, accepted.request_hash);
        self.v(move |v| {
            v.device
                .invite_confirm(&id, &code, &request_hash, true, now_ms())
                .expect("the code is confirmed")
                .expect("the device is committed");
            v.commit().expect("stored");
        })
        .await;
        self.post_all().await.expect("the enrolment is taken");
        Admitted {
            device: accepted.new_device,
            code: accepted.code.numbers(),
        }
    }

    /// Self-join (spec/v1.md 12.4) as a human device does it: waits for an ask `join_ask/<nonce>` of the agent of
    /// `voucher` (a main session group), answers it with an agent invite link in `join_link/<nonce>`, accepts the
    /// Request that comes, and confirms it on its own once the ask names that Request's hash; then founds the new
    /// session. Returns the device and its main session group, and the ask as it was answered.
    pub async fn admit_self_join(&mut self, voucher: &GroupId) -> (Admitted, GroupId, Value) {
        let session = hex(voucher.session_id().expect("a session group").as_bytes());
        let mut nonce = None;
        for _ in 0..600 {
            self.sync().await;
            nonce = self
                .items
                .iter()
                .filter(|(s, _, payload)| *s == session && !payload["value"].is_null())
                .filter_map(|(_, _, payload)| {
                    payload["name"]
                        .as_str()?
                        .strip_prefix("join_ask/")
                        .map(String::from)
                })
                .last();
            if nonce.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let nonce = nonce.expect("an ask to let a session in");
        let invite = self.invite(None).await;
        self.set_register(
            voucher,
            &format!("join_link/{nonce}"),
            Some(json!({ "link": invite.link, "invite_id": invite.id.to_base64url() })),
        )
        .await
        .expect("the link is written");
        let path = format!("/v1/invites/{}", invite.id.to_base64url());
        let request = loop {
            let served = self.hub.get(&path).await.expect("the invite");
            if let Some(first) = served["requests"].as_array().and_then(|list| list.first()) {
                break SignedRequest {
                    request: unb64(first, "request").expect("request"),
                    mac: unb64(first, "mac").expect("mac"),
                    signature: unb64(first, "signature").expect("signature"),
                };
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        };
        let id = invite.id;
        let accepted = self
            .v(move |v| {
                let accepted = v
                    .device
                    .invite_accept(&id, &request, now_ms())
                    .expect("the Request is accepted");
                v.commit().expect("stored");
                accepted
            })
            .await;
        self.hub
            .put(
                &format!("{path}/reveal"),
                &json!({
                    "reveal": b64(&accepted.signed_reveal.reveal),
                    "signature": b64(&accepted.signed_reveal.signature),
                }),
            )
            .await
            .expect("the Reveal is published");
        // No emoji: the ask must name this very Request.
        let wanted = accepted.request_hash.to_base64url();
        let mut ask = Value::Null;
        for _ in 0..600 {
            self.sync().await;
            ask = self
                .register(voucher, &format!("join_ask/{nonce}"))
                .await
                .unwrap_or(Value::Null);
            if ask["request_hash"].as_str() == Some(wanted.as_str()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert_eq!(
            ask["request_hash"].as_str(),
            Some(wanted.as_str()),
            "the ask names the Request"
        );
        let (code, request_hash) = (accepted.code, accepted.request_hash);
        self.v(move |v| {
            v.device
                .invite_confirm(&id, &code, &request_hash, true, now_ms())
                .expect("confirmed")
                .expect("the device is committed");
            v.commit().expect("stored");
        })
        .await;
        self.post_all().await.expect("the enrolment is taken");
        let admitted = Admitted {
            device: accepted.new_device,
            code: accepted.code.numbers(),
        };
        let group = self.found_session(&admitted).await;
        (admitted, group, ask)
    }

    /// The person taps Deny on the next ask of the agent of `voucher` (12.4): `join_link/<nonce>` = `{ refused: denied }`.
    pub async fn deny_self_join(&mut self, voucher: &GroupId) {
        self.refuse_self_join(voucher, "denied").await
    }

    /// Answers the newest ask in the session of `voucher` with `{ refused: why }` (`denied`, `cap`, `busy`).
    pub async fn refuse_self_join(&mut self, voucher: &GroupId, why: &str) {
        let session = hex(voucher.session_id().expect("a session group").as_bytes());
        let mut nonce = None;
        for _ in 0..600 {
            self.sync().await;
            nonce = self
                .items
                .iter()
                .filter(|(s, _, payload)| *s == session && !payload["value"].is_null())
                .filter_map(|(_, _, payload)| {
                    payload["name"]
                        .as_str()?
                        .strip_prefix("join_ask/")
                        .map(String::from)
                })
                .last();
            if nonce.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let nonce = nonce.expect("an ask to let a session in");
        self.set_register(
            voucher,
            &format!("join_link/{nonce}"),
            Some(json!({ "refused": why })),
        )
        .await
        .expect("the answer is written");
    }

    /// Whether the agent of `voucher` asked to let a session in (`join_ask/…`), up to now.
    pub fn asked_in(&self, voucher: &GroupId) -> bool {
        let session = hex(voucher.session_id().expect("a session group").as_bytes());
        self.items.iter().any(|(s, _, payload)| {
            *s == session
                && payload["name"]
                    .as_str()
                    .is_some_and(|n| n.starts_with("join_ask/"))
        })
    }

    /// Does what the device says is left for the devices it committed by link (`invite_steps`), until
    /// nothing is: founding a session, taking one over, handing the history over.
    pub async fn follow_invites(&mut self) {
        for _ in 0..200 {
            self.sync().await;
            let steps = self
                .v(|v| {
                    let steps = v.device.invite_steps().expect("the steps");
                    v.commit().expect("stored");
                    steps
                })
                .await;
            if steps.is_empty() {
                return;
            }
            let mut waited = true;
            for (invite, step) in steps {
                match step {
                    InviteStep::Wait => continue,
                    InviteStep::Commit => {
                        self.v(move |v| {
                            v.device
                                .invite_recommit(&invite, now_ms())
                                .expect("committed again");
                        })
                        .await;
                    }
                    InviteStep::FoundSession { agent, key_package } => {
                        let mut key_packages = vec![key_package];
                        // every other human device of the room the hub holds a KeyPackage of (5.2.5), as the apps
                        // claim them; and those named in `also_add`
                        let me = self.id;
                        let humans: Vec<DeviceId> = self
                            .v(move |v| {
                                v.device
                                    .room_history()
                                    .map(|h| h.newest().humans.iter().copied().filter(|d| *d != me).collect())
                                    .unwrap_or_default()
                            })
                            .await;
                        let mut others: Vec<DeviceId> = humans;
                        for d in self.also_add.clone() {
                            if !others.contains(&d) {
                                others.push(d);
                            }
                        }
                        let ids: Vec<String> = others.iter().map(|d| d.to_base64url()).collect();
                        if !ids.is_empty() {
                            if let Ok(claimed) = self
                                .hub
                                .post("/v1/key-packages/claim", &json!({ "devices": ids, "partial": true }))
                                .await
                            {
                                for id in &ids {
                                    if let Ok(kp) = unb64(&claimed["key_packages"], id) {
                                        key_packages.push(kp);
                                    }
                                }
                            }
                        }
                        self.v(move |v| {
                            v.device
                                .found_session(&agent, &key_packages, now_ms())
                                .expect("the founding is built");
                        })
                        .await;
                    }
                    InviteStep::TakeOver {
                        group,
                        cuts,
                        agent,
                        key_package,
                    } => {
                        let key_package = match key_package {
                            Some(key_package) => key_package,
                            None => {
                                let claimed = self
                                    .hub
                                    .post(
                                        "/v1/key-packages/claim",
                                        &json!({ "devices": [agent.to_base64url()] }),
                                    )
                                    .await
                                    .expect("a KeyPackage of the agent device");
                                unb64(&claimed["key_packages"], &agent.to_base64url())
                                    .expect("its bytes")
                            }
                        };
                        self.v(move |v| {
                            v.device
                                .clean_session(
                                    &group,
                                    &cuts,
                                    Some((&agent, &key_package)),
                                    now_ms(),
                                )
                                .expect("the takeover is built");
                        })
                        .await;
                    }
                    InviteStep::Handover { .. } => {
                        self.v(move |v| {
                            v.device.invite_handover(&invite).expect("the handover");
                        })
                        .await;
                    }
                    InviteStep::CheckHelpers { session } => {
                        let listed = self
                            .hub
                            .get(&format!("/v1/rooms/{}/groups", self.room.to_base64url()))
                            .await
                            .expect("the room's groups");
                        let parent = b64(session.as_bytes());
                        let helpers: Vec<GroupId> = listed
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|row| {
                                row["kind"] == "helper"
                                    && row["live"] != json!(false)
                                    && row["parent"] == parent.as_str()
                            })
                            .filter_map(|row| {
                                GroupId::from_bytes(&unb64(row, "group_id").ok()?).ok()
                            })
                            .collect();
                        // `group-behind`: a Welcome is still to be taken; the next round does.
                        let _ = self
                            .v(move |v| v.device.invite_checked(&invite, &helpers))
                            .await;
                    }
                    InviteStep::AddToSession { group, device } => {
                        // a human device missing from a live session (5.2.7): one KeyPackage of it, the Commit,
                        // the session's old keys; a device the hub holds none of waits for the next round
                        let id = device.to_base64url();
                        let Ok(claimed) = self
                            .hub
                            .post("/v1/key-packages/claim", &json!({ "devices": [id.clone()] }))
                            .await
                        else {
                            continue;
                        };
                        let Ok(kp) = unb64(&claimed["key_packages"], &id) else { continue };
                        self.v(move |v| {
                            if v.device.add_to_session(&group, &device, &kp, now_ms()).is_ok() {
                                let _ = v.device.send_handover(&group, &device);
                            }
                        })
                        .await;
                    }
                }
                waited = false;
                self.v(|v| v.commit().expect("stored")).await;
                self.post_all().await.expect("the step is taken");
            }
            if waited {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
        panic!("the invite never finished");
    }

    /// A second human device of this room, let in by link (12.1): this device makes the invite and confirms the
    /// code; the newcomer joins the room group from its Welcome. The handover the invite owes is NOT sent here
    /// (`follow_invites` sends it).
    pub async fn add_human(&mut self) -> Human {
        let mut invite = self.invite_as(Role::Human, None).await;
        let url = self.address.as_str().to_string();
        let link = invite.link.clone();
        let room = self.room;
        let joining = async move {
            let dir = TempDir::new("human");
            let journal = Journal::open(dir.path()).expect("a journal");
            let vault = Arc::new(Keeper::spawn(move || Vault::create(journal)).expect("a device"));
            let open = Hub::new(&url, None, None).expect("a client");
            let for_link = link.clone();
            let joining = vault
                .call(move |v: &mut Vault| v.device.join_link(&for_link, now_ms()))
                .await
                .unwrap()
                .expect("the link reads");
            let invite_id = joining.invite_id.to_base64url();
            let offer = open
                .open_call(
                    reqwest::Method::GET,
                    &format!("/v1/invites/{invite_id}"),
                    None,
                )
                .await
                .expect("the Offer");
            let offer = trommi_core::invite::SignedOffer {
                offer: unb64(&offer, "offer").unwrap(),
                signature: unb64(&offer, "signature").unwrap(),
                mac: unb64(&offer, "mac").unwrap(),
            };
            let request = vault
                .call(move |v: &mut Vault| {
                    let request = v.device.join_request(&link, &offer, now_ms());
                    v.commit().expect("stored");
                    request
                })
                .await
                .unwrap()
                .expect("the Request");
            let signed = &request.signed_request;
            open.open_call(
                reqwest::Method::POST,
                &format!("/v1/invites/{invite_id}/request"),
                Some(&json!({
                    "request": b64(&signed.request), "mac": b64(&signed.mac),
                    "signature": b64(&signed.signature),
                })),
            )
            .await
            .expect("the Request is posted");
            let reveal = loop {
                if let Ok(reveal) = open
                    .open_call(
                        reqwest::Method::GET,
                        &format!("/v1/invites/{invite_id}/reveal"),
                        None,
                    )
                    .await
                {
                    break reveal;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            };
            let reveal = trommi_core::invite::SignedReveal {
                reveal: unb64(&reveal, "reveal").unwrap(),
                signature: unb64(&reveal, "signature").unwrap(),
            };
            vault
                .call(move |v: &mut Vault| {
                    v.device.join_reveal(&reveal).expect("the Reveal");
                    v.commit().expect("stored");
                })
                .await
                .unwrap();
            let hub = hub_for(&vault, &url, room).expect("a client");
            let welcome = loop {
                if let Ok(list) = hub.get("/v1/welcomes").await {
                    if let Some(row) = list.as_array().and_then(|rows| rows.first()) {
                        break unb64(row, "welcome").expect("its bytes");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            };
            let id = vault
                .call(move |v: &mut Vault| {
                    v.device
                        .join_invited(&welcome, now_ms())
                        .expect("the room group is joined");
                    v.commit().expect("stored");
                    v.me()
                })
                .await
                .unwrap();
            Human {
                vault,
                hub,
                id,
                room,
                address: HubAddress::parse(&url).expect("the hub's address"),
                _dir: dir,
                objects: BTreeMap::new(),
                requests: BTreeSet::new(),
                findings: Vec::new(),
                trail: Vec::new(),
                items: Vec::new(),
                card_chats: Vec::new(),
                also_add: Vec::new(),
                founding_account: None,
            }
        };
        let (newcomer, _) = tokio::join!(joining, self.admit(&mut invite));
        newcomer
    }

    /// Learns the past of `group` from the hub (4.6), as the apps do for a group joined by link.
    pub async fn learn(&mut self, group: &GroupId) {
        let of = *group;
        let founding = self
            .hub
            .get(&format!("/v1/groups/{}/info?epoch=0", b64(of.as_bytes())))
            .await
            .expect("the founding GroupInfo");
        let founding = unb64(&founding, "group_info").expect("its bytes");
        let log = self
            .hub
            .get(&format!(
                "/v1/groups/{}/log?after=0&limit=1000&kind=commit",
                b64(of.as_bytes())
            ))
            .await
            .expect("the Commits");
        let commits: Vec<(Vec<u8>, u64)> = log["items"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|item| item["kind"] == "commit")
            .map(|item| {
                (
                    unb64(item, "bytes").unwrap(),
                    item["change"].as_u64().unwrap_or(0),
                )
            })
            .collect();
        self.v(move |v| {
            let served: Vec<trommi_core::recovery::ServedCommit<'_>> = commits
                .iter()
                .map(|(bytes, change)| trommi_core::recovery::ServedCommit {
                    change: *change,
                    commit: bytes,
                    recovery_auth: None,
                })
                .collect();
            v.device
                .learn_history(&of, &founding, &served)
                .expect("the past is learned");
            v.commit().expect("stored");
        })
        .await;
    }

    /// Reads every envelope the hub serves again, out of order, for its body (9.0.6), as the apps do once keys
    /// came. Returns how many did not open for lack of a key.
    pub async fn read_again(&mut self) -> usize {
        let answer = self
            .hub
            .get("/v1/changes?after=0&limit=2000")
            .await
            .expect("changes");
        let mut no_key = 0;
        let items = answer["items"].as_array().cloned().unwrap_or_default();
        // In the hub's order first (an envelope taken late), then each again for its body.
        for ordered in [true, false] {
            for item in &items {
                if item["kind"] != "envelope" {
                    continue;
                }
                let change = item["change"].as_u64().unwrap_or(0);
                let bytes = unb64(item, "envelope").expect("envelope");
                let got = self
                    .v(move |v| {
                        let got =
                            v.device
                                .receive_envelope(&bytes, change, ordered, None, now_ms());
                        v.commit().expect("stored");
                        got
                    })
                    .await;
                if !ordered && got.is_ok_and(|got| got.code == Some(Error::NoKey)) {
                    no_key += 1;
                }
            }
        }
        no_key
    }

    /// A register's current value in `group`, as this device reads it.
    pub async fn register(&self, group: &GroupId, name: &str) -> Option<Value> {
        let (group, name) = (*group, name.to_string());
        self.v(move |v| v.device.register(&group, &name).expect("the register"))
            .await
            .map(|value| serde_json::from_slice(value.expose()).expect("JSON"))
    }

    /// Posts the key requests (7.1.1) the device says are due at `at`; returns the groups named.
    pub async fn ask_for_keys(&mut self, at: u64) -> Vec<GroupId> {
        let due = self
            .v(move |v| {
                let due = v.device.key_requests(at).expect("the key requests");
                v.commit().expect("stored");
                due
            })
            .await;
        for request in &due {
            self.hub
                .post(
                    "/v1/requests",
                    &json!({ "kind": "handover", "group": b64(request.group.as_bytes()) }),
                )
                .await
                .expect("the hub takes the request");
        }
        due.into_iter().map(|request| request.group).collect()
    }

    /// Answers every key request of the hub's list (7.1.1) and posts the handovers. Returns how many messages.
    pub async fn answer_key_requests(&mut self) -> usize {
        let rows = self.hub.get("/v1/requests").await.expect("the requests");
        let mut sent = 0;
        for row in rows.as_array().cloned().unwrap_or_default() {
            if row["kind"] != "handover" {
                continue;
            }
            let device = DeviceId::from_slice(&unb64(&row, "device").unwrap()).expect("a device");
            let group = row["group_id"]
                .as_str()
                .map(|_| GroupId::from_bytes(&unb64(&row, "group_id").unwrap()).expect("a group"));
            let ids = self
                .v(move |v| {
                    let ids = v
                        .device
                        .answer_key_request(group.as_ref(), &device, now_ms());
                    v.commit().expect("stored");
                    ids
                })
                .await;
            sent += ids.map_or(0, |ids| ids.len());
        }
        self.post_all().await.expect("the handover is taken");
        sent
    }

    /// Founds the main session of an agent device that was admitted without one. Returns its group.
    pub async fn found_session(&mut self, agent: &Admitted) -> GroupId {
        self.follow_invites().await;
        self.groups()
            .await
            .into_iter()
            .find(|summary| {
                summary.session.is_some_and(|s| s.parent.is_zero())
                    && summary.leaves.contains(&agent.device)
            })
            .expect("the agent device's main session")
            .group
    }

    /// Takes a main session over for the device that was admitted for it (5.3): the old agent device left
    /// `agents` with the enrolment; its leaf is replaced, and the history is handed over.
    pub async fn take_over(&mut self, _group: &GroupId, _old: DeviceId, _new: &Admitted) {
        self.follow_invites().await;
    }

    /// Follows the hub's order up to its end.
    pub async fn sync(&mut self) {
        loop {
            let cursor = self.v(|v| v.device.cursor()).await;
            // a hub that restarts (a deploy) answers 502 for a moment: asked again, for up to a minute
            let mut tries = 0;
            let answer = loop {
                match self
                    .hub
                    .get(&format!("/v1/changes?after={cursor}&limit=500"))
                    .await
                {
                    Ok(answer) => break answer,
                    Err(fault) if tries < 30 && fault.status.is_none_or(|s| s >= 500) => {
                        tries += 1;
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    }
                    Err(fault) => panic!("changes: {fault:?}"),
                }
            };
            let items = answer["items"].as_array().cloned().unwrap_or_default();
            for item in &items {
                let change = item["change"].as_u64().unwrap_or(0);
                match item["kind"].as_str() {
                    Some("envelope") => self.take_envelope(item, change).await,
                    Some(kind) => self.take_log(item, change, kind == "commit").await,
                    None => {}
                }
            }
            let moved = self.v(|v| v.device.cursor()).await != cursor;
            if answer["more"] != json!(true) || !moved {
                break;
            }
        }
        // What merging put into the outbox (nothing, for most entries).
        let _ = self.post_all().await;
    }

    async fn take_log(&mut self, item: &Value, change: u64, commit: bool) {
        let group = GroupId::from_bytes(&unb64(item, "group_id").expect("group")).expect("id");
        let bytes = unb64(item, "bytes").expect("bytes");
        // a device that joined by the recovery code (a fresh login) proves it with its Commit (spec 7.4)
        let recovery_auth = item
            .get("recovery_auth")
            .filter(|a| !a.is_null())
            .map(|_| unb64(item, "recovery_auth").expect("recovery_auth"));
        let hand = move |v: &mut Vault| {
            let kind = if commit {
                LogKind::Commit {
                    bytes: &bytes,
                    recovery_auth: recovery_auth.as_deref(),
                }
            } else {
                LogKind::Message { bytes: &bytes }
            };
            let processed = v.device.process_log_entry(
                &LogEntry {
                    change,
                    group,
                    kind,
                },
                now_ms(),
            );
            v.commit().expect("stored");
            (processed, v.device.group(&group).is_ok())
        };
        let (processed, held) = self.v(hand.clone()).await;
        match processed {
            Ok(Processed::Message(Received::WorkTrail {
                turn, number, step, ..
            })) => {
                let step = serde_json::from_slice(&step).unwrap_or(Value::Null);
                self.trail.push((hex(turn.as_bytes()), number, step));
            }
            Ok(_) => {}
            Err(error) => {
                if trommi_core::device::log_finding(&error)
                    != trommi_core::device::LogFinding::Duplicate
                {
                    self.findings.push(format!("log: {}", error.code()));
                }
            }
        }
        // A session an agent founded adds this device: its Welcome belongs at this Commit's place.
        if commit && !held && !group.is_room() {
            let welcomes = self.hub.get("/v1/welcomes").await.expect("welcomes");
            let wanted = b64(group.as_bytes());
            let Some(row) = welcomes
                .as_array()
                .into_iter()
                .flatten()
                .find(|row| row["group_id"] == wanted.as_str())
            else {
                return;
            };
            let welcome = unb64(row, "welcome").expect("welcome");
            let room = self.room;
            let joined = self
                .v(move |v| {
                    let expected = WelcomeExpectation {
                        room,
                        committer: None,
                    };
                    let joined = v.device.join_welcome(&welcome, &expected, now_ms());
                    v.commit().expect("stored");
                    joined.map(|joined| joined.offending.len())
                })
                .await;
            match joined {
                Ok(0) => {
                    // The Commit again: it gives the join its place.
                    let _ = self.v(hand).await;
                }
                Ok(offending) => self
                    .findings
                    .push(format!("welcome: {offending} offending")),
                Err(error) => self.findings.push(format!("welcome: {}", error.code())),
            }
        }
    }

    async fn take_envelope(&mut self, item: &Value, change: u64) {
        let bytes = unb64(item, "envelope").expect("envelope");
        let void_code = item["void_code"]
            .as_str()
            .map(|code| Error::from_code(code).unwrap_or(Error::BadFormat));
        let received = self
            .v(move |v| {
                let received =
                    v.device
                        .receive_envelope(&bytes, change, true, void_code.as_ref(), now_ms());
                v.commit().expect("stored");
                received
            })
            .await;
        let received = match received {
            Ok(received) => received,
            Err(error) => {
                self.findings.push(format!("envelope: {}", error.code()));
                return;
            }
        };
        let header = &received.header;
        let code = received.code.as_ref().map_or("", |code| code.code());
        match received.outcome {
            EnvelopeOutcome::Applied => {}
            EnvelopeOutcome::Refused if matches!(code, "replay" | "group-behind") => return,
            // a writer's older register value or Note version, pruned once a newer one came (v1.md 9.4.2): its
            // header chains, there is nothing to apply, as the apps take it
            EnvelopeOutcome::Chained if code == "pruned" => return,
            EnvelopeOutcome::Refused => {
                self.findings.push(format!("envelope: {code}"));
                return;
            }
            other => {
                self.findings.push(format!("{other:?}: {code}"));
                return;
            }
        }
        if let Subject::Version(fields) | Subject::Request(fields) = &header.subject {
            let id = hex(fields.object_id.as_bytes());
            if matches!(header.subject, Subject::Request(_)) {
                self.requests.insert(id.clone());
            }
            self.objects.insert(id, header.group);
        }
        if let Some(body) = &received.body {
            let session = header
                .group
                .session_id()
                .map(|sid| hex(sid.as_bytes()))
                .unwrap_or_default();
            let payload: Value = serde_json::from_slice(body.payload()).unwrap_or(Value::Null);
            if let Subject::Item(trommi_core::envelope::Timeline::CardChat(card)) = &header.subject
            {
                self.card_chats
                    .push((hex(card.as_bytes()), payload.clone()));
            }
            self.items.push((session, header.sender, payload));
        }
    }

    /// Seals an item, posts it, and follows the hub's order.
    pub async fn write(&mut self, draft: Draft) -> Result<()> {
        self.sync().await;
        self.v(move |v| -> Result<()> {
            v.device
                .seal(&draft, None, &[], now_ms())
                .map_err(fault_of)?;
            v.commit()
        })
        .await?;
        self.post_all().await
    }

    fn session_of(&self, object: &str) -> SessionId {
        self.objects
            .get(object)
            .and_then(GroupId::session_id)
            .expect("the object is known")
    }

    /// Writes a message into a session's Chat; the device addresses it to the session's agent device.
    pub async fn say(&mut self, group: &GroupId, body: Value) -> Result<()> {
        let session = group.session_id().expect("a session group");
        self.write(Draft::SessionChat {
            session,
            payload: payload(&body),
        })
        .await
    }

    /// Answers a card of the agent.
    pub async fn answer(
        &mut self,
        card: &str,
        body: Value,
        choices: &[&str],
        closes: bool,
    ) -> Result<()> {
        self.sync().await;
        self.write(Draft::Answer {
            session: self.session_of(card),
            object_id: object_id(card),
            choices: choices.iter().map(|choice| choice.to_string()).collect(),
            closes,
            payload: payload(&body),
        })
        .await
    }

    /// Takes the answer in force back.
    pub async fn take_back(&mut self, card: &str) -> Result<()> {
        self.sync().await;
        self.write(Draft::TakeBack {
            session: self.session_of(card),
            object_id: object_id(card),
            payload: payload(&json!({})),
        })
        .await
    }

    /// Allows or denies a permission request of the agent.
    pub async fn verdict(&mut self, request: &str, allow: bool) -> Result<()> {
        self.sync().await;
        self.write(Draft::Verdict {
            session: self.session_of(request),
            request_id: object_id(request),
            allow,
            payload: payload(&json!({})),
        })
        .await
    }

    /// Sets a register of a session group (the Desk's goals for its agent).
    pub async fn set_register(
        &mut self,
        group: &GroupId,
        name: &str,
        value: Option<Value>,
    ) -> Result<()> {
        self.write(Draft::Register {
            group: *group,
            name: name.to_string(),
            value: value.as_ref().map(payload),
        })
        .await
    }

    /// Every Commit of the room the hub serves a human device above `after`, as the items of `/v1/changes`.
    pub async fn commits_after(&self, mut after: u64) -> Vec<Value> {
        let mut out = Vec::new();
        loop {
            let answer = self
                .hub
                .get(&format!("/v1/changes?after={after}&limit=500"))
                .await
                .expect("changes");
            let items = answer["items"].as_array().cloned().unwrap_or_default();
            out.extend(items.into_iter().filter(|item| item["kind"] == "commit"));
            let next = answer["change"].as_u64().unwrap_or(after);
            if answer["more"] != json!(true) || next <= after {
                return out;
            }
            after = next;
        }
    }

    /// Whether the object is a permission request the stand-in saw.
    pub fn has_request(&self, object: &str) -> bool {
        self.requests.contains(object)
    }

    /// The objects the stand-in saw, by id as hex.
    pub fn object_ids(&self) -> Vec<String> {
        self.objects.keys().cloned().collect()
    }
}

pub fn object_id(hex_id: &str) -> ObjectId {
    ObjectId::from_slice(&trommi_connector::util::unhex(hex_id).expect("hex"))
        .expect("an object id")
}

/// Waits until `check` holds, at most a minute and a half (a busy machine is slow).
pub async fn eventually<F, Fut>(what: &str, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..1800 {
        if check().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("never happened: {what}");
}
