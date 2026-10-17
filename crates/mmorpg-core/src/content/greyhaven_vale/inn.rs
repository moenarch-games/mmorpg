//! The Greyhaven inn (#158): asset-tooling's `greyhaven-inn-v1` building, walk-in through a
//! hinged door. Boxes are the bundle descriptor's wall and furniture colliders (the visual floor
//! lies on the flat ground and adds no collider), in units in the inn frame: origin at the
//! footprint's base centre, door on the +Z front. [`ORIGIN`] places the frame at the former
//! inn block's centre, unrotated, so the door faces the square.

use crate::{Door, DoorBox, DoorId, StaticCollider};

/// The inn frame's origin in the zone, in units.
pub const ORIGIN: [i32; 3] = [1_500, 0, 0];
/// The building's footprint half extents (14 × 6 × 10 m), for presentation and relief.
pub const FOOTPRINT_HALF_EXTENTS: [i32; 3] = [700, 300, 500];
/// The inn's front door.
pub const DOOR: DoorId = DoorId::new(1);

/// `(min, max)` corners in the inn frame, in units: walls, then furniture.
#[rustfmt::skip]
const BOXES: [([i32; 3], [i32; 3]); 32] = [
    // furniture back-shelf
    ([240, 0, -450], [630, 230, -418]),
    // furniture bar-counter.1
    ([220, 0, -340], [600, 112, -280]),
    // furniture bar-counter.2
    ([220, 0, -280], [280, 112, -160]),
    // furniture barrel.1
    ([563, 0, -227], [637, 90, -153]),
    // furniture barrel.2
    ([563, 0, -147], [637, 90, -73]),
    // furniture barrel.3
    ([-637, 0, 363], [-563, 90, 437]),
    // furniture barrel.4
    ([-562, 0, 368], [-488, 90, 442]),
    // furniture chair.1.1
    ([-490, 0, -304], [-446, 96, -260]),
    // furniture chair.1.2
    ([-414, 0, -304], [-370, 96, -260]),
    // furniture chair.1.3
    ([-490, 0, -180], [-446, 96, -136]),
    // furniture chair.1.4
    ([-414, 0, -180], [-370, 96, -136]),
    // furniture chair.2.1
    ([-490, 0, 56], [-446, 96, 100]),
    // furniture chair.2.2
    ([-414, 0, 56], [-370, 96, 100]),
    // furniture chair.2.3
    ([-490, 0, 180], [-446, 96, 224]),
    // furniture chair.2.4
    ([-414, 0, 180], [-370, 96, 224]),
    // furniture chair.3.1
    ([-220, 0, -204], [-176, 96, -160]),
    // furniture chair.3.2
    ([-144, 0, -204], [-100, 96, -160]),
    // furniture chair.3.3
    ([-220, 0, -80], [-176, 96, -36]),
    // furniture chair.3.4
    ([-144, 0, -80], [-100, 96, -36]),
    // furniture hearth
    ([-650, 0, -100], [-590, 126, 100]),
    // furniture stool.1
    ([301, 0, -254], [339, 72, -216]),
    // furniture stool.2
    ([401, 0, -254], [439, 72, -216]),
    // furniture stool.3
    ([501, 0, -254], [539, 72, -216]),
    // furniture table.1
    ([-500, 0, -260], [-360, 76, -180]),
    // furniture table.2
    ([-500, 0, 100], [-360, 76, 180]),
    // furniture table.3
    ([-230, 0, -160], [-90, 76, -80]),
    // wall back
    ([-680, 0, -480], [680, 320, -450]),
    // wall front.left
    ([-680, 0, 450], [-60, 320, 480]),
    // wall front.lintel
    ([-60, 220, 450], [60, 320, 480]),
    // wall front.right
    ([60, 0, 450], [680, 320, 480]),
    // wall left
    ([-680, 0, -450], [-650, 320, 450]),
    // wall right
    ([650, 0, -450], [680, 320, 450]),
];

fn to_zone(min: [i32; 3], max: [i32; 3]) -> DoorBox {
    DoorBox {
        position: std::array::from_fn(|axis| ORIGIN[axis] + (min[axis] + max[axis]) / 2),
        half_extents: std::array::from_fn(|axis| (max[axis] - min[axis]) / 2),
    }
}

/// The inn's wall and furniture colliders, with IDs from `first_id`.
pub(super) fn colliders(first_id: u32) -> Vec<StaticCollider> {
    BOXES
        .iter()
        .zip(first_id..)
        .map(|(&(min, max), id)| {
            let leaf = to_zone(min, max);
            StaticCollider {
                id,
                position: leaf.position,
                half_extents: leaf.half_extents,
            }
        })
        .collect()
}

/// The front door: closed in the doorway, open swung inward against the left jamb.
pub(super) fn door(body: u32) -> Door {
    Door {
        id: DOOR,
        body,
        closed: to_zone([-59, 2, 442], [59, 220, 464]),
        open: to_zone([-71, 2, 334], [-49, 220, 452]),
    }
}
