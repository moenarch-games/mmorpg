//! Creature navigation (#307): a creature whose straight way to where it is going is blocked
//! by static geometry (a wall, a structure piece, rubble, a closed door) walks a route around it
//! instead, found on a coarse grid of the physical world.
//!
//! - **Blocked**: the creature's capsule, raised to knee height like a charge's (#57), swept
//!   straight toward its goal meets a static body other than the ground or a moving platform.
//! - **Grid**: 1 m cells; a cell is free when the raised capsule standing on it overlaps no
//!   such body. Cell results are cached per body size and dropped whenever static geometry
//!   changes (a door moves, a structure piece breaks or regrows), so the cache always equals
//!   a fresh query of the current world: a restored zone routes exactly as the original.
//! - **Route**: A* over at most a 64 × 64-cell window around start and goal, 8-connected
//!   (diagonals only past two free sides), integer costs, ties by cell order. The creature
//!   steers at the farthest of the next few route cells it can reach in a straight line.
//!
//! Physics owns every geometric question (shape casts and overlap queries on the zone's world);
//! gameplay owns the rule of which way to walk.

use std::collections::{BTreeMap, BTreeSet};

use crate::physics::{BodyId, Vec3i};
use crate::{ZoneError, ZoneSimulation};

/// Side of a navigation cell (1 m).
pub const NAV_CELL_UNITS: i32 = 100;
/// Cells around start and goal a route may use.
const NAV_MARGIN_CELLS: i32 = 8;
/// Widest window a route searches, in cells per axis.
const NAV_MAX_SPAN_CELLS: i32 = 64;
/// Route cells ahead the creature may steer straight at.
const NAV_LOOKAHEAD_CELLS: usize = 6;

/// Cached cell results: `(cell x, cell z, body half-width, body half-height)` → free.
pub(crate) type NavCache = BTreeMap<(i32, i32, i32, i32), bool>;

fn cell_of(at: [i32; 2]) -> (i32, i32) {
    (
        at[0].div_euclid(NAV_CELL_UNITS),
        at[1].div_euclid(NAV_CELL_UNITS),
    )
}

fn centre_of((x, z): (i32, i32)) -> [i32; 2] {
    [
        x * NAV_CELL_UNITS + NAV_CELL_UNITS / 2,
        z * NAV_CELL_UNITS + NAV_CELL_UNITS / 2,
    ]
}

impl ZoneSimulation {
    /// Whether `body` blocks a walking unit: static geometry other than the ground and moving
    /// platforms (which carry units rather than block them).
    fn nav_blocker(&self, body: BodyId) -> bool {
        body.0 < crate::entity::PLAYER_BODY_BASE
            && Some(body) != self.ground_body()
            && !self
                .content
                .platforms()
                .iter()
                .any(|platform| u64::from(platform.body) == body.0)
    }

    /// Static geometry changed: every cached cell result is stale.
    pub(crate) fn invalidate_navigation(&mut self) {
        self.nav_cache.clear();
    }

    /// The raised capsule's half extents for a body of `half`: its bottom at knee height.
    fn raised(half: Vec3i) -> (Vec3i, i32) {
        let lift = crate::forced::CHARGE_CLEARANCE_UNITS / 2;
        (
            Vec3i::new(half.x, (half.y - lift).max(half.x), half.z),
            lift,
        )
    }

    /// Whether a body of `half` centred on `centre` walks straight to `to` (XZ), stopping
    /// `short` units before it, without meeting a blocker.
    pub(crate) fn straight_way_clear(
        &self,
        centre: [f32; 3],
        half: Vec3i,
        to: [i32; 2],
        short: i32,
    ) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let delta = [to[0] as f32 - centre[0], to[1] as f32 - centre[2]];
        let distance = delta[0].hypot(delta[1]);
        #[allow(clippy::cast_precision_loss)]
        let travel = distance - short as f32;
        if travel <= 0.0 {
            return true;
        }
        let (raised, lift) = Self::raised(half);
        #[allow(clippy::cast_precision_loss)]
        let origin = [centre[0], centre[1] + lift as f32, centre[2]];
        let scale = travel / distance;
        self.world
            .cast_shape(
                origin,
                raised,
                [delta[0] * scale, 0.0, delta[1] * scale],
                |body| self.nav_blocker(body),
            )
            .is_none()
    }

    /// Whether a body of `half` fits standing on `cell`.
    fn cell_free(&mut self, cell: (i32, i32), half: Vec3i) -> Result<bool, ZoneError> {
        let key = (cell.0, cell.1, half.x.max(half.z), half.y);
        if let Some(&free) = self.nav_cache.get(&key) {
            return Ok(free);
        }
        let [x, z] = centre_of(cell);
        let (raised, lift) = Self::raised(half);
        let feet = self.ground_y(x, z);
        let centre = Vec3i::new(x, feet + half.y + lift, z);
        let width = half.x.max(half.z);
        // Half a unit inside the body's box, so content boxes on the whole-unit grid never
        // merely touch it: whether touching counts differs between the backends.
        #[allow(clippy::cast_precision_loss)]
        let (centre, half) = (
            [centre.x as f32, centre.y as f32, centre.z as f32],
            [
                width as f32 - 0.5,
                raised.y as f32 - 0.5,
                width as f32 - 0.5,
            ],
        );
        let blocked = self
            .world
            .overlap_box(centre, half)
            .map_err(crate::zone::physics_error)?
            .into_iter()
            .any(|body| self.nav_blocker(body));
        self.nav_cache.insert(key, !blocked);
        Ok(!blocked)
    }

    /// Where a body of `half` centred on `centre` should steer to reach `goal` (XZ) when its
    /// straight way is blocked: a point on an A* route around the blockers, or `None` when the
    /// way is clear or no route exists within the window.
    pub(crate) fn route_waypoint(
        &mut self,
        centre: [f32; 3],
        half: Vec3i,
        goal: [i32; 2],
        short: i32,
    ) -> Result<Option<[i32; 2]>, ZoneError> {
        if self.straight_way_clear(centre, half, goal, short) {
            return Ok(None);
        }
        #[allow(clippy::cast_possible_truncation)]
        let here = [centre[0].round() as i32, centre[2].round() as i32];
        let (start, end) = (cell_of(here), cell_of(goal));
        let min = (
            start.0.min(end.0) - NAV_MARGIN_CELLS,
            start.1.min(end.1) - NAV_MARGIN_CELLS,
        );
        let max = (
            start.0.max(end.0) + NAV_MARGIN_CELLS,
            start.1.max(end.1) + NAV_MARGIN_CELLS,
        );
        if max.0 - min.0 > NAV_MAX_SPAN_CELLS || max.1 - min.1 > NAV_MAX_SPAN_CELLS {
            return Ok(None);
        }
        let inside = |(x, z): (i32, i32)| x >= min.0 && x <= max.0 && z >= min.1 && z <= max.1;
        let heuristic = |(x, z): (i32, i32)| {
            let (dx, dz) = ((x - end.0).abs(), (z - end.1).abs());
            10 * dx.max(dz) + 4 * dx.min(dz)
        };
        let mut open = BTreeSet::from([(heuristic(start), 0, start)]);
        let mut best = BTreeMap::from([(start, 0)]);
        let mut came = BTreeMap::new();
        let mut found = false;
        while let Some((_, cost, cell)) = open.pop_first() {
            if cell == end {
                found = true;
                break;
            }
            if best.get(&cell).is_some_and(|&known| known < cost) {
                continue;
            }
            for (dx, dz) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let next = (cell.0 + dx, cell.1 + dz);
                if !inside(next) || (next != end && !self.cell_free(next, half)?) {
                    continue;
                }
                let diagonal = dx != 0 && dz != 0;
                if diagonal
                    && (!self.cell_free((cell.0 + dx, cell.1), half)?
                        || !self.cell_free((cell.0, cell.1 + dz), half)?)
                {
                    continue;
                }
                let step = if diagonal { 14 } else { 10 };
                let total = cost + step;
                if best.get(&next).is_none_or(|&known| total < known) {
                    best.insert(next, total);
                    came.insert(next, cell);
                    open.insert((total + heuristic(next), total, next));
                }
            }
        }
        if !found {
            return Ok(None);
        }
        let mut route = vec![end];
        while let Some(&previous) = came.get(route.last().unwrap_or(&end)) {
            if previous == start {
                break;
            }
            route.push(previous);
        }
        route.reverse();
        // The farthest of the next few route cells reachable straight; else the next one.
        let ahead = route.iter().take(NAV_LOOKAHEAD_CELLS).copied();
        let waypoint = ahead
            .clone()
            .rev()
            .find(|&cell| self.straight_way_clear(centre, half, centre_of(cell), 0))
            .or_else(|| route.first().copied());
        Ok(waypoint.map(centre_of))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_cover_negative_and_positive_units() {
        assert_eq!(cell_of([0, 99]), (0, 0));
        assert_eq!(cell_of([-1, 100]), (-1, 1));
        assert_eq!(centre_of((-1, 1)), [-50, 150]);
    }
}
