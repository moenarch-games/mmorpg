//! Pushable boulders (#299): an environmental trap on the slope above the Redbrand camp.
//!
//! A boulder is a movable prop ([`crate::PropKind::Boulder`]) that rolls as a real sphere on the
//! physics backend. Units meet it as a fixed sphere where it stands. It rests where placed, even
//! on a slope, until something moves it: walking contact by any player or creature rolls it at
//! a slow, controlled [`WALK_PUSH_SPEED`]; knockbacks, blasts, stomps and Charge use the
//! ordinary movable-prop launch, which is far faster. Rolling downhill it speeds up under
//! gravity; slowing below the rest thresholds it stops again and stays until pushed.
//!
//! At [`IMPACT_SPEED`] or more it passes through units and hits those ahead of it: damage that
//! grows with speed and a knockback along its path, with the usual forced-move immunity so one
//! roll does not chain-hit a victim. Below that speed units simply stop it like any body.
//! Players are hit by the boulder itself; a creature's damage is credited to the last player
//! who pushed it, so kills count. A boulder rolled only by creatures knocks others back but
//! deals them no damage.
//!
//! A boulder cannot be broken. [`RETURN_TICKS`] after its last push, once its spot is clear, it
//! returns to its spawn at rest, so the trap recurs. Its motion, spin, return timer and pusher are
//! canonical ([`CanonicalBoulder`]), so a restored zone continues a roll exactly.

use crate::entity::{EntityRef, body_id, is_unit_body};
use crate::physics::{BodyId, DebrisBody, PieceShape, RigidBody, Vec3i, capsule_dimensions};
use crate::zone::physics_error;
use crate::{CreatureId, PlayerId, ZoneError, ZoneSimulation};

/// Walking into a boulder rolls it this fast (units per tick: 60 cm/s).
pub const WALK_PUSH_SPEED: f32 = 1.0;
/// A boulder at this speed or more hurts what it hits (units per tick: 1.8 m/s).
pub const IMPACT_SPEED: f32 = 3.0;
/// The speed at which an impact deals its full damage (units per tick: 7.2 m/s).
const FULL_IMPACT_SPEED: f32 = 12.0;
/// A boulder never rolls faster than this (units per tick: 10.8 m/s).
const MAX_SPEED: f32 = 18.0;
/// Base damage roll of an impact, before the speed bonus.
pub const IMPACT_DAMAGE: [u16; 2] = [18, 24];
/// Extra damage at [`FULL_IMPACT_SPEED`].
pub const IMPACT_DAMAGE_BONUS: u32 = 50;
/// An impact knocks a unit this far along the boulder's path.
pub const IMPACT_KNOCKBACK_UNITS: i32 = 350;
/// How close a unit must stand (beyond touching) to push or be hit, in units.
const CONTACT_MARGIN_UNITS: f32 = 4.0;
/// Motion within this cosine of the line to the boulder counts as walking into it.
const WALK_INTO_COS: f32 = 0.5;
/// A boulder away from its spot returns to it this long after its last push (30 s).
pub const RETURN_TICKS: u16 = crate::ticks_from_ms(30_000);
/// Within this many units of home a boulder counts as home.
const HOME_UNITS: f32 = 30.0;

/// A boulder's canonical state beyond its creature record's position and velocity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoulderState {
    /// Radians per tick.
    pub spin: [f32; 3],
    /// The last player whose walk or ability moved it, credited with its impacts.
    pub pusher: Option<PlayerId>,
    /// Ticks since its last push while away from home.
    pub idle: u16,
}

/// A boulder's canonical state in snapshots, in creature-ID order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanonicalBoulder {
    pub creature_id: CreatureId,
    pub spin: [f32; 3],
    pub pusher: Option<PlayerId>,
    pub idle: u16,
}

/// A walking unit shouldering a boulder this tick.
pub(crate) struct WalkPush {
    boulder: CreatureId,
    /// Normalized horizontal direction from the unit to the boulder.
    direction: [f32; 2],
    by: Option<PlayerId>,
}

/// Caps `velocity` at [`MAX_SPEED`].
pub(crate) fn cap_speed(velocity: &mut [f32; 3]) {
    let speed = length(*velocity);
    if speed > MAX_SPEED {
        *velocity = velocity.map(|axis| axis / speed * MAX_SPEED);
    }
}

fn length(vector: [f32; 3]) -> f32 {
    vector.iter().map(|axis| axis * axis).sum::<f32>().sqrt()
}

/// Whether a boulder of `radius` at `centre` touches the upright capsule of `unit`, measuring
/// to the capsule's own axis.
fn touches(centre: [f32; 3], radius: f32, unit: &RigidBody) -> bool {
    let (half_segment, unit_radius) = capsule_dimensions(unit.half_extents());
    let [ux, uy, uz] = unit.exact_position();
    let closest_y = centre[1].clamp(uy - half_segment, uy + half_segment);
    let (dx, dy, dz) = (centre[0] - ux, centre[1] - closest_y, centre[2] - uz);
    let reach = radius + unit_radius + CONTACT_MARGIN_UNITS;
    dx * dx + dy * dy + dz * dz <= reach * reach
}

impl ZoneSimulation {
    fn boulder_ids(&self) -> Vec<CreatureId> {
        self.prop_motion
            .iter()
            .filter(|(_, motion)| motion.boulder.is_some())
            .map(|(&id, _)| id)
            .collect()
    }

    fn boulder_radius(&self, id: CreatureId) -> Result<f32, ZoneError> {
        let (_, template) = Self::creature_content(&self.content, id)?;
        #[allow(clippy::cast_precision_loss)]
        Ok(template.half_extents[0] as f32)
    }

    /// Whether `id` is a boulder, which takes no damage.
    pub(crate) fn is_boulder(&self, id: CreatureId) -> bool {
        self.prop_motion
            .get(&id)
            .is_some_and(|motion| motion.boulder.is_some())
    }

    /// The units whose walk this tick presses into a boulder, from their commanded velocities
    /// before the physics step resolves them (ascending unit order).
    pub(crate) fn boulder_walk_pushes(&self) -> Result<Vec<WalkPush>, ZoneError> {
        let mut pushes = Vec::new();
        for id in self.boulder_ids() {
            let Some(motion) = self.prop_motion.get(&id) else {
                continue;
            };
            let centre = motion.position;
            let radius = self.boulder_radius(id)?;
            let [cx, _, cz] = crate::physics::to_units(centre);
            #[allow(clippy::cast_possible_truncation)]
            let reach = radius as i32 + 100;
            for unit in self.interest.near(cx, cz, reach) {
                let by = match unit {
                    EntityRef::Player(player) => Some(player),
                    EntityRef::Creature(creature) if !self.is_prop(unit) => {
                        let _ = creature;
                        None
                    }
                    _ => continue,
                };
                if !self.unit_alive_now(unit) {
                    continue;
                }
                let Some(body) = self.world.body(body_id(unit)) else {
                    continue;
                };
                if !is_unit_body(body.id()) || !touches(centre, radius, body) {
                    continue;
                }
                let [vx, _, vz] = body.exact_velocity();
                let walking = vx.hypot(vz);
                let [ux, _, uz] = body.exact_position();
                let (tx, tz) = (centre[0] - ux, centre[2] - uz);
                let distance = tx.hypot(tz);
                if walking <= 0.0
                    || distance <= 0.0
                    || (vx * tx + vz * tz) / (walking * distance) < WALK_INTO_COS
                {
                    continue;
                }
                pushes.push(WalkPush {
                    boulder: id,
                    direction: [tx / distance, tz / distance],
                    by,
                });
            }
        }
        Ok(pushes)
    }

    /// After the physics step: applies this tick's walking pushes, rolls every moving boulder
    /// one tick, resolves impacts and returns idle boulders home.
    pub(crate) fn advance_boulders(
        &mut self,
        pushes: &[WalkPush],
        now: u64,
    ) -> Result<(), ZoneError> {
        for push in pushes {
            let Some(motion) = self.prop_motion.get_mut(&push.boulder) else {
                continue;
            };
            let Some(state) = &mut motion.boulder else {
                continue;
            };
            let [dx, dz] = push.direction;
            let along = motion.velocity[0] * dx + motion.velocity[2] * dz;
            if along < WALK_PUSH_SPEED {
                let add = WALK_PUSH_SPEED - along;
                motion.velocity[0] += add * dx;
                motion.velocity[2] += add * dz;
            }
            state.idle = 0;
            if push.by.is_some() {
                state.pusher = push.by;
            }
        }
        for id in self.boulder_ids() {
            self.roll_boulder(id)?;
            self.boulder_impacts(id, now)?;
            self.boulder_returns(id)?;
        }
        Ok(())
    }

    /// One tick of a moving boulder: the physics step, then its fixed sphere follows.
    fn roll_boulder(&mut self, id: CreatureId) -> Result<(), ZoneError> {
        let Some(mut motion) = self.prop_motion.get(&id).copied() else {
            return Ok(());
        };
        let Some(mut state) = motion.boulder else {
            return Ok(());
        };
        if motion.velocity == [0.0; 3] && state.spin == [0.0; 3] {
            return Ok(());
        }
        let body = body_id(EntityRef::Creature(id));
        let half = self
            .world
            .body(body)
            .ok_or_else(|| ZoneError::new("boulder physics body is missing"))?
            .half_extents();
        let mut pieces = [DebrisBody {
            position: motion.position,
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: motion.velocity,
            spin: state.spin,
            half_extents: half,
            shape: PieceShape::Sphere,
            resting: false,
        }];
        let exclude: [BodyId; 1] = [body];
        // A slow boulder stops at units; a fast one rolls through and the zone resolves the hit.
        let units_block = length(motion.velocity) < IMPACT_SPEED;
        // A boulder in water sinks, slowed and pushed by the current (#667).
        let media = self.piece_media(&pieces, [crate::PropKind::Boulder]);
        self.world
            .step_boulders(&mut pieces, &exclude, units_block, &media);
        let [piece] = pieces;
        motion.position = piece.position;
        motion.velocity = if piece.resting {
            [0.0; 3]
        } else {
            piece.velocity
        };
        cap_speed(&mut motion.velocity);
        state.spin = if piece.resting { [0.0; 3] } else { piece.spin };
        motion.boulder = Some(state);
        self.prop_motion.insert(id, motion);
        let [x, y, z] = crate::physics::to_units(piece.position);
        self.world
            .replace_body(RigidBody::sphere(body, Vec3i::new(x, y, z), half.x))
            .map_err(physics_error)?;
        self.place_in_interest(EntityRef::Creature(id), x, z);
        Ok(())
    }

    /// A boulder at impact speed hits every living unit it touches ahead of it that is not
    /// already being moved or immune: damage from speed and a knockback along its path.
    fn boulder_impacts(&mut self, id: CreatureId, now: u64) -> Result<(), ZoneError> {
        let Some(motion) = self.prop_motion.get(&id).copied() else {
            return Ok(());
        };
        let Some(state) = motion.boulder else {
            return Ok(());
        };
        let speed = length(motion.velocity);
        let [vx, _, vz] = motion.velocity;
        let rolling = vx.hypot(vz);
        if speed < IMPACT_SPEED || rolling <= 0.0 {
            return Ok(());
        }
        let (rx, rz) = (vx / rolling, vz / rolling);
        let centre = motion.position;
        let radius = self.boulder_radius(id)?;
        let [cx, _, cz] = crate::physics::to_units(centre);
        #[allow(clippy::cast_possible_truncation)]
        let reach = radius as i32 + 100;
        let mut victims: Vec<EntityRef> = Vec::new();
        for unit in self.interest.near(cx, cz, reach) {
            if !matches!(unit, EntityRef::Player(_) | EntityRef::Creature(_)) || !self.movable(unit)
            {
                continue;
            }
            let Some(body) = self.world.body(body_id(unit)) else {
                continue;
            };
            if !touches(centre, radius, body) {
                continue;
            }
            let [ux, _, uz] = body.exact_position();
            // Only what lies ahead: a unit just behind (the one who launched it) is left alone.
            if (ux - centre[0]) * rx + (uz - centre[2]) * rz >= 0.0 {
                victims.push(unit);
            }
        }
        for victim in victims {
            let share =
                ((speed - IMPACT_SPEED) / (FULL_IMPACT_SPEED - IMPACT_SPEED)).clamp(0.0, 1.0);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss
            )]
            let bonus = (IMPACT_DAMAGE_BONUS as f32 * share) as u32;
            let amount = self
                .rng
                .inclusive(u32::from(IMPACT_DAMAGE[0]), u32::from(IMPACT_DAMAGE[1]))
                + bonus;
            let body = self
                .world
                .body(body_id(victim))
                .map(RigidBody::exact_position);
            let direction = body.map_or([rx, rz], |[ux, _, uz]| {
                // Along the path and a little aside, so the victim leaves the boulder's way.
                let (ax, az) = (ux - centre[0], uz - centre[2]);
                let away = ax.hypot(az).max(1.0);
                [rx + 0.6 * ax / away, rz + 0.6 * az / away]
            });
            match victim {
                EntityRef::Player(player) => {
                    self.creature_damages_player(id, player, amount, false)?;
                }
                EntityRef::Creature(creature) => {
                    if let Some(pusher) = state.pusher {
                        self.player_damages_creature(pusher, creature, amount, false, now)?;
                    }
                }
                _ => {}
            }
            #[allow(clippy::cast_possible_truncation)]
            let direction = direction.map(|axis| (axis * 1_000.0).round() as i64);
            self.knock_back(victim, direction, IMPACT_KNOCKBACK_UNITS, None);
        }
        Ok(())
    }

    /// A boulder idle away from home returns once its spot is clear (the trap recurs).
    fn boulder_returns(&mut self, id: CreatureId) -> Result<(), ZoneError> {
        let Some(mut motion) = self.prop_motion.get(&id).copied() else {
            return Ok(());
        };
        let Some(mut state) = motion.boulder else {
            return Ok(());
        };
        let (spawn, template) = Self::creature_content(&self.content, id)?;
        let [x, z] = spawn.position;
        let y = self.stand_y(x, z, template.half_extents);
        let home = crate::physics::from_units([x, y, z]);
        let away = (motion.position[0] - home[0]).hypot(motion.position[2] - home[2]) > HOME_UNITS;
        if !away && motion.velocity == [0.0; 3] {
            if state.idle != 0 {
                state.idle = 0;
                motion.boulder = Some(state);
                self.prop_motion.insert(id, motion);
            }
            return Ok(());
        }
        state.idle = state.idle.saturating_add(1);
        motion.boulder = Some(state);
        if state.idle < RETURN_TICKS || self.prop_spot_taken(id)? {
            self.prop_motion.insert(id, motion);
            return Ok(());
        }
        motion.position = home;
        motion.velocity = [0.0; 3];
        motion.boulder = Some(BoulderState::default());
        self.prop_motion.insert(id, motion);
        let body = body_id(EntityRef::Creature(id));
        let radius = template.half_extents[0];
        self.world
            .replace_body(RigidBody::sphere(body, Vec3i::new(x, y, z), radius))
            .map_err(physics_error)?;
        self.place_in_interest(EntityRef::Creature(id), x, z);
        Ok(())
    }

    /// Every boulder's canonical state, in creature-ID order.
    pub(crate) fn canonical_boulders(&self) -> Vec<CanonicalBoulder> {
        self.prop_motion
            .iter()
            .filter_map(|(&creature_id, motion)| {
                motion.boulder.map(|state| CanonicalBoulder {
                    creature_id,
                    spin: state.spin,
                    pusher: state.pusher,
                    idle: state.idle,
                })
            })
            .collect()
    }

    /// Restores canonical boulder state; each record must name a standing boulder, with no
    /// boulder left out or repeated.
    pub(crate) fn restore_boulders(
        &mut self,
        boulders: &[CanonicalBoulder],
    ) -> Result<(), ZoneError> {
        let expected = self.boulder_ids();
        if boulders.len() != expected.len() {
            return Err(ZoneError::new("snapshot boulders do not match the zone"));
        }
        for (record, id) in boulders.iter().zip(expected) {
            if record.creature_id != id || !record.spin.iter().all(|axis| axis.is_finite()) {
                return Err(ZoneError::new("snapshot boulders do not match the zone"));
            }
            let motion = self
                .prop_motion
                .get_mut(&id)
                .ok_or_else(|| ZoneError::new("snapshot boulders do not match the zone"))?;
            motion.boulder = Some(BoulderState {
                spin: record.spin,
                pusher: record.pusher,
                idle: record.idle,
            });
        }
        Ok(())
    }
}
