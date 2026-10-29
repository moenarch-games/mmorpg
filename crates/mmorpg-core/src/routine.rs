//! The zone clock and deterministic NPC day/night routines (#158, Atlas 5.1.45/5.1.47).
//!
//! Time of day is a pure function of the zone tick: a day lasts [`DAY_TICKS`] (two real hours)
//! and a zone starts at [`ZONE_START_TIME`] in the morning. Projections carry it so
//! every client's sky follows the zone. A suspended zone's tick, and so its time, freezes.
//!
//! An [`NpcRoutine`] walks one NPC along an authored path from its content position (work) to
//! the path's end (home) at [`DUSK`], and back at [`DAWN`]. Its placement is also a pure
//! function of the tick, so it needs no canonical state: recovery and replay place the NPC
//! exactly. While walking it opens a closed door on its path (`with_routines`' `door`), which
//! then stays open like any door a player left. Vendors listed as closed at night refuse trade
//! from dusk to dawn.

use crate::{DoorId, EntityRef, NpcId, ZoneError, ZoneSimulation, trig};

/// Ticks in one zone day: two real hours.
pub const DAY_TICKS: u32 = crate::long_ticks_from_ms(2 * 60 * 60 * 1_000);
/// One zone hour in ticks.
pub const HOUR_TICKS: u32 = DAY_TICKS / 24;
/// A new zone's tick 0 is 08:00.
pub const ZONE_START_TIME: u32 = 8 * HOUR_TICKS;
/// Night starts at 20:00 ...
pub const DUSK: u32 = 20 * HOUR_TICKS;
/// ... and ends at 06:00.
pub const DAWN: u32 = 6 * HOUR_TICKS;
/// A walking NPC covers this many units per tick (2.4 m/s).
pub const NPC_WALK_UNITS_PER_TICK: i64 = crate::units_per_tick(240) as i64;
/// Waypoints a routine path may have.
pub const MAX_ROUTINE_WAYPOINTS: usize = 16;

/// Ticks since midnight at `tick`.
#[must_use]
pub const fn time_of_day(tick: u64) -> u32 {
    ((tick + ZONE_START_TIME as u64) % DAY_TICKS as u64) as u32
}

/// Whether `time` (ticks since midnight) is between dusk and dawn.
#[must_use]
pub const fn is_night(time: u32) -> bool {
    time >= DUSK || time < DAWN
}

/// One NPC's work/home routine. `path` runs from the NPC's content position (exclusive) to its
/// home; `home_facing` is its facing there.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NpcRoutine {
    pub npc: NpcId,
    pub path: Vec<[i32; 2]>,
    pub home_facing: u16,
    /// A door on the path the NPC opens when it comes within reach of it closed.
    pub door: Option<DoorId>,
}

/// Where a routine NPC is at one tick.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NpcPlacement {
    pub position: [i32; 2],
    pub facing: u16,
    /// Units per tick while walking, otherwise zero.
    pub velocity: [i8; 2],
}

/// `(from, to, length)` of each leg, starting at `start`.
pub(crate) fn legs(start: [i32; 2], path: &[[i32; 2]]) -> Vec<([i32; 2], [i32; 2], i64)> {
    let mut from = start;
    path.iter()
        .map(|&to| {
            let length = i64::try_from(trig::xz_length(to[0] - from[0], to[1] - from[1]))
                .unwrap_or(i64::MAX);
            let leg = (from, to, length);
            from = to;
            leg
        })
        .collect()
}

/// The point `distance` units along the legs, walking forward or backward, with its heading.
pub(crate) fn along(
    legs: &[([i32; 2], [i32; 2], i64)],
    distance: i64,
    forward: bool,
) -> NpcPlacement {
    let total: i64 = legs.iter().map(|leg| leg.2).sum();
    let mut remaining = if forward { distance } else { total - distance };
    for &(from, to, length) in legs {
        if remaining <= length && length > 0 {
            let lerp = |a: i32, b: i32| {
                i32::try_from(i64::from(a) + (i64::from(b) - i64::from(a)) * remaining / length)
                    .unwrap_or(a)
            };
            let (dx, dz) = if forward {
                (to[0] - from[0], to[1] - from[1])
            } else {
                (from[0] - to[0], from[1] - to[1])
            };
            let speed = |delta: i32| {
                i8::try_from(i64::from(delta) * NPC_WALK_UNITS_PER_TICK / length).unwrap_or(0)
            };
            return NpcPlacement {
                position: [lerp(from[0], to[0]), lerp(from[1], to[1])],
                facing: trig::yaw_from_vector(dx, dz).unwrap_or(0),
                velocity: [speed(dx), speed(dz)],
            };
        }
        remaining -= length;
    }
    let end = legs.last().map_or([0, 0], |leg| leg.1);
    NpcPlacement {
        position: end,
        facing: 0,
        velocity: [0, 0],
    }
}

impl NpcRoutine {
    /// The placement at `time` (ticks since midnight) of an NPC working at `work`.
    #[must_use]
    pub fn placement(&self, work: [i32; 2], work_facing: u16, time: u32) -> NpcPlacement {
        let legs = legs(work, &self.path);
        let total: i64 = legs.iter().map(|leg| leg.2).sum();
        let since = |mark: u32| i64::from((time + DAY_TICKS - mark) % DAY_TICKS);
        let out = since(DUSK) * NPC_WALK_UNITS_PER_TICK;
        let back = since(DAWN) * NPC_WALK_UNITS_PER_TICK;
        if is_night(time) {
            if out < total {
                return along(&legs, out, true);
            }
            return NpcPlacement {
                position: legs.last().map_or(work, |leg| leg.1),
                facing: self.home_facing,
                velocity: [0, 0],
            };
        }
        if back < total {
            return along(&legs, back, false);
        }
        NpcPlacement {
            position: work,
            facing: work_facing,
            velocity: [0, 0],
        }
    }
}

impl ZoneSimulation {
    /// Where `npc` is now: its routine placement, or its content position.
    #[must_use]
    pub fn npc_placement(&self, npc: NpcId) -> Option<NpcPlacement> {
        let definition = self.content.npc(npc)?;
        Some(match self.content.routine(npc) {
            // A knock woke it (#158): it walks to its door and back to bed.
            Some(routine)
                if let Some(placement) = self
                    .wakes
                    .get(&npc)
                    .and_then(|wake| self.wake_placement(wake, *routine.path.last()?)) =>
            {
                placement
            }
            Some(routine) => routine.placement(
                definition.position,
                definition.facing,
                time_of_day(self.tick),
            ),
            None => NpcPlacement {
                position: definition.position,
                facing: definition.facing,
                velocity: [0, 0],
            },
        })
    }

    /// `npc`'s feet XZ now, in units.
    pub(crate) fn npc_feet(&self, npc: NpcId) -> Option<[i32; 2]> {
        self.npc_placement(npc).map(|placement| placement.position)
    }

    /// Whether the vendor `npc` trades now.
    pub(crate) fn vendor_open(&self, npc: NpcId) -> bool {
        !(self.content.closed_at_night(npc) && is_night(time_of_day(self.tick)))
    }

    /// Moves every routine NPC's body to its placement at the current tick and, with
    /// `open_doors`, opens a closed door it has come within reach of.
    pub(crate) fn place_routine_npcs(&mut self, open_doors: bool) -> Result<(), ZoneError> {
        let routines = self.content.routines().to_vec();
        for routine in routines {
            let Some(placement) = self.npc_placement(routine.npc) else {
                continue;
            };
            let entity = EntityRef::Npc(routine.npc);
            let [x, z] = placement.position;
            let unmoved = self
                .world
                .body(crate::entity::body_id(entity))
                .is_some_and(|body| {
                    let position = body.position();
                    position.x == x && position.z == z
                });
            if unmoved {
                continue;
            }
            self.world
                .replace_body(crate::physics::RigidBody::fixed(
                    crate::entity::body_id(entity),
                    crate::zone::vector([
                        x,
                        self.content.definition().ground_at(x, z)
                            + crate::PLAYER_HALF_EXTENTS_UNITS[1],
                        z,
                    ]),
                    crate::zone::PLAYER_HALF_EXTENTS,
                ))
                .map_err(|error| ZoneError::new(format!("npc body update failed: {error}")))?;
            self.interest.move_if_needed(entity, x, z);
            // A woken resident's door follows its wake (`advance_private_homes`).
            if !open_doors || placement.velocity == [0, 0] || self.wakes.contains_key(&routine.npc)
            {
                continue;
            }
            if let Some(door) = routine.door.and_then(|id| self.content.door(id).copied())
                && !self.doors.get(&door.id).is_some_and(|state| state.open)
                && door.within_reach([x, 0, z])
            {
                self.try_open_for_npc(&door)?;
            }
        }
        Ok(())
    }
}
