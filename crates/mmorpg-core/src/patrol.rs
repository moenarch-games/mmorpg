//! Coordinated Redbrand patrols (#911): two to four creatures on the existing templates, attacks,
//! navigation, leash, aggro and family assist, each with a small role that shapes a fight into a
//! readable spatial challenge instead of a pile.
//!
//! - **Bruiser** holds the front: it chases straight at its target, as every creature does.
//! - **Skirmisher** comes in from the side: it closes on the point [`FLANK_YAW`] off the bearing
//!   from the target to its patrol's bruiser, on whichever side it already stands. The two melee
//!   opponents then stand inside one 100° swing arc, so a player who faces between them strikes
//!   both with one directed slash (#909), while one who lets them split takes blows from a side
//!   lane. After each swing it falls back out of reach for [`RECOVERY_PERCENT`] of its swing
//!   interval, a visible recovery that opens a window and keeps its blows from stacking on the
//!   bruiser's.
//! - **Marksman** keeps its distance: inside [`MARKSMAN_MIN_UNITS`] it backs away at walking pace,
//!   so a player who closes in catches it; beyond [`MARKSMAN_MAX_UNITS`] or without a line of
//!   sight it closes in; in between it holds and shoots its crossbow's wind-up bolt.
//!
//! Every role step derives from canonical positions, swing timers and threat each tick, so a
//! player's repositioning is answered at once and replay and recovery need no new state. Routes
//! around obstacles, casting and evading keep their ordinary behaviour.

use crate::ai::{distance_squared, offset, steer};
use crate::snapshot::CreatureAi;
use crate::{CreatureId, EntityRef, ZoneError, ZoneSimulation, trig};

/// A patrol member's part in its fight.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum PatrolRole {
    /// Holds the front and chases straight in.
    Bruiser,
    /// Flanks the bruiser's target and falls back after each swing.
    Skirmisher,
    /// Shoots from a distance and backs away from a closing player.
    Marksman,
}

impl PatrolRole {
    /// Stable code for the content revision.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Self::Bruiser => 0,
            Self::Skirmisher => 1,
            Self::Marksman => 2,
        }
    }
}

/// One creature spawn's place in an authored patrol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PatrolMember {
    pub creature: CreatureId,
    /// The patrol it belongs to; a skirmisher flanks its own patrol's bruiser.
    pub patrol: u16,
    pub role: PatrolRole,
}

/// A patrol has two to four members.
pub const PATROL_SIZE: [usize; 2] = [2, 4];
/// The skirmisher's approach angle off the bruiser's bearing: 60°, so both stand within 30° of a
/// player facing between them, well inside a 100° swing arc.
pub const FLANK_YAW: u16 = 10_923;
/// The share of its swing interval a skirmisher spends out of reach after a swing.
pub const RECOVERY_PERCENT: u16 = 40;
/// How far beyond its reach a recovering skirmisher stands: 1.5 m.
pub const RECOVERY_STANDOFF_UNITS: i32 = 150;
/// A marksman backs away from a target closer than 5 m …
pub const MARKSMAN_MIN_UNITS: i32 = 500;
/// … and closes on one beyond 15 m, inside its 20 m crossbow range.
pub const MARKSMAN_MAX_UNITS: i32 = 1_500;

/// Whether a skirmisher that swung `swing_ticks` ago, with `timer` ticks left, is recovering.
#[must_use]
pub const fn recovering(timer: u16, swing_ticks: u16) -> bool {
    let recovery = (swing_ticks as u32 * RECOVERY_PERCENT as u32 / 100) as u16;
    timer > swing_ticks.saturating_sub(recovery)
}

/// Where a skirmisher stands against `target`: `standoff` units from it, [`FLANK_YAW`] off the
/// bearing to `anchor`, on the side of that bearing it stands now.
#[must_use]
pub fn flank_point(target: [i32; 2], anchor: [i32; 2], here: [i32; 2], standoff: i32) -> [i32; 2] {
    let bearing = |to: [i32; 2]| {
        let (dx, dz) = offset(target, to);
        trig::yaw_from_vector(dx, dz)
    };
    let Some(front) = bearing(anchor) else {
        return here;
    };
    // The signed turn from the bruiser's bearing to its own picks the side.
    let side = bearing(here).map_or(0, |own| own.wrapping_sub(front).cast_signed());
    let yaw = if side >= 0 {
        front.wrapping_add(FLANK_YAW)
    } else {
        front.wrapping_sub(FLANK_YAW)
    };
    let (x, z) = trig::direction(yaw);
    let scale = |unit| trig::checked_scale(standoff, unit).unwrap_or(0);
    [
        target[0].saturating_add(scale(x)),
        target[1].saturating_add(scale(z)),
    ]
}

/// A marksman's step against a target `distance` away: `Some(true)` backs away, `Some(false)`
/// holds, `None` closes in as an ordinary chase.
#[must_use]
pub const fn marksman_step(distance_squared: i64, sight: bool) -> Option<bool> {
    let min = MARKSMAN_MIN_UNITS as i64;
    let max = MARKSMAN_MAX_UNITS as i64;
    if distance_squared < min * min {
        Some(true)
    } else if distance_squared > max * max || !sight {
        None
    } else {
        Some(false)
    }
}

/// A creature's step this tick: its velocity and its facing, if it turns.
type Step = ([i32; 2], Option<u16>);

impl ZoneSimulation {
    /// Creature `id`'s patrol role step against its chase target (#911): its velocity and
    /// facing, or `None` when its role leaves the ordinary chase in charge.
    pub(crate) fn patrol_step(
        &self,
        id: CreatureId,
        swing_ticks: u16,
        here: [i32; 2],
        (target, to): (EntityRef, [i32; 2]),
    ) -> Result<Option<Step>, ZoneError> {
        let Some(member) = self.content.patrol_member(id) else {
            return Ok(None);
        };
        let (dx, dz) = offset(here, to);
        let facing = trig::yaw_from_vector(dx, dz);
        match member.role {
            PatrolRole::Bruiser => Ok(None),
            PatrolRole::Marksman => {
                let range = distance_squared(here, to);
                let sight = range <= i64::from(MARKSMAN_MAX_UNITS).pow(2)
                    && self.line_of_sight(EntityRef::Creature(id), target)?;
                Ok(marksman_step(range, sight).map(|back| {
                    let velocity = if back {
                        steer(-dx, -dz, crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK)
                    } else {
                        [0, 0]
                    };
                    (velocity, facing)
                }))
            }
            PatrolRole::Skirmisher => {
                let Some(anchor) = self.patrol_bruiser(member.patrol, target)? else {
                    return Ok(None);
                };
                let timer = self
                    .creatures
                    .get(&id)
                    .map_or(0, |creature| creature.swing_timer);
                let reach = crate::ai::creature_reach(target);
                let standoff = if recovering(timer, swing_ticks) {
                    reach + RECOVERY_STANDOFF_UNITS
                } else {
                    reach - 2 * crate::ARRIVAL_RADIUS_UNITS
                };
                let point = flank_point(to, anchor, here, standoff);
                let velocity = if distance_squared(here, point)
                    <= i64::from(crate::ARRIVAL_RADIUS_UNITS).pow(2)
                {
                    [0, 0]
                } else {
                    let (px, pz) = offset(here, point);
                    steer(px, pz, crate::CREATURE_RUN_SPEED_SUBUNITS_PER_TICK)
                };
                Ok(Some((velocity, facing)))
            }
        }
    }

    /// Where `patrol`'s living bruiser fighting `target` stands, if one does.
    fn patrol_bruiser(
        &self,
        patrol: u16,
        target: EntityRef,
    ) -> Result<Option<[i32; 2]>, ZoneError> {
        for member in self.content.patrols() {
            if member.patrol != patrol || member.role != PatrolRole::Bruiser {
                continue;
            }
            let Some(creature) = self.creatures.get(&member.creature) else {
                continue;
            };
            if creature.ai != CreatureAi::Engaged || creature.top_threat() != Some(target) {
                continue;
            }
            if let Some([x, _, z]) = self.creature_position(member.creature, creature)? {
                return Ok(Some([x, z]));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skirmisher_flanks_on_its_own_side_of_the_bruiser() {
        // The bruiser stands south (+Z) of the target; a skirmisher east of that line flanks
        // 60° toward the east, one west of it toward the west.
        let target = [0, 0];
        let anchor = [0, 1_000];
        let east = flank_point(target, anchor, [500, 500], 100);
        let west = flank_point(target, anchor, [-500, 500], 100);
        assert_eq!(east, [87, 50]);
        assert_eq!(west, [-87, 50]);
    }

    #[test]
    fn recovery_covers_the_start_of_the_swing_interval() {
        assert!(recovering(66, 66));
        assert!(recovering(41, 66));
        assert!(!recovering(40, 66));
        assert!(!recovering(0, 66));
    }

    #[test]
    fn a_marksman_keeps_its_band() {
        assert_eq!(marksman_step(400 * 400, true), Some(true));
        assert_eq!(marksman_step(1_000 * 1_000, true), Some(false));
        assert_eq!(marksman_step(1_000 * 1_000, false), None);
        assert_eq!(marksman_step(1_800 * 1_800, true), None);
    }
}
