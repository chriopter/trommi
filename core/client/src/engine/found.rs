//! Founding a room (spec/v1.md 8.1; with the account, 8.9): a new device founds the room under the keys of a fresh
//! recovery code, and its founding (`POST /v1/rooms`, no token) carries the account the hub makes in the same
//! transaction. As the web's `room.ts foundRoom` with `account.ts createAccount`.
//!
//! [`Engine::found_room`] makes the device and the room; the App then gives the founding's body extra (the account,
//! whose sealed copies bind the room id) with [`Engine::set_founding`]. Until the hub took the founding the engine
//! does nothing else: no sign-in (the room is not there yet), no order. A refusal (`account-exists`, a throttle)
//! wipes the device in one write, so the slot is empty again; a failure that is no answer posts the same bytes
//! again (the hub answers a repeated founding as the first). [`Report::Founded`] says how it ended.
//!
//! The extra is held in memory only: it holds the account's login keys. A process that dies before the founding was
//! answered leaves a device whose founding cannot be posted any more; the App then shows the sign-in again.

use super::*;
use crate::hub::OutboxExtra;
use trommi_core::recovery::RecoveryKeys;

pub(super) struct Founding {
    extra: OutboxExtra,
}

impl Engine {
    /// A new device in an empty slot (or one that holds no room) that founds a room under `keys` at `now_ms`.
    /// Refused: `bad-hub`, `room-exists` (the slot holds a room), the protocol core's code.
    #[allow(clippy::too_many_arguments)]
    pub fn found_room(
        client: &str,
        seed: u64,
        revision: u64,
        entries: Vec<DeviceEntry>,
        hub: &str,
        keys: &RecoveryKeys,
        now_ms: u64,
        entropy: Box<dyn Entropy + Send>,
    ) -> Result<Self, &'static str> {
        let address = HubAddress::parse(hub).map_err(|_| "bad-hub")?;
        if address.as_str() != hub {
            return Err("bad-hub");
        }
        let entries: Vec<(Vec<u8>, Vec<u8>)> = entries.into_iter().map(|e| (e.key.0, e.value.0)).collect();
        if revision != 0 && !entries.is_empty() {
            let (store, _) = memstore::store(revision, entries.clone());
            if Device::open(store, Box::new(SystemEntropy)).is_ok_and(|d| d.room().is_some()) {
                return Err("room-exists");
            }
        }
        let stale: Vec<Vec<u8>> = entries.into_iter().map(|(k, _)| k).collect();
        let (store, writes) = memstore::fresh(revision, stale);
        let mut device = Device::create(store, entropy).map_err(|e| e.code())?;
        let room = device.found_room(keys, now_ms).map_err(|e| e.code())?;
        Ok(Self::over(device, writes, room, hub, address, client, seed))
    }

    /// The founding's body extra: the account to make with the room (and the hub's founding word, if any).
    pub fn set_founding(&mut self, extra: OutboxExtra) {
        self.founding = Some(Founding { extra });
    }

    /// Whether the founding waits for the hub.
    pub fn founding_room(&self) -> bool {
        self.founding.is_some()
    }

    /// Posts the founding (once at a time, after the wait of a failure).
    pub(super) fn drive_founding(&mut self, ctx: &mut Ctx<'_>) {
        self.flush(ctx);
        if self.posting.is_some() || self.pump_retry.waiting(ctx.now) {
            return;
        }
        let entries = self.device.outbox();
        let Some(entry) = entries.first().filter(|e| e.kind == OutboxKind::RoomFounding).cloned() else {
            // (nothing to found: an older founding was taken already)
            self.founding = None;
            self.report(Report::Founded(None));
            return;
        };
        let Some(extra) = self.founding.as_ref().map(|f| f.extra.clone()) else { return };
        let room = self.room;
        let (id, kind, group) = (entry.id, entry.kind, entry.group);
        self.posting = self.ask(ctx, Route::Outbox { room: &room, entry: &entry, extra: Some(&extra) }, false, |expect| Asked::Post { expect, entry: id, kind, group });
        if self.posting.is_none() {
            self.founding_failed(ctx, "bad-format");
        }
    }

    /// The hub's answer to the founding.
    pub(super) fn on_founding_posted(&mut self, ctx: &mut Ctx<'_>, entry: u64, kind: OutboxKind, group: Option<GroupId>, answer: Result<Answer, Failure>, refusal: Option<Refusal>) {
        match answer {
            Ok(Answer::Outbox(taken)) => {
                self.pump_retry.ok();
                self.founding = None;
                self.accepted(ctx, entry, kind, group, taken);
                self.report(Report::Founded(None));
                self.drive(ctx);
            }
            Err(Failure::Refused { code: Some(code), .. }) => {
                let code = Error::from_code(&code).map_or("refused", |e| e.code());
                self.founding_failed(ctx, code);
            }
            Err(Failure::Refused { code: None, status }) if status != 401 => self.founding_failed(ctx, "refused"),
            other => {
                let failure = match other {
                    Err(f) => f,
                    Ok(_) => Failure::BadAnswer { why: "a founding answer" },
                };
                let retry_after = match (&failure, refusal) {
                    (Failure::NotNow { retry_after_ms, .. }, _) => *retry_after_ms,
                    (_, Some(r)) => r.retry_after_ms,
                    _ => None,
                };
                self.pump_retry.fail(ctx.now, retry_after, &mut self.jitter);
            }
        }
    }

    /// The founding was refused: the device's state goes in one write, and the engine stops.
    fn founding_failed(&mut self, ctx: &mut Ctx<'_>, code: &'static str) {
        self.founding = None;
        self.flush(ctx);
        if let Some(write) = self.writes.wipe() {
            let id = ctx.id();
            self.pending.insert(id, Asked::DeviceWrite);
            ctx.out.push(Effect::Store {
                id,
                op: StoreOp::DeviceApply { expected_revision: write.expected_revision, put: Vec::new(), delete: write.delete.into_iter().map(Into::into).collect() },
            });
        }
        ctx.log(LogLevel::Warn, format!("founding: {code}"));
        self.report(Report::Founded(Some(code)));
    }
}
