//! The words of the views: every sentence that depends on state, said once, as the apps say it today (the web's
//! `app.mjs` `linkOf`, `blockedOf`, `quietOf` and `ui.mjs` `badge`, `linkCap`). A shell shows `Text.en`; a
//! translation later looks the key up and fills the same arguments.

use super::{FixView, HandView, LinkWords, Text};
use crate::status::{Blocked, LinkView, ASLEEP_MS};

/// A text with its arguments. `en` is written out by the caller: the sentence as it reads with these arguments.
pub fn say(key: &str, en: impl Into<String>, args: &[(&str, &str)]) -> Text {
    Text {
        key: key.into(),
        args: args.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        en: en.into(),
    }
}

/// Whole minutes of a span, at least one (JavaScript's `Math.max(1, Math.round(ms / 60000))`).
pub fn minutes(ms: u64) -> u64 {
    ((ms + 30_000) / 60_000).max(1)
}

/// When a count of minutes since `since` reads one more than it does at `now`.
pub fn minutes_tick(since: u64, now: u64) -> u64 {
    since + minutes(now.saturating_sub(since)) * 60_000 + 30_000
}

/// "1 question", "3 questions".
pub fn questions(n: u32) -> String {
    if n == 1 {
        "1 question".into()
    } else {
        format!("{n} questions")
    }
}

/// The Desk's one big line, picked once per run of the app; a calm one when nothing waits.
pub const GREETINGS: [&str; 20] = [
    "Welcome back.", "There you are.", "The agents missed you.", "Desk’s all yours.", "Ring the bell.", "Decisions, decisions.",
    "Your call.", "Back at it.", "Somebody knocked.", "Look who’s here.", "They’ve been waiting.", "The floor is yours.",
    "Pick a card.", "Over to you.", "Right on time.", "Pull up a chair.", "Yes or no?", "The boss is in.", "What’ll it be?", "Ready when you are.",
];
pub const CALM: [&str; 6] = ["All quiet.", "Nothing needs you.", "Clear desk.", "Carry on.", "As you were.", "Go outside."];

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// A moment in the person's zone: day number since 1970, year, month (1..), day, hour, minute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub days: i64,
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// `ms` (since 1970, UTC) as the wall clock `utc_offset_min` east of UTC shows it.
pub fn local(ms: u64, utc_offset_min: i32) -> Local {
    let t = ms as i64 + utc_offset_min as i64 * 60_000;
    let days = t.div_euclid(86_400_000);
    let of_day = t.rem_euclid(86_400_000);
    // (days to a civil date: Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    Local {
        days,
        year,
        month,
        day,
        hour: (of_day / 3_600_000) as u32,
        minute: (of_day / 60_000 % 60) as u32,
    }
}

/// The first moment of the day after `now` in the person's zone: when "14:05" turns into "3 Oct 14:05".
pub fn next_midnight(now: u64, utc_offset_min: i32) -> u64 {
    let off = utc_offset_min as i64 * 60_000;
    let t = now as i64 + off;
    ((t.div_euclid(86_400_000) + 1) * 86_400_000 - off).max(0) as u64
}

/// "14:05" for a moment of today, else "3 Oct 14:05" (the web's `clock`).
pub fn clock(t: u64, now: u64, utc_offset_min: i32) -> String {
    let (a, n) = (local(t, utc_offset_min), local(now, utc_offset_min));
    let time = format!("{:02}:{:02}", a.hour, a.minute);
    if a.days == n.days {
        time
    } else {
        format!("{} {} {time}", a.day, MONTHS[(a.month - 1) as usize])
    }
}

/// Whom a link's words are about.
pub struct LinkSubject<'a> {
    pub name: &'a str,
    pub online: bool,
    /// Its device says platform "codex": it is restarted, never reconnected in /mcp, and is never asleep.
    pub codex: bool,
    /// Its connector said a last word before its stream closed (`link.exit`).
    pub said_goodbye: bool,
}

const RECONNECT: &str = "/mcp → trommi → Reconnect";

/// A session's link in words, at `now`: the cap under its name, the tooltip, the sentence, and the way out.
/// With the moment the words change by the clock alone (a count of minutes; a time of day that gets its date).
pub fn link_words(who: &LinkSubject, v: &LinkView, now: u64, zone: i32) -> (LinkWords, Option<u64>) {
    let n = who.name;
    let prog = if who.codex { "Codex" } else { "Claude Code" };
    let reconnect = if who.codex { "quit Codex, then: codex resume" } else { RECONNECT };
    let at = v.since.map(|s| clock(s, now, zone)).unwrap_or_default();
    let dated = v.since.map(|_| next_midnight(now, zone));
    let dot_at = if at.is_empty() { String::new() } else { format!(" · {at}") };
    let since_at = if at.is_empty() { String::new() } else { format!(" since {at}") };
    let words = |state: &str, word: Option<Text>, short: Option<Text>, line: Text, fix: Option<FixView>| LinkWords {
        state: state.into(),
        since: v.since,
        word,
        short,
        line,
        fix,
    };
    let cut_caps = || {
        (
            Some(say("link.cut.word", format!("cut off{dot_at}"), &[("at", &at)])),
            Some(if at.is_empty() {
                say("link.cut.short", "Cut off", &[])
            } else {
                say("link.cut.short_since", format!("Cut off since {at}"), &[("at", &at)])
            }),
        )
    };
    let in_terminal = |code: &str| Some(FixView {
        say: say("link.fix.terminal", "In its terminal:", &[]),
        code: code.into(),
    });
    match v.state.as_str() {
        "cut" if !who.online => {
            let (word, short) = cut_caps();
            let line = say(
                "link.cut.line",
                format!("{n} is cut off{since_at}: its {prog} runs, its Trommi tools are gone. It cannot hear you and cannot write to you."),
                &[("name", n), ("at", &at), ("program", prog)],
            );
            (words("cut", word, short, line, in_terminal(reconnect)), dated)
        }
        "gone" => {
            let how = if who.said_goodbye {
                format!("its {prog} session ended.")
            } else {
                "it stopped without a word: its session ended, was killed, or its machine is off the network.".to_string()
            };
            let key = if who.said_goodbye { "link.gone.line_ended" } else { "link.gone.line_silent" };
            let line = say(key, format!("{n} is gone{since_at}: {how}"), &[("name", n), ("at", &at), ("program", prog)]);
            let short = if at.is_empty() {
                say("link.gone.short", "Gone", &[])
            } else {
                say("link.gone.short_since", format!("Gone since {at}"), &[("at", &at)])
            };
            let fix = FixView {
                say: say("link.fix.start_again", "Start it again in its folder:", &[]),
                code: if who.codex { "codex resume" } else { "claude --continue" }.into(),
            };
            (
                words("gone", Some(say("link.gone.word", format!("gone{dot_at}"), &[("at", &at)])), Some(short), line, Some(fix)),
                dated,
            )
        }
        "cut" => {
            let (word, short) = cut_caps();
            let line = say(
                "link.cut.line_folder",
                format!("A {prog} session in {n}'s folder is cut off{since_at}: it runs, its Trommi tools are gone. It cannot hear you and cannot write to you."),
                &[("name", n), ("at", &at), ("program", prog)],
            );
            (words("cut", word, short, line, in_terminal(reconnect)), dated)
        }
        "live" => (
            LinkWords {
                since: None,
                ..words("live", None, None, say("link.live.line", format!("{n} hears you at once."), &[("name", n)]), None)
            },
            None,
        ),
        state => {
            let idle = v.since.map(|s| now.saturating_sub(s));
            let min = idle.map(minutes);
            let min_s = min.map(|m| m.to_string()).unwrap_or_default();
            let tick = v.since.map(|s| minutes_tick(s, now));
            let ago = min.map(|m| format!("; its last one was {m} min ago")).unwrap_or_default();
            // Codex between turns is there, not asleep: it reads what you write when its next turn starts
            if who.codex {
                let fix = idle.filter(|i| *i >= ASLEEP_MS).map(|_| FixView {
                    say: say(
                        "link.fix.codex_turn",
                        "To start a turn now, type anything in its Codex terminal. Trommi wakes Codex by itself once the connector is current:",
                        &[],
                    ),
                    code: "trommi-connector update".into(),
                });
                // (the moment the hint to start a turn appears)
                let hint = v.since.map(|s| s + ASLEEP_MS).filter(|t| *t > now);
                let line = say(
                    "link.oncall.line_codex",
                    format!("{n} runs in Codex: it reads what you write when its next turn starts{ago}."),
                    &[("name", n), ("min", &min_s)],
                );
                return (
                    words(
                        "oncall",
                        Some(say("link.oncall.word_codex", "on its next turn", &[])),
                        Some(say("link.oncall.short_codex", "Reads you on its next turn", &[])),
                        line,
                        fix,
                    ),
                    earliest(tick, hint),
                );
            }
            if state == "asleep" {
                let line = say(
                    "link.asleep.line",
                    format!("{n} is not listening: it hears you only on its next step, and its last one was {min_s} min ago."),
                    &[("name", n), ("min", &min_s)],
                );
                let fix = FixView {
                    say: say("link.fix.wake", "Wake it: type anything in its terminal.", &[]),
                    code: String::new(),
                };
                return (
                    words(
                        "asleep",
                        Some(say("link.asleep.word", format!("not listening · {min_s} min"), &[("min", &min_s)])),
                        Some(say("link.asleep.short", format!("Not listening for {min_s} min"), &[("min", &min_s)])),
                        line,
                        Some(fix),
                    ),
                    tick,
                );
            }
            let line = say("link.oncall.line", format!("{n} hears you on its next step{ago}."), &[("name", n), ("min", &min_s)]);
            (
                words(
                    "oncall",
                    Some(say("link.oncall.word", "on its next step", &[])),
                    Some(say("link.oncall.short", "Hears you on its next step", &[])),
                    line,
                    None,
                ),
                tick,
            )
        }
    }
}

pub fn earliest(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// The red hand in words. `cut_short`: the link's own short words ("Cut off since 14:05") for a session that is
/// cut off. With the moment the words change by the clock alone.
pub fn hand_words(b: &Blocked, cut_short: Option<&Text>, now: u64, zone: i32) -> (HandView, Option<u64>) {
    let hand = |text: Text, since: Option<u64>| HandView {
        why: b.why.clone(),
        text,
        since,
    };
    match b.why.as_str() {
        "cut" => (
            hand(cut_short.cloned().unwrap_or_else(|| say("link.cut.short", "Cut off", &[])), b.since),
            b.since.map(|_| next_midnight(now, zone)),
        ),
        "offline" => match b.since {
            Some(s) => {
                let at = clock(s, now, zone);
                (
                    hand(say("hand.offline.since", format!("Connection lost since {at}"), &[("at", &at)]), b.since),
                    Some(next_midnight(now, zone)),
                )
            }
            None => (hand(say("hand.offline", "Disconnected while working", &[]), None), None),
        },
        "permission" => (hand(say("hand.permission", "Waiting for permission", &[]), None), None),
        _ => (hand(say("hand.blocking", "Waiting for you: a blocking question", &[]), None), None),
    }
}

/// The quiet hint: "quiet for 24 min". With the moment it reads one minute more.
pub fn quiet_words(quiet_since: u64, now: u64) -> (Text, u64) {
    let n = minutes(now.saturating_sub(quiet_since)).to_string();
    (say("session.quiet", format!("quiet for {n} min"), &[("min", &n)]), minutes_tick(quiet_since, now))
}

/// What a row's badge says to a screen reader: stopped, working, disconnected, and how many questions are open.
pub fn badge_words(hand: Option<&HandView>, open: u32, online: bool, working: bool) -> Text {
    let q = questions(open);
    let n = open.to_string();
    match hand {
        Some(h) if open > 0 => say(
            "badge.stopped_open",
            format!("Stopped: {}, {q} open", h.text.en),
            &[("why", &h.text.en), ("n", &n)],
        ),
        Some(h) => say("badge.stopped", format!("Stopped: {}", h.text.en), &[("why", &h.text.en)]),
        None if online && working => say("badge.working_open", format!("Working, {q} open"), &[("n", &n)]),
        None if online => say("badge.open", format!("{q} open"), &[("n", &n)]),
        None => say("badge.offline_open", format!("Disconnected, {q} open"), &[("n", &n)]),
    }
}

/// The cap of a session that hears at once and has not picked answers up: "1 answer waits", "3 answers wait".
pub fn unheard_words(n: u32) -> (Text, Text) {
    let s = n.to_string();
    if n == 1 {
        (say("unheard.word_one", "1 answer waits", &[("n", "1")]), say("unheard.short_one", "Not heard yet", &[]))
    } else {
        (
            say("unheard.word", format!("{n} answers wait"), &[("n", &s)]),
            say("unheard.short", format!("{n} not heard"), &[("n", &s)]),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;
    /// 13 October 2026, 14:05 UTC.
    const NOW: u64 = 1_791_900_300_000;

    fn view(state: &str, since: Option<u64>) -> LinkView {
        LinkView {
            state: state.into(),
            since,
            reason: String::new(),
        }
    }
    fn who(online: bool, codex: bool, said_goodbye: bool) -> LinkSubject<'static> {
        LinkSubject {
            name: "widgets",
            online,
            codex,
            said_goodbye,
        }
    }
    fn en(t: &Option<Text>) -> &str {
        t.as_ref().map(|t| t.en.as_str()).unwrap_or("")
    }

    #[test]
    fn the_clock_is_the_persons_wall_clock() {
        assert_eq!(local(NOW, 0), Local { days: 20_739, year: 2026, month: 10, day: 13, hour: 14, minute: 5 });
        assert_eq!(clock(NOW - 38 * MIN, NOW, 0), "13:27");
        assert_eq!(clock(NOW - 38 * MIN, NOW, 120), "15:27");
        // (yesterday, there: the date stands before the time; a day of one digit has no zero)
        assert_eq!(clock(NOW - 20 * 60 * MIN, NOW, 0), "12 Oct 18:05");
        assert_eq!(clock(NOW - 10 * 24 * 60 * MIN, NOW, 0), "3 Oct 14:05");
        // (late in the evening in Berlin is the next day there already)
        assert_eq!(clock(NOW + 9 * 60 * MIN, NOW + 10 * 60 * MIN, 120), "01:05");
        assert_eq!(clock(NOW, NOW + 10 * 60 * MIN, 120), "13 Oct 16:05");
        assert_eq!(local(951_782_400_000, 0).day, 29, "29 February 2000");
        assert_eq!(next_midnight(NOW, 0), NOW + (9 * 60 + 55) * MIN);
        assert_eq!(next_midnight(NOW, 120), NOW + (7 * 60 + 55) * MIN);
        assert_eq!(next_midnight(NOW, -300), NOW + (14 * 60 + 55) * MIN);
    }

    #[test]
    fn minutes_round_and_are_never_zero() {
        assert_eq!(minutes(0), 1);
        assert_eq!(minutes(89_999), 1);
        assert_eq!(minutes(90_000), 2);
        assert_eq!(minutes(46 * MIN), 46);
        // (the count reads one more half a minute past the whole one)
        assert_eq!(minutes_tick(1_000, 1_000 + 10 * MIN), 1_000 + 10 * MIN + 30_000);
        assert_eq!(minutes(minutes_tick(1_000, 1_000 + 10 * MIN) - 1_000), 11);
        assert_eq!(minutes(minutes_tick(1_000, 1_000 + 10 * MIN) - 1 - 1_000), 10);
        assert_eq!(minutes_tick(1_000, 1_000), 1_000 + 90_000);
    }

    /// The web's `linkOf`, state by state (app/web/public/app.mjs).
    #[test]
    fn a_link_in_words_as_the_web_says_it() {
        let since = NOW - 38 * MIN;
        let (l, wake) = link_words(&who(false, false, true), &view("cut", Some(since)), NOW, 0);
        assert_eq!((l.state.as_str(), en(&l.word), en(&l.short)), ("cut", "cut off · 13:27", "Cut off since 13:27"));
        assert_eq!(l.line.en, "widgets is cut off since 13:27: its Claude Code runs, its Trommi tools are gone. It cannot hear you and cannot write to you.");
        assert_eq!(l.fix.as_ref().map(|f| (f.say.en.as_str(), f.code.as_str())), Some(("In its terminal:", "/mcp → trommi → Reconnect")));
        assert_eq!(wake, Some(next_midnight(NOW, 0)));

        // (connected, and a session of its folder lost its connector)
        let (l, _) = link_words(&who(true, false, false), &view("cut", None), NOW, 0);
        assert_eq!((en(&l.word), en(&l.short)), ("cut off", "Cut off"));
        assert_eq!(l.line.en, "A Claude Code session in widgets's folder is cut off: it runs, its Trommi tools are gone. It cannot hear you and cannot write to you.");

        let (l, _) = link_words(&who(false, true, false), &view("cut", None), NOW, 0);
        assert_eq!(l.fix.unwrap().code, "quit Codex, then: codex resume");

        let (l, _) = link_words(&who(false, false, true), &view("gone", Some(since)), NOW, 0);
        assert_eq!((en(&l.word), en(&l.short)), ("gone · 13:27", "Gone since 13:27"));
        assert_eq!(l.line.en, "widgets is gone since 13:27: its Claude Code session ended.");
        assert_eq!(l.fix.as_ref().map(|f| (f.say.en.as_str(), f.code.as_str())), Some(("Start it again in its folder:", "claude --continue")));
        let (l, wake) = link_words(&who(false, true, false), &view("gone", None), NOW, 0);
        assert_eq!((en(&l.word), en(&l.short)), ("gone", "Gone"));
        assert_eq!(l.line.en, "widgets is gone: it stopped without a word: its session ended, was killed, or its machine is off the network.");
        assert_eq!(l.fix.unwrap().code, "codex resume");
        assert_eq!(wake, None);

        let (l, wake) = link_words(&who(true, false, false), &view("live", None), NOW, 0);
        assert_eq!((l.word.is_none(), l.short.is_none(), l.fix.is_none(), wake), (true, true, true, None));
        assert_eq!(l.line.en, "widgets hears you at once.");

        let last = NOW - 3 * MIN;
        let (l, wake) = link_words(&who(true, false, false), &view("oncall", Some(last)), NOW, 0);
        assert_eq!((en(&l.word), en(&l.short)), ("on its next step", "Hears you on its next step"));
        assert_eq!(l.line.en, "widgets hears you on its next step; its last one was 3 min ago.");
        assert_eq!((l.fix.is_none(), wake), (true, Some(last + 3 * MIN + 30_000)));
        let (l, wake) = link_words(&who(true, false, false), &view("oncall", None), NOW, 0);
        assert_eq!((l.line.en.as_str(), wake), ("widgets hears you on its next step.", None));

        let last = NOW - 46 * MIN;
        let (l, wake) = link_words(&who(true, false, false), &view("asleep", Some(last)), NOW, 0);
        assert_eq!((en(&l.word), en(&l.short)), ("not listening · 46 min", "Not listening for 46 min"));
        assert_eq!(l.line.en, "widgets is not listening: it hears you only on its next step, and its last one was 46 min ago.");
        assert_eq!(l.fix.as_ref().map(|f| (f.say.en.as_str(), f.code.as_str())), Some(("Wake it: type anything in its terminal.", "")));
        assert_eq!(wake, Some(last + 46 * MIN + 30_000));

        // (Codex is never asleep; after ten idle minutes it gets the line that starts a turn)
        let (l, wake) = link_words(&who(true, true, false), &view("oncall", Some(NOW - 3 * MIN)), NOW, 0);
        assert_eq!((en(&l.word), en(&l.short)), ("on its next turn", "Reads you on its next turn"));
        assert_eq!(l.line.en, "widgets runs in Codex: it reads what you write when its next turn starts; its last one was 3 min ago.");
        assert_eq!((l.fix.is_none(), wake), (true, Some(NOW + 30_000)));
        let (l, _) = link_words(&who(true, true, false), &view("oncall", Some(last)), NOW, 0);
        assert_eq!(l.fix.unwrap().code, "trommi-connector update");
    }

    /// The web's `blockedOf` and `quietOf`.
    #[test]
    fn the_red_hand_and_the_quiet_hint_in_words() {
        let b = |why: &str, since| Blocked { why: why.into(), since };
        let short = say("link.cut.short_since", "Cut off since 13:27", &[("at", "13:27")]);
        assert_eq!(hand_words(&b("cut", Some(1)), Some(&short), NOW, 0).0.text.en, "Cut off since 13:27");
        assert_eq!(hand_words(&b("cut", None), None, NOW, 0).0.text.en, "Cut off");
        let (h, wake) = hand_words(&b("offline", Some(NOW - 38 * MIN)), None, NOW, 0);
        assert_eq!((h.text.en.as_str(), h.since, wake), ("Connection lost since 13:27", Some(NOW - 38 * MIN), Some(next_midnight(NOW, 0))));
        assert_eq!(hand_words(&b("offline", None), None, NOW, 0).0.text.en, "Disconnected while working");
        assert_eq!(hand_words(&b("permission", None), None, NOW, 0).0.text.en, "Waiting for permission");
        assert_eq!(hand_words(&b("blocking", None), None, NOW, 0).0.text.en, "Waiting for you: a blocking question");
        let (q, wake) = quiet_words(NOW - 24 * MIN, NOW);
        assert_eq!((q.en.as_str(), q.args["min"].as_str(), wake), ("quiet for 24 min", "24", NOW + 30_000));
    }

    /// The web's `badge` and `linkCap` (app/web/public/ui.mjs).
    #[test]
    fn a_badge_and_what_waits_unheard_in_words() {
        let (hand, _) = hand_words(&Blocked { why: "permission".into(), since: None }, None, NOW, 0);
        assert_eq!(badge_words(Some(&hand), 2, true, true).en, "Stopped: Waiting for permission, 2 questions open");
        assert_eq!(badge_words(Some(&hand), 0, true, true).en, "Stopped: Waiting for permission");
        assert_eq!(badge_words(None, 1, true, true).en, "Working, 1 question open");
        assert_eq!(badge_words(None, 3, true, false).en, "3 questions open");
        assert_eq!(badge_words(None, 3, false, true).en, "Disconnected, 3 questions open");
        assert_eq!(unheard_words(1).0.en, "1 answer waits");
        assert_eq!(unheard_words(1).1.en, "Not heard yet");
        assert_eq!(unheard_words(4).0.en, "4 answers wait");
        assert_eq!(unheard_words(4).1.en, "4 not heard");
    }
}
