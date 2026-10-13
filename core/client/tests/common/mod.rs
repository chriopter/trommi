//! A room for the engine's tests: real devices of the protocol core around the stand-in hub (`hub.rs`), and the
//! App under test as one more human device of it, run by the `Sim`.
//!
//! In the room: `founder` (a human device, which founded it), `agent` (an agent device in its main session), and
//! the App's own device (`me`: a second human device, in the session from its founding). The App's device state is
//! a real stored state: everything up to the hub's newest change is processed, as after an earlier run.
//!
//! The clock is the system's at the start: OpenMLS checks KeyPackage lifetimes against it.

#![allow(dead_code)]

pub mod hub;

use hub::{Hub, HUB_URL};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};
use trommi_client_core::app::io::StreamItem;
use trommi_client_core::app::sim::Sim;
use trommi_client_core::app::{Config, Event};
use trommi_core::crypto::{Secret, SecretBytes, SystemEntropy};
use trommi_core::device::{Accepted, Device, Draft, LogEntry, LogKind, WelcomeExpectation};
use trommi_core::envelope::Urgency;
use trommi_core::hub_auth::HubAddress;
use trommi_core::ids::{DeviceId, GroupId, SessionId};
use trommi_core::invite::{Offer, Role};
use trommi_core::recovery::RecoveryKeys;
use trommi_core::store::{Batch, Loaded, Storage, StorageError};
use trommi_core::Error;

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// A store in memory that a test can look into and copy.
#[derive(Clone, Default)]
pub struct TestStore(pub Rc<RefCell<Stored>>);

/// A revision and the entries under it.
pub type Stored = (u64, BTreeMap<Vec<u8>, Vec<u8>>);

impl Storage for TestStore {
    fn load(&mut self) -> Result<Loaded, StorageError> {
        let inner = self.0.borrow();
        Ok(Loaded { revision: inner.0, entries: inner.1.iter().map(|(k, v)| trommi_core::store::Entry::new(k.clone(), v.clone())).collect() })
    }
    fn apply(&mut self, expected_revision: u64, batch: Batch) -> Result<(), StorageError> {
        let mut inner = self.0.borrow_mut();
        if inner.0 != expected_revision {
            return Err(StorageError::Conflict);
        }
        for key in &batch.delete {
            inner.1.remove(key);
        }
        for entry in &batch.put {
            inner.1.insert(entry.key.clone(), entry.value.clone());
        }
        inner.0 += 1;
        Ok(())
    }
}

pub type Peer = Device<TestStore>;

pub fn new_peer() -> (Peer, TestStore) {
    let store = TestStore::default();
    (Device::create(store.clone(), Box::new(SystemEntropy)).expect("a new device"), store)
}

/// Processes the hub's order after the device's cursor, taking this device's Welcomes on the way.
pub fn sync(hub: &Hub, device: &mut Peer) {
    let me = device.id();
    let room = hub.room_id();
    for item in hub.order.iter().filter(|i| i.change() > device.cursor()).cloned().collect::<Vec<_>>() {
        match &item {
            hub::Item::Log { change, group, commit, bytes, recovery_auth, .. } => {
                let entry = LogEntry {
                    change: *change,
                    group: *group,
                    kind: if *commit { LogKind::Commit { bytes, recovery_auth: recovery_auth.as_deref() } } else { LogKind::Message { bytes } },
                };
                let _ = device.process_log_entry(&entry, now());
                if *commit {
                    let mut joined = false;
                    for w in hub.welcomes.iter().filter(|w| w.device == me && w.group == *group) {
                        if device.group(&w.group).is_err() {
                            joined |= device.join_welcome(&w.bytes, &WelcomeExpectation { room, committer: None }, now()).is_ok();
                        }
                    }
                    if joined {
                        let _ = device.process_log_entry(&entry, now());
                    }
                }
            }
            hub::Item::Envelope { change, bytes, .. } => {
                let _ = device.receive_envelope(bytes, *change, true, None, now());
            }
        }
    }
}

/// Posts the device's outbox, in order, and takes the hub's order after each answer. Returns the refusals.
pub fn post_all(hub: &mut Hub, device: &mut Peer) -> Vec<String> {
    let mut refused = Vec::new();
    for _ in 0..8 {
        let outbox = device.outbox();
        if outbox.is_empty() {
            break;
        }
        for entry in outbox {
            if !device.outbox().iter().any(|held| held.id == entry.id) {
                continue;
            }
            match hub.post(&device.id(), &entry) {
                Ok(v) => {
                    let _ = device.outbox_accepted(entry.id, Accepted { change: v.get("change").and_then(|c| c.as_u64()) });
                    sync(hub, device);
                }
                Err(no) => {
                    let _ = device.outbox_refused(entry.id, &Error::from_code(&no.code).unwrap_or(Error::BadFormat));
                    refused.push(no.code);
                }
            }
        }
    }
    refused
}

fn invite(inviter: &mut Peer, joiner: &mut Peer, role: Role) -> u64 {
    let address = HubAddress::parse(HUB_URL).expect("an address");
    let opened = inviter.invite_open(role, None, "https://app.example", &address, now()).expect("an invite");
    let link = String::from_utf8(opened.link.expose().to_vec()).expect("the link is text");
    let request = joiner.join_request(&link, &opened.signed_offer, now()).expect("a request").signed_request;
    let accepted = inviter.invite_accept(&opened.invite_id, &request, now()).expect("accepted");
    let code = joiner.join_reveal(&accepted.signed_reveal).expect("the code");
    inviter.invite_confirm(&opened.invite_id, &code, &accepted.request_hash, true, now()).expect("confirmed").expect("a Commit");
    Offer::decode(&opened.signed_offer.offer).expect("an offer").room_epoch
}

pub fn text(json: &str) -> SecretBytes {
    SecretBytes::new(json.as_bytes().to_vec())
}

pub struct World {
    pub hub: Rc<RefCell<Hub>>,
    pub founder: Peer,
    pub agent: Peer,
    pub room: GroupId,
    pub session: SessionId,
    pub group: GroupId,
    pub me: DeviceId,
    /// The App's device state: copied into a `Sim` (`sim`), and read back from one (`reopen_me`).
    pub me_store: TestStore,
    cards: u32,
    ticks: u64,
}

impl World {
    pub fn new() -> World {
        let mut hub = Hub::default();
        let (mut founder, _) = new_peer();
        let (mut me, me_store) = new_peer();
        let (mut agent, _) = new_peer();
        let keys = RecoveryKeys::from_code(Secret::new([0xC0; 32])).expect("the keys of the code");
        let room = GroupId::room(founder.found_room(&keys, now()).expect("the room is founded"));
        assert!(post_all(&mut hub, &mut founder).is_empty());
        // a second human device: the one the App runs
        invite(&mut founder, &mut me, Role::Human);
        assert!(post_all(&mut hub, &mut founder).is_empty());
        let welcome = hub.welcomes.last().expect("a Welcome").bytes.clone();
        me.join_invited(&welcome, now()).expect("the App's device joins the room");
        sync(&hub, &mut me);
        me.key_packages_to_upload(0, now()).expect("key packages");
        assert!(post_all(&mut hub, &mut me).is_empty());
        // an agent device, and its main session
        agent.key_packages_to_upload(0, now()).expect("key packages");
        assert!(post_all(&mut hub, &mut agent).is_empty());
        let epoch = invite(&mut founder, &mut agent, Role::Agent);
        assert!(post_all(&mut hub, &mut founder).is_empty());
        let info = hub.group_infos.get(&(room, epoch)).expect("the GroupInfo of the Offer's epoch").clone();
        agent.join_observe(&info).expect("the room is followed");
        sync(&hub, &mut agent);
        let packages = hub.claim(&[me.id(), agent.id()]).expect("a KeyPackage of each");
        let session = founder.found_session(&agent.id(), &packages, now()).expect("the founding is built");
        assert!(post_all(&mut hub, &mut founder).is_empty());
        for device in [&mut founder, &mut agent, &mut me] {
            sync(&hub, device);
            post_all(&mut hub, device);
        }
        for device in [&mut founder, &mut agent, &mut me] {
            sync(&hub, device);
        }
        let group = GroupId::session(room.room_id(), session);
        assert!(me.group(&group).is_ok(), "the App's device is in the session");
        assert!(agent.group(&group).is_ok(), "the agent is in its session");
        let id = me.id();
        drop(me);
        World { hub: Rc::new(RefCell::new(hub)), founder, agent, room, session, group, me: id, me_store, cards: 0, ticks: 0 }
    }

    /// A Sim whose device store holds the App's device as it stands, whose hub is the stand-in.
    pub fn sim(&self) -> Sim {
        let mut sim = Sim::new(now());
        let stored = self.me_store.0.borrow();
        sim.device = (stored.0, stored.1.clone());
        self.attach(&mut sim);
        sim
    }

    pub fn attach(&self, sim: &mut Sim) {
        let hub = self.hub.clone();
        sim.hub = Some(Box::new(move |now, request| hub.borrow_mut().http(now, request)));
        let hub = self.hub.clone();
        sim.stream_hub = Some(Box::new(move |now, request| hub.borrow_mut().open_stream(now, request)));
    }

    /// The App's device opened from what a Sim stored: what a restart would find.
    pub fn reopen_me(&self, sim: &Sim) -> Peer {
        let store = TestStore(Rc::new(RefCell::new((sim.device.0, sim.device.1.clone()))));
        Device::open(store, Box::new(SystemEntropy)).expect("the stored device opens")
    }

    /// The agent says a line in its session's Chat. Returns the change the hub gave it.
    pub fn agent_says(&mut self, line: &str) -> u64 {
        let draft = Draft::SessionChat { session: self.session, payload: text(&format!(r#"{{"content_type":"message","text":"{line}"}}"#)) };
        self.write(false, &draft)
    }

    /// The agent asks a Decision card.
    pub fn agent_asks(&mut self, title: &str) -> u64 {
        self.cards += 1;
        let payload = format!(
            r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}}"#
        );
        let draft = Draft::CardFirst { session: self.session, urgency: Urgency::Normal, push: false, payload: text(&payload) };
        self.write(false, &draft)
    }

    /// The agent asks a Decision card. Returns the card, the hash of this version (as a later version names it) and
    /// the change the hub gave it.
    pub fn agent_card(&mut self, title: &str) -> (trommi_core::ids::ObjectId, String, u64) {
        let payload = format!(
            r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":1,"previous_version_hash":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"}}"#
        );
        let draft = Draft::CardFirst { session: self.session, urgency: Urgency::Normal, push: false, payload: text(&payload) };
        let (sealed, change) = self.seal(false, &draft);
        (sealed.object_id.expect("a card"), trommi_core::ids::base64url_encode(sealed.envelope_hash.as_bytes()), change)
    }

    /// The agent writes a later version of a card. Returns that version's hash and its change.
    pub fn agent_revises(&mut self, card: trommi_core::ids::ObjectId, previous: &str, title: &str, version: u32) -> (String, u64) {
        let payload = format!(
            r#"{{"card_type":"decision","title":"{title}","options":[{{"key":"yes"}},{{"key":"no"}}],"object_version":{version},"previous_version_hash":"{previous}"}}"#
        );
        let draft = Draft::CardVersion { session: self.session, object_id: card, closed: false, urgency: Urgency::Normal, push: false, payload: text(&payload) };
        let (sealed, change) = self.seal(false, &draft);
        (trommi_core::ids::base64url_encode(sealed.envelope_hash.as_bytes()), change)
    }

    /// The founder (the person's other device) says a line.
    pub fn founder_says(&mut self, line: &str) -> u64 {
        let draft = Draft::SessionChat { session: self.session, payload: text(&format!(r#"{{"content_type":"message","text":"{line}"}}"#)) };
        self.write(true, &draft)
    }

    fn write(&mut self, founder: bool, draft: &Draft) -> u64 {
        self.seal(founder, draft).1
    }

    /// Seals and posts. Every write is a little later by the writer's clock than the one before.
    fn seal(&mut self, founder: bool, draft: &Draft) -> (trommi_core::device::Sealed, u64) {
        self.ticks += 10;
        let mut hub = self.hub.borrow_mut();
        let device = if founder { &mut self.founder } else { &mut self.agent };
        sync(&hub, device);
        let sealed = device.seal(draft, None, &[], now() + self.ticks).expect("the item seals");
        assert_eq!(post_all(&mut hub, device), Vec::<String>::new());
        (sealed, hub.change)
    }

    /// The peers take the hub's order up to its end.
    pub fn peers_sync(&mut self) {
        let hub = self.hub.borrow();
        sync(&hub, &mut self.founder);
        sync(&hub, &mut self.agent);
    }

    /// Sends the open stream of the App what the hub's order holds beyond what it was sent. False: nothing new,
    /// or no stream.
    pub fn stream(&self, sim: &mut Sim) -> bool {
        let news = self.hub.borrow_mut().stream_news(&self.me);
        match news {
            Some(bytes) => sim.stream(StreamItem::Chunk { bytes: bytes.into() }),
            None => false,
        }
    }
}

pub fn start() -> Event {
    Event::Start { config: Config { platform: "test".into(), version: "0".into(), hub: HUB_URL.into(), seed: 7, ..Default::default() } }
}
