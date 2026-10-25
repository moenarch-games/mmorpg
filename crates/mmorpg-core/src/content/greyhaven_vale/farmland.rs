//! Farmland and Hamlets (#424, Atlas 5.1.85–5.1.86): the first major region beyond Greyhaven
//! Vale, a gentle agricultural plateau south of the Vale, about as large as the Vale itself
//! (416 m × 388 m). It stays in the Greyhaven zone; entity positions travel as full `i32` units,
//! so no zone boundary or loading step lies on the way.
//!
//! A mountain ridge (24–36 m) closes the Vale's southern side behind Southmeadow Downs. The
//! South Road leaves the Vale Ring Road southwards and climbs a cliff notch through it: a cutting
//! into the ridge's north face, a gorge east along its crest and a cutting south onto the
//! plateau, 8 m above the Vale's floor. Every face of the ridge is steeper than the controllers
//! climb (45°), so the notch is the only way across. The ridge's foot and the notch start 4 m
//! beyond the Vale's old southern wall, past the edge of the ground the Vale had, so the Vale's
//! own heightfield tile and everything that moves on it stay exactly as they were.
//!
//! On the plateau a farm road runs south past fenced fields to three hamlets of houses,
//! farmhouses, barns, wells and windmills, with a keep-style tower as the landmark beside the
//! road halfway down. Hamlets, fields and the tower stand on level ground; the land between them
//! rolls gently and settles before the region's walls. Like the Vale's, every height is
//! sampled on the terrain grid's 4 m lattice.

use super::basin::{Along, Lane};
use super::{smooth_q10, value_noise_q10};
use crate::XzBounds;

/// The region: the ridge and the plateau, out to the boundary walls.
pub const BOUNDS: XzBounds = XzBounds {
    min: [-20_800, 20_800],
    max: [20_800, 64_000],
};
/// The plateau's level ground above the Vale's floor (8 m).
pub const PLATEAU_Y: i32 = 800;
/// The ridge (Z): its foot, the top of its north face, the top of its south face and the
/// plateau at its southern foot.
pub const RIDGE: [i32; 4] = [21_200, 22_000, 24_400, 25_200];
/// The ridge's crest rises this high (24 m) …
pub const CREST_UNITS: i64 = 2_400;
/// … plus up to this much more (12 m) along its length.
pub const CREST_VARIATION_UNITS: i64 = 1_200;
/// The rolling land between the level patches rises and falls by up to this much (3 m).
pub const ROLL_UNITS: i64 = 300;
const SEED: u64 = 0x4641_524d_4c41_4e44;

/// The cliff notch: the cutting into the ridge's north face, the gorge east along its crest and
/// the cutting south onto the plateau, in that order. Each lane is three samples (8 m) wide.
pub const NOTCH: [Lane; 3] = [
    Lane {
        min: [-400, 21_200],
        max: [400, 22_800],
        heights: [0, 400],
        along: Along::Z,
    },
    Lane {
        min: [-400, 22_800],
        max: [3_200, 23_600],
        heights: [400, 600],
        along: Along::X,
    },
    Lane {
        min: [2_400, 23_600],
        max: [3_200, 25_200],
        heights: [600, PLATEAU_Y],
        along: Along::Z,
    },
];

/// Field interiors `(min, max)` inside their fences. Each is as wide as Millbrook's field
/// (45.8 m), so the same crop rows fill it.
pub const FIELDS: [([i32; 2], [i32; 2]); 11] = [
    // Either side of the road below the pass.
    ([-6_000, 26_400], [-1_420, 29_400]),
    ([5_000, 26_400], [9_580, 29_400]),
    // East of the road, north of the eastern hamlet's lane.
    ([5_000, 33_000], [9_580, 36_000]),
    ([5_000, 36_600], [9_580, 39_600]),
    // West of the western hamlet.
    ([-18_000, 30_800], [-13_420, 33_800]),
    ([-18_000, 34_400], [-13_420, 37_400]),
    // South of the eastern hamlet.
    ([8_000, 48_400], [12_580, 51_400]),
    ([13_400, 48_400], [17_980, 51_400]),
    // West of the southern hamlet, and across the road from it.
    ([-16_000, 52_000], [-11_420, 55_000]),
    ([-16_000, 55_600], [-11_420, 58_600]),
    ([5_000, 54_800], [9_580, 57_800]),
];

/// Level ground `(min, max)`: the hamlets, the tower's yard and (through [`FIELDS`]) the fields.
const LEVEL: [([i32; 2], [i32; 2]); 4] = [
    // The western hamlet.
    ([-12_400, 30_400], [-5_600, 37_200]),
    // The eastern hamlet.
    ([9_600, 40_400], [16_400, 47_600]),
    // The southern hamlet.
    ([-9_600, 52_400], [-1_600, 58_400]),
    // The tower's yard.
    ([-400, 42_600], [2_400, 45_400]),
];

/// What a farmland structure looks like.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Building {
    House,
    Farmhouse,
    Barn,
    Windmill,
    Well,
    /// The keep-style landmark tower.
    Tower,
}

/// The hamlets' buildings and the landmark: `(kind, footprint min, footprint max, height)`.
pub const BUILDINGS: [(Building, [i32; 2], [i32; 2], i32); 21] = [
    // The western hamlet, along its lane's end.
    (Building::House, [-10_600, 31_800], [-9_800, 32_400], 500),
    (Building::House, [-10_600, 33_200], [-9_800, 33_800], 500),
    (Building::House, [-8_200, 32_000], [-7_400, 32_600], 500),
    (Building::Farmhouse, [-8_300, 33_600], [-7_500, 34_300], 500),
    (Building::Barn, [-11_400, 34_800], [-10_200, 36_400], 700),
    (Building::Well, [-8_500, 35_200], [-8_300, 35_400], 100),
    (Building::Windmill, [-7_600, 35_800], [-7_100, 36_300], 800),
    // The eastern hamlet, either side of its lane.
    (Building::House, [10_200, 40_800], [11_000, 41_400], 500),
    (Building::House, [11_600, 42_600], [12_400, 43_200], 500),
    (Building::House, [13_600, 43_400], [14_400, 44_000], 500),
    (Building::House, [13_600, 45_000], [14_400, 45_600], 500),
    (Building::Farmhouse, [11_600, 44_400], [12_400, 45_100], 500),
    (Building::Barn, [13_800, 40_400], [15_400, 41_600], 700),
    (Building::Well, [12_400, 46_000], [12_600, 46_200], 100),
    // The southern hamlet, south of its lane.
    (Building::House, [-3_800, 54_600], [-3_000, 55_200], 500),
    (Building::House, [-5_600, 52_800], [-4_800, 53_400], 500),
    (Building::Farmhouse, [-6_000, 54_600], [-5_200, 55_300], 500),
    (Building::Barn, [-8_800, 55_000], [-7_600, 56_600], 700),
    (Building::Windmill, [-4_600, 56_400], [-4_100, 56_900], 800),
    (Building::Well, [-6_900, 53_500], [-6_700, 53_700], 100),
    // The landmark: a keep-style tower, 6 m square and 18 m tall, west of the farm road.
    (Building::Tower, [800, 43_700], [1_400, 44_300], 1_800),
];

/// Fence footprints `(min, max)` round every field, 1.1 m high: a 4 m gate in the middle of
/// the side facing the farm road.
#[must_use]
pub fn fences() -> Vec<([i32; 2], [i32; 2])> {
    const GATE: i32 = 200;
    let mut fences = Vec::new();
    for (min, max) in FIELDS {
        // North and south run the whole length; west and east stand between them.
        fences.push(([min[0] - 20, min[1] - 20], [max[0] + 20, min[1]]));
        fences.push(([min[0] - 20, max[1]], [max[0] + 20, max[1] + 20]));
        let middle = (min[1] + max[1]) / 2 / 10 * 10;
        let road_side_east = max[0] < 2_800;
        for (x, gated) in [(min[0] - 20, !road_side_east), (max[0], road_side_east)] {
            if gated {
                fences.push(([x, min[1]], [x + 20, middle - GATE]));
                fences.push(([x, middle + GATE], [x + 20, max[1]]));
            } else {
                fences.push(([x, min[1]], [x + 20, max[1]]));
            }
        }
    }
    fences
}

/// Whether `(x, z)` lies on level ground: a hamlet, the tower's yard or a field with its fences.
#[must_use]
pub fn level(x: i32, z: i32) -> bool {
    distance_to_level(x, z) == 0
}

/// Chebyshev distance from `(x, z)` to the nearest level patch with its 4 m apron.
fn distance_to_level(x: i32, z: i32) -> i64 {
    const APRON: i32 = 400;
    LEVEL
        .iter()
        .chain(&FIELDS)
        .map(|(min, max)| {
            let over = |value: i32, low: i32, high: i32| {
                i64::from((low - APRON - value).max(value - high - APRON).max(0))
            };
            over(x, min[0], max[0]).max(over(z, min[1], max[1]))
        })
        .min()
        .unwrap_or(i64::MAX)
}

/// The region's ground at `(x, z)`, or `None` north of the ridge.
#[must_use]
pub fn height_at(x: i32, z: i32) -> Option<i32> {
    if let Some(lane) = NOTCH.iter().find(|lane| lane.contains(x, z)) {
        return Some(lane.height(x, z));
    }
    if z < RIDGE[0] {
        return None;
    }
    let (x64, z64) = (i64::from(x), i64::from(z));
    let crest = CREST_UNITS
        + CREST_VARIATION_UNITS * value_noise_q10(x64, z64, 3_000, SEED.rotate_left(29)) / 1_024;
    let plateau = i64::from(PLATEAU_Y) + plateau_relief(x64, z64);
    let [foot, north_top, south_top, south_foot] = RIDGE.map(i64::from);
    let height = if z64 < north_top {
        crest * (z64 - foot) / (north_top - foot)
    } else if z64 <= south_top {
        crest
    } else if z64 < south_foot {
        crest + (plateau - crest) * (z64 - south_top) / (south_foot - south_top)
    } else {
        plateau
    };
    Some(i32::try_from(height).unwrap_or(0))
}

/// The plateau's relief above [`PLATEAU_Y`]: gently rolling land, level on the hamlets and
/// fields; it fades in from the notch's top and settles again before the walls, so the
/// mountains clients draw beyond them start from the plateau.
fn plateau_relief(x: i64, z: i64) -> i64 {
    let level_weight = smooth_q10(
        distance_to_level(i32::try_from(x).unwrap_or(0), i32::try_from(z).unwrap_or(0)),
        0,
        1_600,
    );
    let entry = smooth_q10(z, i64::from(RIDGE[3]), i64::from(RIDGE[3]) + 2_400);
    let wall = (i64::from(BOUNDS.max[0]) - x.abs()).min(i64::from(BOUNDS.max[1]) - z);
    let settle = smooth_q10(wall, 400, 2_400);
    let roll = ROLL_UNITS * value_noise_q10(x, z, 6_000, SEED) / 1_024;
    roll * level_weight / 1_024 * entry / 1_024 * settle / 1_024
}
