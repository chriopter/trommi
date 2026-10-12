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
//!   question; a main session whose agent is connected again shows whatever hid it;
//! - one entry per folder (slot): a connector paired again in its folder is a new agent device with a new main
//!   session; while one session of the slot is connected (else: the newest) the others are archived as replaced,
//!   never one with an open question or one the person fetched back; their helpers go with them;
//! - the parent: the person's choice, else the session's own claim (a helper an agent opened counts only under a
//!   session of that agent); a helper stands on its main's desk;
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
    pub removed: bool,
    pub position: f64,
    pub placed: bool,
    pub desk: String,
    pub starred: bool,
    pub main: bool,
    /// A session of a person's own device (never deleted from the board).
    pub own: bool,
    /// How many of its questions are open.
    pub waiting: usize,
    /// The person fetched it back from the archive.
    pub fetched: bool,
    pub last_activity_at: u64,
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
fn device_name_of(m: &Model, id: Option<&str>) -> String {
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

/// The session list. `reading`: the room is still being read (a session whose profile has not come shows
/// "Loading …" rather than its id).
pub fn session_list(m: &Model, reading: bool) -> Vec<SessionRow> {
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
        .filter(|s| (s.is_active && s.agent_device_id.is_some()) || !s.card_ids.is_empty())
        .collect();
    let me = m.my_device_id.clone().unwrap_or_default();
    let mut out: Vec<SessionRow> = Vec::with_capacity(list.len());
    let mut slots: Vec<Option<(String, u64)>> = Vec::with_capacity(list.len());
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
            desk: text(set.get("desk")).unwrap_or("").to_string(),
            starred: crown.as_deref() == Some(key.as_str())
                || (crown.is_some() && crown == s.agent_device_id),
            main: truthy(p.get("is_main")),
            own,
            waiting: s.open_card_ids.len(),
            fetched: set.contains_key("archived") && !truthy(set.get("archived")),
            last_activity_at: s.last_activity_at,
        });
        let folder = dev
            .and_then(|d| d.folder.as_str())
            .filter(|f| !f.is_empty());
        slots.push(match (s.parent_session_id.is_none(), folder) {
            (true, Some(f)) => {
                let host = dev.and_then(|d| d.host.as_str()).unwrap_or("");
                Some((
                    format!("{host}\n{f}"),
                    dev.map(|d| d.added_entry_number).unwrap_or(0),
                ))
            }
            _ => None,
        });
    }
    replace_in_slots(&mut out, &slots);
    // A helper stands on its main's desk; a session without a desk on the first one ("main").
    let desks: HashMap<String, String> = out
        .iter()
        .map(|a| {
            (
                a.id.clone(),
                if a.desk.is_empty() {
                    "main".to_string()
                } else {
                    a.desk.clone()
                },
            )
        })
        .collect();
    for a in out.iter_mut() {
        match a.parent.as_ref().and_then(|p| desks.get(p)) {
            Some(d) => a.desk = d.clone(),
            None if a.desk.is_empty() => a.desk = "main".into(),
            None => {}
        }
    }
    out.sort_by(|a, b| {
        a.position
            .partial_cmp(&b.position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

/// One entry per folder: see the module's rules.
fn replace_in_slots(out: &mut [SessionRow], slots: &[Option<(String, u64)>]) {
    let mut by_slot: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, s) in slots.iter().enumerate() {
        if let Some((slot, _)) = s {
            if !out[i].archived {
                by_slot.entry(slot.as_str()).or_default().push(i);
            }
        }
    }
    let mut gone: BTreeSet<String> = BTreeSet::new();
    for same in by_slot.values() {
        if same.len() < 2 {
            continue;
        }
        let live: Vec<usize> = same.iter().copied().filter(|i| out[*i].online).collect();
        let keep: Vec<usize> = if !live.is_empty() {
            live
        } else {
            let newest = same
                .iter()
                .copied()
                .reduce(|x, y| {
                    let (bx, by) = (slots[x].as_ref().unwrap().1, slots[y].as_ref().unwrap().1);
                    if by > bx || (by == bx && out[y].last_activity_at > out[x].last_activity_at) {
                        y
                    } else {
                        x
                    }
                })
                .unwrap();
            vec![newest]
        };
        for &i in same {
            let a = &out[i];
            if !keep.contains(&i) && !a.online && a.waiting == 0 && !a.fetched {
                out[i].archived = true;
                out[i].replaced = true;
                gone.insert(out[i].id.clone());
            }
        }
    }
    for a in out.iter_mut() {
        if a.parent.as_ref().is_some_and(|p| gone.contains(p))
            && !a.archived
            && !a.online
            && a.waiting == 0
            && !a.fetched
        {
            a.archived = true;
            a.replaced = true;
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
        // with an open question the older one stays
        m.sessions.get_mut(S0).unwrap().open_card_ids = vec!["c".into()];
        assert_eq!(shown(&m).len(), 2);
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
        // (the archive is per session: a helper of an archived main keeps its own choice, as on the web)
        assert_eq!(shown(&m), vec![S2.to_string()]);
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
