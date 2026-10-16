//! Durable public self-retirement notices. They contain no bearer, recovery code or content key and
//! survive account erasure until the hub acknowledges them. All validation and retry decisions live here.
use super::{App, Effect, Entry, Event, IoResult, Pending, StoreAnswer, StoreOp};
use crate::hub::{Answer, Codec, Expect, Failure, Route, Site};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{DeviceId, RoomId};

pub(super) const PREFIX: &str = "public-departure/";
const MAX_ACTIVE: usize = 8;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Notice { hub: String, room: String, device: String, signature: Vec<u8> }
impl Notice {
    fn ids(&self) -> Option<(RoomId, DeviceId)> {
        HubAddress::parse(&self.hub).ok()?;
        let room = RoomId::from_base64url(&self.room).ok()?;
        let device = DeviceId::from_base64url(&self.device).ok()?;
        trommi_core::hub_auth::verify_departure(&room, &device, &self.signature).ok()?;
        Some((room, device))
    }
    fn key(&self) -> String { format!("{PREFIX}{}.{}", self.room, self.device) }
    fn read(key: &str, bytes: &[u8]) -> Option<Self> {
        if bytes.len() > 2048 { return None; }
        let notice: Self = serde_json::from_slice(bytes).ok()?;
        notice.ids()?;
        (notice.key() == key).then_some(notice)
    }
    fn from_request(request: &super::HttpRequest) -> Option<Self> {
        if request.method != "POST" || request.headers.iter().any(|h| h.name.eq_ignore_ascii_case("authorization")) { return None; }
        let (hub, path) = request.url.split_once("/v1/rooms/")?;
        let room = path.strip_suffix("/departures")?;
        let body = request.body.as_ref()?;
        if body.len() > 1024 { return None; }
        let data: serde_json::Value = serde_json::from_slice(body.as_ref()).ok()?;
        if data.as_object()?.len() != 2 { return None; }
        let signature = trommi_core::ids::base64url_decode(data["signature"].as_str()?).ok()?;
        let notice = Self { hub:hub.into(), room:room.into(), device:data["device"].as_str()?.into(), signature };
        notice.ids()?;
        Some(notice)
    }
    fn request(&self, client: &str) -> Option<(super::HttpRequest, Expect)> {
        let (room, device) = self.ids()?;
        Codec.request(&Site { url:&self.hub, token:None, client, lease:None }, &Route::Depart { room:&room, device:&device, signature:&self.signature }).ok()
    }
}

struct Held { notice: Notice, from_disk: bool, durable: bool, done: bool, busy: bool, at: u64, attempt: u32 }
#[derive(Default)]
pub(super) struct Departures {
    records: BTreeMap<String, Held>,
    loading: bool,
    loaded: bool,
    read_at: u64,
}
impl Pending {
    pub(super) fn is_departure(&self) -> bool { matches!(self, Self::DepartureLoad | Self::DepartureSave(_) | Self::DepartureHttp(_, _) | Self::DepartureDelete(_)) }
}
impl Departures {
    pub(super) fn loaded(&self) -> bool { self.loaded }
    pub(super) fn keeps(&self, key: &str) -> bool { self.records.contains_key(key) }
    pub(super) fn wake(&self) -> Option<u64> {
        let waiting = (self.records.values().filter(|r| r.busy).count() < MAX_ACTIVE)
            .then(|| self.records.values().filter(|r| !r.busy).map(|r| r.at).min()).flatten();
        waiting.into_iter().chain((!self.loaded && !self.loading).then_some(self.read_at)).min()
    }
    pub(super) fn hurry(&mut self) { self.read_at = 0; for r in self.records.values_mut() { r.at = 0; } }
}
impl App {
    pub(super) fn departure_enqueue(&mut self, request: &super::HttpRequest, now: u64, out: &mut Vec<Effect>) {
        if let Some(notice) = Notice::from_request(request) {
            self.departures.records.entry(notice.key()).or_insert(Held { notice, from_disk:false, durable:false, done:false, busy:false, at:now, attempt:0 });
            // Persist this notice before DeviceApply, even when older requests occupy the transport limit.
            // Dropping a valid notice at an arbitrary queue length would strand a real device.
            self.save_departure(now, out);
        }
    }
    fn save_departure(&mut self, now: u64, out: &mut Vec<Effect>) {
        let keys: Vec<_> = self.departures.records.iter().filter(|(_, r)| !r.durable && !r.busy).map(|(key, _)| key.clone()).collect();
        for key in keys {
            let r = self.departures.records.get_mut(&key).unwrap();
            r.busy = true;
            r.at = now;
            let value = serde_json::to_vec(&r.notice).unwrap_or_default().into();
            let id = self.io(Pending::DepartureSave(key.clone()));
            out.push(Effect::Store { id, op:StoreOp::PutDurable { put:vec![Entry { key, value }], delete:vec![] } });
        }
    }
    pub(super) fn departure_entries(&mut self, entries: &[Entry], now: u64) {
        for e in entries.iter().filter(|e| e.key.starts_with(PREFIX)) {
            if let Some(notice) = Notice::read(&e.key, e.value.as_ref()) {
                self.departures.records.entry(e.key.clone()).or_insert(Held { notice, from_disk:true, durable:true, done:false, busy:false, at:now, attempt:0 });
            }
        }
    }
    pub(super) fn resume_departure(&mut self, now: u64, out: &mut Vec<Effect>) {
        if self.logout_requested {
            if self.engine.is_some() { self.sign_out(now, out); return; }
            if self.next_view.phase == crate::views::Phase::SignedOut && self.opening.is_none()
                && !self.pending.values().any(|p| matches!(p, Pending::DeviceLoad | Pending::RoomKey)) {
                self.logout_requested = false;
                self.forget_local_device(now, out);
                return;
            }
        }
        let Some(engine) = self.engine.as_ref() else { return };
        let room = engine.room().to_base64url();
        let device = engine.device().id().to_base64url();
        let key = self.departures.records.iter().find(|(_,r)| r.from_disk && r.notice.room == room && r.notice.device == device).map(|(key,_)| key.clone());
        if let Some(key) = key {
            self.departures.records.get_mut(&key).unwrap().from_disk = false;
            // A crash after the public notice was synced but before key erasure must finish logout,
            // rather than reopen the key whose owner already requested retirement.
            self.sign_out(now, out);
        }
    }
    pub(super) fn start_departures(&mut self, out: &mut Vec<Effect>) {
        if self.departures.loaded || self.departures.loading { return; }
        self.departures.loading = true;
        let id = self.io(Pending::DepartureLoad);
        out.push(Effect::Store { id, op:StoreOp::Scan { prefix:PREFIX.into() } });
    }
    pub(super) fn departure_event(&mut self, now: u64, event: &Event) -> bool {
        let id = match event { Event::Http { id, .. } | Event::Store { id, .. } => *id, _ => return false };
        let Some(pending) = self.pending.get(&id).filter(|p| p.is_departure()).cloned() else { return false };
        self.pending.remove(&id);
        if matches!(pending, Pending::DepartureLoad) {
            self.departures.loading = false;
            if let Event::Store { result:IoResult::Ok(StoreAnswer::Entries { entries }), .. } = event {
                self.departure_entries(entries, now);
                self.departures.loaded = true;
            } else { self.departures.read_at = now + 5000; }
            return true;
        }
        let waiting_key = match &pending {
            Pending::DepartureSave(key) | Pending::DepartureDelete(key) | Pending::DepartureHttp(key, _) => key.clone(),
            _ => return true,
        };
        let (key, success, retry_after) = match (pending, event) {
            (Pending::DepartureSave(key), Event::Store { result, .. }) => {
                let success = matches!(result, IoResult::Ok(StoreAnswer::Done));
                if success { if let Some(r) = self.departures.records.get_mut(&key) { r.durable = true; } }
                (key,success,None)
            }
            (Pending::DepartureDelete(key), Event::Store { result, .. }) => {
                if matches!(result, IoResult::Ok(StoreAnswer::Done)) { self.departures.records.remove(&key); return true; }
                (key,false,None)
            }
            (Pending::DepartureHttp(key, expect), Event::Http { result, .. }) => {
                let answer = match result { IoResult::Ok(response) => Codec.answer(&expect,response), IoResult::Err(_) => Err(Failure::Offline) };
                let retry_after = match &answer { Err(Failure::NotNow { retry_after_ms, .. }) => *retry_after_ms, _ => None };
                let success = matches!(answer, Ok(Answer::Done));
                if success { if let Some(r) = self.departures.records.get_mut(&key) { r.done = true; } }
                (key,success,retry_after)
            }
            _ => (waiting_key, false, None),
        };
        if let Some(r) = self.departures.records.get_mut(&key) {
            r.busy = false;
            if success { r.at = now; r.attempt = 0; } else {
                r.attempt = r.attempt.saturating_add(1);
                r.at = now.saturating_add(crate::engine::Backoff::HUB.delay_ms(r.attempt,retry_after,self.jitter.draw()));
            }
        }
        true
    }
    pub(super) fn drive_departures(&mut self, now: u64, out: &mut Vec<Effect>) {
        if self.demo || self.config.is_none() { return; }
        if !self.departures.loaded && !self.departures.loading && self.departures.read_at <= now { self.start_departures(out); }
        let client = self.config.as_ref().map(|c| format!("{}/{}",c.platform,c.version)).unwrap_or_default();
        let available = MAX_ACTIVE.saturating_sub(self.departures.records.values().filter(|r| r.busy).count());
        let keys: Vec<String> = self.departures.records.iter().filter(|(_,r)| !r.busy && r.at <= now).take(available).map(|(k,_)| k.clone()).collect();
        for key in keys {
            let r = self.departures.records.get_mut(&key).unwrap();
            r.busy = true;
            let (pending,effect) = if r.done {
                (Pending::DepartureDelete(key.clone()), Err(StoreOp::PutDurable { put:vec![], delete:vec![key.clone()] }))
            } else if !r.durable {
                let value = serde_json::to_vec(&r.notice).unwrap_or_default().into();
                (Pending::DepartureSave(key.clone()), Err(StoreOp::PutDurable { put:vec![Entry { key:key.clone(), value }], delete:vec![] }))
            } else if let Some((request,expect)) = r.notice.request(&client) {
                (Pending::DepartureHttp(key.clone(),expect), Ok(request))
            } else { self.departures.records.remove(&key); continue; };
            let id = self.io(pending);
            out.push(match effect { Ok(request) => Effect::Http { id, request }, Err(op) => Effect::Store { id, op } });
        }
    }
}
