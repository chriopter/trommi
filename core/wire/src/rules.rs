//! Pure membership-change rules over verified public facts. No cryptography, private keys or I/O.
//! Callers authenticate and describe a Commit, then apply these same decisions on the hub and devices.
use std::collections::BTreeSet;
pub type Device = [u8; 32];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub code: &'static str,
    pub message: &'static str,
}
type Res<T> = Result<T, Refused>;
fn refuse(code: &'static str, message: &'static str) -> Refused {
    Refused { code, message }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Room<'a> {
    pub recovery_signature_key: &'a [u8],
    pub recovery_hpke_key: &'a [u8],
    pub agents: &'a [Device],
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot<'a> {
    pub leaves: &'a [Device],
    pub room: Option<Room<'a>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    Member(Device),
    External(Device),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Added {
    pub device: Device,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFacts<'a> {
    pub by: By,
    pub adds: Vec<Added>,
    pub removes: &'a [Device],
    pub changes_extensions: bool,
    pub before: Snapshot<'a>,
    pub after_room: Option<Room<'a>>,
    pub after_leaves: Option<&'a [Device]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cut {
    pub device: Device,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitNote {
    pub room_epoch: u64,
    pub room_state: [u8; 32],
    pub cuts: Vec<Cut>,
    pub join: bool,
}
/// What a signature key is to the room now (4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    Human,
    Agent,
    /// once a human or agent device, no longer: never returns (4.2)
    Revoked,
    /// a key that never was any of the three: it can only be a helper device
    Helper,
}

/// The room's public state at its current epoch, as far as the rules need it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomView {
    pub epoch: u64,
    pub state: [u8; 32],
    pub humans: BTreeSet<Device>,
    pub agents: BTreeSet<Device>,
    pub revoked: BTreeSet<Device>,
}

impl RoomView {
    pub fn standing(&self, d: &Device) -> Standing {
        standing(
            self.humans.contains(d),
            self.agents.contains(d),
            self.revoked.contains(d),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Main,
    /// with the agent leaf of its main session now, if that has one: the helper session's opener
    Helper {
        opener: Option<Device>,
    },
}

use crate::mls::MAX_HELPER_DEVICES;

fn bad(message: &'static str) -> Refused {
    refuse("bad-commit", message)
}

/// 3.3: the note every Commit carries, against the room state the hub holds. `room_epoch` must be the current
/// room epoch (5.2.1), in the room group the epoch the Commit builds on.
pub fn check_note(
    note: &CommitNote,
    room_epoch: u64,
    room_state: &[u8; 32],
    facts: &CommitFacts,
) -> Res<()> {
    if note.room_epoch < room_epoch {
        return Err(refuse(
            "room-behind",
            "the Commit names an older room epoch: process the room group first",
        ));
    }
    if note.room_epoch > room_epoch || &note.room_state != room_state {
        return Err(bad("the Commit's note does not name the room's state"));
    }
    if note.join != matches!(facts.by, By::External(_)) {
        return Err(bad("the note's join mark does not fit the Commit"));
    }
    let mut removed = facts.removes.to_vec();
    removed.sort();
    let cut: Vec<Device> = note.cuts.iter().map(|c| c.device).collect();
    // A join from outside may drop its own old leaf; that needs no Cut (the same device goes on).
    if matches!(facts.by, By::External(_)) {
        if !cut.is_empty() {
            return Err(bad("a join from outside carries no Cut"));
        }
    } else if removed != cut {
        return Err(bad("one Cut for every removed device, ascending"));
    }
    Ok(())
}

/// The leaves of a session group that the room's state does not allow: a revoked key; in a main session a key
/// that is neither human nor agent; in a helper session an agent leaf that is not its main session's agent leaf.
/// A group with any such leaf is stale (5.2.8).
pub fn offending_leaves(kind: SessionKind, room: &RoomView, leaves: &[Device]) -> Vec<Device> {
    let entries = leaf_facts(room, leaves);
    disallowed(&SessionLeaves {
        kind,
        entries: &entries,
    })
}

/// 5.2.8: a session group is stale while it holds a leaf the room's state does not allow, and a helper session
/// also while it lacks the opener its main session has now (a takeover that is not finished there).
pub fn is_stale(kind: SessionKind, room: &RoomView, leaves: &[Device]) -> bool {
    let entries = leaf_facts(room, leaves);
    staleness(&SessionLeaves {
        kind,
        entries: &entries,
    })
    .is_stale()
}

/// 5.2.2 and 5.2.3: what the leaves of a session group may be after a Commit.
pub fn check_session_leaves(kind: SessionKind, room: &RoomView, leaves: &[Device]) -> Res<()> {
    let agents = leaves
        .iter()
        .filter(|d| room.standing(d) == Standing::Agent)
        .count();
    let helpers = leaves
        .iter()
        .filter(|d| room.standing(d) == Standing::Helper)
        .count();
    if kind == SessionKind::Main && agents > 1 {
        return Err(bad("a main session has one agent device"));
    }
    if is_stale(kind, room, leaves) {
        return Err(refuse(
            "stale-session",
            "the group holds a leaf the room's state does not allow, or lacks its opener",
        ));
    }
    match kind {
        SessionKind::Helper { .. } if helpers > MAX_HELPER_DEVICES => {
            Err(bad("a helper session has at most 7 helper devices"))
        }
        _ => Ok(()),
    }
}

/// Membership decisions for a session Commit. Callers check signatures, the named room state,
/// cross-session exclusivity and availability of human KeyPackages separately.
pub fn check_session_commit(
    kind: SessionKind,
    room: &RoomView,
    facts: &CommitFacts,
    founding: bool,
) -> Res<()> {
    if facts.changes_extensions {
        return Err(bad("a session's extension never changes"));
    }
    let leaves = transition_leaves(facts)?;
    let (committer, external) = match facts.by {
        By::Member(id) => (id, false),
        By::External(id) => (id, true),
    };
    let human = room.standing(&committer) == Standing::Human;
    let opener = matches!(kind, SessionKind::Helper { opener: Some(id) } if id == committer)
        && room.standing(&committer) == Standing::Agent;
    if external {
        if !human
            || founding
            || !facts.adds.is_empty()
            || facts.removes.len() > 1
            || facts.removes.iter().any(|id| *id != committer)
        {
            return Err(bad(
                "only a human device joins from outside, replacing at most its own leaf",
            ));
        }
        return Ok(());
    }
    if !human && !opener {
        return Err(bad("this device commits nothing in this group"));
    }
    for add in &facts.adds {
        let role = room.standing(&add.device);
        let fits = if opener {
            role == Standing::Helper || (founding && role == Standing::Human)
        } else {
            allows_device(role, kind, add.device)
                && (kind == SessionKind::Main || role != Standing::Helper)
        };
        if !fits {
            return Err(bad("the committer cannot add this device"));
        }
    }
    for gone in facts.removes {
        let role = room.standing(gone);
        if opener && role == Standing::Human {
            return Err(bad("an opener removes no human device"));
        }
        if kind == (SessionKind::Helper { opener: None }) {
            let again = role == Standing::Human && facts.adds.iter().any(|add| add.device == *gone);
            if allows_device(role, kind, *gone) && !again {
                return Err(bad(
                    "a waiting helper loses only unfit leaves or a human leaf replaced immediately",
                ));
            }
        }
        if (kind == SessionKind::Main && role == Standing::Agent)
            || matches!(kind, SessionKind::Helper { opener: Some(id) } if id == *gone && role == Standing::Agent)
        {
            return Err(bad("an enrolled session agent keeps its leaf"));
        }
    }
    let entries = leaf_facts(room, facts.before.leaves);
    let stale_before = staleness(&SessionLeaves {
        kind,
        entries: &entries,
    });
    if stale_before.is_stale() && !founding {
        let removed: BTreeSet<_> = facts.removes.iter().copied().collect();
        let unfit: BTreeSet<_> = stale_before.disallowed.iter().copied().collect();
        if removed != unfit
            || facts
                .adds
                .iter()
                .any(|add| room.standing(&add.device) == Standing::Human)
        {
            return Err(refuse(
                "stale-session",
                "a stale session takes exactly its repair",
            ));
        }
    }
    let entries = leaf_facts(room, &leaves);
    let stale = staleness(&SessionLeaves {
        kind,
        entries: &entries,
    });
    if stale.is_stale() {
        return Err(
            if stale
                .disallowed
                .iter()
                .any(|id| !facts.before.leaves.contains(id))
            {
                bad("the Commit introduces an unfit leaf")
            } else {
                refuse("stale-session", "the session still needs repair")
            },
        );
    }
    if helper_devices(&SessionLeaves {
        kind,
        entries: &entries,
    })
    .len()
        > MAX_HELPER_DEVICES
    {
        return Err(refuse(
            "too-many",
            "a helper session has at most 7 helper devices",
        ));
    }
    if founding {
        if !facts.removes.is_empty()
            || facts.before.leaves.len() != 1
            || !room.humans.iter().any(|id| leaves.contains(id))
        {
            return Err(bad(
                "a founding Commit starts with one leaf, removes nobody and includes a human",
            ));
        }
        match kind {
            SessionKind::Helper { .. } if !opener => {
                return Err(bad("the opener founds a helper session"))
            }
            SessionKind::Main
                if leaves
                    .iter()
                    .filter(|id| room.standing(id) == Standing::Agent)
                    .count()
                    != 1 =>
            {
                return Err(bad("a main session is founded with its agent device"))
            }
            _ => {}
        }
    }
    Ok(())
}

/// What a room Commit changes besides the tree, read from `TrommiRoom` before and after.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoomChange {
    pub humans_added: Vec<Device>,
    pub humans_removed: Vec<Device>,
    pub agents_added: Vec<Device>,
    pub agents_removed: Vec<Device>,
    pub recovery_keys_replaced: bool,
    pub joined_from_outside: Option<Device>,
}

/// 5.1.2 to 5.1.4 and 4.2: a Commit in the room group. `humans` is the limit in force (1000, or 1001 during a
/// recovery). Whether an Add or a new agent key is the outcome of an invite (12.1.7), and whether the recovery
/// keys may change in this request (8.6), is the caller's to check from the returned change.
pub fn check_room_commit(
    room: &RoomView,
    facts: &CommitFacts,
    humans: usize,
    agents: usize,
) -> Res<RoomChange> {
    let (Some(before), Some(after)) = (&facts.before.room, &facts.after_room) else {
        return Err(bad("the room group keeps its TrommiRoom"));
    };
    let mut change = RoomChange::default();
    match &facts.by {
        // every leaf of the room group is a human device
        By::Member(committer) => {
            if room.standing(committer) != Standing::Human {
                return Err(bad("only a human device commits in the room"));
            }
        }
        By::External(joiner) => {
            if facts.before.leaves.contains(joiner) || room.standing(joiner) != Standing::Helper {
                return Err(bad(
                    "a join from outside brings a key the room has not seen",
                ));
            }
            change.joined_from_outside = Some(*joiner);
            change.humans_added.push(*joiner);
        }
    }
    if facts.adds.len() > 1 {
        return Err(bad("one new human device per Commit"));
    }
    for add in &facts.adds {
        change.humans_added.push(add.device);
    }
    change.humans_removed = facts.removes.to_vec();
    if before != after && !facts.changes_extensions {
        return Err(bad("the room's statement changed without a proposal"));
    }
    let (old, new): (BTreeSet<Device>, BTreeSet<Device>) = (agent_set(before), agent_set(after));
    change.agents_added = new.difference(&old).copied().collect();
    change.agents_removed = old.difference(&new).copied().collect();
    change.recovery_keys_replaced = before.recovery_signature_key != after.recovery_signature_key
        || before.recovery_hpke_key != after.recovery_hpke_key;
    if change.recovery_keys_replaced
        && (before.recovery_signature_key == after.recovery_signature_key
            || before.recovery_hpke_key == after.recovery_hpke_key)
    {
        return Err(bad("both recovery keys are replaced together"));
    }
    // 4.2: a key has one role for ever, and a revoked key never returns.
    for d in change.humans_added.iter().chain(change.agents_added.iter()) {
        if room.standing(d) != Standing::Helper {
            return Err(bad("a key that already has or had a role in the room"));
        }
    }
    let leaves: BTreeSet<Device> = transition_leaves(facts)?.into_iter().collect();
    if leaves.intersection(&new).next().is_some() {
        return Err(bad("a human device is not an agent device"));
    }
    // 8.1: neither recovery key equals a device's key
    for key in [&after.recovery_signature_key, &after.recovery_hpke_key] {
        if leaves.iter().any(|d| d[..] == key[..]) || new.iter().any(|d| d[..] == key[..]) {
            return Err(bad("a recovery key that is a device's key"));
        }
    }
    let recovering = matches!(facts.by, By::External(_))
        || (room.humans.len() > crate::mls::MAX_HUMAN_DEVICES && facts.adds.is_empty());
    let humans = if recovering {
        humans
    } else {
        humans.min(crate::mls::MAX_HUMAN_DEVICES)
    };
    if leaves.len() > humans {
        return Err(refuse(
            "too-many",
            "the room holds its limit of human devices",
        ));
    }
    if new.len() > agents {
        return Err(refuse(
            "too-many",
            "the room holds its limit of agent devices",
        ));
    }
    Ok(change)
}

fn agent_set(room: &Room) -> BTreeSet<Device> {
    room.agents.iter().copied().collect()
}

/// Classify a public device at the room epoch being judged; revocation wins over an agent claim.
pub fn standing(human: bool, agent: bool, revoked: bool) -> Standing {
    if human {
        Standing::Human
    } else if revoked {
        Standing::Revoked
    } else if agent {
        Standing::Agent
    } else {
        Standing::Helper
    }
}
/// Whether one leaf's public role is allowed in this session.
pub fn allows_device(role: Standing, kind: SessionKind, device: Device) -> bool {
    match (role, kind) {
        (Standing::Human, _) => true,
        (Standing::Revoked, _) => false,
        (Standing::Agent, SessionKind::Main) => true,
        (Standing::Agent, SessionKind::Helper { opener }) => opener == Some(device),
        (Standing::Helper, SessionKind::Helper { .. }) => true,
        (Standing::Helper, SessionKind::Main) => false,
    }
}

/// A verified leaf's public role, supplied without any MLS types or secret state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leaf {
    pub device: Device,
    pub role: Standing,
}
/// The facts needed to judge a session's leaves. `opener` is the enrolled agent of its main session now.
pub struct SessionLeaves<'a> {
    pub kind: SessionKind,
    pub entries: &'a [Leaf],
}
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Staleness {
    pub disallowed: Vec<Device>,
    pub missing_opener: Option<Device>,
}
impl Staleness {
    pub fn is_stale(&self) -> bool {
        !self.disallowed.is_empty() || self.missing_opener.is_some()
    }
}
fn leaf_facts(room: &RoomView, leaves: &[Device]) -> Vec<Leaf> {
    leaves
        .iter()
        .map(|id| Leaf {
            device: *id,
            role: room.standing(id),
        })
        .collect()
}
/// Every leaf the room does not allow, in device order. A main session with two agents allows neither.
pub fn disallowed(facts: &SessionLeaves<'_>) -> Vec<Device> {
    let mut unfit: Vec<_> = facts
        .entries
        .iter()
        .filter(|leaf| !allows_device(leaf.role, facts.kind, leaf.device))
        .map(|leaf| leaf.device)
        .collect();
    if facts.kind == SessionKind::Main {
        let agents: Vec<_> = facts
            .entries
            .iter()
            .filter(|leaf| leaf.role == Standing::Agent)
            .map(|leaf| leaf.device)
            .collect();
        if agents.len() > 1 {
            unfit.extend(agents);
        }
    }
    unfit.sort_unstable();
    unfit
}
/// The one stale-group predicate for the hub, members and observers.
pub fn staleness(facts: &SessionLeaves<'_>) -> Staleness {
    let missing_opener = match facts.kind {
        SessionKind::Helper { opener } => {
            opener.filter(|id| !facts.entries.iter().any(|leaf| leaf.device == *id))
        }
        SessionKind::Main => None,
    };
    Staleness {
        disallowed: disallowed(facts),
        missing_opener,
    }
}
/// Whether a leaf is neither human nor the opener (including a disallowed leaf, until repaired).
pub fn other_leaf(role: Standing, kind: SessionKind, device: Device) -> bool {
    role != Standing::Human
        && !matches!(kind, SessionKind::Helper { opener: Some(id) } if id == device)
}
pub fn helper_devices(facts: &SessionLeaves<'_>) -> Vec<Device> {
    if facts.kind == SessionKind::Main {
        return Vec::new();
    }
    facts
        .entries
        .iter()
        .filter(|leaf| other_leaf(leaf.role, facts.kind, leaf.device))
        .map(|leaf| leaf.device)
        .collect()
}

/// Apply the verified membership changes once. Observers can also supply their resulting tree for comparison.
pub fn transition_leaves(facts: &CommitFacts<'_>) -> Res<Vec<Device>> {
    let mut leaves: BTreeSet<Device> = facts.before.leaves.iter().copied().collect();
    if leaves.len() != facts.before.leaves.len() {
        return Err(bad("duplicate device in the group"));
    }
    if let By::Member(id) = facts.by {
        if !leaves.contains(&id) {
            return Err(bad("the committer is not a leaf"));
        }
    }
    for gone in facts.removes {
        if !leaves.remove(gone) {
            return Err(bad("a Remove names no leaf"));
        }
    }
    if let By::External(id) = facts.by {
        if !leaves.insert(id) {
            return Err(bad("the joining device is a leaf already"));
        }
    }
    for added in &facts.adds {
        if !leaves.insert(added.device) {
            return Err(bad("an Add repeats a device"));
        }
    }
    if let Some(after) = facts.after_leaves {
        let actual: BTreeSet<_> = after.iter().copied().collect();
        if after.len() != actual.len() || actual != leaves {
            return Err(bad("the tree disagrees with its changes"));
        }
    }
    Ok(leaves.into_iter().collect())
}
