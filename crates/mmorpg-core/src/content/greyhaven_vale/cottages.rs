//! Cottages you can walk into (#843): every 8 × 6 m settlement house (#818) is the Outpost kit's
//! timber-framed house (`content/greyhaven/dressing/settlements.json`), and its body is no
//! longer one solid box but the kit's walls round an open doorway and its furniture: a bed, a
//! hearth, a table with a chair and a chest (asset-tooling's `settlement.props`).
//!
//! Coordinates are in the cottage frame: origin at the footprint's floor centre, the doorway in
//! the +Z wall. A cottage's door faces its settlement's yard: the frame turns a half turn when
//! the yard lies to the north.

use super::{footing, ids, settlements};
use crate::StaticCollider;

/// The footprint's half extents, in units.
pub const HALF: [i32; 2] = [400, 300];
/// The kit's walls: their thickness inside the footprint's edge and their height.
pub const WALL: i32 = 32;
pub const WALL_HEIGHT: i32 = 360;
/// The doorway in the +Z wall: half its width and its height.
pub const DOOR_HALF: i32 = 60;
pub const DOOR_HEIGHT: i32 = 220;
/// Collider IDs each cottage reserves in [`ids::COTTAGES`].
const PARTS: u32 = 16;

/// One cottage: the settlement house collider it replaces, its frame and its floor height.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cottage {
    pub house: u32,
    pub centre: [i32; 2],
    pub floor: i32,
    /// Quarter turns about +Y from the frame to the zone: 0 or 2.
    pub quarters: u8,
}

/// One piece of furniture: its `settlement.props` asset, its frame position, its own quarter
/// turns and its collider boxes `(min, max)` in the frame.
pub struct Furniture {
    pub asset: &'static str,
    pub position: [i32; 2],
    pub quarters: u8,
    pub boxes: &'static [([i32; 3], [i32; 3])],
}

/// The furniture, the same in every cottage: the bed in the back left corner with its head to
/// the back wall, the hearth on the back wall, the table and its chair before it, the chest on
/// the left wall. The way from the doorway to the room's middle stays clear.
pub const FURNITURE: [Furniture; 5] = [
    Furniture {
        asset: "settlement.props.bed",
        position: [-318, -168],
        quarters: 0,
        boxes: &[
            ([-368, 0, -268], [-268, 55, -68]),
            ([-368, 55, -268], [-268, 95, -261]),
        ],
    },
    Furniture {
        asset: "settlement.props.hearth",
        position: [180, -208],
        quarters: 0,
        boxes: &[
            ([90, 0, -268], [270, 113, -183]),
            ([110, 113, -268], [250, 260, -216]),
        ],
    },
    Furniture {
        asset: "settlement.props.table",
        position: [150, 70],
        quarters: 0,
        boxes: &[([80, 0, 30], [220, 76, 110])],
    },
    Furniture {
        asset: "settlement.props.chair",
        position: [150, 140],
        quarters: 2,
        boxes: &[([128, 0, 118], [172, 45, 162])],
    },
    Furniture {
        asset: "settlement.props.chest",
        position: [-341, 60],
        quarters: 1,
        boxes: &[([-368, 0, 14], [-314, 55, 106])],
    },
];

/// The walls round the doorway, `(min, max)` in the frame.
const WALLS: [([i32; 3], [i32; 3]); 6] = [
    (
        [-HALF[0], 0, -HALF[1]],
        [HALF[0], WALL_HEIGHT, -HALF[1] + WALL],
    ),
    (
        [-HALF[0], 0, -HALF[1] + WALL],
        [-HALF[0] + WALL, WALL_HEIGHT, HALF[1] - WALL],
    ),
    (
        [HALF[0] - WALL, 0, -HALF[1] + WALL],
        [HALF[0], WALL_HEIGHT, HALF[1] - WALL],
    ),
    (
        [-HALF[0], 0, HALF[1] - WALL],
        [-DOOR_HALF, WALL_HEIGHT, HALF[1]],
    ),
    (
        [DOOR_HALF, 0, HALF[1] - WALL],
        [HALF[0], WALL_HEIGHT, HALF[1]],
    ),
    (
        [-DOOR_HALF, DOOR_HEIGHT, HALF[1] - WALL],
        [DOOR_HALF, WALL_HEIGHT, HALF[1]],
    ),
];

/// Where a resident goes: outside the doorway, in it, a step inside, the room's middle, beside the bed (facing it)
/// and before the hearth (facing it), in the frame.
pub const DOORSTEP: [i32; 2] = [0, 380];
pub const DOORWAY: [i32; 2] = [0, 230];
pub const INSIDE: [i32; 2] = [0, 120];
/// The room's middle, between the table and the bed, from where the bed and the hearth are clear.
pub const MIDROOM: [i32; 2] = [0, -60];
pub const BEDSIDE: [i32; 2] = [-225, -160];
pub const HEARTHSIDE: [i32; 2] = [180, -130];

/// Every cottage, in settlement collider order.
#[must_use]
pub fn cottages() -> Vec<Cottage> {
    (*ids::SETTLEMENTS.start()..)
        .zip(settlements::SETTLEMENTS.iter().flat_map(|settlement| {
            settlement
                .structures
                .iter()
                .map(move |structure| (settlement.yard, *structure))
        }))
        .filter(|(_, (_, (kind, _, size, _)))| {
            *kind == settlements::Building::House && *size == [2 * HALF[0], 2 * HALF[1]]
        })
        .map(|(house, (yard, (_, centre, _, _)))| {
            let min = [centre[0] - HALF[0], centre[1] - HALF[1]];
            let max = [centre[0] + HALF[0], centre[1] + HALF[1]];
            Cottage {
                house,
                centre,
                floor: footing(min, max),
                // The doorway is on the frame's +Z side: turn it to face the yard.
                quarters: if (yard.0[1] + yard.1[1]) / 2 >= centre[1] {
                    0
                } else {
                    2
                },
            }
        })
        .collect()
}

/// The cottage, if any, whose house collider `id` is.
#[must_use]
pub fn of_house(id: u32) -> Option<Cottage> {
    cottages().into_iter().find(|cottage| cottage.house == id)
}

impl Cottage {
    /// A frame point in the zone, units.
    #[must_use]
    pub const fn spot(&self, [x, z]: [i32; 2]) -> [i32; 2] {
        let [x, z] = if self.quarters == 2 { [-x, -z] } else { [x, z] };
        [self.centre[0] + x, self.centre[1] + z]
    }

    /// A frame facing (0 toward +Z) in the zone.
    #[must_use]
    pub const fn facing(&self, facing: u16) -> u16 {
        facing.wrapping_add(self.quarters as u16 * crate::trig::YAW_QUARTER_TURN)
    }

    /// A frame box in the zone.
    fn collider(&self, id: u32, (min, max): ([i32; 3], [i32; 3])) -> StaticCollider {
        let [x0, z0] = self.spot([min[0], min[2]]);
        let [x1, z1] = self.spot([max[0], max[2]]);
        let (lo, hi) = (
            [x0.min(x1), min[1], z0.min(z1)],
            [x0.max(x1), max[1], z0.max(z1)],
        );
        StaticCollider {
            id,
            position: [
                (lo[0] + hi[0]) / 2,
                self.floor + (lo[1] + hi[1]) / 2,
                (lo[2] + hi[2]) / 2,
            ],
            half_extents: std::array::from_fn(|axis| (hi[axis] - lo[axis]) / 2),
        }
    }

    /// Its walls and furniture, with IDs from `first`.
    fn colliders(&self, first: u32) -> Vec<StaticCollider> {
        boxes()
            .into_iter()
            .zip(first..)
            .map(|(bounds, id)| self.collider(id, bounds))
            .collect()
    }
}

impl Cottage {
    /// The zone point in this cottage's frame, units.
    const fn local(&self, [x, z]: [i32; 2]) -> [i32; 2] {
        let [x, z] = [x - self.centre[0], z - self.centre[1]];
        if self.quarters == 2 { [-x, -z] } else { [x, z] }
    }

    /// Extends a resident's walk home, which ends near this cottage, round the house to its
    /// doorstep, through the doorway and to the bed (the cottage's first resident) or the
    /// hearth (its second); returns the facing there, towards the bed or the fire.
    pub fn walk_in(&self, path: &mut Vec<[i32; 2]>, second: bool) -> u16 {
        let from = path.pop().map_or(DOORSTEP, |end| self.local(end));
        let side = if from[0] < 0 { -1 } else { 1 };
        let clear = HALF[0] + 150;
        let mut local = vec![from];
        // From behind the house, along its side to the front, so no leg crosses the walls.
        if from[1] < HALF[1] + 50 && from[0].abs() <= clear + 50 {
            if from[1] < -HALF[1] {
                local.push([side * clear, from[1]]);
            }
            local.push([side * clear, DOORSTEP[1]]);
        }
        local.extend([DOORSTEP, DOORWAY, INSIDE, MIDROOM]);
        local.push(if second { HEARTHSIDE } else { BEDSIDE });
        path.extend(local.into_iter().map(|point| self.spot(point)));
        path.dedup();
        // Towards the bed (frame −X) or the fire (frame −Z).
        self.facing(if second { 32_768 } else { 49_152 })
    }
}

/// The cottage, if any, whose doorstep a walk home ending at `end` comes to: within 9 m.
#[must_use]
pub fn home_of(end: [i32; 2]) -> Option<Cottage> {
    cottages().into_iter().find(|cottage| {
        let [x, z] = cottage.spot(DOORSTEP);
        (i64::from(x - end[0])).pow(2) + (i64::from(z - end[1])).pow(2) < 900 * 900
    })
}

/// Every cottage's walls and furniture, [`PARTS`] IDs apart in [`ids::COTTAGES`].
pub(super) fn colliders() -> Vec<StaticCollider> {
    cottages()
        .iter()
        .zip((*ids::COTTAGES.start()..).step_by(PARTS as usize))
        .flat_map(|(cottage, first)| cottage.colliders(first))
        .collect()
}

/// Whether collider `id` is part of a cottage.
#[must_use]
pub fn is_cottage(id: u32) -> bool {
    ids::COTTAGES.contains(&id)
}

/// Whether settlement collider `id` is a cottage's house, which no longer collides as a box.
#[must_use]
pub fn is_house(id: u32) -> bool {
    of_house(id).is_some()
}

/// The walls' and furniture's boxes `(min, max)` in the frame, in collider order.
#[must_use]
pub fn boxes() -> Vec<([i32; 3], [i32; 3])> {
    WALLS
        .iter()
        .copied()
        .chain(
            FURNITURE
                .iter()
                .flat_map(|piece| piece.boxes.iter().copied()),
        )
        .collect()
}
