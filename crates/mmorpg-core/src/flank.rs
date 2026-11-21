//! Flanking (#949, Atlas 5.2.i949): the Rogue's first opportunist slice.
//!
//! - **Facing turns.** An engaged creature standing its ground (in reach of its target, not
//!   casting, not stunned) turns toward its target at most [`CREATURE_TURN_PER_TICK`] a tick
//!   (120°/s). A moving creature still faces where it runs, and a casting one its target at once,
//!   so only a player who circles a creature inside its reach outpaces its turn.
//! - **The flank.** A Sinister Strike whose striker stands more than [`FLANK_FRONT_HALF_ARC`]
//!   (60°) off its creature victim's authoritative facing, measured from the victim's centre to
//!   the striker's, deals [`FLANK_DAMAGE_PERCENT`] of its damage.
//! - **The answer.** A flank that dealt damage turns a creature that is not stunned to face its
//!   striker at once, so the next strike from the same spot is an ordinary hit: the Rogue must
//!   reposition again, or strike a creature a Gouge holds, whose stun freezes its facing.
//! - **Wariness.** A flanked creature then guards its flank for [`FLANK_WARY_TICKS`] (3 s): while
//!   it stands its ground it turns [`WARY_TURN_PER_TICK`] a tick (480°/s), faster than a player
//!   circling inside its reach can sweep round it, so circling the same creature again does not
//!   buy the next flank every global cooldown. Flanks stay open through a Gouge, whose stun
//!   freezes its facing, through another creature that is not wary, and once the wariness ends.
//!
//! Everything derives from canonical facing and positions at resolution, so replay and recovery
//! need no new state; animations and the client camera play no part.

use crate::trig::{self, YAW_QUARTER_TURN};
use crate::{EntityRef, PlayerId, ZoneError, ZoneEvent, ZoneSimulation};

/// Half the frontal arc that takes ordinary hits: 60°, so both sides and the rear flank.
pub const FLANK_FRONT_HALF_ARC: u16 = 2 * (YAW_QUARTER_TURN / 3);
/// A flanking Sinister Strike's damage in percent of the ordinary strike.
pub const FLANK_DAMAGE_PERCENT: u32 = 150;
/// How far an engaged creature standing its ground turns a tick: 2°, 120° a second at 60 Hz.
pub const CREATURE_TURN_PER_TICK: u16 = YAW_QUARTER_TURN / 45;
/// How long a flanked creature guards its flank: 3 s.
pub const FLANK_WARY_TICKS: u16 = crate::ticks_from_ms(3_000);
/// How far a wary creature standing its ground turns a tick: 8°, 480° a second at 60 Hz.
pub const WARY_TURN_PER_TICK: u16 = 4 * CREATURE_TURN_PER_TICK;

/// The signed turn from `from` to `to`, the short way round.
#[allow(clippy::cast_possible_wrap)]
const fn yaw_gap(from: u16, to: u16) -> i16 {
    to.wrapping_sub(from) as i16
}

/// `current` turned toward `goal` by at most `limit` (below a half turn).
#[allow(clippy::cast_possible_wrap, clippy::cast_sign_loss)]
pub(crate) const fn turn_toward(current: u16, goal: u16, limit: u16) -> u16 {
    let gap = yaw_gap(current, goal);
    let limit = limit as i16;
    let step = if gap > limit {
        limit
    } else if gap < -limit {
        -limit
    } else {
        gap
    };
    current.wrapping_add(step as u16)
}

/// Whether a striker at `bearing` from a unit facing `facing` stands outside its frontal arc.
#[must_use]
pub const fn flanks(facing: u16, bearing: u16) -> bool {
    yaw_gap(facing, bearing).unsigned_abs() > FLANK_FRONT_HALF_ARC
}

impl ZoneSimulation {
    /// The bearing from creature `victim` to the player when the player stands on its flank.
    pub(crate) fn flank_bearing(
        &self,
        player_id: PlayerId,
        victim: EntityRef,
    ) -> Result<Option<u16>, ZoneError> {
        let EntityRef::Creature(id) = victim else {
            return Ok(None);
        };
        let Some(facing) = self.creatures.get(&id).map(|creature| creature.facing) else {
            return Ok(None);
        };
        let at = self.player_position(player_id)?;
        let here = self.unit_position(victim)?;
        Ok(trig::yaw_from_vector(at.x - here[0], at.z - here[2])
            .filter(|&bearing| flanks(facing, bearing)))
    }

    /// A flank at `bearing` dealt damage: the striker sees it, and the victim turns to face it
    /// unless a stun holds it, and guards its flank for a while (#949).
    pub(crate) fn flank_landed(&mut self, player_id: PlayerId, victim: EntityRef, bearing: u16) {
        if let EntityRef::Creature(id) = victim
            && let Some(creature) = self.creatures.get_mut(&id)
        {
            if !crate::aura::has_kind(&creature.auras, crate::AuraKind::Stun) {
                creature.facing = bearing;
            }
            creature.wary_ticks = FLANK_WARY_TICKS;
        }
        self.notify(
            player_id,
            ZoneEvent::Flanked {
                source: EntityRef::Player(player_id),
                target: victim,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_front_arc_takes_ordinary_hits_and_the_sides_and_rear_flank() {
        let degrees = |d: u32| u16::try_from(d * 65_536 / 360 % 65_536).unwrap();
        for (bearing, flank) in [
            (0, false),
            (59, false),
            (301, false),
            (61, true),
            (90, true),
        ] {
            assert_eq!(flanks(0, degrees(bearing)), flank, "{bearing}°");
        }
        assert!(flanks(degrees(90), degrees(270)));
        assert!(!flanks(degrees(350), degrees(20)));
    }

    #[test]
    fn turning_is_bounded_and_takes_the_short_way() {
        assert_eq!(turn_toward(0, 100, 364), 100);
        assert_eq!(turn_toward(0, 10_000, 364), 364);
        assert_eq!(turn_toward(0, 60_000, 364), 65_172);
        assert_eq!(turn_toward(65_500, 100, 364), 100);
    }
}
