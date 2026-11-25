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
//! from dusk to dawn. By day an NPC may run [`Errand`]s (#819): to the well, the inn's table, the
//! fields or the market at set hours, staying a while and walking back to work.

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
    /// The seat at the path's end, where the NPC sits while it is home (#619).
    pub home_seat: Option<crate::SeatId>,
    /// The seat at its content position, where it sits while it is at work (#619).
    pub work_seat: Option<crate::SeatId>,
    /// Its day's errands from work and back (#819), in time order.
    pub errands: Vec<Errand>,
}

/// One daytime errand (#819): at `leave` (ticks since midnight) the NPC walks `path` from its
/// work post, stays `stay` ticks at the path's end facing `facing` (sitting on `seat`, if any,
/// whose stand is that end), then walks the path back to work.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Errand {
    pub leave: u32,
    pub path: Vec<[i32; 2]>,
    pub stay: u32,
    pub facing: u16,
    pub seat: Option<crate::SeatId>,
}

/// Ticks a walk of `length` units takes, rounded up.
pub(crate) fn walk_ticks(length: i64) -> i64 {
    (length + NPC_WALK_UNITS_PER_TICK - 1) / NPC_WALK_UNITS_PER_TICK
}

impl Errand {
    /// The ticks from leaving work to being back: out, the stay and home.
    #[must_use]
    pub fn span(&self, work: [i32; 2]) -> u32 {
        let total: i64 = legs(work, &self.path).iter().map(|leg| leg.2).sum();
        u32::try_from(2 * walk_ticks(total) + i64::from(self.stay)).unwrap_or(u32::MAX)
    }

    /// Where the NPC is `since` ticks after leaving work, if the errand is still under way.
    fn placement(&self, work: [i32; 2], since: u32) -> Option<NpcPlacement> {
        let legs = legs(work, &self.path);
        let total: i64 = legs.iter().map(|leg| leg.2).sum();
        let walk = walk_ticks(total);
        let since = i64::from(since);
        if since < walk {
            return Some(along(&legs, since * NPC_WALK_UNITS_PER_TICK, true));
        }
        if since < walk + i64::from(self.stay) {
            return Some(NpcPlacement {
                position: legs.last().map_or(work, |leg| leg.1),
                facing: self.facing,
                velocity: [0, 0],
            });
        }
        let back = since - walk - i64::from(self.stay);
        (back < walk).then(|| along(&legs, back * NPC_WALK_UNITS_PER_TICK, false))
    }
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
        if let Some(placement) = self.errands.iter().find_map(|errand| {
            time.checked_sub(errand.leave)
                .and_then(|since| errand.placement(work, since))
        }) {
            return placement;
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
        // An escorted NPC (#1019) is where its escort led it.
        if let Some(escort) = self.escorts.get(&npc) {
            return Some(NpcPlacement {
                position: escort.position,
                facing: escort.facing,
                velocity: escort.velocity,
            });
        }
        // A stray (#1019) is where it was driven.
        if let Some(placement) = self.stray_placement(npc) {
            return Some(placement);
        }
        // A guard injured in a lost raid lies at the farm's shelter (#531).
        if let Some(position) = self.injured_at(npc) {
            return Some(NpcPlacement {
                position,
                facing: definition.facing,
                velocity: [0, 0],
            });
        }
        // A hunter on its outing (#991).
        if let Some((placement, _)) = self.hunt_placement(npc) {
            return Some(placement);
        }
        // A merchant on its cart journey (#992).
        if let Some((placement, ..)) = self.cart_placement(npc) {
            return Some(placement);
        }
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

    /// Moves every routine NPC's and raid aid guard's body to its placement at the current tick
    /// and, with `open_doors`, opens a closed door a routine NPC has come within reach of.
    pub(crate) fn place_npcs(&mut self, open_doors: bool) -> Result<(), ZoneError> {
        let mut movers: Vec<(NpcId, Option<DoorId>)> = self
            .content
            .routines()
            .iter()
            .map(|routine| (routine.npc, routine.door))
            .collect();
        for site in self.content.raids() {
            movers.extend(site.aid_guards().map(|npc| (npc, None)));
        }
        // Escorted and confronted NPCs (#1019) move or leave the world.
        movers.extend(self.content.quest_npcs().iter().map(|&npc| (npc, None)));
        for (npc, routine_door) in movers {
            let Some(placement) = self.npc_placement(npc) else {
                continue;
            };
            let entity = EntityRef::Npc(npc);
            let body = crate::entity::body_id(entity);
            // A confronted NPC is gone while its foe stands (#1019): no body to bump into.
            if self.npc_hidden(npc) {
                self.world.remove_body(body);
                continue;
            }
            let [x, z] = placement.position;
            let present = self.world.body(body).is_some();
            let unmoved = self.world.body(body).is_some_and(|body| {
                let position = body.position();
                position.x == x && position.z == z
            });
            if unmoved {
                continue;
            }
            let moved = crate::physics::RigidBody::fixed(
                body,
                crate::zone::vector([
                    x,
                    self.content.definition().ground_at(x, z) + crate::PLAYER_HALF_EXTENTS_UNITS[1],
                    z,
                ]),
                crate::zone::PLAYER_HALF_EXTENTS,
            );
            if present {
                self.world.replace_body(moved)
            } else {
                self.world.add_body(moved)
            }
            .map_err(|error| ZoneError::new(format!("npc body update failed: {error}")))?;
            self.interest.move_if_needed(entity, x, z);
            // A woken resident's door follows its wake (`advance_private_homes`).
            if !open_doors || placement.velocity == [0, 0] || self.wakes.contains_key(&npc) {
                continue;
            }
            if let Some(door) = routine_door.and_then(|id| self.content.door(id).copied())
                && !self.doors.get(&door.id).is_some_and(|state| state.open)
                && door.within_reach([x, 0, z])
            {
                self.try_open_for_npc(&door)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DUSK, Errand, HOUR_TICKS, NPC_WALK_UNITS_PER_TICK, NpcRoutine, walk_ticks};
    use crate::NpcId;

    #[test]
    fn an_errand_walks_out_stays_and_walks_back_by_day() {
        let leave = 10 * HOUR_TICKS;
        let routine = NpcRoutine {
            npc: NpcId::new(1),
            path: vec![[0, 5_000]],
            home_facing: 0,
            door: None,
            home_seat: None,
            work_seat: None,
            errands: vec![Errand {
                leave,
                path: vec![[2_400, 0]],
                stay: HOUR_TICKS,
                facing: 16_384,
                seat: None,
            }],
        };
        let walk = u32::try_from(walk_ticks(2_400)).unwrap();
        let at = |time: u32| routine.placement([0, 0], 0, time);
        // At work before it leaves, on the way out, at the errand, on the way back, at work.
        assert_eq!(at(leave - 1).position, [0, 0]);
        let out = at(leave + walk / 2);
        assert!(out.position[0] > 0 && out.position[0] < 2_400 && out.velocity[0] > 0);
        assert_eq!(at(leave + walk + 10).position, [2_400, 0]);
        assert_eq!(at(leave + walk + 10).facing, 16_384);
        let back = at(leave + walk + HOUR_TICKS + walk / 2);
        assert!(back.position[0] > 0 && back.velocity[0] < 0);
        assert_eq!(at(leave + 2 * walk + HOUR_TICKS).position, [0, 0]);
        // It still walks home at dusk.
        assert!(at(DUSK + 1).position[1] > 0);
        assert_eq!(
            routine.errands[0].span([0, 0]),
            2 * walk + HOUR_TICKS,
            "{NPC_WALK_UNITS_PER_TICK}"
        );
    }
}
