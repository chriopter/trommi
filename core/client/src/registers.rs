//! Session registers: a session's name, profile and status lines are registers of its group.
//!
//! **A register never creates a session.** Sessions come from the groups this device is in (or the hub's list
//! of them). A register value of a session not known yet is parked, and applied when the session appears; one
//! renamed or written while its session was being joined never stands as a phantom session of its own.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One register value, as the protocol core said it is the current one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegisterValue {
    pub value: serde_json::Value,
    pub change: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegisterOutcome {
    Applied,
    /// The session is not known: kept until it is.
    Parked,
    /// Older than the value held.
    Ignored,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Registers {
    sessions: BTreeSet<String>,
    values: BTreeMap<String, BTreeMap<String, RegisterValue>>,
    parked: BTreeMap<String, BTreeMap<String, RegisterValue>>,
}

impl Registers {
    pub fn new() -> Registers {
        Registers::default()
    }

    /// The sessions known, from groups: the only way a session comes to be.
    pub fn sessions(&self) -> impl Iterator<Item = &str> {
        self.sessions.iter().map(String::as_str)
    }

    /// A session group this device is in (or the hub lists). Parked values are applied now.
    pub fn add_session(&mut self, session: &str) {
        if !self.sessions.insert(session.to_string()) {
            return;
        }
        if let Some(parked) = self.parked.remove(session) {
            for (key, value) in parked {
                self.put(session, &key, value);
            }
        }
    }

    /// A current register value of a session group.
    pub fn apply(
        &mut self,
        session: &str,
        key: &str,
        value: serde_json::Value,
        change: u64,
    ) -> RegisterOutcome {
        let value = RegisterValue { value, change };
        if !self.sessions.contains(session) {
            let slot = self.parked.entry(session.to_string()).or_default();
            if slot.get(key).is_none_or(|v| v.change < change) {
                slot.insert(key.to_string(), value);
            }
            return RegisterOutcome::Parked;
        }
        self.put(session, key, value)
    }

    fn put(&mut self, session: &str, key: &str, value: RegisterValue) -> RegisterOutcome {
        let slot = self.values.entry(session.to_string()).or_default();
        if slot.get(key).is_some_and(|v| v.change >= value.change) {
            return RegisterOutcome::Ignored;
        }
        slot.insert(key.to_string(), value);
        RegisterOutcome::Applied
    }

    pub fn get(&self, session: &str, key: &str) -> Option<&serde_json::Value> {
        self.values.get(session)?.get(key).map(|v| &v.value)
    }

    /// The session's name: the `name` of its `profile` register, or `None` ("Loading …", never its id).
    pub fn name(&self, session: &str) -> Option<String> {
        let profile = self.get(session, "profile")?;
        profile
            .get("name")
            .and_then(|n| n.as_str())
            .filter(|n| !n.is_empty())
            .map(str::to_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_rename_never_creates_a_session() {
        let mut r = Registers::new();
        assert_eq!(
            r.apply("s1", "profile", json!({ "name": "Design" }), 10),
            RegisterOutcome::Parked
        );
        assert_eq!(r.sessions().count(), 0);
        assert_eq!(r.name("s1"), None);
        r.add_session("s1");
        assert_eq!(r.name("s1").as_deref(), Some("Design"));
    }

    #[test]
    fn the_newest_value_stands() {
        let mut r = Registers::new();
        r.add_session("s1");
        r.apply("s1", "profile", json!({ "name": "B" }), 20);
        assert_eq!(
            r.apply("s1", "profile", json!({ "name": "A" }), 10),
            RegisterOutcome::Ignored
        );
        assert_eq!(r.name("s1").as_deref(), Some("B"));
    }
}
