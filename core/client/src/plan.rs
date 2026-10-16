//! The catch-up plan, newest to oldest, without IO: the host asks [`CatchUp::next`] what to fetch, fetches it,
//! hands the result through the protocol core, and reports back. The rules (the load rules of 2026-10-11, measured
//! on the web: a fresh login spent its 60 s on one `GET /v1/welcomes` per history entry, read the history three
//! times and the whole Desk twice per open and again every minute):
//!
//! 1. [`Step::ReadDesk`]: the hub's open list, once per open. With a cursor from an earlier read and a hub that
//!    serves it, only what changed since (`since`). Never on a timer: updates come from the stream.
//! 2. [`Step::ReadObjects`]: the current version of each listed object this device has not read, all in one step
//!    (the host fetches them in parallel). After that the Desk is complete ([`CatchUp::desk_complete`]).
//! 3. [`Step::ReadLog`]: the room's log after the cursor, oldest first, page by page, ONCE. The MLS Commits (epochs)
//!    and each sender's chain check need that order; it is the protocol core's, and runs in the background.
//!    Envelopes it brings at or below the Desk's change are history ([`crate::Source::History`]): they may only close.
//! 4. [`Step::Welcomes`]: at most once per page of the log, and only when that page had a Commit of a group this
//!    device does not hold (a Welcome may wait for it), never once per entry.
//! 5. [`Step::Live`]: at the head; the stream takes over and every fact is [`crate::Source::Live`]. The Desk is
//!    read again only when a session group was joined (the hub lists only what lies in the asker's groups).

use crate::board::{Board, Source};
use crate::desk::Listing;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum Step {
    /// The hub's Desk; `since`: only what changed after that change (a hub that serves deltas).
    ReadDesk {
        since: Option<u64>,
    },
    /// Object ids (hex) whose listed version to read through the core, out of the hub's order, in parallel.
    ReadObjects {
        objects: Vec<String>,
    },
    /// The next page of the log after this change, oldest first.
    ReadLog {
        after: u64,
    },
    /// The device's Welcomes, once for the page just processed.
    Welcomes,
    Live,
}

/// What the host's hub can do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubCan {
    /// `GET /v1/desk?since=<change>` answers with what changed since.
    pub desk_delta: bool,
}

/// How often each step was asked for, for the host's log and the load tests.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub desk_reads: u32,
    pub object_reads: u32,
    pub log_pages: u32,
    pub welcome_checks: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CatchUp {
    can: HubCan,
    listing: Option<Listing>,
    desk_read: bool,
    desk_again: bool,
    objects_asked: bool,
    cursor: u64,
    log_done: bool,
    welcomes_due: bool,
    pub counts: Counts,
}

impl CatchUp {
    /// A catch-up from the device's cursor. `listing`: the Desk as read before (a warm start), whose change is
    /// the cursor of a delta.
    pub fn new(cursor: u64, can: HubCan, listing: Option<Listing>) -> CatchUp {
        CatchUp {
            can,
            listing,
            cursor,
            ..CatchUp::default()
        }
    }

    pub fn listing(&self) -> Option<&Listing> {
        self.listing.as_ref()
    }

    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// What to fetch next.
    pub fn next(&mut self, board: &Board) -> Step {
        if !self.desk_read || self.desk_again {
            self.desk_read = true;
            self.desk_again = false;
            self.counts.desk_reads += 1;
            let since = if self.can.desk_delta {
                self.listing.as_ref().map(|l| l.at)
            } else {
                None
            };
            return Step::ReadDesk { since };
        }
        if !self.objects_asked {
            self.objects_asked = true;
            let missing = self.missing(board);
            if !missing.is_empty() {
                self.counts.object_reads += 1;
                return Step::ReadObjects { objects: missing };
            }
        }
        if self.welcomes_due {
            self.welcomes_due = false;
            self.counts.welcome_checks += 1;
            return Step::Welcomes;
        }
        if !self.log_done {
            self.counts.log_pages += 1;
            return Step::ReadLog { after: self.cursor };
        }
        Step::Live
    }

    fn missing(&self, board: &Board) -> Vec<String> {
        let Some(listing) = &self.listing else {
            return Vec::new();
        };
        listing
            .open
            .iter()
            .filter(|o| o.version_change != 0)
            .filter(|o| {
                board
                    .get(&o.object_id)
                    .is_none_or(|b| !b.is_read() || b.version_change < o.version_change)
            })
            .map(|o| o.object_id.clone())
            .collect()
    }

    /// The Desk the hub answered. A delta (`since` was asked: `delta_closed` names what left) is laid over the list
    /// held: what it names as open is added or replaced, what it names as closed leaves.
    pub fn on_team(&mut self, listing: Listing, delta_closed: Option<&[String]>) {
        self.objects_asked = false;
        if listing.truncated {
            // a list cut short decides nothing; the log brings the rest
            self.objects_asked = true;
            return;
        }
        match (delta_closed, self.listing.take()) {
            (Some(closed), Some(mut held)) => {
                held.open.retain(|o| {
                    !closed.contains(&o.object_id)
                        && !listing.open.iter().any(|n| n.object_id == o.object_id)
                });
                held.open.extend(listing.open);
                held.at = listing.at;
                self.listing = Some(held);
            }
            _ => self.listing = Some(listing),
        }
    }

    /// The Desk is complete: a hub list was read and every listed version was asked for once.
    pub fn desk_complete(&self) -> bool {
        self.listing.is_some() && self.objects_asked
    }

    /// A page of the log was processed up to `cursor`; `more`: the hub has more; `unknown_group_commits`: the page
    /// had Commits of groups this device holds no state of (a Welcome may wait for one of them).
    pub fn on_log(&mut self, cursor: u64, more: bool, unknown_group_commits: bool) {
        let moved = cursor > self.cursor;
        self.cursor = self.cursor.max(cursor);
        if unknown_group_commits {
            self.welcomes_due = true;
        }
        if !more || !moved {
            self.log_done = true;
        }
    }

    /// Where an envelope at `change` read from the log counts: at or below the Desk's change it is history.
    pub fn source_of(&self, change: u64) -> Source {
        match &self.listing {
            Some(l) if !l.truncated && change <= l.at => Source::History,
            _ => Source::Live,
        }
    }

    /// A session group was joined: the hub's Desk is read again (as a delta where the hub can).
    pub fn on_session_joined(&mut self) {
        self.desk_again = true;
    }

    /// Catching up again later in the same run (the device was away, or a host without a stream looks again):
    /// the log from `cursor` on, and NOT the Desk again: what changed since comes in the log.
    pub fn resume(&mut self, cursor: u64) {
        self.cursor = self.cursor.max(cursor);
        self.log_done = false;
    }

    /// The stream brought an item at `change`: the cursor moves; nothing is polled.
    pub fn on_stream(&mut self, change: u64) {
        self.cursor = self.cursor.max(change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desk::Listed;
    use crate::Kind;

    fn listed(id: &str, v: u64) -> Listed {
        Listed {
            object_id: id.into(),
            kind: Kind::Card,
            session_id: None,
            version_change: v,
            first_change: v,
        }
    }

    #[test]
    fn newest_first_then_the_history_once() {
        let board = Board::new();
        let mut plan = CatchUp::new(0, HubCan::default(), None);
        assert_eq!(plan.next(&board), Step::ReadDesk { since: None });
        plan.on_team(
            Listing {
                at: 500,
                truncated: false,
                open: vec![listed("a", 400), listed("b", 450)],
            },
            None,
        );
        assert_eq!(
            plan.next(&board),
            Step::ReadObjects {
                objects: vec!["a".into(), "b".into()]
            }
        );
        assert!(plan.desk_complete());
        assert_eq!(plan.next(&board), Step::ReadLog { after: 0 });
        assert_eq!(plan.source_of(450), Source::History);
        assert_eq!(plan.source_of(501), Source::Live);
        plan.on_log(250, true, false);
        assert_eq!(plan.next(&board), Step::ReadLog { after: 250 });
        plan.on_log(600, false, false);
        assert_eq!(plan.next(&board), Step::Live);
        // live: never the Desk or the history again by itself
        for _ in 0..5 {
            assert_eq!(plan.next(&board), Step::Live);
        }
        assert_eq!(
            plan.counts,
            Counts {
                desk_reads: 1,
                object_reads: 1,
                log_pages: 2,
                welcome_checks: 0
            }
        );
    }

    #[test]
    fn welcomes_once_per_page_that_needs_them() {
        let board = Board::new();
        let mut plan = CatchUp::new(0, HubCan::default(), None);
        plan.next(&board);
        plan.on_team(
            Listing {
                at: 10,
                truncated: false,
                open: vec![],
            },
            None,
        );
        let mut pages = 0;
        loop {
            match plan.next(&board) {
                Step::ReadLog { after } => {
                    pages += 1;
                    // every page holds Commits of groups not held yet: one check per page, not per entry
                    plan.on_log(after + 500, pages < 4, true);
                }
                Step::Welcomes => {}
                Step::Live => break,
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(plan.counts.welcome_checks, 4);
        assert_eq!(plan.counts.log_pages, 4);
    }

    #[test]
    fn a_warm_start_reads_the_desk_as_a_delta() {
        let mut board = Board::new();
        board.apply(&crate::Fact {
            object_id: "a".into(),
            kind: Kind::Card,
            session_id: None,
            change: 400,
            state: crate::State::Open,
            is_version: true,
            body: Some(serde_json::json!({})),
            sent_at: 0,
            expires_at: 0,
            hand_back: None,
            source: Source::Live,
        });
        let before = Listing {
            at: 500,
            truncated: false,
            open: vec![listed("a", 400), listed("gone", 300)],
        };
        let mut plan = CatchUp::new(500, HubCan { desk_delta: true }, Some(before));
        assert_eq!(plan.next(&board), Step::ReadDesk { since: Some(500) });
        plan.on_team(
            Listing {
                at: 700,
                truncated: false,
                open: vec![listed("c", 650)],
            },
            Some(&["gone".to_string()]),
        );
        let ids: Vec<&str> = plan
            .listing()
            .unwrap()
            .open
            .iter()
            .map(|o| o.object_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "c"]);
        // only what changed is fetched
        assert_eq!(
            plan.next(&board),
            Step::ReadObjects {
                objects: vec!["c".into()]
            }
        );
        plan.on_session_joined();
        assert_eq!(plan.next(&board), Step::ReadDesk { since: Some(700) });
    }
}
