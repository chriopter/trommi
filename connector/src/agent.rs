//! What an agent writes: Chat messages, cards and their versions, registers (profile, status lines, `heard`),
//! permission requests, Artifacts with their files and Share links, the work trail, and helper sessions, which
//! it founds and keeps itself (spec/v1.md 5.2.5, 5.3.5). Every write is an envelope the engine seals
//! ([`trommi_client_core::engine::AgentWrite`]), or a step of the engine's device; this file only says what goes
//! into it. A later version is built from the board model (`core/client/src/model`): its content as the core's
//! codec writes it back to the wire, its number and the version it follows, held against the object as the device
//! holds it.
use crate::driver::{b64, Driver, SentInfo};
use crate::error::{Fault, Result};
use crate::util::{hex, now_ms, unhex};
use serde_json::{json, Map, Value};
use trommi_client_core::codec::{encode_body, BodyKind};
use trommi_client_core::engine::AgentWrite;
use trommi_client_core::hub::{Answer, Route};
use trommi_core::crypto::SystemEntropy;
use trommi_core::envelope::{Urgency, SCHEMA_VERSION};
use trommi_core::files::{self, FileRef, ShareLink};
use trommi_core::ids::{FileId, GroupId, Hash32, ObjectId, ShareId, TurnId};

/// The urgencies by name, lowest first.
pub const URGENCIES: [&str; 4] = ["low", "normal", "high", "critical"];

/// The urgency a name stands for; normal for anything else.
pub fn urgency_of(name: &str) -> Urgency {
    match name {
        "low" => Urgency::Low,
        "high" => Urgency::High,
        "critical" => Urgency::Critical,
        _ => Urgency::Normal,
    }
}

fn object_id_of(hex_id: &str) -> Result<ObjectId> {
    unhex(hex_id)
        .and_then(|bytes| ObjectId::from_slice(&bytes).ok())
        .ok_or_else(|| Fault::plain(format!("not an object id: {hex_id}")))
}

/// A hash the model writes as hex, as the wire writes it.
fn wire_hash(hex_hash: &str) -> Result<String> {
    unhex(hex_hash)
        .filter(|bytes| bytes.len() == 32)
        .map(|bytes| b64(&bytes))
        .ok_or_else(|| Fault::new("internal", "a version hash that does not read"))
}

/// The payload with its schema version and `more` beside what the caller gave.
fn with_schema(mut object: Map<String, Value>, more: &[(&str, Value)]) -> String {
    object.insert("schema_version".into(), json!(SCHEMA_VERSION));
    for (key, value) in more {
        object.insert((*key).into(), value.clone());
    }
    Value::Object(object).to_string()
}

/// The fields of the model (non-null) as the wire writes them: the codec's way back.
fn wire_fields(kind: BodyKind, fields: Map<String, Value>) -> Result<Map<String, Value>> {
    let fields: Map<String, Value> = fields.into_iter().filter(|(_, v)| !v.is_null()).collect();
    let text = encode_body(kind, &fields).map_err(|e| {
        Fault::new(
            "internal",
            format!("the board's content does not write back ({e:?})"),
        )
    })?;
    let mut out: Map<String, Value> = serde_json::from_str(&text)?;
    out.remove("schema_version");
    Ok(out)
}

/// The core model names attachments in hex; tool uploads already carry their wire references.
pub(crate) fn attachment_wire(reference: &Value) -> Result<Value> {
    if reference.get("attachment_id").is_some() {
        trommi_client_core::codec::attachment_to_wire(reference)
            .map(Value::Object)
            .map_err(|_| Fault::new("bad-format", "the attachment reference does not read"))
    } else {
        Ok(reference.clone())
    }
}

/// The file ids an item's attachments name: what its header lists (11.2).
fn file_ids_of(fields: &Map<String, Value>) -> Vec<FileId> {
    let mut ids: Vec<FileId> = Vec::new();
    for attachment in fields
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for key in ["file_id", "poster_file_id"] {
            if let Some(id) = attachment
                .get(key)
                .and_then(Value::as_str)
                .and_then(|id| FileId::from_base64url(id).ok())
            {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

/// The version an object stands at, as the model shows it and the device holds it: its number and its hash on
/// the wire. Not yet when the model's version is not the device's current one (the hub has not confirmed it).
fn current_version(
    driver: &Driver,
    session: Option<&str>,
    object: &str,
    model_version: Option<(String, i64)>,
) -> Result<(i64, String)> {
    let not_yet = || {
        Fault::plain(format!(
            "{object} is not on the board yet: the hub has not confirmed it"
        ))
    };
    let (hash, version) = model_version.ok_or_else(not_yet)?;
    let inner = driver.lock();
    let group = inner.group_of(session)?;
    let held = inner
        .engine()
        .device()
        .object(&group, &object_id_of(object)?)
        .ok()
        .flatten()
        .ok_or_else(not_yet)?;
    if hex(held.current.as_bytes()) != hash {
        return Err(not_yet());
    }
    Ok((version, wire_hash(&hash)?))
}

/// A session's id on the wire and its group, from the model's hex (the main one for none).
fn wire_session(driver: &Driver, session_id: Option<&str>) -> Result<(String, GroupId)> {
    let inner = driver.lock();
    let group = inner.group_of(session_id)?;
    let session = group
        .session_id()
        .ok_or_else(|| Fault::new("forbidden", "an agent writes into session groups only"))?;
    Ok((session.to_base64url(), group))
}

impl Driver {
    /// A Chat message: in the Chat of the card `object_id`, or of the session (the main one for none).
    pub async fn send_message(
        &self,
        mut fields: Map<String, Value>,
        object_id: Option<String>,
        session_id: Option<String>,
    ) -> Result<SentInfo> {
        fields.insert("content_type".into(), json!("message"));
        // A file belongs to the first object that names it (11.3): a message that shows an Artifact names the
        // Artifact, not its files.
        let files = if fields.contains_key("artifact_object_id") {
            Vec::new()
        } else {
            file_ids_of(&fields)
        };
        let payload = with_schema(fields, &[]);
        let write = match object_id {
            Some(card) => {
                let session = self
                    .lock()
                    .model()
                    .cards
                    .get(&card)
                    .and_then(|c| c.session_id.clone());
                let (session, _) = wire_session(self, session.as_deref())?;
                AgentWrite::CardChat {
                    session,
                    card: object_id_of(&card)?.to_base64url(),
                    payload,
                }
            }
            None => {
                let (session, _) = wire_session(self, session_id.as_deref())?;
                AgentWrite::SessionChat { session, payload }
            }
        };
        self.write(write, &files).await
    }

    /// A new card. `fields` is its content with `urgency`; returns the card's id.
    pub async fn send_card(
        &self,
        mut fields: Map<String, Value>,
        session_id: Option<String>,
    ) -> Result<String> {
        let urgency = fields
            .remove("urgency")
            .and_then(|v| v.as_str().map(urgency_of))
            .unwrap_or(Urgency::Normal);
        // (a card still being worked on stays off his Desk: no push, spec 9.1.3)
        let push = fields.get("working") != Some(&Value::Bool(true));
        let files = file_ids_of(&fields);
        let (session, _) = wire_session(self, session_id.as_deref())?;
        // 9: the level stands in the body; the header carries only whether it knocks
        let payload = with_schema(
            fields,
            &[
                ("object_version", json!(1)),
                ("previous_version_hash", json!(Hash32::ZERO.to_base64url())),
                ("urgency", json!(urgency.name())),
            ],
        );
        let write = AgentWrite::CardFirst {
            session,
            urgency: urgency.byte(),
            push,
            payload,
        };
        let sent = self.write(write, &files).await?;
        sent.object_id
            .ok_or_else(|| Fault::new("internal", "a card without an id"))
    }

    /// A new version of a card of this agent: its content with `changes` laid over it.
    async fn version(
        &self,
        object_id: &str,
        changes: Map<String, Value>,
        closed: bool,
        urgency: Option<String>,
        push: bool,
    ) -> Result<()> {
        let (fields, session, level, model_version) = {
            let inner = self.lock();
            let card = inner
                .model()
                .cards
                .get(object_id)
                .ok_or_else(|| Fault::plain(format!("no card {object_id}")))?;
            if card.content_state != "ok" {
                return Err(Fault::plain(format!(
                    "card {object_id} has a newer version this connector could not read; it cannot be changed from here"
                )));
            }
            let fields = match serde_json::to_value(&card.fields)? {
                Value::Object(fields) => fields,
                _ => Map::new(),
            };
            (
                fields,
                card.session_id.clone(),
                urgency.unwrap_or_else(|| card.urgency.clone()),
                card.version_hash.clone().map(|h| (h, card.object_version)),
            )
        };
        let mut content = wire_fields(BodyKind::Card, fields)?;
        for (key, value) in changes {
            content.insert(key, value);
        }
        // `working` is true or absent (spec 9.1.3); a version still working pushes nothing.
        let working = content.get("working") == Some(&Value::Bool(true));
        if !working {
            content.remove("working");
        }
        let push = push && !working;
        for key in [
            "schema_version",
            "object_version",
            "previous_version_hash",
            "urgency",
        ] {
            content.remove(key);
        }
        if !URGENCIES.contains(&level.as_str()) {
            return Err(Fault::plain(format!("not an urgency: {level}")));
        }
        let level = urgency_of(&level);
        let (version, previous) =
            current_version(self, session.as_deref(), object_id, model_version)?;
        let (session, _) = wire_session(self, session.as_deref())?;
        let files = file_ids_of(&content);
        let payload = with_schema(
            content,
            &[
                ("object_version", json!(version + 1)),
                ("previous_version_hash", json!(previous)),
                ("urgency", json!(level.name())),
            ],
        );
        let write = AgentWrite::CardVersion {
            session,
            object: object_id_of(object_id)?.to_base64url(),
            closed,
            urgency: level.byte(),
            push,
            payload,
        };
        self.write(write, &files).await.map(|_| ())
    }

    /// Reworks an open card.
    pub async fn revise(
        &self,
        object_id: &str,
        changes: Map<String, Value>,
        urgency: Option<String>,
    ) -> Result<()> {
        self.version(object_id, changes, false, urgency, true).await
    }

    /// Changes an open card's urgency.
    pub async fn set_urgency(
        &self,
        object_id: &str,
        urgency: &str,
        reason: Option<Value>,
    ) -> Result<()> {
        let mut changes = Map::new();
        changes.insert("urgency_reason".into(), reason.unwrap_or(Value::Null));
        self.version(object_id, changes, false, Some(urgency.into()), false)
            .await
    }

    /// Takes an open card away.
    pub async fn withdraw(&self, object_id: &str, reason: &str) -> Result<()> {
        let mut changes = Map::new();
        changes.insert("withdraw_reason".into(), json!(reason));
        self.version(object_id, changes, true, None, false).await
    }

    /// Closes a card with a summary of what was done.
    pub async fn close(&self, object_id: &str, summary: &str) -> Result<()> {
        let mut changes = Map::new();
        changes.insert("close_summary".into(), json!(summary));
        self.version(object_id, changes, true, None, false).await
    }

    /// One new card in place of several open ones, which are closed as merged into it.
    pub async fn merge(
        &self,
        object_ids: &[String],
        mut fields: Map<String, Value>,
        session_id: Option<String>,
    ) -> Result<String> {
        fields.insert("merged_from_object_ids".into(), json!(object_ids));
        let id = self.send_card(fields, session_id).await?;
        for old in object_ids {
            let mut changes = Map::new();
            changes.insert("merged_into_object_id".into(), json!(id));
            self.version(old, changes, true, None, false).await?;
        }
        Ok(id)
    }

    /// Publishes an Artifact: a Page (HTML) or Media, with its file. Returns its id.
    pub async fn publish(
        &self,
        attachments: Value,
        title: Value,
        note: Option<Value>,
        session_id: Option<String>,
    ) -> Result<String> {
        let page = attachments
            .get(0)
            .and_then(|a| a.get("media_type"))
            .and_then(Value::as_str)
            == Some("text/html");
        let mut fields = Map::new();
        fields.insert(
            "artifact_type".into(),
            json!(if page { "page" } else { "media" }),
        );
        fields.insert("title".into(), title);
        fields.insert("note".into(), note.unwrap_or(Value::Null));
        fields.insert("attachments".into(), attachments);
        let files = file_ids_of(&fields);
        let (session, _) = wire_session(self, session_id.as_deref())?;
        let payload = with_schema(
            fields,
            &[
                ("object_version", json!(1)),
                ("previous_version_hash", json!(Hash32::ZERO.to_base64url())),
            ],
        );
        let sent = self
            .write(AgentWrite::ArtifactFirst { session, payload }, &files)
            .await?;
        sent.object_id
            .ok_or_else(|| Fault::new("internal", "an Artifact without an id"))
    }

    /// Revokes an Artifact: a closed version; the hub deletes its files and ends its Share links (11.4).
    pub async fn unpublish(&self, object_id: &str) -> Result<()> {
        let (session, fields, model_version) = {
            let inner = self.lock();
            let p = inner
                .model()
                .published
                .get(object_id)
                .ok_or_else(|| Fault::plain(format!("no asset {object_id}")))?;
            let mut fields = Map::new();
            fields.insert(
                "artifact_type".into(),
                p.artifact_type.clone().unwrap_or(Value::Null),
            );
            fields.insert("title".into(), json!(p.title));
            fields.insert("note".into(), p.note.clone().unwrap_or(Value::Null));
            fields.insert("attachments".into(), p.attachments.clone());
            (
                p.session_id.clone(),
                fields,
                Some((p.version_hash.clone(), p.object_version)),
            )
        };
        let content = wire_fields(BodyKind::Artifact, fields)?;
        let (version, previous) =
            current_version(self, session.as_deref(), object_id, model_version)?;
        let (wire, _) = wire_session(self, session.as_deref())?;
        let files = file_ids_of(&content);
        let payload = with_schema(
            content,
            &[
                ("object_version", json!(version + 1)),
                ("previous_version_hash", json!(previous)),
            ],
        );
        let write = AgentWrite::ArtifactVersion {
            session: wire,
            object: object_id_of(object_id)?.to_base64url(),
            closed: true,
            payload,
        };
        self.write(write, &files).await.map(|_| ())
    }

    /// Sets registers of this agent in a session: `profile`, `status_line/<id>`, `device/<id>`, `heard`. A
    /// null value deletes the name.
    pub async fn set_status(
        &self,
        values: Map<String, Value>,
        session_id: Option<String>,
    ) -> Result<SentInfo> {
        let (_, group) = wire_session(self, session_id.as_deref())?;
        let mut last = SentInfo::default();
        for (name, value) in values {
            let value = (!value.is_null()).then(|| value.to_string());
            let write = AgentWrite::Register {
                group: b64(group.as_bytes()),
                name,
                value,
            };
            last = self.write(write, &[]).await?;
        }
        Ok(last)
    }

    /// The current value of a shared register of this agent's main session (as JSON text), e.g. a human
    /// device's `join_link/<nonce>` (12.4).
    pub async fn session_register(&self, name: &str) -> Result<Option<String>> {
        let inner = self.lock();
        let group = inner.group_of(None)?;
        Ok(inner
            .engine()
            .device()
            .register(&group, name)
            .ok()
            .flatten()
            .map(|value| String::from_utf8_lossy(value.expose()).into_owned()))
    }

    /// Writes this device's label (`device/<id>`: name, platform, folder, host) into its main session, unless
    /// it stands there already.
    pub async fn ensure_device_register(&self, info: &Value) -> Result<()> {
        let name = format!("device/{}", self.me());
        let held = {
            let inner = self.lock();
            let group = inner.group_of(None)?;
            inner
                .engine()
                .device()
                .register_of(&group, &name, &self.device_id())
                .ok()
                .flatten()
                .map(|value| String::from_utf8_lossy(value.expose()).into_owned())
        };
        // Compared as values: the stored text may order its fields otherwise.
        let same = held
            .as_deref()
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .is_some_and(|value| value == *info);
        if same {
            return Ok(());
        }
        let mut values = Map::new();
        values.insert(name, info.clone());
        self.set_status(values, None).await.map(|_| ())
    }

    /// Notes up to which change of a human's messages this agent has been handed them.
    pub async fn mark_heard(&self, up_to: u64, session_id: Option<String>) -> Result<bool> {
        let known = {
            let inner = self.lock();
            let sid = session_id.clone().or_else(|| inner.session_id());
            sid.as_ref()
                .and_then(|sid| inner.model().sessions.get(sid))
                .and_then(|s| s.heard_up_to)
        };
        if known.is_some_and(|known| known >= up_to) {
            return Ok(false);
        }
        let mut values = Map::new();
        values.insert("heard".into(), json!({ "up_to": up_to }));
        self.set_status(values, session_id).await?;
        Ok(true)
    }

    /// Asks the human to allow a tool call: a permission request that expires. Returns its id.
    pub async fn request_permission(
        &self,
        tool_name: &str,
        description: &str,
        input_preview: &str,
        expires_in_ms: u64,
        session_id: Option<String>,
    ) -> Result<String> {
        let mut fields = Map::new();
        fields.insert("tool_name".into(), json!(tool_name));
        fields.insert("description".into(), json!(description));
        fields.insert("input_preview".into(), json!(input_preview));
        let (session, _) = wire_session(self, session_id.as_deref())?;
        let write = AgentWrite::PermissionRequest {
            session,
            urgency: Urgency::High.byte(),
            expires_at: now_ms() + expires_in_ms,
            push: true,
            payload: with_schema(fields, &[]),
        };
        let sent = self.write(write, &[]).await?;
        sent.object_id
            .ok_or_else(|| Fault::new("internal", "a request without an id"))
    }

    /// A permission request has no later version in protocol v1: one that was answered in the terminal stays
    /// on the board until it expires, and a verdict that still comes for it is dropped by the connector.
    pub async fn withdraw_permission(&self, _object_id: &str, _reason: &str) -> Result<bool> {
        Ok(false)
    }

    /// Archives a helper session of before this agent opened (5.2.10): the hub takes nothing more for its group.
    /// Archiving one that is archived already is no error.
    pub async fn archive_child_session(&self, session_id: &str) -> Result<()> {
        self.active()?;
        let group = {
            let inner = self.lock();
            if inner.session_id().as_deref() == Some(session_id) {
                return Err(Fault::new(
                    "forbidden",
                    "an agent archives only its helper sessions, not its main session",
                ));
            }
            match inner.group_of(Some(session_id)) {
                Ok(group) => group,
                Err(_) => return Ok(()),
            }
        };
        self.archive(group).await
    }

    /// One step of the running turn's work trail (7.3): an MLS message in the session's group, numbered from 1
    /// within `turn`. `step` is JSON `{ text, tool? }`.
    pub async fn work_step(
        &self,
        session_id: Option<&str>,
        turn: [u8; 16],
        number: u32,
        step: &Value,
    ) -> Result<()> {
        let group = self.lock().group_of(session_id)?;
        // The step's body as the core writes and checks it (`{ text, tool? }`, with its limits).
        let step = trommi_core::trail::WorkStep {
            text: step
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            tool: step.get("tool").and_then(Value::as_str).map(str::to_owned),
        }
        .encode()?;
        self.device_queue(move |d, now| {
            // A trail is live: while too many steps wait for the hub, new ones are dropped, not queued.
            let waiting = d
                .outbox()
                .iter()
                .filter(|entry| entry.kind == trommi_core::store::OutboxKind::Message)
                .count();
            if waiting >= crate::driver::MAX_WAITING_STEPS {
                return Ok(());
            }
            d.send_work_trail(&group, &TurnId::new(turn), number, &step, now)
                .map(|_| ())
        })
    }

    /// Encrypts a file under a fresh key (section 11), stores it at the hub and returns the attachment
    /// reference for a body: `file_id`, `file_key`, `sha256`, `total_size` and what `meta` says of it.
    pub async fn upload_attachment(
        &self,
        bytes: Vec<u8>,
        meta: Map<String, Value>,
    ) -> Result<Map<String, Value>> {
        let (stored, sealed) = files::encrypt_file(&bytes, &mut SystemEntropy)?;
        let file = sealed.file.file_id;
        match self
            .ask(Route::PutFile {
                file: &file,
                bytes: &stored,
            })
            .await?
        {
            Answer::FileStored { .. } => {}
            _ => return Err(Fault::new("bad-format", "the hub's answer does not read")),
        }
        let file_id = file.to_base64url();
        let mut reference = meta;
        reference.insert("file_id".into(), json!(file_id));
        reference.insert(
            "file_key".into(),
            json!(String::from_utf8_lossy(
                sealed.file.file_key_base64url().expose()
            )),
        );
        reference.insert("sha256".into(), json!(sealed.file.sha256.to_base64url()));
        reference.insert("total_size".into(), json!(sealed.plain_len));
        // Kept under the whole reference (id, key, hash): another reference to the same id finds nothing.
        let cached = format!(
            "{file_id} {} {}",
            reference
                .get("file_key")
                .and_then(Value::as_str)
                .unwrap_or(""),
            reference
                .get("sha256")
                .and_then(Value::as_str)
                .unwrap_or("")
        );
        self.file_keep(cached, bytes);
        Ok(reference)
    }

    /// Fetches the file an attachment reference names and opens it: the stored bytes are compared with the
    /// reference's SHA-256 before anything is decrypted (11.2).
    pub async fn fetch_attachment(&self, reference: &Value) -> Result<Vec<u8>> {
        let reference = attachment_wire(reference)?;
        let text = |key: &str| reference.get(key).and_then(Value::as_str).unwrap_or("");
        let file = FileRef::from_base64url(text("file_id"), text("file_key"), text("sha256"))?;
        let file_id = file.file_id.to_base64url();
        let cached = format!("{file_id} {} {}", text("file_key"), text("sha256"));
        if let Some(bytes) = self.file_cached(&cached) {
            return Ok(bytes);
        }
        let stored = match self
            .ask(Route::GetFile {
                file: &file.file_id,
                range: None,
            })
            .await?
        {
            Answer::File(file) => file.bytes,
            _ => return Err(Fault::new("bad-format", "the hub's answer does not read")),
        };
        Ok(files::decrypt_file(&file, &stored)?)
    }

    /// A Share link for one file of an open Artifact (11.5): `(share_id, link, expires_at)`. The link holds
    /// the secret and the key; the hub gets the secret's hash.
    pub async fn share_attachment(
        &self,
        reference: &Value,
        expires_at: u64,
    ) -> Result<(String, String, u64)> {
        let reference = attachment_wire(reference)?;
        let text = |key: &str| reference.get(key).and_then(Value::as_str).unwrap_or("");
        let file = FileRef::from_base64url(text("file_id"), text("file_key"), text("sha256"))?;
        let link = ShareLink::create(&self.room().app, &file, &mut SystemEntropy)?;
        let secret_hash = crate::util::sha256(link.secret.expose());
        let until = match self
            .ask(Route::PostShare {
                share: &link.share_id,
                secret_hash: &secret_hash,
                file: &file.file_id,
                expires_at,
            })
            .await?
        {
            Answer::Shared { expires_at } => expires_at,
            _ => expires_at,
        };
        let text = String::from_utf8_lossy(link.to_text().expose()).into_owned();
        Ok((link.share_id.to_base64url(), text, until))
    }

    /// Ends a Share link.
    pub async fn revoke_share(&self, share_id: &str) -> Result<()> {
        let share =
            ShareId::from_base64url(share_id).map_err(|_| Fault::plain("not a share id"))?;
        self.ask(Route::DeleteShare { share: &share })
            .await
            .map(|_| ())
    }
}
