//! A group started from the hub's list (section 9.0.6): a human device that signs in reads what is open and
//! what is new, not the group's past.
//!
//! [`Device::start_from_list`] marks a group of which the device holds no other sender's chain as started from
//! the hub's list at a change number. [`Device::adopt_listed`] takes the newest version of each open object
//! the list names from its own signed header. In the hub's order after that change, the chain of a sender the
//! device holds nothing of begins just before the first envelope of it served ([`Chains::start_at`]): what lies
//! before stays an unverified prefix, never read in order. The newest `heads` (9.0.7) each human device of the
//! person signed and this device read out of order bound where such a chain may begin: below a head they
//! name is `withheld`, at its number with another envelope `equivocation`, both kept as a finding.
//!
//! Agent and helper devices never start a group this way: they keep their full check of every chain from its
//! first envelope ([`Device::is_human`] tells the two apart).

use super::content::ReceivedEnvelope;
use super::facts::{
    floor_key, group_key, listed_key, listed_object_key, Facts, SUB_FINDING, SUB_FLOOR,
};
use super::Device;
use crate::chain::{self, Chains, Head};
use crate::codec::{self, Reader, Writer};
use crate::envelope::{Body, Envelope, Header, Subject};
use crate::error::Error;
use crate::ids::{DeviceId, GroupId, ObjectId};
use crate::objects::{self, ObjectStates};
use crate::registers::{self, Value};
use crate::store::{table, Batch, Storage};

use super::content::EnvelopeOutcome;

fn is_fault(error: &Error) -> bool {
    matches!(
        error,
        Error::Storage(_) | Error::Internal(_) | Error::Entropy
    )
}

/// A stored floor: the number of the `heads` envelope in its writer's chain and its value.
fn read_floor(value: &[u8]) -> Result<(u64, Vec<(DeviceId, Head)>), Error> {
    let mut reader = Reader::new(value);
    let seq = reader.u64()?;
    let text = reader.opaque()?.to_vec();
    reader.finish()?;
    let text = std::str::from_utf8(&text).map_err(|_| Error::BadFormat)?;
    Ok((seq, chain::parse_heads(text)?))
}

impl<S: Storage> Device<S> {
    /// Starts `group` from the hub's list of what is open at change `at_change` (9.0.6): from now on, the
    /// chain of a sender this device holds nothing of begins just before the first envelope of it served
    /// after `at_change` in the hub's order, and an object it does not know is taken from the signed header
    /// that names it ([`Device::adopt_listed`] for the list's own versions). Human devices only (`forbidden`
    /// on an agent or helper device, which reads every chain from its start), and only for a group of which
    /// it holds no other sender's chain (`forbidden` otherwise); again for a group started so, nothing
    /// changes.
    pub fn start_from_list(&mut self, group: &GroupId, at_change: u64) -> Result<(), Error> {
        self.transact(|this, batch| {
            this.begin(0);
            if !this.is_human() {
                return Err(Error::Forbidden);
            }
            this.meta(group)?;
            if this.listed_at(group)?.is_some() {
                return Ok(());
            }
            let me = this.id;
            if this
                .chains(group)?
                .heads()
                .iter()
                .any(|(sender, _)| *sender != me)
            {
                return Err(Error::Forbidden);
            }
            this.put_stored(batch, listed_key(group), at_change.to_be_bytes().to_vec());
            Ok(())
        })
    }

    /// Takes the newest version the hub lists of an open object of `group` (a group started from the list,
    /// [`Device::start_from_list`]; `forbidden` otherwise): `envelope` as served, under the hub's `change`.
    /// The envelope passes checks 1 to 5 and the sender's right to write such an item by its role
    /// ([`chain::listed`]); refused, it is [`EnvelopeOutcome::Refused`] with its code and nothing changes.
    /// An object this device does not know yet is taken from the header (owner, current version, state; 9.2.1)
    /// and marked as taken so; the result is then [`EnvelopeOutcome::Applied`] with its body and
    /// `object_after`, or [`EnvelopeOutcome::Chained`] with why the body did not open. An object it knows is
    /// left as it stands (what came in the hub's order is newer than the list): the result is
    /// [`EnvelopeOutcome::Provisional`]. `bad-format` for an envelope that is not of `group` or not of
    /// `object_id`.
    pub fn adopt_listed(
        &mut self,
        group: &GroupId,
        object_id: &ObjectId,
        envelope: &[u8],
        change: u64,
    ) -> Result<ReceivedEnvelope, Error> {
        self.transact(|this, batch| {
            this.begin(0);
            if !this.is_human() || this.listed_at(group)?.is_none() {
                return Err(Error::Forbidden);
            }
            let decoded = Envelope::decode(envelope)?;
            let fields = decoded.header.subject.object().ok_or(Error::BadFormat)?;
            if decoded.header.group != *group || fields.object_id != *object_id {
                return Err(Error::BadFormat);
            }
            let hash = decoded.hash()?;
            let header = decoded.header.clone();
            let (_, _, body) = match chain::listed(&Facts(this), envelope, 0) {
                Ok(listed) => listed,
                Err(fault) if is_fault(&fault) => return Err(fault),
                Err(code) => {
                    let mut refused =
                        ReceivedEnvelope::of(change, hash, header, EnvelopeOutcome::Refused);
                    refused.code = Some(code);
                    return Ok(refused);
                }
            };
            this.split_objects(batch, group)?;
            let held = this.held_objects(group)?;
            let known = held.object(object_id)?;
            let transition = match known {
                Some(_) => None,
                None => match objects::judge(&Facts(this), &held, &header, &hash) {
                    Ok(transition) => transition,
                    Err(fault) if is_fault(&fault) => return Err(fault),
                    Err(code) => {
                        let mut refused =
                            ReceivedEnvelope::of(change, hash, header, EnvelopeOutcome::Refused);
                        refused.code = Some(code);
                        return Ok(refused);
                    }
                },
            };
            drop(held);
            let Some(transition) = transition else {
                let mut shown =
                    ReceivedEnvelope::of(change, hash, header, EnvelopeOutcome::Provisional);
                match body {
                    Ok(body) => shown.body = Some(body),
                    Err(code) => {
                        shown.outcome = EnvelopeOutcome::Refused;
                        shown.code = Some(code);
                    }
                }
                return Ok(shown);
            };
            this.apply_transition(batch, group, &transition)?;
            this.put_stored(
                batch,
                listed_object_key(group, object_id),
                codec::encode(&transition.after)?,
            );
            let mut taken = ReceivedEnvelope::of(change, hash, header, EnvelopeOutcome::Chained);
            taken.object_after = Some((transition.object_id, transition.after));
            match body {
                Ok(body) => {
                    taken.outcome = EnvelopeOutcome::Applied;
                    taken.body = Some(body);
                }
                Err(code) => taken.code = Some(code),
            }
            Ok(taken)
        })
    }

    /// A `heads` value (9.0.7) read out of order, from a human device of the person, whose body opened: kept
    /// as that writer's floor in `group` if it is the newest of that writer read so far. Read when a chain of
    /// a group started from the list begins ([`Device::start_listed`]).
    pub(super) fn keep_floor(
        &mut self,
        batch: &mut Batch,
        header: &Header,
        body: &Body,
    ) -> Result<(), Error> {
        if !matches!(header.subject, Subject::Register(_)) || !self.is_human() {
            return Ok(());
        }
        let Ok(value) = Value::parse(body.payload()) else {
            return Ok(());
        };
        let Some(text) = value.value.filter(|_| value.name == registers::HEADS) else {
            return Ok(());
        };
        if chain::parse_heads(&text).is_err()
            || !self
                .room_history()
                .is_some_and(|history| history.newest().is_human(&header.sender))
        {
            return Ok(());
        }
        let key = floor_key(&header.group, &header.sender);
        if let Some(stored) = self.stored(&key) {
            let (held, _) = read_floor(stored).map_err(|_| damaged())?;
            if held >= header.seq {
                return Ok(());
            }
        }
        let mut writer = Writer::new();
        writer.u64(header.seq);
        writer.opaque(text.as_bytes())?;
        self.put_stored(batch, key, writer.into_bytes());
        Ok(())
    }

    /// Of a group started from the list, for an envelope above the cursor of a sender whose chain this device
    /// holds nothing of, served after the list: `chains` with that chain begun just before it (its `prev`
    /// under the number before), or unchanged. `Err(replay)` for an envelope at or below the number a chain
    /// of such a group began at: that prefix is never read in order.
    pub(super) fn start_listed(
        &self,
        header: &Header,
        change: u64,
        chains: &mut Chains,
    ) -> Result<Result<bool, Error>, Error> {
        let Some(at) = self.listed_at(&header.group)? else {
            return Ok(Ok(false));
        };
        if let Some(began) = chains.started_at(&header.sender) {
            if header.seq <= began.seq {
                return Ok(Err(Error::Replay));
            }
            return Ok(Ok(false));
        }
        if change <= at || header.seq <= 1 || chains.head(&header.sender) != Head::START {
            return Ok(Ok(false));
        }
        chains.start_at(
            header.sender,
            Head {
                seq: header.seq - 1,
                hash: header.prev,
            },
        )?;
        Ok(Ok(true))
    }

    /// The finding, if any, for a chain of a group started from the list that began at `start`: `withheld`
    /// when a floor names a later envelope of that sender, `equivocation` when it names another one under
    /// the same number. The envelope it began before passed its checks: the start is the signed `prev`.
    pub(super) fn below_floor(
        &self,
        group: &GroupId,
        sender: &DeviceId,
        start: Head,
    ) -> Result<Option<Error>, Error> {
        let prefix = group_key(table::CHAIN, SUB_FLOOR, group, &[]);
        let mut finding = None;
        for (_, value) in self.stored_under(&prefix) {
            let (_, heads) = read_floor(&value).map_err(|_| damaged())?;
            for (named, head) in heads {
                if named != *sender {
                    continue;
                }
                if start.seq < head.seq {
                    finding = Some(Error::Withheld);
                } else if start.seq == head.seq && start.hash != head.hash {
                    return Ok(Some(Error::Equivocation));
                }
            }
        }
        Ok(finding)
    }

    /// Keeps `code` as the finding about `sender`'s chain in `group` (section 16: never swallowed).
    pub(super) fn put_finding(
        &mut self,
        batch: &mut Batch,
        group: &GroupId,
        sender: &DeviceId,
        code: &Error,
    ) {
        self.put_stored(
            batch,
            group_key(table::CHAIN, SUB_FINDING, group, sender.as_bytes()),
            code.code().as_bytes().to_vec(),
        );
    }
}

fn damaged() -> Error {
    Error::Storage("stored content state is damaged: a floor".into())
}
