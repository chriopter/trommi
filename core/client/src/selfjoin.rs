//! Hash-bound activation on a computer explicitly approved by a human (spec 12.4).
//! Only authenticated human room grants confer authority. The human app opens bounded agent invites
//! automatically and confirms the Request named by that computer; ordinary agents cannot vouch.
//! Archived admitted sessions are retired, and computer removal cascades through `machines`.
use crate::model::types::Session;
use crate::model::Model;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The most live sessions of one approved computer that joined by themselves, open links counted in.
pub const MAX_VOUCHED: usize = 8;
/// The most admitted sessions of all computers together.
pub const MAX_VOUCHED_ROOM: usize = 24;
/// The most vouched links open at once on one device (the hub keeps 16 open invites a room).
pub const MAX_VOUCHED_OPEN: usize = 8;

/// One ask of an agent.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Ask {
    pub session_id: String,
    pub nonce: String,
    pub ask: Value,
    /// Why it is refused at once (`bad-ask`, `cap`, `busy`), or None: the human app may open an activation link.
    pub refused: Option<String>,
    /// The name the new session gets ([`ask_name`]).
    pub name: String,
}

/// The name of the session the asking agent was before, when the ask names who it is (`agent`, spec/v1.md 12.4.2)
/// and a main session of an agent device naming the same on the same host is known: a resumed session, or one
/// under another account of its program, keeps its name instead of its folder's. (The agent's word, as the rest of
/// the ask.)
pub fn known_name(m: &Model, ask: &Value) -> Option<String> {
    let agent = ask
        .get("agent")
        .and_then(Value::as_str)
        .filter(|a| a.len() == 16 && a.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let host = ask.get("host").and_then(Value::as_str).unwrap_or("");
    let same = |d: &str| {
        m.device_registers.get(d).is_some_and(|r| {
            r.get("agent").and_then(Value::as_str) == Some(agent)
                && r.get("host").and_then(Value::as_str).unwrap_or("") == host
        })
    };
    crate::sessions::session_list(m, false)
        .into_iter()
        .filter(|r| r.parent.is_none() && r.agent_device_id.as_deref().is_some_and(same))
        .max_by_key(|r| (!r.archived, r.last_activity_at))
        .map(|r| r.name)
        .filter(|n| !n.is_empty())
}

/// The name a new session gets: the name it had before ([`known_name`]), else the one it asks under, else its
/// folder's last part.
pub fn ask_name(m: &Model, ask: &Value) -> String {
    known_name(m, ask)
        .or_else(|| ask.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).map(String::from))
        .or_else(|| Some(folder_name(ask.get("folder").and_then(Value::as_str).unwrap_or(""))).filter(|n| !n.is_empty()).map(String::from))
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}

fn truthy(v: Option<&Value>) -> bool {
    crate::model::apply::truthy(v.unwrap_or(&Value::Null))
}

fn setting<'a>(s: &'a Session, key: &str) -> Option<&'a Value> {
    s.settings.as_ref().and_then(|v| v.get(key))
}

fn nonce_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A live main session whose group this device holds: the only place an ask counts.
fn asks_here(s: &Session) -> bool {
    s.is_active
        && !s.stale
        && s.parent_session_id.is_none()
        && s.group_id.is_some()
        && !truthy(setting(s, "archived"))
}

/// Each ask in a live main session with no answer: (session, nonce, ask).
fn open_asks(m: &Model) -> Vec<(&Session, &str, &Value)> {
    let mut out = Vec::new();
    for s in m.sessions.values().filter(|s| asks_here(s)) {
        for (key, reg) in &s.registers {
            let Some(nonce) = key.strip_prefix("join_ask/") else { continue };
            if !nonce_ok(nonce) || !reg.value.is_object() {
                continue;
            }
            let answered = s
                .registers
                .get(&format!("join_link/{nonce}"))
                .is_some_and(|r| !r.value.is_null());
            if !answered {
                out.push((s, nonce, &reg.value));
            }
        }
    }
    out
}

/// The computer quota keys of live admitted sessions.
fn vouched_accounts(m: &Model) -> Vec<String> {
    m.sessions
        .values()
        .filter(|s| s.is_active && !truthy(setting(s, "archived")))
        .filter_map(|s| setting(s, "vouched").filter(|v| v.is_object()))
        .map(|v| v.get("account").and_then(Value::as_str).unwrap_or("").to_string())
        .collect()
}

/// The asks to act on: not in `answered` (this device answered them), with the refusal each gets at once. `pending`:
/// the accounts of this device's vouched links not through yet.
pub fn asks(m: &Model, answered: &BTreeSet<String>, pending: &[String]) -> Vec<Ask> {
    let mut accounts = vouched_accounts(m);
    accounts.extend(pending.iter().cloned());
    let mut open = pending.len();
    let mut out = Vec::new();
    for (s, nonce, ask) in open_asks(m) {
        if answered.contains(nonce) {
            continue;
        }
        let account = crate::machines::quota(&s.session_id);
        let full = accounts.iter().filter(|a| **a == account).count() >= MAX_VOUCHED
            || accounts.len() >= MAX_VOUCHED_ROOM;
        let refused = if !crate::machines::approved(m, &s.session_id) {
            Some("computer-required")
        } else if full {
            Some("cap")
        } else if open >= MAX_VOUCHED_OPEN {
            Some("busy")
        } else {
            None
        };
        if refused.is_none() {
            accounts.push(account);
            open += 1;
        }
        out.push(Ask {
            session_id: s.session_id.clone(),
            nonce: nonce.to_string(),
            ask: ask.clone(),
            refused: refused.map(String::from),
            name: ask_name(m, ask),
        });
    }
    out
}

/// Whether the Request with `request_hash` (base64url) that a link vouched for by `session_id`'s ask `nonce` brought
/// is let in: None while the ask names no hash yet; false when it names another, or the voucher no longer stands (a
/// removed or archived session vouches for nobody).
pub fn verdict(m: &Model, session_id: &str, nonce: &str, request_hash: &str) -> Option<bool> {
    let s = m.sessions.get(session_id)?;
    let named = s
        .registers
        .get(&format!("join_ask/{nonce}"))
        .and_then(|r| r.value.get("request_hash"))
        .and_then(Value::as_str)
        .filter(|h| !h.is_empty())?;
    let standing = crate::machines::approved(m, session_id);
    Some(standing && named == request_hash)
}

/// The agent devices of archived sessions that joined by themselves and are still in the room: they go (12.4.5).
pub fn to_retire(m: &Model) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for s in m.sessions.values() {
        let archived = truthy(setting(s, "archived")) || s.group_archived;
        if !archived || !setting(s, "vouched").is_some_and(Value::is_object) {
            continue;
        }
        if let Some(d) = s.agent_device_id.as_ref() {
            if m.members.get(d).is_some_and(|x| x.is_active && x.device_role != "human") {
                out.insert(d.clone());
            }
        }
    }
    out.into_iter().collect()
}

/// An ask as the sidebar shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JoinRow {
    pub nonce: String,
    pub session_id: String,
    /// `ask`: nobody answered; `joining`: allowed, and its session is not in yet.
    pub state: String,
    pub folder: String,
    pub name: String,
    pub host: String,
    /// `Claude` or `Codex`.
    pub kind: String,
    /// Who vouched: the voucher's name.
    pub by: String,
}

fn folder_name(f: &str) -> &str {
    f.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
}

/// The asks as rows of the sidebar. `invites`: the state of each invite this device knows (`invite_id` → state); an
/// allowed ask whose invite joined, failed, expired or is unknown has no row (the session's own row stands then).
pub fn join_rows(m: &Model, invites: &BTreeMap<String, String>) -> Vec<JoinRow> {
    let mut out = Vec::new();
    for s in m.sessions.values().filter(|s| asks_here(s)) {
        for (key, reg) in &s.registers {
            let Some(nonce) = key.strip_prefix("join_ask/") else { continue };
            let ask = &reg.value;
            if !nonce_ok(nonce) || !ask.is_object() {
                continue;
            }
            let link = s.registers.get(&format!("join_link/{nonce}")).map(|r| &r.value);
            let state = match link {
                None | Some(Value::Null) => "ask",
                Some(l) => {
                    let st = l
                        .get("invite_id")
                        .and_then(Value::as_str)
                        .and_then(|id| invites.get(id));
                    match st.map(String::as_str) {
                        None | Some("joined") | Some("failed") | Some("expired") => continue,
                        Some(_) => "joining",
                    }
                }
            };
            let folder = ask.get("folder").and_then(Value::as_str).unwrap_or("");
            let name = known_name(m, ask)
                .or_else(|| Some(folder_name(folder).to_string()).filter(|n| !n.is_empty()))
                .or_else(|| ask.get("name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(String::from))
                .unwrap_or_else(|| "New session".into());
            let label = ask.get("account_label").and_then(Value::as_str).unwrap_or("");
            let by = setting(s, "name")
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
                .or_else(|| ask.get("vouched_by").and_then(Value::as_str))
                .unwrap_or("")
                .to_string();
            out.push(JoinRow {
                nonce: nonce.to_string(),
                session_id: s.session_id.clone(),
                state: state.into(),
                folder: folder.to_string(),
                name,
                host: ask.get("host").and_then(Value::as_str).unwrap_or("").to_string(),
                kind: if label.starts_with("codex") { "Codex" } else { "Claude" }.into(),
                by,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Member, RegisterValue};
    use serde_json::json;

    const S1: &str = "cccccccccccccccccccccccccccccccc";
    const AGENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn room() -> Model {
        let mut m = Model::new();
        let s = m.session_of(S1);
        s.group_id = Some("g".into());
        s.agent_device_id = Some(AGENT.into());
        let grant = json!({"machine":{"device":AGENT}});
        s.settings = Some(grant.clone());
        m.human.insert(format!("session/{S1}"), crate::model::types::HumanRegister { value: grant, ..Default::default() });
        m
    }
    fn reg(m: &mut Model, sid: &str, key: &str, value: Value) {
        m.sessions.get_mut(sid).unwrap().registers.insert(
            key.into(),
            RegisterValue {
                value,
                ..Default::default()
            },
        );
    }
    fn ask(account: &str) -> Value {
        json!({ "name": "second", "folder": "~/project/", "account": account, "account_label": "claude .claude", "vouched_by": "project" })
    }

    #[test]
    fn an_ask_gets_a_link_once_and_answered_ones_are_left() {
        let mut m = room();
        reg(&mut m, S1, "join_ask/a1", ask("acct"));
        let got = asks(&m, &BTreeSet::new(), &[]);
        assert_eq!((got.len(), got[0].refused.clone()), (1, None));
        assert!(asks(&m, &["a1".to_string()].into(), &[]).is_empty());
        reg(&mut m, S1, "join_link/a1", json!({ "link": "x", "invite_id": "00" }));
        assert!(asks(&m, &BTreeSet::new(), &[]).is_empty(), "answered by another device");
        // not held here: none
        let mut m = room();
        reg(&mut m, S1, "join_ask/a1", ask("acct"));
        m.sessions.get_mut(S1).unwrap().group_id = None;
        assert!(asks(&m, &BTreeSet::new(), &[]).is_empty());
    }

    #[test]
    fn the_bounds() {
        let mut m = room();
        reg(&mut m, S1, "join_ask/b0", ask(""));
        assert_eq!(asks(&m, &BTreeSet::new(), &[])[0].refused, None, "program accounts do not confer computer authority");
        m.human.clear();
        assert_eq!(asks(&m, &BTreeSet::new(), &[])[0].refused.as_deref(), Some("computer-required"));
        let mut m = room();
        for i in 0..10 {
            reg(&mut m, S1, &format!("join_ask/c{i}"), ask("acct"));
        }
        let got = asks(&m, &BTreeSet::new(), &[]);
        assert_eq!(got.iter().filter(|a| a.refused.is_none()).count(), MAX_VOUCHED, "ten at once: eight links");
        let pending = vec![crate::machines::quota(S1); 8];
        assert_eq!(asks(&m, &BTreeSet::new(), &pending).iter().filter(|a| a.refused.is_none()).count(), 0);
        let mut m = room();
        for i in 0..30 {
            reg(&mut m, S1, &format!("join_ask/d{i:02}"), ask(&format!("acct{i}")));
        }
        let got = asks(&m, &BTreeSet::new(), &[]);
        assert_eq!(got.iter().filter(|a| a.refused.is_none()).count(), MAX_VOUCHED_OPEN);
        assert_eq!(got.iter().filter(|a| a.refused.as_deref() == Some("cap")).count(), 30 - MAX_VOUCHED, "changing claimed accounts cannot evade a computer's quota");
        let busy: Vec<String> = (0..MAX_VOUCHED_OPEN).map(|i| format!("other-machine-{i}")).collect();
        assert!(asks(&m, &BTreeSet::new(), &busy).iter().all(|a| a.refused.as_deref() == Some("busy")), "open invites on other computers bound concurrent work");
        let pending: Vec<String> = (0..MAX_VOUCHED_ROOM).map(|i| format!("other-machine-{i}")).collect();
        assert!(asks(&m, &BTreeSet::new(), &pending).iter().all(|a| a.refused.as_deref() == Some("cap")), "the room bound includes other computers");
    }

    #[test]
    fn a_request_is_let_in_only_by_its_hash_and_a_standing_voucher() {
        let mut m = room();
        m.sessions.get_mut(S1).unwrap().is_active = true;
        reg(&mut m, S1, "join_ask/e1", ask("acct"));
        assert_eq!(verdict(&m, S1, "e1", "HASH"), None, "no hash named yet");
        reg(&mut m, S1, "join_ask/e1", json!({ "account": "acct", "request_hash": "HASH" }));
        assert_eq!(verdict(&m, S1, "e1", "HASH"), Some(true));
        assert_eq!(verdict(&m, S1, "e1", "OTHER"), Some(false));
        m.sessions.get_mut(S1).unwrap().settings = Some(json!({ "archived": true }));
        m.human.get_mut(&format!("session/{S1}")).unwrap().value["archived"] = json!(true);
        assert_eq!(verdict(&m, S1, "e1", "HASH"), Some(false), "an archived voucher vouches for nobody");
    }

    #[test]
    fn rows_and_retirement() {
        let mut m = room();
        reg(&mut m, S1, "join_ask/f1", ask("acct"));
        let rows = join_rows(&m, &BTreeMap::new());
        assert_eq!((rows[0].state.as_str(), rows[0].name.as_str(), rows[0].kind.as_str()), ("ask", "project", "Claude"));
        reg(&mut m, S1, "join_link/f1", json!({ "invite_id": "i1" }));
        assert!(join_rows(&m, &BTreeMap::new()).is_empty());
        let invites: BTreeMap<String, String> = [("i1".to_string(), "waiting".to_string())].into();
        assert_eq!(join_rows(&m, &invites)[0].state, "joining");
        // an archived session that joined by itself: its device goes
        m.members.insert(
            AGENT.into(),
            Member {
                device_id: AGENT.into(),
                device_role: "agent".into(),
                is_active: true,
                ..Default::default()
            },
        );
        m.sessions.get_mut(S1).unwrap().settings = Some(json!({ "archived": true, "vouched": { "account": "acct" } }));
        assert_eq!(to_retire(&m), vec![AGENT.to_string()]);
    }

    /// A session that asks again (resumed, or under another account of its program) is named as it was: by the
    /// session of the same agent identity on the same host, not by its folder (the owner's board, 2026-10-14: the
    /// coordinator came back as "saeckel").
    #[test]
    fn an_ask_of_a_known_agent_keeps_its_name() {
        const OLD: &str = "dddddddddddddddddddddddddddddddd";
        const OLD_DEV: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
        let mut m = room();
        m.members.insert(OLD_DEV.into(), Member { device_id: OLD_DEV.into(), device_role: "agent".into(), host: json!("desktop"), folder: json!("~/git/trommi"), ..Default::default() });
        let s = m.session_of(OLD);
        s.group_id = Some("g-old".into());
        s.agent_device_id = Some(OLD_DEV.into());
        s.card_ids = vec!["c".into()];
        m.human.insert(format!("session/{OLD}"), crate::model::types::HumanRegister { value: json!({ "name": "trommi" }), ..Default::default() });
        m.device_registers.insert(OLD_DEV.into(), json!({ "host": "desktop", "agent": "0123456789abcdef" }).as_object().unwrap().clone());
        let mut a = json!({ "name": "saeckel", "folder": "~/git/saeckel", "host": "desktop", "account": "acct", "agent": "0123456789abcdef" });
        reg(&mut m, S1, "join_ask/a7", a.clone());
        assert_eq!(asks(&m, &BTreeSet::new(), &[])[0].name, "trommi");
        assert_eq!(join_rows(&m, &BTreeMap::new())[0].name, "trommi");
        // another agent, or the same identity on another machine: its own name
        a["agent"] = json!("fedcba9876543210");
        assert_eq!(ask_name(&m, &a), "saeckel");
        a["agent"] = json!("0123456789abcdef");
        a["host"] = json!("laptop");
        assert_eq!(ask_name(&m, &a), "saeckel");
        // no name: the folder's
        assert_eq!(ask_name(&m, &json!({ "folder": "~/git/game/" })), "game");
    }
}
