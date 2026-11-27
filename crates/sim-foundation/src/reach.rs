//! Horizontal distances and reach tests every interaction shares (#1253).
//!
//! Positions are integer units. Each test widens before subtracting, so it is exact for every
//! `i32` position, including recovered extreme ones.

use crate::trig;

/// The squared XZ distance from `a` to `b`, saturating at `i64::MAX` (only offsets beyond
/// about 2^31 on both axes reach it). Steering and nearest-unit searches compare it.
#[must_use]
pub const fn xz_distance_squared(a: [i32; 2], b: [i32; 2]) -> i64 {
    let dx = b[0] as i64 - a[0] as i64;
    let dz = b[1] as i64 - a[1] as i64;
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}

/// The floor of the XZ distance between `a` and `b` in units, saturating at `i64::MAX`: the
/// rounded metric routes, carts, crossings, supply patrols, escorts and flocks measure with.
#[must_use]
pub const fn xz_distance(a: [i32; 2], b: [i32; 2]) -> i64 {
    let (dx, dz) = (b[0] as i64 - a[0] as i64, b[1] as i64 - a[1] as i64);
    if dx >= i32::MIN as i64
        && dx <= i32::MAX as i64
        && dz >= i32::MIN as i64
        && dz <= i32::MAX as i64
    {
        let length = trig::xz_length(dx as i32, dz as i32);
        if length <= i64::MAX as u64 {
            return length as i64;
        }
    }
    i64::MAX
}

/// Whether `b` lies within `radius` units of `a` horizontally (the boundary counts), exactly.
#[must_use]
pub const fn within_xz(a: [i32; 2], b: [i32; 2], radius: i32) -> bool {
    let dx = b[0] as i128 - a[0] as i128;
    let dz = b[1] as i128 - a[1] as i128;
    let radius = radius as i128;
    dx * dx + dz * dz <= radius * radius
}

/// Whether `b` lies within `radius` units of `a` in all three axes (the boundary counts),
/// exactly: corpses are looted, skinned and spent from this sphere.
#[must_use]
pub const fn within_sphere(a: [i32; 3], b: [i32; 3], radius: i32) -> bool {
    let dx = b[0] as i128 - a[0] as i128;
    let dy = b[1] as i128 - a[1] as i128;
    let dz = b[2] as i128 - a[2] as i128;
    let radius = radius as i128;
    dx * dx + dy * dy + dz * dz <= radius * radius
}

/// Whether feet at `feet` stand in the reach cylinder around `at`: within `radius` units
/// horizontally and within `height` units vertically (both boundaries count). Interaction
/// spots, caches, loads and mechanisms are reached this way.
#[must_use]
pub const fn within_cylinder(at: [i32; 3], radius: i32, height: i32, feet: [i32; 3]) -> bool {
    let rise = feet[1] as i64 - at[1] as i64;
    within_xz([at[0], at[2]], [feet[0], feet[2]], radius) && rise.abs() <= height as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extreme_positions_neither_overflow_nor_wrap() {
        let (low, high) = ([i32::MIN, i32::MIN], [i32::MAX, i32::MAX]);
        assert_eq!(xz_distance_squared(low, high), i64::MAX);
        assert_eq!(xz_distance(low, high), i64::MAX);
        assert!(!within_xz(low, high, i32::MAX));
        assert!(!within_cylinder(
            [i32::MIN, 0, 0],
            100,
            100,
            [i32::MAX, 0, 0]
        ));
    }

    #[test]
    fn boundaries_count_as_within() {
        assert!(within_xz([0, 0], [300, 400], 500));
        assert!(!within_xz([0, 0], [300, 401], 500));
        assert_eq!(xz_distance([0, 0], [300, 400]), 500);
        assert_eq!(xz_distance_squared([10, 10], [13, 14]), 25);
        assert!(within_cylinder([0, 50, 0], 500, 200, [300, 250, 400]));
        assert!(!within_cylinder([0, 50, 0], 500, 200, [300, 251, 400]));
        assert!(within_cylinder([0, 50, 0], 500, 200, [0, -150, 0]));
    }
}
