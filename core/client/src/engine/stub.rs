//! A stand-in for W2's hub codec: the routes the engine asks today, as JSON, with the checks the engine relies on
//! (a page's order, sizes of ids) and no more. W2's `crate::hub` codec replaces it behind [`super::Codec`]; nothing
//! of the engine depends on anything here but the [`HubCodec`] trait.
//!
//! The framing of the event stream below is the stub's own, bounded ([`LINE_MAX`], [`EVENT_MAX`]); W2's codec
//! brings the strict one.

use super::hubapi::*;
use crate::app::io::{Header, HttpClass, HttpRequest, HttpResponse};
use crate::hub::{classify, Failure};
use serde_json::{json, Map, Value};
use trommi_core::ids::{base64url_decode, base64url_encode as b64, DeviceId, GroupId, ObjectId};
use trommi_core::store::OutboxKind;

/// At most this many bytes of one line of the stream are kept.
pub const LINE_MAX: usize = 4 << 20;
/// At most this many bytes of one event's data.
pub const EVENT_MAX: usize = 4 << 20;

#[derive(Clone, Copy, Debug, Default)]
pub struct StubCodec;

/// What the stub remembers of a request.
#[derive(Clone, Debug)]
pub enum Expect {
    Challenge,
    Token,
    Changes { after: u64, limit: u32 },
    Welcomes { after: u64 },
    GroupLog { group: GroupId, limit: u32 },
    Chain { after: u64, limit: u32 },
    DeskBrief { delta: bool },
    Outbox { kind: OutboxKind },
    Joins { count: usize },
    KeyPackagesHeld,
    Wished,
    Done,
}

const BAD: fn(&'static str) -> Failure = |why| Failure::BadAnswer { why };

fn bytes_of(v: &Value, field: &'static str) -> Result<Vec<u8>, Failure> {
    v.get(field)
        .and_then(Value::as_str)
        .and_then(|s| base64url_decode(s).ok())
        .filter(|b| !b.is_empty())
        .ok_or(BAD(field))
}

fn number(v: &Value, field: &'static str) -> Result<u64, Failure> {
    // (every number of the protocol stays below 2^53)
    v.get(field)
        .and_then(Value::as_u64)
        .filter(|n| *n < (1 << 53))
        .ok_or(BAD(field))
}

fn group_of(v: &Value, field: &'static str) -> Result<GroupId, Failure> {
    GroupId::from_bytes(&bytes_of(v, field)?).map_err(|_| BAD(field))
}

fn log_item(v: &Value, group: Option<GroupId>) -> Result<Option<ChangeItem>, Failure> {
    let commit = match v.get("kind").and_then(Value::as_str) {
        Some("commit") => true,
        Some("message") => false,
        _ => return Ok(None),
    };
    let item = LogItem {
        change: number(v, "change").and_then(|c| if c > 0 { Ok(c) } else { Err(BAD("change")) })?,
        group: match group {
            Some(group) => group,
            None => group_of(v, "group_id")?,
        },
        n: number(v, "n")?,
        epoch: number(v, "epoch")?,
        at: v.get("at").and_then(Value::as_u64).unwrap_or(0),
        sender: match v.get("sender") {
            Some(s) if s.is_string() => DeviceId::from_slice(&bytes_of(v, "sender")?).map_err(|_| BAD("sender"))?,
            _ => DeviceId::new([0; 32]),
        },
        bytes: bytes_of(v, "bytes")?,
        recovery_auth: match v.get("recovery_auth") {
            Some(a) if a.is_string() && commit => Some(bytes_of(v, "recovery_auth")?),
            _ => None,
        },
    };
    Ok(Some(if commit {
        ChangeItem::Commit(item)
    } else {
        ChangeItem::Message(item)
    }))
}

fn envelope_item(v: &Value) -> Result<EnvelopeItem, Failure> {
    Ok(EnvelopeItem {
        change: number(v, "change").and_then(|c| if c > 0 { Ok(c) } else { Err(BAD("change")) })?,
        envelope: bytes_of(v, "envelope")?,
        received_at: v.get("received_at").and_then(Value::as_u64).unwrap_or(0),
        cut: v.get("cut") == Some(&Value::Bool(true)),
        void_code: match v.get("void_code") {
            None | Some(Value::Null) => None,
            Some(Value::String(code)) if code.len() <= 64 => Some(code.clone()),
            Some(_) => return Err(BAD("void_code")),
        },
    })
}

/// An item of the room's order; `None` for a kind a newer hub adds.
fn change_item(v: &Value) -> Result<Option<ChangeItem>, Failure> {
    if !v.is_object() {
        return Err(BAD("a change"));
    }
    match v.get("kind").and_then(Value::as_str) {
        Some("envelope") => Ok(Some(ChangeItem::Envelope(envelope_item(v)?))),
        _ => log_item(v, None),
    }
}

fn outbox_answer(kind: OutboxKind, v: &Value) -> Result<OutboxAnswer, Failure> {
    let change = || number(v, "change").map(Some);
    let mut answer = OutboxAnswer::default();
    match kind {
        OutboxKind::Envelope | OutboxKind::Commit | OutboxKind::ExternalCommit | OutboxKind::RecoveryCode => {
            answer.change = change()?.filter(|c| *c > 0);
            if answer.change.is_none() {
                return Err(BAD("change"));
            }
        }
        OutboxKind::RecoveryFinish => answer.change = change()?,
        OutboxKind::Message => {
            number(v, "n")?;
        }
        OutboxKind::RelayMessage | OutboxKind::SealedKey => {}
        OutboxKind::KeyPackages => answer.unused = Some(number(v, "unused")?.min(u32::MAX as u64) as u32),
        OutboxKind::GroupFounding => {
            bytes_of(v, "group_id")?;
        }
        OutboxKind::RoomFounding => {
            bytes_of(v, "room_id")?;
        }
        OutboxKind::RecoveryCommit => {
            if v.get("kept") != Some(&Value::Bool(true)) {
                return Err(BAD("kept"));
            }
        }
    }
    Ok(answer)
}

fn commit_body(entry: &trommi_core::store::OutboxEntry, welcome: Option<usize>, sealed_key: usize, auth: Option<usize>) -> Map<String, Value> {
    let part = |i: usize| entry.parts.get(i).filter(|p| !p.is_empty()).map(|p| b64(p));
    let mut body = Map::new();
    body.insert("epoch".into(), json!(entry.epoch));
    body.insert("commit".into(), json!(part(0)));
    body.insert("group_info".into(), json!(part(1)));
    if let Some(w) = welcome.and_then(part) {
        body.insert("welcome".into(), json!(w));
    }
    body.insert("sealed_key".into(), json!(part(sealed_key)));
    if let Some(a) = auth.and_then(part) {
        body.insert("recovery_auth".into(), json!(a));
    }
    body
}

impl StubCodec {
    fn build(site: &Site<'_>, method: &str, path: String, body: Option<Value>, class: HttpClass) -> HttpRequest {
        let mut headers = vec![Header {
            name: "trommi-client".into(),
            value: site.client.to_string(),
        }];
        if let Some(token) = site.token {
            headers.push(Header {
                name: "authorization".into(),
                value: format!("Bearer {token}"),
            });
        }
        if body.is_some() {
            headers.push(Header {
                name: "content-type".into(),
                value: "application/json".into(),
            });
        }
        HttpRequest {
            method: method.into(),
            url: format!("{}{path}", site.url),
            headers,
            body: body.map(|b| b.to_string().into_bytes().into()),
            timeout_ms: 30_000,
            max_answer: 16 << 20,
            class,
            key: None,
        }
    }
}

impl HubCodec for StubCodec {
    type Expect = Expect;
    type Stream = Reader;

    fn stream(&self, after: u64) -> Reader {
        Reader { framer: Framer::default(), last: after, status: 0, body: Vec::new() }
    }

    fn request(&self, site: &Site<'_>, route: &Route<'_>) -> Result<(HttpRequest, Expect), Failure> {
        let get = |path: String| Self::build(site, "GET", path, None, HttpClass::Background);
        let send = |method: &str, path: String, body: Value| Self::build(site, method, path, Some(body), HttpClass::Interactive);
        Ok(match *route {
            Route::Challenge { room } => (get(format!("/v1/rooms/{}/challenge", room.to_base64url())), Expect::Challenge),
            Route::Token { room, auth, signature } => (
                send("POST", format!("/v1/rooms/{}/tokens", room.to_base64url()), json!({ "auth": b64(auth), "signature": b64(signature) })),
                Expect::Token,
            ),
            Route::Changes { after, limit, only } => {
                let only = match only {
                    Only::All => String::new(),
                    Only::Active => "&active=1".into(),
                    Only::Group(group) => format!("&group={}", b64(group.as_bytes())),
                };
                (get(format!("/v1/changes?after={after}&limit={limit}{only}")), Expect::Changes { after, limit })
            }
            Route::Stream { after } => {
                let mut request = get(format!("/v1/stream?after={after}"));
                request.timeout_ms = 0;
                request.max_answer = 64 << 10;
                (request, Expect::Done)
            }
            Route::Welcomes { after } => (get(format!("/v1/welcomes?after={after}")), Expect::Welcomes { after }),
            Route::GroupLog { group, after, limit, commits_only } => (
                get(format!(
                    "/v1/groups/{}/log?after={after}&limit={limit}{}",
                    b64(group.as_bytes()),
                    if commits_only { "&kind=commit" } else { "" }
                )),
                Expect::GroupLog { group: *group, limit },
            ),
            Route::Chain { group, sender, after, limit } => (
                get(format!("/v1/groups/{}/chains/{}?after={after}&limit={limit}", b64(group.as_bytes()), sender.to_base64url())),
                Expect::Chain { after, limit },
            ),
            Route::DeskBrief { since, active } => (
                get(format!(
                    "/v1/desk?brief=1{}{}",
                    since.map(|s| format!("&since={s}")).unwrap_or_default(),
                    if active { "&active=1" } else { "" }
                )),
                Expect::DeskBrief { delta: since.is_some() },
            ),
            Route::Outbox { room, entry, extra: _ } => {
                let part = |i: usize| entry.parts.get(i).filter(|p| !p.is_empty()).map(|p| b64(p));
                let group = entry.group.map(|g| b64(g.as_bytes())).unwrap_or_default();
                let request = match entry.kind {
                    OutboxKind::Commit => send("POST", format!("/v1/groups/{group}/commits"), Value::Object(commit_body(entry, Some(2), 3, None))),
                    OutboxKind::ExternalCommit => send("POST", format!("/v1/groups/{group}/commits"), Value::Object(commit_body(entry, None, 2, Some(3)))),
                    OutboxKind::GroupFounding => send(
                        "POST",
                        "/v1/groups".into(),
                        json!({ "group_info_0": part(0), "sealed_key_0": part(1), "commit": part(2), "group_info": part(3), "welcome": part(4), "sealed_key": part(5) }),
                    ),
                    OutboxKind::Message | OutboxKind::RelayMessage => send(
                        "POST",
                        format!("/v1/groups/{group}/messages"),
                        json!({ "epoch": entry.epoch, "message": part(0), "relay": entry.kind == OutboxKind::RelayMessage }),
                    ),
                    OutboxKind::Envelope => send("POST", "/v1/envelopes".into(), json!({ "envelope": part(0) })),
                    OutboxKind::KeyPackages => {
                        let single: Vec<String> = entry.parts.iter().skip(1).map(|p| b64(p)).collect();
                        let mut body = json!({ "single_use": single });
                        if let Some(last) = part(0) {
                            body["last_resort"] = json!(last);
                        }
                        send("PUT", "/v1/key-packages".into(), body)
                    }
                    OutboxKind::SealedKey => send("PUT", "/v1/sealed-keys".into(), json!({ "sealed_key": part(0) })),
                    OutboxKind::RecoveryCode => {
                        let account: Value = entry.parts.get(4).and_then(|p| serde_json::from_slice(p).ok()).unwrap_or(Value::Null);
                        send(
                            "POST",
                            format!("/v1/rooms/{}/recovery-code", room.to_base64url()),
                            json!({ "commit": Value::Object(commit_body(entry, None, 2, None)), "recovery_link": part(3), "account": account }),
                        )
                    }
                    // (W2: a founding goes with its account, a recovery's parts with their recovery)
                    OutboxKind::RoomFounding | OutboxKind::RecoveryCommit | OutboxKind::RecoveryFinish => {
                        return Err(BAD("the stand-in codec does not found rooms or recover them"))
                    }
                };
                (request, Expect::Outbox { kind: entry.kind })
            }
            Route::Joins { entries } => {
                if entries.is_empty() || entries.len() > 64 {
                    return Err(BAD("1 to 64 joins go in one request"));
                }
                let commits: Vec<Value> = entries
                    .iter()
                    .map(|entry| {
                        let mut body = commit_body(entry, None, 2, Some(3));
                        body.insert("group".into(), json!(entry.group.map(|g| b64(g.as_bytes()))));
                        Value::Object(body)
                    })
                    .collect();
                (send("POST", "/v1/commits".into(), json!({ "commits": commits })), Expect::Joins { count: entries.len() })
            }
            Route::KeyPackagesHeld => (send("PUT", "/v1/key-packages".into(), json!({ "single_use": [] })), Expect::KeyPackagesHeld),
            Route::Wish { kind, group, key_package } => {
                let kind = match kind {
                    WishKind::Readmit => "readmit",
                    WishKind::Handover => "handover",
                    WishKind::Session => "session",
                };
                let mut body = json!({ "kind": kind });
                if let Some(group) = group {
                    body["group"] = json!(b64(group.as_bytes()));
                }
                if let Some(package) = key_package {
                    body["key_package"] = json!(b64(package));
                }
                (send("POST", "/v1/requests".into(), body), Expect::Wished)
            }
            Route::RejectCommit { group, n } => (send("POST", format!("/v1/groups/{}/reject", b64(group.as_bytes())), json!({ "n": n })), Expect::Done),
            Route::Object { .. } | Route::GroupsRead { .. } | Route::ClaimKeyPackages { .. } | Route::RoomGroups { .. } | Route::GroupInfo { .. } | Route::Wishes => {
                return Err(BAD("the stand-in codec does not know this route"))
            }
        })
    }

    fn answer(&self, expect: &Expect, response: &HttpResponse) -> Result<Answer, Failure> {
        classify(response)?;
        if matches!(expect, Expect::Done) {
            return Ok(Answer::Done);
        }
        let v: Value = if response.body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&response.body).map_err(|_| BAD("not JSON"))?
        };
        Ok(match expect {
            Expect::Done => Answer::Done,
            Expect::Challenge => {
                let bytes = bytes_of(&v, "challenge")?;
                Answer::Challenge(bytes.try_into().map_err(|_| BAD("challenge"))?)
            }
            Expect::Token => {
                let token = v.get("token").and_then(Value::as_str).filter(|t| !t.is_empty() && t.len() <= 200 && t.bytes().all(|b| b.is_ascii_graphic())).ok_or(BAD("token"))?;
                let role = match v.get("role").and_then(Value::as_str) {
                    Some("human") => Role::Human,
                    Some("agent") => Role::Agent,
                    Some("helper") => Role::Helper,
                    Some("recovery") => Role::Recovery,
                    Some("removed") => Role::Removed,
                    _ => return Err(BAD("role")),
                };
                Answer::Token {
                    token: token.to_string(),
                    expires_at: number(&v, "expires_at")?,
                    role,
                }
            }
            Expect::Changes { after, limit } => {
                let change = number(&v, "change")?;
                let more = v.get("more").and_then(Value::as_bool).ok_or(BAD("more"))?;
                let raw = v.get("items").and_then(Value::as_array).ok_or(BAD("items"))?;
                if change < *after || raw.len() > *limit as usize || (more && change == *after) {
                    return Err(BAD("a page outside what was asked"));
                }
                let mut at = *after;
                let mut items = Vec::with_capacity(raw.len());
                for one in raw {
                    let n = number(one, "change")?;
                    if n <= at || n > change {
                        return Err(BAD("changes out of order"));
                    }
                    at = n;
                    if let Some(item) = change_item(one)? {
                        items.push(item);
                    }
                }
                let head = match v.get("head") {
                    None | Some(Value::Null) => None,
                    Some(_) => Some(number(&v, "head")?),
                };
                Answer::Changes(ChangesPage { items, change, more, head })
            }
            Expect::Welcomes { after } => {
                let raw = v.as_array().ok_or(BAD("welcomes"))?;
                let mut at = *after;
                let mut rows = Vec::with_capacity(raw.len());
                for one in raw {
                    let id = number(one, "id")?;
                    if id <= at {
                        return Err(BAD("Welcomes out of order"));
                    }
                    at = id;
                    rows.push(WelcomeRow {
                        id,
                        group: group_of(one, "group_id")?,
                        welcome: bytes_of(one, "welcome")?,
                        at: one.get("at").and_then(Value::as_u64).unwrap_or(0),
                    });
                }
                Answer::Welcomes(rows)
            }
            Expect::GroupLog { group, limit } => {
                let raw = v.get("items").and_then(Value::as_array).ok_or(BAD("items"))?;
                if raw.len() > *limit as usize {
                    return Err(BAD("a page longer than asked"));
                }
                let mut items = Vec::new();
                let (mut n, mut change) = (0, 0);
                for one in raw {
                    let Some(item) = log_item(one, Some(*group))? else { continue };
                    let (ChangeItem::Commit(log) | ChangeItem::Message(log)) = &item else { continue };
                    if log.n <= n || log.change <= change {
                        return Err(BAD("a log out of order"));
                    }
                    (n, change) = (log.n, log.change);
                    items.push(item);
                }
                Answer::GroupLog {
                    items,
                    more: v.get("more").and_then(Value::as_bool).ok_or(BAD("more"))?,
                }
            }
            Expect::Chain { after, limit } => {
                let raw = v.get("items").and_then(Value::as_array).ok_or(BAD("items"))?;
                if raw.len() > *limit as usize {
                    return Err(BAD("a page longer than asked"));
                }
                let mut seq = *after;
                let mut items = Vec::with_capacity(raw.len());
                for one in raw {
                    let n = number(one, "seq")?;
                    if n <= seq {
                        return Err(BAD("a chain out of order"));
                    }
                    seq = n;
                    items.push(ChainItem {
                        seq: n,
                        item: envelope_item(one)?,
                    });
                }
                Answer::Chain {
                    items,
                    more: v.get("more").and_then(Value::as_bool).ok_or(BAD("more"))?,
                }
            }
            Expect::DeskBrief { delta } => {
                let entries = |field: &'static str| -> Result<Vec<DeskEntry>, Failure> {
                    v.get(field)
                        .and_then(Value::as_array)
                        .ok_or(BAD(field))?
                        .iter()
                        .map(|one| {
                            Ok(DeskEntry {
                                object: ObjectId::from_slice(&bytes_of(one, "object_id")?).map_err(|_| BAD("object_id"))?,
                                version_change: number(one, "version_change")?,
                                head_change: number(one, "head_change")?,
                            })
                        })
                        .collect()
                };
                let settled = if *delta {
                    let raw = v.get("settled").and_then(Value::as_array).ok_or(BAD("settled"))?;
                    let mut out = Vec::with_capacity(raw.len());
                    for one in raw {
                        let bytes = one.as_str().and_then(|s| base64url_decode(s).ok()).ok_or(BAD("settled"))?;
                        out.push(ObjectId::from_slice(&bytes).map_err(|_| BAD("settled"))?);
                    }
                    Some(out)
                } else {
                    None
                };
                Answer::DeskBrief(DeskBrief {
                    cards: entries("cards")?,
                    permission_requests: entries("permission_requests")?,
                    settled,
                    truncated: v.get("truncated").and_then(Value::as_bool).ok_or(BAD("truncated"))?,
                    change: number(&v, "change")?,
                })
            }
            Expect::Outbox { kind } => Answer::Outbox(outbox_answer(*kind, &v)?),
            Expect::Joins { count } => {
                let raw = v.get("answers").and_then(Value::as_array).filter(|a| a.len() == *count).ok_or(BAD("one answer per Commit"))?;
                Answer::Joins(
                    raw.iter()
                        .map(|one| match one.get("error").and_then(Value::as_str) {
                            None => outbox_answer(OutboxKind::ExternalCommit, one),
                            Some(code) => {
                                let status = one.get("status").and_then(Value::as_u64).unwrap_or(500) as u16;
                                Err(if status == 408 || status == 429 || status >= 500 {
                                    Failure::NotNow {
                                        status,
                                        retry_after_ms: one.get("retry_after").and_then(Value::as_u64).filter(|s| *s <= 86_400).map(|s| s * 1000),
                                    }
                                } else {
                                    Failure::Refused {
                                        status,
                                        code: Some(code.to_string()),
                                    }
                                })
                            }
                        })
                        .collect(),
                )
            }
            Expect::KeyPackagesHeld => Answer::KeyPackagesHeld {
                unused: number(&v, "unused")?.min(u32::MAX as u64) as u32,
            },
            Expect::Wished => Answer::Wished { id: number(&v, "id")? },
        })
    }

    fn refusal(&self, response: &HttpResponse) -> Option<Refusal> {
        let Err(Failure::Refused { status, code: Some(code) }) = classify(response) else {
            return None;
        };
        // (W2: only a code of spec 16 under its own status; the stand-in takes any code the protocol core knows)
        trommi_core::Error::from_code(&code)?;
        let v: Value = serde_json::from_slice(&response.body).ok()?;
        Some(Refusal {
            code,
            status,
            voided: v.get("voided") == Some(&Value::Bool(true)),
            retry_after_ms: None,
            epoch: v.get("epoch").and_then(Value::as_u64),
            seq: v.get("seq").and_then(Value::as_u64),
            device: None,
            group: None,
        })
    }
}

/// The stub's reader of one stream.
pub struct Reader {
    framer: Framer,
    /// The last change handed over (or the `after` asked, or what a catch-up took).
    last: u64,
    status: u16,
    /// The body of an answer that is no stream, up to 64 KiB.
    body: Vec<u8>,
}

impl StreamReader for Reader {
    fn opened(&mut self, status: u16, _headers: &[Header]) -> Result<(), Failure> {
        self.status = status;
        if status == 200 {
            Ok(())
        } else {
            Err(BAD("not a stream"))
        }
    }

    fn chunk(&mut self, bytes: &[u8]) -> Fed {
        let mut fed = Fed { events: Vec::new(), failure: None };
        if self.status != 200 {
            let room = (64usize << 10).saturating_sub(self.body.len());
            self.body.extend_from_slice(&bytes[..bytes.len().min(room)]);
            return fed;
        }
        let framed = match self.framer.feed(bytes) {
            Ok(framed) => framed,
            Err(failure) => {
                fed.failure = Some(failure);
                return fed;
            }
        };
        for (name, data) in framed {
            match stream_event(&name, &data) {
                Ok(Some(StreamEvent::Change(item))) => {
                    // (handed over once, and only rising)
                    if item.change() > self.last {
                        self.last = item.change();
                        fed.events.push(StreamEvent::Change(item));
                    }
                }
                Ok(Some(event)) => fed.events.push(event),
                Ok(None) => {}
                Err(failure) => {
                    fed.failure = Some(failure);
                    break;
                }
            }
        }
        fed
    }

    fn ended(&mut self) -> Option<Failure> {
        if self.status == 200 || self.status == 0 {
            return None;
        }
        classify(&HttpResponse {
            status: self.status,
            headers: Vec::new(),
            body: std::mem::take(&mut self.body).into(),
        })
        .err()
    }

    fn caught_up_to(&mut self, change: u64) {
        self.last = self.last.max(change);
    }
}

fn stream_event(name: &str, data: &[u8]) -> Result<Option<StreamEvent>, Failure> {
    let v: Value = serde_json::from_slice(data).map_err(|_| BAD("an event that is no JSON"))?;
    Ok(Some(match name {
        "envelope" | "log" => match change_item(&v)? {
            Some(item) => StreamEvent::Change(item),
            None => return Ok(None),
        },
        "relay" => StreamEvent::Relay {
            group: group_of(&v, "group_id")?,
            message: bytes_of(&v, "message")?,
        },
        "welcome" => StreamEvent::Welcome {
            group: group_of(&v, "group_id")?,
        },
        "presence" if v.get("archived") == Some(&Value::Bool(true)) => StreamEvent::Archived {
            group: group_of(&v, "group_id")?,
        },
        "presence" => StreamEvent::Presence(Presence {
            device: DeviceId::from_slice(&bytes_of(&v, "device")?).map_err(|_| BAD("device"))?,
            online: v.get("online") == Some(&Value::Bool(true)),
            lost: v.get("lost") == Some(&Value::Bool(true)),
            hears: v.get("hears").and_then(Value::as_bool),
            working: v.get("working").and_then(Value::as_bool),
            last_call_at: v.get("last_call_at").and_then(Value::as_u64),
        }),
        "here" => StreamEvent::Here,
        "ping" => return Ok(None),
        "file_evicted" => StreamEvent::FileEvicted {
            file: bytes_of(&v, "file_id")?,
        },
        // (wishes and invite Requests: W2 reads them; the stand-in passes them over)
        _ => return Ok(None),
    }))
}

/// The stub's framing of server-sent events: lines to (`event`, `data`) pairs, bounded.
#[derive(Default)]
pub struct Framer {
    line: Vec<u8>,
    name: String,
    data: Vec<u8>,
    /// The last byte was a CR: a LF that follows belongs to it.
    cr: bool,
}

impl Framer {
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Failure> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' && std::mem::take(&mut self.cr) {
                continue;
            }
            self.cr = b == b'\r';
            if b != b'\n' && b != b'\r' {
                if self.line.len() >= LINE_MAX {
                    return Err(BAD("a line of the stream above the limit"));
                }
                self.line.push(b);
                continue;
            }
            let line = std::mem::take(&mut self.line);
            if line.is_empty() {
                // an event ends; one without data is nothing (a comment's blank line)
                let name = std::mem::take(&mut self.name);
                let data = std::mem::take(&mut self.data);
                if !data.is_empty() {
                    out.push((if name.is_empty() { "message".to_string() } else { name }, data));
                }
                continue;
            }
            let (field, value) = match line.iter().position(|c| *c == b':') {
                Some(0) => continue, // a comment
                Some(at) => (&line[..at], line[at + 1..].strip_prefix(b" ").unwrap_or(&line[at + 1..])),
                None => (&line[..], &[][..]),
            };
            match field {
                b"event" => self.name = String::from_utf8_lossy(value).into_owned(),
                b"data" => {
                    if self.data.len() + value.len() + 1 > EVENT_MAX {
                        return Err(BAD("an event of the stream above the limit"));
                    }
                    if !self.data.is_empty() {
                        self.data.push(b'\n');
                    }
                    self.data.extend_from_slice(value);
                }
                _ => {} // `id`, `retry`: the engine resumes by the change it took, not by what the hub names
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_frame_whatever_the_cut() {
        let text = b": hello\n\nevent: ping\ndata: {}\n\nid: 7\r\nevent: here\r\ndata: {\r\ndata: }\r\n\r\ndata: 1\n\n";
        let whole = Framer::default().feed(text).unwrap();
        assert_eq!(
            whole,
            vec![("ping".to_string(), b"{}".to_vec()), ("here".to_string(), b"{\n}".to_vec()), ("message".to_string(), b"1".to_vec())]
        );
        for cut in 0..text.len() {
            let mut f = Framer::default();
            let mut got = f.feed(&text[..cut]).unwrap();
            got.extend(f.feed(&text[cut..]).unwrap());
            assert_eq!(got, whole, "cut at {cut}");
        }
    }

    #[test]
    fn a_page_out_of_order_is_refused_whole() {
        let page = |items: Value, change: u64, more: bool| HttpResponse {
            status: 200,
            headers: vec![],
            body: json!({ "items": items, "change": change, "more": more }).to_string().into_bytes().into(),
        };
        let env = |n: u64| json!({ "kind": "envelope", "change": n, "envelope": "AAAA" });
        let expect = Expect::Changes { after: 5, limit: 10 };
        let c = StubCodec;
        assert!(c.answer(&expect, &page(json!([env(6), env(8)]), 9, false)).is_ok());
        for bad in [page(json!([env(5)]), 9, false), page(json!([env(7), env(6)]), 9, false), page(json!([env(10)]), 9, false), page(json!([]), 4, false), page(json!([]), 5, true)] {
            assert!(matches!(c.answer(&expect, &bad), Err(Failure::BadAnswer { .. })));
        }
    }
}
