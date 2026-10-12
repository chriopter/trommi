//! The codec's rules, ported from the web app's tests (tests/web/model/codec.test.mjs, the body parts of
//! scribble.test.mjs): the fields of each kind, ids (base64url on the wire, hex in the model), attachment
//! references, register names and values, board items and stroke pieces, and what is refused.

use super::*;
use serde_json::json;
use trommi_core::board_items::Point;

/// n bytes that begin with `tag`, as the wire's base64url.
fn bytes(n: usize, tag: &[u8]) -> String {
    let mut out = vec![0u8; n];
    out[..tag.len()].copy_from_slice(tag);
    ids::base64url_encode(&out)
}
fn device(k: u8) -> String {
    bytes(32, &[0xd0, k])
}
fn id16(tag: &[u8]) -> String {
    bytes(16, tag)
}
/// The model's hex of a wire id.
fn hx(id: &str) -> String {
    hex(&ids::base64url_decode(id).unwrap())
}
fn raw(id: &str) -> [u8; 16] {
    ids::base64url_decode(id).unwrap().try_into().unwrap()
}
fn obj(v: Value) -> Fields {
    match v {
        Value::Object(m) => m,
        _ => panic!("not an object"),
    }
}
fn decode(kind: BodyKind, v: &Value) -> Decoded {
    decode_body(kind, v.to_string().as_bytes())
}
fn body(d: Decoded) -> Value {
    match d {
        Decoded::Body(m) => Value::Object(m),
        other => panic!("not a body: {other:?}"),
    }
}
fn encode(kind: BodyKind, v: Value) -> Result<Value> {
    encode_body(kind, &obj(v)).map(|s| serde_json::from_str(&s).unwrap())
}
fn code<T: std::fmt::Debug>(r: Result<T>) -> &'static str {
    r.expect_err("refused").code
}
fn points() -> String {
    let p = |x, y, t| Point { x, y, t, force: 128, azimuth: 0, altitude: 0 };
    Ink::new(vec![p(0, 0, 0), p(160, 80, 16)], false, false).unwrap().to_base64url()
}

struct Refs {
    file: String,
    poster: String,
    wire: Value,
    model: Value,
}
fn refs() -> Refs {
    let file = id16(&[0xf1]);
    let poster = id16(&[0xf2]);
    let (key, sha) = (bytes(32, &[1]), bytes(32, &[2]));
    let wire = json!({ "file_id": file, "file_key": key, "sha256": sha, "file_name": "clip.mp4", "media_type": "video/mp4", "total_size": 5000, "width": 640, "height": 360, "caption": "the demo", "poster_file_id": poster, "marks": [{ "x": 1 }] });
    let model = json!({ "attachment_id": hx(&file), "file_key": key, "sha256": sha, "file_name": "clip.mp4", "media_type": "video/mp4", "total_size": 5000, "width": 640, "height": 360, "caption": "the demo", "poster_attachment_id": hx(&poster), "marks": [{ "x": 1 }] });
    Refs { file, poster, wire, model }
}
fn with(v: &Value, k: &str, x: Value) -> Value {
    let mut m = obj(v.clone());
    m.insert(k.into(), x);
    Value::Object(m)
}

#[test]
fn an_attachment_reference_both_ways() {
    let r = refs();
    assert_eq!(Value::Object(attachment_from_wire(&r.wire).unwrap()), r.model);
    assert_eq!(Value::Object(attachment_to_wire(&r.model).unwrap()), r.wire);
    // the page a picture was made from names another attachment of the list by its id
    let page = format!("attachment:{}", r.poster);
    let page_hex = format!("attachment:{}", hx(&r.poster));
    assert_eq!(attachment_from_wire(&with(&r.wire, "page", page.clone().into())).unwrap()["page"], json!(page_hex));
    assert_eq!(attachment_to_wire(&with(&r.model, "page", page_hex.into())).unwrap()["page"], json!(page));
    assert_eq!(attachment_to_wire(&with(&r.model, "page", "https://example.org/p".into())).unwrap()["page"], json!("https://example.org/p"));
    // a field a newer client added is kept when read; a writer writes what it knows, with a name and a type always
    assert_eq!(attachment_from_wire(&with(&r.wire, "duration_ms", 900.into())).unwrap()["duration_ms"], json!(900));
    let minimal = json!({ "attachment_id": hx(&r.file), "file_key": "a2V5", "sha256": "c2hh", "total_size": 3, "url": "blob:x" });
    assert_eq!(
        Value::Object(attachment_to_wire(&minimal).unwrap()),
        json!({ "file_id": r.file, "file_name": "file", "media_type": "application/octet-stream", "total_size": 3, "file_key": "a2V5", "sha256": "c2hh" })
    );
    for b in [
        Value::Null,
        json!({}),
        with(&r.wire, "file_id", hx(&r.file).into()),
        with(&r.wire, "file_id", device(1).into()),
        with(&r.wire, "file_key", "not/base64url".into()),
        with(&r.wire, "poster_file_id", "x".into()),
    ] {
        assert_eq!(code(attachment_from_wire(&b)), "bad-format", "{b}");
    }
    assert_eq!(code(attachment_to_wire(&with(&r.model, "attachment_id", r.file.clone().into()))), "bad-argument");
}

#[test]
fn a_chat_message() {
    let r = refs();
    let (card, artifact, note) = (id16(&[0xc1]), id16(&[0xc7]), id16(&[0xb1]));
    let fields = json!({ "text": "see the page", "details": "more", "html": "<b>x</b>", "attachments": [r.model], "hand_back": true, "copied_cards": [hx(&card)], "marks": [1], "note": { "object_id": hx(&note), "written_at": 7 }, "published_object_id": hx(&artifact) });
    let wire = encode(BodyKind::Message, with(&with(&fields, "local_only", "dropped".into()), "x", Value::Null)).unwrap();
    assert_eq!(
        wire,
        json!({ "schema_version": 2, "content_type": "message", "text": "see the page", "details": "more", "html": "<b>x</b>", "attachments": [r.wire], "hand_back": true, "copied_cards": [card], "marks": [1], "note": { "object_id": note, "written_at": 7 }, "artifact_object_id": artifact })
    );
    let back = with(&with(&fields, "schema_version", 2.into()), "content_type", "message".into());
    assert_eq!(body(decode(BodyKind::Message, &wire)), back);
    assert_eq!(body(decode_body(BodyKind::Message, br#"{"text":"plain"}"#)), json!({ "text": "plain", "content_type": "message" }), "no schema_version is version 2; a message without a content type is a message");
    assert_eq!(decode_body(BodyKind::Message, br#"{"schema_version":3,"text":"x"}"#), Decoded::NewerSchema);
    let bytes_in = encode_body(BodyKind::Message, &obj(json!({ "text": "grüß dich" }))).unwrap();
    assert_eq!(body(decode_body(BodyKind::Message, bytes_in.as_bytes())), json!({ "schema_version": 2, "content_type": "message", "text": "grüß dich" }));
    assert_eq!(decode_body(BodyKind::Message, &[0x7b, 0x22, 0xff, 0x22, 0x7d]), Decoded::Bad, "bytes that are no UTF-8");
    for b in ["", "null", "[]", "\"x\"", "{", r#"{"schema_version":"2"}"#, r#"{"attachments":[{"file_id":"x"}]}"#] {
        assert_eq!(decode_body(BodyKind::Message, b.as_bytes()), Decoded::Bad, "{b}");
    }
    let plain = body(decode(BodyKind::Message, &json!({ "text": "x", "note": { "object_id": "not an id" }, "from_the_future": 1 })));
    assert!(plain.get("note").is_none(), "a bad note mark: the message stays, plain");
    assert_eq!(plain["from_the_future"], json!(1), "unknown fields are kept");
    assert_eq!(code(encode(BodyKind::Message, json!({ "text": "x", "note": { "object_id": "short" } }))), "bad-argument");
    assert_eq!(code(encode(BodyKind::Message, json!({ "content_type": "selection_sent", "text": "x" }))), "bad-argument", "a board's selection sent to a session is a message");
    assert_eq!(code(encode(BodyKind::Message, json!({ "terminal": "work", "work": {} }))), "bad-argument", "a work trail is no stored message");
}

#[test]
fn a_card_an_answer_a_request_an_artifact_a_note() {
    let r = refs();
    let (prev, into) = (bytes(32, &[0xee, 1]), id16(&[0xc2]));
    let card = json!({ "object_version": 2, "previous_version_hash": hx(&prev), "card_type": "decision", "title": "Q", "teaser": "short", "body": "long", "options": [{ "key": "a", "label": "A", "final": true }, { "key": "b", "label": "B", "final": false }],
        "sections": null, "allows_multiple": false, "recommended": "a", "urgency_reason": "soon", "attachments": [r.model], "change_note": "reworded", "merged_into_object_id": hx(&into), "merged_from_object_ids": [hx(&into)] });
    let wire = encode(BodyKind::Card, card.clone()).unwrap();
    assert_eq!(wire["previous_version_hash"], json!(prev));
    assert_eq!(wire["merged_into_object_id"], json!(into));
    assert_eq!(wire["merged_from_object_ids"], json!([into]));
    assert_eq!(wire["attachments"], json!([r.wire]));
    assert_eq!(wire["options"], json!([{ "key": "a", "label": "A", "final": true }, { "key": "b", "label": "B" }]), "`final` only where it is true");
    let back = with(&with(&card, "schema_version", 2.into()), "options", wire["options"].clone());
    assert_eq!(body(decode(BodyKind::Card, &wire)), back);
    assert_eq!(code(encode(BodyKind::Card, json!({ "title": "Q", "teaser": "two\nlines" }))), "bad-argument");
    assert!(body(decode(BodyKind::Card, &json!({ "title": "Q", "teaser": " padded " }))).get("teaser").is_none(), "a bad teaser is dropped");
    assert_eq!(decode(BodyKind::Card, &json!({ "title": "Q", "previous_version_hash": "nonsense" })), Decoded::Bad);

    let answer = json!({ "answer_action": "answer", "choices": ["a"], "note": "because", "option_notes": { "a": "yes" }, "attachments": [r.model], "marks": [], "trusted": false });
    let written = encode(BodyKind::Answer, with(&with(&answer, "pending", true.into()), "envelope_number", 5.into())).unwrap();
    assert_eq!(body(decode(BodyKind::Answer, &written)), with(&answer, "schema_version", 2.into()), "the model's own fields of an answer are not written");
    assert_eq!(encode_body(BodyKind::TakeBack, &obj(json!({ "anything": 1 }))).unwrap(), r#"{"schema_version":2}"#);
    assert_eq!(encode_body(BodyKind::Verdict, &Fields::new()).unwrap(), r#"{"schema_version":2}"#);
    let request = encode(BodyKind::Request, json!({ "tool_name": "Bash", "description": "d", "input_preview": "ls", "expires_at": 5 })).unwrap();
    assert_eq!(body(decode(BodyKind::Request, &request)), json!({ "schema_version": 2, "tool_name": "Bash", "description": "d", "input_preview": "ls" }));

    let artifact = json!({ "object_version": 1, "artifact_type": "page", "title": "Report", "note": null, "attachments": [r.model], "released_until": 99 });
    let aw = encode(BodyKind::Artifact, artifact.clone()).unwrap();
    assert_eq!(aw["shared_until"], json!(99));
    assert!(aw.get("released_until").is_none());
    assert_eq!(body(decode(BodyKind::Artifact, &aw)), with(&artifact, "schema_version", 2.into()));

    // a Note's fields are the app's: everything is written
    let note = json!({ "text": "call", "place": "desk", "to": "abc", "held": { "until": 5 }, "attachments": [r.model], "lamport": 4 });
    let nw = encode(BodyKind::Note, note.clone()).unwrap();
    assert_eq!(nw, json!({ "schema_version": 2, "text": "call", "place": "desk", "to": "abc", "held": { "until": 5 }, "attachments": [r.wire], "lamport": 4 }));
    assert_eq!(body(decode(BodyKind::Note, &nw)), with(&note, "schema_version", 2.into()));
}

/// A register write with its value as JSON, to compare.
fn written(key: &str, value: Value) -> (String, Option<Value>) {
    let (name, v) = encode_register(key, &value).unwrap();
    (name, v.map(|v| serde_json::from_str(&v).unwrap()))
}

#[test]
fn registers_keys_names_and_the_ids_in_a_value() {
    let r = refs();
    let (o, board, dev, hash) = (id16(&[0xc1]), id16(&[0xde]), device(3), bytes(32, &[0x44]));
    let pairs = [
        (format!("draft/{o}"), format!("draft/{}", hx(&o))),
        (format!("snooze/{o}"), format!("snooze/{}", hx(&o))),
        (format!("duck/{o}"), format!("duck/{}", hx(&o))),
        (format!("desk/{board}"), format!("desk/{}", hx(&board))),
        (format!("session/{o}"), format!("session/{}", hx(&o))),
        (format!("board_snapshot/{board}"), format!("scribble_snapshot/desk/{}", hx(&board))),
        (format!("device/{dev}"), format!("device/{}", hx(&dev))),
        (format!("alert/{hash}"), format!("alert/{}", hx(&hash))),
        (format!("read/card/{o}"), format!("read/card/{}", hx(&o))),
    ];
    for (name, key) in pairs.iter().map(|(a, b)| (a.as_str(), b.as_str())).chain(
        ["crown", "kit", "heads", "profile", "heard", "goals", "status_line/tests", "desk/main"].iter().map(|n| (*n, *n)),
    ) {
        assert_eq!(register_key_of(name), key, "{name}");
        assert_eq!(register_name_of(key), name, "{key}");
    }
    let read = |v: Value| body(decode(BodyKind::Register, &v));
    assert_eq!(
        read(json!({ "name": format!("session/{o}"), "value": { "name": "S", "desk": board }, "lamport": 6 })),
        json!({ "name": format!("session/{o}"), "key": format!("session/{}", hx(&o)), "value": { "name": "S", "desk": hx(&board) }, "lamport": 6 })
    );
    assert_eq!(read(json!({ "name": "crown", "value": null, "lamport": 1 })), json!({ "name": "crown", "key": "crown", "value": null, "lamport": 1 }));
    assert_eq!(decode_body(BodyKind::Register, br#"{"value":1}"#), Decoded::Bad);
    // what a register write hands the core: the wire's name and the value as JSON (none deletes)
    assert_eq!(
        written(&format!("session/{}", hx(&o)), json!({ "name": "S", "desk": hx(&board), "archived": true })),
        (format!("session/{o}"), Some(json!({ "name": "S", "desk": board, "archived": true })))
    );
    assert_eq!(written("crown", json!({ "session_id": hx(&o), "agent_device_id": hx(&dev) })), ("crown".into(), Some(json!({ "session_id": o, "agent_device_id": dev }))));
    assert_eq!(written(&format!("draft/{}", hx(&o)), Value::Null), (format!("draft/{o}"), None));
    assert_eq!(written("status_line/tests", json!({ "label": "Tests", "object_id": hx(&o) })), ("status_line/tests".into(), Some(json!({ "label": "Tests", "object_id": o }))));
    // the board snapshot as the whiteboard writes it and as spec 10.2 has it
    let snap = written(
        &format!("scribble_snapshot/desk/{}", hx(&board)),
        json!({ "attachment": r.model, "frontier": { hx(&dev): [4, hx(&hash)] }, "last_envelope_number": 300, "shapes": 12 }),
    );
    assert_eq!(snap, (format!("board_snapshot/{board}"), Some(json!({ "attachment": r.wire, "frontier": { dev.clone(): [4, hash] }, "change": 300 }))));
    let back = read(json!({ "name": format!("board_snapshot/{board}"), "value": snap.1.unwrap(), "lamport": 2 }));
    assert_eq!(back["value"], json!({ "attachment": r.model, "frontier": { hx(&dev): [4, hx(&hash)] }, "last_envelope_number": 300 }));
    assert_eq!(code(encode(BodyKind::Register, json!({}))), "bad-argument");
    assert_eq!(code(encode_register("", &json!(1))), "bad-argument");
}

#[test]
fn the_files_a_body_names_and_shape_ids() {
    let r = refs();
    let (file, poster) = (raw(&r.file), raw(&r.poster));
    assert_eq!(file_ids_of(BodyKind::Message, &obj(json!({ "attachments": [r.model, r.model] }))), vec![file, poster]);
    assert_eq!(
        file_ids_of(BodyKind::BoardItem, &obj(json!({ "content_type": "strokes", "strokes": [{ "tool": "pen" }, { "tool": "image", "attachment": r.model }] }))),
        vec![file, poster]
    );
    assert_eq!(file_ids_of(BodyKind::Register, &obj(json!({ "value": { "attachment": { "attachment_id": hx(&r.file) } } }))), vec![file]);
    assert!(file_ids_of(BodyKind::Card, &obj(json!({ "title": "Q" }))).is_empty());
    let dev = device(4);
    assert_eq!(shape_id_from_wire(&format!("{dev}/17/3")), Some(format!("{}/17/3", hx(&dev))));
    assert_eq!(shape_id_to_wire(&format!("{}/17/3", hx(&dev))), Some(format!("{dev}/17/3")));
    for b in [format!("{dev}/0/0"), format!("{dev}/1"), format!("{dev}/1/01"), format!("{}/1/0", hx(&dev)), "x/1/0".into(), format!("{dev}/1/0/2")] {
        assert_eq!(shape_id_from_wire(&b), None, "{b}");
    }
    assert_eq!(shape_id_to_wire(&format!("{dev}/1/0")), None);
    assert_eq!(shape_id_from_wire(&format!("{dev}/1/4294967295")), Some(format!("{}/1/4294967295", hx(&dev))));
    assert_eq!(shape_id_from_wire(&format!("{dev}/1/4294967296")), None);
}

/// spec/register-names.json is shared with the web and iOS tests.
#[test]
fn register_names_and_values_against_the_shared_file() {
    let v: Value = serde_json::from_str(include_str!("../../spec/register-names.json")).unwrap();
    for n in v["names"].as_array().unwrap() {
        let (wire, model) = (n["wire"].as_str().unwrap(), n["model"].as_str().unwrap());
        assert_eq!(register_name_of(model), wire, "written: {model}");
        assert_eq!(register_key_of(wire), model, "read: {wire}");
        if let Some(legacy) = n["legacy_wire"].as_str() {
            assert_eq!(register_key_of(legacy), model, "read as iOS wrote it: {legacy}");
        }
    }
    for x in v["values"].as_array().unwrap() {
        let key = x["key"].as_str().unwrap();
        let name = register_name_of(key);
        let (_, value) = written(key, x["model"].clone());
        assert_eq!(value.as_ref(), Some(&x["wire"]), "written: {key}");
        let read = |w: &Value| body(decode(BodyKind::Register, &json!({ "name": name, "value": w, "lamport": 1 })))["value"].clone();
        assert_eq!(read(&x["wire"]), x["model"], "read: {key}");
        if let Some(legacy) = x.get("legacy_wire") {
            assert_eq!(read(legacy), x["model"], "read as iOS wrote it: {key}");
        }
    }
}

#[test]
fn board_items_entries_round_trip_in_whole_quanta() {
    let pts = points();
    let (key, sha) = (bytes(32, &[1]), bytes(32, &[2]));
    let picture = json!({ "attachment_id": "a".repeat(32), "file_key": key, "sha256": sha, "file_name": "p.png", "media_type": "image/png", "total_size": 10 });
    let stroke = json!({ "tool": "marker", "color": "pink", "width": 28, "points": pts, "z": 3, "group": "g1" });
    let note = json!({ "tool": "sticky", "at": [12.5, -4], "text": "hi", "size": 17, "color": "ink", "wrap": 240 });
    let pic = json!({ "tool": "image", "rect": [0, 0, 300, 200.5], "attachment": picture, "nw": 600, "nh": 400, "mime": "image/png", "name": "p.png" });
    let wire = encode(BodyKind::BoardItem, json!({ "content_type": "strokes", "strokes": [stroke, note, pic] })).unwrap();
    assert_eq!(wire["schema_version"], json!(2));
    assert_eq!(wire["strokes"][0], json!({ "tool": "marker", "color": "pink", "width": 448, "points": pts, "z": 3, "group": "g1" }));
    assert_eq!(wire["strokes"][1], json!({ "tool": "sticky", "at": [200, -64], "text": "hi", "size": 272, "color": "ink", "wrap": 3840 }));
    assert_eq!(wire["strokes"][2]["rect"], json!([0, 0, 4800, 3208]));
    assert_eq!(wire["strokes"][2]["attachment"]["file_id"], json!(ids::base64url_encode(&[0xaa; 16])));
    assert_eq!((&wire["strokes"][2]["attachment"]["width"], &wire["strokes"][2]["attachment"]["height"]), (&json!(600), &json!(400)));
    let back = body(decode(BodyKind::BoardItem, &wire));
    assert_eq!(back["strokes"][0], json!({ "tool": "marker", "color": "pink", "width": 28, "points": pts, "z": 3, "group": "g1" }));
    assert_eq!(back["strokes"][1], json!({ "tool": "sticky", "at": [12.5, -4], "text": "hi", "size": 17, "color": "ink", "wrap": 240 }));
    let p = &back["strokes"][2];
    assert_eq!(
        [&p["rect"], &p["nw"], &p["nh"], &p["mime"], &p["name"], &p["attachment"]["attachment_id"]],
        [&json!([0, 0, 300, 200.5]), &json!(600), &json!(400), &json!("image/png"), &json!("p.png"), &json!("a".repeat(32))]
    );
    assert_eq!(parse_board_item(wire.to_string().as_bytes()), Decoded::Body(obj(wire.clone())), "the wire's own form, checked");
}

#[test]
fn board_items_trommi_core_would_refuse_are_not_written_and_not_read() {
    let pts = points();
    for entry in [
        json!({ "tool": "eraser", "points": pts }),
        json!({ "tool": "hl" }),
        json!({ "tool": "pen", "points": "AA" }),
        json!({ "tool": "image", "rect": [0, 0, 1, 1] }),
        json!({ "tool": "pen", "points": pts, "color": "" }),
    ] {
        let r = encode(BodyKind::BoardItem, json!({ "content_type": "strokes", "strokes": [entry] }));
        assert!(matches!(code(r), "bad-format" | "bad-argument"), "{entry}");
    }
    assert_eq!(code(encode(BodyKind::BoardItem, json!({ "content_type": "strokes", "strokes": [] }))), "bad-format");
    assert_eq!(code(encode(BodyKind::BoardItem, json!({ "content_type": "erase", "stroke_ids": ["nobody/1/0"] }))), "bad-format");
    assert_eq!(code(encode(BodyKind::BoardItem, json!({ "content_type": "lasso" }))), "bad-argument");
    let sender = device(1);
    let id = format!("{sender}/3/0");
    let ok = json!({ "tool": "pen", "color": "ink", "width": 64, "points": pts });
    let read = |b: Value| decode(BodyKind::BoardItem, &b);
    assert_eq!(
        body(read(json!({ "schema_version": 2, "content_type": "move", "shape_ids": [id], "offset": [16, -8] }))),
        json!({ "content_type": "move", "stroke_ids": [format!("{}/3/0", hx(&sender))], "offset": [1, -0.5] })
    );
    assert_eq!(body(read(json!({ "content_type": "erase", "shape_ids": [id] }))), json!({ "content_type": "erase", "stroke_ids": [format!("{}/3/0", hx(&sender))] }));
    assert_eq!(read(json!({ "schema_version": 3, "content_type": "strokes", "strokes": [ok] })), Decoded::NewerSchema);
    let field = |k: &str, v: Value| json!({ "content_type": "strokes", "strokes": [with(&ok, k, v)] });
    let refused = [
        ("a number with a fraction", json!({ "content_type": "move", "shape_ids": [id], "offset": [0.5, 0] })),
        ("an id twice", json!({ "content_type": "erase", "shape_ids": [id, id] })),
        ("no id", json!({ "content_type": "erase", "shape_ids": [] })),
        ("an id with number 0", json!({ "content_type": "erase", "shape_ids": [format!("{sender}/0/0")] })),
        ("an id with a leading zero", json!({ "content_type": "erase", "shape_ids": [format!("{sender}/03/0")] })),
        ("a hex sender on the wire", json!({ "content_type": "erase", "shape_ids": [format!("{}/3/0", hx(&sender))] })),
        ("an unknown content type", json!({ "content_type": "selection_sent", "text": "x" })),
        ("an unknown tool", field("tool", "brush".into())),
        ("a shape that names an id", field("id", id.clone().into())),
        ("a field of another tool", field("text", "x".into())),
        ("a field that is null", field("group", Value::Null)),
        ("a width of 0", field("width", 0.into())),
        ("a width in board units", field("width", json!(4.5))),
        ("an offset on an erase", json!({ "content_type": "erase", "shape_ids": [id], "offset": [1, 1] })),
        ("an older schema", json!({ "schema_version": 1, "content_type": "erase", "shape_ids": [id] })),
    ];
    for (what, b) in refused {
        assert_eq!(read(b.clone()), Decoded::Bad, "{what}");
        assert_eq!(parse_board_item(b.to_string().as_bytes()), Decoded::Bad, "{what}");
    }
}

#[test]
fn snapshot_shapes_carry_their_id() {
    let pts = points();
    let id = format!("{}/3/0", device(1));
    let shape = json!({ "id": id, "tool": "pen", "color": "ink", "width": 64, "points": pts });
    assert_eq!(check_wire_shape(&shape, true), Ok(Family::Pen));
    assert_eq!(code(check_wire_shape(&shape, false)), "bad-format");
    assert_eq!(entry_from_wire(&shape, true).unwrap()["width"], json!(4));
}

#[test]
fn a_stroke_piece_both_ways() {
    let stroke = id16(&[0x5a]);
    let pts = points();
    let text = encode_piece(&json!({ "stroke": hx(&stroke), "number": 2, "tool": "pen", "color": "blue", "width": 4, "points": pts })).unwrap();
    let wire: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(wire, json!({ "stroke": stroke, "number": 2, "tool": "pen", "color": "blue", "width": 64, "points": pts }));
    assert_eq!(
        Value::Object(decode_piece(text.as_bytes()).unwrap()),
        json!({ "content_type": "stroke_piece", "stroke": hx(&stroke), "number": 2, "tool": "pen", "color": "blue", "width": 4, "points": pts })
    );
    // a marker without colour and width gets the marker's
    let m: Value = serde_json::from_str(&encode_piece(&json!({ "stroke": hx(&stroke), "number": 1, "tool": "marker", "points": pts })).unwrap()).unwrap();
    assert_eq!((&m["color"], &m["width"]), (&json!("yellow"), &json!(288)));
    assert_eq!(code(encode_piece(&json!({ "stroke": stroke, "number": 1, "tool": "pen", "points": pts }))), "bad-format");
    assert_eq!(code(encode_piece(&json!({ "stroke": hx(&stroke), "number": 0, "tool": "pen", "points": pts }))), "bad-format");
    for b in [
        json!({ "stroke": stroke, "number": 1, "tool": "brush", "color": "ink", "width": 64, "points": pts }),
        json!({ "stroke": stroke, "number": 1, "tool": "pen", "color": "", "width": 64, "points": pts }),
        json!({ "stroke": stroke, "number": 1, "tool": "pen", "color": "ink", "width": 64, "points": "AA" }),
        json!([1]),
    ] {
        assert_eq!(decode_piece(b.to_string().as_bytes()), None, "{b}");
    }
}

#[test]
fn numbers_as_javascript_has_them() {
    assert_eq!(js_round(2.5), 3.0);
    assert_eq!(js_round(-2.5), -2.0);
    assert_eq!(js_round(0.49999999999999994), 0.0);
    assert_eq!(to_i32(4_294_967_296.0 + 5.0), 5);
    assert_eq!(to_i32(2_147_483_648.0), -2_147_483_648);
    assert_eq!(quantum(Some(&json!(1e12))), to_i32(1.6e13));
    assert_eq!(length_q(Some(&json!(-1)), 4.0), 64);
    assert_eq!(length_q(Some(&json!(5000)), 4.0), 16_000);
    assert_eq!(fnum(2.0), json!(2));
    assert_eq!(fnum(-0.5), json!(-0.5));
    assert!(teaser_valid(&json!("a\u{a0}b")));
    assert!(!teaser_valid(&json!("\u{feff}x")), "JavaScript trims the byte order mark");
    assert!(teaser_valid(&json!("é".repeat(160))));
    assert!(!teaser_valid(&json!("é".repeat(161))));
}
