//! Byte layout of a stored [`Entry`]:
//! `magic "MMCH" · version u8 · revision u64 · generation u64 · lease ·
//! fence · record flag u8 · (record length u32 · record)?`, where a holder is
//! `flag u8 · (zone u32 · epoch u64)?`. Big-endian, strict, fail-closed.

use mmorpg_core::{CharacterId, ZoneId};
use mmorpg_protocol::{decode_character_record, encode_character_record};

use crate::{Entry, LeaseHolder, StoreError};

pub const ENTRY_FORMAT_VERSION: u8 = 1;
const MAGIC: &[u8; 4] = b"MMCH";
/// Far above any record; bounds what a corrupt length can make us read.
const MAX_RECORD_BYTES: usize = 64 * 1024;

pub fn encode_entry(entry: &Entry) -> Result<Vec<u8>, StoreError> {
    let mut bytes = Vec::with_capacity(512);
    bytes.extend_from_slice(MAGIC);
    bytes.push(ENTRY_FORMAT_VERSION);
    bytes.extend_from_slice(&entry.revision.to_be_bytes());
    bytes.extend_from_slice(&entry.generation.to_be_bytes());
    for holder in [entry.lease, entry.fence] {
        match holder {
            None => bytes.push(0),
            Some(holder) => {
                bytes.push(1);
                bytes.extend_from_slice(&holder.zone_id.get().to_be_bytes());
                bytes.extend_from_slice(&holder.zone_epoch.to_be_bytes());
            }
        }
    }
    match &entry.record {
        None => bytes.push(0),
        Some(record) => {
            let record = encode_character_record(record)
                .map_err(|error| StoreError::Corrupt(error.to_string()))?;
            let length = u32::try_from(record.len())
                .ok()
                .filter(|_| record.len() <= MAX_RECORD_BYTES)
                .ok_or_else(|| StoreError::Corrupt("character record is too large".into()))?;
            bytes.push(1);
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend_from_slice(&record);
        }
    }
    Ok(bytes)
}

/// Decodes the entry stored under `id`; a record of another character is corrupt.
pub fn decode_entry(id: &CharacterId, bytes: &[u8]) -> Result<Entry, StoreError> {
    let mut reader = Reader { bytes, offset: 0 };
    if reader.take(4)? != MAGIC {
        return Err(corrupt("not a character entry"));
    }
    if reader.u8()? != ENTRY_FORMAT_VERSION {
        return Err(corrupt("unsupported character entry version"));
    }
    let revision = reader.u64()?;
    let generation = reader.u64()?;
    let lease = reader.holder()?;
    let fence = reader.holder()?;
    let record = match reader.u8()? {
        0 => None,
        1 => {
            let length = usize::try_from(reader.u32()?).unwrap_or(usize::MAX);
            if length > MAX_RECORD_BYTES {
                return Err(corrupt("character record is too large"));
            }
            let record = decode_character_record(reader.take(length)?)
                .map_err(|error| StoreError::Corrupt(error.to_string()))?;
            Some(record)
        }
        _ => return Err(corrupt("record flag must be 0 or 1")),
    };
    if reader.offset != bytes.len() {
        return Err(corrupt("character entry has trailing bytes"));
    }
    let entry = Entry {
        revision,
        generation,
        lease,
        fence,
        record,
    };
    let consistent = entry
        .record
        .as_ref()
        .is_none_or(|record| &record.character_id == id)
        && (entry.revision == 0) == entry.record.is_none()
        && (entry.generation == 0) == entry.fence.is_none()
        && entry.lease.is_none_or(|lease| Some(lease) == entry.fence);
    if consistent {
        Ok(entry)
    } else {
        Err(corrupt("character entry is inconsistent"))
    }
}

fn corrupt(message: &str) -> StoreError {
    StoreError::Corrupt(message.to_owned())
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], StoreError> {
        let end = self
            .offset
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| corrupt("character entry is truncated"))?;
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StoreError> {
        self.take(N)?
            .try_into()
            .map_err(|_| corrupt("character entry is truncated"))
    }

    fn u8(&mut self) -> Result<u8, StoreError> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> Result<u32, StoreError> {
        Ok(u32::from_be_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn holder(&mut self) -> Result<Option<LeaseHolder>, StoreError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(LeaseHolder {
                zone_id: ZoneId::new(self.u32()?),
                zone_epoch: self.u64()?,
            })),
            _ => Err(corrupt("holder flag must be 0 or 1")),
        }
    }
}
