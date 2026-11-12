//! The geometric coverage test of a facing-aimed attack (#517, #754): which bodies a swing or
//! cone reaches. Pure integer geometry; line of sight and target filtering stay with each game.

use sim_foundation::physics::Vec3i;
use sim_foundation::trig;

/// Whether a target centred at `target_xz` lies within `reach` (XZ centre distance) of
/// `origin_xz` and inside `half_width` either side of `facing` (yaw units). A target centred
/// on the origin is always inside.
#[must_use]
pub fn facing_covers(
    origin_xz: [i32; 2],
    facing: u16,
    target_xz: [i32; 2],
    reach: i32,
    half_width: u16,
) -> bool {
    let [dx, dz] = [
        i64::from(target_xz[0]) - i64::from(origin_xz[0]),
        i64::from(target_xz[1]) - i64::from(origin_xz[1]),
    ];
    // Saturates only for positions near opposite ends of the `i32` range, out of every reach.
    let distance_squared = dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz));
    if distance_squared > i64::from(reach).pow(2) {
        return false;
    }
    // Within reach, so the offset fits `i32` again.
    let (dx, dz) = (dx as i32, dz as i32);
    let (dir_x, dir_z) = trig::direction(facing);
    // |v| cos(angle to the facing) >= |v| cos(half width), both Q16.
    let along = i64::from(dx) * i64::from(dir_x) + i64::from(dz) * i64::from(dir_z);
    let length = trig::xz_length(dx, dz) as i64;
    along >= length * i64::from(trig::cos(half_width))
}

/// Whether a melee swing from a swinger standing at `swinger_feet` toward `facing` covers a
/// body centred at `target_center` with half extents `target_half`: [`facing_covers`] for
/// `reach` and `half_width`, and the body's vertical extent overlaps the inclusive `band`
/// above the swinger's feet. The shared player swing passes
/// [`PLAYER_REACH_UNITS`](crate::unit::PLAYER_REACH_UNITS),
/// [`SWING_ARC_HALF_WIDTH`](crate::unit::SWING_ARC_HALF_WIDTH) and
/// [`SWING_BAND_UNITS`](crate::unit::SWING_BAND_UNITS).
#[must_use]
pub fn swing_covers(
    swinger_feet: Vec3i,
    facing: u16,
    target_center: Vec3i,
    target_half: Vec3i,
    reach: i32,
    half_width: u16,
    band: [i32; 2],
) -> bool {
    let bottom = i64::from(target_center.y) - i64::from(target_half.y) - i64::from(swinger_feet.y);
    let top = i64::from(target_center.y) + i64::from(target_half.y) - i64::from(swinger_feet.y);
    top >= i64::from(band[0])
        && bottom <= i64::from(band[1])
        && facing_covers(
            [swinger_feet.x, swinger_feet.z],
            facing,
            [target_center.x, target_center.z],
            reach,
            half_width,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::{PLAYER_REACH_UNITS, SWING_ARC_HALF_WIDTH, SWING_BAND_UNITS};

    const HALF: Vec3i = Vec3i::new(40, 90, 40);

    fn covers(target: Vec3i) -> bool {
        swing_covers(
            Vec3i::ZERO,
            0,
            target,
            HALF,
            PLAYER_REACH_UNITS,
            SWING_ARC_HALF_WIDTH,
            SWING_BAND_UNITS,
        )
    }

    #[test]
    fn covers_reach_arc_and_band() {
        let (_, ahead) = trig::direction(0);
        assert!(ahead > 0, "yaw 0 faces +z");
        assert!(covers(Vec3i::new(0, 90, 250)));
        assert!(!covers(Vec3i::new(0, 90, 251)), "beyond reach");
        assert!(!covers(Vec3i::new(0, 90, -100)), "behind");
        assert!(!covers(Vec3i::new(200, 90, 100)), "outside the 50° arc");
        assert!(covers(Vec3i::new(100, 90, 200)), "inside the arc");
        assert!(
            covers(Vec3i::new(0, 290, 100)),
            "body bottom at the band top"
        );
        assert!(!covers(Vec3i::new(0, 291, 100)), "above the band");
        assert!(
            covers(Vec3i::new(0, -190, 100)),
            "body top at the band bottom"
        );
        assert!(!covers(Vec3i::new(0, -191, 100)), "below the band");
        assert!(covers(Vec3i::new(0, 90, 0)), "centred on the swinger");
    }

    #[test]
    fn far_positions_never_overflow() {
        assert!(!facing_covers(
            [i32::MIN, i32::MIN],
            0,
            [i32::MAX, i32::MAX],
            PLAYER_REACH_UNITS,
            SWING_ARC_HALF_WIDTH,
        ));
    }
}
