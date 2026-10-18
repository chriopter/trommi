//! Settings: the list's rows with what each says (Agents, Teams, Devices), and the person's devices.
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
use super::{AgentSessionView, AgentView, DeviceRow, LogoutView, MeterView, PushChoice, SecurityRow, TeamCountsView, TeamDeleteLine, TeamDeleteWords, SecuritySection, SecurityView, SettingsView, Text, UsageView};
use crate::model::types::Member;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

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
        last_seen_ms: d.last_seen_ms,
        seen: None,
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
/// grouped by program and computer; under each its sessions, each with its helpers counted beside it; the most
/// sessions first, then by program and computer.
pub fn agents_page(b: &Board) -> Vec<AgentView> {
    let m = b.model;
    let me = m.my_device_id.as_deref();
    let member_of = |i: usize| b.agents[i].row.agent_device_id.as_deref().and_then(|d| m.members.get(d));
    let text = |v: &Value| v.as_str().unwrap_or("").to_string();
    let mut groups: Vec<(String, &'static str, String, Vec<usize>)> = Vec::new();
    for (i, a) in b.agents.iter().enumerate().filter(|(_, a)| !a.row.archived && !a.row.removed && !a.row.own) {
        let member = member_of(i);
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
            let tops = list.clone();
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
                    let helpers = b.helpers(i).len();
                    let team = b.teams.iter().find(|d| Some(&d.id) == a.team.as_ref()).map(|d| d.name.clone()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Personal".into());
                    let h = helpers.to_string();
                    AgentSessionView {
                        id: a.row.key.clone(),
                        slug: a.row.id.clone(),
                        name: say("session.name", a.row.name.as_str(), &[]),
                        mark: a.row.mark(),
                        team: say("agents.team", format!("{team} team"), &[("team", &team)]),
                        folder: member_of(i).map(|d| text(&d.folder)).unwrap_or_default(),
                        helpers: helpers as u32,
                        helpers_says: (helpers > 0).then(|| say("agents.helpers", format!("+{}", plural(helpers, "helper", "helpers")), &[("n", &h)])),
                        online: a.row.online,
                        state: if a.row.online { say("agents.connected", "connected", &[]) } else { say("agents.away", "away", &[]) },
                    }
                })
                .collect();
            let n_helpers: usize = tops.iter().map(|i| b.helpers(*i).len()).sum();
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
    let mut view = settings_rows(b);
    view.security = security(&view, b.lens.utc_offset_min);
    view.logout = logout(&view, false);
    view
}

fn settings_rows(b: &Board) -> SettingsView {
    let computers: BTreeMap<&str, &str> = b.model.sessions.keys()
        .filter(|s| crate::machines::is_computer(b.model, s))
        .filter_map(|s| crate::machines::granted_device(b.model, s).map(|d| (d, s.as_str()))).collect();
    let mut people: Vec<&Member> = b.model.members.values().filter(|d| d.device_role == "human" || computers.contains_key(d.device_id.as_str())).collect();
    // (of two with the same number, which a hub does not give, this device first, then by id: the same on every run)
    people.sort_by_key(|d| (d.added_entry_number, !d.is_me, d.device_id.clone()));
    let device_row = |d: &Member| {
        let mut row = row_of(d);
        row.seen = d.last_seen_ms.filter(|_| !row.online).map(|t| {
            let at = super::words::local(t, b.lens.utc_offset_min);
            let when = format!("{:04}-{:02}-{:02} {:02}:{:02}", at.year, at.month, at.day, at.hour, at.minute);
            say("device.last_seen", format!("last seen {when}"), &[("when", &when)])
        });
        if computers.contains_key(d.device_id.as_str()) {
            let host = d.host.as_str().filter(|s| !s.is_empty());
            row.name = host.map_or_else(|| "This computer".into(), |h| format!("This computer: {h}"));
            if d.is_active { row.says = say("device.approved_computer", "approved computer", &[]); }
        }
        row
    };
    let devices: Vec<DeviceRow> = people.iter().copied().filter(|d| d.is_active).map(&device_row).collect();
    let removed: Vec<DeviceRow> = people.iter().copied().filter(|d| !d.is_active).map(device_row).collect();
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
        data: None,
        account: None,
        login: None,
        room: None,
        hub: None,
        team_counts: Vec::new(),
        deleting: Vec::new(),
        push: None,
        push_choices: push_choices(),
        team_delete: team_delete_words(),
        logout: LogoutView::default(),
        security: SecurityView::default(),
        agent_list,
        agents_lead,
        agents,
        teams: (!b.teams.is_empty()).then(|| counted("settings.teams", b.teams.len(), "team", "teams")),
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

/// The choices of Push on this device (the web's `PUSH_LEVELS`).
pub fn push_choices() -> Vec<PushChoice> {
    let c = |level: &str, en: &str, detail: Option<&str>, icon: &str| PushChoice {
        level: level.into(),
        title: say(&format!("push.{level}"), en, &[]),
        detail: detail.map(|d| say(&format!("push.{level}.detail"), d, &[])),
        icon: icon.into(),
    };
    vec![
        c("all", "Yes", Some("every card"), "bell"),
        c("knocking", "Only knocking", Some("high, critical, a lost session"), "knock"),
        c("off", "No", None, "snooze"),
    ]
}

/// Log out of this device: how the person gets in again (the account's ways in, once read), a warning when this is
/// their only device, the demo's own line.
pub fn logout(s: &SettingsView, demo: bool) -> LogoutView {
    let pk = s.login.as_ref().is_some_and(|l| !l.passkeys.is_empty());
    let pw = s.login.as_ref().is_none_or(|l| l.has_password);
    let (again_key, again) = match (pk, pw) {
        (true, true) => ("logout.ask.both", "your passkey or your password"),
        (true, false) => ("logout.ask.passkey", "your passkey"),
        _ => ("logout.ask.password", "email and password"),
    };
    let (ways_key, ways) = match (pk, pw) {
        (true, true) => ("logout.only.both", "your passkey, or your email and password"),
        (true, false) => ("logout.only.passkey", "your passkey"),
        _ => ("logout.only.password", "your email and password"),
    };
    let others = s.devices.iter().any(|d| d.active && !d.this_device);
    let note = if demo {
        Some(say("logout.demo", "The demo keeps nothing; this only ends it.", &[]))
    } else if others {
        None
    } else {
        let verb = if pw { "open" } else { "opens" };
        Some(say(ways_key, format!("This is your only device. After this, {ways} (or your Emergency Kit) {verb} your account."), &[]))
    };
    LogoutView {
        title: say("logout.title", "Log out", &[]),
        ask: say(again_key, format!("Log out of this device? You can log in again with {again}."), &[]),
        note,
        meta: say("logout.meta", "Everything Trommi keeps on this device is deleted here; your account and your other devices stay as they are.", &[]),
        go: say("logout.go", "Log Out", &[]),
        cancel: say("logout.cancel", "Cancel", &[]),
    }
}

/// The words of a team's delete confirm that every team shares.
pub fn team_delete_words() -> TeamDeleteWords {
    TeamDeleteWords {
        lead: say("team_delete.lead", "This goes for good, on every device:", &[]),
        empty: say("team_delete.empty", "Nothing stands on this team: only its name goes.", &[]),
        small: say("team_delete.small", "The server drops the encrypted data within 30 days. Your agents' computers keep nothing of it they can still open. This cannot be undone.", &[]),
        hold: say("team_delete.hold", "Hold to Delete", &[]),
        cancel: say("team_delete.cancel", "Cancel", &[]),
        deleting: say("team_delete.deleting", "Deleting…", &[]),
    }
}

/// One kind a team holds: its key, how many (none while not counted), its word for one and for many, what happens to them.
type Row<'a> = (&'a str, Option<u64>, &'a str, &'a str, Option<&'a str>);

/// A team's confirm: its title and a line per kind it holds (none of a kind, or not yet counted, is left out).
pub fn team_delete_ask(c: &mut TeamCountsView, name: &str) {
    let name = if name.trim().is_empty() { "Personal" } else { name.trim() };
    c.title = say("team_delete.title", format!("Delete {name}?"), &[("name", name)]);
    // (key, how many, one, many, what happens to them)
    let rows: [Row; 10] = [
        ("sessions", Some(c.sessions), "session", "sessions", Some("archived")),
        ("helpers", Some(c.helpers), "helper", "helpers", Some("archived")),
        ("archived", Some(c.archived), "archived session", "archived sessions", None),
        ("agents", Some(c.agents), "agent removed from your account", "agents removed from your account", Some("their keys stop working")),
        ("open_cards", Some(c.open_cards), "open card", "open cards", Some("shredded")),
        ("done_cards", Some(c.done_cards), "done card", "done cards", None),
        ("board", c.board, "thing on the Scribble Board", "things on the Scribble Board", None),
        ("files", Some(c.files), "file", "files", Some("pictures, Artifacts and share links, deleted at once")),
        ("goals", Some(c.goals), "goal", "goals", None),
        ("knowledge", Some(c.knowledge), "entry of its knowledge", "entries of its knowledge", None),
    ];
    c.lines = rows.iter().filter_map(|(key, n, one, many, rest)| {
        let n = n.filter(|n| *n > 0)?;
        Some(TeamDeleteLine {
            key: (*key).into(),
            n,
            word: say(&format!("team_delete.{key}"), if n == 1 { *one } else { *many }, &[("n", &n.to_string())]),
            rest: rest.map(|r| say(&format!("team_delete.{key}.rest"), r, &[])),
        })
    }).collect();
}

/// Settings → Security from the rest of Settings (its devices, its Agents row, the room and hub the App adds): the
/// account key and the Emergency Kit, every device that is in with its fingerprint, the agents, the self test.
pub fn security(s: &SettingsView, utc_offset_min: i32) -> SecurityView {
    let everything = || Some(say("security.everything", "everything", &[]));
    let epoch = s.key_epoch.to_string();
    let key_detail = match s.hub.as_deref().filter(|h| !h.is_empty()) {
        Some(hub) => say("security.key.detail_hub", format!("key epoch {epoch} · {hub}"), &[("epoch", &epoch), ("hub", hub)]),
        None => say("security.key.detail", format!("key epoch {epoch}"), &[("epoch", &epoch)]),
    };
    let key = SecuritySection {
        id: "key".into(),
        label: say("security.key.label", "Account key", &[]),
        rows: vec![
            SecurityRow {
                id: "key".into(),
                title: say("security.key", "Account key", &[]),
                fingerprint: s.room.as_deref().filter(|r| !r.is_empty()).map_or_else(|| "unknown".into(), |r| format!("{}…", r.chars().take(16).collect::<String>())),
                detail: Some(key_detail),
                says: everything(),
                icon: "key".into(),
                ..Default::default()
            },
            SecurityRow { id: "kit".into(), title: say("security.kit", "Emergency Kit", &[]), icon: "page".into(), open: "kit".into(), ..Default::default() },
        ],
        notes: vec![say("security.key.note", "Opens everything; it is also your recovery key. The hub keeps it only sealed: a way in of your account opens it on a new device.", &[])],
        strong: None,
    };
    let devices = SecuritySection {
        id: "devices".into(),
        label: say("security.devices.label", "Authorized devices", &[]),
        rows: s.devices.iter().filter(|d| d.active).map(|d| SecurityRow {
            id: d.id.clone(),
            title: Text { key: String::new(), args: Default::default(), en: d.name.clone() },
            mark: d.this_device.then(|| say("security.this_device", "this device", &[])),
            fingerprint: d.fingerprint.clone(),
            detail: Some(match d.added_ms {
                Some(t) => { let when = super::words::date(t, utc_offset_min); say("security.added", format!("added {when}"), &[("when", &when)]) }
                None => say("security.added_unknown", "added: unknown", &[]),
            }),
            says: everything(),
            icon: if d.icon.is_empty() { "laptop".into() } else { d.icon.clone() },
            open: "device".into(),
        }).collect(),
        notes: vec![say("security.devices.note", "Each of your devices reads everything, also what came before it was added.", &[])],
        strong: None,
    };
    let count = s.agents.as_ref().map_or_else(|| "none".to_string(), |t| t.en.clone());
    let agents = SecuritySection {
        id: "agents".into(),
        label: say("security.agents.label", "Agents", &[]),
        rows: vec![SecurityRow {
            id: "agents".into(),
            title: say("security.agents", "Agents", &[]),
            detail: Some(say("security.agents.detail", format!("{count}: each with its own key"), &[("agents", &count)])),
            says: Some(say("security.own_session", "own session", &[])),
            icon: "terminal".into(),
            open: "agents".into(),
            ..Default::default()
        }],
        notes: vec![say("security.agents.note", "An agent reads only its own session and the helper sessions it starts; a helper reads only its own.", &[])],
        strong: Some(say("security.hub", "Nobody else, not the hub either: it sees which device is in which group, when, and how much.", &[])),
    };
    let check = SecuritySection {
        id: "check".into(),
        label: say("security.check.label", "Check", &[]),
        rows: vec![SecurityRow {
            id: "proof".into(),
            title: say("security.proof", "Self Test", &[]),
            says: Some(say("security.proof.where", "runs on this device", &[])),
            icon: "tick".into(),
            open: "proof".into(),
            ..Default::default()
        }],
        notes: Vec::new(),
        strong: None,
    };
    SecurityView {
        lead: say("security.lead", "Every key that opens your data, and the self test.", &[]),
        sections: vec![key, devices, agents, check],
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
    fn computer_approval_has_a_devices_row_and_never_a_conversation() {
        let mut m = crate::model::Model::new();
        m.members.insert("computer-device".into(), Member { device_id:"computer-device".into(), device_role:"agent".into(), is_active:true, host:json!("workstation"), ..Default::default() });
        let session = m.session_of("computer");
        session.agent_device_id = Some("computer-device".into());
        session.group_id = Some("held".into());
        session.settings = Some(json!({"machine":{"device":"computer-device"}}));
        assert_eq!(crate::sessions::session_list(&m, false).len(), 1, "agent settings cannot hide a conversation");
        m.human.insert("session/computer".into(), crate::model::types::HumanRegister { value:json!({"machine":{"device":"computer-device"}}), ..Default::default() });
        assert!(crate::sessions::session_list(&m, false).is_empty());
        let lens = Lens::default();
        let v = settings(&Board::new(&m, &lens, NOW));
        assert_eq!(v.devices.len(), 1);
        assert_eq!(v.devices[0].name, "This computer: workstation");
        assert_eq!(v.devices[0].says.en, "approved computer");
        assert!(v.agent_list.is_empty());
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
        r.session("b", None, true);
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
        assert!(v.teams.is_none());
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
        r.sessions[0]["status_lines"] = json!([{ "id": "l", "label": "Icons", "state": "working", "helper": "Icons", "updated_at": NOW - 60_000 }]);
        r.session("b", None, false);
        r.session("c", None, false)["profile"]["account"] = json!({ "kind": "codex", "plan": "prolite", "credits": 1234.4, "full_resets": 1 });
        let m = r.model(NOW);
        let lens = Lens::default();
        let v = settings(&Board::new(&m, &lens, NOW));
        let names: Vec<(&str, &str, &str, u32)> = v.agent_list.iter().map(|g| (g.name.en.as_str(), g.state.en.as_str(), g.count.en.as_str(), g.total)).collect();
        assert_eq!(names, [("Claude Code on MacBook", "connected", "2 sessions, 1 helper", 2), ("Codex on an unnamed computer", "away · last seen", "1 session", 1)]);
        assert_eq!(v.agents_lead.unwrap().en, "2 agents on 2 computers · 3 sessions");
        let g = &v.agent_list[0];
        let rows: Vec<(&str, &str, &str, Option<&str>, &str)> =
            g.sessions.iter().map(|s| (s.slug.as_str(), s.team.en.as_str(), s.folder.as_str(), s.helpers_says.as_ref().map(|t| t.en.as_str()), s.state.en.as_str())).collect();
        assert_eq!(rows, [(g.sessions[0].slug.as_str(), "Personal team", "~/git/x", Some("+1 helper"), "connected"), (g.sessions[1].slug.as_str(), "Personal team", "", None, "away")]);
        assert_eq!(g.remove.as_ref().unwrap().en, "Claude Code on MacBook and its 2 sessions can read nothing new after this. The others get a new key; the history stays.");
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

    #[test]
    fn offline_contact_times_are_words_from_the_core_in_the_persons_zone() {
        let mut m = crate::model::Model::new();
        m.members.insert("phone".into(), Member { device_id: "phone".into(), device_name: "Phone".into(), device_role: "human".into(), is_active: true, last_seen_ms: Some(60_000), ..Default::default() });
        let lens = Lens { utc_offset_min: 120, ..Default::default() };
        let view = settings(&Board::new(&m, &lens, 100_000));
        assert_eq!(view.devices[0].last_seen_ms, Some(60_000));
        assert_eq!(view.devices[0].seen.as_ref().unwrap().en, "last seen 1970-01-01 02:01");
        m.members.get_mut("phone").unwrap().is_online = true;
        let view = settings(&Board::new(&m, &lens, 100_000));
        assert!(view.devices[0].seen.is_none(), "an online status does not show an old contact time");
    }

    #[test]
    fn security_without_room_hub_devices_or_agents_still_says_every_group() {
        let s = SettingsView::default();
        let v = security(&s, 0);
        let ids: Vec<&str> = v.sections.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["key", "devices", "agents", "check"]);
        assert_eq!(v.sections[0].rows[0].fingerprint, "unknown");
        assert_eq!(v.sections[0].rows[0].detail.as_ref().unwrap().en, "key epoch 0");
        assert!(v.sections[1].rows.is_empty(), "no device, no row");
        assert_eq!(v.sections[2].rows[0].detail.as_ref().unwrap().en, "none: each with its own key");
        assert_eq!(v.sections[2].strong.as_ref().unwrap().en, "Nobody else, not the hub either: it sees which device is in which group, when, and how much.");
        assert_eq!(v.sections[3].rows[0].open, "proof");
    }

    #[test]
    fn security_lists_the_devices_that_are_in_with_fingerprint_and_day() {
        let row = |id: &str, me: bool, active: bool, added: Option<u64>| DeviceRow { id: id.into(), name: id.into(), this_device: me, active, fingerprint: "ab12 cd34".into(), icon: "phone".into(), added_ms: added, ..Default::default() };
        let s = SettingsView {
            room: Some("e6e33dc6bdcdcdc23d9ce4fcf5028a78".into()),
            hub: Some("https://hub.trommi.com".into()),
            key_epoch: 88,
            agents: Some(say("settings.agents", "2 agents · 7 sessions", &[])),
            devices: vec![row("Phone", true, true, Some(1_790_000_000_000)), row("Old", false, false, None), row("Mac", false, true, None)],
            ..Default::default()
        };
        let v = security(&s, 0);
        let key = &v.sections[0].rows[0];
        assert_eq!(key.fingerprint, "e6e33dc6bdcdcdc2…");
        assert_eq!(key.detail.as_ref().unwrap().en, "key epoch 88 · https://hub.trommi.com");
        let devices = &v.sections[1].rows;
        assert_eq!(devices.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(), ["Phone", "Mac"], "a removed device is not listed");
        assert_eq!(devices[0].mark.as_ref().unwrap().en, "this device");
        assert!(devices[1].mark.is_none());
        assert_eq!(devices[0].detail.as_ref().unwrap().en, "added 21 Sep 2026");
        assert_eq!(devices[1].detail.as_ref().unwrap().en, "added: unknown");
        assert_eq!(v.sections[2].rows[0].detail.as_ref().unwrap().en, "2 agents · 7 sessions: each with its own key");
    }

    #[test]
    fn logout_says_how_to_get_in_again_and_warns_on_the_only_device() {
        let me = DeviceRow { id: "a".into(), this_device: true, active: true, ..Default::default() };
        let mut s = SettingsView { devices: vec![me.clone()], ..Default::default() };
        let v = logout(&s, false);
        assert_eq!(v.ask.en, "Log out of this device? You can log in again with email and password.");
        assert_eq!(v.note.unwrap().en, "This is your only device. After this, your email and password (or your Emergency Kit) open your account.");
        s.devices.push(DeviceRow { id: "b".into(), active: true, ..Default::default() });
        assert!(logout(&s, false).note.is_none(), "another device: no warning");
        s.login = Some(crate::views::LoginView { has_password: false, passkeys: vec![Default::default()], ..Default::default() });
        assert_eq!(logout(&s, false).ask.en, "Log out of this device? You can log in again with your passkey.");
        assert_eq!(logout(&SettingsView::default(), true).note.unwrap().en, "The demo keeps nothing; this only ends it.");
    }

    #[test]
    fn a_team_delete_names_only_what_the_team_holds() {
        let mut c = TeamCountsView { sessions: 1, open_cards: 3, board: None, ..Default::default() };
        team_delete_ask(&mut c, "Fernly");
        assert_eq!(c.title.en, "Delete Fernly?");
        let lines: Vec<(String, u64, String)> = c.lines.iter().map(|l| (l.key.clone(), l.n, l.word.en.clone())).collect();
        assert_eq!(lines, [("sessions".into(), 1, "session".into()), ("open_cards".into(), 3, "open cards".into())]);
        let mut empty = TeamCountsView::default();
        team_delete_ask(&mut empty, "");
        assert_eq!(empty.title.en, "Delete Personal?");
        assert!(empty.lines.is_empty());
        assert_eq!(push_choices().iter().map(|c| c.level.as_str()).collect::<Vec<_>>(), ["all", "knocking", "off"]);
    }
}
