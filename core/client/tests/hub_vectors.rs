//! What the real hub answered (`tests/hub/*.json`, recorded by `tests/hub/codec.rs` of the workspace's test crate
//! with `TROMMI_HUB_VECTORS=write`), read by the codec without a hub, and nothing lost on the way: every value of
//! the raw answer that the codec hands on is compared with the typed one.
//!
//! One file is one exchange: `asked` (the route and its parameters), `request` (method, path, body as text),
//! `status`, `answer` (the raw body as text; of the stream, the raw events). A stand-in hub can serve them as
//! they are.

use serde_json::Value;
use trommi_client_core::app::{Header, HttpResponse};
use trommi_client_core::hub::{
    AccountProof, Answer, ByteRange, ChangeItem, Codec, DeskOnly, EnvelopeItem, Failure, Kdf,
    KdfValue, KitPart, LiveActivity, LogItem, NewAccount, NewPassword, ObjectRoute, Only,
    PasskeyAssertion, PasskeyRegistration, PushLevel, PushRegistration, PushTarget, Route, Site,
    StreamEvent, StreamReader, Timeline, WishKind,
};
use trommi_core::envelope::Envelope;
use trommi_core::ids::{
    base64url_decode, base64url_encode as b64, BoardId, DeviceId, FileId, GroupId, Hash32,
    InviteId, ObjectId, RoomId, SessionId, ShareId,
};
use trommi_core::store::{OutboxEntry, OutboxKind};

const SITE: Site<'static> = Site {
    url: "http://127.0.0.1:1",
    token: Some("abcdefghijklmnop_-0123"),
    client: "test/1.0.0",
    lease: None,
};

fn vectors() -> Vec<(String, Value)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hub");
    let mut out: Vec<(String, Value)> = std::fs::read_dir(dir)
        .expect("tests/hub")
        .map(|f| f.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (
                name,
                serde_json::from_slice(&std::fs::read(&p).unwrap()).expect("a vector is JSON"),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn group(v: &Value) -> GroupId {
    GroupId::from_bytes(&base64url_decode(v.as_str().unwrap()).unwrap()).unwrap()
}

fn same_envelope(raw: &Value, item: &EnvelopeItem) {
    assert_eq!(raw["change"], item.change);
    assert_eq!(raw["envelope"], b64(&item.envelope));
    assert_eq!(raw["received_at"], item.received_at);
    assert_eq!(
        raw.get("void_code").and_then(Value::as_str),
        item.void_code.as_deref()
    );
}

fn same_log(raw: &Value, item: &LogItem) {
    assert_eq!(raw["change"], item.change);
    assert_eq!(raw["group_id"], b64(item.group.as_bytes()));
    assert_eq!(
        (&raw["n"], &raw["epoch"], &raw["at"]),
        (&item.n.into(), &item.epoch.into(), &item.at.into())
    );
    assert_eq!(raw["bytes"], b64(&item.bytes));
    assert_eq!(raw["sender"], b64(item.sender.as_bytes()));
    assert_eq!(
        raw.get("recovery_auth")
            .and_then(Value::as_str)
            .map(str::to_owned),
        item.recovery_auth.as_deref().map(b64)
    );
}

fn same_change(raw: &Value, item: &ChangeItem) {
    match item {
        ChangeItem::Envelope(e) => {
            assert_eq!(raw["kind"], "envelope");
            same_envelope(raw, e);
        }
        ChangeItem::Commit(l) => {
            assert_eq!(raw["kind"], "commit");
            same_log(raw, l);
        }
        ChangeItem::Message(l) => {
            assert_eq!(raw["kind"], "message");
            same_log(raw, l);
        }
    }
}

/// The raw events of a stream, split the plain way (the vectors are the hub's own writing: LF only).
fn raw_events(text: &str) -> Vec<(String, Option<String>, Value)> {
    text.split("\n\n")
        .filter_map(|block| {
            let field = |name: &str| {
                block
                    .lines()
                    .find_map(|l| l.strip_prefix(name).map(str::to_owned))
            };
            let name = field("event: ")?;
            Some((
                name,
                field("id: "),
                serde_json::from_str(&field("data: ")?).unwrap(),
            ))
        })
        .filter(|(name, _, _)| name != "ping")
        .collect()
}

#[test]
fn what_the_real_hub_answered_is_read_without_loss() {
    let all = vectors();
    let mut seen = std::collections::BTreeSet::new();
    for (name, v) in &all {
        let asked = &v["asked"];
        let route = asked["route"].as_str().unwrap();
        seen.insert(route.to_owned());
        let status = v["status"].as_u64().unwrap() as u16;
        let answer = v["answer"].as_str().unwrap();
        let response = HttpResponse {
            status,
            headers: vec![],
            body: answer.as_bytes().to_vec().into(),
        };
        let raw: Value = serde_json::from_str(answer).unwrap_or(Value::Null);
        let read = |route: &Route<'_>| {
            let (request, expect) = Codec.request(&SITE, route).expect("the route builds");
            // the request recorded is the one the codec builds from what was asked
            if !matches!(route, Route::Outbox { .. }) {
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str(),
                    "{name}"
                );
                assert_eq!(request.method, v["request"]["method"], "{name}");
            }
            Codec.answer(&expect, &response)
        };
        match route {
            "changes" => {
                let one;
                let only = match &asked["only"] {
                    Value::String(word) if word == "active" => Only::Active,
                    Value::String(_) => Only::All,
                    with_group => {
                        one = group(&with_group["group"]);
                        Only::Group(&one)
                    }
                };
                let route = Route::Changes {
                    after: asked["after"].as_u64().unwrap(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                    only,
                };
                if status != 200 {
                    assert!(
                        matches!(read(&route), Err(Failure::Refused { code: Some(_), .. })),
                        "{name}"
                    );
                    continue;
                }
                let Ok(Answer::Changes(page)) = read(&route) else {
                    panic!("{name} is not read")
                };
                let items = raw["items"].as_array().unwrap();
                assert_eq!(items.len(), page.items.len(), "{name}");
                for (raw, item) in items.iter().zip(&page.items) {
                    same_change(raw, item);
                }
                assert_eq!(raw["change"], page.change);
                assert_eq!(raw["more"], page.more);
                assert_eq!(raw["head"].as_u64(), page.head);
            }
            "desk_brief" => {
                let route = Route::DeskBrief {
                    since: asked["since"].as_u64(),
                    active: asked["active"].as_bool().unwrap(),
                };
                let Ok(Answer::DeskBrief(brief)) = read(&route) else {
                    panic!("{name} is not read")
                };
                for (list, rows) in [
                    ("cards", &brief.cards),
                    ("permission_requests", &brief.permission_requests),
                ] {
                    let raw = raw[list].as_array().unwrap();
                    assert_eq!(raw.len(), rows.len(), "{name}");
                    for (raw, row) in raw.iter().zip(rows) {
                        assert_eq!(raw["object_id"], b64(row.object.as_bytes()));
                        assert_eq!(raw["version_change"], row.version_change);
                        assert_eq!(raw["head_change"], row.head_change);
                        assert_eq!(raw["first_change"].as_u64(), row.first_change);
                    }
                }
                let settled = raw.get("settled").map(|s| s.as_array().unwrap().clone());
                assert_eq!(
                    settled,
                    brief.settled.map(|ids| ids
                        .iter()
                        .map(|id| Value::from(b64(id.as_bytes())))
                        .collect())
                );
                assert_eq!(raw["truncated"], brief.truncated);
                assert_eq!(raw["change"], brief.change);
            }
            "key_packages_held" => {
                let Ok(Answer::KeyPackagesHeld { unused }) = read(&Route::KeyPackagesHeld) else {
                    panic!("{name} is not read")
                };
                assert_eq!(raw["unused"], unused);
            }
            "outbox" => {
                let kind = match asked["kind"].as_str().unwrap() {
                    "RoomFounding" => OutboxKind::RoomFounding,
                    "GroupFounding" => OutboxKind::GroupFounding,
                    "Commit" => OutboxKind::Commit,
                    "ExternalCommit" => OutboxKind::ExternalCommit,
                    "Message" => OutboxKind::Message,
                    "RelayMessage" => OutboxKind::RelayMessage,
                    "Envelope" => OutboxKind::Envelope,
                    "KeyPackages" => OutboxKind::KeyPackages,
                    "SealedKey" => OutboxKind::SealedKey,
                    other => panic!("{name}: a kind this test does not know: {other}"),
                };
                let entry = OutboxEntry {
                    id: 1,
                    kind,
                    group: asked.get("group").filter(|g| !g.is_null()).map(group),
                    epoch: asked["epoch"].as_u64().unwrap(),
                    parts: vec![vec![1]; asked["parts"].as_u64().unwrap() as usize],
                };
                let room = RoomId::new([1; 32]);
                let nobody = Site {
                    token: None,
                    ..SITE
                };
                let site = if kind == OutboxKind::RoomFounding {
                    &nobody
                } else {
                    &SITE
                };
                let (request, expect) = Codec
                    .request(
                        site,
                        &Route::Outbox {
                            room: &room,
                            entry: &entry,
                            extra: None,
                        },
                    )
                    .expect("the entry builds");
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str(),
                    "{name}"
                );
                // the body recorded has the members the codec writes for this kind
                let (ours, theirs): (Value, Value) = (
                    serde_json::from_slice(request.body.as_ref().unwrap()).unwrap(),
                    serde_json::from_str(v["request"]["body"].as_str().unwrap()).unwrap(),
                );
                let names = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
                if kind != OutboxKind::GroupFounding && kind != OutboxKind::Commit {
                    assert_eq!(names(&ours), names(&theirs), "{name}");
                }
                let got = Codec.answer(&expect, &response);
                if status != 200 {
                    let refusal = Codec.refusal(&response).expect("the hub's own refusal");
                    assert_eq!(
                        got,
                        Err(Failure::Refused {
                            status,
                            code: Some(refusal.code.to_owned())
                        }),
                        "{name}"
                    );
                    assert_eq!(
                        raw.get("seq").and_then(Value::as_u64),
                        refusal.seq,
                        "{name}"
                    );
                    assert_eq!(
                        raw.get("epoch").and_then(Value::as_u64),
                        refusal.epoch,
                        "{name}"
                    );
                    assert_eq!(
                        raw.get("voided").and_then(Value::as_bool).unwrap_or(false),
                        refusal.voided
                    );
                    continue;
                }
                let Ok(Answer::Outbox(answer)) = got else {
                    panic!("{name} is not read: {got:?}")
                };
                assert_eq!(
                    raw.get("change").and_then(Value::as_u64),
                    answer.change,
                    "{name}"
                );
                assert_eq!(
                    raw.get("epoch").and_then(Value::as_u64),
                    answer.epoch,
                    "{name}"
                );
                assert_eq!(raw.get("n").and_then(Value::as_u64), answer.n, "{name}");
                assert_eq!(
                    raw.get("unused").and_then(Value::as_u64),
                    answer.unused.map(u64::from),
                    "{name}"
                );
                assert_eq!(
                    raw.get("room_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    answer.room.map(|r| b64(r.as_bytes())),
                    "{name}"
                );
                assert_eq!(
                    raw.get("group_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                    answer.group.map(|g| b64(g.as_bytes())),
                    "{name}"
                );
            }
            "stream" => {
                let after = asked["after"].as_u64().unwrap();
                let (request, _) = Codec.request(&SITE, &Route::Stream { after }).unwrap();
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str()
                );
                let head = [Header {
                    name: "content-type".into(),
                    value: "text/event-stream".into(),
                }];
                let raw = raw_events(answer);
                // whole, and cut at every byte position: the same events
                let mut reader = StreamReader::new(after);
                reader.opened(status, &head).unwrap();
                let whole = reader.chunk(answer.as_bytes());
                assert_eq!(whole.failure, None);
                assert_eq!(
                    whole.events.len(),
                    raw.len(),
                    "every event of the hub is one of the codec's"
                );
                for cut in 0..answer.len() {
                    let mut reader = StreamReader::new(after);
                    let mut events = reader.chunk(&answer.as_bytes()[..cut]).events;
                    let rest = reader.chunk(&answer.as_bytes()[cut..]);
                    events.extend(rest.events);
                    assert!(
                        rest.failure.is_none() && events == whole.events,
                        "cut at {cut}"
                    );
                }
                for ((name, id, data), event) in raw.iter().zip(&whole.events) {
                    match event {
                        StreamEvent::Change(item) => {
                            assert_eq!(id.as_deref(), Some(item.change().to_string().as_str()));
                            assert_eq!(name == "envelope", matches!(item, ChangeItem::Envelope(_)));
                            same_change(data, item);
                        }
                        StreamEvent::Presence(p) => {
                            assert_eq!(name, "presence");
                            assert_eq!(data["device"], b64(p.device.as_bytes()));
                            assert_eq!(data["online"], p.online);
                            assert_eq!(data.get("hears").and_then(Value::as_bool), p.hears);
                            assert_eq!(data.get("working").and_then(Value::as_bool), p.working);
                            assert_eq!(
                                data.get("last_call_at").and_then(Value::as_u64),
                                p.last_call_at
                            );
                        }
                        StreamEvent::Here => assert_eq!(name, "here"),
                        other => panic!("an event this vector is not known to hold: {other:?}"),
                    }
                }
            }
            "challenge" | "token" => {
                let room =
                    RoomId::from_slice(&base64url_decode(asked["room"].as_str().unwrap()).unwrap())
                        .unwrap();
                let nobody = Site {
                    token: None,
                    ..SITE
                };
                let route = match route {
                    "challenge" => Route::Challenge { room: &room },
                    _ => Route::Token {
                        room: &room,
                        auth: &[1; 100],
                        signature: &[2; 64],
                    },
                };
                let (request, expect) = Codec.request(&nobody, &route).unwrap();
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str(),
                    "{name}"
                );
                match Codec.answer(&expect, &response) {
                    Ok(Answer::Challenge(challenge)) => {
                        assert_eq!(raw["challenge"], b64(&challenge))
                    }
                    Ok(Answer::Token {
                        token,
                        expires_at,
                        role,
                    }) => {
                        assert_eq!(raw["token"], token);
                        assert_eq!(raw["expires_at"], expires_at);
                        assert_eq!(raw["role"], format!("{role:?}").to_lowercase());
                    }
                    other => panic!("{name} is not read: {other:?}"),
                }
            }
            "group_info" => {
                let g = group(&asked["group"]);
                let route = Route::GroupInfo {
                    group: &g,
                    epoch: asked["epoch"].as_u64(),
                };
                let Ok(Answer::GroupInfo { epoch, group_info }) = read(&route) else {
                    panic!("{name} is not read")
                };
                assert_eq!(raw["epoch"], epoch);
                assert_eq!(raw["group_info"], b64(&group_info));
            }
            "group_log" => {
                let g = group(&asked["group"]);
                let route = Route::GroupLog {
                    group: &g,
                    after: asked["after"].as_u64().unwrap(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                    commits_only: asked["commits_only"].as_bool().unwrap(),
                };
                let Ok(Answer::GroupLog { items, more }) = read(&route) else {
                    panic!("{name} is not read")
                };
                let list = raw["items"].as_array().unwrap();
                assert_eq!(list.len(), items.len(), "{name}");
                for (raw, item) in list.iter().zip(&items) {
                    same_change(raw, item);
                }
                assert_eq!(raw["more"], more);
            }
            "welcomes" => {
                let route = Route::Welcomes {
                    after: asked["after"].as_u64().unwrap(),
                };
                let Ok(Answer::Welcomes(rows)) = read(&route) else {
                    panic!("{name} is not read")
                };
                let list = raw.as_array().unwrap();
                assert_eq!(list.len(), rows.len(), "{name}");
                for (raw, row) in list.iter().zip(&rows) {
                    assert_eq!(raw["id"], row.id);
                    assert_eq!(raw["group_id"], b64(row.group.as_bytes()));
                    assert_eq!(raw["welcome"], b64(&row.welcome));
                    assert_eq!(raw["at"], row.at);
                }
            }
            "room_groups" => {
                let room =
                    RoomId::from_slice(&base64url_decode(asked["room"].as_str().unwrap()).unwrap())
                        .unwrap();
                let route = Route::RoomGroups {
                    room: &room,
                    after: asked["after"].as_u64().unwrap(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                };
                let Ok(Answer::RoomGroups { items, more, after }) = read(&route) else {
                    panic!("{name} is not read")
                };
                let list = raw["items"].as_array().unwrap();
                assert_eq!(list.len(), items.len(), "{name}");
                for (raw, row) in list.iter().zip(&items) {
                    assert_eq!(raw["group_id"], b64(row.group.as_bytes()));
                    assert_eq!(raw["kind"], format!("{:?}", row.kind).to_lowercase());
                    assert_eq!(
                        (&raw["epoch"], &raw["room_epoch"]),
                        (&row.epoch.into(), &row.room_epoch.into())
                    );
                    assert_eq!(
                        (&raw["live"], &raw["stale"]),
                        (&row.live.into(), &row.stale.into())
                    );
                    let leaves: Vec<Value> = row
                        .leaves
                        .iter()
                        .map(|l| b64(l.as_bytes()).into())
                        .collect();
                    assert_eq!(raw["leaves"], Value::Array(leaves));
                    assert_eq!(
                        raw.get("session_id")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        row.group.session_id().map(|s| b64(s.as_bytes()))
                    );
                    // (a session that is no helper's names no parent, or one of zeros)
                    assert_eq!(
                        raw.get("parent")
                            .and_then(Value::as_str)
                            .filter(|p| *p != b64(&[0; 16]))
                            .map(str::to_owned),
                        row.parent.map(|p| b64(p.as_bytes()))
                    );
                }
                assert_eq!((&raw["more"], &raw["after"]), (&more.into(), &after.into()));
            }
            "groups_read" => {
                let groups: Vec<GroupId> = asked["groups"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(group)
                    .collect();
                let Ok(Answer::GroupsRead(rows)) = read(&Route::GroupsRead { groups: &groups })
                else {
                    panic!("{name} is not read")
                };
                let list = raw["groups"].as_array().unwrap();
                assert_eq!(list.len(), rows.len(), "{name}");
                for (raw, row) in list.iter().zip(&rows) {
                    match row {
                        Err(failure) => {
                            let code = raw["error"].as_str().map(str::to_owned);
                            let status = raw["status"].as_u64().unwrap() as u16;
                            assert_eq!(failure, &Failure::Refused { status, code });
                        }
                        Ok(row) => {
                            assert_eq!(raw["group"], b64(row.group.as_bytes()));
                            assert_eq!(raw["info"]["epoch"], row.epoch);
                            assert_eq!(raw["info"]["group_info"], b64(&row.info));
                            assert_eq!(
                                raw["founding"]["group_info"].as_str().map(str::to_owned),
                                row.founding.as_deref().map(b64)
                            );
                            let log = raw["log"]["items"].as_array().unwrap();
                            assert_eq!(log.len(), row.log.len(), "{name}");
                            for (raw, item) in log.iter().zip(&row.log) {
                                same_log(raw, item);
                            }
                            assert!(row.complete);
                        }
                    }
                }
            }
            "joins" => {
                let entries: Vec<OutboxEntry> = asked["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| OutboxEntry {
                        id: 1,
                        kind: OutboxKind::ExternalCommit,
                        group: Some(group(&e["group"])),
                        epoch: e["epoch"].as_u64().unwrap(),
                        parts: vec![vec![1]; 4],
                    })
                    .collect();
                let Ok(Answer::Joins(answers)) = read(&Route::Joins { entries: &entries }) else {
                    panic!("{name} is not read")
                };
                let list = raw["answers"].as_array().unwrap();
                assert_eq!(list.len(), answers.len(), "{name}");
                for (raw, answer) in list.iter().zip(&answers) {
                    match answer {
                        Ok(answer) => {
                            assert_eq!(raw["epoch"].as_u64(), answer.epoch);
                            assert_eq!(raw["change"].as_u64(), answer.change);
                        }
                        Err(failure) => {
                            let code = raw["error"].as_str().map(str::to_owned);
                            let status = raw["status"].as_u64().unwrap() as u16;
                            assert_eq!(failure, &Failure::Refused { status, code });
                        }
                    }
                }
            }
            "chain" => {
                let g = group(&asked["group"]);
                let sender = DeviceId::from_slice(
                    &base64url_decode(asked["sender"].as_str().unwrap()).unwrap(),
                )
                .unwrap();
                let route = Route::Chain {
                    group: &g,
                    sender: &sender,
                    after: asked["after"].as_u64().unwrap(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                };
                let Ok(Answer::Chain { items, more }) = read(&route) else {
                    panic!("{name} is not read")
                };
                let list = raw["items"].as_array().unwrap();
                assert_eq!(list.len(), items.len(), "{name}");
                for (raw, item) in list.iter().zip(&items) {
                    assert_eq!(raw["seq"], item.seq);
                    same_envelope(raw, &item.item);
                    assert_eq!(
                        raw.get("cut").and_then(Value::as_bool).unwrap_or(false),
                        item.item.cut
                    );
                }
                assert_eq!(raw["more"], more);
            }
            "object" => {
                let object = ObjectId::from_slice(
                    &base64url_decode(asked["object"].as_str().unwrap()).unwrap(),
                )
                .unwrap();
                let route = Route::Object {
                    route: ObjectRoute::Cards,
                    object: &object,
                    after: asked["after"].as_u64().unwrap(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                };
                let Ok(Answer::Object(got)) = read(&route) else {
                    panic!("{name} is not read")
                };
                assert_eq!(raw["object_id"], b64(got.object.as_bytes()));
                assert_eq!(raw["group_id"], b64(got.group.as_bytes()));
                let list = raw["items"].as_array().unwrap();
                assert_eq!(list.len(), got.items.len(), "{name}");
                for (raw, item) in list.iter().zip(&got.items) {
                    same_envelope(raw, item);
                }
                assert_eq!(raw["more"], got.more);
            }
            "claim" => {
                let devices: Vec<DeviceId> = asked["devices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|d| {
                        DeviceId::from_slice(&base64url_decode(d.as_str().unwrap()).unwrap())
                            .unwrap()
                    })
                    .collect();
                let route = Route::ClaimKeyPackages {
                    devices: &devices,
                    partial: asked["partial"].as_bool().unwrap(),
                };
                if status != 200 {
                    assert!(
                        matches!(read(&route), Err(Failure::Refused { code: Some(_), .. })),
                        "{name}"
                    );
                    continue;
                }
                let Ok(Answer::Claimed {
                    key_packages,
                    missing,
                }) = read(&route)
                else {
                    panic!("{name} is not read")
                };
                assert_eq!(
                    raw["key_packages"].as_object().unwrap().len(),
                    key_packages.len(),
                    "{name}"
                );
                for (device, bytes) in &key_packages {
                    assert_eq!(raw["key_packages"][b64(device.as_bytes())], b64(bytes));
                }
                let named: Vec<Value> = missing.iter().map(|d| b64(d.as_bytes()).into()).collect();
                assert_eq!(
                    raw.get("missing").cloned().unwrap_or(Value::Array(vec![])),
                    Value::Array(named)
                );
            }
            "wish" => {
                let g = GroupId::room(RoomId::new([1; 32]));
                let route = Route::Wish {
                    kind: WishKind::Handover,
                    group: Some(&g),
                    key_package: None,
                };
                let (request, expect) = Codec.request(&SITE, &route).unwrap();
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str()
                );
                let Ok(Answer::Wished { id }) = Codec.answer(&expect, &response) else {
                    panic!("{name} is not read")
                };
                assert_eq!(raw["id"], id);
            }
            "reject" => {
                let g = group(&asked["group"]);
                let route = Route::RejectCommit {
                    group: &g,
                    n: asked["n"].as_u64().unwrap(),
                };
                assert_eq!(read(&route), Ok(Answer::Done), "{name}");
                assert!(raw["id"].is_u64());
            }
            "wishes" => {
                let Ok(Answer::Wishes(rows)) = read(&Route::Wishes) else {
                    panic!("{name} is not read")
                };
                let list = raw.as_array().unwrap();
                assert_eq!(list.len(), rows.len(), "{name}");
                for (raw, row) in list.iter().zip(&rows) {
                    assert_eq!(raw["id"], row.id);
                    assert_eq!(raw["device"], b64(row.device.as_bytes()));
                    assert_eq!(raw["kind"], format!("{:?}", row.kind).to_lowercase());
                    assert_eq!(raw["at"].as_u64(), row.at);
                    assert_eq!(raw.get("n").and_then(Value::as_u64), row.n);
                    assert_eq!(
                        raw.get("group_id")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        row.group.map(|g| b64(g.as_bytes()))
                    );
                    assert_eq!(
                        raw.get("committer")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        row.committer.map(|d| b64(d.as_bytes()))
                    );
                    assert_eq!(
                        raw.get("key_package")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                        row.key_package.as_deref().map(b64)
                    );
                }
            }
            "desk" => {
                let only = match asked["only"].as_str().unwrap() {
                    "active" => DeskOnly::Active,
                    "archived" => DeskOnly::Archived,
                    _ => DeskOnly::All,
                };
                let Ok(Answer::Desk(desk)) = read(&Route::Desk { only }) else {
                    panic!("{name} is not read")
                };
                let kinds = [
                    ("cards", &desk.cards),
                    ("notes", &desk.notes),
                    ("permission_requests", &desk.permission_requests),
                    ("artifacts", &desk.artifacts),
                ];
                for (list, rows) in kinds {
                    let raw = raw[list].as_array().unwrap();
                    assert_eq!(raw.len(), rows.len(), "{name}");
                    for (raw, row) in raw.iter().zip(rows) {
                        assert_eq!(raw["object_id"], b64(row.object.as_bytes()));
                        assert_eq!(raw["group_id"], b64(row.group.as_bytes()));
                        assert_eq!(raw["owner"], b64(row.owner.as_bytes()));
                        assert_eq!(
                            (&raw["state"], &raw["urgency"]),
                            (&row.state.into(), &row.urgency.into())
                        );
                        assert_eq!(raw["answered_at"], row.answered_at);
                        assert_eq!(
                            (&raw["first_change"], &raw["head_change"]),
                            (&row.first_change.into(), &row.head_change.into())
                        );
                        let version = row.version.as_ref().expect("an open object's version");
                        same_envelope(&raw["version"], version);
                        // what the codec checked the version by: its own header names the object and the group
                        let header = Envelope::decode(&version.envelope)
                            .expect("the hub's envelope decodes")
                            .header;
                        assert_eq!(header.group, row.group);
                        assert_eq!(
                            header.subject.object().map(|f| f.object_id),
                            Some(row.object)
                        );
                    }
                }
                let registers = raw["registers"].as_array().unwrap();
                assert_eq!(registers.len(), desk.registers.len(), "{name}");
                for (raw, item) in registers.iter().zip(&desk.registers) {
                    same_envelope(raw, item);
                    assert!(Envelope::decode(&item.envelope).is_ok());
                }
                assert_eq!(
                    raw["groups"].as_array().map(Vec::len),
                    desk.groups.as_ref().map(Vec::len)
                );
                let counts = raw["counts"].as_array().expect("the Desk's counts");
                let got = desk.counts.as_ref().expect("the counts read");
                assert_eq!(counts.len(), got.len(), "{name}");
                for (raw, row) in counts.iter().zip(got) {
                    assert_eq!(raw["group_id"], b64(row.group.as_bytes()));
                    for (kind, n) in [
                        ("cards", row.cards),
                        ("permission_requests", row.permission_requests),
                        ("notes", row.notes),
                        ("artifacts", row.artifacts),
                    ] {
                        assert_eq!(
                            raw[kind],
                            serde_json::json!({ "open": n.open, "answered": n.answered, "closed": n.closed }),
                            "{name}"
                        );
                    }
                }
                assert_eq!(
                    (&raw["truncated"], &raw["change"]),
                    (&desk.truncated.into(), &desk.change.into())
                );
            }
            "settled" => {
                let one;
                let only = match &asked["only"] {
                    Value::String(word) if word == "active" => Only::Active,
                    Value::String(_) => Only::All,
                    with_group => {
                        one = group(&with_group["group"]);
                        Only::Group(&one)
                    }
                };
                let kind = asked["kind"].as_str().map(|k| match k {
                    "cards" => ObjectRoute::Cards,
                    "notes" => ObjectRoute::Notes,
                    "permission_requests" => ObjectRoute::PermissionRequests,
                    _ => ObjectRoute::Artifacts,
                });
                let route = Route::Settled {
                    kind,
                    only,
                    before: asked["before"].as_u64(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                };
                let Ok(Answer::Settled(page)) = read(&route) else {
                    panic!("{name} is not read")
                };
                let items = raw["items"].as_array().unwrap();
                assert_eq!(items.len(), page.items.len(), "{name}");
                for (raw, item) in items.iter().zip(&page.items) {
                    let row = &item.row;
                    assert_eq!(raw["object_id"], b64(row.object.as_bytes()));
                    assert_eq!(raw["group_id"], b64(row.group.as_bytes()));
                    assert_eq!(raw["owner"], b64(row.owner.as_bytes()));
                    assert_eq!(
                        (&raw["state"], &raw["urgency"], &raw["answered_at"]),
                        (
                            &row.state.into(),
                            &row.urgency.into(),
                            &row.answered_at.into()
                        )
                    );
                    assert_eq!(
                        (&raw["first_change"], &raw["head_change"]),
                        (&row.first_change.into(), &row.head_change.into())
                    );
                    assert_eq!(raw["kind"], "cards");
                    assert_eq!(item.kind, ObjectRoute::Cards);
                    same_envelope(&raw["version"], row.version.as_ref().expect("a version"));
                }
                assert_eq!(
                    (&raw["more"], &raw["before"], &raw["change"]),
                    (&page.more.into(), &page.before.into(), &page.change.into())
                );
            }
            "chat" | "board" => {
                // (a Chat is a session's, or a card's)
                let of_card = asked.get("card").is_some();
                let named = match route {
                    "chat" if of_card => "card",
                    "chat" => "session",
                    _ => "board",
                };
                let id: [u8; 16] = base64url_decode(asked[named].as_str().unwrap())
                    .unwrap()
                    .try_into()
                    .unwrap();
                let (session, board, card) =
                    (SessionId::new(id), BoardId::new(id), ObjectId::new(id));
                let limit = asked["limit"].as_u64().unwrap() as u32;
                let route = match route {
                    "chat" => Route::Chat {
                        timeline: match of_card {
                            true => Timeline::Card(&card),
                            false => Timeline::Session(&session),
                        },
                        before: asked["before"].as_u64(),
                        limit,
                    },
                    _ => Route::Board {
                        board: &board,
                        after: asked["after"].as_u64().unwrap(),
                        limit,
                    },
                };
                let Ok(Answer::Envelopes { items, more }) = read(&route) else {
                    panic!("{name} is not read")
                };
                let list = raw["items"].as_array().unwrap();
                assert_eq!(list.len(), items.len(), "{name}");
                for (raw, item) in list.iter().zip(&items) {
                    same_envelope(raw, item);
                }
                assert_eq!(raw["more"], more);
            }
            "board_frontier" => {
                let board = BoardId::new(
                    base64url_decode(asked["board"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                );
                let bytes = |v: &Value| base64url_decode(v.as_str().unwrap()).unwrap();
                let head = |v: &Value| {
                    (
                        v[0].as_u64().unwrap(),
                        Hash32::from_slice(&bytes(&v[1])).unwrap(),
                    )
                };
                let frontier: Vec<(DeviceId, u64, Hash32)> = asked
                    .get("frontier")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|w| {
                        (
                            DeviceId::from_slice(&bytes(&w[0])).unwrap(),
                            w[1].as_u64().unwrap(),
                            Hash32::from_slice(&bytes(&w[2])).unwrap(),
                        )
                    })
                    .collect();
                let route = Route::BoardFrontier {
                    board: &board,
                    frontier: &frontier,
                    files: &[],
                    snapshot: asked.get("snapshot").filter(|s| !s.is_null()).map(head),
                };
                // (the body recorded names the writers and the snapshot the codec writes)
                if !frontier.is_empty() {
                    let (request, _) = Codec.request(&SITE, &route).unwrap();
                    let (ours, theirs): (Value, Value) = (
                        serde_json::from_slice(request.body.as_ref().unwrap()).unwrap(),
                        serde_json::from_str(v["request"]["body"].as_str().unwrap()).unwrap(),
                    );
                    assert_eq!(ours, theirs, "{name}");
                }
                let Ok(Answer::Pruned { pruned }) = read(&route) else {
                    panic!("{name} is not read")
                };
                assert_eq!(raw["pruned"], pruned);
            }
            "sealed_keys" => {
                let route = Route::SealedKeys {
                    after: asked["after"].as_u64().unwrap(),
                    limit: asked["limit"].as_u64().unwrap() as u32,
                };
                let Ok(Answer::SealedKeys(page)) = read(&route) else {
                    panic!("{name} is not read")
                };
                let rows = raw["rows"].as_array().unwrap();
                assert_eq!(rows.len(), page.rows.len(), "{name}");
                for (raw, (change, key)) in rows.iter().zip(&page.rows) {
                    assert_eq!(
                        (&raw["change"], &raw["sealed_key"]),
                        (&(*change).into(), &b64(key).into())
                    );
                }
                assert_eq!(
                    raw["links"].as_array().unwrap().len(),
                    page.links.len(),
                    "{name}"
                );
                assert_eq!(
                    (&raw["change"], &raw["more"]),
                    (&page.change.into(), &page.more.into())
                );
            }
            "put_file" | "get_file" | "delete_file" => {
                let file = FileId::new(
                    base64url_decode(asked["file"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                );
                match route {
                    "put_file" => {
                        let bytes = vec![0; asked["size"].as_u64().unwrap() as usize];
                        let Ok(Answer::FileStored { size, sha256 }) = read(&Route::PutFile {
                            file: &file,
                            bytes: &bytes,
                        }) else {
                            panic!("{name} is not read")
                        };
                        assert_eq!(
                            (&raw["size"], &raw["sha256"]),
                            (&size.into(), &b64(&sha256).into())
                        );
                    }
                    "delete_file" => assert_eq!(
                        read(&Route::DeleteFile { file: &file }),
                        Ok(Answer::Done),
                        "{name}"
                    ),
                    _ => {
                        let range = asked["first"].as_u64().map(|first| ByteRange {
                            first,
                            last: asked["last"].as_u64(),
                        });
                        let (request, expect) = Codec
                            .request(&SITE, &Route::GetFile { file: &file, range })
                            .unwrap();
                        assert_eq!(
                            request.url.strip_prefix(SITE.url),
                            v["request"]["path"].as_str(),
                            "{name}"
                        );
                        let headers = v["content_range"]
                            .as_str()
                            .map(|value| Header {
                                name: "Content-Range".into(),
                                value: value.into(),
                            })
                            .into_iter()
                            .collect();
                        let response = HttpResponse {
                            status,
                            headers,
                            body: answer.as_bytes().to_vec().into(),
                        };
                        let got = Codec.answer(&expect, &response);
                        if status >= 400 {
                            assert!(
                                matches!(got, Err(Failure::Refused { code: Some(_), .. })),
                                "{name}"
                            );
                            continue;
                        }
                        let Ok(Answer::File(got)) = got else {
                            panic!("{name} is not read: {got:?}")
                        };
                        assert_eq!(got.bytes, answer.as_bytes(), "{name}");
                        assert_eq!(
                            (got.size, got.first),
                            (1000, range.map_or(0, |r| r.first)),
                            "{name}"
                        );
                    }
                }
            }
            "post_share" | "delete_share" => {
                let share = ShareId::new(
                    base64url_decode(asked["share"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                );
                if route == "delete_share" {
                    assert_eq!(
                        read(&Route::DeleteShare { share: &share }),
                        Ok(Answer::Done),
                        "{name}"
                    );
                    continue;
                }
                let file = FileId::new(
                    base64url_decode(asked["file"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap(),
                );
                let expires_at = asked["expires_at"].as_u64().unwrap();
                let route = Route::PostShare {
                    share: &share,
                    secret_hash: &[0; 32],
                    file: &file,
                    expires_at,
                };
                assert_eq!(
                    read(&route),
                    Ok(Answer::Shared {
                        expires_at: raw["expires_at"].as_u64().unwrap()
                    }),
                    "{name}"
                );
            }
            "sign_out" => assert_eq!(read(&Route::SignOut), Ok(Answer::Done), "{name}"),
            "open_recovery" | "drop_recovery" => {
                let room =
                    RoomId::from_slice(&base64url_decode(asked["room"].as_str().unwrap()).unwrap())
                        .unwrap();
                if route == "open_recovery" {
                    let Ok(Answer::Recovery { id, expires_at }) =
                        read(&Route::OpenRecovery { room: &room })
                    else {
                        panic!("{name}")
                    };
                    assert_eq!(
                        (&raw["recovery_id"], &raw["expires_at"]),
                        (&b64(&id).into(), &expires_at.into())
                    );
                } else {
                    let id: [u8; 16] = base64url_decode(asked["recovery"].as_str().unwrap())
                        .unwrap()
                        .try_into()
                        .unwrap();
                    let got = read(&Route::DropRecovery {
                        room: &room,
                        recovery: &id,
                    });
                    assert_eq!(
                        got,
                        Ok(Answer::Dropped {
                            dropped: raw["dropped"].as_bool().unwrap()
                        }),
                        "{name}"
                    );
                }
            }
            "create_account"
            | "account"
            | "account_usage"
            | "login"
            | "recover"
            | "passkey_challenge"
            | "account_passkey_challenge"
            | "put_kit"
            | "put_password"
            | "put_email"
            | "passkey_login"
            | "add_passkey"
            | "remove_passkey"
            | "delete_account" => {
                let kit = KitPart {
                    auth_key: [1; 32],
                    sealed_copy: [2; 61],
                };
                let kdf = Kdf(vec![("alg".into(), KdfValue::Text("argon2id".into()))]);
                let password = NewPassword {
                    auth_key: [3; 32],
                    sealed_copy: [2; 61],
                    kdf,
                };
                let proof = AccountProof::Kit { auth_key: [1; 32] };
                let new = NewAccount {
                    email: None,
                    kit: kit.clone(),
                    password: Some(password.clone()),
                    passkey: None,
                };
                let nobody = Site {
                    token: None,
                    ..SITE
                };
                let assertion = PasskeyAssertion {
                    credential_id: vec![4; 20],
                    authenticator_data: vec![5; 37],
                    client_data_json: vec![6; 80],
                    signature: vec![7; 70],
                    user_handle: None,
                };
                let passkey = PasskeyRegistration {
                    attestation_object: vec![8; 120],
                    client_data_json: vec![6; 80],
                    sealed_copy: [2; 61],
                    transports: None,
                };
                let credential = asked
                    .get("credential_id")
                    .map(|c| base64url_decode(c.as_str().unwrap()).unwrap())
                    .unwrap_or_default();
                let (site, route) = match route {
                    "create_account" => (&SITE, Route::CreateAccount { account: &new }),
                    "put_email" => (
                        &SITE,
                        Route::PutEmail {
                            email: "a@b.example",
                            kit: &kit,
                            revision: 1,
                            proof: &proof,
                        },
                    ),
                    "passkey_login" => (
                        &nobody,
                        Route::PasskeyLogin {
                            assertion: &assertion,
                        },
                    ),
                    "add_passkey" => (
                        &SITE,
                        Route::AddPasskey {
                            passkey: &passkey,
                            proof: &proof,
                        },
                    ),
                    "remove_passkey" => (
                        &SITE,
                        Route::RemovePasskey {
                            credential_id: &credential,
                            proof: &proof,
                        },
                    ),
                    "account" => (&SITE, Route::Account),
                    "account_usage" => (&SITE, Route::AccountUsage),
                    "login" => (
                        &nobody,
                        Route::Login {
                            account: "a@b.example",
                            auth_key: &[1; 32],
                        },
                    ),
                    "recover" => (
                        &nobody,
                        Route::Recover {
                            account: "a@b.example",
                            auth_key: &[1; 32],
                        },
                    ),
                    "passkey_challenge" => (&nobody, Route::PasskeyChallenge),
                    "account_passkey_challenge" => (&SITE, Route::AccountPasskeyChallenge),
                    "put_kit" => (
                        &SITE,
                        Route::PutKit {
                            kit: &kit,
                            revision: 1,
                            proof: &proof,
                        },
                    ),
                    "put_password" => (
                        &SITE,
                        Route::PutPassword {
                            password: &password,
                            revision: 1,
                            proof: &proof,
                        },
                    ),
                    _ => (&SITE, Route::DeleteAccount { proof: &proof }),
                };
                let (request, expect) = Codec.request(site, &route).unwrap();
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str(),
                    "{name}"
                );
                assert_eq!(request.method, v["request"]["method"], "{name}");
                let got = Codec.answer(&expect, &response);
                if status >= 400 {
                    let refusal = Codec.refusal(&response).expect("the hub's own refusal");
                    match got {
                        Err(Failure::Refused { code, .. }) => {
                            assert_eq!(code.as_deref(), Some(refusal.code), "{name}")
                        }
                        Err(Failure::NotNow { retry_after_ms, .. }) => {
                            assert_eq!(retry_after_ms, refusal.retry_after_ms, "{name}")
                        }
                        other => panic!("{name}: {other:?}"),
                    }
                    continue;
                }
                match got {
                    Ok(Answer::Account(view)) => {
                        assert_eq!(raw["account"], view.account);
                        assert_eq!(raw["email"].as_str(), view.email.as_deref());
                        assert_eq!(raw["revision"], view.revision);
                        assert_eq!(raw["has_password"], view.has_password);
                        assert_eq!(raw["kit_copy"], b64(&view.kit_copy));
                        assert_eq!(
                            raw["password_copy"].as_str().map(str::to_owned),
                            view.password_copy.map(|c| b64(&c))
                        );
                        assert_eq!(raw["user_handle"], b64(&view.user_handle));
                        assert_eq!(
                            raw["kit_form"],
                            format!("{:?}", view.kit_form).to_lowercase()
                        );
                        assert_eq!(
                            raw["kdf"].as_object().map(|k| k.len()),
                            view.kdf.as_ref().map(|k| k.0.len())
                        );
                        for (key, value) in view.kdf.iter().flat_map(|k| &k.0) {
                            match value {
                                KdfValue::Text(text) => assert_eq!(raw["kdf"][key], text.as_str()),
                                KdfValue::Number(n) => assert_eq!(raw["kdf"][key], *n),
                            }
                        }
                        let passkeys = raw["passkeys"].as_array().unwrap();
                        assert_eq!(passkeys.len(), view.passkeys.len());
                        for (raw, row) in passkeys.iter().zip(&view.passkeys) {
                            assert_eq!(raw["credential_id"], b64(&row.credential_id));
                            assert_eq!(raw["sealed_copy"], b64(&row.sealed_copy));
                            assert_eq!(raw["created_at"], row.created_at);
                            assert_eq!(raw["last_used_at"].as_u64(), row.last_used_at);
                            let transports: Vec<Value> =
                                row.transports.iter().map(|t| t.as_str().into()).collect();
                            assert_eq!(
                                raw.get("transports")
                                    .cloned()
                                    .unwrap_or(Value::Array(vec![])),
                                Value::Array(transports)
                            );
                        }
                        let rooms: Vec<Value> = view
                            .rooms
                            .iter()
                            .map(|r| b64(r.as_bytes()).into())
                            .collect();
                        assert_eq!(raw["rooms"], Value::Array(rooms));
                    }
                    Ok(Answer::Usage(usage)) => {
                        assert_eq!(raw["rooms"], usage.rooms);
                        assert_eq!(raw["devices"], usage.devices);
                        assert_eq!(raw["envelopes"], usage.envelopes);
                        assert_eq!(raw["log_bytes"], usage.log_bytes);
                        assert_eq!(raw["total_bytes"], usage.total_bytes);
                        assert_eq!(raw.as_object().unwrap().len(), 14, "every count is read");
                    }
                    Ok(Answer::Login(login)) => {
                        assert_eq!(raw["account"], login.account);
                        assert_eq!(raw["email"].as_str(), login.email.as_deref());
                        let rooms = raw["rooms"].as_array().unwrap();
                        assert_eq!(rooms.len(), login.rooms.len());
                        for (raw, room) in rooms.iter().zip(&login.rooms) {
                            assert_eq!(raw["room_id"], b64(room.room.as_bytes()));
                            assert_eq!(raw["sealed_copy"], b64(&room.sealed_copy));
                            assert_eq!(raw["challenge"], b64(&room.challenge));
                        }
                    }
                    Ok(Answer::PasskeyChallenge(challenge)) => {
                        assert_eq!(raw["challenge"], b64(&challenge.challenge));
                        assert_eq!(raw["account"], challenge.account);
                        assert_eq!(raw["user_handle"], b64(&challenge.user_handle));
                        assert_eq!(raw.get("kit_form").is_some(), challenge.kit_form.is_some());
                    }
                    Ok(Answer::Revision { revision }) => assert_eq!(raw["revision"], revision),
                    Ok(Answer::PasskeyAdded {
                        credential_id,
                        created_at,
                    }) => {
                        assert_eq!(raw["credential_id"], b64(&credential_id));
                        assert_eq!(raw["created_at"], created_at);
                    }
                    Ok(Answer::Done) => assert_eq!(raw["deleted"], true),
                    other => panic!("{name} is not read: {other:?}"),
                }
            }
            "post_invite"
            | "get_invite"
            | "post_invite_request"
            | "get_reveal"
            | "put_reveal"
            | "delete_invite" => {
                let invite = asked
                    .get("invite")
                    .map(|i| {
                        InviteId::new(
                            base64url_decode(i.as_str().unwrap())
                                .unwrap()
                                .try_into()
                                .unwrap(),
                        )
                    })
                    .unwrap_or(InviteId::new([0; 16]));
                let nobody = Site {
                    token: None,
                    ..SITE
                };
                // (an invite read with a token also lists its Requests)
                let signed_in = raw.get("requests").is_some_and(|r| !r.is_null());
                let (site, route) = match route {
                    "post_invite" => (
                        &SITE,
                        Route::PostInvite {
                            offer: &[1],
                            signature: &[2],
                            mac: &[3],
                        },
                    ),
                    "get_invite" => (
                        if signed_in { &SITE } else { &nobody },
                        Route::GetInvite { invite: &invite },
                    ),
                    "post_invite_request" => (
                        &nobody,
                        Route::PostInviteRequest {
                            invite: &invite,
                            request: &[1],
                            mac: &[2],
                            signature: &[3],
                        },
                    ),
                    "get_reveal" => (&nobody, Route::GetReveal { invite: &invite }),
                    "put_reveal" => (
                        &SITE,
                        Route::PutReveal {
                            invite: &invite,
                            reveal: &[1],
                            signature: &[2],
                        },
                    ),
                    _ => (&SITE, Route::DeleteInvite { invite: &invite }),
                };
                let (request, expect) = Codec.request(site, &route).unwrap();
                assert_eq!(
                    request.url.strip_prefix(SITE.url),
                    v["request"]["path"].as_str(),
                    "{name}"
                );
                assert_eq!(request.method, v["request"]["method"], "{name}");
                match Codec.answer(&expect, &response) {
                    Ok(Answer::Invited { invite }) => {
                        assert_eq!(raw["invite_id"], b64(invite.as_bytes()))
                    }
                    Ok(Answer::Invite(offer)) => {
                        assert_eq!(raw["offer"], b64(&offer.offer));
                        assert_eq!(raw["signature"], b64(&offer.signature));
                        assert_eq!(
                            raw["mac"].as_str().map(str::to_owned),
                            offer.mac.as_deref().map(b64)
                        );
                        assert_eq!(raw["expires_at"], offer.expires_at);
                        let requests = offer.requests.unwrap_or_default();
                        assert_eq!(
                            raw["requests"].as_array().map_or(0, Vec::len),
                            requests.len(),
                            "{name}"
                        );
                        for (raw, request) in raw["requests"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .zip(&requests)
                        {
                            assert_eq!(raw["request"], b64(&request.request));
                            assert_eq!(raw["mac"], b64(&request.mac));
                            assert_eq!(raw["signature"], b64(&request.signature));
                        }
                    }
                    Ok(Answer::Requested { request_hash }) => {
                        assert_eq!(raw["request_hash"], b64(&request_hash))
                    }
                    Ok(Answer::Reveal { reveal, signature }) => {
                        assert_eq!(
                            (&raw["reveal"], &raw["signature"]),
                            (&b64(&reveal).into(), &b64(&signature).into())
                        );
                    }
                    Ok(Answer::Done) => assert!(raw["revealed"] == true || raw["burned"] == true),
                    Err(Failure::Refused {
                        code: Some(code), ..
                    }) => assert_eq!(raw["error"], code),
                    other => panic!("{name} is not read: {other:?}"),
                }
            }
            "push_register" | "push_state" | "push_remove" | "live_activity" => {
                let registration = PushRegistration {
                    target: PushTarget::WebPush {
                        endpoint: "https://push.example/1".into(),
                        p256dh: vec![4; 65],
                        auth: vec![1; 16],
                    },
                    level: PushLevel::All,
                };
                let activity = LiveActivity {
                    start: true,
                    token: "ab".into(),
                    tag: "t".into(),
                    environment: "development".into(),
                    topic: "x".into(),
                };
                let route = match route {
                    "push_register" => Route::PushRegister {
                        registration: &registration,
                    },
                    "push_state" => Route::PushState,
                    "push_remove" => Route::PushRemove {
                        endpoint: Some("https://push.example/1"),
                    },
                    _ => Route::LiveActivity {
                        activity: &activity,
                    },
                };
                match read(&route) {
                    Ok(Answer::Push(state)) => {
                        let rows = raw["subscriptions"].as_array().unwrap();
                        assert_eq!(rows.len(), state.subscriptions.len());
                        for (raw, row) in rows.iter().zip(&state.subscriptions) {
                            assert_eq!(raw["endpoint"], row.endpoint);
                            assert_eq!(raw["created_at"], row.created_at);
                            assert_eq!(raw["level"], format!("{:?}", row.level).to_lowercase());
                        }
                        assert_eq!(raw["vapid_public_key"], b64(&state.vapid_public_key));
                        assert_eq!(raw["apns"], state.apns);
                    }
                    Ok(Answer::Removed { deleted }) => assert_eq!(raw["deleted"], deleted),
                    Ok(Answer::Done) => assert_eq!(raw["registered"], true),
                    Err(Failure::Refused {
                        code: Some(code), ..
                    }) => assert_eq!(raw["error"], code),
                    other => panic!("{name} is not read: {other:?}"),
                }
            }
            other => panic!("{name}: a route this test does not know: {other}"),
        }
    }
    // every route has a vector of the real hub
    let built = [
        "account",
        "account_passkey_challenge",
        "account_usage",
        "add_passkey",
        "board",
        "board_frontier",
        "chain",
        "challenge",
        "changes",
        "chat",
        "claim",
        "create_account",
        "delete_account",
        "delete_file",
        "delete_invite",
        "delete_share",
        "desk",
        "desk_brief",
        "drop_recovery",
        "get_file",
        "get_invite",
        "get_reveal",
        "group_info",
        "group_log",
        "groups_read",
        "joins",
        "key_packages_held",
        "live_activity",
        "login",
        "object",
        "open_recovery",
        "outbox",
        "passkey_challenge",
        "passkey_login",
        "post_invite",
        "post_invite_request",
        "post_share",
        "push_register",
        "push_remove",
        "push_state",
        "put_email",
        "put_file",
        "put_kit",
        "put_password",
        "put_reveal",
        "recover",
        "reject",
        "remove_passkey",
        "room_groups",
        "sealed_keys",
        "settled",
        "sign_out",
        "stream",
        "token",
        "welcomes",
        "wish",
        "wishes",
    ];
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), built);
    assert!(all.len() >= 105, "{} vectors", all.len());
}
