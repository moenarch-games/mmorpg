//! The Outpost's kit buildings (#420): where both clients draw asset-tooling's `outpost-kit`
//! modules.
//!
//! World dressing data supplies each module and its transform; here the existing clients'
//! placed-kit adapters receive those entries. The modules are static:
//! clients place them once. While a client does not draw them (its bundle has not loaded or
//! failed), the static scene keeps the buildings' procedural blockouts
//! ([`crate::Motion::Fallback`]). Presentation only.
use mmorpg_core::UNITS_PER_METRE;
use mmorpg_scenery::Scenery;

use crate::{Kit, batching::matrix, world_dressing};

/// One drawn kit module.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    /// The bundle entry, `outpost.kit.<part>` with `/<variant>` unless it is `default`.
    pub entry: &'static str,
    /// Column-major entry-to-world matrix, metres.
    pub model: [f32; 16],
    /// The building's wear in [0, 1] ([`world_dressing::Placed::wear`]), which the browser blends
    /// from the kit's wear masks; the native client draws the pristine modules.
    pub wear: f32,
}

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// The kit entries the clients load, sorted and once each.
#[must_use]
pub fn entries(scenery: &Scenery) -> Vec<&'static str> {
    world_dressing::assets(scenery, Kit::Outpost)
}

/// Every placed kit module on `scenery`, in the committed dressing order.
#[must_use]
pub fn parts(scenery: &Scenery) -> Vec<Part> {
    world_dressing::placed(scenery)
        .iter()
        .filter(|placed| placed.kit() == Some(Kit::Outpost))
        .map(|placed| Part {
            entry: placed.asset.as_str(),
            model: matrix(placed.frame()),
            wear: f32::from(placed.wear) / 255.,
        })
        .collect()
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
