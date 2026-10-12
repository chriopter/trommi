//! Members and sessions: the device's groups as the core holds them, what a Commit did, who is connected.

use super::change::Change;
use super::types::*;
use super::{timeline_key, Model};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One group as the core holds it (ids hex).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupIn {
    pub group: String,
    /// The session of a session group: its id and, for a helper session, its main session.
    pub session: Option<SessionOfGroup>,
    pub leaves: Vec<String>,
    pub disallowed: usize,
    pub archived: bool,
    pub epoch: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionOfGroup {
    pub session_id: String,
    pub parent: Option<String>,
}

/// The room's roles: its human and agent devices, its recovery key.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RolesIn {
    pub epoch: u64,
    pub humans: Vec<String>,
    pub agents: Vec<String>,
    /// The fingerprint of the room's recovery HPKE key, if it has one.
    pub recovery_fingerprint: Option<String>,
}

/// What a processed Commit did.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommitIn {
    pub group: String,
    /// The group is the room group.
    pub room: bool,
    pub epoch: u64,
    pub adds: Vec<String>,
    pub removes: Vec<String>,
    pub external: bool,
    pub committer: String,
    pub agents: Vec<String>,
    pub time: Option<u64>,
}

/// What the hub says of a device.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeviceReport {
    pub device_id: String,
    #[serde(default)]
    pub is_online: bool,
    #[serde(default)]
    pub offline_since: Option<u64>,
    /// Absent: the report says nothing of the link (null: it says there is none).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub link: Option<Value>,
}

/// A field that is there, even as null.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

/// A key's fingerprint as the app shows it: the first 16 hex digits, in groups of four.
pub fn fingerprint_of(device_id: &str) -> String {
    let head: Vec<char> = device_id.chars().take(16).collect();
    head.chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

fn stamp(v: &Value) -> Option<u64> {
    v.as_f64()
        .filter(|n| n.fract() == 0.0 && *n > 0.0 && *n <= 9_007_199_254_740_991.0)
        .map(|n| n as u64)
}

/// A link report as the model keeps it: known fields of the right type only, none when there is none.
pub fn clean_link(report: &Value) -> Option<Link> {
    let l = report.as_object()?;
    let hears = l.get("hears")?.as_str()?;
    if hears != "live" && hears != "oncall" {
        return None;
    }
    let exit = l.get("exit").and_then(Value::as_object).map(|e| {
        let reason = match e.get("reason") {
            Some(Value::String(s)) => s.chars().take(40).collect(),
            Some(Value::Null) | None => String::new(),
            Some(other) => other.to_string().chars().take(40).collect(),
        };
        let claude = match e.get("claude").and_then(Value::as_str) {
            Some(c @ ("alive" | "gone" | "checking")) => c.to_string(),
            _ => "gone".to_string(),
        };
        LinkExit { reason, claude }
    });
    Some(Link {
        hears: hears.to_string(),
        attached: l.get("attached") != Some(&Value::Bool(false)),
        last_call_at: l.get("last_call_at").and_then(stamp),
        working: l.get("working") == Some(&Value::Bool(true)),
        since: l.get("since").and_then(stamp),
        cut_since: l.get("cut_since").and_then(stamp),
        exit,
    })
}

impl Model {
    /// A session, made if it is new.
    pub fn session_of(&mut self, session_id: &str) -> &mut Session {
        self.sessions
            .entry(session_id.to_string())
            .or_insert_with(|| Session {
                session_id: session_id.to_string(),
                group_id: None,
                agent_device_ids: Vec::new(),
                agent_device_id: None,
                agent_session_id: None,
                device_name: String::new(),
                is_active: true,
                stale: false,
                group_archived: false,
                is_online: false,
                offline_since: None,
                link: None,
                heard_up_to: None,
                heard_at: None,
                session_key_epoch: 0,
                created_by_agent: false,
                creator_device_id: None,
                parent_session_id: None,
                profile: None,
                status_lines: Vec::new(),
                agent_alerts: Vec::new(),
                registers: Default::default(),
                settings: None,
                card_ids: Vec::new(),
                open_card_ids: Vec::new(),
                timeline_key: timeline_key("chat", &format!("session/{session_id}")),
                last_activity_at: 0,
            })
    }

    /// A member, made if it is new (its name from its own register, if one came first).
    pub fn member_of(&mut self, device_id: &str, role: &str, epoch: u64) -> &mut Member {
        let reg = self
            .device_registers
            .get(device_id)
            .cloned()
            .unwrap_or_default();
        let is_me = self.my_device_id.as_deref() == Some(device_id);
        self.members
            .entry(device_id.to_string())
            .or_insert_with(|| Member {
                device_id: device_id.to_string(),
                device_role: role.to_string(),
                device_name: js_string(reg.get("device_name")),
                fingerprint: fingerprint_of(device_id),
                platform: reg.get("platform").cloned().unwrap_or(Value::Null),
                folder: reg.get("folder").cloned().unwrap_or(Value::Null),
                host: reg.get("host").cloned().unwrap_or(Value::Null),
                is_active: true,
                added_entry_number: epoch,
                removed_entry_number: None,
                added_at: None,
                is_me,
                is_online: false,
                offline_since: None,
                link: None,
            })
    }
}

/// `String(v ?? '')` of JavaScript, for the values a register holds.
pub fn js_string(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Array(_)) | Some(Value::Object(_)) => String::from("[object Object]"),
    }
}

/// The session's profile as the views read it: the agent's register, with the parent and "is a main session".
pub fn compose_profile(s: &mut Session) {
    let raw = s
        .registers
        .get("profile")
        .map(|r| r.value.clone())
        .unwrap_or(Value::Null);
    let raw_obj = raw.as_object();
    if raw_obj.is_none() && s.group_id.is_none() {
        s.profile = None;
        return;
    }
    let mut p: Map<String, Value> = raw_obj.cloned().unwrap_or_default();
    p.remove("parent_session");
    p.remove("is_main");
    if let Some(parent) = &s.parent_session_id {
        p.insert("parent_session".into(), Value::from(parent.clone()));
    }
    if s.group_id.is_some() {
        p.insert("is_main".into(), Value::from(s.parent_session_id.is_none()));
    }
    s.profile = Some(p);
}

impl Model {
    /// Copy the speaking device's facts onto its session; a helper session without a device of its own shows its opener's.
    pub fn sync_session_agent(&mut self, session_id: &str) {
        let Some(s) = self.sessions.get(session_id) else {
            return;
        };
        let m = s
            .agent_device_id
            .as_ref()
            .and_then(|d| self.members.get(d))
            .cloned();
        let opener = match (&s.creator_device_id, &s.agent_device_id) {
            (Some(c), a) if Some(c) != a.as_ref() => self.members.get(c).cloned(),
            _ => None,
        };
        let s = self.sessions.get_mut(session_id).expect("the session");
        s.agent_session_id = s
            .agent_device_id
            .as_ref()
            .map(|d| d.chars().take(16).collect());
        s.device_name = match (&m, &opener) {
            (Some(m), _) if !m.device_name.is_empty() => m.device_name.clone(),
            (_, Some(o)) if !o.device_name.is_empty() => o.device_name.clone(),
            _ => String::new(),
        };
        let m_online = m.as_ref().is_some_and(|m| m.is_online);
        let o_online = opener.as_ref().is_some_and(|o| o.is_online);
        s.is_online = m_online || o_online;
        s.offline_since = if s.is_online {
            None
        } else {
            m.as_ref()
                .and_then(|m| m.offline_since)
                .or_else(|| opener.as_ref().and_then(|o| o.offline_since))
        };
        let own = if m_online || !o_online {
            m.as_ref().and_then(|m| m.link.clone())
        } else {
            None
        };
        s.link = own.or_else(|| opener.as_ref().and_then(|o| o.link.clone()));
    }

    /// A device's facts changed: every session it speaks in, or opened, shows them again.
    pub fn touch_agent(&mut self, device_id: &str, change: &mut Change) {
        let ids: Vec<String> = self
            .sessions
            .values()
            .filter(|s| {
                s.agent_device_id.as_deref() == Some(device_id)
                    || s.creator_device_id.as_deref() == Some(device_id)
            })
            .map(|s| s.session_id.clone())
            .collect();
        for id in ids {
            self.sync_session_agent(&id);
            change.sessions.insert(id);
        }
    }

    /// Who holds an object now: its owner while that device is a leaf of the session's group, else the session's
    /// agent device (a helper session: its opener), spec 9.2.
    pub fn holder_of(&self, agent_device_id: &str, session_id: Option<&str>) -> Option<String> {
        let Some(s) = session_id.and_then(|id| self.sessions.get(id)) else {
            return Some(agent_device_id.to_string());
        };
        if s.agent_device_ids.is_empty() || s.agent_device_ids.iter().any(|d| d == agent_device_id)
        {
            return Some(agent_device_id.to_string());
        }
        self.recipient_of(&s.session_id)
    }

    /// Whom a human addresses a Chat message of a session to: its agent device; in a helper session its opener.
    pub fn recipient_of(&self, session_id: &str) -> Option<String> {
        let s = self.sessions.get(session_id)?;
        if s.parent_session_id.is_some() {
            s.creator_device_id.clone()
        } else {
            s.agent_device_id.clone()
        }
    }

    /// Whether a device writes as a human: by the member list; a device never seen as a member is one when it
    /// addressed someone (a human's items name their agent, an agent's name nobody).
    pub fn is_human(&self, device_id: &str, recipient: Option<&str>) -> bool {
        match self.members.get(device_id) {
            Some(m) => m.device_role == "human",
            None => recipient.is_some(),
        }
    }
}

impl Model {
    /// The device's groups as the core holds them now: replaces the members and the sessions' facts. Human devices are
    /// the room's, agent devices the roles', and every other leaf of a session group is a helper device. A device no
    /// longer among them stays as removed, so its old messages keep their name.
    pub fn apply_groups(
        &mut self,
        groups: &[GroupIn],
        roles: Option<&RolesIn>,
        change: &mut Change,
    ) {
        let room = groups.iter().find(|g| g.session.is_none());
        let epoch = roles
            .map(|r| r.epoch)
            .or(room.map(|g| g.epoch))
            .unwrap_or(self.key_epoch);
        if self.key_epoch != epoch {
            self.key_epoch = epoch;
            change.room = true;
        }
        let seen = self.room_commits.take();
        if let Some(fingerprint) = roles.and_then(|r| r.recovery_fingerprint.clone()) {
            let was = self.recovery_key.clone();
            let was_fp = was
                .as_ref()
                .and_then(|w| w.get("fingerprint"))
                .and_then(Value::as_str)
                .map(str::to_string);
            if was_fp.as_deref() != Some(fingerprint.as_str()) {
                let since = match (&was, seen) {
                    (Some(_), Some((1, time))) => time.map(Value::from).unwrap_or(Value::Null),
                    _ => Value::Null,
                };
                self.recovery_key =
                    Some(serde_json::json!({ "fingerprint": fingerprint, "since": since }));
                change.room = true;
            }
        }
        let humans: Vec<String> = roles
            .map(|r| r.humans.clone())
            .or(room.map(|g| g.leaves.clone()))
            .unwrap_or_default();
        let agents: Vec<String> = roles.map(|r| r.agents.clone()).unwrap_or_default();
        let mut now: Vec<(String, &'static str)> = Vec::new();
        let put = |now: &mut Vec<(String, &'static str)>, d: &str, role: &'static str| {
            if let Some(e) = now.iter_mut().find(|(x, _)| x == d) {
                e.1 = role
            } else {
                now.push((d.to_string(), role))
            }
        };
        for d in &humans {
            put(&mut now, d, "human")
        }
        for d in &agents {
            put(&mut now, d, "agent")
        }
        let mut live = std::collections::BTreeSet::new();
        for g in groups {
            let Some(sess) = &g.session else { continue };
            let others: Vec<String> = g
                .leaves
                .iter()
                .filter(|d| !humans.contains(d))
                .cloned()
                .collect();
            for d in &others {
                if !now.iter().any(|(x, _)| x == d) {
                    now.push((d.clone(), "agent"))
                }
            }
            live.insert(sess.session_id.clone());
            let parent = sess.parent.clone();
            let opener = if parent.is_some() {
                others.iter().find(|d| agents.contains(d)).cloned()
            } else {
                None
            };
            let speaker = if parent.is_some() {
                others
                    .iter()
                    .find(|d| Some(*d) != opener.as_ref())
                    .or(others.first())
                    .cloned()
            } else {
                others
                    .iter()
                    .find(|d| agents.contains(d))
                    .or(others.first())
                    .cloned()
            };
            let s = self.session_of(&sess.session_id);
            s.group_id = Some(g.group.clone());
            s.parent_session_id = parent;
            s.creator_device_id = opener;
            s.agent_device_ids = others;
            if speaker.is_some() {
                s.agent_device_id = speaker.clone()
            }
            s.stale = g.disallowed > 0;
            s.group_archived = g.archived;
            s.session_key_epoch = g.epoch;
            s.is_active = !g.archived && speaker.is_some();
        }
        for s in self.sessions.values_mut() {
            if s.group_id.is_some() && !live.contains(&s.session_id) {
                s.is_active = false
            }
            change.sessions.insert(s.session_id.clone());
        }
        for (device_id, role) in &now {
            let me = self.my_device_id.as_deref() == Some(device_id.as_str());
            let m = self.member_of(device_id, role, epoch);
            m.device_role = role.to_string();
            m.is_active = true;
            m.removed_entry_number = None;
            m.is_me = me;
        }
        for m in self.members.values_mut() {
            if !now.iter().any(|(d, _)| d == &m.device_id) && m.is_active {
                m.is_active = false;
                m.removed_entry_number = Some(epoch);
            }
        }
        let ids: Vec<String> = self.sessions.keys().cloned().collect();
        for id in &ids {
            if let Some(s) = self.sessions.get_mut(id) {
                compose_profile(s)
            }
            self.sync_session_agent(id);
        }
        // settings an older web kept under a session's agent device, read before that session was known: its own now
        for id in &ids {
            let s = &self.sessions[id];
            if s.group_id.is_none()
                || self
                    .human
                    .get(&format!("session/{id}"))
                    .is_some_and(|r| !r.value.is_null())
            {
                continue;
            }
            let devices: Vec<String> = s
                .agent_device_id
                .iter()
                .chain(s.agent_device_ids.iter())
                .cloned()
                .collect();
            for d in devices {
                let legacy = self
                    .human
                    .get(&format!("session/{d}"))
                    .map(|r| r.value.clone())
                    .unwrap_or(Value::Null);
                if legacy.is_null() {
                    continue;
                }
                self.sessions.get_mut(id).expect("the session").settings = Some(legacy);
                change.sessions.insert(id.clone());
                break;
            }
        }
        change.members = true;
        change.stack = true;
    }
}

impl Model {
    /// What a processed Commit did, as far as a human is told. The lists come with the next `apply_groups`; this
    /// marks removed devices at once and raises the alerts.
    pub fn apply_commit(
        &mut self,
        facts: &CommitIn,
        change: &mut Change,
        by_recovery: bool,
        removed_me: bool,
        now: u64,
    ) {
        let session_id = self
            .sessions
            .values()
            .find(|s| s.group_id.as_deref() == Some(facts.group.as_str()))
            .map(|s| s.session_id.clone());
        let time = facts.time;
        let mut added = |model: &mut Model, id: &str, role: &str, room: bool| {
            if let Some(known) = model.members.get(id) {
                if known.added_at.is_some() || !room || known.added_entry_number <= facts.epoch {
                    return;
                }
            }
            model.member_of(id, role, facts.epoch + 1).added_at = time;
            change.members = true;
        };
        if session_id.is_none() && facts.room {
            for d in &facts.adds {
                added(self, d, "human", true)
            }
            if facts.external {
                added(self, &facts.committer, "human", true)
            }
            for d in &facts.agents {
                added(self, d, "agent", true)
            }
            let count = self.room_commits.map(|(c, _)| c).unwrap_or(0);
            self.room_commits = Some((count + 1, time));
        } else {
            for d in &facts.adds {
                added(self, d, "agent", false)
            }
        }
        if let Some(sid) = &session_id {
            let s = self.sessions.get_mut(sid).expect("the session");
            s.session_key_epoch = facts.epoch + 1;
            s.agent_device_ids.retain(|d| !facts.removes.contains(d));
            change.sessions.insert(sid.clone());
        } else {
            for id in &facts.removes {
                let Some(m) = self.members.get_mut(id) else {
                    continue;
                };
                if !m.is_active {
                    continue;
                }
                m.is_active = false;
                m.removed_entry_number = Some(facts.epoch + 1);
                change.members = true;
                self.touch_agent(id, change);
            }
        }
        if by_recovery {
            self.push_alert(change, "recovery-add", "a device was added with the recovery code: if that was not you, remove it and make a new recovery code", None, Some(facts.committer.clone()), "local", now);
        }
        if removed_me {
            self.push_alert(
                change,
                "removed",
                "this device was removed from the room",
                None,
                Some(facts.committer.clone()),
                "local",
                now,
            );
        }
    }

    /// Who is connected, and each agent's link report. A device that stopped working, or went, ends its running turns.
    pub fn apply_presence(&mut self, reports: &[DeviceReport], change: &mut Change, now: u64) {
        for d in reports {
            let Some(m) = self.members.get_mut(&d.device_id) else {
                continue;
            };
            m.is_online = d.is_online;
            m.offline_since = if m.is_online { None } else { d.offline_since };
            if let Some(link) = &d.link {
                m.link = clean_link(link)
            } else if !m.is_online {
                m.link = None
            }
            let agent = m.device_role == "agent";
            let stop = !m.is_online || m.link.as_ref().is_some_and(|l| !l.working);
            if agent {
                self.touch_agent(&d.device_id, change);
                if stop {
                    let sender = d.device_id.clone();
                    self.end_turns(|t| t.sender == sender, now, change);
                }
            }
            change.members = true;
        }
    }
}
