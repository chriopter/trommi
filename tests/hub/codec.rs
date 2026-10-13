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
use trommi_hub::wire::{self, ZERO16, ZERO32};

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
        self.record(name, &request, asked, response.status, &response.body);
        self.read(&expect, &response)
    }

    fn read(&self, expect: &Expect, response: &HttpResponse) -> Result<Answer, Failure> {
        Codec.answer(expect, response)
    }

    fn record(&self, name: &str, request: &HttpRequest, asked: Value, status: u16, answer: &[u8]) {
        if !self.write {
            return;
        }
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
    let mut cards = vec![];
    for urgency in [0, 3] {
        let id = enc::object_id(&group, &agent.id(), agent.chain(&group).0 + 1);
        let card = object(wire::KIND_VERSION, id, wire::TYPE_CARD, wire::STATE_OPEN, urgency, ZERO32, ZERO32);
        agent.send(&w.hub, &group, &card).ok();
        cards.push(id);
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
        rec.record("outbox-envelope-gap", &request, asked_entry(&gap), response.status, &response.body);
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
        vec![out.commit.clone(), out.group_info.clone(), out.welcome.clone().unwrap_or_default(), key],
    );
    let Ok(Answer::Outbox(committed)) = outbox(&rec, "outbox-commit", &e, None) else { panic!("a Commit is taken") };
    assert_eq!(committed.epoch, Some(out.epoch + 1));
    w.ada.merge(&group);
    // a Commit on the epoch that is taken
    {
        let stale = entry(OutboxKind::Commit, Some(&group), out.epoch, vec![vec![1; 40], vec![2; 40], vec![], vec![3; 40]]);
        let (request, expect) = Codec.request(&rec.site(), &Route::Outbox { room: &room, entry: &stale, extra: None }).unwrap();
        let response = rec.send(&request);
        rec.record("outbox-commit-refused", &request, asked_entry(&stale), response.status, &response.body);
        assert_eq!(rec.read(&expect, &response), Err(Failure::Refused { status: 409, code: Some("epoch-taken".into()) }));
        // (the group's epoch now, for building again)
        assert_eq!(Codec.refusal(&response).and_then(|r| r.epoch), Some(out.epoch + 1));
    }
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
    rec.record("stream", &request, json!({ "route": "stream", "after": 0 }), status, &bytes);
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
    let Ok(Answer::Challenge(challenge)) = rec.ask(&format!("challenge-{name}"), &Route::Challenge { room }, asked) else {
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
    match rec.ask(&format!("token-{name}"), &route, asked) {
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
        let asked = json!({ "route": "room_groups", "room": b64(&w.room), "after": after, "limit": 2 });
        let name = format!("room-groups-{pages}");
        let Ok(Answer::RoomGroups { items, more, after: next }) = rec.ask(&name, &Route::RoomGroups { room: &room, after, limit: 2 }, asked) else {
            panic!("{name} is read")
        };
        rows.extend(items);
        (after, pages) = (next, pages + 1);
        if !more {
            break;
        }
    }
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].kind, GroupRowKind::Room);
    assert_eq!(rows[0].leaves.len(), 2, "the founder and the device that joined");
    assert!(rows[1..].iter().all(|r| r.kind == GroupRowKind::Main && r.live && r.parent.is_none()));

    let nowhere: [u8; 48] = [&w.room[..], &random::<16>()[..]].concat().try_into().unwrap();
    let sessions = [group_id(&group), group_id(&group2), group_id(&nowhere)];
    let asked = json!({ "route": "groups_read", "groups": sessions.iter().map(|g| b64(g.as_bytes())).collect::<Vec<_>>() });
    let Ok(Answer::GroupsRead(read)) = rec.ask("groups-read", &Route::GroupsRead { groups: &sessions }, asked) else {
        panic!("the sessions are read in one request")
    };
    assert_eq!(read.len(), 3);
    assert_eq!(read[2], Err(Failure::Refused { status: 404, code: Some("not-found".into()) }));
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
