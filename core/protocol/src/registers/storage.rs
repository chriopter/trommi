//! Independent register records. Keys use the writer's random register id, never the private name.
use super::{corrupt, read_held, read_text, write_held, Held, OwnIds, Registers};
use crate::codec::{Reader, Writer};
use crate::error::Error;
use crate::ids::{DeviceId, RegisterId};
use std::collections::BTreeSet;

type Parts = Vec<(Vec<u8>, Vec<u8>)>;

enum Part {
    Clock(u64, u64),
    Value(RegisterId, String, Option<DeviceId>, Held),
    Name(DeviceId, RegisterId, String),
}

fn read_part(key: &[u8], value: &[u8]) -> Result<Part, Error> {
    let (mut key, mut value) = (Reader::new(key), Reader::new(value));
    let part = match key.u8()? {
        0 => Part::Clock(value.u64()?, value.u64()?),
        1 => {
            let sender: DeviceId = key.value()?;
            let id: RegisterId = key.value()?;
            let named: RegisterId = value.value()?;
            let (name, of, held) = read_held(&mut value)?;
            if named != id || held.stamp.sender != sender { return Err(Error::BadFormat); }
            Part::Value(id, name, of, held)
        }
        2 => Part::Name(key.value()?, key.value()?, read_text(&mut value)?),
        _ => return Err(Error::BadFormat),
    };
    key.finish()?;
    value.finish()?;
    Ok(part)
}

impl Registers {
    pub(crate) fn storage_parts(&self) -> Result<Parts, Error> {
        let mut clock = Writer::new();
        clock.u64(self.revision);
        clock.u64(self.largest);
        let mut parts = vec![(vec![0], clock.into_bytes())];
        let mut keys = BTreeSet::new();
        for ((name, of), held) in &self.values {
            let id = self.ids.get(&(held.stamp.sender, name.clone())).ok_or_else(|| corrupt(Error::BadFormat))?;
            let key = [&[1], held.stamp.sender.as_bytes().as_slice(), id.as_bytes().as_slice()].concat();
            if !keys.insert(key.clone()) { return Err(corrupt(Error::BadFormat)); }
            let mut value = Writer::new();
            value.fixed(id.as_bytes());
            write_held(&mut value, name, *of, held)?;
            parts.push((key, value.into_bytes()));
        }
        for ((sender, id), name) in &self.names {
            let key = [&[2], sender.as_bytes().as_slice(), id.as_bytes().as_slice()].concat();
            let mut value = Writer::new();
            value.opaque(name.as_bytes())?;
            parts.push((key, value.into_bytes()));
        }
        Ok(parts)
    }

    pub(crate) fn check_storage_part(key: &[u8], value: &[u8]) -> Result<(), Error> {
        read_part(key, value).map(|_| ()).map_err(corrupt)
    }

    pub(crate) fn from_storage_parts(parts: Parts) -> Result<Self, Error> {
        if parts.is_empty() { return Ok(Self::new()); }
        if parts.iter().map(|(_, value)| value.len()).sum::<usize>() > super::MAX_STATE_LEN { return Err(corrupt(Error::TooLarge)); }
        let (mut registers, mut clock, mut ids) = (Self::new(), false, Vec::new());
        for (key, value) in parts {
            match read_part(&key, &value).map_err(corrupt)? {
                Part::Clock(revision, largest) => {
                    if clock { return Err(corrupt(Error::BadFormat)); }
                    clock = true;
                    registers.revision = revision;
                    registers.largest = largest;
                }
                Part::Value(id, name, of, held) => {
                    ids.push((held.stamp.sender, name.clone(), id));
                    if registers.values.insert((name, of), held).is_some() { return Err(corrupt(Error::BadFormat)); }
                }
                Part::Name(sender, id, name) => {
                    if registers.names.insert((sender, id), name.clone()).is_some()
                        || registers.ids.insert((sender, name), id).is_some() { return Err(corrupt(Error::BadFormat)); }
                }
            }
        }
        if !clock || ids.iter().any(|(sender, name, id)| registers.ids.get(&(*sender, name.clone())) != Some(id)) {
            return Err(corrupt(Error::BadFormat));
        }
        Ok(registers)
    }
}

fn read_own(key: &[u8], value: &[u8]) -> Result<(String, RegisterId), Error> {
    let id = RegisterId::from_slice(key)?;
    let mut value = Reader::new(value);
    let name = read_text(&mut value)?;
    let named: RegisterId = value.value()?;
    value.finish()?;
    if id != named { return Err(Error::BadFormat); }
    Ok((name, id))
}

impl OwnIds {
    pub(crate) fn storage_parts(&self) -> Result<Parts, Error> {
        let mut parts = Vec::new();
        let mut ids = BTreeSet::new();
        for (name, id) in &self.ids {
            if !ids.insert(*id) { return Err(corrupt(Error::BadFormat)); }
            let mut value = Writer::new();
            value.opaque(name.as_bytes())?;
            value.fixed(id.as_bytes());
            parts.push((id.as_bytes().to_vec(), value.into_bytes()));
        }
        Ok(parts)
    }

    pub(crate) fn check_storage_part(key: &[u8], value: &[u8]) -> Result<(), Error> {
        read_own(key, value).map(|_| ()).map_err(corrupt)
    }

    pub(crate) fn from_storage_parts(parts: Parts) -> Result<Self, Error> {
        let mut own = Self::new();
        let mut ids = BTreeSet::new();
        for (key, value) in parts {
            let (name, id) = read_own(&key, &value).map_err(corrupt)?;
            if !ids.insert(id) || own.ids.insert(name, id).is_some() { return Err(corrupt(Error::BadFormat)); }
        }
        Ok(own)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registers::Stamp;

    #[test]
    fn split_registers_keep_the_legacy_values_clock_and_name_bindings() {
        let sender = DeviceId::new([9; 32]);
        let mut registers = Registers::new();
        for n in 1u8..=64 {
            let id = RegisterId::new([n; 16]);
            let name = format!("star/{n:032x}");
            registers.names.insert((sender, id), name.clone());
            registers.ids.insert((sender, name.clone()), id);
            registers.values.insert((name, None), Held {
                stamp: Stamp { lamport: u64::from(n), sender, seq: u64::from(n) },
                value: Some(r#"{"at":1}"#.into()),
            });
        }
        registers.revision = 64;
        registers.largest = 64;
        let parts = registers.storage_parts().unwrap();
        assert_eq!(Registers::from_storage_parts(parts.clone()).unwrap().to_bytes().unwrap(), registers.to_bytes().unwrap());
        assert!(parts.iter().all(|(key, _)| !key.windows(5).any(|bytes| bytes == b"star/")));
        let missing_clock = parts.iter().filter(|(key, _)| key != &[0]).cloned().collect();
        assert!(Registers::from_storage_parts(missing_clock).is_err());
        let missing_bindings = parts.into_iter().filter(|(key, _)| key.first() != Some(&2)).collect();
        assert!(Registers::from_storage_parts(missing_bindings).is_err());
    }

    #[test]
    fn own_register_ids_roundtrip_without_private_names_in_keys() {
        let mut own = OwnIds::new();
        own.ids.insert("private preference".into(), RegisterId::new([1; 16]));
        own.ids.insert("another preference".into(), RegisterId::new([2; 16]));
        let mut parts = own.storage_parts().unwrap();
        assert_eq!(OwnIds::from_storage_parts(parts.clone()).unwrap(), own);
        *parts.first_mut().unwrap().0.first_mut().unwrap() ^= 1;
        assert!(OwnIds::from_storage_parts(parts).is_err());
    }
}
