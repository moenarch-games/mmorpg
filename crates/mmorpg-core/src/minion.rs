//! Summoned minions (#295, Atlas 5.2.84): a Necromancer's skeletons and a Druid's companion.
//!
//! - **Companion** (#296): Call Companion calls one persistent wolf, a minion with no timer.
//!   Each owner keeps one; calling it while it stands brings it back to the owner's side with
//!   the health it has. It levels with its owner, obeys the same group orders, outlives its
//!   owner's death and goes only when killed or when its owner leaves the zone.
//!
//! - **Raising** (Raise Skeleton resolving, tick steps 1 or 6): a minion stands up one step
//!   behind its summoner at the summoner's level. A summoner keeps at most
//!   [`MAX_MINIONS_PER_SUMMONER`]; raising one more crumbles the oldest.
//! - **Group orders** (step 1): the summoner orders all its minions at once to *attack* a
//!   visible living creature, *defend* a point, *follow* or *retreat*. The zone checks the
//!   summoner is alive, has minions, and that the target or point lies within
//!   [`ORDER_RANGE_UNITS`]; a refusal is an `Error` event.
//! - **Decisions** (step 2, after the creatures, in minion-ID order): between orders minions
//!   fight on their own. Following minions take on the creatures fighting their summoner or
//!   its minions near the summoner; defending ones any engaged or aggressive creature near the
//!   point; attacking ones their ordered target until it dies or escapes, then they follow
//!   again; retreating ones ignore every enemy and run to the summoner, then follow. Idle
//!   minions form up in an arc behind the summoner (a ring round a defended point) and keep a
//!   little apart from one another; physics resolves walls and contact.
//! - **Swings** (step 6, after the creatures): a minion inside reach of its target swings on
//!   its own timer. Creatures put the minion on their threat table and fight it; the summoner
//!   taps the creature and earns the kill.
//! - **Crumbling** (step 7): a minion crumbles when its time runs out, when its health reaches
//!   zero, and with its summoner's death; it vanishes without a corpse when the summoner leaves.
//!
//! Minions fight creatures only: players never attack them, and they never attack players.

use std::collections::BTreeSet;

use crate::ability::{AbilityEffect, AbilityId, scaled};
use crate::ai::{distance_squared, offset, steer};
use crate::entity::body_id;
use crate::physics::{RigidBody, Vec3i};
use crate::snapshot::CreatureAi;
use crate::trig::{self, yaw_from_vector};
use crate::unit::{COMBAT_LINGER_TICKS, CREATURE_REACH_UNITS, Swing, roll_swing};
use crate::zone::{physics_error, vector};
use crate::{
    CreatureBehaviour, CreatureId, EntityRef, ErrorCode, MinionId, PlayerId, ZoneError, ZoneEvent,
    ZoneSimulation,
};

/// One summoner commands at most this many minions at once.
pub const MAX_MINIONS_PER_SUMMONER: usize = 4;
/// Every minion of a full zone.
pub const MAX_MINIONS: usize = crate::MAX_PLAYERS_PER_ZONE * MAX_MINIONS_PER_SUMMONER;
/// A skeleton's collision box: 0.5 m × 1.6 m × 0.5 m.
pub const MINION_HALF_EXTENTS_UNITS: [i32; 3] = [25, 80, 25];
/// A companion wolf's collision box (#296): 0.5 m × 0.8 m × 0.5 m.
pub const COMPANION_HALF_EXTENTS_UNITS: [i32; 3] = [25, 40, 25];
/// Minions run as fast as their summoner (6.3 m/s) …
pub const MINION_RUN_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(630);
/// … and catch up or retreat faster (8.1 m/s).
pub const MINION_RUSH_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(810);
/// Formation slots lie this far from the summoner or the defended point (2 m).
pub const FORMATION_RADIUS_UNITS: i32 = 200;
/// A minion this close to its slot stands still.
pub const FORMATION_SLACK_UNITS: i32 = 40;
/// Beyond this distance from its slot a following minion rushes (6 m).
pub const RUSH_DISTANCE_UNITS: i32 = 600;
/// Minions closer than this to one another step apart (0.8 m).
pub const SEPARATION_RADIUS_UNITS: i32 = 80;
/// Following minions take on enemies of the party within this distance of the summoner (15 m).
pub const ENGAGE_RADIUS_UNITS: i32 = 1_500;
/// Defending minions take on enemies within this distance of their point (8 m).
pub const DEFEND_RADIUS_UNITS: i32 = 800;
/// A fight that strays this far from the summoner is dropped (30 m).
pub const MINION_LEASH_RADIUS_UNITS: i32 = 3_000;
/// A minion this far from its summoner is gathered back beside it (60 m).
pub const MINION_GATHER_RADIUS_UNITS: i32 = 6_000;
/// Orders reach a target or point within this distance of the summoner (30 m).
pub const ORDER_RANGE_UNITS: i32 = 3_000;

/// What a summoner last ordered its minions to do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MinionOrder {
    /// Stay with the summoner and fight what fights the party (the default).
    Follow,
    /// Fight this creature until it dies or escapes, then follow.
    Attack(EntityRef),
    /// Hold this XZ point and fight what comes near it.
    Defend([i32; 2]),
    /// Ignore every enemy and run back to the summoner, then follow.
    Retreat,
}

/// One summoned minion's authoritative state; its body lives in physics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Minion {
    pub id: MinionId,
    pub summoner: PlayerId,
    /// The summoning ability; its effect sets health, damage and swing time.
    pub ability: AbilityId,
    /// The summoner's level when it was raised.
    pub level: u8,
    pub health: u32,
    pub facing: u16,
    pub order: MinionOrder,
    /// The creature it is fighting.
    pub target: Option<EntityRef>,
    pub swing_timer: u16,
    /// Ticks until it crumbles; always zero for a persistent companion (#296).
    pub remaining: u16,
}

/// A summoning ability's numbers.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MinionStats {
    pub(crate) max_health: u32,
    pub(crate) damage: [u16; 2],
    pub(crate) swing_ticks: u16,
    /// Ticks until it crumbles; `None` for a persistent companion (#296).
    pub(crate) duration: Option<u16>,
    /// How many of this ability's minions one summoner keeps at once.
    pub(crate) limit: usize,
}

/// The stats a `level` caster's `ability` raises, if it summons or calls a companion.
pub(crate) fn minion_stats(ability: AbilityId, level: u8) -> Option<MinionStats> {
    let (health, health_per_level, damage, damage_per_level, swing_ticks, duration, limit) =
        match crate::ability_by_id(ability)?.effect {
            AbilityEffect::Summon {
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                duration,
            } => (
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                Some(duration),
                MAX_MINIONS_PER_SUMMONER,
            ),
            AbilityEffect::Companion {
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
            } => (
                health,
                health_per_level,
                damage,
                damage_per_level,
                swing_ticks,
                None,
                1,
            ),
            _ => return None,
        };
    let bonus = scaled(0, damage_per_level, level);
    Some(MinionStats {
        max_health: u32::from(scaled(health, health_per_level, level)),
        damage: [
            damage[0].saturating_add(bonus),
            damage[1].saturating_add(bonus),
        ],
        swing_ticks,
        duration,
        limit,
    })
}

/// Whether `ability` calls a persistent companion (#296) rather than a timed minion.
#[must_use]
pub fn is_companion(ability: AbilityId) -> bool {
    crate::ability_by_id(ability)
        .is_some_and(|row| matches!(row.effect, AbilityEffect::Companion { .. }))
}

/// The summoning ability a projected minion's appearance names (#295).
#[must_use]
pub fn minion_ability(appearance: u16) -> AbilityId {
    AbilityId::new(u8::try_from(appearance).unwrap_or(0))
}

/// The collision half extents of a minion raised by `ability`: a skeleton stands 1.6 m tall,
/// a companion wolf 0.8 m. Both are 0.5 m across, so creatures reach them alike.
#[must_use]
pub fn minion_half_extents(ability: AbilityId) -> [i32; 3] {
    if is_companion(ability) {
        COMPANION_HALF_EXTENTS_UNITS
    } else {
        MINION_HALF_EXTENTS_UNITS
    }
}

impl Minion {
    pub(crate) fn max_health(&self) -> u32 {
        minion_stats(self.ability, self.level).map_or(1, |stats| stats.max_health)
    }
}

/// The point `distance` units from `anchor` along `yaw`.
fn point_along(anchor: [i32; 2], yaw: u16, distance: i32) -> [i32; 2] {
    let (x, z) = trig::direction(yaw);
    let step = |unit| trig::checked_scale(distance, unit).unwrap_or(0);
    [
        anchor[0].saturating_add(step(x)),
        anchor[1].saturating_add(step(z)),
    ]
}

/// Formation slot `slot` of `count`: an arc 40° apart behind a summoner facing `facing`, or an
/// even ring round a defended point.
fn slot_point(anchor: [i32; 2], facing: u16, slot: usize, count: usize, ring: bool) -> [i32; 2] {
    let count = count.max(1);
    let yaw = if ring {
        u16::try_from(slot * 65_536 / count).unwrap_or(0)
    } else {
        const SPREAD: i32 = 65_536 * 40 / 360;
        let index = i32::try_from(slot).unwrap_or(0);
        let count = i32::try_from(count).unwrap_or(1);
        let turn = (2 * index - (count - 1)) * SPREAD / 2;
        facing
            .wrapping_add(0x8000)
            .wrapping_add_signed(i16::try_from(turn).unwrap_or(0))
    };
    point_along(anchor, yaw, FORMATION_RADIUS_UNITS)
}

impl ZoneSimulation {
    /// The minions of `summoner` in ID order (oldest first).
    pub(crate) fn minions_of(&self, summoner: PlayerId) -> Vec<MinionId> {
        self.minions
            .values()
            .filter(|minion| minion.summoner == summoner)
            .map(|minion| minion.id)
            .collect()
    }

    /// The summoner of a minion, or the player itself: who answers for a unit's deeds.
    pub(crate) fn controller(&self, unit: EntityRef) -> Option<PlayerId> {
        match unit {
            EntityRef::Player(player_id) => Some(player_id),
            EntityRef::Minion(id) => self.minions.get(&id).map(|minion| minion.summoner),
            EntityRef::Creature(_) | EntityRef::Npc(_) => None,
        }
    }

    /// The minions `summoner` raised with `ability`, in ID order.
    fn minions_by(&self, summoner: PlayerId, ability: AbilityId) -> Vec<MinionId> {
        self.minions
            .values()
            .filter(|minion| minion.summoner == summoner && minion.ability == ability)
            .map(|minion| minion.id)
            .collect()
    }

    /// Raises one minion of `ability` beside `summoner` (#295). A companion called again
    /// (#296) comes back to its owner's side with the health it has.
    pub(crate) fn raise_minion(
        &mut self,
        summoner: PlayerId,
        ability: AbilityId,
    ) -> Result<(), ZoneError> {
        let Some(player) = self.players.get(&summoner) else {
            return Ok(());
        };
        let (level, facing) = (player.level, player.facing);
        let Some(stats) = minion_stats(ability, level) else {
            return Ok(());
        };
        let mut health = stats.max_health;
        let owned = self.minions_by(summoner, ability);
        if owned.len() >= stats.limit
            && let Some(&oldest) = owned.first()
        {
            if stats.duration.is_none() {
                if let Some(pet) = self.dismiss_minion(oldest) {
                    health = pet.health.min(stats.max_health);
                }
            } else {
                self.minion_crumbles(oldest, None)?;
            }
        }
        if self.minions.len() >= MAX_MINIONS {
            return Ok(());
        }
        let Some(next) = self.next_minion.checked_add(1) else {
            return Ok(());
        };
        let id = MinionId::new(self.next_minion);
        self.next_minion = next;
        let centre = self.player_position(summoner)?;
        let here = [centre.x, centre.z];
        let slot = self.minions_of(summoner).len();
        let count = slot + 1;
        // The first clear spot of its formation slot, the other slots, or the summoner's back.
        let half = minion_half_extents(ability);
        let mut spot = slot_point(here, facing, slot, count, false);
        for candidate in
            (0..count).map(|index| slot_point(here, facing, (slot + index) % count, count, false))
        {
            let y = self.ground_y(candidate[0], candidate[1]) + half[1] + 1;
            #[allow(clippy::cast_precision_loss)]
            let probe = [candidate[0] as f32, y as f32, candidate[1] as f32];
            #[allow(clippy::cast_precision_loss)]
            let extent = half.map(|value| value as f32);
            let clear = self
                .world
                .overlap_box(probe, extent)
                .map_err(physics_error)?
                .into_iter()
                .all(|body| Some(body) == self.ground_body());
            if clear {
                spot = candidate;
                break;
            }
        }
        let position = Vec3i::new(spot[0], self.ground_y(spot[0], spot[1]) + half[1], spot[1]);
        let entity = EntityRef::Minion(id);
        self.world
            .add_body(RigidBody::dynamic(
                body_id(entity),
                position,
                Vec3i::ZERO,
                vector(half),
            ))
            .map_err(physics_error)?;
        self.place_in_interest(entity, position.x, position.z);
        self.minions.insert(
            id,
            Minion {
                id,
                summoner,
                ability,
                level,
                health,
                facing,
                order: MinionOrder::Follow,
                target: None,
                swing_timer: 0,
                remaining: stats.duration.unwrap_or(0),
            },
        );
        Ok(())
    }

    /// Validates and applies a summoner's group order (Atlas 5.2.84).
    pub(crate) fn command_minions(
        &mut self,
        summoner: PlayerId,
        order: MinionOrder,
    ) -> Result<crate::casting::Refusal, ZoneError> {
        let Some(player) = self.players.get(&summoner) else {
            return Ok(None);
        };
        if !player.is_alive() {
            return Ok(Some((ErrorCode::YouAreDead, None)));
        }
        let owned = self.minions_of(summoner);
        if owned.is_empty() {
            return Ok(Some((ErrorCode::NoMinions, None)));
        }
        let centre = self.player_position(summoner)?;
        let here = [centre.x, centre.z];
        let reach = i64::from(ORDER_RANGE_UNITS).pow(2);
        match order {
            MinionOrder::Attack(target) => {
                if !self.minion_may_fight(target) || !self.visible_to(summoner, target)? {
                    return Ok(Some((ErrorCode::InvalidTarget, Some(target))));
                }
                let at = self.unit_position(target)?;
                if distance_squared(here, [at[0], at[2]]) > reach {
                    return Ok(Some((ErrorCode::OutOfRange, Some(target))));
                }
            }
            MinionOrder::Defend(point) => {
                if distance_squared(here, point) > reach {
                    return Ok(Some((ErrorCode::OutOfRange, None)));
                }
            }
            MinionOrder::Follow | MinionOrder::Retreat => {}
        }
        for id in owned {
            if let Some(minion) = self.minions.get_mut(&id) {
                minion.order = order;
                minion.target = match order {
                    MinionOrder::Attack(target) => Some(target),
                    MinionOrder::Follow | MinionOrder::Defend(_) | MinionOrder::Retreat => None,
                };
            }
        }
        Ok(None)
    }

    /// Whether minions may fight `unit`: a living creature that is no prop and not evading.
    fn minion_may_fight(&self, unit: EntityRef) -> bool {
        let EntityRef::Creature(id) = unit else {
            return false;
        };
        !self.is_prop(unit)
            && self.creatures.get(&id).is_some_and(|creature| {
                creature.is_alive() && !matches!(creature.ai, CreatureAi::Evading { .. })
            })
    }

    /// Step 2, after the creatures: every minion picks its fight or formation slot and sets its
    /// body velocity.
    pub(crate) fn decide_minions(&mut self) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.minions.keys().copied().collect();
        // Formation slots by summoner, in ID order.
        let mut slots = std::collections::BTreeMap::<PlayerId, Vec<MinionId>>::new();
        for minion in self.minions.values() {
            slots.entry(minion.summoner).or_default().push(minion.id);
        }
        for id in ids {
            self.decide_minion(id, &slots)?;
        }
        Ok(())
    }

    fn decide_minion(
        &mut self,
        id: MinionId,
        slots: &std::collections::BTreeMap<PlayerId, Vec<MinionId>>,
    ) -> Result<(), ZoneError> {
        let Some(minion) = self.minions.get(&id).copied() else {
            return Ok(());
        };
        let entity = EntityRef::Minion(id);
        let body = body_id(entity);
        let current = self
            .world
            .body(body)
            .ok_or_else(|| ZoneError::new("minion physics body is missing"))?;
        let (position, vertical) = (current.position(), current.velocity().y);
        let (centre, half) = (current.exact_position(), current.half_extents());
        let here = [position.x, position.z];
        let Some(summoner) = self.players.get(&minion.summoner) else {
            return Ok(());
        };
        let facing = summoner.facing;
        let leader = self.player_position(minion.summoner)?;
        let leader_at = [leader.x, leader.z];
        // Lost far behind (a fall, a door, a long run): gathered back beside the summoner.
        if distance_squared(here, leader_at) > i64::from(MINION_GATHER_RADIUS_UNITS).pow(2) {
            let spot = point_along(
                leader_at,
                facing.wrapping_add(0x8000),
                FORMATION_RADIUS_UNITS,
            );
            self.world
                .set_position(
                    body,
                    Vec3i::new(spot[0], self.ground_y(spot[0], spot[1]) + half.y, spot[1]),
                )
                .map_err(physics_error)?;
            self.world
                .set_velocity(body, Vec3i::ZERO)
                .map_err(physics_error)?;
            if let Some(minion) = self.minions.get_mut(&id) {
                minion.target = None;
            }
            return Ok(());
        }
        let leashed = |zone: &Self, unit: EntityRef| -> Result<bool, ZoneError> {
            let at = zone.unit_position(unit)?;
            Ok(distance_squared(leader_at, [at[0], at[2]])
                > i64::from(MINION_LEASH_RADIUS_UNITS).pow(2))
        };
        // The fight: an ordered target, the current one while it lasts, or a new enemy.
        let mut order = minion.order;
        let mut target = match order {
            MinionOrder::Retreat => None,
            MinionOrder::Attack(ordered) => Some(ordered),
            MinionOrder::Follow | MinionOrder::Defend(_) => minion.target,
        };
        if let Some(unit) = target
            && (!self.minion_may_fight(unit)
                || self.combat_blocked(entity, unit)
                || leashed(self, unit)?)
        {
            target = None;
            if matches!(order, MinionOrder::Attack(_)) {
                order = MinionOrder::Follow;
            }
        }
        if target.is_none() {
            target = match order {
                MinionOrder::Follow => self.party_enemy(minion.summoner, leader_at, entity)?,
                MinionOrder::Defend(point) => self.defend_enemy(point, entity)?,
                MinionOrder::Attack(_) | MinionOrder::Retreat => None,
            };
        }
        let anchor = match order {
            MinionOrder::Defend(point) => point,
            MinionOrder::Follow | MinionOrder::Attack(_) | MinionOrder::Retreat => leader_at,
        };
        let group = slots.get(&minion.summoner).map_or(&[][..], Vec::as_slice);
        let slot = group.iter().position(|&other| other == id).unwrap_or(0);
        let goal = slot_point(
            anchor,
            facing,
            slot,
            group.len(),
            matches!(order, MinionOrder::Defend(_)),
        );
        let (velocity, facing) = match target {
            Some(unit) => {
                let at = self.unit_position(unit)?;
                let to = [at[0], at[2]];
                let reach = self.minion_reach(unit)?;
                let waypoint = self.route_waypoint(centre, half, to, reach)?;
                let (dx, dz) = offset(here, waypoint.unwrap_or(to));
                let inside = distance_squared(here, to) <= i64::from(reach).pow(2);
                let velocity = if inside && waypoint.is_none() {
                    [0, 0]
                } else {
                    steer(dx, dz, MINION_RUN_SPEED_SUBUNITS_PER_TICK)
                };
                let (fx, fz) = offset(here, to);
                (velocity, yaw_from_vector(fx, fz))
            }
            None => {
                let distance = distance_squared(here, goal);
                if distance <= i64::from(FORMATION_SLACK_UNITS).pow(2) {
                    if order == MinionOrder::Retreat {
                        order = MinionOrder::Follow;
                    }
                    ([0, 0], None)
                } else {
                    let rush = order == MinionOrder::Retreat
                        || distance > i64::from(RUSH_DISTANCE_UNITS).pow(2);
                    let speed = if rush {
                        MINION_RUSH_SPEED_SUBUNITS_PER_TICK
                    } else {
                        MINION_RUN_SPEED_SUBUNITS_PER_TICK
                    };
                    let waypoint = self.route_waypoint(centre, half, goal, 0)?;
                    let (dx, dz) = offset(here, waypoint.unwrap_or(goal));
                    (steer(dx, dz, speed), yaw_from_vector(dx, dz))
                }
            }
        };
        // Crowds spread out: a step away from every minion standing too close.
        let mut push = [0_i32, 0_i32];
        for other in self
            .interest
            .near(here[0], here[1], SEPARATION_RADIUS_UNITS)
        {
            if other == entity || !matches!(other, EntityRef::Minion(_)) {
                continue;
            }
            let Some(other_body) = self.world.body(body_id(other)) else {
                continue;
            };
            let there = other_body.position();
            let (dx, dz) = offset([there.x, there.z], here);
            if distance_squared([0, 0], [dx, dz]) > i64::from(SEPARATION_RADIUS_UNITS).pow(2) {
                continue;
            }
            // Two minions on one spot part along their IDs.
            let (dx, dz) = if (dx, dz) == (0, 0) {
                (if entity < other { -1 } else { 1 }, 0)
            } else {
                (dx, dz)
            };
            let away = steer(dx, dz, MINION_RUN_SPEED_SUBUNITS_PER_TICK / 3);
            push = [push[0] + away[0], push[1] + away[1]];
        }
        let velocity = [velocity[0] + push[0], velocity[1] + push[1]];
        let velocity = self.keep_out_of_deep_water(here, velocity);
        if let Some(minion) = self.minions.get_mut(&id) {
            minion.order = order;
            minion.target = target;
            if let Some(facing) = facing {
                minion.facing = facing;
            }
        }
        self.world
            .set_velocity(body, Vec3i::new(velocity[0], vertical, velocity[1]))
            .map_err(physics_error)
    }

    /// How close a minion must get to hit `unit`: its reach plus the creature's half-width.
    fn minion_reach(&self, unit: EntityRef) -> Result<i32, ZoneError> {
        let width = match unit {
            EntityRef::Creature(id) => {
                let (_, template) = Self::creature_content(&self.content, id)?;
                template.half_extents[0].max(template.half_extents[2])
            }
            _ => crate::PLAYER_HALF_EXTENTS_UNITS[0],
        };
        Ok(CREATURE_REACH_UNITS + width)
    }

    /// The nearest creature near the summoner fighting the summoner or one of its minions, ties
    /// to the lowest ID.
    fn party_enemy(
        &self,
        summoner: PlayerId,
        leader_at: [i32; 2],
        minion: EntityRef,
    ) -> Result<Option<EntityRef>, ZoneError> {
        let party: BTreeSet<EntityRef> = std::iter::once(EntityRef::Player(summoner))
            .chain(self.minions_of(summoner).into_iter().map(EntityRef::Minion))
            .collect();
        self.nearest_enemy(
            leader_at,
            ENGAGE_RADIUS_UNITS,
            minion,
            |creature_id, zone| {
                zone.creatures.get(&creature_id).is_some_and(|creature| {
                    creature.ai == CreatureAi::Engaged
                        && creature
                            .threat
                            .iter()
                            .any(|entry| party.contains(&entry.entity))
                })
            },
        )
    }

    /// The nearest engaged or aggressive creature near a defended point.
    fn defend_enemy(
        &self,
        point: [i32; 2],
        minion: EntityRef,
    ) -> Result<Option<EntityRef>, ZoneError> {
        self.nearest_enemy(point, DEFEND_RADIUS_UNITS, minion, |creature_id, zone| {
            let aggressive = Self::creature_content(&zone.content, creature_id)
                .is_ok_and(|(_, template)| template.behaviour == CreatureBehaviour::Aggressive);
            aggressive
                || zone
                    .creatures
                    .get(&creature_id)
                    .is_some_and(|creature| creature.ai == CreatureAi::Engaged)
        })
    }

    fn nearest_enemy(
        &self,
        centre: [i32; 2],
        radius: i32,
        minion: EntityRef,
        eligible: impl Fn(CreatureId, &Self) -> bool,
    ) -> Result<Option<EntityRef>, ZoneError> {
        let mut best: Option<(i64, EntityRef)> = None;
        for unit in self.interest.near(centre[0], centre[1], radius) {
            let EntityRef::Creature(creature_id) = unit else {
                continue;
            };
            if !self.minion_may_fight(unit)
                || self.combat_blocked(minion, unit)
                || !eligible(creature_id, self)
            {
                continue;
            }
            let at = self.unit_position(unit)?;
            let distance = distance_squared(centre, [at[0], at[2]]);
            if distance <= i64::from(radius).pow(2)
                && best.is_none_or(|best| (distance, unit) < best)
            {
                best = Some((distance, unit));
            }
        }
        Ok(best.map(|(_, unit)| unit))
    }

    /// Step 6, after the creatures: minion swings in ID order.
    pub(crate) fn minion_swings(&mut self, now: u64) -> Result<(), ZoneError> {
        let ids: Vec<_> = self.minions.keys().copied().collect();
        for id in ids {
            let Some(minion) = self.minions.get_mut(&id) else {
                continue;
            };
            minion.swing_timer = minion.swing_timer.saturating_sub(1);
            let minion = *minion;
            let (Some(target), 0) = (minion.target, minion.swing_timer) else {
                continue;
            };
            let Some(stats) = minion_stats(minion.ability, minion.level) else {
                continue;
            };
            let source = EntityRef::Minion(id);
            if !self.minion_may_fight(target) || self.combat_blocked(source, target) {
                continue;
            }
            let from = self.unit_position(source)?;
            let to = self.unit_position(target)?;
            let reach = i64::from(self.minion_reach(target)?);
            if distance_squared([from[0], from[2]], [to[0], to[2]]) > reach * reach {
                continue;
            }
            let summoner = minion.summoner;
            if let Some(minion) = self.minions.get_mut(&id) {
                minion.swing_timer = stats.swing_ticks;
            }
            self.notify_observers(
                &[source, target],
                &BTreeSet::new(),
                ZoneEvent::AttackPerformed { source, target },
            );
            let EntityRef::Creature(creature_id) = target else {
                continue;
            };
            match roll_swing(&mut self.rng, stats.damage) {
                Swing::Miss => {
                    if let Some(creature) = self.creatures.get_mut(&creature_id) {
                        creature.combat_timer = COMBAT_LINGER_TICKS;
                        creature.add_threat(source, 0);
                    }
                    self.notify(summoner, ZoneEvent::Miss { source, target });
                    self.engage(creature_id, source, true)?;
                }
                Swing::Hit { amount, critical } => {
                    self.unit_damages_creature(
                        summoner,
                        source,
                        creature_id,
                        u32::from(amount),
                        critical,
                        now,
                    )?;
                }
            }
        }
        Ok(())
    }

    /// Damage to a minion from a creature's swing or shot: it may crumble.
    pub(crate) fn damage_minion(
        &mut self,
        source: EntityRef,
        id: MinionId,
        amount: u32,
        critical: bool,
    ) -> Result<(), ZoneError> {
        let target = EntityRef::Minion(id);
        let Some(minion) = self.minions.get_mut(&id) else {
            return Ok(());
        };
        let dealt = amount.min(minion.health);
        minion.health -= dealt;
        let (summoner, dead) = (minion.summoner, minion.health == 0);
        self.notify(
            summoner,
            ZoneEvent::DamageTaken {
                source,
                target,
                amount: u16::try_from(dealt).unwrap_or(u16::MAX),
                critical,
            },
        );
        self.update_player(summoner, |player| {
            player.combat_timer = COMBAT_LINGER_TICKS;
        });
        if dead {
            return self.minion_crumbles(id, Some(source));
        }
        Ok(())
    }

    /// A minion falls apart (death, time, its summoner's death or a newer raise): its body
    /// goes, every unit forgets it, and its summoner hears of it.
    pub(crate) fn minion_crumbles(
        &mut self,
        id: MinionId,
        killer: Option<EntityRef>,
    ) -> Result<(), ZoneError> {
        let Some(minion) = self.dismiss_minion(id) else {
            return Ok(());
        };
        self.notify(
            minion.summoner,
            ZoneEvent::Died {
                entity: EntityRef::Minion(id),
                killer,
            },
        );
        Ok(())
    }

    /// Removes a minion without a word: its body, its interest entry, and every reference.
    pub(crate) fn dismiss_minion(&mut self, id: MinionId) -> Option<Minion> {
        let minion = self.minions.remove(&id)?;
        let entity = EntityRef::Minion(id);
        self.world.remove_body(body_id(entity));
        if self.interest.remove(entity) {
            self.interest_work.bucket_removes += 1;
        }
        self.forget_forced_unit(entity);
        for creature in self.creatures.values_mut() {
            creature.forget(entity);
            if creature
                .cast
                .is_some_and(|cast| cast.target == Some(entity))
            {
                creature.cast = None;
            }
        }
        for player in self.players.values_mut() {
            if player.target == Some(entity) {
                player.target = None;
                player.auto_attack = false;
            }
        }
        Some(minion)
    }

    /// Every minion of `summoner` crumbles (its death) or vanishes (it left). A companion
    /// (#296) outlives its owner's death and fights on.
    pub(crate) fn release_minions(
        &mut self,
        summoner: PlayerId,
        quietly: bool,
    ) -> Result<(), ZoneError> {
        for id in self.minions_of(summoner) {
            if quietly {
                self.dismiss_minion(id);
            } else if !self
                .minions
                .get(&id)
                .is_some_and(|minion| is_companion(minion.ability))
            {
                self.minion_crumbles(id, None)?;
            }
        }
        Ok(())
    }

    /// A levelling owner's companion (#296) grows with it, keeping its missing health.
    pub(crate) fn level_companion(&mut self, owner: PlayerId, level: u8) {
        for minion in self.minions.values_mut() {
            if minion.summoner != owner || !is_companion(minion.ability) {
                continue;
            }
            let before = minion.max_health();
            minion.level = level;
            minion.health += minion.max_health().saturating_sub(before);
        }
    }

    /// Step 7: minions whose time ran out crumble, in ID order; companions (#296) are not timed.
    pub(crate) fn age_minions(&mut self) -> Result<(), ZoneError> {
        let mut expired = Vec::new();
        for minion in self.minions.values_mut() {
            if is_companion(minion.ability) {
                continue;
            }
            minion.remaining = minion.remaining.saturating_sub(1);
            if minion.remaining == 0 {
                expired.push(minion.id);
            }
        }
        for id in expired {
            self.minion_crumbles(id, None)?;
        }
        Ok(())
    }

    /// Restores recorded minions after the players (recovery).
    pub(crate) fn restore_minions(
        &mut self,
        records: Vec<CanonicalMinion>,
        next_minion: u32,
    ) -> Result<(), ZoneError> {
        let invalid = || ZoneError::new("snapshot minions are invalid");
        if records.len() > MAX_MINIONS
            || records
                .windows(2)
                .any(|pair| pair[0].minion.id >= pair[1].minion.id)
        {
            return Err(invalid());
        }
        for record in records {
            let minion = record.minion;
            let stats = minion_stats(minion.ability, minion.level).ok_or_else(invalid)?;
            // A companion (#296) outlives its owner's death; timed minions do not.
            let summoner_ok = self
                .players
                .get(&minion.summoner)
                .is_some_and(|player| stats.duration.is_none() || player.is_alive());
            let remaining_ok = match stats.duration {
                Some(duration) => (1..=duration).contains(&minion.remaining),
                None => minion.remaining == 0,
            };
            let target_ok = |target: Option<EntityRef>| {
                target.is_none_or(|unit| {
                    matches!(unit, EntityRef::Creature(id) if self.creatures.contains_key(&id))
                })
            };
            let order_ok = match minion.order {
                MinionOrder::Attack(unit) => target_ok(Some(unit)),
                MinionOrder::Follow | MinionOrder::Defend(_) | MinionOrder::Retreat => true,
            };
            if minion.id.get() >= next_minion
                || !summoner_ok
                || minion.health == 0
                || minion.health > stats.max_health
                || !remaining_ok
                || minion.swing_timer > stats.swing_ticks
                || !target_ok(minion.target)
                || !order_ok
                || self.minions_of(minion.summoner).len() >= MAX_MINIONS_PER_SUMMONER
                || self.minions_by(minion.summoner, minion.ability).len() >= stats.limit
                || record
                    .position
                    .iter()
                    .chain(&record.velocity)
                    .any(|value| !value.is_finite())
            {
                return Err(invalid());
            }
            self.world
                .add_body(RigidBody::moving(
                    body_id(EntityRef::Minion(minion.id)),
                    record.position,
                    record.velocity,
                    vector(minion_half_extents(minion.ability)),
                ))
                .map_err(physics_error)?;
            self.minions.insert(minion.id, minion);
        }
        self.next_minion = next_minion;
        Ok(())
    }

    /// Canonical minion records in ID order.
    pub(crate) fn canonical_minions(&self) -> Result<Vec<CanonicalMinion>, ZoneError> {
        self.minions
            .values()
            .map(|minion| {
                let body = self
                    .world
                    .body(body_id(EntityRef::Minion(minion.id)))
                    .ok_or_else(|| ZoneError::new("minion physics body is missing"))?;
                Ok(CanonicalMinion {
                    minion: *minion,
                    position: body.exact_position(),
                    velocity: body.exact_velocity(),
                })
            })
            .collect()
    }
}

/// A minion and its exact `f32` body state (#271).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanonicalMinion {
    pub minion: Minion,
    pub position: [f32; 3],
    pub velocity: [f32; 3],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formation_arcs_behind_the_summoner_and_rings_a_point() {
        // Facing +Z (yaw 0), the lone minion stands straight behind.
        assert_eq!(
            slot_point([0, 0], 0, 0, 1, false),
            [0, -FORMATION_RADIUS_UNITS]
        );
        // Two minions stand symmetrically behind.
        let [left, right] = [0, 1].map(|slot| slot_point([0, 0], 0, slot, 2, false));
        assert_eq!(left[0], -right[0]);
        assert_eq!(left[1], right[1]);
        assert!(left[1] < 0);
        // Four minions ring a defended point.
        let ring: Vec<_> = (0..4)
            .map(|slot| slot_point([0, 0], 0, slot, 4, true))
            .collect();
        assert_eq!(ring[0], [0, FORMATION_RADIUS_UNITS]);
        assert_eq!(ring[2], [0, -FORMATION_RADIUS_UNITS]);
    }

    #[test]
    fn raise_skeleton_scales_with_the_summoner() {
        let first = minion_stats(crate::ability::ids::RAISE_SKELETON, 3).unwrap();
        let later = minion_stats(crate::ability::ids::RAISE_SKELETON, 10).unwrap();
        assert!(later.max_health > first.max_health);
        assert!(later.damage[1] > first.damage[1]);
        assert!(minion_stats(crate::ability::ids::SHADOW_BOLT, 3).is_none());
    }
}
