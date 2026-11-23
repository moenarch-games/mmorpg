//! Derived XZ broad phase for interest policy and creature perception,
//! never for collision or authority.

use std::collections::HashMap;

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
/// A fixed hasher, so the grid's tables are the same in every process. They are only looked up,
/// never iterated, so no hash order reaches a result.
type Fixed = std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

/// Every visible unit (players, living creatures, corpses and NPCs) in cells
/// one interest radius wide.
///
/// A flat grid (#1086): each occupied cell owns one bucket in a flat list, kept sorted by
/// `EntityRef`, and a cell whose units left keeps its emptied bucket, so moving units back and
/// forth allocates nothing once warm. Queries fill a caller's buffer ([`Self::near_into`]).
#[derive(Default)]
pub(crate) struct InterestIndex {
    cells: HashMap<(i64, i64), usize, Fixed>,
    buckets: Vec<Vec<EntityRef>>,
    memberships: HashMap<EntityRef, (i64, i64), Fixed>,
}

impl InterestIndex {
    pub(crate) fn insert(&mut self, entity: EntityRef, x: i32, z: i32) {
        let cell = Self::cell(x, z);
        debug_assert!(!self.memberships.contains_key(&entity));
        let next = self.buckets.len();
        let bucket = *self.cells.entry(cell).or_insert(next);
        if bucket == next {
            self.buckets.push(Vec::new());
        }
        let entities = &mut self.buckets[bucket];
        if let Err(at) = entities.binary_search(&entity) {
            entities.insert(at, entity);
        }
        self.memberships.insert(entity, cell);
    }

    /// Returns whether the unit was indexed.
    pub(crate) fn remove(&mut self, entity: EntityRef) -> bool {
        let Some(cell) = self.memberships.remove(&entity) else {
            return false;
        };
        if let Some(&bucket) = self.cells.get(&cell) {
            let entities = &mut self.buckets[bucket];
            if let Ok(at) = entities.binary_search(&entity) {
                entities.remove(at);
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

    /// The units of one cell, in `EntityRef` order.
    fn bucket(&self, cell: (i64, i64)) -> &[EntityRef] {
        self.cells
            .get(&cell)
            .map_or(&[], |&bucket| self.buckets[bucket].as_slice())
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
                candidates.extend_from_slice(self.bucket((cell_x, cell_z)));
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
        let mut near = Vec::new();
        self.near_into(x, z, radius, &mut near);
        near
    }

    /// [`Self::near`] into `out`, which it clears first: a warm buffer makes the query
    /// allocation-free (#1086).
    pub(crate) fn near_into(&self, x: i32, z: i32, radius: i32, out: &mut Vec<EntityRef>) {
        let radius = i64::from(radius.max(0));
        let (x, z) = (i64::from(x), i64::from(z));
        self.box_into([x - radius, x + radius], [z - radius, z + radius], out);
    }

    /// Every unit indexed in a cell that the box `x[0]..=x[1]` by `z[0]..=z[1]` touches, in
    /// `EntityRef` order, into `out`, which it clears first.
    pub(crate) fn box_into(&self, x: [i64; 2], z: [i64; 2], out: &mut Vec<EntityRef>) {
        let width = i64::from(INTEREST_RADIUS_UNITS);
        out.clear();
        for cell_x in x[0].div_euclid(width)..=x[1].div_euclid(width) {
            for cell_z in z[0].div_euclid(width)..=z[1].div_euclid(width) {
                out.extend_from_slice(self.bucket((cell_x, cell_z)));
            }
        }
        out.sort_unstable();
    }

    /// Whether any player is indexed in a cell that the square `(x, z) ± radius` touches: the
    /// test a creature wakes by (#814), without gathering anyone.
    pub(crate) fn player_near(&self, x: i32, z: i32, radius: i32) -> bool {
        let radius = i64::from(radius.max(0));
        let width = i64::from(INTEREST_RADIUS_UNITS);
        let (x, z) = (i64::from(x), i64::from(z));
        for cell_x in (x - radius).div_euclid(width)..=(x + radius).div_euclid(width) {
            for cell_z in (z - radius).div_euclid(width)..=(z + radius).div_euclid(width) {
                if self
                    .bucket((cell_x, cell_z))
                    .iter()
                    .any(|entity| matches!(entity, EntityRef::Player(_)))
                {
                    return true;
                }
            }
        }
        false
    }

    /// The cell `entity` is indexed in.
    pub(crate) fn cell_of(&self, entity: EntityRef) -> Option<(i64, i64)> {
        self.memberships.get(&entity).copied()
    }

    /// The cell width, in units.
    pub(crate) const WIDTH: i64 = INTEREST_RADIUS_UNITS as i64;

    fn cell(x: i32, z: i32) -> (i64, i64) {
        let width = i64::from(INTEREST_RADIUS_UNITS);
        (
            i64::from(x).div_euclid(width),
            i64::from(z).div_euclid(width),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CreatureId;

    fn creature(id: u32) -> EntityRef {
        EntityRef::Creature(CreatureId::new(id))
    }

    #[test]
    fn near_matches_a_scan_and_a_warm_buffer_keeps_its_capacity() {
        let mut index = InterestIndex::default();
        let mut placed = Vec::new();
        for id in 0..200_u32 {
            let value = i32::try_from(id).unwrap();
            let at = [(value * 7_919) % 40_000 - 20_000, (value * 104_729) % 40_000 - 20_000];
            index.insert(creature(id), at[0], at[1]);
            placed.push((creature(id), at));
        }
        index.insert(EntityRef::Player(1), 100, 100);
        placed.push((EntityRef::Player(1), [100, 100]));
        let mut out = Vec::with_capacity(256);
        let capacity = out.capacity();
        for (x, z, radius) in [(0, 0, 6_000), (-15_000, 9_000, 4_500), (3, -3, 0)] {
            index.near_into(x, z, radius, &mut out);
            let width = i64::from(INTEREST_RADIUS_UNITS);
            let span = |v: i32| {
                let (v, r) = (i64::from(v), i64::from(radius));
                (v - r).div_euclid(width)..=(v + r).div_euclid(width)
            };
            let mut expected: Vec<_> = placed
                .iter()
                .filter(|(_, at)| {
                    let (cx, cz) = InterestIndex::cell(at[0], at[1]);
                    span(x).contains(&cx) && span(z).contains(&cz)
                })
                .map(|(entity, _)| *entity)
                .collect();
            expected.sort_unstable();
            assert_eq!(out, expected);
            assert_eq!(
                index.player_near(x, z, radius),
                expected.iter().any(|e| matches!(e, EntityRef::Player(_)))
            );
        }
        assert_eq!(out.capacity(), capacity, "the warm buffer is reused");
        // Moving a unit away and back reuses both buckets.
        assert!(index.move_if_needed(creature(3), 30_000, 30_000));
        assert!(index.move_if_needed(creature(3), placed[3].1[0], placed[3].1[1]));
        assert!(index.remove(creature(3)));
        assert!(!index.remove(creature(3)));
    }
}
