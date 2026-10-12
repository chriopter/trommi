//! Seats: every agent device of the room sits in a main session (spec/v1.md 5.2.2, 5.2.5). Its inviter founds it
//! right after "They match"; when that inviter cannot (an iPhone sent away in the middle, a tab closed, an app killed
//! before the founding went out), ANY human device of the room does it here, once the agent is connected and has
//! waited a grace without a session. One rule for every app, so that whichever human device is up takes over.
//!
//! The planner holds no IO. The app tells it what it knows (`Known`) at each upkeep and does what it says:
//! - `Plan::Idle`: nothing to do; `recheck_at` says when to look again while an agent waits out its grace;
//! - `Plan::AskHub(candidates)`: read the hub's list of the room's groups and hand the devices that sit in a live
//!   session there to `after_hub`, which names the one agent to found now (one founding at a time).
//!
//! An agent this device saw in a session, archived ones too, or the hub lists in one, is never given a second: the
//! hub refuses a second live main session of the same agent anyway (5.2.2), so two human devices at once found one.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What the app knows at one upkeep.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct Known {
    /// The room's agent devices (hex), by its newest roles.
    pub agents: Vec<String>,
    /// Devices that are a leaf of any session group this device holds (archived ones too).
    pub seated: BTreeSet<String>,
    /// Devices one of this device's own invites is still at work on (their inviter founds them).
    pub coming: BTreeSet<String>,
    /// A founding of a main session by this device waits for the hub's answer.
    pub founding_pending: bool,
    /// The agent devices connected now (the hub's presence: the model's, when the model plans).
    #[serde(skip)]
    pub online: BTreeSet<String>,
    #[serde(skip)]
    pub now_ms: u64,
}

/// What to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Plan {
    Idle { recheck_at: Option<u64> },
    AskHub { candidates: Vec<String> },
}

/// The grace an agent waits without a session before another human device founds it, in ms (its inviter founds it
/// right after "They match": this is for the inviter that went away).
pub const SEAT_GRACE_MS: u64 = 30_000;

#[derive(Debug, Clone)]
pub struct SeatPlanner {
    /// Agent devices seen without a session: since when.
    seatless: BTreeMap<String, u64>,
    pub grace_ms: u64,
}

impl Default for SeatPlanner {
    fn default() -> Self {
        Self::new(SEAT_GRACE_MS)
    }
}

impl SeatPlanner {
    pub fn new(grace_ms: u64) -> Self {
        Self { seatless: BTreeMap::new(), grace_ms }
    }

    /// One upkeep's look.
    pub fn plan(&mut self, k: &Known) -> Plan {
        let waiting: Vec<&String> = k
            .agents
            .iter()
            .filter(|a| !k.seated.contains(*a) && !k.coming.contains(*a))
            .collect();
        self.seatless.retain(|id, _| waiting.contains(&id));
        if waiting.is_empty() || k.founding_pending {
            return Plan::Idle { recheck_at: None };
        }
        let mut due = Vec::new();
        let mut next: Option<u64> = None;
        for a in waiting {
            let since = *self.seatless.entry(a.clone()).or_insert(k.now_ms);
            let at = since + self.grace_ms;
            if k.now_ms >= at && k.online.contains(a) {
                due.push(a.clone());
            } else {
                // (looked at again when its grace ends; an agent that comes online later wakes the upkeep itself)
                next = Some(next.map_or(at.max(k.now_ms + 1), |n: u64| n.min(at.max(k.now_ms + 1))));
            }
        }
        if due.is_empty() {
            Plan::Idle { recheck_at: next }
        } else {
            Plan::AskHub { candidates: due }
        }
    }

    /// After the hub's list: `hub_seated` are the devices that sit in a live session group there. Returns the one
    /// agent to found now, if any.
    pub fn after_hub(&mut self, candidates: &[String], hub_seated: &BTreeSet<String>) -> Option<String> {
        candidates.iter().find(|a| !hub_seated.contains(*a)).cloned()
    }

    /// The founding went out: the agent no longer waits here.
    pub fn founded(&mut self, agent: &str) {
        self.seatless.remove(agent);
    }
}

impl crate::model::Model {
    /// One upkeep's look at the agents without a session: `k` as the app knows it, who is connected from the model.
    pub fn seat_plan(&mut self, mut k: Known, now: u64) -> Plan {
        k.online = self
            .members
            .values()
            .filter(|m| m.is_online)
            .map(|m| m.device_id.clone())
            .collect();
        k.now_ms = now;
        self.seats.plan(&k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(now: u64) -> Known {
        Known {
            agents: vec!["a1".into(), "a2".into(), "a3".into()],
            seated: ["a1".to_string()].into(),
            coming: ["a3".to_string()].into(),
            online: ["a2".to_string()].into(),
            now_ms: now,
            ..Default::default()
        }
    }

    #[test]
    fn a_seatless_connected_agent_is_founded_after_its_grace_once() {
        let mut p = SeatPlanner::new(1000);
        assert_eq!(p.plan(&known(0)), Plan::Idle { recheck_at: Some(1000) });
        assert_eq!(p.plan(&known(999)), Plan::Idle { recheck_at: Some(1000) });
        assert_eq!(p.plan(&known(1000)), Plan::AskHub { candidates: vec!["a2".into()] });
        // the hub has it in a live session already (another device was quicker): nothing
        assert_eq!(p.after_hub(&["a2".into()], &["a2".to_string()].into()), None);
        assert_eq!(p.after_hub(&["a2".into()], &BTreeSet::new()), Some("a2".into()));
        p.founded("a2");
        // while this device's founding waits for the hub: nothing more
        let mut k = known(2000);
        k.founding_pending = true;
        assert_eq!(p.plan(&k), Plan::Idle { recheck_at: None });
    }

    #[test]
    fn an_offline_agent_waits_and_an_inviters_own_one_is_left_to_it() {
        let mut p = SeatPlanner::new(1000);
        let mut k = known(0);
        k.online.clear();
        assert!(matches!(p.plan(&k), Plan::Idle { .. }));
        k.now_ms = 5000;
        assert!(matches!(p.plan(&k), Plan::Idle { .. }), "not connected: not founded");
        k.online.insert("a2".into());
        assert_eq!(p.plan(&k), Plan::AskHub { candidates: vec!["a2".into()] });
        // a3 is coming through this device's own invite: never a candidate
        k.online.insert("a3".into());
        assert_eq!(p.plan(&k), Plan::AskHub { candidates: vec!["a2".into()] });
    }
}

