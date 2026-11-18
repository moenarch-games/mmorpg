//! Greyhaven Vale, the starter zone: immutable collision content, named
//! areas, the road corridors that must stay walkable, and the creatures,
//! NPCs and graveyard of [`units`].
//!
//! The zone covers the whole Greyhaven Vale region of the continent atlas ([`region`], #810),
//! and invisible boundary walls follow the region's outline. Its ground is hand-authored only in
//! the Vale's heart round the Outpost's basin ([`beyond_core`]); everywhere else it is the
//! region's designed landscape ([`design`], #816), with the Farmland and Hamlets of
//! [`farmland`] to the south (#424), the Bandit Highlands of [`highlands`] beyond Greyfang
//! Ridge to the north (#425) and the Marsh-Lake Wetlands of [`marsh`] to the east (#426). Map
//! north is −Z, toward the Redbrand cliffs, matching the layout table in
//! `docs/STARTER_ZONE.md`. The Outpost's settled floor stays level (y = 0),
//! with gentle rises on its northern approach in [`basin`], below the rim; relief, props
//! without colliders, water and mountains are presentation owned by
//! `mmorpg-scenery`, which derives its structures from the collider IDs in
//! [`ids`]. [`content`] bundles it as the hosted [`ZoneContent`], whose revision
//! is a hash of all of it: changing anything here changes the revision by itself.

use std::sync::{Arc, LazyLock, OnceLock};

use super::{SpawnGrid, StaticCollider, XzBounds, ZoneContent, ZoneDefinition};
use crate::{Area, AreaId, Terrain, ZoneAreas};

pub mod basin;
pub mod brook;
pub mod cave;
pub mod cottages;
pub mod crossings;
pub mod design;
pub mod dressing;
pub mod farmland;
pub mod fort;
pub mod hamlet_creatures;
pub mod highland_bandits;
pub mod highlands;
pub mod inn;
pub mod landscape;
pub mod layout;
pub mod market;
pub mod marsh;
pub mod marsh_creatures;
pub mod marshal_house;
pub mod paint;
pub mod quests;
pub mod raid;
pub mod region;
pub mod residents;
mod roofs;
pub mod settlements;
pub mod shrine;
pub mod traps;
pub mod units;
pub mod vale;
pub mod village_props;
pub mod wilds;

/// The fixed simulation seed, independent of the content revision, so content and economy
/// changes do not reroll Greyhaven's existing creature/combat scripts.
pub const RNG_SEED: u64 = 0x3cbc_808b_be89_b29c;
/// Gravity in subunits per tick squared, authored as 9 m/s² down.
pub const GRAVITY: [i32; 3] = crate::STANDARD_GRAVITY;
/// The rectangle round the region's outline, which the boundary walls follow (#810): the
/// bounds of [`region::Region::outline`].
pub const PLAYABLE_BOUNDS: XzBounds = XzBounds {
    min: [-184_000, -134_000],
    max: [188_000, 128_000],
};
/// Units from `(x, z)` to the hand-authored core (#816): the Vale's heart round the Outpost's
/// basin, Millbrook Farm, Stillwater Lake, Redbrand Hollow and the Greyfang cave, inside the
/// organic outline of `design.json`'s `heart` ([`design::heart_distance`]). Positive outside
/// it, zero or negative inside. Its ground is authored here; round it the region's designed
/// landscape ([`design`]) takes over within [`terrain::BLEND_UNITS`].
#[must_use]
pub fn beyond_core(x: i32, z: i32) -> i64 {
    design::heart_distance(x, z)
}

/// The first regions' extents: the column of the highlands (#425), the Vale and the farmland
/// (#424), and the wetlands east of the Vale (#426). Their decoration scatters within them.
pub const AUTHORED: [XzBounds; 2] = [
    XzBounds {
        min: [-20_800, highlands::BOUNDS.min[1]],
        max: [20_800, farmland::BOUNDS.max[1]],
    },
    marsh::BOUNDS,
];

/// Chebyshev distance (units) from `(x, z)` to the first regions ([`AUTHORED`]): positive
/// outside them, zero or negative inside.
#[must_use]
pub fn beyond_authored(x: i32, z: i32) -> i64 {
    AUTHORED
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

/// Whether `(x, z)` lies in the first regions (half-open, like every rectangle here).
#[must_use]
pub fn inside_authored(x: i32, z: i32) -> bool {
    AUTHORED.iter().any(|bounds| bounds.contains(x, z))
}

/// Whether `(x, z)` lies inside the boundary walls: the region's outline (#810).
#[must_use]
pub fn inside_walls(x: i32, z: i32) -> bool {
    region::data().contains(x, z)
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
/// Boundary walls stand 20 m above the ground (a jump peaks near 1.4 m) and reach as far below
/// it.
const WALL_HEIGHT: i32 = 2_000;
/// The ground's box reaches this far beyond [`PLAYABLE_BOUNDS`].
const WALL_THICKNESS: i32 = 200;
/// The longest stretch of boundary wall between two of its columns, so its faces follow the
/// ground, and the spans per wall piece: at most 40 m, so a piece's bounds hug the wall (#816).
const WALL_COLUMN_SPACING: i64 = 2_000;
const WALL_SPANS_PER_PIECE: usize = 2;
const GROUND_THICKNESS: i32 = 100;

/// Collider IDs by structure. `mmorpg-scenery` maps each ID to its visual.
pub mod ids {
    use std::ops::RangeInclusive;

    pub const GROUND: u32 = 1;
    /// The boundary wall along the region's outline (#810): invisible mesh pieces in outline
    /// order, each over at most 40 m of it (#816).
    pub const BOUNDARY_WALLS: RangeInclusive<u32> = 9_000..=9_999;

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
    /// Closed roof/ceiling collision for the walk-in inn.
    pub const INN_ROOF: u32 = 148;
    /// The inn's front door leaf (#158).
    pub const INN_DOOR: u32 = 149;
    /// The inn's walls and furniture (#158).
    pub const INN_PARTS: RangeInclusive<u32> = 150..=181;
    /// Marshal Elden Greywatch's private home (#158): house 103's footprint is its presentation
    /// anchor, not a collider; its front door leaf, then its walls and furniture.
    pub const MARSHAL_HOUSE: u32 = 103;
    pub const MARSHAL_HOUSE_DOOR: u32 = 182;
    pub const MARSHAL_HOUSE_PARTS: RangeInclusive<u32> = 183..=198;
    /// Closed roof/ceiling collision for Elden's walk-in house.
    pub const MARSHAL_HOUSE_ROOF: u32 = 199;

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
    /// Climbable ropes (#621), in [`super::ROPES`] order.
    pub const ROPES: RangeInclusive<u32> = 610..=612;
    /// The Greyfang cave's rock walls and roof slab (#159).
    pub const CAVE_WALLS: RangeInclusive<u32> = 540..=546;
    pub const CAVE_ROOF: u32 = 547;
    /// Authored outer-cave floor slabs, slope and elevated shelf (#639).
    pub const CAVE_FLOORS: RangeInclusive<u32> = 640..=643;
    pub const CAVE_LEDGE: RangeInclusive<u32> = 644..=645;
    /// Hanging rock in the public chambers and stone jambs at the north mouth.
    pub const CAVE_CEILING_DETAILS: RangeInclusive<u32> = 660..=663;
    pub const CAVE_ENTRANCE_DETAILS: RangeInclusive<u32> = 664..=665;
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

    /// Solid dressing (#577): the fixed signposts, barrels, carts and dock in
    /// [`super::dressing`] order …
    pub const DRESSING: RangeInclusive<u32> = 7_000..=7_089;
    /// The plank step up onto the Lakeshore dock (#577), a ramp body.
    pub const DOCK_RAMP: u32 = 7_090;
    /// … the Outpost layout's lamps and signposts, each in layout order …
    pub const LAYOUT_LAMPS: RangeInclusive<u32> = 7_100..=7_299;
    pub const LAYOUT_SIGNPOSTS: RangeInclusive<u32> = 7_300..=7_499;
    /// … the market square's collision proxies ([`super::market`]) …
    pub const MARKET: RangeInclusive<u32> = 7_500..=7_599;
    /// … and the loose rocks.
    pub const LOOSE_ROCKS: RangeInclusive<u32> = 8_000..=8_999;

    /// The region's designed forests' trunks (#816), in forest order …
    pub const REGION_TREES: RangeInclusive<u32> = 20_000..=39_999;
    /// … and its rock fields' boulders.
    pub const REGION_BOULDERS: RangeInclusive<u32> = 40_000..=41_999;
    /// The settlements' and landmarks' structures (#818), in [`super::settlements`] order.
    pub const SETTLEMENTS: RangeInclusive<u32> = 42_000..=42_999;
    /// The footbridges' decks and rails, then (from +50) their end ramps (#828), in
    /// [`super::crossings::BRIDGES`] order.
    pub const CROSSINGS: RangeInclusive<u32> = 43_000..=43_099;
    /// The walk-in cottages' walls and furniture (#843), 16 IDs each, in
    /// [`super::cottages::cottages`] order.
    pub const COTTAGES: RangeInclusive<u32> = 43_100..=43_999;
    /// The villages' clutter (#851), 4 IDs each, in [`super::village_props::PROPS`] order.
    pub const VILLAGE_PROPS: RangeInclusive<u32> = 44_000..=44_199;
    /// The walk-in farmhouses' walls and furniture (#886), 24 IDs each, in
    /// [`super::cottages::cottages`] order.
    pub const FARMHOUSES: RangeInclusive<u32> = 44_200..=44_399;
    /// The settlements' second props batch (#891), 4 IDs each, in
    /// [`super::village_props::SETTLEMENT_PROPS`] order.
    pub const SETTLEMENT_PROPS: RangeInclusive<u32> = 44_400..=44_799;
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
    wilds::REGION,
    settlements::REGION,
    crossings::REGION,
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

/// A turn's corner is rounded off across at most this much (4 m) of each leg (#816): the width
/// of the basin's climbs (#423), so a rounded switchback stays on its ledge and landing.
const TURN_RADIUS_UNITS: i64 = 400;

/// An authored centre line with its corners rounded (#816): each turn becomes a quarter-step
/// curve from [`TURN_RADIUS_UNITS`] (or half the shorter leg) before the corner to as far after
/// it, so a road bends instead of turning on the spot.
fn rounded_turns(points: &[[i32; 2]]) -> Vec<[i32; 2]> {
    let length = |a: [i32; 2], b: [i32; 2]| {
        let (dx, dz) = (i64::from(b[0] - a[0]), i64::from(b[1] - a[1]));
        i64::try_from((dx * dx + dz * dz).unsigned_abs().isqrt()).unwrap_or(0)
    };
    let toward = |from: [i32; 2], to: [i32; 2], distance: i64| {
        let span = length(from, to).max(1);
        [0, 1].map(|axis| {
            from[axis]
                + i32::try_from(i64::from(to[axis] - from[axis]) * distance / span).unwrap_or(0)
        })
    };
    let mut out = vec![points[0]];
    for window in points.windows(3) {
        let [before, corner, after] = [window[0], window[1], window[2]];
        let cross = i64::from(corner[0] - before[0]) * i64::from(after[1] - corner[1])
            - i64::from(corner[1] - before[1]) * i64::from(after[0] - corner[0]);
        if cross == 0 {
            // Straight on: no turn to round.
            out.push(corner);
            continue;
        }
        let reach = TURN_RADIUS_UNITS
            .min(length(before, corner) / 2)
            .min(length(corner, after) / 2);
        let (start, end) = (toward(corner, before, reach), toward(corner, after, reach));
        // A quadratic Bézier from `start` to `end` with the corner as its control point.
        for step in 0..=4_i64 {
            let point = [0, 1].map(|axis| {
                let (a, c, b) = (
                    i64::from(start[axis]),
                    i64::from(corner[axis]),
                    i64::from(end[axis]),
                );
                let t = step;
                let value = (a * (4 - t) * (4 - t) + 2 * c * t * (4 - t) + b * t * t) / 16;
                i32::try_from(value).unwrap_or(0)
            });
            if out.last() != Some(&point) {
                out.push(point);
            }
        }
    }
    let last = points[points.len() - 1];
    if out.last() != Some(&last) {
        out.push(last);
    }
    out
}

/// A road's name and centre line.
type NamedLine = (&'static str, Vec<[i32; 2]>);

/// The regions' roads and the landscape paths, with their turns rounded.
static AUTHORED_ROADS: LazyLock<Vec<NamedLine>> = LazyLock::new(|| {
    REGIONS
        .iter()
        .flat_map(|region| region.roads)
        .map(|road| (road.name, rounded_turns(road.points)))
        .chain(
            landscape::data()
                .paths
                .iter()
                .map(|path| (path.name.as_str(), rounded_turns(&path.points))),
        )
        .collect()
});

static ROADS: LazyLock<Vec<Road>> = LazyLock::new(|| {
    AUTHORED_ROADS
        .iter()
        .map(|(name, points)| Road {
            name,
            points: points.as_slice(),
        })
        .chain(design::data().roads.iter().map(|road| Road {
            name: road.name.as_str(),
            points: road.points.as_slice(),
        }))
        .collect()
});

/// Every road's centre line, bucketed.
static ALL_ROAD_LINES: LazyLock<design::LineIndex> =
    LazyLock::new(|| design::LineIndex::new(ROADS.iter().map(|road| road.points), 4_000, 1_000));

/// Units from `(x, z)` to the nearest road's centre line, as far as 10 m; `i64::MAX` beyond.
#[must_use]
pub fn road_distance(x: i32, z: i32) -> i64 {
    ALL_ROAD_LINES
        .near(x, z)
        .iter()
        .map(|&index| design::polyline_distance(ROADS[index].points, x, z).0)
        .min()
        .unwrap_or(i64::MAX)
}

/// Every region's roads, followed by the committed landscape paths and the region's designed
/// roads (#816), in authoring order.
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
    /// The zone's whole grid (#810): the authored grid above ([`ORIGIN`], [`COLUMNS`],
    /// [`ROWS`], which painted cells index) within whole physics ground tiles of 106 cells,
    /// out past the region's outline on every side.
    pub const ZONE_ORIGIN: [i32; 2] = [ORIGIN[0] - 4 * TILE, ORIGIN[1] - 2 * TILE];
    pub const ZONE_COLUMNS: u16 = 9 * 106 + 1;
    pub const ZONE_ROWS: u16 = 7 * 106 + 1;
    const TILE: i32 = 106 * STEP;
    /// Beyond the authored ground the region's base terrain takes over across this band.
    pub const BLEND_UNITS: i64 = 4_000;
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
        channel: None,
        name: "Stillwater Lake".to_owned(),
        centre: lake::CENTRE,
        radii: lake::RADII,
        surface: lake::SURFACE,
        air_pockets: Vec::new(),
    }
}

/// Greyhaven's fixed surface hazards (#294): a frozen pond north of the Outpost on flat ground,
/// and tar on the Redbrand camp's floor.
#[must_use]
pub fn surface_hazards() -> Vec<crate::SurfacePatch> {
    vec![
        crate::SurfacePatch {
            id: 1,
            kind: crate::SurfaceKind::Ice,
            centre: [-2_400, -2_600],
            radii: [620, 440],
            height: [-60, 120],
        },
        crate::SurfacePatch {
            id: 2,
            kind: crate::SurfaceKind::Tar,
            centre: [6_400, -15_700],
            radii: [420, 300],
            height: [1_000, 1_250],
        },
    ]
}

/// Stillwater Lake's basin (#274).
pub mod lake {
    pub const CENTRE: [i32; 2] = [-8_700, -5_300];
    /// A half-turn places the existing authored lake/shrine frame on the western shore.
    /// It is its own inverse. Offsets preserve the authoritative 4 m
    /// terrain grid, including the shrine tunnel's paired floor vertices.
    pub const fn place([x, z]: [i32; 2]) -> [i32; 2] {
        [-3200 - x, -10800 - z]
    }
    pub const fn place3([x, y, z]: [i32; 3]) -> [i32; 3] {
        let [x, z] = place([x, z]);
        [x, y, z]
    }
    pub const fn place_bounds((min, max): ([i32; 2], [i32; 2])) -> ([i32; 2], [i32; 2]) {
        (place(max), place(min))
    }
    pub fn place_mesh(mut mesh: crate::StaticMesh) -> crate::StaticMesh {
        mesh.position = place3(mesh.position);
        for vertex in &mut mesh.vertices {
            vertex[0] = -vertex[0];
            vertex[2] = -vertex[2];
        }
        mesh
    }
    pub const RADII: [i32; 2] = [2_200, 1_600];
    pub fn near_shore(x: i32, z: i32, margin: i32) -> bool {
        let a = i64::from(x - CENTRE[0]) * 1024 / i64::from(RADII[0] + margin);
        let b = i64::from(z - CENTRE[1]) * 1024 / i64::from(RADII[1] + margin);
        a * a + b * b <= 1024 * 1024
    }
    /// A rounded shore apron joins the relocated lake to the western rim.
    pub fn shore_ground(x: i32, z: i32, base: i32) -> i32 {
        let a = i64::from(x - CENTRE[0]) * 1024 / i64::from(RADII[0] + 400);
        let b = i64::from(z - CENTRE[1]) * 1024 / i64::from(RADII[1] + 400);
        let fade = ((a * a + b * b).isqrt() - 1024).clamp(0, 256);
        if fade == 256 || super::basin::lane_height(x, z).is_some() {
            return base;
        }
        SHORE_Y + ((i64::from(base - SHORE_Y) * fade) / 256) as i32
    }

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

/// The heart's ground wherever no cave flattens it: the basin's terraces and their ways down
/// (#423), less the lake's basin (#274). Round the heart the designed landscape takes over
/// (#816), so the old outer hills, ridges and escarpment no longer shape it.
fn open_ground(x: i32, z: i32) -> i32 {
    lake::shore_ground(x, z, basin::height_at(x, z).unwrap_or(0)) - lake_depth(x, z)
}

/// One sample of the authored grid (#159), at `column` and `row` from [`terrain::ORIGIN`].
fn authored_ground(column: i32, row: i32) -> i32 {
    let x = terrain::ORIGIN[0] + column * terrain::STEP;
    let z = terrain::ORIGIN[1] + row * terrain::STEP;
    // The cave floor stays flat inside its hillside.
    if cave::GREYFANG.flattens(x, z) {
        cave::GREYFANG.floor_y
    } else if shrine::flattens(x, z) {
        // The Sunken Shrine's plaza and cave floor (#274).
        shrine::FLOOR_Y
    } else {
        let base = open_ground(x, z);
        let base = cave::approach_ground(x, z, base);
        let base = base + landscape::brook_relief(x, z);
        let base = brook::upper_ground(x, z, base);
        base - if base < 100 {
            brook::bed_depth(x, z)
        } else {
            0
        } + paint::height_delta(column, row)
    }
}

/// A graded road lies level across this half width (3 m: the drawn road and a shoulder) …
const ROAD_FLAT_UNITS: i64 = 300;
/// … and meets the landscape again across this much more (10 m), so its banks stay gentle.
const ROAD_FADE_UNITS: i64 = 1_000;
/// Road profiles are sampled this often along a road.
const ROAD_SAMPLE_UNITS: i64 = 400;

/// The designed roads' centre lines, bucketed (#816).
static ROAD_LINES: LazyLock<design::LineIndex> = LazyLock::new(|| {
    design::LineIndex::new(
        design::data()
            .roads
            .iter()
            .map(|road| road.points.as_slice()),
        4_000,
        i32::try_from(ROAD_FLAT_UNITS + ROAD_FADE_UNITS).unwrap_or(0),
    )
});

/// The point `along` units along a polyline, clamped to its ends.
fn point_along(points: &[[i32; 2]], along: i64) -> [i32; 2] {
    let mut run = 0_i64;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let span = i64::try_from(
            (i64::from(b[0] - a[0]).pow(2) + i64::from(b[1] - a[1]).pow(2))
                .unsigned_abs()
                .isqrt(),
        )
        .unwrap_or(0);
        if run + span >= along && span > 0 {
            let t = along - run;
            let at = |from: i32, to: i32| {
                from + i32::try_from(i64::from(to - from) * t / span).unwrap_or(0)
            };
            return [at(a[0], b[0]), at(a[1], b[1])];
        }
        run += span;
    }
    points[points.len() - 1]
}

/// A road profile's height `along` units along its road.
fn profile_height(profile: &[i32], along: i64) -> i32 {
    let at = usize::try_from(along / ROAD_SAMPLE_UNITS)
        .unwrap_or(0)
        .min(profile.len() - 1);
    let next = (at + 1).min(profile.len() - 1);
    let fraction = along.max(0) % ROAD_SAMPLE_UNITS;
    let height = i64::from(profile[at])
        + i64::from(profile[next] - profile[at]) * fraction / ROAD_SAMPLE_UNITS;
    i32::try_from(height).unwrap_or(profile[at])
}

/// A branch eases onto the road it leaves across this many profile samples (24 m).
const ROAD_JOIN_SAMPLES: usize = 6;

/// Each designed road's height every [`ROAD_SAMPLE_UNITS`] along it: the ungraded ground,
/// smoothed over 12 m either way so the road rises and falls gently. A branch ends at the
/// height of the road it leaves, easing onto it, so no step lies where two roads meet.
static ROAD_PROFILES: LazyLock<Vec<Vec<i32>>> = LazyLock::new(|| {
    let roads = &design::data().roads;
    let smoothed: Vec<Vec<i32>> = roads
        .iter()
        .map(|road| {
            let (_, length) = design::polyline_distance(
                &road.points,
                road.points[road.points.len() - 1][0],
                road.points[road.points.len() - 1][1],
            );
            let mut heights: Vec<i32> = (0..=length / ROAD_SAMPLE_UNITS + 1)
                .map(|step| {
                    let [x, z] = point_along(&road.points, step * ROAD_SAMPLE_UNITS);
                    ungraded_ground(x, z)
                })
                .collect();
            for _ in 0..2 {
                heights = (0..heights.len())
                    .map(|index| {
                        let window =
                            &heights[index.saturating_sub(3)..(index + 4).min(heights.len())];
                        let sum: i64 = window.iter().map(|&height| i64::from(height)).sum();
                        i32::try_from(sum / i64::try_from(window.len()).unwrap_or(1)).unwrap_or(0)
                    })
                    .collect();
            }
            heights
        })
        .collect();
    // Roads earlier in the design are the main roads; a later road ending on one is its branch.
    let mut profiles = smoothed.clone();
    for (index, road) in roads.iter().enumerate() {
        let ends = [
            (road.points[0], true),
            (road.points[road.points.len() - 1], false),
        ];
        for (end, first) in ends {
            let joined = roads[..index].iter().enumerate().find_map(|(other, main)| {
                let (distance, along) = design::polyline_distance(&main.points, end[0], end[1]);
                (distance <= 50).then(|| profile_height(&profiles[other], along))
            });
            let Some(target) = joined else { continue };
            let last = profiles[index].len() - 1;
            let shift = i64::from(target - smoothed[index][if first { 0 } else { last }]);
            let samples = ROAD_JOIN_SAMPLES.min(last + 1);
            for step in 0..samples {
                let sample = if first { step } else { last - step };
                let weight = i64::try_from(samples - step).unwrap_or(0);
                let samples = i64::try_from(samples).unwrap_or(1);
                profiles[index][sample] += i32::try_from(shift * weight / samples).unwrap_or(0);
            }
        }
    }
    profiles
});

/// The zone's ground at a grid sample `(x, z)`: the ground of [`ungraded_ground`], with the
/// region's designed roads graded into it beyond the core (#816), except where a road fords a
/// river.
fn zone_ground(x: i32, z: i32) -> i32 {
    let ground = ungraded_ground(x, z);
    let outside = beyond_core(x, z);
    if outside <= 0 {
        return ground;
    }
    let design = design::data();
    let mut nearest: Option<(i64, i32)> = None;
    for &index in ROAD_LINES.near(x, z) {
        let (distance, along) = design::polyline_distance(&design.roads[index].points, x, z);
        if distance < ROAD_FLAT_UNITS + ROAD_FADE_UNITS
            && nearest.is_none_or(|(best, _)| distance < best)
        {
            nearest = Some((distance, profile_height(&ROAD_PROFILES[index], along)));
        }
    }
    let Some((distance, road)) = nearest else {
        return ground;
    };
    let along_road =
        1_024 - smooth_q10(distance, ROAD_FLAT_UNITS, ROAD_FLAT_UNITS + ROAD_FADE_UNITS);
    let past_core = smooth_q10(outside, 0, 1_200);
    let ford = 1_024 - smooth_q10(design::river_distance(x, z), 200, 800);
    let weight = along_road * past_core / 1_024 * (1_024 - ford) / 1_024;
    ground + i32::try_from(i64::from(road - ground) * weight / 1_024).unwrap_or(0)
}

/// The zone's ground at a grid sample `(x, z)` before its roads are graded (#816): the authored
/// ground on [`CORE`], the designed landscape from [`terrain::BLEND_UNITS`] beyond it, and
/// between them a smooth blend from the nearest authored sample.
fn ungraded_ground(x: i32, z: i32) -> i32 {
    let authored = |x: i32, z: i32| {
        authored_ground(
            (x - terrain::ORIGIN[0]) / terrain::STEP,
            (z - terrain::ORIGIN[1]) / terrain::STEP,
        )
    };
    let beyond = beyond_core(x, z);
    if beyond <= 0 {
        return authored(x, z);
    }
    let land = design::ground(x, z);
    if beyond >= terrain::BLEND_UNITS {
        return land;
    }
    let heart = authored(x, z);
    let weight = smooth_q10(beyond, 0, terrain::BLEND_UNITS);
    heart + i32::try_from(i64::from(land - heart) * weight / 1_024).unwrap_or(0)
}

/// The zone's terrain grid: the authored ground (#159) within the region's base terrain (#810).
#[must_use]
pub fn terrain() -> Terrain {
    let (columns, rows) = (
        i32::from(terrain::ZONE_COLUMNS),
        i32::from(terrain::ZONE_ROWS),
    );
    let heights = (0..rows)
        .flat_map(|row| {
            (0..columns).map(move |column| {
                zone_ground(
                    terrain::ZONE_ORIGIN[0] + column * terrain::STEP,
                    terrain::ZONE_ORIGIN[1] + row * terrain::STEP,
                )
            })
        })
        .collect();
    Terrain {
        origin: terrain::ZONE_ORIGIN,
        step: terrain::STEP,
        columns: terrain::ZONE_COLUMNS,
        rows: terrain::ZONE_ROWS,
        heights,
    }
}

static TERRAIN: Seeded<Terrain> = Seeded::new(terrain);

/// A value built on first use, unless a copy was seeded first ([`seed_base`]).
struct Seeded<T> {
    cell: OnceLock<T>,
    build: fn() -> T,
}

impl<T> Seeded<T> {
    const fn new(build: fn() -> T) -> Self {
        Self {
            cell: OnceLock::new(),
            build,
        }
    }
}

impl<T> std::ops::Deref for Seeded<T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.cell.get_or_init(self.build)
    }
}

/// The two costly parts of the base definition: the zone terrain's heights ([`terrain`]) and
/// the colliders before the static meshes' bounds join them, each built once.
#[must_use]
pub fn base_parts() -> (&'static [i32], &'static [StaticCollider]) {
    (&TERRAIN.heights, &COLLIDERS)
}

/// Seeds [`base_parts`] with a copy another instance of this same build made (the browser's
/// world-build worker, #855), so this one need not build them; the base definition is then
/// assembled and validated from them as from built parts. Returns false, changing nothing, once
/// either part was built or seeded, or when the parts do not form a valid definition.
pub fn seed_base(heights: Vec<i32>, colliders: Vec<StaticCollider>) -> bool {
    let terrain = Terrain {
        origin: terrain::ZONE_ORIGIN,
        step: terrain::STEP,
        columns: terrain::ZONE_COLUMNS,
        rows: terrain::ZONE_ROWS,
        heights,
    };
    if TERRAIN.cell.get().is_some()
        || COLLIDERS.cell.get().is_some()
        || terrain.validate().is_err()
        || ZoneDefinition::with_spawn_grid(GRAVITY, SPAWN_GRID, colliders.clone()).is_err()
    {
        return false;
    }
    TERRAIN.cell.set(terrain).is_ok() && COLLIDERS.cell.set(colliders).is_ok()
}

/// The zone's terrain over the authored grid alone ([`terrain::ORIGIN`], [`terrain::COLUMNS`],
/// [`terrain::ROWS`]), the window painted cells and the authored landforms index.
#[must_use]
pub fn authored_terrain() -> Terrain {
    let (columns, rows) = (i32::from(terrain::COLUMNS), i32::from(terrain::ROWS));
    let heights = (0..rows)
        .flat_map(|row| {
            (0..columns).map(move |column| {
                TERRAIN.height_at(
                    terrain::ORIGIN[0] + column * terrain::STEP,
                    terrain::ORIGIN[1] + row * terrain::STEP,
                )
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

/// Where a footprint `[min, max]` rests on the terrain: on level ground (the valley floor, the
/// farm terrace) at its height, on a slope at its lowest corner, sunk 0.3 m so nothing floats.
fn footing(min: [i32; 2], max: [i32; 2]) -> i32 {
    let corners = corners(min, max);
    let lowest = lowest_corner(min, max);
    if corners.iter().all(|&height| height == lowest) {
        lowest
    } else {
        lowest - 30
    }
}

/// The terrain's height at a footprint's corners.
fn corners(min: [i32; 2], max: [i32; 2]) -> [i32; 4] {
    [min, [max[0], min[1]], max, [min[0], max[1]]]
        .map(|corner| TERRAIN.height_at(corner[0], corner[1]))
}

/// The terrain's lowest corner under a footprint.
fn lowest_corner(min: [i32; 2], max: [i32; 2]) -> i32 {
    corners(min, max).into_iter().min().unwrap_or(0)
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

/// Small, patchy groves frame Stillwater's eastern shore, the Hollow's distant backdrop
/// and the southwest ridge: `(origin, cells, seed, percent of cells with a trunk)` on the same
/// 4.5 m jittered grid as Wolfrun Woods. Beyond the heart the region's designed forests grow
/// instead (#816, [`region_forest_trees`]).
const RIM_GROVES: [([i32; 2], [i32; 2], u64, u64); 5] = [
    ([8200, -10500], [9, 19], 0x0051_3f01, 72),
    ([-11000, -15500], [14, 9], 0x0051_3f02, 76),
    ([-11000, 5000], [14, 13], 0x0051_3f03, 70),
    // Continue the woodland behind the western ridge and below Millbrook. Existing
    // groves stay first so their tree identities survive this appended authoring.
    ([-12500, -10500], [13, 14], 0x0051_3f04, 76),
    ([6500, 5800], [13, 12], 0x0051_3f05, 74),
];

/// Whether a grove on the grid `origin`, `cells` grows at `(x, z)` (#816): inside the heart, in
/// a rounded clump inscribed in its rectangle whose edge the noise wobbles by about a fifth, so
/// no wood keeps its grid's straight sides.
fn in_grove(x: i32, z: i32, origin: [i32; 2], cells: [i32; 2], seed: u64) -> bool {
    let half = cells.map(|count| i64::from(count * woods::CELL / 2));
    let dx = (i64::from(x) - i64::from(origin[0]) - half[0]) * 1_024 / half[0];
    let dz = (i64::from(z) - i64::from(origin[1]) - half[1]) * 1_024 / half[1];
    let wobble =
        (value_noise_q10(i64::from(x), i64::from(z), 2_400, seed ^ 0x0043_4c55_4d50) - 512) * 2 / 5;
    beyond_core(x, z) <= 0 && (dx * dx + dz * dz).isqrt() <= 1_100 + wobble
}

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
            if dx * dx + dz * dz < keep_out * keep_out
                || !in_grove(x, z, woods::ORIGIN, woods::CELLS, woods::SEED)
            {
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
            if !brook::wet_ground(x, z) && !shrine::flattens(x, z) {
                trees.push(collider);
            }
            next_id += 1;
        }
    }
    for (origin, cells, seed, percent) in RIM_GROVES {
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
                // Gaps in the canopy are broad patches, not one evenly occupied lattice.
                // Leave the highest spurs bare so the ridges remain visible above the trees.
                if value_noise_q10(i64::from(x), i64::from(z), 1800, seed) < 440
                    || !(600..2300).contains(&TERRAIN.height_at(x, z))
                    || !in_grove(x, z, origin, cells, seed)
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
                if !brook::wet_ground(x, z) && !shrine::flattens(x, z) {
                    trees.push(collider);
                }
                next_id += 1;
            }
        }
    }
    // New canopy patches are authored separately so older forest identities remain stable.
    let posts: Vec<_> = units::creature_spawns()
        .into_iter()
        .map(|unit| unit.position)
        .chain(units::npcs().into_iter().map(|unit| unit.position))
        .collect();
    for forest in &landscape::data().forests {
        for row in 0..forest.cells[1] {
            for column in 0..forest.cells[0] {
                let cell = (u64::from(row.unsigned_abs()) << 32) | u64::from(column.unsigned_abs());
                let hash = splitmix64(forest.seed ^ cell);
                if hash % 100 >= forest.percent {
                    continue;
                }
                let jitter = |shift: u32| (hash >> shift) as i32 & 0xff;
                let x = forest.origin[0]
                    + column * woods::CELL
                    + woods::EDGE_MARGIN
                    + jitter(8) % (span + 1);
                let z = forest.origin[1]
                    + row * woods::CELL
                    + woods::EDGE_MARGIN
                    + jitter(16) % (span + 1);
                if value_noise_q10(i64::from(x), i64::from(z), 1800, forest.seed) < 300
                    || !(forest.elevation[0]..forest.elevation[1])
                        .contains(&TERRAIN.height_at(x, z))
                    || !in_grove(x, z, forest.origin, forest.cells, forest.seed)
                    || brook::wet_ground(x, z)
                    || shrine::flattens(x, z)
                    || posts
                        .iter()
                        .any(|post| (x - post[0]).abs() < 350 && (z - post[1]).abs() < 350)
                {
                    continue;
                }
                let half = 30 + jitter(24) % 16;
                if trees.iter().any(|tree| {
                    (x - tree.position[0]).abs() < half + tree.half_extents[0] + 150
                        && (z - tree.position[2]).abs() < half + tree.half_extents[2] + 150
                }) {
                    continue;
                }
                let base = footing([x - half, z - half], [x + half, z + half]);
                let collider = StaticCollider {
                    id: next_id,
                    position: [x, base + woods::TRUNK_HEIGHT / 2, z],
                    half_extents: [half, woods::TRUNK_HEIGHT / 2, half],
                };
                if !clear_of_roads(&collider) || near_structure(&collider) {
                    continue;
                }
                assert!(
                    ids::TREES.contains(&next_id),
                    "authored forests exceed reserved tree identities"
                );
                trees.push(collider);
                next_id += 1;
            }
        }
    }
    // The shore rocks and shrine moved with Stillwater. Reject intersecting trunks
    // after assigning IDs, so clearing their new sites never renumbers other trees.
    let shore: Vec<_> = (vale::REGION.colliders)()
        .into_iter()
        .filter(|body| ids::SHORE_ROCKS.contains(&body.id))
        .chain(shrine::ruins(*ids::SHRINE_RUINS.start()))
        .chain(shrine::walls(*ids::SHRINE_CAVE_WALLS.start()))
        .chain(
            fort::meshes()
                .into_iter()
                .filter(|mesh| {
                    (fort::REDBRAND_FIRE..=*fort::REDBRAND_SHELTERS.end()).contains(&mesh.id)
                })
                .map(|mesh| mesh.bounds().expect("valid Redbrand camp bounds")),
        )
        .collect();
    let [camp_min, camp_max] = landscape::data().redbrand_camp.clearing;
    trees.retain(|tree| {
        let in_camp = (camp_min[0]..=camp_max[0]).contains(&tree.position[0])
            && (camp_min[1]..=camp_max[1]).contains(&tree.position[2]);
        !in_camp
            && !shore.iter().any(|body| {
                (0..3).all(|axis| {
                    (i64::from(tree.position[axis]) - i64::from(body.position[axis])).abs()
                        < i64::from(tree.half_extents[axis]) + i64::from(body.half_extents[axis])
                })
            })
    });
    trees.extend(region_forest_trees());
    trees
}

/// The region's designed forests (#816): trunks on a jittered grid of each forest's spacing
/// inside its outline, thinned to its cover and opened into natural glades by noise, inside the
/// walls and clear of the core, the roads, the waters and the region's structures. They take
/// [`ids::REGION_TREES`] in forest order.
fn region_forest_trees() -> Vec<StaticCollider> {
    let design = design::data();
    let mut trees = Vec::new();
    let mut next_id = *ids::REGION_TREES.start();
    // Creature and NPC posts keep a wandering creature's reach (7 m) clear, and the crags 2 m.
    let posts: Vec<_> = units::creature_spawns()
        .into_iter()
        .map(|unit| unit.position)
        .chain(units::npcs().into_iter().map(|unit| unit.position))
        .collect();
    let near_crag = |x: i32, z: i32| {
        highlands::CRAGS.iter().any(|&(min, max, _)| {
            x > min[0] - 200 && x < max[0] + 200 && z > min[1] - 200 && z < max[1] + 200
        })
    };
    for forest in &design.forests {
        let [min, max] = forest.bounds();
        let spacing = forest.spacing;
        // Anywhere in its cell but its outer 0.6 m, so the trunks never line up in rows.
        let margin = 60;
        let span = (spacing - 2 * margin).max(1);
        for row in 0..=(max[1] - min[1]) / spacing {
            for column in 0..=(max[0] - min[0]) / spacing {
                let cell = (u64::from(row.unsigned_abs()) << 32) | u64::from(column.unsigned_abs());
                let hash = splitmix64(forest.seed ^ cell);
                let jitter = |shift: u32| (hash >> shift) as i32 & 0xffff;
                let x = min[0] + column * spacing + margin + jitter(8) % span;
                let z = min[1] + row * spacing + margin + jitter(24) % span;
                // Cover clumps: from 40 % to 160 % of the forest's over about 25 m.
                let clump = value_noise_q10(i64::from(x), i64::from(z), 2_500, forest.seed ^ 0xc1);
                let cover = i64::from(forest.cover) * (410 + clump * 1_230 / 1_024) / 1_024;
                if i64::try_from(hash % 100).unwrap_or(0) >= cover
                    || design::forest_at(x, z).map(|grows| grows.seed) != Some(forest.seed)
                    || beyond_core(x, z) <= 0
                    || [(0, 0), (-400, 0), (400, 0), (0, -400), (0, 400)]
                        .iter()
                        .any(|(dx, dz)| !inside_walls(x + dx, z + dz))
                    || design::river_distance(x, z) < 400
                    || design::in_lake(x, z, 600)
                    || marsh::WATERS
                        .iter()
                        .any(|water| water.distance_q10(x, z) < 1_200)
                    || design::on_pad(x, z, 300)
                    || near_crag(x, z)
                    || posts
                        .iter()
                        .any(|post| (x - post[0]).abs() < 700 && (z - post[1]).abs() < 700)
                {
                    continue;
                }
                let half = 30 + jitter(40) % 16;
                let base = footing([x - half, z - half], [x + half, z + half]);
                let collider = StaticCollider {
                    id: next_id,
                    position: [x, base + woods::TRUNK_HEIGHT / 2, z],
                    half_extents: [half, woods::TRUNK_HEIGHT / 2, half],
                };
                if !clear_of_roads(&collider) || near_structure(&collider) {
                    continue;
                }
                assert!(
                    ids::REGION_TREES.contains(&next_id),
                    "the region's forests exceed their reserved tree identities"
                );
                trees.push(collider);
                next_id += 1;
            }
        }
    }
    trees
}

/// The region's designed boulders (#816): one per kept cell of each rock field's spacing, inside
/// its outline, 1.2 m to 2.8 m across and half as tall, resting on the terrain; inside the walls
/// and clear of the core, the roads, the waters, the pads, the posts and every other collider.
/// They take [`ids::REGION_BOULDERS`] in field order.
fn region_boulders(existing: &[StaticCollider]) -> Vec<StaticCollider> {
    /// Footprints `(min, max)` bucketed on a 20 m grid.
    type Footprints = std::collections::BTreeMap<(i32, i32), Vec<([i32; 2], [i32; 2])>>;
    const BUCKET: i32 = 2_000;
    let mut buckets = Footprints::new();
    let block = |min: [i32; 2], max: [i32; 2], buckets: &mut Footprints| {
        for bx in (min[0] - 200).div_euclid(BUCKET)..=(max[0] + 200).div_euclid(BUCKET) {
            for bz in (min[1] - 200).div_euclid(BUCKET)..=(max[1] + 200).div_euclid(BUCKET) {
                buckets.entry((bx, bz)).or_default().push((min, max));
            }
        }
    };
    for collider in existing {
        if collider.id == ids::GROUND {
            continue;
        }
        let [x, _, z] = collider.position;
        let [hx, _, hz] = collider.half_extents;
        if beyond_core(x, z) > 0 {
            block([x - hx, z - hz], [x + hx, z + hz], &mut buckets);
        }
    }
    let posts: Vec<_> = units::creature_spawns()
        .into_iter()
        .map(|unit| unit.position)
        .chain(units::npcs().into_iter().map(|unit| unit.position))
        .collect();
    let mut boulders = Vec::new();
    let mut next_id = *ids::REGION_BOULDERS.start();
    for field in &design::data().rockfields {
        let [min, max] = {
            let mut bounds = [[i32::MAX; 2], [i32::MIN; 2]];
            for point in &field.polygon {
                for axis in 0..2 {
                    bounds[0][axis] = bounds[0][axis].min(point[axis]);
                    bounds[1][axis] = bounds[1][axis].max(point[axis]);
                }
            }
            bounds
        };
        let spacing = field.spacing;
        for row in 0..=(max[1] - min[1]) / spacing {
            for column in 0..=(max[0] - min[0]) / spacing {
                let cell = (u64::from(row.unsigned_abs()) << 32) | u64::from(column.unsigned_abs());
                let hash = splitmix64(field.seed ^ cell);
                let jitter = |shift: u32| (hash >> shift) as i32 & 0xffff;
                let x = min[0] + 150 + jitter(8) % (spacing - 300).max(1);
                let z = min[1] + 150 + jitter(24) % (spacing - 300).max(1);
                let half = 60 + jitter(40) % 81;
                let (lo, hi) = ([x - half, z - half], [x + half, z + half]);
                let near_other = buckets
                    .get(&(x.div_euclid(BUCKET), z.div_euclid(BUCKET)))
                    .is_some_and(|others| {
                        others.iter().any(|(min, max)| {
                            lo[0] < max[0] + 150
                                && hi[0] > min[0] - 150
                                && lo[1] < max[1] + 150
                                && hi[1] > min[1] - 150
                        })
                    });
                if hash % 100 >= u64::from(field.cover)
                    || !design::polygon_contains(&field.polygon, x, z)
                    || beyond_core(x, z) <= 0
                    || [(0, 0), (-500, 0), (500, 0), (0, -500), (0, 500)]
                        .iter()
                        .any(|(dx, dz)| !inside_walls(x + dx, z + dz))
                    || design::river_distance(x, z) < 600
                    || design::in_lake(x, z, 800)
                    || marsh::WATERS
                        .iter()
                        .any(|water| water.distance_q10(x, z) < 1_250)
                    || design::on_pad(x, z, 500)
                    || near_other
                    || posts
                        .iter()
                        .any(|post| (x - post[0]).abs() < 800 && (z - post[1]).abs() < 800)
                {
                    continue;
                }
                let base = footing(lo, hi);
                let collider = StaticCollider {
                    id: next_id,
                    position: [x, base + half / 2, z],
                    half_extents: [half, half / 2, half],
                };
                if !clear_of_roads(&collider) {
                    continue;
                }
                assert!(
                    ids::REGION_BOULDERS.contains(&next_id),
                    "the region's rock fields exceed their reserved identities"
                );
                block(lo, hi, &mut buckets);
                boulders.push(collider);
                next_id += 1;
            }
        }
    }
    boulders
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
    // A segment whose bounding box is a clearance or more from the rectangle on either axis is at least that far
    // from it: the exact test below never answers otherwise, so most segments skip its 128-bit arithmetic.
    let gap = |low: i32, high: i32, axis: usize| {
        let (lo, hi) = (i64::from(low.min(high)), i64::from(low.max(high)));
        (i64::from(min[axis]) - hi).max(lo - i64::from(max[axis]))
    };
    let reach = i64::from(ROAD_CLEARANCE_UNITS);
    if gap(a[0], b[0], 0) >= reach || gap(a[1], b[1], 1) >= reach {
        return true;
    }
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

static COLLIDERS: Seeded<Vec<StaticCollider>> = Seeded::new(colliders);

/// Ground, boundary walls, authored structures and generated trees.
fn colliders() -> Vec<StaticCollider> {
    let mut colliders = undressed_colliders();
    colliders.extend(dressing::pieces().iter().map(dressing::Piece::collider));
    if let Some(house) = colliders
        .iter()
        .find(|collider| collider.id == market::HOUSE)
        .cloned()
    {
        colliders.extend(market::colliders(&house, *ids::MARKET.start()));
    }
    colliders
}

/// Every collider but the solid dressing and the market square (#577), which keep clear of
/// these.
fn undressed_colliders() -> Vec<StaticCollider> {
    // The ground spans the rectangle round the walls' outer faces.
    let min = PLAYABLE_BOUNDS.min.map(|value| value - WALL_THICKNESS);
    let max = PLAYABLE_BOUNDS.max.map(|value| value + WALL_THICKNESS);
    let centre = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2];
    let half = [(max[0] - min[0]) / 2, (max[1] - min[1]) / 2];
    // The boundary walls are meshes along the region's outline ([`boundary_walls`]).
    let mut colliders = vec![StaticCollider {
        id: ids::GROUND,
        position: [centre[0], -GROUND_THICKNESS / 2, centre[1]],
        half_extents: [half[0], GROUND_THICKNESS / 2, half[1]],
    }];
    colliders.extend(REGIONS.iter().flat_map(|region| (region.colliders)()));
    colliders.extend(trees());
    let boulders = region_boulders(&colliders);
    colliders.extend(boulders);
    colliders.extend((*ids::LADDERS.start()..).zip(LADDER_TOWERS).map(ladder));
    // Rope placement reads the authored rock's bounds, not a removed box proxy.
    colliders.extend(
        cave::collision_meshes()
            .iter()
            .filter(|m| (400..=406).contains(&m.id))
            .map(|m| m.bounds().expect("committed Hollow rock bounds are valid")),
    );
    let ropes: Vec<_> = (*ids::ROPES.start()..)
        .zip(ROPES)
        .map(|(id, rope)| rope.collider(id, &colliders))
        .collect();
    colliders.extend(ropes);
    colliders.retain(|c| !(400..=406).contains(&c.id));
    colliders
}

/// A climbable rope (#621) hung from the top of one face of a host collider. Its collider is a
/// 0.3 m square column from the face out to the rope's axis, from the ground to the host's top,
/// so a climber holds on 0.3 m in front of the axis (the Ranger's `Rope_*` stand-off).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rope {
    /// The collider whose face the rope hangs down.
    pub host: u32,
    /// The face's outward normal on XZ: one of `[±1, 0]` and `[0, ±1]`.
    pub out: [i32; 2],
    /// Where along the face the rope hangs: its X on a ±Z face, its Z on a ±X face (units).
    pub along: i32,
}

/// A rope's axis stands this far in front of its face (0.3 m); its column is as wide.
pub const ROPE_STAND_OFF_UNITS: i32 = 30;

impl Rope {
    /// The point on `host`'s face the rope hangs over (XZ, units).
    #[must_use]
    pub const fn face(&self, host: &StaticCollider) -> [i32; 2] {
        if self.out[0] == 0 {
            [
                self.along,
                host.position[2] + self.out[1] * host.half_extents[2],
            ]
        } else {
            [
                host.position[0] + self.out[0] * host.half_extents[0],
                self.along,
            ]
        }
    }

    fn collider(&self, id: u32, colliders: &[StaticCollider]) -> StaticCollider {
        let host = colliders
            .iter()
            .find(|collider| collider.id == self.host)
            .expect("a rope hangs from an existing collider");
        let [x, z] = self.face(host);
        let half = ROPE_STAND_OFF_UNITS / 2;
        let centre = [x + self.out[0] * half, z + self.out[1] * half];
        let (low, high) = (
            [centre[0] - half, centre[1] - half],
            [centre[0] + half, centre[1] + half],
        );
        let base = footing(low, high);
        let top = host.position[1] + host.half_extents[1];
        let mut collider = standing(id, low, high, top - base);
        collider.position[1] += base;
        collider
    }
}

/// The climbable ropes (#621): down the Redbrand Hollow's eastern cliff into the camp, between
/// its lower cliffs; up the Eastwater watchtower's western face, a second way up beside its
/// ladder; and up the southern face of the sheer highland crag west of the trail's bend, the
/// only way up it.
pub const ROPES: [Rope; 3] = [
    Rope {
        host: 402,
        out: [-1, 0],
        along: -10_050,
    },
    Rope {
        host: ids::WATCHTOWER,
        out: [-1, 0],
        along: -6_000,
    },
    Rope {
        host: *ids::HIGHLAND_CRAGS.start() + SHEER_CRAGS[0] as u32,
        out: [0, 1],
        along: -13_400,
    },
];

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
/// shore rocks), timber (the palisades and fences), the ladders and the ropes (#621) climb;
/// buildings, towers,
/// standing stones and cave walls do not. Each kind keeps unclimbable variants: the Redbrand
/// Hollow's sheer back wall, two sheer highland crags and the stronghold's smooth-planked
/// northern palisade. On the terrain, the Vale basin's cliffs are rough rock except the sheer
/// cliff under Stillwater Lake, and each boundary ridge or escarpment has one hard climbable
/// route beside its pass while the rest of it is sheer.
#[must_use]
pub fn climbing() -> crate::ClimbSurfaces {
    use crate::Grip::{Ladder, Rock, Rope, Timber};
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
        .chain(range(65050..=65059, Timber))
        .chain(range(fort::BASIN_CRAGS, Rock))
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
        .chain(range(ids::ROPES, Rope))
        .collect();
    let terrain = vec![
        // The sheer cliff under Stillwater Lake, then the rest of the Vale basin's cliffs.
        (SHEER_LAKE_CLIFF, None),
        (VALE_BASIN, Some(Rock)),
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
/// Marked climbing routes up terrain faces (#490). The escarpment's, farmland's and wetlands'
/// ridges that held them gave way to the region's designed landscape, whose slopes are walked
/// (#816).
pub const CLIMBING_ROUTES: [XzBounds; 0] = [];

/// The mill hoist (#56): a 2 m square stone pillar against the windmill's south face that
/// rests with its top flush with the farm terrace, rises 8 m to the windmill's east gallery
/// at 1.5 m/s, rests 5 s and sinks back. Its box fills the shaft below its top, so it
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

/// Every ramp: the mill's, then the plank step (22°) from the Lakeshore Path's end up onto the
/// dock's deck (#577), against the deck's south side.
#[must_use]
pub fn ramps() -> Vec<crate::Ramp> {
    let dock = dressing::dock();
    let deck = dock.collider();
    let top = deck.position[1] + deck.half_extents[1];
    let bottom = dock.feet();
    let (dx, dz, rises) = match dock.quarters % 4 {
        0 => (-400, 140, crate::RampRise::NegZ),
        1 => (140, 400, crate::RampRise::NegX),
        2 => (400, -140, crate::RampRise::PosZ),
        _ => (-140, -400, crate::RampRise::PosX),
    };
    let half_extents = if dock.quarters.is_multiple_of(2) {
        [100, (top - bottom) / 2, 50]
    } else {
        [50, (top - bottom) / 2, 100]
    };
    let mut ramps = MILL_RAMPS.to_vec();
    ramps.push(crate::Ramp {
        body: ids::DOCK_RAMP,
        position: [dock.at[0] + dx, (top + bottom) / 2, dock.at[1] + dz],
        half_extents,
        rises,
    });
    ramps.extend(brook::ramps());
    // The footbridges' end ramps (#828).
    ramps.extend(crossings::ramps());
    ramps
}

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
        |id: u32, body: u32, x: i32, rubble: Option<crate::PieceBox>| crate::StructurePiece {
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

/// Whether `mesh` is a piece of the invisible boundary wall (#810), which clients never draw.
#[must_use]
pub fn is_boundary_wall(mesh: &crate::StaticMesh) -> bool {
    ids::BOUNDARY_WALLS.contains(&mesh.id)
}

/// The boundary wall along the region's outline (#810): invisible upright faces from
/// [`WALL_HEIGHT`] below the ground to as far above it, solid from both sides, in pieces of
/// [`WALL_SPANS_PER_PIECE`] spans so each piece's bounds stay near the outline.
fn boundary_walls() -> Vec<crate::StaticMesh> {
    let outline = &region::data().outline;
    // The wall's columns all round the outline, at most [`WALL_COLUMN_SPACING`] apart …
    let mut all = Vec::new();
    for index in 0..outline.len() {
        let (a, b) = (outline[index], outline[(index + 1) % outline.len()]);
        let length = i64::from(b[0] - a[0])
            .abs()
            .max(i64::from(b[1] - a[1]).abs());
        let steps = ((length + WALL_COLUMN_SPACING - 1) / WALL_COLUMN_SPACING).max(1);
        for step in 0..steps {
            let along = |from: i32, to: i32| {
                from + i32::try_from(i64::from(to - from) * step / steps).unwrap_or(0)
            };
            all.push([along(a[0], b[0]), along(a[1], b[1])]);
        }
    }
    all.push(outline[0]);
    // … in pieces of [`WALL_SPANS_PER_PIECE`] spans, so each piece's bounds hug the wall.
    let mut walls = Vec::new();
    for (id, start) in ids::BOUNDARY_WALLS.zip((0..all.len() - 1).step_by(WALL_SPANS_PER_PIECE)) {
        let columns = &all[start..=(start + WALL_SPANS_PER_PIECE).min(all.len() - 1)];
        let anchor = columns[0];
        let mut vertices = Vec::with_capacity(columns.len() * 2);
        for point in columns {
            let ground = TERRAIN.height_at(point[0], point[1]);
            let local = [point[0] - anchor[0], point[1] - anchor[1]];
            vertices.push([local[0], ground - WALL_HEIGHT, local[1]]);
            vertices.push([local[0], ground + WALL_HEIGHT, local[1]]);
        }
        let mut triangles = Vec::with_capacity(columns.len() * 4);
        for column in 0..u32::try_from(columns.len() - 1).unwrap_or(0) {
            let [low, high, next_low, next_high] =
                [2 * column, 2 * column + 1, 2 * column + 2, 2 * column + 3];
            // Both windings, so the face is solid from either side.
            triangles.extend([
                [low, high, next_high],
                [low, next_high, next_low],
                [low, next_high, high],
                [low, next_low, next_high],
            ]);
        }
        walls.push(crate::StaticMesh {
            id,
            position: [anchor[0], 0, anchor[1]],
            vertices,
            triangles,
        });
    }
    assert!(
        walls.len() * WALL_SPANS_PER_PIECE >= all.len() - 1,
        "the boundary wall needs more identities than it reserves"
    );
    walls
}

/// The hosted collision content. Hosts, clients and scenery consume this
/// exact definition; the spawn grid is validated clear of every collider.
#[must_use]
pub fn base_definition() -> ZoneDefinition {
    BASE_DEFINITION.clone()
}

static BASE_DEFINITION: LazyLock<ZoneDefinition> = LazyLock::new(build_base_definition);
static VALE_DEFINITION: LazyLock<ZoneDefinition> =
    LazyLock::new(|| layout::compile(layout::SOURCE).expect("committed Outpost layout is valid"));

fn build_base_definition() -> ZoneDefinition {
    ZoneDefinition::with_spawn_grid(GRAVITY, SPAWN_GRID, COLLIDERS.clone())
        .and_then(|definition| definition.with_terrain(TERRAIN.clone()))
        .and_then(|definition| {
            definition.with_static_meshes(
                cave::collision_meshes()
                    .into_iter()
                    .chain(fort::meshes())
                    .chain(shrine::collision_meshes())
                    .chain(roofs::meshes())
                    .chain(boundary_walls())
                    .collect(),
            )
        })
        .expect("built-in Greyhaven Vale content is valid")
}

pub fn greyhaven_vale_definition() -> ZoneDefinition {
    VALE_DEFINITION.clone()
}

static CONTENT: LazyLock<Arc<ZoneContent>> = LazyLock::new(|| {
    content_with_definition(greyhaven_vale_definition())
        .expect("built-in Greyhaven content is valid")
});

/// Shared final validation for authoring previews and every runtime host.
pub fn content_with_definition(
    definition: ZoneDefinition,
) -> Result<Arc<ZoneContent>, crate::ZoneError> {
    content_with_spawns(definition, units::creature_spawns())
}

/// The hosted content with `extra` creature spawns beside Greyhaven's own, validated the same
/// way: diagnostic workloads such as the A/B chain of powder barrels (#666).
pub fn content_with_extra_spawns(
    extra: impl IntoIterator<Item = crate::CreatureSpawn>,
) -> Result<Arc<ZoneContent>, crate::ZoneError> {
    let mut spawns = units::creature_spawns();
    spawns.extend(extra);
    content_with_spawns(greyhaven_vale_definition(), spawns)
}

fn content_with_spawns(
    definition: ZoneDefinition,
    spawns: Vec<crate::CreatureSpawn>,
) -> Result<Arc<ZoneContent>, crate::ZoneError> {
    let content = ZoneContent::new(
        definition,
        areas().clone(),
        units::creature_templates(),
        spawns,
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
            .with_timed_combat(crate::TimedCombat::ARENA)?
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
            .with_ramps(ramps())?
            .with_rocks(dressing::rock_shapes())?
            .with_structures(structure_sites(), watch_post())?
            .with_seats(units::seats())?
            .with_routines(units::routines(), units::closed_at_night())?
            .with_private_homes(vec![marshal_house::home()])?
            .with_waters(
                [stillwater_lake(), shrine::cave_water()]
                    .into_iter()
                    .chain(marsh::water_volumes())
                    .chain(brook::waters())
                    .chain(design::water_volumes())
                    .collect(),
            )?
            .with_surfaces(surface_hazards())?
            .with_traps(traps::traps())?
            .with_climbing(climbing())?
            .with_raid(raid::millbrook_barn())?
            .with_waystones(vec![
                crate::GREYHAVEN_OUTPOST,
                crate::BARROWDOWN,
                crate::FENFORD,
                crate::WESTMERE_LODGE,
            ])?
            .with_depths_threshold(cave::depths_threshold()),
    ))
}

/// The complete hosted content: collision definition, areas, creatures,
/// NPCs and graveyard, validated once and shared.
#[must_use]
pub fn content() -> Arc<ZoneContent> {
    Arc::clone(&CONTENT)
}
