//! The codec's Desk, Chat, board, sealed key, removal, recovery, file and Share routes: the request each builds, a
//! good answer, and every malformation refused whole.

use serde_json::{json, Value};
use trommi_client_core::app::{Header, HttpClass, HttpRequest, HttpResponse};
use trommi_client_core::hub::limits::CAP_FILE;
use trommi_client_core::hub::{
    Answer, ByteRange, Codec, DeskOnly, Expect, Failure, Route, Site, Timeline,
};
use trommi_core::ids::{
    base64url_encode as b64, BoardId, DeviceId, FileId, GroupId, Hash32, ObjectId, RoomId,
    SessionId, ShareId,
};

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

fn body(request: &HttpRequest) -> Value {
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

fn taken(expect: &Expect, body: &str) -> Answer {
    Codec
        .answer(expect, &ok(body))
        .unwrap_or_else(|e| panic!("not taken ({e:?}): {}", &body[..body.len().min(300)]))
}

fn group() -> GroupId {
    GroupId::session(RoomId::new([1; 32]), SessionId::new([2; 16]))
}

fn envelope(change: u64) -> String {
    format!(r#"{{"change":{change},"envelope":"AAEC","received_at":5}}"#)
}

// ---- the Desk

/// The Desk the real hub answered (recorded), as JSON to bend.
fn real_desk() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hub/desk.json");
    let vector: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    serde_json::from_str(vector["answer"].as_str().unwrap()).unwrap()
}

#[test]
fn the_desk() {
    for (only, url) in [
        (DeskOnly::All, "https://hub.example/v1/desk"),
        (DeskOnly::Active, "https://hub.example/v1/desk?active=1"),
        (DeskOnly::Archived, "https://hub.example/v1/desk?archived=1"),
    ] {
        let (r, _) = ask(&Route::Desk { only });
        assert_eq!(
            (r.url.as_str(), r.max_answer, r.class),
            (url, 32 << 20, HttpClass::Background)
        );
    }
    let (_, x) = ask(&Route::Desk {
        only: DeskOnly::Active,
    });
    let good = real_desk();
    let Answer::Desk(desk) = taken(&x, &good.to_string()) else {
        panic!()
    };
    assert!(desk.cards.len() == 2 && desk.registers.len() == 2);
    // a hub that sends no groups with the Desk, an object whose version was cut off
    let mut bare = good.clone();
    bare.as_object_mut().unwrap().remove("groups");
    bare["cards"][0]["version"] = Value::Null;
    let Answer::Desk(desk) = taken(&x, &bare.to_string()) else {
        panic!()
    };
    assert!(desk.groups.is_none() && desk.cards[0].version.is_none());

    let bend = |change: &dyn Fn(&mut Value)| {
        let mut desk = good.clone();
        change(&mut desk);
        refused(&x, &desk.to_string());
    };
    // a version that is another object's (its own header says so), or another group's
    bend(&|d| {
        let other = d["cards"][1]["version"].clone();
        d["cards"][0]["version"] = other;
    });
    bend(&|d| d["cards"][0]["group_id"] = json!(b64(&[9; 48])));
    bend(&|d| d["cards"][0]["object_id"] = json!(b64(&[9; 16])));
    // an object twice, as a card and as an Artifact too
    bend(&|d| {
        let again = d["cards"][0].clone();
        d["cards"].as_array_mut().unwrap().push(again);
    });
    bend(&|d| {
        let again = d["cards"][0].clone();
        d["artifacts"].as_array_mut().unwrap().push(again);
    });
    // not open; change numbers out of their order; a version outside its object's changes; newer than the Desk
    bend(&|d| d["cards"][0]["state"] = json!(2));
    bend(&|d| {
        d["cards"][0]["first_change"] = json!(d["cards"][0]["head_change"].as_u64().unwrap() + 1)
    });
    bend(&|d| d["cards"][0]["head_change"] = json!(d["change"].as_u64().unwrap() + 1));
    bend(&|d| {
        d["cards"][0]["version"]["change"] =
            json!(d["cards"][0]["head_change"].as_u64().unwrap() + 1)
    });
    bend(&|d| d["cards"][0]["version"]["change"] = json!(1));
    // ids and types
    bend(&|d| d["cards"][0]["owner"] = json!(b64(&[9; 31])));
    bend(&|d| d["cards"][0]["urgency"] = json!(256));
    bend(&|d| d["cards"][0]["version"]["envelope"] = json!("not base64url!"));
    bend(&|d| d["notes"] = json!({}));
    bend(&|d| d["truncated"] = json!("no"));
    bend(&|d| {
        d.as_object_mut().unwrap().remove("permission_requests");
    });
    // registers: out of order, one twice, a card's version among them, newer than the Desk
    bend(&|d| d["registers"].as_array_mut().unwrap().reverse());
    bend(&|d| {
        let again = d["registers"][0].clone();
        d["registers"].as_array_mut().unwrap().insert(0, again);
    });
    bend(&|d| {
        let mut again = d["registers"][1].clone();
        again["change"] = json!(again["change"].as_u64().unwrap() + 1);
        d["change"] = json!(d["change"].as_u64().unwrap() + 5);
        d["registers"].as_array_mut().unwrap().push(again);
    });
    bend(&|d| {
        let mut card = d["cards"][0]["version"].clone();
        card["change"] = json!(d["registers"][0]["change"].as_u64().unwrap() - 1);
        d["registers"].as_array_mut().unwrap().insert(0, card);
    });
    bend(&|d| d["change"] = json!(d["registers"][1]["change"].as_u64().unwrap() - 1));
    // the groups: one twice
    bend(&|d| {
        let again = d["groups"][0].clone();
        d["groups"].as_array_mut().unwrap().push(again);
    });
    // more objects of a kind than the Desk holds
    bend(&|d| {
        let cards: Vec<Value> = (0..1001u32)
            .map(|i| {
                let mut id = [0u8; 16];
                id[..4].copy_from_slice(&i.to_be_bytes());
                let mut card = d["cards"][0].clone();
                card["object_id"] = json!(b64(&id));
                card["version"] = Value::Null;
                card
            })
            .collect();
        d["cards"] = json!(cards);
    });
}

// ---- a Chat, a board

#[test]
fn a_chat_and_a_board() {
    let (session, card, board) = (
        SessionId::new([0xab; 16]),
        ObjectId::new([0xcd; 16]),
        BoardId::new([3; 16]),
    );
    let (r, newest) = ask(&Route::Chat {
        timeline: Timeline::Session(&session),
        before: None,
        limit: 500,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/chats/session/{}/items?limit=200",
            "ab".repeat(16)
        )
    );
    let (r, older) = ask(&Route::Chat {
        timeline: Timeline::Card(&card),
        before: Some(40),
        limit: 2,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/chats/card/{}/items?before=40&limit=2",
            "cd".repeat(16)
        )
    );
    let page = |items: &[u64], more: bool| {
        let items: Vec<String> = items.iter().map(|c| envelope(*c)).collect();
        format!(r#"{{"items":[{}],"more":{more}}}"#, items.join(","))
    };
    let Answer::Envelopes { items, more } = taken(&older, &page(&[39, 12], true)) else {
        panic!()
    };
    assert_eq!(
        (items.iter().map(|i| i.change).collect::<Vec<_>>(), more),
        (vec![39, 12], true)
    );
    assert!(matches!(
        taken(&newest, &page(&[900, 3], false)),
        Answer::Envelopes { .. }
    ));
    for bad in [
        page(&[12, 39], false),
        page(&[39, 39], false),
        page(&[40], false),
        page(&[41], false),
        page(&[39, 12, 3], false),
        page(&[], true),
    ] {
        refused(&older, &bad);
    }
    refused(&older, r#"{"items":[]}"#);
    refused(&older, &page(&[39], false).replace("AAEC", "AAE="));

    let (r, x) = ask(&Route::Board {
        board: &board,
        after: 7,
        limit: 9000,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/boards/{}?after_change=7&limit=2000",
            b64(&[3; 16])
        )
    );
    assert!(matches!(
        taken(&x, &page(&[8, 30], true)),
        Answer::Envelopes { more: true, .. }
    ));
    for bad in [
        page(&[30, 8], false),
        page(&[7], false),
        page(&[8, 8], false),
        page(&[], true),
    ] {
        refused(&x, &bad);
    }
}

#[test]
fn a_boards_frontier() {
    let board = BoardId::new([3; 16]);
    let frontier = [(DeviceId::new([5; 32]), 12, Hash32::new([6; 32]))];
    let files = [FileId::new([7; 16])];
    let route = Route::BoardFrontier {
        board: &board,
        frontier: &frontier,
        files: &files,
        snapshot: Some((4, Hash32::new([8; 32]))),
    };
    let (r, x) = ask(&route);
    assert_eq!(
        (r.method.as_str(), r.url.clone()),
        (
            "POST",
            format!("https://hub.example/v1/boards/{}/frontier", b64(&[3; 16]))
        )
    );
    assert_eq!(
        body(&r),
        json!({ "frontier": { b64(&[5; 32]): [12, b64(&[6; 32])] }, "files": [b64(&[7; 16])], "snapshot": [4, b64(&[8; 32])] })
    );
    assert_eq!(taken(&x, r#"{"pruned":3}"#), Answer::Pruned { pruned: 3 });
    refused(&x, r#"{"pruned":-1}"#);
    refused(&x, "{}");
    let (r, _) = ask(&Route::BoardFrontier {
        board: &board,
        frontier: &[],
        files: &[],
        snapshot: None,
    });
    assert_eq!(body(&r), json!({ "frontier": {}, "files": [] }));
    let twice = [frontier[0], frontier[0]];
    assert!(Codec
        .request(
            &SITE,
            &Route::BoardFrontier {
                board: &board,
                frontier: &twice,
                files: &[],
                snapshot: None
            }
        )
        .is_err());
}

// ---- the sealed keys, a removal

#[test]
fn sealed_keys() {
    let (r, x) = ask(&Route::SealedKeys {
        after: 10,
        limit: 3,
    });
    assert_eq!(r.url, "https://hub.example/v1/sealed-keys?after=10&limit=3");
    let page = |rows: &[u64], links: &[u64], change: u64, more: bool| {
        let rows: Vec<String> = rows
            .iter()
            .map(|c| format!(r#"{{"change":{c},"sealed_key":"AAEC"}}"#))
            .collect();
        let links: Vec<String> = links
            .iter()
            .map(|c| format!(r#"{{"room_epoch":2,"change":{c},"recovery_link":"AQ"}}"#))
            .collect();
        format!(
            r#"{{"rows":[{}],"links":[{}],"change":{change},"more":{more}}}"#,
            rows.join(","),
            links.join(",")
        )
    };
    let Answer::SealedKeys(got) = taken(&x, &page(&[11, 14], &[12], 14, true)) else {
        panic!()
    };
    assert_eq!(
        (
            got.rows.len(),
            got.rows[1].0,
            got.links[0].change,
            got.links[0].room_epoch,
            got.change,
            got.more
        ),
        (2, 14, 12, 2, 14, true)
    );
    assert!(
        matches!(taken(&x, &page(&[], &[], 10, false)), Answer::SealedKeys(p) if p.change == 10)
    );
    for bad in [
        // out of order, at the cursor, a cursor that ran ahead or stayed behind, more without a step, too many
        page(&[14, 11], &[], 14, false),
        page(&[10], &[], 10, false),
        page(&[11], &[12, 12], 12, false),
        page(&[11], &[], 20, false),
        page(&[11, 14], &[], 11, false),
        page(&[], &[], 10, true),
        page(&[11, 12, 13, 14], &[], 14, false),
        page(&[11], &[], 11, false).replace("AAEC", ""),
        r#"{"rows":[],"change":10,"more":false}"#.to_owned(),
    ] {
        refused(&x, &bad);
    }
}

#[test]
fn a_removal() {
    let group = group();
    let (r, x) = ask(&Route::Removal {
        group: &group,
        after: 2,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/groups/{}/removal?after=2",
            b64(group.as_bytes())
        )
    );
    let commit = |n: u64| {
        format!(
            r#"{{"change":{},"kind":"commit","group_id":"{}","n":{n},"epoch":{},"at":5,"bytes":"AAEC","sender":"{}"}}"#,
            n + 100,
            b64(group.as_bytes()),
            n - 1,
            b64(&[9; 32])
        )
    };
    let page = |items: &[String], more: bool, removed_at: u64| {
        format!(
            r#"{{"items":[{}],"more":{more},"removed_at":{removed_at}}}"#,
            items.join(",")
        )
    };
    let Answer::Removal {
        items,
        more,
        removed_at,
    } = taken(&x, &page(&[commit(3), commit(4)], false, 4))
    else {
        panic!()
    };
    assert_eq!(
        (items.len(), items[1].n, more, removed_at),
        (2, 4, false, 4)
    );
    assert!(matches!(
        taken(&x, &page(&[commit(3)], true, 9)),
        Answer::Removal { more: true, .. }
    ));
    for bad in [
        // a Commit after the removing one, an end before it, a message, another group, no removed_at
        page(&[commit(3), commit(5)], false, 4),
        page(&[commit(3)], false, 4),
        page(
            &[commit(3).replace("commit", "message"), commit(4)],
            false,
            4,
        ),
        page(
            &[
                commit(3).replace(&b64(group.as_bytes()), &b64(&[7; 48])),
                commit(4),
            ],
            false,
            4,
        ),
        page(&[commit(4), commit(3)], false, 4),
        r#"{"items":[],"more":false}"#.to_owned(),
        page(&[], false, 0),
    ] {
        refused(&x, &bad);
    }
}

// ---- signing out, a recovery

#[test]
fn signing_out_and_a_recovery() {
    let (r, x) = ask(&Route::SignOut);
    assert_eq!(
        (r.method.as_str(), r.url.as_str(), r.body.is_none()),
        ("DELETE", "https://hub.example/v1/token", true)
    );
    assert_eq!(taken(&x, "{}"), Answer::Done);
    refused(&x, "[]");
    refused(&x, "");

    let room = RoomId::new([1; 32]);
    let (r, x) = ask(&Route::OpenRecovery { room: &room });
    assert_eq!(
        (r.method.as_str(), r.url),
        (
            "POST",
            format!("https://hub.example/v1/rooms/{}/recovery", b64(&[1; 32]))
        )
    );
    assert_eq!(
        taken(
            &x,
            &format!(r#"{{"recovery_id":"{}","expires_at":77}}"#, b64(&[4; 16]))
        ),
        Answer::Recovery {
            id: [4; 16],
            expires_at: 77
        }
    );
    refused(
        &x,
        &format!(r#"{{"recovery_id":"{}","expires_at":77}}"#, b64(&[4; 15])),
    );
    refused(&x, &format!(r#"{{"recovery_id":"{}"}}"#, b64(&[4; 16])));
    let (r, x) = ask(&Route::DropRecovery {
        room: &room,
        recovery: &[4; 16],
    });
    assert_eq!(
        (r.method.as_str(), r.url),
        (
            "DELETE",
            format!(
                "https://hub.example/v1/rooms/{}/recovery/{}",
                b64(&[1; 32]),
                b64(&[4; 16])
            )
        )
    );
    assert_eq!(
        taken(&x, r#"{"dropped":false}"#),
        Answer::Dropped { dropped: false }
    );
    refused(&x, r#"{"dropped":"yes"}"#);
}

// ---- files and Share links

fn file_answer(status: u16, range: Option<&str>, body: &[u8]) -> HttpResponse {
    HttpResponse {
        status,
        headers: range
            .map(|value| Header {
                name: "content-range".into(),
                value: value.into(),
            })
            .into_iter()
            .collect(),
        body: body.to_vec().into(),
    }
}

#[test]
fn a_file_goes_up() {
    let file = FileId::new([7; 16]);
    let (r, x) = ask(&Route::PutFile {
        file: &file,
        bytes: &[1, 2, 3],
    });
    assert_eq!(
        (r.method.as_str(), r.url),
        (
            "PUT",
            format!("https://hub.example/v1/files/{}", b64(&[7; 16]))
        )
    );
    assert_eq!(r.body.as_ref().map(|b| b.to_vec()), Some(vec![1, 2, 3]));
    assert!(r
        .headers
        .iter()
        .any(|h| h.name == "content-type" && h.value == "application/octet-stream"));
    assert_eq!(
        (r.class, r.key.as_deref(), r.timeout_ms),
        (
            HttpClass::Upload,
            Some(format!("put-file/{}", b64(&[7; 16])).as_str()),
            600_000
        )
    );
    let stored = |id: &[u8], size: u64| {
        format!(
            r#"{{"file_id":"{}","size":{size},"sha256":"{}"}}"#,
            b64(id),
            b64(&[9; 32])
        )
    };
    assert_eq!(
        taken(&x, &stored(&[7; 16], 3)),
        Answer::FileStored {
            size: 3,
            sha256: [9; 32]
        }
    );
    refused(&x, &stored(&[8; 16], 3));
    refused(&x, &stored(&[7; 16], 4));
    refused(
        &x,
        &stored(&[7; 16], 3).replace(&b64(&[9; 32]), &b64(&[9; 31])),
    );
    let big = vec![0; CAP_FILE + 1];
    assert!(Codec
        .request(
            &SITE,
            &Route::PutFile {
                file: &file,
                bytes: &big
            }
        )
        .is_err());
}

#[test]
fn a_file_comes_down_whole_or_in_a_range() {
    let file = FileId::new([7; 16]);
    let (r, whole) = ask(&Route::GetFile {
        file: &file,
        range: None,
    });
    assert_eq!(
        (r.class, r.max_answer as usize),
        (HttpClass::Download, CAP_FILE)
    );
    assert!(r.headers.iter().all(|h| h.name != "range"));
    let got = Codec
        .answer(&whole, &file_answer(200, None, b"hello"))
        .unwrap();
    assert!(matches!(got, Answer::File(f) if f.bytes == b"hello" && f.first == 0 && f.size == 5));
    // a whole file is 200 and nothing else; a body the shell let through beyond the cap is refused here too
    assert!(matches!(
        Codec.answer(&whole, &file_answer(206, Some("bytes 0-4/5"), b"hello")),
        Err(Failure::BadAnswer { .. })
    ));
    assert!(matches!(
        Codec.answer(&whole, &file_answer(204, None, b"")),
        Err(Failure::BadAnswer { .. })
    ));
    let refusal = file_answer(404, None, br#"{"error":"not-found","message":"x"}"#);
    assert_eq!(
        Codec.answer(&whole, &refusal),
        Err(Failure::Refused {
            status: 404,
            code: Some("not-found".into())
        })
    );

    let (r, x) = ask(&Route::GetFile {
        file: &file,
        range: Some(ByteRange {
            first: 10,
            last: Some(14),
        }),
    });
    assert!(r
        .headers
        .iter()
        .any(|h| h.name == "range" && h.value == "bytes=10-14"));
    assert_eq!(r.max_answer, 5);
    let got = Codec
        .answer(&x, &file_answer(206, Some("bytes 10-14/100"), b"hello"))
        .unwrap();
    assert!(
        matches!(got, Answer::File(f) if f.bytes == b"hello" && f.first == 10 && f.size == 100)
    );
    // the file ends inside the range asked for
    let got = Codec
        .answer(&x, &file_answer(206, Some("bytes 10-12/13"), b"hel"))
        .unwrap();
    assert!(matches!(got, Answer::File(f) if f.size == 13));
    for (status, range, bytes) in [
        // 200 for a range, no header, another start, another end, fewer bytes than announced, more, a total of 0
        (200, Some("bytes 10-14/100"), &b"hello"[..]),
        (206, None, b"hello"),
        (206, Some("bytes 11-15/100"), b"hello"),
        (206, Some("bytes 10-13/100"), b"hell"),
        (206, Some("bytes 10-14/100"), b"hell"),
        (206, Some("bytes 10-14/100"), b"hello!"),
        (206, Some("bytes 10-14/0"), b"hello"),
        (206, Some("bytes 10-14/12"), b"hello"),
        (206, Some("bytes 10-14/*"), b"hello"),
        (206, Some("items 10-14/100"), b"hello"),
        (206, Some("bytes 10-14/99999999999999"), b"hello"),
    ] {
        assert!(
            matches!(
                Codec.answer(&x, &file_answer(status, range, bytes)),
                Err(Failure::BadAnswer { .. })
            ),
            "{range:?}"
        );
    }
    let (r, tail) = ask(&Route::GetFile {
        file: &file,
        range: Some(ByteRange {
            first: 10,
            last: None,
        }),
    });
    assert!(r
        .headers
        .iter()
        .any(|h| h.name == "range" && h.value == "bytes=10-"));
    assert!(Codec
        .answer(&tail, &file_answer(206, Some("bytes 10-14/15"), b"hello"))
        .is_ok());
    assert!(Codec
        .answer(&tail, &file_answer(206, Some("bytes 10-13/15"), b"hell"))
        .is_err());
    // a range that is none
    for range in [
        ByteRange {
            first: 5,
            last: Some(4),
        },
        ByteRange {
            first: 0,
            last: Some(CAP_FILE as u64),
        },
    ] {
        assert!(Codec
            .request(
                &SITE,
                &Route::GetFile {
                    file: &file,
                    range: Some(range)
                }
            )
            .is_err());
    }
}

#[test]
fn share_links() {
    let (share, file) = (ShareId::new([4; 16]), FileId::new([7; 16]));
    let (r, x) = ask(&Route::PostShare {
        share: &share,
        secret_hash: &[5; 32],
        file: &file,
        expires_at: 900,
    });
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/shares")
    );
    assert_eq!(
        body(&r),
        json!({ "share_id": b64(&[4; 16]), "secret_hash": b64(&[5; 32]), "file_id": b64(&[7; 16]), "expires_at": 900 })
    );
    assert_eq!(
        taken(
            &x,
            &format!(r#"{{"share_id":"{}","expires_at":900}}"#, b64(&[4; 16]))
        ),
        Answer::Shared { expires_at: 900 }
    );
    refused(
        &x,
        &format!(r#"{{"share_id":"{}","expires_at":900}}"#, b64(&[6; 16])),
    );
    refused(&x, &format!(r#"{{"share_id":"{}"}}"#, b64(&[4; 16])));

    for route in [
        Route::DeleteShare { share: &share },
        Route::DeleteFile { file: &file },
    ] {
        let (r, x) = ask(&route);
        assert!(r.method == "DELETE" && r.body.is_none());
        assert_eq!(taken(&x, r#"{"deleted":true}"#), Answer::Done);
        refused(&x, r#"{"deleted":false}"#);
        refused(&x, "{}");
    }

    // the shared file: no token, the secret in a header and never in the address
    let nobody = Site {
        token: None,
        ..SITE
    };
    let (r, x) = Codec
        .request(
            &nobody,
            &Route::GetShared {
                share: &share,
                secret: &[6; 32],
                range: None,
            },
        )
        .unwrap();
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/shares/{}", b64(&[4; 16]))
    );
    assert!(r
        .headers
        .iter()
        .any(|h| h.name == "x-share-secret" && h.value == b64(&[6; 32])));
    assert!(r.headers.iter().all(|h| h.name != "authorization"));
    assert!(
        !format!("{r:?}").contains(&b64(&[6; 32])),
        "a header's value never shows in a log"
    );
    assert!(
        matches!(Codec.answer(&x, &file_answer(200, None, b"bytes")), Ok(Answer::File(f)) if f.size == 5)
    );
}
