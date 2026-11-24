//! Bandit Highlands (#425, Atlas 5.1.85–5.1.86): the second major region beyond Greyhaven Vale,
//! rugged high ground north of the Vale, across it from the Farmland and Hamlets (#424). Like
//! the farmland it stays in the Greyhaven zone, so no zone boundary or loading step lies on the
//! way.
//!
//! The region's designed landscape shapes it (#816, [`super::design`]): Greyfang Ridge closes
//! the Vale's northern side, and the Greyfang Road winds from the Redbrand camp up through the
//! Highland Pass, a saddle in the ridge, onto the high ground. There the land is broken and
//! uneven, studded with rock crags. The road winds north to the landmark, a palisaded bandit
//! stronghold round a keep-style tower, with a branch east to a lookout tower on a knoll. The
//! stronghold's yard and the lookout's knoll are level pads.

use super::{Road, ids, on_terrain};
use crate::{AreaId, StaticCollider, XzBounds};

/// The region: the ridge and the high ground, out to the old northern wall.
pub const BOUNDS: XzBounds = XzBounds {
    min: [-20_800, -63_200],
    max: [20_800, -20_800],
};
/// The high ground lies at least this high above the Vale's floor (24 m) round the stronghold.
pub const PLATEAU_Y: i32 = 2_400;

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
/// clear of the trails. Each is a group of the crag kit's sculpted outcrops placed by the world
/// dressing (`content/greyhaven/dressing/crags.json`, #1195), its collision generated from the
/// art and its first outcrop's collider `ids::HIGHLAND_CRAGS.start()` + its index here; the
/// footprint keeps trees and boulders clear of it.
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

/// Whether a tree may stand at `(x, z)`: off the level ground (where the towers and stronghold
/// stand) and at least 2 m from every crag.
#[must_use]
pub fn clear_for_tree(x: i32, z: i32) -> bool {
    const MARGIN: i32 = 200;
    !level(x, z)
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
/// The second major region beyond the Vale (#425): its ridge and high ground.
pub const BANDIT_HIGHLANDS: AreaId = AreaId::new(12);

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: ROADS,
    colliders,
};

/// The stronghold and the lookout; the rock crags are dressing meshes (#1195).
fn colliders() -> Vec<StaticCollider> {
    (*ids::HIGHLAND_BUILDINGS.start()..)
        .zip(BUILDINGS)
        .map(|(id, &(_, min, max, height))| on_terrain(id, min, max, height))
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

/// The region's roads: all designed with the region's other roads (#816, [`super::design`]).
pub const ROADS: &[Road] = &[];
