//! Immutable zone content: collision geometry ([`ZoneDefinition`]) and the
//! complete validated content of a zone ([`ZoneContent`]) with its creature
//! templates, spawns, NPCs and graveyard. Render meshes/materials are
//! presentation data; collision shapes define physical truth. Generic validation
//! lives here; each authored zone is its own submodule.

pub mod greyhaven_vale;
pub mod redbrand_depths;
pub mod seam_test;
mod units;
mod zone_content;

pub(crate) use sim_foundation::collision::within_content_range;
pub use sim_foundation::collision::{
    MAX_CONTENT_COORDINATE_UNITS, MAX_STATIC_COLLIDERS, MAX_TERRAIN_HEIGHT_UNITS, StaticCollider,
    Terrain, XzBounds,
};
pub use sim_foundation::static_mesh::StaticMesh;

pub use units::{
    CreatureBehaviour, CreatureFamily, CreatureSpawn, CreatureTemplate,
    LARGE_CREATURE_HALF_WIDTH_UNITS, MAX_CREATURE_HALF_EXTENT_UNITS, MAX_CREATURE_SPAWNS,
    MAX_CREATURE_TEMPLATES, MAX_NPCS, MAX_UNIT_NAME_BYTES, MAX_WANDER_RADIUS_UNITS, Npc, NpcRole,
    PropKind,
};
pub use zone_content::{CharacterPersistence, PlayerCombatPolicy, ZoneContent};

use crate::entity::SUMMONED_BODY_BASE;
use crate::{MAX_PLAYERS_PER_ZONE, PLAYER_HALF_EXTENTS_UNITS, ZoneError};

/// One render metre corresponds to 100 integer simulation units; Y points up.
pub const UNITS_PER_METRE: i32 = 100;

/// Row-major spawn slots on flat ground. Slot `s` places a character's feet at
/// `(origin[0] + (s % columns) × spacing, 0, origin[1] + (s / columns) × spacing)`.
/// A zone definition proves every slot below `MAX_PLAYERS_PER_ZONE` is clear.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnGrid {
    /// XZ feet position of slot 0.
    pub origin: [i32; 2],
    pub columns: u16,
    pub spacing: i32,
}

impl SpawnGrid {
    /// The grid of content without an authored spawn area: 32 columns, 2 m
    /// apart, starting at the origin.
    pub const DEFAULT: Self = Self {
        origin: [0, 0],
        columns: 32,
        spacing: 200,
    };

    /// Feet position of `slot`, or `None` on arithmetic overflow or an empty grid.
    #[must_use]
    pub fn feet(self, slot: u16) -> Option<[i32; 3]> {
        let columns = u32::from(self.columns);
        let slot = u32::from(slot);
        let column = i32::try_from(slot.checked_rem(columns)?).ok()?;
        let row = i32::try_from(slot / columns).ok()?;
        Some([
            self.origin[0].checked_add(column.checked_mul(self.spacing)?)?,
            0,
            self.origin[1].checked_add(row.checked_mul(self.spacing)?)?,
        ])
    }
}

impl Default for SpawnGrid {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ZoneDefinition {
    gravity: [i32; 3],
    spawn_grid: SpawnGrid,
    colliders: Vec<StaticCollider>,
    terrain: Option<Terrain>,
    static_meshes: Vec<StaticMesh>,
}

impl ZoneDefinition {
    /// Adds validated meshes and their conservative bounds in stable ID order.
    /// Bounds support content placement checks; Rapier uses the triangles, never these boxes.
    pub fn with_static_meshes(mut self, mut meshes: Vec<StaticMesh>) -> Result<Self, ZoneError> {
        meshes.sort_by_key(|mesh| mesh.id);
        let mut colliders = self.colliders.clone();
        for mesh in &meshes {
            colliders.push(mesh.bounds()?);
        }
        let validated = Self::with_spawn_grid(self.gravity, self.spawn_grid, colliders)?;
        self.colliders = validated.colliders;
        self.static_meshes.extend(meshes);
        self.static_meshes.sort_by_key(|mesh| mesh.id);
        Ok(self)
    }

    #[must_use]
    pub fn static_meshes(&self) -> &[StaticMesh] {
        &self.static_meshes
    }

    /// Replaces the flat ground with authored terrain (#159). The spawn grid must stay flat: its
    /// slots stand on y = 0.
    pub fn with_terrain(mut self, terrain: Terrain) -> Result<Self, ZoneError> {
        terrain.validate()?;
        let grid = self.spawn_grid;
        let flat = (0..crate::MAX_PLAYERS_PER_ZONE).all(|slot| {
            u16::try_from(slot)
                .ok()
                .and_then(|slot| grid.feet(slot))
                .is_some_and(|feet| terrain.height_at(feet[0], feet[2]) == 0)
        });
        if !flat {
            return Err(ZoneError::new("terrain must be flat under the spawn grid"));
        }
        self.terrain = Some(terrain);
        Ok(self)
    }

    /// The authored ground, if the content has more than flat ground.
    #[must_use]
    pub const fn terrain(&self) -> Option<&Terrain> {
        self.terrain.as_ref()
    }

    /// The ground's height at `(x, z)` in units: the terrain's, or 0 on flat content.
    #[must_use]
    pub fn ground_at(&self, x: i32, z: i32) -> i32 {
        self.terrain
            .as_ref()
            .map_or(0, |terrain| terrain.height_at(x, z))
    }

    /// Content with the [`SpawnGrid::DEFAULT`] spawn slots.
    pub fn new(gravity: [i32; 3], colliders: Vec<StaticCollider>) -> Result<Self, ZoneError> {
        Self::with_spawn_grid(gravity, SpawnGrid::DEFAULT, colliders)
    }

    /// IDs occupy a disjoint namespace from player bodies. Sorting once gives
    /// stable construction, snapshots, and scene export independent of authoring order.
    /// Every collider bound and the body at each of the `MAX_PLAYERS_PER_ZONE`
    /// spawn slots must lie within [`MAX_CONTENT_COORDINATE_UNITS`], and no
    /// spawned body may overlap a collider (touching, such as feet on the
    /// ground, is allowed).
    pub fn with_spawn_grid(
        gravity: [i32; 3],
        spawn_grid: SpawnGrid,
        mut colliders: Vec<StaticCollider>,
    ) -> Result<Self, ZoneError> {
        if colliders.len() > MAX_STATIC_COLLIDERS {
            return Err(ZoneError::new("zone static collider capacity reached"));
        }
        colliders.sort_by_key(|collider| collider.id);
        for collider in &colliders {
            if u64::from(collider.id) >= SUMMONED_BODY_BASE {
                return Err(ZoneError::new(
                    "static collider id overlaps the summoned terrain or player body namespace",
                ));
            }
            if collider.half_extents.iter().any(|extent| *extent <= 0) {
                return Err(ZoneError::new("static collider extents must be positive"));
            }
            for (position, extent) in collider.position.iter().zip(collider.half_extents) {
                let (Some(max), Some(min)) =
                    (position.checked_add(extent), position.checked_sub(extent))
                else {
                    return Err(ZoneError::new("static collider bounds overflow"));
                };
                if !within_content_range(min) || !within_content_range(max) {
                    return Err(ZoneError::new(
                        "static collider lies outside the content coordinate range",
                    ));
                }
            }
        }
        if colliders.windows(2).any(|pair| pair[0].id == pair[1].id) {
            return Err(ZoneError::new("duplicate static collider id"));
        }
        validate_spawn_grid(spawn_grid, &colliders)?;
        Ok(Self {
            gravity,
            spawn_grid,
            colliders,
            terrain: None,
            static_meshes: Vec::new(),
        })
    }

    #[must_use]
    pub const fn gravity(&self) -> [i32; 3] {
        self.gravity
    }

    #[must_use]
    pub const fn spawn_grid(&self) -> SpawnGrid {
        self.spawn_grid
    }

    #[must_use]
    pub fn colliders(&self) -> &[StaticCollider] {
        &self.colliders
    }
}

/// Spawned bodies lie inside the content range and may touch each other and
/// colliders, but never overlap them.
fn validate_spawn_grid(grid: SpawnGrid, colliders: &[StaticCollider]) -> Result<(), ZoneError> {
    let body = PLAYER_HALF_EXTENTS_UNITS;
    if grid.columns == 0 || grid.spacing < 2 * body[0].max(body[2]) {
        return Err(ZoneError::new(
            "spawn grid needs columns and non-overlapping slots",
        ));
    }
    for slot in 0..MAX_PLAYERS_PER_ZONE {
        let feet = u16::try_from(slot)
            .ok()
            .and_then(|slot| grid.feet(slot))
            .ok_or_else(|| ZoneError::new("spawn grid overflows"))?;
        let center = [feet[0], body[1], feet[2]];
        let inside = (0..3).all(|axis| {
            let (Some(min), Some(max)) = (
                center[axis].checked_sub(body[axis]),
                center[axis].checked_add(body[axis]),
            ) else {
                return false;
            };
            within_content_range(min) && within_content_range(max)
        });
        if !inside {
            return Err(ZoneError::new(
                "spawn slot lies outside the content coordinate range",
            ));
        }
        let blocked = colliders.iter().any(|collider| {
            (0..3).all(|axis| {
                let distance = (i64::from(center[axis]) - i64::from(collider.position[axis])).abs();
                distance < i64::from(body[axis]) + i64::from(collider.half_extents[axis])
            })
        });
        if blocked {
            return Err(ZoneError::new("spawn slot overlaps a static collider"));
        }
    }
    Ok(())
}
