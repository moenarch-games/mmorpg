//! Greyhaven Vale, the starter zone: immutable collision content, named
//! areas, the road corridors that must stay walkable, and the creatures,
//! NPCs and graveyard of [`units`].
//!
//! The Vale spans ±208 m around the origin; south of it, behind a mountain ridge,
//! the Farmland and Hamlets region of [`farmland`] reaches z = +640 m (#424), and north of it,
//! above an escarpment, the Bandit Highlands of [`highlands`] reach z = −632 m (#425); east of
//! it, beyond a second ridge, the Marsh-Lake Wetlands of [`marsh`] reach x = +632 m (#426).
//! Invisible boundary walls enclose all four ([`WALLED`]). Map north is −Z, toward the Redbrand
//! cliffs, matching the
//! layout table in `docs/STARTER_ZONE.md`. The Outpost's valley floor is physically flat (y = 0),
//! in the basin of [`basin`], below its rim and the outer hills; relief, props
//! without colliders, water and mountains are presentation owned by
//! `mmorpg-scenery`, which derives its structures from the collider IDs in
//! [`ids`]. [`content`] bundles it as the hosted [`ZoneContent`], whose revision
//! is a hash of all of it: changing anything here changes the revision by itself.

use std::sync::{Arc, LazyLock};

use super::{SpawnGrid, StaticCollider, XzBounds, ZoneContent, ZoneDefinition};
use crate::{Area, AreaId, Terrain, ZoneAreas};

pub mod basin;
pub mod cave;
pub mod farmland;
pub mod highlands;
pub mod inn;
pub mod layout;
pub mod marsh;
pub mod marshal_house;
pub mod quests;
pub mod raid;
pub mod shrine;
pub mod units;
pub mod vale;

/// The fixed simulation seed, independent of the content revision, so content and economy
/// changes do not reroll Greyhaven's existing creature/combat scripts.
pub const RNG_SEED: u64 = 0x3cbc_808b_be89_b29c;
/// Gravity in subunits per tick squared, authored as 9 m/s² down.
pub const GRAVITY: [i32; 3] = crate::STANDARD_GRAVITY;
/// The rectangle round everything the boundary walls enclose: the Vale, the Farmland and
/// Hamlets region south of it (#424), the Bandit Highlands north of it (#425) and the
/// Marsh-Lake Wetlands east of it (#426). Its corners beyond the farmland and the highlands lie
/// outside the walls ([`WALLED`]).
pub const PLAYABLE_BOUNDS: XzBounds = XzBounds {
    min: [-20_800, highlands::BOUNDS.min[1]],
    max: [marsh::BOUNDS.max[0], farmland::BOUNDS.max[1]],
};
/// The inner faces of the boundary walls enclose these rectangles: the column of the
/// highlands, the Vale and the farmland, and the wetlands east of the Vale (#426).
pub const WALLED: [XzBounds; 2] = [
    XzBounds {
        min: [-20_800, highlands::BOUNDS.min[1]],
        max: [20_800, farmland::BOUNDS.max[1]],
    },
    marsh::BOUNDS,
];

/// Chebyshev distance (units) from `(x, z)` to the walled ground ([`WALLED`]): positive
/// outside the walls, zero or negative inside.
#[must_use]
pub fn beyond_walls(x: i32, z: i32) -> i64 {
    WALLED
        .iter()
        .map(|bounds| {
            let over = |value: i32, low: i32, high: i32| {
                (i64::from(low) - i64::from(value)).max(i64::from(value) - i64::from(high))
            };
            over(x, bounds.min[0], bounds.max[0]).max(over(z, bounds.min[1], bounds.max[1]))
        })
        .min()
        .unwrap_or(i64::MAX)
}

/// Whether `(x, z)` lies inside the walls (half-open, like every rectangle here).
#[must_use]
pub fn inside_walls(x: i32, z: i32) -> bool {
    WALLED.iter().any(|bounds| bounds.contains(x, z))
}
/// Greyhaven Vale itself, the square between the highlands' escarpment and the farmland's
/// ridge.
pub const VALE_BOUNDS: XzBounds = XzBounds {
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
    /// West, east, north (−Z) and south (+Z) walls, then the column's eastern walls north and
    /// south of the wetlands and the wetlands' northern and southern walls (#426).
    pub const BOUNDARY_WALLS: RangeInclusive<u32> = 2..=9;

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
    /// Marshal Elden Greywatch's private home (#158): house 103's footprint is its presentation
    /// anchor, not a collider; its front door leaf, then its walls and furniture.
    pub const MARSHAL_HOUSE: u32 = 103;
    pub const MARSHAL_HOUSE_DOOR: u32 = 182;
    pub const MARSHAL_HOUSE_PARTS: RangeInclusive<u32> = 183..=198;

    pub const BARN: u32 = 200;
    pub const FARMHOUSE: u32 = 201;
    pub const WINDMILL: u32 = 202;
    /// The mill hoist (#56): a lift pillar at the windmill's south face.
    pub const MILL_HOIST: u32 = 203;
    /// The mill's loading dock (#59) and its ramps.
    pub const MILL_DOCK: u32 = 204;
    pub const MILL_RAMPS: RangeInclusive<u32> = 205..=206;
    pub const FENCES: RangeInclusive<u32> = 210..=214;

    pub const SHORE_ROCKS: RangeInclusive<u32> = 300..=305;

    pub const CLIFFS: RangeInclusive<u32> = 400..=406;
    pub const MINE_ENTRANCE: u32 = 410;
    pub const TENTS: RangeInclusive<u32> = 420..=422;
    pub const CRATES: RangeInclusive<u32> = 430..=433;
    pub const CAMPFIRE: u32 = 440;
    /// The Redbrand watch post's destructible pieces (#305).
    pub const WATCH_POST: RangeInclusive<u32> = 460..=462;

    /// The Greyfang standing stones (#159).
    pub const STANDING_STONES: RangeInclusive<u32> = 500..=506;
    /// The Southmeadow ruined farmstead's houses and fences (#159).
    pub const RUINS: RangeInclusive<u32> = 510..=511;
    pub const RUIN_FENCES: RangeInclusive<u32> = 512..=514;
    /// The Eastwater watchtower (#159).
    pub const WATCHTOWER: u32 = 520;
    /// Hollowpine Reach boulders (#159).
    pub const BOULDERS: RangeInclusive<u32> = 530..=537;
    /// Ladders up the southern faces of the Eastwater watchtower, the highland lookout and the
    /// drowned watchtower (#490), in [`super::LADDER_TOWERS`] order.
    pub const LADDERS: RangeInclusive<u32> = 600..=602;
    /// The Greyfang cave's rock walls and roof slab (#159).
    pub const CAVE_WALLS: RangeInclusive<u32> = 540..=546;
    pub const CAVE_ROOF: u32 = 547;
    /// The sealed threshold to Redbrand Hollow's deeper cave, in the inner chamber (#160).
    pub const CAVE_THRESHOLD: u32 = 548;
    /// The Sunken Shrine (#274): its ruins and altar, its cave's rock walls and roof slabs,
    /// drawn as asset-tooling's shrine kit (#431).
    pub const SHRINE_RUINS: RangeInclusive<u32> = 550..=564;
    pub const SHRINE_ALTAR: u32 = 564;
    pub const SHRINE_CAVE_WALLS: RangeInclusive<u32> = 565..=575;
    pub const SHRINE_CAVE_ROOF: RangeInclusive<u32> = 580..=596;

    /// Trunks, numbered in generation order from the start: Wolfrun Woods
    /// first, then the outer woods (#159).
    pub const TREES: RangeInclusive<u32> = 1_000..=4_999;

    /// Farmland and Hamlets (#424): the buildings in [`super::farmland::BUILDINGS`] order …
    pub const FARMLAND_BUILDINGS: RangeInclusive<u32> = 6_000..=6_099;
    /// … and the field fences in [`super::farmland::fences`] order.
    pub const FARMLAND_FENCES: RangeInclusive<u32> = 6_100..=6_199;

    /// Bandit Highlands (#425): the structures in [`super::highlands::BUILDINGS`] order …
    pub const HIGHLAND_BUILDINGS: RangeInclusive<u32> = 6_200..=6_299;
    /// … and the rock crags in [`super::highlands::CRAGS`] order.
    pub const HIGHLAND_CRAGS: RangeInclusive<u32> = 6_300..=6_399;

    /// Marsh-Lake Wetlands (#426): the structures in [`super::marsh::BUILDINGS`] order.
    pub const MARSH_BUILDINGS: RangeInclusive<u32> = 6_400..=6_499;
}

/// One region's tables (#542): its named areas, its roads and its own colliders. Each region
/// module defines its `REGION`; [`REGIONS`] lists them once, and the zone concatenates their
/// tables in that order. A new region adds its module and one line there.
struct Region {
    /// `(id, name, half-open bounds)`, like every other rectangle in this module.
    areas: &'static [(AreaId, &'static str, XzBounds)],
    roads: &'static [Road],
    /// The region's colliders, beside the frame's ground, walls, trees and ladders.
    colliders: fn() -> Vec<StaticCollider>,
}

/// The regions in table order: area, road and collider order follow this list.
const REGIONS: &[Region] = &[
    vale::REGION,
    shrine::REGION,
    farmland::REGION,
    highlands::REGION,
    marsh::REGION,
];

static AREAS: LazyLock<ZoneAreas> = LazyLock::new(|| {
    let areas = REGIONS
        .iter()
        .flat_map(|region| region.areas)
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

static ROADS: LazyLock<Vec<Road>> = LazyLock::new(|| {
    REGIONS
        .iter()
        .flat_map(|region| region.roads)
        .copied()
        .collect()
});

/// Every region's roads, in region order.
#[must_use]
pub fn roads() -> &'static [Road] {
    &ROADS
}

/// `(id, footprint min [x, z], footprint max [x, z], height)`: authored boxes
/// standing on y = 0. Every coordinate is a multiple of ten units, and boxes
/// may touch but never overlap, so each collider has one unambiguous visual.
type Footprint = (u32, [i32; 2], [i32; 2], i32);

/// The outer valley's ground (#159): a 4 m grid of heights over the playable rectangle and a
/// little beyond. The original valley stays flat; hills rise across the ring of outer
/// regions and settle again before the boundary walls. South of them the farmland's ridge and
/// plateau (#424), and north of them the highlands' escarpment and high ground (#425),
/// continue the grid.
pub mod terrain {
    /// The grid's first sample, in units; aligned with the clients' 4 m terrain grid. It lies
    /// 4 m beyond the highlands' northern wall (#425), exactly 106 rows (one physics ground
    /// tile) north of the Vale's first row, so the Vale's tile keeps its samples.
    pub const ORIGIN: [i32; 2] = [-21_200, -63_600];
    pub const STEP: i32 = 400;
    /// Columns reach 4 m beyond the wetlands' eastern wall (#426): exactly 106 more (one
    /// physics ground tile) than the Vale's, so the Vale's tile keeps its samples.
    pub const COLUMNS: u16 = 213;
    /// Rows reach 4 m beyond the farmland's southern wall (#424).
    pub const ROWS: u16 = 321;
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

/// Stillwater Lake's authoritative water (#274): the ellipse the clients draw, its surface
/// 0.15 m above the lake terrace's ground (#423).
#[must_use]
pub fn stillwater_lake() -> crate::WaterVolume {
    crate::WaterVolume {
        name: "Stillwater Lake".to_owned(),
        centre: lake::CENTRE,
        radii: lake::RADII,
        surface: lake::SURFACE,
        air_pockets: Vec::new(),
    }
}

/// Stillwater Lake's basin (#274).
pub mod lake {
    pub const CENTRE: [i32; 2] = [5_500, -5_500];
    pub const RADII: [i32; 2] = [2_200, 1_600];
    /// The shore: the terrace the lake lies in (#423).
    pub const SHORE_Y: i32 = super::basin::TERRACE_Y;
    pub const SURFACE: i32 = SHORE_Y + 15;
    /// The basin falls linearly from the shallow rim to this depth (9 m) at the centre …
    pub const DEPTH: i64 = 900;
    /// … starting at this fraction of the ellipse (Q10): the outer 30 % stays ankle-deep and
    /// level, where the lurkers wade (#423).
    pub const RIM_Q10: i64 = 717;
}

/// How far (units) the lake basin lowers the ground at `(x, z)`.
#[must_use]
pub fn lake_depth(x: i32, z: i32) -> i32 {
    let dx = i128::from(x) - i128::from(lake::CENTRE[0]);
    let dz = i128::from(z) - i128::from(lake::CENTRE[1]);
    let (rx, rz) = (i128::from(lake::RADII[0]), i128::from(lake::RADII[1]));
    // The normalised ellipse distance in Q10.
    let squared = (dx * dx * rz * rz + dz * dz * rx * rx) * (1 << 20) / (rx * rx * rz * rz);
    let distance = i64::try_from(squared.isqrt()).unwrap_or(i64::MAX);
    if distance >= lake::RIM_Q10 {
        return 0;
    }
    i32::try_from(lake::DEPTH * (lake::RIM_Q10 - distance) / lake::RIM_Q10).unwrap_or(0)
}

/// The ground wherever no cave flattens it: the hills (#159), the basin's terraces and their
/// ways down (#423), less the lake's basin (#274); the farmland's ridge, notch and plateau
/// (#424) and the highlands' escarpment, pass and high ground (#425) beyond.
fn open_ground(x: i32, z: i32) -> i32 {
    if let Some(height) = farmland::height_at(x, z)
        .or_else(|| highlands::height_at(x, z))
        .or_else(|| marsh::height_at(x, z))
    {
        return height;
    }
    let hill = hill_height(x, z);
    basin::height_at(x, z).map_or(hill, |terrace| terrace.max(hill)) - lake_depth(x, z)
}

/// The outer valley's terrain grid (#159), sampled from [`hill_height`].
#[must_use]
pub fn terrain() -> Terrain {
    let (columns, rows) = (i32::from(terrain::COLUMNS), i32::from(terrain::ROWS));
    let heights = (0..rows)
        .flat_map(|row| {
            (0..columns).map(move |column| {
                let x = terrain::ORIGIN[0] + column * terrain::STEP;
                let z = terrain::ORIGIN[1] + row * terrain::STEP;
                // The cave floor stays flat inside its hillside.
                if cave::GREYFANG.flattens(x, z) {
                    cave::GREYFANG.floor_y
                } else if shrine::flattens(x, z) {
                    // The Sunken Shrine's plaza and cave floor (#274).
                    shrine::FLOOR_Y
                } else {
                    open_ground(x, z)
                }
            })
        })
        .collect();
    Terrain {
        origin: terrain::ORIGIN,
        step: terrain::STEP,
        columns: terrain::COLUMNS,
        rows: terrain::ROWS,
        heights,
    }
}

static TERRAIN: LazyLock<Terrain> = LazyLock::new(terrain);

/// Where a footprint `[min, max]` rests on the terrain: on level ground (the valley floor, the
/// farm terrace) at its height, on a slope at its lowest corner, sunk 0.3 m so nothing floats.
fn footing(min: [i32; 2], max: [i32; 2]) -> i32 {
    let corners = [min, [max[0], min[1]], max, [min[0], max[1]]]
        .map(|corner| TERRAIN.height_at(corner[0], corner[1]));
    let lowest = corners.iter().copied().min().unwrap_or(0);
    if corners.iter().all(|&height| height == lowest) {
        lowest
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

/// A box over an authored footprint, resting on the terrain there (#159).
fn on_terrain(id: u32, min: [i32; 2], max: [i32; 2], height: i32) -> StaticCollider {
    let mut collider = standing(id, min, max, height);
    collider.position[1] += footing(min, max);
    collider
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
const OUTER_FORESTS: [([i32; 2], [i32; 2], u64, u64); 7] = [
    // Hollowpine Reach: dense pines west of Wolfrun Woods.
    ([-20_400, -11_600], [17, 52], 0x484f_4c4c_4f57_5049, 60),
    // Greyfang Ridge and Southmeadow Downs: scattered copses.
    ([-18_000, -20_400], [80, 10], 0x4752_4559_4641_4e47, 12),
    ([-18_000, 16_400], [80, 9], 0x534f_5554_484d_4544, 12),
    // Eastwater Fen: sparse alders east of the farm and the lake.
    ([12_600, -11_600], [17, 52], 0x4541_5354_5741_5445, 15),
    // Farmland and Hamlets (#424): lone trees between the fields, off the level ground.
    ([-20_400, 25_600], [90, 84], 0x4641_524d_5452_4545, 5),
    // Bandit Highlands (#425): scattered pines across the high ground, off its level ground
    // and clear of its crags.
    ([-20_400, -62_800], [90, 77], 0x4849_4748_5452_4545, 4),
    // Marsh-Lake Wetlands (#426): lone trees across the wetlands, off the water.
    ([25_600, -20_400], [82, 90], 0x4d41_5253_4854_5245, 3),
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
            // On the rim (#423).
            let base = footing([x - half, z - half], [x + half, z + half]);
            let collider = StaticCollider {
                id: next_id,
                position: [x, base + woods::TRUNK_HEIGHT / 2, z],
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
                // Hamlets, fields and the tower's yard, with their aprons, stay clear (#424).
                if farmland::level(x, z)
                    || !highlands::clear_for_tree(x, z)
                    || (x >= marsh::BOUNDS.min[0] && !marsh::clear_for_tree(x, z))
                {
                    continue;
                }
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
    let mut footprints = vale::STRUCTURES
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
    // The ground spans the rectangle round the walls' outer faces.
    let min = PLAYABLE_BOUNDS.min.map(|value| value - WALL_THICKNESS);
    let max = PLAYABLE_BOUNDS.max.map(|value| value + WALL_THICKNESS);
    let centre = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2];
    let half = [(max[0] - min[0]) / 2, (max[1] - min[1]) / 2];
    // A wall over the footprint `[min, max]`.
    let wall = |id, min: [i32; 2], max: [i32; 2]| StaticCollider {
        id,
        position: [
            (min[0] + max[0]) / 2,
            WALL_HEIGHT / 2,
            (min[1] + max[1]) / 2,
        ],
        half_extents: [
            (max[0] - min[0]) / 2,
            WALL_HEIGHT / 2,
            (max[1] - min[1]) / 2,
        ],
    };
    // The walls close round the column of the highlands, the Vale and the farmland and round
    // the wetlands east of the Vale (#426).
    let [column, wetlands] = WALLED;
    let thick = WALL_THICKNESS;
    let ([west, north], [east, south]) = (column.min, column.max);
    let ([_, wet_north], [wet_east, wet_south]) = (wetlands.min, wetlands.max);
    let mut colliders = vec![
        StaticCollider {
            id: ids::GROUND,
            position: [centre[0], -GROUND_THICKNESS / 2, centre[1]],
            half_extents: [half[0], GROUND_THICKNESS / 2, half[1]],
        },
        wall(2, [west - thick, north - thick], [west, south + thick]),
        wall(
            3,
            [wet_east, wet_north - thick],
            [wet_east + thick, wet_south + thick],
        ),
        wall(4, [west - thick, north - thick], [east + thick, north]),
        wall(5, [west - thick, south], [east + thick, south + thick]),
        wall(6, [east, north - thick], [east + thick, wet_north]),
        wall(7, [east, wet_south], [east + thick, south + thick]),
        wall(8, [east, wet_north - thick], [wet_east + thick, wet_north]),
        wall(9, [east, wet_south], [wet_east + thick, wet_south + thick]),
    ];
    colliders.extend(REGIONS.iter().flat_map(|region| (region.colliders)()));
    colliders.extend(trees());
    colliders.extend((*ids::LADDERS.start()..).zip(LADDER_TOWERS).map(ladder));
    colliders
}

/// The towers with a ladder (#490): `(footprint min, footprint max, height)` of the Eastwater
/// watchtower, the highland lookout and the drowned watchtower.
pub const LADDER_TOWERS: [([i32; 2], [i32; 2], i32); 3] = [
    ([16_500, -6_200], [16_900, -5_800], 1_400),
    ([12_000, -39_600], [12_800, -38_800], 1_400),
    ([45_600, -2_400], [46_400, -1_600], 1_800),
];

/// A ladder 0.6 m wide and 0.1 m deep up the middle of a tower's southern (+Z) face, standing
/// on the ground there and reaching the tower's flat top.
fn ladder((id, (min, max, height)): (u32, ([i32; 2], [i32; 2], i32))) -> StaticCollider {
    let middle = (min[0] + max[0]) / 2;
    let (low, high) = ([middle - 30, max[1]], [middle + 30, max[1] + 10]);
    let base = footing(low, high);
    let top = footing(min, max) + height;
    let mut collider = standing(id, low, high, top - base);
    collider.position[1] += base;
    collider
}

/// Which collision is climbable (#490). Rough rock (crags, the Redbrand cliffs, boulders and
/// shore rocks), timber (the palisades and fences) and the ladders climb; buildings, towers,
/// standing stones and cave walls do not. Each kind keeps unclimbable variants: the Redbrand
/// Hollow's sheer back wall, two sheer highland crags and the stronghold's smooth-planked
/// northern palisade. On the terrain, the Vale basin's cliffs are rough rock except the sheer
/// cliff under Stillwater Lake, and each boundary ridge or escarpment has one hard climbable
/// route beside its pass while the rest of it is sheer.
#[must_use]
pub fn climbing() -> crate::ClimbSurfaces {
    use crate::Grip::{Ladder, Rock, Timber};
    let range = |ids: std::ops::RangeInclusive<u32>, grip| ids.map(move |id| (id, grip));
    // The stronghold's northern palisade (its second structure) is smooth-planked.
    let palisades = highlands::BUILDINGS
        .iter()
        .zip(*ids::HIGHLAND_BUILDINGS.start()..)
        .filter(|((building, ..), id)| {
            *building == highlands::Building::Palisade && *id != ids::HIGHLAND_BUILDINGS.start() + 1
        })
        .map(|(_, id)| (id, Timber));
    // Two of the tallest crags are sheer: west of the trail's bend and west of the stronghold.
    let crags = (*ids::HIGHLAND_CRAGS.start()..)
        .zip(highlands::CRAGS)
        .enumerate()
        .filter(|&(index, _)| index != SHEER_CRAGS[0] && index != SHEER_CRAGS[1])
        .map(|(_, (id, _))| (id, Rock));
    let colliders = range(ids::PALISADE, Timber)
        .chain(range(ids::FENCES, Timber))
        .chain(range(ids::RUIN_FENCES, Timber))
        .chain(range(ids::FARMLAND_FENCES, Timber).take(farmland::fences().len()))
        .chain(palisades)
        .chain(range(ids::SHORE_ROCKS, Rock))
        .chain(range(ids::BOULDERS, Rock))
        // The Redbrand Hollow's cliffs, but not its sheer back wall.
        .chain(range(ids::CLIFFS, Rock).filter(|&(id, _)| id != SHEER_HOLLOW_WALL))
        .chain(crags)
        .chain(range(ids::LADDERS, Ladder))
        .collect();
    let terrain = vec![
        // The sheer cliff under Stillwater Lake, then the rest of the Vale basin's cliffs.
        (SHEER_LAKE_CLIFF, None),
        (VALE_BASIN, Some(Rock)),
        // One hard route up each boundary ridge or escarpment, beside its pass.
        (CLIMBING_ROUTES[0], Some(Rock)),
        (CLIMBING_ROUTES[1], Some(Rock)),
        (CLIMBING_ROUTES[2], Some(Rock)),
    ];
    crate::ClimbSurfaces::new(colliders, terrain).expect("Greyhaven climbing is valid")
}

/// The Redbrand Hollow's 14 m back wall under the north wall: sheer (#490).
pub const SHEER_HOLLOW_WALL: u32 = 404;
/// The sheer highland crags, by index in [`highlands::CRAGS`] (#490).
pub const SHEER_CRAGS: [usize; 2] = [4, 12];
/// The Vale basin (#423) and its cliffs, rough rock (#490).
pub const VALE_BASIN: XzBounds = XzBounds {
    min: [-12_800, -12_800],
    max: [12_800, 12_800],
};
/// The basin's northern cliff under Stillwater Lake: sheer (#490).
pub const SHEER_LAKE_CLIFF: XzBounds = XzBounds {
    min: [2_000, -3_200],
    max: [12_800, -2_000],
};
/// The boundary climbing routes (#490): up the highlands' escarpment east of the Highland Pass,
/// over the farmland's ridge west of the South Road's notch and over the wetlands' ridge south
/// of the Marsh Pass. Each is 8 m wide.
pub const CLIMBING_ROUTES: [XzBounds; 3] = [
    XzBounds {
        min: [12_000, -28_000],
        max: [12_800, -20_800],
    },
    XzBounds {
        min: [-2_400, 20_800],
        max: [-1_600, 25_600],
    },
    XzBounds {
        min: [20_800, 6_400],
        max: [25_600, 7_200],
    },
];

/// The mill hoist (#56): a 2 m square stone pillar against the windmill's south face that
/// rests with its top flush with the farm terrace, rises 8 m to the windmill's gallery (its flat
/// top) at 1.5 m/s, rests 5 s and sinks back. Its box fills the shaft below its top, so it
/// sweeps no space a unit could stand in except on top.
pub const MILL_HOIST: crate::Platform = crate::Platform {
    id: crate::PlatformId::new(1),
    body: ids::MILL_HOIST,
    half_extents: [100, 400, 100],
    low: [8_800, basin::TERRACE_Y - 400, 1_748],
    high: [8_800, basin::TERRACE_Y + 400, 1_748],
    speed: crate::subunits_per_tick(150),
    dwell: crate::long_ticks_from_ms(5_000),
};

/// The mill's loading dock ramps (#59): a shallow cart ramp (17°) up the dock's west side
/// and a steep stone chute (60°) on its north side that no unit can walk up.
pub const MILL_RAMPS: [crate::Ramp; 2] = [
    crate::Ramp {
        body: *ids::MILL_RAMPS.start(),
        position: [7_850, basin::TERRACE_Y + 75, 1_950],
        half_extents: [250, 75, 100],
        rises: crate::RampRise::PosX,
    },
    crate::Ramp {
        body: *ids::MILL_RAMPS.end(),
        position: [8_250, basin::TERRACE_Y + 75, 1_757],
        half_extents: [100, 75, 43],
        rises: crate::RampRise::PosZ,
    },
];

/// Greyhaven's destructible sites (#305): the Redbrand camp, whose pieces rebuild after 5 min.
#[must_use]
pub fn structure_sites() -> Vec<crate::StructureSite> {
    vec![crate::StructureSite {
        id: 1,
        name: "Redbrand Hollow".to_owned(),
        regrowth_ticks: crate::long_ticks_from_ms(300_000),
    }]
}

/// The Redbrand watch post (#305) east of the Hollow Road at the camp's southern edge: two
/// timber walls 3 m wide and 2.5 m tall and the lookout deck resting on both. A broken west
/// wall leaves a knee-high rubble heap that still blocks the way.
#[must_use]
pub fn watch_post() -> Vec<crate::StructurePiece> {
    let [west, east, deck] = [460, 461, 462];
    let wall =
        |id: u16, body: u32, x: i32, rubble: Option<crate::PieceBox>| crate::StructurePiece {
            id: crate::PieceId::new(id),
            site: 1,
            body,
            position: [x, basin::TERRACE_Y + 125, -7_200],
            half_extents: [150, 125, 20],
            health: 120,
            supports: Vec::new(),
            rubble,
        };
    vec![
        wall(
            1,
            west,
            500,
            Some(([500, basin::TERRACE_Y + 20, -7_200], [150, 20, 40])),
        ),
        wall(2, east, 900, None),
        crate::StructurePiece {
            id: crate::PieceId::new(3),
            site: 1,
            body: deck,
            position: [700, basin::TERRACE_Y + 265, -7_200],
            half_extents: [350, 15, 120],
            health: 60,
            supports: vec![crate::PieceId::new(1), crate::PieceId::new(2)],
            rubble: None,
        },
    ]
}

/// The hosted collision content. Hosts, clients and scenery consume this
/// exact definition; the spawn grid is validated clear of every collider.
#[must_use]
pub fn base_definition() -> ZoneDefinition {
    ZoneDefinition::with_spawn_grid(GRAVITY, SPAWN_GRID, colliders())
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
            .with_loot_tables(tables)?
            .with_creature_abilities(units::creature_abilities())?
            .with_bosses(units::bosses())?
            .with_shielded(units::shielded())?
            .with_vendors(units::vendors())?
            .with_quests(quests::quests())?
            .with_doors(vec![
                inn::door(ids::INN_DOOR),
                marshal_house::door(ids::MARSHAL_HOUSE_DOOR),
            ])?
            .with_platforms(vec![MILL_HOIST])?
            .with_ramps(MILL_RAMPS.to_vec())?
            .with_structures(structure_sites(), watch_post())?
            .with_routines(units::routines(), units::closed_at_night())?
            .with_private_homes(vec![marshal_house::home()])?
            .with_waters(
                [stillwater_lake(), shrine::cave_water()]
                    .into_iter()
                    .chain(marsh::water_volumes())
                    .collect(),
            )?
            .with_climbing(climbing())?
            .with_raid(raid::millbrook_barn())?
            .with_waystones(vec![crate::GREYHAVEN_OUTPOST])?,
    ))
}

/// The complete hosted content: collision definition, areas, creatures,
/// NPCs and graveyard, validated once and shared.
#[must_use]
pub fn content() -> Arc<ZoneContent> {
    Arc::clone(&CONTENT)
}
