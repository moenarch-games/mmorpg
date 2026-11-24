//! The moving bodies by horizontal cell (#1149): where the engine arm's slide-around intent finds
//! a mover's neighbours, instead of scanning every moving body for each one.
//!
//! Kept on add, remove, placement and each move batch, so a body that does not move (an asleep
//! creature, a unit standing still) costs nothing per tick; it is examined only by a walker whose
//! reach covers its cell. A query returns every body whose cell the reach touches, in body-ID
//! order, so [`super::shared::slide_around`] sees the touching bodies it would see in a full scan,
//! in the same order, and turns the move identically.

use std::collections::{BTreeMap, BTreeSet};

use super::{BodyId, CONTACT_OFFSET_UNITS, RigidBody, capsule_dimensions};

/// A cell's side (units): a little over two player capsules' reach, so a walking player's query
/// covers two cells along each axis at most.
const CELL_UNITS: f64 = 128.0;
/// Extra reach (units) the query covers beyond the slide-around reach, far above the rounding of
/// the intent's own `f32` distance at any zone coordinate.
const MARGIN_UNITS: f64 = 4.0;

type Cell = (i32, i32);

fn cell_of(x: f64, z: f64) -> Cell {
    // `as` saturates (and maps NaN to 0): every position lands in some cell.
    #[allow(clippy::cast_possible_truncation)]
    let cell = |value: f64| (value / CELL_UNITS).floor() as i32;
    (cell(x), cell(z))
}

fn position_cell(position: [f32; 3]) -> Cell {
    cell_of(f64::from(position[0]), f64::from(position[2]))
}

fn radius(body: &RigidBody) -> f32 {
    capsule_dimensions(body.half_extents).1
}

#[derive(Clone, Debug, Default)]
pub(super) struct MoverGrid {
    cells: BTreeMap<Cell, BTreeSet<BodyId>>,
    placed: BTreeMap<BodyId, Cell>,
    /// Capsule radii (as bits; non-negative, so bit order is numeric order) → bodies with that
    /// radius: the largest bounds a query's reach.
    radii: BTreeMap<u32, usize>,
}

impl MoverGrid {
    pub fn insert(&mut self, body: &RigidBody) {
        let cell = position_cell(body.position);
        if let Some(old) = self.placed.insert(body.id, cell) {
            self.leave(body.id, old);
        } else {
            *self.radii.entry(radius(body).to_bits()).or_default() += 1;
        }
        self.cells.entry(cell).or_default().insert(body.id);
    }

    pub fn remove(&mut self, body: &RigidBody) {
        let Some(cell) = self.placed.remove(&body.id) else {
            return;
        };
        self.leave(body.id, cell);
        let key = radius(body).to_bits();
        if let Some(count) = self.radii.get_mut(&key) {
            *count -= 1;
            if *count == 0 {
                self.radii.remove(&key);
            }
        }
    }

    /// Moves `id`, already indexed, to the cell of `position`.
    pub fn moved(&mut self, id: BodyId, position: [f32; 3]) {
        let cell = position_cell(position);
        let Some(old) = self.placed.get_mut(&id) else {
            return;
        };
        if *old == cell {
            return;
        }
        let old = std::mem::replace(old, cell);
        self.leave(id, old);
        self.cells.entry(cell).or_default().insert(id);
    }

    fn leave(&mut self, id: BodyId, cell: Cell) {
        if let Some(ids) = self.cells.get_mut(&cell) {
            ids.remove(&id);
            if ids.is_empty() {
                self.cells.remove(&cell);
            }
        }
    }

    /// Every indexed body (`body` itself included) whose cell lies within `body`'s slide-around
    /// reach of any indexed capsule, into `out` in body-ID order.
    pub fn near(&self, body: &RigidBody, out: &mut Vec<BodyId>) {
        out.clear();
        let Some(&widest) = self.radii.keys().next_back() else {
            return;
        };
        let reach = f64::from(radius(body))
            + f64::from(f32::from_bits(widest))
            + f64::from(CONTACT_OFFSET_UNITS)
            + 1.0
            + MARGIN_UNITS;
        let (x, z) = (f64::from(body.position[0]), f64::from(body.position[2]));
        let (low, high) = (cell_of(x - reach, z - reach), cell_of(x + reach, z + reach));
        for column in low.0..=high.0 {
            for (_, ids) in self.cells.range((column, low.1)..=(column, high.1)) {
                out.extend(ids);
            }
        }
        out.sort_unstable();
    }
}

#[cfg(test)]
mod tests {
    use super::super::{SUBUNITS_PER_UNIT, Vec3i, shared::slide_around};
    use super::*;

    /// A crowd of mixed capsules, some touching, some just out of reach, across cell borders and
    /// on both sides of the origin.
    fn crowd() -> Vec<RigidBody> {
        let mut seed = 0x9e37_79b9_u32;
        let mut next = move |span: i32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            i32::try_from(seed % u32::try_from(2 * span + 1).unwrap()).unwrap() - span
        };
        (0..400)
            .map(|index| {
                let half = match index % 4 {
                    0 => Vec3i::new(30, 90, 30),
                    1 => Vec3i::new(60, 60, 45),
                    2 => Vec3i::new(20, 40, 20),
                    _ => Vec3i::new(150, 200, 120),
                };
                let position = Vec3i::new(next(700), next(40) + 100, next(700));
                let velocity =
                    Vec3i::new(next(6) * SUBUNITS_PER_UNIT, 0, next(6) * SUBUNITS_PER_UNIT);
                RigidBody::dynamic(BodyId(index + 1), position, velocity, half)
            })
            .collect()
    }

    #[test]
    fn neighbours_turn_every_move_as_a_full_scan_does() {
        let bodies = crowd();
        let mut grid = MoverGrid::default();
        for body in &bodies {
            grid.insert(body);
        }
        let by_id: BTreeMap<_, _> = bodies.iter().map(|body| (body.id, body)).collect();
        let mut near = Vec::new();
        let (mut scanned, mut indexed, mut turned) = (0, 0, 0);
        for body in &bodies {
            let full = slide_around(&bodies, body, body.velocity, &mut scanned);
            grid.near(body, &mut near);
            let local = slide_around(
                near.iter().map(|id| by_id[id]),
                body,
                body.velocity,
                &mut indexed,
            );
            assert_eq!(
                full.map(f32::to_bits),
                local.map(f32::to_bits),
                "{:?}",
                body.id
            );
            turned += usize::from(full != body.velocity);
        }
        assert!(turned > 20, "the crowd exercises the intent: {turned}");
        assert!(indexed * 4 < scanned, "{indexed} of {scanned}");
    }

    #[test]
    fn a_moved_or_removed_body_leaves_its_cell() {
        let bodies = crowd();
        let mut grid = MoverGrid::default();
        for body in &bodies[..2] {
            grid.insert(body);
        }
        let mut near = Vec::new();
        grid.moved(bodies[1].id, [5_000.0, 0.0, 5_000.0]);
        grid.near(&bodies[0], &mut near);
        assert_eq!(near, [bodies[0].id]);
        grid.remove(&bodies[1]);
        grid.remove(&bodies[0]);
        assert!(grid.cells.is_empty() && grid.placed.is_empty() && grid.radii.is_empty());
    }
}
