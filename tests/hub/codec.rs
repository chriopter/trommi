//! The app core's hub codec (core/client/src/hub) against the real hub: every request the codec builds is taken by
//! the hub, and every answer the hub gives is read by the codec.
//!
//! The same run records the test vectors of `core/client/tests/hub/`: with `TROMMI_HUB_VECTORS=write` each
//! exchange is written there as one JSON file (what was asked, the status, the raw answer). The client core's own
//! test (`core/client/tests/hub_vectors.rs`) reads them back without a hub and checks that nothing is lost; a
//! stand-in hub of another test can serve them as they are. The room is a throwaway of this test: its keys are
//! made here and belong to nobody.

mod common;

use std::io::{BufRead, BufReader, Read};
use std::time::Duration;

use common::*;
use serde_json::{json, Value};
use trommi_client_core::app::{Header, HttpRequest, HttpResponse};
use trommi_client_core::hub::{
    Answer, ChangeItem, Codec, Expect, Failure, Only, OutboxExtra, Route, Site, StreamEvent, StreamReader,
};
use trommi_core::ids::{GroupId, RoomId};
use trommi_core::store::{OutboxEntry, OutboxKind};
use trommi_hub::util::{b64, random};
use trommi_wire::layout::{self as wire, ZERO16, ZERO32};

struct Recorder {
    port: u16,
    url: String,
    token: Option<String>,
    write: bool,
}

impl Recorder {
    fn site(&self) -> Site<'_> {
        Site {
            url: &self.url,
            token: self.token.as_deref(),
            client: "test/1.0.0",
            lease: None,
        }
    }

    /// Sends exactly the request the codec built (the harness names the client itself).
    fn send(&self, request: &HttpRequest) -> HttpResponse {
        let path = request.url.strip_prefix(&self.url).expect("a URL at this hub");
        let headers: Vec<(&str, String)> = request
            .headers
            .iter()
            .filter(|h| h.name != "trommi-client")
            .map(|h| (h.name.as_str(), h.value.clone()))
            .collect();
        let body = request.body.as_ref().map_or(&[][..], |b| &b[..]);
        let reply = common::request(self.port, &request.method, path, &headers, body);
        assert!(reply.body.len() <= request.max_answer as usize, "an answer above the cap of {path}");
        HttpResponse {
            status: reply.status,
            headers: reply
                .headers
                .iter()
                .map(|(name, value)| Header {
                    name: name.clone(),
                    value: value.clone(),
                })
                .collect(),
            body: reply.body.into(),
        }
    }

    /// One exchange: asked through the codec, answered by the hub, read by the codec, recorded under `name`.
    fn ask(&self, name: &str, route: &Route<'_>, asked: Value) -> Result<Answer, Failure> {
        let (request, expect) = Codec.request(&self.site(), route).expect("the codec builds the route");
        let response = self.send(&request);
        self.record(name, &request, asked, &response);
        self.read(&expect, &response)
    }

    fn read(&self, expect: &Expect, response: &HttpResponse) -> Result<Answer, Failure> {
        Codec.answer(expect, response)
    }

    fn record(&self, name: &str, request: &HttpRequest, asked: Value, response: &HttpResponse) {
        if !self.write || name.is_empty() {
            return;
        }
        let (status, answer) = (response.status, &response.body[..]);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/client/tests/hub");
        std::fs::create_dir_all(&dir).unwrap();
        let vector = json!({
            "name": name,
            "asked": asked,
            "request": {
                "method": request.method,
                "path": request.url.strip_prefix(&self.url).unwrap(),
                "body": request.body.as_ref().map(|b| String::from_utf8(b.to_vec()).unwrap()),
            },
            "status": status,
            "content_range": response.header("content-range"),
            "answer": String::from_utf8(answer.to_vec()).expect("the hub answers text"),
        });
        let text = serde_json::to_string_pretty(&vector).unwrap() + "\n";
        std::fs::write(dir.join(format!("{name}.json")), text).unwrap();
    }

    /// The stream's raw bytes (the chunked coding taken off) until nothing came for a while.
    fn stream(&self, request: &HttpRequest) -> (u16, Vec<Header>, Vec<u8>) {
        let path = request.url.strip_prefix(&self.url).unwrap();
        let headers: Vec<(&str, String)> = request
            .headers
            .iter()
            .filter(|h| h.name != "trommi-client")
            .map(|h| (h.name.as_str(), h.value.clone()))
            .collect();
        let socket = send_request(self.port, "GET", path, &headers, &[]);
        socket.set_read_timeout(Some(Duration::from_millis(1500))).unwrap();
        let mut reader = BufReader::new(socket);
        let (status, head) = read_head(&mut reader);
        let head: Vec<Header> = head
            .into_iter()
            .map(|(name, value)| Header { name, value })
            .collect();
        let mut body = Vec::new();
        let length = head
            .iter()
            .find(|h| h.name == "content-length")
            .and_then(|h| h.value.parse::<usize>().ok());
        if let Some(length) = length {
            body.resize(length, 0);
            reader.read_exact(&mut body).unwrap();
            return (status, head, body);
        }
        loop {
            let mut size = String::new();
            match reader.read_line(&mut size) {
                Ok(n) if n > 0 => {}
                _ => break,
            }
            let Ok(size) = usize::from_str_radix(size.trim(), 16) else { break };
            let mut chunk = vec![0; size + 2];
            if size == 0 || reader.read_exact(&mut chunk).is_err() {
                break;
            }
            body.extend_from_slice(&chunk[..size]);
        }
        (status, head, body)
    }
}

fn group_id(bytes: &[u8]) -> GroupId {
    GroupId::from_bytes(bytes).unwrap()
}

fn entry(kind: OutboxKind, group: Option<&[u8]>, epoch: u64, parts: Vec<Vec<u8>>) -> OutboxEntry {
    OutboxEntry {
        id: 1,
        kind,
        group: group.map(group_id),
        epoch,
        parts,
    }
}

fn asked_entry(e: &OutboxEntry) -> Value {
    json!({
        "route": "outbox",
        "kind": format!("{:?}", e.kind),
        "group": e.group.as_ref().map(|g| b64(g.as_bytes())),
        "epoch": e.epoch,
        "parts": e.parts.len(),
    })
}

#[test]
fn the_codec_and_the_real_hub_agree() {
    let mut w = World::on(TestHub::start_with(&[("HUB_LIMIT_ENVELOPES_PER_SECOND", "100000")]));
    let mut rec = Recorder {
        port: w.hub.port,
        url: w.hub.url.clone(),
        token: w.ada.token.clone(),
        write: std::env::var("TROMMI_HUB_VECTORS").as_deref() == Ok("write"),
    };
    let room = RoomId::new(w.room);
    let outbox = |rec: &Recorder, name: &str, e: &OutboxEntry, extra: Option<&OutboxExtra>| {
        rec.ask(name, &Route::Outbox { room: &room, entry: e, extra }, asked_entry(e))
    };

    // ---- a room with a main session, an agent, some Chat, two cards, a register
    let mut agent = w.enrol_agent();
    let (session, group) = w.found_main(&mut [], Some(&mut agent));
    for text in ["one", "two", "three"] {
        agent.send(&w.hub, &group, &chat(&session, w.ada.id(), text)).ok();
    }
    let (mut cards, mut versions) = (vec![], vec![]);
    for urgency in [0, 3] {
        let id = enc::object_id(&group, &agent.id(), agent.chain(&group).0 + 1);
        let card = object(wire::KIND_VERSION, id, wire::TYPE_CARD, wire::STATE_OPEN, urgency, ZERO32, ZERO32);
        agent.send(&w.hub, &group, &card).ok();
        cards.push(id);
        versions.push(agent.chain(&group).1);
    }
    agent.send(&w.hub, &group, &register(&random(), "a value")).ok();

    // ---- the outbox: an envelope, the same again, one that leaves a gap
    let (first, _) = w.ada.envelope(&group, &chat(&session, agent.id(), "from the human"));
    let e = entry(OutboxKind::Envelope, Some(&group), w.ada.epoch(&group), vec![first]);
    let Ok(Answer::Outbox(stored)) = outbox(&rec, "outbox-envelope", &e, None) else { panic!("an envelope is stored") };
    let Ok(Answer::Outbox(again)) = outbox(&rec, "outbox-envelope-again", &e, None) else { panic!("the first answer again") };
    assert_eq!(stored, again);
    assert!(stored.change.is_some());
    let (skipped, _) = w.ada.envelope(&group, &chat(&session, agent.id(), "skipped"));
    let (beyond, _) = w.ada.envelope(&group, &chat(&session, agent.id(), "beyond a gap"));
    let gap = entry(OutboxKind::Envelope, Some(&group), w.ada.epoch(&group), vec![beyond.clone()]);
    {
        let (request, expect) = Codec.request(&rec.site(), &Route::Outbox { room: &room, entry: &gap, extra: None }).unwrap();
        let response = rec.send(&request);
        rec.record("outbox-envelope-gap", &request, asked_entry(&gap), &response);
        assert_eq!(rec.read(&expect, &response), Err(Failure::Refused { status: 409, code: Some("gap".into()) }));
        let refusal = Codec.refusal(&response).expect("the hub's own refusal");
        assert_eq!((refusal.code, refusal.voided, refusal.seq), ("gap", false, Some(1)));
        assert!(!Codec.voided(&response));
    }
    for bytes in [skipped, beyond] {
        let e = entry(OutboxKind::Envelope, Some(&group), w.ada.epoch(&group), vec![bytes]);
        assert!(outbox(&rec, "outbox-envelope", &e, None).is_ok());
    }

    // ---- a Commit in the session group (an own-leaf update), then a message and a relayed one
    let now = w.ada.room_now();
    let out = w.ada.commit(&group, &Change::default(), now);
    let key = w.ada.sealed_key(&group, out.epoch + 1, &out.group_info, now.0, &w.recovery.hpke_public, true);
    let e = entry(
        OutboxKind::Commit,
        Some(&group),
        out.epoch,
        vec![out.commit.clone(), out.group_info.clone(), out.welcome.clone().unwrap_or_default(), key.clone()],
    );
    let Ok(Answer::Outbox(committed)) = outbox(&rec, "outbox-commit", &e, None) else { panic!("a Commit is taken") };
    assert_eq!(committed.epoch, Some(out.epoch + 1));
    w.ada.merge(&group);
    // a Commit on the epoch that is taken
    {
        let stale = entry(OutboxKind::Commit, Some(&group), out.epoch, vec![vec![1; 40], vec![2; 40], vec![], vec![3; 40]]);
        let (request, expect) = Codec.request(&rec.site(), &Route::Outbox { room: &room, entry: &stale, extra: None }).unwrap();
        let response = rec.send(&request);
        rec.record("outbox-commit-refused", &request, asked_entry(&stale), &response);
        assert_eq!(rec.read(&expect, &response), Err(Failure::Refused { status: 409, code: Some("epoch-taken".into()) }));
        // (the group's epoch now, for building again)
        assert_eq!(Codec.refusal(&response).and_then(|r| r.epoch), Some(out.epoch + 1));
    }
    // ---- the SealedKey of the group's epoch again, on its own (the same bytes: another one would be a replay)
    let e = entry(OutboxKind::SealedKey, Some(&group), out.epoch + 1, vec![key]);
    let alone = outbox(&rec, "outbox-sealed-key", &e, None);
    assert!(matches!(alone, Ok(Answer::Outbox(_))), "a SealedKey on its own is stored: {alone:?}");
    // (only the room group relays)
    let room_group = w.room.to_vec();
    for (kind, name, to) in [(OutboxKind::Message, "outbox-message", &group), (OutboxKind::RelayMessage, "outbox-relay", &room_group)] {
        let message = w.ada.application_message(to, b"a piece");
        let e = entry(kind, Some(to), w.ada.epoch(to), vec![message]);
        let Ok(Answer::Outbox(answer)) = outbox(&rec, name, &e, None) else { panic!("{name} is taken") };
        assert_eq!(answer.n.is_some(), kind == OutboxKind::Message);
    }

    // ---- KeyPackages, and how many the hub holds
    let mut parts = vec![w.ada.key_package(true)];
    parts.extend((0..3).map(|_| w.ada.key_package(false)));
    let e = entry(OutboxKind::KeyPackages, None, 0, parts);
    let Ok(Answer::Outbox(put)) = outbox(&rec, "outbox-key-packages", &e, None) else { panic!("KeyPackages are taken") };
    let held = rec.ask("key-packages-held", &Route::KeyPackagesHeld, json!({ "route": "key_packages_held" }));
    assert_eq!(held.ok(), Some(Answer::KeyPackagesHeld { unused: put.unused.unwrap() }));

    // ---- a second main session, founded through the outbox
    let second: [u8; 16] = random();
    // (an agent device is the agent leaf of one live main session)
    let other = w.enrol_agent();
    let adds = w.claim(&w.ada, &[other.id()]);
    let (group2, info0) = w.ada.create_session(&second, &ZERO16);
    let now = w.ada.room_now();
    let key0 = w.ada.sealed_key(&group2, 0, &info0, now.0, &w.recovery.hpke_public, true);
    let out = w.ada.commit(&group2, &Change { adds, ..Default::default() }, now);
    let key1 = w.ada.sealed_key(&group2, 1, &out.group_info, now.0, &w.recovery.hpke_public, true);
    let e = entry(
        OutboxKind::GroupFounding,
        Some(&group2),
        0,
        vec![info0, key0, out.commit.clone(), out.group_info.clone(), out.welcome.clone().unwrap_or_default(), key1],
    );
    let Ok(Answer::Outbox(founded)) = outbox(&rec, "outbox-group-founding", &e, None) else { panic!("a session is founded") };
    assert_eq!(founded.group, Some(group_id(&group2)));
    w.ada.merge(&group2);

    // ---- catch-up from the start, page by page; the same of the live sessions; one group's envelopes
    let mut total = Vec::new();
    for (only, word) in [(Only::All, "all"), (Only::Active, "active")] {
        let (mut after, mut pages, mut changes) = (0, 0, Vec::new());
        loop {
            let name = format!("changes-{word}-{pages}");
            let route = Route::Changes { after, limit: 6, only };
            let asked = json!({ "route": "changes", "after": after, "limit": 6, "only": word });
            let Ok(Answer::Changes(page)) = rec.ask(&name, &route, asked) else { panic!("{name} is read") };
            assert!(page.head.is_some());
            changes.extend(page.items.iter().map(ChangeItem::change));
            (after, pages) = (page.change, pages + 1);
            if !page.more {
                assert_eq!(page.head, Some(page.change));
                break;
            }
        }
        assert!(pages >= 3, "several pages: {pages}");
        total = changes;
    }
    let one = group_id(&group);
    let asked = json!({ "route": "changes", "after": 0, "limit": 200, "only": { "group": b64(&group) } });
    let Ok(Answer::Changes(page)) = rec.ask("changes-group", &Route::Changes { after: 0, limit: 200, only: Only::Group(&one) }, asked) else {
        panic!("one group's envelopes are read")
    };
    // three Chats, two cards and a register of the agent, three Chats of the human
    assert_eq!(page.items.len(), 9);
    assert!(page.items.iter().all(|i| matches!(i, ChangeItem::Envelope(_))));

    // ---- the open list, whole and since a change
    let Ok(Answer::DeskBrief(brief)) =
        rec.ask("desk-brief", &Route::DeskBrief { since: None, active: true }, json!({ "route": "desk_brief", "since": null, "active": true }))
    else {
        panic!("the open list is read")
    };
    assert_eq!(brief.cards.len(), 2);
    // (knocking first)
    assert_eq!(brief.cards[0].object.as_bytes(), &cards[1]);
    assert!(brief.cards.iter().all(|c| c.first_change.is_some_and(|f| f <= c.version_change)));
    assert!(brief.settled.is_none() && !brief.truncated);
    let since = brief.cards[1].head_change;
    let Ok(Answer::DeskBrief(newer)) = rec.ask(
        "desk-brief-since",
        &Route::DeskBrief { since: Some(since), active: false },
        json!({ "route": "desk_brief", "since": since, "active": false }),
    ) else {
        panic!("the open list since a change is read")
    };
    assert_eq!((newer.cards.len(), newer.settled), (1, Some(vec![])));

    // ---- the live stream from the start: who is here, then every change of the catch-up, in order
    let (request, _) = Codec.request(&rec.site(), &Route::Stream { after: 0 }).unwrap();
    let (status, head, bytes) = rec.stream(&request);
    let streamed = HttpResponse { status, headers: head.clone(), body: bytes.clone().into() };
    rec.record("stream", &request, json!({ "route": "stream", "after": 0 }), &streamed);
    let mut reader = StreamReader::new(0);
    reader.opened(status, &head).expect("a stream of events");
    let fed = reader.chunk(&bytes);
    assert_eq!(fed.failure, None);
    assert!(fed.events.iter().any(|e| matches!(e, StreamEvent::Here)));
    assert!(fed.events.iter().any(|e| matches!(e, StreamEvent::Presence(p) if p.online)));
    let streamed: Vec<u64> = fed
        .events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::Change(item) => Some(item.change()),
            _ => None,
        })
        .collect();
    assert_eq!(streamed, total, "the stream replays what the catch-up of the live sessions reads");

    // ---- an envelope the hub refuses and voids (a human's Chat to nobody): its chain number is used up
    let (to_nobody, _) = w.ada.envelope(&group, &chat(&session, ZERO32, "to nobody"));
    let void = entry(OutboxKind::Envelope, Some(&group), w.ada.epoch(&group), vec![to_nobody]);
    {
        let (request, expect) = Codec.request(&rec.site(), &Route::Outbox { room: &room, entry: &void, extra: None }).unwrap();
        let response = rec.send(&request);
        rec.record("outbox-envelope-voided", &request, asked_entry(&void), &response);
        assert_eq!(rec.read(&expect, &response), Err(Failure::Refused { status: 403, code: Some("forbidden".into()) }));
        assert!(Codec.voided(&response));
        assert!(Codec.refusal(&response).is_some_and(|r| r.voided));
    }

    // ---- a card is closed: the open list since then names it as settled
    let closed = object(wire::KIND_VERSION, cards[0], wire::TYPE_CARD, wire::STATE_CLOSED, 0, versions[0], ZERO32);
    agent.send(&w.hub, &group, &closed).ok();
    let Ok(Answer::DeskBrief(later)) = rec.ask(
        "desk-brief-settled",
        &Route::DeskBrief { since: Some(since), active: false },
        json!({ "route": "desk_brief", "since": since, "active": false }),
    ) else {
        panic!("the open list with a settled card is read")
    };
    assert_eq!(later.cards.len(), 1);
    assert!(matches!(later.settled.as_deref(), Some([one]) if one.as_bytes() == &cards[0]), "{:?}", later.settled);

    // ---- without a token: the hub's refusal on a route and on the stream
    rec.token = Some("a-token-the-hub-never-gave".to_owned());
    let refused = rec.ask("changes-unauthorised", &Route::Changes { after: 0, limit: 6, only: Only::All }, json!({ "route": "changes", "after": 0, "limit": 6, "only": "all" }));
    assert_eq!(refused, Err(Failure::Refused { status: 401, code: Some("unauthorised".into()) }));
    let (request, _) = Codec.request(&rec.site(), &Route::Stream { after: 0 }).unwrap();
    let (status, head, bytes) = rec.stream(&request);
    let mut reader = StreamReader::new(0);
    reader.opened(status, &head).unwrap();
    assert_eq!(reader.chunk(&bytes).failure, None);
    assert_eq!(reader.ended(), Some(Failure::Refused { status: 401, code: Some("unauthorised".into()) }));

    // ---- a room founded through the outbox (no token)
    rec.token = None;
    let new_room: [u8; 32] = random();
    let recovery = Recovery::new();
    let mut founder = Dev::new();
    let info = founder.create_room(&new_room, &recovery);
    let sealed = founder.sealed_key(&new_room, 0, &info, 0, &recovery.hpke_public, true);
    let e = entry(OutboxKind::RoomFounding, Some(&new_room), 0, vec![info, sealed]);
    let Ok(Answer::Outbox(founded)) = outbox(&rec, "outbox-room-founding", &e, None) else { panic!("a room is founded") };
    assert_eq!(founded.room, Some(RoomId::new(new_room)));
}

/// Signs in through the codec with `key` (a device's, or the room's recovery signature key); returns the token.
fn sign_in(rec: &Recorder, name: &str, room: &RoomId, key: &openmls_basic_credential::SignatureKeyPair) -> (String, trommi_client_core::hub::Role) {
    let asked = json!({ "route": "challenge", "room": b64(room.as_bytes()) });
    let named = |what: &str| if name.is_empty() { String::new() } else { format!("{what}-{name}") };
    let Ok(Answer::Challenge(challenge)) = rec.ask(&named("challenge"), &Route::Challenge { room }, asked) else {
        panic!("a challenge is handed out")
    };
    let auth = wire::HubAuth {
        room_id: *room.as_bytes(),
        hub: rec.url.as_bytes().to_vec(),
        device: key.to_public_vec().try_into().unwrap(),
        challenge,
    }
    .bytes();
    let signature = sign_with_label(key, "TrommiHubAuth", &auth);
    let asked = json!({ "route": "token", "room": b64(room.as_bytes()) });
    let route = Route::Token { room, auth: &auth, signature: &signature };
    match rec.ask(&named("token"), &route, asked) {
        Ok(Answer::Token { token, expires_at, role }) => {
            assert!(expires_at > 1_700_000_000_000);
            (token, role)
        }
        other => panic!("no token: {other:?}"),
    }
}

#[test]
fn signing_in_and_joining_through_the_codec() {
    use trommi_client_core::hub::{GroupRowKind, ObjectRoute, Role, WishKind};
    use trommi_core::ids::{DeviceId, ObjectId};

    let mut w = World::on(TestHub::start_with(&[("HUB_LIMIT_ENVELOPES_PER_SECOND", "100000")]));
    let mut rec = Recorder {
        port: w.hub.port,
        url: w.hub.url.clone(),
        token: None,
        write: std::env::var("TROMMI_HUB_VECTORS").as_deref() == Ok("write"),
    };
    let room = RoomId::new(w.room);
    let room_group = w.room.to_vec();

    // ---- a room with two main sessions, some content, and an agent that has not read its Welcome
    let mut agent = w.enrol_agent();
    let (session, group) = w.found_main(&mut [], Some(&mut agent));
    let (helper, founded, _) = w.found_helper(&mut agent, &session, &[w.ada.id()], &[]);
    founded.ok();
    // (32 more, for a post of 32 joins at once)
    let many_helpers: Vec<Vec<u8>> = (0..32)
        .map(|_| {
            let (group, founded, _) = w.found_helper(&mut agent, &session, &[w.ada.id()], &[]);
            founded.ok();
            group
        })
        .collect();
    let mut other = w.enrol_agent();
    let (_, group2) = w.found_main(&mut [], Some(&mut other));
    for text in ["one", "two", "three", "four"] {
        agent.send(&w.hub, &group, &chat(&session, w.ada.id(), text)).ok();
    }
    let card_id = enc::object_id(&group, &agent.id(), agent.chain(&group).0 + 1);
    let card = object(wire::KIND_VERSION, card_id, wire::TYPE_CARD, wire::STATE_OPEN, 0, ZERO32, ZERO32);
    agent.send(&w.hub, &group, &card).ok();
    w.ada.post_message(&w.hub, &group, b"a stored message", false).ok();

    // ---- the recovery key signs in, reads the room group and posts the join of a new device
    let (token, role) = sign_in(&rec, "recovery", &room, &w.recovery.sign);
    assert_eq!(role, Role::Recovery);
    rec.token = Some(token);
    let room_id = group_id(&room_group);
    let asked = json!({ "route": "group_info", "group": b64(&room_group), "epoch": null });
    let Ok(Answer::GroupInfo { epoch, group_info: base }) = rec.ask("group-info", &Route::GroupInfo { group: &room_id, epoch: None }, asked) else {
        panic!("the room's GroupInfo is read")
    };
    let asked = json!({ "route": "group_info", "group": b64(&room_group), "epoch": 0 });
    assert!(matches!(
        rec.ask("group-info-founding", &Route::GroupInfo { group: &room_id, epoch: Some(0) }, asked),
        Ok(Answer::GroupInfo { epoch: 0, .. })
    ));
    let asked = json!({ "route": "group_log", "group": b64(&room_group), "after": 0, "limit": 1, "commits_only": true });
    let route = Route::GroupLog { group: &room_id, after: 0, limit: 1, commits_only: true };
    let Ok(Answer::GroupLog { items, more }) = rec.ask("group-log-commits", &route, asked) else { panic!("the room's Commits are read") };
    assert!(items.len() == 1 && more && items.iter().all(|i| matches!(i, ChangeItem::Commit(_))));

    let now = w.ada.room_now();
    assert_eq!(epoch, now.0);
    let mut dana = Dev::new();
    dana.room = w.room;
    let out = dana.external_join(&base, now);
    let auth = recovery_auth(&w.recovery.sign, &out, &base, now, &dana.id());
    let key = dana.sealed_key(&room_group, out.epoch + 1, &out.group_info, out.epoch, &w.recovery.hpke_public, true);
    let e = entry(OutboxKind::ExternalCommit, Some(&room_group), out.epoch, vec![out.commit.clone(), out.group_info.clone(), key, auth]);
    let joined = rec.ask("outbox-external-commit", &Route::Outbox { room: &room, entry: &e, extra: None }, asked_entry(&e));
    assert!(matches!(&joined, Ok(Answer::Outbox(a)) if a.epoch == Some(out.epoch + 1)), "{joined:?}");

    // ---- the new device signs in as itself, lists the room's groups and joins the sessions in one request
    let (token, role) = sign_in(&rec, "human", &room, &dana.signer);
    assert_eq!(role, Role::Human);
    rec.token = Some(token.clone());
    dana.token = Some(token);
    let (mut after, mut pages, mut rows) = (0, 0, Vec::new());
    loop {
        let asked = json!({ "route": "room_groups", "room": b64(&w.room), "after": after, "limit": 20 });
        let name = format!("room-groups-{pages}");
        let Ok(Answer::RoomGroups { items, more, after: next }) = rec.ask(&name, &Route::RoomGroups { room: &room, after, limit: 20 }, asked) else {
            panic!("{name} is read")
        };
        rows.extend(items);
        (after, pages) = (next, pages + 1);
        if !more {
            break;
        }
    }
    assert_eq!((rows.len(), pages), (36, 2));
    assert_eq!(rows[0].kind, GroupRowKind::Room);
    assert_eq!(rows[0].leaves.len(), 2, "the founder and the device that joined");
    let mains: Vec<_> = rows.iter().filter(|r| r.kind == GroupRowKind::Main).collect();
    assert!(mains.len() == 2 && mains.iter().all(|r| r.live && r.parent.is_none()));
    // the helper session names the session it was opened under
    let helpers: Vec<_> = rows.iter().filter(|r| r.kind == GroupRowKind::Helper).collect();
    assert_eq!(helpers.len(), 33);
    assert!(helpers.iter().all(|h| h.parent.is_some_and(|p| p.as_bytes() == &session)), "{helpers:?}");
    assert!(helpers.iter().any(|h| h.group == group_id(&helper)));

    let nowhere: [u8; 48] = [&w.room[..], &random::<16>()[..]].concat().try_into().unwrap();
    let sessions = [group_id(&group), group_id(&group2), group_id(&nowhere)];
    let asked = json!({ "route": "groups_read", "groups": sessions.iter().map(|g| b64(g.as_bytes())).collect::<Vec<_>>() });
    let Ok(Answer::GroupsRead(read)) = rec.ask("groups-read", &Route::GroupsRead { groups: &sessions }, asked) else {
        panic!("the sessions are read in one request")
    };
    assert_eq!(read.len(), 3);
    assert_eq!(read[2], Err(Failure::Refused { status: 404, code: Some("not-found".into()) }));
    // the most one request takes: 64 groups, the two sessions first and 62 that are nowhere
    let mut many = vec![group_id(&group), group_id(&group2)];
    many.extend((0..62).map(|_| group_id(&[&w.room[..], &random::<16>()[..]].concat())));
    let asked = json!({ "route": "groups_read", "groups": many.iter().map(|g| b64(g.as_bytes())).collect::<Vec<_>>() });
    let Ok(Answer::GroupsRead(full)) = rec.ask("groups-read-64", &Route::GroupsRead { groups: &many }, asked) else {
        panic!("64 groups are read in one request")
    };
    assert_eq!(full.len(), 64);
    assert!(full[..2].iter().all(Result::is_ok));
    assert!(full[2..].iter().all(|r| r == &Err(Failure::Refused { status: 404, code: Some("not-found".into()) })));
    let now = dana.room_now();
    let mut joins = Vec::new();
    for (bytes, one) in [&group, &group2].into_iter().zip(&read) {
        let one = one.as_ref().expect("a session the new device may read");
        assert!(one.complete && one.epoch == 1 && one.log.len() == 1 && one.founding.is_some());
        let out = dana.external_join(&one.info, now);
        let auth = recovery_auth(&w.recovery.sign, &out, &one.info, now, &dana.id());
        let key = dana.sealed_key(bytes, out.epoch + 1, &out.group_info, now.0, &w.recovery.hpke_public, true);
        joins.push(entry(OutboxKind::ExternalCommit, Some(bytes), out.epoch, vec![out.commit, out.group_info, key, auth]));
    }
    let asked_joins = |entries: &[OutboxEntry]| {
        json!({ "route": "joins", "entries": entries.iter().map(|e| json!({ "group": b64(e.group.unwrap().as_bytes()), "epoch": e.epoch })).collect::<Vec<_>>() })
    };
    let Ok(Answer::Joins(answers)) = rec.ask("joins", &Route::Joins { entries: &joins }, asked_joins(&joins)) else {
        panic!("the joins are answered in their places")
    };
    assert!(matches!(&answers[..], [Ok(a), Ok(b)] if a.epoch == Some(2) && b.epoch == Some(2)), "{answers:?}");
    // one that the hub refuses, in its place
    let stray = [entry(OutboxKind::ExternalCommit, Some(&nowhere), 0, vec![vec![1; 600], vec![2; 600], vec![3; 100], vec![4; 100]])];
    let Ok(Answer::Joins(answers)) = rec.ask("joins-refused", &Route::Joins { entries: &stray }, asked_joins(&stray)) else {
        panic!("a refusal in its place")
    };
    assert!(matches!(&answers[..], [Err(Failure::Refused { code: Some(_), .. })]), "{answers:?}");

    // ---- 32 joins in one post: the helper sessions, read in one request
    let helpers: Vec<GroupId> = many_helpers.iter().map(|g| group_id(g)).collect();
    let Ok(Answer::GroupsRead(read)) = rec.ask("", &Route::GroupsRead { groups: &helpers }, json!({})) else {
        panic!("32 helper sessions are read in one request")
    };
    let now = dana.room_now();
    let mut joins = Vec::new();
    for (bytes, one) in many_helpers.iter().zip(&read) {
        let one = one.as_ref().expect("a helper session the new device may read");
        let out = dana.external_join(&one.info, now);
        let auth = recovery_auth(&w.recovery.sign, &out, &one.info, now, &dana.id());
        let key = dana.sealed_key(bytes, out.epoch + 1, &out.group_info, now.0, &w.recovery.hpke_public, true);
        joins.push(entry(OutboxKind::ExternalCommit, Some(bytes), out.epoch, vec![out.commit, out.group_info, key, auth]));
    }
    let Ok(Answer::Joins(answers)) = rec.ask("joins-32", &Route::Joins { entries: &joins }, asked_joins(&joins)) else {
        panic!("32 joins are answered in their places")
    };
    assert_eq!(answers.len(), 32);
    assert!(answers.iter().all(|a| matches!(a, Ok(a) if a.epoch == Some(2))), "{answers:?}");

    // ---- the log with a message, a sender's chain, an object's envelopes
    let session_group = group_id(&group);
    let asked = json!({ "route": "group_log", "group": b64(&group), "after": 0, "limit": 200, "commits_only": false });
    let route = Route::GroupLog { group: &session_group, after: 0, limit: 200, commits_only: false };
    let Ok(Answer::GroupLog { items, more }) = rec.ask("group-log", &route, asked) else { panic!("the session's log is read") };
    assert!(!more && items.iter().any(|i| matches!(i, ChangeItem::Message(_))));
    let rejected = items.iter().rev().find_map(|i| match i {
        ChangeItem::Commit(l) => Some(l.n),
        _ => None,
    });
    let sender = DeviceId::new(agent.id());
    let asked = json!({ "route": "chain", "group": b64(&group), "sender": b64(&agent.id()), "after": 1, "limit": 3 });
    let route = Route::Chain { group: &session_group, sender: &sender, after: 1, limit: 3 };
    let Ok(Answer::Chain { items, more }) = rec.ask("chain", &route, asked) else { panic!("a chain is read") };
    assert_eq!((items.iter().map(|i| i.seq).collect::<Vec<_>>(), more), (vec![2, 3, 4], true));
    let object_id = ObjectId::new(card_id);
    let asked = json!({ "route": "object", "kind": "cards", "object": b64(&card_id), "after": 0, "limit": 10 });
    let route = Route::Object { route: ObjectRoute::Cards, object: &object_id, after: 0, limit: 10 };
    let Ok(Answer::Object(card)) = rec.ask("object-card", &route, asked) else { panic!("a card's envelopes are read") };
    assert_eq!((card.items.len(), card.more, card.group), (1, false, session_group));

    // ---- KeyPackages claimed: all or nothing, and with `partial` the devices the hub holds none of
    let devices = [DeviceId::new(agent.id()), DeviceId::new(dana.id())];
    let asked_claim = |devices: &[DeviceId], partial: bool| {
        json!({ "route": "claim", "devices": devices.iter().map(|d| b64(d.as_bytes())).collect::<Vec<_>>(), "partial": partial })
    };
    let one = &devices[..1];
    let Ok(Answer::Claimed { key_packages, missing }) = rec.ask("claim", &Route::ClaimKeyPackages { devices: one, partial: false }, asked_claim(one, false)) else {
        panic!("a KeyPackage is claimed")
    };
    assert!(key_packages.len() == 1 && key_packages[0].0 == devices[0] && missing.is_empty());
    let Ok(Answer::Claimed { key_packages, missing }) = rec.ask("claim-partial", &Route::ClaimKeyPackages { devices: &devices, partial: true }, asked_claim(&devices, true)) else {
        panic!("a partial claim names what is missing")
    };
    assert_eq!((key_packages.len(), missing), (1, vec![devices[1]]));
    let refused = rec.ask("claim-refused", &Route::ClaimKeyPackages { devices: &devices, partial: false }, asked_claim(&devices, false));
    assert!(matches!(refused, Err(Failure::Refused { code: Some(_), .. })), "{refused:?}");

    // ---- a wish, the list of wishes, a Commit rejected
    let asked = json!({ "route": "wish", "kind": "handover" });
    let wished = rec.ask("wish", &Route::Wish { kind: WishKind::Handover, group: Some(&session_group), key_package: None }, asked);
    let Ok(Answer::Wished { id }) = wished else { panic!("a wish is taken: {wished:?}") };
    let asked = json!({ "route": "reject", "group": b64(&group), "n": rejected });
    let done = rec.ask("reject", &Route::RejectCommit { group: &session_group, n: rejected.unwrap() }, asked);
    assert_eq!(done.ok(), Some(Answer::Done));
    let Ok(Answer::Wishes(list)) = rec.ask("wishes", &Route::Wishes, json!({ "route": "wishes" })) else { panic!("the wishes are listed") };
    assert!(list.iter().any(|w| w.id == id) && list.iter().any(|w| w.n == rejected && w.committer.is_some()), "{list:?}");

    // ---- the Welcome that waits for the agent of the second session
    rec.token = other.token.clone();
    let Ok(Answer::Welcomes(welcomes)) = rec.ask("welcomes", &Route::Welcomes { after: 0 }, json!({ "route": "welcomes", "after": 0 })) else {
        panic!("the Welcomes are read")
    };
    assert_eq!(welcomes.len(), 1);
    assert_eq!(welcomes[0].group, group_id(&group2));
    let after = welcomes[0].id;
    let none = rec.ask("welcomes-none", &Route::Welcomes { after }, json!({ "route": "welcomes", "after": after }));
    assert_eq!(none.ok(), Some(Answer::Welcomes(vec![])));
}


#[test]
fn the_desk_files_and_the_rest_through_the_codec() {
    use sha2::{Digest, Sha256};
    use trommi_client_core::hub::{ByteRange, DeskOnly, ObjectRoute, Role, Timeline};
    use trommi_core::ids::{BoardId, DeviceId, FileId, Hash32, ObjectId, SessionId, ShareId};

    let mut w = World::on(TestHub::start_with(&[("HUB_LIMIT_ENVELOPES_PER_SECOND", "100000")]));
    let mut rec = Recorder {
        port: w.hub.port,
        url: w.hub.url.clone(),
        token: w.ada.token.clone(),
        write: std::env::var("TROMMI_HUB_VECTORS").as_deref() == Ok("write"),
    };
    let room = RoomId::new(w.room);
    let mut agent = w.enrol_agent();
    let (session, group) = w.found_main(&mut [], Some(&mut agent));
    for text in ["one", "two", "three", "four", "five"] {
        agent.send(&w.hub, &group, &chat(&session, w.ada.id(), text)).ok();
    }
    let (mut cards, mut versions) = (vec![], vec![]);
    for urgency in [0, 3] {
        let id = enc::object_id(&group, &agent.id(), agent.chain(&group).0 + 1);
        agent.send(&w.hub, &group, &object(wire::KIND_VERSION, id, wire::TYPE_CARD, wire::STATE_OPEN, urgency, ZERO32, ZERO32)).ok();
        cards.push(id);
        versions.push(agent.chain(&group).1);
    }
    let register_id: [u8; 16] = random();
    agent.send(&w.hub, &group, &register(&register_id, "an older value")).ok();
    agent.send(&w.hub, &group, &register(&register_id, "a value")).ok();
    agent.send(&w.hub, &group, &register(&random(), "another")).ok();

    // ---- a file goes up, is named by an open Artifact, comes down whole and in ranges
    let file_id: [u8; 16] = random();
    let file = FileId::new(file_id);
    let stored: Vec<u8> = (0..1000u32).map(|i| b'a' + (i % 26) as u8).collect();
    let asked = json!({ "route": "put_file", "file": b64(&file_id), "size": stored.len() });
    let put = rec.ask("file-put", &Route::PutFile { file: &file, bytes: &stored }, asked);
    let Ok(Answer::FileStored { size, sha256 }) = put else { panic!("a file is stored: {put:?}") };
    assert_eq!((size, &sha256[..]), (1000, &Sha256::digest(&stored)[..]));
    // (a file is first named by the device that uploaded it: here by a Chat message of the human)
    let mut note = chat(&session, agent.id(), "with a file");
    note.file_ids = vec![file_id];
    w.ada.send(&w.hub, &group, &note).ok();
    // (a Share link is for a file of an open Artifact, which the session's agent writes: its own upload)
    let shared_id: [u8; 16] = random();
    agent.raw(&w.hub, "PUT", &format!("/v1/files/{}", b64(&shared_id)), &[], &stored).ok();
    let artifact_id = enc::object_id(&group, &agent.id(), agent.chain(&group).0 + 1);
    let mut artifact = object(wire::KIND_VERSION, artifact_id, wire::TYPE_ARTIFACT, wire::STATE_OPEN, 1, ZERO32, ZERO32);
    artifact.file_ids = vec![shared_id];
    agent.send(&w.hub, &group, &artifact).ok();
    for (name, range) in [
        ("file-get", None),
        ("file-get-range", Some(ByteRange { first: 10, last: Some(19) })),
        ("file-get-tail", Some(ByteRange { first: 990, last: None })),
        ("file-get-beyond", Some(ByteRange { first: 900, last: Some(5000) })),
    ] {
        let asked = json!({ "route": "get_file", "file": b64(&file_id), "first": range.map(|r| r.first), "last": range.and_then(|r| r.last) });
        let got = rec.ask(name, &Route::GetFile { file: &file, range }, asked);
        let Ok(Answer::File(got)) = got else { panic!("{name} comes down: {got:?}") };
        let first = range.map_or(0, |r| r.first) as usize;
        let last = range.and_then(|r| r.last).map_or(999, |l| l.min(999)) as usize;
        assert_eq!((got.size, got.first as usize, &got.bytes[..]), (1000, first, &stored[first..=last]), "{name}");
    }
    let asked = json!({ "route": "get_file", "file": b64(&file_id), "first": 1000, "last": null });
    let beyond = rec.ask("file-get-refused", &Route::GetFile { file: &file, range: Some(ByteRange { first: 1000, last: None }) }, asked);
    assert_eq!(beyond, Err(Failure::Refused { status: 416, code: Some("range".into()) }));

    // ---- a Share link for it, read without a token by whoever holds the secret, then revoked
    let (share_id, secret): ([u8; 16], [u8; 32]) = (random(), random());
    let share = ShareId::new(share_id);
    let hash: [u8; 32] = Sha256::digest(secret).into();
    let expires_at = trommi_hub::util::now() + 86_400_000;
    let shared_file = FileId::new(shared_id);
    let asked = json!({ "route": "post_share", "share": b64(&share_id), "file": b64(&shared_id), "expires_at": expires_at });
    let shared = rec.ask("share-post", &Route::PostShare { share: &share, secret_hash: &hash, file: &shared_file, expires_at }, asked);
    assert_eq!(shared, Ok(Answer::Shared { expires_at }), "a link is registered");
    let token = rec.token.take();
    // (never recorded: the request's header is the link's secret, and the bytes are the file's)
    let got = rec.ask("", &Route::GetShared { share: &share, secret: &secret, range: None }, json!({}));
    assert!(matches!(&got, Ok(Answer::File(f)) if f.bytes == stored), "{got:?}");
    let wrong = rec.ask("", &Route::GetShared { share: &share, secret: &[7; 32], range: None }, json!({}));
    assert_eq!(wrong, Err(Failure::Refused { status: 404, code: Some("not-found".into()) }));
    rec.token = token;
    let asked = json!({ "route": "delete_share", "share": b64(&share_id) });
    assert_eq!(rec.ask("share-delete", &Route::DeleteShare { share: &share }, asked), Ok(Answer::Done));

    // ---- the Desk, whole: the open objects with their versions, each writer's newest register value
    let Ok(Answer::Desk(desk)) = rec.ask("desk", &Route::Desk { only: DeskOnly::Active }, json!({ "route": "desk", "only": "active" })) else {
        panic!("the Desk is read")
    };
    assert_eq!((desk.cards.len(), desk.artifacts.len(), desk.notes.len(), desk.permission_requests.len()), (2, 1, 0, 0));
    assert_eq!(desk.cards[0].object.as_bytes(), &cards[1], "knocking first");
    assert!(desk.cards.iter().all(|c| c.version.is_some() && c.group == group_id(&group) && c.owner.as_bytes() == &agent.id()));
    assert_eq!(desk.registers.len(), 2, "a writer's newest value per register");
    assert!(!desk.truncated && desk.groups.is_some());
    let Ok(Answer::Desk(archived)) = rec.ask("desk-archived", &Route::Desk { only: DeskOnly::Archived }, json!({ "route": "desk", "only": "archived" })) else {
        panic!("the Desk of the archived sessions is read")
    };
    assert!(archived.cards.is_empty() && archived.registers.is_empty());
    let counts = desk.counts.as_ref().expect("the Desk counts");
    assert_eq!(counts.len(), 1);
    assert_eq!((counts[0].group, counts[0].cards.open, counts[0].artifacts.open), (group_id(&group), 2, 1));

    // ---- the settled objects, newest first, a page at a time: a card he answered, one its agent closed
    let answer = object(wire::KIND_ANSWER, cards[0], wire::TYPE_CARD, wire::STATE_ANSWERED, 0, versions[0], agent.id());
    w.ada.send(&w.hub, &group, &answer).ok();
    let close = object(wire::KIND_VERSION, cards[1], wire::TYPE_CARD, wire::STATE_CLOSED, 3, versions[1], ZERO32);
    agent.send(&w.hub, &group, &close).ok();
    let (mut before, mut settled) = (None, vec![]);
    for page in 0..3 {
        let name = format!("objects-settled-{page}");
        let asked = json!({ "route": "settled", "kind": "cards", "only": "all", "before": before, "limit": 1 });
        let route = Route::Settled { kind: Some(ObjectRoute::Cards), only: Only::All, before, limit: 1 };
        let Ok(Answer::Settled(got)) = rec.ask(&name, &route, asked) else { panic!("{name} is read") };
        settled.extend(got.items.iter().map(|i| (i.row.object, i.row.state)));
        if !got.more {
            break;
        }
        before = Some(got.before);
    }
    assert_eq!(settled.iter().map(|(o, s)| (*o.as_bytes(), *s)).collect::<Vec<_>>(), vec![(cards[1], 3), (cards[0], 2)], "newest first");
    let one = group_id(&group);
    let asked = json!({ "route": "settled", "kind": null, "only": { "group": b64(&group) }, "before": null, "limit": 10 });
    let route = Route::Settled { kind: None, only: Only::Group(&one), before: None, limit: 10 };
    let Ok(Answer::Settled(got)) = rec.ask("objects-settled-group", &route, asked) else { panic!("one group's settled objects are read") };
    assert!(got.items.len() == 2 && !got.more && got.items.iter().all(|i| i.kind == ObjectRoute::Cards && i.row.version.is_some()));

    // ---- a Chat, newest first, page by page; a board's items; a frontier that covers nothing yet
    let timeline = SessionId::new(session);
    let (mut before, mut seen) = (None, Vec::new());
    for page in 0..4 {
        let asked = json!({ "route": "chat", "session": b64(&session), "before": before, "limit": 2 });
        let name = format!("chat-{page}");
        let Ok(Answer::Envelopes { items, more }) = rec.ask(&name, &Route::Chat { timeline: Timeline::Session(&timeline), before, limit: 2 }, asked) else {
            panic!("{name} is read")
        };
        seen.extend(items.iter().map(|i| i.change));
        before = items.last().map(|i| i.change);
        if !more {
            break;
        }
    }
    assert_eq!(seen.len(), 6);
    assert!(seen.windows(2).all(|w| w[0] > w[1]), "newest first: {seen:?}");
    // ---- the Chat of a card
    let mut about = chat(&session, w.ada.id(), "about the card");
    about.subject = wire::Subject::Item { timeline_kind: wire::TIMELINE_CHAT, timeline_scope: wire::SCOPE_CARD, timeline_ref: cards[0] };
    for _ in 0..3 {
        agent.send(&w.hub, &group, &about).ok();
    }
    let card = ObjectId::new(cards[0]);
    let asked = json!({ "route": "chat", "card": b64(&cards[0]), "before": null, "limit": 2 });
    let Ok(Answer::Envelopes { items, more }) = rec.ask("chat-card", &Route::Chat { timeline: Timeline::Card(&card), before: None, limit: 2 }, asked) else {
        panic!("a card's Chat is read")
    };
    assert!(items.len() == 2 && more && items[0].change > items[1].change);
    let board_id: [u8; 16] = random();
    let board = BoardId::new(board_id);
    let room_group = w.room.to_vec();
    for _ in 0..3 {
        w.ada.send(&w.hub, &room_group, &board_item(&board_id)).ok();
    }
    let asked = json!({ "route": "board", "board": b64(&board_id), "after": 0, "limit": 2 });
    let Ok(Answer::Envelopes { items, more }) = rec.ask("board", &Route::Board { board: &board, after: 0, limit: 2 }, asked) else {
        panic!("a board's items are read")
    };
    assert!(items.len() == 2 && more && items[0].change < items[1].change);
    let asked = json!({ "route": "board_frontier", "board": b64(&board_id) });
    let pruned = rec.ask("board-frontier", &Route::BoardFrontier { board: &board, frontier: &[], files: &[], snapshot: None }, asked);
    assert_eq!(pruned, Ok(Answer::Pruned { pruned: 0 }));
    // a frontier that names the writer's head, declared; then bound to the register value written after it
    let writer = DeviceId::new(w.ada.id());
    let head = w.ada.chain(&room_group);
    let frontier = [(writer, head.0, Hash32::new(head.1))];
    let writer_id = w.ada.id();
    let asked_frontier = |snapshot: Option<(u64, [u8; 32])>| {
        json!({
            "route": "board_frontier",
            "board": b64(&board_id),
            "frontier": [[b64(&writer_id), head.0, b64(&head.1)]],
            "snapshot": snapshot.map(|(seq, hash)| json!([seq, b64(&hash)])),
        })
    };
    let declared = rec.ask("board-frontier-declared", &Route::BoardFrontier { board: &board, frontier: &frontier, files: &[], snapshot: None }, asked_frontier(None));
    assert!(matches!(declared, Ok(Answer::Pruned { .. })), "{declared:?}");
    w.ada.send(&w.hub, &room_group, &register(&random(), "snapshot")).ok();
    let value = w.ada.chain(&room_group);
    let route = Route::BoardFrontier { board: &board, frontier: &frontier, files: &[], snapshot: Some((value.0, Hash32::new(value.1))) };
    let bound = rec.ask("board-frontier-snapshot", &route, asked_frontier(Some(value)));
    assert!(matches!(bound, Ok(Answer::Pruned { .. })), "{bound:?}");

    // ---- the sealed keys
    let asked = json!({ "route": "sealed_keys", "after": 0, "limit": 2 });
    let Ok(Answer::SealedKeys(keys)) = rec.ask("sealed-keys", &Route::SealedKeys { after: 0, limit: 2 }, asked) else {
        panic!("the sealed keys are read")
    };
    assert!(keys.rows.len() == 2 && keys.more && keys.change == keys.rows[1].0);

    // ---- a Desk that does not fit: a thousand and one open cards (run, not recorded)
    for _ in 0..1001 {
        let id = enc::object_id(&group, &agent.id(), agent.chain(&group).0 + 1);
        agent.send(&w.hub, &group, &object(wire::KIND_VERSION, id, wire::TYPE_CARD, wire::STATE_OPEN, 0, ZERO32, ZERO32)).ok();
    }
    let Ok(Answer::Desk(full)) = rec.ask("", &Route::Desk { only: DeskOnly::All }, json!({})) else { panic!("a full Desk is read") };
    assert!(full.truncated && full.cards.len() == 1000);

    // ---- the file is deleted; the token is ended; a recovery is opened and dropped
    let asked = json!({ "route": "delete_file", "file": b64(&file_id) });
    assert_eq!(rec.ask("file-delete", &Route::DeleteFile { file: &file }, asked), Ok(Answer::Done));
    assert_eq!(rec.ask("sign-out", &Route::SignOut, json!({ "route": "sign_out" })), Ok(Answer::Done));
    let after = rec.ask("", &Route::SignOut, json!({}));
    assert_eq!(after, Err(Failure::Refused { status: 401, code: Some("unauthorised".into()) }));
    let (token, role) = sign_in(&rec, "", &room, &w.recovery.sign);
    assert_eq!(role, Role::Recovery);
    rec.token = Some(token);
    let asked = json!({ "route": "open_recovery", "room": b64(&w.room) });
    let Ok(Answer::Recovery { id, expires_at }) = rec.ask("recovery-open", &Route::OpenRecovery { room: &room }, asked) else {
        panic!("a recovery is opened")
    };
    assert!(expires_at > trommi_hub::util::now());
    let asked = json!({ "route": "drop_recovery", "room": b64(&w.room), "recovery": b64(&id) });
    assert_eq!(rec.ask("recovery-drop", &Route::DropRecovery { room: &room, recovery: &id }, asked), Ok(Answer::Dropped { dropped: true }));
}

#[test]
fn the_account_invites_and_push_through_the_codec() {
    use sha2::{Digest, Sha256};
    use trommi_client_core::hub::{
        AccountProof, Kdf, KdfValue, KitForm, KitPart, LiveActivity, NewAccount, NewPassword, PushLevel, PushRegistration, PushTarget,
    };
    use trommi_core::ids::InviteId;

    let w = World::new();
    let mut rec = Recorder {
        port: w.hub.port,
        url: w.hub.url.clone(),
        token: w.ada.token.clone(),
        write: std::env::var("TROMMI_HUB_VECTORS").as_deref() == Ok("write"),
    };
    let token = rec.token.clone();
    let copy = |mark: u8| {
        let mut c = [mark; 61];
        c[0] = 2;
        c
    };
    let kdf = || {
        Kdf(vec![
            ("alg".into(), KdfValue::Text("argon2id".into())),
            ("m".into(), KdfValue::Number(65536)),
            ("p".into(), KdfValue::Number(1)),
            ("t".into(), KdfValue::Number(3)),
            ("v".into(), KdfValue::Number(1)),
        ])
    };
    let route = |name: &str| json!({ "route": name });

    // ---- sign-up with an e-mail and a password, the account's view and usage
    let email = format!("ada-{}@example.com", trommi_hub::util::hex(&random::<6>()));
    let (auth, kit_auth): ([u8; 32], [u8; 32]) = (random(), random());
    let account = NewAccount {
        email: Some(email.clone()),
        kit: KitPart { auth_key: kit_auth, sealed_copy: copy(9) },
        password: Some(NewPassword { auth_key: auth, sealed_copy: copy(7), kdf: kdf() }),
        passkey: None,
    };
    let created = rec.ask("account-create", &Route::CreateAccount { account: &account }, route("create_account"));
    let Ok(Answer::Account(view)) = created else { panic!("an account is made: {created:?}") };
    assert_eq!((view.email.as_deref(), view.kit_form, view.has_password, view.revision), (Some(email.as_str()), KitForm::Email, true, 1));
    assert_eq!((view.kit_copy, view.password_copy, view.kdf.clone(), view.rooms.len()), (copy(9), Some(copy(7)), Some(kdf()), 1));
    let Ok(Answer::Account(again)) = rec.ask("account", &Route::Account, route("account")) else { panic!("the account is read") };
    assert_eq!(again, view);
    let Ok(Answer::Usage(usage)) = rec.ask("account-usage", &Route::AccountUsage, route("account_usage")) else {
        panic!("the usage is read")
    };
    assert_eq!((usage.rooms, usage.devices), (1, 1));

    // ---- logging in (no token): by e-mail and by id, with a wrong key, with the kit
    rec.token = None;
    let asked = |name: &str| json!({ "route": name });
    let Ok(Answer::Login(login)) = rec.ask("account-login", &Route::Login { account: &email, auth_key: &auth }, asked("login")) else {
        panic!("a login is answered")
    };
    assert_eq!((login.rooms.len(), login.rooms[0].room.as_bytes(), login.rooms[0].sealed_copy), (1, &w.room, copy(7)));
    assert_eq!((login.account.as_str(), login.email.as_deref(), login.kdf), (view.account.as_str(), Some(email.as_str()), Some(kdf())));
    let by_id = rec.ask("", &Route::Login { account: &view.account, auth_key: &auth }, json!({}));
    assert!(matches!(by_id, Ok(Answer::Login(_))));
    let wrong = rec.ask("account-login-wrong", &Route::Login { account: &email, auth_key: &[1; 32] }, asked("login"));
    assert_eq!(wrong, Err(Failure::Refused { status: 401, code: Some("wrong-login".into()) }));
    let Ok(Answer::Login(recovered)) = rec.ask("account-recover", &Route::Recover { account: &email, auth_key: &kit_auth }, asked("recover")) else {
        panic!("a recovery login is answered")
    };
    assert_eq!(recovered.rooms[0].sealed_copy, copy(9));
    let Ok(Answer::PasskeyChallenge(open)) = rec.ask("passkey-challenge", &Route::PasskeyChallenge, asked("passkey_challenge")) else {
        panic!("a passkey challenge is handed out")
    };
    assert!(open.kit_form.is_none() && open.account != view.account);

    // ---- a new kit and a new password, each with the way in of now proved
    rec.token = token.clone();
    let Ok(Answer::PasskeyChallenge(own)) =
        rec.ask("account-passkey-challenge", &Route::AccountPasskeyChallenge, asked("account_passkey_challenge"))
    else {
        panic!("the account's passkey challenge is handed out")
    };
    assert_eq!((own.account.as_str(), own.user_handle, own.kit_form), (view.account.as_str(), view.user_handle, Some(KitForm::Email)));
    let new_kit: [u8; 32] = random();
    let proof = AccountProof::Password { auth_key: auth };
    let kit = KitPart { auth_key: new_kit, sealed_copy: copy(5) };
    let revised = rec.ask("account-put-kit", &Route::PutKit { kit: &kit, revision: 1, proof: &proof }, asked("put_kit"));
    assert_eq!(revised, Ok(Answer::Revision { revision: 2 }));
    let stale = rec.ask("account-put-kit-stale", &Route::PutKit { kit: &kit, revision: 1, proof: &proof }, asked("put_kit"));
    assert_eq!(stale, Err(Failure::Refused { status: 409, code: Some("account-changed".into()) }));
    let new_auth: [u8; 32] = random();
    let password = NewPassword { auth_key: new_auth, sealed_copy: copy(6), kdf: kdf() };
    let by_kit = AccountProof::Kit { auth_key: new_kit };
    let revised = rec.ask("account-put-password", &Route::PutPassword { password: &password, revision: 2, proof: &by_kit }, asked("put_password"));
    assert_eq!(revised, Ok(Answer::Revision { revision: 3 }));

    // ---- an invite: the Offer, a Request without a token, the Reveal; and one that is burned
    let newcomer = Dev::new();
    let post_offer = |rec: &Recorder, name: &str| -> (InviteId, Vec<u8>, [u8; 32]) {
        let (nonce, invite_id): ([u8; 32], [u8; 16]) = (random(), random());
        let (room_epoch, room_state) = w.ada.room_now();
        let offer = wire::Offer {
            room_id: w.room,
            invite_id,
            role: 1,
            session_id: ZERO16,
            expires_at: trommi_hub::util::now() + 600_000,
            commitment: enc::ref_hash("Trommi Invite Commitment", &[&invite_id[..], &nonce[..]].concat()),
            inviter: w.ada.id(),
            room_epoch,
            room_state,
        }
        .bytes();
        let signature = w.ada.sign("TrommiInviteOffer", &offer);
        let posted = rec.ask(name, &Route::PostInvite { offer: &offer, signature: &signature, mac: &[9; 32] }, json!({ "route": "post_invite" }));
        assert_eq!(posted, Ok(Answer::Invited { invite: InviteId::new(invite_id) }), "an Offer is published");
        (InviteId::new(invite_id), offer, nonce)
    };
    let (invite, offer, nonce) = post_offer(&rec, "invite-post");
    let asked_invite = |name: &str, invite: &InviteId| json!({ "route": name, "invite": b64(invite.as_bytes()) });
    rec.token = None;
    let Ok(Answer::Invite(seen)) = rec.ask("invite-get", &Route::GetInvite { invite: &invite }, asked_invite("get_invite", &invite)) else {
        panic!("an invite is read")
    };
    assert_eq!((&seen.offer, seen.mac.as_deref(), seen.requests.is_none()), (&offer, Some(&[9u8; 32][..]), true));
    let request = wire::InviteRequest {
        room_id: w.room,
        invite_id: *invite.as_bytes(),
        hub: w.hub.url.as_bytes().to_vec(),
        role: 1,
        key_package: newcomer.key_package(false),
        offer_hash: enc::ref_hash("Trommi Invite Offer", &offer),
    }
    .bytes();
    let mac = Sha256::digest([&random::<32>()[..], &request[..]].concat()).to_vec();
    let signed = [&request[..], &mac[..]].concat();
    let signature = newcomer.sign("TrommiInviteRequest", &signed);
    let route_request = Route::PostInviteRequest { invite: &invite, request: &request, mac: &mac, signature: &signature };
    let Ok(Answer::Requested { request_hash }) = rec.ask("invite-request", &route_request, asked_invite("post_invite_request", &invite)) else {
        panic!("a Request is taken")
    };
    assert_eq!(request_hash, enc::ref_hash("Trommi Invite Request", &signed));
    let early = rec.ask("invite-reveal-early", &Route::GetReveal { invite: &invite }, asked_invite("get_reveal", &invite));
    assert_eq!(early, Err(Failure::Refused { status: 404, code: Some("not-found".into()) }));
    rec.token = token.clone();
    let Ok(Answer::Invite(seen)) = rec.ask("invite-get-requests", &Route::GetInvite { invite: &invite }, asked_invite("get_invite", &invite)) else {
        panic!("the inviter reads the Requests")
    };
    assert!(matches!(seen.requests.as_deref(), Some([one]) if one.request == request && one.mac == mac));
    let reveal = wire::Reveal { invite_id: *invite.as_bytes(), nonce, request_hash }.bytes();
    let signature = w.ada.sign("TrommiInviteReveal", &reveal);
    let put = rec.ask("invite-put-reveal", &Route::PutReveal { invite: &invite, reveal: &reveal, signature: &signature }, asked_invite("put_reveal", &invite));
    assert_eq!(put, Ok(Answer::Done));
    rec.token = None;
    let got = rec.ask("invite-reveal", &Route::GetReveal { invite: &invite }, asked_invite("get_reveal", &invite));
    assert_eq!(got, Ok(Answer::Reveal { reveal, signature }));
    rec.token = token.clone();
    let (burned, _, _) = post_offer(&rec, "");
    assert_eq!(rec.ask("invite-delete", &Route::DeleteInvite { invite: &burned }, asked_invite("delete_invite", &burned)), Ok(Answer::Done));

    // ---- push: a registration, the list, its removal
    let key = p256::SecretKey::random(&mut p256::elliptic_curve::rand_core::OsRng);
    let point = p256::elliptic_curve::sec1::ToEncodedPoint::to_encoded_point(&key.public_key(), false);
    let endpoint = "http://127.0.0.1:9/push/1".to_owned();
    let registration = PushRegistration {
        target: PushTarget::WebPush { endpoint: endpoint.clone(), p256dh: point.as_bytes().to_vec(), auth: vec![1; 16] },
        level: PushLevel::Knocking,
    };
    assert_eq!(rec.ask("push-register", &Route::PushRegister { registration: &registration }, asked("push_register")), Ok(Answer::Done));
    let Ok(Answer::Push(state)) = rec.ask("push-state", &Route::PushState, asked("push_state")) else { panic!("the registrations are listed") };
    assert_eq!((state.subscriptions.len(), state.vapid_public_key.len()), (1, 65));
    assert_eq!((state.subscriptions[0].endpoint.as_str(), state.subscriptions[0].level), (endpoint.as_str(), PushLevel::Knocking));
    let activity = LiveActivity {
        start: true,
        token: "ab".repeat(32),
        tag: "desk".into(),
        environment: "development".into(),
        topic: "com.trommi.app.push-type.liveactivity".into(),
    };
    let live = rec.ask("live-activity", &Route::LiveActivity { activity: &activity }, asked("live_activity"));
    assert!(matches!(live, Ok(Answer::Done) | Err(Failure::Refused { code: Some(_), .. })), "{live:?}");
    assert_eq!(rec.ask("push-remove", &Route::PushRemove { endpoint: Some(&endpoint) }, asked("push_remove")), Ok(Answer::Removed { deleted: 1 }));
    assert_eq!(rec.ask("", &Route::PushRemove { endpoint: None }, json!({})), Ok(Answer::Removed { deleted: 0 }));

    // ---- the account is deleted, with the password of now
    let wrong = rec.ask("account-delete-wrong", &Route::DeleteAccount { proof: &proof }, asked("delete_account"));
    assert_eq!(wrong, Err(Failure::Refused { status: 401, code: Some("wrong-login".into()) }));
    // (a wrong proof makes its source wait: told as "not now" with the wait, then the hub's clock moves on)
    let proof = AccountProof::Password { auth_key: new_auth };
    let early = rec.ask("account-delete-wait", &Route::DeleteAccount { proof: &proof }, asked("delete_account"));
    assert_eq!(early, Err(Failure::NotNow { status: 429, retry_after_ms: Some(1000) }));
    w.hub.clock(2000);
    assert_eq!(rec.ask("account-delete", &Route::DeleteAccount { proof: &proof }, asked("delete_account")), Ok(Answer::Done));

    // ---- an account whose only way in is a passkey, made without an e-mail; signing in with the passkey
    use common::passkey::{Authenticator, ORIGIN, RP, UP_UV};
    use trommi_client_core::hub::{PasskeyAssertion, PasskeyRegistration};
    let w = World::new();
    rec = Recorder { port: w.hub.port, url: w.hub.url.clone(), token: None, write: rec.write };
    let token = w.ada.token.clone();
    let registration = |a: &Authenticator, challenge: &[u8], mark: u8| PasskeyRegistration {
        attestation_object: a.attestation(RP, UP_UV),
        client_data_json: Authenticator::client_data("webauthn.create", challenge, ORIGIN),
        sealed_copy: copy(mark),
        transports: Some(vec!["internal".into()]),
    };
    let key = Authenticator::new();
    let Ok(Answer::PasskeyChallenge(start)) = rec.ask("", &Route::PasskeyChallenge, json!({})) else { panic!("a passkey challenge is handed out") };
    let kit_auth: [u8; 32] = random();
    let account = NewAccount {
        email: None,
        kit: KitPart { auth_key: kit_auth, sealed_copy: copy(9) },
        password: None,
        passkey: Some(registration(&key, &start.challenge, 3)),
    };
    rec.token = token.clone();
    let created = rec.ask("account-create-passkey", &Route::CreateAccount { account: &account }, route("create_account"));
    let Ok(Answer::Account(view)) = created else { panic!("an account with a passkey is made: {created:?}") };
    assert_eq!((view.email.as_deref(), view.kit_form, view.has_password, view.account.as_str()), (None, KitForm::Id, false, start.account.as_str()));
    assert!(matches!(&view.passkeys[..], [one] if one.credential_id == key.credential_id && one.sealed_copy == copy(3)));
    rec.token = None;
    let Ok(Answer::PasskeyChallenge(open)) = rec.ask("", &Route::PasskeyChallenge, json!({})) else { panic!("a passkey challenge is handed out") };
    let client = Authenticator::client_data("webauthn.get", &open.challenge, ORIGIN);
    let (data, signature) = key.assertion(RP, UP_UV, &client);
    let assertion = PasskeyAssertion {
        credential_id: key.credential_id.clone(),
        authenticator_data: data,
        client_data_json: client,
        signature,
        user_handle: Some(view.user_handle.to_vec()),
    };
    let login = rec.ask("account-passkey-login", &Route::PasskeyLogin { assertion: &assertion }, asked("passkey_login"));
    let Ok(Answer::Login(login)) = login else { panic!("a passkey signs in: {login:?}") };
    assert_eq!((login.rooms.len(), login.rooms[0].sealed_copy, login.account.as_str()), (1, copy(3), view.account.as_str()));

    // ---- the account gets its e-mail (once), with the kit made anew under it
    rec.token = token.clone();
    let email = format!("ada-{}@example.org", trommi_hub::util::hex(&random::<6>()));
    let kit = KitPart { auth_key: random(), sealed_copy: copy(4) };
    let by_kit = AccountProof::Kit { auth_key: kit_auth };
    let put = Route::PutEmail { email: &email, kit: &kit, revision: view.revision, proof: &by_kit };
    assert_eq!(rec.ask("account-put-email", &put, asked("put_email")), Ok(Answer::Revision { revision: view.revision + 1 }));
    let by_kit = AccountProof::Kit { auth_key: kit.auth_key };
    let again = Route::PutEmail { email: &email, kit: &kit, revision: view.revision + 1, proof: &by_kit };
    assert_eq!(rec.ask("account-put-email-again", &again, asked("put_email")), Err(Failure::Refused { status: 403, code: Some("forbidden".into()) }));

    // ---- a second passkey, added with a challenge of this account, and removed
    let second = Authenticator::new();
    let Ok(Answer::PasskeyChallenge(own)) = rec.ask("", &Route::AccountPasskeyChallenge, json!({})) else {
        panic!("the account's passkey challenge is handed out")
    };
    assert_eq!((own.email.as_deref(), own.kit_form), (Some(email.as_str()), Some(KitForm::Email)));
    let passkey = registration(&second, &own.challenge, 6);
    let added = rec.ask("account-passkey-add", &Route::AddPasskey { passkey: &passkey, proof: &by_kit }, asked("add_passkey"));
    assert!(matches!(&added, Ok(Answer::PasskeyAdded { credential_id, .. }) if credential_id == &second.credential_id), "a passkey is added");
    let Ok(Answer::Account(both)) = rec.ask("account-passkeys", &Route::Account, route("account")) else { panic!("the account is read") };
    assert_eq!((both.passkeys.len(), both.email.as_deref()), (2, Some(email.as_str())));
    let asked_remove = json!({ "route": "remove_passkey", "credential_id": b64(&second.credential_id) });
    let removed = rec.ask("account-passkey-remove", &Route::RemovePasskey { credential_id: &second.credential_id, proof: &by_kit }, asked_remove);
    assert_eq!(removed, Ok(Answer::Done));
}

/// An agent device's lease (13.7) through the codec: `POST /v1/link` acquires and renews it, `Trommi-Lease` on a
/// write and on the stream is the generation the hub holds, and an older process is refused `lease-lost` on both.
#[test]
fn an_agents_lease_through_the_codec() {
    use trommi_client_core::hub::LinkReport;
    let mut w = World::on(TestHub::start());
    let mut agent = w.enrol_agent();
    w.found_main(&mut [], Some(&mut agent));
    let rec = Recorder { port: w.hub.port, url: w.hub.url.clone(), token: agent.token.clone(), write: false };
    let send = |lease: Option<u64>, route: &Route<'_>| {
        let site = Site { lease, ..rec.site() };
        let (request, expect) = Codec.request(&site, route).expect("the codec builds the route");
        let response = rec.send(&request);
        (Codec.answer(&expect, &response), Codec.refusal(&response).map(|r| r.code.to_string()))
    };
    let report = LinkReport { hears: true, working: false, last_call_at: 7 };
    let link = |process: &[u8; 16], generation: Option<u64>, leaving: bool| send(None, &Route::Link { process, generation, report: &report, leaving });
    // a new process acquires it above the one the enrolment took; the same process renews it under its generation
    let (one, two) = (random::<16>(), random::<16>());
    let Ok(Answer::Linked { generation: g1, expires_at }) = link(&one, None, false).0 else { panic!("a lease") };
    assert!(g1 > agent.lease.unwrap_or(0) && expires_at > 1_700_000_000_000);
    assert!(matches!(link(&one, Some(g1), false).0, Ok(Answer::Linked { generation, .. }) if generation == g1));
    // a write carries it; the stream too
    assert!(matches!(send(Some(g1), &Route::KeyPackagesHeld).0, Ok(Answer::KeyPackagesHeld { .. })));
    let (stream, _) = Codec.request(&Site { lease: Some(g1), ..rec.site() }, &Route::Stream { after: 0 }).expect("a stream");
    assert_eq!(stream.headers.iter().find(|h| h.name == "trommi-lease").map(|h| h.value.as_str()), Some(g1.to_string().as_str()));
    assert_eq!(rec.stream(&stream).0, 200);
    // a newer process takes it: the older one's renewal, write and stream are refused
    let Ok(Answer::Linked { generation: g2, .. }) = link(&two, None, false).0 else { panic!("a lease") };
    assert_eq!(g2, g1 + 1);
    assert_eq!(link(&one, Some(g1), false).1.as_deref(), Some("lease-lost"));
    assert_eq!(send(Some(g1), &Route::KeyPackagesHeld).1.as_deref(), Some("lease-lost"));
    let (stale, _) = Codec.request(&Site { lease: Some(g1), ..rec.site() }, &Route::Stream { after: 0 }).expect("a stream");
    assert_eq!(rec.stream(&stale).0, 409);
    assert!(matches!(send(Some(g2), &Route::KeyPackagesHeld).0, Ok(Answer::KeyPackagesHeld { .. })));
    // its last word gives it up
    assert!(matches!(link(&two, Some(g2), true).0, Ok(Answer::Linked { generation, .. }) if generation == g2));
}
