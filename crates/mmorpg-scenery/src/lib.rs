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
//! The original valley's walkable ground is physically flat at y = 0; the outer valley's
//! hills (#159) are physical too, a core-authored terrain the zone's physics walks
//! units on ([`Scenery::ground_at`]). [`Scenery::height_at`] adds, on top of that ground, at
//! most ±0.6 m of relief inside the playable square, flattened to exactly 0
//! under and around structures, roads, the spawn plaza and water, and rises to
//! mountains of up to 40 m beyond the boundary walls. Beyond the terrain grid
//! (72 m past the walls) the same function keeps rising into distant ranges of up to
//! 140 m, which clients draw as a coarse far ring. Clients render a unit at
//! its physics position plus the relief height at its XZ; the offset never
//! feeds back into gameplay.
//! The Outpost uses a saved authored heightfield; surrounding relief retains
//! the original analytic function. Both clients sample the same scenery owner.
//!
//! Everything uses integer arithmetic, fixed seeds and saved inputs, so every
//! platform builds identical scenery ([`Scenery::stable_hash`]).

mod geometry;
pub mod layout;
mod outpost_grass;
mod outpost_relief;
pub mod paint;
mod redbrand_depths;
mod seam_test;
pub mod shrine;

use std::collections::BTreeMap;
use std::sync::{Arc, LazyLock, Mutex};

use strum::{IntoEnumIterator, VariantArray};

pub use geometry::Rect;
use geometry::{Ellipse, Rng, polyline_distance, splitmix64, value_noise};
use mmorpg_core::greyhaven_vale::{self, PLAYABLE_BOUNDS, SPAWN_PLAZA, dressing, ids};
use mmorpg_core::{AreaId, StaticCollider, XzBounds, trig};
pub use outpost_grass::outpost_grass_placements;
pub use redbrand_depths::redbrand_depths_scenery;
pub use seam_test::seam_test_scenery;

/// The rectangle the terrain grid covers: the region round its outline (#810), 20 to 80 m
/// beyond the playable rectangle and inside the zone's terrain grid, so every grid step
/// divides it.
pub const TERRAIN_BOUNDS: XzBounds = XzBounds {
    min: [-190_000, -140_000],
    max: [190_000, 136_000],
};
/// The rectangle the far terrain ring covers: 400 m beyond the terrain grid on every side.
pub const FAR_TERRAIN_BOUNDS: XzBounds = XzBounds {
    min: [-230_000, -180_000],
    max: [230_000, 176_000],
};
/// Largest relief magnitude inside the playable square (0.6 m).
pub const WALKABLE_RELIEF_UNITS: i32 = 60;
/// Beyond the boundary walls the surface is snow from this height (450 m above the Vale) …
pub const SNOW_LINE_UNITS: i32 = 45_000;
/// … bare rock from this one (250 m) …
const ROCK_LINE_UNITS: i32 = 25_000;
/// … and highland grass from this one (100 m), which also marks high ground inside them.
const HIGHLAND_LINE_UNITS: i32 = 10_000;
/// Highest relief of the distant ranges beyond the terrain grid (140 m).
pub const FAR_PEAK_UNITS: i32 = 14_000;
/// The distant ranges rise from nothing at the terrain grid's edge to full
/// height over this distance (120 m).
const FAR_RISE_UNITS: i64 = 12_000;
/// Half the side of the original 240 m valley, whose scatter predates the outer
/// valley (#159).
const ORIGINAL_HALF_EXTENT_UNITS: i32 = 12_000;
/// Seed of the region's ground cover beyond the original valley (#159, #816).
const SEED_OUTER_PROPS: u64 = 0x4f55_5445_5256_414c;
/// Seed of the wetlands' scatter (#426).
const SEED_MARSH_PROPS: u64 = 0x4d41_5253_5052_4f50;

/// Visual half width of every road (3 m wide); core keeps 2 m clear.
pub const ROAD_HALF_WIDTH_UNITS: i32 = 150;

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray,
)]
pub enum TreeVariant {
    #[default]
    Oak,
    Pine,
    Birch,
}

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray,
)]
pub enum RockSize {
    #[default]
    Small,
    Medium,
    Large,
}

/// What a prop looks like. Clients choose a model (or a blockout) per kind.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::EnumIter)]
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
    /// A cave's roof slab (#159), cut away while the viewer stands under it.
    CaveRoof,
    /// A ramp's wedge (#59): its box at yaw 0 rises toward +Z.
    Ramp,
    /// A loading dock (#59): a stone block with a plank deck, fitted to its collider.
    LoadingDock,
    /// A Sunken Shrine collider (#431): drawn by asset-tooling's shrine kit where the world
    /// dressing places it ([`shrine`]), never procedurally.
    ShrineKit,
    /// A ladder (#490): two rails and rungs up its collider, against a tower.
    Ladder,
    /// A climbable rope (#621): a column from the face it hangs down out to the rope's axis
    /// (`mmorpg_core::greyhaven_vale::Rope`).
    Rope,
    /// Graybox level geometry (#552), each fitted exactly to its collider box with a lighter
    /// walkable top: a wall, parapet or solid column;
    GrayboxWall,
    /// a tower;
    GrayboxTower,
    /// a floor slab or bridge one walks on;
    GrayboxWalkway,
    /// a ramp's wedge, its box at yaw 0 rising toward +Z.
    GrayboxRamp,
}

impl PropKind {
    /// Whether this is graybox level geometry, drawn at its exact collider box (feet at the
    /// box's bottom, no terrain relief).
    #[must_use]
    pub const fn graybox(self) -> bool {
        matches!(
            self,
            Self::GrayboxWall | Self::GrayboxTower | Self::GrayboxWalkway | Self::GrayboxRamp
        )
    }
}

impl PropKind {
    /// Every kind with its variants flattened, in declaration order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        static ALL: LazyLock<Vec<PropKind>> = LazyLock::new(|| {
            PropKind::iter()
                .flat_map(|kind| match kind {
                    PropKind::Tree(_) => TreeVariant::VARIANTS
                        .iter()
                        .map(|&v| PropKind::Tree(v))
                        .collect(),
                    PropKind::Rock(_) => RockSize::VARIANTS
                        .iter()
                        .map(|&s| PropKind::Rock(s))
                        .collect(),
                    kind => vec![kind],
                })
                .collect()
        });
        &ALL
    }

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
            Self::CaveRoof => "cave-roof",
            Self::Ramp => "ramp",
            Self::LoadingDock => "loading-dock",
            Self::ShrineKit => "shrine-kit",
            Self::Ladder => "ladder",
            Self::Rope => "rope",
            Self::GrayboxWall => "graybox-wall",
            Self::GrayboxTower => "graybox-tower",
            Self::GrayboxWalkway => "graybox-walkway",
            Self::GrayboxRamp => "graybox-ramp",
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
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, strum::VariantArray)]
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
            Self::Hollow => [102, 115, 72],
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

/// Terrain grids already derived from a [`Scenery`] (#526): deriving one walks every road and
/// patch, and a client asks for the same grid many times while building its static scene. A
/// copy starts empty, because its fields may be edited before it is asked for a grid.
#[derive(Default)]
struct DerivedGrids(Mutex<BTreeMap<(bool, i32), Arc<TerrainGrid>>>);

impl Clone for DerivedGrids {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for DerivedGrids {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for DerivedGrids {}

impl std::fmt::Debug for DerivedGrids {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DerivedGrids")
    }
}

/// Deterministic presentation scenery for one content revision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scenery {
    pub content_revision: u64,
    pub props: Vec<Prop>,
    /// Authored surface hazards (#294) to dress: ice and tar on the ground.
    pub surfaces: Vec<mmorpg_core::SurfacePatch>,
    /// Simplified presentation fallback for authored surfaces without detailed art. The
    /// invisible boundary walls (#810) are left out.
    pub collision_surfaces: Vec<mmorpg_core::StaticMesh>,
    pub roads: Vec<Road>,
    pub water: Vec<Water>,
    /// XZ rectangles (units) the drawn terrain leaves open: the footprints of platforms that
    /// rest flush with the ground, so their top does not z-fight it (#591).
    pub terrain_holes: Vec<Rect>,
    farmland: Vec<Rect>,
    relief: Option<Relief>,
    /// The physical ground units walk on (#159); `None` is flat ground at y = 0.
    ground: Option<mmorpg_core::Terrain>,
    /// The content's climbable surfaces (#490), which clients mark on the rock (#512).
    climbing: mmorpg_core::ClimbSurfaces,
    /// The content's rock colliders' convex proxies (#49), which clients draw as those rocks.
    rocks: Vec<(u32, mmorpg_core::RockShape)>,
    authored_outpost_relief: bool,
    /// Whether this is Greyhaven Vale's scenery (#145): only it has the authored kits'
    /// buildings (the Outpost kit, the Sunken Shrine, the walk-in buildings).
    greyhaven: bool,
    derived: DerivedGrids,
}

/// The visual of a Greyhaven Vale collider, or `None` for the ground and
/// boundary walls (and for IDs the vale does not use).
#[must_use]
/// A rock by its footprint's larger half extent.
fn rock_kind(collider: &StaticCollider) -> PropKind {
    let largest = collider.half_extents[0].max(collider.half_extents[2]);
    PropKind::Rock(match largest {
        100.. => RockSize::Large,
        75..=99 => RockSize::Medium,
        _ => RockSize::Small,
    })
}

pub fn visual_kind(collider: &StaticCollider) -> Option<PropKind> {
    let id = collider.id;
    // The region's settlements and landmarks (#818) reuse the Vale's visuals.
    if let Some(building) = greyhaven_vale::settlements::building(id) {
        use greyhaven_vale::settlements::Building;
        return Some(match building {
            Building::House => PropKind::House,
            Building::Farmhouse => PropKind::Farmhouse,
            Building::Barn => PropKind::Barn,
            Building::Windmill => PropKind::Windmill,
            Building::Well => PropKind::Well,
            Building::Tower => PropKind::Keep,
            Building::Tent => PropKind::Tent,
            Building::Campfire => PropKind::Campfire,
            Building::Crate => PropKind::Crate,
            Building::Barrel => PropKind::Barrel,
            Building::Cart => PropKind::Cart,
            Building::Fence => PropKind::Fence,
            Building::Stone | Building::Waystone => PropKind::Waystone,
            Building::MineEntrance => PropKind::MineEntrance,
            Building::Rock => PropKind::Cliff,
        });
    }
    let kind = match id {
        ids::KEEP => PropKind::Keep,
        _ if greyhaven_vale::fort::TOWER_BODIES.contains(&id) => PropKind::Keep,
        ids::INN => PropKind::Inn,
        // The smithy and house 105 are drawn by asset-tooling's Outpost kit (#420) at
        // in the world dressing data, their blockouts only while the kit is not drawn.
        ids::SMITHY => PropKind::Smithy,
        ids::WELL => PropKind::Well,
        ids::WAYSTONE => PropKind::Waystone,
        ids::BARN => PropKind::Barn,
        ids::FARMHOUSE => PropKind::Farmhouse,
        ids::WINDMILL => PropKind::Windmill,
        ids::MINE_ENTRANCE => PropKind::MineEntrance,
        ids::CAMPFIRE | greyhaven_vale::fort::REDBRAND_FIRE => PropKind::Campfire,
        _ if greyhaven_vale::fort::REDBRAND_PALISADES.contains(&id) => PropKind::PalisadeSegment,
        _ if greyhaven_vale::fort::REDBRAND_SHELTERS.contains(&id) => PropKind::Tent,
        _ if ids::HOUSES.contains(&id) || (60_000..65_000).contains(&id) => PropKind::House,
        _ if ids::PALISADE.contains(&id) => PropKind::PalisadeSegment,
        _ if ids::GATE_POSTS.contains(&id) => PropKind::GatePost,
        _ if ids::GRAVESTONES.contains(&id) => PropKind::Gravestone,
        _ if ids::FENCES.contains(&id) || ids::FARMLAND_FENCES.contains(&id) => PropKind::Fence,
        // Farmland and Hamlets (#424) reuses the Vale's building visuals; its landmark tower is
        // a keep, like the Eastwater watchtower.
        _ if ids::FARMLAND_BUILDINGS.contains(&id) => {
            use greyhaven_vale::farmland::Building;
            match greyhaven_vale::farmland::building(id)? {
                Building::House => PropKind::House,
                Building::Farmhouse => PropKind::Farmhouse,
                Building::Barn => PropKind::Barn,
                Building::Windmill => PropKind::Windmill,
                Building::Well => PropKind::Well,
                Building::Tower => PropKind::Keep,
            }
        }
        // Bandit Highlands (#425) reuses the Vale's camp, palisade and keep visuals; its crags
        // are cliff rock.
        _ if ids::HIGHLAND_BUILDINGS.contains(&id) => {
            use greyhaven_vale::highlands::Building;
            match greyhaven_vale::highlands::building(id)? {
                Building::Tower => PropKind::Keep,
                Building::Palisade => PropKind::PalisadeSegment,
                Building::GatePost => PropKind::GatePost,
                Building::Tent => PropKind::Tent,
                Building::Campfire => PropKind::Campfire,
                Building::Crate => PropKind::Crate,
            }
        }
        _ if ids::HIGHLAND_CRAGS.contains(&id) => PropKind::Cliff,
        // Marsh-Lake Wetlands (#426): the drowned watchtower is a keep, its standing stones are
        // the Greyfang stones' waystones and the cottage is a ruined house, like Southmeadow's.
        _ if ids::MARSH_BUILDINGS.contains(&id) => {
            use greyhaven_vale::marsh::Building;
            match greyhaven_vale::marsh::building(id)? {
                Building::Tower => PropKind::Keep,
                Building::Stone => PropKind::Waystone,
                Building::Ruin => PropKind::House,
            }
        }
        _ if ids::CLIFFS.contains(&id) || ids::CAVE_WALLS.contains(&id) => PropKind::Cliff,
        ids::CAVE_ROOF => PropKind::CaveRoof,
        // The sealed threshold to the deeper cave (#160): a log barricade stand-in until the
        // asset-tooling kit gives it its own kind.
        ids::CAVE_THRESHOLD => PropKind::PalisadeSegment,
        // The Sunken Shrine (#431): the ruins, the altar and the cave's rock are asset-tooling's
        // shrine kit modules, placed as world dressing; the roof slabs over the cave stay the
        // rock that caps its trench in the lake bed and the shore, cut away from inside.
        _ if ids::SHRINE_CAVE_ROOF.contains(&id) => PropKind::CaveRoof,
        _ if ids::SHRINE_RUINS.contains(&id) || ids::SHRINE_CAVE_WALLS.contains(&id) => {
            PropKind::ShrineKit
        }
        _ if ids::TENTS.contains(&id) => PropKind::Tent,
        _ if ids::CRATES.contains(&id) => PropKind::Crate,
        ids::WATCHTOWER => PropKind::Keep,
        _ if ids::LADDERS.contains(&id) => PropKind::Ladder,
        _ if ids::ROPES.contains(&id) => PropKind::Rope,
        ids::MILL_DOCK => PropKind::LoadingDock,
        _ if ids::STANDING_STONES.contains(&id) => PropKind::Waystone,
        _ if ids::RUINS.contains(&id) => PropKind::House,
        _ if ids::RUIN_FENCES.contains(&id) => PropKind::Fence,
        _ if ids::SHORE_ROCKS.contains(&id) || ids::BOULDERS.contains(&id) => rock_kind(collider),
        _ if ids::TREES.contains(&id) => PropKind::Tree(tree_variant(splitmix64(u64::from(id)))),
        // The region's designed forests' trunks and rock fields' boulders (#816).
        _ if let Some(body) = greyhaven_vale::design::region_body(collider) => match body {
            greyhaven_vale::design::RegionBody::Boulder => rock_kind(collider),
            greyhaven_vale::design::RegionBody::Trunk(species) => {
                // Each trunk's kind follows its forest's species.
                let hash = splitmix64(u64::from(id)) % 10;
                PropKind::Tree(match species {
                    Some(greyhaven_vale::design::Species::Pine) => match hash {
                        0..=7 => TreeVariant::Pine,
                        8 => TreeVariant::Birch,
                        _ => TreeVariant::Oak,
                    },
                    Some(greyhaven_vale::design::Species::Broadleaf) => match hash {
                        0..=5 => TreeVariant::Oak,
                        6..=8 => TreeVariant::Birch,
                        _ => TreeVariant::Pine,
                    },
                    _ => tree_variant(hash),
                })
            }
        },
        // Solid dressing (#577): the Outpost layout's lamps and signposts, then the fixed
        // dressing and loose rocks. The market square's proxies are drawn by its kit at
        // the committed world dressing placements.
        _ if ids::LAYOUT_LAMPS.contains(&id) => PropKind::Lamp,
        _ if ids::LAYOUT_SIGNPOSTS.contains(&id) => PropKind::Signpost,
        _ if ids::DRESSING.contains(&id) || ids::LOOSE_ROCKS.contains(&id) => {
            dressing_kind(greyhaven_vale::dressing::piece(id)?.kind)
        }
        _ => return None,
    };
    Some(kind)
}

/// The visual of a solid dressing kind (#577).
#[must_use]
pub const fn dressing_kind(kind: dressing::Kind) -> PropKind {
    match kind {
        dressing::Kind::Lamp => PropKind::Lamp,
        dressing::Kind::Signpost => PropKind::Signpost,
        dressing::Kind::Barrel => PropKind::Barrel,
        dressing::Kind::Cart => PropKind::Cart,
        dressing::Kind::Dock => PropKind::Dock,
        dressing::Kind::SmallRock => PropKind::Rock(RockSize::Small),
        dressing::Kind::MediumRock => PropKind::Rock(RockSize::Medium),
    }
}

/// A piece of solid dressing (#577) drawn exactly at its collider: feet on the collider's
/// bottom, turned by its quarter turns.
#[must_use]
pub fn dressing_prop(piece: &dressing::Piece) -> Prop {
    Prop {
        kind: dressing_kind(piece.kind),
        position: [piece.at[0], piece.feet(), piece.at[1]],
        yaw: piece.yaw(),
        scale_permille: piece.scale_permille,
        collider: Some(piece.id),
        half_extents: piece.half_extents(),
    }
}

/// Whether collider `id` is solid dressing (#577).
#[must_use]
pub fn is_dressing(id: u32) -> bool {
    ids::DRESSING.contains(&id)
        || ids::LAYOUT_LAMPS.contains(&id)
        || ids::LAYOUT_SIGNPOSTS.contains(&id)
        || ids::LOOSE_ROCKS.contains(&id)
        // The settlements' barrels and carts (#818) are drawn like the Vale's.
        || matches!(
            greyhaven_vale::settlements::building(id),
            Some(greyhaven_vale::settlements::Building::Barrel | greyhaven_vale::settlements::Building::Cart)
        )
}

/// The yaw that turns a ramp's +Z rise toward its content rise (#59).
fn ramp_yaw(body: u32) -> u16 {
    let rises = greyhaven_vale::ramps()
        .iter()
        .find(|ramp| ramp.body == body)
        .map_or(mmorpg_core::RampRise::PosZ, |ramp| ramp.rises);
    match rises {
        mmorpg_core::RampRise::PosZ => 0,
        mmorpg_core::RampRise::PosX => 16_384,
        mmorpg_core::RampRise::NegZ => 32_768,
        mmorpg_core::RampRise::NegX => 49_152,
    }
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
        PropKind::CaveRoof => [31, 0],
        PropKind::Ramp => [32, 0],
        PropKind::LoadingDock => [33, 0],
        PropKind::ShrineKit => [34, 0],
        PropKind::Ladder => [35, 0],
        PropKind::Rope => [37, 0],
        PropKind::GrayboxWall => [36, 0],
        PropKind::GrayboxTower => [36, 1],
        PropKind::GrayboxWalkway => [36, 2],
        PropKind::GrayboxRamp => [36, 3],
    }
}

/// The ground an area was designed round, as a half-open rectangle (#1063).
fn area_bounds(id: AreaId) -> XzBounds {
    greyhaven_vale::footprint(id).unwrap_or(PLAYABLE_BOUNDS)
}

fn tree_variant(hash: u64) -> TreeVariant {
    match hash % 10 {
        0..=3 => TreeVariant::Oak,
        4..=8 => TreeVariant::Pine,
        _ => TreeVariant::Birch,
    }
}

const SEED_ROLLING: u64 = 0x4752_4559_4841_5645;
const SEED_DETAIL: u64 = 0x5641_4c45_4445_5441;
const SEED_TINT: u64 = 0x5449_4e54_5449_4e54;
const SEED_PROPS: u64 = 0x5052_4f50_5345_4544;
const SEED_MASSIF: u64 = 0x4d41_5353_4946_2121;
const SEED_CRAGS: u64 = 0x4352_4147_5321_2121;
const LAKE: Water = Water {
    name: "Stillwater Lake",
    centre: greyhaven_vale::lake::CENTRE,
    radii: greyhaven_vale::lake::RADII,
    surface: greyhaven_vale::lake::SURFACE,
};
/// Ploughed field inside the Millbrook fences.
const FARMLAND: Rect = Rect {
    min: [5_010, 5_610],
    max: [9_590, 8_390],
};

/// Every ploughed field: Millbrook's, then the farmland's (#424) inside their fences.
fn fields() -> Vec<Rect> {
    std::iter::once(FARMLAND)
        .chain(
            greyhaven_vale::farmland::FIELDS
                .iter()
                .map(|&(min, max)| Rect { min, max }),
        )
        .collect()
}

/// Builds the Greyhaven Vale scenery from the core content revision.
#[must_use]
pub fn base_scenery() -> Scenery {
    let mut scenery = unmasked_greyhaven_vale_scenery();
    outpost_grass::apply(&mut scenery.props);
    // Apply after procedural placement, preserving every accepted prop/RNG draw.
    scenery.authored_outpost_relief = true;
    scenery
}

/// The scenery of the content a host runs as `zone` (`mmorpg_core::content_for_zone`): a
/// Seam Test zone's (#145) or the Redbrand Depths' (#404) by its ID, Greyhaven Vale's otherwise.
#[must_use]
pub fn scenery_for_zone(zone: mmorpg_core::ZoneId) -> Scenery {
    match zone {
        mmorpg_core::seam_test::WEST_ZONE | mmorpg_core::seam_test::EAST_ZONE => {
            seam_test_scenery(zone)
        }
        zone if mmorpg_core::redbrand_depths::is_depths_zone(zone) => redbrand_depths_scenery(),
        _ => greyhaven_vale_scenery(),
    }
}

pub fn greyhaven_vale_scenery() -> Scenery {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    layout::compose(greyhaven_vale::layout::SOURCE, &definition)
        .expect("committed scenery layout is valid")
}

/// Marshal Elden Greywatch's house (#158) collides through its walls and furniture; house
/// 103's footprint still anchors the building, its flattened ground and the decoration
/// keep-out. Clients draw it from its bundle (`mmorpg_presentation::buildings`), not as a
/// procedural house.
#[must_use]
pub fn marshal_house_anchor() -> StaticCollider {
    let house = &greyhaven_vale::marshal_house::FOOTPRINT_HALF_EXTENTS;
    StaticCollider {
        id: ids::MARSHAL_HOUSE,
        position: [
            greyhaven_vale::marshal_house::ORIGIN[0],
            house[1],
            greyhaven_vale::marshal_house::ORIGIN[2],
        ],
        half_extents: *house,
    }
}

fn unmasked_greyhaven_vale_scenery() -> Scenery {
    let definition = greyhaven_vale::base_definition();
    // The walk-in inn (#158) collides through its walls and furniture; its footprint still
    // anchors the building, its flattened ground and the decoration keep-out.
    let inn = StaticCollider {
        id: ids::INN,
        position: [
            greyhaven_vale::inn::ORIGIN[0],
            greyhaven_vale::inn::FOOTPRINT_HALF_EXTENTS[1],
            greyhaven_vale::inn::ORIGIN[2],
        ],
        half_extents: greyhaven_vale::inn::FOOTPRINT_HALF_EXTENTS,
    };
    // Ramps (#59) are wedges in their boxes; their boxes anchor them like structures. A
    // footbridge's end ramps are drawn with its dressing (#828).
    let ramps: Vec<_> = greyhaven_vale::ramps()
        .iter()
        .filter(|ramp| !greyhaven_vale::crossings::is_crossing(ramp.body))
        .map(|ramp| StaticCollider {
            id: ramp.body,
            position: ramp.position,
            half_extents: ramp.half_extents,
        })
        .collect();
    let house = marshal_house_anchor();
    // In their former collider order, the inn right after the keep and Elden's house after the
    // smithy, so placement and hashes stay put.
    let structures: Vec<_> = definition
        .colliders()
        .iter()
        .filter_map(|collider| visual_kind(collider).map(|kind| (collider, kind)))
        .flat_map(|structure| {
            let inn = (structure.0.id == ids::KEEP).then_some((&inn, PropKind::Inn));
            let house = (structure.0.id == ids::SMITHY).then_some((&house, PropKind::House));
            [Some(structure), inn, house].into_iter().flatten()
        })
        .chain(ramps.iter().map(|ramp| (ramp, PropKind::Ramp)))
        .collect();
    let roads = greyhaven_vale::roads()
        .iter()
        .map(|road| Road {
            name: road.name,
            points: road.points.to_vec(),
            half_width: ROAD_HALF_WIDTH_UNITS,
        })
        .collect::<Vec<_>>();
    // Stillwater Lake, then the wetlands' lake and pools (#426).
    let water: Vec<Water> = std::iter::once(LAKE)
        .chain(greyhaven_vale::marsh::WATERS.iter().map(|water| Water {
            name: water.name,
            centre: water.centre,
            radii: water.radii,
            surface: greyhaven_vale::marsh::SURFACE_Y,
        }))
        // The region's designed lakes (#816).
        .chain(
            greyhaven_vale::design::data()
                .lakes
                .iter()
                .map(|lake| Water {
                    name: lake.name.as_str(),
                    centre: lake.centre,
                    radii: lake.radii,
                    surface: lake.surface,
                }),
        )
        .collect();
    // The walk-in cottages (#843) are walls round open rooms, not one box: their footprints
    // still level the ground and keep the scatter out, as a house did.
    let cottages: Vec<_> = greyhaven_vale::cottages::cottages()
        .iter()
        .map(|cottage| {
            let [hx, hz] = cottage.plan.half();
            let hy = greyhaven_vale::cottages::WALL_HEIGHT / 2;
            StaticCollider {
                id: cottage.house,
                position: [cottage.centre[0], cottage.floor + hy, cottage.centre[1]],
                half_extents: [hx, hy, hz],
            }
        })
        .collect();
    // The villages' clutter (#851, #891) is drawn by its kit alone; the proxies of what stands
    // on the terrain level the ground and keep the scatter out like a crate's.
    let village: Vec<_> = definition
        .colliders()
        .iter()
        .filter(|collider| greyhaven_vale::village_props::on_terrain(collider.id))
        .cloned()
        .collect();
    let relief = Relief::new(
        structures
            .iter()
            .map(|(collider, kind)| (*collider, *kind))
            .chain(cottages.iter().map(|cottage| (cottage, PropKind::House)))
            .chain(village.iter().map(|collider| (collider, PropKind::Crate))),
        &roads,
        &water,
    );
    let mut props: Vec<Prop> = structures
        .iter()
        .map(|(collider, kind)| match dressing::piece(collider.id) {
            Some(piece) => dressing_prop(piece),
            None => Prop {
                kind: *kind,
                position: [
                    collider.position[0],
                    collider.position[1] - collider.half_extents[1],
                    collider.position[2],
                ],
                yaw: if *kind == PropKind::Ramp {
                    ramp_yaw(collider.id)
                } else {
                    0
                },
                scale_permille: 1_000,
                // A ramp is no box collider; its box is given before its yaw.
                collider: (*kind != PropKind::Ramp).then_some(collider.id),
                half_extents: match (*kind, ramp_yaw(collider.id)) {
                    (PropKind::Ramp, 16_384 | 49_152) => {
                        let [x, y, z] = collider.half_extents;
                        [z, y, x]
                    }
                    _ => collider.half_extents,
                },
            },
        })
        .collect();
    let content = greyhaven_vale::content_with_definition(definition.clone())
        .expect("built-in Greyhaven content is valid");
    let mut scenery = Scenery {
        content_revision: content.revision(),
        props: Vec::new(),
        surfaces: content.surfaces().to_vec(),
        collision_surfaces: definition
            .static_meshes()
            .iter()
            .filter(|mesh| !greyhaven_vale::is_boundary_wall(mesh))
            .cloned()
            .collect(),
        roads,
        water,
        terrain_holes: Vec::new(),
        farmland: fields(),
        relief: Some(relief),
        ground: definition.terrain().cloned(),
        climbing: content.climbing().clone(),
        rocks: content.rocks().to_vec(),
        authored_outpost_relief: false,
        greyhaven: true,
        derived: DerivedGrids::default(),
    };
    // The market square's proxies (#577) keep the scatter off it like the structures.
    let market = definition
        .colliders()
        .iter()
        .filter(|collider| ids::MARKET.contains(&collider.id));
    let blockers = Blockers::new(
        structures
            .iter()
            .map(|(collider, _)| *collider)
            .chain(market)
            .chain(cottages.iter())
            .chain(village.iter()),
    );
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
    /// An undecorated flat presentation for a host's flat arena. Never used by a host.
    #[must_use]
    pub fn flat(content_revision: u64) -> Self {
        Self {
            content_revision,
            props: Vec::new(),
            surfaces: Vec::new(),
            collision_surfaces: Vec::new(),
            roads: Vec::new(),
            water: Vec::new(),
            terrain_holes: Vec::new(),
            farmland: Vec::new(),
            relief: None,
            ground: None,
            climbing: mmorpg_core::ClimbSurfaces::default(),
            rocks: Vec::new(),
            authored_outpost_relief: false,
            greyhaven: false,
            derived: DerivedGrids::default(),
        }
    }

    /// Flat ground with graybox level geometry (#552): one prop per box `(collider, kind)`
    /// exactly at its collider, and one [`PropKind::GrayboxRamp`] per ramp wedge. Boxes keep
    /// their collider IDs, so clients treat them as structures (e.g. camera occluders). The
    /// terrain leaves out the footprint of every platform whose top rests flush with the
    /// ground at its low stop (the courtyard lifts, #591).
    #[must_use]
    pub fn graybox(
        content_revision: u64,
        boxes: &[(StaticCollider, PropKind)],
        ramps: &[mmorpg_core::Ramp],
        platforms: &[mmorpg_core::Platform],
    ) -> Self {
        let terrain_holes = platforms
            .iter()
            .filter(|lift| lift.low[1] + lift.half_extents[1] == 0)
            .map(|lift| {
                let [x, _, z] = lift.low;
                let [hx, _, hz] = lift.half_extents;
                Rect {
                    min: [x - hx, z - hz],
                    max: [x + hx, z + hz],
                }
            })
            .collect();
        let feet =
            |position: [i32; 3], half: [i32; 3]| [position[0], position[1] - half[1], position[2]];
        let solids = boxes.iter().map(|(collider, kind)| Prop {
            kind: *kind,
            position: feet(collider.position, collider.half_extents),
            yaw: 0,
            scale_permille: 1_000,
            collider: Some(collider.id),
            half_extents: collider.half_extents,
        });
        let wedges = ramps.iter().map(|ramp| {
            let [x, y, z] = ramp.half_extents;
            // The wedge rises toward local +Z; across X the box turns a quarter.
            let (yaw, half_extents) = match ramp.rises {
                mmorpg_core::RampRise::PosZ => (0, [x, y, z]),
                mmorpg_core::RampRise::PosX => (16_384, [z, y, x]),
                mmorpg_core::RampRise::NegZ => (32_768, [x, y, z]),
                mmorpg_core::RampRise::NegX => (49_152, [z, y, x]),
            };
            Prop {
                kind: PropKind::GrayboxRamp,
                position: feet(ramp.position, ramp.half_extents),
                yaw,
                scale_permille: 1_000,
                collider: None,
                half_extents,
            }
        });
        Self {
            props: solids.chain(wedges).collect(),
            terrain_holes,
            ..Self::flat(content_revision)
        }
    }

    /// Whether this is Greyhaven Vale's scenery, the only one with the authored kits'
    /// buildings (the Outpost kit, the Sunken Shrine and the walk-in buildings). Other zones'
    /// scenery draws none of them.
    #[must_use]
    pub const fn greyhaven(&self) -> bool {
        self.greyhaven
    }

    /// The physical ground's height at an XZ position, in units (#159): what units stand on.
    #[must_use]
    pub fn ground_at(&self, x: i32, z: i32) -> i32 {
        self.ground
            .as_ref()
            .map_or(0, |ground| ground.height_at(x, z))
    }

    /// The biome drawn at an XZ position.
    #[must_use]
    pub fn biome_of(&self, x: i32, z: i32) -> Biome {
        self.biome_at(x, z, self.height_at(x, z))
    }

    /// The drawn terrain surface: the physical ground plus the presentation relief.
    #[must_use]
    pub fn surface_at(&self, x: i32, z: i32) -> i32 {
        self.ground_at(x, z) + self.height_at(x, z)
    }

    /// Presentation relief at an XZ position, in units above the physical
    /// ground ([`Scenery::ground_at`]); units are drawn raised by it.
    #[must_use]
    pub fn height_at(&self, x: i32, z: i32) -> i32 {
        if self.greyhaven
            && (greyhaven_vale::brook::wet_ground(x, z)
                || (-3500..=3500).contains(&x) && (-1200..=5400).contains(&z))
        {
            return 0;
        }
        if self.authored_outpost_relief
            && let Some(height) = outpost_relief::height_at(x, z)
        {
            return height;
        }
        self.relief
            .as_ref()
            .map_or(0, |relief| relief.height_at(x, z))
    }

    /// The content's climbable surfaces (#490): the same data the zone climbs by, so clients
    /// mark exactly the rock a player can climb (#512).
    #[must_use]
    pub const fn climbing(&self) -> &mmorpg_core::ClimbSurfaces {
        &self.climbing
    }

    /// The rock proxy collider `id` collides as (#49), if it is a rock with one.
    #[must_use]
    pub fn rock(&self, id: u32) -> Option<mmorpg_core::RockShape> {
        self.rocks
            .binary_search_by_key(&id, |(rock, _)| *rock)
            .ok()
            .map(|index| self.rocks[index].1)
    }

    /// Ploughed fields (#424: one rectangle per field), whose farmland clients draw as patches.
    #[must_use]
    pub fn farmland(&self) -> &[Rect] {
        &self.farmland
    }

    /// Heights and biome colours over [`TERRAIN_BOUNDS`] every `step_units` (at least one
    /// unit).
    #[must_use]
    pub fn terrain_grid(&self, step_units: i32) -> Arc<TerrainGrid> {
        self.derived_grid(false, TERRAIN_BOUNDS, step_units)
    }

    /// The same relief and biomes over [`FAR_TERRAIN_BOUNDS`], for a coarse far ring around
    /// the terrain grid. Inside [`TERRAIN_BOUNDS`] its samples equal
    /// [`Scenery::terrain_grid`]'s at the same positions.
    #[must_use]
    pub fn far_terrain_grid(&self, step_units: i32) -> Arc<TerrainGrid> {
        self.derived_grid(true, FAR_TERRAIN_BOUNDS, step_units)
    }

    /// The grid over `bounds`, derived once per [`Scenery`] value and step.
    fn derived_grid(&self, far: bool, bounds: XzBounds, step_units: i32) -> Arc<TerrainGrid> {
        let mut grids = self
            .derived
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(
            grids
                .entry((far, step_units))
                .or_insert_with(|| Arc::new(self.grid_over(bounds, step_units))),
        )
    }

    /// Supplies the grid `(far, step_units)` a client has derived before (#526): later requests
    /// for it reuse this value instead of deriving it again. The client is responsible for
    /// handing back exactly what [`Self::terrain_grid`] or [`Self::far_terrain_grid`] produced
    /// for this content.
    pub fn seed_grid(&self, far: bool, step_units: i32, grid: TerrainGrid) {
        self.derived
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert((far, step_units), Arc::new(grid));
    }

    fn grid_over(&self, bounds: XzBounds, step_units: i32) -> TerrainGrid {
        let step = step_units.max(1);
        let count = |axis: usize| {
            usize::try_from((bounds.max[axis] - bounds.min[axis]) / step + 1).unwrap_or(1)
        };
        let (columns, rows) = (count(0), count(1));
        let origin = bounds.min;
        let mut heights = Vec::with_capacity(columns * rows);
        let mut biomes = Vec::with_capacity(columns * rows);
        let mut colors = Vec::with_capacity(columns * rows);
        let mut z = origin[1];
        for _ in 0..rows {
            let mut x = origin[0];
            for _ in 0..columns {
                let height = self.height_at(x, z);
                let biome = self.biome_at(x, z, height);
                heights.push(height + self.ground_at(x, z));
                biomes.push(biome);
                colors.push(biome_color(biome, x, z));
                x += step;
            }
            z += step;
        }
        TerrainGrid {
            origin,
            step,
            columns,
            rows,
            heights,
            biomes,
            colors,
        }
    }

    /// FNV-1a over every prop, road, water body and coarse samples of the
    /// terrain grid and the far ring: equal on every platform for the same
    /// scenery. It leaves out `content_revision`, so item, ability or quest
    /// edits that leave the vale unchanged do not move it (#487); callers
    /// check the revision on its own. The climbable surfaces are content too:
    /// they move the revision, not this hash.
    #[must_use]
    pub fn stable_hash(&self) -> u64 {
        let mut hash = Fnv::default();
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
        for patch in &self.surfaces {
            hash.write(&patch.id.to_be_bytes());
            hash.write(&[patch.kind.code()]);
            for value in patch.centre.iter().chain(&patch.radii) {
                hash.write(&value.to_be_bytes());
            }
        }
        for grid in [self.terrain_grid(1_000), self.far_terrain_grid(6_000)] {
            for (height, color) in grid.heights.iter().zip(&grid.colors) {
                hash.write(&height.to_be_bytes());
                hash.write(color);
            }
        }
        hash.0
    }

    /// Units from `point` to the nearest road's edge; zero or less on a road.
    #[must_use]
    pub fn road_distance(&self, point: [i32; 2]) -> i64 {
        self.roads
            .iter()
            .map(|road| polyline_distance(point, &road.points) - i64::from(road.half_width))
            .min()
            .unwrap_or(i64::MAX)
    }

    /// Units from `point` to the nearest water's shore; zero inside water.
    #[must_use]
    pub fn water_distance(&self, point: [i32; 2]) -> i64 {
        self.water
            .iter()
            .map(|water| water.ellipse().distance(point))
            .min()
            .unwrap_or(i64::MAX)
    }

    /// Whether the physical ground around `(x, z)` rises more than 1 m per metre (45°) on
    /// either axis: a face no unit walks up.
    fn cliff_at(&self, x: i32, z: i32) -> bool {
        const REACH: i32 = 200;
        let rise = |a: i32, b: i32| (a - b).abs() > 2 * REACH;
        rise(self.ground_at(x + REACH, z), self.ground_at(x - REACH, z))
            || rise(self.ground_at(x, z + REACH), self.ground_at(x, z - REACH))
    }

    /// Biome of the region's designed land beyond the core (#816), from the land itself:
    /// water and shores, roads and fields, bare rock on cliffs, forest floor under the designed
    /// forests, damp hollows by the rivers and the marsh, high ground, and meadow broken into
    /// natural patches of rougher grass.
    fn designed_biome(&self, x: i32, z: i32) -> Biome {
        let point = [x, z];
        let water = self.water_distance(point);
        let river = greyhaven_vale::design::river_distance(x, z);
        if water == 0 || river <= 0 {
            return Biome::LakeBed;
        }
        if water < 300 || river < 250 {
            return Biome::Shore;
        }
        if greyhaven_vale::road_distance(x, z) <= i64::from(ROAD_HALF_WIDTH_UNITS) {
            return Biome::Road;
        }
        if self.farmland.iter().any(|field| field.distance(point) == 0) {
            return Biome::Farmland;
        }
        // A road's cut and fill banks are turf, not bare rock.
        if self.cliff_at(x, z) {
            return if greyhaven_vale::road_distance(x, z) < 1_400 {
                Biome::Foothills
            } else {
                Biome::Rock
            };
        }
        // Broad patches over 90 m with a finer 25 m break-up.
        let patch = (3 * value_noise(SEED_TINT ^ 0x5157, x, z, 9_000)
            + value_noise(SEED_TINT ^ 0x5158, x, z, 2_500))
            / 4;
        // Forest floor under the closer forests; sparse trees stand in grass.
        if greyhaven_vale::design::forest_at(x, z).is_some_and(|forest| forest.cover >= 30) {
            return Biome::Woods;
        }
        let damp = river < 2_500
            || greyhaven_vale::marsh::WATERS
                .iter()
                .any(|water| water.distance_q10(x, z) < 1_500);
        if damp && patch > -300 {
            return Biome::Hollow;
        }
        let ground = i64::from(self.ground_at(x, z)) + patch * 3;
        if ground >= i64::from(HIGHLAND_LINE_UNITS) {
            return if patch > 500 {
                Biome::Rock
            } else {
                Biome::Highland
            };
        }
        if patch > 250 {
            Biome::Foothills
        } else {
            Biome::Meadow
        }
    }

    /// Biome of a terrain vertex whose relief is `height`.
    fn biome_at(&self, x: i32, z: i32, height: i32) -> Biome {
        let point = [x, z];
        if !greyhaven_vale::inside_walls(x, z) {
            // Beyond the boundary walls: the region's surroundings by their height (#810).
            // Forest climbs the lower slopes in broad tongues, rock and snow crown the peaks.
            let surface = self.ground_at(x, z) + height;
            let tongue = value_noise(SEED_TINT ^ 0x7a11, x, z, 12_000) * 6;
            return match surface {
                SNOW_LINE_UNITS.. => Biome::Snow,
                ROCK_LINE_UNITS..SNOW_LINE_UNITS => Biome::Rock,
                _ if self.cliff_at(x, z) => Biome::Rock,
                _ if i64::from(surface) + tongue >= i64::from(HIGHLAND_LINE_UNITS) / 2 => {
                    Biome::Woods
                }
                _ => Biome::Foothills,
            };
        }
        if greyhaven_vale::beyond_core(x, z) > 0 {
            return self.designed_biome(x, z);
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
        if SPAWN_PLAZA.contains(x, z) && (-800..=800).contains(&x) && (1600..=2400).contains(&z) {
            return Biome::Plaza;
        }
        if let Some(painted) = paint::biome_at(x, z) {
            return painted;
        }
        // Bare rock on cliff faces too steep to walk (#423), such as the farm terrace's.
        if self.cliff_at(x, z) {
            return Biome::Rock;
        }
        if self.farmland.iter().any(|field| field.distance(point) == 0) {
            return Biome::Farmland;
        }
        // The cave's bare rock floor (#159).
        if greyhaven_vale::cave::GREYFANG.flattens(x, z) {
            return Biome::Rock;
        }
        match greyhaven_vale::footprint_at(x, z) {
            Some(greyhaven_vale::vale::WOLFRUN_WOODS) => Biome::Woods,
            Some(greyhaven_vale::vale::REDBRAND_HOLLOW) => Biome::Hollow,
            Some(greyhaven_vale::vale::OUTPOST) => Biome::Hub,
            Some(greyhaven_vale::marsh::MARSH_LAKE_WETLANDS) => Biome::Woods,
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
    let outside = beyond(x, z, TERRAIN_BOUNDS);
    if outside <= 0 {
        return 0;
    }
    let rise = outside.min(FAR_RISE_UNITS) * 1024 / FAR_RISE_UNITS;
    let massif = (value_noise(SEED_MASSIF, x, z, 18_000) + 1024) / 2;
    let crags = value_noise(SEED_CRAGS, x, z, 6_000);
    let shape = 1_500 + massif * 6_500 / 1024 + crags * 1_500 / 1024;
    rise * shape.max(0) / 1024
}

/// Crags and gullies on the mountains beyond the boundary walls (#816): ridged noise at two
/// scales, up to 14 m high, growing from 40 m up the region's macro terrain and fading in
/// over the 20 m beyond the walls, so the slopes break into spurs and rock faces instead of
/// rising smooth.
fn mountain_crags(x: i32, z: i32) -> i64 {
    let macro_height = greyhaven_vale::design::macro_height(x, z);
    let weight = ((i64::from(macro_height) - 4_000) * 1024 / 8_000).clamp(0, 1024);
    if weight == 0 {
        return 0;
    }
    let near_walls = [(-2_000, 0), (2_000, 0), (0, -2_000), (0, 2_000)]
        .iter()
        .filter(|(dx, dz)| greyhaven_vale::inside_walls(x + dx, z + dz))
        .count();
    let weight = weight * i64::try_from(4 - near_walls).unwrap_or(0) / 4;
    let ridged = |seed: u64, cell: i32| 1024 - 2 * value_noise(seed, x, z, cell).abs();
    // Spurs stand out of the slope; they never cut below it.
    let crags =
        ((2 * ridged(SEED_CRAGS ^ 0x51, 3_500) + ridged(SEED_CRAGS ^ 0x52, 1_200)) / 3 + 1024) / 2;
    weight * crags * 1_400 / (1024 * 1024)
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
            // Trunks get small flat patches; buildings a wider apron. Solid dressing (#577)
            // stands on the drawn surface like the scatter it replaced.
            if is_dressing(collider.id) {
                continue;
            }
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
        // Beyond the boundary walls the region's own terrain carries the mountains (#810); the
        // distant ranges rise past the terrain grid.
        let height = if !greyhaven_vale::inside_walls(x, z) {
            (walkable + distant_ranges(x, z) + mountain_crags(x, z)).min(i64::from(FAR_PEAK_UNITS))
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
            position: [at[0], self.scenery.ground_at(at[0], at[1]), at[1]],
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

    /// Crop rows. The solid hub, farm and lake dressing collides, so core places it (#577).
    fn fixed_props(&mut self) {
        // Crop rows every two metres across each fenced field: Millbrook's, then the
        // farmland's (#424).
        for field in fields() {
            let x = (field.min[0] + field.max[0]) / 2;
            let mut z = field.min[1] + 190;
            while z < field.max[1] - 100 {
                self.place(PropKind::CropRow, [x, z], 0, 1_000, Ground::Shore);
                z += 200;
            }
        }
    }

    /// Seeded vegetation and rocks, rejected against colliders and features.
    fn decorations(&mut self) {
        let mut rng = Rng::new(SEED_PROPS);
        // The original 240 m valley keeps its scatter exactly; the outer valley (#159)
        // scatters from its own seed afterwards.
        let playable = [
            ORIGINAL_HALF_EXTENT_UNITS - 100,
            ORIGINAL_HALF_EXTENT_UNITS - 100,
        ];
        let playable = [-playable[0], playable[1]];
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
                greyhaven_vale::footprint_at(at[0], at[1]),
                Some(greyhaven_vale::vale::WOLFRUN_WOODS | greyhaven_vale::vale::REDBRAND_HOLLOW)
            );
            (meadow && FARMLAND.distance(at) > 0).then_some((PropKind::Flowers, at))
        });
        let woods = area_bounds(greyhaven_vale::vale::WOLFRUN_WOODS);
        self.scatter(&mut rng, 160, 2_000, |rng| {
            Some((PropKind::Bush, within(rng, woods)))
        });
        // The Redbrand Hollow's loose rocks collide, so core scatters them (#577).
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
        // Background forest on the slopes at least 4 m beyond the boundary walls, below the
        // rock line (#424, #810).
        let beyond_walls = |at: [i32; 2]| {
            [(0, 0), (-400, 0), (400, 0), (0, -400), (0, 400)]
                .iter()
                .all(|(dx, dz)| !greyhaven_vale::inside_walls(at[0] + dx, at[1] + dz))
        };
        let mut placed = 0;
        for _ in 0..30_000 {
            if placed == 1_800 {
                break;
            }
            let at = [
                rng.range(TERRAIN_BOUNDS.min[0] + 200, TERRAIN_BOUNDS.max[0] - 200),
                rng.range(TERRAIN_BOUNDS.min[1] + 200, TERRAIN_BOUNDS.max[1] - 200),
            ];
            let variant = match rng.next_u64() % 10 {
                0..=6 => TreeVariant::Pine,
                7..=8 => TreeVariant::Oak,
                _ => TreeVariant::Birch,
            };
            let yaw = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
            let scale = u16::try_from(rng.range(800, 1_500)).unwrap_or(1_000);
            // Background foliage has no body. Keep its roots on supported drawn
            // ground, including the presentation relief beyond the playable boundary.
            let heights = [(0, 0), (-200, 0), (200, 0), (0, -200), (0, 200)]
                .map(|(dx, dz)| self.scenery.surface_at(at[0] + dx, at[1] + dz));
            let supported = heights.into_iter().max().unwrap_or(0)
                - heights.into_iter().min().unwrap_or(0)
                <= 300;
            if beyond_walls(at)
                && supported
                && heights[0] < ROCK_LINE_UNITS
                && self.place(PropKind::Tree(variant), at, yaw, scale, Ground::Shore)
            {
                placed += 1;
            }
        }
        self.ridge_scrub();
        self.hollow_scrub();
        self.marsh_decorations();
        self.region_ground_cover();
        self.region_undergrowth();
    }

    /// Patches of low scrub soften the rocky basin shoulders. This independent seed leaves
    /// existing scatter stable; bushes are foliage and never introduce solid client bodies.
    fn ridge_scrub(&mut self) {
        let mut rng = Rng::new(0x513_5c2b);
        let scenery = self.scenery;
        self.scatter(&mut rng, 180, 2400, |rng| {
            let at = [rng.range(-8500, 8500), rng.range(-9000, 11000)];
            let h = scenery.ground_at(at[0], at[1]);
            let away_from_village = at[0].abs() > 3700 || at[1] < -2000 || at[1] > 5800;
            let patch =
                ((at[0].div_euclid(1800) * 31 + at[1].div_euclid(1800) * 17).rem_euclid(7)) < 4;
            (away_from_village && (300..2600).contains(&h) && patch).then_some((PropKind::Bush, at))
        });
    }

    /// Low scrub at the camp's edges, clear of its central lane and tent approaches.
    fn hollow_scrub(&mut self) {
        let mut rng = Rng::new(0x0051_3ca0);
        self.scatter(&mut rng, 18, 600, |rng| {
            let at: [i32; 2] = [rng.range(-2000, 2000), rng.range(-10400, -7900)];
            (at[0].abs() > 700
                && [-9750, -9450, -8450]
                    .into_iter()
                    .all(|z| (at[1] - z).abs() > 220))
            .then_some((PropKind::Bush, at))
        });
    }

    /// Undergrowth in the region's designed forests and along their edges (#816): clumps of two
    /// to four bushes, foliage that never blocks, thicker under the trees than out in the open.
    fn region_undergrowth(&mut self) {
        let design = greyhaven_vale::design::data();
        let mut rng = Rng::new(0x554e_4445_5247_524f);
        for forest in &design.forests {
            let [min, max] = forest.bounds();
            let area = i64::from(max[0] - min[0]) * i64::from(max[1] - min[1]);
            // About one clump per 400 m² of the outline's bounds, kept where the forest grows
            // or within a few metres of its edge.
            let attempts = usize::try_from(area / 4_000_000).unwrap_or(0);
            for _ in 0..attempts {
                let centre = [rng.range(min[0], max[0]), rng.range(min[1], max[1])];
                let inside = greyhaven_vale::design::forest_at(centre[0], centre[1])
                    .is_some_and(|grows| grows.seed == forest.seed);
                let edge = !inside
                    && [(-600, 0), (600, 0), (0, -600), (0, 600)]
                        .iter()
                        .any(|(dx, dz)| {
                            greyhaven_vale::design::forest_at(centre[0] + dx, centre[1] + dz)
                                .is_some_and(|grows| grows.seed == forest.seed)
                        });
                if !(inside || edge)
                    || (inside && forest.cover < 30 && !rng.next_u64().is_multiple_of(3))
                    || greyhaven_vale::road_distance(centre[0], centre[1]) < 400
                    || greyhaven_vale::design::river_distance(centre[0], centre[1]) < 300
                    || self.scenery.water_distance(centre) < 400
                    || !greyhaven_vale::inside_walls(centre[0], centre[1])
                {
                    continue;
                }
                for _ in 0..2 + rng.next_u64() % 3 {
                    let at = [
                        centre[0] + rng.range(-220, 220),
                        centre[1] + rng.range(-220, 220),
                    ];
                    let yaw = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
                    let scale = u16::try_from(rng.range(700, 1_400)).unwrap_or(1_000);
                    if greyhaven_vale::inside_walls(at[0], at[1]) {
                        self.place(PropKind::Bush, at, yaw, scale, Ground::Open);
                    }
                }
            }
        }
    }

    /// Reeds round every marsh water, partly standing in it (#426).
    fn marsh_decorations(&mut self) {
        let mut rng = Rng::new(SEED_MARSH_PROPS);
        for water in greyhaven_vale::marsh::WATERS {
            // About one clump per 3 m of shore.
            let target = usize::try_from((water.radii[0] + water.radii[1]) / 100).unwrap_or(0);
            let mut placed = 0;
            for _ in 0..target * 6 {
                if placed == target {
                    break;
                }
                let angle = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
                let offset = rng.range(-200, 120);
                let (sin, cos) = trig::direction(angle);
                let along = |radius: i32, unit: i32| {
                    i32::try_from(
                        i64::from(radius + offset) * i64::from(unit) / i64::from(trig::TRIG_ONE),
                    )
                    .unwrap_or(0)
                };
                let at = [
                    water.centre[0] + along(water.radii[0], sin),
                    water.centre[1] + along(water.radii[1], cos),
                ];
                let yaw = angle.wrapping_mul(7);
                let scale = u16::try_from(rng.range(800, 1_400)).unwrap_or(1_000);
                if self.scenery.road_distance(at) > 50
                    && self.place(PropKind::Reeds, at, yaw, scale, Ground::Shore)
                {
                    placed += 1;
                }
            }
        }
    }

    /// Grass, flowers and scrub across the region beyond the original valley (#816), drawn from
    /// the land itself rather than from rectangles, in clumps of three to seven: flower drifts in
    /// the meadows, scrub on rough grass and the high ground, tussocks in the damp hollows and
    /// high patches, and nothing on roads, fields, rock or water.
    fn region_ground_cover(&mut self) {
        let mut rng = Rng::new(SEED_OUTER_PROPS);
        let bounds = greyhaven_vale::PLAYABLE_BOUNDS;
        let mut clumps = 0;
        for _ in 0..40_000 {
            if clumps == 1_100 {
                break;
            }
            let centre = [
                rng.range(bounds.min[0], bounds.max[0]),
                rng.range(bounds.min[1], bounds.max[1]),
            ];
            let original = centre[0].abs().max(centre[1].abs()) < ORIGINAL_HALF_EXTENT_UNITS
                && greyhaven_vale::beyond_core(centre[0], centre[1]) <= 0;
            if original || !greyhaven_vale::inside_walls(centre[0], centre[1]) {
                continue;
            }
            // Patches over about 40 m and drifts over about 20 m decide what grows, so cover
            // gathers in places instead of spreading evenly.
            let patch = value_noise(SEED_OUTER_PROPS ^ 0x50, centre[0], centre[1], 4_000);
            let drift = value_noise(SEED_OUTER_PROPS ^ 0x51, centre[0], centre[1], 2_200);
            let kind = match self.scenery.designed_biome(centre[0], centre[1]) {
                Biome::Meadow if drift > 300 => PropKind::Flowers,
                Biome::Meadow | Biome::Foothills if patch > 450 => PropKind::Bush,
                Biome::Foothills | Biome::Highland if drift > 350 => PropKind::Bush,
                Biome::Hollow | Biome::Highland if patch > 250 => PropKind::GrassTuft,
                _ => continue,
            };
            clumps += 1;
            for _ in 0..3 + rng.next_u64() % 5 {
                let at = [
                    centre[0] + rng.range(-250, 250),
                    centre[1] + rng.range(-250, 250),
                ];
                let yaw = u16::try_from(rng.next_u64() % 65_536).unwrap_or(0);
                let scale = u16::try_from(rng.range(700, 1_300)).unwrap_or(1_000);
                if greyhaven_vale::inside_walls(at[0], at[1]) {
                    self.place(kind, at, yaw, scale, Ground::Open);
                }
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
                && greyhaven_vale::inside_walls(at[0], at[1])
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
