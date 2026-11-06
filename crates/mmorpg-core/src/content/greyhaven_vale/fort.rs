//! Authored Greyhaven architecture and camp geometry, independent of visual detail.
use crate::{StaticCollider, StaticMesh};

pub const TOWERS: [[i32; 2]; 4] = [[-2700, -700], [2700, -700], [-3000, 4000], [3100, 4000]];
pub const TOWER_BODIES: std::ops::RangeInclusive<u32> = 65040..=65043;

#[must_use]
pub fn meshes() -> Vec<StaticMesh> {
    serde_json::from_str(include_str!(
        "../../../content/collision/greyhaven-fort.json"
    ))
    .expect("committed Greyhaven fortification geometry is valid")
}

/// The art kit dresses these bodies; rock outcrops retain the shared rock material.
#[must_use]
pub fn dresses(id: u32) -> bool {
    id == super::ids::KEEP
        || super::ids::PALISADE.contains(&id)
        || super::ids::GATE_POSTS.contains(&id)
        || (65000..=65023).contains(&id)
        || TOWER_BODIES.contains(&id)
        || (65050..65065).contains(&id)
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
    // Palisade, windmill and tent boxes are replaced by authored meshes
    // with the same stable IDs. Conservative bounds must never become physical wall boxes.
    colliders.retain(|c| {
        !super::ids::PALISADE.contains(&c.id)
            && c.id != super::ids::WINDMILL
            && !super::ids::TENTS.contains(&c.id)
    });
    if let Some(keep) = colliders.iter_mut().find(|c| c.id == super::ids::KEEP) {
        keep.position[1] = keep.half_extents[1];
    }
    if let Some(well) = colliders.iter_mut().find(|c| c.id == super::ids::WELL) {
        well.position = [1100, 50, 3000];
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
