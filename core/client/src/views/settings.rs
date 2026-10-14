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
use super::{AgentSessionView, AgentView, DeviceRow, MeterView, SettingsView, Text, UsageView};
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

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The page Agents (the web's `agentsOf` and `agentsMain`): the sessions that are not archived, removed or one's own,
/// grouped by program and computer; under each its main sessions, a helper counted beside its main when that main is
/// of the same agent; the most sessions first, then by program and computer.
pub fn agents_page(b: &Board) -> Vec<AgentView> {
    let m = b.model;
    let me = m.my_device_id.as_deref();
    let member_of = |i: usize| b.agents[i].row.agent_device_id.as_deref().and_then(|d| m.members.get(d));
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let mut groups: Vec<(String, &'static str, String, Vec<usize>)> = Vec::new();
    // (a helper whose own device the room does not list goes with its main session's program and computer)
    let parent_of = |i: usize| b.agents[i].row.parent.as_deref().and_then(|p| b.agents.iter().position(|x| x.row.id == p));
    for (i, a) in b.agents.iter().enumerate().filter(|(_, a)| !a.row.archived && !a.row.removed && !a.row.own) {
        let member = member_of(i).or_else(|| parent_of(i).and_then(member_of));
        let program = program_of(b, &a.row.key, member);
        let host = member.map(|d| text(&d.host)).unwrap_or_default();
        let key = format!("{program} · {host}");
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.3.push(i),
            None => groups.push((key, program, host, vec![i])),
        }
    }
    let account_of = |i: usize| {
        m.sessions
            .get(&b.agents[i].row.key)
            .and_then(|s| s.profile.as_ref())
            .and_then(|p| p.get("account"))
            .filter(|v| v.is_object())
    };
    let mut out: Vec<AgentView> = groups
        .into_iter()
        .map(|(key, program, host, list)| {
            let ids: Vec<&str> = list.iter().map(|i| b.agents[*i].row.id.as_str()).collect();
            let tops: Vec<usize> = list.iter().copied().filter(|i| b.agents[*i].row.parent.as_deref().is_none_or(|p| !ids.contains(&p))).collect();
            let online = list.iter().any(|i| b.agents[*i].row.online);
            let mut devices: Vec<String> = Vec::new();
            for i in &list {
                if let Some(d) = b.agents[*i].row.agent_device_id.as_deref() {
                    let human = m.members.get(d).is_some_and(|x| x.device_role == "human");
                    if Some(d) != me && !human && !devices.iter().any(|x| x == d) {
                        devices.push(d.to_string());
                    }
                }
            }
            let sessions: Vec<AgentSessionView> = tops
                .iter()
                .map(|&i| {
                    let a = &b.agents[i];
                    let helpers = list.iter().filter(|x| b.agents[**x].row.parent.as_deref() == Some(a.row.id.as_str())).count();
                    let desk = b.desks.iter().find(|d| Some(&d.id) == a.desk.as_ref()).map(|d| d.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Personal".into());
                    let h = helpers.to_string();
                    AgentSessionView {
                        id: a.row.key.clone(),
                        slug: a.row.id.clone(),
                        name: say("session.name", a.row.name.as_str(), &[]),
                        mark: if a.row.icon.is_empty() { a.row.id.clone() } else { a.row.icon.clone() },
                        desk: say("agents.desk", format!("{desk} desk"), &[("desk", &desk)]),
                        folder: member_of(i).map(|d| text(&d.folder)).unwrap_or_default(),
                        helpers: helpers as u32,
                        helpers_says: (helpers > 0).then(|| say("agents.helpers", format!("+{}", plural(helpers, "helper", "helpers")), &[("n", &h)])),
                        online: a.row.online,
                        state: if a.row.online { say("agents.connected", "connected", &[]) } else { say("agents.away", "away", &[]) },
                    }
                })
                .collect();
            let n_helpers = list.len() - tops.len();
            let (t, h) = (tops.len().to_string(), n_helpers.to_string());
            let count = if n_helpers > 0 {
                say("agents.count.helpers", format!("{}, {}", plural(tops.len(), "session", "sessions"), plural(n_helpers, "helper", "helpers")), &[("sessions", &t), ("helpers", &h)])
            } else {
                say("agents.count", plural(tops.len(), "session", "sessions"), &[("sessions", &t)])
            };
            let on = if host.is_empty() { "an unnamed computer".to_string() } else { host.clone() };
            let account = list.iter().find(|i| b.agents[**i].row.main && account_of(**i).is_some()).or_else(|| list.iter().find(|i| account_of(**i).is_some())).and_then(|i| account_of(*i));
            let remove = (!devices.is_empty()).then(|| {
                let place = if host.is_empty() { "this computer" } else { host.as_str() };
                let all = plural(list.len(), "session", "sessions");
                let n = list.len().to_string();
                say(
                    "agents.remove",
                    format!("{program} on {place} and its {all} can read nothing new after this. The others get a new key; the history stays."),
                    &[("program", program), ("host", place), ("n", &n)],
                )
            });
            AgentView {
                name: say("agents.name", format!("{program} on {on}"), &[("program", program), ("host", &on)]),
                state: if online { say("agents.connected", "connected", &[]) } else { say("agents.seen", "away · last seen", &[]) },
                seen_ms: list.iter().map(|i| b.agents[*i].row.last_activity_at).max().unwrap_or(0),
                count,
                total: list.len() as u32,
                online,
                usage: account.and_then(|acc| usage_of(acc, b.now)),
                remove,
                devices,
                sessions,
                key,
                program: program.into(),
                host,
            }
        })
        .collect();
    out.sort_by(|x, y| {
        y.total.cmp(&x.total).then_with(|| x.program.cmp(&y.program)).then_with(|| x.host.cmp(&y.host))
    });
    out
}

const PLANS: [(&str, &str); 9] = [("pro", "Pro"), ("max", "Max"), ("team", "Team"), ("enterprise", "Enterprise"), ("free", "Free"), ("plus", "Plus"), ("prolite", "Pro Lite"), ("business", "Business"), ("edu", "Edu")];

/// An agent's account as the page shows it (the web's `usageFull`, `usagePlan`, `usageMeters`); none without a plan
/// or an e-mail.
pub fn usage_of(acc: &Value, now: u64) -> Option<UsageView> {
    let s = |k: &str| match acc.get(k) {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    };
    let (plan, email) = (s("plan"), s("email"));
    if plan.is_empty() && email.is_empty() {
        return None;
    }
    let maker = if s("kind") == "codex" { "ChatGPT" } else { "Claude" };
    let named = PLANS.iter().find(|(k, _)| *k == plan.to_lowercase()).map(|(_, v)| v.to_string()).unwrap_or_else(|| {
        let mut c = plan.chars();
        c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
    });
    let meters = [("five_hour", "5 h", "5-hour window"), ("weekly", "week", "This week")]
        .iter()
        .filter_map(|(key, label, long)| {
            let w = acc.get(*key)?;
            let used = w.get("used").and_then(Value::as_f64).filter(|u| u.is_finite())?;
            let at = w.get("resets_at").and_then(Value::as_f64).filter(|a| a.is_finite() && *a > 0.0);
            if at.is_some_and(|a| a <= now as f64) {
                return None;
            }
            let used = used.clamp(0.0, 100.0).round() as u32;
            Some(MeterView { key: (*key).into(), label: (*label).into(), long: (*long).into(), used, hot: used >= 80, reset_ms: at.map(|a| a as u64) })
        })
        .collect();
    let mut facts = Vec::new();
    if let Some(c) = acc.get("credits").and_then(Value::as_f64).filter(|c| c.is_finite()) {
        let n = c.round().max(0.0) as u64;
        let mut grouped = String::new();
        let digits = n.to_string();
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(ch);
        }
        facts.push(say("usage.credits", format!("{grouped} {}", if n == 1 { "credit" } else { "credits" }), &[("n", &grouped)]));
    }
    if let Some(r) = acc.get("full_resets").and_then(Value::as_u64) {
        let n = r.to_string();
        facts.push(say("usage.full_resets", format!("{r} full {}", if r == 1 { "reset" } else { "resets" }), &[("n", &n)]));
    }
    Some(UsageView {
        plan: if plan.is_empty() { String::new() } else { format!("{maker} {named}").trim().to_string() },
        email,
        meters,
        facts,
    })
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

    let agent_list = agents_page(b);
    let agents_lead = (!agent_list.is_empty()).then(|| {
        let computers: BTreeSet<&str> = agent_list.iter().map(|g| g.host.as_str()).collect();
        let n: usize = agent_list.iter().map(|g| g.total as usize).sum();
        let (a, c, n_s) = (agent_list.len().to_string(), computers.len().to_string(), n.to_string());
        let en = format!(
            "{} on {} · {}",
            counted("", agent_list.len(), "agent", "agents").en,
            counted("", computers.len(), "computer", "computers").en,
            counted("", n, "session", "sessions").en
        );
        say("agents.lead", en, &[("agents", &a), ("computers", &c), ("sessions", &n_s)])
    });

    SettingsView {
        account: None,
        login: None,
        room: None,
        hub: None,
        desk_counts: Vec::new(),
        deleting: Vec::new(),
        push: None,
        agent_list,
        agents_lead,
        agents,
        desks: (!b.desks.is_empty()).then(|| counted("settings.desks", b.desks.len(), "desk", "desks")),
        devices_says: (!devices.is_empty()).then(|| counted("settings.devices", devices.len(), "device", "devices")),
        remove_unfinished: (!stuck.is_empty()).then(|| {
            if stuck.len() == 1 {
                say("settings.remove_unfinished.one", "Remove the device that never finished signing in", &[("n", "1")])
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
    fn the_agents_page_groups_by_program_and_computer() {
        let mut r = Room::default();
        let mut mac = member(&crate::views::testroom::dev("a"), "mac", "agent", 1);
        mac["platform"] = json!("claude-code");
        mac["host"] = json!("MacBook");
        mac["folder"] = json!("~/git/x");
        let mut b2 = member(&crate::views::testroom::dev("b"), "mac", "agent", 2);
        b2["platform"] = json!("claude-code");
        b2["host"] = json!("MacBook");
        let mut cx = member(&crate::views::testroom::dev("c"), "box", "agent", 3);
        cx["platform"] = json!("codex");
        r.members = vec![mac, b2, cx];
        r.session("a", None, true)["profile"]["account"] = json!({ "plan": "max", "email": "x@y.z",
            "five_hour": { "used": 81.6, "resets_at": NOW + 3_600_000 }, "weekly": { "used": 10, "resets_at": NOW - 1 } });
        r.session("a-help", Some("a"), false);
        r.session("b", None, false);
        r.session("c", None, false)["profile"]["account"] = json!({ "kind": "codex", "plan": "prolite", "credits": 1234.4, "full_resets": 1 });
        let m = r.model(NOW);
        let lens = Lens::default();
        let v = settings(&Board::new(&m, &lens, NOW));
        let names: Vec<(&str, &str, &str, u32)> = v.agent_list.iter().map(|g| (g.name.en.as_str(), g.state.en.as_str(), g.count.en.as_str(), g.total)).collect();
        assert_eq!(names, [("Claude Code on MacBook", "connected", "2 sessions, 1 helper", 3), ("Codex on an unnamed computer", "away · last seen", "1 session", 1)]);
        assert_eq!(v.agents_lead.unwrap().en, "2 agents on 2 computers · 4 sessions");
        let g = &v.agent_list[0];
        let rows: Vec<(&str, &str, &str, Option<&str>, &str)> =
            g.sessions.iter().map(|s| (s.slug.as_str(), s.desk.en.as_str(), s.folder.as_str(), s.helpers_says.as_ref().map(|t| t.en.as_str()), s.state.en.as_str())).collect();
        assert_eq!(rows, [(g.sessions[0].slug.as_str(), "Personal desk", "~/git/x", Some("+1 helper"), "connected"), (g.sessions[1].slug.as_str(), "Personal desk", "", None, "away")]);
        assert_eq!(g.remove.as_ref().unwrap().en, "Claude Code on MacBook and its 3 sessions can read nothing new after this. The others get a new key; the history stays.");
        let u = g.usage.clone().unwrap();
        assert_eq!((u.plan.as_str(), u.email.as_str()), ("Claude Max", "x@y.z"));
        assert_eq!(u.meters.iter().map(|m| (m.key.as_str(), m.used, m.hot, m.reset_ms)).collect::<Vec<_>>(), [("five_hour", 82, true, Some(NOW + 3_600_000))]);
        let u = v.agent_list[1].usage.clone().unwrap();
        assert_eq!((u.plan.as_str(), u.facts.iter().map(|t| t.en.as_str()).collect::<Vec<_>>()), ("ChatGPT Pro Lite", vec!["1,234 credits", "1 full reset"]));
    }

    #[test]
    fn a_fingerprint_without_one_is_the_ids_head_in_fours() {
        let d: Member = serde_json::from_value(member("0123456789abcdefXYZ", "", "human", 0)).unwrap();
        assert_eq!(fingerprint_of(&d), "0123 4567 89ab cdef");
    }
}
