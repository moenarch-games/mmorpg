//! Authored Greyhaven architecture and camp geometry, independent of visual detail.
use crate::{StaticCollider, StaticMesh};

/// Small north-entrance hideout: fire, stockade and canvas shelters.
pub const REDBRAND_FIRE: u32 = 65111;
pub const REDBRAND_PALISADES: std::ops::RangeInclusive<u32> = 65112..=65117;
pub const REDBRAND_SHELTERS: std::ops::RangeInclusive<u32> = 65118..=65120;

/// Western watermill, wheel envelope and bank crossing.
pub const BROOK_MILL: std::ops::RangeInclusive<u32> = 65121..=65127;

pub const TOWERS: [[i32; 2]; 4] = [[-2700, -700], [2700, -700], [-3000, 4000], [3100, 4000]];
pub const TOWER_BODIES: std::ops::RangeInclusive<u32> = 65040..=65043;
/// Low talus clusters that break Stillwater's bank silhouette without closing its routes:
/// world dressing whose collision is generated from its art ([`dressing_meshes`], #683).
pub const SHORE_ROCKS: std::ops::RangeInclusive<u32> = 65070..=65093;
/// The mill's walk-off gallery, outside the hoist shaft and clear of the axle housing.
pub const MILL_GALLERY: std::ops::RangeInclusive<u32> = 65103..=65110;

/// Three fractured brook-facing spurs and their smaller talus pieces: world dressing whose
/// collision is generated from its art ([`dressing_meshes`], #683).
pub const WESTERN_ROCKS: std::ops::RangeInclusive<u32> = 65128..=65136;

/// The Eastdown meadow's cairn and rock scar (#872): world dressing whose collision is
/// generated from its art ([`dressing_meshes`]).
pub const EASTDOWN_ROCKS: std::ops::RangeInclusive<u32> = 65138..=65145;

/// The sandstone steading in the Highland Stones (#683.Q1): one collider per placed module of its
/// sandstone cottage and workshop, then its crag-kit rock scar; world dressing whose collision is
/// generated from its art ([`dressing_meshes`]).
pub const SANDSTONE_STEADING: std::ops::RangeInclusive<u32> = 65200..=65258;

/// Leaning rock slabs and their smaller talus pieces at the basin cliff feet: world dressing
/// whose collision is generated from its art ([`dressing_meshes`], #683).
pub const BASIN_ROCKS: std::ops::RangeInclusive<u32> = 65094..=65102;

/// The three climbable crags on the basin rim: world dressing whose collision is generated
/// from its art ([`dressing_meshes`], #683), climbable by their placements' grip
/// ([`dressing_grips`], #1195).
pub const BASIN_CRAGS: std::ops::RangeInclusive<u32> = 65030..=65032;

/// The committed collision generated from the world dressing's art (#683): each placed rock's
/// asset collision mesh, scaled and turned by its placement by
/// `content/greyhaven/authoring/dressing_collision.py`. The zone reads only this reviewed
/// output, never the art, so a new art release changes no collision until it is regenerated.
/// Its `generator` and `sources` (the descriptor and bundle digests) are for review only.
#[derive(serde::Deserialize)]
struct Generated {
    meshes: Vec<GeneratedMesh>,
}

/// One generated mesh with its placement's climbable or sheer setting (#1195).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GeneratedMesh {
    id: u32,
    position: [i32; 3],
    vertices: Vec<[i32; 3]>,
    triangles: Vec<[u32; 3]>,
    #[serde(default)]
    grip: Option<DressingGrip>,
}

/// A dressing placement's `grip` (#1195): rough rock climbs, sheer rock does not.
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum DressingGrip {
    Rock,
    Sheer,
}

fn generated() -> Generated {
    serde_json::from_str(include_str!(
        "../../../content/collision/greyhaven-dressing.json"
    ))
    .expect("committed Greyhaven dressing collision is valid")
}

/// The world dressing's generated collision meshes, by collider ID (#683).
#[must_use]
pub fn dressing_meshes() -> Vec<StaticMesh> {
    generated()
        .meshes
        .into_iter()
        .map(|mesh| StaticMesh {
            id: mesh.id,
            position: mesh.position,
            vertices: mesh.vertices,
            triangles: mesh.triangles,
        })
        .collect()
}

/// The climbable world dressing (#1195): every generated collider whose placement sets
/// `"grip": "rock"`, with that grip. The setting lives with the art's placement, not in a
/// collider-ID range, so a rock's climbability is reviewed with its layout.
#[must_use]
pub fn dressing_grips() -> Vec<(u32, crate::Grip)> {
    generated()
        .meshes
        .into_iter()
        .filter_map(|mesh| match mesh.grip? {
            DressingGrip::Rock => Some((mesh.id, crate::Grip::Rock)),
            DressingGrip::Sheer => None,
        })
        .collect()
}

#[must_use]
pub fn meshes() -> Vec<StaticMesh> {
    let meshes: Vec<StaticMesh> = serde_json::from_str(include_str!(
        "../../../content/collision/greyhaven-fort.json"
    ))
    .expect("committed Greyhaven fortification geometry is valid");
    meshes.into_iter().chain(dressing_meshes()).collect()
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
        || BROOK_MILL.contains(&id)
        || REDBRAND_PALISADES.contains(&id)
        || REDBRAND_SHELTERS.contains(&id)
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
