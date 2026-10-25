//! Moving platforms (#56): content-authored boxes that shuttle between two stops on a fixed
//! schedule, carrying whatever stands on them.
//!
//! The zone owns the schedule: a platform rests [`Platform::dwell`] ticks at its low stop,
//! travels to its high stop at [`Platform::speed`] subunits per tick, rests again and travels
//! back, forever, as a pure function of the zone tick ([`Platform::centre_at`]). Canonical
//! state therefore needs nothing beyond the tick, and every client derives the same position
//! from content and a projection's tick. Physics owns the motion's consequences: each tick the
//! zone asks the physical world to move the platform's body to the tick's centre, and the
//! backend carries the units standing on it ([`crate::physics::World::move_platform`]).
//!
//! A platform's box is authored to sweep only space nothing else can occupy while it moves
//! (a lift pillar that rises out of the ground under its own top), so it never runs into a
//! unit except the ones it carries.

use crate::ZoneError;

/// Platforms a zone's content may author.
pub const MAX_PLATFORMS: usize = 8;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PlatformId(u8);

impl PlatformId {
    #[must_use]
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// One authored platform: its physics body (in the static collider namespace), its box and
/// its schedule between two box centres.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Platform {
    pub id: PlatformId,
    pub body: u32,
    pub half_extents: [i32; 3],
    /// Box centre at the low and the high stop, in units.
    pub low: [i32; 3],
    pub high: [i32; 3],
    /// Subunits per tick ([`crate::physics::SUBUNITS_PER_UNIT`]) while travelling.
    pub speed: i32,
    /// Ticks resting at each stop.
    pub dwell: u32,
}

/// Where in its cycle a platform is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlatformPhase {
    AtLow,
    Rising,
    AtHigh,
    Falling,
}

impl Platform {
    /// The straight distance between the stops, in whole units (rounded up).
    #[must_use]
    pub fn distance(&self) -> u32 {
        let squared: i64 = (0..3)
            .map(|axis| {
                let delta = i64::from(self.high[axis]) - i64::from(self.low[axis]);
                delta * delta
            })
            .sum();
        let root = squared.isqrt();
        let root = if root * root < squared {
            root + 1
        } else {
            root
        };
        u32::try_from(root).unwrap_or(u32::MAX)
    }

    /// Ticks one trip between the stops takes.
    #[must_use]
    pub fn travel_ticks(&self) -> u32 {
        let speed = u64::from(self.speed.unsigned_abs().max(1));
        let subunits = u64::from(self.distance())
            * u64::from(crate::physics::SUBUNITS_PER_UNIT.unsigned_abs());
        u32::try_from(subunits.div_ceil(speed)).unwrap_or(u32::MAX)
    }

    /// Ticks of one whole cycle: rest, rise, rest, fall.
    #[must_use]
    pub fn period(&self) -> u32 {
        2 * (self.dwell + self.travel_ticks())
    }

    /// The phase at zone `tick` and the ticks spent in it.
    #[must_use]
    pub fn phase_at(&self, tick: u64) -> (PlatformPhase, u32) {
        let period = u64::from(self.period().max(1));
        // The remainder is below the u32 period.
        let at = u32::try_from(tick % period).unwrap_or(0);
        let (dwell, travel) = (self.dwell, self.travel_ticks());
        if at < dwell {
            (PlatformPhase::AtLow, at)
        } else if at < dwell + travel {
            (PlatformPhase::Rising, at - dwell)
        } else if at < 2 * dwell + travel {
            (PlatformPhase::AtHigh, at - dwell - travel)
        } else {
            (PlatformPhase::Falling, at - 2 * dwell - travel)
        }
    }

    /// The box centre at the end of zone `tick`, in units: the low stop plus the distance
    /// travelled so far along the straight route, rounded toward the low stop.
    #[must_use]
    pub fn centre_at(&self, tick: u64) -> [i32; 3] {
        let distance = i64::from(self.distance().max(1));
        let speed = i64::from(self.speed.unsigned_abs());
        // Whole units covered after `elapsed` ticks at `speed` subunits per tick.
        let covered = |elapsed: u32| {
            (i64::from(elapsed) * speed / i64::from(crate::physics::SUBUNITS_PER_UNIT))
                .min(distance)
        };
        let travelled = match self.phase_at(tick) {
            (PlatformPhase::AtLow, _) => 0,
            (PlatformPhase::AtHigh, _) => distance,
            (PlatformPhase::Rising, elapsed) => covered(elapsed),
            (PlatformPhase::Falling, elapsed) => distance - covered(elapsed),
        };
        let mut centre = self.low;
        for (axis, value) in centre.iter_mut().enumerate() {
            let delta = i64::from(self.high[axis]) - i64::from(self.low[axis]);
            // |delta × travelled / distance| ≤ |delta|, which fits.
            *value += i32::try_from(delta * travelled / distance).unwrap_or(0);
        }
        centre
    }

    pub(crate) fn validate(&self) -> Result<(), ZoneError> {
        let in_range = |centre: [i32; 3]| {
            (0..3).all(|axis| {
                (i64::from(centre[axis]) - i64::from(self.half_extents[axis])).abs()
                    <= i64::from(crate::MAX_CONTENT_COORDINATE_UNITS)
                    && (i64::from(centre[axis]) + i64::from(self.half_extents[axis])).abs()
                        <= i64::from(crate::MAX_CONTENT_COORDINATE_UNITS)
            })
        };
        if self.half_extents.iter().any(|half| *half <= 0)
            || !in_range(self.low)
            || !in_range(self.high)
            || self.low == self.high
            || self.speed <= 0
            || self.dwell == 0
        {
            return Err(ZoneError::new(
                "platform box, stops, speed or dwell is invalid",
            ));
        }
        Ok(())
    }
}

impl crate::ZoneSimulation {
    /// Moves every platform's body to its centre at the end of `tick` (#56): before the
    /// physics step of that tick, or at once when the zone is built or restored.
    pub(crate) fn schedule_platforms(&mut self, tick: u64, at_once: bool) -> Result<(), ZoneError> {
        for platform in self.content.platforms().to_vec() {
            let [x, y, z] = platform.centre_at(tick);
            let body = crate::physics::BodyId(u64::from(platform.body));
            let centre = crate::physics::Vec3i::new(x, y, z);
            let moved = if at_once {
                self.world.place_platform(body, centre)
            } else {
                self.world.move_platform(body, centre)
            };
            moved.map_err(crate::zone::physics_error)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lift() -> Platform {
        Platform {
            id: PlatformId::new(1),
            body: 7,
            half_extents: [100, 400, 100],
            low: [0, -400, 0],
            high: [0, 400, 0],
            speed: 6 * crate::physics::SUBUNITS_PER_UNIT,
            dwell: 10,
        }
    }

    #[test]
    fn the_schedule_rests_rises_rests_and_falls_forever() {
        let lift = lift();
        assert_eq!(lift.distance(), 800);
        assert_eq!(lift.travel_ticks(), 134);
        assert_eq!(lift.period(), 288);
        assert_eq!(lift.centre_at(0), lift.low);
        assert_eq!(lift.centre_at(9), lift.low);
        assert_eq!(lift.centre_at(11), [0, -394, 0]);
        assert_eq!(lift.centre_at(10 + 134), lift.high);
        assert_eq!(
            lift.centre_at(10 + 133),
            [0, 398, 0],
            "the last step is short"
        );
        assert_eq!(lift.centre_at(2 * 10 + 134 + 1), [0, 394, 0]);
        assert_eq!(lift.centre_at(287), [0, -398, 0]);
        assert_eq!(lift.centre_at(288), lift.low);
        // Never beyond a stop, and at most `speed` per tick.
        for tick in 0..2_000 {
            let (a, b) = (lift.centre_at(tick), lift.centre_at(tick + 1));
            assert!((-400..=400).contains(&b[1]));
            assert!((a[1] - b[1]).abs() <= 6, "{tick}");
        }
    }

    #[test]
    fn invalid_platforms_are_refused() {
        assert!(lift().validate().is_ok());
        for broken in [
            Platform { speed: 0, ..lift() },
            Platform { dwell: 0, ..lift() },
            Platform {
                high: lift().low,
                ..lift()
            },
            Platform {
                half_extents: [0, 1, 1],
                ..lift()
            },
        ] {
            assert!(broken.validate().is_err(), "{broken:?}");
        }
    }
}
