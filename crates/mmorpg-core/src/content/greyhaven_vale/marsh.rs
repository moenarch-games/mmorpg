//! Marsh-Lake Wetlands (#426, Atlas 5.1.85–5.1.86): the third major region beyond Greyhaven
//! Vale, low wetlands east of the Vale beyond Eastwater Fen and Stillwater Lake, about as large
//! as the Vale itself (424 m × 416 m). Like the farmland and the highlands it stays in the
//! Greyhaven zone, so no zone boundary or loading step lies on the way.
//!
//! A mountain ridge (24–36 m) closes the Vale's eastern side behind Eastwater Fen. The Marsh
//! Pass leaves the Vale Ring Road eastwards and winds through it: a cutting into its western
//! face, a gorge climbing north along its crest, a level gorge east across it, a gorge falling
//! back south and a cutting east down onto the wetlands, 4 m above the Vale's floor. Every face
//! of the ridge is steeper than the controllers climb (45°), so the pass is the only way across.
//! Its foot and the pass start 4 m beyond the Vale's old eastern wall, past the edge of the
//! ground the Vale had, so the Vale's own heightfield tile and everything that moves on it stay
//! as they were.
//!
//! Beyond the ridge low hummocks of wet ground surround a lake and four marsh pools, all real
//! water volumes (#274): the pools' rims are ankle-deep, two are deep enough to swim, and the
//! Reed Path wades straight through the shallowest. The landmark is a drowned watchtower on an
//! island in the lake, ringed by standing stones and reached along a raised causeway. Every
//! height is sampled on the terrain grid's 4 m lattice.

use super::basin::{Along, Lane};
use super::{smooth_q10, value_noise_q10};
use crate::{WaterVolume, XzBounds};

/// The region: the ridge and the wetlands, out to the boundary walls.
pub const BOUNDS: XzBounds = XzBounds {
    min: [20_800, -20_800],
    max: [63_200, 20_800],
};
/// The wetlands' base level above the Vale's floor (4 m).
pub const FLOOR_Y: i32 = 400;
/// Every marsh water's surface: 0.15 m above the wetlands' level shores.
pub const SURFACE_Y: i32 = FLOOR_Y + 15;
/// The ridge (X, eastwards): its foot, the top of its western face, the top of its eastern face
/// and the wetlands at its eastern foot.
pub const RIDGE: [i32; 4] = [21_200, 22_000, 24_400, 25_200];
/// The ridge's crest rises this high (24 m) …
pub const CREST_UNITS: i64 = 2_400;
/// … plus up to this much more (12 m) along its length.
pub const CREST_VARIATION_UNITS: i64 = 1_200;
/// The wet ground rises in low hummocks by up to this much (1.2 m) …
pub const HUMMOCK_UNITS: i64 = 120;
/// … and this much (0.4 m) in tussocks.
pub const TUSSOCK_UNITS: i64 = 40;
const SEED: u64 = 0x4d41_5253_484c_414b;

/// The Marsh Pass: the cutting into the ridge's western face, the gorge climbing north, the
/// level gorge east across the crest, the gorge falling back south and the cutting east down
/// onto the wetlands, in that order. Each lane is three samples (8 m) wide.
pub const PASS: [Lane; 5] = [
    Lane {
        min: [21_200, 3_600],
        max: [22_000, 4_400],
        heights: [0, 100],
        along: Along::X,
    },
    Lane {
        min: [22_000, -400],
        max: [22_800, 4_400],
        heights: [800, 100],
        along: Along::Z,
    },
    Lane {
        min: [22_000, -1_200],
        max: [24_400, -400],
        heights: [800, 800],
        along: Along::X,
    },
    Lane {
        min: [23_600, -400],
        max: [24_400, 2_000],
        heights: [800, 600],
        along: Along::Z,
    },
    Lane {
        min: [23_600, 2_000],
        max: [25_200, 2_800],
        heights: [600, FLOOR_Y],
        along: Along::X,
    },
];

/// A body of marsh water: an ellipse whose bed falls linearly from its rim to `depth` units
/// below the shore at its centre, starting at `rim_q10` of the ellipse (Q10); outside that the
/// water stays ankle-deep and level.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Water {
    pub name: &'static str,
    pub centre: [i32; 2],
    pub radii: [i32; 2],
    pub depth: i64,
    pub rim_q10: i64,
}

impl Water {
    /// The normalised ellipse distance of `(x, z)` in Q10: 1 024 on the shore.
    #[must_use]
    pub fn distance_q10(self, x: i32, z: i32) -> i64 {
        let dx = i128::from(x) - i128::from(self.centre[0]);
        let dz = i128::from(z) - i128::from(self.centre[1]);
        let (rx, rz) = (i128::from(self.radii[0]), i128::from(self.radii[1]));
        let squared = (dx * dx * rz * rz + dz * dz * rx * rx) * (1 << 20) / (rx * rx * rz * rz);
        i64::try_from(squared.isqrt()).unwrap_or(i64::MAX)
    }

    /// How far (units) the bed lies below the shore at `(x, z)`.
    #[must_use]
    pub fn depth_at(self, x: i32, z: i32) -> i64 {
        let distance = self.distance_q10(x, z);
        if distance >= self.rim_q10 {
            return 0;
        }
        self.depth * (self.rim_q10 - distance) / self.rim_q10
    }

    /// Roughly how far (units) `(x, z)` lies outside the shore: zero inside.
    fn outside(self, x: i32, z: i32) -> i64 {
        let radius = i64::from(self.radii[0].min(self.radii[1]));
        ((self.distance_q10(x, z) - 1_024) * radius / 1_024).max(0)
    }

    /// The authoritative water volume (#274) clients draw and units wade and swim in.
    #[must_use]
    pub fn volume(self) -> WaterVolume {
        WaterVolume {
            name: self.name.to_owned(),
            centre: self.centre,
            radii: self.radii,
            surface: SURFACE_Y,
            air_pockets: Vec::new(),
        }
    }
}

/// The lake round the landmark island (200 m × 160 m), 6 m deep at its middle.
pub const LAKE: Water = Water {
    name: "Marsh Lake",
    centre: [46_000, -2_000],
    radii: [10_000, 8_000],
    depth: 600,
    rim_q10: 717,
};

/// The lake and the marsh pools.
pub const WATERS: [Water; 5] = [
    LAKE,
    // The shallow ford the Reed Path wades through.
    Water {
        name: "Reed Ford",
        centre: [31_000, -9_000],
        radii: [1_800, 1_200],
        depth: 80,
        rim_q10: 1_024,
    },
    // A deep pool north of the trail, by the ruined cottage.
    Water {
        name: "Cottage Pool",
        centre: [33_000, 11_000],
        radii: [2_400, 1_600],
        depth: 250,
        rim_q10: 870,
    },
    // A shallow pool at the Heron Path's end.
    Water {
        name: "Heron Pool",
        centre: [52_000, 14_000],
        radii: [2_000, 2_000],
        depth: 100,
        rim_q10: 1_024,
    },
    // A deep pool at the Reed Path's end.
    Water {
        name: "Southmere",
        centre: [55_000, -15_000],
        radii: [2_600, 1_800],
        depth: 200,
        rim_q10: 870,
    },
];

/// The authoritative water volumes of [`WATERS`].
#[must_use]
pub fn water_volumes() -> Vec<WaterVolume> {
    WATERS.iter().map(|water| water.volume()).collect()
}

/// The island's level ground above the Vale's floor (5 m), out to this radius (20 m) from the
/// lake's centre …
pub const ISLAND_Y: i32 = 500;
pub const ISLAND_RADIUS: i64 = 2_000;
/// … above the lake's surface: 0.85 m.
const _: () = assert!(ISLAND_Y > SURFACE_Y);

/// The causeway: a ramp up from the lake's western shore and a raised way across the water to
/// the island, three samples (8 m) wide, 0.45 m above the water.
pub const CAUSEWAY: [Lane; 2] = [
    Lane {
        min: [34_400, -2_400],
        max: [36_000, -1_600],
        heights: [FLOOR_Y, 460],
        along: Along::X,
    },
    Lane {
        min: [36_000, -2_400],
        max: [44_000, -1_600],
        heights: [460, ISLAND_Y],
        along: Along::X,
    },
];

/// What a marsh structure looks like.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Building {
    /// The drowned keep-style watchtower: the landmark.
    Tower,
    /// A standing stone round it.
    Stone,
    /// A ruined cottage.
    Ruin,
}

/// The island and the cottage: `(kind, footprint min, footprint max, height)`.
pub const BUILDINGS: [(Building, [i32; 2], [i32; 2], i32); 7] = [
    // The landmark: a watchtower 8 m square and 18 m tall in the island's middle.
    (Building::Tower, [45_600, -2_400], [46_400, -1_600], 1_800),
    // Five standing stones round it, open towards the causeway in the west.
    (Building::Stone, [47_460, -2_040], [47_540, -1_960], 240),
    (Building::Stone, [46_710, -740], [46_790, -660], 240),
    (Building::Stone, [45_210, -740], [45_290, -660], 240),
    (Building::Stone, [45_210, -3_340], [45_290, -3_260], 240),
    (Building::Stone, [46_710, -3_340], [46_790, -3_260], 240),
    // The ruined cottage east of the Cottage Pool.
    (Building::Ruin, [36_400, 12_400], [37_200, 13_000], 300),
];

/// Whether a tree may stand at `(x, z)`: 2 m clear of every water's shore, the causeway and
/// the cottage.
#[must_use]
pub fn clear_for_tree(x: i32, z: i32) -> bool {
    const MARGIN: i32 = 200;
    let outside = |min: [i32; 2], max: [i32; 2]| {
        x < min[0] - MARGIN || x > max[0] + MARGIN || z < min[1] - MARGIN || z > max[1] + MARGIN
    };
    WATERS
        .iter()
        .all(|water| water.outside(x, z) > i64::from(MARGIN))
        && CAUSEWAY.iter().all(|lane| outside(lane.min, lane.max))
        && BUILDINGS.iter().all(|&(_, min, max, _)| outside(min, max))
}

/// The region's ground at `(x, z)`, or `None` west of the ridge.
#[must_use]
pub fn height_at(x: i32, z: i32) -> Option<i32> {
    if let Some(lane) = PASS
        .iter()
        .chain(&CAUSEWAY)
        .find(|lane| lane.contains(x, z))
    {
        return Some(lane.height(x, z));
    }
    if x < RIDGE[0] {
        return None;
    }
    let (x64, z64) = (i64::from(x), i64::from(z));
    let crest = CREST_UNITS
        + CREST_VARIATION_UNITS * value_noise_q10(x64, z64, 3_000, SEED.rotate_left(29)) / 1_024;
    let wetlands = wetlands(x, z);
    let [foot, west_top, east_top, east_foot] = RIDGE.map(i64::from);
    let height = if x64 < west_top {
        crest * (x64 - foot) / (west_top - foot)
    } else if x64 <= east_top {
        crest
    } else if x64 < east_foot {
        crest + (wetlands - crest) * (x64 - east_top) / (east_foot - east_top)
    } else {
        wetlands
    };
    Some(i32::try_from(height).unwrap_or(0))
}

/// The wetlands east of the ridge: hummocks over [`FLOOR_Y`] that flatten towards every
/// water's shore, the waters' beds below it and the island in the lake.
fn wetlands(x: i32, z: i32) -> i64 {
    let (x64, z64) = (i64::from(x), i64::from(z));
    let shore = WATERS
        .iter()
        .map(|water| water.outside(x, z))
        .min()
        .unwrap_or(i64::MAX);
    let dry = smooth_q10(shore, 400, 2_000);
    let entry = smooth_q10(x64, i64::from(RIDGE[3]), i64::from(RIDGE[3]) + 2_400);
    let wall = (i64::from(BOUNDS.max[0]) - x64).min(i64::from(BOUNDS.max[1]) - z64.abs());
    let settle = smooth_q10(wall, 400, 2_400);
    let hummocks = HUMMOCK_UNITS * value_noise_q10(x64, z64, 2_400, SEED) / 1_024
        + TUSSOCK_UNITS * value_noise_q10(x64, z64, 1_000, SEED.rotate_left(13)) / 1_024;
    let bed = WATERS
        .iter()
        .map(|water| water.depth_at(x, z))
        .max()
        .unwrap_or(0);
    let ground = i64::from(FLOOR_Y) + hummocks * dry / 1_024 * entry / 1_024 * settle / 1_024 - bed;
    // The island rises out of the lake and shelves (1:2) into its water.
    let (dx, dz) = (
        x64 - i64::from(LAKE.centre[0]),
        z64 - i64::from(LAKE.centre[1]),
    );
    let radius = (dx * dx + dz * dz).isqrt();
    let island = i64::from(ISLAND_Y) - (radius - ISLAND_RADIUS).max(0) / 2;
    ground.max(island)
}
