//! The bodies of stored content (spec/v1.md 9.1: UTF-8 JSON, `schema_version` 2) between the wire and the model,
//! both directions, and nothing else. The protocol core seals and opens envelopes; [`decode_body`] turns an opened
//! payload into the fields the apps' models and views name, [`encode_body`] turns a draft's fields into the payload
//! the core seals. One implementation for every app: the web app and iOS reach it through the facade
//! (`core/bindings/swift/src/client.rs`, as JSON text), a Rust host uses it directly.
//!
//! What differs between the two forms, and is converted here and nowhere else:
//!
//! ```text
//!   ids             the wire's are base64url, the model's lowercase hex: object ids, device ids, envelope hashes,
//!                   file ids, the ids inside register names and inside a shape id
//!   attachments     wire { file_id, poster_file_id?, … } (9.1.1)  <->  model { attachment_id, poster_attachment_id?, … }
//!   a message       wire artifact_object_id                       <->  model published_object_id
//!   an Artifact     wire shared_until                             <->  model released_until
//!   registers       wire board_snapshot/<board> { attachment, frontier, change }
//!                                         <->  model scribble_snapshot/desk/<board> { attachment, frontier, last_envelope_number }
//!   board items     wire shape_ids, every position and length a whole number of 1/16 board unit (10.5)
//!                                         <->  model stroke_ids, board units
//! ```
//!
//! A reader keeps fields it does not know (9.1.2); a writer writes the fields of its kind and drops the rest,
//! except on a Note, whose fields are the app's. Board items and stroke pieces are read strictly: what
//! trommi-core's `board_items::ItemBody::decode` or `trail::StrokePiece::decode` refuses is [`Decoded::Bad`] here.
//!
//! The numbers follow JavaScript's, which the web app wrote first and every app shares: a whole number may be
//! written `3` or `3.0`, rounding is half up (`Math.round`), a position wraps to 32 bits.
//!
//! A body read keeps the order of its keys at the top level; what this codec builds (registers, board items,
//! attachments, everything written) has its keys sorted (serde_json's map). No reader depends on the order.

use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::Deserialize;
use serde_json::value::RawValue;
use serde_json::{Map, Number, Value};
use std::borrow::Cow;
use trommi_core::board_items::{Ink, ItemBody};
use trommi_core::ids;
use trommi_core::trail::StrokePiece;

/// The body version every client writes; a higher one is shown as "needs a newer Trommi".
pub const SCHEMA_VERSION: u64 = 2;
/// What every client says (or shows) when it meets something only a newer version understands.
pub const UPDATE_MESSAGE: &str = "This needs a newer version of Trommi. Update to see it.";
/// A card's `teaser`: at most this many characters (Unicode scalar values).
pub const TEASER_MAX: usize = 160;
/// One position quantum: 1/16 board unit.
const Q: f64 = 16.0;
const MAX_WIDTH: i64 = 1000 * 16;
const MAX_SHAPES: usize = 1000;
const MAX_SHAPE_IDS: usize = 2000;
/// The largest stroke piece, in bytes (spec 7.2).
const MAX_PIECE_LEN: usize = 40_000;

/// A body as plain fields.
pub type Fields = Map<String, Value>;

/// Which body: a Chat message, a board item, the version of a card, Note or Artifact, an answer, a take back, a
/// permission request, a verdict, a register value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    Message,
    BoardItem,
    Card,
    Note,
    Artifact,
    Answer,
    TakeBack,
    Request,
    Verdict,
    Register,
}

impl BodyKind {
    /// The kind by the name the apps use: `message`, `board_item`, `card`, `note`, `artifact`, `answer`,
    /// `take_back`, `request`, `verdict`, `register`.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "message" => Self::Message,
            "board_item" => Self::BoardItem,
            "card" => Self::Card,
            "note" => Self::Note,
            "artifact" => Self::Artifact,
            "answer" => Self::Answer,
            "take_back" => Self::TakeBack,
            "request" => Self::Request,
            "verdict" => Self::Verdict,
            "register" => Self::Register,
            _ => return None,
        })
    }

    /// The fields a writer writes for this kind (besides `schema_version`), by their model names; none for a
    /// Note (its fields are the app's), a board item and a register.
    #[rustfmt::skip]
    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Message => &[
                "content_type", "text", "details", "html", "attachments", "explain", "hand_back", "present_card",
                "copied_cards", "marks", "terminal", "note", "published_object_id", "helper",
            ],
            Self::Card => &[
                "object_version", "previous_version_hash", "card_type", "title", "teaser", "body", "options",
                "sections", "html", "allows_multiple", "recommended", "urgency_reason", "attachments", "change_note",
                "close_summary", "withdraw_reason", "merged_into_object_id", "merged_from_object_ids", "working",
                "helper",
            ],
            Self::Artifact => &[
                "object_version", "previous_version_hash", "artifact_type", "title", "note", "attachments",
                "released_until",
            ],
            Self::Answer => &[
                "answer_action", "choices", "note", "option_notes", "attachments", "marks", "trusted", "answered_at",
            ],
            Self::Request => &["tool_name", "description", "input_preview"],
            Self::TakeBack | Self::Verdict | Self::Note | Self::BoardItem | Self::Register => &[],
        }
    }
}

/// A card's own fields: everything of its body but the version's number and predecessor.
pub fn card_content_fields() -> &'static [&'static str] {
    &BodyKind::Card.fields()[2..]
}
/// A card's `card_type`.
pub const CARD_TYPES: [&str; 2] = ["decision", "info"];
/// An answer's `answer_action`.
pub const ANSWER_ACTIONS: [&str; 3] = ["answer", "read", "shred"];
/// A board item's content types (10.5).
pub const BOARD_CONTENT_TYPES: [&str; 4] = ["strokes", "erase", "move", "send_away"];

/// A body as the model reads it, or why it is not read.
#[derive(Debug, Clone, PartialEq)]
pub enum Decoded<T = Fields> {
    /// The fields, in the model's names.
    Body(T),
    /// A `schema_version` above 2: "needs a newer Trommi".
    NewerSchema,
    /// No body of its kind.
    Bad,
}

/// Why a body cannot be written or read. `code`: `bad-argument` (a writer's fields no reader would take) or
/// `bad-format` (not what the wire or the model form allows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for BodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BodyError {}

type Result<T> = std::result::Result<T, BodyError>;

fn bad<T>(message: &str) -> Result<T> {
    Err(BodyError {
        code: "bad-format",
        message: message.to_owned(),
    })
}
fn argument<T>(message: &str) -> Result<T> {
    Err(BodyError {
        code: "bad-argument",
        message: message.to_owned(),
    })
}

// ---- numbers, as JavaScript has them ----------------------------------------------------------------------------

/// A JSON number's value.
fn num(v: &Value) -> Option<f64> {
    v.as_f64()
}
/// A whole number (`3` or `3.0`).
fn whole(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_f64(),
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite() && x.fract() == 0.0),
        _ => None,
    }
}
fn is_i32(v: &Value) -> bool {
    whole(v).is_some_and(|x| (-2_147_483_648.0..2_147_483_648.0).contains(&x))
}
fn i32s(v: &Value, n: usize) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() == n && a.iter().all(is_i32))
}
fn is_u32(v: &Value) -> bool {
    whole(v).is_some_and(|x| (0.0..=4_294_967_295.0).contains(&x))
}
const SAFE: f64 = 9_007_199_254_740_991.0;
fn is_safe(v: &Value) -> bool {
    whole(v).is_some_and(|x| x.abs() <= SAFE)
}
/// A width or size in quanta: 1 to 1000 board units.
fn size_ok(v: &Value) -> bool {
    whole(v).is_some_and(|x| x >= 1.0 && x <= MAX_WIDTH as f64)
}
/// `Math.round`: half up.
fn js_round(x: f64) -> f64 {
    let f = x.floor();
    if x - f >= 0.5 {
        f + 1.0
    } else {
        f
    }
}
/// `x | 0`: modulo 2^32, as a signed 32-bit number.
fn to_i32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4_294_967_296.0);
    (m as u32) as i32
}
/// A number as JSON: a whole one without a fraction.
fn fnum(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() <= SAFE {
        Value::from(x as i64)
    } else {
        Number::from_f64(x).map_or(Value::Null, Value::Number)
    }
}
/// Quanta as board units.
fn units(v: &Value) -> Value {
    fnum(num(v).unwrap_or(0.0) / Q)
}
/// A position in board units as its whole number of quanta, modulo 2^32 (ink.ts `quantum`).
fn quantum(v: Option<&Value>) -> i32 {
    let x = v.and_then(num).filter(|x| x.is_finite()).unwrap_or(0.0);
    to_i32(js_round(x * Q))
}
/// A length in board units as quanta, 1 to 1000 units; `fallback` where it is no positive number.
fn length_q(v: Option<&Value>, fallback: f64) -> i64 {
    let x = v
        .and_then(num)
        .filter(|x| x.is_finite() && *x > 0.0)
        .unwrap_or(fallback);
    (js_round(x * Q) as i64).clamp(1, MAX_WIDTH)
}

// ---- ids --------------------------------------------------------------------------------------------------------

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[usize::from(b >> 4)] as char);
        out.push(DIGITS[usize::from(b & 15)] as char);
    }
    out
}
fn unhex(text: &str) -> Option<Vec<u8>> {
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let t = text.as_bytes();
    if !t.len().is_multiple_of(2) {
        return None;
    }
    t.chunks(2)
        .map(|p| Some(digit(p[0])? << 4 | digit(p[1])?))
        .collect()
}
/// A wire id of exactly `n` bytes (base64url) as hex.
fn hex_id_str(text: &str, n: usize) -> Option<String> {
    let mut buf = [0u8; 32];
    let out = buf.get_mut(..n)?;
    ids::base64url_decode_into(text, out).ok()?;
    Some(hex(out))
}
/// A 16-byte id as the wire writes it (base64url) in the model's spelling (32 hex digits); none for anything else.
pub fn hex_of_wire_id(text: &str) -> Option<String> {
    hex_id_str(text, 16)
}
/// A 16-byte id in the model's spelling as the wire writes it; none for anything else.
pub fn wire_of_hex_id(text: &str) -> Option<String> {
    wire_id_str(text, 16)
}
fn hex_id(v: &Value, n: usize) -> Option<String> {
    v.as_str().and_then(|t| hex_id_str(t, n))
}
/// A model id of exactly `n` bytes (lowercase hex) as the wire writes it.
fn wire_id_str(text: &str, n: usize) -> Option<String> {
    if text.len() != n * 2 {
        return None;
    }
    unhex(text).map(|b| ids::base64url_encode(&b))
}
fn wire_id(v: &Value, n: usize) -> Option<String> {
    v.as_str().and_then(|t| wire_id_str(t, n))
}
/// The same, keeping what is not such an id as it stands (a team called `main`, a value a newer client wrote).
fn soft_hex(v: &Value, n: usize) -> Value {
    hex_id(v, n).map_or_else(|| v.clone(), Value::String)
}
fn soft_wire(v: &Value, n: usize) -> Value {
    wire_id(v, n).map_or_else(|| v.clone(), Value::String)
}
type IdMap = fn(&Value, usize) -> Value;

fn id_list(v: &Value, id: IdMap) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(|x| id(x, 16)).collect()),
        other => other.clone(),
    }
}

/// `<shape id>` parts: sender, number (1 to 16 digits, no leading zero), index (0 or up to 10 digits, at most
/// 2^32 - 1).
fn shape_parts(id: &str, sender_ok: fn(&str) -> bool) -> Option<(&str, &str, &str)> {
    let mut parts = id.split('/');
    let (Some(sender), Some(seq), Some(index), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    let seq_ok = digits(seq) && seq.len() <= 16 && !seq.starts_with('0');
    let index_ok = digits(index)
        && (index == "0" || (!index.starts_with('0') && index.len() <= 10))
        && index.parse::<u64>().is_ok_and(|i| i <= 0xffff_ffff);
    (sender_ok(sender) && seq_ok && index_ok).then_some((sender, seq, index))
}
/// A shape id `<sender>/<envelope number>/<index>` (10.1) as the model writes it: the sender in hex.
pub fn shape_id_from_wire(id: &str) -> Option<String> {
    let b64 = |s: &str| {
        s.len() == 43
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    };
    let (sender, seq, index) = shape_parts(id, b64)?;
    Some(format!("{}/{seq}/{index}", hex_id_str(sender, 32)?))
}
/// A shape id of the model as the wire writes it: the sender in base64url.
pub fn shape_id_to_wire(id: &str) -> Option<String> {
    let hex64 = |s: &str| {
        s.len() == 64
            && s.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    };
    let (sender, seq, index) = shape_parts(id, hex64)?;
    Some(format!("{}/{seq}/{index}", wire_id_str(sender, 32)?))
}

// ---- attachments (9.1.1) ----------------------------------------------------------------------------------------

fn b64u_text(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str).is_some_and(|s| {
        !s.is_empty()
            && s.bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    })
}
/// A value that stands and is not null (JavaScript's `!= null`).
fn given(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| !v.is_null())
}
/// A page named as another attachment of the same list: `attachment:<id>`.
fn page_ref(page: &Value, to: fn(&str, usize) -> Option<String>) -> Value {
    match page.as_str().and_then(|p| p.strip_prefix("attachment:")) {
        Some(id) => to(id, 16).map_or_else(
            || page.clone(),
            |id| Value::String(format!("attachment:{id}")),
        ),
        None => page.clone(),
    }
}

/// A wire attachment reference as the model keeps it; `bad-format` for one that names no file. Fields a newer
/// client added are kept.
pub fn attachment_from_wire(a: &Value) -> Result<Fields> {
    let Some(a) = a.as_object() else {
        return bad("an attachment is not an object");
    };
    let Some(attachment_id) = a.get("file_id").and_then(|v| hex_id(v, 16)) else {
        return bad("an attachment names no file");
    };
    if !b64u_text(a.get("file_key")) || !b64u_text(a.get("sha256")) {
        return bad("an attachment names no file");
    }
    let mut out = Fields::new();
    out.insert("attachment_id".into(), Value::String(attachment_id));
    for (k, v) in a {
        if k != "file_id" && k != "poster_file_id" {
            out.insert(k.clone(), v.clone());
        }
    }
    if let Some(poster) = given(a.get("poster_file_id")) {
        let Some(id) = hex_id(poster, 16) else {
            return bad("a poster names no file");
        };
        out.insert("poster_attachment_id".into(), Value::String(id));
    }
    if let Some(page) = out.get_mut("page") {
        *page = page_ref(page, hex_id_str);
    }
    Ok(out)
}

const ATTACHMENT_FIELDS: [&str; 10] = [
    "file_key",
    "sha256",
    "file_name",
    "media_type",
    "total_size",
    "width",
    "height",
    "caption",
    "page",
    "marks",
];

/// A model attachment reference as a body carries it: the fields of 9.1.1 alone, with a name and a type always.
pub fn attachment_to_wire(a: &Value) -> Result<Fields> {
    let Some(a) = a.as_object() else {
        return argument("an attachment is not an object");
    };
    let file_id = a.get("attachment_id").and_then(|v| wire_id(v, 16));
    let (Some(file_id), true, true) = (
        file_id,
        a.get("file_key").is_some_and(Value::is_string),
        a.get("sha256").is_some_and(Value::is_string),
    ) else {
        return argument("an attachment needs attachment_id, file_key and sha256");
    };
    let mut out = Fields::new();
    out.insert("file_id".into(), Value::String(file_id));
    out.insert("file_name".into(), "file".into());
    out.insert("media_type".into(), "application/octet-stream".into());
    out.insert("total_size".into(), 0.into());
    for f in ATTACHMENT_FIELDS {
        if let Some(v) = given(a.get(f)) {
            out.insert(f.into(), v.clone());
        }
    }
    if let Some(page) = out.get_mut("page") {
        *page = page_ref(page, wire_id_str);
    }
    if let Some(poster) = given(a.get("poster_attachment_id")) {
        let Some(id) = wire_id(poster, 16) else {
            return bad("a poster names no file");
        };
        out.insert("poster_file_id".into(), Value::String(id));
    }
    Ok(out)
}

fn list_from_wire(list: &Value) -> Result<Value> {
    let Some(list) = list.as_array() else {
        return bad("attachments is not a list");
    };
    list.iter()
        .map(|a| attachment_from_wire(a).map(Value::Object))
        .collect::<Result<_>>()
        .map(Value::Array)
}
fn list_to_wire(list: &Value) -> Result<Value> {
    let Some(list) = list.as_array() else {
        return bad("attachments is not a list");
    };
    list.iter()
        .map(|a| attachment_to_wire(a).map(Value::Object))
        .collect::<Result<_>>()
        .map(Value::Array)
}

// ---- small rules of single fields -------------------------------------------------------------------------------

/// JavaScript's white space (`String.prototype.trim`).
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}
/// A card's `teaser`: the two short lines the Desk row shows under the title. Plain text: not empty, already
/// trimmed, no control characters (no line breaks), at most [`TEASER_MAX`] characters.
pub fn teaser_valid(t: &Value) -> bool {
    t.as_str().is_some_and(teaser_text_valid)
}
fn teaser_text_valid(t: &str) -> bool {
    !t.is_empty()
        && t.trim_matches(js_space) == t
        && t.chars().count() <= TEASER_MAX
        && !t.chars().any(|c| c < '\u{20}' || c == '\u{7f}')
}
/// A message's `note` in the model: the note of the human it was sent from, `{ object_id: 32 hex, written_at: ms }`.
/// Nothing else in it, `written_at` a whole number of ms (or null).
pub fn note_ref_valid(v: &Value) -> bool {
    let Some(o) = v.as_object() else { return false };
    if o.keys().any(|k| k != "object_id" && k != "written_at") {
        return false;
    }
    let id_ok = o.get("object_id").and_then(Value::as_str).is_some_and(|s| {
        s.len() == 32
            && s.bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    });
    id_ok
        && given(o.get("written_at"))
            .is_none_or(|at| is_safe(at) && whole(at).is_some_and(|x| x >= 0.0))
}
/// Options as every client reads them: `final` only where it is true.
fn plain_options(options: &[Value]) -> Value {
    Value::Array(
        options
            .iter()
            .map(|o| match o.as_object() {
                Some(m) if m.get("final").is_some_and(|f| f != &Value::Bool(true)) => {
                    let mut m = m.clone();
                    m.remove("final");
                    Value::Object(m)
                }
                _ => o.clone(),
            })
            .collect(),
    )
}

// ---- registers (9.3) --------------------------------------------------------------------------------------------

/// Register names whose tail is one id of 16 bytes (spec/v1.md 9.3.3). A new per-card or per-session register goes
/// into this list and needs nothing else. Written: base64url, always. Read: base64url, and the 32 hex digits an
/// earlier iOS app wrote on the wire, which already are the model's key (it stays as it is).
const ID_NAMES: [&str; 13] = [
    "desk/", "session/", "draft/", "snooze/", "duck/", "star/", "knowledge/", "knowledge_log/", "knowledge_ask/", "knowledge_done/",
    "knowledge_seen/", "wakeup/", "wakeup_edit/",
];
const READ_SCOPES: [&str; 3] = ["read/session/", "read/card/", "read/desk/"];

fn map_name(
    name: &str,
    id: fn(&str, usize) -> Option<String>,
    snapshot_from: &str,
    snapshot_to: &str,
) -> String {
    let tail = |prefix: &str, n: usize, out: &str| -> Option<String> {
        let rest = name.strip_prefix(prefix)?;
        Some(id(rest, n).map_or_else(|| name.to_owned(), |got| format!("{out}{got}")))
    };
    ID_NAMES
        .iter()
        .chain(READ_SCOPES.iter())
        .find_map(|p| tail(p, 16, p))
        .or_else(|| tail("device/", 32, "device/"))
        .or_else(|| tail("alert/", 32, "alert/"))
        .or_else(|| tail(snapshot_from, 16, snapshot_to))
        .unwrap_or_else(|| name.to_owned())
}
/// A register's name on the wire -> its key in the model (`board_snapshot/<board>` is
/// `scribble_snapshot/desk/<board>`).
pub fn register_key_of(name: &str) -> String {
    map_name(
        name,
        hex_id_str,
        "board_snapshot/",
        "scribble_snapshot/desk/",
    )
}
/// A register's key in the model -> its name on the wire.
pub fn register_name_of(key: &str) -> String {
    map_name(
        key,
        wire_id_str,
        "scribble_snapshot/desk/",
        "board_snapshot/",
    )
}

fn with_id(value: &Fields, field: &str, n: usize, id: IdMap) -> Value {
    let mut out = value.clone();
    if let Some(v) = out.get_mut(field) {
        *v = id(v, n);
    }
    Value::Object(out)
}
fn crown_ids(v: &Value, id: IdMap) -> Value {
    let Some(o) = v.as_object() else {
        return v.clone();
    };
    let mut out = o.clone();
    if let Some(s) = out.get_mut("session_id") {
        *s = id(s, 16);
    }
    if let Some(a) = out.get_mut("agent_device_id") {
        *a = id(a, 32);
    }
    Value::Object(out)
}
fn frontier_ids(f: &Value, id: IdMap) -> Value {
    let Some(o) = f.as_object() else {
        return f.clone();
    };
    let mut out = Fields::new();
    for (k, v) in o {
        let key = match id(&Value::String(k.clone()), 32) {
            Value::String(s) => s,
            _ => k.clone(),
        };
        let head = match v {
            Value::Array(a) => Value::Array(vec![
                a.first().cloned().unwrap_or(Value::Null),
                id(a.get(1).unwrap_or(&Value::Null), 32),
            ]),
            other => other.clone(),
        };
        out.insert(key, head);
    }
    Value::Object(out)
}
/// The ids inside a register's value, one way or the other; the rest of the value is the app's and stays.
fn register_value(key: &str, value: &Value, to_model: bool) -> Result<Value> {
    let Some(o) = value.as_object() else {
        return Ok(value.clone());
    };
    let id: IdMap = if to_model { soft_hex } else { soft_wire };
    Ok(if key == "crown" {
        crown_ids(value, id)
    } else if key.starts_with("desk/") {
        match o.get("crown") {
            Some(c) => {
                let mut out = o.clone();
                out.insert("crown".into(), crown_ids(c, id));
                Value::Object(out)
            }
            None => value.clone(),
        }
    } else if key.starts_with("session/") || key.starts_with("knowledge/") {
        with_id(o, "desk", 16, id)
    } else if key.starts_with("knowledge_log/") {
        let Value::Object(o) = with_id(o, "desk", 16, id) else { return Ok(value.clone()) };
        with_id(&o, "entry", 16, id)
    } else if key.starts_with("knowledge_ask/") || key.starts_with("knowledge_done/") {
        with_id(o, "entry", 16, id)
    } else if key == "goals" {
        with_id(o, "desk_id", 16, id)
    } else if key.starts_with("status_line/") {
        with_id(o, "object_id", 16, id)
    } else if key.starts_with("alert/") {
        with_id(o, "sender_device_id", 32, id)
    } else if key.starts_with("scribble_snapshot/") {
        let null = Value::Null;
        let mut out = Fields::new();
        let frontier = o.get("frontier").map(|f| frontier_ids(f, id));
        if to_model {
            for (k, v) in o {
                if k != "change" && k != "frontier" {
                    out.insert(k.clone(), v.clone());
                }
            }
            out.insert(
                "attachment".into(),
                Value::Object(attachment_from_wire(o.get("attachment").unwrap_or(&null))?),
            );
            if let Some(f) = frontier {
                out.insert("frontier".into(), f);
            }
            out.insert(
                "last_envelope_number".into(),
                given(o.get("change")).cloned().unwrap_or(0.into()),
            );
        } else {
            out.insert(
                "attachment".into(),
                Value::Object(attachment_to_wire(o.get("attachment").unwrap_or(&null))?),
            );
            if let Some(f) = frontier {
                out.insert("frontier".into(), f);
            }
            let change = given(o.get("last_envelope_number")).or_else(|| given(o.get("change")));
            out.insert("change".into(), change.cloned().unwrap_or(0.into()));
        }
        Value::Object(out)
    } else {
        value.clone()
    })
}
/// A register's name and value as they came (9.3.1) -> its key in the model and the value in the model's form.
/// `bad-format` for a board snapshot's value that names no file.
pub fn register_from_wire(name: &str, value: &Value) -> Result<(String, Value)> {
    let key = register_key_of(name);
    let value = register_value(&key, value, true)?;
    Ok((key, value))
}
/// A register write as the core's draft takes it: the wire's name for the model's key, and the value as JSON
/// (`None` deletes). The core adds the lamport and makes the body.
pub fn encode_register(key: &str, value: &Value) -> Result<(String, Option<String>)> {
    if key.is_empty() {
        return argument("a register needs its key");
    }
    let name = register_name_of(key);
    if value.is_null() {
        return Ok((name, None));
    }
    let v = register_value(key, value, false)?;
    Ok((name, Some(v.to_string())))
}

// ---- board items (10.5) and stroke pieces (7.2) ------------------------------------------------------------------

fn utf8_len_in(v: Option<&Value>, min: usize, max: usize) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| (min..=max).contains(&s.len()))
}
fn points_ok(v: Option<&Value>) -> bool {
    v.and_then(Value::as_str)
        .is_some_and(|s| Ink::from_base64url(s).is_ok())
}
const SHAPE_FIELDS: [&str; 10] = [
    "color",
    "width",
    "points",
    "live",
    "at",
    "text",
    "size",
    "wrap",
    "rect",
    "attachment",
];
const PENS: [&str; 2] = ["pen", "marker"];
const NOTES: [&str; 3] = ["text", "sticky", "voice"];

/// The family of a shape: what its tool draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Pen,
    Note,
    Image,
}

impl Family {
    pub fn name(self) -> &'static str {
        match self {
            Self::Pen => "pen",
            Self::Note => "note",
            Self::Image => "image",
        }
    }
    fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Pen => &["color", "width", "points", "live"],
            Self::Note => &["at", "text", "size", "color", "wrap"],
            Self::Image => &["rect", "attachment"],
        }
    }
}

/// One shape as the wire carries it, checked as trommi-core checks it (with its `id` in a snapshot file, without
/// in a board item); its family.
pub fn check_wire_shape(e: &Value, with_id: bool) -> Result<Family> {
    let Some(e) = e.as_object() else {
        return bad("a shape is not an object");
    };
    let tool = e.get("tool").and_then(Value::as_str).unwrap_or("");
    let family = if PENS.contains(&tool) {
        Family::Pen
    } else if NOTES.contains(&tool) {
        Family::Note
    } else if tool == "image" && e.get("tool").is_some_and(Value::is_string) {
        Family::Image
    } else {
        return bad("an unknown tool");
    };
    if e.contains_key("id") != with_id {
        return bad(if with_id {
            "a shape without its id"
        } else {
            "a shape names an id"
        });
    }
    for f in SHAPE_FIELDS.iter().chain(["group", "id"].iter()) {
        if let Some(v) = e.get(*f) {
            if v.is_null() || (SHAPE_FIELDS.contains(f) && !family.fields().contains(f)) {
                return bad("a shape's field does not belong");
            }
        }
    }
    if e.get("z").is_some_and(|z| !is_i32(z)) {
        return bad("a layer is not a whole number");
    }
    if e.contains_key("group") && !utf8_len_in(e.get("group"), 1, 80) {
        return bad("a group name is out of range");
    }
    match family {
        Family::Pen => {
            if !utf8_len_in(e.get("color"), 1, 40)
                || !e.get("width").is_some_and(size_ok)
                || !points_ok(e.get("points"))
            {
                return bad("a stroke is not readable");
            }
            if e.get("live").is_some_and(|l| hex_id(l, 16).is_none()) {
                return bad("a live stroke id is not 16 bytes");
            }
        }
        Family::Note => {
            let wrap_ok = e
                .get("wrap")
                .is_none_or(|w| is_i32(w) && whole(w).is_some_and(|x| x > 0.0));
            if !e.get("at").is_some_and(|a| i32s(a, 2))
                || !utf8_len_in(e.get("text"), 0, 20_000)
                || !e.get("size").is_some_and(size_ok)
                || !utf8_len_in(e.get("color"), 1, 40)
                || !wrap_ok
            {
                return bad("a note is not readable");
            }
        }
        Family::Image => {
            let a = e.get("attachment").and_then(Value::as_object);
            let readable = e.get("rect").is_some_and(|r| i32s(r, 4))
                && a.is_some_and(|a| {
                    utf8_len_in(a.get("file_name"), 1, 255)
                        && utf8_len_in(a.get("media_type"), 1, 255)
                        && a.get("total_size")
                            .is_some_and(|t| is_safe(t) && whole(t).is_some_and(|x| x >= 0.0))
                });
            let Some(a) = a.filter(|_| readable) else {
                return bad("a picture is not readable");
            };
            if ["width", "height"]
                .iter()
                .any(|f| a.get(*f).is_some_and(|v| !is_u32(v)))
            {
                return bad("a picture size is not a whole number");
            }
            if a.get("file_key").and_then(|k| hex_id(k, 32)).is_none()
                || a.get("sha256").and_then(|s| hex_id(s, 32)).is_none()
            {
                return bad("a picture names no file");
            }
            attachment_from_wire(e.get("attachment").unwrap_or(&Value::Null))?;
        }
    }
    Ok(family)
}

/// A wire shape -> the model's entry (board units; a picture's own size, type and name beside its attachment).
pub fn entry_from_wire(e: &Value, with_id: bool) -> Result<Fields> {
    let family = check_wire_shape(e, with_id)?;
    let null = Value::Null;
    let get = |k: &str| e.get(k).unwrap_or(&null);
    let mut out = Fields::new();
    out.insert("tool".into(), get("tool").clone());
    match family {
        Family::Pen => {
            out.insert("color".into(), get("color").clone());
            out.insert("width".into(), units(get("width")));
            out.insert("points".into(), get("points").clone());
            if let Some(live) = e.get("live") {
                out.insert(
                    "live".into(),
                    hex_id(live, 16).map_or(Value::Null, Value::String),
                );
            }
        }
        Family::Note => {
            let at = get("at")
                .as_array()
                .map(|a| a.iter().map(units).collect())
                .unwrap_or_default();
            out.insert("at".into(), Value::Array(at));
            out.insert("text".into(), get("text").clone());
            out.insert("size".into(), units(get("size")));
            out.insert("color".into(), get("color").clone());
            if let Some(w) = e.get("wrap") {
                out.insert("wrap".into(), units(w));
            }
        }
        Family::Image => {
            let a = attachment_from_wire(get("attachment"))?;
            let rect = get("rect")
                .as_array()
                .map(|r| r.iter().map(units).collect())
                .unwrap_or_default();
            out.insert("rect".into(), Value::Array(rect));
            let or = |k: &str, d: Value| given(a.get(k)).cloned().unwrap_or(d);
            out.insert("nw".into(), or("width", 0.into()));
            out.insert("nh".into(), or("height", 0.into()));
            out.insert("mime".into(), or("media_type", Value::Null));
            out.insert("name".into(), or("file_name", Value::Null));
            out.insert("attachment".into(), Value::Object(a));
        }
    }
    if e.get("z").and_then(num).is_some_and(|z| z != 0.0) {
        out.insert("z".into(), get("z").clone());
    }
    if let Some(g) = e.get("group") {
        out.insert("group".into(), g.clone());
    }
    Ok(out)
}

/// JavaScript's `String(v)` for what a note's text may be.
pub(crate) fn js_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n
            .as_f64()
            .map_or_else(|| n.to_string(), |x| fnum(x).to_string()),
        Value::Array(a) => a
            .iter()
            .map(|x| {
                if x.is_null() {
                    String::new()
                } else {
                    js_string(x)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// A model entry -> the wire's shape; `bad-format` for what no reader would take.
pub fn entry_to_wire(e: &Value) -> Result<Fields> {
    let Some(e) = e.as_object() else {
        return bad("a shape is not an object");
    };
    let null = Value::Null;
    let tool = e.get("tool").unwrap_or(&null);
    let name = tool.as_str().unwrap_or("");
    let mut out = Fields::new();
    if e.contains_key("tool") {
        out.insert("tool".into(), tool.clone());
    }
    let marker = name == "marker";
    if PENS.contains(&name) {
        let color = given(e.get("color"))
            .cloned()
            .unwrap_or_else(|| (if marker { "yellow" } else { "ink" }).into());
        out.insert("color".into(), color);
        out.insert(
            "width".into(),
            length_q(e.get("width"), if marker { 18.0 } else { 4.0 }).into(),
        );
        if let Some(p) = e.get("points") {
            out.insert("points".into(), p.clone());
        }
        if let Some(live) = given(e.get("live")) {
            let Some(id) = wire_id(live, 16) else {
                return bad("a live stroke id is not 16 bytes");
            };
            out.insert("live".into(), Value::String(id));
        }
    } else if NOTES.contains(&name) {
        let at = e.get("at").and_then(Value::as_array);
        let at_i = |i: usize| quantum(at.and_then(|a| a.get(i)));
        out.insert(
            "at".into(),
            Value::Array(vec![at_i(0).into(), at_i(1).into()]),
        );
        out.insert(
            "text".into(),
            Value::String(given(e.get("text")).map_or_else(String::new, js_string)),
        );
        out.insert("size".into(), length_q(e.get("size"), 20.0).into());
        out.insert(
            "color".into(),
            given(e.get("color"))
                .cloned()
                .unwrap_or_else(|| "ink".into()),
        );
        if let Some(w) = given(e.get("wrap")) {
            out.insert("wrap".into(), length_q(Some(w), 1.0).into());
        }
    } else if name == "image" {
        let rect = e.get("rect").and_then(Value::as_array);
        let mut a = attachment_to_wire(e.get("attachment").unwrap_or(&null))?;
        for k in ["page", "caption", "marks"] {
            a.remove(k);
        }
        for (k, from) in [("width", "nw"), ("height", "nh")] {
            if let Some(x) = e.get(from).and_then(num).filter(|x| *x > 0.0) {
                a.insert(k.into(), fnum(js_round(x)));
            }
        }
        if let Some(m) = e
            .get("mime")
            .and_then(Value::as_str)
            .filter(|m| !m.is_empty())
        {
            a.insert("media_type".into(), m.into());
        }
        if let Some(n) = e
            .get("name")
            .and_then(Value::as_str)
            .filter(|n| !n.is_empty())
        {
            a.insert("file_name".into(), n.into());
        }
        let r: Vec<Value> = (0..4)
            .map(|i| quantum(rect.and_then(|r| r.get(i))).into())
            .collect();
        out.insert("rect".into(), Value::Array(r));
        out.insert("attachment".into(), Value::Object(a));
    }
    if let Some(z) = e
        .get("z")
        .and_then(num)
        .filter(|z| *z != 0.0 && !z.is_nan())
    {
        out.insert("z".into(), to_i32(js_round(z)).into());
    }
    if let Some(g) = e
        .get("group")
        .and_then(Value::as_str)
        .filter(|g| !g.is_empty())
    {
        out.insert("group".into(), g.into());
    }
    let out = Value::Object(out);
    check_wire_shape(&out, false)?;
    match out {
        Value::Object(m) => Ok(m),
        _ => bad("a shape is not an object"),
    }
}

fn unique(list: &[Value]) -> bool {
    let mut seen = std::collections::HashSet::with_capacity(list.len());
    list.iter().all(|v| seen.insert(v.to_string()))
}
fn ids_from_wire(list: Option<&Value>) -> Result<Value> {
    let Some(list) = list
        .and_then(Value::as_array)
        .filter(|l| !l.is_empty() && l.len() <= MAX_SHAPE_IDS && unique(l))
    else {
        return bad("shape ids: 1 to 2 000, none twice");
    };
    list.iter()
        .map(|id| {
            id.as_str()
                .and_then(shape_id_from_wire)
                .map(Value::String)
                .map_or_else(|| bad("a shape id is not readable"), Ok)
        })
        .collect::<Result<_>>()
        .map(Value::Array)
}
fn board_from_wire(b: &Fields) -> Result<Fields> {
    // (a field that stands is never null)
    let has = |f: &str| -> Result<bool> {
        match b.get(f) {
            Some(Value::Null) => bad("a field is null"),
            Some(_) => Ok(true),
            None => Ok(false),
        }
    };
    let ty = b.get("content_type").and_then(Value::as_str).unwrap_or("");
    let mut out = Fields::new();
    if ty == "strokes" && has("strokes")? && !has("shape_ids")? && !has("offset")? {
        let Some(list) = b
            .get("strokes")
            .and_then(Value::as_array)
            .filter(|l| !l.is_empty() && l.len() <= MAX_SHAPES)
        else {
            return bad("strokes: 1 to 1 000 shapes");
        };
        let strokes = list
            .iter()
            .map(|e| entry_from_wire(e, false).map(Value::Object))
            .collect::<Result<_>>()?;
        out.insert("content_type".into(), ty.into());
        out.insert("strokes".into(), Value::Array(strokes));
        return Ok(out);
    }
    if (ty == "erase" || ty == "send_away") && !has("strokes")? && !has("offset")? {
        out.insert("content_type".into(), ty.into());
        out.insert("stroke_ids".into(), ids_from_wire(b.get("shape_ids"))?);
        return Ok(out);
    }
    if ty == "move" && !has("strokes")? && has("offset")? {
        let offset = b.get("offset").filter(|o| i32s(o, 2));
        let Some(offset) = offset.and_then(Value::as_array) else {
            return bad("an offset is two whole numbers");
        };
        out.insert("content_type".into(), ty.into());
        out.insert("stroke_ids".into(), ids_from_wire(b.get("shape_ids"))?);
        out.insert(
            "offset".into(),
            Value::Array(offset.iter().map(units).collect()),
        );
        return Ok(out);
    }
    bad("not a board item")
}
fn board_to_wire(c: &Fields) -> Result<Fields> {
    let ty = c.get("content_type").and_then(Value::as_str).unwrap_or("");
    let ids = || -> Result<Value> {
        let Some(list) = c.get("stroke_ids").and_then(Value::as_array) else {
            return bad("stroke_ids is not a list");
        };
        let list: Vec<Value> = list
            .iter()
            .map(|id| {
                id.as_str()
                    .and_then(shape_id_to_wire)
                    .map(Value::String)
                    .map_or_else(|| bad("a shape id is not readable"), Ok)
            })
            .collect::<Result<_>>()?;
        if list.is_empty() || list.len() > MAX_SHAPE_IDS || !unique(&list) {
            return bad("shape ids: 1 to 2 000, none twice");
        }
        Ok(Value::Array(list))
    };
    let mut out = Fields::new();
    match ty {
        "strokes" => {
            let list = c
                .get("strokes")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            if list.is_empty() || list.len() > MAX_SHAPES {
                return bad("strokes: 1 to 1 000 shapes");
            }
            let strokes = list
                .iter()
                .map(|e| entry_to_wire(e).map(Value::Object))
                .collect::<Result<_>>()?;
            out.insert("strokes".into(), Value::Array(strokes));
        }
        "erase" | "send_away" => {
            out.insert("shape_ids".into(), ids()?);
        }
        "move" => {
            let o = c.get("offset").and_then(Value::as_array);
            out.insert("shape_ids".into(), ids()?);
            let at = |i: usize| Value::from(quantum(o.and_then(|o| o.get(i))));
            out.insert("offset".into(), Value::Array(vec![at(0), at(1)]));
        }
        _ => {
            let shown = c
                .get("content_type")
                .map_or_else(|| "undefined".to_owned(), js_string);
            return argument(&format!("unknown content_type {shown}"));
        }
    }
    out.insert("content_type".into(), ty.into());
    Ok(out)
}

/// The `piece` of a stroke piece message -> the model's form (`content_type: stroke_piece`, the stroke's id in hex,
/// the width in board units), or `None` for one trommi-core would refuse.
pub fn decode_piece(json: &[u8]) -> Option<Fields> {
    if json.len() > MAX_PIECE_LEN {
        return None;
    }
    let p: Value = serde_json::from_slice(json).ok()?;
    let p = p.as_object()?;
    let stroke = p.get("stroke").and_then(|s| hex_id(s, 16))?;
    let number_ok = p
        .get("number")
        .is_some_and(|n| is_u32(n) && whole(n).is_some_and(|x| x >= 1.0));
    let tool = p
        .get("tool")
        .and_then(Value::as_str)
        .filter(|t| PENS.contains(t))?;
    let width = p.get("width").filter(|w| size_ok(w))?;
    if !number_ok
        || !utf8_len_in(p.get("color"), 1, 40)
        || !points_ok(p.get("points"))
        || StrokePiece::decode(json).is_err()
    {
        return None;
    }
    let mut out = Fields::new();
    out.insert("content_type".into(), "stroke_piece".into());
    out.insert("stroke".into(), Value::String(stroke));
    out.insert("number".into(), p.get("number")?.clone());
    out.insert("tool".into(), tool.into());
    out.insert("color".into(), p.get("color")?.clone());
    out.insert("width".into(), units(width));
    out.insert("points".into(), p.get("points")?.clone());
    Some(out)
}
/// The model's piece (`stroke` in hex, `number`, `tool`, `color?`, `width?` in board units, `points`) -> the `piece`
/// text a stroke piece message carries; `bad-format` for one out of range.
pub fn encode_piece(p: &Value) -> Result<String> {
    let null = Value::Null;
    let get = |k: &str| p.get(k).unwrap_or(&null);
    let Some(stroke) = wire_id(get("stroke"), 16) else {
        return bad("a live stroke id is not 16 bytes");
    };
    let marker = get("tool").as_str() == Some("marker");
    let mut out = Fields::new();
    out.insert("stroke".into(), Value::String(stroke));
    out.insert("number".into(), get("number").clone());
    out.insert("tool".into(), get("tool").clone());
    out.insert(
        "color".into(),
        given(p.get("color"))
            .cloned()
            .unwrap_or_else(|| (if marker { "yellow" } else { "ink" }).into()),
    );
    out.insert(
        "width".into(),
        length_q(p.get("width"), if marker { 18.0 } else { 4.0 }).into(),
    );
    out.insert("points".into(), get("points").clone());
    let json = Value::Object(out).to_string();
    if decode_piece(json.as_bytes()).is_none() {
        return bad("a stroke piece is out of range");
    }
    Ok(json)
}

// ---- the bodies -------------------------------------------------------------------------------------------------
//
// Reading is on every envelope that arrives, so a body is read without building it whole: its top level is taken
// as its keys and the raw JSON text of each value (checked, not built), only the fields this codec converts are
// built and replaced, and the text is written again from those pieces. A body keeps its own order of keys; a key
// named twice stands where it first stood, with the value it was given last (as JavaScript reads it). Board items
// and registers, which are small or read strictly, are built whole.

/// A key of the body's top level, borrowed from the payload where it holds no escape.
struct Key<'a>(Cow<'a, str>);

impl<'de> Deserialize<'de> for Key<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Text;
        impl<'de> Visitor<'de> for Text {
            type Value = Key<'de>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a key")
            }
            fn visit_borrowed_str<E: de::Error>(
                self,
                text: &'de str,
            ) -> std::result::Result<Key<'de>, E> {
                Ok(Key(Cow::Borrowed(text)))
            }
            fn visit_str<E: de::Error>(self, text: &str) -> std::result::Result<Key<'de>, E> {
                Ok(Key(Cow::Owned(text.to_owned())))
            }
        }
        deserializer.deserialize_str(Text)
    }
}

/// A value of the top level: as it stands in the payload, or made here.
enum Piece<'a> {
    Raw(&'a RawValue),
    Made(Value),
    /// Made here, already written as JSON text.
    Text(String),
}

impl Piece<'_> {
    fn value(&self) -> Value {
        match self {
            // (the text was checked when the body was read: it is JSON)
            Piece::Raw(raw) => serde_json::from_str(raw.get()).unwrap_or(Value::Null),
            Piece::Text(text) => serde_json::from_str(text).unwrap_or(Value::Null),
            Piece::Made(v) => v.clone(),
        }
    }
    fn text(&self) -> Cow<'_, str> {
        match self {
            Piece::Raw(raw) => Cow::Borrowed(raw.get()),
            Piece::Text(text) => Cow::Borrowed(text),
            Piece::Made(v) => Cow::Owned(v.to_string()),
        }
    }
    fn is_null(&self) -> bool {
        self.text() == "null"
    }
    fn is_array(&self) -> bool {
        self.text().starts_with('[')
    }
    /// A string that stands without escapes, as it stands (no value is built).
    fn plain_str(&self) -> Option<&str> {
        match self {
            Piece::Raw(raw) => {
                let t = raw.get();
                let inner = t.strip_prefix('"')?.strip_suffix('"')?;
                (!inner.contains('\\')).then_some(inner)
            }
            Piece::Made(Value::String(s)) => Some(s),
            Piece::Made(_) | Piece::Text(_) => None,
        }
    }
    /// The id of `n` bytes this value names (base64url), as hex.
    fn hex_id(&self, n: usize) -> Option<String> {
        match self.plain_str() {
            Some(s) => hex_id_str(s, n),
            None => hex_id(&self.value(), n),
        }
    }
}

/// The top level of a body: its keys in order, each with its value.
struct Body<'a>(Vec<(Cow<'a, str>, Piece<'a>)>);

impl<'de> Deserialize<'de> for Body<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Body<'de>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Body<'de>, A::Error> {
                let mut body = Body(Vec::with_capacity(map.size_hint().unwrap_or(8)));
                while let Some(Key(key)) = map.next_key::<Key<'de>>()? {
                    let value: &'de RawValue = map.next_value()?;
                    body.put_piece(key, Piece::Raw(value));
                }
                Ok(body)
            }
        }
        deserializer.deserialize_map(Object)
    }
}

impl<'a> Body<'a> {
    fn at(&self, key: &str) -> Option<usize> {
        self.0.iter().position(|(k, _)| k == key)
    }
    fn get(&self, key: &str) -> Option<&Piece<'a>> {
        self.at(key).map(|i| &self.0[i].1)
    }
    /// The value of `key` when it stands and is not null.
    fn given(&self, key: &str) -> Option<Value> {
        self.get(key).filter(|p| !p.is_null()).map(Piece::value)
    }
    fn put_piece(&mut self, key: Cow<'a, str>, piece: Piece<'a>) {
        match self.at(&key) {
            Some(i) => self.0[i].1 = piece,
            None => self.0.push((key, piece)),
        }
    }
    /// Sets `key`: in its place where it stands, else at the end.
    fn put(&mut self, key: &'static str, value: Value) {
        self.put_piece(Cow::Borrowed(key), Piece::Made(value));
    }
    /// Sets `key` to a value already written as JSON text.
    fn put_text(&mut self, key: &'static str, text: String) {
        self.put_piece(Cow::Borrowed(key), Piece::Text(text));
    }
    fn remove(&mut self, key: &str) -> Option<Piece<'a>> {
        self.at(key).map(|i| self.0.remove(i).1)
    }
    fn write(&self, len: usize) -> String {
        let mut out = String::with_capacity(len + 64);
        out.push('{');
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            if k.bytes().all(|c| c >= 0x20 && c != b'"' && c != b'\\') {
                out.push('"');
                out.push_str(k);
                out.push('"');
            } else {
                out.push_str(&Value::String(k.to_string()).to_string());
            }
            out.push(':');
            out.push_str(&v.text());
        }
        out.push('}');
        out
    }
}

/// Options (their JSON text) as every client reads them, `final` only where it is true, written again from their
/// own text; none for a text that is no list.
fn plain_options_text(text: &str) -> Option<String> {
    let list: Vec<&RawValue> = serde_json::from_str(text).ok()?;
    let mut out = String::with_capacity(text.len());
    out.push('[');
    for (i, option) in list.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        match serde_json::from_str::<Body>(option.get()) {
            Ok(mut o) => {
                if o.get("final").is_some_and(|f| f.text() != "true") {
                    o.remove("final");
                }
                out.push_str(&o.write(option.get().len()));
            }
            Err(_) => out.push_str(option.get()),
        }
    }
    out.push(']');
    Some(out)
}

/// Whether a body's `schema_version` is one this client reads: `Err` names why not.
fn version_ok(version: Option<&Value>) -> std::result::Result<(), Decoded<String>> {
    match version {
        None => Ok(()),
        Some(v) => match whole(v) {
            Some(x) if x > SCHEMA_VERSION as f64 => Err(Decoded::NewerSchema),
            Some(x) if x == SCHEMA_VERSION as f64 => Ok(()),
            _ => Err(Decoded::Bad),
        },
    }
}

/// The ids and renamed fields of a body that is no board item and no register, wire -> model.
fn fields_from_wire(kind: BodyKind, out: &mut Body<'_>) -> Result<()> {
    match out.get("attachments") {
        // (an empty list stays as it is)
        Some(list) if list.text() == "[]" => {}
        Some(list) if list.is_array() => {
            let list = list_from_wire(&list.value())?;
            out.put("attachments", list);
        }
        _ => {
            out.remove("attachments");
        }
    }
    if kind == BodyKind::Message {
        match out.get("content_type") {
            None => out.put("content_type", "message".into()),
            Some(p) if p.is_null() => out.put("content_type", "message".into()),
            // (a content type of a newer client is kept, and shown as one; anything else is no message)
            Some(p) if !p.text().starts_with('"') => return bad("content_type is not a text"),
            Some(_) => {}
        }
        // (the model's name for the wire's artifact_object_id: never read from the wire itself)
        out.remove("published_object_id");
        if let Some(cards) = out.get("copied_cards") {
            let cards = id_list(&cards.value(), soft_hex);
            out.put("copied_cards", cards);
        }
        if let Some(artifact) = out.remove("artifact_object_id") {
            if let Some(id) = artifact.hex_id(16) {
                out.put("published_object_id", Value::String(id));
            }
        }
        if let Some(note) = out.get("note") {
            // a bad note mark: the message stays, plain
            let note = match note.value() {
                Value::Object(mut n) => {
                    let id = n.get("object_id").and_then(|v| hex_id(v, 16));
                    n.insert("object_id".into(), id.map_or(Value::Null, Value::String));
                    Some(Value::Object(n)).filter(note_ref_valid)
                }
                _ => None,
            };
            match note {
                Some(n) => out.put("note", n),
                None => {
                    out.remove("note");
                }
            }
        }
    }
    if matches!(kind, BodyKind::Card | BodyKind::Artifact | BodyKind::Note) {
        if let Some(prev) = out.get("previous_version_hash").filter(|p| !p.is_null()) {
            let Some(h) = prev.hex_id(32) else {
                return bad("previous_version_hash is not a hash");
            };
            out.put("previous_version_hash", Value::String(h));
        }
    }
    if kind == BodyKind::Card {
        if let Some(v) = out.given("merged_into_object_id") {
            out.put("merged_into_object_id", soft_hex(&v, 16));
        }
        if let Some(v) = out.given("merged_from_object_ids") {
            out.put("merged_from_object_ids", id_list(&v, soft_hex));
        }
        let teaser_bad = match out.get("teaser").filter(|t| !t.is_null()) {
            Some(t) => match t.plain_str() {
                Some(text) => !teaser_text_valid(text),
                None => !teaser_valid(&t.value()),
            },
            None => false,
        };
        if teaser_bad {
            out.remove("teaser"); // a bad teaser: the Desk falls back to the body
        }
        // (options in which `final` stands nowhere stay as they are)
        let options = out.get("options").filter(|o| o.is_array());
        let may_change = options.is_some_and(|o| {
            let t = o.text();
            t.contains('\\') || t.contains("\"final\"")
        });
        if may_change {
            let plain = options.and_then(|o| plain_options_text(&o.text()));
            match plain {
                Some(text) => out.put_text("options", text),
                None => return bad("options are not readable"),
            }
        }
    }
    if kind == BodyKind::Artifact {
        if let Some(until) = out.get("shared_until").map(Piece::value) {
            out.put("released_until", until);
            out.remove("shared_until");
        }
    }
    Ok(())
}

/// A board item or a register value, built whole, wire -> model.
fn whole_from_wire(kind: BodyKind, b: &Fields) -> Result<Fields> {
    if kind == BodyKind::BoardItem {
        return board_from_wire(b);
    }
    let Some(name) = b.get("name").and_then(Value::as_str) else {
        return bad("a register without a name");
    };
    let (key, value) = register_from_wire(name, b.get("value").unwrap_or(&Value::Null))?;
    let lamport = b
        .get("lamport")
        .filter(|l| l.is_number())
        .cloned()
        .unwrap_or(0.into());
    let mut out = Fields::new();
    out.insert("name".into(), name.into());
    out.insert("key".into(), Value::String(key));
    out.insert("value".into(), value);
    out.insert("lamport".into(), lamport);
    Ok(out)
}

/// A payload's text: UTF-8, a byte order mark read past (as the web's TextDecoder does).
fn payload_text(payload: &[u8]) -> Option<&str> {
    std::str::from_utf8(payload.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(payload)).ok()
}

/// A payload as one JSON object of a version this client reads, built whole.
fn parse(payload: &[u8]) -> std::result::Result<Fields, Decoded<String>> {
    let Some(Ok(Value::Object(parsed))) = payload_text(payload).map(serde_json::from_str::<Value>)
    else {
        return Err(Decoded::Bad);
    };
    version_ok(parsed.get("schema_version"))?;
    Ok(parsed)
}

/// A body's payload (the JSON the core opened) as the model's fields, as JSON text: what the apps take across the
/// facade. [`Decoded::NewerSchema`] for a `schema_version` above 2, [`Decoded::Bad`] for what is no body of its
/// kind. A register gives `{ name, key, value, lamport }`: `key` is the model's key for the wire's `name`.
pub fn decode_body_text(kind: BodyKind, payload: &[u8]) -> Decoded<String> {
    if matches!(kind, BodyKind::BoardItem | BodyKind::Register) {
        return match decode_whole(kind, payload) {
            Decoded::Body(m) => Decoded::Body(Value::Object(m).to_string()),
            Decoded::NewerSchema => Decoded::NewerSchema,
            Decoded::Bad => Decoded::Bad,
        };
    }
    let Some(Ok(mut body)) = payload_text(payload).map(serde_json::from_str::<Body>) else {
        return Decoded::Bad;
    };
    // (a number beyond f64 is kept as written by the raw pieces: read it whole as the fields path does, and refuse
    // it as that path does; only a payload that holds an exponent or a long run of digits is read twice)
    if numbers_doubtful(payload) && serde_json::from_slice::<Value>(payload).is_err() {
        return Decoded::Bad;
    }
    let version = body.get("schema_version").filter(|v| v.text() != "2");
    if let Err(not) = version_ok(version.map(Piece::value).as_ref()) {
        return not;
    }
    match fields_from_wire(kind, &mut body) {
        Ok(()) => Decoded::Body(body.write(payload.len())),
        Err(_) => Decoded::Bad,
    }
}

/// Whether a payload may hold a number serde_json cannot read into an f64: a digit with an exponent after it, or a
/// run of more than 300 digits. (Strings may trigger it too: they only cost a second read.)
fn numbers_doubtful(payload: &[u8]) -> bool {
    let mut run = 0usize;
    for (i, b) in payload.iter().enumerate() {
        if b.is_ascii_digit() {
            run += 1;
            if run > 300 {
                return true;
            }
        } else {
            if run > 0 && (*b == b'e' || *b == b'E') && payload.get(i + 1).is_some_and(|n| n.is_ascii_digit() || *n == b'+' || *n == b'-') {
                return true;
            }
            run = 0;
        }
    }
    false
}

fn decode_whole(kind: BodyKind, payload: &[u8]) -> Decoded {
    let parsed = match parse(payload) {
        Ok(p) => p,
        Err(Decoded::NewerSchema) => return Decoded::NewerSchema,
        Err(_) => return Decoded::Bad,
    };
    if kind == BodyKind::BoardItem && ItemBody::decode(payload).is_err() {
        return Decoded::Bad;
    }
    whole_from_wire(kind, &parsed).map_or(Decoded::Bad, Decoded::Body)
}

/// A body's payload (the JSON the core opened) as the model's fields; [`Decoded::NewerSchema`] for a
/// `schema_version` above 2, [`Decoded::Bad`] for what is no body of its kind. A register gives
/// `{ name, key, value, lamport }`: `key` is the model's key for the wire's `name`.
pub fn decode_body(kind: BodyKind, payload: &[u8]) -> Decoded {
    if matches!(kind, BodyKind::BoardItem | BodyKind::Register) {
        return decode_whole(kind, payload);
    }
    match decode_body_text(kind, payload) {
        Decoded::Body(text) => serde_json::from_str(&text).map_or(Decoded::Bad, Decoded::Body),
        Decoded::NewerSchema => Decoded::NewerSchema,
        Decoded::Bad => Decoded::Bad,
    }
}

/// A board item's payload in the wire's own form, checked as [`decode_body`] checks it: what a board's reducer
/// takes.
pub fn parse_board_item(payload: &[u8]) -> Decoded {
    let parsed = match parse(payload) {
        Ok(p) => p,
        Err(Decoded::NewerSchema) => return Decoded::NewerSchema,
        Err(_) => return Decoded::Bad,
    };
    if ItemBody::decode(payload).is_err() || board_from_wire(&parsed).is_err() {
        return Decoded::Bad;
    }
    Decoded::Body(parsed)
}

/// The ids and renamed fields of a body, model -> wire.
fn to_wire(kind: BodyKind, f: &Fields) -> Result<Fields> {
    match kind {
        BodyKind::BoardItem => return board_to_wire(f),
        BodyKind::Register => {
            return argument("a register is written with encode_register: the core makes its body")
        }
        _ => {}
    }
    let mut out = Fields::new();
    if kind == BodyKind::Note {
        // a Note's fields are the app's
        for (k, v) in f {
            if k != "schema_version" {
                out.insert(k.clone(), v.clone());
            }
        }
    } else {
        for name in kind.fields() {
            if let Some(v) = f.get(*name) {
                out.insert((*name).into(), v.clone());
            }
        }
        if kind == BodyKind::Message && !f.contains_key("content_type") {
            out.insert("content_type".into(), "message".into());
        }
    }
    if let Some(list) = given(out.get("attachments")) {
        let list = list_to_wire(list)?;
        out.insert("attachments".into(), list);
    }
    if kind == BodyKind::Message {
        let ct = out.get("content_type").cloned().unwrap_or(Value::Null);
        if ct.as_str() != Some("message") {
            let shown = if ct.is_null() {
                "null".to_owned()
            } else {
                js_string(&ct)
            };
            return argument(&format!(
                "a Chat message has content_type message, not {shown}"
            ));
        }
        if out.get("terminal").and_then(Value::as_str) == Some("work") {
            return argument("a work trail is no stored message (spec 7.3)");
        }
        if let Some(v) = out.get_mut("copied_cards").filter(|v| !v.is_null()) {
            *v = id_list(v, soft_wire);
        }
        if let Some(p) = given(out.get("published_object_id")) {
            let Some(id) = wire_id(p, 16) else {
                return bad("published_object_id is not an object id");
            };
            out.remove("published_object_id");
            out.insert("artifact_object_id".into(), Value::String(id));
        }
        if let Some(note) = out.get("note") {
            if !note_ref_valid(note) {
                return argument("note must be { object_id: 32 hex, written_at?: ms }");
            }
            let mut n = note.as_object().cloned().unwrap_or_default();
            let id = n
                .get("object_id")
                .and_then(|v| wire_id(v, 16))
                .map_or(Value::Null, Value::String);
            n.insert("object_id".into(), id);
            out.insert("note".into(), Value::Object(n));
        }
    }
    if matches!(kind, BodyKind::Card | BodyKind::Artifact | BodyKind::Note) {
        if let Some(prev) = given(out.get("previous_version_hash")) {
            let Some(h) = wire_id(prev, 32) else {
                return bad("previous_version_hash is not a hash");
            };
            out.insert("previous_version_hash".into(), Value::String(h));
        }
    }
    if kind == BodyKind::Card {
        if given(out.get("teaser")).is_some_and(|t| !teaser_valid(t)) {
            return argument(&format!(
                "teaser must be plain one-paragraph text, trimmed, at most {TEASER_MAX} characters"
            ));
        }
        if let Some(Value::Array(options)) = out.get("options") {
            let plain = plain_options(options);
            out.insert("options".into(), plain);
        }
        if let Some(v) = out
            .get_mut("merged_into_object_id")
            .filter(|v| !v.is_null())
        {
            *v = soft_wire(v, 16);
        }
        if let Some(v) = out
            .get_mut("merged_from_object_ids")
            .filter(|v| !v.is_null())
        {
            *v = id_list(v, soft_wire);
        }
    }
    if kind == BodyKind::Artifact {
        if let Some(until) = out.remove("released_until") {
            out.insert("shared_until".into(), until);
        }
    }
    Ok(out)
}

/// A draft's fields (the model's names) as the payload the core seals: one JSON object with `schema_version` 2.
/// [`BodyError`] for fields no reader would take.
pub fn encode_body(kind: BodyKind, fields: &Fields) -> Result<String> {
    let mut out = to_wire(kind, fields)?;
    out.insert("schema_version".into(), SCHEMA_VERSION.into());
    Ok(Value::Object(out).to_string())
}

/// The files a body names (the model's fields), as sealing takes them for the envelope's header: none twice, in
/// the order they stand. A board item's pictures and a register value's attachment count.
pub fn file_ids_of(kind: BodyKind, fields: &Fields) -> Vec<[u8; 16]> {
    let mut out: Vec<[u8; 16]> = Vec::new();
    let mut add = |a: Option<&Value>| {
        let Some(a) = a.and_then(Value::as_object) else {
            return;
        };
        for f in ["attachment_id", "poster_attachment_id"] {
            let id = a
                .get(f)
                .and_then(Value::as_str)
                .filter(|s| s.len() == 32)
                .and_then(unhex);
            if let Some(Ok(id)) = id.map(<[u8; 16]>::try_from) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    };
    for a in fields
        .get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        add(Some(a));
    }
    if kind == BodyKind::BoardItem {
        for e in fields
            .get("strokes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            add(e.get("attachment"));
        }
    }
    if kind == BodyKind::Register {
        add(fields.get("value").and_then(|v| v.get("attachment")));
    }
    out
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
