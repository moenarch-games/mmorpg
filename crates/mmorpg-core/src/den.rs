//! Wildlife dens and roaming packs (#912, owner choice 2026-11-17: hybrid ecosystem).
//!
//! A den is an authored home range: the den itself and a few outing spots around it. Its pack
//! rests at the den, then goes out to one outing spot, back to the den, out to the next, and so
//! on, one leg every [`DEN_LEG_TICKS`]. The leg is a pure function of the zone tick, so replay
//! and recovery store nothing: a pack wolf's home (where it wanders, leashes and evades to) is
//! its slot beside the current leg's anchor.
//!
//! - **Roam**: an idle pack wolf beyond its wander radius from its slot walks there.
//! - **Hunt**: on an outing, the pack runs at the nearest of the den's prey animals within
//!   [`HUNT_RADIUS_UNITS`] of the outing spot, if that animal is idle. The prey flees from any
//!   pack wolf within [`FLEE_RADIUS_UNITS`] at the same speed, so the hunt is a chase that ends
//!   once the prey leaves the hunting ground; then each side returns to its own range. Nobody
//!   takes damage: this is zone-local behaviour, not a predator/prey simulation.
//! - **Fight**: players are handled by the existing rules: aggro, family assist (the pack moves
//!   together, so it answers together), leash and evade back to the pack's range.
//! - **Off-screen**: a sleeping pack wolf (#814) does no per-tick work. At each leg change one
//!   that no player could see is placed at its new slot, so a pack keeps its schedule unseen.

use std::sync::Arc;

use crate::ai::{approach, offset, steer};
use crate::entity::body_id;
use crate::physics::Vec3i;
use crate::reach::xz_distance_squared;
use crate::snapshot::CreatureAi;
use crate::trig::yaw_from_vector;
use crate::zone::physics_error;
use crate::{CreatureId, EntityRef, ZoneError, ZoneSimulation};

/// Ticks per leg of a pack's round: 40 s at the den, 40 s out, and so on.
pub const DEN_LEG_TICKS: u64 = crate::long_ticks_from_ms(40_000) as u64;
/// A prey animal this close to the outing spot is hunted.
pub const HUNT_RADIUS_UNITS: i32 = 2_000;
/// A hunted animal flees from any pack wolf this close.
pub const FLEE_RADIUS_UNITS: i32 = 1_000;
/// Hunting wolves stop this close to their prey.
const CLOSE_UNITS: i32 = 150;
/// Pack members' places around an anchor, in pack order (repeating past the fourth).
const SLOTS: [[i32; 2]; 4] = [[0, 0], [180, 120], [-180, 120], [0, -200]];

/// One authored den and its pack's home range (#912).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WildlifeDen {
    pub name: String,
    /// Where the pack rests between outings.
    pub den: [i32; 2],
    /// The pack, in slot order; each a content spawn of at most one den.
    pub pack: Vec<CreatureId>,
    /// Outing spots, visited in order with a rest at the den between each.
    pub outings: Vec<[i32; 2]>,
    /// Animals the pack hunts when one grazes near an outing spot.
    pub prey: Vec<CreatureId>,
}

impl WildlifeDen {
    /// Legs in one round: the den before every outing.
    fn legs(&self) -> u64 {
        2 * self.outings.len().max(1) as u64
    }

    /// The pack's anchor at `tick`, and whether it is on an outing.
    #[must_use]
    pub fn anchor(&self, tick: u64) -> ([i32; 2], bool) {
        let leg = (tick / DEN_LEG_TICKS) % self.legs();
        let outing = usize::try_from(leg / 2).ok();
        match outing.and_then(|index| self.outings.get(index)) {
            Some(&spot) if leg % 2 == 1 => (spot, true),
            _ => (self.den, false),
        }
    }

    /// Every anchor the pack ever uses: the den, then the outing spots.
    pub fn anchors(&self) -> impl Iterator<Item = [i32; 2]> + '_ {
        std::iter::once(self.den).chain(self.outings.iter().copied())
    }

    /// `member`'s slot offset, if it belongs to the pack.
    #[must_use]
    pub fn slot(&self, member: CreatureId) -> Option<[i32; 2]> {
        let index = self.pack.iter().position(|&id| id == member)?;
        Some(SLOTS[index % SLOTS.len()])
    }
}

fn at(anchor: [i32; 2], slot: [i32; 2]) -> [i32; 2] {
    [anchor[0] + slot[0], anchor[1] + slot[1]]
}

impl ZoneSimulation {
    fn den_of(&self, id: CreatureId) -> Option<(&WildlifeDen, [i32; 2])> {
        self.content
            .dens()
            .iter()
            .find_map(|den| den.slot(id).map(|slot| (den, slot)))
    }

    /// A pack wolf's home at this tick: its slot beside the pack's anchor.
    pub(crate) fn den_home(&self, id: CreatureId) -> Option<[i32; 2]> {
        let (den, slot) = self.den_of(id)?;
        Some(at(den.anchor(self.tick).0, slot))
    }

    fn idle_alive(&self, id: CreatureId) -> Option<[i32; 2]> {
        let creature = self.creatures.get(&id)?;
        if !creature.is_alive() || creature.raid.is_some() {
            return None;
        }
        if !matches!(creature.ai, CreatureAi::Idle { .. }) {
            return None;
        }
        let position = self
            .world
            .body(body_id(EntityRef::Creature(id)))?
            .position();
        Some([position.x, position.z])
    }

    /// The prey `den`'s pack hunts now: on an outing, its nearest idle prey animal within the
    /// hunting ground, ties to the lowest ID.
    fn quarry(&self, den: &WildlifeDen) -> Option<(CreatureId, [i32; 2])> {
        let (spot, outing) = den.anchor(self.tick);
        if !outing {
            return None;
        }
        den.prey
            .iter()
            .filter_map(|&prey| self.idle_alive(prey).map(|at| (prey, at)))
            .filter(|&(_, at)| crate::reach::within_xz(spot, at, HUNT_RADIUS_UNITS))
            .min_by_key(|&(prey, at)| (xz_distance_squared(spot, at), prey))
    }

    /// Step 2 for an idle pack wolf or hunted animal (#912): its velocity and facing, held for
    /// `elapsed` ticks. `None` leaves it to wander.
    pub(crate) fn den_step(
        &self,
        id: CreatureId,
        here: [i32; 2],
        wander_radius: i32,
        elapsed: u16,
    ) -> Option<crate::raid::Steering> {
        if let Some((den, slot)) = self.den_of(id) {
            self.idle_alive(id)?;
            let (to, stop, speed) = match self.quarry(den) {
                Some((_, prey)) => (
                    prey,
                    CLOSE_UNITS,
                    crate::CREATURE_RUN_SPEED_SUBUNITS_PER_TICK,
                ),
                None => {
                    let home = at(den.anchor(self.tick).0, slot);
                    if crate::reach::within_xz(here, home, wander_radius) {
                        return None;
                    }
                    (home, 0, crate::CREATURE_WALK_SPEED_SUBUNITS_PER_TICK)
                }
            };
            let (dx, dz) = offset(here, to);
            return Some((
                approach(dx, dz, stop, speed, elapsed),
                yaw_from_vector(dx, dz),
            ));
        }
        // A hunted animal runs straight away from the nearest wolf of the pack after it.
        let den = self
            .content
            .dens()
            .iter()
            .find(|den| self.quarry(den).is_some_and(|(prey, _)| prey == id))?;
        let chaser = den
            .pack
            .iter()
            .filter_map(|&wolf| self.idle_alive(wolf))
            .filter(|&at| crate::reach::within_xz(here, at, FLEE_RADIUS_UNITS))
            .min_by_key(|&at| xz_distance_squared(here, at))?;
        let (dx, dz) = offset(chaser, here);
        let (dx, dz) = if (dx, dz) == (0, 0) { (0, 1) } else { (dx, dz) };
        Some((
            steer(dx, dz, crate::CREATURE_RUN_SPEED_SUBUNITS_PER_TICK),
            yaw_from_vector(dx, dz),
        ))
    }

    /// Off-screen continuation (#912): at each leg change, every sleeping idle pack wolf whose
    /// old and new places no player is near is placed at its new slot.
    pub(crate) fn continue_dens_offscreen(&mut self) -> Result<(), ZoneError> {
        if !self.tick.is_multiple_of(DEN_LEG_TICKS) || self.content.dens().is_empty() {
            return Ok(());
        }
        let content = Arc::clone(&self.content);
        for den in content.dens() {
            let anchor = den.anchor(self.tick).0;
            for (&id, &slot) in den.pack.iter().zip(SLOTS.iter().cycle()) {
                let Some(here) = self.idle_alive(id) else {
                    continue;
                };
                let to = at(anchor, slot);
                let seen = [here, to].into_iter().any(|point| {
                    self.interest
                        .player_near(point[0], point[1], crate::CREATURE_WAKE_RADIUS_UNITS)
                });
                if seen {
                    continue;
                }
                let (_, template) = Self::creature_content(&content, id)?;
                let body = body_id(EntityRef::Creature(id));
                let y = self.stand_y(to[0], to[1], template.half_extents);
                self.world
                    .set_position(body, Vec3i::new(to[0], y, to[1]))
                    .map_err(physics_error)?;
                self.world
                    .set_velocity(body, Vec3i::ZERO)
                    .map_err(physics_error)?;
                if let Some(creature) = self.creatures.get_mut(&id)
                    && let CreatureAi::Idle { destination, .. } = &mut creature.ai
                {
                    *destination = None;
                }
            }
        }
        Ok(())
    }
}

/// Whether `to` is a wander point of `member` around any of its den's anchors, for recovery.
pub(crate) fn is_pack_wander_point(
    content: &crate::ZoneContent,
    member: CreatureId,
    to: [i32; 2],
    radius: i32,
) -> bool {
    content.dens().iter().any(|den| {
        den.slot(member).is_some_and(|slot| {
            den.anchors()
                .any(|anchor| crate::ai::is_wander_point(at(anchor, slot), to, radius))
        })
    })
}
