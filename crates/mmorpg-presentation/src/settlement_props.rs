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
/// the zone collides with (`mmorpg_core::greyhaven_vale::market`), as
/// `(centre, half extents, colour)` in metres.
pub(crate) fn fallback(entry: &str) -> Vec<([f64; 3], [f64; 3], u32)> {
    use mmorpg_core::greyhaven_vale::market::Item;
    let Some(item) = settlement_props::item(entry) else {
        return Vec::new();
    };
    let colour = match item {
        Item::Fence | Item::Signpost => DARK,
        Item::LanternPost => IRON,
        Item::Planter => SOIL,
        _ => WOOD,
    };
    item.proxies()
        .iter()
        .enumerate()
        .map(|(index, (centre, half))| {
            // A stall's second proxy is its canvas awning.
            let colour = if item == Item::Stall && index == 1 {
                CANVAS
            } else {
                colour
            };
            (centre.map(metres), half.map(metres), colour)
        })
        .collect()
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
