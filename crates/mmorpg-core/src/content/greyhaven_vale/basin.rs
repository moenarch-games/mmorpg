//! Greyhaven's village basin (#423, Atlas 5.1.84): Millbrook Farm (slice 1) and Stillwater Lake
//! with the Sunken Shrine (slice 2) stand on one terrace 11 m above the Outpost's valley floor,
//! behind cliffs the controllers cannot climb (a 4 m grid cell rising 11 m is 70°; they climb
//! at most 45°). On top the farm and the lake join along the Stillwater Rim Trail. Three ways
//! down are cut into the terrain itself:
//!
//! - **Millbrook Climb**, the road: east from the Outpost's gate, north along a ledge in the
//!   cliff face, round a hairpin landing and back south up a cutting onto the terrace
//!   (about 1:5, 11°).
//! - **Millbrook Steps**, a steeper footpath (about 1:3.6, 15°): south from the farmyard down a
//!   second ledge to the valley floor, a return that does not use the road.
//! - **The Lakeshore Climb**, the Lakeshore Path (about 1:6, then 1:4.5): from the Hollow Road
//!   north along a ledge in the lake terrace's west cliff, round a landing and back south up a
//!   cutting onto the lake's west shore.
//!
//! The lake's water and basin, the shrine's plaza and its flooded cave all lie below the
//! terrace's ground, as they lay below the old flat valley. To the east the terrace falls
//! gently into the Eastwater Fen hills, where the Fen Track leaves it. Every height here is
//! sampled on the terrain grid's 4 m lattice, so each lane is exactly two samples wide and flat
//! across.

/// The terrace's height above the valley floor (11 m).
pub const TERRACE_Y: i32 = 1_100;
/// The basin cliff, west of the farm terrace: the valley floor up to the first X, the terrace
/// from the second.
pub const WEST_CLIFF: [i32; 2] = [4_400, 4_800];
/// The farm terrace's northern and southern cliffs (Z): floor to terrace, terrace to floor.
pub const NORTH_CLIFF: [i32; 2] = [-2_800, -2_400];
pub const SOUTH_CLIFF: [i32; 2] = [9_600, 10_000];
/// East of the farm and the lake the terrace descends gently (17°) into the Fen hills, beyond
/// the Sunken Shrine's cave.
pub const EAST_SLOPE: [i32; 2] = [12_800, 16_400];
/// The lake terrace's west cliff (X), north of the basin: floor, then terrace.
pub const LAKE_WEST_CLIFF: [i32; 2] = [1_600, 2_000];
/// The lake terrace's northern cliff (Z): floor, then terrace. West of [`HOLLOW_WALL_X`] it
/// stops short of Redbrand Hollow's camp, behind the Lakeshore Climb's landing …
pub const LAKE_NORTH_CLIFF: [i32; 2] = [-6_800, -6_400];
/// … and east of it, behind the Hollow's eastern rock wall, it reaches further north.
pub const LAKE_FAR_NORTH_CLIFF: [i32; 2] = [-8_400, -8_000];
/// The X from which the lake terrace takes [`LAKE_FAR_NORTH_CLIFF`]: the far side of the
/// Hollow's eastern rock wall, which hides the step between the two.
pub const HOLLOW_WALL_X: i32 = 3_200;
/// … and its southern cliff over the basin's northern apron: terrace, then floor. East of the
/// farm's west cliff the lake terrace runs on into the farm terrace.
pub const LAKE_SOUTH_CLIFF: [i32; 2] = [-2_800, -2_400];

/// A lane of the terrain lattice: samples in `[min, max]` (inclusive) take a height that
/// varies linearly along Z, from `heights[0]` at `min[1]` to `heights[1]` at `max[1]`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lane {
    pub min: [i32; 2],
    pub max: [i32; 2],
    pub heights: [i32; 2],
}

impl Lane {
    #[must_use]
    pub const fn contains(self, x: i32, z: i32) -> bool {
        self.min[0] <= x && x <= self.max[0] && self.min[1] <= z && z <= self.max[1]
    }

    fn height(self, z: i32) -> i32 {
        let [low, high] = self.heights.map(i64::from);
        let (start, span) = (i64::from(self.min[1]), i64::from(self.max[1] - self.min[1]));
        let height = low + (high - low) * (i64::from(z) - start) / span.max(1);
        i32::try_from(height).unwrap_or(0)
    }
}

/// Height of the Millbrook Climb's hairpin landing (6.4 m).
pub const LANDING_Y: i32 = 640;

/// The Millbrook Climb: the cliff ledge, the hairpin landing and the cutting, in that order.
pub const CLIMB: [Lane; 3] = [
    Lane {
        min: [4_000, -1_600],
        max: [4_400, 2_000],
        heights: [LANDING_Y, 0],
    },
    Lane {
        min: [4_000, -2_000],
        max: [5_600, -1_600],
        heights: [LANDING_Y, LANDING_Y],
    },
    Lane {
        min: [5_200, -1_600],
        max: [5_600, 1_200],
        heights: [LANDING_Y, TERRACE_Y],
    },
];

/// The Millbrook Steps: the farmyard landing and the ledge down to the valley floor.
pub const STEPS: [Lane; 2] = [
    Lane {
        min: [4_000, 3_200],
        max: [4_400, 3_600],
        heights: [TERRACE_Y, TERRACE_Y],
    },
    Lane {
        min: [4_000, 3_600],
        max: [4_400, 7_600],
        heights: [TERRACE_Y, 0],
    },
];

/// Height of the Lakeshore Climb's landing (5.6 m).
pub const LAKE_LANDING_Y: i32 = 560;

/// The Lakeshore Climb: the west cliff's ledge, the landing and the cutting, in that order.
pub const LAKESHORE: [Lane; 3] = [
    Lane {
        min: [1_200, -6_000],
        max: [1_600, -2_800],
        heights: [LAKE_LANDING_Y, 0],
    },
    Lane {
        min: [1_200, -6_400],
        max: [2_800, -6_000],
        heights: [LAKE_LANDING_Y, LAKE_LANDING_Y],
    },
    Lane {
        min: [2_400, -6_000],
        max: [2_800, -3_600],
        heights: [LAKE_LANDING_Y, TERRACE_Y],
    },
];

/// Linear rise from 0 at `edge[0]` to 1024 at `edge[1]` (Q10).
fn rise_q10(value: i32, edge: [i32; 2]) -> i64 {
    let (value, [start, end]) = (i64::from(value), edge.map(i64::from));
    ((value - start) * 1_024 / (end - start)).clamp(0, 1_024)
}

/// The terrace's ground at `(x, z)`, or `None` on the untouched valley floor. Lanes override
/// the terrace and the floor alike.
#[must_use]
pub fn height_at(x: i32, z: i32) -> Option<i32> {
    if let Some(lane) = CLIMB
        .iter()
        .chain(&STEPS)
        .chain(&LAKESHORE)
        .find(|lane| lane.contains(x, z))
    {
        return Some(lane.height(z));
    }
    let farm = rise_q10(x, WEST_CLIFF)
        .min(rise_q10(z, NORTH_CLIFF))
        .min(1_024 - rise_q10(z, SOUTH_CLIFF));
    let lake = rise_q10(x, LAKE_WEST_CLIFF)
        .min(rise_q10(
            z,
            if x >= HOLLOW_WALL_X {
                LAKE_FAR_NORTH_CLIFF
            } else {
                LAKE_NORTH_CLIFF
            },
        ))
        .min(1_024 - rise_q10(z, LAKE_SOUTH_CLIFF));
    let weight = farm.max(lake).min(1_024 - rise_q10(x, EAST_SLOPE));
    (weight > 0).then(|| i32::try_from(i64::from(TERRACE_Y) * weight / 1_024).unwrap_or(0))
}
