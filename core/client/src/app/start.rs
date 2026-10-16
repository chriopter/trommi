//! The start's line: while the app starts (`Phase::Starting`) and opens its room (`Phase::Opening`), `ViewModel.status`
//! says which step it is at, so a shell shows what happens instead of a bare "Opening".
//!
//! The steps, in order (`args.step` of `args.steps`, so a shell can draw how far along it is without knowing them):
//! 1. `start.device` "Unlocking this device": the stored device is read and opened.
//! 2. `start.desk` "Loading your Desk": the board this device kept is read back.
//! 3. `start.catching_up` "Catching up: 120 of 480": the hub's order is read up to its end (`args.done`, `args.total`
//!    once the hub named its head; plain "Catching up" before).
//! 4. `start.almost` "Almost there": the order is read, the live stream is being opened.
//!
//! Once open and live, or signed out, the line is gone. Offline and stopped have a line of their own, so no shell
//! shows the board it last had as if it were now (`phase.offline`, `phase.stopped`). A line set elsewhere (a stop's
//! own reason, `account.wipe_failed`) is left.

use super::App;
use crate::views::words::say;
use crate::views::{Phase, Text};

const STEPS: &str = "4";

fn step(n: &str, key: &str, en: String, more: &[(&str, &str)]) -> Text {
    let mut args = vec![("step", n), ("steps", STEPS)];
    args.extend_from_slice(more);
    say(key, en, &args)
}

/// A number with thousands grouped, as the Desk writes counts ("12,400").
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The line for where the start stands: the phase, whether the kept board is still being read, and the first
/// pass over the hub's order (none: not begun; `Some(None)`: begun, no head named yet; then done of total).
pub fn line(phase: Phase, board_read: bool, pass: Option<Option<(u64, u64)>>, caught_up: bool) -> Option<Text> {
    match phase {
        Phase::Starting => Some(step("1", "start.device", "Unlocking this device".into(), &[])),
        Phase::Opening if !board_read => Some(step("2", "start.desk", "Loading your Desk".into(), &[])),
        Phase::Opening if caught_up => Some(step("4", "start.almost", "Almost there".into(), &[])),
        Phase::Opening => match pass.flatten() {
            Some((done, total)) => {
                let (d, t) = (done.to_string(), total.to_string());
                Some(step("3", "start.catching_up", format!("Catching up: {} of {}", grouped(done), grouped(total)), &[("done", &d), ("total", &t)]))
            }
            None => Some(step("3", "start.catching_up", "Catching up".into(), &[])),
        },
        Phase::Offline => Some(say("phase.offline", "Offline: what this device last had", &[])),
        Phase::Stopped => Some(say("phase.stopped", "Stopped: close Trommi and open it again", &[])),
        _ => None,
    }
}

impl App {
    /// Sets the start's line for this render (see the module).
    pub(super) fn start_status(&mut self) {
        let ours = self.next_view.status.as_ref().is_none_or(|t| t.key.starts_with("start.") || t.key.starts_with("phase."));
        if !ours {
            return;
        }
        let board_read = self.cache.as_ref().is_none_or(|cache| cache.ready());
        let engine = self.engine.as_ref();
        let line = match engine {
            None if self.next_view.phase == Phase::Opening => line(Phase::Starting, true, None, false),
            _ => line(self.next_view.phase, board_read, engine.map(|e| e.catch_up()), engine.is_some_and(|e| e.caught_up())),
        };
        self.next_view.status = line;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn en(t: Option<Text>) -> String {
        t.map(|t| t.en).unwrap_or_default()
    }

    #[test]
    fn each_step_says_what_happens_and_how_far_along_it_is() {
        let device = line(Phase::Starting, false, None, false).unwrap();
        assert_eq!((device.key.as_str(), device.en.as_str()), ("start.device", "Unlocking this device"));
        assert_eq!((device.args["step"].as_str(), device.args["steps"].as_str()), ("1", "4"));
        assert_eq!(en(line(Phase::Opening, false, None, false)), "Loading your Desk");
        assert_eq!(en(line(Phase::Opening, true, None, false)), "Catching up");
        assert_eq!(en(line(Phase::Opening, true, Some(None), false)), "Catching up");
        let pass = line(Phase::Opening, true, Some(Some((12_400, 27_000))), false).unwrap();
        assert_eq!(pass.en, "Catching up: 12,400 of 27,000");
        assert_eq!((pass.args["done"].as_str(), pass.args["total"].as_str(), pass.args["step"].as_str()), ("12400", "27000", "3"));
        let almost = line(Phase::Opening, true, None, true).unwrap();
        assert_eq!((almost.en.as_str(), almost.args["step"].as_str()), ("Almost there", "4"));
    }

    #[test]
    fn an_open_app_has_no_start_line() {
        for phase in [Phase::Live, Phase::SignedOut] {
            assert_eq!(line(phase, true, Some(Some((1, 2))), false), None, "{phase:?}");
        }
    }

    #[test]
    fn offline_and_stopped_say_so() {
        assert_eq!(line(Phase::Offline, true, None, true).map(|t| t.key), Some("phase.offline".into()));
        assert_eq!(line(Phase::Stopped, true, None, true).map(|t| t.key), Some("phase.stopped".into()));
    }
}
