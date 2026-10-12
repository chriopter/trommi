//! Presence: who is connected, and what an agent's link report says, from the hub's live stream. One reading for every
//! app, so that a device shows the same "online", "gone" and link on the web and on the iPhone.
//!
//! The hub tells a human device's stream who is here: when the stream opens, a `presence` event for every device that
//! is online now, ended by a `here` event; after that, a `presence` event whenever a device comes, goes or reports its
//! link (spec/hub-api.md 20). Presence is not replayed, so:
//! - **the opening list is whole** ([`Model::apply_here`]): a device this device shows online that the list did not
//!   name left while no stream was open, and is offline as of the `here`; a device never seen online keeps its state;
//! - **an event without `hears` says nothing of the link**: the report held stands while the device is online (a stream
//!   opened or closed); with `hears`, the report is the event's (`attached`: true, its `working`, `last_call_at`);
//! - **offline since** the hub's `offline_since` when the event names one, else the moment the event came;
//! - **lost** (the hub saw the agent's stream break without a word): an alert for the person.
use crate::model::change::Change;
use crate::model::groups::DeviceReport;
use crate::model::Model;
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

/// A `presence` event of the live stream, as the hub sends it (the device as base64url, or hex).
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PresenceEvent {
    /// (none: the event names a group the hub archived, `{group_id, archived}`, which is not presence)
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub online: bool,
    #[serde(default)]
    pub lost: bool,
    #[serde(default)]
    pub hears: Option<bool>,
    #[serde(default)]
    pub working: Option<bool>,
    #[serde(default)]
    pub last_call_at: Option<u64>,
    #[serde(default)]
    pub offline_since: Option<u64>,
}

/// A device id as the model names it (64 hex digits), from the wire's base64url of its 32 bytes or from hex.
pub fn device_hex(id: &str) -> Option<String> {
    if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(id.to_ascii_lowercase());
    }
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return None,
        } as u32)
    };
    let mut bits = 0u32;
    let mut n = 0;
    let mut out = String::with_capacity(64);
    for c in id.trim_end_matches('=').bytes() {
        bits = (bits << 6) | value(c)?;
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push_str(&format!("{:02x}", (bits >> n) & 0xff));
        }
    }
    (out.len() == 64).then_some(out)
}

impl Model {
    /// The live stream opened (again): the hub's list of who is here comes first, ended by its `here`.
    pub fn stream_opened(&mut self) {
        self.named_here = Some(BTreeSet::new());
    }

    /// One `presence` event. Returns whether an agent device came online (a human device then looks whether it waits
    /// for its main session: the seat rule).
    pub fn apply_presence_event(&mut self, e: &PresenceEvent, change: &mut Change, now: u64) -> bool {
        let Some(id) = device_hex(&e.device) else {
            return false;
        };
        if e.online {
            if let Some(named) = self.named_here.as_mut() {
                named.insert(id.clone());
            }
        }
        let link = e.hears.map(|h| {
            let mut l = json!({ "hears": if h { "live" } else { "oncall" }, "attached": true, "working": e.working == Some(true) });
            if let Some(t) = e.last_call_at.filter(|t| *t > 0) {
                l["last_call_at"] = json!(t);
            }
            l
        });
        let report = DeviceReport {
            device_id: id.clone(),
            is_online: e.online,
            offline_since: if e.online { None } else { Some(e.offline_since.unwrap_or(now)) },
            link,
        };
        let was = self.members.get(&id).map(|m| (m.is_online, m.device_role == "agent"));
        self.apply_presence(std::slice::from_ref(&report), change, now);
        if e.lost {
            self.push_alert(change, "agent-lost", "An agent lost its connection.", None, Some(id.clone()), "local", now);
        }
        matches!(was, Some((false, true))) && e.online
    }

    /// The `here` that ends a new stream's list of who is here: every other device shown online is offline as of
    /// `now`. Nothing without a list (a `here` with no stream opened before it).
    pub fn apply_here(&mut self, change: &mut Change, now: u64) {
        let Some(named) = self.named_here.take() else {
            return;
        };
        let gone: Vec<DeviceReport> = self
            .members
            .values()
            .filter(|m| m.is_online && !m.is_me && !named.contains(&m.device_id))
            .map(|m| DeviceReport {
                device_id: m.device_id.clone(),
                is_online: false,
                offline_since: Some(now),
                link: None,
            })
            .collect();
        if !gone.is_empty() {
            self.apply_presence(&gone, change, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Link, Member};

    const D0: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";
    const D1: &str = "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1";

    fn room() -> Model {
        let mut m = Model::new();
        for d in [D0, D1] {
            m.members.insert(
                d.into(),
                Member {
                    device_id: d.into(),
                    device_role: "agent".into(),
                    is_active: true,
                    ..Default::default()
                },
            );
        }
        m
    }
    fn ev(device: &str, online: bool, hears: Option<bool>) -> PresenceEvent {
        PresenceEvent {
            device: device.into(),
            online,
            hears,
            ..Default::default()
        }
    }

    #[test]
    fn ids_in_either_form() {
        let b64 = "0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NDQ0NA";
        assert_eq!(device_hex(b64).as_deref(), Some(D0));
        assert_eq!(device_hex(D0).as_deref(), Some(D0));
        assert_eq!(device_hex("short"), None);
    }

    /// An event without `hears` leaves the link report as it was; with it, the report is the event's.
    #[test]
    fn a_stream_event_without_hears_keeps_the_link() {
        let mut m = room();
        let mut c = Change::new();
        assert!(m.apply_presence_event(&ev(D0, true, Some(false)), &mut c, 10), "an agent came online");
        assert_eq!(m.members[D0].link.as_ref().map(|l| l.hears.as_str()), Some("oncall"));
        assert!(!m.apply_presence_event(&ev(D0, true, None), &mut c, 20));
        assert_eq!(m.members[D0].link.as_ref().map(|l| l.hears.as_str()), Some("oncall"), "kept");
        m.apply_presence_event(&ev(D0, false, None), &mut c, 30);
        assert_eq!(m.members[D0].offline_since, Some(30), "offline as of the event");
        assert_eq!(m.members[D0].link, None::<Link>);
    }

    /// The opening list is whole: a device it did not name left while no stream was open; one never online stays.
    #[test]
    fn here_ends_the_opening_list() {
        let mut m = room();
        let mut c = Change::new();
        m.apply_presence_event(&ev(D0, true, None), &mut c, 10);
        m.stream_opened();
        m.apply_presence_event(&ev(D1, true, None), &mut c, 20);
        m.apply_here(&mut c, 25);
        assert!(!m.members[D0].is_online, "not named: it left meanwhile");
        assert_eq!(m.members[D0].offline_since, Some(25));
        assert!(m.members[D1].is_online);
        // a `here` without a list says nothing
        m.apply_here(&mut c, 30);
        assert!(m.members[D1].is_online);
    }
}
