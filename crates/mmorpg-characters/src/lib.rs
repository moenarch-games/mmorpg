#![forbid(unsafe_code)]

//! The global character store (#30, TECH-002): durable character records and
//! the exclusive lease that lets exactly one zone mutate a character live.
//!
//! Offline, the store owns a character's committed record. A zone that
//! admits the character first [acquires](CharacterStore::acquire) its lease
//! as a [`LeaseHolder`] — the zone and its control-plane lease epoch — and
//! becomes the only writer: it [commits](CharacterStore::commit) progress and
//! finally [releases](CharacterStore::release) the character back to the
//! store. Every write names the lease it holds and fails closed once that
//! lease is no longer current.
//!
//! A held lease blocks every other zone. Only a newer epoch of the **same**
//! zone may supersede it: the control plane has already fenced that zone's
//! older host, which may have crashed while holding the character. Once a
//! zone epoch has held a character, older epochs of that zone can never
//! acquire it again.
//!
//! [Committed reads](CharacterStore::committed) return the last committed
//! record without asking any zone; they never include uncommitted live
//! progress. The store performs I/O and is never called inside a zone tick.
//!
//! The rules live once, in [`CharacterStore`], over an [`EntryBackend`] that
//! only needs an atomic read-modify-write of one entry: [`FileEntries`] for
//! development and LAN hosts, the browser's `localStorage` in `mmorpg-wasm`,
//! and [`PostgresEntries`] (feature `postgres`) for production (TECH-002).

mod entry;
#[cfg(not(target_arch = "wasm32"))]
mod file;
#[cfg(feature = "postgres")]
pub mod postgres;

use std::error::Error;
use std::fmt;

use mmorpg_core::{CharacterId, CharacterRecord, ZoneId};

pub use entry::{ENTRY_FORMAT_VERSION, decode_entry, encode_entry};
#[cfg(not(target_arch = "wasm32"))]
pub use file::FileEntries;
#[cfg(feature = "postgres")]
pub use postgres::PostgresEntries;

/// A zone at one control-plane lease epoch: the only party that may hold a
/// character lease.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LeaseHolder {
    pub zone_id: ZoneId,
    pub zone_epoch: u64,
}

/// Proof of the exclusive right to mutate one character. Its generation is
/// unique per acquisition, so a superseded holder's lease never matches again.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CharacterLease {
    pub character_id: CharacterId,
    pub holder: LeaseHolder,
    pub generation: u64,
}

/// A successful acquisition: the lease, the committed revision and record.
/// A character that has never been committed has revision 0 and no record;
/// the zone creates it and its first commit stores it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acquired {
    pub lease: CharacterLease,
    pub revision: u64,
    pub record: Option<CharacterRecord>,
}

/// The last committed record of a character.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Committed {
    pub revision: u64,
    pub record: CharacterRecord,
    /// The zone currently holding the character live, if any.
    pub leased_by: Option<LeaseHolder>,
}

/// The persisted state of one character: everything the rules need.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Entry {
    /// Increments with every committed record.
    pub revision: u64,
    /// Increments with every acquisition.
    pub generation: u64,
    pub lease: Option<LeaseHolder>,
    /// The newest holder ever granted; older epochs of its zone are fenced.
    pub fence: Option<LeaseHolder>,
    pub record: Option<CharacterRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    /// Another zone, or a not-newer epoch of the same zone, holds the character.
    Leased(LeaseHolder),
    /// The holder's zone has granted the character to a newer epoch.
    FencedEpoch(LeaseHolder),
    /// The lease is no longer the character's current lease.
    StaleLease,
    /// The record names another character than the lease.
    WrongCharacter,
    /// An atomic composition was prepared from another committed revision.
    RevisionConflict { expected: u64, actual: u64 },
    /// Persisted bytes are unreadable, of another version or inconsistent.
    Corrupt(String),
    /// The backend failed; nothing is known to have changed.
    Backend(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Leased(holder) => write!(
                formatter,
                "the character is in play in zone {} (epoch {})",
                holder.zone_id.get(),
                holder.zone_epoch
            ),
            Self::FencedEpoch(holder) => write!(
                formatter,
                "zone {} has granted the character to newer epoch {}",
                holder.zone_id.get(),
                holder.zone_epoch
            ),
            Self::StaleLease => formatter.write_str("the character lease is no longer current"),
            Self::WrongCharacter => formatter.write_str("the record belongs to another character"),
            Self::RevisionConflict { expected, actual } => write!(
                formatter,
                "character revision changed: expected {expected}, found {actual}"
            ),
            Self::Corrupt(message) => write!(formatter, "stored character is corrupt: {message}"),
            Self::Backend(message) => write!(formatter, "character storage failed: {message}"),
        }
    }
}

impl Error for StoreError {}

/// Derives the next entry bytes from the current ones, if any.
pub type EntryChange<'a> = dyn FnMut(Option<&[u8]>) -> Result<Vec<u8>, StoreError> + 'a;

/// Storage of encoded [`Entry`] bytes by character.
pub trait EntryBackend {
    /// The current bytes, or `None` for a character never stored.
    fn read(&self, id: &CharacterId) -> Result<Option<Vec<u8>>, StoreError>;

    /// Atomically replaces the entry with what `change` derives from the
    /// current bytes. Concurrent updates of one character serialize; a
    /// failing `change` or write leaves the stored entry unchanged.
    fn update(&mut self, id: &CharacterId, change: &mut EntryChange<'_>) -> Result<(), StoreError>;
}

pub struct CharacterStore<B> {
    backend: B,
}

impl<B: EntryBackend> CharacterStore<B> {
    pub const fn new(backend: B) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    /// Grants `holder` the exclusive lease of `id`; see the crate docs for
    /// who may supersede a held lease.
    pub fn acquire(
        &mut self,
        id: &CharacterId,
        holder: LeaseHolder,
    ) -> Result<Acquired, StoreError> {
        let mut acquired = None;
        self.transition(id, &mut |entry| {
            if let Some(fence) = entry.fence
                && fence.zone_id == holder.zone_id
                && fence.zone_epoch > holder.zone_epoch
            {
                return Err(StoreError::FencedEpoch(fence));
            }
            if let Some(held) = entry.lease
                && !(held.zone_id == holder.zone_id && holder.zone_epoch > held.zone_epoch)
            {
                return Err(StoreError::Leased(held));
            }
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or_else(|| StoreError::Corrupt("lease generations are exhausted".into()))?;
            entry.lease = Some(holder);
            entry.fence = Some(holder);
            acquired = Some(Acquired {
                lease: CharacterLease {
                    character_id: id.clone(),
                    holder,
                    generation: entry.generation,
                },
                revision: entry.revision,
                record: entry.record.clone(),
            });
            Ok(())
        })?;
        acquired.ok_or_else(|| StoreError::Backend("the backend skipped the update".into()))
    }

    /// Stores `record` as the character's new committed revision and returns it.
    pub fn commit(
        &mut self,
        lease: &CharacterLease,
        record: &CharacterRecord,
    ) -> Result<u64, StoreError> {
        self.write(lease, Some(record), false, None)
    }

    /// Commits only if the leased record is still at `expected_revision`.
    /// Used when the character mutation and its journal/grant IDs share a transaction.
    pub fn commit_at_revision(
        &mut self,
        lease: &CharacterLease,
        expected_revision: u64,
        record: &CharacterRecord,
    ) -> Result<u64, StoreError> {
        self.write(lease, Some(record), false, Some(expected_revision))
    }

    /// Commits `record`, if given, and returns the character to the store.
    pub fn release(
        &mut self,
        lease: &CharacterLease,
        record: Option<&CharacterRecord>,
    ) -> Result<u64, StoreError> {
        self.write(lease, record, true, None)
    }

    /// The last committed record, without contacting the holding zone.
    pub fn committed(&self, id: &CharacterId) -> Result<Option<Committed>, StoreError> {
        let Some(bytes) = self.backend.read(id)? else {
            return Ok(None);
        };
        let entry = decode_entry(id, &bytes)?;
        Ok(entry.record.map(|record| Committed {
            revision: entry.revision,
            record,
            leased_by: entry.lease,
        }))
    }

    fn write(
        &mut self,
        lease: &CharacterLease,
        record: Option<&CharacterRecord>,
        release: bool,
        expected_revision: Option<u64>,
    ) -> Result<u64, StoreError> {
        let mut revision = 0;
        self.transition(&lease.character_id, &mut |entry| {
            if record.is_some_and(|record| record.character_id != lease.character_id) {
                return Err(StoreError::WrongCharacter);
            }
            if entry.lease != Some(lease.holder) || entry.generation != lease.generation {
                return Err(StoreError::StaleLease);
            }
            if let Some(expected) = expected_revision
                && entry.revision != expected
            {
                return Err(StoreError::RevisionConflict {
                    expected,
                    actual: entry.revision,
                });
            }
            if let Some(record) = record {
                entry.revision = entry
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| StoreError::Corrupt("revisions are exhausted".into()))?;
                entry.record = Some(record.clone());
            }
            if release {
                entry.lease = None;
            }
            revision = entry.revision;
            Ok(())
        })?;
        Ok(revision)
    }

    fn transition(
        &mut self,
        id: &CharacterId,
        change: &mut dyn FnMut(&mut Entry) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        self.backend.update(id, &mut |bytes| {
            let mut entry = match bytes {
                Some(bytes) => decode_entry(id, bytes)?,
                None => Entry::default(),
            };
            change(&mut entry)?;
            encode_entry(&entry)
        })
    }
}
