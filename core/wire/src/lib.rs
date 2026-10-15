//! What the hub and the app core must read the same way, written once.
//!
//! The hub (`hub/`) and the clients (`core/client`, `core/protocol`) are built from separate code: the hub links no
//! client code and no key handling. Where both sides must agree on the wire, the agreement lives here:
//!
//! - [`codes`]: the refusal codes of spec/v1.md section 16 and the status each comes with.
//! - [`limits`]: the pages, windows and counts the hub's routes keep to, which the client asks within.
//! - [`account`], [`envelope`], [`mls`], [`invite`], [`signin`], [`recovery`], [`push`]: the constants of the structs and
//!   records of spec/v1.md that both sides write or check (versions, kinds, labels, lengths, lifetimes).
//! - [`layout`] (feature `layout`): the hub's strict parsers and encoders of those structs; they read headers and
//!   public fields only.
//!
//! Nothing here holds a key, decides for a client or does I/O: formats, limits, codes and pure checks.

#![forbid(unsafe_code)]

pub mod account;
pub mod codes;
pub mod envelope;
pub mod invite;
pub mod limits;
pub mod mls;
pub mod push;
pub mod recovery;
pub mod rules;
pub mod signin;

#[cfg(feature = "layout")]
pub mod layout;
