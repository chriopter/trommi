//! An answer's JSON, read strictly and within bounds, and the checks every route's reader is made of.
//!
//! The hub is not trusted, so the text is not read into whatever it describes:
//!
//! - **Size first.** A body over the route's cap is refused before a byte of it is parsed.
//! - **A bound on what is built.** A body may hold one value per 8 bytes and 64 more (a member's name counts as
//!   one): the densest the hub really sends is a row of the open list, a value per 10 bytes, and a body made of
//!   `0,0,0,…` cannot make the parser build many times its own size.
//! - **One member per name.** An object that names a member twice is refused: two readers could take different ones.
//! - **Whole numbers only**, from 0 to 2^53 − 1 (what every number of the protocol's JSON is, and what JavaScript
//!   holds exactly). A fraction, a negative number, an exponent form or anything larger is no number here.
//! - **Byte strings in their one spelling**: base64url without padding and without a set bit behind the last byte.
//! - Nesting deeper than serde_json's limit (128), text that is not UTF-8 and bytes after the value are refused.
//!
//! A member this client does not know is ignored (a later hub may add one); a member it reads must be exactly what
//! the route promises. Every failure is [`Failure::BadAnswer`] with a fixed text that names the place and never
//! what stood there.

use super::Failure;
use crate::app::bytes::Bytes;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::cell::Cell;
use trommi_core::ids::{base64url_decode, base64url_len, GroupId};

pub(crate) type Read<T> = Result<T, Failure>;

/// The largest number an answer may hold: 2^53 − 1.
pub(crate) const MAX_NUMBER: u64 = (1 << 53) - 1;

pub(crate) fn bad<T>(why: &'static str) -> Read<T> {
    Err(Failure::BadAnswer { why })
}

#[derive(Debug)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    /// A whole number from 0 to [`MAX_NUMBER`].
    Int(u64),
    /// Any other number: never accepted.
    OtherNumber,
    Text(String),
    List(Vec<Json>),
    Map(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

struct Seed<'a> {
    left: &'a Cell<usize>,
}

impl Seed<'_> {
    fn spend<E: de::Error>(&self) -> Result<(), E> {
        match self.left.get().checked_sub(1) {
            Some(left) => {
                self.left.set(left);
                Ok(())
            }
            None => Err(E::custom("more values than an answer of this size holds")),
        }
    }
}

impl<'de> DeserializeSeed<'de> for Seed<'_> {
    type Value = Json;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<Json, D::Error> {
        d.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Seed<'_> {
    type Value = Json;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Json, E> {
        self.spend()?;
        Ok(Json::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Json, E> {
        self.spend()?;
        Ok(Json::Bool(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Json, E> {
        self.spend()?;
        Ok(if v <= MAX_NUMBER {
            Json::Int(v)
        } else {
            Json::OtherNumber
        })
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Json, E> {
        match u64::try_from(v) {
            Ok(v) => self.visit_u64(v),
            Err(_) => {
                self.spend()?;
                Ok(Json::OtherNumber)
            }
        }
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Json, E> {
        self.spend()?;
        Ok(Json::OtherNumber)
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Json, E> {
        self.spend()?;
        Ok(Json::Text(v.to_owned()))
    }
    fn visit_string<E: de::Error>(self, v: String) -> Result<Json, E> {
        self.spend()?;
        Ok(Json::Text(v))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Json, A::Error> {
        self.spend()?;
        let mut out = Vec::new();
        while let Some(item) = seq.next_element_seed(Seed { left: self.left })? {
            out.push(item);
        }
        Ok(Json::List(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Json, A::Error> {
        self.spend()?;
        let mut out: Vec<(String, Json)> = Vec::new();
        while let Some(name) = map.next_key::<String>()? {
            self.spend()?;
            if out.iter().any(|(n, _)| *n == name) {
                return Err(de::Error::custom("a member named twice"));
            }
            let value = map.next_value_seed(Seed { left: self.left })?;
            out.push((name, value));
        }
        Ok(Json::Map(out))
    }
}

/// How many members an object may have before its names are compared pairwise no more: no object of the protocol
/// has more than the 1024 devices of a claim's `key_packages`.
const MAX_MEMBERS: usize = 4096;

impl Json {
    /// Reads `body` as one JSON value: at most `cap` bytes, within the bounds above.
    pub(crate) fn parse(body: &[u8], cap: usize) -> Read<Json> {
        if body.len() > cap {
            return bad("an answer larger than its route allows");
        }
        let left = Cell::new(body.len() / 8 + 64);
        let mut de = serde_json::Deserializer::from_slice(body);
        let value = match (Seed { left: &left }).deserialize(&mut de) {
            Ok(value) => value,
            Err(_) => return bad("an answer that is not the JSON of this protocol"),
        };
        if de.end().is_err() {
            return bad("an answer with bytes behind its JSON");
        }
        if !value.members_bounded() {
            return bad("an object with more members than any of the protocol");
        }
        Ok(value)
    }

    fn members_bounded(&self) -> bool {
        match self {
            Json::List(items) => items.iter().all(Json::members_bounded),
            Json::Map(members) => {
                members.len() <= MAX_MEMBERS && members.iter().all(|(_, v)| v.members_bounded())
            }
            _ => true,
        }
    }

    pub(crate) fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    pub(crate) fn obj(&self, why: &'static str) -> Read<Obj<'_>> {
        match self {
            Json::Map(members) => Ok(Obj(members)),
            _ => bad(why),
        }
    }

    /// A list of at most `max` values.
    pub(crate) fn list(&self, max: usize, why: &'static str) -> Read<&[Json]> {
        match self {
            Json::List(items) if items.len() <= max => Ok(items),
            _ => bad(why),
        }
    }

    pub(crate) fn int(&self, why: &'static str) -> Read<u64> {
        match self {
            Json::Int(v) => Ok(*v),
            _ => bad(why),
        }
    }

    /// A whole number of at least 1 (a change number, a log number, a chain number).
    pub(crate) fn count(&self, why: &'static str) -> Read<u64> {
        match self.int(why)? {
            0 => bad(why),
            v => Ok(v),
        }
    }

    pub(crate) fn bool(&self, why: &'static str) -> Read<bool> {
        match self {
            Json::Bool(v) => Ok(*v),
            _ => bad(why),
        }
    }

    /// A text of at most `max` bytes.
    pub(crate) fn text(&self, max: usize, why: &'static str) -> Read<&str> {
        match self {
            Json::Text(v) if v.len() <= max => Ok(v),
            _ => bad(why),
        }
    }

    /// A byte string of `min` to `max` bytes, in its one base64url spelling.
    pub(crate) fn bytes(&self, min: usize, max: usize, why: &'static str) -> Read<Bytes> {
        let text = match self {
            Json::Text(v) if v.len() <= base64url_len(max) => v,
            _ => return bad(why),
        };
        match base64url_decode(text) {
            Ok(bytes) if bytes.len() >= min && bytes.len() <= max => Ok(Bytes(bytes)),
            _ => bad(why),
        }
    }

    /// An id of the protocol core: bytes of its one length.
    pub(crate) fn id<T, E>(&self, from: fn(&[u8]) -> Result<T, E>, why: &'static str) -> Read<T> {
        let bytes = self.bytes(1, 48, why)?;
        from(&bytes).or(bad(why))
    }

    /// A group id: 32 bytes (the room group) or 48 (a session group).
    pub(crate) fn group(&self, why: &'static str) -> Read<GroupId> {
        self.id(GroupId::from_bytes, why)
    }

    /// One of `allowed`.
    pub(crate) fn one_of<T: Copy>(&self, allowed: &[(&str, T)], why: &'static str) -> Read<T> {
        let text = self.text(64, why)?;
        match allowed.iter().find(|(name, _)| *name == text) {
            Some((_, value)) => Ok(*value),
            None => bad(why),
        }
    }
}

/// An object's members.
#[derive(Clone, Copy)]
pub(crate) struct Obj<'a>(&'a [(String, Json)]);

impl<'a> Obj<'a> {
    /// The member, or null when the object has none of this name: a reader that needs it then fails on its type.
    pub(crate) fn need(&self, name: &str) -> &'a Json {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&NULL, |(_, v)| v)
    }

    /// The member when it is there and not null.
    pub(crate) fn maybe(&self, name: &str) -> Option<&'a Json> {
        Some(self.need(name)).filter(|v| !v.is_null())
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.0.iter().any(|(n, _)| n == name)
    }

    pub(crate) fn members(&self) -> &'a [(String, Json)] {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Read<Json> {
        Json::parse(text.as_bytes(), 1 << 20)
    }

    #[test]
    fn numbers_are_whole_and_below_2_53() {
        let v =
            parse(r#"{"a":9007199254740991,"b":9007199254740992,"c":-1,"d":1.0,"e":1e3,"f":0}"#)
                .unwrap();
        let o = v.obj("o").unwrap();
        assert_eq!(o.need("a").int("a"), Ok(MAX_NUMBER));
        assert_eq!(o.need("f").int("f"), Ok(0));
        assert!(o.need("f").count("f").is_err());
        for name in ["b", "c", "d", "e", "missing"] {
            assert!(o.need(name).int("n").is_err(), "{name}");
        }
    }

    #[test]
    fn a_member_named_twice_is_refused() {
        assert!(parse(r#"{"change":1,"change":2}"#).is_err());
        assert!(parse(r#"{"a":{"x":1,"x":1}}"#).is_err());
        assert!(parse(r#"{"a":1,"A":2}"#).is_ok());
    }

    #[test]
    fn size_values_depth_and_trailing_bytes_are_bounded() {
        assert!(Json::parse(b"[1,2,3]", 6).is_err());
        assert!(Json::parse(b"[1,2,3]", 7).is_ok());
        // more values than a body of this size may hold: 2 bytes each
        let many = format!("[{}0]", "0,".repeat(4000));
        assert!(parse(&many).is_err());
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(parse(&deep).is_err());
        assert!(parse("{} x").is_err());
        assert!(parse("{}{}").is_err());
        assert!(parse("").is_err());
        assert!(Json::parse(b"\"\xff\"", 16).is_err());
    }

    #[test]
    fn byte_strings_have_one_spelling() {
        let v = parse(r#"["AAEC","AAEC=","AAE","AAF","AA E","",7]"#).unwrap();
        let l = v.list(8, "l").unwrap();
        assert_eq!(l[0].bytes(1, 8, "b"), Ok(Bytes(vec![0, 1, 2])));
        assert!(l[1].bytes(1, 8, "b").is_err(), "padding");
        assert_eq!(l[2].bytes(1, 8, "b"), Ok(Bytes(vec![0, 1])));
        assert!(
            l[3].bytes(1, 8, "b").is_err(),
            "a set bit behind the last byte"
        );
        assert!(l[4].bytes(1, 8, "b").is_err(), "white space");
        assert!(l[5].bytes(1, 8, "b").is_err(), "empty");
        assert!(l[6].bytes(1, 8, "b").is_err(), "a number");
        assert!(l[0].bytes(4, 8, "b").is_err(), "too short");
        assert!(l[0].bytes(1, 2, "b").is_err(), "too long");
    }

    #[test]
    fn lists_and_absent_members() {
        let v = parse(r#"{"l":[1,2,3],"n":null}"#).unwrap();
        let o = v.obj("o").unwrap();
        assert!(o.need("l").list(2, "l").is_err());
        assert_eq!(o.need("l").list(3, "l").unwrap().len(), 3);
        assert!(o.maybe("n").is_none() && o.maybe("gone").is_none() && o.maybe("l").is_some());
        assert!(o.has("n") && !o.has("gone"));
        assert!(v.list(1, "l").is_err() && o.need("l").obj("o").is_err());
    }
}
