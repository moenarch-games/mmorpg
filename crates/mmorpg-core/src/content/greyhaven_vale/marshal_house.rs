//! Marshal Elden Greywatch's private home (#158, Atlas 5.1.48): asset-tooling's rolling
//! `greyhaven-marshal-house` building on house 103's 8 × 6 m footprint, entered through a
//! hinged door. Boxes are the bundle descriptor's wall and furniture colliders (the visual floor
//! lies on the flat ground and adds no collider), in units in the house frame: origin at the
//! footprint's base centre, door on the +Z front. [`ORIGIN`] places the frame at collider 103's
//! centre, turned half a turn so the door faces the square to the north (−Z).

use crate::{Door, DoorBox, DoorId, NpcId, PrivateHome, StaticCollider};

/// The house frame's origin in the zone, in units.
pub const ORIGIN: [i32; 3] = [-2_400, 0, 3_600];
/// The footprint's half extents (8 × 5 × 6 m), for presentation and relief.
pub const FOOTPRINT_HALF_EXTENTS: [i32; 3] = [400, 250, 300];
/// The house's front door.
pub const DOOR: DoorId = DoorId::new(2);
/// Its resident.
pub const RESIDENT: NpcId = NpcId::new(1);

/// `(min, max)` corners in the house frame, in units: furniture, then walls.
#[rustfmt::skip]
const BOXES: [([i32; 3], [i32; 3]); 16] = [
    // furniture barrel
    ([294, 0, -254], [354, 90, -194]),
    // furniture bed
    ([-364, 0, -264], [-254, 100, -54]),
    // furniture bench
    ([95, 0, -215], [225, 47, -185]),
    // furniture chair
    ([137, 0, -62], [183, 98, -16]),
    // furniture chest
    ([-352, 0, -48], [-266, 56, 2]),
    // furniture hearth
    ([272, 0, -92], [368, 295, 92]),
    // furniture log-stack
    ([305, 0, 104], [360, 40, 149]),
    // furniture table
    ([90, 0, -160], [230, 106, -80]),
    // furniture weapon-rack
    ([-366, 0, 95], [-330, 243, 225]),
    // wall back
    ([-400, 0, -300], [400, 320, -268]),
    // wall chimney, from the right wall's outer face (the descriptor's box reaches 8 cm into it)
    ([400, 0, -60], [482, 600, 60]),
    // wall front.left
    ([-400, 0, 268], [-60, 320, 300]),
    // wall front.lintel
    ([-60, 220, 268], [60, 320, 300]),
    // wall front.right
    ([60, 0, 268], [400, 320, 300]),
    // wall left
    ([-400, 0, -268], [-368, 320, 268]),
    // wall right
    ([368, 0, -268], [400, 320, 268]),
];

/// A house-frame XZ point in the zone: the frame is turned half a turn about +Y.
const fn spot([x, z]: [i32; 2]) -> [i32; 2] {
    [ORIGIN[0] - x, ORIGIN[2] - z]
}

fn to_zone(min: [i32; 3], max: [i32; 3]) -> DoorBox {
    let [x0, z0] = spot([max[0], max[2]]);
    let [x1, z1] = spot([min[0], min[2]]);
    let (min, max) = ([x0, ORIGIN[1] + min[1], z0], [x1, ORIGIN[1] + max[1], z1]);
    DoorBox {
        position: std::array::from_fn(|axis| (min[axis] + max[axis]) / 2),
        half_extents: std::array::from_fn(|axis| (max[axis] - min[axis]) / 2),
    }
}

/// The house's wall and furniture colliders, with IDs from `first_id`.
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
        closed: to_zone([-59, 1, 282], [58, 219, 296]),
        open: to_zone([-64, 1, 172], [-50, 219, 289]),
    }
}

/// Outside the door, facing it: where a knock is heard (the descriptor's doorstep spot).
pub const DOORSTEP: [i32; 2] = spot([0, 360]);
/// Just inside the doorway, where Elden answers a knock (the descriptor's doorway spot).
pub const DOORWAY: [i32; 2] = spot([0, 218]);
/// Beside the bed, where Elden sleeps (the descriptor's bedside spot).
pub const BEDSIDE: [i32; 2] = spot([-210, -160]);
/// One step in from the doorway, clear of the open leaf.
const HALL: [i32; 2] = spot([0, 120]);

/// Elden's walk home at dusk from his post south of the keep: down past the tannery to his
/// doorstep, through the door and to his bed (#158, Atlas 5.1.45).
pub(super) fn path() -> Vec<[i32; 2]> {
    vec![[-2_200, 3_100], DOORSTEP, DOORWAY, HALL, BEDSIDE]
}

/// Facing the bed (house −X, zone +X).
pub(super) const BEDSIDE_FACING: u16 = 16_384;

/// Elden's home: a knock wakes him to the doorway, facing out through the door (zone −Z).
pub(super) fn home() -> PrivateHome {
    PrivateHome {
        npc: RESIDENT,
        door: DOOR,
        wake_path: vec![HALL, DOORWAY],
        doorway_facing: 32_768,
        woken_line: "Mm? Who's hammering at this hour... Ah, it's you. If it's news, out with it, \
                     and quickly. An old soldier needs his sleep."
            .into(),
    }
}
