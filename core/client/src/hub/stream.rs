//! `GET /v1/stream?after=`: the live stream, from raw bytes to checked events (hub-api.md "Decided" 20).
//!
//! The shell passes the bytes as they arrive, cut anywhere; [`StreamReader`] is the one parser for every app. It
//! keeps the unfinished line and event of one connection and nothing else, and it is bounded: a line or an event
//! above [`CAP_EVENT`] ends the stream, as does anything the protocol does not allow. What it checks:
//!
//! - the framing of server-sent events: lines end in LF, CRLF or CR; `event:`, `data:` and `id:` fields, one
//!   space after the colon dropped; a comment or an unknown field is passed over; an empty line ends an event;
//! - an `envelope` or `log` event is the item of `/v1/changes` it is named after, its event id is its change
//!   number in decimal, and its number lies above the cursor the stream was opened at and above every number the
//!   stream brought before: a hub can neither replay a change nor go back;
//! - every other event's data is exactly its kind's shape; `ping` is swallowed, a name a later hub adds is passed
//!   over.
//!
//! The same bytes give the same events however they are cut. Events that were whole and good before a bad one
//! are still handed over ([`Fed`]); after a failure the reader takes nothing more, and the engine closes the
//! connection and resumes after the last change it took.

use super::items::change_item;
use super::json::{bad, Json, Read, MAX_NUMBER};
use super::limits::{CAP_EVENT, MAX_KEY_PACKAGE, MAX_MLS};
use super::refusal::CAP_REFUSAL;
use super::request::begin;
use super::types::{ChangeItem, Presence, Site, StreamEvent, WishRow, WishRowKind};
use super::{classify, Failure};
use crate::app::io::{Header, HttpRequest, HttpResponse};
use trommi_core::ids::{DeviceId, FileId, InviteId};

/// The request of an `OpenStream` effect. Its `max_answer` bounds the body of an answer that is no 200.
pub(crate) fn request(site: &Site<'_>, after: u64) -> Read<HttpRequest> {
    if after > MAX_NUMBER {
        return bad("the request: a cursor beyond any change number");
    }
    Ok(begin(site, "GET", "/v1/stream", true, CAP_REFUSAL)?
        .query("after", after)
        .header("accept", "text/event-stream".to_owned())
        .done())
}

/// What a chunk brought: the events it completed, in order, and why the stream is over if it is.
#[derive(Debug, PartialEq, Eq)]
pub struct Fed {
    pub events: Vec<StreamEvent>,
    pub failure: Option<Failure>,
}

impl Fed {
    /// For a caller that takes nothing of a chunk that held a bad event.
    pub fn into_result(self) -> Result<Vec<StreamEvent>, Failure> {
        match self.failure {
            None => Ok(self.events),
            Some(failure) => Err(failure),
        }
    }
}

#[derive(Debug)]
enum State {
    /// Events are read.
    Open,
    /// The answer was no 200: its body is collected (at most [`CAP_REFUSAL`]) and sorted when it ends.
    Refused {
        status: u16,
        headers: Vec<Header>,
        body: Vec<u8>,
    },
    /// Over: nothing more is taken.
    Failed,
}

/// The parser of one connection's stream. Make a new one per connection.
#[derive(Debug)]
pub struct StreamReader {
    /// No change at or below it may come: the cursor asked from, then the last change brought.
    floor: u64,
    /// Changes at or below it are checked and not handed over (a catch-up beside the stream took them).
    taken: u64,
    state: State,
    /// The line being read, without its end.
    line: Vec<u8>,
    /// The last byte was a CR: an LF right after it belongs to the same line end.
    after_cr: bool,
    /// The event being read.
    name: Vec<u8>,
    data: Vec<u8>,
    data_lines: usize,
    id: Option<Vec<u8>>,
    /// Bytes of the event being read, its unfinished line included.
    size: usize,
}

impl Default for StreamReader {
    /// A reader for a stream opened at the start (`after=0`).
    fn default() -> Self {
        Self::new(0)
    }
}

impl StreamReader {
    /// For a stream opened with `after`.
    pub fn new(after: u64) -> Self {
        Self {
            floor: after,
            taken: after,
            state: State::Open,
            line: Vec::new(),
            after_cr: false,
            name: Vec::new(),
            data: Vec::new(),
            data_lines: 0,
            id: None,
            size: 0,
        }
    }

    /// The last change the stream brought (or the cursor it was opened at): where the next one resumes.
    pub fn last_change(&self) -> u64 {
        self.floor
    }

    /// A catch-up beside the stream took everything up to `change`: the stream's own copies of those are still
    /// checked for their order, and not handed over a second time.
    pub fn caught_up_to(&mut self, change: u64) {
        self.taken = self.taken.max(change);
    }

    /// The answer's head (`StreamItem::Opened`). 200 with `content-type: text/event-stream`: events follow.
    /// Another status is not a failure yet: its body follows as chunks and [`ended`](Self::ended) sorts it.
    pub fn opened(&mut self, status: u16, headers: &[Header]) -> Result<(), Failure> {
        if status != 200 {
            self.state = State::Refused {
                status,
                headers: headers.to_vec(),
                body: Vec::new(),
            };
            return Ok(());
        }
        let stream = headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case("content-type"))
            .is_some_and(|h| {
                h.value
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("text/event-stream")
            });
        if !stream {
            self.state = State::Failed;
            return bad("not a stream of events");
        }
        Ok(())
    }

    /// The next bytes (`StreamItem::Chunk`), cut anywhere.
    pub fn chunk(&mut self, bytes: &[u8]) -> Fed {
        let mut events = Vec::new();
        let failure = match &mut self.state {
            State::Failed => Some(Failure::BadAnswer {
                why: "a stream that is over",
            }),
            State::Refused { body, .. } => {
                // (beyond the cap it is no refusal of this protocol anyway: the rest is dropped)
                let room = CAP_REFUSAL.saturating_add(1).saturating_sub(body.len());
                body.extend(bytes.iter().take(room));
                None
            }
            State::Open => self.read(bytes, &mut events).err(),
        };
        if failure.is_some() {
            self.fail();
        }
        Fed { events, failure }
    }

    /// The stream ended (`StreamItem::Ended`): the failure of an answer that was no 200, sorted as any answer's.
    /// `None` for a stream of events the hub ended (its token ran out, the hub restarts); an event cut short by
    /// the end is dropped, as server-sent events have it.
    pub fn ended(&mut self) -> Option<Failure> {
        match std::mem::replace(&mut self.state, State::Failed) {
            State::Refused {
                status,
                headers,
                body,
            } => {
                let response = HttpResponse {
                    status,
                    headers,
                    body: body.into(),
                };
                Some(classify(&response).err().unwrap_or(Failure::BadAnswer {
                    why: "a stream answered with a status that is no stream",
                }))
            }
            _ => {
                self.fail();
                None
            }
        }
    }

    fn fail(&mut self) {
        self.state = State::Failed;
        self.line = Vec::new();
        self.name = Vec::new();
        self.data = Vec::new();
        self.id = None;
    }

    fn read(&mut self, mut bytes: &[u8], events: &mut Vec<StreamEvent>) -> Read<()> {
        while !bytes.is_empty() {
            if self.after_cr {
                self.after_cr = false;
                if let [b'\n', rest @ ..] = bytes {
                    bytes = rest;
                    continue;
                }
            }
            let end = bytes.iter().position(|b| *b == b'\n' || *b == b'\r');
            let (part, rest) = match end {
                Some(end) => (&bytes[..end], &bytes[end + 1..]),
                None => (bytes, &[][..]),
            };
            self.size = self.size.saturating_add(part.len());
            if self.size > CAP_EVENT {
                return bad("an event larger than the protocol allows");
            }
            self.line.extend_from_slice(part);
            if let Some(end) = end {
                self.after_cr = bytes[end] == b'\r';
                let line = std::mem::take(&mut self.line);
                if let Some(event) = self.line_ended(&line)? {
                    events.push(event);
                }
            }
            bytes = rest;
        }
        Ok(())
    }

    /// One whole line: a field of the event being read, or (empty) its end.
    fn line_ended(&mut self, line: &[u8]) -> Read<Option<StreamEvent>> {
        if line.is_empty() {
            let (name, data, id) = (
                std::mem::take(&mut self.name),
                std::mem::take(&mut self.data),
                self.id.take(),
            );
            (self.data_lines, self.size) = (0, 0);
            return self.event(&name, &data, id.as_deref());
        }
        // (the line's end counts too, so that an event of empty lines' worth of fields is bounded as well)
        self.size = self.size.saturating_add(1);
        let (field, value) = match line.iter().position(|b| *b == b':') {
            // a comment; between events it belongs to none (a proxy's keep-alive may never send an empty line)
            Some(0) => {
                if self.name.is_empty() && self.data_lines == 0 && self.id.is_none() {
                    self.size = 0;
                }
                return Ok(None);
            }
            Some(colon) => {
                let value = &line[colon + 1..];
                (&line[..colon], value.strip_prefix(b" ").unwrap_or(value))
            }
            None => (line, &[][..]),
        };
        match field {
            b"event" => self.name = value.to_vec(),
            b"data" => {
                if self.data_lines > 0 {
                    self.data.push(b'\n');
                }
                self.data_lines += 1;
                self.data.extend_from_slice(value);
            }
            b"id" => self.id = Some(value.to_vec()),
            _ => {}
        }
        Ok(None)
    }

    /// One whole event.
    fn event(&mut self, name: &[u8], data: &[u8], id: Option<&[u8]>) -> Read<Option<StreamEvent>> {
        let name = match std::str::from_utf8(name) {
            Ok("") | Ok("ping") => return Ok(None),
            Ok(name) => name,
            Err(_) => return bad("an event whose name is not text"),
        };
        if !matches!(
            name,
            "envelope"
                | "log"
                | "relay"
                | "welcome"
                | "request"
                | "presence"
                | "here"
                | "file_evicted"
        ) {
            // an event of a later hub: not this client's
            return Ok(None);
        }
        let body = Json::parse(data, CAP_EVENT)?;
        if name == "envelope" || name == "log" {
            let item = change_item(&body)?;
            let change = item.change();
            if matches!(item, ChangeItem::Envelope(_)) != (name == "envelope") {
                return bad("an event that is not what it is named");
            }
            if id != Some(change.to_string().as_bytes()) {
                return bad("an event whose id is not its change number");
            }
            if change <= self.floor {
                return bad("a change that was sent before");
            }
            self.floor = change;
            return Ok((change > self.taken).then_some(StreamEvent::Change(item)));
        }
        let o = body.obj("an event's data: an object")?;
        Ok(Some(match name {
            "relay" => StreamEvent::Relay {
                group: o.need("group_id").group("relay: group_id")?,
                epoch: o.need("epoch").int("relay: epoch")?,
                sender: o.need("sender").id(DeviceId::from_slice, "relay: sender")?,
                message: o.need("message").bytes(1, MAX_MLS, "relay: message")?.0,
            },
            "welcome" => StreamEvent::Welcome {
                group: o.need("group_id").group("welcome: group_id")?,
            },
            "file_evicted" => StreamEvent::FileEvicted {
                file: o
                    .need("file_id")
                    .id(FileId::from_slice, "file_evicted: file_id")?,
            },
            "request" if matches!(o.need("kind"), Json::Text(kind) if kind == "invite") => {
                StreamEvent::InviteRequest {
                    invite: o
                        .need("invite_id")
                        .id(InviteId::from_slice, "request: invite_id")?,
                }
            }
            "request" => StreamEvent::Wish(wish_row(o)?),
            "presence" if matches!(o.need("archived"), Json::Bool(true)) => StreamEvent::Archived {
                group: o.need("group_id").group("presence: group_id")?,
            },
            "presence" => StreamEvent::Presence(Presence {
                device: o
                    .need("device")
                    .id(DeviceId::from_slice, "presence: device")?,
                online: o.need("online").bool("presence: online")?,
                lost: matches!(o.need("lost"), Json::Bool(true)),
                hears: o
                    .maybe("hears")
                    .map(|v| v.bool("presence: hears"))
                    .transpose()?,
                working: o
                    .maybe("working")
                    .map(|v| v.bool("presence: working"))
                    .transpose()?,
                last_call_at: o
                    .maybe("last_call_at")
                    .map(|v| v.int("presence: last_call_at"))
                    .transpose()?,
            }),
            _ => StreamEvent::Here,
        }))
    }
}

/// A wish a device left for the human devices, as the stream announces it and `GET /v1/requests` lists it.
pub(crate) fn wish_row(o: super::json::Obj<'_>) -> Read<WishRow> {
    Ok(WishRow {
        id: o.need("id").int("a request's id")?,
        device: o
            .need("device")
            .id(DeviceId::from_slice, "a request's device")?,
        kind: o.need("kind").one_of(
            &[
                ("readmit", WishRowKind::Readmit),
                ("handover", WishRowKind::Handover),
                ("session", WishRowKind::Session),
                ("reject", WishRowKind::Reject),
            ],
            "a request's kind",
        )?,
        group: o
            .maybe("group_id")
            .map(|v| v.group("a request's group_id"))
            .transpose()?,
        key_package: o
            .maybe("key_package")
            .map(|v| {
                v.bytes(1, MAX_KEY_PACKAGE, "a request's key_package")
                    .map(|b| b.0)
            })
            .transpose()?,
        n: o.maybe("n").map(|v| v.int("a request's n")).transpose()?,
        committer: o
            .maybe("committer")
            .map(|v| v.id(DeviceId::from_slice, "a request's committer"))
            .transpose()?,
        at: o.maybe("at").map(|v| v.int("a request's at")).transpose()?,
    })
}
