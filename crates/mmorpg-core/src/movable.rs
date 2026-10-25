//! Movable props (#58): heavy crates that gameplay impulses slide around.
//!
//! A movable prop is a breakable prop (#288) whose body the zone moves: Shield Bash, Charge,
//! blasts, stomps and shoves give it a horizontal velocity instead of a forced move, and each
//! tick Rapier slides it upright (it never tips) under gravity with friction, stopped by walls,
//! other props and units, until it comes to rest. Units meet it as a fixed box where it stands.
//! Its exact `f32` position and velocity are the creature record's in canonical snapshots, so a
//! restored zone continues a slide exactly. The dynamics run on Rapier in both physics arms
//! (physics-engine parks dynamic bodies), like debris.

use crate::entity::body_id;
use crate::physics::{BodyId, DebrisBody, RigidBody, Vec3i};
use crate::zone::physics_error;
use crate::{CreatureId, EntityRef, ZoneError, ZoneSimulation};

/// Effective sliding friction of a prop on the ground (Rapier averages the prop's 0.8 with the
/// fixed world's 0.5), so a push of `d` units slides it about `d`.
const SLIDE_FRICTION: f32 = 0.65;

/// A movable prop's exact state while it stands.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct PropMotion {
    /// Centre, in units.
    pub position: [f32; 3],
    /// Units per tick; zero at rest.
    pub velocity: [f32; 3],
}

impl ZoneSimulation {
    /// Whether `unit` is a standing movable prop.
    pub(crate) fn is_movable_prop(&self, unit: EntityRef) -> bool {
        let EntityRef::Creature(id) = unit else {
            return false;
        };
        self.prop_motion.contains_key(&id)
    }

    /// A movable prop at `position` starts at rest (spawned or respawned).
    pub(crate) fn place_movable_prop(&mut self, id: CreatureId, position: [f32; 3]) {
        self.prop_motion.insert(
            id,
            PropMotion {
                position,
                velocity: [0.0; 3],
            },
        );
    }

    /// Gives a movable prop a horizontal push of `distance` units along `direction` (XZ), as a
    /// knockback of that distance would.
    pub(crate) fn push_prop(&mut self, id: CreatureId, direction: [i64; 2], distance: i32) -> bool {
        let Some(motion) = self.prop_motion.get_mut(&id) else {
            return false;
        };
        #[allow(clippy::cast_precision_loss)]
        let (x, z) = (direction[0] as f32, direction[1] as f32);
        let length = x.hypot(z);
        if distance <= 0 || length == 0.0 {
            return false;
        }
        // Launch speed whose friction stopping distance v²/(2·μ·g) is about `distance`.
        #[allow(clippy::cast_precision_loss)]
        let gravity = {
            let [gx, gy, gz] = self
                .content
                .definition()
                .gravity()
                .map(|axis| axis as f32 / crate::physics::SUBUNITS_PER_UNIT as f32);
            (gx * gx + gy * gy + gz * gz).sqrt()
        };
        #[allow(clippy::cast_precision_loss)]
        let speed = (2.0 * SLIDE_FRICTION * gravity * distance as f32).sqrt();
        motion.velocity[0] += x / length * speed;
        motion.velocity[2] += z / length * speed;
        true
    }

    /// After the physics step: every moving prop slides one tick (Rapier, upright), then its
    /// fixed box follows to its rounded centre. Returns how many moved.
    pub(crate) fn advance_movable_props(&mut self) -> Result<usize, ZoneError> {
        let moving: Vec<_> = self
            .prop_motion
            .iter()
            .filter(|(_, motion)| motion.velocity != [0.0; 3])
            .map(|(&id, motion)| (id, *motion))
            .collect();
        if moving.is_empty() {
            return Ok(0);
        }
        let mut bodies = Vec::with_capacity(moving.len());
        for &(id, motion) in &moving {
            let half = self
                .world
                .body(body_id(EntityRef::Creature(id)))
                .ok_or_else(|| ZoneError::new("movable prop physics body is missing"))?
                .half_extents();
            bodies.push(DebrisBody {
                position: motion.position,
                rotation: [0.0, 0.0, 0.0, 1.0],
                velocity: motion.velocity,
                spin: [0.0; 3],
                half_extents: half,
                resting: false,
            });
        }
        let exclude: Vec<BodyId> = moving
            .iter()
            .map(|&(id, _)| body_id(EntityRef::Creature(id)))
            .collect();
        self.world.step_props(&mut bodies, &exclude);
        for ((id, _), body) in moving.into_iter().zip(bodies) {
            let velocity = if body.resting {
                [0.0; 3]
            } else {
                body.velocity
            };
            self.prop_motion.insert(
                id,
                PropMotion {
                    position: body.position,
                    velocity,
                },
            );
            let [x, y, z] = crate::physics::to_units(body.position);
            let half = body.half_extents;
            self.world
                .replace_body(RigidBody::fixed(
                    body_id(EntityRef::Creature(id)),
                    Vec3i::new(x, y, z),
                    half,
                ))
                .map_err(physics_error)?;
            self.place_in_interest(EntityRef::Creature(id), x, z);
        }
        Ok(exclude.len())
    }

    /// The movable props' exact state, for canonical snapshots.
    pub(crate) fn prop_motion_of(&self, id: CreatureId) -> Option<PropMotion> {
        self.prop_motion.get(&id).copied()
    }
}
