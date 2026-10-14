//! What goes into [`App::handle`](super::App::handle): the person's actions, I/O results, the clock, the platform.

use crate::actions::Action;
use crate::app::bytes::Bytes;
use crate::app::io::{HttpResponse, IoId, IoResult, PlatformAnswer, StoreAnswer, StreamItem};
use crate::views::Focus;
use serde::{Deserialize, Serialize};

/// What the shell knows at start and the core cannot find out itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Which shell this is: "web", "ios", "mac", "linux", "test". Only for logs and the hub's client header.
    pub platform: String,
    /// The shell's version, for the same.
    pub version: String,
    /// The hub's address ("https://hub.trommi.com"); empty: the one the stored room names, or the default.
    #[serde(default)]
    pub hub: String,
    /// The shell's name for the device state this App owns ("default", or a room's id): the device space of
    /// every `Store` effect of this App is that slot's, and the shell holds that slot's lock (one owner).
    #[serde(default)]
    pub slot: String,
    /// Seeds the core's jitter (retries, backoff): a random number below 2^53 in a shipped app (every number in
    /// the contract stays below 2^53, so JavaScript holds it exactly), fixed in a test. The protocol's own
    /// randomness is the system's and never comes from here.
    pub seed: u64,
    /// Minutes east of UTC now (120 in Berlin in summer). The core's local-time rules ("until tomorrow 7:00")
    /// use it; `Event::Zone` brings a change.
    #[serde(default)]
    pub utc_offset_min: i32,
    /// The person's locale ("de-DE"); texts still come in English (`Text.en`), this picks dates and numbers.
    #[serde(default)]
    pub locale: String,
    /// What this platform can do: decides which effects the core uses.
    #[serde(default)]
    pub can: Abilities,
    /// The demo room (`demo/data/fixture.json`): the App runs it with no hub and no store; what the person does
    /// shows and stays (until the App ends).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub demo: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Abilities {
    /// The shell shows local notifications ([`Effect::Notify`](super::Effect::Notify)).
    #[serde(default)]
    pub notify: bool,
    /// The shell keeps secrets in a platform store ([`Effect::Secret`](super::Effect::Secret)).
    #[serde(default)]
    pub secrets: bool,
    /// The shell shows widgets or Live Activities ([`Effect::Snapshot`](super::Effect::Snapshot)).
    #[serde(default)]
    pub snapshots: bool,
    /// The shell has passkeys with the PRF extension ([`PlatformOp::PasskeyCreate`](super::io::PlatformOp)).
    #[serde(default)]
    pub passkeys: bool,
    /// The shell can register for push: "apns", "web_push", or empty.
    #[serde(default)]
    pub push: String,
}

/// One input. `now_ms` travels beside it ([`App::handle`](super::App::handle)), never inside.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// The first event, once per App. The shell holds the device lock (one owner) before it sends it.
    Start {
        config: Config,
    },
    /// The person did something.
    Action {
        action: Action,
    },
    /// What the person looks at changed (screen, open session or card, window in front or not). Drives heard and
    /// unheard, which timelines load, and how eagerly the core syncs.
    Focus {
        focus: Focus,
    },
    Http {
        id: IoId,
        result: IoResult<HttpResponse>,
    },
    Stream {
        id: IoId,
        item: StreamItem,
    },
    Store {
        id: IoId,
        result: IoResult<StoreAnswer>,
    },
    Secret {
        id: IoId,
        result: IoResult<Option<Bytes>>,
    },
    /// The answer to an `Effect::Platform`.
    Platform {
        id: IoId,
        result: IoResult<PlatformAnswer>,
    },
    /// How far a request's body went up, or its answer came down (optional; for a progress bar).
    Progress {
        id: IoId,
        done: u64,
        total: Option<u64>,
    },
    /// A transfer that outlived an earlier App (a background upload on iOS) ended: `key` is the request's.
    /// Sent after `Start`; the core decides whether it still matters.
    Finished {
        key: String,
        result: IoResult<HttpResponse>,
    },
    /// The timer of the last [`Effect::Timer`](super::Effect::Timer) went off (or later: a shell may be late).
    Wake,
    /// The app came to the front, or the platform woke it (BGAppRefresh, a push handled in the app).
    Foreground,
    /// The app went to the back: the core closes the stream and writes its cache out.
    Background,
    /// The platform's view of the network changed. Only a hint: the core trusts its own I/O results more.
    Net {
        online: bool,
    },
    /// The platform gave a new push token or subscription, or took it away (`token: None`): the core registers
    /// it with the hub. `kind`: "apns", "apns_live_activity", "web_push" (the subscription's JSON as bytes).
    PushToken {
        kind: String,
        token: Option<Bytes>,
    },
    /// The person's time zone changed (travel, summer time): minutes east of UTC from now on.
    Zone {
        utc_offset_min: i32,
    },
    /// A push reached the app (sealed, as it came). The core opens it, catches up, and may answer with a Notify.
    Push {
        payload: Bytes,
    },
}
