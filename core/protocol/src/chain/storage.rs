//! Independently stored chain components. The whole-state codec remains the legacy reader.
use super::{corrupt, Chains, CountEntry, HeadEntry, StartEntry};
use crate::codec::{self, Reader};
use crate::error::Error;
use crate::ids::DeviceId;

type Parts = Vec<(Vec<u8>, Vec<u8>)>;

enum Part {
    Revision(u64),
    Head(HeadEntry),
    Count(CountEntry),
    Start(StartEntry),
}

fn read_part(key: &[u8], value: &[u8]) -> Result<Part, Error> {
    let mut key = Reader::new(key);
    let part = match key.u8()? {
        0 => {
            let mut value = Reader::new(value);
            let revision = value.u64()?;
            value.finish()?;
            Part::Revision(revision)
        }
        1 => {
            let sender: DeviceId = key.value()?;
            let entry: HeadEntry = codec::decode(value, value.len())?;
            if entry.0 != sender { return Err(Error::BadFormat); }
            Part::Head(entry)
        }
        2 => {
            let epoch = key.u64()?;
            let entry: CountEntry = codec::decode(value, value.len())?;
            if entry.0 != epoch { return Err(Error::BadFormat); }
            Part::Count(entry)
        }
        3 => {
            let sender: DeviceId = key.value()?;
            let entry: StartEntry = codec::decode(value, value.len())?;
            if entry.0 != sender { return Err(Error::BadFormat); }
            Part::Start(entry)
        }
        _ => return Err(Error::BadFormat),
    };
    key.finish()?;
    Ok(part)
}

impl Chains {
    pub(crate) fn storage_parts(&self) -> Result<Parts, Error> {
        let mut parts = vec![(vec![0], self.revision.to_be_bytes().to_vec())];
        for (sender, head) in &self.heads {
            let key = [&[1], sender.as_bytes().as_slice()].concat();
            parts.push((key, codec::encode(&HeadEntry(*sender, *head))?));
        }
        for (epoch, count) in &self.counts {
            let key = [&[2], epoch.to_be_bytes().as_slice()].concat();
            parts.push((key, codec::encode(&CountEntry(*epoch, *count))?));
        }
        for (sender, start) in &self.starts {
            let key = [&[3], sender.as_bytes().as_slice()].concat();
            parts.push((key, codec::encode(&StartEntry(*sender, *start))?));
        }
        Ok(parts)
    }

    pub(crate) fn check_storage_part(key: &[u8], value: &[u8]) -> Result<(), Error> {
        read_part(key, value).map(|_| ()).map_err(corrupt)
    }

    pub(crate) fn from_storage_parts(parts: Parts) -> Result<Self, Error> {
        if parts.is_empty() { return Ok(Self::new()); }
        if parts.iter().map(|(_, value)| value.len()).sum::<usize>() > super::MAX_STATE_LEN {
            return Err(corrupt(Error::TooLarge));
        }
        let (mut revision, mut heads, mut counts, mut starts) = (None, Vec::new(), Vec::new(), Vec::new());
        for (key, value) in parts {
            match read_part(&key, &value).map_err(corrupt)? {
                Part::Revision(n) => {
                    if revision.replace(n).is_some() { return Err(corrupt(Error::BadFormat)); }
                }
                Part::Head(entry) => heads.push(entry),
                Part::Count(entry) => counts.push(entry),
                Part::Start(entry) => starts.push(entry),
            }
        }
        Self::from_entries(revision.ok_or_else(|| corrupt(Error::BadFormat))?, heads, counts, starts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::Head;
    use crate::ids::Hash32;
    use std::collections::BTreeMap;

    #[test]
    fn a_chain_step_changes_only_its_head_count_and_revision() {
        let mut chains = Chains::new();
        for n in 1u16..=100 {
            let mut id = [0; 32];
            id.get_mut(..2).unwrap().copy_from_slice(&n.to_be_bytes());
            chains.heads.insert(DeviceId::new(id), Head { seq: 1, hash: Hash32::new(id) });
        }
        chains.counts.insert(0, 100);
        chains.revision = 100;
        let legacy_bytes = chains.to_bytes().unwrap().len();
        let before: BTreeMap<_, _> = chains.storage_parts().unwrap().into_iter().collect();
        let sender = *chains.heads.keys().next().unwrap();
        chains.heads.get_mut(&sender).unwrap().seq = 2;
        chains.counts.insert(0, 101);
        chains.revision = 101;
        let parts = chains.storage_parts().unwrap();
        let changed: Vec<_> = parts.iter().filter(|(key, value)| before.get(key) != Some(value)).collect();
        let delta_bytes: usize = changed.iter().map(|(key, value)| key.len() + value.len()).sum();
        assert_eq!(changed.len(), 3);
        assert!(delta_bytes < 256, "{delta_bytes}");
        assert!(legacy_bytes > 7_000, "{legacy_bytes}");
        assert_eq!(Chains::from_storage_parts(parts).unwrap(), chains);
        println!("chain component bytes: whole {legacy_bytes}, changed {delta_bytes}");
    }

    #[test]
    fn incomplete_or_renamed_chain_components_are_refused() {
        let sender = DeviceId::new([1; 32]);
        let mut chains = Chains::new();
        chains.heads.insert(sender, Head::START);
        let mut parts = chains.storage_parts().unwrap();
        parts.remove(0);
        assert!(Chains::from_storage_parts(parts).is_err());
        let mut parts = chains.storage_parts().unwrap();
        *parts.get_mut(1).unwrap().0.get_mut(1).unwrap() ^= 1;
        assert!(Chains::from_storage_parts(parts).is_err());
    }
}
