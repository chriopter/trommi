//! Off your mind as one pile (the owner, 13 October 2026: "nicht zwei Stapel", his pick "untereinander"): what the
//! agents are working on and what is done or put off lie on ONE pile at the foot of the Desk. The first
//! [`PILE_TOP`] running cards stand on top, one under the other; everything else lies on the pile, which says how
//! much lies on it ("+2 running · 1 put off · 14 done"). Opened, the pile shows the other running cards, then the
//! put-off ones, then the done ones, small and quiet at the bottom. On the Desk at most [`PILE_REST`] of the put-off
//! and done ones are shown; the page Off your mind (`full`) shows every one, the pile opened.
//!
//! One rule for every app: which card is running, put off or done is the app's place rule (web `app.mjs` stackOf,
//! iOS `DeskModel.stackOf`, the same as [`crate::status::card_place`]); how the pile is laid out from those counts
//! is decided here. The counts are the real totals, never only the rows an app has drawn.
use serde::Serialize;

/// How many running cards stand on top of the pile.
pub const PILE_TOP: u64 = 3;
/// How many put-off and done cards the opened pile shows on the Desk (the page Off your mind shows them all).
pub const PILE_REST: u64 = 12;
/// How many sheets at most are drawn behind the pile's top.
pub const PILE_SHEETS: u64 = 3;

/// Off your mind laid out as one pile.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OffPile {
    /// The running cards on top, the first ones in the app's order.
    pub top: u64,
    /// The running cards on the pile, after the top ones.
    pub more_running: u64,
    /// The put-off cards on the pile (all of them).
    pub put_off: u64,
    /// The done, withdrawn and shredded cards on the pile (all of them).
    pub closed: u64,
    /// Whether there is a pile at all (something beyond the top).
    pub pile: bool,
    /// The sheets drawn behind the pile's top while it is closed.
    pub behind: u64,
    /// How many of the put-off and done cards the opened pile shows (the put-off ones first).
    pub rest_shown: u64,
    /// Whether it starts opened (the page Off your mind).
    pub open: bool,
}

/// The pile for `running` cards being worked on, `put_off` put off and `closed` done or thrown away (real totals).
/// `full`: the page Off your mind, where every card shows and the pile stands open.
pub fn off_pile(running: u64, put_off: u64, closed: u64, full: bool) -> OffPile {
    let top = running.min(PILE_TOP);
    let more_running = running - top;
    let rest = put_off.saturating_add(closed);
    let pile = more_running > 0 || rest > 0;
    OffPile {
        top,
        more_running,
        put_off,
        closed,
        pile,
        behind: if pile { (more_running + u64::from(rest > 0)).clamp(1, PILE_SHEETS) } else { 0 },
        rest_shown: if full { rest } else { rest.min(PILE_REST) },
        open: full,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_running_on_top_the_rest_on_the_pile() {
        let p = off_pile(5, 1, 40, false);
        assert_eq!((p.top, p.more_running, p.put_off, p.closed), (3, 2, 1, 40));
        assert!(p.pile && !p.open);
        assert_eq!(p.behind, 3, "two more running and the done ones: three sheets at most");
        assert_eq!(p.rest_shown, PILE_REST, "the Desk shows twelve of the forty-one");
    }

    #[test]
    fn the_counts_are_the_real_totals() {
        // (the Desk draws only a few closed rows; the pile still counts every one)
        let p = off_pile(1, 0, 2_345, false);
        assert_eq!(p.closed, 2_345);
        assert_eq!(p.rest_shown, PILE_REST);
        assert_eq!(off_pile(1, 0, 2_345, true).rest_shown, 2_345, "the page shows them all");
    }

    #[test]
    fn up_to_three_running_and_nothing_else_is_no_pile() {
        for n in 0..=3 {
            let p = off_pile(n, 0, 0, false);
            assert_eq!(p.top, n);
            assert!(!p.pile);
            assert_eq!(p.behind, 0);
        }
    }

    #[test]
    fn nothing_running_only_the_pile() {
        let p = off_pile(0, 2, 3, false);
        assert_eq!((p.top, p.more_running), (0, 0));
        assert!(p.pile);
        assert_eq!(p.behind, 1);
        assert_eq!(p.rest_shown, 5);
    }

    #[test]
    fn the_page_stands_open() {
        let p = off_pile(7, 3, 9, true);
        assert!(p.open);
        assert_eq!((p.top, p.more_running, p.rest_shown), (3, 4, 12));
    }
}
