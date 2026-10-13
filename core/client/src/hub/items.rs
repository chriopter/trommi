//! What the hub serves on several routes: an envelope as a read route gives it, an entry of a group's log, one
//! item of the room's one order (hub-api.md "Decided" 19).

use super::json::{bad, Json, Obj, Read};
use super::limits::{MAX_ENVELOPE, MAX_MLS, MAX_SMALL_STRUCT};
use super::types::{ChangeItem, EnvelopeItem, LogItem};
use trommi_core::ids::DeviceId;

fn void_code(v: &Json) -> Read<String> {
    let code = v.text(40, "void_code: a code")?;
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-';
    if code.is_empty() || !code.bytes().all(plain) {
        return bad("void_code: a code");
    }
    Ok(code.to_owned())
}

/// An envelope item; `cut` is read on the chain route only.
pub(crate) fn envelope_item(o: Obj<'_>, chain: bool) -> Read<EnvelopeItem> {
    Ok(EnvelopeItem {
        change: o.need("change").count("an envelope's change")?,
        envelope: o
            .need("envelope")
            .bytes(1, MAX_ENVELOPE, "envelope: the bytes of an envelope")?
            .0,
        received_at: o.need("received_at").int("received_at")?,
        cut: chain && matches!(o.need("cut"), Json::Bool(true)),
        void_code: o.maybe("void_code").map(void_code).transpose()?,
    })
}

/// A log entry and whether it is a Commit.
pub(crate) fn log_item(o: Obj<'_>) -> Read<(LogItem, bool)> {
    let commit = o.need("kind").one_of(
        &[("commit", true), ("message", false)],
        "the kind of a log entry",
    )?;
    let recovery_auth = match o.maybe("recovery_auth") {
        Some(v) if commit => Some(v.bytes(1, MAX_SMALL_STRUCT, "recovery_auth")?.0),
        _ => None,
    };
    let item = LogItem {
        change: o.need("change").count("a log entry's change")?,
        group: o.need("group_id").group("a log entry's group_id")?,
        n: o.need("n").count("a log entry's n")?,
        epoch: o.need("epoch").int("a log entry's epoch")?,
        at: o.need("at").int("a log entry's at")?,
        bytes: o.need("bytes").bytes(1, MAX_MLS, "a log entry's bytes")?.0,
        sender: o
            .need("sender")
            .id(DeviceId::from_slice, "a log entry's sender")?,
        recovery_auth,
    };
    Ok((item, commit))
}

pub(crate) fn change_item(v: &Json) -> Read<ChangeItem> {
    let o = v.obj("a change: an object")?;
    if matches!(o.need("kind"), Json::Text(kind) if kind == "envelope") {
        return envelope_item(o, false).map(ChangeItem::Envelope);
    }
    Ok(match log_item(o)? {
        (item, true) => ChangeItem::Commit(item),
        (item, false) => ChangeItem::Message(item),
    })
}
