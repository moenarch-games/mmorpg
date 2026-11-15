//! The region's new sub-regions round the first ones (#816): their named areas and their
//! creatures on the existing templates, tougher the further they lie from the Vale. Wolf packs
//! and boars roam Westmere Woods, Southwood and the Crown Foothills, with lurkers in Westmere
//! Tarn; Redbrand outriders camp on the Greyfang Approach and hold the Delving Heights' old
//! workings with hill ogres about them; field marauders camp by the Fenwater's ford and on the
//! King's Road Downs among boars and rats.
use crate::content::CreatureSpawn;
use crate::{AreaId, CreatureId, CreatureTemplateId, StaticCollider, XzBounds};

use super::units::WANDER_RADIUS_UNITS;
use super::units::templates::{
    FIELD_MARAUDER, GRAIN_RAT, HILL_OGRE, MIREFIN_LURKER, REDBRAND_BANDIT, REDBRAND_CROSSBOWMAN,
    TIMBER_WOLF, YOUNG_BOAR,
};

pub const WESTMERE_WOODS: AreaId = AreaId::new(14);
pub const SOUTHWOOD: AreaId = AreaId::new(15);
pub const CROWN_FOOTHILLS: AreaId = AreaId::new(16);
pub const GREYFANG_APPROACH: AreaId = AreaId::new(17);
pub const DELVING_HEIGHTS: AreaId = AreaId::new(18);
pub const FENWATER_CROSSING: AreaId = AreaId::new(19);
pub const KINGS_ROAD_DOWNS: AreaId = AreaId::new(20);
/// The wild sub-regions' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 1_200..=1_399;

const fn bounds(min: [i32; 2], max: [i32; 2]) -> XzBounds {
    XzBounds { min, max }
}

/// The sub-regions' named areas: each the heart of its sub-region, clear of the first regions'.
pub const AREAS: &[(AreaId, &str, XzBounds)] = &[
    (
        WESTMERE_WOODS,
        "Westmere Woods",
        bounds([-100_000, -20_000], [-42_000, 30_000]),
    ),
    (
        SOUTHWOOD,
        "Southwood",
        bounds([-76_000, 42_000], [-23_000, 96_000]),
    ),
    (
        CROWN_FOOTHILLS,
        "Crown Foothills",
        bounds([-140_000, -90_000], [-56_000, -30_000]),
    ),
    (
        GREYFANG_APPROACH,
        "Greyfang Approach",
        bounds([-35_000, -110_000], [40_000, -64_000]),
    ),
    (
        DELVING_HEIGHTS,
        "Delving Heights",
        bounds([56_000, -115_000], [150_000, -42_000]),
    ),
    (
        FENWATER_CROSSING,
        "Fenwater Crossing",
        bounds([64_000, -20_000], [125_000, 42_000]),
    ),
    (
        KINGS_ROAD_DOWNS,
        "King's Road Downs",
        bounds([22_000, 42_000], [105_000, 100_000]),
    ),
];

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: &[],
    colliders: no_colliders,
};

fn no_colliders() -> Vec<StaticCollider> {
    Vec::new()
}

const SOUTH: u16 = 0;
const EAST: u16 = 16_384;
const NORTH: u16 = 32_768;
const WEST: u16 = 49_152;

const LOW: [u8; 2] = [1, 2];
const TOP: [u8; 2] = [2, 2];
const LURKER: [u8; 2] = [3, 3];
const MARAUDER: [u8; 2] = [3, 3];
const OUTRIDER: [u8; 2] = [4, 5];
const VETERAN: [u8; 2] = [5, 5];
const OGRE: [u8; 2] = [5, 5];

type Row = (u32, CreatureTemplateId, [i32; 2], u16, [u8; 2], i32);

/// `(id, template, feet XZ in metres, facing, levels, wander radius)`; ids 1200–1399.
#[rustfmt::skip]
const SPAWNS: &[Row] = {
    const W: i32 = WANDER_RADIUS_UNITS;
    const L: i32 = 200;
    &[
        // Westmere Woods: three wolf packs, boars, and lurkers in the tarn's deep water.
        (1200, TIMBER_WOLF, [-640, -60], EAST, TOP, W),
        (1201, TIMBER_WOLF, [-632, -52], EAST, TOP, W),
        (1202, TIMBER_WOLF, [-650, -48], SOUTH, TOP, W),
        (1203, TIMBER_WOLF, [-900, -120], EAST, TOP, W),
        (1204, TIMBER_WOLF, [-892, -110], SOUTH, TOP, W),
        (1205, TIMBER_WOLF, [-910, -108], EAST, TOP, W),
        (1206, TIMBER_WOLF, [-600, 215], NORTH, TOP, W),
        (1207, TIMBER_WOLF, [-592, 225], WEST, TOP, W),
        (1210, YOUNG_BOAR, [-760, 140], EAST, TOP, W),
        (1211, YOUNG_BOAR, [-480, -40], WEST, TOP, W),
        (1212, YOUNG_BOAR, [-500, 180], NORTH, TOP, W),
        (1213, YOUNG_BOAR, [-700, 200], SOUTH, TOP, W),
        (1220, MIREFIN_LURKER, [-800, 40], WEST, LURKER, L),
        (1221, MIREFIN_LURKER, [-830, 32], EAST, LURKER, L),
        (1222, MIREFIN_LURKER, [-772, 48], WEST, LURKER, L),
        // Southwood: boars under the oaks and two wolf packs.
        (1230, YOUNG_BOAR, [-400, 620], EAST, LOW, W),
        (1231, YOUNG_BOAR, [-520, 640], SOUTH, LOW, W),
        (1232, YOUNG_BOAR, [-600, 720], NORTH, LOW, W),
        (1233, YOUNG_BOAR, [-470, 840], WEST, LOW, W),
        (1234, YOUNG_BOAR, [-560, 840], EAST, LOW, W),
        (1235, YOUNG_BOAR, [-360, 780], NORTH, LOW, W),
        (1240, TIMBER_WOLF, [-560, 560], SOUTH, TOP, W),
        (1241, TIMBER_WOLF, [-552, 570], WEST, TOP, W),
        (1242, TIMBER_WOLF, [-600, 760], EAST, TOP, W),
        (1243, TIMBER_WOLF, [-590, 770], NORTH, TOP, W),
        // Crown Foothills: two wolf packs and a hill ogre below the peaks.
        (1250, TIMBER_WOLF, [-850, -580], SOUTH, TOP, W),
        (1251, TIMBER_WOLF, [-842, -572], EAST, TOP, W),
        (1252, TIMBER_WOLF, [-860, -568], SOUTH, TOP, W),
        (1253, TIMBER_WOLF, [-1050, -620], EAST, TOP, W),
        (1254, TIMBER_WOLF, [-1040, -612], SOUTH, TOP, W),
        (1255, TIMBER_WOLF, [-1060, -632], NORTH, TOP, W),
        (1256, HILL_OGRE, [-930, -660], SOUTH, OGRE, 0),
        // Greyfang Approach: a Redbrand outriders' camp beside the road north.
        (1260, REDBRAND_BANDIT, [60, -900], EAST, OUTRIDER, W),
        (1261, REDBRAND_BANDIT, [70, -912], NORTH, OUTRIDER, W),
        (1262, REDBRAND_BANDIT, [48, -915], EAST, OUTRIDER, W),
        (1263, REDBRAND_BANDIT, [82, -890], SOUTH, OUTRIDER, W),
        (1264, REDBRAND_CROSSBOWMAN, [38, -880], EAST, OUTRIDER, W),
        (1265, REDBRAND_CROSSBOWMAN, [92, -930], WEST, OUTRIDER, W),
        (1266, REDBRAND_CROSSBOWMAN, [28, -925], SOUTH, OUTRIDER, W),
        // Delving Heights: Redbrand veterans hold the old workings; hill ogres roam about them.
        (1270, REDBRAND_BANDIT, [1060, -690], SOUTH, VETERAN, W),
        (1271, REDBRAND_BANDIT, [1105, -640], WEST, VETERAN, W),
        (1272, REDBRAND_BANDIT, [1095, -700], NORTH, VETERAN, W),
        (1273, REDBRAND_CROSSBOWMAN, [1045, -645], EAST, VETERAN, W),
        (1274, REDBRAND_CROSSBOWMAN, [1120, -680], WEST, VETERAN, W),
        (1275, HILL_OGRE, [950, -760], SOUTH, OGRE, 0),
        (1276, HILL_OGRE, [1220, -560], WEST, OGRE, 0),
        // Fenwater Crossing: marauders camped by the ford, boars in the alders.
        (1280, FIELD_MARAUDER, [1000, 370], WEST, MARAUDER, W),
        (1281, FIELD_MARAUDER, [1012, 382], NORTH, MARAUDER, W),
        (1282, FIELD_MARAUDER, [990, 385], EAST, MARAUDER, W),
        (1283, FIELD_MARAUDER, [1022, 360], SOUTH, MARAUDER, W),
        (1284, YOUNG_BOAR, [850, 430], EAST, TOP, W),
        (1285, YOUNG_BOAR, [900, 470], NORTH, TOP, W),
        (1286, YOUNG_BOAR, [820, 470], WEST, TOP, W),
        // King's Road Downs: a marauders' camp on the knoll, boars and rats by the spinney.
        (1290, FIELD_MARAUDER, [520, 620], SOUTH, MARAUDER, W),
        (1291, FIELD_MARAUDER, [532, 630], WEST, MARAUDER, W),
        (1292, FIELD_MARAUDER, [508, 632], EAST, MARAUDER, W),
        (1293, FIELD_MARAUDER, [540, 612], NORTH, MARAUDER, W),
        (1294, YOUNG_BOAR, [700, 820], WEST, TOP, W),
        (1295, YOUNG_BOAR, [760, 760], SOUTH, TOP, W),
        (1296, YOUNG_BOAR, [600, 830], NORTH, TOP, W),
        (1297, GRAIN_RAT, [380, 700], EAST, [1, 1], W),
        (1298, GRAIN_RAT, [395, 718], NORTH, [1, 1], W),
        (1299, GRAIN_RAT, [362, 690], WEST, [1, 1], W),
    ]
};

/// The sub-regions' creatures (#816).
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    SPAWNS
        .iter()
        .map(
            |&(id, template, [x, z], facing, levels, wander_radius)| CreatureSpawn {
                id: CreatureId::new(id),
                template,
                position: [x * 100, z * 100],
                facing,
                wander_radius,
                levels: Some(levels),
            },
        )
        .collect()
}
