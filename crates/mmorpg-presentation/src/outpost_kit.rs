//! The Outpost's kit buildings (#420): where both clients draw asset-tooling's `outpost-kit`
//! modules.
//!
//! [`mmorpg_scenery::outpost_kit::placements`] composes the modules on the adopted buildings'
//! collider footprints; here each becomes an entry-to-world matrix. The modules are static:
//! clients place them once. While a client does not draw them (its bundle has not loaded or
//! failed), the static scene keeps the buildings' procedural blockouts
//! ([`crate::Motion::Fallback`]). Presentation only.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::{Scenery, outpost_kit};

use crate::{
    batching::matrix,
    geometry::{compose, ry, translate},
};

/// One drawn kit module.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// The bundle entry, `outpost.kit.<part>` with `/<variant>` unless it is `default`.
    pub entry: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// The kit entries the clients load, sorted and once each.
#[must_use]
pub fn entries(scenery: &Scenery) -> Vec<&'static str> {
    outpost_kit::entries(scenery)
}

/// Every placed kit module on `scenery`.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    outpost_kit::placements(scenery)
        .iter()
        .map(|placement| {
            let [x, y, z] = placement.position;
            let yaw = f64::from(placement.yaw) / 65_536. * std::f64::consts::TAU;
            Part {
                entry: placement.entry,
                model: matrix(compose(&[
                    translate(metres(x), metres(y), metres(z)),
                    ry(yaw),
                ])),
            }
        })
        .collect()
}
