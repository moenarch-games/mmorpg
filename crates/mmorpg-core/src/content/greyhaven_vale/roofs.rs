//! Authoritative roofs for the two walk-in buildings. Their presentation hides the roof and
//! ceiling indoors, but the zone must keep both solid. Each closed gable prism follows the
//! authored roof's slope and fills its inaccessible attic down to the ceiling underside.

use super::{ids, inn, marshal_house};
use crate::StaticMesh;

/// A closed pentagonal prism in the house's frame. The lower face is the ceiling, and the two
/// pitched faces are the walkable (when reachable) outside roof; no oversized AABB is physical.
fn gable(
    id: u32,
    origin: [i32; 3],
    half_x: i32,
    half_z: i32,
    ceiling: i32,
    eave: i32,
    ridge: i32,
    half_turn: bool,
) -> StaticMesh {
    let profile = [
        [-half_z, ceiling],
        [half_z, ceiling],
        [half_z, eave],
        [0, ridge],
        [-half_z, eave],
    ];
    let vertices = [-half_x, half_x]
        .into_iter()
        .flat_map(|x| {
            profile.into_iter().map(
                move |[z, y]| {
                    if half_turn { [-x, y, -z] } else { [x, y, z] }
                },
            )
        })
        .collect();
    let mut triangles = Vec::with_capacity(16);
    for i in 1..4 {
        triangles.push([0, i, i + 1]);
        triangles.push([5, 5 + i + 1, 5 + i]);
    }
    for i in 0..5 {
        let next = (i + 1) % 5;
        triangles.push([i, 5 + i, 5 + next]);
        triangles.push([i, 5 + next, next]);
    }
    StaticMesh {
        id,
        position: origin,
        vertices,
        triangles,
    }
}

pub(super) fn meshes() -> [StaticMesh; 2] {
    [
        // Inn: 14 × 10 m eaves, 3.0 m ceiling, 6.0 m ridge (asset-tooling/inn.py).
        gable(
            ids::INN_ROOF,
            inn::ORIGIN,
            700,
            500,
            inn::CEILING_UNDERSIDE,
            321,
            600,
            false,
        ),
        // Elden's home: the 8 × 6 m Outpost roof with 0.35 m eaves and 0.3 m verges.
        // Its ceiling is at 2.95 m, the ridge at 5.52 m; the house is rotated 180°.
        gable(
            ids::MARSHAL_HOUSE_ROOF,
            marshal_house::ORIGIN,
            430,
            335,
            marshal_house::CEILING_UNDERSIDE,
            350,
            552,
            true,
        ),
    ]
}
