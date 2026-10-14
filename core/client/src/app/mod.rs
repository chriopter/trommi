//! The App: one object per room on a device, and the whole of the app's logic. Sans-I/O: [`App::handle`] takes one
//! [`Event`] and the shell's clock and answers with [`Effect`]s. It does no I/O, reads no clock, starts no thread
//! and holds no callback. Every shell (web worker, iOS, Linux, a test) drives it the same way:
//!
//! ```text
//! shell                                 App
//!   Event::Start{config}         ──▶   handle(now, event) ──▶ [Render, Store{DeviceLoad}, …]
//!   runs the effects, in order
//!   Event::Store{id, result}     ──▶   handle(now, event) ──▶ [Render, Http{…}, OpenStream{…}, Timer{…}]
//!   Event::Http / Stream / Wake  ──▶   …
//!   Event::Action{action}        ──▶   …  (the person did something)
//! ```
//!
//! core/client/README.md, "The app core", is the contract in words: the order of effects, errors and retries,
//! threads per platform, the stored format, and what a shell may and may not do.
//!
//! This is W0's skeleton: it starts, reads the device state, and shows `SignedOut` or `Opening`. W1 (engine), W3
//! (actions, account) and W4 (views) fill it in behind this same surface.

pub mod bytes;
pub mod edge;
pub mod effect;
pub mod event;
pub mod io;
mod invites;
mod account_steps;
mod selfjoin;
pub mod script;
pub mod sim;

pub use bytes::Bytes;
pub use effect::{Effect, LogLevel, Notice, WidgetSnapshot};
pub use event::{Abilities, Config, Event};
pub use io::{
    DeviceEntry, Entry, Header, HttpClass, HttpRequest, HttpResponse, IoError, IoErrorKind, IoId,
    IoResult, PlatformAnswer, PlatformOp, SecretOp, StoreAnswer, StoreOp, StreamItem,
};

use crate::actions::run::{Doing, NotDone};
use crate::actions::sent::{NoteNext, Sent};
use crate::actions::{self, Action};
use crate::engine::{self, Connection, Engine, Jitter, OpenError, Report};
use crate::model::change::Change;
use crate::views::{Lens, Phase, Screen, ViewModel};
use std::collections::BTreeMap;

/// The hub of a shell that names none.
pub const DEFAULT_HUB: &str = "https://hub.trommi.com";
/// At most so many of the engine's alerts are kept for the views.
const ALERTS_MAX: usize = 200;

/// A device that closed itself is opened again in place at most so many times in a row (the count starts anew once
/// it is live again, or the app comes to the front).
pub const REOPEN_TRIES: u32 = 3;
/// At most so many invites with a Request wait for the invite flow.
const INVITE_REQUESTS_MAX: usize = 64;

/// What a piece of I/O in flight was for.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pending {
    DeviceLoad,
    /// A cache entry deleted (a file the hub evicted): nothing waits for it.
    CacheDelete,
    /// The stored room's hub, read at the start when `Config.hub` names none ([`ROOM_KEY`]).
    RoomKey,
    /// The login of a sign-in (`account::signin`).
    Login,
    /// The stored room's hub written by a join with the code.
    RoomKeyWrite,
    /// The device state read for a join with the code ([`App::join_room`]).
    JoinLoad,
    /// The device state read for a founding (`AccountAction::Found`).
    FoundLoad,
    /// The cache space read to wipe it (a sign-out).
    Wipe,
    /// The device state read for a join by an invite link (`AccountAction::Join`).
    LinkLoad,
    /// I/O of an engine that was dropped (a join of the room that failed): its answer is only logged.
    Dropped,
}

/// The app store's key that names the room of this slot and its hub: written by a join with the code before the
/// device's first write, read at the start when `Config.hub` is empty.
pub const ROOM_KEY: &str = "room";

#[derive(serde::Serialize, serde::Deserialize)]
struct RoomKey {
    v: u32,
    hub: String,
    room: String,
}

pub struct App {
    config: Option<Config>,
    next_id: u64,
    pending: BTreeMap<IoId, Pending>,
    /// The view model as the core decides it now.
    next_view: ViewModel,
    /// The view model as the shell has it (what was rendered so far).
    view: ViewModel,
    rendered_once: bool,
    /// The App's own draws (the waits before a closed device is opened again); the engine draws its own.
    jitter: Jitter,
    /// The sync engine, once the stored device opened.
    engine: Option<Engine>,
    /// The timer the shell holds.
    timer: Option<u64>,
    /// What the engine reported for the person (until the views show them: W4).
    alerts: Vec<engine::Alert>,
    sessions_not_joined: Option<Vec<(trommi_core::ids::GroupId, &'static str)>>,
    /// How the last join of the room with the code stands; none before one.
    room_join: Option<engine::room_join::RoomJoinStatus>,
    /// A join of the room asked for, waiting for the device state it is made on.
    room_join_wait: Option<(IoId, engine::room_join::RoomJoin)>,
    /// A founding waiting for the device state, then for the hub (`account::found`); its kit's words are shown once
    /// the hub took it.
    founding: Option<(Option<IoId>, crate::account::found::Founding)>,
    /// A join by an invite link waiting for the slot's device state (the link holds the invite's secret).
    linking: Option<crate::account::Secret>,
    /// A sign-in waiting for its login answer.
    signing_in: Option<(IoId, crate::account::signin::Waiting)>,
    /// A sign-in's join of the room runs: its end is shown on the account's screen.
    signed_in_join: bool,
    /// The actions' requests answered since they were last taken.
    answers: Vec<engine::requests::Answered>,
    /// Files attached, opened, saved and shared (`crate::files`).
    files: crate::files::Files,
    /// Invites this device opened (`app::invites`).
    invites: invites::Invites,
    /// The account's steps in flight (`app::account_steps`).
    steps: account_steps::Steps,
    /// Agents' asks to let in a new session of their machine (`app::selfjoin`).
    selfjoin: selfjoin::SelfJoin,
    /// Actions that write the room's registers, held while the device catches up: a register written before the
    /// device read the newest value would lose to an older one read later (its write counter is too low).
    held_actions: Vec<Action>,
    /// The hub the stored room names ([`ROOM_KEY`]).
    stored_hub: Option<String>,
    /// The device state loaded, while the stored room's hub is read.
    opening: Option<(u64, Vec<DeviceEntry>)>,
    /// The board model, fed from the engine's reports.
    board: engine::feed::Board,
    /// What this device alone chooses: the desk and the pages in view, the zone (`Event::Focus`, `Event::Zone`).
    lens: Lens,
    /// The board's units are to be made again: the model, the lens or the clock moved.
    stale_views: bool,
    /// When the board's units change by the clock alone.
    view_wake: Option<u64>,
    /// The device's source of randomness when it is not the system's (a seeded test).
    entropy: Option<Box<dyn trommi_core::crypto::Entropy + Send>>,
    /// Invites of this room a newcomer sent its Request for, oldest first (the invite flow reads them: W3).
    invite_requests: Vec<trommi_core::ids::InviteId>,
    /// The device closed itself (a failed write, a failure of its own): when the stored state is read again, and
    /// how often it was in a row.
    reopen_at: Option<u64>,
    reopen_tries: u32,
    /// The actions in flight: their outbox entries and echoes (`actions::sent`).
    sent: Sent,
    /// The room's cache: read before the engine starts, written behind the board (`cache::keeper`).
    cache: Option<crate::cache::Keeper>,
    /// The model moved: the sessions' goals are compared with their desks' once live (`Doing::sync_goals`).
    goals_check: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        App {
            config: None,
            next_id: 1,
            pending: BTreeMap::new(),
            next_view: ViewModel::default(),
            view: ViewModel::default(),
            rendered_once: false,
            jitter: Jitter::new(0),
            engine: None,
            timer: None,
            alerts: Vec::new(),
            sessions_not_joined: None,
            room_join: None,
            room_join_wait: None,
            signing_in: None,
            founding: None,
            linking: None,
            signed_in_join: false,
            stored_hub: None,
            answers: Vec::new(),
            files: crate::files::Files::default(),
            invites: invites::Invites::default(),
            steps: account_steps::Steps::default(),
            selfjoin: selfjoin::SelfJoin::default(),
            held_actions: Vec::new(),
            opening: None,
            board: engine::feed::Board::default(),
            lens: Lens::default(),
            stale_views: false,
            view_wake: None,
            entropy: None,
            invite_requests: Vec::new(),
            reopen_at: None,
            reopen_tries: 0,
            sent: Sent::default(),
            cache: None,
            goals_check: false,
        }
    }

    /// An App whose device draws its randomness from `entropy` and not from the system: for a test that must
    /// repeat exactly. A shipped app uses [`App::new`].
    pub fn with_entropy(entropy: Box<dyn trommi_core::crypto::Entropy + Send>) -> Self {
        App {
            entropy: Some(entropy),
            ..Self::new()
        }
    }

    /// The board model as the engine's reports left it (the views are made from it: W4).
    pub fn board(&self) -> &engine::feed::Board {
        &self.board
    }

    /// The sync engine, once the stored device opened.
    pub fn engine(&self) -> Option<&Engine> {
        self.engine.as_ref()
    }

    /// An action on the device (seal, commit, answer) at the shell's clock: what it wrote is stored before anything
    /// it implies is sent, and the outbox is pumped. The actions (W3) go through this; `None` without a device.
    pub fn act<T>(
        &mut self,
        now_ms: u64,
        action: impl FnOnce(&mut engine::Device, u64) -> Result<T, trommi_core::Error>,
    ) -> (Option<Result<T, trommi_core::Error>>, Vec<Effect>) {
        let mut out = Vec::new();
        let result = self.with_engine(now_ms, &mut out, |engine, ctx| engine.act(ctx, action));
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        (result, out)
    }

    /// Joins the live session groups of a sign-in with the code (`engine::sync::joins`): read and posted together.
    /// The sign-in's flow calls this once the device is in the room; nothing happens without a device.
    pub fn join_sessions(&mut self, now_ms: u64, joins: engine::sync::joins::SessionJoins) -> Vec<Effect> {
        let mut out = Vec::new();
        self.with_engine(now_ms, &mut out, |engine, ctx| engine.join_sessions(ctx, joins));
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        out
    }

    /// Joins the room with the recovery code (`engine::room_join`), on a slot without a device in a room: the
    /// device is made (or a join posted earlier goes on), the room read and checked, the join posted, and then the
    /// session groups joined with the same keys ([`App::sessions_not_joined`]). How it stands: [`App::room_join`].
    /// The keys stay in the core and are dropped when the sessions are through.
    pub fn join_room(&mut self, now_ms: u64, join: engine::room_join::RoomJoin) -> Vec<Effect> {
        use engine::room_join::{RoomJoinError, RoomJoinStatus, RoomJoinStep};
        let mut out = Vec::new();
        if self.config.is_none() {
            out.push(log(LogLevel::Warn, "join_room: before start"));
            return out;
        }
        let canonical = trommi_core::hub_auth::HubAddress::parse(&join.hub).is_ok_and(|a| a.as_str() == join.hub);
        self.room_join = Some(if self.engine.is_some() {
            RoomJoinStatus::Failed(RoomJoinError::RoomExists)
        } else if !canonical {
            RoomJoinStatus::Failed(RoomJoinError::Refused("bad-hub"))
        } else {
            // the room's hub first: a device that exists is always found with its hub
            let key = RoomKey { v: 1, hub: join.hub.clone(), room: join.room.to_base64url() };
            let id = self.io(Pending::RoomKeyWrite);
            let value = serde_json::to_vec(&key).unwrap_or_default();
            out.push(Effect::Store { id, op: StoreOp::Put { put: vec![Entry { key: ROOM_KEY.into(), value: value.into() }], delete: Vec::new() } });
            let id = self.io(Pending::JoinLoad);
            out.push(Effect::Store { id, op: StoreOp::DeviceLoad });
            if let Some((old, _)) = self.room_join_wait.replace((id, join)) {
                self.pending.insert(old, Pending::Dropped);
            }
            RoomJoinStatus::Step(RoomJoinStep::Reading)
        });
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        out
    }

    /// An account action (`account`): the sign-ins here; the other flows are W3's.
    fn account(&mut self, now_ms: u64, action: crate::account::AccountAction, out: &mut Vec<Effect>) {
        use crate::account::signin::{self, By};
        use crate::account::AccountAction;
        if self.invite_action(now_ms, &action, out) || self.account_step(now_ms, &action, out) || self.selfjoin_action(now_ms, &action, out) {
            return;
        }
        match action {
            AccountAction::KitKept => {
                if self.next_view.account.step == "keep_kit" {
                    self.next_view.account = crate::views::AccountView { step: "none".into(), ..Default::default() };
                }
                return;
            }
            AccountAction::Found { email, password, hub } => return self.found(&email, &password, hub.as_deref(), out),
            AccountAction::Join { link } => {
                if self.engine.is_some() || self.founding.is_some() || self.signing_in.is_some() || self.linking.is_some() {
                    out.push(log(LogLevel::Warn, "account: a join by link on a slot that has a device or is busy"));
                    return;
                }
                let id = self.io(Pending::LinkLoad);
                out.push(Effect::Store { id, op: StoreOp::DeviceLoad });
                self.linking = Some(link);
                self.next_view.account = crate::views::AccountView { step: "join".into(), ..Default::default() };
                return;
            }
            _ => {}
        }
        let (by, account, secret, hub) = match action {
            AccountAction::SignIn { email, password, hub } => (By::Password, email, password, hub),
            AccountAction::SignInWithKit { email, words, hub } => (By::Kit, email, words, hub),
            other => {
                out.push(log(LogLevel::Debug, &format!("account: {other:?} is not handled here")));
                return;
            }
        };
        let Some(config) = self.config.as_ref() else { return };
        if self.engine.is_some() || self.room_join_wait.is_some() || self.signing_in.is_some() {
            out.push(log(LogLevel::Warn, "account: a sign-in on a slot that has a device or signs in already"));
            return;
        }
        let client = format!("{}/{}", config.platform, config.version);
        let begun = signin::hub_of(hub.as_deref(), &config.hub).and_then(|hub| signin::begin(hub, by, &account, &secret, &client));
        drop(secret);
        match begun {
            Ok((waiting, request)) => {
                let id = self.io(Pending::Login);
                self.signing_in = Some((id, waiting));
                self.next_view.account = crate::views::AccountView { step: "working".into(), ..Default::default() };
                out.push(Effect::Http { id, request });
            }
            Err(problem) => self.sign_in_failed(problem),
        }
    }

    /// `Found`: keys derived and the code made here; the room is founded once the slot's device state is read.
    fn found(&mut self, email: &str, password: &crate::account::Secret, hub: Option<&str>, out: &mut Vec<Effect>) {
        let Some(config) = self.config.as_ref() else { return };
        if self.engine.is_some() || self.room_join_wait.is_some() || self.signing_in.is_some() || self.founding.is_some() {
            out.push(log(LogLevel::Warn, "account: a founding on a slot that has a device or is busy"));
            return;
        }
        let hub = match crate::account::signin::hub_of(hub, &config.hub) {
            Ok(hub) => hub,
            Err(problem) => return self.found_failed(problem.text()),
        };
        match crate::account::found::begin(hub, email, password, &mut trommi_core::crypto::SystemEntropy) {
            Ok(founding) => {
                let id = self.io(Pending::FoundLoad);
                out.push(Effect::Store { id, op: StoreOp::DeviceLoad });
                self.founding = Some((None, founding));
                self.next_view.account = crate::views::AccountView { step: "working".into(), ..Default::default() };
            }
            Err(text) => self.found_failed(text),
        }
    }

    fn found_failed(&mut self, text: crate::views::Text) {
        self.founding = None;
        self.next_view.account = crate::views::AccountView { step: "found".into(), problem: Some(text), ..Default::default() };
    }

    /// The slot's device state for a founding: the device and its room are made, and the founding goes out.
    fn found_loaded(&mut self, now_ms: u64, revision: u64, entries: Vec<DeviceEntry>, out: &mut Vec<Effect>) {
        let Some(config) = self.config.as_ref() else { return };
        let Some((_, founding)) = self.founding.as_ref() else { return };
        let client = format!("{}/{}", config.platform, config.version);
        let entropy = self.entropy.take().unwrap_or_else(|| Box::new(trommi_core::crypto::SystemEntropy));
        let engine = Engine::found_room(&client, config.seed, revision, entries, &founding.hub, &founding.keys, now_ms, entropy);
        let mut engine = match engine {
            Ok(engine) => engine,
            Err(code) => {
                let text = crate::views::Text::new("account.found_failed", &format!("The room was not made ({code})."));
                return self.found_failed(text);
            }
        };
        let body = match founding.account_body(&engine.room(), &mut trommi_core::crypto::SystemEntropy) {
            Ok(body) => body,
            Err(code) => {
                let text = crate::views::Text::new("account.found_failed", &format!("The room was not made ({code})."));
                return self.found_failed(text);
            }
        };
        engine.set_founding(crate::hub::OutboxExtra { account: Some(body), ..Default::default() });
        // the room's hub first, as a join with the code writes it
        let key = RoomKey { v: 1, hub: founding.hub.clone(), room: engine.room().to_base64url() };
        let id = self.io(Pending::RoomKeyWrite);
        out.push(Effect::Store { id, op: StoreOp::Put { put: vec![Entry { key: ROOM_KEY.into(), value: serde_json::to_vec(&key).unwrap_or_default().into() }], delete: Vec::new() } });
        self.board = engine::feed::Board::new(engine.device());
        self.stored_hub = Some(engine.hub().to_string());
        self.engine = Some(engine);
        self.next_view.phase = Phase::Opening;
        self.with_engine(now_ms, out, |engine, ctx| engine.start(ctx));
    }

    /// The slot's device state for a join by link: the device is made and the join begins.
    fn link_loaded(&mut self, now_ms: u64, revision: u64, entries: Vec<DeviceEntry>, out: &mut Vec<Effect>) {
        let Some(config) = self.config.as_ref() else { return };
        let Some(link) = self.linking.take() else { return };
        let client = format!("{}/{}", config.platform, config.version);
        let entropy = self.entropy.take().unwrap_or_else(|| Box::new(trommi_core::crypto::SystemEntropy));
        match Engine::join_by_link(&client, config.seed, revision, entries, &link.0, now_ms, entropy) {
            Ok(engine) => {
                let key = RoomKey { v: 1, hub: engine.hub().to_string(), room: engine.room().to_base64url() };
                let id = self.io(Pending::RoomKeyWrite);
                out.push(Effect::Store { id, op: StoreOp::Put { put: vec![Entry { key: ROOM_KEY.into(), value: serde_json::to_vec(&key).unwrap_or_default().into() }], delete: Vec::new() } });
                self.board = engine::feed::Board::new(engine.device());
                self.stored_hub = Some(engine.hub().to_string());
                self.engine = Some(engine);
                self.with_engine(now_ms, out, |engine, ctx| engine.start(ctx));
            }
            Err(code) => self.next_view.account = crate::views::AccountView { step: "join".into(), problem: Some(link_problem(code)), ..Default::default() },
        }
    }

    fn sign_in_failed(&mut self, problem: crate::account::signin::Problem) {
        self.next_view.account = crate::views::AccountView { step: "sign_in".into(), problem: Some(problem.text()), ..Default::default() };
    }

    /// The hub answered a sign-in's login: the code is opened and the room joined with it.
    fn login_answered(&mut self, now_ms: u64, id: IoId, result: IoResult<HttpResponse>, out: &mut Vec<Effect>) {
        use crate::account::signin;
        let Some((_, waiting)) = self.signing_in.take_if(|(asked, _)| *asked == id) else { return };
        let answer = match result {
            IoResult::Ok(response) => crate::hub::Codec.answer(&waiting.expect, &response),
            IoResult::Err(error) => Err(crate::hub::classify_io(&error).unwrap_or(crate::hub::Failure::Offline)),
        };
        match signin::answered(&waiting, answer) {
            Ok((room, keys)) => {
                self.signed_in_join = true;
                let hub = waiting.hub.clone();
                drop(waiting);
                let effects = self.join_room(now_ms, engine::room_join::RoomJoin { hub, room, keys });
                out.extend(effects.into_iter().filter(|e| !matches!(e, Effect::Render { .. })));
            }
            Err(problem) => self.sign_in_failed(problem),
        }
    }

    /// A sign-in's join of the room, as the account's screen shows it.
    fn follow_sign_in(&mut self) {
        use engine::room_join::{RoomJoinError, RoomJoinStatus};
        if !self.signed_in_join {
            return;
        }
        match self.room_join {
            Some(RoomJoinStatus::Joined) => {
                self.signed_in_join = false;
                self.next_view.account = crate::views::AccountView { step: "none".into(), ..Default::default() };
            }
            Some(RoomJoinStatus::Failed(error)) => {
                self.signed_in_join = false;
                use crate::views::words::say;
                let problem = match error {
                    RoomJoinError::TooMany => say("account.too_many", "This room has as many devices as it may. Remove one first.", &[]),
                    RoomJoinError::Interrupted => say("account.interrupted", "Signing in was cut off. Sign in again to finish it.", &[]),
                    other => say("account.join_failed", format!("Joining the room did not work ({}).", other.code()), &[("code", other.code())]),
                };
                self.next_view.account = crate::views::AccountView { step: "sign_in".into(), problem: Some(problem), ..Default::default() };
            }
            _ => {}
        }
    }

    /// Sends a request an action built with the codec's builder for its route, through the engine, which adds the
    /// token, renews it and retries (`engine::requests`). `keep`: a name for a request with side effects at the hub,
    /// sent again after a restart (the route must be idempotent). The answer comes in [`App::take_answers`] under
    /// the id returned. `None` without a device.
    pub fn request(
        &mut self,
        now_ms: u64,
        build: impl FnOnce(&crate::hub::Site<'_>) -> Result<crate::app::io::HttpRequest, crate::hub::Failure>,
        keep: Option<String>,
    ) -> (Option<Result<IoId, &'static str>>, Vec<Effect>) {
        let mut out = Vec::new();
        let result = self.with_engine(now_ms, &mut out, |engine, ctx| engine.request(ctx, build, keep));
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        (result, out)
    }

    /// Archives a session group and retires the agent devices vouched into it (`engine::requests`,
    /// `Engine::archive_session`): kept across a restart. The hub's answer comes in [`App::take_answers`] too.
    pub fn archive_session(
        &mut self,
        now_ms: u64,
        group: trommi_core::ids::GroupId,
        retire: Vec<trommi_core::ids::DeviceId>,
    ) -> (Option<Result<IoId, &'static str>>, Vec<Effect>) {
        let mut out = Vec::new();
        let result = self.with_engine(now_ms, &mut out, |engine, ctx| engine.archive_session(ctx, group, retire));
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        (result, out)
    }

    /// The actions' requests answered (or given up) since the last call, in the order they ended.
    pub fn take_answers(&mut self) -> Vec<engine::requests::Answered> {
        std::mem::take(&mut self.answers)
    }

    /// How the last join of the room with the code stands; none before one.
    pub fn room_join(&self) -> Option<engine::room_join::RoomJoinStatus> {
        self.room_join
    }

    /// The device state for a join with the code is read: the engine is made on it.
    fn join_loaded(&mut self, now_ms: u64, id: IoId, revision: u64, entries: Vec<DeviceEntry>, out: &mut Vec<Effect>) {
        use engine::room_join::RoomJoinStatus;
        let Some((_, join)) = self.room_join_wait.take_if(|(waiting, _)| *waiting == id) else { return };
        let Some(config) = self.config.as_ref() else { return };
        if self.engine.is_some() {
            self.room_join = Some(RoomJoinStatus::Failed(engine::room_join::RoomJoinError::RoomExists));
            return;
        }
        let client = format!("{}/{}", config.platform, config.version);
        let entropy = self.entropy.take().unwrap_or_else(|| Box::new(trommi_core::crypto::SystemEntropy));
        match Engine::join_room(&client, config.seed, revision, entries, join, entropy) {
            Ok(engine) => {
                self.board = engine::feed::Board::new(engine.device());
                self.stored_hub = Some(engine.hub().to_string());
                self.engine = Some(engine);
                self.sessions_not_joined = None;
                self.with_engine(now_ms, out, |engine, ctx| engine.start_room_join(ctx));
            }
            Err(error) => self.room_join = Some(RoomJoinStatus::Failed(error)),
        }
    }

    /// Once a sign-in's sessions are through: the ones this device is not in, each with the reason. None before.
    pub fn sessions_not_joined(&self) -> Option<&[(trommi_core::ids::GroupId, &'static str)]> {
        self.sessions_not_joined.as_deref()
    }

    /// The invites a newcomer sent its Request for since they were last taken, oldest first, each once.
    pub fn take_invite_requests(&mut self) -> Vec<trommi_core::ids::InviteId> {
        std::mem::take(&mut self.invite_requests)
    }

    /// The engine's alerts so far, oldest first.
    pub fn alerts(&self) -> &[engine::Alert] {
        &self.alerts
    }

    /// The view model as last rendered: what a shell shows after folding every `Render` it was given.
    pub fn view(&self) -> &ViewModel {
        &self.view
    }

    /// One event in, the effects out. `now_ms` is the shell's clock (ms since 1970).
    pub fn handle(&mut self, now_ms: u64, event: Event) -> Vec<Effect> {
        let mut out = Vec::new();
        match event {
            Event::Start { config } => {
                if self.config.is_some() {
                    out.push(log(LogLevel::Warn, "start: already started"));
                } else {
                    self.jitter = Jitter::new(config.seed);
                    self.sent.seed = config.seed;
                    self.lens.utc_offset_min = config.utc_offset_min;
                    // (the Desk's greeting: one for the whole run)
                    self.lens.dice = (config.seed ^ (config.seed >> 32)) as u32;
                    self.config = Some(config);
                    self.next_view.phase = Phase::Starting;
                    let id = self.io(Pending::DeviceLoad);
                    out.push(Effect::Store {
                        id,
                        op: StoreOp::DeviceLoad,
                    });
                }
            }
            _ if self.config.is_none() => out.push(log(LogLevel::Warn, "an event before start")),
            Event::Store { id, result } => match self.pending.remove(&id) {
                Some(Pending::Wipe) => {
                    if let IoResult::Ok(StoreAnswer::Entries { entries }) = result {
                        self.wipe_cache(entries.into_iter().map(|e| e.key).collect(), &mut out);
                    }
                }
                Some(Pending::LinkLoad) => match result {
                    IoResult::Ok(StoreAnswer::Device { revision, entries }) => self.link_loaded(now_ms, revision, entries, &mut out),
                    _ => {
                        self.linking = None;
                        self.next_view.account = crate::views::AccountView { step: "join".into(), problem: Some(crate::views::Text::new("join.failed", "Joining did not work (storage).")), ..Default::default() };
                    }
                },
                Some(Pending::FoundLoad) => match result {
                    IoResult::Ok(StoreAnswer::Device { revision, entries }) => self.found_loaded(now_ms, revision, entries, &mut out),
                    _ => self.found_failed(crate::views::Text::new("account.found_failed", "The room was not made (storage).")),
                },
                Some(Pending::JoinLoad) => match result {
                    IoResult::Ok(StoreAnswer::Device { revision, entries }) => self.join_loaded(now_ms, id, revision, entries, &mut out),
                    _ => {
                        out.push(log(LogLevel::Error, "join: the device state was not read"));
                        self.room_join_wait = None;
                        self.room_join = Some(engine::room_join::RoomJoinStatus::Failed(engine::room_join::RoomJoinError::Device("storage")));
                    }
                },
                Some(Pending::Login) | Some(Pending::Dropped) if false => {}
                Some(Pending::RoomKey) => {
                    if let IoResult::Ok(StoreAnswer::Values { values }) = &result {
                        let key: Option<RoomKey> = values.first().and_then(Option::as_ref).and_then(|v| serde_json::from_slice(&v.0).ok());
                        self.stored_hub = key.filter(|k| k.v == 1).map(|k| k.hub);
                    }
                    if let Some((revision, entries)) = self.opening.take() {
                        self.open(now_ms, revision, entries, &mut out);
                    }
                }
                Some(Pending::RoomKeyWrite) | Some(Pending::Dropped) | Some(Pending::Login) => {
                    if let IoResult::Err(e) = &result {
                        out.push(log(LogLevel::Warn, &format!("store: {:?}", e.kind)));
                    }
                }
                Some(Pending::DeviceLoad) => match result {
                    IoResult::Ok(StoreAnswer::Device { revision, entries })
                        if revision != 0
                            && !entries.is_empty()
                            && self.engine.is_none()
                            && self.stored_hub.is_none()
                            && self.config.as_ref().is_some_and(|c| c.hub.is_empty()) =>
                    {
                        // (no hub named: the stored room names its hub, read before the device is opened)
                        self.opening = Some((revision, entries));
                        let id = self.io(Pending::RoomKey);
                        out.push(Effect::Store { id, op: StoreOp::Get { keys: vec![ROOM_KEY.into()] } });
                    }
                    IoResult::Ok(StoreAnswer::Device { revision, entries }) => {
                        self.open(now_ms, revision, entries, &mut out)
                    }
                    IoResult::Ok(_) => {
                        out.push(log(LogLevel::Error, "device load: not a device answer"))
                    }
                    IoResult::Err(e) => {
                        out.push(log(LogLevel::Error, &format!("device load: {:?}", e.kind)));
                        self.closed(now_ms, &mut out);
                    }
                },
                // (a cache entry that stays is only a cache entry: the hub no longer serves the file)
                Some(Pending::CacheDelete) => {}
                None if self.cache.is_some() && self.cached(now_ms, id, &result, &mut out) => {}
                None => {
                    let mine = self.with_engine(now_ms, &mut out, |engine, ctx| {
                        engine.on_store(ctx, id, &result)
                    });
                    if mine != Some(true) {
                        out.push(log(LogLevel::Warn, "store: an answer to nothing asked"));
                    }
                }
            },
            Event::Http { id, result } if self.pending.get(&id) == Some(&Pending::Login) => {
                self.pending.remove(&id);
                self.login_answered(now_ms, id, result, &mut out);
            }
            Event::Http { id, result } => {
                let mine = self.with_engine(now_ms, &mut out, |engine, ctx| {
                    engine.on_http(ctx, id, result)
                });
                if mine != Some(true) && self.pending.remove(&id).is_none() {
                    out.push(log(LogLevel::Warn, "an answer to nothing asked"));
                }
            }
            Event::Secret { id, .. } | Event::Platform { id, .. } => {
                if self.pending.remove(&id).is_none() {
                    out.push(log(LogLevel::Warn, "an answer to nothing asked"));
                }
            }
            Event::Stream { id, item } => {
                // (an item of a stream the engine closed is dropped: the id is forgotten)
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_stream(ctx, id, item));
            }
            Event::Progress { id, done, total } => {
                self.files.progress(id, done, total.unwrap_or(0));
                self.stale_views = true;
            }
            Event::Finished { .. } => {}
            Event::PushToken { .. } => {}
            Event::Zone { utc_offset_min } => {
                self.lens.utc_offset_min = utc_offset_min;
                self.stale_views = true;
            }
            Event::Action { action: crate::actions::Action::Account { action } } => self.account(now_ms, action, &mut out),
            Event::Action { action } => self.action(now_ms, action, &mut out),
            Event::Focus { focus } => {
                // (the single desk chosen last keeps the note and the Scribble Board on All desks)
                if let Some(desk) = focus.desk.as_deref().filter(|d| *d != crate::views::board::ALL_DESKS) {
                    self.lens.home_desk = Some(desk.to_string());
                }
                self.lens.desk = focus.desk;
                let (session, card, off) = match focus.screen {
                    Screen::Session { session_id } => (Some(session_id), None, None),
                    Screen::Card { card_id } => (None, Some(card_id), None),
                    Screen::Off { query } => (None, None, Some(query)),
                    _ => (None, None, None),
                };
                // The seam to the reading of the past: a card or a session opened is where old history is read
                // (after a sign-in only the open cards and what is new are loaded). `Engine::read_past` is to be
                // called here, once it exists, for a page whose past this device has not read.
                if session != self.lens.session || card != self.lens.card {
                    self.opened(session.as_deref(), card.as_deref());
                }
                self.lens.session = session;
                self.lens.card = card;
                self.lens.off = off;
                self.stale_views = true;
            }
            Event::Wake => {
                self.timer = None;
                self.invites_look(now_ms, None, &mut out);
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_wake(ctx));
                if self.reopen_at.is_some_and(|at| now_ms >= at) {
                    self.reopen(&mut out);
                }
            }
            Event::Foreground => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_foreground(ctx));
                // (one that gave up on its stored state tries again when the person comes back)
                if self.next_view.phase == Phase::Stopped && self.reopen_tries > REOPEN_TRIES {
                    self.reopen_tries = 0;
                    self.reopen(&mut out);
                }
            }
            Event::Background => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_background(ctx));
            }
            Event::Net { online } => {
                self.with_engine(now_ms, &mut out, |engine, ctx| engine.on_net(ctx, online));
            }
            Event::Push { .. } => {}
        }
        self.after_engine(now_ms, &mut out);
        self.render(&mut out);
        out
    }

    /// The person did something: the actions of `actions::run`, the toasts, a retry.
    fn action(&mut self, now_ms: u64, action: Action, out: &mut Vec<Effect>) {
        match &action {
            Action::Dismiss { toast_id } => {
                self.next_view.toasts.retain(|t| &t.id != toast_id);
                return;
            }
            Action::Retry => {
                self.with_engine(now_ms, out, |engine, ctx| engine.on_net(ctx, true));
                return;
            }
            _ => {}
        }
        if self.engine.is_none() {
            let text = crate::views::Text::new("action.no_room", "Sign in first.");
            self.sent.toast(&mut self.next_view.toasts, text, None);
            return;
        }
        if self.file_action(now_ms, &action, out) {
            return;
        }
        if writes_room_registers(&action) && self.next_view.phase != Phase::Live {
            if self.held_actions.len() < 64 {
                self.held_actions.push(action);
            }
            return;
        }
        self.run_action(now_ms, action, out);
    }

    /// An action of `actions::run`, done; an archived session's group is archived at the hub too.
    fn run_action(&mut self, now_ms: u64, action: Action, out: &mut Vec<Effect>) {
        let mut done = false;
        self.doing(now_ms, out, |doing| match doing.run(&action) {
            Ok(()) => done = true,
            Err(NotDone { text, again }) => doing.sent.toast(doing.toasts, text, again.then_some(action.clone())),
        });
        // an archived session is over for good (5.2.10): the hub takes nothing more for its group, and the agent
        // device a self-joined session brought along goes with it
        if let (true, Action::ArchiveSession { session_id, on: true }) = (done, &action) {
            self.archive_group(now_ms, session_id, out);
        }
    }

    fn archive_group(&mut self, now_ms: u64, session_id: &str, out: &mut Vec<Effect>) {
        let Some(s) = self.board.model.sessions.get(session_id) else { return };
        let unhex = |h: &str| (0..h.len() / 2).map(|i| u8::from_str_radix(h.get(i * 2..i * 2 + 2).unwrap_or(""), 16).ok()).collect::<Option<Vec<u8>>>();
        let Some(group) = s.group_id.as_deref().and_then(unhex).and_then(|b| trommi_core::ids::GroupId::from_bytes(&b).ok()) else { return };
        let vouched = s.settings.as_ref().and_then(|v| v.get("vouched")).is_some_and(|v| v.is_object());
        let retire: Vec<trommi_core::ids::DeviceId> = if vouched {
            s.agent_device_id.as_deref().and_then(unhex).and_then(|b| <[u8; 32]>::try_from(b).ok()).map(trommi_core::ids::DeviceId::new).into_iter().collect()
        } else {
            Vec::new()
        };
        if let Some(Err(why)) = self.with_engine(now_ms, out, |engine, ctx| engine.archive_session(ctx, group, retire)) {
            out.push(log(LogLevel::Warn, &format!("archive: {why}")));
        }
    }

    /// The file actions (`crate::files`): true when `action` was one.
    fn file_action(&mut self, now_ms: u64, action: &Action, out: &mut Vec<Effect>) -> bool {
        let Some(engine) = self.engine.as_mut() else { return false };
        let send = match action {
            Action::Attach { local_id, name, media_type, bytes } => {
                engine.with_entropy(|entropy| self.files.attach(local_id, name, media_type, bytes, entropy)).map(Some)
            }
            Action::OpenFile { attachment_id, save } => self.files.open(attachment_id, *save, out),
            Action::Share { attachment_id, expires_ms } => {
                engine.with_entropy(|entropy| self.files.share(attachment_id, *expires_ms, now_ms, entropy)).map(Some)
            }
            Action::RevokeShare { share_id } => self.files.unshare(share_id).map(Some),
            Action::CancelFile { file_id } => {
                for id in self.files.cancel(file_id) {
                    if let Some(flying) = engine.forget_request(id) {
                        out.push(Effect::Cancel { id: flying });
                    }
                }
                Ok(None)
            }
            _ => return false,
        };
        self.stale_views = true;
        match send {
            Ok(Some(send)) => {
                let (build, keep, pending, expect) = send.split();
                let sent = self.with_engine(now_ms, out, |engine, ctx| engine.request(ctx, build, keep));
                let expect = expect.borrow_mut().take();
                match (sent, expect) {
                    (Some(Ok(id)), Some(expect)) => self.files.sent(id, pending, expect),
                    _ => {
                        self.files.not_sent(pending);
                        let text = crate::views::Text::new("file.not_sent", "That could not be sent. Try again.");
                        self.sent.toast(&mut self.next_view.toasts, text, None);
                    }
                }
            }
            Ok(None) => {}
            Err(text) => self.sent.toast(&mut self.next_view.toasts, text, None),
        }
        true
    }

    /// Runs `step` with what an action needs (the model, the bookkeeping, the device's seal), then shows what it
    /// changed. Nothing without a device.
    fn doing(&mut self, now_ms: u64, out: &mut Vec<Effect>, step: impl FnOnce(&mut Doing<'_>)) {
        let utc_offset_min = self.lens.utc_offset_min;
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let room = engine.room();
        let mut ctx = engine::Ctx {
            now: now_ms,
            next_id: &mut self.next_id,
            out,
        };
        let mut seal = |draft: &trommi_core::device::Draft,
                        to: Option<&trommi_core::ids::DeviceId>,
                        files: &[trommi_core::ids::FileId]| {
            engine.act(&mut ctx, |device, now| device.seal(draft, to, files, now))
        };
        let files = &self.files;
        let file = |id: &str| files.reference(id).cloned();
        let mut doing = Doing {
            file: &file,
            model: &mut self.board.model,
            sent: &mut self.sent,
            toasts: &mut self.next_view.toasts,
            room,
            now: now_ms,
            utc_offset_min,
            seal: &mut seal,
            change: Change::default(),
        };
        step(&mut doing);
        let mut change = std::mem::take(&mut doing.change);
        self.board.model.project(&mut change, now_ms);
        self.stale_views = true;
        self.goals_check = true;
    }

    /// A Note's version is back, refused or lost: the edit that waited behind it is sealed on what stands now.
    fn notes_free(&mut self, now_ms: u64, free: Vec<(String, Option<NoteNext>)>, out: &mut Vec<Effect>) {
        for (id, next) in free {
            let Some(next) = next else { continue };
            self.doing(now_ms, out, |doing| {
                if let Err(NotDone { text, again }) = doing.seal_note(&next.action, Some(&id), &next.own, next.closed, &next.local_id) {
                    doing.sent.toast(doing.toasts, text, again.then_some(next.action.clone()));
                }
            });
        }
    }

    /// A card or a session came into view (or left it): the hook for reading its past.
    fn opened(&mut self, _session: Option<&str>, _card: Option<&str>) {}

    /// What the hub said of the actions in flight, and the ones whose entry is gone without a word: each echo of
    /// a refused or lost entry is rolled back, with a toast that offers the action again.
    fn settle_actions(&mut self, now_ms: u64, refused: Vec<(u64, &'static str)>, out: &mut Vec<Effect>) {
        // (goals refused are written again at the next look)
        self.sent.goals.retain(|_, (_, entry)| !refused.iter().any(|(id, _)| id == entry));
        let mut gone: Vec<(u64, crate::views::Text)> =
            refused.into_iter().map(|(id, code)| (id, actions::run::refused(code))).collect();
        if !self.sent.is_empty() {
            if let Some(engine) = self.engine.as_ref() {
                let held = engine.device().outbox().iter().map(|e| e.id).collect();
                gone.extend(self.sent.gone(&held).into_iter().map(|id| (id, actions::run::lost())));
            }
        }
        if gone.is_empty() {
            return;
        }
        let mut change = Change::default();
        let mut offered: Vec<Action> = Vec::new();
        let mut free = Vec::new();
        for (id, text) in gone {
            free.extend(self.sent.note_back(None, Some(id)));
            for out in self.sent.take(id) {
                if let Some(local_id) = &out.local_id {
                    self.board.model.rollback_echo(local_id, &mut change);
                }
                // (one toast per action, however many entries it made)
                if !offered.contains(&out.action) {
                    offered.push(out.action.clone());
                    self.sent.toast(&mut self.next_view.toasts, text.clone(), Some(out.action));
                }
            }
        }
        self.board.model.project(&mut change, now_ms);
        self.stale_views = true;
        self.notes_free(now_ms, free, out);
    }

    /// The stored device as the shell loaded it: an engine over it, or the sign-in screens.
    fn open(&mut self, now_ms: u64, revision: u64, entries: Vec<DeviceEntry>, out: &mut Vec<Effect>) {
        let Some(config) = self.config.as_ref() else {
            return;
        };
        let hub = if !config.hub.is_empty() {
            config.hub.as_str()
        } else {
            self.stored_hub.as_deref().unwrap_or(DEFAULT_HUB)
        };
        let client = format!("{}/{}", config.platform, config.version);
        let entropy = self
            .entropy
            .take()
            .unwrap_or_else(|| Box::new(trommi_core::crypto::SystemEntropy));
        // (opened again in place: the board stays as it was shown, the new engine brings what is new)
        let again = self.engine.is_some();
        match Engine::open_with(hub, &client, config.seed, revision, entries, entropy) {
            Ok(engine) if again && engine.device().id() != self.board_device() => {
                out.push(log(LogLevel::Error, "device open: another device is stored now"));
                self.next_view.phase = Phase::Stopped;
            }
            Ok(engine) => {
                if !again {
                    self.board = engine::feed::Board::new(engine.device());
                }
                self.stale_views = true;
                // the cache first: the board it holds is drawn before the engine asks the hub anything
                let room = engine::feed::hex(engine.room().as_bytes());
                let device = engine::feed::hex(engine.device().id().as_bytes());
                self.cache = Some(crate::cache::Keeper::open(&room, &device, revision, &mut self.next_id, out));
                self.engine = Some(engine);
                self.next_view.phase = Phase::Opening;
            }
            Err(OpenError::NoRoom) if !again => self.next_view.phase = Phase::SignedOut,
            Err(OpenError::Interrupted) if !again => {
                self.next_view.phase = Phase::SignedOut;
                self.room_join = Some(engine::room_join::RoomJoinStatus::Failed(engine::room_join::RoomJoinError::Interrupted));
            }
            Err(OpenError::Interrupted) => {
                out.push(log(LogLevel::Error, "device open: the stored state holds no room now"));
                self.next_view.phase = Phase::Stopped;
            }
            Err(OpenError::NoRoom) => {
                out.push(log(LogLevel::Error, "device open: the stored state holds no room now"));
                self.next_view.phase = Phase::Stopped;
            }
            Err(OpenError::Device(code)) => {
                out.push(log(LogLevel::Error, &format!("device open: {code}")));
                self.closed(now_ms, out);
            }
            Err(OpenError::Hub) => {
                out.push(log(LogLevel::Error, "start: the hub's address is not canonical"));
                self.next_view.phase = Phase::Stopped;
            }
        }
    }

    /// The id of the device the board is of (the one opened first).
    fn board_device(&self) -> trommi_core::ids::DeviceId {
        self.engine.as_ref().map_or(trommi_core::ids::DeviceId::new([0; 32]), |e| e.device().id())
    }

    /// The device closed itself, or its stored state did not open again: read again after a wait, at most
    /// [`REOPEN_TRIES`] times in a row; then the App stops. Only a device that ran here is opened again in place.
    fn closed(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if self.engine.is_none() {
            self.next_view.phase = Phase::Stopped;
            return;
        }
        self.reopen_tries += 1;
        if self.reopen_tries > REOPEN_TRIES {
            out.push(log(LogLevel::Error, "the device does not open again: stopped"));
            self.reopen_at = None;
            self.next_view.phase = Phase::Stopped;
            return;
        }
        let wait = engine::Backoff::HUB.delay_ms(self.reopen_tries, None, self.jitter.draw());
        self.reopen_at = Some(now_ms + wait);
        self.next_view.phase = Phase::Opening;
    }

    /// Reads the stored state again, to open it in place of the closed device.
    fn reopen(&mut self, out: &mut Vec<Effect>) {
        self.reopen_at = None;
        let id = self.io(Pending::DeviceLoad);
        out.push(Effect::Store { id, op: StoreOp::DeviceLoad });
    }

    /// An answer to one of the cache's store ops; false when it was none. Once the start's read is through, the board
    /// holds what it gave and the engine starts.
    fn cached(&mut self, now_ms: u64, id: IoId, result: &IoResult<StoreAnswer>, out: &mut Vec<Effect>) -> bool {
        let Some(cache) = self.cache.as_mut() else {
            return false;
        };
        let read = match cache.on_store(id, result, &mut self.board.model, now_ms, &mut self.next_id, out) {
            crate::cache::Took::No => return false,
            crate::cache::Took::Mine => return true,
            crate::cache::Took::Read(read) => read,
        };
        match read {
            crate::cache::Read::Loaded { listing, .. } => {
                // the Desk the hub listed last stands until it lists anew
                if let Some(listing) = listing {
                    self.board.model.set_desk_listing(Some((listing.at, listing.open)));
                    self.board.model.set_desk_hold(false);
                }
                let change = self.board.finish(now_ms);
                cache.note(&change);
            }
            crate::cache::Read::Dropped => {
                if let Some(engine) = self.engine.as_ref() {
                    self.board = engine::feed::Board::new(engine.device());
                }
                out.push(log(LogLevel::Warn, "the cache was not whole or not this device's: dropped"));
            }
            crate::cache::Read::Empty => {}
        }
        self.stale_views = true;
        self.with_engine(now_ms, out, |engine, ctx| engine.start(ctx));
        true
    }

    fn with_engine<T>(
        &mut self,
        now_ms: u64,
        out: &mut Vec<Effect>,
        step: impl FnOnce(&mut Engine, &mut engine::Ctx<'_>) -> T,
    ) -> Option<T> {
        let engine = self.engine.as_mut()?;
        let mut ctx = engine::Ctx {
            now: now_ms,
            next_id: &mut self.next_id,
            out,
        };
        Some(step(engine, &mut ctx))
    }

    /// What the engine reported since the last event: the phase, the alerts, the timer.
    fn after_engine(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let mut fed = false;
        let mut evicted = Vec::new();
        let mut stopped = false;
        let mut refused = Vec::new();
        let mut dropped = false;
        let joining_room = engine.joining_room();
        let mut notes_back = Vec::new();
        let mut invite_news = Vec::new();
        let mut invite_answers = Vec::new();
        let mut step_answers = Vec::new();
        while let Some(report) = engine.next_report() {
            self.board.take(&report, engine.device(), now_ms);
            fed = true;
            match report {
                Report::Desk { at, open, truncated: false } => {
                    if let Some(cache) = self.cache.as_mut() {
                        let open = open.iter().map(|(id, version)| (engine::feed::hex(id.as_bytes()), *version)).collect();
                        cache.listed(crate::cache::Listing { at, open });
                    }
                }
                // an own Note's version back from the hub: the edit that waited behind it can follow
                Report::Envelope { received, .. } if !self.sent.notes.is_empty() => {
                    let hash = crate::engine::feed::hex(received.envelope_hash.as_bytes());
                    notes_back.extend(self.sent.note_back(Some(&hash), None));
                }
                Report::Connection(Connection::Live) => {
                    self.next_view.phase = Phase::Live;
                    self.reopen_tries = 0;
                }
                Report::Connection(Connection::Offline) => self.next_view.phase = Phase::Offline,
                Report::Connection(Connection::CatchingUp) => {
                    if self.next_view.phase != Phase::Live {
                        self.next_view.phase = Phase::Opening
                    }
                }
                Report::Stopped(_) if joining_room => {
                    // (a device that is joining the room is not opened again: the join is asked for again)
                    dropped = true;
                    self.room_join = Some(engine::room_join::RoomJoinStatus::Failed(engine::room_join::RoomJoinError::Device("storage")));
                }
                Report::Stopped(why) if why.reopens() => stopped = true,
                Report::Stopped(_) => {
                    self.next_view.phase = Phase::Stopped;
                    if let Some(cache) = self.cache.as_mut() {
                        cache.stop();
                    }
                }
                Report::LinkJoin(status) => {
                    use crate::engine::sync::link_join::LinkJoinStatus;
                    match status {
                        LinkJoinStatus::Asking => {}
                        LinkJoinStatus::Code(code) => {
                            self.next_view.account = crate::views::AccountView { step: "compare_emoji".into(), emoji: invites::emoji_of(code), ..Default::default() };
                        }
                        LinkJoinStatus::Joined => {
                            self.next_view.account = crate::views::AccountView { step: "none".into(), ..Default::default() };
                            self.next_view.phase = Phase::Opening;
                        }
                        LinkJoinStatus::Failed(code) => {
                            dropped = true;
                            self.next_view.account = crate::views::AccountView { step: "join".into(), problem: Some(link_problem(code)), ..Default::default() };
                        }
                    }
                }
                Report::Founded(None) => {
                    if let Some((_, founding)) = self.founding.take() {
                        self.next_view.account = crate::views::AccountView { step: "keep_kit".into(), kit: founding.kit_words(), ..Default::default() };
                    }
                }
                Report::Founded(Some(code)) => {
                    dropped = true;
                    self.founding = None;
                    let text = match code {
                        "account-exists" => crate::views::Text::new("account.exists", "An account with this e-mail exists. Sign in instead."),
                        "rate-limited" | "throttled" => crate::views::Text::new("account.throttled", "Too many tries. Wait a moment, then try again."),
                        code => crate::views::Text::new("account.found_failed", &format!("The room was not made ({code}).")),
                    };
                    self.next_view.account = crate::views::AccountView { step: "found".into(), problem: Some(text), ..Default::default() };
                }
                Report::SessionsJoined { not_joined, .. } => self.sessions_not_joined = Some(not_joined),
                Report::Answer(answered) if self.invites.owns(answered.id) => invite_answers.push(*answered),
                Report::Answer(answered) if self.steps.asked.contains_key(&answered.id) => step_answers.push(*answered),
                Report::Answer(answered) if self.files.owns(answered.id) => {
                    if let Some(text) = self.files.answered(answered.id, &answered.result, out) {
                        self.sent.toast(&mut self.next_view.toasts, text, None);
                    }
                    self.stale_views = true;
                }
                Report::Answer(answered) => {
                    if let Some(keep) = answered.keep.as_deref().filter(|k| k.starts_with("unshare/")) {
                        self.files.kept_answered(keep);
                        self.stale_views = true;
                    } else {
                        self.answers.push(*answered);
                    }
                }
                Report::RoomJoin(status) => {
                    use engine::room_join::RoomJoinStatus;
                    match status {
                        RoomJoinStatus::Failed(_) => dropped = true,
                        RoomJoinStatus::Joined => self.next_view.phase = Phase::Opening,
                        _ => {}
                    }
                    self.room_join = Some(status);
                }
                Report::InviteRequest { invite } => {
                    invite_news.push(invite);
                    if !self.invite_requests.contains(&invite) {
                        if self.invite_requests.len() >= INVITE_REQUESTS_MAX {
                            self.invite_requests.remove(0);
                        }
                        self.invite_requests.push(invite);
                    }
                }
                Report::FileEvicted { file } => evicted.push(file),
                Report::Accepted { id, .. } => {
                    self.sent.take(id);
                }
                Report::Refused { id, code, .. } => refused.push((id, code)),
                Report::Alert(alert) => {
                    if self.alerts.len() >= ALERTS_MAX {
                        self.alerts.remove(0);
                    }
                    self.alerts.push(alert);
                }
                _ => {}
            }
        }
        if !evicted.is_empty() {
            // the cached bytes of a file the hub deleted (11.3) go
            let room = crate::engine::feed::hex(engine.room().as_bytes());
            let delete = evicted.iter().map(|file| crate::cache::key(&room, crate::cache::part::FILE, Some(&crate::engine::feed::hex(file.as_bytes())))).collect();
            let id = IoId(self.next_id);
            self.next_id += 1;
            self.pending.insert(id, Pending::CacheDelete);
            out.push(Effect::Store { id, op: StoreOp::Put { put: Vec::new(), delete } });
        }
        let engine_wake = if dropped {
            // a join of the room that failed: the engine goes (what it wrote last is the wipe, still stored)
            for id in engine.forget() {
                self.pending.insert(id, Pending::Dropped);
            }
            self.engine = None;
            self.board = engine::feed::Board::default();
            self.stale_views = true;
            self.next_view.phase = Phase::SignedOut;
            None
        } else {
            engine.next_wake()
        };
        // (a view's moment by the clock, a link that falls asleep: the model's statuses are worked out at its clock,
        //  so the board is brought to now first)
        let view_due = self.view_wake.is_some_and(|at| now_ms >= at);
        if fed || view_due || (self.board.model.desk_wake != 0 && now_ms >= self.board.model.desk_wake) {
            let change = self.board.finish(now_ms);
            if let Some(cache) = self.cache.as_mut() {
                cache.note(&change);
            }
            self.stale_views = true;
        }
        // (the cache's writes go behind every device write handed out so far: the shell holds them at its barrier)
        let mut cache_wake = None;
        if let Some(cache) = self.cache.as_mut() {
            cache.saw(out);
            cache.step(now_ms, &mut self.next_id, out);
            cache_wake = cache.next_wake();
        }
        self.settle_actions(now_ms, refused, out);
        for answered in invite_answers {
            self.invite_answered(now_ms, answered.id, &answered.result, out);
        }
        for answered in step_answers {
            self.step_answered(now_ms, answered.id, &answered.result, out);
        }
        for invite in invite_news {
            self.invites_look(now_ms, Some(invite), out);
        }
        self.invites_settle(now_ms, out);
        if fed {
            self.selfjoin_tend(now_ms, out);
        }
        self.notes_free(now_ms, notes_back, out);
        self.goals_check |= fed;
        if !self.held_actions.is_empty() && self.next_view.phase == Phase::Live {
            for action in std::mem::take(&mut self.held_actions) {
                self.run_action(now_ms, action, out);
            }
        }
        if self.goals_check && self.next_view.phase == Phase::Live {
            self.doing(now_ms, out, |doing| doing.sync_goals());
            self.goals_check = false;
        }
        // The board's units: made again when the model, the lens or the clock moved (every unit whole; a unit that
        // did not change is not sent, `ViewModel::diff`).
        if self.stale_views || view_due {
            self.stale_views = false;
            self.lens.reading = self.board.model.reading;
            let (wake, shown) = crate::views::render_with_files(&self.board.model, &self.lens, now_ms, &mut self.next_view);
            self.view_wake = wake;
            self.files.shown(shown, &mut self.next_view.files);
        }
        if stopped {
            self.closed(now_ms, out);
        }
        let model_wake = Some(self.board.model.desk_wake).filter(|at| *at != 0);
        let wake = [engine_wake, model_wake, self.view_wake, cache_wake, self.reopen_at, self.invites.wake()].into_iter().flatten().min();
        if wake != self.timer {
            self.timer = wake;
            out.push(Effect::Timer { at_ms: wake });
        }
    }

    fn io(&mut self, what: Pending) -> IoId {
        let id = IoId(self.next_id);
        self.next_id += 1;
        self.pending.insert(id, what);
        id
    }

    /// Puts a `Render` first when the view changed: the shell draws before it starts the I/O.
    fn render(&mut self, out: &mut Vec<Effect>) {
        self.follow_sign_in();
        let delta = if !self.rendered_once {
            self.rendered_once = true;
            let mut whole = self.next_view.clone();
            whole.rev = 0;
            Some(crate::views::ViewDelta {
                base: 0,
                parts: vec![crate::views::ViewPart::Whole {
                    model: Box::new(whole),
                }],
            })
        } else {
            self.view.diff(&self.next_view)
        };
        if let Some(delta) = delta {
            // the core's own fold, so `view` is exactly what a shell holds
            if self.view.apply(&delta).is_ok() {
                self.next_view.rev = self.view.rev;
                out.insert(0, Effect::Render { delta });
            }
        }
    }
}

/// The words of a join by link that did not work.
fn link_problem(code: &str) -> crate::views::Text {
    use crate::views::Text;
    match code {
        "invite-expired" => Text::new("join.expired", "The link has ended. Ask for a new one."),
        "invite-used" => Text::new("join.used", "The link was used already. Ask for a new one."),
        "code-mismatch" => Text::new("join.mismatch", "The other device said the emoji do not match. Nothing was joined."),
        "bad-invite" | "bad-format" => Text::new("join.bad_link", "That is not an invite link."),
        "room-exists" => Text::new("join.room_exists", "This device is in a room already."),
        code => Text::new("join.failed", &format!("Joining did not work ({code}).")),
    }
}

/// Whether an action writes a register of the room group (stars, snoozes, drafts, sessions' settings, desks).
fn writes_room_registers(action: &Action) -> bool {
    matches!(
        action,
        Action::Star { .. }
            | Action::Snooze { .. }
            | Action::Unsnooze { .. }
            | Action::Draft { .. }
            | Action::RenameSession { .. }
            | Action::ArchiveSession { .. }
            | Action::MoveSession { .. }
            | Action::AddDesk { .. }
            | Action::RenameDesk { .. }
            | Action::RemoveDesk { .. }
            | Action::SetGoal { .. }
    )
}

fn log(level: LogLevel, text: &str) -> Effect {
    Effect::Log {
        level,
        text: text.to_string(),
    }
}

#[cfg(test)]
mod tests;
