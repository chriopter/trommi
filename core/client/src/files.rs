//! Files, as the person attaches, opens, saves and shares them (`Action::Attach`, `OpenFile`, `Share`,
//! `RevokeShare`, `CancelFile`; spec/v1.md 11). The web's `uploadAttachment`, `fetchAttachment`, `shareAttachment`
//! and `revokeShare`, iOS `RoomFiles.swift`, written once.
//!
//! - **Attach**: the picked bytes are encrypted under a key made for the file alone (`trommi_core::files`), the
//!   stored bytes go to the hub (`PUT /v1/files/{id}`, an upload), and the file shows in `ViewModel.files` under
//!   the action's `local_id`: `uploading`, then `ready` with its `attachment_id`, or `failed`. A line or an answer
//!   names it by either id; its reference (with the key) is put into the body when it is sealed.
//! - **OpenFile**: a file the views show (only those: a file id the person never saw is refused) is fetched,
//!   checked against the reference's SHA-256 and decrypted; its bytes go to the shell as `FileReady` (to draw) or
//!   `SaveFile` (to save). Bytes this device uploaded are handed from memory.
//! - **Share**: a Share link (11.5) for a file the views show: the hub learns the hash of the link's secret only.
//!   The link (with the key behind its `#`) is shown under the file (`FileView.shares`), kept in memory only.
//! - **RevokeShare**: the link ends at the hub (`DELETE /v1/shares/{id}`, kept across a restart: idempotent).
//! - **CancelFile**: an upload or a download the person started is dropped (its request cancelled).
//!
//! Every request goes through the engine (the token, the retries); a refusal is a toast. Keys stay out of every
//! `Debug`, log and stored entry.

use crate::app::bytes::Bytes;
use crate::app::io::IoId;
use crate::app::Effect;
use crate::hub::{Answer, Codec, Expect, Failure, Route};
use crate::views::{FileShareView, FileView, Text};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use trommi_core::files::{FileRef, Sealed};
use trommi_core::ids::ShareId;

/// The most bytes one attached file may have (the hub's limit of a stored file is a little above).
pub const ATTACH_MAX: usize = 60 * 1024 * 1024;
/// Uploaded files whose plaintext is kept in memory for showing them again without a download, at most.
const KEPT_PLAIN: usize = 16;
/// How long a Share link lives unless the action says (the hub's most: 30 days), less a minute.
pub const SHARE_DAYS_MS: u64 = 30 * 86_400_000 - 60_000;
/// The app's origin a Share link points at.
pub const SHARE_APP: &str = "https://app.trommi.com";

/// What was asked of the hub for a file.
enum Asked {
    Put { local_id: String, expect: Expect },
    Get { attachment_id: String, save: bool, expect: Expect },
    Share { attachment_id: String, share_id: String, link: String, expect: Expect },
    Unshare { share_id: String, expect: Expect },
}

struct Upload {
    /// The reference in the model's spelling, with the key: what a body names.
    reference: Map<String, Value>,
    sha256: [u8; 32],
    request: Option<IoId>,
}

/// The files of one App: what is shown, uploads, transfers, shares.
#[derive(Default)]
pub struct Files {
    /// The references (model spelling, with the key) of the files the views showed last, by attachment id.
    shown: BTreeMap<String, Map<String, Value>>,
    uploads: BTreeMap<String, Upload>,
    asked: BTreeMap<IoId, Asked>,
    /// The App's word on a file, laid over what the views made: uploads, transfers, failures, loaded.
    states: BTreeMap<String, FileView>,
    /// Plaintext of files uploaded here, by attachment id, oldest first.
    plain: Vec<(String, Bytes)>,
    /// Share links made here, by attachment id.
    shares: BTreeMap<String, Vec<FileShareView>>,
}

/// What a file action wants of the engine: a request to send through it.
pub struct Send {
    build: Builder,
    keep: Option<String>,
    pending: PendingAsk,
}

type Builder = Box<dyn FnOnce(&crate::hub::Site<'_>) -> Result<(crate::app::io::HttpRequest, Expect), Failure>>;
/// What the engine sends: builds the request on its site, leaving what the route expects in the cell.
pub type Build = Box<dyn FnOnce(&crate::hub::Site<'_>) -> Result<crate::app::io::HttpRequest, Failure>>;

enum PendingAsk {
    Put { local_id: String },
    Get { attachment_id: String, save: bool },
    Share { attachment_id: String, share_id: String, link: String },
    Unshare { share_id: String },
}

/// Why a file action was not done: the toast's words.
pub type NotDone = Text;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex16(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(text.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn b64(bytes: &[u8]) -> String {
    trommi_core::ids::base64url_encode(bytes)
}

/// The reference of a model attachment as the protocol core takes it.
fn file_ref(reference: &Map<String, Value>) -> Option<FileRef> {
    let id = unhex16(reference.get("attachment_id")?.as_str()?)?;
    FileRef::from_base64url(&b64(&id), reference.get("file_key")?.as_str()?, reference.get("sha256")?.as_str()?).ok()
}

/// A shown file as the views name it (no key).
pub fn view_of(reference: &Map<String, Value>) -> FileView {
    let text = |k: &str| reference.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    FileView {
        name: text("file_name"),
        media_type: text("media_type"),
        size: reference.get("total_size").and_then(Value::as_u64).unwrap_or(0),
        state: "ready".into(),
        attachment_id: reference.get("attachment_id").and_then(Value::as_str).map(String::from),
        percent: None,
        loaded: false,
        shares: Vec::new(),
    }
}

impl Files {
    /// The files the views made this time (`views::render`), with their references: what may be opened and shared.
    pub fn shown(&mut self, shown: BTreeMap<String, Map<String, Value>>, files: &mut BTreeMap<String, FileView>) {
        self.shown = shown;
        files.clear();
        for (id, reference) in &self.shown {
            files.insert(id.clone(), view_of(reference));
        }
        for (id, state) in &self.states {
            files.insert(id.clone(), state.clone());
        }
        for (id, shares) in &self.shares {
            if let Some(f) = files.get_mut(id) {
                f.shares = shares.clone();
            }
        }
    }

    /// The reference (model spelling, with the key) of a file a body may name: an upload of this device that is
    /// stored, by its local id or its attachment id, or a file the views show.
    pub(crate) fn upload_failed(&self, id: &str) -> bool { self.states.get(id).is_some_and(|s| s.state == "failed") }

    pub fn reference(&self, id: &str) -> Option<&Map<String, Value>> {
        if let Some(u) = self.uploads.get(id).filter(|u| u.request.is_none()) {
            return Some(&u.reference);
        }
        if let Some(u) = self.uploads.values().find(|u| u.request.is_none() && u.reference.get("attachment_id").and_then(Value::as_str) == Some(id)) {
            return Some(&u.reference);
        }
        self.shown.get(id)
    }

    /// `Attach`: encrypts the bytes and asks for the upload.
    pub fn attach(&mut self, local_id: &str, name: &str, media_type: &str, bytes: &Bytes, entropy: &mut dyn trommi_core::crypto::Entropy) -> Result<Send, NotDone> {
        if local_id.is_empty() || local_id.len() > 64 || self.uploads.contains_key(local_id) {
            return Err(Text::new("file.bad_id", "That file is already being attached."));
        }
        if bytes.0.len() > ATTACH_MAX {
            return Err(Text::new("file.too_large", "That file is too large (60 MB at most)."));
        }
        let (stored, Sealed { file, plain_len, .. }) =
            trommi_core::files::encrypt_file(&bytes.0, entropy).map_err(|_| Text::new("file.not_sealed", "That file could not be encrypted."))?;
        let attachment_id = hex(file.file_id.as_bytes());
        let mut reference = Map::new();
        reference.insert("attachment_id".into(), json!(attachment_id));
        reference.insert("file_key".into(), json!(b64(file.file_key.expose())));
        reference.insert("sha256".into(), json!(b64(file.sha256.as_bytes())));
        reference.insert("file_name".into(), json!(if name.trim().is_empty() { "file" } else { name.trim() }));
        reference.insert("media_type".into(), json!(if media_type.is_empty() { "application/octet-stream" } else { media_type }));
        reference.insert("total_size".into(), json!(plain_len));
        let mut view = view_of(&reference);
        view.state = "uploading".into();
        view.percent = Some(0);
        view.loaded = true;
        self.states.insert(local_id.to_string(), view);
        self.keep_plain(&attachment_id, bytes.clone());
        self.uploads.insert(local_id.to_string(), Upload { reference, sha256: *file.sha256.as_bytes(), request: None });
        let file_id = file.file_id;
        Ok(Send {
            build: Box::new(move |site| Codec.request(site, &Route::PutFile { file: &file_id, bytes: &stored })),
            keep: None,
            pending: PendingAsk::Put { local_id: local_id.to_string() },
        })
    }

    /// `Attach` in the demo room: no hub; the file is ready at once under its local id, its bytes kept here.
    pub fn demo_attach(&mut self, local_id: &str, name: &str, media_type: &str, bytes: &Bytes) {
        let mut reference = Map::new();
        reference.insert("attachment_id".into(), json!(local_id));
        reference.insert("file_name".into(), json!(if name.trim().is_empty() { "file" } else { name.trim() }));
        reference.insert("media_type".into(), json!(if media_type.is_empty() { "application/octet-stream" } else { media_type }));
        reference.insert("total_size".into(), json!(bytes.0.len()));
        let mut view = view_of(&reference);
        view.state = "ready".into();
        view.attachment_id = Some(local_id.to_string());
        self.states.insert(local_id.to_string(), view);
        self.keep_plain(local_id, bytes.clone());
        self.uploads.insert(local_id.to_string(), Upload { reference, sha256: [0; 32], request: None });
    }

    /// `OpenFile` in the demo room: from memory, or the demo file's own address (its reference's `url`): the
    /// request to make, and what to hand over when it comes.
    pub fn demo_open(&mut self, attachment_id: &str, save: bool, out: &mut Vec<Effect>) -> Option<(String, DemoOpen)> {
        let reference = self.reference(attachment_id).cloned()?;
        let id = reference.get("attachment_id").and_then(Value::as_str).unwrap_or(attachment_id).to_string();
        if let Some((_, bytes)) = self.plain.iter().find(|(k, _)| *k == id) {
            let bytes = bytes.clone();
            self.hand(&id, &reference, save, bytes, out);
            return None;
        }
        let url = reference.get("url").and_then(Value::as_str)?.to_string();
        Some((url, DemoOpen { id, reference, save }))
    }

    /// The demo file's bytes came.
    pub fn demo_opened(&mut self, open: DemoOpen, bytes: Bytes, out: &mut Vec<Effect>) {
        self.hand(&open.id, &open.reference, open.save, bytes, out);
    }

    /// `OpenFile`: the bytes from memory at once, or a download.
    pub fn open(&mut self, attachment_id: &str, save: bool, out: &mut Vec<Effect>) -> Result<Option<Send>, NotDone> {
        let reference = self.reference(attachment_id).cloned().ok_or_else(|| Text::new("file.unknown", "That file is not here."))?;
        let id = reference.get("attachment_id").and_then(Value::as_str).unwrap_or(attachment_id).to_string();
        if let Some((_, bytes)) = self.plain.iter().find(|(k, _)| *k == id) {
            let bytes = bytes.clone();
            self.hand(&id, &reference, save, bytes, out);
            return Ok(None);
        }
        if self.asked.values().any(|a| matches!(a, Asked::Get { attachment_id, save: s, .. } if *attachment_id == id && *s == save)) {
            return Ok(None);
        }
        let file = file_ref(&reference).ok_or_else(|| Text::new("file.unknown", "That file is not here."))?;
        let mut view = view_of(&reference);
        view.state = "loading".into();
        self.states.insert(id.clone(), view);
        let file_id = file.file_id;
        Ok(Some(Send {
            build: Box::new(move |site| Codec.request(site, &Route::GetFile { file: &file_id, range: None })),
            keep: None,
            pending: PendingAsk::Get { attachment_id: id, save },
        }))
    }

    /// `Share`: a new link for a file the views show.
    pub fn share(&mut self, attachment_id: &str, expires_ms: Option<u64>, now: u64, entropy: &mut dyn trommi_core::crypto::Entropy) -> Result<Send, NotDone> {
        let reference = self.reference(attachment_id).cloned().ok_or_else(|| Text::new("file.unknown", "That file is not here."))?;
        let file = file_ref(&reference).ok_or_else(|| Text::new("file.unknown", "That file is not here."))?;
        let link = trommi_core::files::ShareLink::create(SHARE_APP, &file, entropy).map_err(|_| Text::new("file.not_shared", "No link could be made."))?;
        let expires_at = expires_ms.unwrap_or(now + SHARE_DAYS_MS).min(now + SHARE_DAYS_MS);
        let share = link.share_id;
        let secret_hash = *link.secret_hash().map_err(|_| Text::new("file.not_shared", "No link could be made."))?.as_bytes();
        let text = String::from_utf8(link.to_text().expose().to_vec()).unwrap_or_default();
        let file_id = file.file_id;
        let id = hex(file_id.as_bytes());
        Ok(Send {
            build: Box::new(move |site| Codec.request(site, &Route::PostShare { share: &share, secret_hash: &secret_hash, file: &file_id, expires_at })),
            keep: None,
            pending: PendingAsk::Share { attachment_id: id, share_id: hex(share.as_bytes()), link: text },
        })
    }

    /// `RevokeShare`.
    pub fn unshare(&mut self, share_id: &str) -> Result<Send, NotDone> {
        let share = unhex16(share_id).map(ShareId::new).ok_or_else(|| Text::new("file.unknown_share", "That link is not here."))?;
        Ok(Send {
            build: Box::new(move |site| Codec.request(site, &Route::DeleteShare { share: &share })),
            keep: Some(format!("unshare/{share_id}")),
            pending: PendingAsk::Unshare { share_id: share_id.to_string() },
        })
    }

    /// The request of `send` went out under `id`.
    pub fn sent(&mut self, id: IoId, pending: PendingAskHandle, expect: Expect) {
        let asked = match pending.0 {
            PendingAsk::Put { local_id } => {
                if let Some(u) = self.uploads.get_mut(&local_id) {
                    u.request = Some(id);
                }
                Asked::Put { local_id, expect }
            }
            PendingAsk::Get { attachment_id, save } => Asked::Get { attachment_id, save, expect },
            PendingAsk::Share { attachment_id, share_id, link } => Asked::Share { attachment_id, share_id, link, expect },
            PendingAsk::Unshare { share_id } => Asked::Unshare { share_id, expect },
        };
        self.asked.insert(id, asked);
    }

    /// The request could not be sent (the engine refused it): what it changed goes back.
    pub fn not_sent(&mut self, pending: PendingAskHandle) {
        match pending.0 {
            PendingAsk::Put { local_id } => self.failed(&local_id),
            PendingAsk::Get { attachment_id, .. } => {
                self.states.remove(&attachment_id);
            }
            _ => {}
        }
    }

    fn failed(&mut self, key: &str) {
        if let Some(v) = self.states.get_mut(key) {
            v.state = "failed".into();
            v.percent = None;
        }
    }

    /// `CancelFile`: the requests of that file to cancel.
    pub fn cancel(&mut self, file_id: &str) -> Vec<IoId> {
        let mut gone = Vec::new();
        self.asked.retain(|id, a| {
            let mine = match a {
                Asked::Put { local_id, .. } => local_id == file_id,
                Asked::Get { attachment_id, .. } => attachment_id == file_id,
                _ => false,
            };
            if mine {
                gone.push(*id);
            }
            !mine
        });
        if self.uploads.get(file_id).is_some_and(|u| u.request.is_some()) {
            self.uploads.remove(file_id);
            self.states.remove(file_id);
        } else if self.states.get(file_id).is_some_and(|v| v.state == "loading") {
            self.states.remove(file_id);
        }
        gone
    }

    /// Whether an answered request is one of the files'.
    pub fn owns(&self, id: IoId) -> bool {
        self.asked.contains_key(&id)
    }

    /// How far a transfer is (`Event::Progress`).
    pub fn progress(&mut self, id: IoId, done: u64, total: u64) {
        let key = match self.asked.get(&id) {
            Some(Asked::Put { local_id, .. }) => local_id.clone(),
            Some(Asked::Get { attachment_id, .. }) => attachment_id.clone(),
            _ => return,
        };
        if let Some(v) = self.states.get_mut(&key) {
            v.percent = Some((done.min(total) * 100).checked_div(total).unwrap_or(0) as u8);
        }
    }

    /// The hub answered (or the engine gave up): the file's state, its bytes for the shell, or a toast.
    pub fn answered(&mut self, id: IoId, result: &Result<crate::app::io::HttpResponse, Failure>, out: &mut Vec<Effect>) -> Option<Text> {
        let asked = self.asked.remove(&id)?;
        let answer = |expect: &Expect| match result {
            Ok(response) => Codec.answer(expect, response),
            Err(failure) => Err(failure.clone()),
        };
        match asked {
            Asked::Put { local_id, expect } => {
                let upload = self.uploads.get_mut(&local_id)?;
                match answer(&expect) {
                    Ok(Answer::FileStored { sha256, .. }) if sha256 == upload.sha256 => {
                        upload.request = None;
                        let attachment_id = upload.reference.get("attachment_id").and_then(Value::as_str).map(String::from);
                        if let Some(v) = self.states.get_mut(&local_id) {
                            v.state = "ready".into();
                            v.percent = None;
                            v.attachment_id = attachment_id;
                        }
                        None
                    }
                    other => {
                        self.uploads.remove(&local_id);
                        self.failed(&local_id);
                        Some(problem(other.err().as_ref(), "file.upload_failed", "The file was not uploaded."))
                    }
                }
            }
            Asked::Get { attachment_id, save, expect } => {
                let reference = self.reference(&attachment_id).cloned();
                match (answer(&expect), reference.as_ref().and_then(file_ref)) {
                    (Ok(Answer::File(bytes)), Some(file)) => match trommi_core::files::decrypt_file(&file, &bytes.bytes) {
                        Ok(plain) => {
                            let reference = reference.unwrap_or_default();
                            self.hand(&attachment_id, &reference, save, Bytes(plain), out);
                            None
                        }
                        Err(_) => {
                            self.failed(&attachment_id);
                            Some(Text::new("file.broken", "That file does not open: it is not the one that was sent."))
                        }
                    },
                    (other, _) => {
                        self.failed(&attachment_id);
                        Some(problem(other.err().as_ref(), "file.download_failed", "The file could not be fetched."))
                    }
                }
            }
            Asked::Share { attachment_id, share_id, link, expect } => match answer(&expect) {
                Ok(Answer::Shared { expires_at }) => {
                    self.shares.entry(attachment_id).or_default().push(FileShareView { share_id, link, expires_ms: expires_at });
                    None
                }
                other => Some(problem(other.err().as_ref(), "file.share_failed", "No link was made.")),
            },
            Asked::Unshare { share_id, expect } => {
                match answer(&expect) {
                    Ok(_) | Err(Failure::Refused { status: 404, .. }) => {
                        for list in self.shares.values_mut() {
                            list.retain(|s| s.share_id != share_id);
                        }
                        self.shares.retain(|_, l| !l.is_empty());
                        None
                    }
                    other => Some(problem(other.err().as_ref(), "file.unshare_failed", "The link was not ended.")),
                }
            }
        }
    }

    /// A kept request (an ended link) answered after a restart: its share is gone either way.
    pub fn kept_answered(&mut self, keep: &str) {
        if let Some(share_id) = keep.strip_prefix("unshare/") {
            for list in self.shares.values_mut() {
                list.retain(|s| s.share_id != share_id);
            }
        }
    }

    fn hand(&mut self, id: &str, reference: &Map<String, Value>, save: bool, bytes: Bytes, out: &mut Vec<Effect>) {
        let view = view_of(reference);
        if save {
            out.push(Effect::SaveFile { name: view.name.clone(), media_type: view.media_type.clone(), bytes });
            self.states.remove(id).filter(|v| v.state != "loading").map(|v| self.states.insert(id.to_string(), v));
        } else {
            out.push(Effect::FileReady { file_id: id.to_string(), media_type: view.media_type.clone(), bytes });
            let mut view = view;
            view.loaded = true;
            self.states.insert(id.to_string(), view);
        }
    }

    fn keep_plain(&mut self, id: &str, bytes: Bytes) {
        self.plain.retain(|(k, _)| k != id);
        if self.plain.len() >= KEPT_PLAIN {
            self.plain.remove(0);
        }
        self.plain.push((id.to_string(), bytes));
    }
}

/// A demo file asked for by its address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoOpen {
    id: String,
    reference: Map<String, Value>,
    save: bool,
}

/// A send's pending part, handed back with the request's id ([`Files::sent`]).
pub struct PendingAskHandle(PendingAsk);

impl Send {
    /// Splits the send into what the engine sends, its name to keep it by, and what [`Files::sent`] takes back
    /// with the cell that holds what the route expects once it was built.
    pub fn split(self) -> (Build, Option<String>, PendingAskHandle, std::rc::Rc<std::cell::RefCell<Option<Expect>>>) {
        let cell = std::rc::Rc::new(std::cell::RefCell::new(None));
        let put = cell.clone();
        let build = self.build;
        let build: Build = Box::new(move |site| {
            let (request, expect) = build(site)?;
            *put.borrow_mut() = Some(expect);
            Ok(request)
        });
        (build, self.keep, PendingAskHandle(self.pending), cell)
    }
}

fn problem(failure: Option<&Failure>, key: &str, en: &str) -> Text {
    match failure {
        Some(Failure::Offline | Failure::NotNow { .. }) => Text::new("file.offline", "The hub cannot be reached. Try again."),
        Some(Failure::Refused { status: 413, .. }) => Text::new("file.too_large", "That file is too large (60 MB at most)."),
        Some(Failure::Refused { code: Some(code), .. }) if code == "quota" || code == "over-quota" => Text::new("file.quota", "The room's storage is full."),
        _ => Text::new(key, en),
    }
}
