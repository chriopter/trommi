//! The codec's account, invite and push routes: the request each builds, a good answer, and every malformation
//! refused whole. Login keys and sealed copies never show in a URL, a header or `Debug`.

use serde_json::{json, Value};
use trommi_client_core::app::{HttpRequest, HttpResponse};
use trommi_client_core::hub::{
    account_copies_body, new_account_body, AccountCopies, AccountProof, Answer, Codec, CopyOfWayIn,
    Expect, Failure, Kdf, KdfValue, KitForm, KitPart, LiveActivity, NewAccount, NewPassword,
    OutboxExtra, PasskeyAssertion, PasskeyRegistration, PushKind, PushLevel, PushRegistration,
    PushTarget, Route, Site,
};
use trommi_core::ids::{base64url_encode as b64, GroupId, InviteId, RoomId};
use trommi_core::store::{OutboxEntry, OutboxKind};

const SITE: Site<'static> = Site {
    url: "https://hub.example",
    token: Some("abcdefghijklmnop_-0123"),
    client: "web/1.2.3",
    lease: None,
};
const NOBODY: Site<'static> = Site {
    token: None,
    ..SITE
};
const ID: &str = "0f0f0f0f-0f0f-0f0f-0f0f-0f0f0f0f0f0f";

fn ok(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: vec![],
        body: body.as_bytes().to_vec().into(),
    }
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

fn copy(mark: u8) -> [u8; 61] {
    let mut c = [mark; 61];
    c[0] = 2;
    c
}

fn kdf() -> Kdf {
    Kdf(vec![
        ("alg".into(), KdfValue::Text("argon2id".into())),
        ("m".into(), KdfValue::Number(65536)),
    ])
}

fn kit() -> KitPart {
    KitPart {
        auth_key: [1; 32],
        sealed_copy: copy(9),
    }
}

fn password() -> NewPassword {
    NewPassword {
        auth_key: [3; 32],
        sealed_copy: copy(7),
        kdf: kdf(),
    }
}

fn registration() -> PasskeyRegistration {
    PasskeyRegistration {
        attestation_object: vec![4; 10],
        client_data_json: vec![5; 10],
        sealed_copy: copy(6),
        transports: Some(vec!["internal".into(), "hybrid".into()]),
    }
}

fn assertion() -> PasskeyAssertion {
    PasskeyAssertion {
        credential_id: vec![1, 2],
        authenticator_data: vec![3],
        client_data_json: vec![4],
        signature: vec![5],
        user_handle: Some(vec![0x0f; 16]),
    }
}

// ---- logging in

#[test]
fn logging_in() {
    let (r, x) = Codec
        .request(
            &NOBODY,
            &Route::Login {
                account: "ada@example.com",
                auth_key: &[1; 32],
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/account/login")
    );
    assert_eq!(
        body(&r),
        json!({ "account": "ada@example.com", "auth_key": b64(&[1; 32]) })
    );
    assert!(r.headers.iter().all(|h| h.name != "authorization"));
    let (r, _) = Codec
        .request(
            &NOBODY,
            &Route::Recover {
                account: ID,
                auth_key: &[1; 32],
            },
        )
        .unwrap();
    assert_eq!(r.url, "https://hub.example/v1/account/recover");
    for name in ["", &"a".repeat(255)] {
        assert!(Codec
            .request(
                &NOBODY,
                &Route::Login {
                    account: name,
                    auth_key: &[1; 32]
                }
            )
            .is_err());
    }
    let room = |n: u8| {
        format!(
            r#"{{"room_id":"{}","sealed_copy":"{}","challenge":"{}"}}"#,
            b64(&[n; 32]),
            b64(&copy(7)),
            b64(&[8; 32])
        )
    };
    let answer = |rooms: &[String], account: &str| {
        format!(
            r#"{{"rooms":[{}],"kdf":{{"m":65536,"alg":"argon2id"}},"account":"{account}","email":"ada@example.com"}}"#,
            rooms.join(",")
        )
    };
    let Answer::Login(login) = taken(&x, &answer(&[room(1)], ID)) else {
        panic!()
    };
    assert_eq!(
        (
            login.rooms.len(),
            login.rooms[0].room,
            login.rooms[0].sealed_copy,
            login.rooms[0].challenge
        ),
        (1, RoomId::new([1; 32]), copy(7), [8; 32])
    );
    assert_eq!(
        (login.account.as_str(), login.email.as_deref(), login.kdf.clone()),
        (ID, Some("ada@example.com"), Some(kdf()))
    );
    // an account without e-mail and without a password
    assert!(
        matches!(taken(&x, &format!(r#"{{"rooms":[],"kdf":null,"account":"{ID}","email":null}}"#)), Answer::Login(l) if l.kdf.is_none() && l.email.is_none())
    );
    for bad in [
        // a room twice, a copy of another length, a challenge of another length, an id in another spelling
        answer(&[room(1), room(1)], ID),
        answer(&[room(1).replace(&b64(&copy(7)), &b64(&[2; 60]))], ID),
        answer(&[room(1).replace(&b64(&[8; 32]), &b64(&[8; 31]))], ID),
        answer(&[room(1)], &ID.to_uppercase()),
        answer(&[room(1)], &ID.replace('-', "")),
        answer(&[room(1)], "ada"),
        answer(&[room(1)], ID).replace(r#""m":65536"#, r#""m":65536.5"#),
        answer(&[room(1)], ID).replace(r#""m":65536"#, r#""m":[1]"#),
        answer(&(0..17).map(room).collect::<Vec<_>>(), ID),
        format!(r#"{{"kdf":null,"account":"{ID}"}}"#),
    ] {
        refused(&x, &bad);
    }
    // a login's answer never shows in a log
    let shown = format!("{:?} {login:?}", taken(&x, &answer(&[room(1)], ID)));
    assert!(
        !shown.contains(&b64(&copy(7))) && !shown.contains("ada@"),
        "{shown}"
    );
}

#[test]
fn passkey_challenges_and_logins() {
    let (r, open) = Codec.request(&NOBODY, &Route::PasskeyChallenge).unwrap();
    assert_eq!(
        (r.url.as_str(), body(&r)),
        (
            "https://hub.example/v1/account/passkey/challenge",
            json!({})
        )
    );
    let (r, own) = Codec
        .request(&SITE, &Route::AccountPasskeyChallenge)
        .unwrap();
    assert_eq!(r.url, "https://hub.example/v1/account/passkeys/challenge");
    assert!(r.headers.iter().any(|h| h.name == "authorization"));
    let answer = |account: &str, more: &str| {
        format!(
            r#"{{"challenge":"{}","account":"{account}","user_handle":"{}"{more}}}"#,
            b64(&[7; 32]),
            b64(&[0x0f; 16])
        )
    };
    let Answer::PasskeyChallenge(c) = taken(&open, &answer(ID, "")) else {
        panic!()
    };
    assert_eq!(
        (c.challenge, c.account.as_str(), c.user_handle, c.kit_form),
        ([7; 32], ID, [0x0f; 16], None)
    );
    let Answer::PasskeyChallenge(c) = taken(&own, &answer(ID, r#","email":null,"kit_form":"id""#))
    else {
        panic!()
    };
    assert_eq!((c.kit_form, c.email), (Some(KitForm::Id), None));
    // the id is not the text of the handle; this room's account's challenge says what its kit is salted with
    refused(&open, &answer("1f0f0f0f-0f0f-0f0f-0f0f-0f0f0f0f0f0f", ""));
    refused(&own, &answer(ID, ""));
    refused(&own, &answer(ID, r#","kit_form":"phone""#));
    refused(
        &open,
        &answer(ID, "").replace(&b64(&[7; 32]), &b64(&[7; 16])),
    );

    let a = assertion();
    let (r, _) = Codec
        .request(&NOBODY, &Route::PasskeyLogin { assertion: &a })
        .unwrap();
    assert_eq!(r.url, "https://hub.example/v1/account/passkey/login");
    assert_eq!(
        body(&r),
        json!({ "credential_id": "AQI", "authenticator_data": "Aw", "client_data_json": "BA", "signature": "BQ", "user_handle": b64(&[0x0f; 16]) })
    );
    let empty = PasskeyAssertion {
        signature: vec![],
        ..assertion()
    };
    assert!(Codec
        .request(&NOBODY, &Route::PasskeyLogin { assertion: &empty })
        .is_err());
}

// ---- the account

fn view(passkeys: &str, rooms: &str) -> String {
    format!(
        r#"{{"email":"ada@example.com","account":"{ID}","kit_form":"email","revision":4,"has_password":true,"kdf":{{"alg":"argon2id","m":65536}},"password_copy":"{}","kit_copy":"{}","user_handle":"{}","passkeys":[{passkeys}],"rooms":[{rooms}]}}"#,
        b64(&copy(7)),
        b64(&copy(9)),
        b64(&[0x0f; 16])
    )
}

#[test]
fn the_accounts_view_and_usage() {
    let account = NewAccount {
        email: Some("ada@example.com".into()),
        kit: kit(),
        password: Some(password()),
        passkey: Some(registration()),
    };
    let (r, x) = Codec
        .request(&SITE, &Route::CreateAccount { account: &account })
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/account")
    );
    assert_eq!(
        body(&r),
        json!({
            "email": "ada@example.com",
            "kit": { "auth_key": b64(&[1; 32]), "sealed_copy": b64(&copy(9)) },
            "password": { "auth_key": b64(&[3; 32]), "sealed_copy": b64(&copy(7)), "kdf": { "alg": "argon2id", "m": 65536 } },
            "passkey": { "attestation_object": b64(&[4; 10]), "client_data_json": b64(&[5; 10]), "sealed_copy": b64(&copy(6)), "transports": ["internal", "hybrid"] },
        })
    );
    // the same body founds an account with its room
    assert_eq!(
        serde_json::from_slice::<Value>(&new_account_body(&account).unwrap()).unwrap(),
        body(&r)
    );
    // no way in; a secret never in Debug
    let none = NewAccount {
        email: None,
        kit: kit(),
        password: None,
        passkey: None,
    };
    assert!(Codec
        .request(&SITE, &Route::CreateAccount { account: &none })
        .is_err());
    assert_eq!(
        format!(
            "{account:?} {:?}",
            Route::CreateAccount { account: &account }
        ),
        "NewAccount(<redacted>) Route(<redacted>)"
    );

    let key = format!(
        r#"{{"credential_id":"AQI","sealed_copy":"{}","transports":["usb"],"created_at":5,"last_used_at":null}}"#,
        b64(&copy(6))
    );
    let room = format!("\"{}\"", b64(&[1; 32]));
    let Answer::Account(got) = taken(&x, &view(&key, &room)) else {
        panic!()
    };
    assert_eq!(
        (
            got.account.as_str(),
            got.kit_form,
            got.revision,
            got.has_password
        ),
        (ID, KitForm::Email, 4, true)
    );
    assert_eq!(
        (
            got.password_copy,
            got.kit_copy,
            got.user_handle,
            got.rooms.clone()
        ),
        (
            Some(copy(7)),
            copy(9),
            [0x0f; 16],
            vec![RoomId::new([1; 32])]
        )
    );
    assert_eq!(
        (
            got.passkeys.len(),
            got.passkeys[0].credential_id.clone(),
            got.passkeys[0].last_used_at
        ),
        (1, vec![1, 2], None)
    );
    assert_eq!(format!("{got:?}"), "AccountView(<redacted>)");
    for bad in [
        view(&format!("{key},{key}"), &room),
        view(&key, &format!("{room},{room}")),
        view(&key.replace(&b64(&copy(6)), "AQ"), &room),
        view(&key, &room).replace(&b64(&[0x0f; 16]), &b64(&[0x0e; 16])),
        view(&key, &room).replace(r#""kit_form":"email""#, r#""kit_form":"words""#),
        view(&key, &room).replace(r#""revision":4"#, r#""revision":"4""#),
        view(&key, &room).replace(&b64(&copy(9)), &b64(&[2; 62])),
        view(&key, "\"AAEC\""),
    ] {
        refused(&x, &bad);
    }
    let (r, _) = Codec.request(&SITE, &Route::Account).unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("GET", "https://hub.example/v1/account")
    );

    let (r, x) = Codec.request(&SITE, &Route::AccountUsage).unwrap();
    assert_eq!(r.url, "https://hub.example/v1/account/usage");
    let names = [
        "rooms",
        "groups",
        "live_groups",
        "devices",
        "commits",
        "messages",
        "envelopes",
        "cards",
        "open_cards",
        "files",
        "file_bytes",
        "log_bytes",
        "envelope_bytes",
        "total_bytes",
    ];
    let usage: serde_json::Map<String, Value> = names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.to_string(), json!(i)))
        .collect();
    let Answer::Usage(got) = taken(&x, &Value::Object(usage.clone()).to_string()) else {
        panic!()
    };
    assert_eq!((got.rooms, got.live_groups, got.total_bytes), (0, 2, 13));
    let mut short = usage;
    short.remove("files");
    refused(&x, &Value::Object(short).to_string());
}

#[test]
fn changing_a_way_in_carries_its_proof() {
    let by_password = AccountProof::Password { auth_key: [8; 32] };
    let by_passkey = AccountProof::Passkey(assertion());
    let (r, x) = Codec
        .request(
            &SITE,
            &Route::PutPassword {
                password: &password(),
                revision: 4,
                proof: &by_password,
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("PUT", "https://hub.example/v1/account/password")
    );
    assert_eq!(
        body(&r),
        json!({ "auth_key": b64(&[3; 32]), "sealed_copy": b64(&copy(7)), "kdf": { "alg": "argon2id", "m": 65536 }, "revision": 4, "proof": { "password": { "auth_key": b64(&[8; 32]) } } })
    );
    assert_eq!(
        taken(&x, r#"{"revision":5}"#),
        Answer::Revision { revision: 5 }
    );
    refused(&x, r#"{"revision":"5"}"#);
    refused(&x, "{}");

    let (r, _) = Codec
        .request(
            &SITE,
            &Route::PutKit {
                kit: &kit(),
                revision: 4,
                proof: &AccountProof::Kit { auth_key: [8; 32] },
            },
        )
        .unwrap();
    assert_eq!(r.url, "https://hub.example/v1/account/kit");
    assert_eq!(
        body(&r)["proof"],
        json!({ "kit": { "auth_key": b64(&[8; 32]) } })
    );
    let (r, _) = Codec
        .request(
            &SITE,
            &Route::PutEmail {
                email: "ada@example.com",
                kit: &kit(),
                revision: 4,
                proof: &by_passkey,
            },
        )
        .unwrap();
    assert_eq!(r.url, "https://hub.example/v1/account/email");
    // (a proof by passkey names no user handle)
    assert_eq!(
        body(&r),
        json!({
            "email": "ada@example.com",
            "kit": { "auth_key": b64(&[1; 32]), "sealed_copy": b64(&copy(9)) },
            "revision": 4,
            "proof": { "passkey": { "credential_id": "AQI", "authenticator_data": "Aw", "client_data_json": "BA", "signature": "BQ" } },
        })
    );
    assert!(Codec
        .request(
            &SITE,
            &Route::PutEmail {
                email: &"a".repeat(255),
                kit: &kit(),
                revision: 4,
                proof: &by_passkey
            }
        )
        .is_err());

    let (r, x) = Codec
        .request(
            &SITE,
            &Route::AddPasskey {
                passkey: &registration(),
                proof: &by_password,
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/account/passkeys")
    );
    assert_eq!(
        body(&r)["proof"],
        json!({ "password": { "auth_key": b64(&[8; 32]) } })
    );
    assert_eq!(
        taken(&x, r#"{"credential_id":"AQI","created_at":9}"#),
        Answer::PasskeyAdded {
            credential_id: vec![1, 2],
            created_at: 9
        }
    );
    refused(&x, r#"{"credential_id":"","created_at":9}"#);
    let odd = PasskeyRegistration {
        transports: Some(vec!["USB port".into()]),
        ..registration()
    };
    assert!(Codec
        .request(
            &SITE,
            &Route::AddPasskey {
                passkey: &odd,
                proof: &by_password
            }
        )
        .is_err());

    for (route, url) in [
        (
            Route::RemovePasskey {
                credential_id: &[1, 2],
                proof: &by_password,
            },
            "https://hub.example/v1/account/passkeys/AQI",
        ),
        (
            Route::DeleteAccount {
                proof: &by_password,
            },
            "https://hub.example/v1/account",
        ),
    ] {
        let (r, x) = Codec.request(&SITE, &route).unwrap();
        assert_eq!((r.method.as_str(), r.url.as_str()), ("DELETE", url));
        assert!(
            body(&r).to_string().contains(&b64(&[8; 32])),
            "the proof goes in the body"
        );
        assert_eq!(taken(&x, r#"{"deleted":true}"#), Answer::Done);
        refused(&x, r#"{"deleted":false}"#);
    }
    assert_eq!(
        body(
            &Codec
                .request(
                    &SITE,
                    &Route::DeleteAccount {
                        proof: &by_password
                    }
                )
                .unwrap()
                .0
        ),
        json!({ "password": { "auth_key": b64(&[8; 32]) } })
    );
    assert!(Codec
        .request(
            &SITE,
            &Route::RemovePasskey {
                credential_id: &[],
                proof: &by_password
            }
        )
        .is_err());
    // a login key is in no URL and no header
    for route in [
        Route::PutPassword {
            password: &password(),
            revision: 4,
            proof: &by_password,
        },
        Route::DeleteAccount {
            proof: &by_password,
        },
    ] {
        let (r, _) = Codec.request(&SITE, &route).unwrap();
        assert!(
            !r.url.contains(&b64(&[8; 32]))
                && r.headers.iter().all(|h| !h.value.contains(&b64(&[8; 32])))
        );
    }
}

#[test]
fn the_accounts_copies_travel_with_the_outbox() {
    let room = RoomId::new([1; 32]);
    let entry = |copies: Vec<u8>| OutboxEntry {
        id: 1,
        kind: OutboxKind::RecoveryCode,
        group: Some(GroupId::room(room)),
        epoch: 4,
        parts: vec![vec![1], vec![2], vec![3], vec![4], copies],
    };
    let ways = [
        None,
        Some(CopyOfWayIn::Password {
            sealed_copy: copy(7),
        }),
        Some(CopyOfWayIn::NewPassword(NewPassword {
            kdf: Kdf(vec![
                ("alg".into(), KdfValue::Text("argon2id".into())),
                ("v".into(), KdfValue::Number(1)),
            ]),
            ..password()
        })),
        Some(CopyOfWayIn::Passkey {
            credential_id: vec![1, 2],
            sealed_copy: copy(6),
        }),
        Some(CopyOfWayIn::NewPasskey(registration())),
    ];
    for way_in in ways {
        let copies = AccountCopies {
            kit: kit(),
            revision: Some(3),
            way_in,
        };
        let bytes = account_copies_body(Some(&copies)).unwrap();
        // what the builder writes is what the outbox takes, member for member
        let e = entry(bytes.clone());
        let (r, _) = Codec
            .request(
                &SITE,
                &Route::Outbox {
                    room: &room,
                    entry: &e,
                    extra: None,
                },
            )
            .unwrap();
        assert_eq!(
            body(&r)["account"],
            serde_json::from_slice::<Value>(&bytes).unwrap()
        );
        assert_eq!(body(&r)["account"]["revision"], 3);
    }
    assert_eq!(account_copies_body(None).unwrap(), Vec::<u8>::new());

    // a room founded with its account
    let account = NewAccount {
        email: None,
        kit: kit(),
        password: None,
        passkey: Some(registration()),
    };
    let extra = OutboxExtra {
        account: Some(new_account_body(&account).unwrap()),
        found_token: None,
        recovery_id: None,
    };
    let founding = OutboxEntry {
        id: 1,
        kind: OutboxKind::RoomFounding,
        group: None,
        epoch: 0,
        parts: vec![vec![1], vec![2]],
    };
    let (r, _) = Codec
        .request(
            &NOBODY,
            &Route::Outbox {
                room: &room,
                entry: &founding,
                extra: Some(&extra),
            },
        )
        .unwrap();
    assert_eq!(body(&r)["account"]["passkey"]["sealed_copy"], b64(&copy(6)));
    assert_eq!(format!("{extra:?}"), "OutboxExtra(<redacted>)");
}

// ---- invites

#[test]
fn invites() {
    let invite = InviteId::new([4; 16]);
    let at = format!("https://hub.example/v1/invites/{}", b64(&[4; 16]));
    // without a token the Offer; with one also the Requests
    let (r, x) = Codec
        .request(&NOBODY, &Route::GetInvite { invite: &invite })
        .unwrap();
    assert!(r.url == at && r.headers.iter().all(|h| h.name != "authorization"));
    let (r, _) = Codec
        .request(&SITE, &Route::GetInvite { invite: &invite })
        .unwrap();
    assert!(r.headers.iter().any(|h| h.name == "authorization"));
    let request = r#"{"request":"AQ","mac":"Ag","signature":"Aw"}"#;
    let offer = |mac: &str, requests: &str| {
        format!(
            r#"{{"offer":"AAEC","signature":"BA","mac":{mac},"expires_at":99,"requests":{requests}}}"#
        )
    };
    let Answer::Invite(got) = taken(&x, &offer("\"BQ\"", &format!("[{request}]"))) else {
        panic!()
    };
    assert_eq!(
        (got.offer, got.signature, got.mac, got.expires_at),
        (vec![0, 1, 2], vec![4], Some(vec![5]), 99)
    );
    assert_eq!(
        got.requests.map(|r| (r.len(), r[0].request.clone())),
        Some((1, vec![1]))
    );
    assert!(
        matches!(taken(&x, &offer("null", "null")), Answer::Invite(o) if o.mac.is_none() && o.requests.is_none())
    );
    for bad in [
        offer("\"\"", "null"),
        offer(
            "\"BQ\"",
            &format!("[{request},{request},{request},{request},{request}]"),
        ),
        offer("\"BQ\"", r#"[{"request":"AQ","mac":"Ag"}]"#),
        offer("\"BQ\"", "null").replace("AAEC", &b64(&[0; 1025])),
        offer("\"BQ\"", "null").replace(r#""expires_at":99"#, r#""expires_at":-1"#),
        r#"{"signature":"BA","expires_at":99}"#.to_owned(),
    ] {
        refused(&x, &bad);
    }

    let (r, x) = Codec
        .request(
            &NOBODY,
            &Route::PostInviteRequest {
                invite: &invite,
                request: &[1],
                mac: &[2],
                signature: &[3],
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.clone()),
        ("POST", format!("{at}/request"))
    );
    assert_eq!(
        body(&r),
        json!({ "request": "AQ", "mac": "Ag", "signature": "Aw" })
    );
    assert_eq!(
        taken(&x, &format!(r#"{{"request_hash":"{}"}}"#, b64(&[6; 32]))),
        Answer::Requested {
            request_hash: [6; 32]
        }
    );
    refused(&x, &format!(r#"{{"request_hash":"{}"}}"#, b64(&[6; 31])));
    assert!(Codec
        .request(
            &NOBODY,
            &Route::PostInviteRequest {
                invite: &invite,
                request: &[],
                mac: &[2],
                signature: &[3]
            }
        )
        .is_err());

    let (r, x) = Codec
        .request(&NOBODY, &Route::GetReveal { invite: &invite })
        .unwrap();
    assert_eq!(r.url, format!("{at}/reveal"));
    assert_eq!(
        taken(&x, r#"{"reveal":"AQ","signature":"Ag"}"#),
        Answer::Reveal {
            reveal: vec![1],
            signature: vec![2]
        }
    );
    refused(&x, r#"{"reveal":"AQ"}"#);

    let (r, x) = Codec
        .request(
            &SITE,
            &Route::PostInvite {
                offer: &[1],
                signature: &[2],
                mac: &[3],
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/invites")
    );
    assert_eq!(
        body(&r),
        json!({ "offer": "AQ", "signature": "Ag", "mac": "Aw" })
    );
    assert_eq!(
        taken(&x, &format!(r#"{{"invite_id":"{}"}}"#, b64(&[4; 16]))),
        Answer::Invited { invite }
    );
    refused(&x, r#"{"invite_id":"AAEC"}"#);
    assert!(
        Codec
            .request(
                &NOBODY,
                &Route::PostInvite {
                    offer: &[1],
                    signature: &[2],
                    mac: &[3]
                }
            )
            .is_err(),
        "a human device's"
    );

    let (r, x) = Codec
        .request(
            &SITE,
            &Route::PutReveal {
                invite: &invite,
                reveal: &[1],
                signature: &[2],
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.clone(), body(&r)),
        (
            "PUT",
            format!("{at}/reveal"),
            json!({ "reveal": "AQ", "signature": "Ag" })
        )
    );
    assert_eq!(taken(&x, r#"{"revealed":true}"#), Answer::Done);
    refused(&x, r#"{"burned":true}"#);
    let (r, x) = Codec
        .request(&SITE, &Route::DeleteInvite { invite: &invite })
        .unwrap();
    assert_eq!((r.method.as_str(), r.url.as_str()), ("DELETE", at.as_str()));
    assert_eq!(taken(&x, r#"{"burned":true}"#), Answer::Done);
    refused(&x, r#"{"burned":false}"#);
}

// ---- push

#[test]
fn push() {
    let web = PushRegistration {
        target: PushTarget::WebPush {
            endpoint: "https://push.example/abc".into(),
            p256dh: vec![4; 65],
            auth: vec![1; 16],
        },
        level: PushLevel::All,
    };
    let (r, x) = Codec
        .request(&SITE, &Route::PushRegister { registration: &web })
        .unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("POST", "https://hub.example/v1/push")
    );
    assert_eq!(
        body(&r),
        json!({ "level": "all", "web_push": { "endpoint": "https://push.example/abc", "keys": { "p256dh": b64(&[4; 65]), "auth": b64(&[1; 16]) } } })
    );
    assert_eq!(taken(&x, r#"{"registered":true}"#), Answer::Done);
    refused(&x, "{}");
    let apns = PushRegistration {
        target: PushTarget::Apns {
            token: "ab12".into(),
            key: vec![7; 32],
            environment: "production".into(),
            topic: "com.trommi.app".into(),
        },
        level: PushLevel::Knocking,
    };
    let (r, _) = Codec
        .request(
            &SITE,
            &Route::PushRegister {
                registration: &apns,
            },
        )
        .unwrap();
    assert_eq!(
        body(&r),
        json!({ "level": "knocking", "apns": { "token": "ab12", "key": b64(&[7; 32]), "environment": "production", "topic": "com.trommi.app" } })
    );
    assert_eq!(format!("{web:?}"), "PushRegistration(<redacted>)");
    let spaced = PushRegistration {
        target: PushTarget::WebPush {
            endpoint: "https://push.example/a b".into(),
            p256dh: vec![4; 65],
            auth: vec![1; 16],
        },
        level: PushLevel::All,
    };
    assert!(Codec
        .request(
            &SITE,
            &Route::PushRegister {
                registration: &spaced
            }
        )
        .is_err());

    let (r, x) = Codec.request(&SITE, &Route::PushState).unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("GET", "https://hub.example/v1/push")
    );
    let row = |endpoint: &str| {
        format!(
            r#"{{"kind":"web_push","endpoint":"{endpoint}","level":"knocking","created_at":5}}"#
        )
    };
    let state = |rows: &[String], key: &[u8]| {
        format!(
            r#"{{"subscriptions":[{}],"vapid_public_key":"{}","apns":true}}"#,
            rows.join(","),
            b64(key)
        )
    };
    let Answer::Push(got) = taken(
        &x,
        &state(
            &[
                row("https://a.example/1"),
                row("https://a.example/2").replace("web_push", "apns"),
            ],
            &[4; 65],
        ),
    ) else {
        panic!()
    };
    assert_eq!(
        (
            got.subscriptions.len(),
            got.subscriptions[1].kind,
            got.subscriptions[0].level,
            got.apns
        ),
        (2, PushKind::Apns, PushLevel::Knocking, true)
    );
    for bad in [
        state(
            &[row("https://a.example/1"), row("https://a.example/1")],
            &[4; 65],
        ),
        state(
            &[row("https://a.example/1").replace("web_push", "fax")],
            &[4; 65],
        ),
        state(
            &[row("https://a.example/1").replace("knocking", "loud")],
            &[4; 65],
        ),
        state(&[], &[4; 64]),
        state(&[row(&"x".repeat(2049))], &[4; 65]),
        r#"{"subscriptions":[],"apns":true}"#.to_owned(),
    ] {
        refused(&x, &bad);
    }

    let (r, x) = Codec
        .request(
            &SITE,
            &Route::PushRemove {
                endpoint: Some("https://push.example/abc"),
            },
        )
        .unwrap();
    assert_eq!(
        (r.method.as_str(), body(&r)),
        ("DELETE", json!({ "endpoint": "https://push.example/abc" }))
    );
    assert_eq!(
        taken(&x, r#"{"deleted":2}"#),
        Answer::Removed { deleted: 2 }
    );
    refused(&x, r#"{"deleted":true}"#);
    assert_eq!(
        body(
            &Codec
                .request(&SITE, &Route::PushRemove { endpoint: None })
                .unwrap()
                .0
        ),
        json!({})
    );

    let activity = LiveActivity {
        start: false,
        token: "ab".into(),
        tag: "desk".into(),
        environment: "production".into(),
        topic: "com.trommi.app".into(),
    };
    let (r, x) = Codec
        .request(
            &SITE,
            &Route::LiveActivity {
                activity: &activity,
            },
        )
        .unwrap();
    assert_eq!(r.url, "https://hub.example/v1/live-activity");
    assert_eq!(
        body(&r),
        json!({ "kind": "activity", "token": "ab", "tag": "desk", "environment": "production", "topic": "com.trommi.app" })
    );
    assert_eq!(taken(&x, r#"{"registered":true}"#), Answer::Done);
    let untagged = LiveActivity {
        tag: String::new(),
        ..activity
    };
    assert!(Codec
        .request(
            &SITE,
            &Route::LiveActivity {
                activity: &untagged
            }
        )
        .is_err());
}
