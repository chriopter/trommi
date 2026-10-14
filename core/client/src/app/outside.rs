//! What the app shows outside its window, decided here so a shell only carries it out (iOS `LiveActivities.swift`
//! `candidates()`/`busy()` and `NotifyBridge.swift` did it before):
//!
//! - [`Effect::Snapshot`] (where `Config.can.snapshots`): what waits on every desk and how many sessions work, for
//!   widgets and the Live Activity. Sent when it changed, at most once per answer, never before the room was read
//!   up to its end once (the engine's `caught_up`); the demo room sends it too. A sign-out sends an empty one.
//! - [`Effect::Notify`] (where `Config.can.notify`): a card or request that newly starts waiting while the app is
//!   in the back. "New" is against the set that waited at the last look; that set is first taken once the room is
//!   live (and not while its history is read), so nothing read at the start, in a catch-up of history or after a
//!   sign-in rings. In the back: after `Event::Background` or `Focus { visible: false }`, and an App whose first
//!   news is `Event::Push` (launched by a push); in front again with `Event::Foreground` or `Focus { visible: true }`.
//!   The push level of this device filters: off, none; knocking, only the urgent ones.
//! - [`Effect::NotifyContext`] (where `Config.can.notify` and `Config.can.push` is "apns"): the context of the
//!   notification extension and the content keys of each session group's current epoch, sent again whenever an
//!   epoch moves (the engine's `Groups` and `Log` reports); keys empty while push is off here. Not before the push
//!   level is known (read from the hub once live, or set here) and the notify key is (the secret
//!   [`NOTIFY_KEY`](super::push::NOTIFY_KEY), read at the start). `None` at a sign-out (and a deleted account).

use super::effect::{NotifyContext, NotifyKey, WaitingCard, WaitingOption};
use super::push::{PushChoice, NOTIFY_KEY};
use super::{App, Effect, IoId, Notice, SecretOp, WidgetSnapshot};
use crate::views::{Board, Lens, Text};
use std::collections::BTreeSet;

/// At most so many waiting cards travel in a snapshot.
pub const WAITING_MAX: usize = 20;
/// At most so many notifications per answer (a burst rings a few times, not fifty).
const NOTICES_MAX: usize = 5;

#[derive(Default)]
pub(super) struct Outside {
    /// The snapshot sent last.
    sent: Option<WidgetSnapshot>,
    /// What waited at the last look, once the room was live; none before.
    known: Option<BTreeSet<String>>,
    /// The app is in front (none: not said yet).
    front: Option<bool>,
    /// The board may have changed since the last look.
    pub(super) due: bool,
    /// An epoch may have moved: the keys are read again.
    pub(super) keys_due: bool,
    /// The cursor goes with the next context (the app went to the back).
    cursor_due: bool,
    /// The keys as last read.
    keys: Vec<NotifyKey>,
    /// The context sent last.
    context: Option<NotifyContext>,
    /// The read of the notify key in flight.
    pub(super) key_read: Option<IoId>,
}

/// 2: a request or a blocking card, 1: a knock, 0: the rest (iOS `LiveState.rank`).
fn rank(c: &WaitingCard) -> u8 {
    if c.kind == "permission" || c.urgency == "critical" {
        2
    } else if c.urgency == "high" {
        1
    } else {
        0
    }
}

/// The snapshot of a board model at `now`, every waiting card in it (the caller cuts it to [`WAITING_MAX`]).
pub(crate) fn snapshot_of(model: &crate::model::Model, now: u64) -> WidgetSnapshot {
    let lens = Lens::default();
    let b = Board::new(model, &lens, now);
    let shown = |agent: Option<usize>| agent.is_none_or(|i| !b.agents[i].row.archived && !b.agents[i].row.removed);
    let session = |agent: Option<usize>| agent.map_or_else(|| "Agent".to_string(), |i| b.agents[i].row.name.clone());
    let mut cards: Vec<WaitingCard> = Vec::new();
    let mut urgent = false;
    for fresh in b.all_fresh.iter().filter(|f| shown(f.agent)) {
        urgent |= fresh.knocks;
        if fresh.permission {
            let Some(p) = model.permissions.get(&fresh.id) else { continue };
            cards.push(WaitingCard {
                id: fresh.id.clone(),
                title: format!("Approval: {}", p.tool_name),
                session: session(fresh.agent),
                kind: "permission".into(),
                urgency: "critical".into(),
                created_ms: p.sent_at,
                options: Vec::new(),
                multiple: false,
            });
        } else {
            let Some(c) = model.cards.get(&fresh.id) else { continue };
            cards.push(WaitingCard {
                id: fresh.id.clone(),
                title: crate::views::desk::shown_title(c),
                session: session(fresh.agent),
                kind: c.field("card_type").as_str().filter(|t| !t.is_empty()).unwrap_or("decision").to_string(),
                urgency: if c.urgency.is_empty() { "normal".into() } else { c.urgency.clone() },
                created_ms: c.created_at,
                options: crate::views::desk::options_of(&c.field("options")).into_iter().map(|o| WaitingOption { key: o.key, label: o.label }).collect(),
                multiple: c.field("allows_multiple") == serde_json::Value::Bool(true),
            });
        }
    }
    cards.sort_by(|a, b| rank(b).cmp(&rank(a)).then(a.created_ms.cmp(&b.created_ms)).then_with(|| a.id.cmp(&b.id)));
    let waiting = cards.len() as u32;
    let working = b.agents.iter().filter(|a| a.row.online && a.row.working && !a.row.archived && !a.row.removed).count() as u32;
    WidgetSnapshot { waiting, working, urgent, cards }
}

impl App {
    fn can(&self) -> (bool, bool, bool) {
        let can = self.config.as_ref().map(|c| &c.can);
        (
            can.is_some_and(|c| c.snapshots),
            can.is_some_and(|c| c.notify),
            can.is_some_and(|c| c.notify && c.push == "apns"),
        )
    }

    /// The app's place on the screen: `Some(true)` in front, `Some(false)` in the back; a push says "in the back"
    /// only to an App that was told nothing yet.
    pub(super) fn outside_front(&mut self, front: Option<bool>, pushed: bool) {
        match front {
            Some(f) => self.outside.front = Some(f),
            None if pushed && self.outside.front.is_none() => self.outside.front = Some(false),
            None => {}
        }
        if self.outside.front == Some(false) {
            self.outside.cursor_due = true;
        }
        self.outside.due = true;
    }

    /// The device opened: the notify key is read once, where it matters.
    pub(super) fn outside_opened(&mut self, out: &mut Vec<Effect>) {
        self.outside.keys_due = true;
        self.outside.due = true;
        if self.can().2 && self.push.notify_key.is_none() && self.outside.key_read.is_none() {
            let id = IoId(self.next_id);
            self.next_id += 1;
            self.outside.key_read = Some(id);
            out.push(Effect::Secret { id, op: SecretOp::Get { name: NOTIFY_KEY.into() } });
        }
    }

    /// The notify key as the platform's store had it.
    pub(super) fn outside_key(&mut self, value: Option<Vec<u8>>) {
        self.outside.key_read = None;
        if let Some(key) = value.and_then(|v| <[u8; 32]>::try_from(v.as_slice()).ok()) {
            if self.push.notify_key.is_none() {
                self.push.notify_key = Some(key);
            }
        }
        self.outside.due = true;
    }

    /// Signed out: the extension's context goes, the widgets show nothing, nothing known waits.
    pub(super) fn outside_signed_out(&mut self, out: &mut Vec<Effect>) {
        let (snapshots, _, context) = self.can();
        let front = self.outside.front;
        self.outside = Outside { front, ..Default::default() };
        if context {
            out.push(Effect::NotifyContext { context: None });
        }
        if snapshots {
            let empty = WidgetSnapshot::default();
            self.outside.sent = Some(empty.clone());
            out.push(Effect::Snapshot { snapshot: empty });
        }
    }

    /// Once per answer: the snapshot, the notifications and the extension's context, each where it changed.
    pub(super) fn outside(&mut self, now_ms: u64, out: &mut Vec<Effect>) {
        if !std::mem::take(&mut self.outside.due) {
            return;
        }
        let (snapshots, notify, context) = self.can();
        if context {
            self.outside_context(out);
        }
        if !snapshots && !notify {
            return;
        }
        let ready = self.demo || self.engine.as_ref().is_some_and(|e| e.caught_up());
        if !ready {
            return;
        }
        let mut snapshot = snapshot_of(&self.board.model, now_ms);
        if notify {
            self.outside_notices(&snapshot, out);
        }
        snapshot.cards.truncate(WAITING_MAX);
        if snapshots && self.outside.sent.as_ref() != Some(&snapshot) {
            self.outside.sent = Some(snapshot.clone());
            out.push(Effect::Snapshot { snapshot });
        }
    }

    fn outside_notices(&mut self, snapshot: &WidgetSnapshot, out: &mut Vec<Effect>) {
        let live = self.demo || self.next_view.phase == crate::views::Phase::Live;
        if !live || self.board.model.reading {
            return;
        }
        let now: BTreeSet<String> = snapshot.cards.iter().map(|c| c.id.clone()).collect();
        let Some(known) = self.outside.known.replace(now.clone()) else { return };
        if self.outside.front != Some(false) {
            return;
        }
        let level = self.push.level;
        let mut rung = 0;
        for c in snapshot.cards.iter().filter(|c| !known.contains(&c.id)) {
            let urgent = rank(c) > 0;
            match level {
                Some(PushChoice::Off) => return,
                Some(PushChoice::Knocking) if !urgent => continue,
                _ => {}
            }
            if rung == NOTICES_MAX {
                break;
            }
            rung += 1;
            out.push(Effect::Notify {
                notice: Notice {
                    id: c.id.clone(),
                    title: Text::new("notice.session", &c.session),
                    body: Text::new("notice.card", &c.title),
                    target: Some(c.id.clone()),
                    urgent,
                },
            });
        }
    }

    fn outside_context(&mut self, out: &mut Vec<Effect>) {
        let Some(engine) = self.engine.as_ref() else { return };
        let (Some(level), Some(push_key)) = (self.push.level, self.push.notify_key) else { return };
        if std::mem::take(&mut self.outside.keys_due) {
            self.outside.keys = current_keys(engine.device());
        }
        let Some(config) = self.config.as_ref() else { return };
        let position = engine.position();
        let mut next = NotifyContext {
            room: crate::engine::feed::hex(engine.room().as_bytes()),
            hub: engine.hub().to_string(),
            client: format!("{}/{}", config.platform, config.version),
            cursor: self.outside.context.as_ref().map_or(position, |c| c.cursor),
            push_key: push_key.to_vec().into(),
            keys: if level == PushChoice::Off { Vec::new() } else { self.outside.keys.clone() },
        };
        if std::mem::take(&mut self.outside.cursor_due) || self.outside.context.as_ref() != Some(&next) {
            next.cursor = position;
        }
        if self.outside.context.as_ref() != Some(&next) {
            self.outside.context = Some(next.clone());
            out.push(Effect::NotifyContext { context: Some(next) });
        }
    }
}

/// Of each session group the device is in, the content key of its current epoch (spec 15.2; the bindings'
/// `notify_keys`): no room group, no older epoch, none of a group it does not trust.
fn current_keys(device: &crate::engine::Device) -> Vec<NotifyKey> {
    let Ok(groups) = device.groups() else { return Vec::new() };
    let mut keys = Vec::new();
    for summary in groups.into_iter().filter(|s| s.session.is_some()) {
        if let Ok(key) = device.content_key(&summary.group, summary.epoch) {
            keys.push(NotifyKey { group: crate::engine::feed::hex(summary.group.as_bytes()), epoch: summary.epoch, key: key.expose().to_vec().into() });
        }
    }
    keys
}
