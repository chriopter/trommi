//! The codec's sign-in, delivery-service and content routes: the request each builds, a good answer, and every
//! malformation refused whole.

use trommi_client_core::app::{HttpRequest, HttpResponse};
use trommi_client_core::hub::limits::{MAX_KEY_PACKAGE, MAX_MLS};
use trommi_client_core::hub::{
    Answer, ChangeItem, Codec, Expect, Failure, GroupRowKind, ObjectRoute, Role, Route, Site,
    WishKind,
};
use trommi_core::ids::{base64url_encode as b64, DeviceId, GroupId, ObjectId, RoomId, SessionId};
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

fn taken(expect: &Expect, body: &str) -> Answer {
    Codec
        .answer(expect, &ok(body))
        .unwrap_or_else(|e| panic!("not taken ({e:?}): {}", &body[..body.len().min(300)]))
}

fn room() -> RoomId {
    RoomId::new([1; 32])
}

fn group() -> GroupId {
    GroupId::session(room(), SessionId::new([2; 16]))
}

fn g() -> String {
    b64(group().as_bytes())
}

fn device(n: u8) -> DeviceId {
    DeviceId::new([n; 32])
}

fn commit(n: u64, epoch: u64, change: u64) -> String {
    format!(
        r#"{{"change":{change},"kind":"commit","group_id":"{}","n":{n},"epoch":{epoch},"at":5,"bytes":"AAEC","sender":"{}"}}"#,
        g(),
        b64(&[9; 32])
    )
}

// ---- signing in

#[test]
fn the_challenge_and_the_token() {
    let room = room();
    let nobody = Site {
        token: None,
        ..SITE
    };
    let (r, x) = Codec
        .request(&nobody, &Route::Challenge { room: &room })
        .unwrap();
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/rooms/{}/challenge", b64(&[1; 32]))
    );
    assert!(r.method == "GET" && r.headers.iter().all(|h| h.name != "authorization"));
    assert_eq!(
        taken(&x, &format!(r#"{{"challenge":"{}"}}"#, b64(&[7; 32]))),
        Answer::Challenge([7; 32])
    );
    for bad in [
        format!(r#"{{"challenge":"{}"}}"#, b64(&[7; 31])),
        format!(r#"{{"challenge":"{}"}}"#, b64(&[7; 33])),
        r#"{"challenge":7}"#.into(),
        "{}".into(),
    ] {
        refused(&x, &bad);
    }

    let (r, x) = Codec
        .request(
            &nobody,
            &Route::Token {
                room: &room,
                auth: &[1, 2],
                signature: &[3],
            },
        )
        .unwrap();
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/rooms/{}/tokens", b64(&[1; 32]))
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "auth": "AQI", "signature": "Aw" })
    );
    assert!(r.headers.iter().all(|h| h.name != "authorization"));
    let token = "0123456789abcdef-_";
    assert_eq!(
        taken(
            &x,
            &format!(r#"{{"token":"{token}","expires_at":99,"role":"recovery"}}"#)
        ),
        Answer::Token {
            token: token.into(),
            expires_at: 99,
            role: Role::Recovery
        }
    );
    for bad in [
        r#"{"token":"short","expires_at":99,"role":"human"}"#.to_owned(),
        r#"{"token":"0123456789abcdef\r\nx: y","expires_at":99,"role":"human"}"#.to_owned(),
        format!(
            r#"{{"token":"{}","expires_at":99,"role":"human"}}"#,
            "a".repeat(201)
        ),
        format!(r#"{{"token":"{token}","expires_at":"soon","role":"human"}}"#),
        format!(r#"{{"token":"{token}","expires_at":99,"role":"admin"}}"#),
        format!(r#"{{"token":"{token}","expires_at":99}}"#),
    ] {
        refused(&x, &bad);
    }
    assert!(Codec
        .request(
            &nobody,
            &Route::Token {
                room: &room,
                auth: &[],
                signature: &[3]
            }
        )
        .is_err());
    assert!(Codec
        .request(
            &nobody,
            &Route::Token {
                room: &room,
                auth: &[1],
                signature: &[3; 129]
            }
        )
        .is_err());
    // a token answer never shows in a log
    let answer = taken(
        &x,
        &format!(r#"{{"token":"{token}","expires_at":99,"role":"human"}}"#),
    );
    assert!(!format!("{answer:?}").contains(token));
}

// ---- Welcomes

#[test]
fn welcomes() {
    let (r, x) = ask(&Route::Welcomes { after: 4 });
    assert_eq!(r.url, "https://hub.example/v1/welcomes?after=4");
    let row = |id: u64| {
        format!(
            r#"{{"id":{id},"group_id":"{}","welcome":"AAEC","at":8}}"#,
            g()
        )
    };
    let Answer::Welcomes(rows) = taken(&x, &format!("[{},{}]", row(5), row(9))) else {
        panic!()
    };
    assert_eq!(rows.iter().map(|r| r.id).collect::<Vec<_>>(), [5, 9]);
    assert_eq!(
        (rows[0].group, &rows[0].welcome[..], rows[0].at),
        (group(), &[0u8, 1, 2][..], 8)
    );
    assert_eq!(taken(&x, "[]"), Answer::Welcomes(vec![]));
    for bad in [
        format!("[{},{}]", row(9), row(5)),
        format!("[{},{}]", row(5), row(5)),
        format!("[{}]", row(4)),
        format!("[{}]", row(5).replace(&g(), "AAEC")),
        format!("[{}]", row(5).replace("AAEC", &b64(&vec![0; MAX_MLS + 1]))),
        format!("[{}]", row(5).replace(r#""at":8"#, r#""at":-8"#)),
        r#"{"items":[]}"#.to_owned(),
        "[7]".to_owned(),
    ] {
        refused(&x, &bad);
    }
}

// ---- a group's log

#[test]
fn group_log() {
    let group = group();
    let (r, x) = ask(&Route::GroupLog {
        group: &group,
        after: 3,
        limit: 5000,
        commits_only: true,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/groups/{}/log?after=3&limit=1000&kind=commit",
            g()
        )
    );
    let (r, any) = ask(&Route::GroupLog {
        group: &group,
        after: 3,
        limit: 2,
        commits_only: false,
    });
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/groups/{}/log?after=3&limit=2", g())
    );
    let page = |items: &[String], more: bool| {
        format!(r#"{{"items":[{}],"more":{more}}}"#, items.join(","))
    };
    let message = commit(5, 3, 21).replace("commit", "message");
    let Answer::GroupLog { items, more } =
        taken(&any, &page(&[commit(4, 3, 20), message.clone()], true))
    else {
        panic!()
    };
    assert!(
        more && matches!(&items[..], [ChangeItem::Commit(c), ChangeItem::Message(m)] if c.n == 4 && m.n == 5 && c.epoch == 3)
    );
    for bad in [
        // numbers not rising, at the cursor, change numbers going back, another group's entry
        page(&[commit(5, 3, 20), commit(4, 4, 21)], false),
        page(&[commit(3, 3, 20)], false),
        page(&[commit(4, 3, 21), commit(5, 4, 20)], false),
        page(&[commit(4, 3, 20).replace(&g(), &b64(&[8; 48]))], false),
        // more than asked for, more without a step, types
        page(
            &[commit(4, 3, 20), commit(5, 4, 21), commit(6, 5, 22)],
            false,
        ),
        page(&[], true),
        r#"{"items":[]}"#.to_owned(),
        r#"{"items":[[]],"more":false}"#.to_owned(),
    ] {
        refused(&any, &bad);
    }
    // only Commits were asked for
    refused(&x, &page(&[message], false));
    assert!(matches!(
        taken(&x, &page(&[], false)),
        Answer::GroupLog { more: false, .. }
    ));
}

// ---- a GroupInfo

#[test]
fn group_info() {
    let group = group();
    let (r, current) = ask(&Route::GroupInfo {
        group: &group,
        epoch: None,
    });
    assert_eq!(r.url, format!("https://hub.example/v1/groups/{}/info", g()));
    assert_eq!(r.max_answer, 2 << 20);
    let (r, founding) = ask(&Route::GroupInfo {
        group: &group,
        epoch: Some(0),
    });
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/groups/{}/info?epoch=0", g())
    );
    assert_eq!(
        taken(&current, r#"{"epoch":6,"group_info":"AAEC"}"#),
        Answer::GroupInfo {
            epoch: 6,
            group_info: vec![0, 1, 2]
        }
    );
    assert!(matches!(
        taken(&founding, r#"{"epoch":0,"group_info":"AAEC"}"#),
        Answer::GroupInfo { epoch: 0, .. }
    ));
    refused(&founding, r#"{"epoch":6,"group_info":"AAEC"}"#);
    for bad in [
        r#"{"epoch":6}"#,
        r#"{"epoch":6,"group_info":""}"#,
        r#"{"epoch":"6","group_info":"AAEC"}"#,
        r#"{"group_info":"AAEC"}"#,
    ] {
        refused(&current, bad);
    }
    let big = format!(
        r#"{{"epoch":6,"group_info":"{}"}}"#,
        b64(&vec![0; MAX_MLS + 1])
    );
    refused(&current, &big);
}

// ---- several joins in one request

fn join(group: GroupId, epoch: u64) -> OutboxEntry {
    OutboxEntry {
        id: 1,
        kind: OutboxKind::ExternalCommit,
        group: Some(group),
        epoch,
        parts: vec![vec![1], vec![2], vec![3], vec![4]],
    }
}

#[test]
fn joins() {
    let other = GroupId::session(room(), SessionId::new([3; 16]));
    let entries = [join(group(), 7), join(other, 2)];
    let (r, x) = ask(&Route::Joins { entries: &entries });
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/commits")
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "commits": [
            { "group": g(), "epoch": 7, "commit": "AQ", "group_info": "Ag", "sealed_key": "Aw", "recovery_auth": "BA" },
            { "group": b64(other.as_bytes()), "epoch": 2, "commit": "AQ", "group_info": "Ag", "sealed_key": "Aw", "recovery_auth": "BA" },
        ] })
    );
    let refusal = r#"{"error":"epoch-taken","message":"x","status":409,"epoch":4}"#;
    let Answer::Joins(answers) = taken(
        &x,
        &format!(r#"{{"answers":[{{"epoch":8,"change":30}},{refusal}]}}"#),
    ) else {
        panic!()
    };
    assert!(matches!(&answers[0], Ok(a) if a.epoch == Some(8) && a.change == Some(30)));
    assert_eq!(
        answers[1],
        Err(Failure::Refused {
            status: 409,
            code: Some("epoch-taken".into())
        })
    );
    let busy = r#"{"error":"overloaded","message":"x","status":503,"retry_after":2}"#;
    let Answer::Joins(answers) = taken(&x, &format!(r#"{{"answers":[{busy},{busy}]}}"#)) else {
        panic!()
    };
    assert_eq!(
        answers[0],
        Err(Failure::NotNow {
            status: 503,
            retry_after_ms: Some(2000)
        })
    );
    for bad in [
        // fewer answers, more answers, an epoch that is not the next, a refusal that is none of this protocol
        r#"{"answers":[{"epoch":8,"change":30}]}"#.to_owned(),
        r#"{"answers":[{"epoch":8,"change":30},{"epoch":3,"change":31},{"epoch":3,"change":32}]}"#.to_owned(),
        r#"{"answers":[{"epoch":8,"change":30},{"epoch":4,"change":31}]}"#.to_owned(),
        r#"{"answers":[{"epoch":8,"change":30},{"error":"made-up","message":"x","status":409}]}"#.to_owned(),
        r#"{"answers":[{"epoch":8,"change":30},{"error":"epoch-taken","message":"x","status":403}]}"#.to_owned(),
        r#"{"answers":[{"epoch":8,"change":30},{"error":"epoch-taken","message":"x"}]}"#.to_owned(),
        r#"{"answers":[{"epoch":8,"change":30},{"error":"epoch-taken","message":"x","status":200}]}"#.to_owned(),
        r#"{"answers":{}}"#.to_owned(),
    ] {
        refused(&x, &bad);
    }
    // the request's own form: none, more than 64, a group twice, an entry that is no join
    assert!(Codec
        .request(&SITE, &Route::Joins { entries: &[] })
        .is_err());
    let many: Vec<OutboxEntry> = (0..65u8)
        .map(|i| join(GroupId::session(room(), SessionId::new([i; 16])), 1))
        .collect();
    assert!(Codec
        .request(&SITE, &Route::Joins { entries: &many })
        .is_err());
    assert!(Codec
        .request(
            &SITE,
            &Route::Joins {
                entries: &many[..64]
            }
        )
        .is_ok());
    assert!(Codec
        .request(
            &SITE,
            &Route::Joins {
                entries: &[join(group(), 7), join(group(), 7)]
            }
        )
        .is_err());
    let mut commit = join(group(), 7);
    commit.kind = OutboxKind::Commit;
    assert!(Codec
        .request(&SITE, &Route::Joins { entries: &[commit] })
        .is_err());
    let mut no_auth = join(group(), 7);
    no_auth.parts[3].clear();
    assert!(Codec
        .request(
            &SITE,
            &Route::Joins {
                entries: &[no_auth]
            }
        )
        .is_err());
}

// ---- several groups read in one request

#[test]
fn groups_read() {
    let other = GroupId::session(room(), SessionId::new([3; 16]));
    let groups = [group(), other];
    let (r, x) = ask(&Route::GroupsRead { groups: &groups });
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/groups/read")
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "groups": [g(), b64(other.as_bytes())] })
    );
    assert_eq!(r.max_answer, 12 << 20);
    let o = b64(other.as_bytes());
    let read = |group: &str, epoch: u64, founding: &str, log: &[String], more: bool| {
        format!(
            r#"{{"group":"{group}","info":{{"epoch":{epoch},"group_info":"AAEC"}},"founding":{founding},"log":{{"items":[{}],"more":{more}}}}}"#,
            log.join(",")
        )
    };
    let f0 = r#"{"epoch":0,"group_info":"AQ"}"#;
    let young = read(&o, 0, "null", &[], false);
    let two = [commit(1, 0, 10), commit(2, 1, 11)];
    let answer = |rows: &[String]| format!(r#"{{"groups":[{}]}}"#, rows.join(","));
    let Answer::GroupsRead(rows) = taken(
        &x,
        &answer(&[read(&g(), 2, f0, &two, false), young.clone()]),
    ) else {
        panic!()
    };
    let first = rows[0].as_ref().unwrap();
    assert_eq!(
        (
            first.group,
            first.epoch,
            first.founding.as_deref(),
            first.log.len(),
            first.complete
        ),
        (group(), 2, Some(&[1u8][..]), 2, true)
    );
    let second = rows[1].as_ref().unwrap();
    assert_eq!(
        (
            second.epoch,
            &second.founding,
            second.log.len(),
            second.complete
        ),
        (0, &None, 0, true)
    );
    // the Commits did not fit the first page: not complete, read alone
    let Answer::GroupsRead(rows) =
        taken(&x, &answer(&[read(&g(), 9, f0, &two, true), young.clone()]))
    else {
        panic!()
    };
    assert!(!rows[0].as_ref().unwrap().complete);
    // a refusal in a group's place
    let gone = format!(r#"{{"group":"{o}","error":"not-found","message":"x","status":404}}"#);
    let Answer::GroupsRead(rows) = taken(&x, &answer(&[read(&g(), 2, f0, &two, false), gone]))
    else {
        panic!()
    };
    assert_eq!(
        rows[1],
        Err(Failure::Refused {
            status: 404,
            code: Some("not-found".into())
        })
    );
    // the answer was full: that group, and every one after it, is read alone
    let full = |group: &str| {
        format!(
            r#"{{"group":"{group}","error":"too-large","message":"the answer is full: read this group alone","status":413}}"#
        )
    };
    let Answer::GroupsRead(rows) = taken(&x, &answer(&[read(&g(), 2, f0, &two, false), full(&o)]))
    else {
        panic!()
    };
    assert_eq!(
        rows[1],
        Err(Failure::Refused {
            status: 413,
            code: Some("too-large".into())
        })
    );
    for bad in [
        // one group, three groups, the groups in another order, a group twice
        answer(&[read(&g(), 2, f0, &two, false)]),
        answer(&[read(&g(), 2, f0, &two, false), young.clone(), young.clone()]),
        answer(&[young.clone(), read(&g(), 2, f0, &two, false)]),
        answer(&[
            read(&g(), 2, f0, &two, false),
            read(&g(), 2, f0, &two, false),
        ]),
        // no founding beside a later epoch, a founding beside epoch 0, a founding of another epoch
        answer(&[read(&g(), 2, "null", &two, false), young.clone()]),
        answer(&[read(&g(), 2, f0, &two, false), read(&o, 0, f0, &[], false)]),
        answer(&[
            read(&g(), 2, r#"{"epoch":1,"group_info":"AQ"}"#, &two, false),
            young.clone(),
        ]),
        // a log that skips an epoch, runs past the current one, does not reach it and says no more
        answer(&[
            read(&g(), 2, f0, &[commit(1, 0, 10), commit(2, 2, 11)], false),
            young.clone(),
        ]),
        answer(&[read(&g(), 1, f0, &two, false), young.clone()]),
        answer(&[read(&g(), 3, f0, &two, false), young.clone()]),
        // a log of another group, a message in it, entries out of order
        answer(&[
            read(
                &g(),
                2,
                f0,
                &[commit(1, 0, 10).replace(&g(), &o), commit(2, 1, 11)],
                false,
            ),
            young.clone(),
        ]),
        answer(&[
            read(
                &g(),
                2,
                f0,
                &[
                    commit(1, 0, 10),
                    commit(2, 1, 11).replace("commit", "message"),
                ],
                false,
            ),
            young.clone(),
        ]),
        answer(&[
            read(&g(), 2, f0, &[commit(2, 0, 10), commit(1, 1, 11)], false),
            young.clone(),
        ]),
        // a refusal that is none
        answer(&[
            read(&g(), 2, f0, &two, false),
            format!(r#"{{"group":"{o}","error":"nope","message":"x","status":404}}"#),
        ]),
        r#"{"groups":7}"#.to_owned(),
    ] {
        refused(&x, &bad);
    }
    // more Commits than one page holds
    let long: Vec<String> = (0..257).map(|i| commit(i + 1, i, 10 + i)).collect();
    refused(&x, &answer(&[read(&g(), 300, f0, &long, true), young]));
    // the request's own form
    assert!(Codec
        .request(&SITE, &Route::GroupsRead { groups: &[] })
        .is_err());
    assert!(Codec
        .request(
            &SITE,
            &Route::GroupsRead {
                groups: &[group(), group()]
            }
        )
        .is_err());
    let many: Vec<GroupId> = (0..65u8)
        .map(|i| GroupId::session(room(), SessionId::new([i; 16])))
        .collect();
    assert!(Codec
        .request(&SITE, &Route::GroupsRead { groups: &many })
        .is_err());
}

// ---- KeyPackages claimed

#[test]
fn claim_key_packages() {
    let devices = [device(1), device(2), device(3)];
    let (r, all) = ask(&Route::ClaimKeyPackages {
        devices: &devices,
        partial: false,
    });
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/key-packages/claim")
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "devices": [b64(&[1; 32]), b64(&[2; 32]), b64(&[3; 32])] })
    );
    let (r, partial) = ask(&Route::ClaimKeyPackages {
        devices: &devices,
        partial: true,
    });
    assert_eq!(body(&r)["partial"], true);
    let (d1, d2, d3, d4) = (b64(&[1; 32]), b64(&[2; 32]), b64(&[3; 32]), b64(&[4; 32]));
    // in the order asked, whatever order the hub wrote
    let Answer::Claimed {
        key_packages,
        missing,
    } = taken(
        &all,
        &format!(r#"{{"key_packages":{{"{d3}":"Aw","{d1}":"AQ","{d2}":"Ag"}}}}"#),
    )
    else {
        panic!()
    };
    assert_eq!(
        key_packages,
        vec![
            (device(1), vec![1]),
            (device(2), vec![2]),
            (device(3), vec![3])
        ]
    );
    assert!(missing.is_empty());
    let Answer::Claimed {
        key_packages,
        missing,
    } = taken(
        &partial,
        &format!(r#"{{"key_packages":{{"{d2}":"Ag"}},"missing":["{d3}","{d1}"]}}"#),
    )
    else {
        panic!()
    };
    assert_eq!(
        (key_packages, missing),
        (vec![(device(2), vec![2])], vec![device(3), device(1)])
    );
    for bad in [
        // one missing, one of a device not asked for, one too many, `missing` on a claim that is not partial
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag"}}}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag","{d4}":"Aw"}}}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag","{d3}":"Aw","{d4}":"Aw"}}}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag"}},"missing":["{d3}"]}}"#),
        // a KeyPackage that is empty, too large, no byte string
        format!(r#"{{"key_packages":{{"{d1}":"","{d2}":"Ag","{d3}":"Aw"}}}}"#),
        format!(
            r#"{{"key_packages":{{"{d1}":"{}","{d2}":"Ag","{d3}":"Aw"}}}}"#,
            b64(&vec![0; MAX_KEY_PACKAGE + 1])
        ),
        format!(r#"{{"key_packages":{{"{d1}":7,"{d2}":"Ag","{d3}":"Aw"}}}}"#),
        r#"{"key_packages":[]}"#.to_owned(),
    ] {
        refused(&all, &bad);
    }
    for bad in [
        // a device both given and missing, missing twice, missing one that was not asked for, no `missing`
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag"}},"missing":["{d2}"]}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ"}},"missing":["{d2}","{d2}"]}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag"}},"missing":["{d4}"]}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag","{d3}":"Aw"}}}}"#),
        format!(r#"{{"key_packages":{{"{d1}":"AQ","{d2}":"Ag"}},"missing":["AAEC"]}}"#),
    ] {
        refused(&partial, &bad);
    }
    assert!(Codec
        .request(
            &SITE,
            &Route::ClaimKeyPackages {
                devices: &[],
                partial: false
            }
        )
        .is_err());
    assert!(Codec
        .request(
            &SITE,
            &Route::ClaimKeyPackages {
                devices: &[device(1), device(1)],
                partial: false
            }
        )
        .is_err());
}

// ---- the room's groups

#[test]
fn room_groups() {
    let room = room();
    let (r, x) = ask(&Route::RoomGroups {
        room: &room,
        after: 2,
        limit: 3,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/rooms/{}/groups?after=2&limit=3",
            b64(&[1; 32])
        )
    );
    let leaves = format!(r#"["{}","{}"]"#, b64(&[5; 32]), b64(&[6; 32]));
    let row = |group: &[u8], kind: &str, parent: &str, leaves: &str| {
        let session = if group.len() == 48 {
            format!("\"{}\"", b64(&group[32..]))
        } else {
            "null".to_owned()
        };
        format!(
            r#"{{"group_id":"{}","kind":"{kind}","session_id":{session},"parent":{parent},"epoch":4,"room_epoch":9,"live":true,"stale":false,"leaves":{leaves}}}"#,
            b64(group)
        )
    };
    let helper_group = GroupId::session(room, SessionId::new([3; 16]));
    let parent = format!("\"{}\"", b64(&[2; 16]));
    let zero = format!("\"{}\"", b64(&[0; 16]));
    let page = |rows: &[String], more: bool, after: u64| {
        format!(
            r#"{{"items":[{}],"more":{more},"after":{after}}}"#,
            rows.join(",")
        )
    };
    let rows = [
        row(&[1; 32], "room", "null", &leaves),
        row(group().as_bytes(), "main", &zero, &leaves),
        row(helper_group.as_bytes(), "helper", &parent, "[]"),
    ];
    let Answer::RoomGroups { items, more, after } = taken(&x, &page(&rows, true, 5)) else {
        panic!()
    };
    assert_eq!((items.len(), more, after), (3, true, 5));
    assert_eq!(
        (
            items[0].kind,
            items[0].group.is_room(),
            items[0].leaves.len()
        ),
        (GroupRowKind::Room, true, 2)
    );
    assert_eq!(
        (
            items[1].kind,
            items[1].parent,
            items[1].epoch,
            items[1].room_epoch
        ),
        (GroupRowKind::Main, None, 4, 9)
    );
    assert_eq!(
        (items[2].kind, items[2].parent),
        (GroupRowKind::Helper, Some(SessionId::new([2; 16])))
    );
    for bad in [
        // a group twice, more than asked for, a cursor that goes back or does not move with more
        page(&[rows[1].clone(), rows[1].clone()], false, 5),
        page(
            &[
                rows[0].clone(),
                rows[1].clone(),
                rows[2].clone(),
                row(&[1; 48], "main", "null", "[]"),
            ],
            false,
            5,
        ),
        page(&rows, false, 1),
        page(&rows, true, 2),
        // a group of another room, a kind that is not what the id says, a session id that is not the group's
        page(&[row(&[8; 32], "room", "null", "[]")], false, 5),
        page(&[row(&[1; 32], "main", "null", "[]")], false, 5),
        page(&[row(group().as_bytes(), "room", "null", "[]")], false, 5),
        page(&[rows[1].replace(&b64(&[2; 16]), &b64(&[4; 16]))], false, 5),
        // a helper without a parent, a main session with one, a leaf twice, a leaf of the wrong length
        page(
            &[row(helper_group.as_bytes(), "helper", "null", "[]")],
            false,
            5,
        ),
        page(&[row(group().as_bytes(), "main", &parent, "[]")], false, 5),
        page(
            &[row(
                &[1; 32],
                "room",
                "null",
                &format!(r#"["{0}","{0}"]"#, b64(&[5; 32])),
            )],
            false,
            5,
        ),
        page(&[row(&[1; 32], "room", "null", r#"["AAEC"]"#)], false, 5),
        page(
            &[rows[0].replace(r#""live":true"#, r#""live":1"#)],
            false,
            5,
        ),
        // the bare list of a hub before pages
        format!("[{}]", rows[0]),
    ] {
        refused(&x, &bad);
    }
}

// ---- a chain, an object

fn envelope(change: u64, seq: Option<u64>) -> String {
    let seq = seq.map_or(String::new(), |s| format!(r#","seq":{s}"#));
    format!(r#"{{"change":{change},"envelope":"AAEC","received_at":5{seq}}}"#)
}

#[test]
fn chain() {
    let (group, sender) = (group(), device(9));
    let (r, x) = ask(&Route::Chain {
        group: &group,
        sender: &sender,
        after: 2,
        limit: 3,
    });
    assert_eq!(
        r.url,
        format!(
            "https://hub.example/v1/groups/{}/chains/{}?after=2&limit=3",
            g(),
            b64(&[9; 32])
        )
    );
    let page = |items: &[String], more: bool| {
        format!(r#"{{"items":[{}],"more":{more}}}"#, items.join(","))
    };
    let cut = envelope(31, Some(4)).replace(
        r#""received_at":5"#,
        r#""received_at":5,"cut":true,"void_code":"gap""#,
    );
    let Answer::Chain { items, more } = taken(&x, &page(&[envelope(30, Some(3)), cut], true))
    else {
        panic!()
    };
    assert_eq!(
        (
            items[0].seq,
            items[0].item.cut,
            items[1].seq,
            items[1].item.cut,
            more
        ),
        (3, false, 4, true, true)
    );
    assert_eq!(items[1].item.void_code.as_deref(), Some("gap"));
    for bad in [
        page(&[envelope(30, Some(4)), envelope(31, Some(3))], false),
        page(&[envelope(30, Some(3)), envelope(31, Some(3))], false),
        page(&[envelope(30, Some(2))], false),
        page(&[envelope(30, None)], false),
        page(&[envelope(30, Some(0))], false),
        page(
            &[
                envelope(30, Some(3)),
                envelope(31, Some(4)),
                envelope(32, Some(5)),
                envelope(33, Some(6)),
            ],
            false,
        ),
        page(&[], true),
        page(&[envelope(30, Some(3)).replace("AAEC", "AAEC==")], false),
    ] {
        refused(&x, &bad);
    }
}

#[test]
fn an_objects_envelopes() {
    let object = ObjectId::new([4; 16]);
    for (route, word) in [
        (ObjectRoute::Cards, "cards"),
        (ObjectRoute::Notes, "notes"),
        (ObjectRoute::PermissionRequests, "permission-requests"),
        (ObjectRoute::Artifacts, "artifacts"),
    ] {
        let (r, _) = ask(&Route::Object {
            route,
            object: &object,
            after: 0,
            limit: 9000,
        });
        assert_eq!(
            r.url,
            format!(
                "https://hub.example/v1/{word}/{}?after=0&limit=2000",
                b64(&[4; 16])
            )
        );
    }
    let (_, x) = ask(&Route::Object {
        route: ObjectRoute::Cards,
        object: &object,
        after: 7,
        limit: 2,
    });
    let page = |id: &[u8], items: &[String], more: bool| {
        format!(
            r#"{{"object_id":"{}","group_id":"{}","state":1,"urgency":0,"owner":"{}","first_change":8,"head_change":9,"items":[{}],"more":{more}}}"#,
            b64(id),
            g(),
            b64(&[9; 32]),
            items.join(",")
        )
    };
    let Answer::Object(got) = taken(
        &x,
        &page(&[4; 16], &[envelope(8, None), envelope(9, None)], false),
    ) else {
        panic!()
    };
    assert_eq!(
        (got.object, got.group, got.items.len(), got.more),
        (object, group(), 2, false)
    );
    for bad in [
        // another object, out of order, at the cursor, too many, more without a step
        page(&[5; 16], &[envelope(8, None)], false),
        page(&[4; 16], &[envelope(9, None), envelope(8, None)], false),
        page(&[4; 16], &[envelope(7, None)], false),
        page(
            &[4; 16],
            &[envelope(8, None), envelope(9, None), envelope(10, None)],
            false,
        ),
        page(&[4; 16], &[], true),
        page(&[4; 16], &[], false).replace(&g(), "AAEC"),
    ] {
        refused(&x, &bad);
    }
}

// ---- wishes

#[test]
fn wishes_and_a_rejected_commit() {
    let group = group();
    let (r, x) = ask(&Route::Wish {
        kind: WishKind::Session,
        group: Some(&group),
        key_package: Some(&[1, 2]),
    });
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/requests")
    );
    assert_eq!(
        body(&r),
        serde_json::json!({ "kind": "session", "group": g(), "key_package": "AQI" })
    );
    assert_eq!(taken(&x, r#"{"id":12}"#), Answer::Wished { id: 12 });
    refused(&x, r#"{"id":"12"}"#);
    refused(&x, "{}");
    let (r, _) = ask(&Route::Wish {
        kind: WishKind::Readmit,
        group: None,
        key_package: None,
    });
    assert_eq!(body(&r), serde_json::json!({ "kind": "readmit" }));
    assert!(Codec
        .request(
            &SITE,
            &Route::Wish {
                kind: WishKind::Handover,
                group: None,
                key_package: Some(&[])
            }
        )
        .is_err());

    let (r, x) = ask(&Route::RejectCommit {
        group: &group,
        n: 6,
    });
    assert_eq!(
        r.url,
        format!("https://hub.example/v1/groups/{}/reject", g())
    );
    assert_eq!(body(&r), serde_json::json!({ "n": 6 }));
    assert_eq!(taken(&x, r#"{"id":13}"#), Answer::Done);
    refused(&x, r#"{"ok":true}"#);
    assert!(Codec
        .request(
            &SITE,
            &Route::RejectCommit {
                group: &group,
                n: 0
            }
        )
        .is_err());

    let (r, x) = ask(&Route::Wishes);
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("GET", "https://hub.example/v1/requests")
    );
    let row = |id: u64, kind: &str| {
        format!(
            r#"{{"id":{id},"device":"{}","kind":"{kind}","group_id":"{}","at":5}}"#,
            b64(&[9; 32]),
            g()
        )
    };
    let reject = row(3, "reject").replace(
        r#""at":5"#,
        &format!(r#""at":5,"n":6,"committer":"{}""#, b64(&[8; 32])),
    );
    let Answer::Wishes(rows) = taken(&x, &format!("[{},{}]", row(2, "readmit"), reject)) else {
        panic!()
    };
    assert_eq!(
        (rows[0].id, rows[0].group, rows[0].at, rows[0].n),
        (2, Some(group), Some(5), None)
    );
    assert_eq!((rows[1].n, rows[1].committer), (Some(6), Some(device(8))));
    for bad in [
        format!("[{},{}]", row(2, "readmit"), row(2, "session")),
        format!("[{}]", row(2, "other")),
        format!("[{}]", row(2, "readmit").replace(r#","at":5"#, "")),
        format!(
            "[{}]",
            row(2, "readmit").replace(&b64(&[9; 32]), &b64(&[9; 33]))
        ),
        r#"{"requests":[]}"#.to_owned(),
    ] {
        refused(&x, &bad);
    }
}
