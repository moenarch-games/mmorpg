//! Derived XZ broad phase for interest policy and creature perception,
//! never for collision or authority.

use std::collections::{BTreeMap, BTreeSet};

use crate::{EntityRef, INTEREST_RADIUS_UNITS, ZoneSnapshot};

/// Deterministic query work, excluding index maintenance and serialization.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterestQueryStats {
    pub cells_visited: usize,
    pub candidates_tested: usize,
    /// Candidates inside the interest radius, before the priority cap.
    pub relevant: usize,
}

/// Cumulative deterministic index maintenance work since zone construction or recovery.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct InterestMaintenanceStats {
    pub full_rebuilds: usize,
    pub bucket_inserts: usize,
    pub bucket_removes: usize,
    pub bucket_moves: usize,
    /// Units with a physics body (players and living creatures) whose
    /// position index maintenance re-read.
    pub units_inspected: usize,
}

/// The same projection used for publication, with deterministic workload evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayerProjection {
    pub snapshot: ZoneSnapshot,
    pub stats: InterestQueryStats,
    /// Records and sheets the projection built (#1056).
    pub work: crate::ProjectionWork,
    /// What each stamped sheet was built from at this tick.
    pub sheets: crate::SheetStamps,
    /// The stamps the publisher said it held: a sheet whose stamp equals its held one was left
    /// out of the snapshot, for the publisher to send from its own encoding.
    pub held: crate::SheetStamps,
}

/// Every visible unit (players, living creatures, corpses and NPCs) in cells
/// one interest radius wide.
#[derive(Default)]
pub(crate) struct InterestIndex {
    cells: BTreeMap<(i64, i64), BTreeSet<EntityRef>>,
    memberships: BTreeMap<EntityRef, (i64, i64)>,
}

impl InterestIndex {
    pub(crate) fn insert(&mut self, entity: EntityRef, x: i32, z: i32) {
        let cell = Self::cell(x, z);
        debug_assert!(!self.memberships.contains_key(&entity));
        self.cells.entry(cell).or_default().insert(entity);
        self.memberships.insert(entity, cell);
    }

    /// Returns whether the unit was indexed.
    pub(crate) fn remove(&mut self, entity: EntityRef) -> bool {
        let Some(cell) = self.memberships.remove(&entity) else {
            return false;
        };
        if let Some(entities) = self.cells.get_mut(&cell) {
            entities.remove(&entity);
            if entities.is_empty() {
                self.cells.remove(&cell);
            }
        }
        true
    }

    /// Returns true only when a bucket membership actually changes.
    pub(crate) fn move_if_needed(&mut self, entity: EntityRef, x: i32, z: i32) -> bool {
        let cell = Self::cell(x, z);
        if self.memberships.get(&entity) == Some(&cell) {
            return false;
        }
        self.remove(entity);
        self.insert(entity, x, z);
        true
    }

    /// Candidates for the interest radius around `(x, z)`, in `EntityRef` order.
    pub(crate) fn candidates(&self, x: i32, z: i32) -> (Vec<EntityRef>, InterestQueryStats) {
        let (center_x, center_z) = Self::cell(x, z);
        let mut candidates = Vec::new();
        let mut stats = InterestQueryStats::default();
        // Cell width equals the interest radius: all relevant centers are in these
        // nine cells. Use i64 and floor division for negative/extreme coordinates.
        for cell_x in center_x - 1..=center_x + 1 {
            for cell_z in center_z - 1..=center_z + 1 {
                stats.cells_visited += 1;
                if let Some(entities) = self.cells.get(&(cell_x, cell_z)) {
                    candidates.extend(entities);
                }
            }
        }
        candidates.sort_unstable();
        stats.candidates_tested = candidates.len();
        (candidates, stats)
    }

    /// Every unit indexed in a cell that the square `(x, z) ± radius` touches,
    /// in `EntityRef` order. Callers apply their exact distance rule. A radius
    /// up to the interest radius visits at most four cells.
    pub(crate) fn near(&self, x: i32, z: i32, radius: i32) -> Vec<EntityRef> {
        let radius = i64::from(radius.max(0));
        let width = i64::from(INTEREST_RADIUS_UNITS);
        let (x, z) = (i64::from(x), i64::from(z));
        let mut near = Vec::new();
        for cell_x in (x - radius).div_euclid(width)..=(x + radius).div_euclid(width) {
            for cell_z in (z - radius).div_euclid(width)..=(z + radius).div_euclid(width) {
                if let Some(entities) = self.cells.get(&(cell_x, cell_z)) {
                    near.extend(entities);
                }
            }
        }
        near.sort_unstable();
        near
    }

    fn cell(x: i32, z: i32) -> (i64, i64) {
        let width = i64::from(INTEREST_RADIUS_UNITS);
        (
            i64::from(x).div_euclid(width),
            i64::from(z).div_euclid(width),
        )
    }
}
