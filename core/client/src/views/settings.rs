//! Settings: the list's rows with what each says (Agents, Desks, Devices), and the person's devices.
//!
//! The rules are the web's of before (`auth.mjs` `homeMain`, `devicesMain`, `lists`, `devState`, `devName`, `fp`,
//! `devIcon`, `unfinished`; `ui.mjs` `agentsOf`, `programOf`) and the iPhone's (`SettingsScreen.swift` `SettingsHome`,
//! `DevicesPage`, `DeviceRow`):
//! - Devices are the person's own (phones, tablets, browsers), in the order they joined; the agents are under Agents;
//! - a device's state: "this device", "never finished signing in" (in, not this one, a person's, never named
//!   itself, not connected: it holds no KeyPackage, so every new session is refused while it is there), "online",
//!   "away"; a removed one "removed";
//! - Agents counts the sessions (not archived, not removed, not one's own) grouped by program and computer.
//!
//! What only this device knows (the theme, the Push level, the hub's bytes) is the shell's; the account's e-mail is the
//! account's (W3).

use super::board::Board;
use super::words::say;
use super::{DeviceRow, SettingsView, Text};
use crate::model::types::Member;
use serde_json::Value;
use std::collections::BTreeSet;

fn name_of(d: &Member) -> String {
    if !d.device_name.is_empty() {
        d.device_name.clone()
    } else if d.device_role == "human" {
        "Device".into()
    } else {
        "Agent".into()
    }
}

/// The key fingerprint as every device shows it; a member list without one shows the id's first 16 signs in fours.
fn fingerprint_of(d: &Member) -> String {
    if !d.fingerprint.is_empty() {
        return d.fingerprint.clone();
    }
    let head: Vec<char> = d.device_id.chars().take(16).collect();
    head.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

fn unfinished(d: &Member) -> bool {
    d.is_active && !d.is_me && d.device_role == "human" && d.device_name.is_empty() && !d.is_online
}

fn icon_of(d: &Member) -> &'static str {
    if d.device_role != "human" {
        return "terminal";
    }
    let words = format!("{} {}", d.device_name, d.platform.as_str().unwrap_or("")).to_ascii_lowercase();
    if ["phone", "ipad", "ios", "android"].iter().any(|w| words.contains(w)) {
        "phone"
    } else {
        "laptop"
    }
}

fn row_of(d: &Member) -> DeviceRow {
    let (state, en) = if !d.is_active {
        ("removed", "removed")
    } else if d.is_me {
        ("this_device", "this device")
    } else if unfinished(d) {
        ("unfinished", "never finished signing in")
    } else if d.is_online {
        ("online", "online")
    } else {
        ("away", "away")
    };
    DeviceRow {
        id: d.device_id.clone(),
        name: name_of(d),
        this_device: d.is_me,
        role: d.device_role.clone(),
        active: d.is_active,
        online: d.is_active && (d.is_online || d.is_me),
        state: state.into(),
        says: say(&format!("device.{state}"), en, &[]),
        fingerprint: fingerprint_of(d),
        icon: icon_of(d).into(),
        added_ms: d.added_at,
    }
}

fn counted(key: &str, n: usize, one: &str, many: &str) -> Text {
    let n_s = n.to_string();
    say(key, format!("{n} {}", if n == 1 { one } else { many }), &[("n", &n_s)])
}

/// The program a session runs in: its device's platform, else what its model's name says.
fn program_of(b: &Board, key: &str, member: Option<&Member>) -> &'static str {
    match member.and_then(|m| m.platform.as_str()) {
        Some("claude-code") => return "Claude Code",
        Some("codex") => return "Codex",
        _ => {}
    }
    let model = b
        .model
        .sessions
        .get(key)
        .and_then(|s| s.profile.as_ref())
        .and_then(|p| p.get("model"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    let codex = model.starts_with("gpt")
        || model.starts_with("codex")
        || (model.starts_with('o') && model[1..].starts_with(|c: char| c.is_ascii_digit()));
    if codex {
        "Codex"
    } else if model.contains("claude") {
        "Claude Code"
    } else {
        "Agent"
    }
}

pub fn settings(b: &Board) -> SettingsView {
    let mut people: Vec<&Member> = b.model.members.values().filter(|d| d.device_role == "human").collect();
    // (of two with the same number, which a hub does not give, this device first, then by id: the same on every run)
    people.sort_by_key(|d| (d.added_entry_number, !d.is_me, d.device_id.clone()));
    let devices: Vec<DeviceRow> = people.iter().filter(|d| d.is_active).map(|d| row_of(d)).collect();
    let removed: Vec<DeviceRow> = people.iter().filter(|d| !d.is_active).map(|d| row_of(d)).collect();
    let stuck: Vec<String> = devices.iter().filter(|d| d.state == "unfinished").map(|d| d.id.clone()).collect();

    // Agents: the sessions grouped by program and computer.
    let mut groups = BTreeSet::new();
    let mut sessions = 0;
    for a in b.agents.iter().filter(|a| !a.row.archived && !a.row.removed && !a.row.own) {
        let member = a.row.agent_device_id.as_deref().and_then(|d| b.model.members.get(d));
        let host = member.and_then(|m| m.host.as_str()).unwrap_or("").to_string();
        groups.insert((program_of(b, &a.row.key, member), host));
        sessions += 1;
    }
    let agents = (!groups.is_empty()).then(|| {
        let (g, s) = (groups.len().to_string(), sessions.to_string());
        let en = format!("{} · {}", counted("", groups.len(), "agent", "agents").en, counted("", sessions, "session", "sessions").en);
        say("settings.agents", en, &[("agents", &g), ("sessions", &s)])
    });

    SettingsView {
        account: None,
        agents,
        desks: (!b.desks.is_empty()).then(|| counted("settings.desks", b.desks.len(), "desk", "desks")),
        devices_says: (!devices.is_empty()).then(|| counted("settings.devices", devices.len(), "device", "devices")),
        remove_unfinished: (!stuck.is_empty()).then(|| {
            if stuck.len() == 1 {
                say("settings.remove_unfinished.one", "Remove the device that never finished signing in", &[])
            } else {
                let n = stuck.len().to_string();
                say("settings.remove_unfinished", format!("Remove the {n} devices that never finished signing in"), &[("n", &n)])
            }
        }),
        unfinished: stuck,
        key_epoch: b.model.key_epoch,
        devices,
        removed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::testroom::{Room, NOW};
    use crate::views::Lens;
    use serde_json::json;

    fn member(id: &str, name: &str, role: &str, added: u64) -> Value {
        json!({ "device_id": id, "device_role": role, "device_name": name, "is_active": true, "added_entry_number": added, "is_me": false, "is_online": false })
    }

    #[test]
    fn the_persons_devices_in_the_order_they_joined() {
        let mut r = Room::default();
        let mut me = member("bbbbccccddddeeeeffff", "Laptop", "human", 1);
        me["is_me"] = json!(true);
        me["fingerprint"] = json!("AB12 CD34");
        let mut phone = member("p", "Phone", "human", 2);
        phone["is_online"] = json!(true);
        let mut gone = member("g", "Old iPad", "human", 0);
        gone["is_active"] = json!(false);
        r.members = vec![member("hung", "", "human", 3), phone, me, gone, member("agentdev", "fernly", "agent", 4)];
        r.session("a", None, true);
        r.session("b", Some("a"), true);
        let m = r.model(NOW);
        let lens = Lens::default();
        let v = settings(&Board::new(&m, &lens, NOW));
        let rows: Vec<(&str, &str, &str, bool)> = v.devices.iter().map(|d| (d.name.as_str(), d.state.as_str(), d.icon.as_str(), d.online)).collect();
        assert_eq!(
            rows,
            [("Laptop", "this_device", "laptop", true), ("Phone", "online", "phone", true), ("Device", "unfinished", "laptop", false)]
        );
        assert_eq!(v.devices[0].fingerprint, "AB12 CD34");
        assert_eq!(v.devices[2].fingerprint, "hung");
        assert_eq!(v.devices[2].says.en, "never finished signing in");
        assert_eq!(v.removed.iter().map(|d| (d.name.as_str(), d.state.as_str())).collect::<Vec<_>>(), [("Old iPad", "removed")]);
        assert_eq!(v.unfinished, ["hung"]);
        assert_eq!(v.remove_unfinished.unwrap().en, "Remove the device that never finished signing in");
        assert_eq!(v.devices_says.unwrap().en, "3 devices");
        // two sessions of one program on one computer: one agent
        assert_eq!(v.agents.unwrap().en, "1 agent · 2 sessions");
        assert!(v.desks.is_none());
    }

    #[test]
    fn a_fingerprint_without_one_is_the_ids_head_in_fours() {
        let d: Member = serde_json::from_value(member("0123456789abcdefXYZ", "", "human", 0)).unwrap();
        assert_eq!(fingerprint_of(&d), "0123 4567 89ab cdef");
    }
}
