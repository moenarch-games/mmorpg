//! Greyhaven Vale, the starter zone: immutable collision content, named
//! areas, the road corridors that must stay walkable, and the creatures,
//! NPCs and graveyard of [`units`].
//!
//! The playable square spans ±120 m around the origin inside invisible
//! boundary walls. Map north is −Z, toward the Redbrand cliffs, matching the
//! layout table in `docs/STARTER_ZONE.md`. Walkable ground is physically flat (y = 0); relief, props
//! without colliders, water and mountains are presentation owned by
//! `mmorpg-scenery`, which derives its structures from the collider IDs in
//! [`ids`]. Everything here belongs to content [`REVISION`]; changing any of
//! it requires a new revision. [`content`] bundles it as the hosted
//! [`ZoneContent`].

use std::sync::{Arc, LazyLock};

use super::{SpawnGrid, StaticCollider, XzBounds, ZoneContent, ZoneDefinition};
use crate::{Area, AreaId, Terrain, ZoneAreas};

pub mod cave;
pub mod inn;
pub mod layout;
pub mod quests;
pub mod units;

/// Content revision of this zone. Revision 1 was the former test outpost;
/// revision 2 had no creatures or NPCs; revision 6 binds the ability catalog
/// (Mirefin Lurkers cast Muck Bolt, Redbrand Bandits use Crude Bandage);
/// revision 7 binds item catalog 2 (equipment) and loot catalog 2;
/// revision 8 binds vendor catalog 1 (Innkeeper Bram Tolliver's stock);
/// revision 9 binds quest catalog 1 (the Greyhaven chain), item catalog 3
/// (the Wolf Pelt) and vendor catalog 2; revision 10 replaces the inn block
/// with the walk-in inn and its door (#158) and moves Innkeeper Bram Tolliver
/// inside; revision 15 adds breakable crates, barrels, fences and powder barrels (#288);
/// revision 16 composes the saved Outpost layout overrides (#321).
pub const REVISION: u64 = 16;
/// Revision 3's simulation seed, retained when revision 4 adds bag content.
/// Economy changes must not reroll Greyhaven's existing creature/combat scripts.
pub const RNG_SEED: u64 = 0x3cbc_808b_be89_b29c;
/// Gravity in units per tick squared.
pub const GRAVITY: [i32; 3] = [0, -1, 0];
/// The inner faces of the boundary walls enclose this square.
pub const PLAYABLE_BOUNDS: XzBounds = XzBounds {
    min: [-20_800, -20_800],
    max: [20_800, 20_800],
};
/// The hub plaza: kept free of colliders so every spawn slot is clear.
pub const SPAWN_PLAZA: XzBounds = XzBounds {
    min: [-1_600, 1_200],
    max: [1_600, 2_800],
};
/// 32 × 16 slots, one metre apart, centred in [`SPAWN_PLAZA`].
pub const SPAWN_GRID: SpawnGrid = SpawnGrid {
    origin: [-1_550, 1_250],
    columns: 32,
    spacing: 100,
};
/// No collider comes closer than this to a road centre line (2 m).
pub const ROAD_CLEARANCE_UNITS: i32 = 200;
/// Boundary walls are 20 m tall; a jump peaks near 1.4 m.
const WALL_HEIGHT: i32 = 2_000;
const WALL_THICKNESS: i32 = 200;
const GROUND_THICKNESS: i32 = 100;

/// Collider IDs by structure. `mmorpg-scenery` maps each ID to its visual.
pub mod ids {
    use std::ops::RangeInclusive;

    pub const GROUND: u32 = 1;
    /// West, east, north (−Z) and south (+Z) walls.
    pub const BOUNDARY_WALLS: RangeInclusive<u32> = 2..=5;

    pub const KEEP: u32 = 100;
    /// The inn's footprint: a presentation anchor, not a collider (#158).
    pub const INN: u32 = 101;
    pub const SMITHY: u32 = 102;
    pub const HOUSES: RangeInclusive<u32> = 103..=106;
    pub const WELL: u32 = 107;
    pub const WAYSTONE: u32 = 108;
    pub const PALISADE: RangeInclusive<u32> = 110..=117;
    pub const GATE_POSTS: RangeInclusive<u32> = 120..=127;
    pub const GRAVESTONES: RangeInclusive<u32> = 130..=137;
    /// The inn's front door leaf (#158).
    pub const INN_DOOR: u32 = 149;
    /// The inn's walls and furniture (#158).
    pub const INN_PARTS: RangeInclusive<u32> = 150..=181;

    pub const BARN: u32 = 200;
    pub const FARMHOUSE: u32 = 201;
    pub const WINDMILL: u32 = 202;
    pub const FENCES: RangeInclusive<u32> = 210..=214;

    pub const SHORE_ROCKS: RangeInclusive<u32> = 300..=305;

    pub const CLIFFS: RangeInclusive<u32> = 400..=406;
    pub const MINE_ENTRANCE: u32 = 410;
    pub const TENTS: RangeInclusive<u32> = 420..=422;
    pub const CRATES: RangeInclusive<u32> = 430..=433;
    pub const CAMPFIRE: u32 = 440;

    /// The Greyfang standing stones (#159).
    pub const STANDING_STONES: RangeInclusive<u32> = 500..=506;
    /// The Southmeadow ruined farmstead's houses and fences (#159).
    pub const RUINS: RangeInclusive<u32> = 510..=511;
    pub const RUIN_FENCES: RangeInclusive<u32> = 512..=514;
    /// The Eastwater watchtower (#159).
    pub const WATCHTOWER: u32 = 520;
    /// Hollowpine Reach boulders (#159).
    pub const BOULDERS: RangeInclusive<u32> = 530..=537;
    /// The Greyfang cave's rock walls and roof slab (#159).
    pub const CAVE_WALLS: RangeInclusive<u32> = 540..=546;
    pub const CAVE_ROOF: u32 = 547;

    /// Trunks, numbered in generation order from the start: Wolfrun Woods
    /// first, then the outer woods (#159).
    pub const TREES: RangeInclusive<u32> = 1_000..=4_999;
}

pub const OUTPOST: AreaId = AreaId::new(1);
pub const WOLFRUN_WOODS: AreaId = AreaId::new(2);
pub const MILLBROOK_FARM: AreaId = AreaId::new(3);
pub const STILLWATER_LAKE: AreaId = AreaId::new(4);
pub const REDBRAND_HOLLOW: AreaId = AreaId::new(5);
pub const GREYFANG_RIDGE: AreaId = AreaId::new(6);
pub const SOUTHMEADOW_DOWNS: AreaId = AreaId::new(7);
pub const HOLLOWPINE_REACH: AreaId = AreaId::new(8);
pub const EASTWATER_FEN: AreaId = AreaId::new(9);

/// The five subzones as `(id, name, half-open bounds)`, like every other
/// rectangle in this module. Bounds are disjoint.
const AREA_TABLE: [(AreaId, &str, XzBounds); 9] = [
    (
        OUTPOST,
        "Greyhaven Outpost",
        XzBounds {
            min: [-3_500, -1_300],
            max: [3_500, 5_300],
        },
    ),
    (
        WOLFRUN_WOODS,
        "Wolfrun Woods",
        XzBounds {
            min: [-11_900, -5_000],
            max: [-4_000, 5_000],
        },
    ),
    (
        MILLBROOK_FARM,
        "Millbrook Farm",
        XzBounds {
            min: [4_000, 1_300],
            max: [10_500, 9_000],
        },
    ),
    (
        STILLWATER_LAKE,
        "Stillwater Lake",
        XzBounds {
            min: [3_600, -7_800],
            max: [8_600, -3_200],
        },
    ),
    (
        REDBRAND_HOLLOW,
        "Redbrand Hollow",
        XzBounds {
            min: [-3_500, -11_900],
            max: [3_500, -7_000],
        },
    ),
    // The outer valley (#159): a ring of four regions around the original five.
    (
        GREYFANG_RIDGE,
        "Greyfang Ridge",
        XzBounds {
            min: [-20_800, -20_800],
            max: [20_800, -12_000],
        },
    ),
    (
        SOUTHMEADOW_DOWNS,
        "Southmeadow Downs",
        XzBounds {
            min: [-20_800, 12_000],
            max: [20_800, 20_800],
        },
    ),
    (
        HOLLOWPINE_REACH,
        "Hollowpine Reach",
        XzBounds {
            min: [-20_800, -12_000],
            max: [-12_000, 12_000],
        },
    ),
    (
        EASTWATER_FEN,
        "Eastwater Fen",
        XzBounds {
            min: [12_000, -12_000],
            max: [20_800, 12_000],
        },
    ),
];

static AREAS: LazyLock<ZoneAreas> = LazyLock::new(|| {
    let areas = AREA_TABLE
        .iter()
        .map(|(id, name, bounds)| {
            // `ZoneAreas` bounds are inclusive: a half-open `[min, max)`
            // rectangle covers `[min, max - 1]` in integer units.
            Area::new(*id, *name, bounds.min, bounds.max.map(|value| value - 1))
                .expect("built-in vale area is valid")
        })
        .collect();
    ZoneAreas::new(areas).expect("built-in vale areas are valid")
});

/// The named areas of this content revision, ordered by ID.
#[must_use]
pub fn areas() -> &'static ZoneAreas {
    &AREAS
}

/// The area containing an XZ point, if any.
#[must_use]
pub fn area_at(x: i32, z: i32) -> Option<&'static Area> {
    AREAS.area_at(x, z)
}

/// A road centre line. Roads are open corridors: no collider comes within
/// [`ROAD_CLEARANCE_UNITS`] of any segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Road {
    pub name: &'static str,
    /// XZ polyline in units, at least two points.
    pub points: &'static [[i32; 2]],
}

static ROADS: [Road; 8] = [
    Road {
        name: "Hollow Road",
        points: &[[0, 1_200], [0, -1_000], [0, -4_000], [0, -8_600]],
    },
    Road {
        name: "Lakeshore Path",
        points: &[[0, -4_000], [2_500, -4_800], [3_200, -4_900]],
    },
    Road {
        name: "Millbrook Road",
        points: &[
            [1_600, 2_000],
            [3_200, 2_000],
            [4_400, 2_000],
            [5_400, 2_800],
            [6_400, 3_300],
        ],
    },
    Road {
        name: "Wolfrun Trail",
        points: &[
            [-1_600, 2_000],
            [-3_200, 2_000],
            [-4_400, 2_000],
            [-5_600, 900],
            [-6_600, 300],
            [-7_600, 0],
        ],
    },
    Road {
        name: "South Road",
        points: &[[0, 2_800], [0, 5_000], [0, 10_500], [0, 15_000]],
    },
    // The outer valley (#159): a ring road through the four outer regions and
    // two trails that join the farm and the woods to it without the outpost.
    Road {
        name: "Vale Ring Road",
        points: &[
            [-15_000, -15_000],
            [15_000, -15_000],
            [15_000, 15_000],
            [-15_000, 15_000],
            [-15_000, -15_000],
        ],
    },
    Road {
        name: "Fen Track",
        points: &[[4_400, 2_000], [11_000, -1_000], [15_000, -3_000]],
    },
    Road {
        name: "Downs Trail",
        points: &[[-4_400, 2_000], [-4_400, 6_000], [-15_000, 6_000]],
    },
];

#[must_use]
pub fn roads() -> &'static [Road] {
    &ROADS
}

/// `(id, footprint min [x, z], footprint max [x, z], height)`: authored boxes
/// standing on y = 0. Every coordinate is a multiple of ten units, and boxes
/// may touch but never overlap, so each collider has one unambiguous visual.
type Footprint = (u32, [i32; 2], [i32; 2], i32);

#[rustfmt::skip]
static STRUCTURES: [Footprint; 83] = [
    // Greyhaven Outpost: keep, inn, smithy, houses, well and waystone.
    (ids::KEEP, [-2_600, -500], [-800, 700], 1_000),
    (ids::SMITHY, [2_400, 100], [3_000, 800], 450),
    (103, [-2_800, 3_300], [-2_000, 3_900], 500),
    (104, [-1_500, 3_500], [-700, 4_100], 500),
    (105, [800, 3_500], [1_600, 4_100], 500),
    (106, [2_000, 3_300], [2_800, 3_900], 500),
    (ids::WELL, [400, 3_000], [600, 3_200], 100),
    (ids::WAYSTONE, [-600, 3_000], [-500, 3_100], 250),
    // Palisade: north, south, west and east sides with a 6 m gap on each road.
    (110, [-3_230, -1_030], [-300, -970], 300),
    (111, [300, -1_030], [3_230, -970], 300),
    (112, [-3_230, 4_970], [-300, 5_030], 300),
    (113, [300, 4_970], [3_230, 5_030], 300),
    (114, [-3_230, -970], [-3_170, 1_700], 300),
    (115, [-3_230, 2_300], [-3_170, 4_970], 300),
    (116, [3_170, -970], [3_230, 1_700], 300),
    (117, [3_170, 2_300], [3_230, 4_970], 300),
    // 0.8 m gate posts stand inside each gap, leaving a 4.4 m clear opening.
    (120, [-300, -1_040], [-220, -960], 400),
    (121, [220, -1_040], [300, -960], 400),
    (122, [-300, 4_960], [-220, 5_040], 400),
    (123, [220, 4_960], [300, 5_040], 400),
    (124, [-3_240, 1_700], [-3_160, 1_780], 400),
    (125, [-3_240, 2_220], [-3_160, 2_300], 400),
    (126, [3_160, 1_700], [3_240, 1_780], 400),
    (127, [3_160, 2_220], [3_240, 2_300], 400),
    // Graveyard plot: two rows of gravestones.
    (130, [-2_830, 4_390], [-2_770, 4_410], 100),
    (131, [-2_530, 4_390], [-2_470, 4_410], 100),
    (132, [-2_230, 4_390], [-2_170, 4_410], 100),
    (133, [-1_930, 4_390], [-1_870, 4_410], 100),
    (134, [-2_830, 4_690], [-2_770, 4_710], 100),
    (135, [-2_530, 4_690], [-2_470, 4_710], 100),
    (136, [-2_230, 4_690], [-2_170, 4_710], 100),
    (137, [-1_930, 4_690], [-1_870, 4_710], 100),
    // Millbrook Farm: barn, farmhouse, windmill base and field fences.
    (ids::BARN, [7_400, 3_600], [8_600, 5_200], 700),
    (ids::FARMHOUSE, [5_800, 4_300], [6_600, 5_000], 500),
    (ids::WINDMILL, [8_550, 1_850], [9_050, 2_350], 800),
    (210, [5_000, 5_590], [7_000, 5_610], 110),
    (211, [7_400, 5_590], [9_600, 5_610], 110),
    (212, [5_000, 8_390], [9_600, 8_410], 110),
    (213, [4_990, 5_610], [5_010, 8_390], 110),
    (214, [9_590, 5_610], [9_610, 8_390], 110),
    // Stillwater Lake: rocks outside the walkable shallow water.
    (300, [7_900, -5_280], [8_100, -5_120], 150),
    (301, [7_080, -7_390], [7_320, -7_210], 180),
    (302, [4_920, -7_470], [5_080, -7_330], 100),
    (303, [3_500, -6_800], [3_700, -6_600], 160),
    (304, [6_710, -3_780], [6_890, -3_620], 120),
    (305, [4_330, -3_760], [4_470, -3_640], 90),
    // Redbrand Hollow: cliffs under the north wall, the mine entrance, the camp.
    (400, [-3_200, -12_000], [-2_200, -7_400], 1_200),
    (401, [-2_200, -8_400], [-1_600, -7_400], 800),
    (402, [2_200, -12_000], [3_200, -7_400], 1_200),
    (403, [1_600, -9_600], [2_200, -8_600], 900),
    (404, [-2_200, -12_000], [2_200, -11_000], 1_400),
    (405, [-2_200, -11_000], [-800, -10_400], 1_000),
    (406, [800, -11_000], [2_200, -10_500], 1_000),
    (ids::MINE_ENTRANCE, [-300, -11_000], [300, -10_900], 420),
    (420, [-1_700, -9_600], [-1_400, -9_300], 220),
    (421, [1_100, -9_900], [1_400, -9_600], 220),
    (422, [-1_500, -8_600], [-1_200, -8_300], 220),
    (430, [500, -10_500], [600, -10_400], 100),
    (431, [620, -10_500], [720, -10_400], 100),
    (432, [560, -10_380], [660, -10_280], 100),
    (433, [-700, -10_300], [-600, -10_200], 100),
    (ids::CAMPFIRE, [-60, -9_260], [60, -9_140], 30),
    // Greyfang Ridge: seven standing stones in an 8 m ring (#159).
    (500, [-40, -15_730], [40, -15_670], 250),
    (501, [590, -16_030], [670, -15_970], 230),
    (502, [740, -16_710], [820, -16_650], 260),
    (503, [310, -17_250], [390, -17_190], 240),
    (504, [-390, -17_250], [-310, -17_190], 250),
    (505, [-820, -16_710], [-740, -16_650], 220),
    (506, [-670, -16_030], [-590, -15_970], 260),
    // Southmeadow Downs: a ruined farmstead inside its old fence (#159).
    (510, [5_000, 16_000], [5_800, 16_600], 300),
    (511, [6_200, 16_200], [6_800, 16_700], 250),
    (512, [4_700, 15_800], [7_000, 15_820], 110),
    (513, [4_700, 15_820], [4_720, 17_000], 110),
    (514, [6_980, 15_820], [7_000, 17_000], 110),
    // Eastwater Fen: the old watchtower (#159).
    (ids::WATCHTOWER, [16_500, -6_200], [16_900, -5_800], 1_400),
    // Hollowpine Reach: boulders beside the ring road (#159).
    (530, [-15_700, -9_200], [-15_480, -9_000], 160),
    (531, [-14_600, -4_300], [-14_420, -4_140], 120),
    (532, [-15_620, 1_800], [-15_420, 1_960], 140),
    (533, [-14_640, 9_400], [-14_460, 9_580], 110),
    (534, [-15_800, 12_600], [-15_560, 12_820], 180),
    (535, [-14_560, -12_800], [-14_400, -12_660], 100),
    (536, [-15_660, 4_400], [-15_500, 4_540], 90),
    (537, [-14_620, -1_200], [-14_440, -1_040], 130),
];

/// The outer valley's ground (#159): a 4 m grid of heights over the playable square and a
/// little beyond. The original valley stays flat; hills rise across the ring of outer
/// regions and settle again before the boundary walls.
pub mod terrain {
    /// The grid's first sample, in units; aligned with the clients' 4 m terrain grid.
    pub const ORIGIN: [i32; 2] = [-21_200, -21_200];
    pub const STEP: i32 = 400;
    pub const COUNT: u16 = 107;
    /// Tallest hill (10 m).
    pub const HILL_UNITS: i64 = 1_000;
    /// Hills start beyond this Chebyshev distance from the origin and reach full height at
    /// [`RISE_END`] …
    pub const RISE_START: i64 = 12_600;
    pub const RISE_END: i64 = 15_600;
    /// … and settle again from [`SETTLE_START`] to [`SETTLE_END`], before the walls.
    pub const SETTLE_START: i64 = 18_000;
    pub const SETTLE_END: i64 = 20_400;
    /// Noise lattice spacing of the large and small hills.
    pub const CELLS: [i64; 2] = [7_000, 3_500];
    pub const SEED: u64 = 0x4752_4559_4849_4c4c;
}

/// Smoothstep of `value` between `start` and `end` in Q10 (0..=1024).
const fn smooth_q10(value: i64, start: i64, end: i64) -> i64 {
    if value <= start {
        return 0;
    }
    if value >= end {
        return 1_024;
    }
    let t = (value - start) * 1_024 / (end - start);
    t * t * (3 * 1_024 - 2 * t) / (1_024 * 1_024)
}

/// Value noise in Q10 at `(x, z)` on a lattice of `cell` units: smoothly interpolated hashes.
fn value_noise_q10(x: i64, z: i64, cell: i64, seed: u64) -> i64 {
    let corner = |cx: i64, cz: i64| {
        let key = ((cx as u64) << 32) ^ (cz as u64 & 0xffff_ffff);
        (splitmix64(seed ^ key) % 1_025) as i64
    };
    let (cx, cz) = (x.div_euclid(cell), z.div_euclid(cell));
    let fx = smooth_q10(x.rem_euclid(cell), 0, cell);
    let fz = smooth_q10(z.rem_euclid(cell), 0, cell);
    let top = corner(cx, cz) * (1_024 - fx) + corner(cx + 1, cz) * fx;
    let bottom = corner(cx, cz + 1) * (1_024 - fx) + corner(cx + 1, cz + 1) * fx;
    (top * (1_024 - fz) + bottom * fz) / (1_024 * 1_024)
}

/// The authored hill height at `(x, z)` in units (#159).
#[must_use]
pub fn hill_height(x: i32, z: i32) -> i32 {
    let (x, z) = (i64::from(x), i64::from(z));
    let reach = x.abs().max(z.abs());
    let weight = smooth_q10(reach, terrain::RISE_START, terrain::RISE_END)
        * (1_024 - smooth_q10(reach, terrain::SETTLE_START, terrain::SETTLE_END))
        / 1_024;
    if weight == 0 {
        return 0;
    }
    let large = value_noise_q10(x, z, terrain::CELLS[0], terrain::SEED);
    let small = value_noise_q10(x, z, terrain::CELLS[1], terrain::SEED.rotate_left(17));
    let noise = (large * 4 + small) / 5;
    i32::try_from(terrain::HILL_UNITS * noise * weight / (1_024 * 1_024)).unwrap_or(0)
}

/// The outer valley's terrain grid (#159), sampled from [`hill_height`].
#[must_use]
pub fn terrain() -> Terrain {
    let count = i32::from(terrain::COUNT);
    let heights = (0..count)
        .flat_map(|row| {
            (0..count).map(move |column| {
                let x = terrain::ORIGIN[0] + column * terrain::STEP;
                let z = terrain::ORIGIN[1] + row * terrain::STEP;
                // The cave floor stays flat inside its hillside.
                if cave::GREYFANG.flattens(x, z) {
                    0
                } else {
                    hill_height(x, z)
                }
            })
        })
        .collect();
    Terrain {
        origin: terrain::ORIGIN,
        step: terrain::STEP,
        columns: terrain::COUNT,
        rows: terrain::COUNT,
        heights,
    }
}

static TERRAIN: LazyLock<Terrain> = LazyLock::new(terrain);

/// Where a footprint `[min, max]` rests on the terrain: its lowest corner, sunk 0.3 m so
/// nothing floats on a slope.
fn footing(min: [i32; 2], max: [i32; 2]) -> i32 {
    let corners = [min, [max[0], min[1]], max, [min[0], max[1]]];
    let lowest = corners
        .iter()
        .map(|corner| TERRAIN.height_at(corner[0], corner[1]))
        .min()
        .unwrap_or(0);
    if lowest == 0
        && corners
            .iter()
            .all(|corner| TERRAIN.height_at(corner[0], corner[1]) == 0)
    {
        0
    } else {
        lowest - 30
    }
}

/// A box standing on y = 0 over an authored footprint.
const fn standing(id: u32, min: [i32; 2], max: [i32; 2], height: i32) -> StaticCollider {
    let half = [(max[0] - min[0]) / 2, height / 2, (max[1] - min[1]) / 2];
    StaticCollider {
        id,
        position: [min[0] + half[0], half[1], min[1] + half[2]],
        half_extents: half,
    }
}

/// Wolfrun Woods trunk generator: a jittered grid of 4.5 m cells west of the
/// outpost. Seeded integer hashing keeps it deterministic; a trunk centre stays
/// at least 1.2 m from its cell edge, so neighbouring trunks (at most 0.9 m
/// wide) always leave a walkable gap of 1.5 m or more.
mod woods {
    pub const ORIGIN: [i32; 2] = [-11_600, -4_500];
    pub const CELLS: [i32; 2] = [15, 20];
    pub const CELL: i32 = 450;
    pub const EDGE_MARGIN: i32 = 120;
    pub const SEED: u64 = 0x5752_4f4c_4652_554e;
    /// The wolf den clearing where the Wolfrun Trail ends.
    pub const CLEARING_CENTRE: [i32; 2] = [-8_000, 0];
    pub const CLEARING_RADIUS: i32 = 900;
    pub const TRUNK_HEIGHT: i32 = 500;
}

/// The outer valley's woods (#159): `(origin, cells, seed, percent of cells
/// with a trunk)` on the same 4.5 m jittered grid as Wolfrun Woods.
const OUTER_FORESTS: [([i32; 2], [i32; 2], u64, u64); 4] = [
    // Hollowpine Reach: dense pines west of Wolfrun Woods.
    ([-20_400, -11_600], [17, 52], 0x484f_4c4c_4f57_5049, 60),
    // Greyfang Ridge and Southmeadow Downs: scattered copses.
    ([-18_000, -20_400], [80, 10], 0x4752_4559_4641_4e47, 12),
    ([-18_000, 16_400], [80, 9], 0x534f_5554_484d_4544, 12),
    // Eastwater Fen: sparse alders east of the farm and the lake.
    ([12_600, -11_600], [17, 52], 0x4541_5354_5741_5445, 15),
];

/// SplitMix64 finaliser: a fixed integer hash, identical on every platform.
const fn splitmix64(value: u64) -> u64 {
    let mut z = value.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn trees() -> Vec<StaticCollider> {
    let mut trees = Vec::new();
    let mut next_id = *ids::TREES.start();
    let span = woods::CELL - 2 * woods::EDGE_MARGIN;
    for row in 0..woods::CELLS[1] {
        for column in 0..woods::CELLS[0] {
            let cell = (u64::from(row.unsigned_abs()) << 32) | u64::from(column.unsigned_abs());
            let hash = splitmix64(woods::SEED ^ cell);
            let border = row == 0
                || column == 0
                || row == woods::CELLS[1] - 1
                || column == woods::CELLS[0] - 1;
            // A quarter of the cells are glades; half of the border cells thin the edge.
            if hash.is_multiple_of(4) || (border && (hash >> 2).is_multiple_of(2)) {
                continue;
            }
            let jitter = |shift: u32| (hash >> shift) as i32 & 0xff;
            let x = woods::ORIGIN[0]
                + column * woods::CELL
                + woods::EDGE_MARGIN
                + jitter(8) % (span + 1);
            let z =
                woods::ORIGIN[1] + row * woods::CELL + woods::EDGE_MARGIN + jitter(16) % (span + 1);
            // Trunks 0.6 m to 0.9 m wide.
            let half = 30 + jitter(24) % 16;
            let dx = i64::from(x - woods::CLEARING_CENTRE[0]);
            let dz = i64::from(z - woods::CLEARING_CENTRE[1]);
            let keep_out = i64::from(woods::CLEARING_RADIUS + half);
            if dx * dx + dz * dz < keep_out * keep_out {
                continue;
            }
            let collider = StaticCollider {
                id: next_id,
                position: [x, woods::TRUNK_HEIGHT / 2, z],
                half_extents: [half, woods::TRUNK_HEIGHT / 2, half],
            };
            if !clear_of_roads(&collider) {
                continue;
            }
            trees.push(collider);
            next_id += 1;
        }
    }
    for (origin, cells, seed, percent) in OUTER_FORESTS {
        for row in 0..cells[1] {
            for column in 0..cells[0] {
                let cell = (u64::from(row.unsigned_abs()) << 32) | u64::from(column.unsigned_abs());
                let hash = splitmix64(seed ^ cell);
                if hash % 100 >= percent {
                    continue;
                }
                let jitter = |shift: u32| (hash >> shift) as i32 & 0xff;
                let x =
                    origin[0] + column * woods::CELL + woods::EDGE_MARGIN + jitter(8) % (span + 1);
                let z =
                    origin[1] + row * woods::CELL + woods::EDGE_MARGIN + jitter(16) % (span + 1);
                let half = 30 + jitter(24) % 16;
                let base = footing([x - half, z - half], [x + half, z + half]);
                let collider = StaticCollider {
                    id: next_id,
                    position: [x, base + woods::TRUNK_HEIGHT / 2, z],
                    half_extents: [half, woods::TRUNK_HEIGHT / 2, half],
                };
                if !clear_of_roads(&collider) || near_structure(&collider) {
                    continue;
                }
                trees.push(collider);
                next_id += 1;
            }
        }
    }
    trees
}

/// Whether a trunk stands within 1.5 m of an authored structure's footprint.
fn near_structure(collider: &StaticCollider) -> bool {
    const MARGIN: i32 = 150;
    let [x, _, z] = collider.position;
    let [hx, _, hz] = collider.half_extents;
    // The cave's roof covers its walls.
    let mut footprints = STRUCTURES
        .iter()
        .map(|&(_, min, max, _)| (min, max))
        .chain([cave::GREYFANG.roof]);
    footprints.any(|(min, max)| {
        x + hx + MARGIN > min[0]
            && x - hx - MARGIN < max[0]
            && z + hz + MARGIN > min[1]
            && z - hz - MARGIN < max[1]
    })
}

/// Whether a collider's footprint keeps [`ROAD_CLEARANCE_UNITS`] from every road.
#[must_use]
pub fn clear_of_roads(collider: &StaticCollider) -> bool {
    let min = [
        collider.position[0] - collider.half_extents[0],
        collider.position[2] - collider.half_extents[2],
    ];
    let max = [
        collider.position[0] + collider.half_extents[0],
        collider.position[2] + collider.half_extents[2],
    ];
    ROADS.iter().all(|road| {
        road.points
            .windows(2)
            .all(|segment| segment_clears_rectangle(segment[0], segment[1], min, max))
    })
}

/// Exact integer test that segment `a–b` stays at least the road clearance
/// away from the closed rectangle `[min, max]`. For a segment that does not
/// cross the rectangle, the closest pair involves a segment endpoint or a
/// rectangle corner.
fn segment_clears_rectangle(a: [i32; 2], b: [i32; 2], min: [i32; 2], max: [i32; 2]) -> bool {
    let clearance = i128::from(ROAD_CLEARANCE_UNITS);
    let clearance_squared = clearance * clearance;
    let corners = [min, [max[0], min[1]], max, [min[0], max[1]]];
    let inside = |point: [i32; 2]| {
        (min[0]..=max[0]).contains(&point[0]) && (min[1]..=max[1]).contains(&point[1])
    };
    if inside(a) || inside(b) {
        return false;
    }
    for edge in 0..4 {
        if segments_intersect(a, b, corners[edge], corners[(edge + 1) % 4]) {
            return false;
        }
    }
    let point_to_rectangle = |point: [i32; 2]| {
        let dx = i128::from((min[0] - point[0]).max(point[0] - max[0]).max(0));
        let dz = i128::from((min[1] - point[1]).max(point[1] - max[1]).max(0));
        dx * dx + dz * dz
    };
    if point_to_rectangle(a) < clearance_squared || point_to_rectangle(b) < clearance_squared {
        return false;
    }
    let direction = [
        i128::from(b[0]) - i128::from(a[0]),
        i128::from(b[1]) - i128::from(a[1]),
    ];
    let length_squared = direction[0] * direction[0] + direction[1] * direction[1];
    corners.iter().all(|corner| {
        let offset = [
            i128::from(corner[0]) - i128::from(a[0]),
            i128::from(corner[1]) - i128::from(a[1]),
        ];
        let along = offset[0] * direction[0] + offset[1] * direction[1];
        if along <= 0 || along >= length_squared {
            // The nearest segment point is an endpoint, already measured above.
            return true;
        }
        let cross = direction[0] * offset[1] - direction[1] * offset[0];
        cross * cross >= clearance_squared * length_squared
    })
}

fn segments_intersect(a: [i32; 2], b: [i32; 2], c: [i32; 2], d: [i32; 2]) -> bool {
    fn orientation(p: [i32; 2], q: [i32; 2], r: [i32; 2]) -> i128 {
        let value = (i128::from(q[0]) - i128::from(p[0])) * (i128::from(r[1]) - i128::from(p[1]))
            - (i128::from(q[1]) - i128::from(p[1])) * (i128::from(r[0]) - i128::from(p[0]));
        value.signum()
    }
    fn on_segment(p: [i32; 2], q: [i32; 2], r: [i32; 2]) -> bool {
        (p[0].min(r[0])..=p[0].max(r[0])).contains(&q[0])
            && (p[1].min(r[1])..=p[1].max(r[1])).contains(&q[1])
    }
    let (o1, o2) = (orientation(a, b, c), orientation(a, b, d));
    let (o3, o4) = (orientation(c, d, a), orientation(c, d, b));
    (o1 != o2 && o3 != o4)
        || (o1 == 0 && on_segment(a, c, b))
        || (o2 == 0 && on_segment(a, d, b))
        || (o3 == 0 && on_segment(c, a, d))
        || (o4 == 0 && on_segment(c, b, d))
}

/// Ground, boundary walls, authored structures and generated trees.
fn colliders() -> Vec<StaticCollider> {
    let extent = PLAYABLE_BOUNDS.max[0] + WALL_THICKNESS;
    let wall_middle = PLAYABLE_BOUNDS.max[0] + WALL_THICKNESS / 2;
    let wall = |id, position: [i32; 2], half: [i32; 2]| StaticCollider {
        id,
        position: [position[0], WALL_HEIGHT / 2, position[1]],
        half_extents: [half[0], WALL_HEIGHT / 2, half[1]],
    };
    let thin = WALL_THICKNESS / 2;
    let mut colliders = vec![
        StaticCollider {
            id: ids::GROUND,
            position: [0, -GROUND_THICKNESS / 2, 0],
            half_extents: [extent, GROUND_THICKNESS / 2, extent],
        },
        wall(2, [-wall_middle, 0], [thin, extent]),
        wall(3, [wall_middle, 0], [thin, extent]),
        wall(4, [0, -wall_middle], [extent, thin]),
        wall(5, [0, wall_middle], [extent, thin]),
    ];
    colliders.extend(STRUCTURES.iter().map(|&(id, min, max, height)| {
        // Structures out on the hills stand on the terrain (#159).
        let mut collider = standing(id, min, max, height);
        collider.position[1] += footing(min, max);
        collider
    }));
    colliders.extend(inn::colliders(*ids::INN_PARTS.start()));
    colliders.extend(cave::GREYFANG.colliders(*ids::CAVE_WALLS.start()));
    colliders.extend(trees());
    colliders
}

/// The hosted collision content. Hosts, clients and scenery consume this
/// exact definition; the spawn grid is validated clear of every collider.
#[must_use]
pub fn base_definition() -> ZoneDefinition {
    ZoneDefinition::with_spawn_grid(REVISION, GRAVITY, SPAWN_GRID, colliders())
        .and_then(|definition| definition.with_terrain(TERRAIN.clone()))
        .expect("built-in Greyhaven Vale content is valid")
}

pub fn greyhaven_vale_definition() -> ZoneDefinition {
    layout::compile(layout::SOURCE).expect("committed Outpost layout is valid")
}

static CONTENT: LazyLock<Arc<ZoneContent>> = LazyLock::new(|| {
    content_with_definition(greyhaven_vale_definition())
        .expect("built-in Greyhaven content is valid")
});

/// Shared final validation for authoring previews and every runtime host.
pub fn content_with_definition(
    definition: ZoneDefinition,
) -> Result<Arc<ZoneContent>, crate::ZoneError> {
    let content = ZoneContent::new(
        definition,
        areas().clone(),
        units::creature_templates(),
        units::creature_spawns(),
        units::npcs(),
        units::GRAVEYARD,
    )?
    .with_rng_seed(RNG_SEED);
    let tables = content
        .creature_templates()
        .iter()
        // Breakable props (#288) drop nothing.
        .filter(|template| template.prop().is_none())
        .map(|template| {
            (
                template.id,
                crate::loot_table(template.id)
                    .expect("each Greyhaven template has an authored loot table")
                    .clone(),
            )
        })
        .collect();
    Ok(Arc::new(
        content
            .with_loot_tables(crate::LOOT_CATALOG_REVISION, tables)?
            .with_creature_abilities(crate::ABILITY_CATALOG_REVISION, units::creature_abilities())?
            .with_vendors(crate::VENDOR_CATALOG_REVISION, units::vendors())?
            .with_quests(crate::QUEST_CATALOG_REVISION, quests::quests())?
            .with_doors(vec![inn::door(ids::INN_DOOR)])?
            .with_routines(units::routines(), units::closed_at_night())?,
    ))
}

/// The complete hosted content: collision definition, areas, creatures,
/// NPCs and graveyard, validated once and shared.
#[must_use]
pub fn content() -> Arc<ZoneContent> {
    Arc::clone(&CONTENT)
}
