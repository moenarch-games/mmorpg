//! The dodge roll (#516): a fast, short reposition on a tap of Shift.
//!
//! A living, grounded player who is not swimming, climbing, mantling or stunned rolls in the
//! direction they move, or straight back when they stand still. The roll moves the body through
//! the character controller like any other move ([`ROLL_SPEED_UNITS_PER_SECOND`] for
//! [`ROLL_TICKS`], about 3.5 m), keeps gravity and ignores movement and jump intent until it
//! ends. It is movement only: no invulnerability and no cost, so attacks still hit mid-roll. A
//! roll cancels the player's cast, and a cast started mid-roll is cancelled too. After it, the
//! player cannot roll again for [`ROLL_COOLDOWN_TICKS`]. Death, a stun, deep water or a forced
//! move ends a roll early; its cooldown still runs.
//!
//! The zone resolves a press on the next tick ([`Roll::Pending`]). The state is canonical, so a
//! restored zone continues a roll exactly.

use crate::trig;

/// A roll lasts this long (0.6 s) …
pub const ROLL_TICKS: u16 = crate::ticks_from_ms(600);
/// … at this speed (5.85 m/s), so it covers about 3.5 m on flat ground.
pub const ROLL_SPEED_UNITS_PER_SECOND: i32 = 585;
pub const ROLL_SPEED_SUBUNITS_PER_TICK: i32 = crate::subunits_per_tick(ROLL_SPEED_UNITS_PER_SECOND);
/// After a roll ends, the next can start this much later (1 s).
pub const ROLL_COOLDOWN_TICKS: u16 = crate::ticks_from_ms(1_000);

/// A player's dodge roll (#516).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Roll {
    /// No roll and no cooldown: a press is accepted.
    #[default]
    Ready,
    /// Pressed; the next tick starts the roll if the player can roll, or drops the press.
    Pending,
    /// Rolling along yaw `yaw`; `ticks` of [`ROLL_TICKS`] have passed.
    Rolling { yaw: u16, ticks: u16 },
    /// The roll ended; `ticks` of [`ROLL_COOLDOWN_TICKS`] have passed.
    Recovering { ticks: u16 },
}

impl Roll {
    /// The yaw of a roll in progress.
    #[must_use]
    pub const fn rolling(self) -> Option<u16> {
        match self {
            Self::Rolling { yaw, .. } => Some(yaw),
            Self::Ready | Self::Pending | Self::Recovering { .. } => None,
        }
    }

    /// Whether every field is within range.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        match self {
            Self::Ready | Self::Pending => true,
            Self::Rolling { ticks, .. } => ticks < ROLL_TICKS,
            Self::Recovering { ticks } => ticks < ROLL_COOLDOWN_TICKS,
        }
    }

    /// One tick on: a roll runs out into its cooldown and the cooldown into ready. A pending
    /// press is left for [`Self::start`].
    #[must_use]
    pub(crate) const fn advanced(self) -> Self {
        match self {
            Self::Rolling { yaw, ticks } if ticks + 1 < ROLL_TICKS => Self::Rolling {
                yaw,
                ticks: ticks + 1,
            },
            Self::Rolling { .. } => Self::Recovering { ticks: 0 },
            Self::Recovering { ticks } if ticks + 1 < ROLL_COOLDOWN_TICKS => {
                Self::Recovering { ticks: ticks + 1 }
            }
            Self::Recovering { .. } => Self::Ready,
            Self::Ready | Self::Pending => self,
        }
    }

    /// Ends a roll in progress early: its cooldown starts.
    #[must_use]
    pub(crate) const fn ended(self) -> Self {
        match self {
            Self::Rolling { .. } => Self::Recovering { ticks: 0 },
            Self::Ready | Self::Pending | Self::Recovering { .. } => self,
        }
    }

    /// Resolves a pending press: a roll along `yaw` when the player `can` roll, otherwise the
    /// press is dropped.
    #[must_use]
    pub(crate) const fn start(self, can: bool, yaw: u16) -> Self {
        match self {
            Self::Pending if can => Self::Rolling { yaw, ticks: 0 },
            Self::Pending => Self::Ready,
            Self::Ready | Self::Rolling { .. } | Self::Recovering { .. } => self,
        }
    }

    /// The horizontal controller velocity of a roll along `yaw`, in subunits per tick, scaled by
    /// the player's movement speed `percent` (roots stop it, snares slow it).
    pub(crate) fn velocity(yaw: u16, percent: i32) -> [i32; 2] {
        let (x, z) = trig::direction(yaw);
        let along = |unit| trig::checked_scale(ROLL_SPEED_SUBUNITS_PER_TICK, unit).unwrap_or(0);
        crate::aura::scale_velocity([along(x), along(z)], percent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_roll_runs_then_recovers_then_is_ready() {
        let mut roll = Roll::Pending.start(true, 7);
        let mut rolling = 0;
        let mut recovering = 0;
        while roll != Roll::Ready {
            match roll {
                Roll::Rolling { yaw, .. } => {
                    assert_eq!(yaw, 7);
                    rolling += 1;
                }
                Roll::Recovering { .. } => recovering += 1,
                Roll::Ready | Roll::Pending => unreachable!(),
            }
            assert!(roll.is_valid());
            roll = roll.advanced();
        }
        assert_eq!(rolling, ROLL_TICKS);
        assert_eq!(recovering, ROLL_COOLDOWN_TICKS);
    }

    #[test]
    fn a_press_that_cannot_roll_is_dropped_and_an_ended_roll_recovers() {
        assert_eq!(Roll::Pending.start(false, 0), Roll::Ready);
        assert_eq!(Roll::Ready.start(true, 0), Roll::Ready);
        assert_eq!(
            Roll::Rolling { yaw: 3, ticks: 4 }.ended(),
            Roll::Recovering { ticks: 0 }
        );
        assert!(
            !Roll::Rolling {
                yaw: 0,
                ticks: ROLL_TICKS
            }
            .is_valid()
        );
    }

    #[test]
    fn the_tuning_covers_about_three_and_a_half_metres() {
        let distance =
            ROLL_SPEED_UNITS_PER_SECOND * i32::from(ROLL_TICKS) / i32::from(crate::TICK_HZ);
        assert_eq!(distance, 351);
    }
}
