//! Computer approval is a human room-register grant, never an agent's profile or session register.
//! The grant names the exact device: taking a session over on another computer carries no approval.
use crate::model::Model;
use serde_json::Value;
use std::collections::BTreeSet;

fn settings<'a>(m: &'a Model, session: &str) -> Option<&'a Value> {
    m.human.get(&format!("session/{session}")).map(|r| &r.value)
}

/// The device the human approved, including after its removal (for retiring its admitted sessions).
pub fn granted_device<'a>(m: &'a Model, session: &str) -> Option<&'a str> {
    settings(m, session)?
        .get("machine")?
        .get("device")?
        .as_str()
        .filter(|d| !d.is_empty())
}

/// This live main session still belongs to the exact approved computer device.
pub fn approved(m: &Model, session: &str) -> bool {
    let Some(s) = m.sessions.get(session) else {
        return false;
    };
    let Some(device) = granted_device(m, session) else {
        return false;
    };
    s.parent_session_id.is_none()
        && s.agent_device_id.as_deref() == Some(device)
        && s.is_active
        && !s.stale
        && !settings(m, session)
            .and_then(|v| v.get("archived"))
            .is_some_and(crate::model::apply::truthy)
}

/// Computer sessions have a device-management row instead of a conversation, including while offline.
pub fn is_computer(m: &Model, session: &str) -> bool {
    let Some(s) = m.sessions.get(session) else {
        return false;
    };
    s.parent_session_id.is_none()
        && granted_device(m, session).is_some_and(|d| s.agent_device_id.as_deref() == Some(d))
}

/// A separate quota for each approved computer, independent of the account an agent claims.
pub fn quota(session: &str) -> String {
    format!("machine:{session}")
}

fn descendants(m: &Model, roots: BTreeSet<String>) -> BTreeSet<String> {
    let mut sessions = roots;
    loop {
        let before = sessions.len();
        for s in m.sessions.values() {
            if s.parent_session_id
                .as_ref()
                .is_some_and(|p| sessions.contains(p))
            {
                sessions.insert(s.session_id.clone());
            }
        }
        if sessions.len() == before {
            break;
        }
    }
    sessions
        .iter()
        .filter_map(|s| m.sessions.get(s))
        .flat_map(|s| {
            s.agent_device_id
                .iter()
                .chain(s.agent_device_ids.iter())
                .cloned()
        })
        .collect()
}

/// Removing an approved computer also removes every session it admitted, including helpers.
pub fn removal_targets(m: &Model, devices: &[String]) -> Vec<String> {
    let mut out: BTreeSet<String> = devices.iter().cloned().collect();
    let computers: BTreeSet<&str> = m
        .sessions
        .keys()
        .filter_map(|s| {
            granted_device(m, s)
                .filter(|d| out.contains(*d))
                .map(|_| s.as_str())
        })
        .collect();
    let roots = m
        .sessions
        .keys()
        .filter(|s| {
            let Some(v) = settings(m, s).and_then(|v| v.get("vouched")) else {
                return false;
            };
            let by_computer = v
                .get("by")
                .and_then(Value::as_str)
                .is_some_and(|by| computers.contains(by));
            let same_device = v.get("machine").is_none()
                || v.get("device").and_then(Value::as_str)
                    == m.sessions
                        .get(*s)
                        .and_then(|s| s.agent_device_id.as_deref());
            by_computer && same_device
        })
        .cloned()
        .collect();
    out.extend(descendants(m, roots));
    out.into_iter().collect()
}

/// Catch a removal made by another human device, including an older app without cascade removal.
/// `enrolled` is the engine's verified room membership, never the hub's device list or online status.
pub fn retired_children(m: &Model, enrolled: &BTreeSet<String>) -> Vec<String> {
    let roots = m
        .sessions
        .keys()
        .filter(|s| {
            let Some(v) = settings(m, s).and_then(|v| v.get("vouched")) else {
                return false;
            };
            let Some(device) = v.get("machine").and_then(Value::as_str) else {
                return false;
            };
            let child = m
                .sessions
                .get(*s)
                .and_then(|s| s.agent_device_id.as_deref());
            // Missing registers may just be unread. Only verified room membership proves the parent is out.
            // A session taken over elsewhere is another device, which this computer did not admit.
            !enrolled.contains(device)
                && child.is_some()
                && v.get("device").and_then(Value::as_str) == child
        })
        .cloned()
        .collect();
    descendants(m, roots).into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{HumanRegister, RegisterValue};
    use serde_json::json;

    fn room() -> Model {
        let mut m = Model::new();
        let s = m.session_of("computer");
        s.agent_device_id = Some("machine-device".into());
        s.is_active = true;
        m
    }
    fn human(m: &mut Model, session: &str, value: Value) {
        m.session_of(session).settings = Some(value.clone());
        m.human.insert(
            format!("session/{session}"),
            HumanRegister {
                value,
                ..Default::default()
            },
        );
    }
    fn grant(m: &mut Model) {
        human(
            m,
            "computer",
            json!({"machine":{"device":"machine-device"}}),
        );
    }

    #[test]
    fn agent_claims_and_profile_changes_never_grant_computer_authority() {
        let mut m = room();
        m.session_of("computer").settings = Some(json!({"machine":{"device":"machine-device"}}));
        m.session_of("computer").registers.insert(
            "session/computer".into(),
            RegisterValue {
                value: json!({"machine":{"device":"machine-device"}}),
                ..Default::default()
            },
        );
        assert!(!approved(&m, "computer"));
        grant(&mut m);
        assert!(approved(&m, "computer"));
        m.session_of("computer").agent_device_id = Some("replacement-device".into());
        assert!(
            !approved(&m, "computer"),
            "a takeover must be approved again"
        );
    }

    #[test]
    fn stale_or_archived_computers_admit_nobody_but_staleness_does_not_remove_children() {
        let mut m = room();
        grant(&mut m);
        human(
            &mut m,
            "child",
            json!({"vouched":{"by":"computer","machine":"machine-device","device":"child-device"}}),
        );
        m.session_of("child").agent_device_id = Some("child-device".into());
        m.session_of("computer").stale = true;
        assert!(!approved(&m, "computer"));
        assert!(retired_children(&m, &["machine-device".into()].into()).is_empty());
        m.session_of("computer").stale = false;
        human(
            &mut m,
            "computer",
            json!({"machine":{"device":"machine-device"},"archived":true}),
        );
        assert!(!approved(&m, "computer"));
    }

    #[test]
    fn removing_a_computer_retires_its_sessions_and_helpers_without_another_computers_sessions() {
        let mut m = room();
        grant(&mut m);
        human(
            &mut m,
            "child",
            json!({"vouched":{"by":"computer","machine":"machine-device","device":"child-device"}}),
        );
        m.session_of("child").agent_device_id = Some("child-device".into());
        m.session_of("helper").parent_session_id = Some("child".into());
        m.session_of("helper").agent_device_id = Some("helper-device".into());
        human(
            &mut m,
            "other",
            json!({"vouched":{"by":"another","machine":"other-machine","device":"other-device"}}),
        );
        m.session_of("other").agent_device_id = Some("other-device".into());
        assert_eq!(
            removal_targets(&m, &["machine-device".into()]),
            ["child-device", "helper-device", "machine-device"]
        );
        assert_eq!(
            retired_children(&m, &["other-machine".into()].into()),
            ["child-device", "helper-device"]
        );
        // An unread grant of the unrelated computer cannot prove a removal.
        human(
            &mut m,
            "another",
            json!({"machine":{"device":"other-machine"}}),
        );
        assert_eq!(
            retired_children(&m, &["other-machine".into()].into()),
            ["child-device", "helper-device"]
        );
    }

    #[test]
    fn unread_grants_do_not_remove_devices_and_a_session_takeover_does_not_inherit_the_parent() {
        let mut m = room();
        grant(&mut m);
        human(
            &mut m,
            "child",
            json!({"vouched":{"by":"computer","machine":"machine-device","device":"child-device"}}),
        );
        m.session_of("child").agent_device_id = Some("child-device".into());
        human(&mut m, "computer", Value::Null);
        assert!(retired_children(&m, &["machine-device".into()].into()).is_empty());
        assert_eq!(retired_children(&m, &BTreeSet::new()), ["child-device"]);
        m.session_of("child").agent_device_id = Some("replacement-child".into());
        assert!(retired_children(&m, &BTreeSet::new()).is_empty());
        grant(&mut m);
        assert_eq!(
            removal_targets(&m, &["machine-device".into()]),
            ["machine-device"]
        );
    }
}
