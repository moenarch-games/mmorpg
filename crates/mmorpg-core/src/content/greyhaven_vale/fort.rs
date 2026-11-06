//! Authored Greyhaven architecture and camp geometry, independent of visual detail.
use crate::{StaticCollider, StaticMesh};

pub const TOWERS: [[i32; 2]; 4] = [[-2700, -700], [2700, -700], [-3000, 4000], [3100, 4000]];
pub const TOWER_BODIES: std::ops::RangeInclusive<u32> = 65040..=65043;
/// Low authored outcrops that break Stillwater's bank silhouette without closing its routes.
pub const SHORE_ROCKS: std::ops::RangeInclusive<u32> = 65070..=65093;
/// The mill's walk-off gallery, outside the hoist shaft and clear of the axle housing.
pub const MILL_GALLERY: std::ops::RangeInclusive<u32> = 65103..=65110;

/// Authored leaning rock slabs and their smaller talus pieces at the basin cliff feet.
pub const BASIN_ROCKS: std::ops::RangeInclusive<u32> = 65094..=65102;

#[must_use]
pub fn meshes() -> Vec<StaticMesh> {
    let meshes: Vec<StaticMesh> = serde_json::from_str(include_str!(
        "../../../content/collision/greyhaven-fort.json"
    ))
    .expect("committed Greyhaven fortification geometry is valid");
    meshes
        .into_iter()
        .map(|mesh| {
            if SHORE_ROCKS.contains(&mesh.id) {
                super::lake::place_mesh(mesh)
            } else {
                mesh
            }
        })
        .collect()
}

/// The art kit dresses these bodies; rock outcrops retain the shared rock material.
#[must_use]
pub fn dresses(id: u32) -> bool {
    id == super::ids::KEEP
        || id == super::ids::WELL
        || id == super::ids::CAVE_THRESHOLD
        || super::ids::PALISADE.contains(&id)
        || super::ids::GATE_POSTS.contains(&id)
        || (65000..=65023).contains(&id)
        || TOWER_BODIES.contains(&id)
        || MILL_GALLERY.contains(&id)
        || (65050..=65069).contains(&id)
        || super::ids::TENTS.contains(&id)
        || super::ids::CRATES.contains(&id)
        || [
            super::ids::BARN,
            super::ids::FARMHOUSE,
            super::ids::WINDMILL,
        ]
        .contains(&id)
}

pub(super) fn apply(colliders: &mut Vec<StaticCollider>) {
    // Palisade, windmill, well and tent boxes are replaced by authored meshes
    // with the same stable IDs. Conservative bounds must never become physical wall boxes.
    colliders.retain(|c| {
        !super::ids::PALISADE.contains(&c.id)
            && c.id != super::ids::WINDMILL
            && c.id != super::ids::WELL
            && !super::ids::TENTS.contains(&c.id)
    });
    if let Some(keep) = colliders.iter_mut().find(|c| c.id == super::ids::KEEP) {
        keep.position[1] = keep.half_extents[1];
    }
    colliders.extend(
        TOWERS
            .into_iter()
            .zip(TOWER_BODIES)
            .map(|([x, z], id)| StaticCollider {
                id,
                position: [x, 275, z],
                half_extents: [140, 275, 140],
            }),
    );
}
