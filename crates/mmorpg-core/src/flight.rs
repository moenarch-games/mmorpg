//! Flight (#995, Atlas 5.2.i995): one narrow rule for a flying creature, the Greyfang cave bat,
//! inside the ordinary creature life cycle. No 3D pathfinding: a flier moves across the ground
//! like any creature (its chase, leash, evade and wall routing), and the zone only sets its
//! height.
//!
//! - **Weightless.** A flier's body has no gravity. Every tick the zone moves its centre toward
//!   a height goal at a bounded rate; the physics step still stops it at walls, the roof and
//!   the floor, so it never flies through rock.
//! - **Roost.** Idle, evading or home again it hangs still at its roost, [`ROOST_RISE_UNITS`]
//!   above the ground at its spawn point, and never wanders.
//! - **Fight.** Engaged, it flies [`CRUISE_UNITS`] above its target's centre (no higher than
//!   its roost), out of a sword's height band. Its dive ([`crate::creature_attack`]) stoops
//!   down to [`STRIKE_UNITS`] above the target's centre over the dive's last ticks; it hangs
//!   there through its recovery, within the target's reach, and climbs back afterwards.
//! - **Bite.** Its dive lands only on a target within [`BITE_HEIGHT_UNITS`] of its height.
//! - **Leash.** Beyond [`LEASH_UNITS`] of its roost it gives up and flies home.
//! - **Death.** Its corpse lies on the ground under the spot it fell.
//!
//! The height goal is derived from canonical state each tick (its life, fight, pending attack
//! and swing timer), so replay and recovery need nothing stored.

use crate::physics::SUBUNITS_PER_UNIT;

/// A roosting flier's centre stands this far above the ground at its spawn point (3.2 m): it
/// hangs from the cave roof's lowest rock, over a player's head.
pub const ROOST_RISE_UNITS: i32 = 320;
/// In a fight it flies with its centre this far above its target's (1.7 m): its body clears a
/// sword's height band ((`sim_combat::SWING_BAND_UNITS`)) over a player standing on the same floor.
pub const CRUISE_UNITS: i32 = 170;
/// It dives to, and recovers with, its centre this far above its target's (0.3 m): chest
/// height, where it bites and can be hit back.
pub const STRIKE_UNITS: i32 = 30;
/// Its bite lands only on a target whose centre is within this height of its own (1 m).
pub const BITE_HEIGHT_UNITS: i32 = 100;
/// It gives a fight up beyond this XZ distance from its roost (15 m): a player who runs off,
/// out of the cave mouth, escapes it.
pub const LEASH_UNITS: i32 = 1_500;
/// It climbs and sinks at most 3 m/s …
pub const CLIMB_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(300);
/// … and stoops at 4.2 m/s through its dive, the 1.4 m from cruise to strike height in
/// [`crate::BAT_DIVE_TICKS`].
pub const STOOP_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(420);

/// The vertical velocity (subunits per tick) that carries a flier's centre from `y` toward
/// `goal` (units), at most `rate` either way.
#[must_use]
pub const fn vertical_velocity(y: i32, goal: i32, rate: i32) -> i32 {
    let gap = goal.saturating_sub(y).saturating_mul(SUBUNITS_PER_UNIT);
    if gap > rate {
        rate
    } else if gap < -rate {
        -rate
    } else {
        gap
    }
}

/// Where a flier is in its fight, which sets its height goal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    /// Not fighting: at its roost.
    Roost,
    /// Flying after its target or winding up its dive.
    Cruise,
    /// Stooping down its dive line.
    Stoop,
    /// Hanging low after its contact.
    Recover,
}

impl Phase {
    /// The height goal for a flier's centre and the rate it may move at: `target` is its
    /// target's centre height, `roost` its roost's.
    #[must_use]
    pub const fn goal(self, target: i32, roost: i32) -> (i32, i32) {
        let cruise = target.saturating_add(CRUISE_UNITS);
        match self {
            Self::Roost => (roost, CLIMB_SUBUNITS_PER_TICK),
            Self::Cruise => (
                if cruise < roost { cruise } else { roost },
                CLIMB_SUBUNITS_PER_TICK,
            ),
            Self::Stoop => (target.saturating_add(STRIKE_UNITS), STOOP_SUBUNITS_PER_TICK),
            Self::Recover => (target.saturating_add(STRIKE_UNITS), CLIMB_SUBUNITS_PER_TICK),
        }
    }
}
