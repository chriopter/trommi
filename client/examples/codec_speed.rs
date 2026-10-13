//! The codec alone: 10,000 card and message bodies read into the model's form (JSON text), as the facade hands
//! them to the apps. `cargo run --release -p trommi-client-core --example codec_speed`.

use serde_json::json;
use std::time::Instant;
use trommi_client_core::codec::{decode_body_text, BodyKind, Decoded};

fn b64(n: usize) -> String {
    let bytes: Vec<u8> = (0..n).map(|i| ((i * 7 + n) & 255) as u8).collect();
    trommi_core::ids::base64url_encode(&bytes)
}

fn main() {
    let att = json!({ "file_id": b64(16), "file_key": b64(32), "sha256": b64(32), "file_name": "shot.png", "media_type": "image/png", "total_size": 48211, "width": 800, "height": 600 });
    let items: Vec<(BodyKind, Vec<u8>)> = (0..10_000)
        .map(|i| {
            let atts = |every: usize| if i % every == 0 { json!([att]) } else { json!([]) };
            if i % 2 == 1 {
                let body = json!({ "schema_version": 2, "object_version": 1 + i % 3, "previous_version_hash": b64(32), "card_type": "decision", "title": format!("Ship the release {i}"), "teaser": "Two options, one pick", "body": "Some longer body text that explains the question in a few sentences. ".repeat(3), "options": [{ "key": "a", "label": "Ship", "final": true }, { "key": "b", "label": "Wait", "final": false }], "recommended": "a", "urgency_reason": "blocks the release", "attachments": atts(4) });
                (BodyKind::Card, body.to_string().into_bytes())
            } else {
                let body = json!({ "schema_version": 2, "content_type": "message", "text": format!("Done: the tests are green and the branch is pushed. {i}"), "details": "more detail ".repeat(10), "attachments": atts(5) });
                (BodyKind::Message, body.to_string().into_bytes())
            }
        })
        .collect();
    for round in 0..5 {
        let start = Instant::now();
        let mut bytes = 0;
        for (kind, payload) in &items {
            if let Decoded::Body(text) = decode_body_text(*kind, payload) {
                bytes += text.len();
            }
        }
        let parse = Instant::now();
        let mut n = 0;
        for (_, payload) in &items {
            n += serde_json::from_slice::<serde_json::Value>(payload).map_or(0, |v| v.as_object().map_or(0, |m| m.len()));
        }
        let raw = Instant::now();
        for (_, payload) in &items {
            n += serde_json::from_slice::<std::collections::BTreeMap<&str, &serde_json::value::RawValue>>(payload).map_or(0, |m| m.len());
        }
        println!("  (a Value parse: {:.2} ms, raw top level: {:.2} ms, {n})", (raw - parse).as_secs_f64() * 1e3, raw.elapsed().as_secs_f64() * 1e3);
        for want in [BodyKind::Card, BodyKind::Message] {
            let t = Instant::now();
            for (kind, payload) in items.iter().filter(|(k, _)| *k == want) {
                let _ = decode_body_text(*kind, payload);
            }
            println!("  {want:?}: {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
        }
        println!("round {round}: {:.2} ms for 10,000 bodies ({bytes} bytes out)", start.elapsed().as_secs_f64() * 1e3);
    }
}
