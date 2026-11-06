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
    let mut entries = outpost_kit::entries(scenery);
    if scenery.greyhaven() {
        entries.push("greyhaven.fort.settlement");
    }
    entries
}

/// Every placed kit module on `scenery`.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    let mut parts: Vec<_> = outpost_kit::placements(scenery)
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
        .collect();
    if scenery.greyhaven() {
        parts.push(Part {
            entry: "greyhaven.fort.settlement",
            model: matrix(crate::geometry::ID),
        });
    }
    parts
}

/// Exact coloured collision surfaces remain visible if the authored fort bundle fails to load.
pub(crate) fn fallbacks(scenery: &Scenery) -> Vec<crate::Group> {
    use crate::{
        Draw, Group, Kit,
        batching::{Cull, material},
        geometry::{Geometry, ID},
    };
    use std::sync::Arc;
    scenery
        .collision_surfaces
        .iter()
        .filter(|mesh| mmorpg_core::greyhaven_vale::fort::dresses(mesh.id))
        .map(|mesh| {
            let mut geometry = Geometry::default();
            for triangle in &mesh.triangles {
                geometry.face(&triangle.map(|i| {
                    std::array::from_fn(|axis| {
                        metres(mesh.vertices[i as usize][axis] + mesh.position[axis])
                    })
                }));
            }
            let min = [0, 2].map(|axis| {
                geometry
                    .p
                    .iter()
                    .map(|v| v[axis])
                    .fold(f64::INFINITY, f64::min)
            });
            let max = [0, 2].map(|axis| {
                geometry
                    .p
                    .iter()
                    .map(|v| v[axis])
                    .fold(f64::NEG_INFINITY, f64::max)
            });
            let id = format!("fort-fallback-{}", mesh.id);
            Group {
                draws: vec![Draw {
                    id: id.clone(),
                    resource_key: format!("scenery:{id}"),
                    geometry: Arc::new(geometry),
                    model: matrix(ID),
                    material: material(0x78634b, false),
                }],
                cull: Cull::Far,
                instanced: false,
                fallback: Some(Kit::Outpost),
                min,
                max,
            }
        })
        .collect()
}
