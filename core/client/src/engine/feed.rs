//! From the engine's reports to the board model: what the protocol core took, in the model's own words. Every
//! check was the core's; nothing is decided here.

use super::sync::Report;
use super::Device;
use crate::model::change::Change;
use crate::model::groups::{fingerprint_of, CommitIn, GroupIn, RolesIn, SessionOfGroup};
use crate::model::received as rx;
use crate::model::Model;
use crate::presence::PresenceEvent;
use trommi_core::device::{Confirmation, EnvelopeOutcome, Processed, Received, ReceivedEnvelope};
use trommi_core::envelope::{Bind, ObjectState, ObjectType, Subject, Timeline, Verdict};
use trommi_core::mls::profile::GroupKind;
use trommi_core::mls::rules::{CommitFacts, ContextChange};

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

fn hex_or_none(bytes: &[u8]) -> Option<String> {
    (!bytes.iter().all(|b| *b == 0)).then(|| hex(bytes))
}

fn state_name(state: ObjectState) -> &'static str {
    match state {
        ObjectState::Open => "open",
        ObjectState::Answered => "answered",
        ObjectState::Closed => "closed",
    }
}

fn type_name(object_type: ObjectType) -> &'static str {
    match object_type {
        ObjectType::Card => "card",
        ObjectType::Note => "note",
        ObjectType::Request => "request",
        ObjectType::Artifact => "artifact",
    }
}

/// One received envelope as the model reads it.
pub fn received_of(r: &ReceivedEnvelope) -> rx::Received {
    let h = &r.header;
    let (kind, timeline) = match &h.subject {
        Subject::Item(timeline) => {
            let (kind, id) = match timeline {
                Timeline::SessionChat(session) => (rx::TimelineKind::SessionChat, hex(session.as_bytes())),
                Timeline::CardChat(card) => (rx::TimelineKind::CardChat, hex(card.as_bytes())),
                Timeline::Board(board) => (rx::TimelineKind::Board, hex(board.as_bytes())),
            };
            (rx::EnvelopeKind::Item, Some(rx::TimelineRef { kind, id }))
        }
        Subject::Version(_) => (rx::EnvelopeKind::Version, None),
        Subject::Answer(_) => (rx::EnvelopeKind::Answer, None),
        Subject::Request(_) => (rx::EnvelopeKind::Request, None),
        Subject::Verdict(_) => (rx::EnvelopeKind::Verdict, None),
        Subject::Register(_) => (rx::EnvelopeKind::Register, None),
        Subject::TakeBack(_) => (rx::EnvelopeKind::TakeBack, None),
        Subject::Reserved { .. } => (rx::EnvelopeKind::Reserved, None),
    };
    rx::Received {
        change: r.change,
        hash: hex(r.envelope_hash.as_bytes()),
        outcome: match r.outcome {
            EnvelopeOutcome::Applied => rx::Outcome::Applied,
            EnvelopeOutcome::Chained => rx::Outcome::Chained,
            EnvelopeOutcome::Void => rx::Outcome::Void,
            EnvelopeOutcome::Provisional => rx::Outcome::Provisional,
            EnvelopeOutcome::Refused => rx::Outcome::Refused,
        },
        code: r.code.as_ref().map(|c| c.code().to_string()),
        kind,
        sender: hex(h.sender.as_bytes()),
        seq: h.seq,
        recipient: Some(h.recipient).filter(|d| !d.is_zero()).map(|d| hex(d.as_bytes())),
        time: h.time,
        session_id: h.group.session_id().map(|s| hex(s.as_bytes())),
        timeline,
        object: h.subject.object().map(|o| rx::ObjectHead {
            object_id: hex(o.object_id.as_bytes()),
            object_type: type_name(o.object_type).to_string(),
            object_state: state_name(o.state).to_string(),
            urgency: o.urgency.name().to_string(),
            answered_at: o.answered_at,
            object_ref: hex_or_none(o.object_ref.as_bytes()),
        }),
        object_after: r.object_after.as_ref().map(|(_, a)| rx::ObjectAfter {
            owner: hex(a.owner.as_bytes()),
            object_state: state_name(a.state).to_string(),
            current: hex(a.current.as_bytes()),
        }),
        bind: r.body.as_ref().and_then(|body| match body.bind() {
            Bind::None => None,
            Bind::Answer(a) => Some(rx::Bind::Answer {
                version_hash: Some(hex(a.version_hash.as_bytes())),
                choices: a.choices.iter().map(|c| String::from_utf8_lossy(c).into_owned()).collect(),
            }),
            Bind::TakeBack(t) => Some(rx::Bind::TakeBack {
                previous_hash: Some(hex(t.previous_hash.as_bytes())),
            }),
            Bind::Request(q) => Some(rx::Bind::Request { expires_at: q.expires_at }),
            Bind::Verdict(v) => Some(rx::Bind::Verdict { allow: v.verdict == Verdict::Allow }),
        }),
        register: r.register.as_ref().map(|x| rx::RegisterTaken { name: x.name.clone(), current: x.current }),
        payload: r.body.as_ref().map(|body| String::from_utf8_lossy(body.payload()).into_owned()),
        shown: r
            .shown
            .iter()
            .map(|shown| rx::Shown { name: shown.name.clone(), now: shown.now.as_deref().map(|now| Box::new(received_of(now))) })
            .collect(),
    }
}

fn commit_in(facts: &CommitFacts) -> CommitIn {
    CommitIn {
        group: hex(facts.group.as_bytes()),
        room: facts.group.is_room(),
        epoch: facts.epoch,
        adds: facts.adds.iter().map(|d| hex(d.as_bytes())).collect(),
        removes: facts.removes.iter().map(|d| hex(d.as_bytes())).collect(),
        external: facts.external,
        committer: hex(facts.committer.as_bytes()),
        agents: match &facts.context {
            ContextChange::To(GroupKind::Room(room)) => room.agents.iter().map(|d| hex(d.as_bytes())).collect(),
            _ => Vec::new(),
        },
        time: facts.note.as_ref().map(|note| note.time),
    }
}

/// The board model of the App, and what the reports since the last projection touched.
#[derive(Default)]
pub struct Board {
    pub model: Model,
    change: Change,
    /// The hub's first list of this run has come (or was cut short): nothing is held for it any more.
    listed: bool,
    /// The Desk is complete: the hub's list and every listed card's version and first version were read.
    pub desk_complete: bool,
}

impl Board {
    /// A board for the device: everything open is held until the hub's first list.
    pub fn new(device: &Device) -> Board {
        let mut board = Board::default();
        board.model.my_device_id = Some(hex(device.id().as_bytes()));
        board.model.reading = true;
        board.model.set_desk_hold(true);
        board
    }

    /// One report of the engine into the model.
    pub fn take(&mut self, report: &Report, device: &Device, now: u64) {
        let (m, c) = (&mut self.model, &mut self.change);
        match report {
            Report::Envelope { received, .. } => {
                if received.outcome == EnvelopeOutcome::Refused {
                    return;
                }
                // `heads` is the devices' own bookkeeping (9.0.7): nothing of it is on the board
                if matches!(received.header.subject, Subject::Register(_)) && received.register.as_ref().is_some_and(|r| r.name == "heads") {
                    return;
                }
                let _ = matches!(received.provisional, Some(Confirmation::Confirmed));
                m.apply_envelope(&received_of(received), c, now);
            }
            Report::Log { change, group, processed } => match &**processed {
                Processed::Commit { facts, removed, .. } => {
                    let by_recovery = facts.external && facts.group.is_room();
                    m.apply_commit(&commit_in(facts), c, by_recovery, *removed && group.is_room(), now);
                    refresh_cut_registers(m, c, device, facts, now);
                }
                Processed::Observed(facts) => {
                    m.apply_commit(&commit_in(facts), c, false, false, now);
                    refresh_cut_registers(m, c, device, facts, now);
                },
                Processed::Message(Received::WorkTrail { from, turn, number, time, step }) => {
                    if let Some(session) = group.session_id() {
                        m.apply_work_trail(&hex(session.as_bytes()), &hex(from.as_bytes()), &hex(turn.as_bytes()), u64::from(*number), *time, step, *change, c);
                    }
                }
                _ => {}
            },
            Report::Groups => {
                let groups: Vec<GroupIn> = device
                    .groups()
                    .unwrap_or_default()
                    .iter()
                    .map(|g| GroupIn {
                        group: hex(g.group.as_bytes()),
                        session: g.session.as_ref().map(|s| SessionOfGroup {
                            session_id: hex(s.session_id.as_bytes()),
                            parent: hex_or_none(s.parent.as_bytes()),
                        }),
                        leaves: g.leaves.iter().map(|d| hex(d.as_bytes())).collect(),
                        disallowed: g.disallowed.len(),
                        archived: g.archived,
                        epoch: g.epoch,
                    })
                    .collect();
                let roles = device.room_history().map(|history| {
                    let state = history.newest();
                    RolesIn {
                        epoch: state.epoch,
                        humans: state.humans.iter().map(|d| hex(d.as_bytes())).collect(),
                        agents: state.room.agents.iter().map(|d| hex(d.as_bytes())).collect(),
                        recovery_fingerprint: Some(fingerprint_of(&hex(&state.room.recovery_hpke_key))),
                    }
                });
                m.apply_groups(&groups, roles.as_ref(), c);
            }
            Report::Desk { at, open, truncated } => {
                // a list cut short decides nothing: the hub's order brings the rest
                if !*truncated {
                    m.set_desk_listing(Some((*at, open.iter().map(|(id, version)| (hex(id.as_bytes()), *version)).collect())));
                }
                if !self.listed {
                    self.listed = true;
                    m.set_desk_hold(false);
                }
            }
            Report::Counts { at, settled } => {
                let settled = settled.iter().filter_map(|(group, n)| group.session_id().map(|s| (hex(s.as_bytes()), *n))).collect();
                m.set_hub_settled(*at, settled);
            }
            Report::DeskComplete => {
                self.desk_complete = true;
                m.reading = false;
            }
            Report::StreamOpened => m.stream_opened(),
            Report::Presence(p) => {
                let event = PresenceEvent {
                    device: hex(p.device.as_bytes()),
                    online: p.online,
                    lost: p.lost,
                    hears: p.hears,
                    working: p.working,
                    last_call_at: p.last_call_at,
                    offline_since: None,
                    last_seen_ms: p.last_seen_ms,
                };
                m.apply_presence_event(&event, c, now);
            }
            Report::Here => m.apply_here(c, now),
            Report::Relay(received) => {
                match &**received {
                    Received::StrokePiece { from, board, piece } => {
                        m.apply_stroke_piece(&hex(from.as_bytes()), &hex(board.as_bytes()), piece, now, c);
                    }
                    Received::AnswerPreview { from, turn, number, preview } => {
                        m.apply_answer_preview(&hex(from.as_bytes()), &hex(turn.as_bytes()), *number, preview, now, c);
                    }
                    _ => {}
                }
            }
            Report::Founded(_) | Report::LinkJoin(_) | Report::InviteRequest { .. } | Report::FileEvicted { .. } | Report::Alert(_) | Report::Connection(_) | Report::Accepted { .. } | Report::Refused { .. } | Report::Blocked(_) | Report::SessionsJoined { .. } | Report::RoomJoin(_) | Report::Answer(_) | Report::Stopped(_) | Report::Command(_) | Report::Lease(_) | Report::Resealed { .. } => {}
        }
    }

    /// A timeline's window as a page read from the hub left it (`app::past`): the oldest change read, and whether
    /// the hub holds more before it.
    pub fn window(&mut self, key: &str, down_to: Option<u64>, more: bool) {
        self.model.put_items(key, Vec::new(), down_to, Some(more), &mut self.change);
    }

    /// Projects the Desk from what the reports touched and hands back what changed; the model's next wake by the
    /// clock is `model.desk_wake`.
    pub fn finish(&mut self, now: u64) -> Change {
        self.model.project(&mut self.change, now);
        std::mem::take(&mut self.change)
    }
}

/// A Cut can invalidate a value already handed to the model, including a value read through its chain.
/// Ask the device for what stands now; do not rebuild register ordering in the projection.
fn refresh_cut_registers(model: &mut Model, change: &mut Change, device: &Device, facts: &CommitFacts, now: u64) {
    if facts.note.as_ref().is_none_or(|note| note.cuts.is_empty()) { return; }
    let session = facts.group.session_id().map(|id| hex(id.as_bytes()));
    let mut keys: Vec<String> = match &session {
        Some(id) => model.sessions.get(id).map(|s| s.registers.keys().cloned().collect()).unwrap_or_default(),
        None => model.human.iter().filter(|(_, r)| !r.pending).map(|(key, _)| key.clone()).collect(),
    };
    keys.extend(model.device_register_groups.iter().filter(|(_, group)| **group == session).map(|(id, _)| format!("device/{id}")));
    let names: Vec<String> = keys.iter().map(|key| crate::codec::register_name_of(key)).collect();
    let values = match device.shown_registers(&facts.group, &names) {
        Ok(values) => values,
        Err(_) => {
            model.push_alert(change, "storage", "the current register values could not be read after a Cut", None, None, "local", now);
            names.iter().map(|name| trommi_core::device::RegisterShown { name: name.clone(), of: None, now: None }).collect()
        }
    };
    for (key, value) in keys.into_iter().zip(values) {
        // Only the verified replacement list is applied. The remaining fields locate a deleted value.
        let template = rx::Received {
            change: 0, hash: String::new(), outcome: rx::Outcome::Applied, code: None,
            kind: rx::EnvelopeKind::Register,
            sender: key.strip_prefix("device/").unwrap_or("").to_string(), seq: 0,
            recipient: None, time: now, session_id: session.clone(), timeline: None,
            object: None, object_after: None, bind: None,
            register: None, payload: None,
            shown: vec![rx::Shown { name: value.name, now: value.now.as_deref().map(|r| Box::new(received_of(r))) }],
        };
        model.apply_register_changes(&template, change, now);
    }
}
