//! The zone's authoritative physical world, resolved by Rapier (#271, Atlas 2.1.4).
//!
//! `mmorpg-core` decides movement intent, jumps, gravity and every gameplay outcome; this
//! module asks Rapier where that intent physically leads. Static content colliders, world
//! limits, door leaves and NPCs are fixed cuboid colliders. Players and creatures are
//! kinematic position-based bodies with upright capsule colliders, moved by Rapier's
//! `KinematicCharacterController`, which resolves contact, sliding and support.
//!
//! The game speaks centimetre units in `f32`: a moving body's position and velocity (units
//! per tick) are authoritative here and go into canonical snapshots exactly; gameplay rules
//! and projections read a rounded whole-unit view ([`RigidBody::position`]). Rapier simulates
//! in metres, the scale its tolerances and parry's epsilons assume. Nothing of Rapier's own
//! state is saved: recovery rebuilds this world from the bodies the game restores, and every
//! step rebuilds it in body-ID order first, so a restored zone continues exactly.
//!
//! The controller's contact queries go through [`queries`], exact closed forms for the
//! upright capsules and boxes the zone uses, ahead of parry's general GJK/EPA path.

use std::collections::BTreeMap;
use std::fmt;

use rapier3d::control::{CharacterLength, KinematicCharacterController};
use rapier3d::parry::query::{
    DefaultQueryDispatcher, QueryDispatcher, QueryDispatcherChain, ShapeCastOptions,
};
use rapier3d::prelude::{
    Capsule, ColliderBuilder, ColliderHandle, Cuboid, PhysicsWorld, Pose, QueryFilter,
    Ray as RapierRay, RigidBodyBuilder, RigidBodyHandle, SharedShape, Vector,
};

use queries::UprightShapes;

mod queries;

/// Centimetre units per metre.
const UNITS_PER_METRE: f32 = 100.0;
/// The gap (in units) Rapier's controller keeps between a body and what it touches: well
/// above `f32` resolution across the zone and below the half unit projections round away.
const CONTACT_OFFSET_UNITS: f32 = 0.25;
/// [`CONTACT_OFFSET_UNITS`] in metres.
const CONTACT_OFFSET: f32 = CONTACT_OFFSET_UNITS / UNITS_PER_METRE;
/// How far (in metres) below a body the controller still finds the ground: its contact
/// offset plus the fixed 5 cm its grounded test predicts. Support queries and snapping use
/// the same reach.
const SUPPORT_REACH: f32 = CONTACT_OFFSET + 0.05;
/// A ground gap (units) below this counts as standing on it.
const GAP_TOLERANCE_UNITS: f32 = 1.0e-3;

/// An integer vector in units: the rounded view gameplay rules read, and static geometry.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Vec3i {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl Vec3i {
    pub const ZERO: Self = Self::new(0, 0, 0);

    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }
}

/// A whole-unit vector as exact `f32` units (canonical snapshot state).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn from_units(value: [i32; 3]) -> [f32; 3] {
    value.map(|axis| axis as f32)
}

/// Exact `f32` units rounded to whole units, as gameplay rules see them.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn to_units(value: [f32; 3]) -> [i32; 3] {
    value.map(|axis| axis.round() as i32)
}

/// A game-chosen body identity (content colliders, players, creatures and NPCs each take
/// their own range, see `entity.rs`).
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BodyId(pub u64);

/// An axis-aligned query box: centre and half extents in units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Aabb {
    pub center: Vec3i,
    pub half_extents: Vec3i,
}

impl Aabb {
    #[must_use]
    pub const fn new(center: Vec3i, half_extents: Vec3i) -> Self {
        Self {
            center,
            half_extents,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhysicsError(String);

impl fmt::Display for PhysicsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PhysicsError {}

fn error(message: impl Into<String>) -> PhysicsError {
    PhysicsError(message.into())
}

/// Whether a body is fixed geometry, a flat floor or a moving unit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Kind {
    Fixed,
    /// An upward half-space whose surface is at the body's `y`.
    Floor,
    Moving,
}

/// One body: a fixed box, or a moving unit's capsule with its authoritative `f32` state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidBody {
    id: BodyId,
    kind: Kind,
    /// Centre, in units.
    position: [f32; 3],
    /// Units per tick; zero for fixed bodies.
    velocity: [f32; 3],
    half_extents: Vec3i,
}

fn round(value: [f32; 3]) -> Vec3i {
    // Positions stay within the content range, far inside `i32`.
    #[allow(clippy::cast_possible_truncation)]
    let round = |value: f32| value.round() as i32;
    Vec3i::new(round(value[0]), round(value[1]), round(value[2]))
}

#[allow(clippy::cast_precision_loss)]
const fn exact(value: Vec3i) -> [f32; 3] {
    [value.x as f32, value.y as f32, value.z as f32]
}

impl RigidBody {
    /// A fixed box centred on `position`.
    #[must_use]
    pub const fn fixed(id: BodyId, position: Vec3i, half_extents: Vec3i) -> Self {
        Self {
            id,
            kind: Kind::Fixed,
            position: exact(position),
            velocity: [0.0; 3],
            half_extents,
        }
    }

    /// A flat floor: everything below height `y` (units) is solid. Floors meet capsules
    /// through exact queries, where a large box's top face would go through parry's GJK.
    #[must_use]
    pub const fn floor(id: BodyId, y: i32) -> Self {
        Self {
            id,
            kind: Kind::Floor,
            position: exact(Vec3i::new(0, y, 0)),
            velocity: [0.0; 3],
            half_extents: Vec3i::ZERO,
        }
    }

    /// A moving unit at a whole-unit position and velocity.
    #[must_use]
    pub const fn dynamic(
        id: BodyId,
        position: Vec3i,
        velocity: Vec3i,
        half_extents: Vec3i,
    ) -> Self {
        Self::moving(id, exact(position), exact(velocity), half_extents)
    }

    /// A moving unit at an exact position and velocity (recovery).
    #[must_use]
    pub const fn moving(
        id: BodyId,
        position: [f32; 3],
        velocity: [f32; 3],
        half_extents: Vec3i,
    ) -> Self {
        Self {
            id,
            kind: Kind::Moving,
            position,
            velocity,
            half_extents,
        }
    }

    #[must_use]
    pub const fn id(&self) -> BodyId {
        self.id
    }

    /// The centre rounded to whole units, as gameplay rules and projections see it.
    #[must_use]
    pub fn position(&self) -> Vec3i {
        round(self.position)
    }

    /// The velocity rounded to whole units per tick.
    #[must_use]
    pub fn velocity(&self) -> Vec3i {
        round(self.velocity)
    }

    /// The authoritative centre, in units.
    #[must_use]
    pub const fn exact_position(&self) -> [f32; 3] {
        self.position
    }

    /// The authoritative velocity, in units per tick.
    #[must_use]
    pub const fn exact_velocity(&self) -> [f32; 3] {
        self.velocity
    }

    #[must_use]
    pub const fn half_extents(&self) -> Vec3i {
        self.half_extents
    }

    const fn is_moving(&self) -> bool {
        matches!(self.kind, Kind::Moving)
    }
}

/// What one step did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepStats {
    /// Moving bodies (players and living creatures) the step visited.
    pub moving: usize,
    /// Moving bodies the character controller moved.
    pub moved_bodies: usize,
    /// Bodies in the rebuilt Rapier world.
    pub bodies: usize,
}

/// A body's Rapier handles.
#[derive(Clone, Copy)]
struct Handles {
    body: Option<RigidBodyHandle>,
    collider: ColliderHandle,
}

/// The authored ground as a Rapier heightfield (#159).
#[derive(Clone)]
struct Ground {
    id: BodyId,
    terrain: crate::Terrain,
    collider: Option<ColliderHandle>,
}

pub struct World {
    gravity: [f32; 3],
    ground: Option<Ground>,
    world: PhysicsWorld,
    queries: QueryDispatcherChain<UprightShapes, DefaultQueryDispatcher>,
    controller: KinematicCharacterController,
    bodies: BTreeMap<BodyId, RigidBody>,
    handles: BTreeMap<BodyId, Handles>,
}

impl fmt::Debug for World {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("World")
            .field("bodies", &self.bodies.len())
            .finish_non_exhaustive()
    }
}

impl Clone for World {
    fn clone(&self) -> Self {
        let mut world = Self::new(round(self.gravity));
        world.gravity = self.gravity;
        world.bodies = self.bodies.clone();
        world.ground = self.ground.clone();
        world.rebuild();
        world
    }
}

fn metres(value: [f32; 3]) -> Vector {
    Vector::new(value[0], value[1], value[2]) / UNITS_PER_METRE
}

/// A moving body's Rapier position: its game centre raised by the contact offset, so a body
/// the controller rests on the ground (one offset above it) has its feet exactly on the
/// ground in game coordinates.
fn lifted(value: [f32; 3]) -> Vector {
    metres(value) + Vector::new(0.0, CONTACT_OFFSET, 0.0)
}

fn units(value: Vector) -> [f32; 3] {
    let value = value * UNITS_PER_METRE;
    [value.x, value.y, value.z]
}

fn empty_world() -> PhysicsWorld {
    let mut world = PhysicsWorld::new();
    // Gravity and jumps are movement intent the zone integrates: nothing is dynamic.
    world.gravity = Vector::ZERO;
    world.integration_parameters.dt = 1.0 / 30.0;
    world
}

/// An upright capsule as wide and tall as `half_extents`, in metres.
fn capsule(half_extents: Vec3i) -> Capsule {
    #[allow(clippy::cast_precision_loss)]
    let radius = half_extents.x.max(half_extents.z) as f32;
    #[allow(clippy::cast_precision_loss)]
    let half_height = (half_extents.y as f32 - radius).max(0.0);
    Capsule::new_y(half_height / UNITS_PER_METRE, radius / UNITS_PER_METRE)
}

fn cuboid(half_extents: Vec3i) -> Cuboid {
    Cuboid::new(metres(exact(half_extents)))
}

fn insert(world: &mut PhysicsWorld, body: &RigidBody) -> Handles {
    let user_data = u128::from(body.id.0);
    if body.is_moving() {
        let handle = world.bodies.insert(
            RigidBodyBuilder::kinematic_position_based()
                .translation(lifted(body.position))
                .build(),
        );
        let collider = world.colliders.insert_with_parent(
            ColliderBuilder::new(SharedShape::new(capsule(body.half_extents)))
                .user_data(user_data)
                .build(),
            handle,
            &mut world.bodies,
        );
        Handles {
            body: Some(handle),
            collider,
        }
    } else if matches!(body.kind, Kind::Floor) {
        let collider = world.colliders.insert(
            ColliderBuilder::new(SharedShape::halfspace(Vector::Y))
                .translation(metres(body.position))
                .user_data(user_data)
                .build(),
        );
        Handles {
            body: None,
            collider,
        }
    } else {
        let collider = world.colliders.insert(
            ColliderBuilder::new(SharedShape::new(cuboid(body.half_extents)))
                .translation(metres(body.position))
                .user_data(user_data)
                .build(),
        );
        Handles {
            body: None,
            collider,
        }
    }
}

/// Inserts `terrain` as a heightfield collider: its rows run along +Z and its columns along +X.
fn insert_ground(world: &mut PhysicsWorld, id: BodyId, terrain: &crate::Terrain) -> ColliderHandle {
    let (columns, rows) = (usize::from(terrain.columns), usize::from(terrain.rows));
    // Parry's heightfield is column-major with rows along Z: entry (row, column) at row + column × rows.
    let mut data = vec![0.0; columns * rows];
    for row in 0..rows {
        for column in 0..columns {
            #[allow(clippy::cast_precision_loss)]
            let height = terrain.heights[row * columns + column] as f32 / UNITS_PER_METRE;
            data[row + column * rows] = height;
        }
    }
    #[allow(clippy::cast_precision_loss)]
    let extent = |count: usize| (count - 1) as f32 * terrain.step as f32;
    let (width, depth) = (extent(columns), extent(rows));
    #[allow(clippy::cast_precision_loss)]
    let centre = [
        terrain.origin[0] as f32 + width / 2.0,
        0.0,
        terrain.origin[1] as f32 + depth / 2.0,
    ];
    let mut field = rapier3d::parry::shape::HeightField::new(
        rapier3d::parry::utils::Array2::new(rows, columns, data),
        Vector::new(width / UNITS_PER_METRE, 1.0, depth / UNITS_PER_METRE),
    );
    // Flat cells are left to the floor half-space, whose exact queries keep resting bodies
    // still; parry's heightfield contacts let them sink by fractions of a unit (#159).
    for row in 0..rows - 1 {
        for column in 0..columns - 1 {
            let flat = [
                (row, column),
                (row + 1, column),
                (row, column + 1),
                (row + 1, column + 1),
            ]
            .iter()
            .all(|&(r, c)| terrain.heights[r * columns + c] == 0);
            if flat {
                field.set_cell_status(
                    row,
                    column,
                    rapier3d::parry::shape::HeightFieldCellStatus::CELL_REMOVED,
                );
            }
        }
    }
    world.colliders.insert(
        ColliderBuilder::new(SharedShape::new(field))
            .translation(metres(centre))
            .user_data(u128::from(id.0))
            .build(),
    )
}

impl World {
    /// An empty world; `gravity` is in units per tick squared.
    #[must_use]
    pub fn new(gravity: Vec3i) -> Self {
        Self {
            gravity: exact(gravity),
            ground: None,
            world: empty_world(),
            queries: UprightShapes.chain(DefaultQueryDispatcher),
            controller: KinematicCharacterController {
                up: Vector::Y,
                offset: CharacterLength::Absolute(CONTACT_OFFSET),
                slide: true,
                // No stairs or ledges exist yet (#59 adds slopes and steps).
                autostep: None,
                max_slope_climb_angle: std::f32::consts::FRAC_PI_4,
                min_slope_slide_angle: std::f32::consts::FRAC_PI_4,
                snap_to_ground: Some(CharacterLength::Absolute(SUPPORT_REACH)),
                normal_nudge_factor: KinematicCharacterController::default().normal_nudge_factor,
            },
            bodies: BTreeMap::new(),
            handles: BTreeMap::new(),
        }
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
        let handles = insert(&mut self.world, &body);
        self.handles.insert(body.id, handles);
        self.bodies.insert(body.id, body);
        self.refresh();
        Ok(())
    }

    /// Replaces an existing body, for example a door leaf moving to its other box.
    pub fn replace_body(&mut self, body: RigidBody) -> Result<(), PhysicsError> {
        if self.remove_body(body.id).is_none() {
            return Err(error(format!("physics body {} is missing", body.id.0)));
        }
        self.add_body(body)
    }

    pub fn remove_body(&mut self, id: BodyId) -> Option<RigidBody> {
        let body = self.bodies.remove(&id)?;
        if let Some(handles) = self.handles.remove(&id) {
            match handles.body {
                Some(handle) => {
                    self.world.remove_body(handle);
                }
                None => {
                    self.world.remove_collider(handles.collider);
                }
            }
        }
        self.refresh();
        Some(body)
    }

    #[must_use]
    pub fn body(&self, id: BodyId) -> Option<&RigidBody> {
        self.bodies.get(&id)
    }

    /// Places a moving body at `position` without resolving contact: placement by the zone
    /// (respawns, evade resets, recovery), never movement.
    pub fn set_position(&mut self, id: BodyId, position: Vec3i) -> Result<(), PhysicsError> {
        self.place(id, exact(position))
    }

    fn place(&mut self, id: BodyId, position: [f32; 3]) -> Result<(), PhysicsError> {
        let body = self
            .bodies
            .get_mut(&id)
            .filter(|body| body.is_moving())
            .ok_or_else(|| error(format!("moving physics body {} is missing", id.0)))?;
        if body.position == position {
            return Ok(());
        }
        body.position = position;
        let handle = self.handles.get(&id).and_then(|handles| handles.body);
        if let Some(rapier) = handle.and_then(|handle| self.world.bodies.get_mut(handle)) {
            rapier.set_translation(lifted(position), false);
        }
        self.refresh();
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

    /// Rebuilds the Rapier world from the bodies in ID order. Rapier's broad phase depends on
    /// the history of insertions, removals and moves, and the controller breaks exact ties
    /// between equally near contacts by its traversal order; rebuilding first makes each
    /// step a function of zone state alone.
    fn rebuild(&mut self) {
        let mut world = empty_world();
        self.handles = self
            .bodies
            .values()
            .map(|body| (body.id, insert(&mut world, body)))
            .collect();
        if let Some(ground) = self.ground.as_mut() {
            ground.collider = Some(insert_ground(&mut world, ground.id, &ground.terrain));
        }
        self.world = world;
        self.refresh();
    }

    /// Lays the authored ground as a heightfield collider identified as `id` (#159).
    pub fn set_terrain(&mut self, id: BodyId, terrain: crate::Terrain) {
        if let Some(handle) = self.ground.as_ref().and_then(|ground| ground.collider) {
            self.world.remove_collider(handle);
        }
        let collider = insert_ground(&mut self.world, id, &terrain);
        self.ground = Some(Ground {
            id,
            terrain,
            collider: Some(collider),
        });
        self.refresh();
    }

    /// The height (units) of the authored ground straight below or above `(x, z)`, if any.
    #[must_use]
    pub fn terrain_height(&self, x: i32, z: i32) -> Option<f32> {
        let handle = self.ground.as_ref()?.collider?;
        let predicate =
            |collider: ColliderHandle, _: &rapier3d::prelude::Collider| collider == handle;
        let queries = self.world.broad_phase.as_query_pipeline(
            &self.queries,
            &self.world.bodies,
            &self.world.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        #[allow(clippy::cast_precision_loss)]
        let top = crate::MAX_TERRAIN_HEIGHT_UNITS as f32 + 100.0;
        // A ray straight down a heightfield's grid line can slip between its cells' triangles;
        // a hundredth of a unit aside it always meets one.
        let ray = RapierRay::new(metres([x as f32 + 0.01, top, z as f32 + 0.01]), -Vector::Y);
        let (_, time_of_impact) = queries.cast_ray(&ray, 2.0 * top / UNITS_PER_METRE, true)?;
        Some(top - time_of_impact * UNITS_PER_METRE)
    }

    /// Advances one tick: each moving body, in ID order, gains gravity and asks the character
    /// controller where its velocity leads. Landing on ground or meeting a ceiling ends its
    /// vertical velocity.
    pub fn step(&mut self) -> Result<StepStats, PhysicsError> {
        self.rebuild();
        let ids: Vec<_> = self
            .bodies
            .values()
            .filter(|body| body.is_moving())
            .map(|body| body.id)
            .collect();
        let mut moved_bodies = 0;
        let moving_count = ids.len();
        for id in ids {
            let body = *self
                .bodies
                .get(&id)
                .ok_or_else(|| error("moving body vanished"))?;
            let mut velocity = body.velocity;
            // A body on the ground stays on it unless it jumps: gravity would only press it
            // into the ground, which the controller answers with a nudge back up. A body just
            // above the ground settles onto it.
            let mut settle = 0.0;
            match self.ground_gap(&body, body.position) {
                Some(gap) if velocity[1] <= 0.0 => {
                    velocity[1] = 0.0;
                    if gap > GAP_TOLERANCE_UNITS {
                        settle = -gap;
                    }
                }
                _ => {
                    for (axis, gravity) in velocity.iter_mut().zip(self.gravity) {
                        *axis += gravity;
                    }
                }
            }
            let desired = [velocity[0], velocity[1] + settle, velocity[2]];
            // A body with nowhere to go stays exactly where it is: asked to move nowhere, the
            // controller still snaps and nudges it by fractions of a unit.
            if desired == [0.0; 3] {
                self.bodies
                    .get_mut(&id)
                    .ok_or_else(|| error("moving body vanished"))?
                    .velocity = velocity;
                continue;
            }
            let desired = self.slide_around(&body, desired);
            let (moved, grounded) = self.move_shape(&body, desired)?;
            let to = [
                body.position[0] + moved[0],
                body.position[1] + moved[1],
                body.position[2] + moved[2],
            ];
            let supported = grounded && self.ground_below(&body, to);
            if (supported && velocity[1] <= 0.0)
                || (velocity[1] > 0.0 && moved[1] < velocity[1] - 1.0e-3)
            {
                velocity[1] = 0.0;
            }
            if moved != [0.0; 3] {
                moved_bodies += 1;
            }
            // The horizontal velocity is what contact let the body realize this tick.
            velocity[0] = moved[0];
            velocity[2] = moved[2];
            self.place(id, to)?;
            self.bodies
                .get_mut(&id)
                .ok_or_else(|| error("moving body vanished"))?
                .velocity = velocity;
        }
        Ok(StepStats {
            moving: moving_count,
            moved_bodies,
            bodies: self.bodies.len(),
        })
    }

    /// `desired` with the horizontal part that presses into a touching moving capsule turned
    /// along it, so characters slide around each other instead of locking head-on: Rapier's
    /// controller only stops a kinematic body at another (#273; physics-engine #308 will do
    /// this natively). Exactly head-on, both step aside to opposite sides at full speed.
    fn slide_around(&self, body: &RigidBody, desired: [f32; 3]) -> [f32; 3] {
        let [mut vx, vy, mut vz] = desired;
        let speed = vx.hypot(vz);
        if speed == 0.0 {
            return desired;
        }
        #[allow(clippy::cast_precision_loss)]
        let radius = |half: Vec3i| half.x.max(half.z) as f32;
        #[allow(clippy::cast_precision_loss)]
        let (low, high) = (
            body.position[1] - body.half_extents.y as f32,
            body.position[1] + body.half_extents.y as f32,
        );
        for other in self.bodies.values() {
            if other.id == body.id || !other.is_moving() {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let other_half_y = other.half_extents.y as f32;
            if other.position[1] + other_half_y < low || other.position[1] - other_half_y > high {
                continue;
            }
            let (dx, dz) = (
                body.position[0] - other.position[0],
                body.position[2] - other.position[2],
            );
            let distance = dx.hypot(dz);
            let reach =
                radius(body.half_extents) + radius(other.half_extents) + CONTACT_OFFSET_UNITS + 1.0;
            if distance == 0.0 || distance > reach {
                continue;
            }
            // `n` points from the other body to this one; only motion against it is turned.
            let (nx, nz) = (dx / distance, dz / distance);
            let into = vx * nx + vz * nz;
            if into >= 0.0 {
                continue;
            }
            vx -= into * nx;
            vz -= into * nz;
            if vx.hypot(vz) < 0.25 * speed {
                // The tangent (-nz, nx): mirrored for the other body, so two characters
                // meeting exactly head-on step to opposite sides.
                vx = -nz * speed;
                vz = nx * speed;
            }
        }
        [vx, vy, vz]
    }

    /// The translation (units) the controller applies to `body` for `desired`, and whether
    /// it ends grounded.
    fn move_shape(
        &self,
        body: &RigidBody,
        desired: [f32; 3],
    ) -> Result<([f32; 3], bool), PhysicsError> {
        let handle = self
            .handles
            .get(&body.id)
            .and_then(|handles| handles.body)
            .ok_or_else(|| error("moving body has no Rapier body"))?;
        let queries = self.world.broad_phase.as_query_pipeline(
            &self.queries,
            &self.world.bodies,
            &self.world.colliders,
            QueryFilter::default().exclude_rigid_body(handle),
        );
        let movement = self.controller.move_shape(
            self.world.integration_parameters.dt,
            &queries,
            &capsule(body.half_extents),
            &Pose::from_translation(lifted(body.position)),
            metres(desired),
            |_| {},
        );
        Ok((units(movement.translation), movement.grounded))
    }

    /// Whether static ground lies within the support reach below `body` placed at `position`;
    /// another unit's body is no ground.
    fn ground_below(&self, body: &RigidBody, position: [f32; 3]) -> bool {
        self.ground_gap(body, position).is_some()
    }

    /// How far (units) static ground within the support reach lies below `body` at
    /// `position`, beyond the controller's contact offset.
    fn ground_gap(&self, body: &RigidBody, position: [f32; 3]) -> Option<f32> {
        let queries = self.world.broad_phase.as_query_pipeline(
            &self.queries,
            &self.world.bodies,
            &self.world.colliders,
            QueryFilter::only_fixed(),
        );
        queries
            .cast_shape(
                &Pose::from_translation(lifted(position)),
                -Vector::Y,
                &capsule(body.half_extents),
                ShapeCastOptions {
                    target_distance: CONTACT_OFFSET,
                    stop_at_penetration: false,
                    max_time_of_impact: SUPPORT_REACH,
                    compute_impact_geometry_on_penetration: true,
                },
            )
            .filter(|(_, hit)| {
                hit.normal1.dot(Vector::Y) >= self.controller.max_slope_climb_angle.cos()
            })
            .map(|(_, hit)| hit.time_of_impact * UNITS_PER_METRE)
    }

    /// Whether a moving body stands on static ground.
    #[must_use]
    pub fn grounded(&self, id: BodyId) -> bool {
        self.bodies
            .get(&id)
            .filter(|body| body.is_moving())
            .is_some_and(|body| self.ground_below(body, body.position))
    }

    /// Every body whose shape intersects the box, in ID order.
    pub fn overlap_query(&self, aabb: Aabb) -> Result<Vec<BodyId>, PhysicsError> {
        let queries = self.world.broad_phase.as_query_pipeline(
            &self.queries,
            &self.world.bodies,
            &self.world.colliders,
            QueryFilter::default(),
        );
        let shape = cuboid(aabb.half_extents);
        let mut hits: Vec<_> = queries
            .intersect_shape(Pose::from_translation(metres(exact(aabb.center))), &shape)
            .map(|(_, collider)| BodyId(u64::try_from(collider.user_data).unwrap_or(u64::MAX)))
            .collect();
        hits.sort_unstable();
        hits.dedup();
        Ok(hits)
    }

    /// Whether a body accepted by `blocks` lies on the segment from `origin` along
    /// `translation` (units), strictly before its end.
    pub fn segment_blocked(
        &self,
        origin: Vec3i,
        translation: Vec3i,
        blocks: impl Fn(BodyId) -> bool,
    ) -> bool {
        if translation == Vec3i::ZERO {
            return false;
        }
        let predicate = |_, collider: &rapier3d::prelude::Collider| {
            blocks(BodyId(
                u64::try_from(collider.user_data).unwrap_or(u64::MAX),
            ))
        };
        let queries = self.world.broad_phase.as_query_pipeline(
            &self.queries,
            &self.world.bodies,
            &self.world.colliders,
            QueryFilter::default().predicate(&predicate),
        );
        queries
            .cast_ray(
                &RapierRay::new(metres(exact(origin)), metres(exact(translation))),
                1.0,
                true,
            )
            .is_some_and(|(_, time_of_impact)| time_of_impact < 1.0)
    }

    /// Brings Rapier's broad phase (the query structure) up to date.
    fn refresh(&mut self) {
        self.world.detect_collisions(&(), &());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Vec3i = Vec3i::new(30, 90, 30);

    fn world() -> World {
        let mut world = World::new(Vec3i::new(0, -1, 0));
        world.add_body(RigidBody::floor(BodyId(1), 0)).unwrap();
        world
    }

    #[test]
    fn a_unit_falls_lands_and_walks_into_a_wall() {
        let mut world = world();
        world
            .add_body(RigidBody::fixed(
                BodyId(2),
                Vec3i::new(300, 100, 0),
                Vec3i::new(50, 100, 500),
            ))
            .unwrap();
        let unit = BodyId(10);
        world
            .add_body(RigidBody::dynamic(
                unit,
                Vec3i::new(0, 200, 0),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        for _ in 0..60 {
            world.step().unwrap();
        }
        let body = *world.body(unit).unwrap();
        assert_eq!(
            body.position(),
            Vec3i::new(0, 90, 0),
            "landed with feet on y = 0"
        );
        assert!(
            (body.exact_position()[1] - 90.0).abs() < 1.0e-3,
            "{:?}",
            body.exact_position()
        );
        world.step().unwrap();
        assert_eq!(
            world.body(unit).unwrap().exact_position(),
            body.exact_position(),
            "rests exactly"
        );
        assert_eq!(body.exact_velocity()[1], 0.0);
        assert!(world.grounded(unit));
        world.set_velocity(unit, Vec3i::new(10, 0, 0)).unwrap();
        for _ in 0..60 {
            world.step().unwrap();
        }
        let x = world.body(unit).unwrap().exact_position()[0];
        assert!((219.0..=220.0).contains(&x), "stopped at the wall: {x}");
        assert_eq!(world.body(unit).unwrap().position().y, 90);
    }

    #[test]
    fn characters_meeting_head_on_slide_around_each_other() {
        let mut world = world();
        world
            .add_body(RigidBody::dynamic(
                BodyId(10),
                Vec3i::new(-200, 90, 0),
                Vec3i::new(10, 0, 0),
                BODY,
            ))
            .unwrap();
        world
            .add_body(RigidBody::dynamic(
                BodyId(11),
                Vec3i::new(200, 90, 0),
                Vec3i::new(-10, 0, 0),
                BODY,
            ))
            .unwrap();
        for _ in 0..60 {
            for (id, vx) in [(10, 10), (11, -10)] {
                world
                    .set_velocity(BodyId(id), Vec3i::new(vx, 0, 0))
                    .unwrap();
            }
            world.step().unwrap();
        }
        let (a, b) = (
            world.body(BodyId(10)).unwrap().position(),
            world.body(BodyId(11)).unwrap().position(),
        );
        assert!(
            a.x > 200 && b.x < -200,
            "they passed each other: {a:?} {b:?}"
        );
    }

    #[test]
    fn queries_see_bodies_and_rebuilt_worlds_continue_exactly() {
        let mut world = world();
        world
            .add_body(RigidBody::dynamic(
                BodyId(10),
                Vec3i::new(0, 90, 0),
                Vec3i::new(3, 0, 1),
                BODY,
            ))
            .unwrap();
        world
            .add_body(RigidBody::dynamic(
                BodyId(11),
                Vec3i::new(100, 90, 0),
                Vec3i::new(-3, 0, 0),
                BODY,
            ))
            .unwrap();
        assert_eq!(
            world
                .overlap_query(Aabb::new(Vec3i::new(100, 90, 0), Vec3i::new(5, 5, 5)))
                .unwrap(),
            [BodyId(11)]
        );
        assert!(
            world.segment_blocked(Vec3i::new(-100, 90, 0), Vec3i::new(300, 0, 0), |id| id.0
                >= 10)
        );
        assert!(
            !world.segment_blocked(Vec3i::new(-100, 90, 0), Vec3i::new(300, 0, 0), |id| id.0
                < 10)
        );
        for _ in 0..10 {
            world.step().unwrap();
        }
        let mut copy = world.clone();
        for _ in 0..30 {
            world.step().unwrap();
            copy.step().unwrap();
        }
        assert_eq!(world.body(BodyId(10)), copy.body(BodyId(10)));
        assert_eq!(world.body(BodyId(11)), copy.body(BodyId(11)));
    }
}
