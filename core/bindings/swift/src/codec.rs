//! The client core's body codec (core/client/src/codec.rs) for the apps: the bodies of stored content (spec/v1.md 9.1)
//! between the wire and the model. The model's side crosses as JSON text: one text each way, whatever a body holds,
//! and the app parses it as it would have parsed the payload.
//!
//! A kind is named as the apps name it: `message`, `board_item`, `card`, `note`, `artifact`, `answer`,
//! `take_back`, `request`, `verdict`, `register`. A body that cannot be written is refused with `bad-format`, its
//! message beginning with the codec's own code: `bad-argument: …` (fields no reader would take) or
//! `bad-format: …`.

use crate::CoreError;
use crate::ErrorCode;
use serde_json::Value;
use trommi_client_core::codec::{self, BodyError, BodyKind, Decoded, Fields};

record! {
    /// A register write as a draft takes it: the wire's name, and the value as JSON (none deletes).
    pub struct RegisterWire {
        pub name: String,
        pub value: Option<Vec<u8>>,
    }
}

fn kind(name: &str) -> Result<BodyKind, CoreError> {
    BodyKind::from_name(name).ok_or_else(|| CoreError::bad_format("a body kind"))
}
fn refused(error: BodyError) -> CoreError {
    CoreError::Refused {
        code: ErrorCode::BadFormat,
        message: error.to_string(),
    }
}
fn json(text: &str, what: &str) -> Result<Value, CoreError> {
    serde_json::from_str(text).map_err(|_| CoreError::bad_format(what))
}
fn fields(text: &str) -> Result<Fields, CoreError> {
    match json(text, "a body's fields as a JSON object")? {
        Value::Object(m) => Ok(m),
        _ => Err(CoreError::bad_format("a body's fields as a JSON object")),
    }
}
/// What a read gives: the model's fields as JSON text, or `newer_schema`, or `bad`.
fn said(decoded: Decoded<String>) -> String {
    match decoded {
        Decoded::Body(text) => text,
        Decoded::NewerSchema => "newer_schema".into(),
        Decoded::Bad => "bad".into(),
    }
}

/// A body's payload (the JSON the core opened) as the model's fields: their JSON text (it begins with `{`), or
/// `newer_schema` for a `schema_version` above 2, or `bad` for what is no body of its kind. A register gives
/// `{ name, key, value, lamport }`, `key` the model's key for the wire's `name`. A received envelope carries this
/// already (`ReceivedEnvelope::body`).
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn body_from_wire(kind_name: String, payload: Vec<u8>) -> Result<String, CoreError> {
    Ok(said(codec::decode_body_text(kind(&kind_name)?, &payload)))
}

/// Several bodies at once, as [`body_from_wire`] reads each: one JSON list whose entries are the model's fields
/// (an object) or the text `newer_schema` or `bad`. One crossing for a page of envelopes.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn bodies_from_wire(
    kind_names: Vec<String>,
    payloads: Vec<Vec<u8>>,
) -> Result<String, CoreError> {
    if kind_names.len() != payloads.len() {
        return Err(CoreError::bad_format("a kind for each payload"));
    }
    let mut out = String::with_capacity(payloads.iter().map(|p| p.len() + 8).sum::<usize>() + 2);
    out.push('[');
    for (i, (name, payload)) in kind_names.iter().zip(&payloads).enumerate() {
        if i > 0 {
            out.push(',');
        }
        match codec::decode_body_text(kind(name)?, payload) {
            Decoded::Body(text) => out.push_str(&text),
            Decoded::NewerSchema => out.push_str("\"newer_schema\""),
            Decoded::Bad => out.push_str("\"bad\""),
        }
    }
    out.push(']');
    Ok(out)
}

/// A draft's fields (the model's names, a JSON object) as the payload the core seals: JSON with `schema_version`
/// 2. A register is written with [`register_to_wire`].
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn body_to_wire(kind_name: String, fields_json: String) -> Result<String, CoreError> {
    codec::encode_body(kind(&kind_name)?, &fields(&fields_json)?).map_err(refused)
}

/// A board item's payload in the wire's own form, checked as [`body_from_wire`] checks it: its JSON text, or
/// `newer_schema`, or `bad`.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn board_item_parse(payload: Vec<u8>) -> String {
    match codec::parse_board_item(&payload) {
        Decoded::Body(m) => Value::Object(m).to_string(),
        Decoded::NewerSchema => "newer_schema".into(),
        Decoded::Bad => "bad".into(),
    }
}

/// A register write: the wire's name for the model's `key`, and the value (JSON; `null` deletes) as the draft
/// takes it.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn register_to_wire(key: String, value_json: String) -> Result<RegisterWire, CoreError> {
    let (name, value) =
        codec::encode_register(&key, &json(&value_json, "a register value as JSON")?)
            .map_err(refused)?;
    Ok(RegisterWire {
        name,
        value: value.map(String::into_bytes),
    })
}

/// A register's name and value (JSON) as they came -> `{ key, value }` in the model's form, as JSON text.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn register_from_wire(name: String, value_json: String) -> Result<String, CoreError> {
    let (key, value) =
        codec::register_from_wire(&name, &json(&value_json, "a register value as JSON")?)
            .map_err(refused)?;
    Ok(serde_json::json!({ "key": key, "value": value }).to_string())
}

/// A register's name on the wire -> its key in the model.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn register_key_of(name: String) -> String {
    codec::register_key_of(&name)
}

/// A register's key in the model -> its name on the wire.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn register_name_of(key: String) -> String {
    codec::register_name_of(&key)
}

/// The files a body (the model's fields, JSON) names, for the envelope's header: none twice.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn body_file_ids(kind_name: String, fields_json: String) -> Result<Vec<Vec<u8>>, CoreError> {
    let ids = codec::file_ids_of(kind(&kind_name)?, &fields(&fields_json)?);
    Ok(ids.iter().map(|id| id.to_vec()).collect())
}

/// A wire attachment reference (JSON) as the model keeps it.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn attachment_from_wire(attachment_json: String) -> Result<String, CoreError> {
    let a = codec::attachment_from_wire(&json(&attachment_json, "an attachment as JSON")?)
        .map_err(refused)?;
    Ok(Value::Object(a).to_string())
}

/// A model attachment reference (JSON) as a body carries it.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn attachment_to_wire(attachment_json: String) -> Result<String, CoreError> {
    let a = codec::attachment_to_wire(&json(&attachment_json, "an attachment as JSON")?)
        .map_err(refused)?;
    Ok(Value::Object(a).to_string())
}

/// One shape of a snapshot file (`with_id`) or of a board item, as the wire carries it (JSON), as the model's
/// entry: board units, the picture's own size, type and name beside its attachment.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn board_entry_from_wire(entry_json: String, with_id: bool) -> Result<String, CoreError> {
    let e =
        codec::entry_from_wire(&json(&entry_json, "a shape as JSON")?, with_id).map_err(refused)?;
    Ok(Value::Object(e).to_string())
}

/// A shape id of the wire (`<sender base64url>/<number>/<index>`) as the model writes it; none for no such id.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn shape_id_from_wire(id: String) -> Option<String> {
    codec::shape_id_from_wire(&id)
}

/// A shape id of the model as the wire writes it; none for no such id.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn shape_id_to_wire(id: String) -> Option<String> {
    codec::shape_id_to_wire(&id)
}

/// The `piece` of a stroke piece message as the model's form (JSON); none for one the core would refuse.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn piece_from_wire(piece: Vec<u8>) -> Option<String> {
    codec::decode_piece(&piece).map(|m| Value::Object(m).to_string())
}

/// The model's piece (JSON: `stroke` in hex, `number`, `tool`, `color?`, `width?`, `points`) as the `piece` a
/// stroke piece message carries.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn piece_to_wire(piece_json: String) -> Result<Vec<u8>, CoreError> {
    let text =
        codec::encode_piece(&json(&piece_json, "a stroke piece as JSON")?).map_err(refused)?;
    Ok(text.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_names_the_codecs_code() {
        let e = body_to_wire("message".into(), r#"{"terminal":"work"}"#.into()).unwrap_err();
        assert_eq!(e.code(), ErrorCode::BadFormat);
        assert!(e.message().starts_with("bad-argument: "), "{}", e.message());
        assert_eq!(
            body_from_wire("message".into(), b"[]".to_vec()).unwrap(),
            "bad"
        );
        assert!(body_from_wire("nothing".into(), b"{}".to_vec()).is_err());
        let list = bodies_from_wire(
            vec!["message".into(), "card".into()],
            vec![b"{}".to_vec(), br#"{"schema_version":3}"#.to_vec()],
        )
        .unwrap();
        assert_eq!(list, r#"[{"content_type":"message"},"newer_schema"]"#);
    }
}
