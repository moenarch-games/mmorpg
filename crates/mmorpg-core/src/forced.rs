//! Forced movement (#287, Atlas 5.2.72): knockbacks, pushes and charges.
//!
//! Large creatures (#297) shove: a large creature whose step this tick would carry it into a
//! smaller living unit pushes that unit [`SHOVE_UNITS`] aside, off its path, as a forced move
//! with the same immunity. While the unit is immune the creature's body simply blocks it.
//!
//! A forced move overrides a unit's horizontal motion for a few ticks: the zone sets the body's
//! velocity right before the physics step, so Rapier's character controller still resolves it
//! against the world — a wall stops it, and a move the world stopped ends early. Every forced
//! move is followed by [`FORCED_IMMUNITY_TICKS`] of immunity, so nobody is chain-juggled; a unit
//! already being moved or still immune ignores further forced moves. Players (including PvP
//! later) and creatures follow the same rules.

use std::collections::BTreeMap;

use crate::entity::{EntityRef, body_id};
use crate::physics::Vec3i;
use crate::{ZoneError, ZoneSimulation};

/// Immunity after a forced move ends: 1 s.
pub const FORCED_IMMUNITY_TICKS: u16 = 30;
/// A knockback covers its distance in this many ticks.
pub const KNOCKBACK_TICKS: u16 = 8;
/// A charge rushes at 13.5 m/s for at most 1 s.
pub const CHARGE_SPEED_UNITS: i32 = 45;
pub const CHARGE_MAX_TICKS: u16 = 30;
/// A charge's path (#57) must be clear for the charger's body above this height (units over
/// its feet): knee height, so the ground, slopes, kerbs and campfires it runs over do not
/// count. Even, so the raised capsule stays whole units; obstacles exactly as high as the
/// clearance would graze the sweep, where the backends' contact tolerances differ.
pub const CHARGE_CLEARANCE_UNITS: i32 = 32;
/// How far (units) a large creature (#297) shoves a smaller unit in its way.
pub const SHOVE_UNITS: i32 = 120;
/// A unit counts as in a large creature's way when its body is within the creature's step
/// plus this margin (units) of touching it: the contact offsets both bodies keep, with room.
const SHOVE_CONTACT_UNITS: i64 = 4;

/// One forced move in progress.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ForcedMove {
    /// Horizontal velocity, units per tick.
    pub velocity: [i32; 2],
    pub remaining: u16,
    /// A charge steers toward this unit and knocks it back this many units on arrival.
    pub charge: Option<(EntityRef, i32)>,
}

/// A unit's forced-movement state: the move in progress and the immunity left after it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ForcedState {
    pub moving: Option<ForcedMove>,
    pub immune: u16,
}

impl ForcedState {
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.moving.is_none() && self.immune == 0
    }
}

/// `direction` (XZ) scaled to `length`, or `None` for a zero direction.
fn scaled(direction: [i64; 2], length: i32) -> Option<[i32; 2]> {
    let norm = (direction[0] * direction[0] + direction[1] * direction[1]).isqrt();
    if norm == 0 {
        return None;
    }
    Some(direction.map(|axis| i32::try_from(axis * i64::from(length) / norm).unwrap_or(0)))
}

impl ZoneSimulation {
    fn unit_alive_now(&self, unit: EntityRef) -> bool {
        match unit {
            EntityRef::Player(id) => self.players.get(&id).is_some_and(|p| p.is_alive()),
            EntityRef::Creature(id) => self.creatures.get(&id).is_some_and(|c| c.is_alive()),
            EntityRef::Npc(_) => false,
        }
    }

    /// Whether `unit` can be force-moved now: alive, not a fixed prop (#288), not moving and not
    /// immune.
    fn movable(&self, unit: EntityRef) -> bool {
        self.unit_alive_now(unit)
            && !self.is_prop(unit)
            && self.forced.get(&unit).is_none_or(ForcedState::is_settled)
    }

    /// Knocks `unit` `distance` units along `direction` (XZ), unless it is immune; returns
    /// whether it moves.
    pub(crate) fn knock_back(
        &mut self,
        unit: EntityRef,
        direction: [i64; 2],
        distance: i32,
    ) -> bool {
        if distance <= 0 || !self.movable(unit) {
            return false;
        }
        let ticks = i32::from(KNOCKBACK_TICKS);
        let Some(velocity) = scaled(direction, (distance + ticks - 1) / ticks) else {
            return false;
        };
        self.forced.insert(
            unit,
            ForcedState {
                moving: Some(ForcedMove {
                    velocity,
                    remaining: KNOCKBACK_TICKS,
                    charge: None,
                }),
                immune: 0,
            },
        );
        true
    }

    /// Knocks `unit` `distance` units away from `from` (XZ units); straight along `fallback`
    /// when it stands on `from`.
    pub(crate) fn knock_away(
        &mut self,
        unit: EntityRef,
        from: [i32; 2],
        fallback: [i64; 2],
        distance: i32,
    ) -> Result<bool, ZoneError> {
        // A prop broken by the same blast (#288) has no position left.
        if !self.movable(unit) {
            return Ok(false);
        }
        let at = self.unit_position(unit)?;
        let away = [i64::from(at[0] - from[0]), i64::from(at[2] - from[1])];
        let direction = if away == [0, 0] { fallback } else { away };
        Ok(self.knock_back(unit, direction, distance))
    }

    /// Whether `unit`'s body fits along the straight path of a charge at `target` (#57): its
    /// capsule, raised by [`CHARGE_CLEARANCE_UNITS`], swept horizontally toward the target until
    /// it would arrive within melee reach meets no static geometry but the ground.
    pub(crate) fn charge_path_clear(
        &self,
        unit: EntityRef,
        target: EntityRef,
    ) -> Result<bool, ZoneError> {
        let body = *self
            .world
            .body(body_id(unit))
            .ok_or_else(|| ZoneError::new("charging unit physics body is missing"))?;
        let here = body.exact_position();
        let there = self.unit_position(target)?;
        #[allow(clippy::cast_precision_loss)]
        let delta = [there[0] as f32 - here[0], there[2] as f32 - here[2]];
        let distance = delta[0].hypot(delta[1]);
        #[allow(clippy::cast_precision_loss)]
        let travel = distance - crate::unit::PLAYER_REACH_UNITS as f32;
        if travel <= 0.0 {
            return Ok(true);
        }
        let half = body.half_extents();
        let lift = CHARGE_CLEARANCE_UNITS / 2;
        #[allow(clippy::cast_precision_loss)]
        let origin = [here[0], here[1] + lift as f32, here[2]];
        let scale = travel / distance;
        let ground = self.ground_body();
        let hit = self.world.cast_shape(
            origin,
            Vec3i::new(half.x, half.y - lift, half.z),
            [delta[0] * scale, 0.0, delta[1] * scale],
            |body| body.0 < crate::entity::PLAYER_BODY_BASE && Some(body) != ground,
        );
        Ok(hit.is_none())
    }

    /// `unit` charges at `target`, knocking it back `knockback` units when it arrives.
    pub(crate) fn charge(&mut self, unit: EntityRef, target: EntityRef, knockback: i32) -> bool {
        if !self.movable(unit) {
            return false;
        }
        self.forced.insert(
            unit,
            ForcedState {
                moving: Some(ForcedMove {
                    velocity: [0, 0],
                    remaining: CHARGE_MAX_TICKS,
                    charge: Some((target, knockback)),
                }),
                immune: 0,
            },
        );
        true
    }

    fn end_forced_move(&mut self, unit: EntityRef) {
        if let Some(state) = self.forced.get_mut(&unit) {
            state.moving = None;
            state.immune = FORCED_IMMUNITY_TICKS;
        }
    }

    /// Before forced moves apply (#297): each large creature, in ID order, shoves every
    /// smaller living unit its step this tick would run into aside (in `EntityRef` order).
    pub(crate) fn shove_with_large_creatures(&mut self) -> Result<(), ZoneError> {
        let content = std::sync::Arc::clone(&self.content);
        let large: Vec<_> = self
            .creatures
            .iter()
            .filter(|(_, creature)| creature.is_alive())
            .filter_map(|(&id, _)| {
                let (_, template) = Self::creature_content(&content, id).ok()?;
                template.is_large().then_some((id, template))
            })
            .collect();
        for (id, template) in large {
            let shover = EntityRef::Creature(id);
            // A creature being moved itself does not shove.
            if self
                .forced
                .get(&shover)
                .is_some_and(|state| state.moving.is_some())
            {
                continue;
            }
            let Some(body) = self.world.body(body_id(shover)).copied() else {
                continue;
            };
            let velocity = body.velocity();
            let step = [i64::from(velocity.x), i64::from(velocity.z)];
            if step == [0, 0] {
                continue;
            }
            let here = body.position();
            let speed = (step[0] * step[0] + step[1] * step[1]).isqrt();
            let width = i64::from(template.half_width());
            let mut near = self.interest.near(
                here.x,
                here.z,
                i32::try_from(2 * width + speed + SHOVE_CONTACT_UNITS).unwrap_or(i32::MAX),
            );
            near.sort_unstable();
            for unit in near {
                if unit == shover || !self.movable(unit) {
                    continue;
                }
                let Some(other) = self.world.body(body_id(unit)).copied() else {
                    continue;
                };
                let other_half = other.half_extents();
                let other_width = i64::from(other_half.x.max(other_half.z));
                if other_width >= width {
                    continue;
                }
                let there = other.position();
                let delta = [
                    i64::from(there.x) - i64::from(here.x),
                    i64::from(there.z) - i64::from(here.z),
                ];
                let reach = width + other_width;
                // Ahead of the step, within it of touching, and across its path.
                let along = delta[0] * step[0] + delta[1] * step[1];
                let across = (delta[0] * step[1] - delta[1] * step[0]).abs();
                let vertical = (i64::from(there.y) - i64::from(here.y)).abs();
                let distance = (delta[0] * delta[0] + delta[1] * delta[1]).isqrt();
                if along <= 0
                    || across >= reach * speed
                    || distance > reach + speed + SHOVE_CONTACT_UNITS
                    || vertical >= i64::from(body.half_extents().y + other_half.y)
                {
                    continue;
                }
                // Aside, off the creature's path: the unit's offset across the step, or to the
                // step's right for a unit dead ahead.
                let squared = step[0] * step[0] + step[1] * step[1];
                let aside = [
                    delta[0] * squared - step[0] * along,
                    delta[1] * squared - step[1] * along,
                ];
                let direction = if aside == [0, 0] {
                    [step[1], -step[0]]
                } else {
                    aside
                };
                self.knock_back(unit, direction, SHOVE_UNITS);
            }
        }
        Ok(())
    }

    /// Before the physics step: every forced move sets its unit's horizontal velocity; a
    /// charge that reached its target ends and knocks it back.
    pub(crate) fn apply_forced_moves(&mut self) -> Result<(), ZoneError> {
        let units: Vec<_> = self.forced.keys().copied().collect();
        for unit in units {
            if !self.unit_alive_now(unit) {
                self.forced.remove(&unit);
                continue;
            }
            let Some(state) = self.forced.get(&unit).copied() else {
                continue;
            };
            let Some(mut moving) = state.moving else {
                let immune = state.immune.saturating_sub(1);
                if immune == 0 {
                    self.forced.remove(&unit);
                } else if let Some(state) = self.forced.get_mut(&unit) {
                    state.immune = immune;
                }
                continue;
            };
            if moving.remaining == 0 {
                self.end_forced_move(unit);
                continue;
            }
            if let Some((target, knockback)) = moving.charge {
                if !self.unit_alive_now(target) {
                    self.end_forced_move(unit);
                    continue;
                }
                let here = self.unit_position(unit)?;
                let there = self.unit_position(target)?;
                let delta = [i64::from(there[0] - here[0]), i64::from(there[2] - here[2])];
                let reach = i64::from(crate::unit::PLAYER_REACH_UNITS);
                if delta[0] * delta[0] + delta[1] * delta[1] <= reach * reach {
                    self.end_forced_move(unit);
                    self.knock_back(target, delta, knockback);
                    continue;
                }
                moving.velocity = scaled(delta, CHARGE_SPEED_UNITS).unwrap_or([0, 0]);
            }
            moving.remaining -= 1;
            let body = body_id(unit);
            let vertical = self
                .world
                .body(body)
                .ok_or_else(|| ZoneError::new("forced unit physics body is missing"))?
                .velocity()
                .y;
            self.world
                .set_velocity(
                    body,
                    Vec3i::new(moving.velocity[0], vertical, moving.velocity[1]),
                )
                .map_err(crate::zone::physics_error)?;
            if let Some(state) = self.forced.get_mut(&unit) {
                state.moving = Some(moving);
            }
        }
        Ok(())
    }

    /// After the physics step: a forced move the world stopped (a wall) ends early.
    pub(crate) fn settle_forced_moves(&mut self) {
        let stopped: Vec<_> = self
            .forced
            .iter()
            .filter_map(|(&unit, state)| {
                let moving = state.moving?;
                let realized = self.world.body(body_id(unit))?.exact_velocity();
                let wanted = [moving.velocity[0], moving.velocity[1]].map(|axis| axis as f32);
                let along = realized[0] * wanted[0] + realized[2] * wanted[1];
                let full = wanted[0] * wanted[0] + wanted[1] * wanted[1];
                (full > 0.0 && along < full / 4.0).then_some(unit)
            })
            .collect();
        for unit in stopped {
            self.end_forced_move(unit);
        }
    }

    /// The forced-movement states, for canonical snapshots.
    pub(crate) fn forced_states(&self) -> Vec<(EntityRef, ForcedState)> {
        self.forced
            .iter()
            .map(|(&unit, &state)| (unit, state))
            .collect()
    }

    pub(crate) fn restore_forced(
        &mut self,
        states: Vec<(EntityRef, ForcedState)>,
    ) -> Result<(), ZoneError> {
        let mut forced = BTreeMap::new();
        for (unit, state) in states {
            if state.is_settled()
                || state.immune > FORCED_IMMUNITY_TICKS
                || !self.unit_alive_now(unit)
                || forced.insert(unit, state).is_some()
            {
                return Err(ZoneError::new("snapshot forced movement is invalid"));
            }
        }
        self.forced = forced;
        Ok(())
    }
}
