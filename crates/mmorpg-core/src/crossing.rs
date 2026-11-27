//! A cart over the ford under attack (#1019, *A Crossing Under Fire*, *The Ford Watch*): a
//! carter drives his cart along an authored route across a ford while reserved raiders go for it,
//! and the players near it see it over only if it reaches the far bank alive.
//!
//! - **Crossing** ([`WagonCrossing`]): the carter NPC, whose post is the route's first point, the
//!   route to the far bank, the cart's health, the reserved raiders and the radius round the cart
//!   whose players share its arrival. The raiders stand only while a crossing runs and never
//!   respawn on their own.
//! - **Start**: a [`crate::QuestObjective::Crossing`]'s [`crate::ZoneCommand::QuestAction`] at the
//!   carter starts a crossing when none runs and every raider of the last one is gone. A start
//!   while one runs joins it and changes nothing; while the last one settles it is refused with
//!   [`crate::ErrorCode::CartResting`].
//! - **Drive**: the cart rolls along the route at [`CART_UNITS_PER_TICK`], its placement the
//!   point that far along the route, so it never leaves the authored line. It halts while a living
//!   raider stands within [`STRIKE_REACH_UNITS`] of it.
//! - **Raiders**: each stands at its spawn [`CrossingRaider::after_ticks`] into the crossing and
//!   runs for the cart. One out of a fight that reaches it strikes it on its swing timer for its
//!   damage range's midpoint. A raider picks no fight with a player but answers one that strikes
//!   it, so players protect the cart by drawing the raiders off or cutting them down.
//! - **Arrival**: the cart reaching the route's end with health left is over: every player within
//!   [`WagonCrossing::radius`] of it with the crossing objective ready completes it, once.
//!   Reaching the far bank oneself completes nothing.
//! - **Loss**: the cart at no health, or a crossing outlasting [`CROSSING_TICKS`], is lost: every
//!   player in the zone with the objective ready hears [`crate::ErrorCode::CartLost`].
//! - **Rest**: either way the raiders still standing leave, the cart stands where it ended for
//!   [`REST_TICKS`] (longer than a body lies), and then the carter is back at his post with a
//!   whole cart, ready to try again.
//!
//! Crossings are canonical zone state, so snapshots, replay and recovery continue them; a carter
//! at his post with every raider gone stores nothing.

use std::sync::Arc;

use crate::creature::Life;
use crate::events::push_event;
use crate::physics::Vec3i;
use crate::routine::distance;
use crate::snapshot::CreatureAi;
use crate::{
    CreatureId, EntityRef, ErrorCode, NpcId, NpcPlacement, QuestObjective, ZoneError, ZoneEvent,
    ZoneSimulation,
};

/// Most crossings a zone may hold.
pub const MAX_CROSSINGS: usize = 4;
/// Most raiders a crossing may have.
pub const MAX_CROSSING_RAIDERS: usize = 6;
/// Most points a crossing's route may have.
pub const MAX_CROSSING_ROUTE_POINTS: usize = 16;
/// The cart rolls this many units a tick (1.2 m/s), slower than a walking player.
pub const CART_UNITS_PER_TICK: i32 = crate::units_per_tick(120);
/// A raider this close (2 m) to the cart strikes it, and the cart halts while one stands there.
pub const STRIKE_REACH_UNITS: i32 = 200;
/// A raider running for the cart stops this close (1.4 m).
const STRIKE_STOP_UNITS: i32 = 140;
/// A crossing this long (4 min) has failed: the cart is lost.
pub const CROSSING_TICKS: u32 = crate::long_ticks_from_ms(240_000);
/// After a crossing the cart stands this long (45 s), longer than a body lies, before the carter
/// is back at his post.
pub const REST_TICKS: u32 = crate::long_ticks_from_ms(45_000);

/// One raider of a crossing (#1019): a reserved creature spawn standing `after_ticks` into it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrossingRaider {
    pub creature: CreatureId,
    pub after_ticks: u32,
}

/// A cart crossing a ford (#1019), started at its carter `npc`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WagonCrossing {
    /// The carter, posted at the route's first point.
    pub npc: NpcId,
    /// Feet XZ in units: the carter's post, the waypoints, then the far bank.
    pub route: Vec<[i32; 2]>,
    /// The cart's health.
    pub health: u32,
    pub raiders: Vec<CrossingRaider>,
    /// Players this close to the cart when it arrives see it over.
    pub radius: i32,
}

impl WagonCrossing {
    /// The index of `creature` among the raiders, if it is one.
    #[must_use]
    pub fn raider(&self, creature: CreatureId) -> Option<usize> {
        self.raiders
            .iter()
            .position(|raider| raider.creature == creature)
    }

    /// The route's length in units.
    #[must_use]
    pub fn length(&self) -> u32 {
        let total: i64 = self.legs().map(|leg| leg.2).sum();
        u32::try_from(total).unwrap_or(u32::MAX)
    }

    /// The point `progress` units along the route and the heading of its leg there, sampled
    /// like every other route ([`crate::routine`], #1231). Content keeps every leg longer than
    /// zero, so the heading is always its leg's.
    #[must_use]
    pub fn point_at(&self, progress: u32) -> ([i32; 2], u16) {
        let placement = crate::routine::along_legs(self.legs(), i64::from(progress), true);
        (placement.position, placement.facing)
    }

    /// The route's legs, from its first point.
    fn legs(&self) -> impl Iterator<Item = crate::routine::Leg> + Clone + '_ {
        let (start, path) = self
            .route
            .split_first()
            .map_or(([0, 0], &[][..]), |(start, path)| (*start, path));
        crate::routine::leg_iter(start, path)
    }

    /// Whether no two consecutive route points are the same: content checks it.
    pub(crate) fn legs_have_length(&self) -> bool {
        self.legs().all(|leg| leg.2 > 0)
    }
}

/// A crossing under way or settling (#1019), canonical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CrossingRun {
    /// The carter.
    pub npc: NpcId,
    /// The tick it began.
    pub started: u64,
    /// Units the cart has rolled along the route.
    pub progress: u32,
    /// The cart's health left.
    pub health: u32,
    /// Bit `i`: raider `i` has stood.
    pub stood: u8,
    /// Whether a raider held the cart up on the last tick.
    pub halted: bool,
    /// Zero while it runs; once over, the tick the carter is back at his post.
    pub rests_until: u64,
}

impl ZoneSimulation {
    /// The crossing carter `npc` drives (#1019), under way or settling, if any.
    #[must_use]
    pub fn crossing_run(&self, npc: NpcId) -> Option<&CrossingRun> {
        self.crossing_runs.get(&npc)
    }

    /// Where carter `npc` stands while a crossing runs or settles (#1019): with his cart, rolling
    /// along the route unless held up.
    pub(crate) fn crossing_placement(&self, npc: NpcId) -> Option<NpcPlacement> {
        let run = self.crossing_runs.get(&npc)?;
        let crossing = self.content.crossing(npc)?;
        let (position, facing) = crossing.point_at(run.progress);
        let rolling = run.rests_until == 0 && !run.halted && run.progress < crossing.length();
        let velocity = if rolling {
            let (x, z) = crate::trig::direction(facing);
            [x, z].map(|axis| {
                i8::try_from(crate::trig::checked_scale(CART_UNITS_PER_TICK, axis).unwrap_or(0))
                    .unwrap_or_default()
            })
        } else {
            [0, 0]
        };
        Some(NpcPlacement {
            position,
            facing,
            velocity,
        })
    }

    /// The health of carter `npc`'s cart in percent while a crossing runs or settles (#1019),
    /// rounded up while any is left.
    #[must_use]
    pub fn cart_health(&self, npc: NpcId) -> Option<u8> {
        let run = self.crossing_runs.get(&npc)?;
        let full = u64::from(self.content.crossing(npc)?.health.max(1));
        u8::try_from((u64::from(run.health) * 100).div_ceil(full)).ok()
    }

    /// Whether `creature` is a raider of a running crossing (#1019): it decides every tick, picks
    /// no fights and runs for the cart instead of wandering.
    pub(crate) fn crossing_raiding(&self, creature: CreatureId) -> bool {
        // Most ticks no crossing runs: every creature's question ends here.
        if self.crossing_runs.is_empty() {
            return false;
        }
        self.content
            .crossing_of(creature)
            .and_then(|crossing| self.crossing_runs.get(&crossing.npc))
            .is_some_and(|run| run.rests_until == 0)
    }

    /// A crossing objective's `QuestAction` at carter `npc` (#1019): starts a crossing, or joins
    /// the one under way; `false` while the last one settles or a raider of it remains.
    pub(crate) fn begin_crossing(&mut self, npc: NpcId) -> bool {
        let Some(crossing) = self.content.crossing(npc) else {
            return false;
        };
        if let Some(run) = self.crossing_runs.get(&npc) {
            return run.rests_until == 0;
        }
        let gone = crossing.raiders.iter().all(|raider| {
            self.creatures
                .get(&raider.creature)
                .is_none_or(|state| matches!(state.life, Life::Despawned { .. }))
        });
        if !gone {
            return false;
        }
        let run = CrossingRun {
            npc,
            started: self.tick,
            progress: 0,
            health: crossing.health,
            stood: 0,
            halted: false,
            rests_until: 0,
        };
        self.crossing_runs.insert(npc, run);
        true
    }

    /// Step 2 for an idle raider of a running crossing (#1019): toward the cart, stopping within
    /// strike reach. `None` for any other creature.
    pub(crate) fn crossing_step(
        &mut self,
        id: CreatureId,
        centre: [f32; 3],
        half: Vec3i,
    ) -> Result<Option<crate::raid::Steering>, ZoneError> {
        if !self.crossing_raiding(id)
            || !self
                .creatures
                .get(&id)
                .is_some_and(|state| matches!(state.ai, CreatureAi::Idle { .. }))
        {
            return Ok(None);
        }
        let Some(cart) = self
            .content
            .crossing_of(id)
            .and_then(|crossing| self.crossing_placement(crossing.npc))
        else {
            return Ok(None);
        };
        #[allow(clippy::cast_possible_truncation)]
        let here = [centre[0].round() as i32, centre[2].round() as i32];
        let goal = cart.position;
        let (dx, dz) = crate::ai::offset(here, goal);
        if distance(here, goal) <= i64::from(STRIKE_STOP_UNITS) {
            return Ok(Some(([0, 0], crate::trig::yaw_from_vector(dx, dz))));
        }
        let toward = self
            .route_waypoint(centre, half, goal, STRIKE_STOP_UNITS)?
            .unwrap_or(goal);
        let (dx, dz) = crate::ai::offset(here, toward);
        Ok(Some((
            crate::ai::steer(dx, dz, crate::CREATURE_RUN_SPEED_SUBUNITS_PER_TICK),
            crate::trig::yaw_from_vector(dx, dz),
        )))
    }

    /// Tick step (#1019): every content crossing's raiders stand and strike, its cart rolls, and
    /// it arrives, is lost or settles (see the module documentation).
    pub(crate) fn advance_crossings(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for crossing in content.crossings() {
            self.tick_work.crossings_visited += 1;
            let Some(mut run) = self.crossing_runs.get(&crossing.npc).cloned() else {
                continue;
            };
            if run.rests_until > 0 {
                let mut standing = false;
                for raider in &crossing.raiders {
                    self.tick_work.crossing_raiders_visited += 1;
                    if let Some(state) = self.creatures.get(&raider.creature)
                        && state.is_alive()
                    {
                        // A raider still fighting when the crossing ended leaves once it is over.
                        if matches!(state.ai, CreatureAi::Idle { .. }) {
                            self.retire_raider(raider.creature, now)?;
                        } else {
                            standing = true;
                        }
                    }
                }
                if now >= run.rests_until && !standing {
                    self.crossing_runs.remove(&crossing.npc);
                }
                continue;
            }
            let elapsed = now.saturating_sub(run.started);
            let (cart, _) = crossing.point_at(run.progress);
            let mut halted = false;
            for (index, raider) in crossing.raiders.iter().enumerate() {
                self.tick_work.crossing_raiders_visited += 1;
                let bit = 1_u8 << index;
                if run.stood & bit == 0 {
                    if elapsed >= u64::from(raider.after_ticks) {
                        run.stood |= bit;
                        self.spawn_creature(raider.creature)?;
                    }
                    continue;
                }
                let Some(creature) = self.creatures.get(&raider.creature) else {
                    continue;
                };
                if !creature.is_alive() {
                    continue;
                }
                let at = self.unit_position(EntityRef::Creature(raider.creature))?;
                if distance([at[0], at[2]], cart) > i64::from(STRIKE_REACH_UNITS) {
                    continue;
                }
                halted = true;
                if !self.objective_swing_ready(raider.creature) {
                    continue;
                }
                let (_, template) = Self::creature_content(&content, raider.creature)?;
                let carter = Some(EntityRef::Npc(crossing.npc));
                if let Some(amount) = self.strike_objective(raider.creature, template, carter) {
                    run.health = run.health.saturating_sub(amount);
                }
            }
            run.halted = halted;
            let length = crossing.length();
            if !halted && run.health > 0 {
                run.progress = run
                    .progress
                    .saturating_add(CART_UNITS_PER_TICK.unsigned_abs())
                    .min(length);
            }
            let arrived = run.health > 0 && run.progress >= length;
            let lost = run.health == 0 || elapsed >= u64::from(CROSSING_TICKS);
            if arrived || lost {
                run.rests_until = now.saturating_add(u64::from(REST_TICKS));
                self.crossing_runs.insert(crossing.npc, run.clone());
                self.end_crossing(crossing, &run, arrived, now)?;
            } else {
                self.crossing_runs.insert(crossing.npc, run);
            }
        }
        Ok(())
    }

    /// A crossing ended (#1019): its raiders still standing leave, and each player near the cart
    /// with the objective ready completes it if it `arrived`; otherwise each player with it ready
    /// hears the cart was lost.
    fn end_crossing(
        &mut self,
        crossing: &WagonCrossing,
        run: &CrossingRun,
        arrived: bool,
        now: u64,
    ) -> Result<(), ZoneError> {
        for raider in &crossing.raiders {
            if self
                .creatures
                .get(&raider.creature)
                .is_some_and(crate::creature::CreatureState::is_alive)
            {
                self.retire_raider(raider.creature, now)?;
            }
        }
        let objective = QuestObjective::Crossing { npc: crossing.npc };
        if arrived {
            let (at, _) = crossing.point_at(run.progress);
            for entity in self.interest.near(at[0], at[1], crossing.radius) {
                let EntityRef::Player(player) = entity else {
                    continue;
                };
                let feet = self.player_position(player)?;
                if distance([feet.x, feet.z], at) <= i64::from(crossing.radius) {
                    self.complete_ready(player, objective);
                }
            }
            return Ok(());
        }
        let waiting: Vec<_> = self
            .players
            .keys()
            .copied()
            .filter(|&player| self.ready_objective(player, objective))
            .collect();
        for player in waiting {
            if let Some(state) = self.players.get_mut(&player) {
                push_event(
                    &mut state.events,
                    ZoneEvent::Error {
                        code: ErrorCode::CartLost,
                        target: Some(EntityRef::Npc(crossing.npc)),
                    },
                );
            }
        }
        Ok(())
    }

    /// Canonical crossings in carter order.
    pub(crate) fn canonical_crossing_runs(&self) -> Vec<CrossingRun> {
        self.crossing_runs.values().cloned().collect()
    }

    /// Restores the crossings from a snapshot taken at `tick`.
    pub(crate) fn restore_crossing_runs(
        &mut self,
        runs: &[CrossingRun],
        tick: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let valid = runs.windows(2).all(|pair| pair[0].npc < pair[1].npc)
            && runs.iter().all(|run| {
                content.crossing(run.npc).is_some_and(|crossing| {
                    run.progress <= crossing.length()
                        && run.health <= crossing.health
                        && u32::from(run.stood) < (1_u32 << crossing.raiders.len())
                        && run.started <= tick
                        && run.rests_until <= tick.saturating_add(u64::from(REST_TICKS))
                })
            });
        if !valid {
            return Err(ZoneError::new("snapshot crossings are invalid"));
        }
        self.crossing_runs = runs.iter().map(|run| (run.npc, run.clone())).collect();
        Ok(())
    }
}
