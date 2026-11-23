//! The day/night clock both clients light and dress by (#158, #1059): the zone's hour, the
//! Pages demo's cosmetic twenty-minute day and whether it is night. Time of day is
//! `mmorpg_core`'s pure function of the zone tick; this only reads it for presentation.

use mmorpg_core::{DAY_TICKS, HOUR_TICKS, ZONE_START_TIME, is_night, time_of_day};

/// The Pages demo's cosmetic day in real seconds, zero at noon.
pub const PAGES_DAY_SECONDS: f64 = 1200.0;

/// The zone's hour at `tick`: the sky follows the zone's clock rather than the client's.
#[must_use]
pub fn zone_hour(tick: u64) -> f32 {
    (f64::from(time_of_day(tick)) / f64::from(DAY_TICKS) * 24.0) as f32
}

/// The hour the sky shows at zone `tick`: the zone's hour, or with reduced motion the start of
/// that zone hour, so the sky steps once an hour instead of drifting.
#[must_use]
pub fn sky_hour(tick: u64, animate: bool) -> f32 {
    if animate {
        return zone_hour(tick);
    }
    // The zone starts on the hour, so the zone hour starts where the tick's does.
    const _: () = assert!(ZONE_START_TIME.is_multiple_of(HOUR_TICKS));
    zone_hour(tick - tick % u64::from(HOUR_TICKS))
}

/// The Pages cycle's hour at `seconds`, whose zero is noon; reduced motion holds noon.
#[must_use]
pub fn pages_hour(seconds: f64, animate: bool) -> f32 {
    let phase = if animate && seconds.is_finite() {
        (seconds / PAGES_DAY_SECONDS).rem_euclid(1.0)
    } else {
        0.0
    };
    ((12.0 + phase * 24.0) % 24.0) as f32
}

/// The day's fraction since noon at `hour`, in `[0, 1)`: the cosmetic sky's phase.
#[must_use]
pub fn day_fraction(hour: f32) -> f32 {
    ((hour - 12.0) / 24.0).rem_euclid(1.0)
}

/// Whether zone `tick` falls between dusk and dawn (`mmorpg_core::is_night`).
#[must_use]
pub const fn zone_night(tick: u64) -> bool {
    is_night(time_of_day(tick))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_cycle_starts_at_noon_and_the_zone_at_eight() {
        assert_eq!(pages_hour(0.0, true), 12.0);
        assert_eq!(pages_hour(600.0, true), 0.0);
        assert_eq!(pages_hour(600.0, false), 12.0);
        assert!((zone_hour(0) - 8.0).abs() < 1e-4);
    }

    #[test]
    fn a_zone_day_is_one_sky_cycle_and_reduced_motion_steps_by_the_hour() {
        let hour = u64::from(HOUR_TICKS);
        assert!((zone_hour(4 * hour) - 12.0).abs() < 1e-4);
        assert!(zone_hour(16 * hour).abs() < 1e-4);
        assert_eq!(zone_hour(u64::from(DAY_TICKS)), zone_hour(0));
        assert!(day_fraction(zone_hour(4 * hour)).abs() < 1e-5);
        assert!((day_fraction(zone_hour(0)) - 5.0 / 6.0).abs() < 1e-5);
        assert!((sky_hour(4 * hour + hour / 2, false) - 12.0).abs() < 1e-4);
        assert!((sky_hour(4 * hour + hour / 2, true) - 12.5).abs() < 1e-4);
        assert!((sky_hour(0, false) - 8.0).abs() < 1e-4);
    }

    #[test]
    fn night_runs_from_dusk_to_dawn() {
        let hour = u64::from(HOUR_TICKS);
        assert!(!zone_night(0));
        assert!(zone_night(12 * hour));
        assert!(zone_night(21 * hour));
        assert!(!zone_night(22 * hour));
    }
}
