//! Stillwater's Sunken Shrine, placed with its lake northwest of Greyhaven.
//! The authored module grid stays immutable; one core-owned half-turn places its
//! collision, water, areas and client kit on the western shore with stable IDs.

pub mod authored;
use super::{ids, lake};
use crate::{AirPocket, StaticCollider, WaterVolume, XzBounds};
pub use authored::{CELL, CLEAR, FLOOR_Y, ROOM_CLEAR, SHORE_Y, SUNKEN_SHRINE, facing};

pub const PLAZA: ([i32; 2], [i32; 2]) = lake::place_bounds(authored::PLAZA);
pub const CAVE: ([i32; 2], [i32; 2]) = lake::place_bounds(authored::CAVE);
pub const CAVE_Z: i32 = lake::place([0, authored::CAVE_Z])[1];
pub const ALTAR_CHAMBER: ([i32; 2], [i32; 2]) = lake::place_bounds(authored::ALTAR_CHAMBER);
pub const ROOM: ([i32; 2], [i32; 2]) = lake::place_bounds(authored::ROOM);
pub const AIR_POCKET: AirPocket = {
    let (min, max) = lake::place_bounds((
        [authored::AIR_POCKET.min[0], authored::AIR_POCKET.min[2]],
        [authored::AIR_POCKET.max[0], authored::AIR_POCKET.max[2]],
    ));
    AirPocket {
        min: [min[0], authored::AIR_POCKET.min[1], min[1]],
        max: [max[0], authored::AIR_POCKET.max[1], max[1]],
    }
};

#[must_use]
pub fn flattens(x: i32, z: i32) -> bool {
    let [x, z] = lake::place([x, z]);
    authored::flattens(x, z)
}

#[must_use]
pub fn cave_water() -> WaterVolume {
    let mut water = authored::cave_water();
    water.centre = lake::place(water.centre);
    water.air_pockets = vec![AIR_POCKET];
    water
}

#[must_use]
pub fn ruins(first_id: u32) -> Vec<StaticCollider> {
    authored::ruins(first_id)
        .into_iter()
        .map(placed_collider)
        .collect()
}

#[must_use]
pub fn walls(first_id: u32) -> Vec<StaticCollider> {
    authored::walls(first_id)
        .into_iter()
        .map(placed_collider)
        .collect()
}

fn placed_collider(mut collider: StaticCollider) -> StaticCollider {
    collider.position = lake::place3(collider.position);
    collider
}

#[must_use]
pub fn collision_meshes() -> Vec<crate::StaticMesh> {
    authored::collision_meshes()
        .into_iter()
        .map(lake::place_mesh)
        .collect()
}

pub(super) const REGION: super::Region = super::Region {
    areas: AREAS,
    roads: &[],
    colliders,
};
fn colliders() -> Vec<StaticCollider> {
    ruins(*ids::SHRINE_RUINS.start())
        .into_iter()
        .chain(walls(*ids::SHRINE_CAVE_WALLS.start()))
        .collect()
}
pub const AREAS: &[(crate::AreaId, &str, XzBounds)] = &[(
    SUNKEN_SHRINE,
    "Sunken Shrine",
    XzBounds {
        min: ALTAR_CHAMBER.0,
        max: ALTAR_CHAMBER.1,
    },
)];
