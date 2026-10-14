//! Whatever a hub answers, the codec answers with a value or a failure and never panics: every recorded answer of
//! the real hub, whole, cut short and with bytes changed, is read as the answer to every kind of route and as a
//! stream. (Seeded, so a failure repeats; the libFuzzer target of tests/fuzz can take the same entry points.)

use trommi_client_core::app::{Header, HttpResponse};
use trommi_client_core::hub::{
    AccountProof, ByteRange, Codec, DeskOnly, Expect, ObjectRoute, Only, Route, Site, StreamReader,
    Timeline, WishKind,
};
use trommi_core::ids::{
    BoardId, DeviceId, FileId, GroupId, InviteId, ObjectId, RoomId, SessionId, ShareId,
};
use trommi_core::store::{OutboxEntry, OutboxKind};

const SITE: Site<'static> = Site {
    url: "https://hub.example",
    token: Some("abcdefghijklmnop_-0123"),
    client: "web/1.2.3",
    lease: None,
};

/// One `Expect` of every kind the codec reads an answer by.
fn expects() -> Vec<Expect> {
    let room = RoomId::new([1; 32]);
    let group = GroupId::session(room, SessionId::new([2; 16]));
    let (device, object, file, share) = (
        DeviceId::new([3; 32]),
        ObjectId::new([4; 16]),
        FileId::new([5; 16]),
        ShareId::new([6; 16]),
    );
    let (session, board, invite) = (
        SessionId::new([2; 16]),
        BoardId::new([7; 16]),
        InviteId::new([8; 16]),
    );
    let proof = AccountProof::Kit { auth_key: [1; 32] };
    let entry = |kind: OutboxKind, parts: usize| OutboxEntry {
        id: 1,
        kind,
        group: Some(group),
        epoch: 1,
        parts: vec![vec![1]; parts],
    };
    let kinds = [
        (OutboxKind::GroupFounding, 6),
        (OutboxKind::Commit, 4),
        (OutboxKind::Message, 1),
        (OutboxKind::RelayMessage, 1),
        (OutboxKind::Envelope, 1),
        (OutboxKind::KeyPackages, 2),
        (OutboxKind::SealedKey, 1),
    ];
    let entries: Vec<OutboxEntry> = kinds
        .iter()
        .map(|(kind, parts)| entry(*kind, *parts))
        .collect();
    let joins = [entry(OutboxKind::ExternalCommit, 4)];
    let range = Some(ByteRange {
        first: 0,
        last: Some(9),
    });
    let mut routes = vec![
        Route::Changes {
            after: 0,
            limit: 200,
            only: Only::All,
        },
        Route::Changes {
            after: 0,
            limit: 200,
            only: Only::Group(&group),
        },
        Route::DeskBrief {
            since: None,
            active: true,
        },
        Route::DeskBrief {
            since: Some(1),
            active: true,
        },
        Route::Desk {
            only: DeskOnly::All,
        },
        Route::KeyPackagesHeld,
        Route::Welcomes { after: 0 },
        Route::GroupLog {
            group: &group,
            after: 0,
            limit: 200,
            commits_only: false,
        },
        Route::GroupInfo {
            group: &group,
            epoch: None,
        },
        Route::Joins { entries: &joins },
        Route::GroupsRead {
            groups: std::slice::from_ref(&group),
        },
        Route::ClaimKeyPackages {
            devices: std::slice::from_ref(&device),
            partial: true,
        },
        Route::RoomGroups {
            room: &room,
            after: 0,
            limit: 100,
        },
        Route::Chain {
            group: &group,
            sender: &device,
            after: 0,
            limit: 100,
        },
        Route::Object {
            route: ObjectRoute::Cards,
            object: &object,
            after: 0,
            limit: 100,
        },
        Route::Wish {
            kind: WishKind::Session,
            group: None,
            key_package: None,
        },
        Route::Wishes,
        Route::RejectCommit {
            group: &group,
            n: 1,
        },
        Route::SignOut,
        Route::Chat {
            timeline: Timeline::Session(&session),
            before: None,
            limit: 50,
        },
        Route::Board {
            board: &board,
            after: 0,
            limit: 50,
        },
        Route::BoardFrontier {
            board: &board,
            frontier: &[],
            files: &[],
            snapshot: None,
        },
        Route::SealedKeys {
            after: 0,
            limit: 100,
        },
        Route::Removal {
            group: &group,
            after: 0,
        },
        Route::OpenRecovery { room: &room },
        Route::DropRecovery {
            room: &room,
            recovery: &[9; 16],
        },
        Route::PutFile {
            file: &file,
            bytes: &[1, 2, 3],
        },
        Route::GetFile {
            file: &file,
            range: None,
        },
        Route::GetFile { file: &file, range },
        Route::DeleteFile { file: &file },
        Route::PostShare {
            share: &share,
            secret_hash: &[1; 32],
            file: &file,
            expires_at: 9,
        },
        Route::GetInvite { invite: &invite },
        Route::PostInviteRequest {
            invite: &invite,
            request: &[1],
            mac: &[2],
            signature: &[3],
        },
        Route::GetReveal { invite: &invite },
        Route::PostInvite {
            offer: &[1],
            signature: &[2],
            mac: &[3],
        },
        Route::PutReveal {
            invite: &invite,
            reveal: &[1],
            signature: &[2],
        },
        Route::Account,
        Route::AccountUsage,
        Route::AccountPasskeyChallenge,
        Route::DeleteAccount { proof: &proof },
        Route::PushState,
        Route::PushRemove { endpoint: None },
    ];
    for entry in &entries {
        routes.push(Route::Outbox {
            room: &room,
            entry,
            extra: None,
        });
    }
    let mut out: Vec<Expect> = routes
        .iter()
        .map(|r| Codec.request(&SITE, r).expect("the route builds").1)
        .collect();
    let nobody = Site {
        token: None,
        ..SITE
    };
    for route in [
        Route::Challenge { room: &room },
        Route::Token {
            room: &room,
            auth: &[1],
            signature: &[2],
        },
        Route::Login {
            account: "a@b.example",
            auth_key: &[1; 32],
        },
        Route::PasskeyChallenge,
    ] {
        out.push(Codec.request(&nobody, &route).unwrap().1);
    }
    out
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn no_answer_makes_the_codec_panic() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/hub");
    let mut answers: Vec<Vec<u8>> = std::fs::read_dir(dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
            v["answer"].as_str().unwrap().as_bytes().to_vec()
        })
        .collect();
    answers.sort();
    answers.extend([
        b"".to_vec(),
        b"null".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        vec![b'['; 5000],
        vec![0xff; 64],
    ]);
    let expects = expects();
    let mut rng = Rng(0x7a6d_6d69);
    let (mut read, mut taken) = (0u32, 0u32);
    for answer in &answers {
        let mut forms = vec![answer.clone()];
        for _ in 0..6 {
            let mut bent = answer.clone();
            if bent.is_empty() {
                continue;
            }
            match rng.next() % 4 {
                0 => bent.truncate(rng.next() as usize % bent.len()),
                1 => {
                    let at = rng.next() as usize % bent.len();
                    bent[at] = rng.next() as u8;
                }
                2 => {
                    // a digit becomes another, a quote goes
                    let at = rng.next() as usize % bent.len();
                    if let Some(i) = bent[at..]
                        .iter()
                        .position(|b| b.is_ascii_digit() || *b == b'"')
                    {
                        bent[at + i] = if bent[at + i] == b'"' {
                            b' '
                        } else {
                            b'0' + (rng.next() % 10) as u8
                        };
                    }
                }
                _ => {
                    let at = rng.next() as usize % bent.len();
                    let piece = bent[at..(at + 40).min(bent.len())].to_vec();
                    bent.splice(at..at, piece);
                }
            }
            forms.push(bent);
        }
        for form in &forms {
            for status in [200, 206, 404, 429, 500] {
                let response = HttpResponse {
                    status,
                    headers: vec![Header {
                        name: "content-range".into(),
                        value: "bytes 0-9/10".into(),
                    }],
                    body: form.clone().into(),
                };
                for expect in &expects {
                    read += 1;
                    taken += Codec.answer(expect, &response).is_ok() as u32;
                }
                let _ = Codec.refusal(&response);
            }
            let mut reader = StreamReader::new(0);
            let half = form.len() / 2;
            let _ = reader.chunk(&form[..half]);
            let _ = reader.chunk(&form[half..]);
            let _ = reader.ended();
        }
    }
    // (most answers are no answer to most routes: the count only shows that both ways were walked)
    assert!(
        read > 100_000 && taken > 100 && taken < read / 10,
        "{taken} of {read}"
    );
}
