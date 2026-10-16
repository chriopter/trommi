//! The session list: which sessions the person's devices show, under which name, online or gone, archived or not,
//! under which parent and in which order. One rule for every app (the web's sidebar, the iPhone's, the Linux app),
//! so that every device of the person shows the same list.
//!
//! The rules are the web app's `BoardState.agents` of before (app/web/public/app.mjs), which the iPhone had copied
//! with gaps (no slot replacement, another fallback name):
//! - one entry per session key (`session_id`, or the agent device for a record without one); a record that only a
//!   register of another session's agent device made is none; a removed session with no card is none;
//! - the name: the person's label, else what the agent calls itself (`profile.agent_name`), else the device's name,
//!   else "Loading …" while the room is still read and the session's profile has not come, else its short id;
//! - archived: the person's choice (`session/<id>` register, `archived`), else a helper its agent closed with no open
//!   question, else a helper whose main is archived while its agent is away (not one he fetched back); a main session
//!   whose agent is connected again shows whatever hid it;
//! - one entry per agent session in a folder (slot): a connector paired again in its folder is a new agent device
//!   with a new main session; while one session of the slot is connected (else: the newest) the others are archived
//!   as replaced (`replaced_by` names the one that took the place), never one the person fetched back, whatever
//!   cards of it were read so far; two sessions whose devices name different agent identities (`device/<id>`
//!   `agent`, spec/v1.md 9.3.3) are two agents and never replace each other. A replaced session's helpers stand
//!   under the session that took its place (and go with it to the archive while their agent is away), the crown
//!   moves there too, and their open cards stay on the Desk ([`archived_sessions`] leaves them out);
//! - the parent: the person's choice, else the session's own claim (a helper an agent opened counts only under a
//!   session of that agent); a helper stands on its main's team;
//! - the order: the person's places (`position`), else the order of the session ids.
use crate::model::types::Session;
use crate::model::Model;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A session's short id on the board: the start of its key (its address /chat/<id>).
pub const SESSION_ID_LEN: usize = 12;

/// One entry of the session list.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct SessionRow {
    /// The board's id of the session (`agent_session_id` when readable, else the key's first 12 characters).
    pub id: String,
    /// The session's key in the model: its session id (or the agent device of a record without one).
    pub key: String,
    pub session_id: Option<String>,
    pub agent_device_id: Option<String>,
    /// What shows: the person's label, else `given`.
    pub name: String,
    /// What the agent calls itself (or the fallback).
    pub given: String,
    pub label: String,
    pub icon: String,
    pub online: bool,
    pub offline_since: Option<u64>,
    /// The parent entry's `id`, if any.
    pub parent: Option<String>,
    /// The parent entry's key, if any.
    pub parent_key: Option<String>,
    pub archived: bool,
    /// Archived because a newer pairing in the same folder took its place.
    pub replaced: bool,
    /// The entry that took its place (its `id`), for a replaced one.
    pub replaced_by: Option<String>,
    pub removed: bool,
    pub position: f64,
    pub placed: bool,
    pub team: String,
    pub starred: bool,
    pub main: bool,
    /// A session of a person's own device (never deleted from the board).
    pub own: bool,
    /// How many of its questions are open.
    pub waiting: usize,
    /// The person fetched it back from the archive.
    pub fetched: bool,
    pub last_activity_at: u64,
    /// Its link at the model's clock (none for an archived or removed session): status.rs.
    pub link: Option<crate::status::LinkView>,
    /// Why it is stopped (the red hand), at the model's clock: status.rs.
    pub blocked: Option<crate::status::Blocked>,
    /// A status line of it says "working".
    pub working: bool,
    /// Quiet: its last sign of life, when that was long ago (status.rs `QUIET_MS`).
    pub quiet_since: Option<u64>,
    /// When the agent process behind it last said anything, in any of its sessions.
    pub device_active: u64,
    /// When its link, red hand or quiet hint may change by the clock alone.
    pub recheck_at: Option<u64>,
    /// What each of its status lines shows now, by line id (status.rs `line_state`).
    pub lines: BTreeMap<String, String>,
    /// Where it stands, as a card's page names its sender: its team (when there are two or more), its main session
    /// (for a helper), itself. `session_path`.
    pub path: Vec<String>,
    /// The model it runs on as its heading says it (`model_label`: "Opus 5.5", "GPT-6.1"); empty when it named none.
    pub model_label: String,
}

fn is_hex_id(s: &str) -> bool {
    s.len() >= 12
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn key_of(s: &Session) -> String {
    if s.session_id.is_empty() {
        s.agent_device_id.clone().unwrap_or_default()
    } else {
        s.session_id.clone()
    }
}

/// The board's id of a session.
pub fn agent_id_of(s: &Session) -> String {
    match s.agent_session_id.as_deref() {
        Some(a) if !a.is_empty() && !is_hex_id(a) => a.to_string(),
        _ => key_of(s).chars().take(SESSION_ID_LEN).collect(),
    }
}

/// The sessions as the board knows them: one per key, without records only a register of another session's agent
/// device made.
pub fn sessions_of(m: &Model) -> Vec<&Session> {
    let agents: BTreeSet<&str> = m
        .sessions
        .values()
        .filter(|s| s.group_id.is_some())
        .flat_map(|s| s.agent_device_id.iter().chain(s.agent_device_ids.iter()))
        .map(String::as_str)
        .collect();
    m.sessions
        .iter()
        .filter(|(k, s)| **k == key_of(s) && !(s.group_id.is_none() && agents.contains(k.as_str())))
        .map(|(_, s)| s)
        .collect()
}

/// The person's settings of a session (`session/<key>` among the room registers), else the session's own.
/// A main session's settings an older web kept under its agent device's id (`session/<device>`) count as its own.
fn settings_of(m: &Model, s: &Session, key: &str) -> serde_json::Map<String, Value> {
    let mut keys = vec![key.to_string()];
    if s.parent_session_id.is_none() {
        if let Some(d) = &s.agent_device_id {
            keys.push(d.clone());
        }
    }
    for k in keys {
        if let Some(r) = m.human.get(&format!("session/{k}")) {
            if let Value::Object(o) = &r.value {
                return o.clone();
            }
        }
    }
    match &s.settings {
        Some(Value::Object(o)) => o.clone(),
        _ => serde_json::Map::new(),
    }
}

/// The name a device gave itself (its register `device/<id>`): `device_name`, else the last part of its folder.
pub(crate) fn device_name_of(m: &Model, id: Option<&str>) -> String {
    let Some(id) = id else { return String::new() };
    let member = m.members.get(id);
    let reg = m.device_registers.get(id);
    if let Some(n) = member
        .map(|x| x.device_name.as_str())
        .filter(|n| !n.is_empty())
    {
        return n.to_string();
    }
    if let Some(n) = reg
        .and_then(|r| r.get("device_name"))
        .and_then(Value::as_str)
        .filter(|n| !n.is_empty())
    {
        return n.to_string();
    }
    let folder = member
        .and_then(|x| x.folder.as_str())
        .filter(|f| !f.is_empty())
        .or_else(|| reg.and_then(|r| r.get("folder")).and_then(Value::as_str))
        .unwrap_or("");
    folder
        .split(['/', '\\'])
        .rfind(|p| !p.is_empty())
        .unwrap_or("")
        .to_string()
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|x| x != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn text(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The parent a session claims (`profile.parent_session`): a helper an agent opened itself counts only under a
/// session that agent (or the agent a human handed the helper to) is assigned to.
fn parent_claim<'a>(m: &'a Model, s: &'a Session) -> Option<&'a str> {
    // the group's own parent first (5.2.5): a helper session's group names its main
    if let Some(p) = s.parent_session_id.as_deref() {
        return Some(p);
    }
    let want = text(s.profile.as_ref().and_then(|p| p.get("parent_session")))?;
    if !s.created_by_agent {
        return Some(want);
    }
    let parent = m.sessions.get(want)?;
    if parent.session_id == s.session_id {
        return None;
    }
    let ok = parent.agent_device_ids.iter().any(|a| {
        s.creator_device_id.as_deref() == Some(a.as_str()) || s.agent_device_ids.contains(a)
    });
    ok.then_some(want)
}

/// A model's id as a session's heading shows it, short and quiet: "claude-opus-5-5" is "Opus 5.5",
/// "claude-sonnet-4-5-20250929" "Sonnet 4.5", "gpt-6.1" "GPT-6.1", "gpt-5.1-codex" "GPT-5.1 Codex". An id of another
/// form stays as it is; none is "".
pub fn model_label(id: &str) -> String {
    let id = id.trim();
    if id.is_empty() {
        return String::new();
    }
    let lower = id.to_ascii_lowercase();
    // (Claude: family and version; a date and a context tag such as "[1m]" are left out)
    if let Some(rest) = lower.strip_prefix("claude-") {
        let rest = rest.split('[').next().unwrap_or(rest);
        let parts: Vec<&str> = rest.split('-').filter(|p| !(p.len() == 8 && p.bytes().all(|b| b.is_ascii_digit()))).collect();
        if let Some(fam) = parts.iter().position(|p| p.chars().all(|c| c.is_ascii_alphabetic())) {
            let name = parts[fam];
            let version: Vec<&str> = parts.iter().filter(|p| p.chars().all(|c| c.is_ascii_digit())).copied().collect();
            let mut out = name[..1].to_ascii_uppercase() + &name[1..];
            if !version.is_empty() {
                out.push(' ');
                out.push_str(&version.join("."));
            }
            return out;
        }
    }
    if let Some(rest) = lower.strip_prefix("gpt-") {
        let mut words = rest.split('-');
        let mut out = format!("GPT-{}", words.next().unwrap_or(""));
        for w in words {
            out.push(' ');
            out.push_str(&(w[..1].to_ascii_uppercase() + &w[1..]));
        }
        return out;
    }
    id.to_string()
}

/// The session list. `reading`: the room is still being read (a session whose profile has not come shows
/// "Loading …" rather than its id).
pub fn session_list(m: &Model, reading: bool) -> Vec<SessionRow> {
    let (mut out, list) = rows_of(m, reading);
    // Each session's link, red hand and quiet hint, at the model's clock (status.rs).
    let ctx = crate::status::Context::new(m, m.clock);
    for (a, s) in out.iter_mut().zip(list.iter()) {
        let st = crate::status::status_of(&ctx, s, a.archived, a.removed, a.online, a.parent.is_some());
        a.link = st.link;
        a.blocked = st.blocked;
        a.working = st.working;
        a.quiet_since = st.quiet_since;
        a.device_active = st.device_active;
        a.recheck_at = st.recheck_at;
        a.lines = st.lines;
    }
    out.sort_by(|a, b| {
        a.position
            .partial_cmp(&b.position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    session_paths(m, &mut out);
    out
}

/// The teams that are there (a register `team/<id>` with a value), in the menu's order: the place he dragged each to
/// (`order`), else the first team ("main") first, then by age. Each with its name ("Personal" when it has none).
pub fn teams_in_order(m: &Model) -> Vec<(String, String)> {
    let mut d: Vec<(String, Option<f64>, u64, String)> = m
        .human
        .iter()
        .filter_map(|(k, r)| k.strip_prefix("desk/").map(|id| (id, &r.value)))
        .filter(|(_, v)| v.is_object())
        .map(|(id, v)| {
            let name = v.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Personal");
            (id.to_string(), v.get("order").and_then(Value::as_f64), v.get("created_at").and_then(Value::as_u64).unwrap_or(0), name.to_string())
        })
        .collect();
    let ordered = d.iter().any(|x| x.1.is_some());
    d.sort_by(|a, b| {
        if ordered {
            a.1.unwrap_or(f64::INFINITY).partial_cmp(&b.1.unwrap_or(f64::INFINITY)).unwrap_or(std::cmp::Ordering::Equal).then(a.2.cmp(&b.2))
        } else {
            (a.0 != "main").cmp(&(b.0 != "main")).then(a.2.cmp(&b.2))
        }
    });
    d.into_iter().map(|(id, _, _, name)| (id, name)).collect()
}

/// Each row's path (`SessionRow::path`): its team's name while there are two teams or more (a team that is gone counts
/// as the first), its main session's name for a helper, its own name.
fn session_paths(m: &Model, out: &mut [SessionRow]) {
    let teams = teams_in_order(m);
    let names: HashMap<String, String> = out.iter().map(|a| (a.id.clone(), a.name.clone())).collect();
    for a in out.iter_mut() {
        let mut p = Vec::new();
        if teams.len() > 1 {
            let d = teams.iter().find(|(id, _)| *id == a.team).or(teams.first());
            if let Some((_, n)) = d {
                p.push(n.clone());
            }
        }
        if let Some(main) = a.parent.as_ref().and_then(|x| names.get(x)) {
            p.push(main.clone());
        }
        p.push(a.name.clone());
        a.path = p;
    }
}

/// The sessions the list shows as archived (their session ids): the Desk leaves their cards out, so that a card
/// stands on the Desk exactly when its session stands in the list. A replaced session stands in the list through
/// the one that took its place: its open cards stay on the Desk.
pub fn archived_sessions(m: &Model) -> Vec<String> {
    rows_of(m, false)
        .0
        .into_iter()
        .filter(|r| r.archived && !r.replaced)
        .filter_map(|r| r.session_id)
        .collect()
}

/// The rows in the list's order before the places, with the session each was made from (no status yet).
fn rows_of(m: &Model, reading: bool) -> (Vec<SessionRow>, Vec<&Session>) {
    let all = sessions_of(m);
    let mut dev_to_agent: HashMap<String, String> = HashMap::new();
    let mut agent_to_dev: HashMap<String, String> = HashMap::new();
    // (two sessions naming the same id, a connector's bug or a forged register: the one active last holds it)
    let mut holder: HashMap<String, (u64, String)> = HashMap::new();
    for s in &all {
        let id = agent_id_of(s);
        dev_to_agent.insert(key_of(s), id.clone());
        let mine = (s.last_activity_at, key_of(s));
        match holder.get(&id) {
            Some(had) if *had >= mine => {}
            _ => {
                holder.insert(id.clone(), mine);
            }
        }
    }
    for (id, (_, key)) in holder {
        agent_to_dev.insert(id, key);
    }
    let crown = m.human.get("crown").and_then(|r| {
        let v = &r.value;
        text(v.get("session_id"))
            .or_else(|| text(v.get("agent_device_id")))
            .map(String::from)
    });
    // (a session removed with no card is none; so is a record without an agent device and without a card: a register
    // of a session this device does not hold made it)
    let list: Vec<&Session> = all
        .into_iter()
        .filter(|s| !crate::machines::is_computer(m, &s.session_id))
        .filter(|s| (s.is_active && s.agent_device_id.is_some()) || !s.card_ids.is_empty())
        .collect();
    let me = m.my_device_id.clone().unwrap_or_default();
    let mut out: Vec<SessionRow> = Vec::with_capacity(list.len());
    let mut slots: Vec<Option<Slot>> = Vec::with_capacity(list.len());
    for (i, s) in list.iter().enumerate() {
        let key = key_of(s);
        let set = settings_of(m, s, &key);
        let p = s.profile.clone().unwrap_or_default();
        let id = dev_to_agent
            .get(&key)
            .cloned()
            .unwrap_or_else(|| key.chars().take(SESSION_ID_LEN).collect());
        let claimed = parent_claim(m, s);
        let wanted: Option<String> = if set.contains_key("parent") {
            text(set.get("parent")).map(String::from)
        } else {
            claimed.map(|c| {
                dev_to_agent
                    .get(c)
                    .cloned()
                    .unwrap_or_else(|| c.to_string())
            })
        };
        let parent = wanted.filter(|w| agent_to_dev.contains_key(w));
        let closed_child = truthy(p.get("closed_at"))
            && truthy(p.get("parent_session"))
            && s.open_card_ids.is_empty();
        // (the person's choice; else a helper its agent closed, or a group archived at the hub, by its agent or on a
        // device of the person: also when this device missed the close itself, as it reads archived groups lazily)
        let chosen = if set.contains_key("archived") {
            truthy(set.get("archived"))
        } else {
            closed_child || s.group_archived
        };
        let back = s.is_online && s.is_active && s.parent_session_id.is_none() && parent.is_none();
        let dev = s.agent_device_id.as_deref().and_then(|d| m.members.get(d));
        let profile_known = s.registers.contains_key("profile");
        let given = text(p.get("agent_name"))
            .map(String::from)
            .or_else(|| (!s.device_name.is_empty()).then(|| s.device_name.clone()))
            .or_else(|| {
                Some(device_name_of(m, s.agent_device_id.as_deref())).filter(|n| !n.is_empty())
            })
            // (a helper session without a device of its own shows its opener's, as the session's facts do)
            .or_else(|| {
                Some(device_name_of(m, s.creator_device_id.as_deref())).filter(|n| !n.is_empty())
            })
            .unwrap_or_else(|| {
                if reading && !profile_known {
                    "Loading …".to_string()
                } else {
                    id.clone()
                }
            });
        let label = text(set.get("name")).unwrap_or("").to_string();
        let icon = text(set.get("icon"))
            .or_else(|| text(p.get("icon")))
            .unwrap_or("")
            .to_string();
        let position = set
            .get("position")
            .and_then(Value::as_f64)
            .filter(|x| x.is_finite());
        let own = s.agent_device_id.as_deref() == Some(me.as_str())
            || dev.is_some_and(|d| d.device_role == "human");
        out.push(SessionRow {
            id: id.clone(),
            key: key.clone(),
            session_id: (!s.session_id.is_empty()).then(|| s.session_id.clone()),
            agent_device_id: s.agent_device_id.clone(),
            name: if label.is_empty() {
                given.clone()
            } else {
                label.clone()
            },
            given,
            label,
            icon,
            online: s.is_online,
            offline_since: s.offline_since,
            parent_key: parent.as_ref().and_then(|p| agent_to_dev.get(p).cloned()),
            parent,
            archived: chosen && !back,
            replaced: false,
            removed: !s.is_active && !s.group_archived,
            position: position.unwrap_or(i as f64),
            placed: position.is_some(),
            team: text(set.get("desk")).unwrap_or("").to_string(),
            model_label: model_label(text(p.get("model")).unwrap_or("")),
            starred: crown.as_deref() == Some(key.as_str())
                || (crown.is_some() && crown == s.agent_device_id),
            main: truthy(p.get("is_main")),
            own,
            waiting: s.open_card_ids.len(),
            fetched: set.contains_key("archived") && !truthy(set.get("archived")),
            last_activity_at: s.last_activity_at,
            ..Default::default()
        });
        let folder = dev
            .and_then(|d| d.folder.as_str())
            .filter(|f| !f.is_empty());
        let agent = s
            .agent_device_id
            .as_deref()
            .and_then(|d| m.device_registers.get(d))
            .and_then(|r| r.get("agent"))
            .and_then(Value::as_str)
            .filter(|a| a.len() == 16 && a.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(String::from);
        slots.push(match (s.parent_session_id.is_none(), folder) {
            (true, Some(f)) => {
                let host = dev.and_then(|d| d.host.as_str()).unwrap_or("");
                Some(Slot {
                    place: format!("{host}\n{f}"),
                    added: dev.map(|d| d.added_entry_number).unwrap_or(0),
                    agent,
                })
            }
            _ => None,
        });
    }
    replace_in_slots(&mut out, &slots);
    // A helper of a main session the person archived goes to the archive with it while its agent is away, unless
    // he fetched the helper back (archiving a coordinator brought its hidden helpers back as "gone" rows).
    let shelved: BTreeSet<String> = out
        .iter()
        .filter(|a| a.archived && a.parent.is_none())
        .map(|a| a.id.clone())
        .collect();
    for a in out.iter_mut() {
        if !a.archived && !a.online && !a.fetched && a.parent.as_ref().is_some_and(|p| shelved.contains(p)) {
            a.archived = true;
        }
    }
    // A helper stands on its main's team; a session without a team on the first one ("main").
    let teams: HashMap<String, String> = out
        .iter()
        .map(|a| {
            (
                a.id.clone(),
                if a.team.is_empty() {
                    "main".to_string()
                } else {
                    a.team.clone()
                },
            )
        })
        .collect();
    for a in out.iter_mut() {
        match a.parent.as_ref().and_then(|p| teams.get(p)) {
            Some(d) => a.team = d.clone(),
            None if a.team.is_empty() => a.team = "main".into(),
            None => {}
        }
    }
    (out, list)
}

/// Where a main session's agent runs: its machine and folder, when its device was paired (the room's entry
/// number), and who the agent session is across its devices (`device/<id>` `agent`), if its connector names it.
struct Slot {
    place: String,
    added: u64,
    agent: Option<String>,
}

/// One entry per agent session in a folder: see the module's rules. Whether a session is replaced depends only on
/// the slots, the links and the person's choices, never on how many of its cards were read so far (archived groups
/// are read lazily: a count that grows while reading made the list differ from load to load).
fn replace_in_slots(out: &mut [SessionRow], slots: &[Option<Slot>]) {
    let mut by_place: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, s) in slots.iter().enumerate() {
        if let Some(slot) = s {
            by_place.entry(slot.place.as_str()).or_default().push(i);
        }
    }
    // (connected first, then the newest pairing, then the one active last; the key settles a tie)
    let rank = |i: usize| {
        let a = &out[i];
        (a.online, slots[i].as_ref().map_or(0, |s| s.added), a.last_activity_at, a.key.clone())
    };
    // two sessions are one agent's unless both name who they are and that differs
    let same_agent = |i: usize, j: usize| match (&slots[i].as_ref().unwrap().agent, &slots[j].as_ref().unwrap().agent) {
        (Some(a), Some(b)) => a == b,
        _ => true,
    };
    let mut by: Vec<Option<usize>> = vec![None; out.len()];
    for same in by_place.values() {
        if same.len() < 2 {
            continue;
        }
        for &i in same {
            if out[i].online || out[i].fetched {
                continue;
            }
            // (an archived session takes no place; one archived itself still has its helpers and crown follow)
            let best = same
                .iter()
                .copied()
                .filter(|&j| j != i && !out[j].archived && same_agent(i, j))
                .max_by_key(|&j| rank(j));
            if let Some(j) = best.filter(|&j| rank(j) > rank(i)) {
                by[i] = Some(j);
            }
        }
    }
    // the session that took the place in the end (a replaced one may have replaced another before)
    let last = |mut i: usize| {
        for _ in 0..by.len() {
            match by[i] {
                Some(j) => i = j,
                None => break,
            }
        }
        i
    };
    let mut took: HashMap<String, usize> = HashMap::new();
    for i in 0..out.len() {
        if by[i].is_none() {
            continue;
        }
        let k = last(i);
        took.insert(out[i].id.clone(), k);
        // (one the person archived stays his choice: its cards leave the Desk)
        if !out[i].archived {
            out[i].archived = true;
            out[i].replaced = true;
        }
        out[i].replaced_by = Some(out[k].id.clone());
        // the crown follows the session that took the place
        if out[i].starred {
            out[i].starred = false;
            out[k].starred = true;
        }
    }
    // a replaced session's helpers stand under the one that took its place; away with their agent, they go with it
    let moves: Vec<(usize, usize)> = (0..out.len())
        .filter_map(|i| out[i].parent.as_ref().and_then(|p| took.get(p)).map(|&k| (i, k)))
        .collect();
    for (i, k) in moves {
        let (id, key) = (out[k].id.clone(), out[k].key.clone());
        let a = &mut out[i];
        a.parent = Some(id.clone());
        a.parent_key = Some(key);
        if !a.archived && !a.online && !a.fetched {
            a.archived = true;
            a.replaced = true;
            a.replaced_by = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{HumanRegister, Member};
    use serde_json::json;

    const D0: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";
    const D1: &str = "d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1d1";
    const S0: &str = "50505050505050505050505050505050";
    const S1: &str = "51515151515151515151515151515151";
    const S2: &str = "52525252525252525252525252525252";

    fn member(m: &mut Model, id: &str, epoch: u64, online: bool) {
        m.members.insert(
            id.into(),
            Member {
                device_id: id.into(),
                device_role: "agent".into(),
                device_name: "trommi-hub".into(),
                folder: json!("~/git/trommi-hub"),
                host: json!("desktop"),
                is_active: true,
                added_entry_number: epoch,
                is_online: online,
                ..Default::default()
            },
        );
    }

    fn session(
        m: &mut Model,
        sid: &str,
        dev: &str,
        online: bool,
        parent: Option<&str>,
        open: usize,
    ) {
        let s = m.session_of(sid);
        s.group_id = Some(format!("g{sid}"));
        s.agent_device_id = Some(dev.into());
        s.agent_device_ids = vec![dev.into()];
        s.is_online = online;
        s.parent_session_id = parent.map(String::from);
        s.creator_device_id = parent.map(|_| dev.to_string());
        let mut p = serde_json::Map::new();
        p.insert("is_main".into(), json!(parent.is_none()));
        if let Some(pa) = parent {
            p.insert("parent_session".into(), json!(pa));
        }
        s.profile = Some(p);
        s.open_card_ids = (0..open).map(|i| format!("c{i}")).collect();
    }

    /// The path a card's page shows: team (two or more teams), main session (a helper), the session itself.
    #[test]
    fn the_path_names_the_desk_the_main_session_and_itself() {
        let mut m = Model::new();
        member(&mut m, "d0", 1, true);
        member(&mut m, "d1", 1, true);
        session(&mut m, S0, "d0", true, None, 0);
        session(&mut m, S1, "d1", true, Some(S0), 0);
        let row = |m: &Model, k: &str| session_list(m, false).into_iter().find(|r| r.key == k).unwrap();
        // one team: no team in the path
        m.human.insert("desk/main".into(), HumanRegister { value: json!({ "name": "Work" }), ..Default::default() });
        assert_eq!(row(&m, S0).path, vec![row(&m, S0).name.clone()]);
        assert_eq!(row(&m, S1).path, vec![row(&m, S0).name.clone(), row(&m, S1).name.clone()]);
        // two teams: the team's name first; a session on no team that is there stands on the first
        m.human.insert("desk/lab".into(), HumanRegister { value: json!({ "name": "Lab", "created_at": 5 }), ..Default::default() });
        m.human.insert(format!("session/{S0}"), HumanRegister { value: json!({ "desk": "lab", "name": "Core" }), ..Default::default() });
        assert_eq!(row(&m, S0).path, vec!["Lab".to_string(), "Core".to_string()]);
        assert_eq!(row(&m, S1).path[..2], ["Lab".to_string(), "Core".to_string()]);
        m.human.insert(format!("session/{S0}"), HumanRegister { value: json!({ "desk": "gone", "name": "Core" }), ..Default::default() });
        assert_eq!(row(&m, S0).path, vec!["Work".to_string(), "Core".to_string()]);
    }

    #[test]
    fn model_labels_are_short() {
        assert_eq!(model_label("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(model_label("claude-sonnet-4-5-20250929"), "Sonnet 4.5");
        assert_eq!(model_label("claude-opus-5-5[1m]"), "Opus 5.5");
        assert_eq!(model_label("claude-haiku-4-5"), "Haiku 4.5");
        assert_eq!(model_label("gpt-6.1"), "GPT-6.1");
        assert_eq!(model_label("gpt-5.1-codex"), "GPT-5.1 Codex");
        assert_eq!(model_label("o4-mini"), "o4-mini");
        assert_eq!(model_label("  "), "");
    }

    fn shown(m: &Model) -> Vec<String> {
        session_list(m, false)
            .into_iter()
            .filter(|a| !a.archived)
            .map(|a| a.key)
            .collect()
    }

    /// A connector paired again in its folder: the older session of the folder is no second entry.
    #[test]
    fn a_folder_paired_twice_shows_once() {
        let mut m = Model::new();
        m.my_device_id = Some("ee".repeat(16));
        member(&mut m, D0, 3, false);
        member(&mut m, D1, 5, true);
        session(&mut m, S0, D0, false, None, 0);
        session(&mut m, S1, D1, true, None, 0);
        assert_eq!(shown(&m), vec![S1.to_string()]);
        let old = session_list(&m, false).into_iter().find(|r| r.key == S0).unwrap();
        assert!(old.replaced && old.replaced_by.as_deref() == Some(&S1[..12]), "{old:?}");
        // fetched back by the person: it stays
        m.human.insert(format!("session/{S0}"), HumanRegister { value: json!({ "archived": false }), ..Default::default() });
        assert_eq!(shown(&m).len(), 2);
    }

    /// Whether a gone session is replaced does not hang on how many of its cards were read (archived groups are read
    /// lazily: the list differed from load to load); its open cards stay on the Desk, through the session that took
    /// its place.
    #[test]
    fn a_replaced_session_stays_replaced_however_many_cards_are_read_and_its_cards_stay_reachable() {
        let mut m = Model::new();
        member(&mut m, D0, 3, false);
        member(&mut m, D1, 5, true);
        session(&mut m, S0, D0, false, None, 0);
        session(&mut m, S1, D1, true, None, 0);
        let before = shown(&m);
        m.sessions.get_mut(S0).unwrap().open_card_ids = vec!["c0".into(), "c1".into()];
        m.sessions.get_mut(S0).unwrap().card_ids = vec!["c0".into(), "c1".into()];
        assert_eq!(shown(&m), before, "the same list with its cards read");
        assert_eq!(before, vec![S1.to_string()]);
        assert!(!archived_sessions(&m).contains(&S0.to_string()), "its open cards stay on the Desk");
        // the person's own archive still takes its cards off the Desk
        m.human.insert(format!("session/{S0}"), HumanRegister { value: json!({ "archived": true }), ..Default::default() });
        assert!(archived_sessions(&m).contains(&S0.to_string()));
    }

    /// A coordinator that joined again in its folder: its helpers stand under the new session (none on top as an
    /// orphan), away ones go to the archive with it, and the crown moves to the new session.
    #[test]
    fn a_replaced_coordinators_helpers_and_crown_follow_the_new_session() {
        const S3: &str = "53535353535353535353535353535353";
        let mut m = Model::new();
        member(&mut m, D0, 3, false);
        member(&mut m, D1, 5, true);
        session(&mut m, S0, D0, false, None, 0);
        session(&mut m, S2, D0, false, Some(S0), 1);
        session(&mut m, S3, D0, true, Some(S0), 0);
        session(&mut m, S1, D1, true, None, 0);
        m.human.insert("crown".into(), HumanRegister { value: json!({ "session_id": S0 }), ..Default::default() });
        let rows = session_list(&m, false);
        let row = |k: &str| rows.iter().find(|r| r.key == k).unwrap().clone();
        assert!(row(S0).archived && !row(S0).starred);
        assert!(row(S1).starred, "the crown moves to the new session");
        assert_eq!(row(S2).parent_key.as_deref(), Some(S1));
        assert!(row(S2).archived && row(S2).replaced, "an away helper goes with it");
        assert!(!archived_sessions(&m).contains(&S2.to_string()), "its open card stays on the Desk");
        assert_eq!(row(S3).parent_key.as_deref(), Some(S1), "a connected helper stands under the new session");
        assert!(!row(S3).archived);
        assert_eq!(shown(&m), vec![S1.to_string(), S3.to_string()]);
        // the person archives the replaced coordinator: its away helpers stay out of the list, the others stay under
        // the new session, the crown stays there, and its cards leave the Desk (his choice)
        m.human.insert(format!("session/{S0}"), HumanRegister { value: json!({ "archived": true }), ..Default::default() });
        assert_eq!(shown(&m), vec![S1.to_string(), S3.to_string()]);
        let rows = session_list(&m, false);
        let row = |k: &str| rows.iter().find(|r| r.key == k).unwrap().clone();
        assert_eq!(row(S3).parent_key.as_deref(), Some(S1));
        assert!(row(S1).starred && !row(S0).replaced);
        assert!(archived_sessions(&m).contains(&S0.to_string()));
    }

    /// Two agents that ran in one folder are two entries when their devices say who they are; the same agent joined
    /// again is one; a device that names nobody is taken as the same agent (as before identities).
    #[test]
    fn a_folder_joins_only_one_agents_sessions() {
        let mut m = Model::new();
        member(&mut m, D0, 3, false);
        member(&mut m, D1, 5, true);
        session(&mut m, S0, D0, false, None, 0);
        session(&mut m, S1, D1, true, None, 0);
        let agent = |m: &mut Model, d: &str, a: &str| {
            m.device_registers.insert(d.into(), json!({ "device_name": "trommi-hub", "agent": a }).as_object().unwrap().clone());
        };
        agent(&mut m, D0, "0123456789abcdef");
        agent(&mut m, D1, "fedcba9876543210");
        assert_eq!(shown(&m).len(), 2, "two agents");
        agent(&mut m, D1, "0123456789abcdef");
        assert_eq!(shown(&m), vec![S1.to_string()], "one agent joined again");
        m.device_registers.remove(D1);
        assert_eq!(shown(&m), vec![S1.to_string()], "one without an identity");
    }

    /// The person's archive wins; a main session whose agent is back shows anyway; a closed helper is archived.
    #[test]
    fn archived_sessions_and_closed_helpers() {
        let mut m = Model::new();
        member(&mut m, D0, 3, false);
        session(&mut m, S0, D0, false, None, 0);
        session(&mut m, S2, D0, false, Some(S0), 0);
        m.human.insert(
            format!("session/{S0}"),
            HumanRegister {
                value: json!({ "archived": true }),
                ..Default::default()
            },
        );
        // (an away helper goes to the archive with its main; one he fetched back stays)
        assert_eq!(shown(&m), Vec::<String>::new());
        m.human.insert(format!("session/{S2}"), HumanRegister { value: json!({ "archived": false }), ..Default::default() });
        assert_eq!(shown(&m), vec![S2.to_string()]);
        m.human.remove(&format!("session/{S2}"));
        m.sessions.get_mut(S0).unwrap().is_online = true;
        assert_eq!(shown(&m), vec![S0.to_string(), S2.to_string()]);
        m.sessions
            .get_mut(S2)
            .unwrap()
            .profile
            .as_mut()
            .unwrap()
            .insert("closed_at".into(), json!(1));
        assert_eq!(shown(&m), vec![S0.to_string()]);
    }

    /// Two sessions naming one agent_session_id: no confusion, the one active last holds the id (a parent named by
    /// it is that one).
    #[test]
    fn a_duplicate_agent_session_id_is_held_by_the_newest() {
        let mut m = Model::new();
        member(&mut m, D0, 3, true);
        member(&mut m, D1, 4, true);
        session(&mut m, S0, D0, true, None, 0);
        session(&mut m, S1, D1, true, None, 0);
        session(&mut m, S2, D1, true, Some(S1), 0);
        for (sid, at) in [(S0, 1_000), (S1, 2_000)] {
            let s = m.sessions.get_mut(sid).unwrap();
            s.agent_session_id = Some("same-agent".into());
            s.last_activity_at = at;
        }
        m.human.insert(
            format!("session/{S2}"),
            HumanRegister {
                value: json!({ "parent": "same-agent" }),
                ..Default::default()
            },
        );
        let rows = session_list(&m, false);
        assert_eq!(rows.iter().filter(|r| r.id == "same-agent").count(), 2);
        assert_eq!(
            rows.iter()
                .find(|r| r.key == S2)
                .unwrap()
                .parent_key
                .as_deref(),
            Some(S1)
        );
        m.sessions.get_mut(S0).unwrap().last_activity_at = 3_000;
        let rows = session_list(&m, false);
        assert_eq!(
            rows.iter()
                .find(|r| r.key == S2)
                .unwrap()
                .parent_key
                .as_deref(),
            Some(S0)
        );
    }

    /// Names: the label, else the agent's name, else the device's, else "Loading …" while reading, else the short id.
    #[test]
    fn names() {
        let mut m = Model::new();
        member(&mut m, D0, 3, true);
        session(&mut m, S0, D0, true, None, 0);
        // the device's own name, else the last part of its folder
        assert_eq!(session_list(&m, false)[0].name, "trommi-hub");
        m.members.get_mut(D0).unwrap().device_name.clear();
        assert_eq!(
            session_list(&m, false)[0].name,
            "trommi-hub",
            "from the folder ~/git/trommi-hub"
        );
        m.members.get_mut(D0).unwrap().folder = Value::Null;
        assert_eq!(session_list(&m, false)[0].name, &S0[..12]);
        assert_eq!(session_list(&m, true)[0].name, "Loading …");
        m.sessions.get_mut(S0).unwrap().device_name = "project".into();
        assert_eq!(session_list(&m, true)[0].name, "project");
        m.sessions
            .get_mut(S0)
            .unwrap()
            .profile
            .as_mut()
            .unwrap()
            .insert("agent_name".into(), json!("Core"));
        assert_eq!(session_list(&m, false)[0].name, "Core");
        m.human.insert(
            format!("session/{S0}"),
            HumanRegister {
                value: json!({ "name": "Mine", "position": 4 }),
                ..Default::default()
            },
        );
        let row = &session_list(&m, false)[0];
        assert_eq!(
            (
                row.name.as_str(),
                row.given.as_str(),
                row.position,
                row.placed
            ),
            ("Mine", "Core", 4.0, true)
        );
    }

    /// A helper whose group the hub archived (its agent closed it) is in the archive on every device, also on one
    /// that missed the close itself (it reads archived groups lazily: the torture's iPhones showed them).
    #[test]
    fn a_helper_whose_group_is_archived_is_archived() {
        let mut m = Model::new();
        member(&mut m, D0, 3, true);
        session(&mut m, S0, D0, true, None, 0);
        session(&mut m, S2, D0, true, Some(S0), 1);
        let s = m.sessions.get_mut(S2).unwrap();
        s.group_archived = true;
        s.is_active = false;
        s.card_ids = vec!["c0".into()];
        assert_eq!(shown(&m), vec![S0.to_string()]);
        let row = session_list(&m, false).into_iter().find(|r| r.key == S2).unwrap();
        assert!(row.archived && !row.removed);
        // fetched back by the person: it shows
        m.human.insert(
            format!("session/{S2}"),
            HumanRegister {
                value: json!({ "archived": false }),
                ..Default::default()
            },
        );
        assert_eq!(shown(&m), vec![S0.to_string(), S2.to_string()]);
    }

    /// Two devices found an agent's session at once: the hub refuses the second, its core forgets the group. On that
    /// device the session that held nothing is gone, as on every other device.
    #[test]
    fn a_founding_the_hub_refused_leaves_no_session() {
        use crate::model::change::Change;
        use crate::model::groups::{GroupIn, SessionOfGroup};
        let mut m = Model::new();
        m.my_device_id = Some("ee".repeat(16));
        let me = "ee".repeat(16);
        let room = GroupIn { group: "r".into(), session: None, leaves: vec![me.clone()], disallowed: 0, archived: false, epoch: 1 };
        let mine = GroupIn {
            group: "g".into(),
            session: Some(SessionOfGroup { session_id: S1.into(), parent: None }),
            leaves: vec![me.clone(), D0.into()],
            disallowed: 0,
            archived: false,
            epoch: 0,
        };
        let mut c = Change::new();
        m.apply_groups(&[room.clone(), mine], None, &mut c);
        assert!(m.sessions.contains_key(S1));
        m.apply_groups(&[room], None, &mut c);
        assert!(!m.sessions.contains_key(S1), "forgotten with its group");
    }
}
