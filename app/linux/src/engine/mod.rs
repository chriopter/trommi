//! The Linux app's engine: one human device of one room, as the iOS app's `Room` drives its device, over the same
//! facade (`trommi_core_ffi`: `CoreDevice`, `ClientModel`).
//!
//! - **The device** (`CoreDevice`) checks and keeps everything of the protocol: groups, keys, chains, the outbox.
//!   Its state is the connector's journal on disk.
//! - **The model** (`ClientModel`) is the client core's board model: the Desk's order, the session list, the cards
//!   and the Chats, decided the same way on every app. Its patches keep this app's copy ([`crate::view::Mirror`]).
//! - **This engine** does what each app does itself: the hub's routes, the catch-up in the hub's order, the live
//!   stream, the outbox, the local cache.
//!
//! The order of a catch-up is the client core's rule: the Welcomes, then the hub's open list (the Desk is right
//! before any history is read), then the changes from the cursor, oldest first, for the MLS epochs and the chains.

pub mod files;
pub mod signin;
mod sync;

use crate::store::{self, DurableStore, RoomRecord};
use crate::view::Mirror;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use trommi_connector::error::{Fault, Result};
use trommi_connector::hub::{b64, Hub, Signer};
use trommi_connector::store::Journal;
use trommi_connector::util::{hex, now_ms, unhex};
use trommi_core::hub_auth::SignedHubAuth;
use trommi_core::ids::RoomId;
use trommi_core_ffi::{
    body_file_ids, body_to_wire, error_code_text, ClientModel, CoreDevice, CoreError, Draft, DraftKind, GroupSummary,
};
use zeroize::Zeroizing;

/// What the window is told.
#[derive(Clone, Debug, Default)]
pub struct Update {
    /// One line on where the app stands.
    pub status: Option<String>,
    /// The views, as JSON text (`view.rs`).
    pub desk: Option<String>,
    pub sessions: Option<String>,
    pub chat: Option<String>,
    /// The live stream is open.
    pub live: Option<bool>,
    /// Signed in: a room is open.
    pub signed_in: Option<bool>,
    /// The person's own account line ("demo@example.org · 52 cards").
    pub title: Option<String>,
}

pub type Sink = Arc<dyn Fn(Update) + Send + Sync>;

pub fn fault(e: CoreError) -> Fault {
    Fault::new(error_code_text(e.code()), e.message().to_string())
}

/// A hub client that signs in as this device.
pub fn hub_for_device(device: &Arc<CoreDevice>, hub_url: &str, room: &[u8]) -> Result<Hub> {
    let room_id = RoomId::from_slice(room).map_err(Fault::from)?;
    let (device, address) = (device.clone(), hub_url.to_string());
    let signer: Signer = Arc::new(move |challenge| {
        let (device, address) = (device.clone(), address.clone());
        Box::pin(async move {
            let signed = device
                .hub_sign_in(address, challenge.to_vec())
                .map_err(fault)?;
            Ok(SignedHubAuth {
                auth: signed.auth,
                signature: signed.signature,
            })
        })
    });
    Hub::new(hub_url, Some(room_id), Some(signer))
}

/// One room on this machine, signed in.
pub struct Engine {
    pub dir: PathBuf,
    pub hub_url: String,
    pub room: Vec<u8>,
    /// This device, hex.
    pub me: String,
    pub device: Arc<CoreDevice>,
    pub hub: Hub,
    pub model: ClientModel,
    pub mirror: Mutex<Mirror>,
    /// One catch-up, one stream event, one send at a time (iOS `serial`).
    pub(crate) serial: tokio::sync::Mutex<()>,
    /// The change the model reaches (the device's own cursor may stand further: what it took before the app last
    /// saved its cache is read again, `sync.rs`).
    pub(crate) cursor: AtomicU64,
    pub(crate) synced: AtomicBool,
    pub(crate) live: AtomicBool,
    pub(crate) pumping: AtomicBool,
    pub(crate) closed: AtomicBool,
    pub(crate) key_packages_at_hub: Mutex<Option<u32>>,
    pub(crate) groups: Mutex<Vec<GroupSummary>>,
    pub(crate) selected: Mutex<Option<String>>,
    pub(crate) record: Mutex<RoomRecord>,
    /// The recovery code while the session joins of a sign-in with it are not through (8.5: forgotten then).
    pub(crate) code: Mutex<Option<Zeroizing<Vec<u8>>>>,
    pub(crate) unsaved: AtomicU64,
    pub(crate) next_wake: AtomicU64,
    sink: Sink,
}

impl Engine {
    /// The room the newest folder holds, opened from its cache: shown at once, before anything is read.
    pub fn open_newest(sink: Sink) -> Result<Option<Arc<Engine>>> {
        for dir in store::rooms() {
            if let Some(engine) = Self::open(&dir, sink.clone())? {
                return Ok(Some(engine));
            }
        }
        Ok(None)
    }

    pub fn open(dir: &Path, sink: Sink) -> Result<Option<Arc<Engine>>> {
        let Some(record) = RoomRecord::load(dir) else {
            return Ok(None);
        };
        let journal = Journal::open(&dir.join("device"))?;
        let device = Arc::new(CoreDevice::open_on(Box::new(DurableStore::new(journal))).map_err(fault)?);
        let engine = Self::with_device(dir, record, device, sink)?;
        if let Some((snapshot, cursor)) = store::load_model(dir) {
            if engine.model.load(snapshot).is_ok() {
                let _ = engine.model.touch_all();
                engine.cursor.store(cursor, Ordering::SeqCst);
            }
        }
        engine.refresh_groups()?;
        engine.emit();
        Ok(Some(engine))
    }

    pub(crate) fn with_device(
        dir: &Path,
        record: RoomRecord,
        device: Arc<CoreDevice>,
        sink: Sink,
    ) -> Result<Arc<Engine>> {
        let room = unhex(&record.room).ok_or_else(|| Fault::new("state-damaged", "room.json"))?;
        let hub = hub_for_device(&device, &record.hub, &room)?;
        let me = hex(&device.id().map_err(fault)?);
        let model = ClientModel::new(me.clone());
        // (every item header of a Chat stays in the model: the Chat is read whole, as on iOS)
        let _ = model.set_windows_open(true);
        Ok(Arc::new(Engine {
            dir: dir.to_path_buf(),
            hub_url: record.hub.clone(),
            room,
            me,
            device,
            hub,
            model,
            mirror: Mutex::new(Mirror::default()),
            serial: tokio::sync::Mutex::new(()),
            cursor: AtomicU64::new(0),
            synced: AtomicBool::new(false),
            live: AtomicBool::new(false),
            pumping: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            key_packages_at_hub: Mutex::new(None),
            groups: Mutex::new(Vec::new()),
            selected: Mutex::new(None),
            record: Mutex::new(record),
            code: Mutex::new(None),
            unsaved: AtomicU64::new(0),
            next_wake: AtomicU64::new(0),
            sink,
        }))
    }

    pub fn say(&self, status: impl Into<String>) {
        (self.sink)(Update {
            status: Some(status.into()),
            ..Update::default()
        });
    }

    /// The end of a batch: the model's patch into the copy, and the views to the window.
    pub fn emit(&self) {
        let now = now_ms();
        let patch = self.model.finish(now).unwrap_or_default();
        let mut mirror = self.mirror.lock().unwrap_or_else(|e| e.into_inner());
        if !patch.is_empty() {
            mirror.apply(&patch);
        }
        self.next_wake.store(mirror.wake_at, Ordering::SeqCst);
        let selected = self.selected.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let chat = selected.map(|s| mirror.chat_json(&s, &self.me, now));
        let (open, sessions) = mirror.counts();
        let update = Update {
            desk: Some(mirror.desk_json(now)),
            sessions: Some(mirror.sessions_json()),
            chat,
            signed_in: Some(true),
            title: Some(format!("{open} open · {sessions} sessions")),
            ..Update::default()
        };
        drop(mirror);
        (self.sink)(update);
    }

    /// The window shows this session's Chat (none: the Desk alone).
    pub fn select(&self, session: Option<String>) {
        *self.selected.lock().unwrap_or_else(|e| e.into_inner()) = session;
        self.emit();
    }

    /// The device's groups and the room's roles into the model.
    pub fn refresh_groups(&self) -> Result<()> {
        let groups = self.device.groups().map_err(fault)?;
        let roles = self.device.room_roles().map_err(fault)?;
        self.model
            .apply_groups(groups.clone(), roles)
            .map_err(fault)?;
        *self.groups.lock().unwrap_or_else(|e| e.into_inner()) = groups;
        Ok(())
    }

    /// The session groups this device holds (hex), not archived.
    pub(crate) fn session_groups(&self) -> std::collections::BTreeSet<String> {
        self.groups
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|g| g.session.is_some() && !g.archived)
            .map(|g| hex(&g.group))
            .collect()
    }

    /// The model as a cache on disk, with the change it reaches.
    pub fn save_cache(&self) {
        if let Ok(snapshot) = self.model.snapshot() {
            let _ = store::save_model(&self.dir, &snapshot, self.cursor.load(Ordering::SeqCst));
            self.unsaved.store(0, Ordering::SeqCst);
        }
    }

    // ---- what the person does ------------------------------------------------------------------------------

    /// A body of `kind` in the wire's names (the client core's codec, as every app writes it).
    fn wire(kind: &str, content: &Value) -> Result<Vec<u8>> {
        let text = body_to_wire(kind.to_string(), content.to_string()).map_err(fault)?;
        if text.len() > 60_000 {
            return Err(Fault::new("too-large", "body over 60 KB"));
        }
        Ok(text.into_bytes())
    }

    /// The files a body names, as the core seals them with it (the client core's codec).
    fn files_of(kind: &str, content: &Value) -> Result<Vec<Vec<u8>>> {
        body_file_ids(kind.to_string(), content.to_string()).map_err(fault)
    }

    fn draft(kind: DraftKind) -> Draft {
        Draft {
            kind,
            session: None,
            card: None,
            board: None,
            group: None,
            name: None,
            value: None,
            object_id: None,
            request_id: None,
            choices: None,
            closes: None,
            closed: None,
            allow: None,
            urgency: None,
            push: None,
            expires_at: None,
            payload: None,
        }
    }

    fn local_id() -> String {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        format!("local-{}-{}", now_ms(), SEQ.fetch_add(1, Ordering::SeqCst))
    }

    /// Seals one envelope (it is in the outbox from then on, durable), confirms its echo, and sends.
    async fn send(self: &Arc<Self>, local_id: &str, draft: Draft, recipient: Option<Vec<u8>>, files: Vec<Vec<u8>>) -> Result<()> {
        let sealed = {
            let _one = self.serial.lock().await;
            let sealed = self
                .device
                .seal(draft, recipient, files, now_ms())
                .map_err(fault)?;
            let object = sealed.object_id.as_ref().map(|o| hex(o));
            let _ = self
                .model
                .confirm_echo(local_id.to_string(), hex(&sealed.envelope_hash), sealed.seq, object);
            sealed
        };
        let _ = sealed;
        self.emit();
        self.pump_outbox();
        Ok(())
    }

    /// Answers a decision card with these options (an info card: read, `choices` empty).
    pub async fn answer(self: &Arc<Self>, card_id: &str, choices: Vec<String>) -> Result<()> {
        self.answer_with(card_id, choices, Vec::new()).await
    }

    /// Answers a card with these options and these files (attachment references, `upload_file`).
    pub async fn answer_with(self: &Arc<Self>, card_id: &str, choices: Vec<String>, attachments: Vec<Value>) -> Result<()> {
        let card = self
            .mirror
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .card(card_id)
            .cloned()
            .ok_or_else(|| Fault::new("not-found", "no such card"))?;
        if card["object_state"] != json!("open") {
            return Err(Fault::new("card-closed", "the card is not open"));
        }
        if card["version_hash"].is_null() || card["content_state"] != json!("ok") {
            return Err(Fault::new("card-pruned", "this device holds only the card's header"));
        }
        let session = card["session_id"]
            .as_str()
            .and_then(unhex)
            .ok_or_else(|| Fault::new("not-found", "the card's session"))?;
        let info = card["card_type"] == json!("info");
        let action = if info && choices.is_empty() { "read" } else { "answer" };
        let finals: Vec<&str> = card["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|o| o["final"] == json!(true))
            .filter_map(|o| o["key"].as_str())
            .collect();
        // (an answer with files says more than its choice: it does not settle the card by itself)
        let settles = action == "answer" && attachments.is_empty() && !choices.is_empty() && choices.iter().all(|c| finals.contains(&c.as_str()));
        let closes = action != "answer" || settles;
        let at = now_ms();
        let mut content = json!({ "answer_action": action, "choices": choices, "answered_at": at });
        if !attachments.is_empty() {
            content["attachments"] = json!(attachments);
        }
        let local = Self::local_id();
        let echo = json!({
            "answer_action": action, "choices": choices, "note": null, "option_notes": {}, "attachments": attachments, "marks": [],
            "trusted": false, "bound_version_hash": card["version_hash"], "bound_object_version": card["object_version"],
            "envelope_number": null, "envelope_hash": null, "by_device_id": self.me, "answered_at": at,
            "taken_back_at": null, "taken_back_sent_at": null,
        });
        let _ = self.model.echo_answer(
            local.clone(),
            card_id.to_string(),
            echo.to_string(),
            if closes { "closed".into() } else { "answered".into() },
        );
        self.emit();
        let payload = Self::wire("answer", &content).and_then(|p| Ok((p, Self::files_of("answer", &content)?)));
        let result = match payload {
            Ok((payload, files)) => {
                let mut d = Self::draft(DraftKind::Answer);
                d.session = Some(session);
                d.object_id = unhex(card_id);
                d.choices = Some(choices);
                d.closes = Some(closes);
                d.payload = Some(payload);
                self.send(&local, d, None, files).await
            }
            Err(e) => Err(e),
        };
        if result.is_err() {
            let _ = self.model.rollback_echo(local);
            self.emit();
        }
        result
    }

    /// Allows or denies a permission request.
    pub async fn verdict(self: &Arc<Self>, request_id: &str, allow: bool) -> Result<()> {
        let asked = self
            .mirror
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .permission(request_id)
            .cloned()
            .ok_or_else(|| Fault::new("not-found", "no such permission request"))?;
        let session = asked["session_id"]
            .as_str()
            .and_then(unhex)
            .ok_or_else(|| Fault::new("not-found", "the request's session"))?;
        let mut d = Self::draft(DraftKind::Verdict);
        d.session = Some(session);
        d.request_id = unhex(request_id);
        d.allow = Some(allow);
        d.payload = Some(Self::wire("verdict", &json!({}))?);
        self.send(&Self::local_id(), d, None, Vec::new()).await
    }

    /// A message to a session's agent, shown at once.
    pub async fn say_to(self: &Arc<Self>, session_id: &str, text: &str) -> Result<()> {
        self.say_with(session_id, text, Vec::new()).await
    }

    /// A message with files (attachment references, `upload_file`).
    pub async fn say_with(self: &Arc<Self>, session_id: &str, text: &str, attachments: Vec<Value>) -> Result<()> {
        let text = text.trim();
        if text.is_empty() && attachments.is_empty() {
            return Ok(());
        }
        let recipient = self
            .mirror
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .agent_of(session_id)
            .ok_or_else(|| Fault::new("bad-argument", "this session has no agent to write to"))?;
        let mut content = json!({ "content_type": "message", "text": text });
        if !attachments.is_empty() {
            content["attachments"] = json!(attachments);
        }
        let local = Self::local_id();
        let key = format!("chat:session/{session_id}");
        let _ = self.model.echo_item(
            local.clone(),
            key,
            content.to_string(),
            Some(recipient.clone()),
            None,
            now_ms(),
        );
        self.emit();
        let result = async {
            let mut d = Self::draft(DraftKind::SessionChat);
            d.session = Some(unhex(session_id).ok_or_else(|| Fault::new("bad-argument", "session"))?);
            d.payload = Some(Self::wire("message", &content)?);
            self.send(&local, d, None, Self::files_of("message", &content)?).await
        }
        .await;
        if result.is_err() {
            let _ = self.model.rollback_echo(local);
            self.emit();
        }
        result
    }

    /// Closes the device (the journal's lock is let go).
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.save_cache();
        self.device.close();
    }
}

/// The JSON a page of the hub holds under `field`, base64url, as bytes.
pub(crate) fn bytes_of(v: &Value, field: &str) -> Result<Vec<u8>> {
    trommi_connector::hub::unb64(v, field)
}

pub(crate) fn b64s(bytes: &[u8]) -> String {
    b64(bytes)
}
