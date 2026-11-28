//! Server-simulated livestock (#1019): a stray to drive home and a flock to keep through a night
//! watch. Both are NPCs the zone moves and wounds; clients draw them as farm animals.
//!
//! - **Stray** ([`Stray`]): an NPC standing at its scatter point (its authored post) that shies
//!   from players. A living player within [`SHY_RADIUS_UNITS`] drives it: it trots straight away
//!   from the nearest one at [`STRAY_UNITS_PER_TICK`], and when that way is blocked (a fence, a
//!   wall, a hurdle) it tries 45° and then 90° to either side, so a player herds it along a wall.
//!   Physics owns the geometric test (the creatures' straight-way shape cast). Once it stands in
//!   its authored pen it is home: every [`crate::QuestObjective::Herd`] of it is done for the
//!   player who drove it in and each party mate within quest reach, once per character and only
//!   once the quest's earlier objectives (its tracks) are done, and it
//!   grazes in the pen for [`PEN_REST_TICKS`] before it slips out to its scatter point again. A
//!   stray left alone or stuck away from its scatter point for [`STRAY_TIMEOUT_TICKS`] bolts back
//!   there ([`crate::ErrorCode::StrayBolted`] to its driver).
//! - **Watch** ([`HedgeWatch`]): a [`crate::QuestObjective::Watch`]'s
//!   [`crate::ZoneCommand::QuestAction`] at the shepherd starts one when the flock stands and no
//!   watch runs; otherwise [`crate::ErrorCode::FlockUnsettled`]. Its prowlers, reserved wolf spawns,
//!   stand on their authored ticks and go for the flock: each first to its gap in the hedge, where
//!   the bait or snare laid there holds it [`GAP_HOLD_TICKS`], then for the nearest standing sheep,
//!   which it bites on its swing timer for its damage range's midpoint. A prowler picks no fight
//!   with a player but answers one that strikes it. The watch ends when every prowler has stood
//!   and none still stands, when fewer than [`HedgeWatch::hold`] sheep stand, or after
//!   [`WATCH_TICKS`]; the sheep standing decide it, not the kills. Held, every player within its
//!   radius with the watch objective ready (the bait and snare laid) completes it; lost, each hears
//!   [`crate::ErrorCode::FlockFell`]. Prowlers still standing slink off, and after
//!   [`RECOVER_TICKS`] the flock stands whole again and the watch may be tried again.
//!
//! Driven strays and the watch are canonical zone state, so snapshots, replay and recovery
//! continue them; a stray at its scatter point and a flock without a watch store nothing.

use std::sync::Arc;

use crate::creature::Life;
use crate::events::push_event;
use crate::physics::Vec3i;
use crate::reach::xz_distance;
use crate::snapshot::CreatureAi;
use crate::{
    CreatureId, EntityRef, ErrorCode, NpcId, PlayerId, QuestObjective, ZoneError, ZoneEvent,
    ZoneSimulation,
};

/// A stray shies from a living player this close (5 m).
pub const SHY_RADIUS_UNITS: i32 = 500;
/// A driven stray trots this many units per tick (2.4 m/s), slower than a walking player.
pub const STRAY_UNITS_PER_TICK: i32 = crate::units_per_tick(240);
/// A stray looks this far ahead for a blocker before each step (0.6 m).
pub const STRAY_LOOKAHEAD_UNITS: i32 = 60;
/// A stray alone or stuck away from its scatter point this long (30 s) bolts back there.
pub const STRAY_TIMEOUT_TICKS: u16 = crate::ticks_from_ms(30_000);
/// A stray driven home grazes in its pen this long (1 min) before it slips out again.
pub const PEN_REST_TICKS: u32 = crate::long_ticks_from_ms(60_000);
/// A stray's body for the way test: a sheep, 0.8 m across and 0.9 m tall.
const STRAY_HALF_EXTENTS: [i32; 3] = [40, 45, 40];
/// A prowler at its gap is held there this long (6 s) by the bait or snare laid there.
pub const GAP_HOLD_TICKS: u16 = crate::ticks_from_ms(6_000);
/// A prowler this close to its gap is at it.
pub const GAP_REACH_UNITS: i32 = 150;
/// A prowler bites a standing sheep this close (1.4 m) ...
pub const BITE_REACH_UNITS: i32 = 140;
/// ... and closes in to stop this close.
const BITE_STOP_UNITS: i32 = 100;
/// A watch lasts at most this long (3 min); prowlers still standing then slink off.
pub const WATCH_TICKS: u32 = crate::long_ticks_from_ms(180_000);
/// After a watch the flock stands whole again this long after (45 s): longer than a corpse
/// lies, so every prowler is gone before the next watch.
pub const RECOVER_TICKS: u32 = crate::long_ticks_from_ms(45_000);
/// Most sheep a watch's flock and most prowlers it may hold.
pub const MAX_FLOCK: usize = 8;
pub const MAX_PROWLERS: usize = 8;
/// Most strays a zone may hold.
pub const MAX_STRAYS: usize = 64;

/// A stray (#1019): the NPC `npc` shies from players at its post and is driven into `pen`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Stray {
    pub npc: NpcId,
    /// The pen, inclusive `(min, max)` XZ in units.
    pub pen: ([i32; 2], [i32; 2]),
}

impl Stray {
    /// Whether `at` stands in the pen.
    #[must_use]
    pub const fn penned(&self, at: [i32; 2]) -> bool {
        let (min, max) = self.pen;
        at[0] >= min[0] && at[0] <= max[0] && at[1] >= min[1] && at[1] <= max[1]
    }
}

/// One prowler of a watch (#1019): a reserved creature spawn standing `after_ticks` into the
/// watch, held at `gap` before it goes for the flock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Prowler {
    pub creature: CreatureId,
    pub after_ticks: u32,
    pub gap: [i32; 2],
}

/// A night watch over a flock (#1019), started at its shepherd `npc`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HedgeWatch {
    pub npc: NpcId,
    /// The sheep at stake, NPCs standing at their posts.
    pub flock: Vec<NpcId>,
    /// Each sheep's health.
    pub health: u32,
    /// Sheep that must stand for the watch to hold.
    pub hold: u8,
    pub prowlers: Vec<Prowler>,
    /// The watch's ground: players within `radius` of `centre` share its outcome.
    pub centre: [i32; 2],
    pub radius: i32,
}

impl HedgeWatch {
    /// The index of `npc` in the flock, if it is one of it.
    #[must_use]
    pub fn sheep(&self, npc: NpcId) -> Option<usize> {
        self.flock.iter().position(|&sheep| sheep == npc)
    }

    /// The index of `creature` among the prowlers, if it is one.
    #[must_use]
    pub fn prowler(&self, creature: CreatureId) -> Option<usize> {
        self.prowlers
            .iter()
            .position(|prowler| prowler.creature == creature)
    }
}

/// A stray away from its scatter point or resting in its pen (#1019), canonical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StrayState {
    pub npc: NpcId,
    /// Its feet XZ, in units.
    pub position: [i32; 2],
    pub facing: u16,
    /// Units per tick of its last step.
    pub velocity: [i8; 2],
    /// The player it last shied from.
    pub driver: Option<PlayerId>,
    /// Ticks it has stood without a step away from its scatter point.
    pub still: u16,
    /// Zero while out; once penned, the tick it slips out to its scatter point.
    pub rests_until: u64,
}

/// A running or settling watch (#1019), canonical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchState {
    /// The tick it began.
    pub started: u64,
    /// Each sheep's health left, in flock order.
    pub flock: Vec<u32>,
    /// Ticks each prowler has been held at its gap, in prowler order.
    pub held: Vec<u16>,
    /// Bit `i`: prowler `i` has stood.
    pub stood: u8,
    /// Zero while it runs; once over, the tick the flock stands whole again.
    pub recovers_at: u64,
}

impl WatchState {
    /// The sheep still standing.
    #[must_use]
    pub fn standing(&self) -> u8 {
        u8::try_from(self.flock.iter().filter(|&&health| health > 0).count()).unwrap_or(u8::MAX)
    }
}

impl ZoneSimulation {
    /// Where stray `npc` stands while it is driven or penned (#1019): feet, facing and velocity.
    pub(crate) fn stray_placement(&self, npc: NpcId) -> Option<crate::NpcPlacement> {
        self.strays.get(&npc).map(|stray| crate::NpcPlacement {
            position: stray.position,
            facing: stray.facing,
            velocity: stray.velocity,
        })
    }

    /// The running or settling watch (#1019), if any.
    #[must_use]
    pub fn watch(&self) -> Option<&WatchState> {
        self.watch.as_ref()
    }

    /// The driven or penned stray `npc` (#1019), if it is away from its scatter point.
    #[must_use]
    pub fn stray(&self, npc: NpcId) -> Option<&StrayState> {
        self.strays.get(&npc)
    }

    /// The health of sheep `npc` of a watch in percent (#1019), rounded up while any is left;
    /// `None` outside a watch.
    #[must_use]
    pub fn sheep_health(&self, npc: NpcId) -> Option<u8> {
        let watch = self.content.watch()?;
        let state = self.watch.as_ref()?;
        let left = u64::from(*state.flock.get(watch.sheep(npc)?)?);
        let full = u64::from(watch.health.max(1));
        u8::try_from((left * 100).div_ceil(full)).ok()
    }

    /// Whether `creature` is a prowler of a running watch (#1019): it never rests, picks no
    /// fights and walks its prowl instead of wandering.
    pub(crate) fn prowling(&self, creature: CreatureId) -> bool {
        self.watch
            .as_ref()
            .is_some_and(|state| state.recovers_at == 0)
            && self
                .content
                .watch()
                .is_some_and(|watch| watch.prowler(creature).is_some())
    }

    /// The watch objective's `QuestAction` at its shepherd (#1019): starts a watch when the flock
    /// stands and none runs; `false` when it may not start.
    pub(crate) fn begin_watch(&mut self) -> bool {
        let Some(watch) = self.content.watch() else {
            return false;
        };
        let gone = watch.prowlers.iter().all(|prowler| {
            self.creatures
                .get(&prowler.creature)
                .is_none_or(|state| matches!(state.life, Life::Despawned { .. }))
        });
        if self.watch.is_some() || !gone {
            return false;
        }
        self.watch = Some(WatchState {
            started: self.tick,
            flock: vec![watch.health; watch.flock.len()],
            held: vec![0; watch.prowlers.len()],
            stood: 0,
            recovers_at: 0,
        });
        true
    }

    /// Step 2 for an idle prowler of a running watch (#1019): toward its gap until it has been
    /// held there, then toward the nearest standing sheep, stopping within bite reach. `None` for
    /// any other creature.
    pub(crate) fn prowl(
        &mut self,
        id: CreatureId,
        centre: [f32; 3],
        half: Vec3i,
    ) -> Result<Option<crate::raid::Steering>, ZoneError> {
        if !self.prowling(id)
            || !self
                .creatures
                .get(&id)
                .is_some_and(|state| matches!(state.ai, CreatureAi::Idle { .. }))
        {
            return Ok(None);
        }
        let content = Arc::clone(&self.content);
        let (Some(watch), Some(state)) = (content.watch(), self.watch.as_ref()) else {
            return Ok(None);
        };
        let Some(index) = watch.prowler(id) else {
            return Ok(None);
        };
        #[allow(clippy::cast_possible_truncation)]
        let here = [centre[0].round() as i32, centre[2].round() as i32];
        let (goal, stop, speed) = if state.held[index] < GAP_HOLD_TICKS {
            (
                watch.prowlers[index].gap,
                crate::ARRIVAL_RADIUS_UNITS,
                crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK,
            )
        } else {
            let health = state.flock.clone();
            match self.nearest_sheep(&health, here)? {
                Some((_, at)) => (
                    at,
                    BITE_STOP_UNITS,
                    crate::CREATURE_RUN_SPEED_SUBUNITS_PER_TICK,
                ),
                None => return Ok(Some(([0, 0], None))),
            }
        };
        if xz_distance(here, goal) <= i64::from(stop) {
            let (dx, dz) = crate::ai::offset(here, goal);
            return Ok(Some(([0, 0], crate::trig::yaw_from_vector(dx, dz))));
        }
        let toward = self
            .route_waypoint(centre, half, goal, stop)?
            .unwrap_or(goal);
        let (dx, dz) = crate::ai::offset(here, toward);
        Ok(Some((
            crate::ai::steer(dx, dz, speed),
            crate::trig::yaw_from_vector(dx, dz),
        )))
    }

    /// The nearest sheep of the watch to `here` still standing by `health` (flock order), the
    /// first in flock order on a tie: its index and feet.
    fn nearest_sheep(
        &self,
        health: &[u32],
        here: [i32; 2],
    ) -> Result<Option<(usize, [i32; 2])>, ZoneError> {
        let Some(watch) = self.content.watch() else {
            return Ok(None);
        };
        let mut nearest: Option<(i64, usize, [i32; 2])> = None;
        for (index, (&sheep, &health)) in watch.flock.iter().zip(health).enumerate() {
            if health == 0 {
                continue;
            }
            let at = self
                .npc_feet(sheep)
                .ok_or_else(|| ZoneError::new("watched sheep has no placement"))?;
            let far = xz_distance(here, at);
            if nearest.is_none_or(|(best, ..)| far < best) {
                nearest = Some((far, index, at));
            }
        }
        Ok(nearest.map(|(_, index, at)| (index, at)))
    }

    /// Tick step (#1019): every stray shies, is penned or bolts, and a running watch stands its
    /// prowlers, holds them at their gaps, lets them bite and ends (see the module
    /// documentation).
    pub(crate) fn advance_flock(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for stray in content.strays() {
            self.tick_work.strays_visited += 1;
            self.advance_stray(stray, now)?;
        }
        self.advance_watch(now)
    }

    /// One stray's step: see the module documentation.
    fn advance_stray(&mut self, stray: &Stray, now: u64) -> Result<(), ZoneError> {
        let npc = stray.npc;
        let post = self
            .content
            .npc(npc)
            .map(|definition| definition.position)
            .ok_or_else(|| ZoneError::new("stray npc is missing"))?;
        let mut state = self.strays.get(&npc).cloned().unwrap_or(StrayState {
            npc,
            position: post,
            facing: self
                .content
                .npc(npc)
                .map_or(0, |definition| definition.facing),
            velocity: [0, 0],
            driver: None,
            still: 0,
            rests_until: 0,
        });
        if state.rests_until > 0 {
            if now >= state.rests_until {
                self.strays.remove(&npc);
            }
            return Ok(());
        }
        let here = state.position;
        let mut stepped = false;
        if let Some((player, from)) = self.nearest_driver(here)? {
            let away = [here[0] - from[0], here[1] - from[1]];
            let yaw = crate::trig::yaw_from_vector(away[0], away[1]).unwrap_or(state.facing);
            for turn in [0_u16, 8_192, 57_344, 16_384, 49_152] {
                let heading = yaw.wrapping_add(turn);
                self.tick_work.stray_ways_tested += 1;
                if !self.stray_way_clear(here, heading) {
                    continue;
                }
                let step = crate::trig::point_along([0, 0], heading, STRAY_UNITS_PER_TICK);
                state.position = [here[0] + step[0], here[1] + step[1]];
                state.velocity =
                    step.map(|axis| i8::try_from(axis.clamp(-127, 127)).unwrap_or_default());
                state.facing = heading;
                state.driver = Some(player);
                state.still = 0;
                stepped = true;
                break;
            }
        }
        if !stepped {
            state.velocity = [0, 0];
            if state.position == post && state.driver.is_none() {
                // Untouched at its scatter point: nothing to store.
                self.strays.remove(&npc);
                return Ok(());
            }
            state.still = state.still.saturating_add(1);
        }
        if stray.penned(state.position) {
            state.velocity = [0, 0];
            state.rests_until = now.saturating_add(u64::from(PEN_REST_TICKS));
            let driver = state.driver;
            self.strays.insert(npc, state);
            if let Some(driver) = driver {
                self.credit_herd(driver, npc)?;
            }
            return Ok(());
        }
        if state.still > STRAY_TIMEOUT_TICKS {
            self.strays.remove(&npc);
            if let Some(driver) = state.driver
                && let Some(player) = self.players.get_mut(&driver)
            {
                push_event(
                    &mut player.events,
                    ZoneEvent::Error {
                        code: ErrorCode::StrayBolted,
                        target: Some(EntityRef::Npc(npc)),
                    },
                );
            }
            return Ok(());
        }
        self.strays.insert(npc, state);
        Ok(())
    }

    /// The nearest living player within [`SHY_RADIUS_UNITS`] of `here`, the lowest ID on a tie,
    /// with its feet.
    fn nearest_driver(&self, here: [i32; 2]) -> Result<Option<(PlayerId, [i32; 2])>, ZoneError> {
        let mut nearest: Option<(i64, PlayerId, [i32; 2])> = None;
        for entity in self.interest.near(here[0], here[1], SHY_RADIUS_UNITS) {
            let EntityRef::Player(player) = entity else {
                continue;
            };
            if !self
                .players
                .get(&player)
                .is_some_and(|state| state.is_alive())
            {
                continue;
            }
            let at = self.player_position(player)?;
            let at = [at.x, at.z];
            let far = xz_distance(here, at);
            if far <= i64::from(SHY_RADIUS_UNITS) && nearest.is_none_or(|(best, ..)| far < best) {
                nearest = Some((far, player, at));
            }
        }
        Ok(nearest.map(|(_, player, at)| (player, at)))
    }

    /// Whether a sheep at `here` trots clear toward `heading`.
    fn stray_way_clear(&self, here: [i32; 2], heading: u16) -> bool {
        let ahead = crate::trig::point_along([0, 0], heading, STRAY_LOOKAHEAD_UNITS);
        let half = Vec3i::new(
            STRAY_HALF_EXTENTS[0],
            STRAY_HALF_EXTENTS[1],
            STRAY_HALF_EXTENTS[2],
        );
        let feet = self.content.definition().ground_at(here[0], here[1]);
        #[allow(clippy::cast_precision_loss)]
        let centre = [
            here[0] as f32,
            (feet + STRAY_HALF_EXTENTS[1]) as f32,
            here[1] as f32,
        ];
        self.straight_way_clear(centre, half, [here[0] + ahead[0], here[1] + ahead[1]], 0)
    }

    /// A stray penned by `driver` (#1019): its herd objective is done for the driver and each
    /// living party mate within quest reach of it, once each.
    fn credit_herd(&mut self, driver: PlayerId, npc: NpcId) -> Result<(), ZoneError> {
        let herd = QuestObjective::Herd { npc };
        if self.players.contains_key(&driver) {
            self.complete_ready(driver, herd);
        }
        for mate in self.party_mates_near(driver, npc)? {
            self.complete_ready(mate, herd);
        }
        Ok(())
    }

    /// A running watch's step: see the module documentation.
    fn advance_watch(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let (Some(watch), Some(mut state)) = (content.watch(), self.watch.clone()) else {
            return Ok(());
        };
        if state.recovers_at > 0 {
            if now >= state.recovers_at {
                self.watch = None;
            }
            return Ok(());
        }
        let elapsed = now.saturating_sub(state.started);
        for (index, prowler) in watch.prowlers.iter().enumerate() {
            self.tick_work.prowlers_visited += 1;
            let bit = 1_u8 << index;
            if state.stood & bit == 0 {
                if elapsed >= u64::from(prowler.after_ticks) {
                    state.stood |= bit;
                    self.spawn_creature(prowler.creature)?;
                }
                continue;
            }
            let Some(creature) = self.creatures.get(&prowler.creature) else {
                continue;
            };
            if !creature.is_alive()
                || !matches!(creature.ai, CreatureAi::Idle { .. })
                || crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun)
            {
                continue;
            }
            let ready = self.objective_swing_ready(prowler.creature);
            let at = self.unit_position(EntityRef::Creature(prowler.creature))?;
            let here = [at[0], at[2]];
            if state.held[index] < GAP_HOLD_TICKS {
                if xz_distance(here, prowler.gap) <= i64::from(GAP_REACH_UNITS) {
                    state.held[index] += 1;
                }
                continue;
            }
            if !ready {
                continue;
            }
            let Some((sheep, feet)) = self.nearest_sheep(&state.flock, here)? else {
                continue;
            };
            if xz_distance(here, feet) > i64::from(BITE_REACH_UNITS) {
                continue;
            }
            let (_, template) = Self::creature_content(&content, prowler.creature)?;
            let bitten = Some(EntityRef::Npc(watch.flock[sheep]));
            if let Some(amount) = self.strike_objective(prowler.creature, template, bitten) {
                state.flock[sheep] = state.flock[sheep].saturating_sub(amount);
            }
        }
        let all_stood =
            usize::try_from(state.stood.count_ones()).ok() == Some(watch.prowlers.len());
        let none_left = watch.prowlers.iter().all(|prowler| {
            self.creatures
                .get(&prowler.creature)
                .is_none_or(|creature| !creature.is_alive())
        });
        let lost = state.standing() < watch.hold;
        let over = elapsed >= u64::from(WATCH_TICKS);
        if lost || (all_stood && none_left) || over {
            state.recovers_at = now.saturating_add(u64::from(RECOVER_TICKS));
            self.watch = Some(state);
            self.end_watch(watch, !lost, now)?;
        } else {
            self.watch = Some(state);
        }
        Ok(())
    }

    /// A watch ended (#1019): prowlers still standing slink off, and each player on the watch's
    /// ground with its objective open completes it if it `held`, or hears that it fell.
    fn end_watch(&mut self, watch: &HedgeWatch, held: bool, now: u64) -> Result<(), ZoneError> {
        for prowler in &watch.prowlers {
            if self
                .creatures
                .get(&prowler.creature)
                .is_some_and(|creature| creature.is_alive())
            {
                self.retire_raider(prowler.creature, now)?;
            }
        }
        let shepherd = watch.npc;
        let objective = QuestObjective::Watch { npc: shepherd };
        let [x, z] = watch.centre;
        for entity in self.interest.near(x, z, watch.radius) {
            let EntityRef::Player(player) = entity else {
                continue;
            };
            let at = self.player_position(player)?;
            if xz_distance([at.x, at.z], watch.centre) > i64::from(watch.radius)
                || !self.ready_objective(player, objective)
            {
                continue;
            }
            if held {
                self.complete_ready(player, objective);
            } else if let Some(state) = self.players.get_mut(&player) {
                push_event(
                    &mut state.events,
                    ZoneEvent::Error {
                        code: ErrorCode::FlockFell,
                        target: Some(EntityRef::Npc(shepherd)),
                    },
                );
            }
        }
        Ok(())
    }

    /// Whether `player` has `objective` open in an active quest with every earlier objective of
    /// its quest done, so a quest's steps come in their authored order.
    pub(crate) fn ready_objective(&self, player: PlayerId, objective: QuestObjective) -> bool {
        let Some(state) = self.players.get(&player) else {
            return false;
        };
        state.quests.entries.iter().any(|entry| {
            self.content.quest(entry.quest).is_some_and(|quest| {
                let current = crate::quest::progress(quest, entry, &state.inventory, *state.depths);
                quest
                    .objectives
                    .iter()
                    .enumerate()
                    .any(|(index, candidate)| {
                        *candidate == objective
                            && current[index] < candidate.required()
                            && quest.objectives[..index]
                                .iter()
                                .zip(current)
                                .all(|(earlier, count)| count >= earlier.required())
                    })
            })
        })
    }

    /// Completes `objective` for `player` once if it is ready ([`Self::ready_objective`]).
    /// Completes `objective` for each player within `radius` of `at` that has it ready (#1273):
    /// escorts and patrols credit the players standing by when they arrive.
    pub(crate) fn credit_near(
        &mut self,
        at: [i32; 2],
        radius: i32,
        objective: QuestObjective,
    ) -> Result<(), ZoneError> {
        for entity in self.interest.near(at[0], at[1], radius) {
            let EntityRef::Player(player) = entity else {
                continue;
            };
            let feet = self.player_position(player)?;
            if xz_distance([feet.x, feet.z], at) <= i64::from(radius) {
                self.complete_ready(player, objective);
            }
        }
        Ok(())
    }

    pub(crate) fn complete_ready(&mut self, player: PlayerId, objective: QuestObjective) {
        if self.ready_objective(player, objective) {
            self.complete_objectives(player, |candidate| candidate == objective);
        }
    }

    /// Canonical strays in NPC order.
    pub(crate) fn canonical_strays(&self) -> Vec<StrayState> {
        self.strays.values().cloned().collect()
    }

    /// Restores strays and the watch from a snapshot taken at `tick`.
    pub(crate) fn restore_flock(
        &mut self,
        strays: &[StrayState],
        watch: Option<&WatchState>,
        tick: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        if strays.windows(2).any(|pair| pair[0].npc >= pair[1].npc)
            || strays.iter().any(|stray| {
                content.stray(stray.npc).is_none()
                    || stray.still > STRAY_TIMEOUT_TICKS
                    || stray.rests_until > tick.saturating_add(u64::from(PEN_REST_TICKS))
                    || stray
                        .position
                        .iter()
                        .any(|value| value.abs() > crate::MAX_CONTENT_COORDINATE_UNITS)
            })
        {
            return Err(ZoneError::new("snapshot strays are invalid"));
        }
        if let Some(state) = watch {
            let valid = content.watch().is_some_and(|watch| {
                state.flock.len() == watch.flock.len()
                    && state.flock.iter().all(|&health| health <= watch.health)
                    && state.held.len() == watch.prowlers.len()
                    && state.held.iter().all(|&held| held <= GAP_HOLD_TICKS)
                    && u32::from(state.stood) < (1_u32 << watch.prowlers.len())
                    && state.started <= tick
                    && state.recovers_at <= tick.saturating_add(u64::from(RECOVER_TICKS))
            });
            if !valid {
                return Err(ZoneError::new("snapshot watch is invalid"));
            }
        }
        self.strays = strays
            .iter()
            .map(|stray| (stray.npc, stray.clone()))
            .collect();
        self.watch = watch.cloned();
        Ok(())
    }
}
