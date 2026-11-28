//! Supply patrols on the road (#1019): a column of creatures that walks an authored route to its
//! camp when a lookout calls it out, to be watched from the lookout and stopped on the way.
//!
//! - **Patrol** ([`SupplyPatrol`]): reserved creature spawns, the first of them the carrier, an
//!   authored route from the point where they form up to their camp, and the lookout NPC who
//!   watches the road. The members stand only while a run is under way and never respawn on their
//!   own.
//! - **Call**: a [`crate::QuestObjective::Sight`] or [`crate::QuestObjective::Ambush`]'s
//!   [`crate::ZoneCommand::QuestAction`] at the lookout calls the patrol out when none is under
//!   way and every member of the last run is gone: the members stand at their posts and set out.
//!   A call while one marches joins it and changes nothing; while the last run rests, or a member
//!   of it still stands or lies dead, it is refused with [`crate::ErrorCode::PatrolResting`].
//! - **March**: each member walks the route's waypoints in order at a walk, around static
//!   geometry; a member that meets a player fights as any creature does (aggro, assist, leash), its
//!   leash anchored where the fight began, and walks on from there once it is over.
//! - **Sighting**: while any living member is within [`SupplyPatrol::sight`] of the lookout, every
//!   player within [`SupplyPatrol::post`] of the lookout with a sight objective of it ready (its
//!   earlier objectives done) completes it.
//! - **Interception**: the carrier falling before it reaches the camp ends the run: every player
//!   within [`INTERCEPT_RADIUS_UNITS`] of its body with an ambush objective of the patrol ready
//!   completes it. The carrier's body yields what a quest collects from its template (the order
//!   pouch), as any creature's does for its tapper.
//! - **Arrival**: the carrier passing the route's last waypoint, or a march outlasting
//!   [`MARCH_TICKS`], ends the run as arrived: every player in the zone with a sight or ambush
//!   objective of the patrol ready hears [`crate::ErrorCode::PatrolArrived`] and asks the lookout
//!   again once the patrol has rested.
//! - **Rest**: after a run ends, living members out of a fight leave the world, and the patrol
//!   rests [`REST_TICKS`] (longer than a body lies) before it may be called again.
//!
//! Runs are canonical zone state, so snapshots, replay and recovery continue them; a patrol at
//! rest with every member gone stores nothing.

use std::sync::Arc;

use crate::creature::Life;
use crate::events::push_event;
use crate::physics::Vec3i;
use crate::reach::xz_distance;
use crate::snapshot::CreatureAi;
use crate::{
    CreatureId, EntityRef, ErrorCode, NpcId, QuestObjective, ZoneError, ZoneEvent, ZoneSimulation,
};

/// Most supply patrols a zone may hold.
pub const MAX_SUPPLY_PATROLS: usize = 16;
/// Most members a supply patrol may have.
pub const MAX_SUPPLY_MEMBERS: usize = 6;
/// Most points a supply patrol's route may have.
pub const MAX_SUPPLY_ROUTE_POINTS: usize = 16;
/// Players this close (40 m) to the carrier's body share its interception.
pub const INTERCEPT_RADIUS_UNITS: i32 = 4_000;
/// A march this long (5 min) has slipped through: it ends as arrived.
pub const MARCH_TICKS: u32 = crate::long_ticks_from_ms(300_000);
/// A patrol rests this long (45 s) after a run before it may be called again: longer than a
/// body lies, so every member of the last run is gone by then.
pub const REST_TICKS: u32 = crate::long_ticks_from_ms(45_000);

/// A supply patrol (#1019) the lookout `npc` calls out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupplyPatrol {
    /// The lookout who watches the road; quest actions at it call the patrol out.
    pub npc: NpcId,
    /// Reserved creature spawns in marching order; the first is the carrier.
    pub members: Vec<CreatureId>,
    /// Feet XZ in units: where the members form up, the waypoints, then the camp.
    pub route: Vec<[i32; 2]>,
    /// A living member this close to the lookout is in its sight.
    pub sight: i32,
    /// Players this close to the lookout watch with it.
    pub post: i32,
}

impl SupplyPatrol {
    /// The carrier, whose fall stops the patrol.
    #[must_use]
    pub fn carrier(&self) -> CreatureId {
        self.members[0]
    }

    /// The index of `creature` among the members, if it is one.
    #[must_use]
    pub fn member(&self, creature: CreatureId) -> Option<usize> {
        self.members.iter().position(|&member| member == creature)
    }
}

/// One member's progress along a run's route (#1019), canonical.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupplyMarch {
    /// The next route point it walks to; the route's length once it has passed the last.
    pub waypoint: u8,
    /// Where it last walked from: a fight leashes to it and evading returns to it.
    pub anchor: [i32; 2],
}

/// A supply patrol's run under way or at rest (#1019), canonical.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SupplyRun {
    /// The patrol's lookout.
    pub npc: NpcId,
    /// The tick it was called out.
    pub started: u64,
    /// Each member's march, in member order.
    pub marches: Vec<SupplyMarch>,
    /// Zero while it marches; once over, the tick it may be called again.
    pub rests_until: u64,
}

impl ZoneSimulation {
    /// The run of the patrol lookout `npc` calls (#1019), under way or at rest, if any.
    #[must_use]
    pub fn supply_run(&self, npc: NpcId) -> Option<&SupplyRun> {
        self.supply_runs.get(&npc)
    }

    /// The marching run and member index of `creature` (#1019), if it marches in one.
    fn marching(&self, creature: CreatureId) -> Option<(&SupplyPatrol, usize)> {
        // Most ticks no run is under way: every creature's question ends here.
        if self.supply_runs.is_empty() {
            return None;
        }
        let patrol = self.content.supply_patrol_of(creature)?;
        let index = patrol.member(creature)?;
        self.supply_runs
            .get(&patrol.npc)
            .filter(|run| run.rests_until == 0)
            .map(|_| (patrol, index))
    }

    /// Whether `creature` marches in a supply patrol's run (#1019): it decides every tick.
    pub(crate) fn supply_marching(&self, creature: CreatureId) -> bool {
        self.marching(creature).is_some()
    }

    /// Where a member of a run (#1019) leashes and evades to: where it last walked from.
    pub(crate) fn supply_anchor(&self, creature: CreatureId) -> Option<[i32; 2]> {
        if self.supply_runs.is_empty() {
            return None;
        }
        let patrol = self.content.supply_patrol_of(creature)?;
        let index = patrol.member(creature)?;
        self.supply_runs
            .get(&patrol.npc)
            .and_then(|run| run.marches.get(index))
            .map(|march| march.anchor)
    }

    /// A sight or ambush objective's `QuestAction` at the lookout `npc` (#1019): calls its patrol
    /// out, or joins the one marching; `false` while the last run rests or a member of it remains.
    pub(crate) fn call_supply_patrol(&mut self, npc: NpcId) -> Result<bool, ZoneError> {
        let content = Arc::clone(&self.content);
        let Some(patrol) = content.supply_patrol(npc) else {
            return Ok(false);
        };
        if let Some(run) = self.supply_runs.get(&npc) {
            return Ok(run.rests_until == 0);
        }
        let gone = patrol.members.iter().all(|member| {
            self.creatures
                .get(member)
                .is_none_or(|state| matches!(state.life, Life::Despawned { .. }))
        });
        if !gone {
            return Ok(false);
        }
        let mut marches = Vec::with_capacity(patrol.members.len());
        for &member in &patrol.members {
            self.spawn_creature(member)?;
            let (spawn, _) = Self::creature_content(&content, member)?;
            marches.push(SupplyMarch {
                waypoint: 0,
                anchor: spawn.position,
            });
        }
        self.supply_runs.insert(
            npc,
            SupplyRun {
                npc,
                started: self.tick,
                marches,
                rests_until: 0,
            },
        );
        Ok(true)
    }

    /// Step 2 for an idle member of a marching run (#1019): the velocity and facing toward its next
    /// route point around static geometry, advancing past points it has reached; standing still
    /// once it has passed the last. `None` for any other creature.
    pub(crate) fn supply_step(
        &mut self,
        id: CreatureId,
        centre: [f32; 3],
        half: Vec3i,
    ) -> Result<Option<crate::raid::Steering>, ZoneError> {
        let content = Arc::clone(&self.content);
        let Some((patrol, index)) = self.marching(id) else {
            return Ok(None);
        };
        if !self
            .creatures
            .get(&id)
            .is_some_and(|state| matches!(state.ai, CreatureAi::Idle { .. }))
        {
            return Ok(None);
        }
        let npc = patrol.npc;
        let Some(patrol) = content.supply_patrol(npc) else {
            return Ok(None);
        };
        #[allow(clippy::cast_possible_truncation)]
        let here = [centre[0].round() as i32, centre[2].round() as i32];
        let Some(march) = self
            .supply_runs
            .get(&npc)
            .and_then(|run| run.marches.get(index))
            .copied()
        else {
            return Ok(None);
        };
        let last = patrol.route.len().saturating_sub(1);
        // Route points on the way pass loosely; the camp, exactly.
        let radius = |point: usize| {
            if point >= last {
                crate::ARRIVAL_RADIUS_UNITS
            } else {
                crate::raid::WAYPOINT_RADIUS_UNITS
            }
        };
        let mut waypoint = usize::from(march.waypoint);
        while let Some(&point) = patrol.route.get(waypoint)
            && xz_distance(here, point) <= i64::from(radius(waypoint))
        {
            waypoint += 1;
        }
        if let Some(run) = self.supply_runs.get_mut(&npc)
            && let Some(march) = run.marches.get_mut(index)
        {
            march.waypoint = u8::try_from(waypoint).unwrap_or(u8::MAX);
            // A fight on the march leashes to where it began.
            march.anchor = here;
        }
        let Some(&goal) = patrol.route.get(waypoint) else {
            return Ok(Some(([0, 0], None)));
        };
        let toward = self.route_waypoint(centre, half, goal, 0)?.unwrap_or(goal);
        let (dx, dz) = crate::ai::offset(here, toward);
        Ok(Some((
            crate::ai::steer(dx, dz, crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK),
            crate::trig::yaw_from_vector(dx, dz),
        )))
    }

    /// Tick step (#1019): every content patrol's run is sighted, intercepted, arrives or rests (see
    /// the module documentation).
    pub(crate) fn advance_supply(&mut self, now: u64) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        for patrol in content.supply_patrols() {
            self.tick_work.supply_patrols_visited += 1;
            let Some(mut run) = self.supply_runs.get(&patrol.npc).cloned() else {
                continue;
            };
            if run.rests_until > 0 {
                let mut standing = false;
                for &member in &patrol.members {
                    self.tick_work.supply_members_visited += 1;
                    let Some(state) = self.creatures.get(&member) else {
                        continue;
                    };
                    if !state.is_alive() {
                        continue;
                    }
                    // A member out of its fight leaves for the camp.
                    if matches!(state.ai, CreatureAi::Idle { .. }) {
                        self.retire_raider(member, now)?;
                    } else {
                        standing = true;
                    }
                }
                if now >= run.rests_until && !standing {
                    self.supply_runs.remove(&patrol.npc);
                }
                continue;
            }
            let lookout = self
                .npc_feet(patrol.npc)
                .ok_or_else(|| ZoneError::new("a supply patrol's lookout has no placement"))?;
            let mut in_sight = false;
            for &member in &patrol.members {
                self.tick_work.supply_members_visited += 1;
                if !self
                    .creatures
                    .get(&member)
                    .is_some_and(crate::creature::CreatureState::is_alive)
                {
                    continue;
                }
                let at = self.unit_position(EntityRef::Creature(member))?;
                in_sight |= xz_distance([at[0], at[2]], lookout) <= i64::from(patrol.sight);
            }
            if in_sight {
                self.credit_near(
                    lookout,
                    patrol.post,
                    QuestObjective::Sight { npc: patrol.npc },
                )?;
            }
            // A carrier's fall ended the run as it happened ([`Self::supply_member_fell`]); one
            // out of the world otherwise ends it with nothing to credit.
            let carrier_stands = self
                .creatures
                .get(&patrol.carrier())
                .is_some_and(crate::creature::CreatureState::is_alive);
            if !carrier_stands {
                run.rests_until = now.saturating_add(u64::from(REST_TICKS));
            }
            let arrived = run
                .marches
                .first()
                .is_some_and(|march| usize::from(march.waypoint) >= patrol.route.len());
            let overdue = now.saturating_sub(run.started) >= u64::from(MARCH_TICKS);
            if run.rests_until == 0 && (arrived || overdue) {
                self.supply_arrived(patrol.npc);
                run.rests_until = now.saturating_add(u64::from(REST_TICKS));
            }
            self.supply_runs.insert(patrol.npc, run);
        }
        Ok(())
    }

    /// `creature` died at `at` (#1019): the carrier of a marching run is intercepted there, which
    /// completes the ambush for every player near its body with it ready and ends the run.
    pub(crate) fn supply_member_fell(
        &mut self,
        creature: CreatureId,
        at: [i32; 2],
        now: u64,
    ) -> Result<(), ZoneError> {
        let Some((patrol, _)) = self.marching(creature) else {
            return Ok(());
        };
        if patrol.carrier() != creature {
            return Ok(());
        }
        let npc = patrol.npc;
        self.credit_near(at, INTERCEPT_RADIUS_UNITS, QuestObjective::Ambush { npc })?;
        if let Some(run) = self.supply_runs.get_mut(&npc) {
            run.rests_until = now.saturating_add(u64::from(REST_TICKS));
        }
        Ok(())
    }

    /// Completes `objective` for each player within `radius` of `at` that has it ready.
    fn credit_near(
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

    /// The patrol of lookout `npc` reached its camp (#1019): each player with a sight or ambush of
    /// it ready hears so.
    fn supply_arrived(&mut self, npc: NpcId) {
        let waiting: Vec<_> = self
            .players
            .keys()
            .copied()
            .filter(|&player| {
                self.ready_objective(player, QuestObjective::Sight { npc })
                    || self.ready_objective(player, QuestObjective::Ambush { npc })
            })
            .collect();
        for player in waiting {
            if let Some(state) = self.players.get_mut(&player) {
                push_event(
                    &mut state.events,
                    ZoneEvent::Error {
                        code: ErrorCode::PatrolArrived,
                        target: Some(EntityRef::Npc(npc)),
                    },
                );
            }
        }
    }

    /// Canonical runs in lookout order.
    pub(crate) fn canonical_supply_runs(&self) -> Vec<SupplyRun> {
        self.supply_runs.values().cloned().collect()
    }

    /// Restores the runs from a snapshot taken at `tick`.
    pub(crate) fn restore_supply_runs(
        &mut self,
        runs: &[SupplyRun],
        tick: u64,
    ) -> Result<(), ZoneError> {
        let content = Arc::clone(&self.content);
        let valid = runs.windows(2).all(|pair| pair[0].npc < pair[1].npc)
            && runs.iter().all(|run| {
                content.supply_patrol(run.npc).is_some_and(|patrol| {
                    run.marches.len() == patrol.members.len()
                        && run.marches.iter().all(|march| {
                            usize::from(march.waypoint) <= patrol.route.len()
                                && march
                                    .anchor
                                    .iter()
                                    .all(|value| value.abs() <= crate::MAX_CONTENT_COORDINATE_UNITS)
                        })
                        && run.started <= tick
                        && run.rests_until <= tick.saturating_add(u64::from(REST_TICKS))
                })
            });
        if !valid {
            return Err(ZoneError::new("snapshot supply runs are invalid"));
        }
        self.supply_runs = runs.iter().map(|run| (run.npc, run.clone())).collect();
        Ok(())
    }
}
