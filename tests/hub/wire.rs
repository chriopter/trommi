//! The hub and the app core read the wire the same way (core/wire): the hub's refusals are read by the client
//! codec as the hub meant them, every request the client codec recorded (`core/client/tests/hub/*.json`) is a
//! route of the hub and every refusal in them stands under its code's status, and the hub's parsers
//! (`trommi_wire::layout`) read the specification's vectors as the protocol core does.

mod common;

use std::path::Path;

use common::TestHub;
use serde_json::Value;
use trommi_client_core::app::{Header, HttpResponse};
use trommi_client_core::hub::{classify, refusal, Failure};
use trommi_hub::error::refuse;
use trommi_wire::codes::{status_of, CODES};
use trommi_wire::layout::{self as wire, HeaderError};

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn workspace() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// The answer the hub sends for a refusal (http.rs: its status, `retry-after` when it names a wait, its body),
/// as the client codec is handed it.
fn hub_answer(refused: &trommi_hub::error::Refused) -> HttpResponse {
    HttpResponse {
        status: refused.status(),
        headers: refused
            .retry_after
            .map(|s| Header {
                name: "retry-after".into(),
                value: s.to_string(),
            })
            .into_iter()
            .collect(),
        body: refused.body().to_string().into_bytes().into(),
    }
}

#[test]
fn every_refusal_the_hub_answers_the_client_reads_as_the_hubs_own() {
    let mut n = 0;
    for (status, codes) in CODES {
        for code in *codes {
            let refused = match *code {
                "rate-limited" | "overloaded" => refuse(code, "x").retry(3),
                _ => refuse(code, "x"),
            };
            assert_eq!(refused.status(), *status, "{code}");
            let answer = hub_answer(&refused);
            let read =
                refusal(&answer).unwrap_or_else(|| panic!("{code}: the client does not read it"));
            assert_eq!((read.code, read.status), (*code, *status));
            match classify(&answer) {
                Err(Failure::Refused { code: Some(c), .. }) => assert_eq!(c, *code),
                Err(Failure::NotNow { retry_after_ms, .. }) => {
                    assert!(matches!(*status, 408 | 429) || *status >= 500, "{code}");
                    if refused.retry_after.is_some() {
                        assert_eq!(retry_after_ms, Some(3000), "{code}");
                    }
                }
                other => panic!("{code}: {other:?}"),
            }
            n += 1;
        }
    }
    assert_eq!(n, CODES.iter().map(|(_, c)| c.len()).sum::<usize>());
    // a code outside the table is the hub's own fault, 500, and no refusal the client counts
    let odd = refuse("made-up", "x");
    assert_eq!(odd.status(), 500);
    assert_eq!(refusal(&hub_answer(&odd)), None);
}

#[test]
fn every_recorded_exchange_is_a_route_of_the_hub_under_the_one_table() {
    let dir = workspace().join("core/client/tests/hub");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|f| f.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    assert!(files.len() >= 80, "{} recorded exchanges", files.len());
    let hub = TestHub::start_with(&[
        ("HUB_LIMIT_OPEN_PER_IP_MINUTE", "100000000"),
        ("HUB_LIMIT_FOUND_PER_IP_HOUR", "100000000"),
        ("HUB_QUIET", "1"),
    ]);
    let (mut refusals, mut routes) = (0, 0);
    for path in &files {
        let v = json_file(path);
        let name = path.file_stem().unwrap().to_string_lossy();
        // what the real hub answered: a refusal stands under its code's status in the one table
        let status = v["status"].as_u64().unwrap() as u16;
        let answer = v["answer"]
            .as_str()
            .and_then(|a| serde_json::from_str::<Value>(a).ok());
        if let Some(code) = answer.as_ref().and_then(|a| a["error"].as_str()) {
            assert!(status >= 400, "{name}");
            assert_eq!(status_of(code), Some(status), "{name}: {code}");
            refusals += 1;
        }
        // the request as the codec built it, sent again without a token to a hub of its own: whatever it
        // answers, it knows the route
        let request = &v["request"];
        let (method, target) = (
            request["method"].as_str().unwrap(),
            request["path"].as_str().unwrap(),
        );
        let body = request["body"].as_str().unwrap_or("").as_bytes();
        let reply = common::request(hub.port, method, target, &[], body);
        let said = reply.json();
        assert!(
            !(reply.status == 404 && said["message"] == "no such route"),
            "{name}: {method} {target} is no route of the hub"
        );
        if reply.status >= 400 {
            if let Some(code) = said["error"].as_str() {
                assert_eq!(status_of(code), Some(reply.status), "{name}: {code}");
            }
        }
        routes += 1;
    }
    assert!(refusals >= 10, "{refusals} recorded refusals");
    assert_eq!(routes, files.len());
}

#[test]
fn the_hub_reads_the_envelope_vectors_as_the_protocol_core_does() {
    let v = json_file(&workspace().join("spec/vectors/envelope.json"));
    let envelopes = v["envelopes"].as_array().unwrap();
    assert!(envelopes.len() >= 10);
    for e in envelopes {
        let what = e["what"].as_str().unwrap();
        let hash = unhex(e["envelope_hash"].as_str().unwrap());
        for form in ["full", "pruned"] {
            let bytes = unhex(e[form].as_str().unwrap());
            let hub =
                wire::Envelope::parse(&bytes).unwrap_or_else(|x| panic!("{what} {form}: {x:?}"));
            let core = trommi_core::envelope::Envelope::decode(&bytes).unwrap();
            assert_eq!(hub.hash().to_vec(), hash, "{what} {form}");
            assert_eq!(
                core.hash().unwrap().as_bytes().to_vec(),
                hash,
                "{what} {form}"
            );
            assert_eq!(
                hub.header_bytes,
                unhex(e["header"].as_str().unwrap()),
                "{what}"
            );
            assert_eq!(hub.header.kind, core.header.subject.kind(), "{what}");
            assert_eq!(hub.header.seq, core.header.seq, "{what}");
            assert_eq!(&hub.header.sender, core.header.sender.as_bytes(), "{what}");
            assert_eq!(hub.header.group_id, core.header.group.as_bytes(), "{what}");
            assert_eq!(hub.header.push(), core.header.push, "{what}");
            assert_eq!(
                hub.header.file_ids.len(),
                core.header.file_ids.len(),
                "{what}"
            );
            assert_eq!(hub.body.is_none(), core.is_pruned(), "{what} {form}");
            let parts = wire::encode_envelope(
                &hub.header_bytes,
                &hub.nonce,
                hub.body.as_deref(),
                &hub.body_hash,
                &hub.signature,
            );
            assert_eq!(parts, bytes, "{what} {form}");
            assert_eq!(core.encode().unwrap(), bytes, "{what} {form}");
            // the id of an object, from its first version
            if let (Some(object), Some(fields)) = (
                match &hub.header.subject {
                    wire::Subject::Object { object_id, .. } => Some(object_id),
                    _ => None,
                },
                core.header.subject.object(),
            ) {
                assert_eq!(object, fields.object_id.as_bytes(), "{what}");
                if e["kind"] == "2" && e["seq"] == "1" {
                    let id =
                        wire::object_id(&hub.header.group_id, &hub.header.sender, hub.header.seq);
                    let core_id = trommi_core::envelope::object_id(
                        &core.header.group,
                        &core.header.sender,
                        core.header.seq,
                    )
                    .unwrap();
                    assert_eq!(&id, core_id.as_bytes(), "{what}");
                }
            }
        }
    }
    // what the protocol core refuses to decode, the hub refuses too, with the same code
    let mut refused = 0;
    for r in v["refused"].as_array().unwrap() {
        if r["at"] != "decode" {
            continue;
        }
        let bytes = unhex(r["bytes"].as_str().unwrap());
        let why = r["why"].as_str().unwrap();
        let hub = wire::Envelope::parse(&bytes);
        let core = trommi_core::envelope::Envelope::decode(&bytes);
        assert!(core.is_err(), "{why}");
        match (r["code"].as_str().unwrap(), hub) {
            ("newer-version", Err(HeaderError::NewerVersion)) => {}
            ("bad-format", Err(HeaderError::Malformed(_))) => {}
            (code, other) => panic!("{why}: the protocol says {code}, the hub {other:?}"),
        }
        refused += 1;
    }
    assert!(refused >= 8, "{refused}");
}

#[test]
fn the_hub_reads_sign_in_invite_and_recovery_vectors_as_written() {
    let auth = json_file(&workspace().join("spec/vectors/hub_auth.json"));
    let bytes = unhex(auth["auth"].as_str().unwrap());
    let parsed = wire::HubAuth::parse(&bytes).unwrap();
    assert_eq!(parsed.encode(), bytes);
    assert_eq!(parsed.hub, auth["hub"].as_str().unwrap().as_bytes());
    assert_eq!(
        parsed.challenge.to_vec(),
        unhex(auth["challenge"].as_str().unwrap())
    );

    let invites = json_file(&workspace().join("spec/vectors/invite.json"));
    let mut n = 0;
    for i in invites["invites"].as_array().unwrap() {
        let offer = unhex(i["offer"].as_str().unwrap());
        let parsed = wire::Offer::parse(&offer).unwrap();
        assert_eq!(parsed.encode(), offer);
        assert_eq!(
            parsed.invite_id.to_vec(),
            unhex(i["invite_id"].as_str().unwrap())
        );
        assert_eq!(
            wire::ref_hash(trommi_wire::invite::LABEL_OFFER_HASH, &offer).to_vec(),
            unhex(i["offer_hash"].as_str().unwrap())
        );
        let request = unhex(i["request"].as_str().unwrap());
        let parsed = wire::InviteRequest::parse(&request).unwrap();
        assert_eq!(parsed.encode(), request);
        assert_eq!(
            parsed.key_package,
            unhex(i["key_package"].as_str().unwrap())
        );
        let reveal = unhex(i["reveal"].as_str().unwrap());
        let parsed = wire::Reveal::parse(&reveal).unwrap();
        assert_eq!(parsed.encode(), reveal);
        assert_eq!(
            wire::ref_hash(
                trommi_wire::invite::LABEL_COMMITMENT,
                &[&parsed.invite_id[..], &parsed.nonce[..]].concat()
            )
            .to_vec(),
            unhex(i["commitment"].as_str().unwrap())
        );
        n += 1;
    }
    assert!(n >= 2);

    let recovery = json_file(&workspace().join("spec/vectors/recovery.json"));
    let sealed = &recovery["sealed_key"];
    let bytes = unhex(sealed["sealed_key"].as_str().unwrap());
    let parsed = wire::SealedKey::parse(&bytes).unwrap();
    assert_eq!(parsed.encode(), bytes);
    assert_eq!(
        parsed.context.group_info.to_vec(),
        unhex(sealed["group_info_hash"].as_str().unwrap())
    );
    assert_eq!(
        wire::ref_hash(
            trommi_wire::recovery::GROUP_INFO_LABEL,
            &unhex(sealed["group_info"].as_str().unwrap())
        )
        .to_vec(),
        unhex(sealed["group_info_hash"].as_str().unwrap())
    );
    let link = unhex(
        recovery["recovery"]["recovery_link"]["link"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(wire::RecoveryLink::parse(&link).unwrap().encode(), link);
}

#[test]
fn the_constants_both_sides_take_from_core_wire_are_the_protocol_cores() {
    use trommi_wire::account::*;
    // the record a device writes is the pinned one the hub checks
    let record: Value = serde_json::from_str(trommi_core::account::KDF_RECORD).unwrap();
    assert_eq!(record["alg"], KDF_ALG);
    assert_eq!(
        (&record["v"], &record["m"], &record["t"], &record["p"]),
        (
            &Value::from(KDF_RECORD_VERSION),
            &Value::from(KDF_MEMORY_KIB),
            &Value::from(KDF_PASSES),
            &Value::from(KDF_LANES)
        )
    );
    assert!(trommi_hub::accounts::kdf_record(&serde_json::json!({ "kdf": record })).is_ok());
    assert_eq!(trommi_core::account::SEALED_COPY_LEN, SEALED_COPY_LEN);
}
