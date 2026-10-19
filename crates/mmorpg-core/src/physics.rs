//! The zone's authoritative physical world (#271, Atlas 2.1.4 and 2.1.6).
//!
//! `mmorpg-core` decides movement intent, jumps, gravity and every gameplay outcome; this
//! module asks the physics backend where that intent physically leads. Static content
//! colliders, world limits, door leaves and NPCs are fixed boxes; the flat ground is a floor
//! half-space and the authored hills a heightfield. Players and creatures are upright capsules
//! moved by a kinematic character controller, which resolves contact, sliding and support.
//!
//! The game speaks centimetre units in `f32`: a moving body's position and velocity (units
//! per tick) are authoritative here and go into canonical snapshots exactly; gameplay rules
//! and projections read a rounded whole-unit view ([`RigidBody::position`]).
//!
//! Rapier (`physics/rapier.rs`) is the only backend in every production and WASM build. With
//! the non-default `physics-ab` feature (the `physics_ab` test and example only,
//! physics-engine #330), [`World`] can instead run on physics-engine's `approximate::World`
//! (`physics/engine.rs`) so the same zone simulation can be compared on both; `Backend` and
//! `using_backend` select it at runtime. No call site changes with the backend.

use std::fmt;

#[cfg(feature = "physics-ab")]
mod engine;
mod queries;
mod rapier;

/// The gap (in units) the character controller keeps between a body and what it touches:
/// well above `f32` resolution across the zone and below the half unit projections round
/// away. A moving body's backend position is its game centre raised by this offset, so a body
/// resting on the ground (one offset above it) has its feet exactly on the ground in game
/// coordinates.
const CONTACT_OFFSET_UNITS: f32 = 0.25;

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

/// The first body a ray met: its identity, the fraction of the ray travelled and the
/// surface normal there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RayHit {
    pub body: BodyId,
    pub fraction: f32,
    pub normal: [f32; 3],
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
    /// A fixed box the zone moves on a schedule (#56); its velocity is the displacement the
    /// next step applies.
    Platform,
    /// A fixed wedge (#59) filling the lower half of its box, its top rising toward the
    /// direction.
    Ramp(crate::RampRise),
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

    /// A fixed ramp (#59): a wedge in the box centred on `position`, its top rising toward
    /// `rises`.
    #[must_use]
    pub const fn ramp(
        id: BodyId,
        position: Vec3i,
        half_extents: Vec3i,
        rises: crate::RampRise,
    ) -> Self {
        Self {
            id,
            kind: Kind::Ramp(rises),
            position: exact(position),
            velocity: [0.0; 3],
            half_extents,
        }
    }

    /// A moving platform's box (#56) centred on `position`, at rest.
    #[must_use]
    pub const fn platform(id: BodyId, position: Vec3i, half_extents: Vec3i) -> Self {
        Self {
            id,
            kind: Kind::Platform,
            position: exact(position),
            velocity: [0.0; 3],
            half_extents,
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

    pub(crate) const fn is_moving(&self) -> bool {
        matches!(self.kind, Kind::Moving)
    }

    /// Whether it is a moving platform's box (#56).
    #[must_use]
    pub const fn is_platform(&self) -> bool {
        matches!(self.kind, Kind::Platform)
    }
}

/// A short-lived dynamic rigid body (#288): one piece of a broken prop. Rapier integrates it
/// under gravity against the fixed world (content, terrain, doors, NPCs and standing props);
/// units neither push it nor feel it. Like every body here its state is the game's, so a
/// restored zone continues it exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DebrisBody {
    /// Centre, in units.
    pub position: [f32; 3],
    /// Unit quaternion `[x, y, z, w]`.
    pub rotation: [f32; 4],
    /// Units per tick.
    pub velocity: [f32; 3],
    /// Radians per tick.
    pub spin: [f32; 3],
    pub half_extents: Vec3i,
    /// Settled: no longer simulated, and a fixed obstacle for pieces still moving.
    pub resting: bool,
}

impl DebrisBody {
    /// Whether the state is finite and the rotation a unit quaternion.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        let finite = self
            .position
            .iter()
            .chain(&self.rotation)
            .chain(&self.velocity)
            .chain(&self.spin)
            .all(|value| value.is_finite());
        let norm: f32 = self.rotation.iter().map(|value| value * value).sum();
        finite
            && (norm - 1.0).abs() < 1.0e-3
            && [
                self.half_extents.x,
                self.half_extents.y,
                self.half_extents.z,
            ]
            .iter()
            .all(|&extent| extent > 0)
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

/// Which physics backend a [`World`] runs on (physics-engine #330, test/bench only).
#[cfg(feature = "physics-ab")]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Backend {
    /// Rapier, the production authority.
    Rapier,
    /// physics-engine's `approximate::World`; debris stays on Rapier.
    Engine,
}

#[cfg(feature = "physics-ab")]
impl Backend {
    pub const ALL: [Self; 2] = [Self::Rapier, Self::Engine];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Rapier => "rapier",
            Self::Engine => "physics-engine",
        }
    }
}

#[cfg(feature = "physics-ab")]
thread_local! {
    static DEFAULT_BACKEND: std::cell::Cell<Backend> = const { std::cell::Cell::new(Backend::Rapier) };
}

/// Runs `run` with every [`World::new`] on this thread using `backend`, so whole zones (and
/// their recoveries) are built on it without any call site naming a backend.
#[cfg(feature = "physics-ab")]
pub fn using_backend<R>(backend: Backend, run: impl FnOnce() -> R) -> R {
    struct Restore(Backend);
    impl Drop for Restore {
        fn drop(&mut self) {
            DEFAULT_BACKEND.with(|cell| cell.set(self.0));
        }
    }
    let _restore = Restore(DEFAULT_BACKEND.with(|cell| cell.replace(backend)));
    run()
}

// Rapier stays inline: it is the only variant in production builds. The engine variant exists
// only with `physics-ab` and is boxed.
#[cfg_attr(feature = "physics-ab", allow(clippy::large_enum_variant))]
#[derive(Clone)]
enum Worlds {
    Rapier(rapier::World),
    #[cfg(feature = "physics-ab")]
    Engine(Box<engine::World>),
}

/// The zone's physical world, on Rapier unless `physics-ab` selects physics-engine.
pub struct World {
    worlds: Worlds,
    /// Nanoseconds spent in this world's calls, debris excluded (`physics-ab` timing).
    #[cfg(feature = "physics-ab")]
    spent: std::sync::atomic::AtomicU64,
}

impl fmt::Debug for World {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.worlds {
            Worlds::Rapier(world) => world.fmt(formatter),
            #[cfg(feature = "physics-ab")]
            Worlds::Engine(world) => world.fmt(formatter),
        }
    }
}

impl Clone for World {
    fn clone(&self) -> Self {
        Self {
            worlds: self.worlds.clone(),
            #[cfg(feature = "physics-ab")]
            spent: std::sync::atomic::AtomicU64::new(
                self.spent.load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

/// Calls the same method on whichever backend `$worlds` holds; with `physics-ab`, the call's
/// wall-clock time is added to the world's [`World::spent_nanos`].
macro_rules! dispatch {
    ($self:ident, $worlds:expr, $world:ident => $call:expr) => {{
        #[cfg(feature = "physics-ab")]
        let started = std::time::Instant::now();
        let result = match $worlds {
            Worlds::Rapier($world) => $call,
            #[cfg(feature = "physics-ab")]
            Worlds::Engine($world) => $call,
        };
        #[cfg(feature = "physics-ab")]
        $self.spent.fetch_add(
            u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
            std::sync::atomic::Ordering::Relaxed,
        );
        result
    }};
}

impl World {
    /// An empty world; `gravity` is in units per tick squared.
    #[must_use]
    pub fn new(gravity: Vec3i) -> Self {
        #[cfg(feature = "physics-ab")]
        {
            Self::with_backend(gravity, DEFAULT_BACKEND.with(std::cell::Cell::get))
        }
        #[cfg(not(feature = "physics-ab"))]
        {
            Self {
                worlds: Worlds::Rapier(rapier::World::new(gravity)),
            }
        }
    }

    /// An empty world on `backend`.
    #[cfg(feature = "physics-ab")]
    #[must_use]
    pub fn with_backend(gravity: Vec3i, backend: Backend) -> Self {
        let worlds = match backend {
            Backend::Rapier => Worlds::Rapier(rapier::World::new(gravity)),
            Backend::Engine => Worlds::Engine(Box::new(engine::World::new(gravity))),
        };
        Self {
            worlds,
            spent: std::sync::atomic::AtomicU64::new(0),
        }
    }

    #[cfg(feature = "physics-ab")]
    #[must_use]
    pub const fn backend(&self) -> Backend {
        match self.worlds {
            Worlds::Rapier(_) => Backend::Rapier,
            Worlds::Engine(_) => Backend::Engine,
        }
    }

    /// Wall-clock nanoseconds spent in this world's calls since it was created: the whole
    /// physics path of the zone (steps, queries and body updates), debris excluded.
    #[cfg(feature = "physics-ab")]
    #[must_use]
    pub fn spent_nanos(&self) -> u64 {
        self.spent.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn add_body(&mut self, body: RigidBody) -> Result<(), PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.add_body(body))
    }

    /// Replaces an existing body, for example a door leaf moving to its other box.
    pub fn replace_body(&mut self, body: RigidBody) -> Result<(), PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.replace_body(body))
    }

    pub fn remove_body(&mut self, id: BodyId) -> Option<RigidBody> {
        dispatch!(self, &mut self.worlds, world => world.remove_body(id))
    }

    #[must_use]
    pub fn body(&self, id: BodyId) -> Option<&RigidBody> {
        match &self.worlds {
            Worlds::Rapier(world) => world.body(id),
            #[cfg(feature = "physics-ab")]
            Worlds::Engine(world) => world.body(id),
        }
    }

    /// Places a moving body at `position` without resolving contact: placement by the zone
    /// (respawns, evade resets, recovery), never movement.
    pub fn set_position(&mut self, id: BodyId, position: Vec3i) -> Result<(), PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.set_position(id, position))
    }

    pub fn set_velocity(&mut self, id: BodyId, velocity: Vec3i) -> Result<(), PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.set_velocity(id, velocity))
    }

    /// Moves platform `id`'s box (#56) to centre `to` during the next [`World::step`], before
    /// or after the units (so it never runs into the ones it carries), carrying every unit
    /// that stands on it and is not rising: the displacement is added to that unit's move.
    pub fn move_platform(&mut self, id: BodyId, to: Vec3i) -> Result<(), PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.move_platform(id, to))
    }

    /// Places platform `id`'s box at centre `to` at once, carrying nothing: building or
    /// restoring a zone.
    pub fn place_platform(&mut self, id: BodyId, to: Vec3i) -> Result<(), PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.place_platform(id, to))
    }

    /// Lays the authored ground as a heightfield identified as `id` (#159).
    pub fn set_terrain(&mut self, id: BodyId, terrain: crate::Terrain) {
        dispatch!(self, &mut self.worlds, world => world.set_terrain(id, terrain));
    }

    /// The height (units) of the authored ground straight below or above `(x, z)`, if any.
    #[must_use]
    pub fn terrain_height(&self, x: i32, z: i32) -> Option<f32> {
        dispatch!(self, &self.worlds, world => world.terrain_height(x, z))
    }

    /// Advances one tick: each moving body, in ID order, gains gravity and asks the character
    /// controller where its velocity leads. Landing on ground or meeting a ceiling ends its
    /// vertical velocity.
    pub fn step(&mut self) -> Result<StepStats, PhysicsError> {
        dispatch!(self, &mut self.worlds, world => world.step())
    }

    /// Whether a moving body stands on static ground.
    #[must_use]
    pub fn grounded(&self, id: BodyId) -> bool {
        dispatch!(self, &self.worlds, world => world.grounded(id))
    }

    /// Every body whose shape intersects the box, in ID order.
    ///
    /// Rapier's `intersect_shape` silently skips cuboid–heightfield pairs (parry reports them
    /// unsupported), so on Rapier the terrain never shows up here. Door probes
    /// (`door::leaf_blocked`) only look for units, so that is harmless for now.
    pub fn overlap_query(&self, aabb: Aabb) -> Result<Vec<BodyId>, PhysicsError> {
        dispatch!(self, &self.worlds, world => world.overlap_query(aabb))
    }

    /// Whether a body accepted by `blocks` lies on the segment from `origin` along
    /// `translation` (units), strictly before its end.
    pub fn segment_blocked(
        &self,
        origin: Vec3i,
        translation: Vec3i,
        blocks: impl Fn(BodyId) -> bool,
    ) -> bool {
        dispatch!(self, &self.worlds, world => world.segment_blocked(origin, translation, &blocks))
    }

    /// The first body accepted by `hits` on the segment from `origin` along `translation`
    /// (units), with the hit's fraction of the segment and its surface normal (#286). Rays
    /// see every collider kind, terrain heightfield included.
    pub fn cast_ray(
        &self,
        origin: [f32; 3],
        translation: [f32; 3],
        hits: impl Fn(BodyId) -> bool,
    ) -> Option<RayHit> {
        dispatch!(self, &self.worlds, world => world.cast_ray(origin, translation, &hits))
    }

    /// Advances `debris` (#288) one tick: Rapier steps a world of the pieces still moving as
    /// dynamic bodies, the settled ones and the fixed geometry around them, rebuilt from the
    /// game's state in slice order like every step here. A piece that slows below the rest
    /// thresholds settles once `settle` allows it. Returns how many pieces moved.
    ///
    /// Debris always runs on Rapier, whichever backend the world uses (physics-engine #330
    /// parks dynamics), and is excluded from [`World::spent_nanos`].
    ///
    /// Rapier gap (#288): rebuilding each tick drops the solver's warm-start contact impulses,
    /// so a resting stack jitters; pieces settle into fixed obstacles instead of sleeping.
    pub fn step_debris(&self, debris: &mut [DebrisBody], settle: &[bool]) -> usize {
        match &self.worlds {
            Worlds::Rapier(world) => world.step_debris(debris, settle),
            #[cfg(feature = "physics-ab")]
            Worlds::Engine(world) => world.step_debris(debris, settle),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: Vec3i = Vec3i::new(30, 90, 30);

    /// Runs `test` on Rapier and, with `physics-ab`, again on physics-engine.
    fn on_each_backend(test: fn()) {
        test();
        #[cfg(feature = "physics-ab")]
        using_backend(Backend::Engine, test);
    }

    fn world() -> World {
        let mut world = World::new(Vec3i::new(0, -1, 0));
        world.add_body(RigidBody::floor(BodyId(1), 0)).unwrap();
        world
    }

    #[test]
    fn a_unit_falls_lands_and_walks_into_a_wall() {
        on_each_backend(a_unit_falls_lands_and_walks_into_a_wall_on);
    }

    fn a_unit_falls_lands_and_walks_into_a_wall_on() {
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
        on_each_backend(characters_meeting_head_on_slide_around_each_other_on);
    }

    fn characters_meeting_head_on_slide_around_each_other_on() {
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
    fn characters_meeting_off_axis_slide_around_each_other() {
        on_each_backend(characters_meeting_off_axis_slide_around_each_other_on);
    }

    fn characters_meeting_off_axis_slide_around_each_other_on() {
        // Head-on and slightly off-axis pairs, at walking and running speeds, both ways round.
        let mut stuck = Vec::new();
        for offset in [0, 1, 3, 5, 8, 12, 20, 30, 45] {
            for speed in [8, 21] {
                for flip in [false, true] {
                    let mut world = world();
                    let (a, b) = if flip {
                        (BodyId(11), BodyId(10))
                    } else {
                        (BodyId(10), BodyId(11))
                    };
                    world
                        .add_body(RigidBody::dynamic(
                            a,
                            Vec3i::new(-300, 90, 0),
                            Vec3i::ZERO,
                            BODY,
                        ))
                        .unwrap();
                    world
                        .add_body(RigidBody::dynamic(
                            b,
                            Vec3i::new(300, 90, offset),
                            Vec3i::ZERO,
                            BODY,
                        ))
                        .unwrap();
                    for _ in 0..120 {
                        world.set_velocity(a, Vec3i::new(speed, 0, 0)).unwrap();
                        world.set_velocity(b, Vec3i::new(-speed, 0, 0)).unwrap();
                        world.step().unwrap();
                    }
                    let (pa, pb) = (
                        world.body(a).unwrap().position(),
                        world.body(b).unwrap().position(),
                    );
                    if !(pa.x > 300 && pb.x < -300) {
                        stuck.push((offset, speed, flip, pa, pb));
                    }
                }
            }
        }
        assert!(stuck.is_empty(), "pairs deadlocked: {stuck:?}");
    }

    /// A ramp rising toward +X from the floor at x = -300 to `height` at x = 300, a dock box
    /// behind its high end at the same height, and a unit at x = -600 walking +X for 200
    /// ticks. Returns where the unit ends and its lowest grounded height while on the ramp.
    fn walk_up_ramp(height: i32, run: i32) -> ([f32; 3], bool) {
        let mut world = world();
        world
            .add_body(RigidBody::ramp(
                BodyId(2),
                Vec3i::new(0, height / 2, 0),
                Vec3i::new(run / 2, height / 2, 200),
                crate::RampRise::PosX,
            ))
            .unwrap();
        world
            .add_body(RigidBody::fixed(
                BodyId(3),
                Vec3i::new(run / 2 + 1_000, height / 2, 0),
                Vec3i::new(1_000, height / 2, 200),
            ))
            .unwrap();
        let unit = BodyId(10);
        world
            .add_body(RigidBody::dynamic(
                unit,
                Vec3i::new(-run / 2 - 300, 90, 0),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        let mut grounded = true;
        for _ in 0..200 {
            world
                .set_velocity(
                    unit,
                    Vec3i::new(8, world.body(unit).unwrap().velocity().y, 0),
                )
                .unwrap();
            world.step().unwrap();
            let x = world.body(unit).unwrap().exact_position()[0];
            #[allow(clippy::cast_precision_loss)]
            if x.abs() < run as f32 / 2.0 - 40.0 {
                grounded &= world.grounded(unit);
            }
        }
        (world.body(unit).unwrap().exact_position(), grounded)
    }

    #[test]
    fn a_unit_walks_up_a_shallow_ramp_onto_the_dock_but_not_up_a_steep_one() {
        on_each_backend(a_unit_walks_up_a_shallow_ramp_onto_the_dock_but_not_up_a_steep_one_on);
    }

    fn a_unit_walks_up_a_shallow_ramp_onto_the_dock_but_not_up_a_steep_one_on() {
        // 20°: 220 units over a 600-unit run. Grounded all the way up, then on the dock.
        let (end, grounded) = walk_up_ramp(220, 600);
        assert!(grounded, "grounded on the ramp");
        assert!(
            end[0] > 350.0 && (end[1] - 310.0).abs() < 0.5,
            "on the dock: {end:?}"
        );
        // 60°: 208 units over 120. The unit cannot climb it and stays on the floor before it.
        let (end, _) = walk_up_ramp(208, 120);
        assert!(
            end[0] < 0.0 && end[1] < 150.0,
            "stopped by the steep ramp: {end:?}"
        );
    }

    #[test]
    fn a_unit_walks_down_and_across_a_ramp_on_its_surface() {
        on_each_backend(a_unit_walks_down_and_across_a_ramp_on_its_surface_on);
    }

    fn a_unit_walks_down_and_across_a_ramp_on_its_surface_on() {
        let mut world = world();
        let ramp = crate::Ramp {
            body: 2,
            position: [0, 110, 0],
            half_extents: [300, 110, 300],
            rises: crate::RampRise::NegZ,
        };
        world
            .add_body(RigidBody::ramp(
                BodyId(2),
                Vec3i::new(0, 110, 0),
                Vec3i::new(300, 110, 300),
                ramp.rises,
            ))
            .unwrap();
        // Dropped onto the high end, then walking across (+X) and down (+Z).
        let unit = BodyId(10);
        world
            .add_body(RigidBody::dynamic(
                unit,
                Vec3i::new(-200, 400, -200),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        for velocity in [Vec3i::ZERO, Vec3i::new(6, 0, 0), Vec3i::new(0, 0, 6)] {
            for _ in 0..60 {
                let vertical = world.body(unit).unwrap().velocity().y;
                world
                    .set_velocity(unit, Vec3i::new(velocity.x, vertical, velocity.z))
                    .unwrap();
                world.step().unwrap();
            }
            let [x, y, z] = world.body(unit).unwrap().exact_position();
            #[allow(clippy::cast_possible_truncation)]
            let top = ramp.top_at(x.round() as i32, z.round() as i32).unwrap();
            // The capsule's lowest point rests on the slope where it touches, a little uphill
            // of its centre: within the capsule's radius times the slope.
            #[allow(clippy::cast_possible_truncation)]
            let feet = f64::from(y) - 90.0;
            assert!(
                feet >= top - 0.5 && feet <= top + 30.0 * (110.0 / 300.0) + 1.0,
                "on the surface at {x} {z}: feet {feet}, top {top}"
            );
            assert!(world.grounded(unit), "grounded on the ramp at {x} {z}");
        }
    }

    #[test]
    fn line_queries_cover_thin_touching_inside_tied_filtered_and_removed_bodies() {
        on_each_backend(
            line_queries_cover_thin_touching_inside_tied_filtered_and_removed_bodies_on,
        );
    }

    /// Line-of-sight edge cases (#57) on the world's ray and segment queries.
    fn line_queries_cover_thin_touching_inside_tied_filtered_and_removed_bodies_on() {
        let mut world = world();
        let all = |_: BodyId| true;
        // A 2-unit-thin wall across x = 500, from z = -200 to 200.
        let wall = RigidBody::fixed(BodyId(2), Vec3i::new(500, 100, 0), Vec3i::new(1, 100, 200));
        world.add_body(wall).unwrap();
        let along = Vec3i::new(1_000, 0, 0);
        assert!(
            world.segment_blocked(Vec3i::new(0, 100, 0), along, all),
            "thin wall blocks"
        );
        assert!(
            !world.segment_blocked(Vec3i::new(0, 100, 202), along, all),
            "a line two units past its edge is clear"
        );
        assert!(
            !world.segment_blocked(Vec3i::new(0, 100, 0), Vec3i::new(498, 0, 0), all),
            "a line ending a unit short of it is clear"
        );
        // Touching: from a unit off either face, away is clear and into it is blocked; from
        // inside, blocked at once.
        assert!(!world.segment_blocked(Vec3i::new(502, 100, 0), along, all));
        assert!(world.segment_blocked(Vec3i::new(498, 100, 0), along, all));
        let inside = world
            .cast_ray([500.0, 100.0, 0.0], [300.0, 0.0, 0.0], all)
            .expect("a ray from inside hits");
        assert_eq!((inside.body, inside.fraction), (BodyId(2), 0.0));
        // Ties: a second box sharing the wall's near face; the lower ID wins on both backends.
        world
            .add_body(RigidBody::fixed(
                BodyId(3),
                Vec3i::new(510, 100, 0),
                Vec3i::new(11, 50, 50),
            ))
            .unwrap();
        let tie = world
            .cast_ray([0.0, 100.0, 0.0], [1_000.0, 0.0, 0.0], all)
            .unwrap();
        assert_eq!(tie.body, BodyId(2));
        assert!((tie.fraction - 0.499).abs() < 1.0e-4, "{tie:?}");
        // Filters: skipping the wall finds the box behind it; units are skipped when excluded.
        let behind = world
            .cast_ray([0.0, 100.0, 0.0], [1_000.0, 0.0, 0.0], |id| id != BodyId(2))
            .unwrap();
        assert_eq!(behind.body, BodyId(3));
        let unit = BodyId(10);
        world
            .add_body(RigidBody::dynamic(
                unit,
                Vec3i::new(250, 90, 0),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        assert_eq!(
            world
                .cast_ray([0.0, 100.0, 0.0], [1_000.0, 0.0, 0.0], all)
                .unwrap()
                .body,
            unit
        );
        assert_eq!(
            world
                .cast_ray([0.0, 100.0, 0.0], [1_000.0, 0.0, 0.0], |id| id.0 < 10)
                .unwrap()
                .body,
            BodyId(2)
        );
        // Removal and ID reuse: the wall's ID now names a box elsewhere.
        assert!(world.remove_body(BodyId(2)).is_some());
        assert!(world.remove_body(BodyId(3)).is_some());
        assert!(!world.segment_blocked(Vec3i::new(0, 100, 0), along, |id| id.0 < 10));
        world
            .add_body(RigidBody::fixed(
                BodyId(2),
                Vec3i::new(500, 100, 600),
                Vec3i::new(1, 100, 200),
            ))
            .unwrap();
        assert!(!world.segment_blocked(Vec3i::new(0, 100, 0), along, |id| id.0 < 10));
        assert!(world.segment_blocked(Vec3i::new(0, 100, 600), along, |id| id.0 < 10));
    }

    /// A lift pillar (#56) whose top rises from the floor to 300 units at 5 units a tick.
    fn lift(world: &mut World) -> BodyId {
        let lift = BodyId(5);
        world
            .add_body(RigidBody::platform(
                lift,
                Vec3i::new(0, -150, 0),
                Vec3i::new(100, 150, 100),
            ))
            .unwrap();
        lift
    }

    #[test]
    fn a_platform_carries_its_rider_up_and_down_and_lets_it_walk_off() {
        // Rapier carries; physics-engine has no moving support and says so.
        let mut world = world();
        let lift = lift(&mut world);
        let rider = BodyId(10);
        world
            .add_body(RigidBody::dynamic(
                rider,
                Vec3i::new(0, 90, 0),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        world.step().unwrap();
        for tick in 1..=60 {
            world
                .move_platform(lift, Vec3i::new(0, -150 + 5 * tick, 0))
                .unwrap();
            world.step().unwrap();
            let body = world.body(rider).unwrap();
            assert!(
                (body.exact_position()[1] - (90.0 + 5.0 * tick as f32)).abs() < 0.05,
                "carried up at tick {tick}: {:?}",
                body.exact_position()
            );
            assert!(world.grounded(rider));
        }
        for tick in (0..60).rev() {
            world
                .move_platform(lift, Vec3i::new(0, -150 + 5 * tick, 0))
                .unwrap();
            world.step().unwrap();
            let y = world.body(rider).unwrap().exact_position()[1];
            assert!(
                (y - (90.0 + 5.0 * tick as f32)).abs() < 0.05,
                "carried down at tick {tick}: {y}"
            );
        }
        // Up again, then step off the top onto nothing: the rider falls to the floor.
        for tick in 1..=60 {
            world
                .move_platform(lift, Vec3i::new(0, -150 + 5 * tick, 0))
                .unwrap();
            world.step().unwrap();
        }
        world.set_velocity(rider, Vec3i::new(10, 0, 0)).unwrap();
        for _ in 0..90 {
            world.step().unwrap();
        }
        let body = *world.body(rider).unwrap();
        assert!(body.position().x > 150, "walked off: {:?}", body.position());
        assert_eq!(body.position().y, 90, "and fell to the floor");
    }

    #[test]
    fn a_rider_falls_when_its_platform_is_removed_and_a_jump_leaves_it() {
        let mut world = world();
        let lift = lift(&mut world);
        world.place_platform(lift, Vec3i::new(0, 150, 0)).unwrap();
        let rider = BodyId(10);
        world
            .add_body(RigidBody::dynamic(
                rider,
                Vec3i::new(0, 390, 0),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        world.step().unwrap();
        assert!(world.grounded(rider));
        // A jump while the platform sinks: the rider rises on its own, not carried down.
        world.set_velocity(rider, Vec3i::new(0, 12, 0)).unwrap();
        world.move_platform(lift, Vec3i::new(0, 145, 0)).unwrap();
        world.step().unwrap();
        assert!(world.body(rider).unwrap().exact_position()[1] > 390.0);
        for _ in 0..40 {
            world.step().unwrap();
        }
        assert!(world.grounded(rider));
        assert!(world.remove_body(lift).is_some());
        for _ in 0..60 {
            world.step().unwrap();
        }
        assert_eq!(
            world.body(rider).unwrap().position().y,
            90,
            "fell to the floor"
        );
    }

    #[cfg(feature = "physics-ab")]
    #[test]
    fn physics_engine_refuses_to_carry_a_rider() {
        let mut world = World::with_backend(Vec3i::new(0, -1, 0), Backend::Engine);
        world.add_body(RigidBody::floor(BodyId(1), 0)).unwrap();
        let lift = lift(&mut world);
        world
            .add_body(RigidBody::dynamic(
                BodyId(10),
                Vec3i::new(0, 90, 0),
                Vec3i::ZERO,
                BODY,
            ))
            .unwrap();
        world.step().unwrap();
        world.move_platform(lift, Vec3i::new(0, -145, 0)).unwrap();
        let failure = world.step().unwrap_err().to_string();
        assert!(failure.contains("unsupported"), "{failure}");
        // A platform nobody stands on moves.
        world.remove_body(BodyId(10));
        world.move_platform(lift, Vec3i::new(0, -140, 0)).unwrap();
        world.step().unwrap();
        assert_eq!(world.body(lift).unwrap().position(), Vec3i::new(0, -140, 0));
    }

    #[test]
    fn queries_see_bodies_and_rebuilt_worlds_continue_exactly() {
        on_each_backend(queries_see_bodies_and_rebuilt_worlds_continue_exactly_on);
    }

    fn queries_see_bodies_and_rebuilt_worlds_continue_exactly_on() {
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
