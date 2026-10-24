//! The Rapier backend: the production authority (#271, Atlas 2.1.6).
//!
//! Players and creatures are kinematic position-based bodies with upright capsule colliders,
//! moved by Rapier's `KinematicCharacterController`, which resolves contact, sliding and
//! support. Rapier simulates in metres, the scale its tolerances and parry's epsilons assume.
//! Nothing of Rapier's own state is saved: recovery rebuilds this world from the bodies the
//! game restores, and every step rebuilds it in body-ID order first, so a restored zone
//! continues exactly.
//!
//! The controller's contact queries go through [`super::queries`], exact closed forms for the
//! upright capsules and boxes the zone uses, ahead of parry's general GJK/EPA path.

use std::collections::BTreeMap;
use std::fmt;

use rapier3d::control::{CharacterLength, KinematicCharacterController};
use rapier3d::parry::query::{
    DefaultQueryDispatcher, QueryDispatcher, QueryDispatcherChain, ShapeCastOptions,
};
use rapier3d::prelude::{
    Capsule, ColliderBuilder, ColliderHandle, Cuboid, PhysicsWorld, Pose, QueryFilter,
    Ray as RapierRay, RigidBodyBuilder, RigidBodyHandle, Rotation, SharedShape, Vector,
};

use super::queries::UprightShapes;
use super::{
    Aabb, BodyId, CONTACT_OFFSET_UNITS, DebrisBody, Kind, PhysicsError, RayHit, RigidBody,
    StepStats, Vec3i, error, exact, from_subunits,
};

/// Centimetre units per metre.
const UNITS_PER_METRE: f32 = 100.0;
/// [`CONTACT_OFFSET_UNITS`] in metres.
const CONTACT_OFFSET: f32 = CONTACT_OFFSET_UNITS / UNITS_PER_METRE;
/// Approaches within 60 degrees of head-on step fully aside in [`World::slide_around`].
const NEAR_HEAD_ON_COS: f32 = 0.5;
/// The outward motion (units per tick: 30 units/s) [`World::slide_around`] adds against a
/// touching body.
const SEPARATION_UNITS: f32 = 30.0 / crate::TICK_HZ as f32;
/// How far (in metres) below a body the controller still finds the ground: its contact
/// offset plus the fixed 5 cm its grounded test predicts. Support queries and snapping use
/// the same reach.
const SUPPORT_REACH: f32 = CONTACT_OFFSET + 0.05;
/// Ray hits this close (units) along the ray tie, broken by body ID (#57).
const TIE_UNITS: f32 = 1.0e-3;
/// At most this many too-steep surfaces are skipped looking for ground below a body.
const MAX_STEEP_SUPPORTS: usize = 4;
/// A ground gap (units) below this counts as standing on it.
const GAP_TOLERANCE_UNITS: f32 = 1.0e-3;

/// A piece slower than this (units per tick: 18 units/s) and turning slower than
/// [`DEBRIS_REST_SPIN`] (radians per tick: 0.9 rad/s) settles.
pub(super) const DEBRIS_REST_SPEED_UNITS: f32 = 18.0 / crate::TICK_HZ as f32;
pub(super) const DEBRIS_REST_SPIN: f32 = 0.9 / crate::TICK_HZ as f32;
/// Fixed geometry this far (units) around the pieces takes part in a debris step.
pub(super) const DEBRIS_REACH_UNITS: f32 = 1_000.0;
pub(super) const TICKS_PER_SECOND: f32 = crate::TICK_HZ as f32;

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

pub(super) struct World {
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
        let mut world = Self::new(Vec3i::ZERO);
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
    world.integration_parameters.dt = 1.0 / TICKS_PER_SECOND;
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

/// A ramp's wedge (#59) in metres around its box centre: the box's bottom rectangle and its
/// top edge at the high end, as a convex hull.
fn wedge(half_extents: Vec3i, rises: crate::RampRise) -> SharedShape {
    let [hx, hy, hz] = exact(half_extents).map(|value| value / UNITS_PER_METRE);
    let [dx, dz] = rises.direction();
    let mut points = Vec::with_capacity(6);
    for sx in [-1.0, 1.0] {
        for sz in [-1.0, 1.0] {
            points.push(Vector::new(sx * hx, -hy, sz * hz));
            // The top edge lies over the high end of the bottom.
            #[allow(clippy::cast_precision_loss)]
            let high = (dx != 0 && sx == dx as f32) || (dz != 0 && sz == dz as f32);
            if high {
                points.push(Vector::new(sx * hx, hy, sz * hz));
            }
        }
    }
    SharedShape::convex_hull(&points).expect("a ramp's box spans a solid wedge")
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
    } else if let Kind::Ramp(rises) = body.kind {
        let collider = world.colliders.insert(
            ColliderBuilder::new(wedge(body.half_extents, rises))
                .translation(metres(body.position))
                .user_data(user_data)
                .build(),
        );
        Handles {
            body: None,
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
    // still; parry's heightfield contacts let them sink by fractions of a unit (#159). Flat cells
    // next to a basin stay: a body straddling its edge has the floor opened under it (#274).
    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let cell_centre =
        |index: usize, origin: i32| origin + index as i32 * terrain.step + terrain.step / 2;
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
            let by_basin = || {
                let (x, z) = (
                    cell_centre(column, terrain.origin[0]),
                    cell_centre(row, terrain.origin[1]),
                );
                terrain.low_within(x, z, terrain.step) < 0
            };
            if flat && !by_basin() {
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
    /// An empty world; `gravity` is in subunits per tick squared.
    #[must_use]
    pub fn new(gravity: Vec3i) -> Self {
        Self {
            gravity: from_subunits(gravity),
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

    /// The platform `id` (#56), mutably.
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
        platform.velocity = [0.0; 3];
        let by = [0, 1, 2].map(|axis| exact(to)[axis] - platform.position[axis]);
        self.shift_platform(id, by)
    }

    /// Moves platform `id`'s box by `by` (units) and its collider with it.
    fn shift_platform(&mut self, id: BodyId, by: [f32; 3]) -> Result<(), PhysicsError> {
        if by == [0.0; 3] {
            return Ok(());
        }
        let platform = self.platform_mut(id)?;
        for (axis, delta) in platform.position.iter_mut().zip(by) {
            *axis += delta;
        }
        let position = platform.position;
        if let Some(collider) = self
            .handles
            .get(&id)
            .and_then(|handles| self.world.colliders.get_mut(handles.collider))
        {
            collider.set_translation(metres(position));
        }
        self.refresh();
        Ok(())
    }

    /// Whether `body` stands on platform `platform`: its walkable top within the support
    /// reach below.
    fn stands_on(&self, body: &RigidBody, platform: BodyId) -> bool {
        let Some(collider) = self.handles.get(&platform).map(|handles| handles.collider) else {
            return false;
        };
        let predicate =
            |handle: ColliderHandle, _: &rapier3d::prelude::Collider| handle == collider;
        self.support_gap(
            body,
            body.position,
            QueryFilter::default().predicate(&predicate),
        )
        .is_some()
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

    pub fn set_swimming(&mut self, id: BodyId, swimming: bool) -> Result<(), PhysicsError> {
        let body = self
            .bodies
            .get_mut(&id)
            .filter(|body| body.is_moving())
            .ok_or_else(|| error(format!("moving physics body {} is missing", id.0)))?;
        body.swimming = swimming;
        Ok(())
    }

    /// The floor half-space's collider, when `body` at `position` stands over terrain that dips
    /// below it (a lake basin, #274): there only the heightfield is ground. Every grid cell
    /// under the body's footprint counts, since the triangulated heightfield may dip below zero
    /// anywhere in a cell with a sample below it; elsewhere it never dips under the floor.
    fn open_floor(&self, body: &RigidBody, position: [f32; 3]) -> Option<ColliderHandle> {
        let ground = self.ground.as_ref()?;
        #[allow(clippy::cast_possible_truncation)]
        let (x, z) = (position[0].round() as i32, position[2].round() as i32);
        let reach = body.half_extents.x.max(body.half_extents.z) + 1;
        if ground.terrain.low_within(x, z, reach) >= 0 {
            return None;
        }
        self.handles.get(&ground.id).map(|handles| handles.collider)
    }

    fn without_open_floor(
        filter: QueryFilter<'_>,
        floor: Option<ColliderHandle>,
    ) -> QueryFilter<'_> {
        match floor {
            Some(floor) => filter.exclude_collider(floor),
            None => filter,
        }
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
    ///
    /// Moving platforms (#56) carry the units standing on them that are not rising: a
    /// platform's displacement is added to each rider's move. A platform that does not rise
    /// moves before the units and a rising one after them, so neither runs into its riders.
    pub fn step(&mut self) -> Result<StepStats, PhysicsError> {
        self.rebuild();
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
            match (!body.swimming).then(|| self.ground_gap(&body, body.position)) {
                // A swimmer (#274) keeps the vertical velocity the zone gave it.
                None => {}
                Some(Some(gap)) if velocity[1] <= 0.0 => {
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
            // A rider follows its platform instead of settling onto it: the platform that
            // sank before the units already opened the gap the carry closes.
            let carry = carried.get(&id).copied().unwrap_or([0.0; 3]);
            if carried.contains_key(&id) {
                settle = 0.0;
            }
            let desired = [
                velocity[0] + carry[0],
                velocity[1] + settle + carry[1],
                velocity[2] + carry[2],
            ];
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
            // The horizontal velocity is what contact let the body realize this tick, less
            // what its platform carried.
            velocity[0] = moved[0] - carry[0];
            velocity[2] = moved[2] - carry[2];
            self.place(id, to)?;
            self.bodies
                .get_mut(&id)
                .ok_or_else(|| error("moving body vanished"))?
                .velocity = velocity;
        }
        for &(platform, displacement) in &platforms {
            if displacement[1] > 0.0 {
                self.shift_platform(platform, displacement)?;
            }
            self.platform_mut(platform)?.velocity = [0.0; 3];
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
    /// this natively). Within 60 degrees of head-on, both step aside to opposite sides at full
    /// speed.
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
            // The tangent (-nz, nx) is mirrored for the other body, whose `n` is opposite.
            let (tx, tz) = (-nz, nx);
            let along = vx * tx + vz * tz;
            let along = if into < -NEAR_HEAD_ON_COS * speed {
                // Near head-on, projecting would leave a crawl: step aside at full speed, to
                // the side the motion already leans or, exactly head-on, the tangent's side
                // (opposite for the two bodies, so they never pick the same one).
                let side = if along.abs() > 1.0e-3 * speed {
                    along.signum()
                } else {
                    1.0
                };
                side * speed
            } else {
                along
            };
            // A little outward motion too: against a touching capsule Rapier's controller
            // otherwise stops even motion along it.
            vx = along * tx + SEPARATION_UNITS * nx;
            vz = along * tz + SEPARATION_UNITS * nz;
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
            Self::without_open_floor(
                QueryFilter::default().exclude_rigid_body(handle),
                self.open_floor(body, body.position),
            ),
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
        self.support_gap(
            body,
            position,
            Self::without_open_floor(QueryFilter::only_fixed(), self.open_floor(body, position)),
        )
    }

    /// [`Self::ground_gap`] against the colliders `filter` admits.
    fn support_gap(
        &self,
        body: &RigidBody,
        position: [f32; 3],
        filter: QueryFilter<'_>,
    ) -> Option<f32> {
        // A too-steep surface the cast meets first (a chute's side at its foot) is no support,
        // but it must not hide walkable ground just below it: skip it and cast again.
        let mut steep: Vec<ColliderHandle> = Vec::new();
        loop {
            let admitted = |handle: ColliderHandle, collider: &rapier3d::prelude::Collider| {
                !steep.contains(&handle)
                    && filter
                        .predicate
                        .is_none_or(|predicate| predicate(handle, collider))
            };
            let queries = self.world.broad_phase.as_query_pipeline(
                &self.queries,
                &self.world.bodies,
                &self.world.colliders,
                QueryFilter {
                    predicate: Some(&admitted),
                    ..filter
                },
            );
            let (handle, hit) = queries.cast_shape(
                &Pose::from_translation(lifted(position)),
                -Vector::Y,
                &capsule(body.half_extents),
                ShapeCastOptions {
                    target_distance: CONTACT_OFFSET,
                    stop_at_penetration: false,
                    max_time_of_impact: SUPPORT_REACH,
                    compute_impact_geometry_on_penetration: true,
                },
            )?;
            if hit.normal1.dot(Vector::Y) >= self.controller.max_slope_climb_angle.cos() {
                return Some(hit.time_of_impact * UNITS_PER_METRE);
            }
            if steep.len() >= MAX_STEEP_SUPPORTS {
                return None;
            }
            steep.push(handle);
        }
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
    ///
    /// Rapier's `intersect_shape` silently skips cuboid–heightfield pairs (parry reports them
    /// unsupported), so the terrain never shows up here. Door probes (`door::leaf_blocked`)
    /// only look for units, so that is harmless for now.
    pub fn overlap_query(&self, aabb: Aabb) -> Result<Vec<BodyId>, PhysicsError> {
        self.overlap_box(exact(aabb.center), exact(aabb.half_extents))
    }

    pub fn overlap_box(
        &self,
        centre: [f32; 3],
        half_extents: [f32; 3],
    ) -> Result<Vec<BodyId>, PhysicsError> {
        let queries = self.world.broad_phase.as_query_pipeline(
            &self.queries,
            &self.world.bodies,
            &self.world.colliders,
            QueryFilter::default(),
        );
        let shape = Cuboid::new(metres(half_extents));
        let mut hits: Vec<_> = queries
            .intersect_shape(Pose::from_translation(metres(centre)), &shape)
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

    /// The first body accepted by `hits` on the segment from `origin` along `translation`
    /// (units), with the hit's fraction of the segment and its surface normal (#286). Rapier
    /// rays see every collider kind, terrain heightfield included.
    pub fn cast_ray(
        &self,
        origin: [f32; 3],
        translation: [f32; 3],
        hits: impl Fn(BodyId) -> bool,
    ) -> Option<RayHit> {
        if translation == [0.0; 3] {
            return None;
        }
        let body = |collider: &rapier3d::prelude::Collider| {
            BodyId(u64::try_from(collider.user_data).unwrap_or(u64::MAX))
        };
        let ray = RapierRay::new(metres(origin), metres(translation));
        // The first accepted hit below `below` (an ID), within `reach` of the ray.
        let first = |below: Option<BodyId>, reach: f32| {
            let predicate = |_, collider: &rapier3d::prelude::Collider| {
                let id = body(collider);
                below.is_none_or(|below| id < below) && hits(id)
            };
            let queries = self.world.broad_phase.as_query_pipeline(
                &self.queries,
                &self.world.bodies,
                &self.world.colliders,
                QueryFilter::default().predicate(&predicate),
            );
            let (handle, hit) = queries.cast_ray_and_get_normal(&ray, reach, true)?;
            Some((body(self.world.colliders.get(handle)?), hit))
        };
        // Rapier breaks equally near hits by its tree order; the game breaks them by body ID
        // (#57), like physics-engine: look for a lower ID at the same distance (within a
        // thousandth of a unit) until none.
        let length = translation
            .iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt();
        let tie = TIE_UNITS / length;
        let (mut id, mut hit) = first(None, 1.0)?;
        while let Some((lower, tied)) = first(Some(id), hit.time_of_impact + tie) {
            (id, hit) = (lower, tied);
        }
        Some(RayHit {
            body: id,
            fraction: hit.time_of_impact,
            normal: [hit.normal.x, hit.normal.y, hit.normal.z],
        })
    }

    pub fn cast_shape(
        &self,
        origin: [f32; 3],
        half_extents: Vec3i,
        translation: [f32; 3],
        hits: impl Fn(BodyId) -> bool,
    ) -> Option<RayHit> {
        if translation == [0.0; 3] {
            return None;
        }
        let body = |collider: &rapier3d::prelude::Collider| {
            BodyId(u64::try_from(collider.user_data).unwrap_or(u64::MAX))
        };
        let shape = capsule(half_extents);
        let pose = Pose::from_translation(metres(origin));
        let velocity = metres(translation);
        // The first accepted hit below `below` (an ID), within `reach` of the sweep.
        let first = |below: Option<BodyId>, reach: f32| {
            let predicate = |_, collider: &rapier3d::prelude::Collider| {
                let id = body(collider);
                below.is_none_or(|below| id < below) && hits(id)
            };
            let queries = self.world.broad_phase.as_query_pipeline(
                &self.queries,
                &self.world.bodies,
                &self.world.colliders,
                QueryFilter::default().predicate(&predicate),
            );
            let (handle, hit) = queries.cast_shape(
                &pose,
                velocity,
                &shape,
                ShapeCastOptions {
                    target_distance: 0.0,
                    stop_at_penetration: true,
                    max_time_of_impact: reach,
                    compute_impact_geometry_on_penetration: true,
                },
            )?;
            Some((body(self.world.colliders.get(handle)?), hit))
        };
        // Ties within a thousandth of a unit go to the lower body ID, as for rays.
        let length = translation
            .iter()
            .map(|axis| axis * axis)
            .sum::<f32>()
            .sqrt();
        let tie = TIE_UNITS / length;
        let (mut id, mut hit) = first(None, 1.0)?;
        while let Some((lower, tied)) = first(Some(id), hit.time_of_impact + tie) {
            (id, hit) = (lower, tied);
        }
        // `normal2` is the target's outward normal at the contact, in world space.
        Some(RayHit {
            body: id,
            fraction: hit.time_of_impact,
            normal: [hit.normal2.x, hit.normal2.y, hit.normal2.z],
        })
    }

    /// Advances `debris` (#288) one tick: Rapier steps a world of the pieces still moving as
    /// dynamic bodies, the settled ones and the fixed geometry around them, rebuilt from the
    /// game's state in slice order like every step here. A piece that slows below the rest
    /// thresholds settles once `settle` allows it. Returns how many pieces moved.
    ///
    /// Rapier gap (#288): rebuilding each tick drops the solver's warm-start contact impulses,
    /// so a resting stack jitters; pieces settle into fixed obstacles instead of sleeping.
    pub fn step_debris(&self, debris: &mut [DebrisBody], settle: &[bool]) -> usize {
        let ground = self
            .ground
            .as_ref()
            .map(|ground| (ground.id, &ground.terrain));
        step_debris(self.gravity, &self.bodies, ground, debris, settle)
    }

    pub fn step_props(&self, props: &mut [DebrisBody], exclude: &[BodyId]) -> usize {
        let ground = self
            .ground
            .as_ref()
            .map(|ground| (ground.id, &ground.terrain));
        step_props(self.gravity, &self.bodies, ground, props, exclude)
    }

    /// Brings Rapier's broad phase (the query structure) up to date.
    fn refresh(&mut self) {
        self.world.detect_collisions(&(), &());
    }
}

/// [`World::step_debris`] over the game's fixed `bodies` and `ground`.
fn step_debris(
    gravity: [f32; 3],
    bodies: &BTreeMap<BodyId, RigidBody>,
    ground: Option<(BodyId, &crate::Terrain)>,
    debris: &mut [DebrisBody],
    settle: &[bool],
) -> usize {
    step_dynamic(gravity, bodies, ground, debris, settle, &Dynamic::default())
}

/// How [`step_dynamic`] treats its pieces.
#[derive(Default)]
pub(super) struct Dynamic<'a> {
    /// Pieces keep upright: they slide but never turn (movable props, #58).
    pub upright: bool,
    /// Units' capsules stop the pieces (movable props), instead of letting them pass (debris).
    pub units_block: bool,
    /// Bodies left out of the step: the movable props' own fixed boxes.
    pub exclude: &'a [BodyId],
}

/// [`World::step_props`] (#58): movable props slide upright under gravity and impulses,
/// stopped by the fixed world and by units.
fn step_props(
    gravity: [f32; 3],
    bodies: &BTreeMap<BodyId, RigidBody>,
    ground: Option<(BodyId, &crate::Terrain)>,
    props: &mut [DebrisBody],
    exclude: &[BodyId],
) -> usize {
    let settle = vec![true; props.len()];
    step_dynamic(
        gravity,
        bodies,
        ground,
        props,
        &settle,
        &Dynamic {
            upright: true,
            units_block: true,
            exclude,
        },
    )
}

/// The game bodies a dynamic step takes part with: the floors and everything within
/// [`DEBRIS_REACH_UNITS`] of a piece, units only when they block, minus `options.exclude`.
pub(super) fn nearby<'a>(
    bodies: &'a BTreeMap<BodyId, RigidBody>,
    debris: &[DebrisBody],
    options: &'a Dynamic<'_>,
) -> impl Iterator<Item = &'a RigidBody> {
    let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
    for piece in debris {
        for axis in 0..3 {
            low[axis] = low[axis].min(piece.position[axis] - DEBRIS_REACH_UNITS);
            high[axis] = high[axis].max(piece.position[axis] + DEBRIS_REACH_UNITS);
        }
    }
    bodies
        .values()
        .filter(|body| options.units_block || !body.is_moving())
        .filter(|body| !options.exclude.contains(&body.id))
        .filter(move |body| {
            #[allow(clippy::cast_precision_loss)]
            let near = matches!(body.kind, Kind::Floor)
                || (0..3).all(|axis| {
                    let half = [
                        body.half_extents.x,
                        body.half_extents.y,
                        body.half_extents.z,
                    ][axis] as f32;
                    body.position[axis] + half >= low[axis]
                        && body.position[axis] - half <= high[axis]
                });
            near
        })
}

fn step_dynamic(
    gravity: [f32; 3],
    bodies: &BTreeMap<BodyId, RigidBody>,
    ground: Option<(BodyId, &crate::Terrain)>,
    debris: &mut [DebrisBody],
    settle: &[bool],
    options: &Dynamic<'_>,
) -> usize {
    let moving = debris.iter().filter(|piece| !piece.resting).count();
    if moving == 0 {
        return 0;
    }
    let mut world = empty_world();
    let per_second = TICKS_PER_SECOND * TICKS_PER_SECOND;
    world.gravity = metres(gravity) * per_second;
    for body in nearby(bodies, debris, options) {
        if body.is_moving() {
            // A unit stands in the props' way as a fixed capsule (#58).
            world.colliders.insert(
                ColliderBuilder::new(SharedShape::new(capsule(body.half_extents)))
                    .translation(lifted(body.position))
                    .build(),
            );
        } else {
            insert(&mut world, body);
        }
    }
    if let Some((id, terrain)) = ground {
        insert_ground(&mut world, id, terrain);
    }
    let pose = |piece: &DebrisBody| {
        let [x, y, z, w] = piece.rotation;
        Pose::from_parts(metres(piece.position), Rotation::from_xyzw(x, y, z, w))
    };
    let mut handles = Vec::with_capacity(debris.len());
    for piece in debris.iter() {
        let shape = SharedShape::new(cuboid(piece.half_extents));
        let collider = ColliderBuilder::new(shape)
            .friction(0.8)
            .restitution(0.15)
            .density(600.0);
        if piece.resting {
            world
                .colliders
                .insert(collider.position(pose(piece)).build());
            handles.push(None);
            continue;
        }
        let mut builder = RigidBodyBuilder::dynamic()
            .pose(pose(piece))
            .linvel(metres(piece.velocity) * TICKS_PER_SECOND)
            .angvel(Vector::from(piece.spin) * TICKS_PER_SECOND)
            .linear_damping(0.2)
            .angular_damping(0.8);
        if options.upright {
            builder = builder.lock_rotations();
        }
        let handle = world.bodies.insert(builder.build());
        world
            .colliders
            .insert_with_parent(collider.build(), handle, &mut world.bodies);
        handles.push(Some(handle));
    }
    world.step();
    for ((piece, handle), &settles) in debris.iter_mut().zip(handles).zip(settle) {
        let Some(body) = handle.and_then(|handle| world.bodies.get(handle)) else {
            continue;
        };
        piece.position = units(body.translation());
        piece.rotation = body.rotation().normalize().to_array();
        piece.velocity = units(body.linvel() / TICKS_PER_SECOND);
        piece.spin = (body.angvel() / TICKS_PER_SECOND).to_array();
        let speed = piece.velocity.iter().map(|v| v * v).sum::<f32>().sqrt();
        let spin = piece.spin.iter().map(|v| v * v).sum::<f32>().sqrt();
        if settles && speed < DEBRIS_REST_SPEED_UNITS && spin < DEBRIS_REST_SPIN {
            piece.resting = true;
            piece.velocity = [0.0; 3];
            piece.spin = [0.0; 3];
        }
    }
    moving
}
