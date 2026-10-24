//! The Sunken Shrine (#431): where both clients draw asset-tooling's `shrine-kit` modules.
//!
//! [`mmorpg_scenery::shrine::placements`] lays the modules over the zone's shrine colliders;
//! here each becomes an entry-to-world matrix lifted by the presentation relief like every
//! prop. The modules are static: clients place them once. Presentation only.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::{Scenery, shrine};

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
};

/// One drawn kit module.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub key: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// The kit entries the clients load, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    shrine::entries()
}

/// Every placed kit module on `scenery`'s relief.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    shrine::placements()
        .iter()
        .map(|placement| {
            let [x, y, z] = placement.position;
            let lift = scenery.height_at(x, z);
            let yaw = f64::from(placement.yaw) / 65_536. * std::f64::consts::TAU;
            Part {
                key: placement.key,
                model: matrix(compose(&[
                    translate(metres(x), metres(y + lift), metres(z)),
                    ry(yaw),
                ])),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kit_module_is_placed_once_or_more() {
        let keys = entries();
        assert_eq!(keys.len(), 14, "{keys:?}");
        assert!(keys.iter().all(|key| key.starts_with("shrine.kit.")));
    }
}
