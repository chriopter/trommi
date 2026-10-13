//! Push (section 15.2): the key an app registers for its notifications, and the opening of what arrives. A push
//! carries no content: it names a room and a change, and for APNs a ticket that fetches one envelope.
//!
//! The one exception to "a content key never leaves the core": the phone's notification extension may hold, read
//! only, the content key of each session group at its current epoch ([`crate::CoreDevice::notify_keys`]), and opens
//! with it the one envelope that rang ([`open_notify_envelope`]) to show a card's title. It checks the envelope's
//! signature against the sender its header names, and nothing else: whether that sender is a member and where the
//! envelope stands in its chain is the app's to check, as for every envelope.

use crate::content::EnvelopeHeader;
use crate::CoreError;
use trommi_core::crypto::{Secret, SystemEntropy};
use trommi_core::envelope::Envelope;
use trommi_core::push;

record! {
    /// What a push says. The ticket fetches an envelope for a day: it is not printed.
    secret pub struct PushNote {
        /// The room something changed in, 32 bytes.
        pub room_id: Vec<u8>,
        /// The hub's change number of the envelope that rang.
        pub change: u64,
        /// 0 low, 1 normal, 2 high, 3 critical.
        pub urgency: u8,
        /// For APNs, what fetches that one envelope for a day; empty when there is none, and for a Web Push.
        pub ticket: Vec<u8>,
    }
}

/// A fresh key for the APNs payload: 32 random bytes, registered at the hub and kept in the app group for the
/// notification extension.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn generate_push_key() -> Result<Vec<u8>, CoreError> {
    Ok(push::generate_key(&mut SystemEntropy)?.expose().to_vec())
}

/// Opens the sealed payload of an APNs notification under the key the app registered; `decrypt-failed` when it
/// does not open, `bad-format` for a payload of another spelling.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn open_apns_push(key: Vec<u8>, sealed: Vec<u8>) -> Result<PushNote, CoreError> {
    let opened = push::open(&Secret::<32>::from_slice(&key)?, &sealed)?;
    Ok(PushNote {
        room_id: opened.room_id.as_bytes().to_vec(),
        change: opened.change,
        urgency: opened.urgency,
        ticket: opened.ticket.clone(),
    })
}

record! {
    /// The exact counts of a Live Activity, as the hub sealed them for this phone (spec/v1.md 15.3).
    pub struct LiveActivityCounts {
        /// Agents at work.
        pub working: u64,
        /// Questions waiting.
        pub waiting: u64,
    }
}

/// Opens a Live Activity's sealed block under the key the app registered for its pushes; `decrypt-failed` when it
/// does not open, `bad-format` for another length.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn open_live_activity(key: Vec<u8>, sealed: Vec<u8>) -> Result<LiveActivityCounts, CoreError> {
    let (working, waiting) = push::open_live(&Secret::<32>::from_slice(&key)?, &sealed)?;
    Ok(LiveActivityCounts { working, waiting })
}

/// Reads the payload of a Web Push, after the browser decrypted it; `bad-format` for another spelling.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn read_web_push(payload: Vec<u8>) -> Result<PushNote, CoreError> {
    let read = push::WebPush::decode(&payload)?;
    Ok(PushNote {
        room_id: read.room_id.as_bytes().to_vec(),
        change: read.change,
        urgency: read.urgency,
        ticket: Vec::new(),
    })
}

record! {
    /// The content key of one session group at its current epoch, as the app hands it to its notification
    /// extension (section 15.2). It opens everything written in that group during that epoch: never log it.
    secret pub struct NotifyKey {
        /// The session group: the room's 32 bytes followed by the session's 16.
        pub group: Vec<u8>,
        /// The epoch of the key.
        pub epoch: u64,
        /// The content key, 32 bytes (section 6).
        pub key: Vec<u8>,
    }
}

record! {
    /// One envelope opened by the notification extension: its header, signed by the sender it names, and its body's
    /// payload, one JSON object. The sender's membership and chain are not checked.
    secret pub struct NotifyOpened {
        /// The header.
        pub header: EnvelopeHeader,
        /// The body's payload: UTF-8 JSON.
        pub payload: Vec<u8>,
    }
}

/// Opens one envelope, without a device, with the key among `keys` of its header's group and epoch: decodes it,
/// checks the signature of the sender its header names, opens the body. `no-key` when none of `keys` is the one,
/// `bad-signature`, `pruned`, `decrypt-failed`, `newer-version` and `bad-format` as for any envelope. Nothing is
/// written and nothing is checked against a group's members or a sender's chain: what it gives is for showing a
/// title in a notification, never for the board.
#[cfg_attr(feature = "uniffi", uniffi::export)]
pub fn open_notify_envelope(
    keys: Vec<NotifyKey>,
    envelope: Vec<u8>,
) -> Result<NotifyOpened, CoreError> {
    let envelope = Envelope::decode(&envelope)?;
    envelope.verify()?;
    let group = envelope.header.group.as_bytes();
    let key = keys
        .iter()
        .find(|key| key.group == group && key.epoch == envelope.header.epoch)
        .ok_or(trommi_core::Error::NoKey)?;
    let body = envelope.open(&Secret::<32>::from_slice(&key.key)?)?;
    Ok(NotifyOpened {
        header: EnvelopeHeader::from(&envelope.header),
        payload: body.payload().to_vec(),
    })
}
