//! The village basin's rim rock band (#1195 part 4, owner decision Q1 = A): one continuous
//! sculpted band of rock along the basin's outline, in closed segments of about 20 m. Each
//! segment collides as its own authored mesh and climbs by its authored `grip`: rough rock
//! climbs, and the cliff under Stillwater's terrace ([`super::SHEER_LAKE_CLIFF`]) is sheer.
//!
//! The terrain under the band only steps from the valley floor to the rim
//! ([`super::basin::rock_q10`]); the band stands over that step, so its face is the cliff the
//! player sees and climbs, with the six climbs' ledges, landings and cuttings left open as
//! terrain lanes cut into it. `content/greyhaven/authoring/basin_rim.py` generates
//! `content/collision/basin-rim.json` from the terrain and the lanes.

use crate::{Grip, StaticCollider, StaticMesh};
use std::sync::LazyLock;

/// One persisted segment of the band.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Persisted {
    id: u32,
    position: [i32; 3],
    vertices: Vec<[i32; 3]>,
    triangles: Vec<[u32; 3]>,
    grip: RockGrip,
}

/// A segment's `grip`: rough rock climbs, sheer rock does not.
#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum RockGrip {
    Rock,
    Sheer,
}

static SEGMENTS: LazyLock<Vec<(StaticMesh, RockGrip, StaticCollider)>> = LazyLock::new(|| {
    let persisted: Vec<Persisted> =
        serde_json::from_str(include_str!("../../../content/collision/basin-rim.json"))
            .expect("committed basin rim collision JSON is valid");
    persisted
        .into_iter()
        .map(|segment| {
            let mesh = StaticMesh {
                id: segment.id,
                position: segment.position,
                vertices: segment.vertices,
                triangles: segment.triangles,
            };
            let bounds = mesh
                .bounds()
                .expect("committed basin rim segments have bounds");
            (mesh, segment.grip, bounds)
        })
        .collect()
});

/// The band's segments, in order round the basin.
#[must_use]
pub fn collision_meshes() -> Vec<StaticMesh> {
    SEGMENTS.iter().map(|(mesh, ..)| mesh.clone()).collect()
}

/// The climbable segments: every one whose authored `grip` is `rock`.
#[must_use]
pub fn climbable_rock() -> Vec<(u32, Grip)> {
    SEGMENTS
        .iter()
        .filter_map(|(mesh, grip, _)| match grip {
            RockGrip::Rock => Some((mesh.id, Grip::Rock)),
            RockGrip::Sheer => None,
        })
        .collect()
}

/// Whether static mesh `id` is one of the band's segments.
#[must_use]
pub fn is_segment(id: u32) -> bool {
    super::ids::BASIN_RIM.contains(&id)
}

/// Whether the band's rock stands over any corner or the middle of the XZ rectangle
/// `[min, max]` (units): scenery keeps its scatter off it.
#[must_use]
pub fn covers(min: [i32; 2], max: [i32; 2]) -> bool {
    let near = |bounds: &StaticCollider| {
        min[0] <= bounds.position[0] + bounds.half_extents[0]
            && max[0] >= bounds.position[0] - bounds.half_extents[0]
            && min[1] <= bounds.position[2] + bounds.half_extents[2]
            && max[1] >= bounds.position[2] - bounds.half_extents[2]
    };
    if !SEGMENTS.iter().any(|(.., bounds)| near(bounds)) {
        return false;
    }
    let middle = [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2];
    // Under the climbs' lanes the rock lies below the ground, which stays open.
    [min, max, [min[0], max[1]], [max[0], min[1]], middle]
        .into_iter()
        .any(|[x, z]| rock_top([x, z]).is_some_and(|top| top > super::TERRAIN.height_at(x, z)))
}

/// The top of the band over `(x, z)` (units), if it stands there: the highest upward face of
/// its segments. Scenery keeps its scatter off it, and tests find its faces with it.
#[must_use]
pub fn rock_top([x, z]: [i32; 2]) -> Option<i32> {
    SEGMENTS
        .iter()
        .filter(|(.., bounds)| {
            (x - bounds.position[0]).abs() <= bounds.half_extents[0]
                && (z - bounds.position[2]).abs() <= bounds.half_extents[2]
        })
        .filter_map(|(mesh, ..)| super::surface_at(mesh, [x, z]))
        .max()
}
