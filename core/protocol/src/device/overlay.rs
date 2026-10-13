//! Register values read out of the hub's order (spec 9.3.6): the overlay of a human device.
//!
//! The hub's Desk serves every writer's newest register value of a group before the device has read the group's
//! chains up to it. On a human device such a value is kept in an overlay, under its group, sender and number,
//! and never enters the registers its chains build ([`Registers`]): no names or ids of a writer, no largest
//! lamport, no replay. It is judged against those registers whenever it is looked at, with their largest lamport
//! and their writers' names and ids, and stands for its name only while it is newer, by the name's own order
//! (9.3.2), than the value the chains hold and than every other overlay value of that name. A value the chains
//! hold is reported as current only while no overlay value of its name stands above it, so a value shown never
//! gives way to an older one.
//!
//! An overlay value goes as soon as its sender's chain reaches its number (taken there with that very envelope,
//! refused, voided, or another envelope under that number), and when a Cut ends its sender below it. A record
//! the chain holds without a body takes the overlay's body when the hash is the same: it is that very envelope.
//! When what stands for a name changes with an envelope of the chain (the value that stood went, or no longer
//! passes against the chains' registers), the envelope reports what stands then ([`RegisterShown`]). Agent and
//! helper devices keep no overlay: they take a register only from its chain.
//!
//! Stored under the table of registers, part [`SUB_OVERLAY`]: the record under `0 ‖ sender ‖ seq`, and an
//! empty index entry under `1 ‖ name ‖ of ‖ sender ‖ seq`, so that the values of one name and of one sender are
//! found without reading the others.

use super::content::{EnvelopeOutcome, ReceivedEnvelope, RegisterChange};
use super::facts::{group_key, Facts, Record, Status, SUB_OVERLAY};
use super::Device;
use crate::codec::{self, Reader};
use crate::envelope::{Bind, Body, Header, Subject};
use crate::error::Error;
use crate::ids::{DeviceId, GroupId, Hash32};
use crate::registers::{name_owner, NameOwner, Registers, Stamp, Value};
use crate::store::{table, Batch, Storage};
use std::collections::BTreeMap;

/// A register value read out of the hub's order that stood for its name went, or what stands for the name
/// changed with it (9.3.6): what stands for the name now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisterShown {
    /// The name.
    pub name: String,
    /// The sender, for a name of which each device holds its own value.
    pub of: Option<DeviceId>,
    /// The envelope whose value stands for the name now, with its body and `register.current`: the one its
    /// chain holds (`applied`), or another value read out of the hub's order (`provisional`). None when the
    /// name has no value now.
    pub now: Option<Box<ReceivedEnvelope>>,
}

/// A name of a group, with the sender for a name of which each device holds its own value.
type Name = (String, Option<DeviceId>);

const AT: u8 = 0;
const BY_NAME: u8 = 1;

fn damaged(what: &'static str) -> Error {
    Error::Storage(format!("{what} does not decode"))
}

fn at_prefix(group: &GroupId, sender: Option<&DeviceId>) -> Vec<u8> {
    let sender = sender.map_or(&[][..], |sender| &sender.as_bytes()[..]);
    group_key(
        table::REGISTER,
        SUB_OVERLAY,
        group,
        &[&[AT][..], sender].concat(),
    )
}

fn at_key(group: &GroupId, sender: &DeviceId, seq: u64) -> Vec<u8> {
    [at_prefix(group, Some(sender)), seq.to_be_bytes().to_vec()].concat()
}

fn name_prefix(group: &GroupId, (name, of): &Name) -> Vec<u8> {
    let mut rest = vec![BY_NAME, name.len() as u8];
    rest.extend_from_slice(name.as_bytes());
    match of {
        None => rest.push(0),
        Some(of) => {
            rest.push(1);
            rest.extend_from_slice(of.as_bytes());
        }
    }
    group_key(table::REGISTER, SUB_OVERLAY, group, &rest)
}

fn name_key(group: &GroupId, name: &Name, sender: &DeviceId, seq: u64) -> Vec<u8> {
    [
        name_prefix(group, name),
        sender.as_bytes().to_vec(),
        seq.to_be_bytes().to_vec(),
    ]
    .concat()
}

/// The name a register payload writes, without judging it: none for what is no register value.
fn name_of(header: &Header, payload: &[u8]) -> Option<Name> {
    let name = Value::parse(payload).ok()?.name;
    let each = name_owner(&name, header.group.is_room()) == Some(NameOwner::EachDevice);
    Some((name, each.then_some(header.sender)))
}

/// Checks a stored overlay entry when the device is opened: `rest` is its key after the table and part. A
/// record must be a register value of the group, sender and number its key names; an index entry is empty and
/// names a name, a sender and a number.
pub(super) fn check_entry(rest: &[u8], value: &[u8]) -> Result<(), Error> {
    let mut reader = Reader::new(rest);
    let len = reader.u8()? as usize;
    let group = reader.take(len)?.to_vec();
    let fits = GroupId::from_bytes(&group).is_ok()
        && match reader.u8()? {
        AT => {
            let record = codec::decode::<Record>(value, value.len())?;
            let sender = reader.take(DeviceId::LEN)?;
            let seq = reader.u64()?;
            group == record.header.group.as_bytes()
                && sender == record.header.sender.as_bytes()
                && seq == record.header.seq
                && record.status == Status::Taken
                && record.code.is_none()
                && matches!(record.header.subject, Subject::Register(_))
                && name_of(&record.header, &record.extra).is_some()
        }
        BY_NAME => {
            let len = reader.u8()? as usize;
            let name_fits = std::str::from_utf8(reader.take(len)?).is_ok();
            let of_fits = match reader.u8()? {
                0 => true,
                1 => reader.take(DeviceId::LEN).is_ok(),
                _ => false,
            };
            name_fits
                && of_fits
                && reader.take(DeviceId::LEN).is_ok()
                && reader.u64().is_ok()
                && value.is_empty()
        }
        _ => false,
    };
    if fits && reader.is_empty() {
        Ok(())
    } else {
        Err(Error::BadFormat)
    }
}

fn is_fault(error: &Error) -> bool {
    matches!(error, Error::Storage(_) | Error::Internal(_))
}

/// An overlay value as it stands against the chains' registers now.
struct Judged {
    name: Name,
    stamp: Stamp,
}

/// What stood for some names before an envelope of the chain was taken ([`Device::overlay_watch`]).
pub(super) struct Watched(BTreeMap<Name, Option<Hash32>>);

impl<S: Storage> Device<S> {
    fn overlay_record(
        &self,
        group: &GroupId,
        sender: &DeviceId,
        seq: u64,
    ) -> Result<Option<Record>, Error> {
        self.stored(&at_key(group, sender, seq))
            .map(|value| {
                codec::decode::<Record>(value, value.len()).map_err(|_| damaged("an overlay value"))
            })
            .transpose()
    }

    fn overlay_under(&self, prefix: &[u8]) -> Result<Vec<Record>, Error> {
        self.stored_under(prefix)
            .into_iter()
            .map(|(_, value)| {
                codec::decode::<Record>(&value, value.len())
                    .map_err(|_| damaged("an overlay value"))
            })
            .collect()
    }

    /// Whether the overlay holds a value of a name: read from its index alone.
    fn overlay_has(&self, group: &GroupId, name: &Name) -> bool {
        !self.stored_under(&name_prefix(group, name)).is_empty()
    }

    /// The overlay values of one name.
    fn overlay_named(&self, group: &GroupId, name: &Name) -> Result<Vec<Record>, Error> {
        let prefix = name_prefix(group, name);
        let mut found = Vec::new();
        for (key, _) in self.stored_under(&prefix) {
            let rest = key.get(prefix.len()..).unwrap_or_default();
            let (Some(sender), Some(seq)) = (rest.get(..DeviceId::LEN), rest.get(DeviceId::LEN..))
            else {
                return Err(damaged("an overlay index entry"));
            };
            let sender =
                DeviceId::from_slice(sender).map_err(|_| damaged("an overlay index entry"))?;
            let seq = u64::from_be_bytes(
                seq.try_into()
                    .map_err(|_| damaged("an overlay index entry"))?,
            );
            found.extend(self.overlay_record(group, &sender, seq)?);
        }
        Ok(found)
    }

    fn put_overlay(
        &mut self,
        batch: &mut Batch,
        record: &Record,
        name: &Name,
    ) -> Result<(), Error> {
        let (group, sender, seq) = (record.header.group, record.header.sender, record.header.seq);
        self.put_stored(batch, at_key(&group, &sender, seq), codec::encode(record)?);
        self.put_stored(batch, name_key(&group, name, &sender, seq), Vec::new());
        Ok(())
    }

    fn delete_overlay(&mut self, batch: &mut Batch, record: &Record) {
        let (group, sender, seq) = (record.header.group, record.header.sender, record.header.seq);
        self.delete_stored(batch, at_key(&group, &sender, seq));
        if let Some(name) = name_of(&record.header, &record.extra) {
            self.delete_stored(batch, name_key(&group, &name, &sender, seq));
        }
    }

    /// An overlay value judged against the chains' registers now: none when they do not take it (its writer
    /// may not write the name in its epoch, or uses its register id for another name).
    fn judged(&self, registers: &Registers, record: &Record) -> Result<Option<Judged>, Error> {
        match registers.judge(&Facts(self), &record.header, &record.extra) {
            Ok(update) => Ok(Some(Judged {
                name: (update.name.clone(), update.of),
                stamp: update.stamp(),
            })),
            Err(fault) if is_fault(&fault) => Err(fault),
            Err(_) => Ok(None),
        }
    }

    /// The overlay value that stands for a name now: the one with the highest stamp among those above the
    /// value the chains hold.
    fn overlay_standing(
        &self,
        group: &GroupId,
        registers: &Registers,
        name: &Name,
    ) -> Result<Option<(Record, Stamp)>, Error> {
        let held = registers.held(&name.0, name.1);
        let mut best: Option<(Record, Stamp)> = None;
        for record in self.overlay_named(group, name)? {
            let Some(judged) = self.judged(registers, &record)? else {
                continue;
            };
            if judged.name != *name || held.is_some_and(|held| held >= judged.stamp) {
                continue;
            }
            if best.as_ref().is_none_or(|(_, stamp)| *stamp < judged.stamp) {
                best = Some((record, judged.stamp));
            }
        }
        Ok(best)
    }

    /// A register envelope read out of the hub's order that passed the checks of 9.0.6 (`record`: its hub's
    /// number, hash, header and payload): on a human device it goes into the overlay, unless the chains hold
    /// a value of its name at least as new, or the overlay holds another envelope under its number. What it
    /// is now; none on an agent or helper device and for a value the registers would not take.
    pub(super) fn take_out_of_order(
        &mut self,
        batch: &mut Batch,
        record: Record,
    ) -> Result<Option<RegisterChange>, Error> {
        if !matches!(record.header.subject, Subject::Register(_)) || !self.is_human() {
            return Ok(None);
        }
        let (group, sender, seq) = (record.header.group, record.header.sender, record.header.seq);
        if self
            .overlay_record(&group, &sender, seq)?
            .is_some_and(|held| held.hash != record.hash)
        {
            return Ok(None);
        }
        let registers = self.registers(&group)?;
        let Some(judged) = self.judged(&registers, &record)? else {
            return Ok(None);
        };
        let newer = registers
            .held(&judged.name.0, judged.name.1)
            .is_none_or(|held| held < judged.stamp);
        if newer {
            self.put_overlay(batch, &record, &judged.name)?;
        }
        let current = newer
            && self
                .overlay_standing(&group, &registers, &judged.name)?
                .is_some_and(|(standing, _)| standing.hash == record.hash);
        Ok(Some(RegisterChange {
            name: judged.name.0,
            of: judged.name.1,
            current,
        }))
    }

    /// Before an envelope of the chain is taken: what stands for the names it may change. Those are the name
    /// its body writes, the name of the overlay value under its number, the names of the overlay values of its
    /// sender when it uses a register id for the first time (the chain binds it to a name then), and every
    /// name when the group's registers are built again (`all`). Nothing on an agent or helper device, which
    /// keeps no overlay.
    pub(super) fn overlay_watch(
        &self,
        header: &Header,
        payload: Option<&[u8]>,
        all: bool,
    ) -> Result<Watched, Error> {
        let mut names = BTreeMap::new();
        if !self.is_human() {
            return Ok(Watched(names));
        }
        let group = header.group;
        let mut records = if all {
            self.overlay_under(&at_prefix(&group, None))?
        } else {
            self.overlay_record(&group, &header.sender, header.seq)?
                .into_iter()
                .collect()
        };
        let mut registers = None;
        if let Subject::Register(register) = &header.subject {
            let held = self.registers(&group)?;
            // (a register id its sender uses for the first time binds a name: its other overlay values may no
            //  longer pass)
            if !all && !held.binds(&header.sender, register) {
                records.extend(self.overlay_under(&at_prefix(&group, Some(&header.sender)))?);
            }
            if let Some(name) = payload.and_then(|payload| name_of(header, payload)) {
                names.insert(name, None);
            }
            registers = Some(held);
        }
        for record in &records {
            if let Some(name) = name_of(&record.header, &record.extra) {
                names.insert(name, None);
            }
        }
        names.retain(|name, _| self.overlay_has(&group, name));
        if names.is_empty() {
            return Ok(Watched(names));
        }
        let registers = match registers {
            Some(registers) => registers,
            None => self.registers(&group)?,
        };
        for (name, standing) in names.iter_mut() {
            *standing = self
                .overlay_standing(&group, &registers, name)?
                .map(|(record, _)| record.hash);
        }
        Ok(Watched(names))
    }

    /// The chain reaches number `seq` of `sender`: the overlay value of that number leaves the overlay,
    /// whatever envelope the chain holds there. Returns it.
    pub(super) fn overlay_reached(
        &mut self,
        batch: &mut Batch,
        group: &GroupId,
        sender: &DeviceId,
        seq: u64,
    ) -> Result<Option<Record>, Error> {
        let record = self.overlay_record(group, sender, seq)?;
        if let Some(record) = &record {
            self.delete_overlay(batch, record);
        }
        Ok(record)
    }

    /// A register value the chains hold is reported current only while no overlay value of its name stands
    /// above it.
    pub(super) fn overlay_mask(
        &self,
        group: &GroupId,
        change: Option<RegisterChange>,
    ) -> Result<Option<RegisterChange>, Error> {
        let Some(mut change) = change else {
            return Ok(None);
        };
        let name = (change.name.clone(), change.of);
        if change.current && self.is_human() && self.overlay_has(group, &name) {
            let registers = self.registers(group)?;
            change.current = self
                .overlay_standing(group, &registers, &name)?
                .is_none();
        }
        Ok(Some(change))
    }

    /// After an envelope of the chain was taken (`received`): for each watched name whose standing overlay
    /// value changed, what stands for it now is reported, unless `received` itself is that value.
    pub(super) fn overlay_settle(
        &self,
        group: &GroupId,
        received: &mut ReceivedEnvelope,
        watched: Watched,
    ) -> Result<(), Error> {
        if watched.0.is_empty() {
            return Ok(());
        }
        let registers = self.registers(group)?;
        for (name, before) in watched.0 {
            let after = self
                .overlay_standing(group, &registers, &name)?
                .map(|(record, _)| record.hash);
            let itself = received
                .register
                .as_ref()
                .is_some_and(|taken| taken.current && taken.name == name.0 && taken.of == name.1);
            if after != before && !itself {
                received
                    .shown
                    .push(self.overlay_shown(group, &registers, name)?);
            }
        }
        Ok(())
    }

    /// What stands for a name now: the overlay's value, else the chains' value, else nothing.
    fn overlay_shown(
        &self,
        group: &GroupId,
        registers: &Registers,
        name: Name,
    ) -> Result<RegisterShown, Error> {
        let now = match self.overlay_standing(group, registers, &name)? {
            Some((record, _)) => Some((record, EnvelopeOutcome::Provisional)),
            None => match registers.held(&name.0, name.1) {
                Some(stamp) => self
                    .record(group, &stamp.sender, stamp.seq)?
                    .filter(|record| record.status == Status::Taken && !record.extra.is_empty())
                    .map(|record| (record, EnvelopeOutcome::Applied)),
                None => None,
            },
        };
        let now = match now {
            Some((record, outcome)) => {
                let body = Body::new(Bind::None, &record.extra)
                    .map_err(|_| damaged("a register's record"))?;
                let mut received =
                    ReceivedEnvelope::of(record.change, record.hash, record.header, outcome);
                received.body = Some(body);
                received.register = Some(RegisterChange {
                    name: name.0.clone(),
                    of: name.1,
                    current: true,
                });
                Some(Box::new(received))
            }
            None => None,
        };
        Ok(RegisterShown {
            name: name.0,
            of: name.1,
            now,
        })
    }

    /// A Cut ended `device`'s chain at `seq` with the envelope `hash` (9.0.10): its overlay values beyond go,
    /// and another envelope under the Cut's own number.
    pub(super) fn overlay_cut(
        &mut self,
        batch: &mut Batch,
        group: &GroupId,
        device: &DeviceId,
        seq: u64,
        hash: &Hash32,
    ) -> Result<(), Error> {
        for record in self.overlay_under(&at_prefix(group, Some(device)))? {
            if record.header.seq > seq || (record.header.seq == seq && record.hash != *hash) {
                self.delete_overlay(batch, &record);
            }
        }
        Ok(())
    }
}
