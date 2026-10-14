//! An answer's JSON, read strictly and within bounds, and the checks every route's reader is made of.
//!
//! The hub is not trusted, so the text is not read into whatever it describes:
//!
//! - **Size first.** A body over the route's cap is refused before a byte of it is parsed.
//! - **A bound on what is built.** A body may hold one value per 8 bytes, and never fewer than [`SMALL_VALUES`]
//!   (a member's name counts as one). Up to 64 KiB that is every value JSON can put in a body (one per 2 bytes at
//!   the most), so no small answer is refused for being dense: the answer to 32 joins, `{"change":N,"epoch":2}` a
//!   row, holds a value per 5 bytes. A long answer is one of the lists, whose densest rows are a value per 9 to 10
//!   bytes (a row of the open list, a wish, a group's row; see the tests), and a long body made of `0,0,0,…`
//!   cannot make the parser build many times its own size.
//! - **One member per name.** An object that names a member twice is refused: two readers could take different ones.
//! - **No more members than the protocol has.** An object's 4097th member ends the reading where it stands.
//! - **Work by the byte.** Every bound holds while the text is read, not after: no answer costs more than a few
//!   steps per byte of it (an object's names are sorted once; none is compared with every other).
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
    /// At most [`MAX_MEMBERS`] members, sorted by name, each name once.
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
        // (a name beyond the cap is refused before it is read: see `Name`)
        while let Some(name) = map.next_key_seed(Name {
            full: out.len() == MAX_MEMBERS,
        })? {
            self.spend()?;
            let value = map.next_value_seed(Seed { left: self.left })?;
            out.push((name, value));
        }
        // sorted by name: a name twice then stands side by side, and a reader finds a member in a few steps
        out.sort_unstable_by(|a, b| order(&a.0, &b.0));
        if out
            .windows(2)
            .any(|pair| order(&pair[0].0, &pair[1].0).is_eq())
        {
            return Err(de::Error::custom("a member named twice"));
        }
        Ok(Json::Map(out))
    }
}

/// A member's name. In an object that is `full` there is none: the parser calls this once it knows that another
/// member follows and before it reads a byte of the name, so an object beyond the cap ends there.
struct Name {
    full: bool,
}

impl<'de> DeserializeSeed<'de> for Name {
    type Value = String;
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<String, D::Error> {
        if self.full {
            return Err(de::Error::custom("more members than any object has"));
        }
        #[cfg(test)]
        NAMES.with(|c| c.set(c.get() + 1));
        serde::Deserialize::deserialize(d)
    }
}

/// The order of two member names. Every comparison of names goes through here, so that a test can count them.
fn order(a: &str, b: &str) -> std::cmp::Ordering {
    #[cfg(test)]
    COMPARED.with(|c| c.set(c.get() + 1));
    a.cmp(b)
}

#[cfg(test)]
thread_local! {
    /// How many names this thread compared.
    static COMPARED: Cell<u64> = const { Cell::new(0) };
    /// How many members' names this thread read.
    static NAMES: Cell<u64> = const { Cell::new(0) };
}

/// The values any answer may hold, however short: as many as JSON can put in 64 KiB (the most of a write's
/// answer): a few MiB of memory at the most.
const SMALL_VALUES: usize = 32 << 10;

/// Most members an object may have: no object of the protocol has more than the 1024 devices of a claim's
/// `key_packages`. The parser stops at the member beyond it.
const MAX_MEMBERS: usize = 4096;

impl Json {
    /// Reads `body` as one JSON value: at most `cap` bytes, within the bounds above.
    pub(crate) fn parse(body: &[u8], cap: usize) -> Read<Json> {
        if body.len() > cap {
            return bad("an answer larger than its route allows");
        }
        let left = Cell::new((body.len() / 8).max(SMALL_VALUES) + 64);
        let mut de = serde_json::Deserializer::from_slice(body);
        let value = match (Seed { left: &left }).deserialize(&mut de) {
            Ok(value) => value,
            Err(_) => return bad("an answer that is not the JSON of this protocol"),
        };
        if de.end().is_err() {
            return bad("an answer with bytes behind its JSON");
        }
        Ok(value)
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

    /// Exactly `N` bytes.
    pub(crate) fn fixed<const N: usize>(&self, why: &'static str) -> Read<[u8; N]> {
        let bytes = self.bytes(N, N, why)?;
        <[u8; N]>::try_from(&bytes.0[..]).or(bad(why))
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

/// An object's members, sorted by name.
#[derive(Clone, Copy)]
pub(crate) struct Obj<'a>(&'a [(String, Json)]);

impl<'a> Obj<'a> {
    fn find(&self, name: &str) -> Option<&'a Json> {
        let at = self.0.binary_search_by(|(n, _)| order(n, name)).ok()?;
        Some(&self.0[at].1)
    }

    /// The member, or null when the object has none of this name: a reader that needs it then fails on its type.
    pub(crate) fn need(&self, name: &str) -> &'a Json {
        self.find(name).unwrap_or(&NULL)
    }

    /// The member when it is there and not null.
    pub(crate) fn maybe(&self, name: &str) -> Option<&'a Json> {
        Some(self.need(name)).filter(|v| !v.is_null())
    }

    pub(crate) fn has(&self, name: &str) -> bool {
        self.find(name).is_some()
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
        // more values than a body of this size may hold: 2 bytes each, beyond the floor of small answers
        let many = format!("[{}0]", "0,".repeat(200_000));
        assert!(parse(&many).is_err());
        let one_too_many = format!("[{}0]", "0,".repeat(SMALL_VALUES + 64 - 1));
        assert!(parse(&one_too_many).is_err());
        let all = format!("[{}0]", "0,".repeat(SMALL_VALUES + 64 - 2));
        assert!(parse(&all).is_ok());
        // any body of up to 64 KiB, however dense
        let small = format!("[{}0]", "0,".repeat((64 << 10) / 2 - 2));
        assert!(small.len() <= 64 << 10 && parse(&small).is_ok());
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(parse(&deep).is_err());
        assert!(parse("{} x").is_err());
        assert!(parse("{}{}").is_err());
        assert!(parse("").is_err());
        assert!(Json::parse(b"\"\xff\"", 16).is_err());
    }

    /// The names compared, and the names read, while `text` was read.
    fn compared(text: &str) -> (bool, u64, u64) {
        COMPARED.with(|c| c.set(0));
        NAMES.with(|c| c.set(0));
        let good = parse(text).is_ok();
        (good, COMPARED.with(Cell::get), NAMES.with(Cell::get))
    }

    /// The `i`th of the names below: distinct, and in no order.
    fn name(i: usize) -> String {
        format!("member{:08x}", (i as u32).wrapping_mul(2_654_435_761))
    }

    /// An object of distinct members, 19 bytes each: inside the bound on values (two per member, 8 bytes each).
    fn object_of(members: usize) -> String {
        let list: Vec<String> = (0..members).map(|i| format!("\"{}\":0", name(i))).collect();
        format!("{{{}}}", list.join(","))
    }

    /// Most comparisons of names per member that reading an object may take: a sort of [`MAX_MEMBERS`] names.
    /// (Comparing each name with all before it takes 2 047 per member there, and 15 000 at 30 000 members.)
    const PER_MEMBER: u64 = 32;

    #[test]
    fn reading_an_object_costs_no_more_than_its_size() {
        // the work is counted, not timed
        for members in [16, 256, 1024, MAX_MEMBERS] {
            let (good, work, names) = compared(&object_of(members));
            assert!(good, "{members} members");
            assert_eq!(names, members as u64);
            assert!(
                work <= PER_MEMBER * members as u64,
                "{members} members: {work}"
            );
        }
        // one member more than any object of the protocol: refused where that member begins, with no name
        // compared and none read beyond the cap
        let cap = MAX_MEMBERS as u64;
        assert_eq!(compared(&object_of(MAX_MEMBERS + 1)), (false, 0, cap));
        // 30 000 distinct members fit 1 MiB (and over 800 000 the 16 MiB of a list): the same
        let many = object_of(30_000);
        assert!(many.len() < 1 << 20);
        assert_eq!(compared(&many), (false, 0, cap));
        // what follows the cap is not read: not the name (here one that is no text at all), not the rest
        let cut = object_of(MAX_MEMBERS).replacen('}', ",\"\\uZZZZ", 1);
        assert_eq!(compared(&cut), (false, 0, cap));
        // many full objects in one answer, side by side and one in the other: still per member
        let full = object_of(MAX_MEMBERS);
        let list = format!("[{}]", [full.as_str(); 8].join(","));
        let (good, work, names) = compared(&list);
        assert!(good && names == 8 * cap);
        assert!(work <= 8 * PER_MEMBER * cap, "{work}");
        let nested = full.replacen(":0", &format!(":{full}"), 1);
        let (good, work, names) = compared(&nested);
        assert!(good && names == 2 * cap);
        assert!(work <= 2 * PER_MEMBER * cap, "{work}");
        // an object in a full object's member beyond the cap is not read either
        let beyond = object_of(MAX_MEMBERS + 1).replacen(":0}", &format!(":{full}}}"), 1);
        assert_eq!(compared(&beyond), (false, 0, cap));
    }

    #[test]
    fn a_name_twice_is_found_among_many() {
        let most = object_of(MAX_MEMBERS - 1);
        for twice in [0, 2047, MAX_MEMBERS - 2] {
            let again = format!("\"{}\":1", name(twice));
            let first = most.replacen('{', &format!("{{{again},"), 1);
            let last = most.replacen('}', &format!(",{again}}}"), 1);
            assert!(parse(&first).is_err() && parse(&last).is_err(), "{twice}");
        }
        assert!(parse(&most).is_ok());
    }

    #[test]
    fn a_member_is_found_whatever_the_order_of_the_text() {
        let text = object_of(MAX_MEMBERS).replacen(":0", ":7", 1);
        let v = parse(&text).unwrap();
        let o = v.obj("o").unwrap();
        assert_eq!(o.members().len(), MAX_MEMBERS);
        assert_eq!(o.need(&name(0)).int("n"), Ok(7));
        for i in [1, 2047, MAX_MEMBERS - 1] {
            assert_eq!(o.need(&name(i)).int("n"), Ok(0), "{i}");
            assert!(o.has(&name(i)));
        }
        assert!(!o.has(&name(MAX_MEMBERS)) && o.need("member").is_null() && !o.has(""));
        // a reader's look for one member: a few steps, not one per member
        COMPARED.with(|c| c.set(0));
        o.need(&name(5));
        assert!(COMPARED.with(Cell::get) <= 13);
    }

    /// The densest rows of the real answers, at their shortest (numbers of one digit), many of them: never refused
    /// by the bound on values. A row of the open list, of `GET /v1/requests`, of the room's groups (the room's own
    /// row, the shortest), of `POST /v1/commits`; the long lists fill 4 MiB.
    #[test]
    fn the_densest_real_answers_are_within_the_bound() {
        let id = |n: usize| "A".repeat(n);
        let open = format!(
            r#"{{"object_id":"{}","version_change":1,"head_change":1}}"#,
            id(22)
        );
        let wish = format!(
            r#"{{"id":1,"device":"{}","kind":"session","at":1}}"#,
            id(43)
        );
        // (as the hub writes it: the optional members as null)
        let wish_nulls = format!(
            r#"{{"id":1,"device":"{}","kind":"session","group_id":null,"key_package":null,"n":null,"at":1,"committer":null}}"#,
            id(43)
        );
        let group = format!(
            r#"{{"group_id":"{}","kind":"room","epoch":0,"room_epoch":0,"live":true,"stale":false,"leaves":[]}}"#,
            id(43)
        );
        let group_nulls = format!(
            r#"{{"group_id":"{}","kind":"room","session_id":null,"parent":null,"epoch":0,"room_epoch":0,"live":true,"stale":false,"leaves":[]}}"#,
            id(43)
        );
        for (row, at_least) in [
            (&open, 9),
            (&wish, 9),
            (&wish_nulls, 8),
            (&group, 8),
            (&group_nulls, 8),
        ] {
            let rows = (4 << 20) / (row.len() + 1);
            let body = format!(
                r#"{{"change":1,"more":false,"items":[{}]}}"#,
                vec![row.as_str(); rows].join(",")
            );
            let v = parse_big(&body).unwrap_or_else(|_| panic!("{row}"));
            let values = 1 + 6 + rows * row.matches(':').count() * 2 + rows;
            assert!(
                body.len() / values >= at_least,
                "{row}: {} bytes a value",
                body.len() / values
            );
            drop(v);
        }
        let joins: Vec<String> = (0..64)
            .map(|i| format!(r#"{{"change":{i},"epoch":2}}"#))
            .collect();
        assert!(parse(&format!(r#"{{"answers":[{}]}}"#, joins.join(","))).is_ok());
    }

    fn parse_big(text: &str) -> Read<Json> {
        Json::parse(text.as_bytes(), 16 << 20)
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
