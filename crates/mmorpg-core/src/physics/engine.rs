//! The physics-engine backend (physics-engine #330): a thin adapter over
//! `physics_engine::approximate::World`, compiled only with the test/bench `physics-ab`
//! feature. Rapier stays the production authority (Atlas 2.1.6).
//!
//! Units: the engine world is configured with `Config::length_unit = 100`, so it works in the
//! game's centimetre units directly; nothing is converted. Its built-in character defaults
//! scale to the same 0.25-unit contact offset and 5-unit snap the Rapier backend uses.
//!
//! What maps where:
//!
//! - Fixed boxes, door leaves, world-limit slabs and NPCs are static `Shape::Box` bodies, the
//!   flat ground and the bottom world limit `Shape::HalfSpace` bodies, and the authored hills
//!   one `add_heightfield` terrain. Every body is keyed by its game [`BodyId`], so traversal
//!   and ties follow game-ID order as on Rapier; the terrain shares its game ID with the floor
//!   it rises from, so it stays keyless and is mapped back to that ID here.
//! - Players and creatures are external capsules made characters with the world's default
//!   [`CharacterSettings`]. A step moves all of them in one `move_characters` batch, in body
//!   order, each seeing the earlier movers at their new positions, as the Rapier step does
//!   body by body.
//! - Rays, overlaps and segment checks are `cast_ray`/`cast_ray_first` and `overlap_shape`.
//! - Debris (#288) stays on Rapier: [`World::step_debris`] runs the shared Rapier debris step
//!   over this adapter's fixed bodies.
//!
//! Intentional differences from the Rapier backend (listed in `docs/PHYSICS_AB.md`):
//!
//! - Characters meeting head-on use the engine's native sidestep instead of the Rapier
//!   backend's `slide_around` workaround; characters do not push each other.
//! - Grounded is the engine's own result: walkable static support within two contact offsets
//!   after the last move, or snapped to (Rapier: any static ground within offset + 5 cm).
//!   A body that was placed or added since its last move is answered by the same test as a
//!   shape cast.
//! - Overlap queries see the terrain (Rapier skips box–heightfield pairs). Flat terrain cells
//!   are removed on both backends and left to the floor half-space.

use std::collections::BTreeMap;
use std::fmt;

use physics_engine::BodyId as EngineId;
use physics_engine::approximate::{
    Body, CharacterMove, CharacterMoveResult, CharacterSettings, Config, HeightfieldDesc,
    QueryFilter, QueryPose, RayCast, Shape, ShapeCast, Vector,
};

use super::{
    Aabb, BodyId, CONTACT_OFFSET_UNITS, DebrisBody, Kind, PhysicsError, RayHit, RigidBody,
    StepStats, Vec3i, error, exact,
};

/// The game's centimetres per metre: the engine's length unit.
const UNITS_PER_METRE: f32 = 100.0;
/// A ceiling stops a rising body when it realizes less than this much (units) of its rise.
const CEILING_TOLERANCE_UNITS: f32 = 1.0e-3;

fn vector(value: [f32; 3]) -> Vector {
    Vector(value[0], value[1], value[2])
}

fn array(value: Vector) -> [f32; 3] {
    [value.0, value.1, value.2]
}

/// A moving body's engine position: its game centre raised by the contact offset.
fn lifted(position: [f32; 3]) -> Vector {
    Vector(position[0], position[1] + CONTACT_OFFSET_UNITS, position[2])
}

fn lowered(position: Vector) -> [f32; 3] {
    [position.0, position.1 - CONTACT_OFFSET_UNITS, position.2]
}

/// An upright capsule as wide and tall as `half_extents`, in units.
fn capsule(half_extents: Vec3i) -> Shape {
    #[allow(clippy::cast_precision_loss)]
    let radius = half_extents.x.max(half_extents.z) as f32;
    #[allow(clippy::cast_precision_loss)]
    let half_height = (half_extents.y as f32 - radius).max(0.0);
    Shape::capsule(half_height, radius)
}

fn engine_error(what: &str, failure: impl fmt::Debug) -> PhysicsError {
    error(format!("physics-engine {what} failed: {failure:?}"))
}

/// The authored ground: its game identity, the content it came from and its engine body.
#[derive(Clone)]
struct Ground {
    id: BodyId,
    terrain: crate::Terrain,
    body: EngineId,
}

#[derive(Clone)]
pub(super) struct World {
    gravity: [f32; 3],
    engine: physics_engine::approximate::World,
    settings: CharacterSettings,
    bodies: BTreeMap<BodyId, RigidBody>,
    ground: Option<Ground>,
    /// Game body → engine body.
    handles: BTreeMap<BodyId, EngineId>,
    /// Engine body bits → game body (the terrain maps to its floor's ID).
    games: BTreeMap<u64, BodyId>,
    /// Each moving body's grounded result from its last move; absent once it was placed or
    /// added, until a step moves it again.
    grounded: BTreeMap<BodyId, bool>,
    moves: Vec<CharacterMove>,
    results: Vec<CharacterMoveResult>,
    velocities: Vec<(BodyId, [f32; 3])>,
}

impl fmt::Debug for World {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("World")
            .field("backend", &"physics-engine")
            .field("bodies", &self.bodies.len())
            .finish_non_exhaustive()
    }
}

impl World {
    /// An empty world; `gravity` is in units per tick squared and stays the game's: the
    /// engine never integrates (every body is static or a character).
    pub fn new(gravity: Vec3i) -> Self {
        let engine = physics_engine::approximate::World::new(Config {
            gravity: Vector::ZERO,
            length_unit: UNITS_PER_METRE,
            ..Config::default()
        })
        .expect("the adapter's engine configuration is valid");
        let settings = CharacterSettings {
            offset: CONTACT_OFFSET_UNITS,
            ..engine.character_settings()
        };
        Self {
            gravity: exact(gravity),
            engine,
            settings,
            bodies: BTreeMap::new(),
            ground: None,
            handles: BTreeMap::new(),
            games: BTreeMap::new(),
            grounded: BTreeMap::new(),
            moves: Vec::new(),
            results: Vec::new(),
            velocities: Vec::new(),
        }
    }

    fn insert(&mut self, body: &RigidBody) -> Result<EngineId, PhysicsError> {
        let key = body.id.0;
        let engine_body = match body.kind {
            Kind::Moving => {
                let mut engine_body =
                    Body::keyed(key, capsule(body.half_extents), lifted(body.position), 1.0);
                engine_body.external = true;
                engine_body
            }
            Kind::Floor => Body::keyed(key, Shape::HalfSpace, vector(body.position), 0.0),
            Kind::Fixed => Body::keyed(
                key,
                Shape::Box(vector(exact(body.half_extents))),
                vector(body.position),
                0.0,
            ),
        };
        let handle = self
            .engine
            .add_body(engine_body)
            .map_err(|failure| engine_error("add_body", failure))?;
        if body.is_moving() {
            self.engine
                .add_character(handle, self.settings)
                .map_err(|failure| engine_error("add_character", failure))?;
        }
        self.games.insert(handle.to_bits(), body.id);
        Ok(handle)
    }

    pub fn add_body(&mut self, body: RigidBody) -> Result<(), PhysicsError> {
        if self.bodies.contains_key(&body.id) {
            return Err(error(format!("physics body {} already exists", body.id.0)));
        }
        if body
            .position
            .iter()
            .chain(&body.velocity)
            .any(|value| !value.is_finite())
        {
            return Err(error("physics body state must be finite"));
        }
        let handle = self.insert(&body)?;
        self.handles.insert(body.id, handle);
        self.grounded.remove(&body.id);
        self.bodies.insert(body.id, body);
        Ok(())
    }

    pub fn replace_body(&mut self, body: RigidBody) -> Result<(), PhysicsError> {
        if self.remove_body(body.id).is_none() {
            return Err(error(format!("physics body {} is missing", body.id.0)));
        }
        self.add_body(body)
    }

    pub fn remove_body(&mut self, id: BodyId) -> Option<RigidBody> {
        let body = self.bodies.remove(&id)?;
        if let Some(handle) = self.handles.remove(&id) {
            self.engine.remove_body(handle);
            self.games.remove(&handle.to_bits());
        }
        self.grounded.remove(&id);
        Some(body)
    }

    pub fn body(&self, id: BodyId) -> Option<&RigidBody> {
        self.bodies.get(&id)
    }

    pub fn set_position(&mut self, id: BodyId, position: Vec3i) -> Result<(), PhysicsError> {
        let position = exact(position);
        let body = self
            .bodies
            .get_mut(&id)
            .filter(|body| body.is_moving())
            .ok_or_else(|| error(format!("moving physics body {} is missing", id.0)))?;
        if body.position == position {
            return Ok(());
        }
        body.position = position;
        let handle = self.handles[&id];
        self.engine
            .set_kinematic_position(handle, lifted(position))
            .map_err(|failure| engine_error("set_kinematic_position", failure))?;
        self.grounded.remove(&id);
        Ok(())
    }

    pub fn set_velocity(&mut self, id: BodyId, velocity: Vec3i) -> Result<(), PhysicsError> {
        let body = self
            .bodies
            .get_mut(&id)
            .filter(|body| body.is_moving())
            .ok_or_else(|| error(format!("moving physics body {} is missing", id.0)))?;
        body.velocity = exact(velocity);
        Ok(())
    }

    /// Lays `terrain` as an engine heightfield: rows along +Z, columns along +X, heights
    /// row-major as the content stores them, with the flat cells removed like the Rapier
    /// backend removes them.
    pub fn set_terrain(&mut self, id: BodyId, terrain: crate::Terrain) {
        if let Some(ground) = self.ground.take() {
            self.engine.remove_body(ground.body);
            self.games.remove(&ground.body.to_bits());
        }
        let (columns, rows) = (usize::from(terrain.columns), usize::from(terrain.rows));
        // Flat cells are left to the floor half-space, as on the Rapier backend.
        let removed_cells = (0..rows - 1)
            .flat_map(|row| (0..columns - 1).map(move |column| (row, column)))
            .map(|(row, column)| {
                [
                    (row, column),
                    (row + 1, column),
                    (row, column + 1),
                    (row + 1, column + 1),
                ]
                .iter()
                .all(|&(r, c)| terrain.heights[r * columns + c] == 0)
            })
            .collect();
        #[allow(clippy::cast_precision_loss)]
        let desc = HeightfieldDesc {
            origin: Vector(terrain.origin[0] as f32, 0.0, terrain.origin[1] as f32),
            cell_size: (terrain.step as f32, terrain.step as f32),
            columns,
            rows,
            heights: terrain
                .heights
                .iter()
                .map(|&height| height as f32)
                .collect(),
            removed_cells: Some(removed_cells),
        };
        // Content validation already bounds the grid; an engine refusal is a broken invariant.
        let body = self
            .engine
            .add_heightfield(desc, None)
            .expect("validated content terrain is a valid heightfield");
        self.games.insert(body.to_bits(), id);
        self.ground = Some(Ground { id, terrain, body });
    }

    pub fn terrain_height(&self, x: i32, z: i32) -> Option<f32> {
        let terrain = self.ground.as_ref()?.body;
        #[allow(clippy::cast_precision_loss)]
        let top = crate::MAX_TERRAIN_HEIGHT_UNITS as f32 + 100.0;
        // The same probe as the Rapier backend: a hundredth of a unit off the grid lines.
        #[allow(clippy::cast_precision_loss)]
        let ray = RayCast::new(
            Vector(x as f32 + 0.01, top, z as f32 + 0.01),
            Vector(0.0, -2.0 * top, 0.0),
        );
        let mut hits = Vec::new();
        self.engine
            .cast_ray(ray, QueryFilter::default(), &mut hits)
            .expect("physics-engine terrain ray failed");
        hits.iter()
            .find(|hit| hit.body == terrain)
            .map(|hit| top - hit.fraction * 2.0 * top)
    }

    /// Moves `moves` in one engine batch and takes the results back into game state: exact
    /// positions from the engine's poses, and each body's grounded result.
    fn run_batch(&mut self, moves: &[CharacterMove]) -> Result<(), PhysicsError> {
        let mut results = std::mem::take(&mut self.results);
        let outcome = self
            .engine
            .move_characters(moves, &mut results)
            .map_err(|failure| engine_error("move_characters", failure));
        if outcome.is_ok() {
            for result in &results {
                let id = self.games[&result.body.to_bits()];
                self.grounded.insert(id, result.grounded);
                if result.translation == Vector::ZERO && result.depenetration == Vector::ZERO {
                    continue;
                }
                let position = self
                    .engine
                    .body(result.body)
                    .map(|body| lowered(body.position))
                    .ok_or_else(|| error("moved engine body vanished"))?;
                if let Some(body) = self.bodies.get_mut(&id) {
                    body.position = position;
                }
            }
        }
        self.results = results;
        outcome
    }

    /// Advances one tick like the Rapier backend: gravity unless grounded and not rising, one
    /// character move per moving body (all in one batch), then landing or a ceiling ends the
    /// vertical velocity and the horizontal velocity becomes what the move realized.
    pub fn step(&mut self) -> Result<StepStats, PhysicsError> {
        let mut moves = std::mem::take(&mut self.moves);
        moves.clear();
        let mut velocities = std::mem::take(&mut self.velocities);
        velocities.clear();
        for body in self.bodies.values().filter(|body| body.is_moving()) {
            let mut velocity = body.velocity;
            if self.grounded(body.id) && velocity[1] <= 0.0 {
                velocity[1] = 0.0;
            } else {
                for (axis, gravity) in velocity.iter_mut().zip(self.gravity) {
                    *axis += gravity;
                }
            }
            velocities.push((body.id, velocity));
            moves.push(CharacterMove {
                body: self.handles[&body.id],
                desired: vector(velocity),
            });
        }
        let outcome = self.run_batch(&moves);
        self.moves = moves;
        if let Err(failure) = outcome {
            self.velocities = velocities;
            return Err(failure);
        }
        let mut moved_bodies = 0;
        for ((id, mut velocity), result) in velocities.iter().copied().zip(&self.results) {
            let moved = array(result.translation);
            if (result.grounded && velocity[1] <= 0.0)
                || (velocity[1] > 0.0 && moved[1] < velocity[1] - CEILING_TOLERANCE_UNITS)
            {
                velocity[1] = 0.0;
            }
            if moved != [0.0; 3] || result.depenetration != Vector::ZERO {
                moved_bodies += 1;
            }
            velocity[0] = moved[0];
            velocity[2] = moved[2];
            if let Some(body) = self.bodies.get_mut(&id) {
                body.velocity = velocity;
            }
        }
        let moving = velocities.len();
        self.velocities = velocities;
        Ok(StepStats {
            moving,
            moved_bodies,
            bodies: self.bodies.len(),
        })
    }

    /// Whether a moving body stands on static ground: its last move's engine result, or for a
    /// body placed or added since, the engine's grounded test as a downward shape cast
    /// (walkable static support within two contact offsets).
    pub fn grounded(&self, id: BodyId) -> bool {
        let Some(body) = self.bodies.get(&id).filter(|body| body.is_moving()) else {
            return false;
        };
        if let Some(&grounded) = self.grounded.get(&id) {
            return grounded;
        }
        let offset = self.settings.offset;
        let reach = offset + self.settings.snap_to_ground.unwrap_or(0.0) + offset;
        let cast = ShapeCast::new(
            QueryPose::new(capsule(body.half_extents), lifted(body.position)),
            Vector(0.0, -reach, 0.0),
        );
        let filter = QueryFilter {
            exclude: Some(self.handles[&id]),
            ..QueryFilter::default()
        };
        let mut hits = Vec::new();
        self.engine
            .cast_shape(cast, filter, &mut hits)
            .expect("physics-engine grounded cast failed");
        let min_up = self.settings.max_slope.cos();
        hits.iter()
            .find(|hit| {
                (hit.starts_overlapping || hit.normal.1 > 0.0)
                    && self
                        .games
                        .get(&hit.body.to_bits())
                        .and_then(|game| self.bodies.get(game))
                        .is_none_or(|body| !body.is_moving())
            })
            .is_some_and(|hit| hit.normal.1 >= min_up && hit.fraction * reach <= 2.0 * offset)
    }

    pub fn overlap_query(&self, aabb: Aabb) -> Result<Vec<BodyId>, PhysicsError> {
        let pose = QueryPose::new(
            Shape::Box(vector(exact(aabb.half_extents))),
            vector(exact(aabb.center)),
        );
        let mut hits = Vec::new();
        self.engine
            .overlap_shape(pose, QueryFilter::default(), &mut hits)
            .map_err(|failure| engine_error("overlap_shape", failure))?;
        let mut bodies: Vec<_> = hits
            .iter()
            .filter_map(|hit| self.games.get(&hit.body.to_bits()).copied())
            .collect();
        bodies.sort_unstable();
        bodies.dedup();
        Ok(bodies)
    }

    /// The first ray hit on a body accepted by `accept`, in the engine's order (fraction, then
    /// body order). The common case, an accepted first hit, needs no hit list.
    fn first_ray_hit(
        &self,
        ray: RayCast,
        accept: &impl Fn(BodyId) -> bool,
    ) -> Option<(BodyId, physics_engine::approximate::RayHit)> {
        let game = |hit: &physics_engine::approximate::RayHit| {
            self.games.get(&hit.body.to_bits()).copied()
        };
        let first = self
            .engine
            .cast_ray_first(ray, QueryFilter::default())
            .expect("physics-engine ray query failed")?;
        if let Some(id) = game(&first).filter(|&id| accept(id)) {
            return Some((id, first));
        }
        let mut hits = Vec::new();
        self.engine
            .cast_ray(ray, QueryFilter::default(), &mut hits)
            .expect("physics-engine ray query failed");
        hits.into_iter()
            .find_map(|hit| game(&hit).filter(|&id| accept(id)).map(|id| (id, hit)))
    }

    pub fn segment_blocked(
        &self,
        origin: Vec3i,
        translation: Vec3i,
        blocks: impl Fn(BodyId) -> bool,
    ) -> bool {
        if translation == Vec3i::ZERO {
            return false;
        }
        let ray = RayCast::new(vector(exact(origin)), vector(exact(translation)));
        self.first_ray_hit(ray, &blocks)
            .is_some_and(|(_, hit)| hit.fraction < 1.0)
    }

    pub fn cast_ray(
        &self,
        origin: [f32; 3],
        translation: [f32; 3],
        hits: impl Fn(BodyId) -> bool,
    ) -> Option<RayHit> {
        if translation == [0.0; 3] {
            return None;
        }
        let ray = RayCast::new(vector(origin), vector(translation));
        self.first_ray_hit(ray, &hits).map(|(body, hit)| RayHit {
            body,
            fraction: hit.fraction,
            normal: array(hit.normal),
        })
    }

    /// Debris stays on Rapier (physics-engine #330 parks dynamics): the shared Rapier debris
    /// step over this world's fixed bodies and terrain.
    pub fn step_debris(&self, debris: &mut [DebrisBody], settle: &[bool]) -> usize {
        let ground = self
            .ground
            .as_ref()
            .map(|ground| (ground.id, &ground.terrain));
        super::rapier::step_debris(self.gravity, &self.bodies, ground, debris, settle)
    }
}
