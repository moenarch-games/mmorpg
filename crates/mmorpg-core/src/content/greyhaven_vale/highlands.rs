//! Bandit Highlands (#425, Atlas 5.1.85–5.1.86): the second major region beyond Greyhaven Vale,
//! rugged high ground north of the Vale, across it from the Farmland and Hamlets (#424) and
//! about as large as the Vale itself (416 m × 424 m). Like the farmland it stays in the
//! Greyhaven zone, so no zone boundary or loading step lies on the way.
//!
//! JSON-authored terraced ridges (33–53 m crowns, with the marked hard climb retained) close
//! the Vale's northern side behind Greyfang Ridge. The
//! Highland Pass joins the north-cave approach and winds up through it: a cutting into its
//! southern face, a ledge climbing east, a hairpin landing, a ledge climbing back west and a
//! gorge north onto the high ground, 24 m above the Vale's floor. Its steep middle faces
//! retain the pass and the marked hard climbing route as the established ways up. The face
//! has weathered buttresses and recesses; its low toe joins the Vale's settled northern edge.
//! The pass and climbing route retain their old terrain profiles and neighbouring grid cells.
//!
//! On the high ground the land is broken and uneven, studded with rock crags. The Highland
//! Trail winds north to the landmark, a palisaded bandit stronghold round a keep-style tower,
//! with a branch east to a lookout tower on a knoll. The stronghold's yard and the lookout's
//! knoll are level; the rough land fades in from the gorge's top and settles before the
//! region's walls. Every height is sampled on the terrain grid's 4 m lattice.
//!
//! Sculpted landforms (#528, `highlandRelief` in the landscape data) stand on that rough
//! ground: long spines with terraced, dipping strata, a saddle where the Trail crosses them on
//! its last rise, dry gullies and a few levelled overlooks. Their flanks stay walkable, the
//! roads keep their own grades through them, and clients draw and collide with the same grid.

use super::basin::{Along, Lane};
use super::{Road, ids, on_terrain, smooth_q10, value_noise_q10};
use crate::{AreaId, StaticCollider, XzBounds};

/// The region: the escarpment and the high ground, out to the boundary walls.
pub const BOUNDS: XzBounds = XzBounds {
    min: [-20_800, -63_200],
    max: [20_800, -20_800],
};
/// The high ground's base level above the Vale's floor (24 m).
pub const PLATEAU_Y: i32 = 2_400;
/// The escarpment (Z, northwards): its foot, the top of its southern face, the top of its
/// northern face and the high ground at its northern foot.
pub const ESCARPMENT: [i32; 4] = [-21_200, -22_000, -26_800, -27_600];
/// The escarpment's crest rises this high (40 m) …
pub const CREST_UNITS: i64 = 4_000;
/// … plus up to this much more (12 m) along its length.
pub const CREST_VARIATION_UNITS: i64 = 1_200;
/// The broken land rises by up to this much (6 m) in broad swells …
pub const ROUGH_UNITS: i64 = 600;
/// … and this much (2 m) in smaller knolls.
pub const KNOLL_UNITS: i64 = 200;
const SEED: u64 = 0x4849_474c_414e_4453;

/// The Highland Pass: the cutting into the escarpment's southern face, the ledge climbing east,
/// the hairpin landing, the ledge climbing back west and the gorge north onto the high ground,
/// in that order. Each lane is three samples (8 m) wide.
pub const PASS: [Lane; 5] = [
    Lane {
        min: [4_800, -22_800],
        max: [5_600, -21_200],
        heights: [400, 0],
        along: Along::Z,
    },
    Lane {
        min: [4_800, -23_600],
        max: [10_800, -22_800],
        heights: [400, 1_600],
        along: Along::X,
    },
    Lane {
        min: [10_000, -25_200],
        max: [10_800, -23_600],
        heights: [1_800, 1_600],
        along: Along::Z,
    },
    Lane {
        min: [3_200, -26_000],
        max: [10_800, -25_200],
        heights: [PLATEAU_Y, 1_800],
        along: Along::X,
    },
    Lane {
        min: [3_200, -27_600],
        max: [4_000, -26_000],
        heights: [PLATEAU_Y, PLATEAU_Y],
        along: Along::Z,
    },
];

/// The stronghold's yard inside its palisade `(min, max)`.
pub const STRONGHOLD: ([i32; 2], [i32; 2]) = ([-4_000, -56_000], [4_000, -49_600]);
/// The lookout's knoll `(min, max)`.
pub const LOOKOUT: ([i32; 2], [i32; 2]) = ([10_400, -40_400], [14_400, -36_400]);

/// What a highland structure looks like.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Building {
    /// A keep-style tower: the stronghold's and the lookout's.
    Tower,
    Palisade,
    GatePost,
    Tent,
    Campfire,
    Crate,
}

/// The stronghold and the lookout: `(kind, footprint min, footprint max, height)`.
pub const BUILDINGS: &[(Building, [i32; 2], [i32; 2], i32)] = &[
    // The landmark: a keep-style tower 8 m square and 20 m tall at the yard's north end.
    (Building::Tower, [-400, -55_200], [400, -54_400], 2_000),
    // The palisade, 3 m high, round the yard, with a 4.4 m gate in its southern side.
    (Building::Palisade, [-4_030, -56_030], [4_030, -55_970], 300),
    (Building::Palisade, [-4_030, -49_630], [-300, -49_570], 300),
    (Building::Palisade, [300, -49_630], [4_030, -49_570], 300),
    (
        Building::Palisade,
        [-4_030, -55_970],
        [-3_970, -49_630],
        300,
    ),
    (Building::Palisade, [3_970, -55_970], [4_030, -49_630], 300),
    (Building::GatePost, [-300, -49_640], [-220, -49_560], 400),
    (Building::GatePost, [220, -49_640], [300, -49_560], 400),
    // The bandits' tents, campfire and stores.
    (Building::Tent, [-3_000, -52_200], [-2_700, -51_900], 220),
    (Building::Tent, [-3_000, -53_600], [-2_700, -53_300], 220),
    (Building::Tent, [-1_800, -54_800], [-1_500, -54_500], 220),
    (Building::Tent, [2_700, -52_200], [3_000, -51_900], 220),
    (Building::Tent, [2_700, -53_600], [3_000, -53_300], 220),
    (Building::Tent, [1_500, -54_800], [1_800, -54_500], 220),
    (Building::Campfire, [-1_260, -52_060], [-1_140, -51_940], 30),
    (Building::Crate, [-3_600, -55_500], [-3_500, -55_400], 100),
    (Building::Crate, [-3_480, -55_500], [-3_380, -55_400], 100),
    (Building::Crate, [-3_540, -55_380], [-3_440, -55_280], 100),
    (Building::Crate, [3_400, -55_500], [3_500, -55_400], 100),
    (Building::Crate, [3_520, -55_500], [3_620, -55_400], 100),
    (Building::Crate, [3_460, -55_380], [3_560, -55_280], 100),
    // The lookout: a keep-style tower 8 m square and 14 m tall on its knoll.
    (Building::Tower, [12_000, -39_600], [12_800, -38_800], 1_400),
];

/// Rock crags across the high ground: `(footprint min, footprint max, height)`, 8–16 m tall,
/// clear of the trails.
pub const CRAGS: &[([i32; 2], [i32; 2], i32)] = &[
    // Flanking the gorge's top.
    ([1_200, -30_000], [1_800, -29_200], 1_200),
    ([6_400, -30_400], [7_200, -29_600], 1_000),
    // Along the trail's first leg and its bend west.
    ([-5_200, -32_000], [-4_400, -31_200], 1_400),
    ([-2_200, -38_000], [-1_400, -37_200], 1_000),
    ([-13_600, -36_400], [-12_600, -35_400], 1_600),
    ([-12_800, -42_000], [-12_000, -41_200], 1_200),
    ([-5_600, -40_400], [-4_800, -39_600], 900),
    // Round the lookout's knoll.
    ([15_600, -36_000], [16_400, -35_200], 1_300),
    ([16_000, -42_400], [16_800, -41_600], 1_100),
    ([8_200, -42_000], [8_800, -41_400], 800),
    // East of the trail's southern leg.
    ([5_400, -45_600], [6_200, -44_800], 1_400),
    ([9_600, -48_800], [10_400, -48_000], 1_000),
    // West and east of the stronghold.
    ([-9_600, -52_000], [-8_600, -51_000], 1_600),
    ([-8_000, -58_800], [-7_200, -58_000], 1_200),
    ([8_400, -56_400], [9_200, -55_600], 1_500),
    ([7_200, -60_800], [8_000, -60_000], 1_000),
    // Far west and far east.
    ([-17_200, -47_200], [-16_400, -46_400], 1_300),
    ([17_200, -51_600], [18_000, -50_800], 1_200),
];

/// Level ground `(min, max)`: the stronghold's yard and the lookout's knoll.
const LEVEL: [([i32; 2], [i32; 2]); 2] = [STRONGHOLD, LOOKOUT];

/// Whether `(x, z)` lies on level ground: the stronghold's yard or the lookout's knoll.
#[must_use]
pub fn level(x: i32, z: i32) -> bool {
    distance_to_level(x, z) == 0
}

/// Whether a tree may stand at `(x, z)`: supported on the escarpment, off the level ground
/// (where the towers and stronghold stand) and at least 2 m from every crag.
#[must_use]
pub fn clear_for_tree(x: i32, z: i32) -> bool {
    const MARGIN: i32 = 200;
    // Trunks on a sheer face stood on the highest corner of their footprint, leaving
    // their visible roots above the hillside. Use the actual terrain before placing them.
    let sections = &super::landscape::data().northern_ridge.sections;
    let south = sections.iter().map(|s| s.profile[0][0]).max().unwrap() + 400;
    let north = sections.iter().map(|s| s.profile[7][0]).min().unwrap() - 400;
    let on_face = (north..=south).contains(&z);
    let supported = !on_face || {
        let heights = [
            [x, z],
            [x - 100, z],
            [x + 100, z],
            [x, z - 100],
            [x, z + 100],
        ]
        .map(|[x, z]| super::TERRAIN.height_at(x, z));
        heights.into_iter().max().unwrap_or(0) - heights.into_iter().min().unwrap_or(0) <= 120
    };
    supported
        && !level(x, z)
        && CRAGS.iter().all(|&(min, max, _)| {
            x < min[0] - MARGIN || x > max[0] + MARGIN || z < min[1] - MARGIN || z > max[1] + MARGIN
        })
}

/// Chebyshev distance from `(x, z)` to the nearest level patch with its 4 m apron.
fn distance_to_level(x: i32, z: i32) -> i64 {
    const APRON: i32 = 400;
    LEVEL
        .iter()
        .map(|(min, max)| {
            let over = |value: i32, low: i32, high: i32| {
                i64::from((low - APRON - value).max(value - high - APRON).max(0))
            };
            over(x, min[0], max[0]).max(over(z, min[1], max[1]))
        })
        .min()
        .unwrap_or(i64::MAX)
}

/// The region's ground at `(x, z)`, or `None` south of the escarpment.
#[must_use]
pub fn height_at(x: i32, z: i32) -> Option<i32> {
    if let Some(lane) = PASS.iter().find(|lane| lane.contains(x, z)) {
        return Some(lane.height(x, z));
    }
    let ridge = &super::landscape::data().northern_ridge;
    let south_limit = ridge
        .sections
        .iter()
        .map(|s| s.profile[0][0])
        .max()
        .unwrap()
        + 400;
    if z > south_limit {
        return None;
    }
    let (x64, z64) = (i64::from(x), i64::from(z));
    let crest = CREST_UNITS
        + CREST_VARIATION_UNITS * value_noise_q10(x64, z64, 3_000, SEED.rotate_left(29)) / 1_024;
    let plateau = i64::from(PLATEAU_Y) + rough_ground(x64, z64);
    // Distances northwards from the foot.
    let north = -z64;
    let [foot, south_top, north_top, north_foot] = ESCARPMENT.map(|value| -i64::from(value));
    let height = if north < south_top {
        crest * (north - foot).max(0) / (south_top - foot)
    } else if north <= north_top {
        crest
    } else if north < north_foot {
        crest + (plateau - crest) * (north - north_top) / (north_foot - north_top)
    } else {
        plateau
    };
    // Roads and the hard climbing route retain a full terrain cell of their old shoulder.
    // Between those shoulders, fixed integer weathering breaks the toe and face into
    // gullies and buttresses while the routes keep their established clearances.
    let (distance, ledge) = PASS
        .iter()
        .map(|lane| {
            let dx = (lane.min[0] - x).max(x - lane.max[0]).max(0);
            let dz = (lane.min[1] - z).max(z - lane.max[1]).max(0);
            let height = lane.height(
                x.clamp(lane.min[0], lane.max[0]),
                z.clamp(lane.min[1], lane.max[1]),
            );
            (i64::from(dx.max(dz)), i64::from(height))
        })
        .min_by_key(|(distance, _)| *distance)
        .unwrap();
    let climb_distance = i64::from((12_000 - x).max(x - 12_800).max(0));
    let [min, max] = ridge.protected_approach;
    let approach_distance = i64::from(
        (min[0] - x)
            .max(x - max[0])
            .max(0)
            .max((min[1] - z).max(z - max[1]).max(0)),
    );
    let authored = authored_escarpment(x64, z64, plateau);
    // Ledge shoulders extend their walkable height, rather than preserving the old
    // 40 m wall above/below every road. The marked hard climb alone keeps that face.
    let lane_weight = smooth_q10(distance, 400, 1_600);
    let ledged = ledge + (authored - ledge) * lane_weight / 1024;
    let approach_weight = smooth_q10(approach_distance, 400, 1_600);
    let approached = ledged * approach_weight / 1024;
    let climb_weight = smooth_q10(climb_distance, 400, 1_600);
    let height = i32::try_from(height + (approached - height) * climb_weight / 1024).unwrap_or(0);
    // The extended toe meets the Vale's existing low hills instead of cutting a new step.
    Some(if north < foot {
        height.max(super::hill_height(x, z))
    } else {
        height
    })
}

/// Interpolate the authored west/east sections on the existing integer terrain lattice.
/// No extra collision mesh or render-only relief is involved.
fn authored_escarpment(x: i64, z: i64, plateau: i64) -> i64 {
    let sections = &super::landscape::data().northern_ridge.sections;
    let pair = sections
        .windows(2)
        .find(|pair| x <= i64::from(pair[1].x))
        .unwrap_or(&sections[sections.len() - 2..]);
    let width = i64::from(pair[1].x - pair[0].x);
    let along = (x - i64::from(pair[0].x)).clamp(0, width);
    let profile: [[i64; 2]; 8] = std::array::from_fn(|point| {
        std::array::from_fn(|axis| {
            let a = i64::from(pair[0].profile[point][axis]);
            a + i64::from(pair[1].profile[point][axis] - pair[0].profile[point][axis]) * along
                / width
        })
    });
    let ribs = value_noise_q10(x, 0, 2800, SEED.rotate_left(7));
    let z = z + (ribs - 512) * 800 / 1024;
    if z >= profile[0][0] {
        return 0;
    }
    if z <= profile[7][0] {
        return plateau;
    }
    let segment = profile.windows(2).find(|p| z >= p[1][0]).unwrap();
    let height = segment[0][1]
        + (segment[1][1] - segment[0][1]) * (segment[0][0] - z) / (segment[0][0] - segment[1][0]);
    let rough =
        (plateau - i64::from(PLATEAU_Y)) * smooth_q10(-z, -profile[5][0], -profile[7][0]) / 1024;
    let fissure = value_noise_q10(x, 0, 1100, SEED.rotate_left(41));
    let gully = (512 - fissure).max(0) * 180 / 512;
    let envelope = smooth_q10(-z, -profile[0][0], -profile[1][0])
        * (1024 - smooth_q10(-z, -profile[5][0], -profile[7][0]))
        / 1024;
    height + rough - gully * envelope / 1024
}

/// Q10 weight of the high ground's relief: level on the stronghold's yard and the lookout's
/// knoll; it fades in from the gorge's top and settles again before the walls, so the
/// mountains clients draw beyond them start from the high ground.
fn relief_weight(x: i64, z: i64) -> i64 {
    let level_weight = smooth_q10(
        distance_to_level(i32::try_from(x).unwrap_or(0), i32::try_from(z).unwrap_or(0)),
        0,
        1_600,
    );
    let entry = smooth_q10(
        -z,
        -i64::from(ESCARPMENT[3]),
        -i64::from(ESCARPMENT[3]) + 2_400,
    );
    let wall = (i64::from(BOUNDS.max[0]) - x.abs()).min(z - i64::from(BOUNDS.min[1]));
    let settle = smooth_q10(wall, 400, 2_400);
    level_weight * entry / 1_024 * settle / 1_024
}

/// The high ground's relief above [`PLATEAU_Y`]: broken swells and knolls under the sculpted
/// landforms (#528).
fn rough_ground(x: i64, z: i64) -> i64 {
    let rough = ROUGH_UNITS * value_noise_q10(x, z, 5_000, SEED) / 1_024
        + KNOLL_UNITS * value_noise_q10(x, z, 1_600, SEED.rotate_left(13)) / 1_024;
    (rough + sculpted_relief(x, z)) * relief_weight(x, z) / 1_024
}

/// How far the authored widths and crowns vary along their length, in Q10 (about 12%).
const FEATURE_VARIATION_Q10: i64 = 120;

/// The closest approach of `(x, z)` to a feature's points: the squared distance and the
/// interpolated half width and crown or depth there.
fn closest_on_feature(points: &[[i32; 4]], x: i64, z: i64) -> (i64, i64, i64) {
    points
        .windows(2)
        .map(|pair| {
            let [ax, az, aw, ah] = pair[0].map(i64::from);
            let [bx, bz, bw, bh] = pair[1].map(i64::from);
            let along = along_segment([ax, az], [bx, bz], x, z);
            let (dx, dz) = (bx - ax, bz - az);
            let length = dx * dx + dz * dz;
            let (px, pz) = (ax + dx * along / length, az + dz * along / length);
            (
                (x - px) * (x - px) + (z - pz) * (z - pz),
                aw + (bw - aw) * along / length,
                ah + (bh - ah) * along / length,
            )
        })
        .min_by_key(|sample| sample.0)
        .unwrap_or((i64::MAX, 0, 0))
}

/// How far along the segment from `a` to `b` the closest point to `(x, z)` lies, out of the
/// segment's squared length.
fn along_segment(a: [i64; 2], b: [i64; 2], x: i64, z: i64) -> i64 {
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    ((x - a[0]) * dx + (z - a[1]) * dz).clamp(0, dx * dx + dz * dz)
}

/// A spine's or gully's height at `(x, z)`: straight flanks from its toe to its crown, its
/// width and crown wandering around the authored values so no two stretches repeat.
fn feature_height(points: &[[i32; 4]], x: i64, z: i64, seed: u64) -> i64 {
    let (squared, width, peak) = closest_on_feature(points, x, z);
    let distance = squared.isqrt();
    let vary = |value: i64, cell: i64, seed: u64| {
        value * (1_024 + (value_noise_q10(x, z, cell, seed) - 512) * FEATURE_VARIATION_Q10 / 512)
            / 1_024
    };
    let width = vary(width, 1_900, seed);
    if width <= 0 || distance >= width {
        return 0;
    }
    vary(peak, 3_100, seed.rotate_left(11)) * (width - distance) / width
}

/// Terraces a feature's height: within each stratum a gentle tread gives way to a steeper
/// riser. The bands dip across the region, so the benches do not simply follow the contours.
fn strata(height: i64, x: i64, z: i64) -> i64 {
    /// The share of a band's rise taken by its tread, and the share of its height that tread
    /// keeps, in Q10.
    const TREAD: i64 = 576;
    const TREAD_RISE: i64 = 256;
    let bands = &super::landscape::data().highland_relief.strata_bands;
    let total: i64 = bands.iter().copied().map(i64::from).sum();
    let mut within = (height + (x * 3 + z * 2).div_euclid(64)).rem_euclid(total);
    let mut band = total;
    for &candidate in bands {
        let candidate = i64::from(candidate);
        if within < candidate {
            band = candidate;
            break;
        }
        within -= candidate;
    }
    let u = within * 1_024 / band;
    let v = if u < TREAD {
        u * TREAD_RISE / TREAD
    } else {
        TREAD_RISE + (u - TREAD) * (1_024 - TREAD_RISE) / (1_024 - TREAD)
    };
    // Only where the land has risen, so the terraces do not dig below the toes.
    height + (v - u) * band / 1_024 * smooth_q10(height, 150, 500) / 1_024
}

/// The sculpted landforms' height above the rough ground at `(x, z)`: the highest spine,
/// terraced, less the deepest gully, terraced the same way.
fn unfaded_sculpt(x: i64, z: i64) -> i64 {
    let relief = &super::landscape::data().highland_relief;
    let highest = |features: &[super::landscape::ReliefFeature], offset: u32| {
        features
            .iter()
            .enumerate()
            .map(|(index, feature)| {
                let turn = offset + u32::try_from(index).unwrap_or(0);
                feature_height(&feature.points, x, z, SEED.rotate_left(turn))
            })
            .max()
            .unwrap_or(0)
    };
    strata(highest(&relief.spines, 3), x, z) - strata(highest(&relief.gullies, 37), x, z)
}

/// The landforms levelled at the ledges and faded away from the Highland Trail and the
/// Lookout Path, so the routes keep their own grades.
fn sculpted_relief(x: i64, z: i64) -> i64 {
    const LEDGE_BLEND: i64 = 600;
    let mut sculpt = unfaded_sculpt(x, z);
    for ledge in &super::landscape::data().highland_relief.ledges {
        let [lx, lz] = ledge.center.map(i64::from);
        let distance = ((x - lx) * (x - lx) + (z - lz) * (z - lz)).isqrt();
        let radius = i64::from(ledge.radius);
        if distance < radius + LEDGE_BLEND {
            let level = unfaded_sculpt(lx, lz);
            sculpt = level
                + (sculpt - level) * smooth_q10(distance, radius, radius + LEDGE_BLEND) / 1_024;
        }
    }
    sculpt * smooth_q10(route_distance(x, z), 500, 2_600) / 1_024
}

/// Distance from `(x, z)` to the Highland Trail or the Lookout Path.
fn route_distance(x: i64, z: i64) -> i64 {
    ROADS
        .iter()
        .filter(|road| road.name != "Highland Pass")
        .flat_map(|road| road.points.windows(2))
        .map(|pair| {
            let (a, b) = (pair[0].map(i64::from), pair[1].map(i64::from));
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let length = dx * dx + dz * dz;
            let along = along_segment(a, b, x, z);
            let (px, pz) = (a[0] + dx * along / length, a[1] + dz * along / length);
            (x - px) * (x - px) + (z - pz) * (z - pz)
        })
        .min()
        .unwrap_or(0)
        .isqrt()
}

/// The second major region beyond the Vale (#425): its escarpment and high ground.
pub const BANDIT_HIGHLANDS: AreaId = AreaId::new(12);

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: ROADS,
    colliders,
};

/// The stronghold, the lookout and the rock crags.
fn colliders() -> Vec<StaticCollider> {
    let buildings = BUILDINGS
        .iter()
        .map(|&(_, min, max, height)| (min, max, height));
    (*ids::HIGHLAND_BUILDINGS.start()..)
        .zip(buildings)
        .chain((*ids::HIGHLAND_CRAGS.start()..).zip(CRAGS.iter().copied()))
        .map(|(id, (min, max, height))| on_terrain(id, min, max, height))
        .collect()
}

/// The highland structure a collider ID belongs to (#425).
#[must_use]
pub fn building(id: u32) -> Option<Building> {
    let index = id.checked_sub(*ids::HIGHLAND_BUILDINGS.start())?;
    BUILDINGS
        .get(usize::try_from(index).ok()?)
        .map(|building| building.0)
}

/// The region's named area as `(id, name, half-open bounds)`.
pub const AREAS: &[(AreaId, &str, XzBounds)] = &[(BANDIT_HIGHLANDS, "Bandit Highlands", BOUNDS)];

/// The region's roads.
pub const ROADS: &[Road] = &[
    // Bandit Highlands (#425): the pass up the escarpment from the Vale Ring Road, the trail to
    // the stronghold and the path to the lookout.
    Road {
        name: "Highland Pass",
        points: &[
            // Join the existing north-cave approach instead of crossing the rock mound.
            [6_400, -16_000],
            [5_200, -17_600],
            [5_200, -23_200],
            [10_400, -23_200],
            [10_400, -25_600],
            [3_600, -25_600],
            [3_600, -27_600],
        ],
    },
    Road {
        name: "Highland Trail",
        points: &[
            [3_600, -27_600],
            [3_600, -33_000],
            [-8_000, -38_000],
            [-8_000, -44_000],
            [0, -47_000],
            [0, -53_000],
        ],
    },
    Road {
        name: "Lookout Path",
        points: &[[3_600, -33_000], [9_000, -35_000], [12_400, -37_600]],
    },
];
