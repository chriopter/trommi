//! The hub codec, route by route: the request it builds, a good answer, and every malformation refused whole.

use trommi_client_core::app::{Header, HttpClass, HttpRequest, HttpResponse};
use trommi_client_core::hub::limits::{CHANGES_WINDOW, MAX_ENVELOPE, MAX_OPEN};
use trommi_client_core::hub::{
    Answer, ChangeItem, Codec, Expect, Failure, Only, OutboxAnswer, OutboxExtra, Route, Site,
};
use trommi_core::ids::{base64url_encode as b64, GroupId, ObjectId, RoomId, SessionId};
use trommi_core::store::{OutboxEntry, OutboxKind};

const SITE: Site<'static> = Site {
    url: "https://hub.example",
    token: Some("abcdefghijklmnop_-0123"),
    client: "web/1.2.3",
};

fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: body.as_bytes().to_vec().into(),
    }
}

fn ask(route: &Route<'_>) -> (HttpRequest, Expect) {
    Codec.request(&SITE, route).expect("the route builds")
}

fn header<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request
        .headers
        .iter()
        .find(|h| h.name == name)
        .map(|h| h.value.as_str())
}

fn body(request: &HttpRequest) -> serde_json::Value {
    serde_json::from_slice(request.body.as_ref().expect("a body")).expect("a JSON body")
}

fn refused(expect: &Expect, body: &str) {
    assert!(
        matches!(
            Codec.answer(expect, &ok(body)),
            Err(Failure::BadAnswer { .. })
        ),
        "taken, and must not be: {}",
        &body[..body.len().min(300)]
    );
}

fn room() -> RoomId {
    RoomId::new([1; 32])
}

fn session_group() -> GroupId {
    GroupId::session(room(), SessionId::new([2; 16]))
}

// ---- catch-up

fn envelope(change: u64) -> String {
    format!(r#"{{"change":{change},"kind":"envelope","envelope":"AAEC","received_at":5}}"#)
}

fn commit(change: u64) -> String {
    let (group, sender) = (b64(&[7; 48]), b64(&[9; 32]));
    format!(
        r#"{{"change":{change},"kind":"commit","group_id":"{group}","n":1,"epoch":0,"at":5,"bytes":"AAEC","sender":"{sender}"}}"#
    )
}

fn page(items: &[String], change: u64, more: bool) -> String {
    format!(
        r#"{{"items":[{}],"change":{change},"more":{more},"head":{}}}"#,
        items.join(","),
        change + more as u64
    )
}

fn changes(after: u64) -> Expect {
    ask(&Route::Changes {
        after,
        limit: 3,
        only: Only::All,
    })
    .1
}

#[test]
fn changes_the_request_names_the_cursor_and_what_is_wanted() {
    let group = session_group();
    let (r, _) = ask(&Route::Changes {
        after: 12,
        limit: 5000,
        only: Only::Active,
    });
    assert_eq!(
        r.url,
        "https://hub.example/v1/changes?after=12&limit=1000&active=1"
    );
    assert_eq!(
        (r.method.as_str(), r.class, r.body.is_none()),
        ("GET", HttpClass::Background, true)
    );
    assert_eq!(
        header(&r, "authorization"),
        Some("Bearer abcdefghijklmnop_-0123")
    );
    assert_eq!(header(&r, "trommi-client"), Some("web/1.2.3"));
    assert_eq!(r.max_answer, 16 << 20);
    let (r, _) = ask(&Route::Changes {
        after: 0,
        limit: 0,
        only: Only::Group(&group),
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/changes?after=0&limit=1&group={}",
            b64(group.as_bytes())
        )
    );
    // a cursor no hub can hold, a site without a token
    let far = Route::Changes {
        after: 1 << 53,
        limit: 1,
        only: Only::All,
    };
    assert!(Codec.request(&SITE, &far).is_err());
    let nobody = Site {
        token: None,
        ..SITE
    };
    assert!(Codec
        .request(
            &nobody,
            &Route::Changes {
                after: 0,
                limit: 1,
                only: Only::All
            }
        )
        .is_err());
}

#[test]
fn changes_a_good_page_parses() {
    let Ok(Answer::Changes(got)) = Codec.answer(
        &changes(10),
        &ok(&page(&[commit(11), envelope(14)], 20, true)),
    ) else {
        panic!("a good page")
    };
    assert_eq!((got.change, got.more, got.head), (20, true, Some(21)));
    assert_eq!(
        got.items.iter().map(ChangeItem::change).collect::<Vec<_>>(),
        [11, 14]
    );
    assert!(
        matches!(&got.items[0], ChangeItem::Commit(l) if l.n == 1 && l.recovery_auth.is_none() && l.bytes == [0, 1, 2])
    );
    assert!(
        matches!(&got.items[1], ChangeItem::Envelope(e) if e.envelope == [0, 1, 2] && !e.cut && e.received_at == 5)
    );
    // nothing new: the cursor stays; a hub that names no head
    let Ok(Answer::Changes(got)) = Codec.answer(
        &changes(10),
        &ok(r#"{"items":[],"change":10,"more":false}"#),
    ) else {
        panic!("an empty page")
    };
    assert_eq!((got.items.len(), got.change, got.head), (0, 10, None));
    // a member this client does not know is passed over
    assert!(Codec
        .answer(
            &changes(10),
            &ok(r#"{"items":[],"change":10,"more":false,"later":[1]}"#)
        )
        .is_ok());
}

#[test]
fn changes_every_malformation_refuses_the_whole_page() {
    let a = changes(10);
    for body in [
        // order: not ascending, twice, at or below the cursor asked from, above the answer's cursor
        page(&[envelope(14), envelope(11)], 20, false),
        page(&[envelope(11), envelope(11)], 20, false),
        page(&[envelope(10)], 20, false),
        page(&[envelope(21)], 20, false),
        // the cursor: backwards, beyond the window, no step with more
        page(&[], 9, false),
        page(&[], 10 + CHANGES_WINDOW + 1, false),
        page(&[], 10, true),
        // counts and types
        page(
            &[envelope(11), envelope(12), envelope(13), envelope(14)],
            20,
            false,
        ),
        r#"{"items":{},"change":20,"more":false}"#.to_owned(),
        r#"{"items":[],"change":"20","more":false}"#.to_owned(),
        r#"{"items":[],"change":20.0,"more":false}"#.to_owned(),
        r#"{"items":[],"change":20,"more":0}"#.to_owned(),
        r#"{"items":[],"change":20}"#.to_owned(),
        r#"{"items":[],"change":20,"change":20,"more":false}"#.to_owned(),
        r#"{"items":[],"change":20,"more":false,"head":19}"#.to_owned(),
        r#"{"items":[],"change":20,"more":false,"head":-1}"#.to_owned(),
        r#"{"items":[],"change":20,"more":false,"head":9007199254740992}"#.to_owned(),
        r#"{"items":[7],"change":20,"more":false}"#.to_owned(),
        r#"{"items":[],"change":20,"more":false} trailing"#.to_owned(),
        "[]".to_owned(),
        String::new(),
        // an item: no change, change 0, bad base64url, an empty envelope, a wrong void code
        page(
            &[r#"{"kind":"envelope","envelope":"AAEC","received_at":5}"#.to_owned()],
            20,
            false,
        ),
        page(&[envelope(0)], 20, false),
        page(&[envelope(11).replace("AAEC", "AAEC=")], 20, false),
        page(&[envelope(11).replace("AAEC", "AAF")], 20, false),
        page(&[envelope(11).replace("AAEC", "AA+/")], 20, false),
        page(&[envelope(11).replace("AAEC", "")], 20, false),
        page(
            &[envelope(11).replace(
                r#""received_at":5"#,
                r#""received_at":5,"void_code":"Bad Code""#,
            )],
            20,
            false,
        ),
        // a log entry: ids of the wrong length, an unknown kind, n 0, an empty RecoveryAuth
        page(
            &[commit(11).replace(&b64(&[7; 48]), &b64(&[7; 47]))],
            20,
            false,
        ),
        page(
            &[commit(11).replace(&b64(&[9; 32]), &b64(&[9; 31]))],
            20,
            false,
        ),
        page(&[commit(11).replace("commit", "proposal")], 20, false),
        page(&[commit(11).replace(r#""n":1"#, r#""n":0"#)], 20, false),
        page(
            &[commit(11).replace(r#""at":5"#, r#""at":5,"recovery_auth":"""#)],
            20,
            false,
        ),
        // an oversize envelope
        page(
            &[envelope(11).replace("AAEC", &b64(&vec![0; MAX_ENVELOPE + 1]))],
            20,
            false,
        ),
    ] {
        refused(&a, &body);
    }
    // an envelope of the largest size is taken
    let big = envelope(11).replace("AAEC", &b64(&vec![0; MAX_ENVELOPE]));
    assert!(Codec.answer(&a, &ok(&page(&[big], 20, false))).is_ok());
    // one group's envelopes hold nothing of the log
    let group = session_group();
    let (_, one) = ask(&Route::Changes {
        after: 10,
        limit: 3,
        only: Only::Group(&group),
    });
    refused(&one, &page(&[commit(11)], 20, false));
    assert!(Codec
        .answer(&one, &ok(&page(&[envelope(11)], 20, false)))
        .is_ok());
}

#[test]
fn an_answer_over_its_cap_is_refused_before_it_is_read() {
    let mut body = page(&[], 20, false);
    body.push_str(&" ".repeat((16 << 20) + 1 - body.len()));
    refused(&changes(10), &body);
    // and one that would build far more values than its bytes are worth
    let zeros = format!(
        r#"{{"items":[],"change":20,"more":false,"x":[{}0]}}"#,
        "0,".repeat(1 << 20)
    );
    refused(&changes(10), &zeros);
}

#[test]
fn an_error_status_is_its_failure_on_every_route() {
    let a = changes(0);
    let answer = |status: u16, headers: &[(&str, &str)], body: &str| HttpResponse {
        status,
        headers: headers
            .iter()
            .map(|(n, v)| Header {
                name: n.to_string(),
                value: v.to_string(),
            })
            .collect(),
        body: body.as_bytes().to_vec().into(),
    };
    assert_eq!(
        Codec
            .answer(
                &a,
                &answer(403, &[], r#"{"error":"not-member","message":"x"}"#)
            )
            .unwrap_err(),
        Failure::Refused {
            status: 403,
            code: Some("not-member".into())
        }
    );
    // a proxy's page under 404, a good-looking body under 404: neither is the hub's refusal, neither is an answer
    assert_eq!(
        Codec.answer(&a, &answer(404, &[], "<html>")).unwrap_err(),
        Failure::Refused {
            status: 404,
            code: None
        }
    );
    assert_eq!(
        Codec
            .answer(&a, &answer(404, &[], &page(&[], 0, false)))
            .unwrap_err(),
        Failure::Refused {
            status: 404,
            code: None
        }
    );
    assert_eq!(
        Codec
            .answer(&a, &answer(503, &[("retry-after", "3")], ""))
            .unwrap_err(),
        Failure::NotNow {
            status: 503,
            retry_after_ms: Some(3000)
        }
    );
    assert!(matches!(
        Codec.answer(
            &a,
            &answer(
                307,
                &[("location", "https://elsewhere.example/v1/changes")],
                ""
            )
        ),
        Err(Failure::BadAnswer { .. })
    ));
    let void = answer(
        409,
        &[],
        r#"{"error":"gap","message":"x","seq":4,"voided":true}"#,
    );
    assert!(Codec.voided(&void));
    assert_eq!(Codec.refusal(&void).and_then(|r| r.seq), Some(4));
    assert!(!Codec.voided(&answer(409, &[], r#"{"error":"gap","message":"x"}"#)));
    assert!(!Codec.voided(&answer(502, &[], r#"{"voided":true}"#)));
}

// ---- the open list

fn listed(n: u8, version: u64, head: u64) -> String {
    format!(
        r#"{{"object_id":"{}","version_change":{version},"head_change":{head}}}"#,
        b64(&[n; 16])
    )
}

fn brief(cards: &[String], requests: &[String], settled: Option<&[u8]>, change: u64) -> String {
    let settled = settled.map_or(String::new(), |ids| {
        let ids: Vec<String> = ids
            .iter()
            .map(|n| format!("\"{}\"", b64(&[*n; 16])))
            .collect();
        format!(r#","settled":[{}]"#, ids.join(","))
    });
    format!(
        r#"{{"cards":[{}],"permission_requests":[{}]{settled},"truncated":false,"change":{change}}}"#,
        cards.join(","),
        requests.join(",")
    )
}

#[test]
fn desk_brief_requests() {
    let (r, _) = ask(&Route::DeskBrief {
        since: None,
        active: true,
    });
    assert_eq!(r.url, "https://hub.example/v1/desk?brief=1&active=1");
    assert_eq!((r.method.as_str(), r.max_answer), ("GET", 16 << 20));
    let (r, _) = ask(&Route::DeskBrief {
        since: Some(40),
        active: false,
    });
    assert_eq!(r.url, "https://hub.example/v1/desk?brief=1&since=40");
}

#[test]
fn desk_brief_a_good_list_parses() {
    let (_, whole) = ask(&Route::DeskBrief {
        since: None,
        active: true,
    });
    let body = brief(
        &[listed(1, 5, 9), listed(2, 7, 7)],
        &[listed(3, 8, 8)],
        None,
        9,
    );
    let Ok(Answer::DeskBrief(got)) = Codec.answer(&whole, &ok(&body)) else {
        panic!("a good list")
    };
    assert_eq!(got.cards.len(), 2);
    assert_eq!(got.cards[0].object, ObjectId::new([1; 16]));
    assert_eq!(
        (got.cards[0].version_change, got.cards[0].head_change),
        (5, 9)
    );
    assert_eq!(
        (
            got.permission_requests.len(),
            got.settled,
            got.truncated,
            got.change
        ),
        (1, None, false, 9)
    );
    let (_, since) = ask(&Route::DeskBrief {
        since: Some(6),
        active: true,
    });
    let body = brief(&[listed(1, 5, 9)], &[], Some(&[4, 5]), 9);
    let Ok(Answer::DeskBrief(got)) = Codec.answer(&since, &ok(&body)) else {
        panic!("a good list")
    };
    assert_eq!(
        got.settled,
        Some(vec![ObjectId::new([4; 16]), ObjectId::new([5; 16])])
    );
}

#[test]
fn desk_brief_every_malformation_refuses_the_whole_list() {
    let (_, whole) = ask(&Route::DeskBrief {
        since: None,
        active: true,
    });
    let (_, since) = ask(&Route::DeskBrief {
        since: Some(6),
        active: true,
    });
    for body in [
        // an object twice: among the cards, as a card and as a request
        brief(&[listed(1, 5, 9), listed(1, 5, 9)], &[], None, 9),
        brief(&[listed(1, 5, 9)], &[listed(1, 5, 9)], None, 9),
        // an id of the wrong length, a version of 0, a row newer than the list
        brief(
            &[listed(1, 5, 9).replace(&b64(&[1; 16]), &b64(&[1; 15]))],
            &[],
            None,
            9,
        ),
        brief(&[listed(1, 0, 9)], &[], None, 9),
        brief(&[listed(1, 5, 10)], &[], None, 9),
        brief(&[listed(1, 10, 9)], &[], None, 9),
        // types
        r#"{"cards":{},"permission_requests":[],"truncated":false,"change":9}"#.to_owned(),
        r#"{"cards":[],"truncated":false,"change":9}"#.to_owned(),
        r#"{"cards":[],"permission_requests":[],"truncated":"no","change":9}"#.to_owned(),
        r#"{"cards":[],"permission_requests":[],"truncated":false}"#.to_owned(),
        r#"{"cards":[7],"permission_requests":[],"truncated":false,"change":9}"#.to_owned(),
    ] {
        refused(&whole, &body);
    }
    for body in [
        // with since: no settled list, a row that did not change since, an object open and settled, settled twice
        brief(&[listed(1, 5, 9)], &[], None, 9),
        brief(&[listed(1, 5, 6)], &[], Some(&[]), 9),
        brief(&[listed(1, 5, 9)], &[], Some(&[1]), 9),
        brief(&[], &[], Some(&[4, 4]), 9),
        brief(&[], &[], Some(&[4]), 9).replace(&b64(&[4; 16]), "AAEC"),
    ] {
        refused(&since, &body);
    }
    // more rows than the list may hold, counted over all three lists
    let many: Vec<String> = (0..MAX_OPEN as u32)
        .map(|i| {
            let mut id = [0u8; 16];
            id[..4].copy_from_slice(&i.to_be_bytes());
            format!(
                r#"{{"object_id":"{}","version_change":7,"head_change":7}}"#,
                b64(&id)
            )
        })
        .collect();
    assert!(Codec
        .answer(&whole, &ok(&brief(&many, &[], None, 9)))
        .is_ok());
    refused(&whole, &brief(&many, &[listed(255, 7, 7)], None, 9));
    refused(&since, &brief(&many, &[], Some(&[255]), 9));
}

// ---- the outbox

fn entry(kind: OutboxKind, group: Option<GroupId>, epoch: u64, parts: &[&[u8]]) -> OutboxEntry {
    OutboxEntry {
        id: 1,
        kind,
        group,
        epoch,
        parts: parts.iter().map(|p| p.to_vec()).collect(),
    }
}

fn outbox(
    entry: &OutboxEntry,
    extra: Option<&OutboxExtra>,
) -> Result<(HttpRequest, Expect), Failure> {
    let room = room();
    Codec.request(
        &SITE,
        &Route::Outbox {
            room: &room,
            entry,
            extra,
        },
    )
}

fn taken(expect: &Expect, body: &str) -> OutboxAnswer {
    match Codec.answer(expect, &ok(body)) {
        Ok(Answer::Outbox(answer)) => answer,
        other => panic!("not taken: {other:?}"),
    }
}

#[test]
fn outbox_an_envelope() {
    let e = entry(
        OutboxKind::Envelope,
        Some(session_group()),
        3,
        &[&[0, 1, 2]],
    );
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/envelopes")
    );
    assert_eq!(body(&r), serde_json::json!({ "envelope": "AAEC" }));
    assert_eq!(header(&r, "content-type"), Some("application/json"));
    assert_eq!(r.max_answer, 64 << 10);
    assert_eq!(taken(&x, r#"{"change":41}"#).change, Some(41));
    for bad in [
        r#"{"change":0}"#,
        r#"{"change":"41"}"#,
        r#"{}"#,
        r#"[41]"#,
        r#"{"change":41}{"change":42}"#,
    ] {
        refused(&x, bad);
    }
    assert!(outbox(&entry(OutboxKind::Envelope, None, 0, &[]), None).is_err());
    assert!(outbox(&entry(OutboxKind::Envelope, None, 0, &[&[]]), None).is_err());
    assert!(outbox(&entry(OutboxKind::Envelope, None, 0, &[&[1], &[2]]), None).is_err());
}

#[test]
fn outbox_commits() {
    let group = session_group();
    let path = format!(
        "https://hub.example/v1/groups/{}/commits",
        b64(group.as_bytes())
    );
    // a member's Commit: Commit, GroupInfo, Welcome (may be empty), SealedKey
    let e = entry(OutboxKind::Commit, Some(group), 7, &[&[1], &[2], &[], &[4]]);
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(r.url, path);
    assert_eq!(
        body(&r),
        serde_json::json!({ "epoch": 7, "commit": "AQ", "group_info": "Ag", "sealed_key": "BA" })
    );
    let e = entry(
        OutboxKind::Commit,
        Some(group),
        7,
        &[&[1], &[2], &[3], &[4]],
    );
    assert_eq!(body(&outbox(&e, None).unwrap().0)["welcome"], "Aw");
    let got = taken(&x, r#"{"epoch":8,"change":90}"#);
    assert_eq!((got.epoch, got.change), (Some(8), Some(90)));
    // another epoch than the next one, no change
    for bad in [
        r#"{"epoch":7,"change":90}"#,
        r#"{"epoch":9,"change":90}"#,
        r#"{"epoch":8,"change":0}"#,
        r#"{"epoch":8}"#,
    ] {
        refused(&x, bad);
    }
    // a join from outside: Commit, GroupInfo, SealedKey, RecoveryAuth
    let e = entry(
        OutboxKind::ExternalCommit,
        Some(group),
        7,
        &[&[1], &[2], &[4], &[5]],
    );
    let (r, _) = outbox(&e, None).unwrap();
    assert_eq!(
        body(&r),
        serde_json::json!({ "epoch": 7, "commit": "AQ", "group_info": "Ag", "sealed_key": "BA", "recovery_auth": "BQ" })
    );
    assert!(outbox(
        &entry(
            OutboxKind::ExternalCommit,
            Some(group),
            7,
            &[&[1], &[2], &[4], &[]]
        ),
        None
    )
    .is_err());
    assert!(outbox(
        &entry(OutboxKind::Commit, None, 7, &[&[1], &[2], &[], &[4]]),
        None
    )
    .is_err());
    assert!(outbox(
        &entry(OutboxKind::Commit, Some(group), 7, &[&[1], &[2], &[]]),
        None
    )
    .is_err());
}

#[test]
fn outbox_messages() {
    let group = session_group();
    let e = entry(OutboxKind::Message, Some(group), 2, &[&[9]]);
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/groups/{}/messages",
            b64(group.as_bytes())
        )
    );
    assert_eq!(body(&r), serde_json::json!({ "epoch": 2, "message": "CQ" }));
    assert_eq!(taken(&x, r#"{"n":5}"#).n, Some(5));
    refused(&x, r#"{"n":null}"#);
    refused(&x, r#"{"n":0}"#);
    let e = entry(OutboxKind::RelayMessage, Some(group), 2, &[&[9]]);
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(
        body(&r),
        serde_json::json!({ "epoch": 2, "message": "CQ", "relay": true })
    );
    assert_eq!(taken(&x, r#"{"n":null}"#), OutboxAnswer::default());
    refused(&x, r#"{"n":5}"#);
}

#[test]
fn outbox_foundings() {
    let room_group = GroupId::room(room());
    let e = entry(OutboxKind::RoomFounding, Some(room_group), 0, &[&[1], &[2]]);
    // no token, the account made with the room, the founding word
    let nobody = Site {
        token: None,
        ..SITE
    };
    let extra = OutboxExtra {
        account: Some(br#"{"kit":{"auth_key":"x"}}"#.to_vec()),
        found_token: Some("the-word".to_owned()),
        recovery_id: None,
    };
    let (r, x) = Codec
        .request(
            &nobody,
            &Route::Outbox {
                room: &room(),
                entry: &e,
                extra: Some(&extra),
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/rooms")
    );
    assert_eq!(header(&r, "authorization"), None);
    assert_eq!(header(&r, "x-found-token"), Some("the-word"));
    assert_eq!(
        body(&r),
        serde_json::json!({ "group_info": "AQ", "sealed_key": "Ag", "account": { "kit": { "auth_key": "x" } } })
    );
    assert_eq!(
        taken(&x, &format!(r#"{{"room_id":"{}"}}"#, b64(&[1; 32]))).room,
        Some(room())
    );
    refused(&x, &format!(r#"{{"room_id":"{}"}}"#, b64(&[3; 32])));
    refused(&x, &format!(r#"{{"room_id":"{}"}}"#, b64(&[1; 31])));
    for (account, word) in [
        (&b"[1]"[..], "w"),
        (b"{", "w"),
        (b"{}", "two words"),
        (b"{}", "line\nbreak"),
        (b"{}", ""),
    ] {
        let extra = OutboxExtra {
            account: Some(account.to_vec()),
            found_token: Some(word.to_owned()),
            recovery_id: None,
        };
        assert!(outbox(&e, Some(&extra)).is_err(), "{word:?}");
    }

    let group = session_group();
    let e = entry(
        OutboxKind::GroupFounding,
        Some(group),
        0,
        &[&[1], &[2], &[3], &[4], &[], &[6]],
    );
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(r.url, "https://hub.example/v1/groups");
    assert_eq!(
        body(&r),
        serde_json::json!({ "group_info_0": "AQ", "sealed_key_0": "Ag", "commit": "Aw", "group_info": "BA", "sealed_key": "Bg" })
    );
    assert_eq!(
        taken(
            &x,
            &format!(r#"{{"group_id":"{}"}}"#, b64(group.as_bytes()))
        )
        .group,
        Some(group)
    );
    refused(&x, &format!(r#"{{"group_id":"{}"}}"#, b64(&[1; 48])));
    refused(&x, &format!(r#"{{"group_id":"{}"}}"#, b64(&[1; 32])));
}

#[test]
fn outbox_key_packages_and_sealed_keys() {
    let e = entry(OutboxKind::KeyPackages, None, 0, &[&[1], &[2], &[3]]);
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("PUT", "https://hub.example/v1/key-packages")
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "last_resort": "AQ", "single_use": ["Ag", "Aw"] })
    );
    assert_eq!(taken(&x, r#"{"unused":2}"#).unused, Some(2));
    refused(&x, r#"{"unused":-1}"#);
    refused(&x, r#"{"unused":4294967296}"#);
    let e = entry(OutboxKind::KeyPackages, None, 0, &[&[], &[2]]);
    assert_eq!(
        body(&outbox(&e, None).unwrap().0),
        serde_json::json!({ "single_use": ["Ag"] })
    );
    assert!(outbox(&entry(OutboxKind::KeyPackages, None, 0, &[]), None).is_err());
    let many: Vec<&[u8]> = std::iter::repeat_n(&[1u8][..], 102).collect();
    assert!(outbox(&entry(OutboxKind::KeyPackages, None, 0, &many), None).is_err());

    let (r, x) = ask(&Route::KeyPackagesHeld);
    assert_eq!(body(&r), serde_json::json!({ "single_use": [] }));
    assert!(matches!(
        Codec.answer(&x, &ok(r#"{"unused":7}"#)),
        Ok(Answer::KeyPackagesHeld { unused: 7 })
    ));
    refused(&x, r#"{"unused":"7"}"#);

    let e = entry(OutboxKind::SealedKey, None, 0, &[&[1]]);
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("PUT", "https://hub.example/v1/sealed-keys")
    );
    assert_eq!(body(&r), serde_json::json!({ "sealed_key": "AQ" }));
    assert_eq!(taken(&x, r#"{"stored":true}"#), OutboxAnswer::default());
    refused(&x, r#"{"stored":false}"#);
    refused(&x, r#"{}"#);
}

#[test]
fn outbox_recovery() {
    let room_b64 = b64(&[1; 32]);
    let room_group = GroupId::room(room());
    let (key, copy) = (b64(&[5; 32]), b64(&[2; 61]));
    let copies = format!(
        r#"{{"kit":{{"auth_key":"{key}","sealed_copy":"{copy}"}},"password":{{"sealed_copy":"{copy}"}},"revision":3}}"#
    );
    // the replacing of the code: Commit, GroupInfo, SealedKey, RecoveryLink, the account's copies
    let e = entry(
        OutboxKind::RecoveryCode,
        Some(room_group),
        4,
        &[&[1], &[2], &[3], &[4], copies.as_bytes()],
    );
    let (r, x) = outbox(&e, None).unwrap();
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/rooms/{room_b64}/recovery-code")
    );
    assert_eq!(
        body(&r),
        serde_json::json!({
            "commit": { "epoch": 4, "commit": "AQ", "group_info": "Ag", "sealed_key": "Aw" },
            "recovery_link": "BA",
            "account": serde_json::from_str::<serde_json::Value>(&copies).unwrap(),
        })
    );
    assert_eq!(taken(&x, r#"{"epoch":5,"change":12}"#).epoch, Some(5));
    refused(&x, r#"{"epoch":4,"change":12}"#);
    // a room without an account sends null
    let e = entry(
        OutboxKind::RecoveryCode,
        Some(room_group),
        4,
        &[&[1], &[2], &[3], &[4], &[]],
    );
    assert_eq!(
        body(&outbox(&e, None).unwrap().0)["account"],
        serde_json::Value::Null
    );
    // copies that are not the body the hub takes
    for bad in [
        "[]".to_owned(),
        copies.replace("\"revision\":3", "\"revision\":-3"),
        copies.replace("\"revision\":3", "\"other\":3"),
        copies.replace(&key, &b64(&[5; 31])),
        copies.replace(
            "\"password\":{",
            "\"passkey\":{\"credential_id\":\"AQ\",\"sealed_copy\":\"x\"},\"password\":{",
        ),
        copies.replace(
            &format!(r#""password":{{"sealed_copy":"{copy}"}}"#),
            r#""password":{"sealed_copy":"AQ"}"#,
        ),
    ] {
        let e = entry(
            OutboxKind::RecoveryCode,
            Some(room_group),
            4,
            &[&[1], &[2], &[3], &[4], bad.as_bytes()],
        );
        assert!(outbox(&e, None).is_err(), "{bad}");
    }

    // a part of a recovery, and its finish
    let extra = OutboxExtra {
        account: None,
        found_token: None,
        recovery_id: Some(vec![8; 16]),
    };
    let group = session_group();
    let e = entry(
        OutboxKind::RecoveryCommit,
        Some(group),
        4,
        &[&[1], &[2], &[], &[4], &[5]],
    );
    assert!(outbox(&e, None).is_err(), "no recovery named");
    let (r, x) = outbox(&e, Some(&extra)).unwrap();
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/rooms/{room_b64}/recovery/{}/commits",
            b64(&[8; 16])
        )
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "group_id": b64(group.as_bytes()), "epoch": 4, "commit": "AQ", "group_info": "Ag", "sealed_key": "BA", "recovery_auth": "BQ" })
    );
    let got = taken(&x, r#"{"epoch":5,"kept":true}"#);
    assert_eq!(
        (got.epoch, got.kept, got.change),
        (Some(5), Some(true), None)
    );
    refused(&x, r#"{"epoch":5}"#);

    let e = entry(OutboxKind::RecoveryFinish, None, 0, &[&[4], &[]]);
    let (r, x) = outbox(&e, Some(&extra)).unwrap();
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/rooms/{room_b64}/recovery/{}/finish",
            b64(&[8; 16])
        )
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "recovery_link": "BA", "account": null })
    );
    let device = b64(&[6; 32]);
    let got = taken(
        &x,
        &format!(r#"{{"published":true,"first_change":20,"change":24,"device":"{device}"}}"#),
    );
    let finished = got.finished.unwrap();
    assert_eq!(
        (got.change, finished.published, finished.first_change),
        (Some(24), true, 20)
    );
    refused(
        &x,
        &format!(r#"{{"published":true,"first_change":25,"change":24,"device":"{device}"}}"#),
    );
    refused(
        &x,
        r#"{"published":true,"first_change":20,"change":24,"device":"AQ"}"#,
    );
    let short = OutboxExtra {
        recovery_id: Some(vec![8; 15]),
        ..OutboxExtra::default()
    };
    assert!(outbox(&e, Some(&short)).is_err());
}
