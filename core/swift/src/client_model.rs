//! The board model of the client core (client/src/model): the records every human device shows, built from what the
//! protocol core accepted. An app hands it what came (`apply_envelopes`, the groups, a Commit, presence, its own
//! sends) and takes, once per batch, the patch of what changed (`finish`): JSON in the web model's shape.

use crate::client::DeskListed;
use crate::{BindKind, EnvelopeKind, EnvelopeOutcome, TimelineKind};
use crate::{CommitSummary, CoreError, GroupSummary, ReceivedEnvelope, RoomRoles};
use std::sync::Mutex;
use trommi_client_core::model::change::Change;
use trommi_client_core::model::groups::{CommitIn, DeviceReport, GroupIn, RolesIn, SessionOfGroup};
use trommi_client_core::model::received as rx;
use trommi_client_core::model::Model;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn hex_or_none(bytes: &[u8]) -> Option<String> {
    if bytes.iter().all(|b| *b == 0) {
        None
    } else {
        Some(hex(bytes))
    }
}

/// The client core's view of one received envelope.
pub(crate) fn received_of(r: &ReceivedEnvelope) -> rx::Received {
    let h = &r.header;
    rx::Received {
        change: r.change,
        hash: hex(&r.envelope_hash),
        outcome: match r.outcome {
            EnvelopeOutcome::Applied => rx::Outcome::Applied,
            EnvelopeOutcome::Chained => rx::Outcome::Chained,
            EnvelopeOutcome::Void => rx::Outcome::Void,
            EnvelopeOutcome::Provisional => rx::Outcome::Provisional,
            EnvelopeOutcome::Refused => rx::Outcome::Refused,
        },
        code: r.code.map(|c| c.text().to_string()),
        kind: match h.kind {
            EnvelopeKind::Item => rx::EnvelopeKind::Item,
            EnvelopeKind::Version => rx::EnvelopeKind::Version,
            EnvelopeKind::Answer => rx::EnvelopeKind::Answer,
            EnvelopeKind::Request => rx::EnvelopeKind::Request,
            EnvelopeKind::Verdict => rx::EnvelopeKind::Verdict,
            EnvelopeKind::Register => rx::EnvelopeKind::Register,
            EnvelopeKind::TakeBack => rx::EnvelopeKind::TakeBack,
            EnvelopeKind::Reserved => rx::EnvelopeKind::Reserved,
        },
        sender: hex(&h.sender),
        seq: h.seq,
        recipient: h.recipient.as_deref().map(hex),
        time: h.time,
        session_id: h.session_id.as_deref().map(hex),
        timeline: h.timeline.as_ref().map(|t| rx::TimelineRef {
            kind: match t.kind {
                TimelineKind::SessionChat => rx::TimelineKind::SessionChat,
                TimelineKind::CardChat => rx::TimelineKind::CardChat,
                TimelineKind::Board => rx::TimelineKind::Board,
            },
            id: hex(&t.id),
        }),
        object: h.object.as_ref().map(|o| rx::ObjectHead {
            object_id: hex(&o.object_id),
            object_type: o.object_type.text().to_string(),
            object_state: o.object_state.text().to_string(),
            urgency: o.urgency.text().to_string(),
            answered_at: o.answered_at,
            object_ref: hex_or_none(&o.object_ref),
        }),
        object_after: r.object_after.as_ref().map(|a| rx::ObjectAfter {
            owner: hex(&a.owner),
            object_state: a.object_state.text().to_string(),
            current: hex(&a.current),
        }),
        bind: r.bind.as_ref().map(|b| match b.kind {
            BindKind::Answer => rx::Bind::Answer {
                version_hash: b.version_hash.as_deref().map(hex),
                choices: b.choices.clone(),
            },
            BindKind::TakeBack => rx::Bind::TakeBack {
                previous_hash: b.previous_hash.as_deref().map(hex),
            },
            BindKind::Request => rx::Bind::Request {
                expires_at: b.expires_at,
            },
            BindKind::Verdict => rx::Bind::Verdict { allow: b.allow },
        }),
        register: r.register.as_ref().map(|x| rx::RegisterTaken {
            name: x.name.clone(),
            current: x.current,
        }),
        payload: r
            .payload
            .as_ref()
            .map(|p| String::from_utf8_lossy(p).into_owned()),
    }
}

/// The board model of one device, kept by its app.
#[cfg_attr(feature = "uniffi", derive(uniffi::Object))]
pub struct ClientModel(Mutex<(Model, Change)>);

impl ClientModel {
    fn with<T>(
        &self,
        f: impl FnOnce(&mut Model, &mut Change) -> Result<T, CoreError>,
    ) -> Result<T, CoreError> {
        let mut g = self
            .0
            .lock()
            .map_err(|_| CoreError::internal("the model failed before"))?;
        let (m, c) = &mut *g;
        f(m, c)
    }
}

fn json_err(_: serde_json::Error) -> CoreError {
    CoreError::bad_format("not the JSON this call takes")
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl ClientModel {
    /// An empty model of the device `my_device_id` (hex; empty while not known).
    #[cfg_attr(feature = "uniffi", uniffi::constructor)]
    pub fn new(my_device_id: String) -> Self {
        let mut m = Model::new();
        if !my_device_id.is_empty() {
            m.my_device_id = Some(my_device_id);
        }
        Self(Mutex::new((m, Change::new())))
    }

    /// The device this model is of (hex), once it is known.
    pub fn set_me(&self, my_device_id: String) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.my_device_id = Some(my_device_id);
            Ok(())
        })
    }

    /// The model as an app's cache held it (the web model's snapshot JSON): records only, no echoes.
    pub fn load(&self, snapshot: String) -> Result<(), CoreError> {
        let v: serde_json::Value = serde_json::from_str(&snapshot).map_err(json_err)?;
        self.with(|m, _| m.load(&v).map_err(|e| CoreError::bad_format(&e)))
    }

    /// Envelopes as the core took them, in order. Returns, per envelope, whether it changed what the model shows
    /// (the code of a finding in `refused`), as JSON `[{applied, refused}]`.
    pub fn apply_envelopes(
        &self,
        received: Vec<ReceivedEnvelope>,
        now_ms: u64,
    ) -> Result<String, CoreError> {
        self.with(|m, c| {
            let out: Vec<serde_json::Value> = received
                .iter()
                .map(|r| {
                    let a = m.apply_envelope(&received_of(r), c, now_ms);
                    serde_json::json!({ "applied": a.applied, "refused": a.refused })
                })
                .collect();
            Ok(serde_json::Value::Array(out).to_string())
        })
    }

    /// Envelopes as the client core reads them (JSON, `client/src/model/received.rs` `Received`: ids as hex, the body
    /// as its JSON text): for an app whose own records of the core's answers are not this binding's (iOS), and for
    /// the replay of what an app stored. Returns as `apply_envelopes` does.
    pub fn apply_received_json(&self, received: String, now_ms: u64) -> Result<String, CoreError> {
        let list: Vec<rx::Received> = serde_json::from_str(&received).map_err(json_err)?;
        self.with(|m, c| {
            let out: Vec<serde_json::Value> = list
                .iter()
                .map(|r| {
                    let a = m.apply_envelope(r, c, now_ms);
                    serde_json::json!({ "applied": a.applied, "refused": a.refused })
                })
                .collect();
            Ok(serde_json::Value::Array(out).to_string())
        })
    }

    /// Every timeline's window is open from here on: every item header stays (an app that keeps them all).
    pub fn set_windows_open(&self, open: bool) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.windows_open = open;
            Ok(())
        })
    }

    /// The deeds on cards (hand back, What??, present) of a page of card Chats read out of the hub's order.
    pub fn apply_card_deeds(&self, received: Vec<ReceivedEnvelope>) -> Result<(), CoreError> {
        self.with(|m, c| {
            let list: Vec<rx::Received> = received.iter().map(received_of).collect();
            m.apply_card_deeds(&list, c);
            Ok(())
        })
    }

    /// The device's groups as the core holds them now, and the room's roles.
    pub fn apply_groups(
        &self,
        groups: Vec<GroupSummary>,
        roles: Option<RoomRoles>,
    ) -> Result<(), CoreError> {
        let groups: Vec<GroupIn> = groups
            .iter()
            .map(|g| GroupIn {
                group: hex(&g.group),
                session: g.session.as_ref().map(|s| SessionOfGroup {
                    session_id: hex(&s.session_id),
                    parent: hex_or_none(&s.parent),
                }),
                leaves: g.leaves.iter().map(|d| hex(d)).collect(),
                disallowed: g.disallowed.len(),
                archived: g.archived,
                epoch: g.epoch,
            })
            .collect();
        let roles = roles.map(|r| RolesIn {
            epoch: r.epoch,
            humans: r.humans.iter().map(|d| hex(d)).collect(),
            agents: r.agents.iter().map(|d| hex(d)).collect(),
            recovery_fingerprint: if r.recovery_hpke_key.is_empty() {
                None
            } else {
                Some(trommi_client_core::model::groups::fingerprint_of(&hex(
                    &r.recovery_hpke_key
                )))
            },
        });
        self.with(|m, c| {
            m.apply_groups(&groups, roles.as_ref(), c);
            Ok(())
        })
    }

    /// What a processed Commit did: `by_recovery` (a join with the recovery code into the room group), `removed_me`.
    pub fn apply_commit(
        &self,
        facts: CommitSummary,
        by_recovery: bool,
        removed_me: bool,
        now_ms: u64,
    ) -> Result<(), CoreError> {
        let f = CommitIn {
            group: hex(&facts.group),
            room: facts.group.len() == 32,
            epoch: facts.epoch,
            adds: facts.adds.iter().map(|d| hex(d)).collect(),
            removes: facts.removes.iter().map(|d| hex(d)).collect(),
            external: facts.external,
            committer: hex(&facts.committer),
            agents: facts
                .agents
                .unwrap_or_default()
                .iter()
                .map(|d| hex(d))
                .collect(),
            time: facts.time,
        };
        self.with(|m, c| {
            m.apply_commit(&f, c, by_recovery, removed_me, now_ms);
            Ok(())
        })
    }

    /// The live stream opened (again): the hub's list of who is here comes first, ended by its `here`.
    pub fn stream_opened(&self) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.stream_opened();
            Ok(())
        })
    }

    /// One `presence` event of the live stream, JSON as the hub sends it (`{device, online, lost?, hears?, working?,
    /// last_call_at?, offline_since?}`, the device as base64url or hex). Returns whether an agent device came online.
    pub fn apply_presence_event(&self, event: String, now_ms: u64) -> Result<bool, CoreError> {
        let e: trommi_client_core::presence::PresenceEvent = serde_json::from_str(&event).map_err(json_err)?;
        self.with(|m, c| Ok(m.apply_presence_event(&e, c, now_ms)))
    }

    /// The `here` that ends a new stream's list of who is here: every other device shown online is offline now.
    pub fn apply_here(&self, now_ms: u64) -> Result<(), CoreError> {
        self.with(|m, c| {
            m.apply_here(c, now_ms);
            Ok(())
        })
    }

    /// The seat rule's look (client/src/seats.rs): JSON `{agents, seated, coming, founding_pending}` (hex ids) as the
    /// app knows it; who is connected is the model's. Answers `{recheck_at}` (nothing to do; look again then) or
    /// `{candidates}` (read the hub's list of groups, then `seat_after_hub`).
    pub fn seat_plan(&self, known: String, now_ms: u64) -> Result<String, CoreError> {
        let k: trommi_client_core::seats::Known = serde_json::from_str(&known).map_err(json_err)?;
        self.with(|m, _| Ok(serde_json::to_string(&m.seat_plan(k, now_ms)).unwrap_or_default()))
    }
    /// After the hub's list: the one agent to found now, of `candidates` (JSON list of hex), none of which the hub lists
    /// in a session group (`hub_seated`, JSON list of hex).
    pub fn seat_after_hub(&self, candidates: String, hub_seated: String) -> Result<Option<String>, CoreError> {
        let c: Vec<String> = serde_json::from_str(&candidates).map_err(json_err)?;
        let h: std::collections::BTreeSet<String> = serde_json::from_str(&hub_seated).map_err(json_err)?;
        self.with(|m, _| Ok(m.seats.after_hub(&c, &h)))
    }
    /// The founding of `agent`'s main session went out.
    pub fn seat_founded(&self, agent: String) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.seats.founded(&agent);
            Ok(())
        })
    }
    /// How long an agent waits without a session before this device founds it (tests; default 30 s).
    pub fn set_seat_grace(&self, ms: u64) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.seats.grace_ms = ms;
            Ok(())
        })
    }
    /// Self-join (client/src/selfjoin.rs): the agents' asks to act on, JSON `[{session_id, nonce, ask, refused}]`.
    /// `answered`: the nonces this device answered; `pending`: the accounts of its vouched links not through yet.
    pub fn self_join_asks(&self, answered: String, pending: String) -> Result<String, CoreError> {
        let a: std::collections::BTreeSet<String> = serde_json::from_str(&answered).map_err(json_err)?;
        let p: Vec<String> = serde_json::from_str(&pending).map_err(json_err)?;
        self.with(|m, _| Ok(serde_json::to_string(&trommi_client_core::selfjoin::asks(m, &a, &p)).unwrap_or_default()))
    }
    /// Whether a vouched Request is let in: none while the ask names no hash yet.
    pub fn self_join_verdict(&self, session_id: String, nonce: String, request_hash: String) -> Result<Option<bool>, CoreError> {
        self.with(|m, _| Ok(trommi_client_core::selfjoin::verdict(m, &session_id, &nonce, &request_hash)))
    }
    /// The devices of archived self-joined sessions still in the room, JSON list of hex.
    pub fn self_join_retire(&self) -> Result<String, CoreError> {
        self.with(|m, _| Ok(serde_json::to_string(&trommi_client_core::selfjoin::to_retire(m)).unwrap_or_default()))
    }
    /// The asks as rows of the sidebar, JSON `[{nonce, session_id, state, folder, name, host, kind, by}]`; `invites`:
    /// JSON `{invite_id: state}` of the invites this device knows.
    pub fn join_rows(&self, invites: String) -> Result<String, CoreError> {
        let i: std::collections::BTreeMap<String, String> = serde_json::from_str(&invites).map_err(json_err)?;
        self.with(|m, _| Ok(serde_json::to_string(&trommi_client_core::selfjoin::join_rows(m, &i)).unwrap_or_default()))
    }

    /// What the hub says of devices: JSON `[{device_id, is_online, offline_since, link?}]`.
    pub fn apply_presence(&self, reports: String, now_ms: u64) -> Result<(), CoreError> {
        let reports: Vec<DeviceReport> = serde_json::from_str(&reports).map_err(json_err)?;
        self.with(|m, c| {
            m.apply_presence(&reports, c, now_ms);
            Ok(())
        })
    }
}

#[cfg_attr(feature = "uniffi", uniffi::export)]
impl ClientModel {
    /// One step of a running turn of the session `session_id` (hex): false for a step that is not readable.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_work_trail(
        &self,
        session_id: String,
        from: Vec<u8>,
        turn: Vec<u8>,
        number: u32,
        time: u64,
        payload: Vec<u8>,
        change_number: u64,
    ) -> Result<bool, CoreError> {
        self.with(|m, c| {
            Ok(m.apply_work_trail(
                &session_id,
                &hex(&from),
                &hex(&turn),
                number as u64,
                time,
                &payload,
                change_number,
                c,
            ))
        })
    }

    /// The points of a stroke still being drawn on the board `board`.
    pub fn apply_stroke_piece(
        &self,
        from: Vec<u8>,
        board: Vec<u8>,
        payload: Vec<u8>,
        now_ms: u64,
    ) -> Result<bool, CoreError> {
        self.with(|m, c| Ok(m.apply_stroke_piece(&hex(&from), &hex(&board), &payload, now_ms, c)))
    }

    /// A message or a board item of this device, shown at once: `content` its body's JSON in the model's form.
    pub fn echo_item(
        &self,
        local_id: String,
        timeline_key: String,
        content: String,
        recipient: Option<String>,
        object_id: Option<String>,
        now_ms: u64,
    ) -> Result<String, CoreError> {
        let content: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&content).map_err(json_err)?;
        self.with(|m, c| {
            Ok(serde_json::Value::Object(m.echo_timeline_item(
                &local_id,
                &timeline_key,
                content,
                recipient,
                object_id,
                now_ms,
                c,
            ))
            .to_string())
        })
    }

    /// Room registers of this device, shown at once: JSON `{key: value}`, null deletes.
    pub fn echo_registers(&self, local_id: String, values: String) -> Result<(), CoreError> {
        let values: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&values).map_err(json_err)?;
        self.with(|m, c| {
            m.echo_registers(&local_id, &values, c);
            Ok(())
        })
    }

    /// An answer of this device, shown at once (JSON, the model's answer fields). False without the card.
    pub fn echo_answer(
        &self,
        local_id: String,
        object_id: String,
        answer: String,
        object_state: String,
    ) -> Result<bool, CoreError> {
        let answer: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&answer).map_err(json_err)?;
        self.with(|m, c| Ok(m.echo_answer(&local_id, &object_id, answer, &object_state, c)))
    }

    /// A note or a new version of one, shown at once. Returns its key in the notes.
    pub fn echo_note(
        &self,
        local_id: String,
        object_id: Option<String>,
        fields: String,
        closed: bool,
    ) -> Result<String, CoreError> {
        let fields: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&fields).map_err(json_err)?;
        self.with(|m, c| Ok(m.echo_note(&local_id, object_id.as_deref(), &fields, closed, c)))
    }

    /// An echo's envelope is sealed: its hash (hex), its number in the sender's chain, a new note's object id.
    pub fn confirm_echo(
        &self,
        local_id: String,
        hash: String,
        seq: u64,
        object_id: Option<String>,
    ) -> Result<(), CoreError> {
        self.with(|m, c| {
            m.confirm_echo(&local_id, &hash, seq, object_id.as_deref(), c);
            Ok(())
        })
    }

    /// An echo is taken back: what it showed is as it was.
    pub fn rollback_echo(&self, local_id: String) -> Result<(), CoreError> {
        self.with(|m, c| {
            m.rollback_echo(&local_id, c);
            Ok(())
        })
    }

    /// The hub's Desk as last read: what it lists as open, at which change.
    pub fn set_desk_listing(&self, at: u64, listed: Vec<DeskListed>) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.set_desk_listing(Some((
                at,
                listed
                    .into_iter()
                    .map(|l| (l.object_id, l.version_change))
                    .collect(),
            )));
            Ok(())
        })
    }

    /// No list of the hub: the device's own view stands.
    pub fn forget_desk_listing(&self) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.set_desk_listing(None);
            Ok(())
        })
    }

    /// The app waits for the hub's first list of a run: every open card and request is held meanwhile.
    pub fn set_desk_hold(&self, hold: bool) -> Result<(), CoreError> {
        self.with(|m, _| {
            m.set_desk_hold(hold);
            Ok(())
        })
    }

    /// A timeline's window as the app reads it: opened, loaded down to `loaded_down_to` (0: nothing loaded), more
    /// before it.
    pub fn set_window(
        &self,
        timeline_key: String,
        open: bool,
        loaded_down_to: u64,
        has_more: bool,
    ) -> Result<(), CoreError> {
        self.with(|m, c| {
            let t = m.timeline_of(&timeline_key);
            t.window_open = open;
            t.loaded_down_to = if loaded_down_to == 0 {
                None
            } else {
                Some(loaded_down_to)
            };
            t.has_more = has_more;
            c.timelines.insert(timeline_key);
            Ok(())
        })
    }

    /// A page of one timeline read out of the hub's order: applied to the model (its window opened) or, `scratch`,
    /// to a model of its own. Returns the items it gave (JSON list).
    pub fn apply_page(
        &self,
        timeline_key: String,
        received: Vec<ReceivedEnvelope>,
        scratch: bool,
        now_ms: u64,
    ) -> Result<String, CoreError> {
        let list: Vec<rx::Received> = received.iter().map(received_of).collect();
        self.with(|m, c| {
            Ok(serde_json::Value::Array(
                m.apply_page(&timeline_key, &list, scratch, now_ms, c)
                    .into_iter()
                    .map(serde_json::Value::Object)
                    .collect(),
            )
            .to_string())
        })
    }

    /// Items (JSON list, the app's cache or a page) into a timeline's window where it holds none with a body; the
    /// window loaded down to `loaded_down_to`, more before it `has_more`, where given.
    pub fn put_items(
        &self,
        timeline_key: String,
        items: String,
        loaded_down_to: Option<u64>,
        has_more: Option<bool>,
    ) -> Result<(), CoreError> {
        let items: Vec<serde_json::Map<String, serde_json::Value>> =
            serde_json::from_str(&items).map_err(json_err)?;
        self.with(|m, c| {
            m.put_items(&timeline_key, items, loaded_down_to, has_more, c);
            Ok(())
        })
    }

    /// An alert of the app's own (`source` local).
    pub fn push_alert(
        &self,
        code: String,
        message: String,
        envelope_number: Option<u64>,
        sender_device_id: Option<String>,
        now_ms: u64,
    ) -> Result<String, CoreError> {
        self.with(|m, c| {
            Ok(m.push_alert(
                c,
                &code,
                &message,
                envelope_number,
                sender_device_id,
                "local",
                now_ms,
            )
            .alert_id)
        })
    }

    /// Something only a newer Trommi understands was met.
    pub fn note_newer(&self, what: String, envelope_number: Option<u64>) -> Result<(), CoreError> {
        self.with(|m, c| {
            m.note_newer(c, &what, envelope_number);
            Ok(())
        })
    }

    /// The end of a batch: the Desk projected at `now_ms`, and the patch of everything the batch changed (JSON, the
    /// web model's `ModelPatch`); "" when nothing changed.
    pub fn finish(&self, now_ms: u64) -> Result<String, CoreError> {
        self.with(|m, c| {
            m.project(c, now_ms);
            if c.is_empty() {
                return Ok(String::new());
            }
            let p = m.patch(c);
            *c = Change::new();
            Ok(p.to_string())
        })
    }

    /// The deeds on cards of a page of card Chats read out of the hub's order, as JSON (`Received` list).
    pub fn apply_card_deeds_json(&self, received: String) -> Result<(), CoreError> {
        let list: Vec<rx::Received> = serde_json::from_str(&received).map_err(json_err)?;
        self.with(|m, c| {
            m.apply_card_deeds(&list, c);
            Ok(())
        })
    }

    /// Every record is named changed: the next `finish` gives the whole model (a copy made anew).
    pub fn touch_all(&self) -> Result<(), CoreError> {
        self.with(|m, c| {
            m.touch_all(c);
            Ok(())
        })
    }

    /// Room groups and their sessions as JSON (client/src/model/groups.rs `GroupIn` list, and `RolesIn` or null):
    /// for an app whose records of the core's answers are not this binding's (iOS).
    pub fn apply_groups_json(&self, groups: String, roles: String) -> Result<(), CoreError> {
        let groups: Vec<GroupIn> = serde_json::from_str(&groups).map_err(json_err)?;
        let roles: Option<RolesIn> = serde_json::from_str(&roles).map_err(json_err)?;
        self.with(|m, c| {
            m.apply_groups(&groups, roles.as_ref(), c);
            Ok(())
        })
    }

    /// What a processed Commit did, as JSON (`CommitIn`).
    pub fn apply_commit_json(
        &self,
        facts: String,
        by_recovery: bool,
        removed_me: bool,
        now_ms: u64,
    ) -> Result<(), CoreError> {
        let f: CommitIn = serde_json::from_str(&facts).map_err(json_err)?;
        self.with(|m, c| {
            m.apply_commit(&f, c, by_recovery, removed_me, now_ms);
            Ok(())
        })
    }

    /// A page of one timeline as JSON (`Received` list), as `apply_page` does.
    pub fn apply_page_json(
        &self,
        timeline_key: String,
        received: String,
        scratch: bool,
        now_ms: u64,
    ) -> Result<String, CoreError> {
        let list: Vec<rx::Received> = serde_json::from_str(&received).map_err(json_err)?;
        self.with(|m, c| {
            Ok(serde_json::Value::Array(
                m.apply_page(&timeline_key, &list, scratch, now_ms, c)
                    .into_iter()
                    .map(serde_json::Value::Object)
                    .collect(),
            )
            .to_string())
        })
    }

    /// The whole model as JSON (the web model's snapshot): for a new copy.
    pub fn snapshot(&self) -> Result<String, CoreError> {
        self.with(|m, _| Ok(m.snapshot().to_string()))
    }

    /// The room is being read (`reading`) or not: names a session without a profile "Loading …" meanwhile.
    pub fn set_reading(&self, reading: bool) -> Result<(), CoreError> {
        self.with(|m, c| {
            if m.reading != reading {
                m.reading = reading;
                c.room = true;
            }
            Ok(())
        })
    }

    /// The session list every app shows (client/src/sessions.rs), as JSON: one entry per session, in order, with its
    /// name, online or gone, archived, parent. `reading`: the room is still being read.
    pub fn session_list(&self, reading: bool) -> Result<String, CoreError> {
        self.with(|m, _| Ok(serde_json::to_string(&trommi_client_core::sessions::session_list(m, reading)).unwrap_or_default()))
    }
}
