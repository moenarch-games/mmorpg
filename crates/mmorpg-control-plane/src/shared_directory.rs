//! A directory every host shares through one stored row (#1146): the party directory
//! ([`crate::party`]) and the instance run directory ([`crate::instance`]).
//!
//! Hosts reconcile against it every poll, and almost every poll changes nothing. So a
//! transaction first reads the row's revision without a lock and decodes the stored encoding
//! only when another host moved it ([`DirectoryRow::peek`]); the body runs on a copy of the
//! cached directory, and only a body that changed it takes the row's lock to write
//! ([`DirectoryRow::write`]). A row that moved between the read and the lock is decoded again
//! and the body runs again on it, under the lock, so every write still applies to the latest
//! directory and hosts stay linearized. A body may therefore run twice; it only assigns its
//! outputs, so the last run's stand.
//!
//! The stored encoding names its format, and a write keeps the row's previous encoding beside
//! the new one. A row this build cannot decode fails every transaction with an error naming
//! the directory, revision and format ([`DirectoryError::Undecodable`]) and is never
//! overwritten by a host; the operator's repair step restores the previous encoding
//! (`MMORPG_PERSISTENCE_MODE=repair-directories`, `docs/NATIVE_CLIENT.md`).

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::ZoneLease;

/// Why a stored encoding does not decode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeFailure {
    /// A format this build does not know: a newer build wrote it, or it is damaged.
    UnknownFormat,
    /// A known format whose bytes are malformed.
    Malformed,
}

/// A directory kept as one stored encoding.
pub trait StoredDirectory: Clone + Eq {
    /// What the directory is, for errors.
    const NAME: &'static str;
    /// The format this build writes. A layout change takes the next format and keeps decoding
    /// the older ones, so no migration has to clear a directory again.
    const FORMAT: i16;
    /// The directory of an empty row.
    fn empty() -> Self;
    fn encode(&self) -> Vec<u8>;
    /// Reads an encoding of `format`; empty bytes are the empty directory in any format.
    fn decode(format: i16, bytes: &[u8]) -> Result<Self, DecodeFailure>;
}

/// The stored row: its revision (each write adds one), the format and the encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredRow {
    pub revision: u64,
    pub format: i16,
    pub state: Vec<u8>,
}

/// Decodes `row`, or the error naming it.
pub fn decode_row<D: StoredDirectory>(row: &StoredRow) -> Result<D, DirectoryError> {
    if row.state.is_empty() {
        return Ok(D::empty());
    }
    D::decode(row.format, &row.state).map_err(|failure| DirectoryError::Undecodable {
        directory: D::NAME,
        revision: row.revision,
        format: row.format,
        known: D::FORMAT,
        failure,
    })
}

/// A failed directory transaction, which applies nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectoryError {
    /// The issuing zone's lease is not current.
    Fenced,
    /// The stored row does not decode with this build.
    Undecodable {
        directory: &'static str,
        revision: u64,
        format: i16,
        /// The newest format this build reads.
        known: i16,
        failure: DecodeFailure,
    },
    Backend(String),
}

impl fmt::Display for DirectoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fenced => formatter.write_str("the zone's lease is not current"),
            Self::Undecodable {
                directory,
                revision,
                format,
                known,
                failure,
            } => {
                write!(
                    formatter,
                    "the stored {directory} (revision {revision}, format {format}) "
                )?;
                match failure {
                    DecodeFailure::UnknownFormat if format > known => write!(
                        formatter,
                        "was written by a newer build; this build reads formats up to {known}, \
                         so upgrade it"
                    ),
                    DecodeFailure::UnknownFormat => formatter.write_str(
                        "has an unknown format; MMORPG_PERSISTENCE_MODE=repair-directories \
                         restores its last good revision",
                    ),
                    DecodeFailure::Malformed => formatter.write_str(
                        "is malformed; MMORPG_PERSISTENCE_MODE=repair-directories restores its \
                         last good revision",
                    ),
                }
            }
            Self::Backend(cause) => write!(formatter, "directory store: {cause}"),
        }
    }
}

impl std::error::Error for DirectoryError {}

/// What a locked write did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Written {
    /// The row now holds the new encoding at this revision.
    Stored(u64),
    /// The body changed nothing on the moved row, which stays at this revision.
    Unchanged(u64),
}

/// The encoding to store under the lock, or `None` to store nothing. Gets the store's clock and
/// the row when it moved since the revision the write expected.
pub type Apply<'a> =
    dyn FnMut(u64, Option<StoredRow>) -> Result<Option<Vec<u8>>, DirectoryError> + 'a;

/// Where a shared directory's row lives.
pub trait DirectoryRow {
    /// Without a lock: the store's clock (milliseconds) and the row, unless it is still at
    /// revision `known`. Fails with [`DirectoryError::Fenced`] when `issuer` is not its zone's
    /// current grant.
    fn peek(
        &mut self,
        issuer: Option<&ZoneLease>,
        known: Option<u64>,
    ) -> Result<(u64, Option<StoredRow>), DirectoryError>;
    /// Under the row's lock, and fenced by `issuer` until it commits: stores what `apply`
    /// returns in `format`, at the next revision, keeping the replaced encoding as the
    /// previous one.
    fn write(
        &mut self,
        issuer: Option<&ZoneLease>,
        known: u64,
        format: i16,
        apply: &mut Apply<'_>,
    ) -> Result<Written, DirectoryError>;
}

/// The work a [`SharedDirectory`] did, for `directory-poll` in `.performance/contract.json`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DirectoryWork {
    /// Unlocked revision reads, one per transaction.
    pub reads: u64,
    /// Stored encodings decoded.
    pub decodes: u64,
    /// Writes under the row's lock.
    pub locked_writes: u64,
}

/// One host's view of a shared directory: the row and the last revision it decoded.
pub struct SharedDirectory<D, R> {
    row: R,
    cached: Option<(u64, D)>,
    work: DirectoryWork,
}

impl<D: StoredDirectory, R: DirectoryRow> SharedDirectory<D, R> {
    #[must_use]
    pub const fn new(row: R) -> Self {
        Self {
            row,
            cached: None,
            work: DirectoryWork {
                reads: 0,
                decodes: 0,
                locked_writes: 0,
            },
        }
    }

    #[must_use]
    pub const fn work(&self) -> DirectoryWork {
        self.work
    }

    /// Runs `body` on the directory as of now and stores what it changed; see the module
    /// documentation.
    pub fn transact(
        &mut self,
        issuer: Option<&ZoneLease>,
        body: &mut dyn FnMut(&mut D, u64),
    ) -> Result<(), DirectoryError> {
        let Self { row, cached, work } = self;
        work.reads += 1;
        let (now, moved) = row.peek(issuer, cached.as_ref().map(|(revision, _)| *revision))?;
        if let Some(moved) = moved {
            work.decodes += 1;
            *cached = Some((moved.revision, decode_row(&moved)?));
        }
        let Some((revision, current)) = cached.as_ref() else {
            return Err(DirectoryError::Backend(format!(
                "the {} row was not read",
                D::NAME
            )));
        };
        let mut next = current.clone();
        body(&mut next, now);
        if next == *current {
            return Ok(());
        }
        let expected = *revision;
        work.locked_writes += 1;
        let mut stored = None;
        let written = row.write(issuer, expected, D::FORMAT, &mut |now, moved| {
            let Some(moved) = moved else {
                let bytes = next.encode();
                stored = Some(next.clone());
                return Ok(Some(bytes));
            };
            work.decodes += 1;
            let base: D = decode_row(&moved)?;
            let mut again = base.clone();
            body(&mut again, now);
            let bytes = (again != base).then(|| again.encode());
            stored = Some(again);
            Ok(bytes)
        })?;
        if let (Written::Stored(revision) | Written::Unchanged(revision), Some(directory)) =
            (written, stored)
        {
            *cached = Some((revision, directory));
        }
        Ok(())
    }
}

/// A stored row with its previous encoding, as both stores keep it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeptRow {
    pub current: StoredRow,
    /// The encoding the last write replaced, which decoded then.
    pub previous: Option<StoredRow>,
}

impl KeptRow {
    /// An empty directory at revision 0.
    #[must_use]
    pub const fn empty(format: i16) -> Self {
        Self {
            current: StoredRow {
                revision: 0,
                format,
                state: Vec::new(),
            },
            previous: None,
        }
    }
}

/// How the repair step left a directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Repair {
    /// It decodes; nothing changed.
    Healthy,
    /// It did not decode; the previous encoding, of this revision, is current again.
    Restored { previous_revision: u64 },
    /// Neither encoding decoded and the operator asked for a reset: it is empty.
    Reset,
}

/// The repair step for one row (`MMORPG_PERSISTENCE_MODE=repair-directories`): restores the
/// previous encoding when only the current one fails to decode; with `reset`, empties a row
/// neither of whose encodings decodes. Refuses a row a newer build wrote: upgrading is the
/// repair. The revision always advances, so every host decodes the repaired row.
pub fn repair<D: StoredDirectory>(
    row: &mut KeptRow,
    reset: bool,
) -> Result<Repair, DirectoryError> {
    let error = match decode_row::<D>(&row.current) {
        Ok(_) => return Ok(Repair::Healthy),
        Err(error) => error,
    };
    if row.current.format > D::FORMAT {
        return Err(error);
    }
    let revision = row.current.revision + 1;
    if let Some(previous) = &row.previous
        && decode_row::<D>(previous).is_ok()
    {
        let previous_revision = previous.revision;
        row.current = StoredRow {
            revision,
            ..previous.clone()
        };
        return Ok(Repair::Restored { previous_revision });
    }
    if !reset {
        return Err(error);
    }
    row.current = StoredRow {
        revision,
        format: D::FORMAT,
        state: Vec::new(),
    };
    Ok(Repair::Reset)
}

/// The row in memory, for tests and work samples: a clone is another host's connection to the
/// same row. It has no zone grants, so it fences nothing.
#[derive(Clone, Debug)]
pub struct MemoryRow {
    row: Arc<Mutex<KeptRow>>,
    clock: Arc<Mutex<u64>>,
}

impl MemoryRow {
    #[must_use]
    pub fn new(format: i16) -> Self {
        Self {
            row: Arc::new(Mutex::new(KeptRow::empty(format))),
            clock: Arc::default(),
        }
    }

    fn with<T>(&self, body: impl FnOnce(&mut KeptRow) -> T) -> T {
        body(&mut self.row.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The row as stored.
    #[must_use]
    pub fn kept(&self) -> KeptRow {
        self.with(|row| row.clone())
    }

    /// Replaces the stored row, as a damaged write or the repair step would.
    pub fn store(&self, kept: KeptRow) {
        self.with(|row| *row = kept);
    }

    /// Sets the store's clock.
    pub fn set_clock(&self, now_ms: u64) {
        *self.clock.lock().unwrap_or_else(PoisonError::into_inner) = now_ms;
    }

    fn now(&self) -> u64 {
        *self.clock.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl DirectoryRow for MemoryRow {
    fn peek(
        &mut self,
        _issuer: Option<&ZoneLease>,
        known: Option<u64>,
    ) -> Result<(u64, Option<StoredRow>), DirectoryError> {
        let now = self.now();
        Ok((
            now,
            self.with(|row| (known != Some(row.current.revision)).then(|| row.current.clone())),
        ))
    }

    fn write(
        &mut self,
        _issuer: Option<&ZoneLease>,
        known: u64,
        format: i16,
        apply: &mut Apply<'_>,
    ) -> Result<Written, DirectoryError> {
        let now = self.now();
        let mut row = self.row.lock().unwrap_or_else(PoisonError::into_inner);
        let moved = (row.current.revision != known).then(|| row.current.clone());
        let Some(state) = apply(now, moved)? else {
            return Ok(Written::Unchanged(row.current.revision));
        };
        let revision = row.current.revision + 1;
        let previous = std::mem::replace(
            &mut row.current,
            StoredRow {
                revision,
                format,
                state,
            },
        );
        row.previous = Some(previous);
        Ok(Written::Stored(revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter directory; format 1 is one big-endian `u32`, format 2 a `u64`.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct Counter(u64);

    impl StoredDirectory for Counter {
        const NAME: &'static str = "counter";
        const FORMAT: i16 = 2;
        fn empty() -> Self {
            Self(0)
        }
        fn encode(&self) -> Vec<u8> {
            self.0.to_be_bytes().to_vec()
        }
        fn decode(format: i16, bytes: &[u8]) -> Result<Self, DecodeFailure> {
            match format {
                1 => Ok(Self(u64::from(u32::from_be_bytes(
                    bytes.try_into().map_err(|_| DecodeFailure::Malformed)?,
                )))),
                2 => Ok(Self(u64::from_be_bytes(
                    bytes.try_into().map_err(|_| DecodeFailure::Malformed)?,
                ))),
                _ => Err(DecodeFailure::UnknownFormat),
            }
        }
    }

    fn host(row: &MemoryRow) -> SharedDirectory<Counter, MemoryRow> {
        SharedDirectory::new(row.clone())
    }

    fn read(directory: &mut SharedDirectory<Counter, MemoryRow>) -> u64 {
        let mut value = 0;
        directory
            .transact(None, &mut |counter, _| value = counter.0)
            .unwrap();
        value
    }

    #[test]
    fn idle_polls_neither_lock_nor_decode_and_a_change_writes_once() {
        let row = MemoryRow::new(Counter::FORMAT);
        let mut directory = host(&row);
        for _ in 0..100 {
            assert_eq!(read(&mut directory), 0);
        }
        directory
            .transact(None, &mut |counter, _| counter.0 += 1)
            .unwrap();
        for _ in 0..100 {
            assert_eq!(read(&mut directory), 1);
        }
        assert_eq!(
            directory.work(),
            DirectoryWork {
                reads: 201,
                decodes: 1,
                locked_writes: 1,
            }
        );
        assert_eq!(row.kept().current.revision, 1);
        assert_eq!(row.kept().previous.unwrap().revision, 0);
    }

    #[test]
    fn hosts_see_each_others_writes_and_a_raced_write_reapplies_on_the_latest() {
        let row = MemoryRow::new(Counter::FORMAT);
        let (mut west, mut east) = (host(&row), host(&row));
        west.transact(None, &mut |counter, _| counter.0 += 1)
            .unwrap();
        assert_eq!(read(&mut east), 1);
        // East moves the row while west's body runs: west's write runs again on east's.
        let mut raced = false;
        west.transact(None, &mut |counter, _| {
            if !raced {
                raced = true;
                east.transact(None, &mut |counter, _| counter.0 += 10)
                    .unwrap();
            }
            counter.0 += 1;
        })
        .unwrap();
        assert_eq!(read(&mut west), 12);
        assert_eq!(row.kept().current.revision, 3);
    }

    #[test]
    fn an_older_format_still_decodes_and_the_next_write_upgrades_it() {
        let row = MemoryRow::new(1);
        row.store(KeptRow {
            current: StoredRow {
                revision: 4,
                format: 1,
                state: 7_u32.to_be_bytes().to_vec(),
            },
            previous: None,
        });
        let mut directory = host(&row);
        assert_eq!(read(&mut directory), 7);
        directory
            .transact(None, &mut |counter, _| counter.0 += 1)
            .unwrap();
        let kept = row.kept();
        assert_eq!((kept.current.format, kept.current.revision), (2, 5));
        assert_eq!(kept.current.state, 8_u64.to_be_bytes());
    }

    #[test]
    fn an_undecodable_row_is_reported_left_alone_and_repaired_from_its_previous_encoding() {
        let row = MemoryRow::new(Counter::FORMAT);
        let mut directory = host(&row);
        directory
            .transact(None, &mut |counter, _| counter.0 = 5)
            .unwrap();
        let mut damaged = row.kept();
        damaged.previous = Some(damaged.current.clone());
        damaged.current = StoredRow {
            revision: 2,
            format: 2,
            state: vec![1, 2, 3],
        };
        row.store(damaged.clone());
        let mut fresh = host(&row);
        let error = fresh
            .transact(None, &mut |counter, _| counter.0 += 1)
            .unwrap_err();
        assert!(matches!(
            error,
            DirectoryError::Undecodable {
                revision: 2,
                format: 2,
                failure: DecodeFailure::Malformed,
                ..
            }
        ));
        assert!(error.to_string().contains("revision 2, format 2"));
        assert_eq!(
            row.kept(),
            damaged,
            "no host overwrites what it cannot read"
        );

        let mut kept = row.kept();
        assert_eq!(
            repair::<Counter>(&mut kept, false),
            Ok(Repair::Restored {
                previous_revision: 1
            })
        );
        row.store(kept);
        assert_eq!(read(&mut fresh), 5);
        assert_eq!(
            read(&mut directory),
            5,
            "the revision advanced past the cache"
        );
        let mut kept = row.kept();
        assert_eq!(repair::<Counter>(&mut kept, false), Ok(Repair::Healthy));
    }

    #[test]
    fn repair_refuses_a_newer_build_and_resets_only_when_asked() {
        let newer = KeptRow {
            current: StoredRow {
                revision: 9,
                format: 3,
                state: vec![0; 3],
            },
            previous: None,
        };
        let error = repair::<Counter>(&mut newer.clone(), true).unwrap_err();
        assert!(error.to_string().contains("newer build"));
        let mut lost = KeptRow {
            current: StoredRow {
                format: 2,
                ..newer.current
            },
            previous: None,
        };
        assert!(repair::<Counter>(&mut lost, false).is_err());
        assert_eq!(repair::<Counter>(&mut lost, true), Ok(Repair::Reset));
        assert_eq!(lost.current.revision, 10);
        assert!(lost.current.state.is_empty());
    }
}
