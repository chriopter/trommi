//! Adapt the hub observer's verified facts to the pure rules shared with members.
use crate::error::{refuse, Res};
use crate::observer::{By, CommitFacts, Device};
use trommi_wire::layout::CommitNote;
use trommi_wire::rules as wire;
pub use wire::{is_stale, offending_leaves, RoomChange, RoomView, SessionKind, Standing};

fn refused(error: wire::Refused) -> crate::error::Refused {
    refuse(error.code, error.message)
}
fn snapshot(value: &crate::observer::Snapshot) -> wire::Snapshot<'_> {
    wire::Snapshot {
        leaves: &value.leaves,
        room: value.room.as_ref().map(|room| wire::Room {
            agents: &room.agents,
            recovery_signature_key: &room.recovery_signature_key,
            recovery_hpke_key: &room.recovery_hpke_key,
        }),
    }
}
fn facts(value: &CommitFacts) -> wire::CommitFacts<'_> {
    wire::CommitFacts {
        by: match value.by {
            By::Member(id) => wire::By::Member(id),
            By::External(id) => wire::By::External(id),
        },
        adds: value
            .adds
            .iter()
            .map(|add| wire::Added { device: add.device })
            .collect(),
        removes: &value.removes,
        changes_extensions: value.changes_extensions,
        before: snapshot(&value.before),
        after_room: snapshot(&value.after).room,
        after_leaves: Some(&value.after.leaves),
    }
}
pub fn check_note(
    note: &CommitNote,
    room_epoch: u64,
    room_state: &[u8; 32],
    change: &CommitFacts,
) -> Res<()> {
    let note = wire::CommitNote {
        room_epoch: note.room_epoch,
        room_state: note.room_state,
        join: note.join,
        cuts: note
            .cuts
            .iter()
            .map(|cut| wire::Cut { device: cut.device })
            .collect(),
    };
    wire::check_note(&note, room_epoch, room_state, &facts(change)).map_err(refused)
}
pub fn check_session_leaves(kind: SessionKind, room: &RoomView, leaves: &[Device]) -> Res<()> {
    wire::check_session_leaves(kind, room, leaves).map_err(refused)
}
pub fn check_session_commit(
    kind: SessionKind,
    room: &RoomView,
    change: &CommitFacts,
    founding: bool,
) -> Res<()> {
    wire::check_session_commit(kind, room, &facts(change), founding).map_err(refused)
}
pub fn check_room_commit(
    room: &RoomView,
    change: &CommitFacts,
    humans: usize,
    agents: usize,
) -> Res<RoomChange> {
    wire::check_room_commit(room, &facts(change), humans, agents).map_err(refused)
}
