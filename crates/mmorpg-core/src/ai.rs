//! Deterministic creature AI (tick step 2), in creature-ID order.
//!
//! - **Idle**: aggressive creatures engage the nearest living player within
//!   their aggro radius, found through the zone's spatial index. Otherwise
//!   they wait 5–15 s (zone RNG), then walk to a random point within their
//!   wander radius at walking speed.
//! - **Engaged**: the target is the threat leader. The creature steers
//!   straight at it at run speed until it is inside melee reach and faces it;
//!   combat swings on the swing timer. An empty threat table or straying
//!   beyond the leash radius from the spawn point starts evading.
//! - **Evading**: run home, ignoring damage, with a cleared threat table; on
//!   arrival reset to full health and idle. A creature still away after
//!   10 s is moved home through the physics API.
//!
//! When a creature engages from idle, idle creatures of its family within
//! the assist radius engage the same target (one level, no chains). Steering
//! sets body velocity only; physics resolves obstacles and contact.

use std::sync::Arc;

use physics_engine::Vec3i;

use crate::creature::{Life, aggro_radius};
use crate::entity::body_id;
use crate::rng::ZoneRng;
use crate::snapshot::CreatureAi;
use crate::trig::{self, xz_length, yaw_from_vector};
use crate::zone::physics_error;
use crate::{
    ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS, CREATURE_EVADE_SPEED_UNITS_PER_TICK,
    CREATURE_RUN_SPEED_UNITS_PER_TICK, CREATURE_WALK_SPEED_UNITS_PER_TICK, CreatureBehaviour,
    CreatureId, EVADE_TIMEOUT_TICKS, EntityRef, LEASH_RADIUS_UNITS, PLAYER_HALF_EXTENTS_UNITS,
    PlayerId, WANDER_WAIT_TICKS, WANDER_WALK_TIMEOUT_TICKS, ZoneError, ZoneSimulation,
    unit::CREATURE_REACH_UNITS,
};

/// Squared XZ distance, saturating at `i64::MAX`. Only recovered player
/// positions near opposite ends of the `i32` range get that far apart, and
/// every radius test fails there as it should.
pub(crate) fn distance_squared(from: [i32; 2], to: [i32; 2]) -> i64 {
    let dx = i64::from(to[0]) - i64::from(from[0]);
    let dz = i64::from(to[1]) - i64::from(from[1]);
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}

/// The XZ offset from `from` to `to` that steering follows. An offset beyond
/// the `i32` range, which only a recovered extreme player position produces,
/// is halved on both axes, which keeps its direction.
fn offset(from: [i32; 2], to: [i32; 2]) -> (i32, i32) {
    let dx = i64::from(to[0]) - i64::from(from[0]);
    let dz = i64::from(to[1]) - i64::from(from[1]);
    match (i32::try_from(dx), i32::try_from(dz)) {
        (Ok(dx), Ok(dz)) => (dx, dz),
        // |dx|, |dz| < 2^32, so each half fits.
        _ => (
            i32::try_from(dx / 2).unwrap_or(0),
            i32::try_from(dz / 2).unwrap_or(0),
        ),
    }
}

/// Whether `to` is a wander point of a spawn at `home`: the direction table
/// and its rounding place a point at most one unit beyond the radius.
pub(crate) fn is_wander_point(home: [i32; 2], to: [i32; 2], radius: i32) -> bool {
    distance_squared(home, to) <= (i64::from(radius) + 1).pow(2)
}

/// The point `distance` units from `home` along `yaw`.
fn wander_point(home: [i32; 2], yaw: u16, distance: i32) -> [i32; 2] {
    let (x, z) = trig::direction(yaw);
    let offset = |unit| trig::checked_scale(distance, unit).unwrap_or(0);
    [
        home[0].saturating_add(offset(x)),
        home[1].saturating_add(offset(z)),
    ]
}

/// Velocity of at most `speed` along `(dx, dz)` without overshooting it.
/// Integer normalisation: each component is `delta · step / length`, rounded
/// toward zero.
fn steer(dx: i32, dz: i32, speed: i32) -> [i32; 2] {
    let length = xz_length(dx, dz);
    if length == 0 {
        return [0, 0];
    }
    let length = i64::try_from(length).unwrap_or(i64::MAX);
    let step = i64::from(speed).min(length);
    let scale = |delta: i32| i32::try_from(i64::from(delta) * step / length).unwrap_or(0);
    [scale(dx), scale(dz)]
}

/// A chasing creature's last step aims this far inside its reach, so the
/// truncating integer steering can neither stall it just outside the reach
/// nor leave it there: a step of at least this length always moves it.
const CHASE_CLOSE_IN_UNITS: i64 = 10;

/// Chase velocity toward a target `(dx, dz)` away: run speed while beyond
/// `reach`, then a last step that ends inside it, and zero once inside.
fn chase_velocity(dx: i32, dz: i32, reach: i32) -> [i32; 2] {
    if distance_squared([0, 0], [dx, dz]) <= i64::from(reach).pow(2) {
        return [0, 0];
    }
    let gap = i64::try_from(xz_length(dx, dz))
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::from(reach));
    let step =
        i64::from(CREATURE_RUN_SPEED_UNITS_PER_TICK).min(gap.saturating_add(CHASE_CLOSE_IN_UNITS));
    steer(dx, dz, i32::try_from(step).unwrap_or(0))
}

/// How close a creature must get to hit `target`: its reach plus the
/// target's half-width.
pub(crate) const fn creature_reach(target: EntityRef) -> i32 {
    // Only players are attacked so far.
    match target {
        EntityRef::Player(_) | EntityRef::Creature(_) | EntityRef::Npc(_) => {
            CREATURE_REACH_UNITS + PLAYER_HALF_EXTENTS_UNITS[0]
        }
    }
}

impl ZoneSimulation {
    /// Step 2: every living creature decides and sets its body velocity.
    pub(crate) fn decide_creatures(&mut self) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.creatures.keys().copied().collect();
        for id in ids {
            self.decide_creature(id)?;
        }
        Ok(())
    }

    fn decide_creature(&mut self, id: CreatureId) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (spawn, template) = Self::creature_content(&content, id)?;
        let Some(creature) = self.creatures.get(&id) else {
            return Ok(());
        };
        if creature.life != Life::Alive {
            return Ok(());
        }
        self.tick_work.creature_ai_evaluations += 1;
        let body = body_id(EntityRef::Creature(id));
        let current = self
            .world
            .body(body)
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
        let (position, vertical) = (current.position(), current.velocity().y);
        let here = [position.x, position.z];
        let (idle, level) = (
            matches!(creature.ai, CreatureAi::Idle { .. }),
            creature.level,
        );

        if idle
            && template.behaviour == CreatureBehaviour::Aggressive
            && let Some(player) = self.aggro_candidate(here, level)?
        {
            self.engage(id, EntityRef::Player(player), true)?;
        }

        // Read the chase target before mutating; dead or departed players
        // already left every threat table.
        let chase = match self.creatures.get(&id) {
            Some(creature) if creature.ai == CreatureAi::Engaged => match creature.top_threat() {
                Some(target @ EntityRef::Player(player)) => {
                    let position = self.player_position(player)?;
                    Some((target, [position.x, position.z]))
                }
                _ => None,
            },
            _ => None,
        };

        let home = spawn.position;
        let leashed = distance_squared(home, here) > i64::from(LEASH_RADIUS_UNITS).pow(2);
        // An engaged creature that keeps fighting may cast its ability.
        let casting = match chase {
            Some((target, to)) if !leashed => self.try_creature_ability(id, target, here, to)?,
            _ => false,
        };
        let Some(creature) = self.creatures.get_mut(&id) else {
            return Ok(());
        };
        if creature.ai == CreatureAi::Engaged && (chase.is_none() || leashed) {
            creature.ai = CreatureAi::Evading { ticks: 0 };
            creature.threat.clear();
            creature.tapped_by = None;
            creature.combat_timer = 0;
            creature.reset_abilities();
        }
        let speed = crate::aura::speed_percent(&creature.auras);
        let stunned = crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun);

        let mut teleport_home = false;
        let (velocity, facing) = match creature.ai {
            CreatureAi::Idle { .. } => wander(
                &mut self.rng,
                &mut creature.ai,
                here,
                home,
                spawn.wander_radius,
            ),
            CreatureAi::Engaged => match chase {
                // A casting creature stands and faces its target.
                Some((_, to)) if casting => {
                    let (dx, dz) = offset(here, to);
                    ([0, 0], yaw_from_vector(dx, dz))
                }
                Some((target, to)) => {
                    let (dx, dz) = offset(here, to);
                    (
                        chase_velocity(dx, dz, creature_reach(target)),
                        yaw_from_vector(dx, dz),
                    )
                }
                None => ([0, 0], None),
            },
            CreatureAi::Evading { ticks } => {
                let (dx, dz) = offset(here, home);
                let arrived =
                    distance_squared(here, home) <= i64::from(ARRIVAL_RADIUS_UNITS).pow(2);
                if arrived || ticks >= EVADE_TIMEOUT_TICKS {
                    teleport_home = !arrived;
                    creature.health = template.max_health(creature.level);
                    creature.swing_timer = 0;
                    creature.combat_timer = 0;
                    creature.tapped_by = None;
                    creature.reset_abilities();
                    creature.ai = CreatureAi::Idle {
                        timer: wander_wait(&mut self.rng),
                        destination: None,
                    };
                    ([0, 0], None)
                } else {
                    creature.ai = CreatureAi::Evading { ticks: ticks + 1 };
                    (
                        steer(dx, dz, CREATURE_EVADE_SPEED_UNITS_PER_TICK),
                        yaw_from_vector(dx, dz),
                    )
                }
            }
        };
        // Stuns freeze a creature in place; roots stop it and snares slow it.
        let velocity = crate::aura::scale_velocity(velocity, speed);
        if let Some(facing) = facing
            && !stunned
        {
            creature.facing = facing;
        }
        if teleport_home {
            self.world
                .set_position(body, Vec3i::new(home[0], template.half_extents[1], home[1]))
                .map_err(physics_error)?;
        }
        self.world
            .set_velocity(body, Vec3i::new(velocity[0], vertical, velocity[1]))
            .map_err(physics_error)
    }

    /// The nearest living player inside its aggro radius, ties to the lowest ID.
    fn aggro_candidate(&self, here: [i32; 2], level: u8) -> Result<Option<PlayerId>, ZoneError> {
        let mut best: Option<(i64, PlayerId)> = None;
        for entity in self
            .interest
            .near(here[0], here[1], crate::AGGRO_MAX_RADIUS_UNITS)
        {
            let EntityRef::Player(player_id) = entity else {
                continue;
            };
            let Some(player) = self.players.get(&player_id) else {
                continue;
            };
            if !player.is_alive() {
                continue;
            }
            let position = self.player_position(player_id)?;
            let distance = distance_squared(here, [position.x, position.z]);
            let radius = i64::from(aggro_radius(level, player.level));
            if distance <= radius * radius && best.is_none_or(|best| (distance, player_id) < best) {
                best = Some((distance, player_id));
            }
        }
        Ok(best.map(|(_, player_id)| player_id))
    }

    /// Puts `target` on a creature's threat table and engages it. Evading
    /// creatures ignore it. An idle creature engaging with `assist` pulls in
    /// idle creatures of its family within the assist radius.
    pub(crate) fn engage(
        &mut self,
        id: CreatureId,
        target: EntityRef,
        assist: bool,
    ) -> Result<(), ZoneError> {
        let Some(creature) = self.creatures.get_mut(&id) else {
            return Ok(());
        };
        if !creature.is_alive() || matches!(creature.ai, CreatureAi::Evading { .. }) {
            return Ok(());
        }
        let was_idle = matches!(creature.ai, CreatureAi::Idle { .. });
        creature.add_threat(target, 0);
        creature.ai = CreatureAi::Engaged;
        if was_idle {
            self.arm_creature_ability(id)?;
        }
        if !(was_idle && assist) {
            return Ok(());
        }
        let content = Arc::clone(&self.content);
        let (_, template) = Self::creature_content(&content, id)?;
        let position = self
            .world
            .body(body_id(EntityRef::Creature(id)))
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
            .position();
        let here = [position.x, position.z];
        for entity in self.interest.near(here[0], here[1], ASSIST_RADIUS_UNITS) {
            let EntityRef::Creature(other) = entity else {
                continue;
            };
            if other == id {
                continue;
            }
            let Some(helper) = self.creatures.get(&other) else {
                continue;
            };
            if !helper.is_alive() || !matches!(helper.ai, CreatureAi::Idle { .. }) {
                continue;
            }
            let (_, helper_template) = Self::creature_content(&content, other)?;
            if helper_template.family != template.family {
                continue;
            }
            let helper_position = self
                .world
                .body(body_id(entity))
                .ok_or_else(|| ZoneError::new("creature physics body is missing"))?
                .position();
            let distance = distance_squared(here, [helper_position.x, helper_position.z]);
            if distance <= i64::from(ASSIST_RADIUS_UNITS).pow(2) {
                self.engage(other, target, false)?;
            }
        }
        Ok(())
    }
}

fn wander_wait(rng: &mut ZoneRng) -> u16 {
    let wait = rng.inclusive(
        u32::from(WANDER_WAIT_TICKS[0]),
        u32::from(WANDER_WAIT_TICKS[1]),
    );
    u16::try_from(wait).unwrap_or(WANDER_WAIT_TICKS[1])
}

/// Idle behaviour: count down, pick a wander point, walk to it, repeat.
/// Returns the horizontal velocity and a new facing, if any.
fn wander(
    rng: &mut ZoneRng,
    ai: &mut CreatureAi,
    here: [i32; 2],
    home: [i32; 2],
    radius: i32,
) -> ([i32; 2], Option<u16>) {
    let CreatureAi::Idle { timer, destination } = ai else {
        return ([0, 0], None);
    };
    match *destination {
        Some(to) => {
            let arrived = distance_squared(here, to) <= i64::from(ARRIVAL_RADIUS_UNITS).pow(2);
            if arrived || *timer == 0 {
                *destination = None;
                *timer = wander_wait(rng);
                ([0, 0], None)
            } else {
                *timer -= 1;
                let (dx, dz) = offset(here, to);
                (
                    steer(dx, dz, CREATURE_WALK_SPEED_UNITS_PER_TICK),
                    yaw_from_vector(dx, dz),
                )
            }
        }
        None if *timer > 0 => {
            *timer -= 1;
            ([0, 0], None)
        }
        None => {
            // A random heading and distance around the spawn point.
            let yaw = u16::try_from(rng.below(1 << 16)).unwrap_or(0);
            let distance = i32::try_from(rng.inclusive(0, radius.unsigned_abs())).unwrap_or(0);
            let to = wander_point(home, yaw, distance);
            *destination = Some(to);
            *timer = WANDER_WALK_TIMEOUT_TICKS;
            let (dx, dz) = offset(here, to);
            (
                steer(dx, dz, CREATURE_WALK_SPEED_UNITS_PER_TICK),
                yaw_from_vector(dx, dz),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_normalises_with_integers_and_never_overshoots() {
        assert_eq!(steer(0, 0, 19), [0, 0]);
        assert_eq!(steer(300, 400, 19), [11, 15]);
        assert_eq!(steer(-300, 400, 19), [-11, 15]);
        assert_eq!(steer(3, 4, 19), [3, 4], "a short step lands on the target");
        assert_eq!(steer(1_000, 0, 24), [24, 0]);
    }

    #[test]
    fn distances_and_offsets_never_overflow() {
        let (low, high) = ([i32::MIN, i32::MIN], [i32::MAX, i32::MAX]);
        assert_eq!(distance_squared(low, high), i64::MAX);
        assert_eq!(distance_squared([i32::MIN, 0], [i32::MAX, 0]), i64::MAX);
        assert_eq!(distance_squared([-3, 4], [0, 0]), 25);
        assert_eq!(offset([5, -7], [2, 9]), (-3, 16));
        assert_eq!(offset(low, high), (i32::MAX, i32::MAX));
        assert_eq!(offset(high, low), (-i32::MAX, -i32::MAX));
        assert_eq!(offset([i32::MAX, 0], [i32::MIN, 10]), (-i32::MAX, 5));
    }

    /// Recovery accepts every destination wandering can choose.
    #[test]
    fn every_wander_point_lies_within_the_recovery_bound() {
        let home = [-31_000, 29_500];
        for radius in [0, 1, 7, 600, crate::MAX_WANDER_RADIUS_UNITS] {
            for yaw in 0..=u16::MAX {
                for distance in [0, radius / 2, radius] {
                    let to = wander_point(home, yaw, distance);
                    assert!(
                        is_wander_point(home, to, radius),
                        "{radius} {yaw} {distance}"
                    );
                }
            }
            assert!(!is_wander_point(
                home,
                [home[0] + radius + 2, home[1]],
                radius
            ));
        }
    }

    /// Against the plain rule "outside reach, get closer; near it, arrive
    /// inside": every offset outside reach moves, never away, and every
    /// offset within one close-in step of the reach lands inside it.
    #[test]
    fn a_chase_always_progresses_and_its_last_step_lands_inside_reach() {
        let reach = creature_reach(EntityRef::Player(1));
        let reach_squared = i64::from(reach).pow(2);
        assert_eq!(
            chase_velocity(203, -109, reach),
            [8, -4],
            "no stall at 230.4"
        );
        for dx in (-300..=300).step_by(3) {
            for dz in (-300..=300).step_by(3) {
                let before = distance_squared([0, 0], [dx, dz]);
                let [vx, vz] = chase_velocity(dx, dz, reach);
                if before <= reach_squared {
                    assert_eq!([vx, vz], [0, 0], "({dx}, {dz}) is inside reach");
                    continue;
                }
                let after = distance_squared([0, 0], [dx - vx, dz - vz]);
                assert!(after < before, "({dx}, {dz}) makes progress");
                let within_last_step = i64::try_from(xz_length(dx, dz)).unwrap()
                    <= i64::from(reach) + i64::from(CREATURE_RUN_SPEED_UNITS_PER_TICK)
                        - CHASE_CLOSE_IN_UNITS;
                if within_last_step {
                    assert!(after <= reach_squared, "({dx}, {dz}) arrives inside reach");
                }
            }
        }
    }
}
