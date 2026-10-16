#![forbid(unsafe_code)]
//! Presentation-only scenery for shared zone content: props, roads, water,
//! terrain relief and a coloured terrain grid.
//!
//! # Why a separate crate
//!
//! Hosts must not depend on presentation data. `mmorpg-core` owns physical
//! truth (colliders, spawn plaza, road corridors, named areas) and every host
//! links it; this crate derives visuals *from* that content for clients only.
//! It depends on `mmorpg-core`, never the other way round, and neither
//! `mmorpg-game-server` nor anything it depends on links it
//! (`mmorpg-game-server/tests/dependency_boundary.rs`). It has two independent
//! consumers: the native client and, through WASM, the browser (#21).
//!
//! # Presentation-only relief
//!
//! Walkable ground is physically flat at y = 0. [`Scenery::height_at`] adds at
//! most ±0.6 m of relief inside the playable square, flattened to exactly 0
//! under and around structures, roads, the spawn plaza and water, and rises to
//! mountains of up to 40 m beyond the boundary walls. Beyond the ±200 m
//! terrain grid the same function keeps rising into distant ranges of up to
//! 140 m, which clients draw as a coarse far ring. Clients render a unit at
//! its physics position plus the relief height at its XZ; the offset never
//! feeds back into gameplay.
//! The Outpost uses a saved authored heightfield; surrounding relief retains
//! the original analytic function. Both clients sample the same scenery owner.
//!
//! Everything uses integer arithmetic, fixed seeds and saved inputs, so every
//! platform builds identical scenery ([`Scenery::stable_hash`]).

mod geometry;
mod outpost_grass;
mod outpost_relief;

use std::collections::BTreeMap;

pub use geometry::Rect;
use geometry::{Ellipse, Rng, polyline_distance, splitmix64, value_noise};
use mmorpg_core::greyhaven_vale::{self, PLAYABLE_BOUNDS, SPAWN_PLAZA, ids};
use mmorpg_core::{Area, AreaId, StaticCollider, XzBounds, greyhaven_vale_definition, trig};
pub use outpost_grass::outpost_grass_placements;

/// Half the side of the square the terrain grid covers (±200 m).
pub const TERRAIN_EXTENT_UNITS: i32 = 20_000;
/// Half the side of the square the far terrain ring covers (±600 m).
pub const FAR_TERRAIN_EXTENT_UNITS: i32 = 60_000;
/// Largest relief magnitude inside the playable square (0.6 m).
pub const WALKABLE_RELIEF_UNITS: i32 = 60;
/// Highest mountain relief beyond the boundary walls, within the terrain
/// grid (40 m).
pub const MOUNTAIN_PEAK_UNITS: i32 = 4_000;
/// Snow caps start here (50 m): above every mountain within the terrain grid,
/// so only the distant ranges beyond it carry snow and read as far peaks.
pub const SNOW_LINE_UNITS: i32 = 5_000;
/// Highest relief of the distant ranges beyond the terrain grid (140 m).
pub const FAR_PEAK_UNITS: i32 = 14_000;
/// The distant ranges rise from nothing at the terrain grid's edge to full
/// height over this distance (120 m).
const FAR_RISE_UNITS: i64 = 12_000;
/// Visual half width of every road (3 m wide); core keeps 2 m clear.
pub const ROAD_HALF_WIDTH_UNITS: i32 = 150;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum TreeVariant {
    Oak,
    Pine,
    Birch,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum RockSize {
    Small,
    Medium,
    Large,
}

/// What a prop looks like. Clients choose a model (or a blockout) per kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PropKind {
    Keep,
    Inn,
    House,
    Smithy,
    Barn,
    Farmhouse,
    Windmill,
    Well,
    PalisadeSegment,
    GatePost,
    Waystone,
    Gravestone,
    Tree(TreeVariant),
    Bush,
    Rock(RockSize),
    Cliff,
    GrassTuft,
    Flowers,
    Reeds,
    Fence,
    Tent,
    Campfire,
    Crate,
    Barrel,
    Cart,
    Signpost,
    Lamp,
    MineEntrance,
    CropRow,
    Dock,
}

impl PropKind {
    /// Every kind with its variants flattened, in declaration order.
    pub const ALL: [Self; 34] = [
        Self::Keep,
        Self::Inn,
        Self::House,
        Self::Smithy,
        Self::Barn,
        Self::Farmhouse,
        Self::Windmill,
        Self::Well,
        Self::PalisadeSegment,
        Self::GatePost,
        Self::Waystone,
        Self::Gravestone,
        Self::Tree(TreeVariant::Oak),
        Self::Tree(TreeVariant::Pine),
        Self::Tree(TreeVariant::Birch),
        Self::Bush,
        Self::Rock(RockSize::Small),
        Self::Rock(RockSize::Medium),
        Self::Rock(RockSize::Large),
        Self::Cliff,
        Self::GrassTuft,
        Self::Flowers,
        Self::Reeds,
        Self::Fence,
        Self::Tent,
        Self::Campfire,
        Self::Crate,
        Self::Barrel,
        Self::Cart,
        Self::Signpost,
        Self::Lamp,
        Self::MineEntrance,
        Self::CropRow,
        Self::Dock,
    ];

    /// Stable kebab-case name with the variant, independent of `Debug`
    /// output; clients key their models by it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Inn => "inn",
            Self::House => "house",
            Self::Smithy => "smithy",
            Self::Barn => "barn",
            Self::Farmhouse => "farmhouse",
            Self::Windmill => "windmill",
            Self::Well => "well",
            Self::PalisadeSegment => "palisade",
            Self::GatePost => "gate-post",
            Self::Waystone => "waystone",
            Self::Gravestone => "gravestone",
            Self::Tree(TreeVariant::Oak) => "tree-oak",
            Self::Tree(TreeVariant::Pine) => "tree-pine",
            Self::Tree(TreeVariant::Birch) => "tree-birch",
            Self::Bush => "bush",
            Self::Rock(RockSize::Small) => "rock-small",
            Self::Rock(RockSize::Medium) => "rock-medium",
            Self::Rock(RockSize::Large) => "rock-large",
            Self::Cliff => "cliff",
            Self::GrassTuft => "grass-tuft",
            Self::Flowers => "flowers",
            Self::Reeds => "reeds",
            Self::Fence => "fence",
            Self::Tent => "tent",
            Self::Campfire => "campfire",
            Self::Crate => "crate",
            Self::Barrel => "barrel",
            Self::Cart => "cart",
            Self::Signpost => "signpost",
            Self::Lamp => "lamp",
            Self::MineEntrance => "mine-entrance",
            Self::CropRow => "crop-row",
            Self::Dock => "dock",
        }
    }
}

/// One placed prop. `half_extents` describe its ground-level body box (a
/// tree's trunk, a building's walls) before yaw; clients add kind-specific
/// parts such as canopies and roofs above it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prop {
    pub kind: PropKind,
    /// Feet position in units, on y = 0 before relief.
    pub position: [i32; 3],
    /// Rotation about +Y, 65 536 steps per turn, 0 keeping local +Z on +Z.
    pub yaw: u16,
    /// Size relative to the kind's base size, in thousandths.
    pub scale_permille: u16,
    /// The core collider this prop visualises, if any.
    pub collider: Option<u32>,
    pub half_extents: [i32; 3],
}

impl Prop {
    /// Axis-aligned XZ bounds of the rotated body footprint.
    #[must_use]
    pub fn footprint(&self) -> Rect {
        let sin = i64::from(trig::sin(self.yaw).abs());
        let cos = i64::from(trig::cos(self.yaw).abs());
        let [hx, _, hz] = self.half_extents.map(i64::from);
        let one = i64::from(trig::TRIG_ONE);
        let round_up = |value: i64| i32::try_from((value + one - 1) / one).unwrap_or(i32::MAX);
        Rect::around(
            [self.position[0], self.position[2]],
            [round_up(cos * hx + sin * hz), round_up(sin * hx + cos * hz)],
        )
    }
}

/// What the ground at a terrain vertex is. Clients without per-vertex
/// colours draw one surface per biome in its [`Biome::base_color`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Biome {
    Meadow,
    Hub,
    Woods,
    Hollow,
    Farmland,
    Road,
    Plaza,
    Shore,
    LakeBed,
    Foothills,
    Highland,
    Rock,
    Snow,
}

impl Biome {
    /// Every biome in declaration order, which is also [`Biome::id`] order.
    pub const ALL: [Self; 13] = [
        Self::Meadow,
        Self::Hub,
        Self::Woods,
        Self::Hollow,
        Self::Farmland,
        Self::Road,
        Self::Plaza,
        Self::Shore,
        Self::LakeBed,
        Self::Foothills,
        Self::Highland,
        Self::Rock,
        Self::Snow,
    ];

    /// Stable numeric identity, independent of `Debug` output.
    #[must_use]
    pub const fn id(self) -> u8 {
        match self {
            Self::Meadow => 0,
            Self::Hub => 1,
            Self::Woods => 2,
            Self::Hollow => 3,
            Self::Farmland => 4,
            Self::Road => 5,
            Self::Plaza => 6,
            Self::Shore => 7,
            Self::LakeBed => 8,
            Self::Foothills => 9,
            Self::Highland => 10,
            Self::Rock => 11,
            Self::Snow => 12,
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Meadow => "meadow",
            Self::Hub => "hub",
            Self::Woods => "woods",
            Self::Hollow => "hollow",
            Self::Farmland => "farmland",
            Self::Road => "road",
            Self::Plaza => "plaza",
            Self::Shore => "shore",
            Self::LakeBed => "lake bed",
            Self::Foothills => "foothills",
            Self::Highland => "highland",
            Self::Rock => "rock",
            Self::Snow => "snow",
        }
    }

    /// sRGB colour before the per-vertex tint.
    #[must_use]
    pub const fn base_color(self) -> [u8; 3] {
        match self {
            Self::Meadow => [92, 132, 62],
            Self::Hub => [104, 128, 68],
            Self::Woods => [62, 80, 44],
            Self::Hollow => [122, 104, 84],
            Self::Farmland => [112, 84, 54],
            Self::Road => ROAD_COLOR,
            Self::Plaza => PLAZA_COLOR,
            Self::Shore => [196, 180, 132],
            Self::LakeBed => LAKE_BED_COLOR,
            Self::Foothills => [72, 104, 56],
            Self::Highland => [88, 98, 70],
            Self::Rock => [124, 120, 112],
            Self::Snow => SNOW_COLOR,
        }
    }

    /// Roads, the plaza, the lake bed and snow are uniform so their edges
    /// read clearly; every other biome takes a seeded per-vertex tint.
    const fn tinted(self) -> bool {
        !matches!(self, Self::Road | Self::Plaza | Self::LakeBed | Self::Snow)
    }
}

/// A road surface along a core road centre line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Road {
    pub name: &'static str,
    pub points: Vec<[i32; 2]>,
    pub half_width: i32,
}

/// Walkable shallow water: an axis-aligned ellipse with a visual surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Water {
    pub name: &'static str,
    pub centre: [i32; 2],
    pub radii: [i32; 2],
    /// Surface height in units above the physically flat ground.
    pub surface: i32,
}

impl Water {
    #[must_use]
    pub fn contains(&self, x: i32, z: i32) -> bool {
        self.ellipse().contains([x, z])
    }

    const fn ellipse(&self) -> Ellipse {
        Ellipse {
            centre: self.centre,
            radii: self.radii,
        }
    }
}

/// Relief heights, biomes and biome colours sampled on a square vertex grid,
/// row by row along +Z, each row along +X.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TerrainGrid {
    /// XZ position of vertex (column 0, row 0).
    pub origin: [i32; 2],
    pub step: i32,
    pub columns: usize,
    pub rows: usize,
    pub heights: Vec<i32>,
    pub biomes: Vec<Biome>,
    /// sRGB vertex colours: the biome's base colour, tinted per vertex.
    pub colors: Vec<[u8; 3]>,
}

impl TerrainGrid {
    #[must_use]
    pub fn vertex(&self, column: usize, row: usize) -> Option<([i32; 3], [u8; 3])> {
        if column >= self.columns || row >= self.rows {
            return None;
        }
        let index = row * self.columns + column;
        let offset = |count: usize| i32::try_from(count).ok()?.checked_mul(self.step);
        Some((
            [
                self.origin[0] + offset(column)?,
                *self.heights.get(index)?,
                self.origin[1] + offset(row)?,
            ],
            *self.colors.get(index)?,
        ))
    }
}

/// Deterministic presentation scenery for one content revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scenery {
    pub content_revision: u64,
    pub props: Vec<Prop>,
    pub roads: Vec<Road>,
    pub water: Vec<Water>,
    farmland: Vec<Rect>,
    relief: Relief,
    authored_outpost_relief: bool,
}

/// The visual of a Greyhaven Vale collider, or `None` for the ground and
/// boundary walls (and for IDs the vale does not use).
#[must_use]
pub fn visual_kind(collider: &StaticCollider) -> Option<PropKind> {
    let id = collider.id;
    let kind = match id {
        ids::KEEP => PropKind::Keep,
        ids::INN => PropKind::Inn,
        ids::SMITHY => PropKind::Smithy,
        ids::WELL => PropKind::Well,
        ids::WAYSTONE => PropKind::Waystone,
        ids::BARN => PropKind::Barn,
        ids::FARMHOUSE => PropKind::Farmhouse,
        ids::WINDMILL => PropKind::Windmill,
        ids::MINE_ENTRANCE => PropKind::MineEntrance,
        ids::CAMPFIRE => PropKind::Campfire,
        _ if ids::HOUSES.contains(&id) => PropKind::House,
        _ if ids::PALISADE.contains(&id) => PropKind::PalisadeSegment,
        _ if ids::GATE_POSTS.contains(&id) => PropKind::GatePost,
        _ if ids::GRAVESTONES.contains(&id) => PropKind::Gravestone,
        _ if ids::FENCES.contains(&id) => PropKind::Fence,
        _ if ids::CLIFFS.contains(&id) => PropKind::Cliff,
        _ if ids::TENTS.contains(&id) => PropKind::Tent,
        _ if ids::CRATES.contains(&id) => PropKind::Crate,
        _ if ids::SHORE_ROCKS.contains(&id) => {
            let largest = collider.half_extents[0].max(collider.half_extents[2]);
            PropKind::Rock(match largest {
                100.. => RockSize::Large,
                75..=99 => RockSize::Medium,
                _ => RockSize::Small,
            })
        }
        _ if ids::TREES.contains(&id) => PropKind::Tree(tree_variant(splitmix64(u64::from(id)))),
        _ => return None,
    };
    Some(kind)
}

/// Stable two-byte code of a kind for hashing; independent of `Debug` output.
const fn kind_code(kind: PropKind) -> [u8; 2] {
    match kind {
        PropKind::Keep => [1, 0],
        PropKind::Inn => [2, 0],
        PropKind::House => [3, 0],
        PropKind::Smithy => [4, 0],
        PropKind::Barn => [5, 0],
        PropKind::Farmhouse => [6, 0],
        PropKind::Windmill => [7, 0],
        PropKind::Well => [8, 0],
        PropKind::PalisadeSegment => [9, 0],
        PropKind::GatePost => [10, 0],
        PropKind::Waystone => [11, 0],
        PropKind::Gravestone => [12, 0],
        PropKind::Tree(TreeVariant::Oak) => [13, 1],
        PropKind::Tree(TreeVariant::Pine) => [13, 2],
        PropKind::Tree(TreeVariant::Birch) => [13, 3],
        PropKind::Bush => [14, 0],
        PropKind::Rock(RockSize::Small) => [15, 1],
        PropKind::Rock(RockSize::Medium) => [15, 2],
        PropKind::Rock(RockSize::Large) => [15, 3],
        PropKind::Cliff => [16, 0],
        PropKind::GrassTuft => [17, 0],
        PropKind::Flowers => [18, 0],
        PropKind::Reeds => [19, 0],
        PropKind::Fence => [20, 0],
        PropKind::Tent => [21, 0],
        PropKind::Campfire => [22, 0],
        PropKind::Crate => [23, 0],
        PropKind::Barrel => [24, 0],
        PropKind::Cart => [25, 0],
        PropKind::Signpost => [26, 0],
        PropKind::Lamp => [27, 0],
        PropKind::MineEntrance => [28, 0],
        PropKind::CropRow => [29, 0],
        PropKind::Dock => [30, 0],
    }
}

/// An area as a half-open rectangle; core area bounds are inclusive.
fn area_bounds(id: AreaId) -> XzBounds {
    greyhaven_vale::areas()
        .areas()
        .iter()
        .find(|area| area.id() == id)
        .map_or(PLAYABLE_BOUNDS, |area| XzBounds {
            min: area.min_xz(),
            max: area.max_xz().map(|value| value + 1),
        })
}

fn tree_variant(hash: u64) -> TreeVariant {
    match hash % 10 {
        0..=4 => TreeVariant::Oak,
        5..=7 => TreeVariant::Pine,
        _ => TreeVariant::Birch,
    }
}

const SEED_ROLLING: u64 = 0x4752_4559_4841_5645;
const SEED_DETAIL: u64 = 0x5641_4c45_4445_5441;
const SEED_RIDGE: u64 = 0x5249_4447_4553_2121;
const SEED_TINT: u64 = 0x5449_4e54_5449_4e54;
const SEED_PROPS: u64 = 0x5052_4f50_5345_4544;
const SEED_MASSIF: u64 = 0x4d41_5353_4946_2121;
const SEED_CRAGS: u64 = 0x4352_4147_5321_2121;
const LAKE: Water = Water {
    name: "Stillwater Lake",
    centre: [5_500, -5_500],
    radii: [2_200, 1_600],
    surface: 15,
};
/// Ploughed field inside the Millbrook fences.
const FARMLAND: Rect = Rect {
    min: [5_010, 5_610],
    max: [9_590, 8_390],
};

/// Builds the Greyhaven Vale scenery from the core content revision.
#[must_use]
pub fn greyhaven_vale_scenery() -> Scenery {
    let mut scenery = unmasked_greyhaven_vale_scenery();
    outpost_grass::apply(&mut scenery.props);
    // Apply after procedural placement, preserving every accepted prop/RNG draw.
    scenery.authored_outpost_relief = true;
    scenery
}

fn unmasked_greyhaven_vale_scenery() -> Scenery {
    let definition = greyhaven_vale_definition();
    let structures: Vec<_> = definition
        .colliders()
        .iter()
        .filter_map(|collider| visual_kind(collider).map(|kind| (collider, kind)))
        .collect();
    let roads = greyhaven_vale::roads()
        .iter()
        .map(|road| Road {
            name: road.name,
            points: road.points.to_vec(),
            half_width: ROAD_HALF_WIDTH_UNITS,
        })
        .collect::<Vec<_>>();
    let water = vec![LAKE];
    let relief = Relief::new(
        structures.iter().map(|(collider, kind)| (*collider, *kind)),
        &roads,
        &water,
    );
    let mut props: Vec<Prop> = structures
        .iter()
        .map(|(collider, kind)| Prop {
            kind: *kind,
            position: [
                collider.position[0],
                collider.position[1] - collider.half_extents[1],
                collider.position[2],
            ],
            yaw: 0,
            scale_permille: 1_000,
            collider: Some(collider.id),
            half_extents: collider.half_extents,
        })
        .collect();
    let mut scenery = Scenery {
        content_revision: definition.revision(),
        props: Vec::new(),
        roads,
        water,
        farmland: vec![FARMLAND],
        relief,
        authored_outpost_relief: false,
    };
    let blockers = Blockers::new(structures.iter().map(|(collider, _)| *collider));
    let mut placer = Placer {
        scenery: &scenery,
        blockers: &blockers,
        props: &mut props,
    };
    placer.fixed_props();
    placer.decorations();
    scenery.props = props;
    scenery
}

impl Scenery {
    /// Presentation relief at an XZ position, in units above the physically
    /// flat ground (y = 0).
    #[must_use]
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        if self.authored_outpost_relief
            && let Some(height) = outpost_relief::height_at(x, z)
        {
            return height;
        }
        self.relief.height_at(x, z)
    }

    /// Heights and biome colours over ±[`TERRAIN_EXTENT_UNITS`] every
    /// `step_units` (at least one unit).
    #[must_use]
    pub fn terrain_grid(&self, step_units: i32) -> TerrainGrid {
        self.grid_over(TERRAIN_EXTENT_UNITS, step_units)
    }

    /// The same relief and biomes over ±[`FAR_TERRAIN_EXTENT_UNITS`], for a
    /// coarse far ring around the terrain grid. Inside ±[`TERRAIN_EXTENT_UNITS`]
    /// its samples equal [`Scenery::terrain_grid`]'s at the same positions.
    #[must_use]
    pub fn far_terrain_grid(&self, step_units: i32) -> TerrainGrid {
        self.grid_over(FAR_TERRAIN_EXTENT_UNITS, step_units)
    }

    fn grid_over(&self, extent: i32, step_units: i32) -> TerrainGrid {
        let step = step_units.max(1);
        let count = usize::try_from(2 * extent / step + 1).unwrap_or(1);
        let origin = [-extent, -extent];
        let mut heights = Vec::with_capacity(count * count);
        let mut biomes = Vec::with_capacity(count * count);
        let mut colors = Vec::with_capacity(count * count);
        let mut z = origin[1];
        for _ in 0..count {
            let mut x = origin[0];
            for _ in 0..count {
                let height = self.height_at(x, z);
                let biome = self.biome_at(x, z, height);
                heights.push(height);
                biomes.push(biome);
                colors.push(biome_color(biome, x, z));
                x += step;
            }
            z += step;
        }
        TerrainGrid {
            origin,
            step,
            columns: count,
            rows: count,
            heights,
            biomes,
            colors,
        }
    }

    /// FNV-1a over every prop, road, water body and coarse samples of the
    /// terrain grid and the far ring: equal on every platform for the same
    /// content revision.
    #[must_use]
    pub fn stable_hash(&self) -> u64 {
        let mut hash = Fnv::default();
        hash.write(&self.content_revision.to_be_bytes());
        for prop in &self.props {
            hash.write(&kind_code(prop.kind));
            for value in prop.position.iter().chain(&prop.half_extents) {
                hash.write(&value.to_be_bytes());
            }
            hash.write(&prop.yaw.to_be_bytes());
            hash.write(&prop.scale_permille.to_be_bytes());
            hash.write(&prop.collider.map_or(u64::MAX, u64::from).to_be_bytes());
        }
        for road in &self.roads {
            hash.write(road.name.as_bytes());
            hash.write(&road.half_width.to_be_bytes());
            for point in road.points.iter().flatten() {
                hash.write(&point.to_be_bytes());
            }
        }
        for water in &self.water {
            hash.write(water.name.as_bytes());
            for value in water.centre.iter().chain(&water.radii) {
                hash.write(&value.to_be_bytes());
            }
            hash.write(&water.surface.to_be_bytes());
        }
        for grid in [self.terrain_grid(1_000), self.far_terrain_grid(6_000)] {
            for (height, color) in grid.heights.iter().zip(&grid.colors) {
                hash.write(&height.to_be_bytes());
                hash.write(color);
            }
        }
        hash.0
    }

    fn road_distance(&self, point: [i32; 2]) -> i64 {
        self.roads
            .iter()
            .map(|road| polyline_distance(point, &road.points) - i64::from(road.half_width))
            .min()
            .unwrap_or(i64::MAX)
    }

    fn water_distance(&self, point: [i32; 2]) -> i64 {
        self.water
            .iter()
            .map(|water| water.ellipse().distance(point))
            .min()
            .unwrap_or(i64::MAX)
    }

    /// Biome of a terrain vertex whose relief is `height`.
    fn biome_at(&self, x: i32, z: i32, height: i32) -> Biome {
        let point = [x, z];
        if beyond_playable(x, z) > 0 {
            return match height {
                SNOW_LINE_UNITS.. => Biome::Snow,
                1_500..SNOW_LINE_UNITS => Biome::Rock,
                700..=1_499 => Biome::Highland,
                _ => Biome::Foothills,
            };
        }
        let water = self.water_distance(point);
        if water == 0 {
            return Biome::LakeBed;
        }
        if water < 300 {
            return Biome::Shore;
        }
        if self.road_distance(point) <= 0 {
            return Biome::Road;
        }
        if SPAWN_PLAZA.contains(x, z) {
            return Biome::Plaza;
        }
        if self.farmland.iter().any(|field| field.distance(point) == 0) {
            return Biome::Farmland;
        }
        match greyhaven_vale::area_at(x, z).map(Area::id) {
            Some(greyhaven_vale::WOLFRUN_WOODS) => Biome::Woods,
            Some(greyhaven_vale::REDBRAND_HOLLOW) => Biome::Hollow,
            Some(greyhaven_vale::OUTPOST) => Biome::Hub,
            _ => Biome::Meadow,
        }
    }
}

/// Vertex colour: the biome's base colour, tinted by seeded noise.
fn biome_color(biome: Biome, x: i32, z: i32) -> [u8; 3] {
    let base = biome.base_color();
    if !biome.tinted() {
        return base;
    }
    let tint = i32::try_from(value_noise(SEED_TINT, x, z, 700) / 128).unwrap_or(0);
    base.map(|channel| clamp_channel(i32::from(channel) + tint))
}

/// Packed dirt of every road surface.
pub const ROAD_COLOR: [u8; 3] = [140, 112, 76];
/// Trampled earth of the hub plaza.
pub const PLAZA_COLOR: [u8; 3] = [150, 136, 110];
/// The lake bed under the water surface.
pub const LAKE_BED_COLOR: [u8; 3] = [120, 112, 88];
/// Snow caps above [`SNOW_LINE_UNITS`].
pub const SNOW_COLOR: [u8; 3] = [236, 238, 242];

fn clamp_channel(value: i32) -> u8 {
    u8::try_from(value.clamp(0, 255)).unwrap_or(u8::MAX)
}

/// Chebyshev distance outside the playable square, zero or negative inside.
fn beyond_playable(x: i32, z: i32) -> i64 {
    beyond(x, z, PLAYABLE_BOUNDS)
}

/// Chebyshev distance outside `bounds`, zero or negative inside.
fn beyond(x: i32, z: i32, bounds: XzBounds) -> i64 {
    let over = |value: i32, bounds: [i32; 2]| {
        (i64::from(bounds[0]) - i64::from(value)).max(i64::from(value) - i64::from(bounds[1]))
    };
    over(x, [bounds.min[0], bounds.max[0]]).max(over(z, [bounds.min[1], bounds.max[1]]))
}

/// Extra height of the distant ranges: zero on and inside the terrain
/// grid's edge, rising over [`FAR_RISE_UNITS`] to seeded massifs and crags.
fn distant_ranges(x: i32, z: i32) -> i64 {
    let grid = XzBounds {
        min: [-TERRAIN_EXTENT_UNITS, -TERRAIN_EXTENT_UNITS],
        max: [TERRAIN_EXTENT_UNITS, TERRAIN_EXTENT_UNITS],
    };
    let outside = beyond(x, z, grid);
    if outside <= 0 {
        return 0;
    }
    let rise = outside.min(FAR_RISE_UNITS) * 1024 / FAR_RISE_UNITS;
    let massif = (value_noise(SEED_MASSIF, x, z, 18_000) + 1024) / 2;
    let crags = value_noise(SEED_CRAGS, x, z, 6_000);
    let shape = 1_500 + massif * 6_500 / 1024 + crags * 1_500 / 1024;
    rise * shape.max(0) / 1024
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Shape {
    Rect(Rect),
    Segment([i32; 2], [i32; 2]),
    Ellipse(Ellipse),
}

/// Relief is exactly zero within `flat` units of the shape and returns to
/// full strength over the following `ramp` units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FlatFeature {
    shape: Shape,
    flat: i32,
    ramp: i32,
}

impl FlatFeature {
    fn distance(&self, point: [i32; 2]) -> i64 {
        match self.shape {
            Shape::Rect(rect) => rect.distance(point),
            Shape::Segment(a, b) => geometry::segment_distance(point, a, b),
            Shape::Ellipse(ellipse) => ellipse.distance(point),
        }
    }

    fn bounds(&self) -> Rect {
        let reach = self.flat + self.ramp;
        match self.shape {
            Shape::Rect(rect) => rect.expanded(reach),
            Shape::Segment(a, b) => Rect {
                min: [a[0].min(b[0]), a[1].min(b[1])],
                max: [a[0].max(b[0]), a[1].max(b[1])],
            }
            .expanded(reach),
            Shape::Ellipse(ellipse) => ellipse.bounds().expanded(reach),
        }
    }

    /// Relief strength in Q10: 0 on the flat core, 1024 beyond the ramp.
    fn strength(&self, point: [i32; 2]) -> i64 {
        let outside = self.distance(point) - i64::from(self.flat);
        (outside.max(0) * 1024 / i64::from(self.ramp.max(1))).min(1024)
    }
}

/// Flattening features bucketed on a coarse XZ grid.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Relief {
    features: Vec<FlatFeature>,
    buckets: BTreeMap<(i32, i32), Vec<usize>>,
}

const RELIEF_BUCKET_UNITS: i32 = 2_000;

impl Relief {
    fn new<'a>(
        structures: impl Iterator<Item = (&'a StaticCollider, PropKind)>,
        roads: &[Road],
        water: &[Water],
    ) -> Self {
        let mut features = Vec::new();
        for (collider, kind) in structures {
            let footprint = Rect::around(
                [collider.position[0], collider.position[2]],
                [collider.half_extents[0], collider.half_extents[2]],
            );
            // Trunks get small flat patches; buildings a wider apron.
            let (flat, ramp) = match kind {
                PropKind::Tree(_) => (30, 150),
                _ => (100, 300),
            };
            features.push(FlatFeature {
                shape: Shape::Rect(footprint),
                flat,
                ramp,
            });
        }
        for road in roads {
            for segment in road.points.windows(2) {
                features.push(FlatFeature {
                    shape: Shape::Segment(segment[0], segment[1]),
                    flat: road.half_width + 50,
                    ramp: 300,
                });
            }
        }
        features.push(FlatFeature {
            shape: Shape::Rect(Rect {
                min: SPAWN_PLAZA.min,
                max: SPAWN_PLAZA.max,
            }),
            flat: 100,
            ramp: 400,
        });
        for water in water {
            features.push(FlatFeature {
                shape: Shape::Ellipse(water.ellipse()),
                flat: 300,
                ramp: 300,
            });
        }
        let mut buckets: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (index, feature) in features.iter().enumerate() {
            let bounds = feature.bounds();
            let cell = |value: i32| value.div_euclid(RELIEF_BUCKET_UNITS);
            for cell_x in cell(bounds.min[0])..=cell(bounds.max[0]) {
                for cell_z in cell(bounds.min[1])..=cell(bounds.max[1]) {
                    buckets.entry((cell_x, cell_z)).or_default().push(index);
                }
            }
        }
        Self { features, buckets }
    }

    fn height_at(&self, x: i32, z: i32) -> i32 {
        let rolling =
            (3 * value_noise(SEED_ROLLING, x, z, 2_400) + value_noise(SEED_DETAIL, x, z, 700)) / 4;
        let walkable = rolling * i64::from(WALKABLE_RELIEF_UNITS) / 1024;
        let beyond = beyond_playable(x, z);
        let height = if beyond > 0 {
            let base = (beyond * 2 / 3).min(3_200);
            let ridge =
                value_noise(SEED_RIDGE, x, z, 3_000) * 800 / 1024 * beyond.min(3_000) / 3_000;
            let near = (walkable + base + ridge).clamp(
                -i64::from(WALKABLE_RELIEF_UNITS),
                i64::from(MOUNTAIN_PEAK_UNITS),
            );
            (near + distant_ranges(x, z)).min(i64::from(FAR_PEAK_UNITS))
        } else {
            let cell = (
                x.div_euclid(RELIEF_BUCKET_UNITS),
                z.div_euclid(RELIEF_BUCKET_UNITS),
            );
            let strength = self.buckets.get(&cell).map_or(1024, |indices| {
                indices
                    .iter()
                    .map(|&index| self.features[index].strength([x, z]))
                    .min()
                    .unwrap_or(1024)
            });
            walkable * strength / 1024
        };
        i32::try_from(height).unwrap_or(0)
    }
}

/// Collider footprints bucketed for decoration rejection.
struct Blockers {
    buckets: BTreeMap<(i32, i32), Vec<Rect>>,
}

impl Blockers {
    fn new<'a>(colliders: impl Iterator<Item = &'a StaticCollider>) -> Self {
        let mut buckets: BTreeMap<(i32, i32), Vec<Rect>> = BTreeMap::new();
        for collider in colliders {
            let rect = Rect::around(
                [collider.position[0], collider.position[2]],
                [collider.half_extents[0], collider.half_extents[2]],
            );
            for cell in cells(rect) {
                buckets.entry(cell).or_default().push(rect);
            }
        }
        Self { buckets }
    }

    fn clear(&self, footprint: Rect) -> bool {
        cells(footprint).all(|cell| {
            self.buckets
                .get(&cell)
                .is_none_or(|rects| rects.iter().all(|rect| !rect.intersects(footprint)))
        })
    }
}

fn cells(rect: Rect) -> impl Iterator<Item = (i32, i32)> {
    let cell = |value: i32| value.div_euclid(RELIEF_BUCKET_UNITS);
    let (x0, x1) = (cell(rect.min[0]), cell(rect.max[0]));
    let (z0, z1) = (cell(rect.min[1]), cell(rect.max[1]));
    (x0..=x1).flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
}

/// Base body half extents of decorative kinds at scale 1 000.
const fn base_half_extents(kind: PropKind) -> [i32; 3] {
    match kind {
        PropKind::GrassTuft => [20, 15, 20],
        PropKind::Flowers => [18, 12, 18],
        PropKind::Bush => [60, 45, 60],
        PropKind::Reeds => [12, 55, 12],
        PropKind::Tree(_) => [35, 250, 35],
        PropKind::Rock(RockSize::Small) => [40, 25, 35],
        PropKind::Rock(RockSize::Medium) => [80, 50, 70],
        PropKind::Rock(RockSize::Large) => [130, 80, 110],
        PropKind::Lamp => [10, 160, 10],
        PropKind::Signpost => [8, 110, 8],
        PropKind::Barrel => [30, 45, 30],
        PropKind::Cart => [110, 60, 60],
        PropKind::Dock => [500, 15, 90],
        PropKind::CropRow => [2_090, 25, 30],
        _ => [50, 50, 50],
    }
}

/// Where a decoration may stand.
#[derive(Clone, Copy, Eq, PartialEq)]
enum Ground {
    /// Off roads, the plaza and water.
    Open,
    /// Anywhere clear of colliders, including water (reeds, the dock).
    Shore,
}

struct Placer<'a> {
    scenery: &'a Scenery,
    blockers: &'a Blockers,
    props: &'a mut Vec<Prop>,
}

impl Placer<'_> {
    /// Places a prop if it clears every collider (and, for open ground,
    /// roads, the plaza and water). Returns whether it was placed.
    fn place(
        &mut self,
        kind: PropKind,
        at: [i32; 2],
        yaw: u16,
        scale: u16,
        ground: Ground,
    ) -> bool {
        let half = base_half_extents(kind).map(|value| {
            i32::try_from(i64::from(value) * i64::from(scale) / 1_000).unwrap_or(i32::MAX)
        });
        let prop = Prop {
            kind,
            position: [at[0], 0, at[1]],
            yaw,
            scale_permille: scale,
            collider: None,
            half_extents: half,
        };
        let footprint = prop.footprint();
        if !self.blockers.clear(footprint) {
            return false;
        }
        if ground == Ground::Open {
            let reach = i64::from(
                (footprint.max[0] - footprint.min[0]).max(footprint.max[1] - footprint.min[1]) / 2,
            );
            let point = [at[0], at[1]];
            let plaza = Rect {
                min: SPAWN_PLAZA.min,
                max: SPAWN_PLAZA.max,
            };
            if self.scenery.road_distance(point) <= reach
                || self.scenery.water_distance(point) <= reach
                || plaza.intersects(footprint)
            {
                return false;
            }
        }
        self.props.push(prop);
        true
    }

    /// Hand-placed hub, farm and lake dressing.
    fn fixed_props(&mut self) {
        let fixed: [(PropKind, [i32; 2], u16); 13] = [
            (PropKind::Lamp, [-1_800, 1_000], 0),
            (PropKind::Lamp, [1_800, 1_000], 0),
            (PropKind::Lamp, [-1_800, 3_000], 0),
            (PropKind::Lamp, [1_800, 3_000], 0),
            (PropKind::Signpost, [300, 5_300], 0),
            (PropKind::Signpost, [-4_600, 2_300], 16_384),
            (PropKind::Signpost, [300, -1_300], 32_768),
            (PropKind::Barrel, [3_080, 300], 0),
            (PropKind::Barrel, [3_080, 400], 9_000),
            (PropKind::Barrel, [2_300, 600], 20_000),
            (PropKind::Cart, [1_200, 800], 3_000),
            (PropKind::Cart, [6_900, 3_000], 40_000),
            (PropKind::Barrel, [7_300, 3_300], 0),
        ];
        for (kind, at, yaw) in fixed {
            self.place(kind, at, yaw, 1_000, Ground::Open);
        }
        // The dock runs east from the Lakeshore Path into the shallow water.
        self.place(PropKind::Dock, [3_800, -4_900], 0, 1_000, Ground::Shore);
        // Crop rows every two metres across the fenced field.
        let mut z = FARMLAND.min[1] + 190;
        while z < FARMLAND.max[1] - 100 {
            self.place(PropKind::CropRow, [7_300, z], 0, 1_000, Ground::Shore);
            z += 200;
        }
    }

    /// Seeded vegetation and rocks, rejected against colliders and features.
    fn decorations(&mut self) {
        let mut rng = Rng::new(SEED_PROPS);
        let playable = [PLAYABLE_BOUNDS.min[0] + 100, PLAYABLE_BOUNDS.max[0] - 100];
        let within = |rng: &mut Rng, bounds: XzBounds| {
            [
                rng.range(bounds.min[0], bounds.max[0]),
                rng.range(bounds.min[1], bounds.max[1]),
            ]
        };
        let playable = XzBounds {
            min: [playable[0], playable[0]],
            max: [playable[1], playable[1]],
        };
        self.scatter(&mut rng, 2_400, 10_000, |rng| {
            let at = within(rng, playable);
            (FARMLAND.distance(at) > 0).then_some((PropKind::GrassTuft, at))
        });
        self.scatter(&mut rng, 700, 6_000, |rng| {
            let at = within(rng, playable);
            let meadow = !matches!(
                greyhaven_vale::area_at(at[0], at[1]).map(Area::id),
                Some(greyhaven_vale::WOLFRUN_WOODS | greyhaven_vale::REDBRAND_HOLLOW)
            );
            (meadow && FARMLAND.distance(at) > 0).then_some((PropKind::Flowers, at))
        });
        let woods = area_bounds(greyhaven_vale::WOLFRUN_WOODS);
        self.scatter(&mut rng, 160, 2_000, |rng| {
            Some((PropKind::Bush, within(rng, woods)))
        });
        let hollow = area_bounds(greyhaven_vale::REDBRAND_HOLLOW);
        self.scatter(&mut rng, 24, 1_000, |rng| {
            let at = within(rng, hollow);
            let size = if rng.next_u64().is_multiple_of(3) {
                RockSize::Medium
            } else {
                RockSize::Small
            };
            Some((PropKind::Rock(size), at))
        });
        // Reeds ring the lake shore, partly standing in the shallow water.
        let mut placed = 0;
        for _ in 0..1_000 {
            if placed == 140 {
                break;
            }
            let angle = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
            let offset = rng.range(-200, 80);
            let (sin, cos) = trig::direction(angle);
            let along = |radius: i32, unit: i32| {
                i32::try_from(
                    i64::from(radius + offset) * i64::from(unit) / i64::from(trig::TRIG_ONE),
                )
                .unwrap_or(0)
            };
            let at = [
                LAKE.centre[0] + along(LAKE.radii[0], sin),
                LAKE.centre[1] + along(LAKE.radii[1], cos),
            ];
            let yaw = angle.wrapping_mul(7);
            let scale = u16::try_from(rng.range(800, 1_300)).unwrap_or(1_000);
            if self.scenery.road_distance(at) > 50
                && self.place(PropKind::Reeds, at, yaw, scale, Ground::Shore)
            {
                placed += 1;
            }
        }
        // Background forest on the mountain slopes beyond the walls.
        let extent = TERRAIN_EXTENT_UNITS - 200;
        let mut placed = 0;
        for _ in 0..4_000 {
            if placed == 520 {
                break;
            }
            let at = [rng.range(-extent, extent), rng.range(-extent, extent)];
            let variant = match rng.next_u64() % 10 {
                0..=6 => TreeVariant::Pine,
                7..=8 => TreeVariant::Oak,
                _ => TreeVariant::Birch,
            };
            let yaw = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
            let scale = u16::try_from(rng.range(800, 1_500)).unwrap_or(1_000);
            if beyond_playable(at[0], at[1]) >= 400
                && self.scenery.height_at(at[0], at[1]) < 2_600
                && self.place(PropKind::Tree(variant), at, yaw, scale, Ground::Shore)
            {
                placed += 1;
            }
        }
    }

    /// Up to `target` props from at most `attempts` candidates.
    fn scatter(
        &mut self,
        rng: &mut Rng,
        target: usize,
        attempts: usize,
        mut candidate: impl FnMut(&mut Rng) -> Option<(PropKind, [i32; 2])>,
    ) {
        let mut placed = 0;
        for _ in 0..attempts {
            if placed == target {
                break;
            }
            let candidate = candidate(rng);
            let yaw = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
            let scale = u16::try_from(rng.range(700, 1_300)).unwrap_or(1_000);
            if let Some((kind, at)) = candidate
                && PLAYABLE_BOUNDS.contains(at[0], at[1])
                && self.place(kind, at, yaw, scale, Ground::Open)
            {
                placed += 1;
            }
        }
    }
}

/// 64-bit FNV-1a.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_footprints_bound_the_body() {
        let prop = Prop {
            kind: PropKind::Cart,
            position: [0, 0, 0],
            yaw: 0,
            scale_permille: 1_000,
            collider: None,
            half_extents: [100, 50, 20],
        };
        assert_eq!(prop.footprint(), Rect::around([0, 0], [100, 20]));
        let quarter = Prop {
            yaw: trig::YAW_QUARTER_TURN,
            ..prop
        };
        assert_eq!(quarter.footprint(), Rect::around([0, 0], [20, 100]));
        let eighth = Prop {
            yaw: trig::YAW_EIGHTH_TURN,
            ..prop
        };
        // (100 + 20) / √2 ≈ 84.9, rounded up.
        assert_eq!(eighth.footprint(), Rect::around([0, 0], [85, 85]));
    }

    #[test]
    fn lattice_hash_separates_coordinates() {
        use geometry::lattice_hash;
        assert_ne!(lattice_hash(1, 0, 1), lattice_hash(1, 1, 0));
        assert_ne!(lattice_hash(1, -1, 0), lattice_hash(1, 1, 0));
    }
}
