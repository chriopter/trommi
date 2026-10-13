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
    Answer, ChangeItem, Codec, EnvelopeItem, Failure, LogItem, Only, Route, Site, StreamEvent,
    StreamReader,
};
use trommi_core::ids::{base64url_decode, base64url_encode as b64, GroupId, RoomId};
use trommi_core::store::{OutboxEntry, OutboxKind};

const SITE: Site<'static> = Site {
    url: "http://127.0.0.1:1",
    token: Some("abcdefghijklmnop_-0123"),
    client: "test/1.0.0",
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
                    "Message" => OutboxKind::Message,
                    "RelayMessage" => OutboxKind::RelayMessage,
                    "Envelope" => OutboxKind::Envelope,
                    "KeyPackages" => OutboxKind::KeyPackages,
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
                let founded = raw
                    .get("room_id")
                    .or(raw.get("group_id"))
                    .and_then(Value::as_str);
                assert_eq!(
                    founded.map(str::to_owned),
                    answer.founded.map(|g| b64(g.as_bytes())),
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
            other => panic!("{name}: a route this test does not know: {other}"),
        }
    }
    // every route that is built has a vector of the real hub
    let built = [
        "changes",
        "desk_brief",
        "key_packages_held",
        "outbox",
        "stream",
    ];
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), built);
    assert!(all.len() >= 20, "{} vectors", all.len());
}
