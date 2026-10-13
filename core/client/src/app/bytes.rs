//! Bytes in the contract's types, and how they cross the edge.
//!
//! In Rust a [`Bytes`] is a `Vec<u8>`. In JSON it is written one of two ways:
//!
//! - inside [`edge::encode`](super::edge::encode) / [`edge::decode`](super::edge::decode) (the bindings): a number,
//!   the index of the bytes in the `blobs` list that travels beside the JSON text, so an envelope or a file never
//!   goes through base64 or a JSON string;
//! - anywhere else (conformance scripts, logs of tests): base64url without padding, the protocol's spelling.

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;

#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Bytes(pub Vec<u8>);

impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bytes({} bytes)", self.0.len())
    }
}

impl From<Vec<u8>> for Bytes {
    fn from(v: Vec<u8>) -> Self {
        Bytes(v)
    }
}

impl From<&[u8]> for Bytes {
    fn from(v: &[u8]) -> Self {
        Bytes(v.to_vec())
    }
}

impl std::ops::Deref for Bytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

thread_local! {
    /// While the edge writes: the blobs written so far. While it reads: the blobs handed in.
    static BLOBS: RefCell<Option<Vec<Vec<u8>>>> = const { RefCell::new(None) };
}

/// Runs `f` with a blob table: what `f` serialises as [`Bytes`] goes into the table (returned), what it
/// deserialises is taken from `blobs`.
pub(crate) fn with_blobs<T>(blobs: Vec<Vec<u8>>, f: impl FnOnce() -> T) -> (T, Vec<Vec<u8>>) {
    let before = BLOBS.with(|b| b.replace(Some(blobs)));
    let out = f();
    let blobs = BLOBS.with(|b| b.replace(before)).unwrap_or_default();
    (out, blobs)
}

impl Serialize for Bytes {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let index = BLOBS.with(|b| {
            b.borrow_mut().as_mut().map(|blobs| {
                blobs.push(self.0.clone());
                blobs.len() - 1
            })
        });
        match index {
            Some(i) => s.serialize_u64(i as u64),
            None => s.serialize_str(&trommi_core::ids::base64url_encode(&self.0)),
        }
    }
}

impl<'de> Deserialize<'de> for Bytes {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> de::Visitor<'de> for V {
            type Value = Bytes;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a blob index or base64url text")
            }
            fn visit_u64<E: de::Error>(self, i: u64) -> Result<Bytes, E> {
                BLOBS.with(|b| {
                    b.borrow()
                        .as_ref()
                        .and_then(|blobs| blobs.get(i as usize).cloned())
                        .map(Bytes)
                        .ok_or_else(|| E::custom("no such blob"))
                })
            }
            fn visit_str<E: de::Error>(self, t: &str) -> Result<Bytes, E> {
                trommi_core::ids::base64url_decode(t)
                    .map(Bytes)
                    .map_err(|_| E::custom("not base64url"))
            }
        }
        d.deserialize_any(V)
    }
}
