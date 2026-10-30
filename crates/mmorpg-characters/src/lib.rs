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
//! A cross-zone transfer (#145) moves the lease without a gap or an overlap.
//! The source zone, still the holder, [begins](CharacterStore::begin_handoff)
//! a handoff with the frozen [`CharacterTransfer`]: from then on nobody may
//! write, acquire or release the character, so neither side mutates it. The
//! control plane's fenced handoff record decides the outcome; the store only
//! carries it out. [Completing](CharacterStore::complete_handoff) commits the
//! transfer's record and grants the lease to the destination in one atomic
//! step; [cancelling](CharacterStore::cancel_handoff) leaves the source's
//! lease and committed record as they were. Both are idempotent per transfer.
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

use mmorpg_core::{CharacterId, CharacterRecord, CharacterTransfer, TransferId, ZoneId};

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
    /// The handoff the holder began and the control plane has not settled.
    pub handoff: Option<PendingHandoff>,
    /// The last completed handoff and the lease generation it granted, so a
    /// retried completion returns that lease while it is still current.
    pub last_handoff: Option<(TransferId, u64)>,
}

/// A transfer out of the holding zone, waiting for its outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PendingHandoff {
    pub transfer: CharacterTransfer,
    /// Who holds the lease once the handoff completes.
    pub to: LeaseHolder,
    /// Who held the lease when the handoff began; it holds it until the end.
    pub from: LeaseHolder,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoreError {
    /// A handoff is pending; the control plane must settle it first.
    HandoffPending(TransferId),
    /// The handoff already completed and can no longer be cancelled.
    HandoffCompleted(TransferId),
    /// No pending or completed handoff has this transfer ID.
    UnknownHandoff(TransferId),
    /// The handoff names the holder's own zone or another character.
    InvalidHandoff(&'static str),
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
            Self::HandoffPending(transfer) => write!(
                formatter,
                "the character is being handed off (transfer {})",
                transfer.get()
            ),
            Self::HandoffCompleted(transfer) => {
                write!(formatter, "transfer {} already completed", transfer.get())
            }
            Self::UnknownHandoff(transfer) => {
                write!(formatter, "no handoff has transfer {}", transfer.get())
            }
            Self::InvalidHandoff(reason) => write!(formatter, "invalid handoff: {reason}"),
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
            if let Some(pending) = &entry.handoff {
                return Err(StoreError::HandoffPending(pending.transfer.transfer_id));
            }
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

    /// Starts handing the character held under `lease` to `to` with the
    /// source's frozen `transfer`. From now on the character can be neither
    /// written, released nor acquired until the handoff completes or is
    /// cancelled. Beginning the same handoff again is a no-op.
    pub fn begin_handoff(
        &mut self,
        lease: &CharacterLease,
        transfer: &CharacterTransfer,
        to: LeaseHolder,
    ) -> Result<(), StoreError> {
        if transfer.record.character_id != lease.character_id {
            return Err(StoreError::WrongCharacter);
        }
        if to.zone_id == lease.holder.zone_id || transfer.source_zone != lease.holder.zone_id {
            return Err(StoreError::InvalidHandoff(
                "a handoff leaves the holder's zone for another zone",
            ));
        }
        if transfer.record.location.zone_id != to.zone_id {
            return Err(StoreError::InvalidHandoff(
                "the transferred record is not located in the destination zone",
            ));
        }
        let pending = PendingHandoff {
            transfer: transfer.clone(),
            to,
            from: lease.holder,
        };
        self.transition(&lease.character_id, &mut |entry| {
            if entry.lease != Some(lease.holder) || entry.generation != lease.generation {
                return Err(StoreError::StaleLease);
            }
            match &entry.handoff {
                Some(existing) if *existing == pending => Ok(()),
                Some(existing) => Err(StoreError::HandoffPending(existing.transfer.transfer_id)),
                None => {
                    entry.handoff = Some(pending.clone());
                    Ok(())
                }
            }
        })
    }

    /// Commits the pending handoff's record and grants its destination the
    /// lease, atomically. Only the control plane's acceptance of the handoff
    /// authorizes this. Completing again returns the same lease while no
    /// later acquisition superseded it.
    pub fn complete_handoff(
        &mut self,
        id: &CharacterId,
        transfer_id: TransferId,
    ) -> Result<Acquired, StoreError> {
        let mut acquired = None;
        self.transition(id, &mut |entry| {
            let Some(pending) = entry
                .handoff
                .take_if(|pending| pending.transfer.transfer_id == transfer_id)
            else {
                if entry.last_handoff == Some((transfer_id, entry.generation))
                    && let (Some(holder), Some(record)) = (entry.lease, &entry.record)
                {
                    acquired = Some(Acquired {
                        lease: CharacterLease {
                            character_id: id.clone(),
                            holder,
                            generation: entry.generation,
                        },
                        revision: entry.revision,
                        record: Some(record.clone()),
                    });
                    return Ok(());
                }
                return Err(match &entry.handoff {
                    Some(other) => StoreError::HandoffPending(other.transfer.transfer_id),
                    None if entry
                        .last_handoff
                        .is_some_and(|(last, _)| last == transfer_id) =>
                    {
                        StoreError::StaleLease
                    }
                    None => StoreError::UnknownHandoff(transfer_id),
                });
            };
            entry.revision = entry
                .revision
                .checked_add(1)
                .ok_or_else(|| StoreError::Corrupt("revisions are exhausted".into()))?;
            entry.generation = entry
                .generation
                .checked_add(1)
                .ok_or_else(|| StoreError::Corrupt("lease generations are exhausted".into()))?;
            entry.record = Some(pending.transfer.record.clone());
            entry.lease = Some(pending.to);
            entry.fence = Some(pending.to);
            entry.last_handoff = Some((transfer_id, entry.generation));
            acquired = Some(Acquired {
                lease: CharacterLease {
                    character_id: id.clone(),
                    holder: pending.to,
                    generation: entry.generation,
                },
                revision: entry.revision,
                record: Some(pending.transfer.record),
            });
            Ok(())
        })?;
        acquired.ok_or_else(|| StoreError::Backend("the backend skipped the update".into()))
    }

    /// Drops the pending handoff: the source keeps its lease and committed
    /// record. Only the control plane's abort of the handoff, or the absence
    /// of any handoff record, authorizes this. Returns whether it was
    /// pending; a completed handoff can no longer be cancelled.
    pub fn cancel_handoff(
        &mut self,
        id: &CharacterId,
        transfer_id: TransferId,
    ) -> Result<bool, StoreError> {
        let Some(bytes) = self.backend.read(id)? else {
            return Ok(false);
        };
        let entry = decode_entry(id, &bytes)?;
        if entry
            .last_handoff
            .is_some_and(|(last, _)| last == transfer_id)
        {
            return Err(StoreError::HandoffCompleted(transfer_id));
        }
        if entry
            .handoff
            .is_none_or(|pending| pending.transfer.transfer_id != transfer_id)
        {
            return Ok(false);
        }
        self.transition(id, &mut |entry| {
            if entry
                .last_handoff
                .is_some_and(|(last, _)| last == transfer_id)
            {
                return Err(StoreError::HandoffCompleted(transfer_id));
            }
            entry
                .handoff
                .take_if(|pending| pending.transfer.transfer_id == transfer_id)
                .map(|_| ())
                .ok_or(StoreError::UnknownHandoff(transfer_id))
        })?;
        Ok(true)
    }

    /// The pending handoff of `id`, if any.
    pub fn pending_handoff(&self, id: &CharacterId) -> Result<Option<PendingHandoff>, StoreError> {
        let Some(bytes) = self.backend.read(id)? else {
            return Ok(None);
        };
        Ok(decode_entry(id, &bytes)?.handoff)
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
            if let Some(pending) = &entry.handoff {
                return Err(StoreError::HandoffPending(pending.transfer.transfer_id));
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
