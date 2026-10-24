//! The Outpost market square (#421): where both clients draw asset-tooling's
//! `settlement-props`.
//!
//! [`mmorpg_scenery::settlement_props::placements`] composes the square before house 106; here
//! each prop becomes an entry-to-world matrix on the drawn surface. The props are static:
//! clients place them once. While a client does not draw them (its bundle has not loaded or
//! failed), the static scene draws [`fallback`] boxes in their place
//! ([`crate::Motion::Fallback`] for [`crate::Kit::SettlementProps`]). Presentation only.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::{Scenery, settlement_props};

use crate::{
    batching::matrix,
    geometry::{Affine, compose, ry, translate},
};

/// One drawn prop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// The bundle entry, `settlement.props.<prop>`.
    pub entry: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// The prop entries the clients load, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    settlement_props::entries()
}

/// Every placed prop's entry and entry-to-world frame on `scenery`'s drawn surface.
pub(crate) fn frames(scenery: &Scenery) -> Vec<(&'static str, Affine)> {
    settlement_props::placements(scenery)
        .iter()
        .map(|placement| {
            let [x, y, z] = placement.position;
            let lift = scenery.surface_at(x, z);
            let yaw = f64::from(placement.yaw) / 65_536. * std::f64::consts::TAU;
            (
                placement.entry,
                compose(&[translate(metres(x), metres(y + lift), metres(z)), ry(yaw)]),
            )
        })
        .collect()
}

/// Every placed prop on `scenery`.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    frames(scenery)
        .into_iter()
        .map(|(entry, frame)| Part {
            entry,
            model: matrix(frame),
        })
        .collect()
}

const WOOD: u32 = 0x8d6641;
const DARK: u32 = 0x5f4430;
const CANVAS: u32 = 0xd9cbad;
const IRON: u32 = 0x55585e;
const SOIL: u32 = 0x6b5a3e;

/// The fallback boxes of `entry` in its own frame: the descriptor's solid collision proxies
/// as `(centre, half extents, colour)`, metres.
pub(crate) fn fallback(entry: &str) -> &'static [([f64; 3], [f64; 3], u32)] {
    match entry.strip_prefix("settlement.props.") {
        Some("barrel") => &[([0., 0.45, 0.], [0.31, 0.45, 0.31], WOOD)],
        Some("bench") => &[([0., 0.23, 0.], [0.8, 0.23, 0.21], WOOD)],
        Some("cart") => &[([0., 0.44, 0.38], [0.68, 0.44, 1.22], WOOD)],
        Some("crate") => &[([0., 0.35, 0.], [0.355, 0.35, 0.355], WOOD)],
        Some("fence") => &[([0., 0.57, 0.], [1., 0.57, 0.08], DARK)],
        Some("firewood") => &[([0., 0.571_875, 0.], [0.87, 0.571_875, 0.42], WOOD)],
        Some("lantern-post") => &[([0., 1.53, 0.], [0.2, 1.53, 0.2], IRON)],
        Some("planter") => &[([0., 0.245, 0.], [0.6, 0.245, 0.25], SOIL)],
        Some("signpost") => &[([0., 1.48, 0.], [0.22, 1.48, 0.22], DARK)],
        Some("stall") => &[
            ([0., 0.5, 0.04], [1.24, 0.5, 0.74], WOOD),
            ([0., 2.375, 0.135], [1.3, 0.225, 0.915], CANVAS),
        ],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_prop_has_a_fallback() {
        let keys = entries();
        assert_eq!(keys.len(), 10, "{keys:?}");
        assert!(keys.iter().all(|key| !fallback(key).is_empty()), "{keys:?}");
    }
}
