//! Deterministic creature AI (tick step 2), in creature-ID order.
//!
//! - **Idle**: aggressive creatures engage the nearest living player within
//!   their aggro radius, found through the zone's spatial index, and a territorial
//!   neutral (#997) one within its territory. Otherwise
//!   they wait 5–15 s (zone RNG), then walk to a random point within their
//!   wander radius at walking speed. Large creatures (#297) hold their
//!   ground instead.
//! - **Engaged**: the target is the threat leader. The creature steers
//!   straight at it at run speed until it is inside melee reach and faces it;
//!   combat swings on the swing timer. An empty threat table or straying
//!   beyond the leash radius from the spawn point starts evading.
//! - **Evading**: run home, ignoring damage, with a cleared threat table; on
//!   arrival reset to full health and idle. A creature still away after
//!   10 s is moved home through the physics API.
//!
//! - **Trailing** (#274): an idle aquatic creature follows a submerged player
//!   within 8 m for 6 s without combat, then loses interest and evades home.
//!   Swimming players are never aggro candidates, and an engaged creature
//!   drops every target it cannot fight across the water's surface.
//!
//! - **Raiders** (#530): a night raider leashes and evades to its anchor (where its last fight
//!   began, or its camp spot), not its staging spawn. Idle on a march, it walks its route
//!   instead of wandering (`raid`). A withdrawing raider picks no fights, and one back on its
//!   march after an evade keeps its wounds.
//!
//! - **Patrols** (#911): an authored patrol member in the open takes its role's step instead
//!   of the straight chase: a skirmisher flanks, a marksman keeps its distance (`patrol`).
//!
//! When a creature engages from idle, idle creatures of its family within
//! the assist radius engage the same target (one level, no chains). Steering
//! sets body velocity only; physics resolves obstacles and contact.

use crate::reach::xz_distance_squared;
use std::collections::BTreeSet;
use std::sync::Arc;

use crate::physics::Vec3i;

use crate::creature::{Life, aggro_radius};
use crate::creature_store::Activity;
use crate::entity::body_id;
use crate::rng::ZoneRng;
use crate::snapshot::CreatureAi;
use crate::trig::{self, xz_length, yaw_from_vector};
use crate::zone::physics_error;
use crate::{
    ARRIVAL_RADIUS_UNITS, ASSIST_RADIUS_UNITS, CREATURE_EVADE_SPEED_SUBUNITS_PER_TICK,
    CREATURE_RUN_SPEED_SUBUNITS_PER_TICK, CREATURE_WALK_SPEED_SUBUNITS_PER_TICK, CreatureBehaviour,
    CreatureId, EVADE_TIMEOUT_TICKS, EntityRef, LEASH_RADIUS_UNITS, PLAYER_HALF_EXTENTS_UNITS,
    PlayerId, WANDER_WAIT_TICKS, WANDER_WALK_TIMEOUT_TICKS, ZoneError, ZoneSimulation,
    unit::CREATURE_REACH_UNITS,
};

/// The XZ offset from `from` to `to` that steering follows. An offset beyond
/// the `i32` range, which only a recovered extreme player position produces,
/// is halved on both axes, which keeps its direction.
pub(crate) fn offset(from: [i32; 2], to: [i32; 2]) -> (i32, i32) {
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
    xz_distance_squared(home, to) <= (i64::from(radius) + 1).pow(2)
}

/// Velocity of at most `speed` along `(dx, dz)` without overshooting it.
/// Integer normalisation: each component is `delta · step / length` in subunits per tick,
/// rounded toward zero; `speed` is in subunits per tick and `(dx, dz)` in units, and a step
/// never overshoots the target.
pub(crate) fn steer(dx: i32, dz: i32, speed: i32) -> [i32; 2] {
    let length = xz_length(dx, dz);
    if length == 0 {
        return [0, 0];
    }
    let length = i64::try_from(length).unwrap_or(i64::MAX);
    let step = i64::from(speed).min(length.saturating_mul(SUBUNITS));
    let scale = |delta: i32| i32::try_from(i64::from(delta) * step / length).unwrap_or(0);
    [scale(dx), scale(dz)]
}

/// A chasing creature's last step aims this far inside its reach, so the
/// truncating integer steering can neither stall it just outside the reach
/// nor leave it there: a step of at least this length always moves it.
const CHASE_CLOSE_IN_UNITS: i64 = 10;
/// A creature that held a boss's rally walks home within this many ticks (#276).
const RALLY_RETURN_TICKS: u16 = crate::ticks_from_ms(20_000);

/// How a creature takes this tick (#814): its body when the zone sets its rest, whether that
/// body sits the physics step out, whether the creature decides and the ticks its decision
/// covers.
struct Pace {
    body: Option<crate::physics::BodyId>,
    asleep: bool,
    decides: bool,
    elapsed: u16,
}

/// Subunits per unit, for steering in subunits per tick over unit offsets.
const SUBUNITS: i64 = crate::physics::SUBUNITS_PER_UNIT as i64;

/// Chase velocity toward a target `(dx, dz)` away: `speed` while beyond
/// `reach`, then a last step that ends inside it, and zero once inside.
fn chase_velocity(dx: i32, dz: i32, reach: i32, speed: i32) -> [i32; 2] {
    if crate::reach::within_xz([0, 0], [dx, dz], reach) {
        return [0, 0];
    }
    let gap = i64::try_from(xz_length(dx, dz))
        .unwrap_or(i64::MAX)
        .saturating_sub(i64::from(reach));
    let step = i64::from(speed).min(
        gap.saturating_add(CHASE_CLOSE_IN_UNITS)
            .saturating_mul(SUBUNITS),
    );
    steer(dx, dz, i32::try_from(step).unwrap_or(0))
}

/// A set-piece attacker's step through its [`crate::Lunge`] (#989, #990), or `None` between
/// attacks: it holds still through the wind-up (a lynx's crouch, an adder's hiss), turning toward
/// its target `(dx, dz)` away, leaps along its facing for the wind-up's last
/// [`crate::Lunge::leap_ticks`], and holds still through the recovery after its contact. The
/// leap closes only the gap to `reach`, like a chase's last step, never carrying it into its
/// target. Its swing timer, set as the attack began, dates the recovery.
fn lunge_step(
    creature: &crate::creature::CreatureState,
    template: &crate::CreatureTemplate,
    lunge: crate::Lunge,
    (dx, dz): (i32, i32),
    reach: i32,
) -> Option<([i32; 2], Option<u16>)> {
    let turn = yaw_from_vector(dx, dz);
    match creature.attack {
        Some(pending) if pending.leaping(template.family) => {
            let (x, z) = trig::direction(creature.facing);
            let gap = i64::try_from(xz_length(dx, dz))
                .unwrap_or(i64::MAX)
                .saturating_sub(i64::from(reach))
                .saturating_add(CHASE_CLOSE_IN_UNITS)
                .max(0);
            let speed = i64::from(lunge.leap_speed).min(gap.saturating_mul(SUBUNITS));
            let speed = i32::try_from(speed).unwrap_or(0);
            let leap = |unit| trig::checked_scale(speed, unit).unwrap_or(0);
            Some(([leap(x), leap(z)], None))
        }
        Some(_) => Some(([0, 0], turn)),
        None => recovering(creature, template, lunge).then_some(([0, 0], turn)),
    }
}

/// Whether a set-piece attacker is still in its [`crate::Lunge::recover_ticks`] after its last
/// contact (#989, #995): its swing timer, set as the attack began, dates it.
fn recovering(
    creature: &crate::creature::CreatureState,
    template: &crate::CreatureTemplate,
    lunge: crate::Lunge,
) -> bool {
    let since = template.swing_ticks.saturating_sub(creature.swing_timer);
    let contact = crate::contact_ticks(template.family, crate::CreatureAttack::Quick).unwrap_or(0);
    since < contact + lunge.recover_ticks
}

/// How close a creature must get to hit `target`: its reach plus the
/// target's half-width.
pub(crate) const fn creature_reach(target: EntityRef) -> i32 {
    // Players and minions (#295) are attacked.
    match target {
        EntityRef::Minion(_) => CREATURE_REACH_UNITS + crate::MINION_HALF_EXTENTS_UNITS[0],
        EntityRef::Player(_) | EntityRef::Creature(_) | EntityRef::Npc(_) => {
            CREATURE_REACH_UNITS + PLAYER_HALF_EXTENTS_UNITS[0]
        }
    }
}

impl ZoneSimulation {
    /// Step 2: every living creature decides and sets its body velocity. An idle creature
    /// with no player near sleeps instead, and an awake idle one decides on its staggered
    /// tick only, walking on in between (#814).
    ///
    /// Only creatures whose [`Activity::Decide`] bit is set are visited (#1086): a creature
    /// resting asleep keeps it clear until it changes, something changes its body or a player
    /// comes near ([`Self::wake_resting_creatures`]); visiting it would change nothing.
    pub(crate) fn decide_creatures(&mut self) -> Result<(), ZoneError> {
        self.continue_dens_offscreen()?;
        self.wake_resting_creatures();
        let mut after = None;
        while let Some(id) = self.creatures.next_active(Activity::Decide, after) {
            after = Some(id);
            self.tick_work.creatures_visited += 1;
            let pace = self.creature_pace(id)?;
            if let Some(body) = pace.body {
                self.world
                    .set_asleep(body, pace.asleep)
                    .map_err(physics_error)?;
                if pace.asleep {
                    self.world
                        .set_velocity(body, Vec3i::ZERO)
                        .map_err(physics_error)?;
                }
            }
            if pace.decides {
                self.decide_creature(id, pace.elapsed)?;
            }
            // Asleep, dead or a prop, it stays as it is until something changes it.
            let rest = match pace.body {
                Some(body) if pace.asleep => self
                    .world
                    .body(body)
                    .map(|body| [body.position().x, body.position().z]),
                _ => None,
            };
            let (_, template) = Self::creature_content(&self.content, id)?;
            let quiet = rest.is_some()
                || template.prop().is_some()
                || self
                    .creatures
                    .get(&id)
                    .is_none_or(|creature| !creature.is_alive());
            self.creatures.rest(id, rest);
            if quiet {
                self.creatures.settle(Activity::Decide, id);
            }
        }
        Ok(())
    }

    /// Sets the [`Activity::Decide`] bit of every resting creature a player came near, by the
    /// pace's own test, and of every one whose asleep body something changed (#1086).
    pub(crate) fn wake_resting_creatures(&mut self) {
        let mut scratch = std::mem::take(&mut self.creature_scratch);
        self.world.take_disturbed_sleepers(&mut scratch.bodies);
        for body in scratch.bodies.drain(..) {
            if let Some(EntityRef::Creature(id)) = crate::entity::entity_of(body) {
                self.creatures.wake(Activity::Decide, id);
            }
        }
        scratch.cells.clear();
        scratch.cells.extend(
            self.players
                .keys()
                .filter_map(|&player| self.interest.cell_of(EntityRef::Player(player))),
        );
        scratch.cells.sort_unstable();
        scratch.cells.dedup();
        // A creature at x sees a player's cell cx while x - R < (cx + 1)·w and x + R >= cx·w.
        let (width, radius) = (
            crate::interest::InterestIndex::WIDTH,
            i64::from(crate::CREATURE_WAKE_RADIUS_UNITS),
        );
        for &(cell_x, cell_z) in &scratch.cells {
            let span = |cell: i64| [cell * width - radius, (cell + 1) * width + radius - 1];
            self.creatures
                .resting_in(span(cell_x), span(cell_z), &mut scratch.near);
            for &entity in &scratch.near {
                if let EntityRef::Creature(id) = entity
                    && let Some([x, z]) = self.creatures.rest_at(id)
                    && self
                        .interest
                        .player_near(x, z, crate::CREATURE_WAKE_RADIUS_UNITS)
                {
                    self.creatures.wake(Activity::Decide, id);
                }
            }
        }
        self.creature_scratch = scratch;
    }

    /// Whether creature `id` sleeps, holds its course or decides this tick (#814). Only a
    /// living idle land creature on the ground, outside any raid, ever rests: a swimmer's
    /// swimming and a flier's flight (#995) set its height every tick, so it decides every
    /// tick. The pace is derived from canonical state each tick, so replay and recovery need
    /// nothing stored.
    fn creature_pace(&self, id: CreatureId) -> Result<Pace, ZoneError> {
        let body = body_id(EntityRef::Creature(id));
        let current = self.world.body(body).filter(|body| body.is_moving());
        // Every other creature with a body is awake.
        let decides = Pace {
            body: current.map(|_| body),
            asleep: false,
            decides: true,
            elapsed: 1,
        };
        let (_, template) = Self::creature_content(&self.content, id)?;
        let (Some(creature), Some(current)) = (self.creatures.get(&id), current) else {
            return Ok(decides);
        };
        if creature.life != Life::Alive
            || template.prop().is_some()
            || template.family.swims()
            || template.family.flies()
            || creature.raid.is_some()
            || self.prowling(id)
            || self.supply_marching(id)
            || self.crossing_raiding(id)
            || !matches!(creature.ai, CreatureAi::Idle { .. })
            || current.velocity().y != 0
        {
            return Ok(decides);
        }
        let position = current.position();
        let near =
            self.interest
                .player_near(position.x, position.z, crate::CREATURE_WAKE_RADIUS_UNITS);
        if !near {
            return Ok(Pace {
                asleep: true,
                decides: false,
                ..decides
            });
        }
        let interval = u64::from(crate::IDLE_DECISION_INTERVAL_TICKS);
        Ok(Pace {
            decides: (self.tick + u64::from(id.get())).is_multiple_of(interval),
            elapsed: crate::IDLE_DECISION_INTERVAL_TICKS,
            ..decides
        })
    }

    /// `elapsed`: ticks since its last decision, which idle timers count down by (#814).
    fn decide_creature(&mut self, id: CreatureId, elapsed: u16) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (spawn, template) = Self::creature_content(&content, id)?;
        let Some(creature) = self.creatures.get(&id) else {
            return Ok(());
        };
        // Props (#288) decide nothing.
        if creature.life != Life::Alive || template.prop().is_some() {
            return Ok(());
        }
        self.tick_work.creature_ai_evaluations += 1;
        let body = body_id(EntityRef::Creature(id));
        let current = self
            .world
            .body(body)
            .ok_or_else(|| ZoneError::new("creature physics body is missing"))?;
        let (position, vertical) = (current.position(), current.velocity().y);
        let (centre, half) = (current.exact_position(), current.half_extents());
        let here = [position.x, position.z];
        let (idle, level) = (
            matches!(creature.ai, CreatureAi::Idle { .. }),
            creature.level,
        );

        // A withdrawing raider (#530) leaves rather than picks fights; it still defends itself.
        // A prowler of a watch (#1019) goes for the flock and picks no fight either, nor does a
        // crossing's raider (#1019), which goes for the cart.
        let withdrawing = creature
            .raid
            .is_some_and(|member| member.leg == crate::RaidLeg::Retreat);
        let withdrawing = withdrawing || self.prowling(id) || self.crossing_raiding(id);
        let home = self.creature_home(id, spawn);
        // A territorial neutral (#997) engages a player who comes within its territory.
        let territorial =
            template.behaviour == CreatureBehaviour::Neutral && template.family.territorial();
        if idle
            && !withdrawing
            && (template.behaviour == CreatureBehaviour::Aggressive || territorial)
            && let Some(player) = self.aggro_candidate(here, level, template.family, territorial)?
            && !self.in_handoff_corridor(player)
        {
            self.engage(id, EntityRef::Player(player), true)?;
        }
        // Aquatic creatures (#274) trail a submerged player nearby instead, without combat.
        let aquatic = template.family.swims();
        if aquatic
            && matches!(
                self.creatures.get(&id).map(|creature| creature.ai),
                Some(CreatureAi::Idle { .. })
            )
            && let Some(player) = self.trail_candidate([position.x, position.y, position.z])?
            && let Some(creature) = self.creatures.get_mut(&id)
        {
            creature.ai = CreatureAi::Trailing {
                player,
                ticks: crate::water::TRAIL_TICKS,
            };
        }
        // No combat into, out of or between submerged units (#274): an engaged creature drops
        // every target it cannot fight, and evades once none is left.
        if let Some(creature) = self.creatures.get(&id)
            && creature.ai == CreatureAi::Engaged
        {
            let blocked: Vec<EntityRef> = creature
                .threat
                .iter()
                .map(|entry| entry.entity)
                .filter(|&entity| self.combat_blocked(EntityRef::Creature(id), entity))
                .collect();
            if !blocked.is_empty()
                && let Some(creature) = self.creatures.get_mut(&id)
            {
                creature
                    .threat
                    .retain(|entry| !blocked.contains(&entry.entity));
            }
        }
        // The trailed player's body while it is still worth following.
        let trail = match self.creatures.get(&id).map(|creature| creature.ai) {
            Some(CreatureAi::Trailing { player, ticks }) => {
                let follows = ticks > 0
                    && self
                        .players
                        .get(&player)
                        .is_some_and(|state| state.is_alive())
                    && self.submerged(EntityRef::Player(player));
                if follows {
                    Some(self.player_position(player)?)
                } else {
                    None
                }
            }
            _ => None,
        };

        // Read the chase target before mutating; dead or departed players
        // already left every threat table.
        let chase = match self.creatures.get(&id) {
            Some(creature) if creature.ai == CreatureAi::Engaged => match creature.top_threat() {
                Some(target @ EntityRef::Player(player)) => {
                    let position = self.player_position(player)?;
                    Some((target, [position.x, position.z]))
                }
                // A minion that hit it or was put on its table (#295).
                Some(target @ EntityRef::Minion(minion)) if self.minions.contains_key(&minion) => {
                    let position = self.unit_position(target)?;
                    Some((target, [position[0], position[2]]))
                }
                _ => None,
            },
            _ => None,
        };

        // Creatures also turn back before a cross-zone handoff corridor and
        // never fight a target standing in one (#145).
        // A flier (#995) sets its own height: its roost's, or in a fight its target's and its
        // dive's; it keeps a shorter leash.
        let flies = template.family.flies();
        let roost = flies.then(|| self.rest_y(home, template));
        let target_y = match chase {
            Some((target, _)) if flies => Some(self.unit_position(target)?[1]),
            _ => None,
        };
        let leash = if flies {
            crate::flight::LEASH_UNITS
        } else {
            LEASH_RADIUS_UNITS
        };
        let leashed = !crate::reach::within_xz(home, here, leash)
            || chase.is_some_and(|(_, to)| self.handoff_corridor_at(to[0], to[1]).is_some())
            || self.near_handoff_corridor(here, [half.x, half.z]);
        // Walls, pieces and rubble in the way (#307): a route around them.
        let ai = self.creatures.get(&id).map(|creature| creature.ai);
        let route = match (ai, chase) {
            (Some(CreatureAi::Engaged), Some((target, to))) if !leashed => {
                self.route_waypoint(centre, half, to, creature_reach(target))?
            }
            (Some(CreatureAi::Evading { .. }), _) => self.route_waypoint(centre, half, home, 0)?,
            (Some(CreatureAi::Rallying { point, .. }), _) => {
                self.route_waypoint(centre, half, point, 0)?
            }
            _ => None,
        };
        // A raider on its march (#530) walks its route instead of wandering, as does a supply
        // patrol's member (#1019); a pack wolf or its prey (#912) roams, hunts or flees.
        let march = match self.march(id, centre, half)? {
            None => match self.prowl(id, centre, half)? {
                None => match self.supply_step(id, centre, half)? {
                    None => match self.crossing_step(id, centre, half)? {
                        None => self.den_step(id, here, spawn.wander_radius, elapsed),
                        raid => raid,
                    },
                    supply => supply,
                },
                prowl => prowl,
            },
            march => march,
        };
        // An engaged creature that keeps fighting may cast its ability.
        // Abilities are aimed at players only; minions (#295) take swings.
        let casting = match chase {
            Some((target @ EntityRef::Player(_), to)) if !leashed => {
                self.try_creature_ability(id, target, here, to)?
            }
            _ => false,
        };
        // A patrol member (#911) takes its role's step in the open.
        let patrol = match chase {
            Some(chase) if !leashed && !casting && route.is_none() => {
                self.patrol_step(id, template.swing_ticks, here, chase)?
            }
            _ => None,
        };
        // A lure lying near it (#1019) draws its facing.
        let lured = self.lure_bearing(id, spawn.template, here);
        let Some(creature) = self.creatures.get_mut(&id) else {
            return Ok(());
        };
        if creature.ai == CreatureAi::Engaged && (chase.is_none() || leashed) {
            creature.ai = CreatureAi::Evading { ticks: 0 };
            creature.reset();
            // A boss's attempt ends here (#276, 5.1.77): its profile and contributors go, and
            // it rests at full unscaled health only once home.
            if creature.encounter.take().is_some() {
                creature.health = creature.health.min(template.max_health(creature.level));
            }
        }
        let speed = crate::aura::speed_percent(&creature.auras);
        let stunned = crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun);
        let lunge = crate::lunge(template.family);
        let pounce = match (chase, lunge) {
            (Some((target, to)), Some(lunge)) if !leashed && !casting => {
                let reach = creature_reach(target);
                lunge_step(creature, template, lunge, offset(here, to), reach)
            }
            _ => None,
        };
        // A flier's phase (#995): stooping through its dive's last ticks, low through its
        // recovery, else cruising over its target; out of a fight, at its roost.
        let phase = match (creature.ai, target_y, lunge) {
            (CreatureAi::Engaged, Some(_), Some(lunge)) if !leashed => match creature.attack {
                Some(pending) if pending.leaping(template.family) => crate::flight::Phase::Stoop,
                Some(_) => crate::flight::Phase::Cruise,
                None if recovering(creature, template, lunge) => crate::flight::Phase::Recover,
                None => crate::flight::Phase::Cruise,
            },
            _ => crate::flight::Phase::Roost,
        };

        let mut teleport_home = false;
        let mut lost_interest = None;
        let (velocity, facing) = match creature.ai {
            // Trailing (#274): follow the submerged player, keeping clear of it, until the
            // time runs out or it surfaces, dies or leaves; then swim home.
            CreatureAi::Trailing { player, ticks } => match trail {
                Some(at) => {
                    creature.ai = CreatureAi::Trailing {
                        player,
                        ticks: ticks - 1,
                    };
                    let to = [at.x, at.z];
                    let (dx, dz) = offset(here, to);
                    let near = crate::reach::within_xz(here, to, crate::water::TRAIL_KEEP_UNITS);
                    (
                        if near {
                            [0, 0]
                        } else {
                            steer(dx, dz, crate::water::TRAIL_SPEED_SUBUNITS_PER_TICK)
                        },
                        yaw_from_vector(dx, dz),
                    )
                }
                None => {
                    creature.ai = CreatureAi::Evading { ticks: 0 };
                    lost_interest = Some(player);
                    ([0, 0], None)
                }
            },
            // Large creatures (#297) hold their ground while idle instead of wandering, so they
            // draw nothing from their stream until a fight.
            CreatureAi::Idle { .. } if march.is_some() => march.unwrap_or(([0, 0], None)),
            CreatureAi::Idle { .. } if template.is_large() => ([0, 0], None),
            // A flier (#995) hangs still at its roost.
            CreatureAi::Idle { .. } if flies => ([0, 0], None),
            CreatureAi::Idle { .. } => wander(
                &mut creature.rng,
                &mut creature.ai,
                here,
                home,
                spawn.wander_radius,
                elapsed,
            ),
            CreatureAi::Engaged => match chase {
                // A casting creature stands and faces its target; a smash (#160) keeps the
                // facing it wound up with, so stepping aside dodges it.
                Some((_, to)) if casting => {
                    let locked = creature.cast.is_some_and(|cast| {
                        cast.elapsed > 0
                            && crate::ability_by_id(cast.ability).is_some_and(|ability| {
                                matches!(ability.effect, crate::AbilityEffect::Smash { .. })
                            })
                    });
                    let (dx, dz) = offset(here, to);
                    (
                        [0, 0],
                        if locked {
                            None
                        } else {
                            yaw_from_vector(dx, dz)
                        },
                    )
                }
                Some(_) if patrol.is_some() => patrol.unwrap_or(([0, 0], None)),
                Some((_, _)) if route.is_some() => {
                    let (dx, dz) = offset(here, route.unwrap_or(here));
                    (
                        steer(dx, dz, CREATURE_RUN_SPEED_SUBUNITS_PER_TICK),
                        yaw_from_vector(dx, dz),
                    )
                }
                Some(_) if pounce.is_some() => pounce.unwrap_or(([0, 0], None)),
                // A set-piece attacker (#989, #990) closes in at its approach (a lynx's stalk, an
                // adder's slither) to its leap's range; the rest run to reach.
                Some((target, to)) => {
                    let (dx, dz) = offset(here, to);
                    let (reach, speed) = match lunge {
                        Some(lunge) => (
                            creature_reach(target) + lunge.leap_units,
                            lunge.approach_speed,
                        ),
                        None => (creature_reach(target), CREATURE_RUN_SPEED_SUBUNITS_PER_TICK),
                    };
                    (
                        chase_velocity(dx, dz, reach, speed),
                        yaw_from_vector(dx, dz),
                    )
                }
                None => ([0, 0], None),
            },
            // Rallying after its boss fell (#276): run to the rally point, hold it, then walk home.
            CreatureAi::Rallying { ticks: 0, .. } => {
                creature.ai = CreatureAi::Idle {
                    timer: RALLY_RETURN_TICKS,
                    destination: Some(home),
                };
                ([0, 0], None)
            }
            CreatureAi::Rallying { ticks, point } => {
                creature.ai = CreatureAi::Rallying {
                    ticks: ticks - 1,
                    point,
                };
                if crate::reach::within_xz(here, point, ARRIVAL_RADIUS_UNITS) {
                    ([0, 0], None)
                } else {
                    let (dx, dz) = route.map_or_else(|| offset(here, point), |to| offset(here, to));
                    (
                        steer(dx, dz, CREATURE_RUN_SPEED_SUBUNITS_PER_TICK),
                        yaw_from_vector(dx, dz),
                    )
                }
            }
            CreatureAi::Evading { ticks } => {
                let (dx, dz) = offset(here, home);
                let arrived = crate::reach::within_xz(here, home, ARRIVAL_RADIUS_UNITS);
                if arrived || ticks >= EVADE_TIMEOUT_TICKS {
                    teleport_home = !arrived;
                    // A raider back on its march (#530) keeps its wounds and marches on at once.
                    let marching = creature
                        .raid
                        .is_some_and(|member| member.leg != crate::RaidLeg::Camp);
                    if !marching {
                        creature.health = template.max_health(creature.level);
                    }
                    creature.reset();
                    creature.ai = CreatureAi::Idle {
                        timer: if marching {
                            0
                        } else {
                            wander_wait(&mut creature.rng)
                        },
                        destination: None,
                    };
                    ([0, 0], None)
                } else {
                    creature.ai = CreatureAi::Evading { ticks: ticks + 1 };
                    let (dx, dz) = route.map_or((dx, dz), |waypoint| offset(here, waypoint));
                    (
                        steer(dx, dz, CREATURE_EVADE_SPEED_SUBUNITS_PER_TICK),
                        yaw_from_vector(dx, dz),
                    )
                }
            }
        };
        // A lured creature (#1019) holds its ground and turns toward the bait at the bounded rate.
        let (velocity, facing) = match lured {
            Some(bait) => ([0, 0], Some(bait)),
            None => (velocity, facing),
        };
        // Stuns freeze a creature in place; roots stop it and snares slow it; so does tar (#294).
        let surface = crate::surface::under_feet(
            self.content.surfaces(),
            [position.x, position.y - half.y, position.z],
        );
        let speed = speed * crate::surface::speed_percent(surface) / 100;
        let velocity = crate::aura::scale_velocity(velocity, speed);
        if let Some(facing) = facing
            && !stunned
        {
            // Standing its ground in a fight, it turns at a bounded rate (#949), so a player who
            // circles inside its reach can reach its flank; faster while it guards a flank just
            // struck.
            let pivots = (creature.ai == CreatureAi::Engaged && !casting && velocity == [0, 0])
                || lured.is_some();
            let turn = if creature.wary_ticks > 0 && lured.is_none() {
                crate::WARY_TURN_PER_TICK
            } else {
                crate::CREATURE_TURN_PER_TICK
            };
            creature.facing = if pivots {
                crate::flank::turn_toward(creature.facing, facing, turn)
            } else {
                facing
            };
        }
        creature.wary_ticks = creature.wary_ticks.saturating_sub(1);
        if let Some(player) = lost_interest {
            self.notify(
                player,
                crate::ZoneEvent::LostInterest {
                    source: EntityRef::Creature(id),
                },
            );
        }
        // Land creatures do not swim (#274): they stop where the water gets too deep to wade.
        // Aquatic ones swim with no gravity, at the surface or down to a trailed player. A flier
        // (#995) flies with no gravity toward its phase's height; stunned, it hangs where it is.
        let (velocity, vertical) = if let Some(roost) = roost {
            self.world
                .set_weightless(body, true)
                .map_err(physics_error)?;
            let (goal, rate) = phase.goal(target_y.unwrap_or(roost), roost);
            let rise = if stunned {
                0
            } else {
                crate::flight::vertical_velocity(position.y, goal, rate)
            };
            (velocity, rise)
        } else if aquatic {
            let feet = [position.x, position.y - half.y, position.z];
            let goal = trail.map(|at| at.y - PLAYER_HALF_EXTENTS_UNITS[1]);
            let swim = self.creature_swim(feet, 2 * half.y, goal);
            self.world
                .set_weightless(body, swim.is_some())
                .map_err(physics_error)?;
            (velocity, swim.unwrap_or(vertical))
        } else {
            (self.keep_out_of_deep_water(here, velocity), vertical)
        };
        if teleport_home {
            self.world
                .set_position(
                    body,
                    Vec3i::new(home[0], self.rest_y(home, template), home[1]),
                )
                .map_err(physics_error)?;
        }
        self.world
            .set_velocity(body, Vec3i::new(velocity[0], vertical, velocity[1]))
            .map_err(physics_error)
    }

    /// The nearest living player inside its aggro radius, an ambusher's (#889) the fixed
    /// [`crate::AMBUSH_RADIUS_UNITS`] and a territorial neutral's (#997)
    /// [`crate::TERRITORY_RADIUS_UNITS`], ties to the lowest ID.
    fn aggro_candidate(
        &mut self,
        here: [i32; 2],
        level: u8,
        family: crate::CreatureFamily,
        territorial: bool,
    ) -> Result<Option<PlayerId>, ZoneError> {
        // A warm buffer: the query allocates nothing (#1086).
        let mut near = std::mem::take(&mut self.creature_scratch.near);
        self.interest
            .near_into(here[0], here[1], crate::AGGRO_MAX_RADIUS_UNITS, &mut near);
        let best = self.nearest_aggro_candidate(&near, here, level, family, territorial);
        self.creature_scratch.near = near;
        best
    }

    fn nearest_aggro_candidate(
        &self,
        near: &[EntityRef],
        here: [i32; 2],
        level: u8,
        family: crate::CreatureFamily,
        territorial: bool,
    ) -> Result<Option<PlayerId>, ZoneError> {
        let mut best: Option<(i64, PlayerId)> = None;
        for &entity in near {
            let EntityRef::Player(player_id) = entity else {
                continue;
            };
            let Some(player) = self.players.get(&player_id) else {
                continue;
            };
            // Swimmers start no fights (#274): deep water is the lurkers' noncombat encounter,
            // and submerged players are out of combat's reach.
            if !player.is_alive() || self.unit_water_mode(entity).swims() {
                continue;
            }
            let position = self.player_position(player_id)?;
            let distance = xz_distance_squared(here, [position.x, position.z]);
            let radius = i64::from(if territorial {
                crate::TERRITORY_RADIUS_UNITS
            } else if family.ambushes() {
                crate::AMBUSH_RADIUS_UNITS
            } else {
                aggro_radius(level, player.level)
            });
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
        // Props (#288) never engage.
        if self.is_prop(EntityRef::Creature(id)) {
            return Ok(());
        }
        let Some(creature) = self.creatures.get_mut(&id) else {
            return Ok(());
        };
        if !creature.is_alive() || matches!(creature.ai, CreatureAi::Evading { .. }) {
            return Ok(());
        }
        let was_idle = matches!(
            creature.ai,
            CreatureAi::Idle { .. } | CreatureAi::Rallying { .. } | CreatureAi::Trailing { .. }
        );
        creature.add_threat(target, 0);
        creature.ai = CreatureAi::Engaged;
        if was_idle {
            self.arm_creature_ability(id)?;
            self.lock_encounter(id, target)?;
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
        // A warm buffer: the query allocates nothing (#1086). Helpers engage without assist,
        // so nothing below queries again while it is out.
        let mut near = std::mem::take(&mut self.creature_scratch.near);
        self.interest
            .near_into(here[0], here[1], ASSIST_RADIUS_UNITS, &mut near);
        let assisted = self.assist(id, target, template, here, &near);
        self.creature_scratch.near = near;
        assisted
    }

    /// Idle creatures of `template`'s family among `near` within the assist radius of `here`
    /// join creature `id`'s fight with `target`.
    fn assist(
        &mut self,
        id: CreatureId,
        target: EntityRef,
        template: &crate::CreatureTemplate,
        here: [i32; 2],
        near: &[EntityRef],
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for &entity in near {
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
            let distance = xz_distance_squared(here, [helper_position.x, helper_position.z]);
            if distance <= i64::from(ASSIST_RADIUS_UNITS).pow(2) {
                self.engage(other, target, false)?;
            }
        }
        Ok(())
    }
}

impl ZoneSimulation {
    /// A resting boss's first engagement (#276, 5.1.76): snapshots the players actually fighting
    /// it, the puller and those already in a threat table of its family within the engage
    /// radius, and locks its scaled health for the attempt before any damage resolves.
    fn lock_encounter(&mut self, id: CreatureId, target: EntityRef) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (spawn, template) = Self::creature_content(&content, id)?;
        let Some(rules) = content.boss(spawn.template) else {
            return Ok(());
        };
        let here = self.unit_position(EntityRef::Creature(id))?;
        let mut participants = BTreeSet::new();
        if let EntityRef::Player(player) = target {
            participants.insert(player);
        }
        for entity in self.interest.near(here[0], here[2], rules.engage_radius) {
            let EntityRef::Creature(other) = entity else {
                continue;
            };
            let Some(fighter) = self.creatures.get(&other) else {
                continue;
            };
            if other == id || !fighter.is_alive() || fighter.ai != CreatureAi::Engaged {
                continue;
            }
            let (_, other_template) = Self::creature_content(&content, other)?;
            if other_template.family != template.family {
                continue;
            }
            participants.extend(
                fighter
                    .threat
                    .iter()
                    .filter_map(|entry| match entry.entity {
                        EntityRef::Player(player) => Some(player),
                        _ => None,
                    }),
            );
        }
        let Some(creature) = self.creatures.get_mut(&id) else {
            return Ok(());
        };
        if participants.is_empty() {
            return Ok(());
        }
        let base = template.max_health(creature.level);
        let encounter = crate::BossEncounter {
            participants: participants
                .into_iter()
                .take(crate::MAX_ENCOUNTER_PLAYERS)
                .collect(),
            contributors: Vec::new(),
        };
        let scaled = encounter.scale_health(rules, base);
        creature.health = creature.health.saturating_add(scaled.saturating_sub(base));
        creature.encounter = Some(encounter);
        self.begin_boss_attempt(id);
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

/// Walking velocity toward `(dx, dz)` that a creature may keep for `ticks` ticks without
/// overshooting it (#814).
fn walk(dx: i32, dz: i32, ticks: u16) -> [i32; 2] {
    approach(dx, dz, 0, CREATURE_WALK_SPEED_SUBUNITS_PER_TICK, ticks)
}

/// A velocity toward `(dx, dz)` at `speed` that stops `stop` short and does not overshoot in
/// the `ticks` it is held: walking creatures and den wildlife step this way (#1266).
pub(crate) fn approach(dx: i32, dz: i32, stop: i32, speed: i32, ticks: u16) -> [i32; 2] {
    let length = i64::try_from(xz_length(dx, dz)).unwrap_or(i64::MAX);
    let gap = length.saturating_sub(i64::from(stop)).max(0);
    let reach = gap.saturating_mul(SUBUNITS) / i64::from(ticks.max(1));
    steer(
        dx,
        dz,
        i32::try_from(i64::from(speed).min(reach)).unwrap_or(0),
    )
}

/// Idle behaviour: count down, pick a wander point, walk to it, repeat. `elapsed` ticks have
/// passed since the last call, and the velocity holds until the next (#814).
/// Returns the horizontal velocity and a new facing, if any.
fn wander(
    rng: &mut ZoneRng,
    ai: &mut CreatureAi,
    here: [i32; 2],
    home: [i32; 2],
    radius: i32,
    elapsed: u16,
) -> ([i32; 2], Option<u16>) {
    let CreatureAi::Idle { timer, destination } = ai else {
        return ([0, 0], None);
    };
    match *destination {
        Some(to) => {
            let arrived = crate::reach::within_xz(here, to, ARRIVAL_RADIUS_UNITS);
            if arrived || *timer == 0 {
                *destination = None;
                *timer = wander_wait(rng);
                ([0, 0], None)
            } else {
                *timer = timer.saturating_sub(elapsed);
                let (dx, dz) = offset(here, to);
                (walk(dx, dz, elapsed), yaw_from_vector(dx, dz))
            }
        }
        None if *timer > 0 => {
            *timer = timer.saturating_sub(elapsed);
            ([0, 0], None)
        }
        None => {
            // A random heading and distance around the spawn point.
            let yaw = u16::try_from(rng.below(1 << 16)).unwrap_or(0);
            let distance = i32::try_from(rng.inclusive(0, radius.unsigned_abs())).unwrap_or(0);
            let to = trig::point_along(home, yaw, distance);
            *destination = Some(to);
            *timer = WANDER_WALK_TIMEOUT_TICKS;
            let (dx, dz) = offset(here, to);
            (walk(dx, dz, elapsed), yaw_from_vector(dx, dz))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_normalises_with_integers_and_never_overshoots() {
        let s = crate::physics::SUBUNITS_PER_UNIT;
        assert_eq!(steer(0, 0, 19 * s), [0, 0]);
        assert_eq!(steer(300, 400, 19 * s), [2918, 3891]);
        assert_eq!(steer(-300, 400, 19 * s), [-2918, 3891]);
        assert_eq!(
            steer(3, 4, 19 * s),
            [3 * s, 4 * s],
            "a short step lands on the target"
        );
        assert_eq!(steer(1_000, 0, 24 * s), [24 * s, 0]);
    }

    #[test]
    fn distances_and_offsets_never_overflow() {
        let (low, high) = ([i32::MIN, i32::MIN], [i32::MAX, i32::MAX]);
        assert_eq!(xz_distance_squared(low, high), i64::MAX);
        assert_eq!(xz_distance_squared([i32::MIN, 0], [i32::MAX, 0]), i64::MAX);
        assert_eq!(xz_distance_squared([-3, 4], [0, 0]), 25);
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
                    let to = trig::point_along(home, yaw, distance);
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
        assert_ne!(
            chase_velocity(203, -109, reach, CREATURE_RUN_SPEED_SUBUNITS_PER_TICK),
            [0, 0],
            "no stall at 230.4"
        );
        let s = f64::from(crate::physics::SUBUNITS_PER_UNIT);
        let units = |v: i32| f64::from(v) / s;
        for dx in (-300..=300).step_by(3) {
            for dz in (-300..=300).step_by(3) {
                let before = xz_distance_squared([0, 0], [dx, dz]);
                let [vx, vz] = chase_velocity(dx, dz, reach, CREATURE_RUN_SPEED_SUBUNITS_PER_TICK);
                if before <= reach_squared {
                    assert_eq!([vx, vz], [0, 0], "({dx}, {dz}) is inside reach");
                    continue;
                }
                // The body moves by the velocity in units.
                let (x, z) = (f64::from(dx) - units(vx), f64::from(dz) - units(vz));
                let after = x * x + z * z;
                #[allow(clippy::cast_precision_loss)]
                let before = before as f64;
                assert!(after < before, "({dx}, {dz}) makes progress");
                let within_last_step = i64::try_from(xz_length(dx, dz)).unwrap()
                    <= i64::from(reach)
                        + i64::from(
                            CREATURE_RUN_SPEED_SUBUNITS_PER_TICK
                                / crate::physics::SUBUNITS_PER_UNIT,
                        )
                        - CHASE_CLOSE_IN_UNITS;
                if within_last_step {
                    #[allow(clippy::cast_precision_loss)]
                    let inside = reach_squared as f64;
                    assert!(after <= inside, "({dx}, {dz}) arrives inside reach");
                }
            }
        }
    }
}
