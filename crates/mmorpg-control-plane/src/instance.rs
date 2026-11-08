//! The run directory of the party-private depths (#404 part 2, ADR 0009).
//!
//! The directory is the one owner of which private run each original character belongs to,
//! which depths zone (instance slot) a run holds, its frozen roster and party size, and how long
//! an empty run stays resumable. It never touches zone or character gameplay: the run's
//! progress (cleared chambers and the checkpoint) is reported to it as data so a restarted host
//! can rebind the run.
//!
//! One [`InstanceAuthority::transact`] applies all or nothing under the store's exclusion, so
//! two parties touching the threshold at once cannot create or join the same run, and a stale
//! zone lease changes nothing.
//!
//! Empty-run retention uses host time (`now`, milliseconds), not simulation ticks: it is a
//! lifecycle setting and never enters deterministic gameplay.

use std::collections::BTreeMap;
use std::fmt;

use mmorpg_core::{CharacterId, ErrorCode, MAX_PARTY_MEMBERS, ZoneId};

use crate::ZoneLease;

/// How long an empty run stays resumable unless a host configures otherwise (15 minutes).
pub const DEFAULT_RUN_GRACE_MS: u64 = 15 * 60 * 1_000;

/// One candidate of a new run: a character standing at the threshold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EntryCandidate {
    pub character: CharacterId,
    /// Whether it has completed the qualifying quest.
    pub qualified: bool,
}

/// A private run in the directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstanceRun {
    pub id: u64,
    pub slot: ZoneId,
    /// The frozen original roster, in entry order; its length is the party-size profile.
    pub roster: Vec<CharacterId>,
    /// Bit `i` set: chamber `i` is cleared.
    pub cleared: u8,
    pub checkpoint: bool,
    /// When the run became empty; `None` while a character is in it.
    pub emptied_at_ms: Option<u64>,
}

/// The directory's answer to a character touching the threshold.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub run: InstanceRun,
    /// Whether this request created the run.
    pub created: bool,
}

/// The run directory; see the module documentation.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstanceDirectory {
    next_run: u64,
    runs: BTreeMap<u64, InstanceRun>,
    /// Each original character's run while the run exists.
    by_character: BTreeMap<CharacterId, u64>,
    /// Every instance slot and the run holding it.
    slots: BTreeMap<ZoneId, Option<u64>>,
}

impl InstanceDirectory {
    #[must_use]
    pub fn new() -> Self {
        Self {
            next_run: 1,
            ..Self::default()
        }
    }

    /// Registers instance slots; already known slots are left alone.
    pub fn bind_slots(&mut self, slots: impl IntoIterator<Item = ZoneId>) {
        for slot in slots {
            self.slots.entry(slot).or_insert(None);
        }
    }

    /// Resolves a character touching a threshold, `candidates[0]` being that character.
    ///
    /// A character already on a live run's roster resumes it (the unlock is not rechecked:
    /// Atlas 5.1.36). Otherwise a new run takes every candidate not already on another run,
    /// provided one of them qualifies (5.1.36) and a slot is free. Refusals change nothing.
    pub fn enter(
        &mut self,
        candidates: &[EntryCandidate],
        now_ms: u64,
    ) -> Result<Entry, ErrorCode> {
        let initiator = candidates.first().ok_or(ErrorCode::DepthsClosed)?;
        if let Some(id) = self.by_character.get(&initiator.character).copied() {
            let run = self.runs.get_mut(&id).ok_or(ErrorCode::DepthsClosed)?;
            // Re-entry refreshes retention, so a transfer in flight always lands before expiry.
            run.emptied_at_ms = Some(now_ms);
            return Ok(Entry {
                run: run.clone(),
                created: false,
            });
        }
        let mut roster: Vec<_> = Vec::new();
        for candidate in candidates {
            if !self.by_character.contains_key(&candidate.character)
                && !roster
                    .iter()
                    .any(|entry: &&EntryCandidate| entry.character == candidate.character)
                && roster.len() < MAX_PARTY_MEMBERS
            {
                roster.push(candidate);
            }
        }
        if !roster.iter().any(|candidate| candidate.qualified) {
            return Err(ErrorCode::DepthsLocked);
        }
        let slot = self
            .slots
            .iter()
            .find(|(_, held)| held.is_none())
            .map(|(slot, _)| *slot)
            .ok_or(ErrorCode::DepthsClosed)?;
        let id = self.next_run.max(1);
        self.next_run = id.saturating_add(1);
        let run = InstanceRun {
            id,
            slot,
            roster: roster
                .iter()
                .map(|candidate| candidate.character.clone())
                .collect(),
            cleared: 0,
            checkpoint: false,
            emptied_at_ms: Some(now_ms),
        };
        for character in &run.roster {
            self.by_character.insert(character.clone(), id);
        }
        self.slots.insert(slot, Some(id));
        self.runs.insert(id, run.clone());
        Ok(Entry { run, created: true })
    }

    /// The run a character is on, if any.
    #[must_use]
    pub fn run_of(&self, character: &CharacterId) -> Option<&InstanceRun> {
        self.runs.get(self.by_character.get(character)?)
    }

    /// The run holding `slot`, if any.
    #[must_use]
    pub fn run_in_slot(&self, slot: ZoneId) -> Option<&InstanceRun> {
        self.runs.get(self.slots.get(&slot)?.as_ref()?)
    }

    /// The number of runs in the directory.
    #[must_use]
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Records whether `slot`'s run has a character in it. Level-triggered.
    pub fn sync_slot(&mut self, slot: ZoneId, occupied: bool, now_ms: u64) {
        let Some(run) = self.slots.get(&slot).copied().flatten() else {
            return;
        };
        let Some(run) = self.runs.get_mut(&run) else {
            return;
        };
        if occupied {
            run.emptied_at_ms = None;
        } else if run.emptied_at_ms.is_none() {
            run.emptied_at_ms = Some(now_ms);
        }
    }

    /// Merges a run's reported progress; a report for another run or an older state changes
    /// nothing. Progress is only ever added (a stale report cannot undo a cleared chamber).
    pub fn report(&mut self, slot: ZoneId, run_id: u64, cleared: u8, checkpoint: bool) {
        let Some(held) = self.slots.get(&slot).copied().flatten() else {
            return;
        };
        if held != run_id {
            return;
        }
        if let Some(run) = self.runs.get_mut(&run_id) {
            run.cleared |= cleared;
            run.checkpoint |= checkpoint;
        }
    }

    /// Retires every run empty for at least `grace_ms`; returns how many. A character whose
    /// stored location was a retired run re-enters through its next admission
    /// (`ZoneSimulation::admit_character` puts it at the public entrance).
    pub fn sweep(&mut self, now_ms: u64, grace_ms: u64) -> usize {
        let expired: Vec<u64> = self
            .runs
            .values()
            .filter(|run| {
                run.emptied_at_ms
                    .is_some_and(|at| now_ms.saturating_sub(at) >= grace_ms)
            })
            .map(|run| run.id)
            .collect();
        for id in &expired {
            if let Some(run) = self.runs.remove(id) {
                for character in &run.roster {
                    self.by_character.remove(character);
                }
                self.slots.insert(run.slot, None);
            }
        }
        expired.len()
    }

    /// The stored form of the directory.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.next_run.to_be_bytes());
        out.extend_from_slice(&(self.slots.len() as u32).to_be_bytes());
        for (slot, run) in &self.slots {
            out.extend_from_slice(&slot.get().to_be_bytes());
            out.extend_from_slice(&run.unwrap_or(0).to_be_bytes());
        }
        out.extend_from_slice(&(self.runs.len() as u32).to_be_bytes());
        for run in self.runs.values() {
            out.extend_from_slice(&run.id.to_be_bytes());
            out.extend_from_slice(&run.slot.get().to_be_bytes());
            out.push(run.roster.len() as u8);
            for character in &run.roster {
                encode_text(&mut out, character.as_str());
            }
            out.push(run.cleared);
            out.push(u8::from(run.checkpoint));
            out.extend_from_slice(&run.emptied_at_ms.unwrap_or(u64::MAX).to_be_bytes());
        }
        out
    }

    /// Restores a stored directory; malformed bytes fail closed.
    pub fn decode(bytes: &[u8]) -> Result<Self, InstanceStoreError> {
        let mut reader = Reader { bytes, offset: 0 };
        let mut directory = Self {
            next_run: reader.u64()?,
            ..Self::default()
        };
        for _ in 0..reader.u32()? {
            let slot = ZoneId::new(reader.u32()?);
            let run = reader.u64()?;
            directory.slots.insert(slot, (run != 0).then_some(run));
        }
        for _ in 0..reader.u32()? {
            let id = reader.u64()?;
            let slot = ZoneId::new(reader.u32()?);
            let mut roster = Vec::new();
            for _ in 0..reader.u8()? {
                let character = reader.character()?;
                directory.by_character.insert(character.clone(), id);
                roster.push(character);
            }
            let cleared = reader.u8()?;
            let checkpoint = reader.u8()? != 0;
            let emptied = reader.u64()?;
            directory.runs.insert(
                id,
                InstanceRun {
                    id,
                    slot,
                    roster,
                    cleared,
                    checkpoint,
                    emptied_at_ms: (emptied != u64::MAX).then_some(emptied),
                },
            );
        }
        if reader.offset != bytes.len() {
            return Err(InstanceStoreError::corrupt());
        }
        Ok(directory)
    }
}

fn encode_text(out: &mut Vec<u8>, text: &str) {
    out.push(text.len() as u8);
    out.extend_from_slice(text.as_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Reader<'_> {
    fn take(&mut self, length: usize) -> Result<&[u8], InstanceStoreError> {
        let end = self
            .offset
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(InstanceStoreError::corrupt)?;
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, InstanceStoreError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, InstanceStoreError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| InstanceStoreError::corrupt())?,
        ))
    }
    fn u64(&mut self) -> Result<u64, InstanceStoreError> {
        Ok(u64::from_be_bytes(
            self.take(8)?
                .try_into()
                .map_err(|_| InstanceStoreError::corrupt())?,
        ))
    }
    fn character(&mut self) -> Result<CharacterId, InstanceStoreError> {
        let length = usize::from(self.u8()?);
        let text =
            std::str::from_utf8(self.take(length)?).map_err(|_| InstanceStoreError::corrupt())?;
        CharacterId::new(text.to_owned()).map_err(|_| InstanceStoreError::corrupt())
    }
}

/// A failed directory transaction, which applies nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstanceStoreError {
    /// The issuing zone's lease is not current.
    Fenced,
    Backend(String),
}

impl InstanceStoreError {
    #[must_use]
    pub fn corrupt() -> Self {
        Self::Backend("stored instance directory is malformed".to_owned())
    }
}

impl fmt::Display for InstanceStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fenced => formatter.write_str("the zone lease is not current"),
            Self::Backend(message) => write!(formatter, "instance directory: {message}"),
        }
    }
}

impl std::error::Error for InstanceStoreError {}

/// A store of the one run directory; `body` runs under the store's exclusion and applies only
/// when the transaction commits.
pub trait InstanceAuthority {
    fn transact(
        &mut self,
        issuer: Option<&ZoneLease>,
        body: &mut dyn FnMut(&mut InstanceDirectory, u64),
    ) -> Result<(), InstanceStoreError>;
}

/// The directory of a single process; a restart starts it empty.
#[derive(Debug)]
pub struct LocalInstances {
    directory: InstanceDirectory,
    started: std::time::Instant,
}

impl LocalInstances {
    #[must_use]
    pub fn new() -> Self {
        Self {
            directory: InstanceDirectory::new(),
            started: std::time::Instant::now(),
        }
    }
}

impl Default for LocalInstances {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceAuthority for LocalInstances {
    fn transact(
        &mut self,
        _issuer: Option<&ZoneLease>,
        body: &mut dyn FnMut(&mut InstanceDirectory, u64),
    ) -> Result<(), InstanceStoreError> {
        let now = self.started.elapsed().as_millis();
        body(&mut self.directory, u64::try_from(now).unwrap_or(u64::MAX));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(name: &str, qualified: bool) -> EntryCandidate {
        EntryCandidate {
            character: CharacterId::new(name).unwrap(),
            qualified,
        }
    }

    fn directory() -> InstanceDirectory {
        let mut directory = InstanceDirectory::new();
        directory.bind_slots([ZoneId::new(20_000), ZoneId::new(20_001)]);
        directory
    }

    fn party(names: &[&str]) -> Vec<EntryCandidate> {
        names.iter().map(|name| candidate(name, false)).collect()
    }

    #[test]
    fn a_qualified_solo_player_starts_a_run_and_an_unqualified_one_is_refused() {
        let mut directory = directory();
        assert_eq!(
            directory.enter(&[candidate("rook", false)], 1).unwrap_err(),
            ErrorCode::DepthsLocked
        );
        let entry = directory.enter(&[candidate("aria", true)], 1).unwrap();
        assert!(entry.created);
        assert_eq!(entry.run.roster.len(), 1);
        assert_eq!(entry.run.slot, ZoneId::new(20_000));
    }

    #[test]
    fn one_qualified_member_opens_the_run_for_the_whole_gathered_party() {
        let mut directory = directory();
        let mut candidates = party(&["bren", "cole"]);
        candidates.insert(1, candidate("aria", true));
        let entry = directory.enter(&candidates, 1).unwrap();
        assert!(entry.created);
        assert_eq!(entry.run.roster.len(), 3);
        // A member who touches the threshold later reaches the same run, unchecked.
        let again = directory.enter(&party(&["cole"]), 2).unwrap();
        assert_eq!(again.run.id, entry.run.id);
        assert!(!again.created);
    }

    #[test]
    fn a_fully_unqualified_party_is_refused_and_changes_nothing() {
        let mut directory = directory();
        let before = directory.clone();
        assert_eq!(
            directory.enter(&party(&["rook", "tam"]), 1).unwrap_err(),
            ErrorCode::DepthsLocked
        );
        assert_eq!(directory, before);
    }

    #[test]
    fn the_roster_and_profile_are_frozen_and_no_one_joins_later() {
        let mut directory = directory();
        let entry = directory
            .enter(&[candidate("aria", true), candidate("bren", false)], 1)
            .unwrap();
        // A newcomer standing beside a roster member starts nothing inside that run: its own
        // candidate list never reaches the run, and without a qualified member it is refused.
        assert_eq!(
            directory.enter(&[candidate("dana", false)], 2).unwrap_err(),
            ErrorCode::DepthsLocked
        );
        let run = directory
            .run_of(&CharacterId::new("aria").unwrap())
            .unwrap();
        assert_eq!(run.id, entry.run.id);
        assert_eq!(run.roster.len(), 2);
        // Even with a qualified newcomer, a new run takes only characters not already on one.
        let second = directory
            .enter(&[candidate("dana", true), candidate("bren", false)], 3)
            .unwrap();
        assert_ne!(second.run.id, entry.run.id);
        assert_eq!(second.run.roster, vec![CharacterId::new("dana").unwrap()]);
    }

    #[test]
    fn two_unrelated_parties_get_different_runs_and_slots() {
        let mut directory = directory();
        let first = directory.enter(&[candidate("aria", true)], 1).unwrap();
        let second = directory.enter(&[candidate("tess", true)], 1).unwrap();
        assert_ne!(first.run.id, second.run.id);
        assert_ne!(first.run.slot, second.run.slot);
    }

    #[test]
    fn no_free_slot_refuses_a_new_run_but_not_a_resume() {
        let mut directory = directory();
        directory.enter(&[candidate("a", true)], 1).unwrap();
        directory.enter(&[candidate("b", true)], 1).unwrap();
        assert_eq!(
            directory.enter(&[candidate("c", true)], 1).unwrap_err(),
            ErrorCode::DepthsClosed
        );
        assert!(directory.enter(&[candidate("a", true)], 2).is_ok());
    }

    #[test]
    fn an_empty_run_stays_resumable_until_its_grace_ends() {
        let mut directory = directory();
        let id = directory
            .enter(&[candidate("aria", true)], 0)
            .unwrap()
            .run
            .id;
        directory.sync_slot(ZoneId::new(20_000), true, 0);
        directory.sync_slot(ZoneId::new(20_000), false, 1_000);
        assert_eq!(directory.sweep(1_000 + 899_999, 900_000), 0);
        assert_eq!(
            directory
                .run_of(&CharacterId::new("aria").unwrap())
                .unwrap()
                .id,
            id
        );
        // A return inside the grace resets the clock.
        directory.sync_slot(ZoneId::new(20_000), true, 5_000);
        directory.sync_slot(ZoneId::new(20_000), false, 6_000);
        assert_eq!(directory.sweep(6_000 + 900_000, 900_000), 1);
        assert_eq!(directory.run_count(), 0);
        assert!(
            directory
                .run_of(&CharacterId::new("aria").unwrap())
                .is_none()
        );
        // The slot is free again and a new run takes it.
        let next = directory
            .enter(&[candidate("aria", true)], 7_000_000)
            .unwrap();
        assert!(next.created);
        assert_ne!(next.run.id, id);
    }

    #[test]
    fn progress_is_kept_for_the_right_run_and_only_grows() {
        let mut directory = directory();
        let entry = directory.enter(&[candidate("aria", true)], 0).unwrap();
        directory.report(entry.run.slot, entry.run.id, 0b001, true);
        directory.report(entry.run.slot, entry.run.id, 0b000, false);
        directory.report(entry.run.slot, entry.run.id + 1, 0b111, true);
        let run = directory.run_in_slot(entry.run.slot).unwrap();
        assert_eq!(run.cleared, 0b001);
        assert!(run.checkpoint);
    }

    #[test]
    fn a_stored_directory_round_trips_and_malformed_bytes_fail_closed() {
        let mut directory = directory();
        let entry = directory
            .enter(&[candidate("aria", true), candidate("bren", false)], 5)
            .unwrap();
        directory.report(entry.run.slot, entry.run.id, 1, true);
        let bytes = directory.encode();
        assert_eq!(InstanceDirectory::decode(&bytes).unwrap(), directory);
        assert!(InstanceDirectory::decode(&bytes[..bytes.len() - 1]).is_err());
    }
}
