//! seed-account: makes an account on a hub and fills its room from a demo fixture, with real devices and the
//! real protocol. A tool for people, not a test, and not shipped: it is how the App Store's reviewers get a
//! filled account to sign in to.
//!
//! ```text
//! cargo run -p trommi-tests --bin seed-account -- \
//!     --hub https://hub.example --email someone@example.org --env-out <file> \
//!     [--fixture demo/data/fixture.json] [--pace-ms 150]
//! ```
//!
//! What it does, in this order:
//! - a human device (the stand-in of `tests/connector/common`) founds the room with an account in the same
//!   request, as the apps sign up: the e-mail, a password made here (20 random characters) and an Emergency Kit;
//!   both are written to `--env-out` (mode 0600) as `APPSTORE_DEMO_USER`, `APPSTORE_DEMO_PASSWORD` and
//!   `TROMMI_REVIEW_KIT`, and never printed;
//! - one connector device (the connector's own client, in process) per main session of the fixture, joined by
//!   link and confirmed by the human device; each writes its profile and opens its helper sessions as the
//!   connector does (the opener writes for its helpers);
//! - the content in the order of its times in the fixture: cards with their pictures, Chats (the agents' and the
//!   human's), work trails, Artifacts; the human device answers, reads and shreds where the fixture did, and the
//!   agents close, withdraw and merge;
//! - at the end the status lines, the Desks with their Goals and crowns, which session sits on which Desk, drafts,
//!   the corner note.
//!
//! Not seeded: the times (everything is written now, in the fixture's order), permission requests (they expire
//! within minutes), the Scribble Boards, the human devices the fixture lists beside the one that looks, and the
//! archived sessions with all they hold: a device that signs in later joins only the live session groups (8.4),
//! so on the reviewer's device an archived session would stand without its name and content.
//! The agents stay offline afterwards. The fixture is read as it stands, so a new demo is seeded by running this
//! again with another e-mail.
#[path = "../connector/common/mod.rs"]
mod common;

use common::Human;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use trommi_connector::client::Client;
use trommi_connector::hub::b64;
use trommi_connector::join::join_room;
use trommi_connector::store::Journal;
use trommi_connector::util::{random_hex, unhex};
use trommi_core::account::{self, Way};
use trommi_core::crypto::{SecretBytes, SystemEntropy};
use trommi_core::device::Draft;
use trommi_core::ids::{GroupId, ObjectId, SessionId};

type Res<T> = std::result::Result<T, String>;

struct Args {
    hub: String,
    email: String,
    fixture: PathBuf,
    env_out: PathBuf,
    pace: Duration,
    /// Extra cards after the fixture (a big account for the iOS mega run), each with a few chat lines.
    bulk: usize,
    /// After seeding: stay online in this folder (the human device follows the room and answers key requests, the
    /// agents stay live) and run the rounds named by files `<name>.cmd` there; `stop` ends it.
    serve: Option<PathBuf>,
}

fn args() -> Res<Args> {
    let mut map = HashMap::new();
    let mut it = std::env::args().skip(1);
    while let Some(key) = it.next() {
        let value = it.next().ok_or(format!("{key} needs a value"))?;
        map.insert(key.trim_start_matches("--").to_string(), value);
    }
    let take = |k: &str| map.get(k).cloned();
    Ok(Args {
        hub: take("hub").ok_or("--hub")?,
        email: take("email").ok_or("--email")?,
        fixture: take("fixture")
            .unwrap_or_else(|| {
                concat!(env!("CARGO_MANIFEST_DIR"), "/../demo/data/fixture.json").into()
            })
            .into(),
        env_out: take("env-out").ok_or("--env-out")?.into(),
        pace: Duration::from_millis(
            take("pace-ms")
                .map_or(Ok(150), |v| v.parse())
                .map_err(|e| format!("--pace-ms: {e}"))?,
        ),
        bulk: take("bulk").map_or(Ok(0), |v| v.parse()).map_err(|e| format!("--bulk: {e}"))?,
        serve: take("serve").map(PathBuf::from),
    })
}

/// A password a reviewer can type on a phone: four groups of five from 32 letters and digits (100 bits).
fn new_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789AB";
    let bytes = unhex(&random_hex(20)).expect("hex");
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| ALPHABET[(*b as usize) % 32] as char)
        .collect();
    chars
        .chunks(5)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// The env file, written before anything else can fail: mode 0600, never overwritten.
fn write_env(path: &Path, email: &str, password: &str, kit: &str) -> Res<()> {
    use std::os::unix::fs::OpenOptionsExt;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(
        file,
        "# The App Store review account (made by tests/tools/seed_account.rs). Secret: keep it out of git.\n\
         APPSTORE_DEMO_USER={email}\nAPPSTORE_DEMO_PASSWORD={password}\n\
         # The account's Emergency Kit words (Settings > Account, or \"forgot password\")\nTROMMI_REVIEW_KIT=\"{kit}\""
    )
    .map_err(|e| e.to_string())
}

fn wire(hex_id: &str) -> String {
    b64(&unhex(hex_id).unwrap_or_default())
}

fn with_schema(mut body: Map<String, Value>) -> SecretBytes {
    body.insert("schema_version".into(), json!(2));
    SecretBytes::new(Value::Object(body).to_string().into_bytes())
}

fn obj(v: &Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap_or_default()
}

/// One agent device: the connector's client of a main session.
struct Agent {
    client: Arc<Client>,
    _dir: common::TempDir,
}

/// Where a fixture device writes: its agent and, for a helper, the helper session it writes in.
#[derive(Clone)]
struct Seat {
    agent: usize,
    session: Option<String>,
    /// The session id as hex (the main one's for an agent).
    sid: String,
}

/// One step of the fixture in time order.
enum Step {
    Card(Value),
    Answer { card: String, answer: Value },
    Finish(Value),
    Item { key: String, item: Value },
    Artifact(Value),
}

struct Seeder {
    args: Args,
    fixture: Value,
    files: PathBuf,
    human: Human,
    agents: Vec<Agent>,
    seats: HashMap<String, Seat>,
    humans: std::collections::HashSet<String>,
    /// Fixture object id (hex) -> the real one (hex).
    ids: HashMap<String, String>,
    counts: BTreeMap<&'static str, usize>,
    /// Bulk cards still open: (card id, seat).
    bulk_open: Vec<(String, Seat)>,
    bulk_made: usize,
}

impl Seeder {
    async fn pace(&self) {
        tokio::time::sleep(self.args.pace).await;
    }

    fn count(&mut self, what: &'static str) {
        *self.counts.entry(what).or_default() += 1;
    }

    fn real(&self, fixture_id: &str) -> Option<String> {
        self.ids.get(fixture_id).cloned()
    }

    /// Uploads the files a list of fixture attachments names, by the device of `seat`; returns the references.
    async fn upload(&self, seat: &Seat, list: &Value) -> Res<Value> {
        let mut out = Vec::new();
        for a in list.as_array().into_iter().flatten() {
            let url = a["url"].as_str().unwrap_or("");
            let name = url.rsplit('/').next().unwrap_or("");
            let bytes = std::fs::read(self.files.join(name)).map_err(|e| format!("{name}: {e}"))?;
            let mut meta = Map::new();
            for key in ["file_name", "media_type", "width", "height", "caption"] {
                if let Some(v) = a.get(key).filter(|v| !v.is_null()) {
                    meta.insert(key.into(), v.clone());
                }
            }
            let reference = self.agents[seat.agent]
                .client
                .upload_attachment(bytes, meta)
                .await
                .map_err(|e| format!("upload {name}: {e}"))?;
            out.push(Value::Object(reference));
            self.pace().await;
        }
        Ok(Value::Array(out))
    }

    fn seat_of(&self, device: &str) -> Res<Seat> {
        self.seats
            .get(device)
            .cloned()
            .ok_or(format!("no seat for device {device}"))
    }

    async fn join_agent(&mut self) -> Res<usize> {
        let dir = common::TempDir::new("seed-agent");
        let journal = Journal::open(dir.path()).map_err(|e| e.to_string())?;
        let mut invite = self.human.invite(None).await;
        let link = invite.link.clone();
        let for_join = journal.clone();
        let joining =
            tokio::spawn(async move { join_room(&link, for_join, |_| {}, 400, 120_000).await });
        let admitted = self.human.admit(&mut invite).await;
        joining
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("join: {e}"))?;
        let (client, _events) = Client::open(journal).await.map_err(|e| e.to_string())?;
        client.start().await.map_err(|e| e.to_string())?;
        self.human.found_session(&admitted).await;
        let c = client.clone();
        common::eventually("the agent is in its session", || async {
            c.core.lock().await.session_id().is_some()
        })
        .await;
        self.agents.push(Agent { client, _dir: dir });
        Ok(self.agents.len() - 1)
    }

    /// The agents, their main sessions and the helper sessions, with their profiles and device names.
    async fn sessions(&mut self) -> Res<()> {
        let sessions = self.fixture["sessions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let members = self.fixture["members"].clone();
        let device_name = |d: &str| -> String {
            members
                .as_array()
                .into_iter()
                .flatten()
                .find(|m| m["device_id"] == d)
                .and_then(|m| m["device_name"].as_str())
                .unwrap_or("agent")
                .to_string()
        };
        let mut main_of: HashMap<String, usize> = HashMap::new();
        for s in sessions.iter().filter(|s| s["profile"]["is_main"] == true) {
            let name = device_name(s["agent_device_id"].as_str().unwrap_or(""));
            let agent = self.join_agent().await?;
            let client = self.agents[agent].client.clone();
            let sid = client.session_id_now().await;
            client
                .set_status(obj(&json!({ "profile": profile_of(&s["profile"]) })), None)
                .await
                .map_err(|e| e.to_string())?;
            client
                .ensure_device_register(&json!({ "device_name": name, "platform": "macos" }))
                .await
                .map_err(|e| e.to_string())?;
            let device = s["agent_device_id"].as_str().unwrap_or("").to_string();
            self.seats.insert(
                device,
                Seat {
                    agent,
                    session: None,
                    sid,
                },
            );
            main_of.insert(
                s["agent_session_id"].as_str().unwrap_or("").to_string(),
                agent,
            );
            self.count("main sessions");
            println!("main session {}", s["agent_session_id"]);
            self.pace().await;
        }
        for s in sessions.iter().filter(|s| s["profile"]["is_main"] != true) {
            let parent = s["profile"]["parent_session"].as_str().unwrap_or("");
            let agent = *main_of
                .get(parent)
                .ok_or(format!("no main session {parent}"))?;
            let sid = self.agents[agent]
                .client
                .open_child_session(obj(&profile_of(&s["profile"])))
                .await
                .map_err(|e| format!("helper {}: {e}", s["agent_session_id"]))?;
            let device = s["agent_device_id"].as_str().unwrap_or("").to_string();
            self.seats.insert(
                device,
                Seat {
                    agent,
                    session: Some(sid.clone()),
                    sid,
                },
            );
            self.count("helper sessions");
            self.pace().await;
        }
        Ok(())
    }

    /// Every step of the fixture with a time, in time order.
    fn steps(&self) -> Vec<(u64, usize, Step)> {
        let mut out = Vec::new();
        let mut n = 0;
        let mut push = |t: u64, s: Step| {
            n += 1;
            out.push((t, n, s));
        };
        for card in self.fixture["cards"].as_array().into_iter().flatten() {
            let created = card["created_at"].as_u64().unwrap_or(0);
            push(created, Step::Card(card.clone()));
            let id = card["object_id"].as_str().unwrap_or("").to_string();
            let mut last = created;
            for a in card["answers"].as_array().into_iter().flatten() {
                let at = a["answered_at"]
                    .as_u64()
                    .unwrap_or(created + 1)
                    .max(created + 1);
                last = last.max(at);
                push(
                    at,
                    Step::Answer {
                        card: id.clone(),
                        answer: a.clone(),
                    },
                );
            }
            let content = &card["versions"]
                .as_array()
                .and_then(|v| v.last())
                .map(|v| v["content"].clone())
                .unwrap_or_default();
            if content["close_summary"].is_string() || content["withdraw_reason"].is_string() {
                let at = card["updated_at"]
                    .as_u64()
                    .unwrap_or(last + 1)
                    .max(last + 1);
                push(at, Step::Finish(card.clone()));
            }
        }
        for (key, items) in self.fixture["timelines"].as_object().into_iter().flatten() {
            if !key.starts_with("chat:") {
                continue;
            }
            for item in items.as_array().into_iter().flatten() {
                let at = item["sent_at"].as_u64().unwrap_or(0);
                push(
                    at,
                    Step::Item {
                        key: key.clone(),
                        item: item.clone(),
                    },
                );
            }
        }
        for p in self.fixture["published"].as_array().into_iter().flatten() {
            push(
                p["sent_at"].as_u64().unwrap_or(0),
                Step::Artifact(p.clone()),
            );
        }
        out.sort_by_key(|(t, n, _)| (*t, *n));
        out
    }

    async fn card(&mut self, card: &Value) -> Res<()> {
        let seat = self.seat_of(card["agent_device_id"].as_str().unwrap_or(""))?;
        let first = card["versions"][0]["content"].clone();
        let mut fields = obj(&first);
        for key in [
            "close_summary",
            "withdraw_reason",
            "merged_into_object_id",
            "merged_from_object_ids",
        ] {
            fields.remove(key);
        }
        fields.retain(|_, v| !v.is_null());
        fields.insert(
            "attachments".into(),
            self.upload(&seat, &first["attachments"]).await?,
        );
        fields.insert("urgency".into(), card["versions"][0]["urgency"].clone());
        let client = self.agents[seat.agent].client.clone();
        let merged: Vec<String> = first["merged_from_object_ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str().and_then(|id| self.real(id)))
            .collect();
        let id = if merged.is_empty() {
            client.send_card(fields, seat.session.clone()).await
        } else {
            fields.remove("urgency");
            client.merge(&merged, fields, seat.session.clone()).await
        }
        .map_err(|e| format!("card {}: {e}", card["title"]))?;
        self.ids.insert(
            card["object_id"].as_str().unwrap_or("").to_string(),
            id.clone(),
        );
        // Later versions: what the agent reworked.
        for version in card["versions"].as_array().into_iter().flatten().skip(1) {
            let mut changes = obj(&version["content"]);
            for key in [
                "close_summary",
                "withdraw_reason",
                "merged_into_object_id",
                "merged_from_object_ids",
                "attachments",
            ] {
                changes.remove(key);
            }
            changes.retain(|_, v| !v.is_null());
            client
                .revise(&id, changes, version["urgency"].as_str().map(str::to_owned))
                .await
                .map_err(|e| format!("revise {}: {e}", card["title"]))?;
        }
        self.count("cards");
        Ok(())
    }

    async fn answer(&mut self, card: &str, answer: &Value) -> Res<()> {
        let id = self
            .real(card)
            .ok_or(format!("answer to an unknown card {card}"))?;
        let action = answer["answer_action"].as_str().unwrap_or("answer");
        let choices: Vec<String> = answer["choices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| c.as_str().map(str::to_owned))
            .collect();
        let fixture_card = self.fixture["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["object_id"] == card)
            .cloned()
            .unwrap_or_default();
        let options = fixture_card["options"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let note = answer["note"].as_str().unwrap_or("");
        let all_final = !choices.is_empty()
            && choices.iter().all(|c| {
                options
                    .iter()
                    .any(|o| o["key"] == c.as_str() && o["final"] == true)
            });
        let closes = matches!(action, "read" | "shred") || (all_final && note.is_empty());
        let mut body = Map::new();
        body.insert("answer_action".into(), json!(action));
        body.insert("choices".into(), json!(choices));
        for key in ["note", "option_notes", "marks", "trusted"] {
            if let Some(v) = answer.get(key).filter(|v| !v.is_null()) {
                body.insert(key.into(), v.clone());
            }
        }
        body.insert("attachments".into(), json!([]));
        let refs: Vec<&str> = choices.iter().map(String::as_str).collect();
        self.human
            .answer(&id, Value::Object(with_schema_map(body)), &refs, closes)
            .await
            .map_err(|e| format!("answer {card}: {e}"))?;
        // The agent has it before it closes the card.
        let seat = self.seat_of(fixture_card["agent_device_id"].as_str().unwrap_or(""))?;
        let client = self.agents[seat.agent].client.clone();
        let want = id.clone();
        common::eventually("the agent has the answer", || async {
            client
                .core
                .lock()
                .await
                .model
                .cards
                .get(&want)
                .is_some_and(|c| !c.answers.is_empty())
        })
        .await;
        self.count("answers");
        Ok(())
    }

    async fn finish(&mut self, card: &Value) -> Res<()> {
        let id = self
            .real(card["object_id"].as_str().unwrap_or(""))
            .ok_or("finish of an unknown card")?;
        let seat = self.seat_of(card["agent_device_id"].as_str().unwrap_or(""))?;
        let client = self.agents[seat.agent].client.clone();
        let content = card["versions"]
            .as_array()
            .and_then(|v| v.last())
            .map(|v| v["content"].clone())
            .unwrap_or_default();
        if let Some(summary) = content["close_summary"].as_str() {
            client
                .close(&id, summary)
                .await
                .map_err(|e| format!("close: {e}"))?;
        } else if let Some(reason) = content["withdraw_reason"].as_str() {
            client
                .withdraw(&id, reason)
                .await
                .map_err(|e| format!("withdraw: {e}"))?;
        }
        Ok(())
    }

    async fn item(&mut self, key: &str, item: &Value) -> Res<()> {
        let sender = item["sender_device_id"].as_str().unwrap_or("");
        let content = &item["content"];
        let (scope, target) = key
            .trim_start_matches("chat:")
            .split_once('/')
            .unwrap_or(("", ""));
        if self.humans.contains(sender) {
            let mut body = obj(content);
            body.insert("content_type".into(), json!("message"));
            body.retain(|_, v| !v.is_null());
            if body
                .get("attachments")
                .is_some_and(|a| a.as_array().is_some_and(|a| !a.is_empty()))
            {
                body.insert("attachments".into(), json!([]));
            }
            if let Some(note) = body
                .get("note")
                .and_then(|n| n["object_id"].as_str())
                .map(str::to_owned)
            {
                match self.real(&note) {
                    Some(real) => {
                        body.insert("note".into(), json!({ "object_id": wire(&real) }));
                    }
                    None => {
                        body.remove("note");
                    }
                }
            }
            let payload = with_schema(body);
            let draft = if scope == "card" {
                let card = self
                    .real(target)
                    .ok_or(format!("a message on an unknown card {target}"))?;
                let session = self.session_of_card(target)?;
                Draft::CardChat {
                    session,
                    card: object_id(&card),
                    payload,
                }
            } else {
                let seat = self.seat_of(target)?;
                Draft::SessionChat {
                    session: session_id(&seat.sid),
                    payload,
                }
            };
            self.human
                .write(draft)
                .await
                .map_err(|e| format!("human message: {e}"))?;
            self.count("human messages");
            return Ok(());
        }
        let seat = self.seat_of(sender)?;
        let client = self.agents[seat.agent].client.clone();
        if content["terminal"] == "work" {
            let turn: [u8; 16] = unhex(&random_hex(16))
                .and_then(|b| b.try_into().ok())
                .ok_or("turn")?;
            let mut number = 0;
            for step in content["work"]["items"].as_array().into_iter().flatten() {
                number += 1;
                let text = step["text"]
                    .as_str()
                    .or(step["title"].as_str())
                    .or(step["subject"].as_str())
                    .unwrap_or("");
                let mut s = json!({ "text": text });
                if let Some(tool) = step["tool"].as_str() {
                    s["tool"] = json!(tool);
                }
                client
                    .work_step(seat.session.as_deref(), turn, number, &s)
                    .await
                    .map_err(|e| format!("work: {e}"))?;
            }
            self.count("work trails");
            return Ok(());
        }
        let mut fields = obj(content);
        fields.retain(|_, v| !v.is_null());
        let published = fields.remove("published_object_id");
        let has_files = fields
            .get("attachments")
            .is_some_and(|a| a.as_array().is_some_and(|a| !a.is_empty()));
        if has_files {
            fields.insert(
                "attachments".into(),
                self.upload(&seat, &content["attachments"]).await?,
            );
        }
        if let Some(p) = published.as_ref().and_then(Value::as_str) {
            if let Some(real) = self.real(p) {
                fields.insert("artifact_object_id".into(), json!(wire(&real)));
            }
        }
        let card = if scope == "card" {
            Some(
                self.real(target)
                    .ok_or(format!("a message on an unknown card {target}"))?,
            )
        } else {
            None
        };
        client
            .send_message(
                fields,
                card,
                if scope == "card" {
                    None
                } else {
                    seat.session.clone()
                },
            )
            .await
            .map_err(|e| format!("message: {e}"))?;
        self.count("agent messages");
        Ok(())
    }

    async fn artifact(&mut self, p: &Value) -> Res<()> {
        let seat = self.seat_of(p["agent_device_id"].as_str().unwrap_or(""))?;
        let attachments = self.upload(&seat, &p["attachments"]).await?;
        let id = self.agents[seat.agent]
            .client
            .publish(
                attachments,
                p["title"].clone(),
                p["note"].as_str().map(|n| json!(n)),
                seat.session.clone(),
            )
            .await
            .map_err(|e| format!("publish {}: {e}", p["title"]))?;
        self.ids
            .insert(p["object_id"].as_str().unwrap_or("").to_string(), id);
        self.count("artifacts");
        Ok(())
    }

    /// One bulk card of a session, with `chat` lines of the agent on it; answered and closed unless `keep_open`.
    async fn bulk_card(&mut self, seat: &Seat, keep_open: bool, chat: usize) -> Res<()> {
        self.bulk_made += 1;
        let n = self.bulk_made;
        let client = self.agents[seat.agent].client.clone();
        let fields = obj(&json!({
            "card_type": "decision",
            "title": format!("Bulk question {n}: ship step {n}?"),
            "teaser": format!("A made-up question of the mega run, number {n}."),
            "body": format!("Question {n} of the big account. Nothing depends on it."),
            "options": [{ "key": "yes", "label": "Yes", "short": "Yes" }, { "key": "no", "label": "No", "short": "No", "final": true }],
            "allows_multiple": false,
            "recommended": "yes",
            "attachments": [],
            "urgency": if n.is_multiple_of(7) { "high" } else { "normal" },
        }));
        let id = client
            .send_card(fields, seat.session.clone())
            .await
            .map_err(|e| format!("bulk card {n}: {e}"))?;
        for line in 0..chat {
            client
                .send_message(obj(&json!({ "text": format!("Working on step {n}, part {line}.") })), Some(id.clone()), None)
                .await
                .map_err(|e| format!("bulk chat: {e}"))?;
        }
        self.count("bulk cards");
        if keep_open {
            self.bulk_open.push((id, seat.clone()));
            return Ok(());
        }
        self.settle_bulk(&id, seat).await
    }

    /// The human device answers a bulk card "yes", the agent closes it.
    async fn settle_bulk(&mut self, id: &str, seat: &Seat) -> Res<()> {
        // (a card the human device has not taken: the agent closes it alone, as it closes one answered elsewhere)
        self.human.sync().await;
        if !self.human.object_ids().iter().any(|o| o == id) {
            let client = self.agents[seat.agent].client.clone();
            client.close(id, "Done elsewhere.").await.map_err(|e| format!("bulk close: {e}"))?;
            self.count("bulk closed by the agent alone");
            return Ok(());
        }
        let body = with_schema_map(obj(&json!({ "answer_action": "answer", "choices": ["yes"], "attachments": [] })));
        // (an epoch that ended under the answer: followed, and answered once more)
        if let Err(first) = self.human.answer(id, Value::Object(body.clone()), &["yes"], false).await {
            self.human.sync().await;
            self.human
                .answer(id, Value::Object(body), &["yes"], false)
                .await
                .map_err(|e| format!("bulk answer: {first}; again: {e}"))?;
        }
        let client = self.agents[seat.agent].client.clone();
        let want = id.to_string();
        common::eventually("the agent has the bulk answer", || async {
            client.core.lock().await.model.cards.get(&want).is_some_and(|c| !c.answers.is_empty())
        })
        .await;
        client.close(id, "Done.").await.map_err(|e| format!("bulk close: {e}"))?;
        self.count("bulk settled");
        Ok(())
    }

    fn bulk_seats(&self) -> Vec<Seat> {
        let mut seats: Vec<Seat> = self.seats.values().cloned().collect();
        seats.sort_by(|a, b| a.sid.cmp(&b.sid));
        seats
    }

    /// `n` bulk cards spread over every session: one in five stays open, the rest is answered and closed.
    async fn bulk(&mut self, n: usize) -> Res<()> {
        let seats = self.bulk_seats();
        for i in 0..n {
            let seat = seats[i % seats.len()].clone();
            self.bulk_card(&seat, i % 5 == 0, 2).await?;
            if i % 50 == 49 {
                println!("bulk: {} cards", i + 1);
            }
        }
        Ok(())
    }

    /// A round written while the phone is away: `settle` open cards answered and closed, `new` new cards (half of
    /// them left open), `chat` chat lines each.
    async fn away_round(&mut self, settle: usize, new: usize, chat: usize) -> Res<()> {
        for _ in 0..settle.min(self.bulk_open.len()) {
            let (id, seat) = self.bulk_open.remove(0);
            self.settle_bulk(&id, &seat).await?;
        }
        let seats = self.bulk_seats();
        for i in 0..new {
            let seat = seats[(self.bulk_made + i) % seats.len()].clone();
            self.bulk_card(&seat, i % 2 == 0, chat).await?;
        }
        Ok(())
    }

    /// Stays online (see `Args::serve`). A file `<name>.cmd` holding "settle new chat" runs one away round and is
    /// answered by `<name>.done` with the counts; `stop` ends it.
    async fn serve(&mut self, dir: &Path) -> Res<()> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("ready"), b"ready").map_err(|e| e.to_string())?;
        println!("serving in {}", dir.display());
        loop {
            if dir.join("stop").exists() {
                return Ok(());
            }
            let mut cmds: Vec<PathBuf> = std::fs::read_dir(dir)
                .map_err(|e| e.to_string())?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "cmd") && !p.with_extension("done").exists())
                .collect();
            cmds.sort();
            for cmd in cmds {
                let text = std::fs::read_to_string(&cmd).unwrap_or_default();
                let n: Vec<usize> = text.split_whitespace().filter_map(|w| w.parse().ok()).collect();
                let (settle, new, chat) = (n.first().copied().unwrap_or(0), n.get(1).copied().unwrap_or(0), n.get(2).copied().unwrap_or(0));
                let started = std::time::Instant::now();
                self.away_round(settle, new, chat).await?;
                for agent in &self.agents {
                    agent.client.settle(120_000).await.map_err(|e| format!("settle: {e}"))?;
                }
                self.human.sync().await;
                let open: Vec<&str> = self.bulk_open.iter().map(|(id, _)| id.as_str()).collect();
                std::fs::write(
                    cmd.with_extension("done"),
                    json!({ "settled": settle, "new": new, "chat": chat, "ms": started.elapsed().as_millis() as u64, "bulk_open": open, "findings": self.human.findings, "counts": format!("{:?}", self.counts) }).to_string(),
                )
                .map_err(|e| e.to_string())?;
                println!("round {} done", cmd.display());
            }
            self.human.sync().await;
            self.human.answer_key_requests().await;
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    fn session_of_card(&self, fixture_card: &str) -> Res<SessionId> {
        let card = self.fixture["cards"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|c| c["object_id"] == fixture_card)
            .ok_or("no such card")?;
        let seat = self.seat_of(card["agent_device_id"].as_str().unwrap_or(""))?;
        Ok(session_id(&seat.sid))
    }

    /// Status lines, Desks, crowns, session settings, drafts, the note, archived sessions.
    async fn finishing(&mut self) -> Res<()> {
        let made = self.fixture["made_at"].as_u64().unwrap_or(0);
        let at = |t: u64| -> u64 {
            trommi_connector::util::now_ms().saturating_sub(made.saturating_sub(t))
        };
        for s in self.fixture["sessions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let seat = self.seat_of(s["agent_device_id"].as_str().unwrap_or(""))?;
            let mut values = Map::new();
            for line in s["status_lines"].as_array().into_iter().flatten() {
                let object = line["object_id"]
                    .as_str()
                    .and_then(|o| self.real(o))
                    .map(|o| json!(wire(&o)));
                values.insert(
                    format!("status_line/{}", line["id"].as_str().unwrap_or("x")),
                    json!({ "label": line["label"], "state": line["state"], "detail": line["detail"], "object_id": object.unwrap_or(Value::Null) }),
                );
            }
            if !values.is_empty() {
                self.agents[seat.agent]
                    .client
                    .set_status(values, seat.session.clone())
                    .await
                    .map_err(|e| format!("status: {e}"))?;
                self.count("status lines");
            }
            self.pace().await;
        }
        let room = GroupId::room(self.human.room);
        let human = &self.fixture["human"];
        let crown_of = |c: &Value| -> Value {
            c["agent_device_id"]
                .as_str()
                .and_then(|d| self.seats.get(d))
                .map(|seat| json!({ "agent_device_id": self.agents[seat.agent].client.device_id().to_base64url() }))
                .unwrap_or(Value::Null)
        };
        let mut registers: Vec<(GroupId, String, Value)> = Vec::new();
        registers.push((
            room,
            format!("device/{}", self.human.id().to_base64url()),
            json!({ "device_name": "Laptop", "platform": "web" }),
        ));
        for (desk, value) in human["desks"].as_object().into_iter().flatten() {
            let mut v = obj(value);
            if let Some(c) = value.get("crown") {
                v.insert("crown".into(), crown_of(c));
            }
            v.insert(
                "created_at".into(),
                json!(at(value["created_at"].as_u64().unwrap_or(0))),
            );
            registers.push((room, format!("desk/{desk}"), Value::Object(v)));
        }
        if human["crown"].is_object() {
            registers.push((room, "crown".into(), crown_of(&human["crown"])));
        }
        for (position, s) in self.fixture["sessions"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let seat = self.seat_of(s["agent_device_id"].as_str().unwrap_or(""))?;
            let settings = &s["settings"];
            let name = format!("session/{}", wire(&seat.sid));
            let mut v = obj(settings);
            v.insert("archived".into(), json!(false));
            // the sidebar's order: the fixture's (a seeded room has no activity times to order by)
            v.entry("position").or_insert(json!(position));
            registers.push((room, name, Value::Object(v)));
            if seat.session.is_none() {
                let desk = settings["desk"].as_str().unwrap_or("main");
                let d = &human["desks"][desk];
                let goals = if d["goals"].is_string() {
                    json!({ "desk_id": desk, "desk_name": d["name"], "goals": d["goals"] })
                } else {
                    Value::Null
                };
                registers.push((
                    GroupId::session(self.human.room, session_id(&seat.sid)),
                    "goals".into(),
                    goals,
                ));
            }
        }
        for (object, draft) in human["drafts"].as_object().into_iter().flatten() {
            if let Some(real) = self.real(object) {
                registers.push((room, format!("draft/{}", wire(&real)), draft.clone()));
            }
        }
        for (group, name, value) in registers {
            let value = (!value.is_null()).then_some(value);
            self.human
                .set_register(&group, &name, value)
                .await
                .map_err(|e| format!("register {name}: {e}"))?;
            self.count("registers");
            self.pace().await;
        }
        for note in self.fixture["notes"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let body = obj(&json!({
                "text": note["text"], "object_version": 1, "previous_version_hash": b64(&[0u8; 32]),
            }));
            self.human
                .write(Draft::NoteFirst {
                    payload: with_schema(body),
                })
                .await
                .map_err(|e| format!("note: {e}"))?;
            self.count("notes");
        }
        Ok(())
    }
}

/// The fixture without its archived sessions (and their helpers' sessions): their cards, Chats and Artifacts go
/// with them. Returns how many sessions were left out.
fn without_archived(fixture: &mut Value) -> usize {
    let sessions = fixture["sessions"].as_array().cloned().unwrap_or_default();
    let archived: Vec<&Value> = sessions
        .iter()
        .filter(|s| s["settings"]["archived"] == true)
        .collect();
    let names: std::collections::HashSet<&str> = archived
        .iter()
        .filter_map(|s| s["agent_session_id"].as_str())
        .collect();
    let gone: std::collections::HashSet<String> = sessions
        .iter()
        .filter(|s| {
            s["settings"]["archived"] == true
                || s["profile"]["parent_session"]
                    .as_str()
                    .is_some_and(|p| names.contains(p))
        })
        .filter_map(|s| s["agent_device_id"].as_str().map(str::to_owned))
        .collect();
    let by = |v: &Value, key: &str| gone.contains(v[key].as_str().unwrap_or(""));
    let cards: std::collections::HashSet<String> = fixture["cards"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|c| by(c, "agent_device_id"))
        .filter_map(|c| c["object_id"].as_str().map(str::to_owned))
        .collect();
    for (list, key) in [
        ("sessions", "agent_device_id"),
        ("cards", "agent_device_id"),
        ("published", "agent_device_id"),
    ] {
        if let Some(items) = fixture[list].as_array_mut() {
            items.retain(|v| !by(v, key));
        }
    }
    if let Some(timelines) = fixture["timelines"].as_object_mut() {
        timelines.retain(|key, _| {
            let target = key.rsplit('/').next().unwrap_or("");
            !gone.contains(target) && !cards.contains(target)
        });
    }
    gone.len()
}

fn with_schema_map(mut body: Map<String, Value>) -> Map<String, Value> {
    body.insert("schema_version".into(), json!(2));
    body
}

fn profile_of(p: &Value) -> Value {
    let mut out = Map::new();
    for key in ["model", "task", "icon", "agent_name"] {
        if let Some(v) = p.get(key).filter(|v| !v.is_null()) {
            out.insert(key.into(), v.clone());
        }
    }
    Value::Object(out)
}

fn object_id(hex_id: &str) -> ObjectId {
    ObjectId::from_slice(&unhex(hex_id).unwrap_or_default()).expect("an object id")
}

fn session_id(hex_id: &str) -> SessionId {
    SessionId::from_slice(&unhex(hex_id).unwrap_or_default()).expect("a session id")
}

trait SessionNow {
    async fn session_id_now(&self) -> String;
}
impl SessionNow for Arc<Client> {
    async fn session_id_now(&self) -> String {
        self.core.lock().await.session_id().unwrap_or_default()
    }
}

async fn run() -> Res<()> {
    let args = args()?;
    let fixture: Value = serde_json::from_slice(
        &std::fs::read(&args.fixture).map_err(|e| format!("{}: {e}", args.fixture.display()))?,
    )
    .map_err(|e| e.to_string())?;
    let mut fixture = fixture;
    let left_out = without_archived(&mut fixture);
    if left_out > 0 {
        println!("{left_out} session(s) of archived sessions left out");
    }
    let files = args
        .fixture
        .parent()
        .unwrap_or(Path::new("."))
        .join("files");
    if args.env_out.exists() {
        return Err(format!(
            "{} exists: the account it names is kept; give another --env-out",
            args.env_out.display()
        ));
    }
    let email = account::normalise_email(&args.email).map_err(|e| e.code().to_string())?;
    let password = new_password();
    account::check_password(&password).map_err(|e| e.code().to_string())?;
    let words =
        account::generate_kit_words(&mut SystemEntropy).map_err(|e| e.code().to_string())?;
    let words = String::from_utf8(words.expose().to_vec()).map_err(|e| e.to_string())?;
    write_env(&args.env_out, &email, &password, &words)?;
    println!(
        "credentials written to {} (mode 0600)",
        args.env_out.display()
    );

    let (e2, p2, w2) = (email.clone(), password.clone(), words.clone());
    let human = Human::found_with(&args.hub, move |room, code| {
        let pw = account::password_keys(&e2, &p2, None).expect("password keys");
        let kit = account::kit_keys(&e2, &w2).expect("kit keys");
        let seal = |key, way| b64(&account::seal_code(key, room, way, code, &mut SystemEntropy).expect("sealed"));
        let kdf: Value = serde_json::from_str(account::KDF_RECORD).expect("kdf");
        Some(json!({
            "email": e2,
            "kit": { "auth_key": b64(pw_kit(&kit.auth_key)), "sealed_copy": seal(&kit.wrap_key, Way::Kit) },
            "password": { "auth_key": b64(pw_kit(&pw.auth_key)), "sealed_copy": seal(&pw.wrap_key, Way::Password), "kdf": kdf },
        }))
    })
    .await;
    println!("account and room founded");
    let humans = fixture["members"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["device_role"] == "human")
        .filter_map(|m| m["device_id"].as_str().map(str::to_owned))
        .collect();
    let mut s = Seeder {
        args,
        fixture,
        files,
        human,
        agents: Vec::new(),
        seats: HashMap::new(),
        humans,
        ids: HashMap::new(),
        counts: BTreeMap::new(),
        bulk_open: Vec::new(),
        bulk_made: 0,
    };
    s.sessions().await?;
    s.human.sync().await;
    let steps = s.steps();
    println!("{} steps", steps.len());
    for (_, _, step) in steps {
        match step {
            Step::Card(card) => s.card(&card).await?,
            Step::Answer { card, answer } => s.answer(&card, &answer).await?,
            Step::Finish(card) => s.finish(&card).await?,
            Step::Item { key, item } => s.item(&key, &item).await?,
            Step::Artifact(p) => s.artifact(&p).await?,
        }
        s.pace().await;
    }
    s.finishing().await?;
    if s.args.bulk > 0 {
        let n = s.args.bulk;
        s.bulk(n).await?;
    }
    for agent in &s.agents {
        agent
            .client
            .settle(120_000)
            .await
            .map_err(|e| format!("settle: {e}"))?;
    }
    s.human.sync().await;
    if let Some(dir) = s.args.serve.clone() {
        s.serve(&dir).await?;
    }
    for agent in &s.agents {
        agent.client.stop().await;
    }
    if !s.human.findings.is_empty() {
        println!("findings of the human device: {:?}", s.human.findings);
    }
    println!("seeded: {:?}", s.counts);
    Ok(())
}

fn pw_kit(k: &trommi_core::crypto::Secret<32>) -> &[u8] {
    k.expose()
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("seed-account: {e}");
        std::process::exit(1);
    }
}
