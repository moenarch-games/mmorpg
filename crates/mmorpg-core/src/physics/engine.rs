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
//! - Debris (#288) and movable props (#58) are engine dynamic boxes in a world rebuilt each
//!   tick from the nearby game bodies, as the Rapier backend rebuilds its own
//!   ([`World::step_debris`], [`World::step_props`]).
//!
//! Intentional differences from the Rapier backend (listed in `docs/PHYSICS_AB.md`):
//!
//! - Characters meeting head-on use the engine's native sidestep instead of the Rapier
//!   backend's `slide_around` workaround; characters do not push each other.
//! - Grounded is the engine's own result: walkable static support within the contact offset
//!   plus 5 cm after the last move, or snapped to (Rapier: any static ground within offset +
//!   5 cm). A body that was placed or added since its last move is answered by the same test
//!   as a shape cast.
//! - Overlap queries see the terrain (Rapier skips box–heightfield pairs). Flat terrain cells
//!   are removed on both backends and left to the floor half-space.

use std::collections::BTreeMap;
use std::fmt;

use physics_engine::BodyId as EngineId;
use physics_engine::approximate::{
    Body, CharacterMove, CharacterMoveResult, CharacterSettings, Config, HeightfieldDesc, Material,
    Quaternion, QueryFilter, QueryPose, RayCast, Shape, ShapeCast, Vector,
};

use super::rapier::{DEBRIS_REST_SPEED_UNITS, DEBRIS_REST_SPIN, Dynamic, TICKS_PER_SECOND, nearby};
use super::{
    Aabb, BodyId, CONTACT_OFFSET_UNITS, DebrisBody, Kind, PhysicsError, RayHit, RigidBody,
    StepStats, Vec3i, error, exact, from_subunits,
};

/// The game's centimetres per metre: the engine's length unit.
const UNITS_PER_METRE: f32 = 100.0;
/// Debris and props weigh as the Rapier backend's colliders do (kg/m³).
const DEBRIS_DENSITY: f32 = 600.0;
/// The pieces' material on both backends. Fixed geometry and the heightfield keep the engine's
/// defaults (friction 0.5, restitution 0, `CombineRule::Average`), which are Rapier's.
const PIECE_FRICTION: f32 = 0.8;
const PIECE_RESTITUTION: f32 = 0.15;
/// Ground within the contact offset plus this (5 cm) counts as support, as in Rapier.
const GROUND_PREDICTION_UNITS: f32 = 5.0;
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

/// A unit's upright capsule inside its box (`super::capsule_dimensions`), in units.
fn capsule(half_extents: Vec3i) -> Shape {
    let (half_height, radius) = super::capsule_dimensions(half_extents);
    Shape::capsule(half_height, radius)
}

fn engine_error(what: &str, failure: impl fmt::Debug) -> PhysicsError {
    error(format!("physics-engine {what} failed: {failure:?}"))
}

/// The authored ground: the content it came from, its engine body and the game ID it shares
/// with the floor it rises from.
#[derive(Clone)]
struct Ground {
    terrain: crate::Terrain,
    body: EngineId,
    floor: BodyId,
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
    /// An empty world; `gravity` is in subunits per tick squared and stays the game's: the
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
            gravity: from_subunits(gravity),
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
        let engine_body = engine_body(body);
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
}

/// `body`'s engine body, keyed by its game ID: a unit is an external capsule, everything else
/// static.
fn engine_body(body: &RigidBody) -> Body {
    let key = body.id.0;
    match body.kind {
        Kind::Moving => {
            let mut engine_body =
                Body::keyed(key, capsule(body.half_extents), lifted(body.position), 1.0);
            engine_body.external = true;
            engine_body
        }
        Kind::Floor => Body::keyed(key, Shape::HalfSpace, vector(body.position), 0.0),
        // A ramp (#59) is the engine's wedge, which rises toward local -X, turned about Y
        // toward the ramp's rise; along Z its local X extent is the box's Z extent.
        Kind::Ramp(rises) => {
            let [hx, hy, hz] = exact(body.half_extents);
            let (half, turn) = match rises {
                crate::RampRise::NegX => (Vector(hx, hy, hz), 0.0),
                crate::RampRise::PosX => (Vector(hx, hy, hz), std::f32::consts::PI),
                crate::RampRise::PosZ => (Vector(hz, hy, hx), std::f32::consts::FRAC_PI_2),
                crate::RampRise::NegZ => (Vector(hz, hy, hx), -std::f32::consts::FRAC_PI_2),
            };
            let mut wedge = Body::keyed(key, Shape::Wedge(half), vector(body.position), 0.0);
            let (sin, cos) = (turn / 2.0).sin_cos();
            wedge.orientation = Quaternion(0.0, sin, 0.0, cos);
            wedge
        }
        // A moving platform (#56) is a static box the adapter moves in place where the zone
        // moves it; the adapter carries its riders.
        Kind::Fixed | Kind::Platform => Body::keyed(
            key,
            Shape::Box(vector(exact(body.half_extents))),
            vector(body.position),
            0.0,
        ),
    }
}

impl World {
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

    fn platform_mut(&mut self, id: BodyId) -> Result<&mut RigidBody, PhysicsError> {
        self.bodies
            .get_mut(&id)
            .filter(|body| body.is_platform())
            .ok_or_else(|| error(format!("platform body {} is missing", id.0)))
    }

    pub fn move_platform(&mut self, id: BodyId, to: Vec3i) -> Result<(), PhysicsError> {
        let platform = self.platform_mut(id)?;
        let to = exact(to);
        platform.velocity = [0, 1, 2].map(|axis| to[axis] - platform.position[axis]);
        Ok(())
    }

    pub fn place_platform(&mut self, id: BodyId, to: Vec3i) -> Result<(), PhysicsError> {
        let platform = self.platform_mut(id)?;
        let by = [0, 1, 2].map(|axis| exact(to)[axis] - platform.position[axis]);
        self.shift_platform(id, by)
    }

    /// Moves platform `id`'s static box by `by`, in place (`set_static_position`).
    fn shift_platform(&mut self, id: BodyId, by: [f32; 3]) -> Result<(), PhysicsError> {
        let platform = self.platform_mut(id)?;
        for (axis, delta) in platform.position.iter_mut().zip(by) {
            *axis += delta;
        }
        platform.velocity = [0.0; 3];
        let position = platform.position;
        let handle = self.handles[&id];
        self.engine
            .set_static_position(handle, vector(position))
            .map_err(|failure| engine_error("set_static_position", failure))
    }

    /// Whether moving `body` stands on platform `platform`: walkable top within the ground
    /// prediction below it.
    fn stands_on(&self, body: &RigidBody, platform: BodyId) -> bool {
        let Some(&target) = self.handles.get(&platform) else {
            return false;
        };
        let offset = self.settings.offset;
        let predict = offset + GROUND_PREDICTION_UNITS;
        let reach = predict + offset;
        let cast = ShapeCast::new(
            QueryPose::new(capsule(body.half_extents), lifted(body.position)),
            Vector(0.0, -reach, 0.0),
        );
        let filter = QueryFilter {
            exclude: Some(self.handles[&body.id]),
            ..QueryFilter::default()
        };
        let mut hits = Vec::new();
        self.engine
            .cast_shape(cast, filter, &mut hits)
            .expect("physics-engine platform support cast failed");
        let min_up = self.settings.max_slope.cos();
        hits.iter().any(|hit| {
            hit.body == target
                && (hit.starts_overlapping || hit.normal.1 >= min_up)
                && hit.fraction * reach <= predict
        })
    }

    pub fn set_velocity(&mut self, id: BodyId, velocity: Vec3i) -> Result<(), PhysicsError> {
        let body = self
            .bodies
            .get_mut(&id)
            .filter(|body| body.is_moving())
            .ok_or_else(|| error(format!("moving physics body {} is missing", id.0)))?;
        body.velocity = from_subunits(velocity);
        Ok(())
    }

    pub fn set_weightless(&mut self, id: BodyId, weightless: bool) -> Result<(), PhysicsError> {
        let body = self
            .bodies
            .get_mut(&id)
            .filter(|body| body.is_moving())
            .ok_or_else(|| error(format!("moving physics body {} is missing", id.0)))?;
        body.weightless = weightless;
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
        // Content validation already bounds the grid; an engine refusal is a broken invariant.
        let body = self
            .engine
            .add_heightfield(heightfield(&terrain), None)
            .expect("validated content terrain is a valid heightfield");
        self.games.insert(body.to_bits(), id);
        self.ground = Some(Ground {
            terrain,
            body,
            floor: id,
        });
    }
}

/// `terrain` as an engine heightfield (see [`World::set_terrain`]).
fn heightfield(terrain: &crate::Terrain) -> HeightfieldDesc {
    let (columns, rows) = (usize::from(terrain.columns), usize::from(terrain.rows));
    // Flat cells are left to the floor half-space, as on the Rapier backend; flat cells next to
    // a basin stay, since a body straddling its edge has the floor opened under it (#274).
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let cell_centre =
        |index: usize, origin: i32| origin + index as i32 * terrain.step + terrain.step / 2;
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
                && terrain.low_within(
                    cell_centre(column, terrain.origin[0]),
                    cell_centre(row, terrain.origin[1]),
                    terrain.step,
                ) >= 0
        })
        .collect();
    #[allow(clippy::cast_precision_loss)]
    HeightfieldDesc {
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
        material: Material::default(),
    }
}

impl World {
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
    ///
    /// Moving platforms (#56) carry the units standing on them that are not rising, as on the
    /// Rapier backend: a platform's displacement is added to each rider's move, a platform that
    /// does not rise moves before the units and a rising one after them.
    pub fn step(&mut self) -> Result<StepStats, PhysicsError> {
        let platforms: Vec<_> = self
            .bodies
            .values()
            .filter(|body| body.is_platform() && body.velocity != [0.0; 3])
            .map(|body| (body.id, body.velocity))
            .collect();
        let mut carried = BTreeMap::new();
        for &(platform, displacement) in &platforms {
            for body in self.bodies.values().filter(|body| body.is_moving()) {
                if body.velocity[1] <= 0.0
                    && !carried.contains_key(&body.id)
                    && self.stands_on(body, platform)
                {
                    carried.insert(body.id, displacement);
                }
            }
        }
        for &(platform, displacement) in &platforms {
            if displacement[1] <= 0.0 {
                self.shift_platform(platform, displacement)?;
            }
        }
        let stats = self.step_characters(&carried)?;
        for &(platform, displacement) in &platforms {
            if displacement[1] > 0.0 {
                self.shift_platform(platform, displacement)?;
            }
        }
        // A rider's grounded result predates its platform's rise: ask the geometry again.
        for id in carried.keys() {
            self.grounded.remove(id);
        }
        Ok(stats)
    }

    fn step_characters(
        &mut self,
        carried: &BTreeMap<BodyId, [f32; 3]>,
    ) -> Result<StepStats, PhysicsError> {
        let mut moves = std::mem::take(&mut self.moves);
        moves.clear();
        let mut velocities = std::mem::take(&mut self.velocities);
        velocities.clear();
        for body in self.bodies.values().filter(|body| body.is_moving()) {
            let mut velocity = body.velocity;
            // A swimmer (#274) keeps the zone's vertical velocity.
            if !body.weightless {
                if self.grounded(body.id) && velocity[1] <= 0.0 {
                    velocity[1] = 0.0;
                } else {
                    for (axis, gravity) in velocity.iter_mut().zip(self.gravity) {
                        *axis += gravity;
                    }
                }
            }
            velocities.push((body.id, velocity));
            let carry = carried.get(&body.id).copied().unwrap_or([0.0; 3]);
            moves.push(CharacterMove {
                body: self.handles[&body.id],
                desired: vector([0, 1, 2].map(|axis| velocity[axis] + carry[axis])),
                exclude: self.open_floor(body),
                autostep: None,
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
            // The horizontal velocity is what the move realized, less what the platform carried.
            let carry = carried.get(&id).copied().unwrap_or([0.0; 3]);
            velocity[0] = moved[0] - carry[0];
            velocity[2] = moved[2] - carry[2];
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

    /// The floor half-space's engine body when `body` stands over terrain that dips below it
    /// (a lake basin, #274; the same footprint test as the Rapier backend): there only the
    /// heightfield is ground, and the body's move (`CharacterMove::exclude`) and support cast
    /// pass through the floor.
    fn open_floor(&self, body: &RigidBody) -> Option<EngineId> {
        let ground = self.ground.as_ref()?;
        #[allow(clippy::cast_possible_truncation)]
        let (x, z) = (
            body.position[0].round() as i32,
            body.position[2].round() as i32,
        );
        let reach = body.half_extents.x.max(body.half_extents.z) + 1;
        if ground.terrain.low_within(x, z, reach) >= 0 {
            return None;
        }
        self.handles.get(&ground.floor).copied()
    }

    /// Whether a moving body stands on static ground: its last move's engine result, or for a
    /// body placed or added since, the engine's grounded test as a downward shape cast
    /// (walkable static support within the contact offset plus 5 cm).
    pub fn grounded(&self, id: BodyId) -> bool {
        let Some(body) = self.bodies.get(&id).filter(|body| body.is_moving()) else {
            return false;
        };
        if let Some(&grounded) = self.grounded.get(&id) {
            return grounded;
        }
        // The engine's (and Rapier's) ground prediction: offset + 5 cm.
        let offset = self.settings.offset;
        let predict = offset + GROUND_PREDICTION_UNITS;
        let reach = predict + offset;
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
        // The nearest walkable static support: a too-steep face met first (a ramp's chute end)
        // is passed over, as physics-engine's own support test does (physics-engine #395). Over
        // the lake basin the floor is open, as in the body's moves.
        let min_up = self.settings.max_slope.cos();
        let open = self.open_floor(body);
        hits.iter()
            .find(|hit| {
                Some(hit.body) != open
                    && (hit.starts_overlapping || hit.normal.1 > 0.0)
                    && hit.normal.1 >= min_up
                    && self
                        .games
                        .get(&hit.body.to_bits())
                        .and_then(|game| self.bodies.get(game))
                        .is_none_or(|body| !body.is_moving())
            })
            .is_some_and(|hit| hit.fraction * reach <= predict)
    }

    pub fn overlap_query(&self, aabb: Aabb) -> Result<Vec<BodyId>, PhysicsError> {
        self.overlap_box(exact(aabb.center), exact(aabb.half_extents))
    }

    pub fn overlap_box(
        &self,
        centre: [f32; 3],
        half_extents: [f32; 3],
    ) -> Result<Vec<BodyId>, PhysicsError> {
        let pose = QueryPose::new(Shape::Box(vector(half_extents)), vector(centre));
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

    pub fn cast_shape(
        &self,
        origin: [f32; 3],
        half_extents: Vec3i,
        translation: [f32; 3],
        accept: impl Fn(BodyId) -> bool,
    ) -> Option<RayHit> {
        if translation == [0.0; 3] {
            return None;
        }
        let cast = ShapeCast::new(
            QueryPose::new(capsule(half_extents), vector(origin)),
            vector(translation),
        );
        let mut hits = Vec::new();
        self.engine
            .cast_shape(cast, QueryFilter::default(), &mut hits)
            .expect("physics-engine shape cast failed");
        // Hits come ordered by fraction, then body order (game ID).
        hits.iter().find_map(|hit| {
            let id = self.games.get(&hit.body.to_bits()).copied()?;
            accept(id).then_some(RayHit {
                body: id,
                fraction: hit.fraction,
                normal: array(hit.normal),
            })
        })
    }

    /// Debris (#288) as engine dynamic boxes: see [`Self::step_dynamic`].
    pub fn step_debris(&self, debris: &mut [DebrisBody], settle: &[bool]) -> usize {
        self.step_dynamic(debris, settle, &Dynamic::default())
            .expect("physics-engine debris step failed")
    }

    /// Movable props (#58) as rotation-locked engine boxes stopped by units' capsules.
    pub fn step_props(&self, props: &mut [DebrisBody], exclude: &[BodyId]) -> usize {
        let settle = vec![true; props.len()];
        let options = Dynamic {
            upright: true,
            units_block: true,
            exclude,
        };
        self.step_dynamic(props, &settle, &options)
            .expect("physics-engine props step failed")
    }

    /// One tick of `pieces` in a fresh engine world of the nearby game bodies, rebuilt from
    /// game state like the Rapier backend's step: the same materials (see [`PIECE_FRICTION`],
    /// density 600, linear damping 0.2, angular 0.8), sleep off, and the
    /// game's settle rule. Two substeps of four passes for debris and of one for upright props,
    /// as physics-engine's `debris` and `props` oracle scenarios calibrate them.
    fn step_dynamic(
        &self,
        pieces: &mut [DebrisBody],
        settle: &[bool],
        options: &Dynamic<'_>,
    ) -> Result<usize, PhysicsError> {
        let moving = pieces.iter().filter(|piece| !piece.resting).count();
        if moving == 0 {
            return Ok(0);
        }
        let per_second = TICKS_PER_SECOND * TICKS_PER_SECOND / UNITS_PER_METRE;
        let mut engine = physics_engine::approximate::World::new(Config {
            gravity: vector(self.gravity.map(|g| g * per_second)),
            substeps: 2,
            velocity_iterations: if options.upright { 1 } else { 4 },
            length_unit: UNITS_PER_METRE,
            ..Config::default()
        })
        .map_err(|failure| engine_error("dynamic world", failure))?;
        let add = |engine: &mut physics_engine::approximate::World, body: Body| {
            engine
                .add_body(body)
                .map_err(|failure| engine_error("add_body", failure))
        };
        for body in nearby(&self.bodies, pieces, options) {
            let mut fixed = engine_body(body);
            if body.is_moving() {
                // A unit stands in the props' way as a fixed capsule (#58).
                fixed.mass = 0.0;
                fixed.external = false;
            }
            add(&mut engine, fixed)?;
        }
        if let Some(ground) = &self.ground {
            engine
                .add_heightfield(heightfield(&ground.terrain), None)
                .map_err(|failure| engine_error("add_heightfield", failure))?;
        }
        let mut handles = Vec::with_capacity(pieces.len());
        for piece in pieces.iter() {
            let half = vector(exact(piece.half_extents));
            let mass = DEBRIS_DENSITY * 8.0 * half.0 * half.1 * half.2 / UNITS_PER_METRE.powi(3);
            let [x, y, z, w] = piece.rotation;
            let mut body = Body::new(
                Shape::Box(half),
                vector(piece.position),
                if piece.resting { 0.0 } else { mass },
            );
            body.orientation = Quaternion(x, y, z, w);
            body.friction = PIECE_FRICTION;
            body.restitution = PIECE_RESTITUTION;
            if !piece.resting {
                body.velocity = vector(piece.velocity.map(|v| v * TICKS_PER_SECOND));
                body.angular_velocity = vector(piece.spin.map(|v| v * TICKS_PER_SECOND));
                body.linear_damping = 0.2;
                body.angular_damping = 0.8;
                body.rotation_locked = options.upright;
                body.sleep_allowed = false;
            }
            let handle = add(&mut engine, body)?;
            handles.push((!piece.resting).then_some(handle));
        }
        engine
            .step(1.0 / f64::from(TICKS_PER_SECOND))
            .map_err(|failure| engine_error("dynamic step", failure))?;
        for ((piece, handle), &settles) in pieces.iter_mut().zip(handles).zip(settle) {
            let Some(body) = handle.and_then(|handle| engine.body(handle)) else {
                continue;
            };
            let Quaternion(x, y, z, w) = body.orientation;
            let norm = (x * x + y * y + z * z + w * w).sqrt();
            piece.position = array(body.position);
            piece.rotation = [x / norm, y / norm, z / norm, w / norm];
            piece.velocity = array(body.velocity).map(|v| v / TICKS_PER_SECOND);
            piece.spin = array(body.angular_velocity).map(|v| v / TICKS_PER_SECOND);
            let speed = piece.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
            let spin = piece.spin.iter().map(|v| v * v).sum::<f32>().sqrt();
            if settles && speed < DEBRIS_REST_SPEED_UNITS && spin < DEBRIS_REST_SPIN {
                piece.resting = true;
                piece.velocity = [0.0; 3];
                piece.spin = [0.0; 3];
            }
        }
        Ok(moving)
    }
}
