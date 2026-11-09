//! Static collision geometry shared by the physics world and authored content.

use crate::ZoneError;

/// Every collider bound and every spawned body lies within `±` this many units
/// on each axis. `ZoneSimulation` closes this cube with fixed world-limit
/// bodies, so no body admitted at a spawn slot leaves it, whatever the
/// content's walls or gravity (1 km, room for regions beyond Greyhaven Vale,
/// #424). Player projections carry positions as full `i32` units.
pub const MAX_CONTENT_COORDINATE_UNITS: i32 = 100_000;

pub const fn within_content_range(value: i32) -> bool {
    -MAX_CONTENT_COORDINATE_UNITS <= value && value <= MAX_CONTENT_COORDINATE_UNITS
}

/// Authored ground heights on a square grid (#159): `heights[row * columns + column]` is the
/// ground's height in units at `(origin[0] + column × step, origin[1] + row × step)`. The zone's
/// physics walks units on it; between samples the ground is bilinear.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Terrain {
    pub origin: [i32; 2],
    pub step: i32,
    pub columns: u16,
    pub rows: u16,
    pub heights: Vec<i32>,
}

/// Terrain heights stay within ±this many units (100 m).
pub const MAX_TERRAIN_HEIGHT_UNITS: i32 = 10_000;

impl Terrain {
    /// The ground height at `(x, z)` in units: bilinear between samples, clamped to the edge
    /// outside the grid.
    #[must_use]
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        let columns = i64::from(self.columns);
        let rows = i64::from(self.rows);
        let step = i64::from(self.step);
        let local = |value: i32, origin: i32, count: i64| {
            let offset = (i64::from(value) - i64::from(origin)).clamp(0, (count - 1) * step);
            let cell = (offset / step).min(count - 2);
            (cell, offset - cell * step)
        };
        let (column, fx) = local(x, self.origin[0], columns);
        let (row, fz) = local(z, self.origin[1], rows);
        let at =
            |c: i64, r: i64| i64::from(self.heights[usize::try_from(r * columns + c).unwrap_or(0)]);
        let top = at(column, row) * (step - fx) + at(column + 1, row) * fx;
        let bottom = at(column, row + 1) * (step - fx) + at(column + 1, row + 1) * fx;
        i32::try_from((top * (step - fz) + bottom * fz) / (step * step)).unwrap_or(0)
    }

    /// The lowest sample of the grid cells within `reach` units of `(x, z)` (a square): their
    /// ground, triangulated or bilinear, never dips below it.
    #[must_use]
    pub fn low_within(&self, x: i32, z: i32, reach: i32) -> i32 {
        let columns = i64::from(self.columns);
        let rows = i64::from(self.rows);
        let step = i64::from(self.step);
        let cell = |value: i64, origin: i32, count: i64| {
            ((value - i64::from(origin)).clamp(0, (count - 1) * step) / step).min(count - 2)
        };
        let span = |value: i32, origin: i32, count: i64| {
            let (value, reach) = (i64::from(value), i64::from(reach.max(0)));
            cell(value - reach, origin, count)..=cell(value + reach, origin, count) + 1
        };
        let mut low = i32::MAX;
        for row in span(z, self.origin[1], rows) {
            for column in span(x, self.origin[0], columns) {
                let index = usize::try_from(row * columns + column).unwrap_or(0);
                low = low.min(self.heights[index]);
            }
        }
        low
    }

    pub fn validate(&self) -> Result<(), ZoneError> {
        let samples = usize::from(self.columns) * usize::from(self.rows);
        let far = |origin: i32, count: u16| {
            i64::from(origin) + i64::from(count.saturating_sub(1)) * i64::from(self.step)
        };
        if self.columns < 2
            || self.rows < 2
            || self.step <= 0
            || self.heights.len() != samples
            || self
                .heights
                .iter()
                .any(|height| height.abs() > MAX_TERRAIN_HEIGHT_UNITS)
            || !within_content_range(self.origin[0])
            || !within_content_range(self.origin[1])
            || far(self.origin[0], self.columns) > i64::from(MAX_CONTENT_COORDINATE_UNITS)
            || far(self.origin[1], self.rows) > i64::from(MAX_CONTENT_COORDINATE_UNITS)
        {
            return Err(ZoneError::new(
                "terrain grid is empty, inconsistent or out of range",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StaticCollider {
    pub id: u32,
    pub position: [i32; 3],
    pub half_extents: [i32; 3],
}
