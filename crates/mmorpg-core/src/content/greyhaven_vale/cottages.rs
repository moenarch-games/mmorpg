//! Cottages you can walk into (#843): every 8 × 6 m house of the settlements (#818) and of the
//! farmland's hamlets (#424, #847) is the Outpost kit's timber-framed house
//! (`content/greyhaven/dressing/settlements.json`), and its body is no longer one solid box but the
//! kit's walls round an open doorway and its furniture: a bed, a hearth, a table with a chair and
//! a chest (asset-tooling's `settlement.props`).
//!
//! The six 8 × 7 m farmhouses (#886) — the Barrow Rest, the Ford Inn, Millbrook's and the three
//! hamlets' — are the kit's two-room farmhouse ([`Plan::Farmhouse`]): a hall with the hearth on
//! its −X gable and, through the partition's doorway at the back, a chamber with the bed.
//!
//! Coordinates are in the cottage frame: origin at the footprint's floor centre, the doorway in
//! the +Z wall. A settlement cottage's door faces its settlement's yard: the frame turns a half turn when
//! the yard lies to the north; a hamlet cottage's faces [`HAMLET_QUARTERS`], a farmhouse's
//! [`FARMHOUSES`].

use super::{farmland, ids, lowest_corner, settlements, vale};
use crate::StaticCollider;

/// A cottage's footprint half extents, in units.
pub const HALF: [i32; 2] = [400, 300];
/// A farmhouse's (#886).
pub const FARMHOUSE_HALF: [i32; 2] = [400, 350];
/// The kit's walls: their thickness inside the footprint's edge and their height.
pub const WALL: i32 = 32;
pub const WALL_HEIGHT: i32 = 360;
/// The doorway in the +Z wall: half its width and its height.
pub const DOOR_HALF: i32 = 60;
pub const DOOR_HEIGHT: i32 = 220;
/// The farmhouse's partition (asset-tooling's `partition-door`): its middle on X, half its
/// thickness, its doorway's Z span and its doorway's height.
pub const PARTITION_X: i32 = 100;
pub const PARTITION_HALF: i32 = 7;
pub const PARTITION_DOOR: [i32; 2] = [-240, -140];
pub const PARTITION_DOOR_HEIGHT: i32 = 225;

/// What a walk-in house is built as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Plan {
    /// The 8 × 6 m one-room cottage (#843).
    Cottage,
    /// The 8 × 7 m two-room farmhouse (#886).
    Farmhouse,
}

/// One cottage: the settlement house collider it replaces, its frame and its floor height.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cottage {
    pub house: u32,
    pub centre: [i32; 2],
    /// The floor stands on the footprint's lowest corner, not sunk below it as a solid box is: on
    /// a slope the ground inside then rises a little above it, but never to the lintel.
    pub floor: i32,
    /// Quarter turns about +Y from the frame to the zone: 0 or 2.
    pub quarters: u8,
    pub plan: Plan,
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

/// A farmhouse's furniture (#886), placed clear of the walk from the doorway down the hall's
/// middle, through the partition's doorway and up the chamber (checked with `clear-spot
/// --indoors`). The hall: a table between two benches on the left, firewood beside the hearth,
/// a barrel in the front corner. The chamber: the bed along the +X gable with its head to the
/// front wall, a chest at its foot.
pub const FARMHOUSE_FURNITURE: [Furniture; 7] = [
    Furniture {
        asset: "settlement.props.table",
        position: [-190, 170],
        quarters: 0,
        boxes: &[([-260, 0, 130], [-120, 76, 210])],
    },
    Furniture {
        asset: "settlement.props.bench",
        position: [-190, 250],
        quarters: 2,
        boxes: &[([-270, 0, 229], [-110, 46, 271])],
    },
    Furniture {
        asset: "settlement.props.bench",
        position: [-190, 90],
        quarters: 0,
        boxes: &[([-270, 0, 69], [-110, 46, 111])],
    },
    Furniture {
        asset: "settlement.props.firewood",
        position: [-320, -200],
        quarters: 1,
        boxes: &[([-362, 0, -287], [-278, 114, -113])],
    },
    Furniture {
        asset: "settlement.props.barrel",
        position: [-335, 270],
        quarters: 0,
        boxes: &[([-366, 0, 239], [-304, 90, 301])],
    },
    Furniture {
        asset: "settlement.props.bed",
        position: [318, 218],
        quarters: 2,
        boxes: &[
            ([268, 0, 118], [368, 55, 318]),
            ([268, 55, 311], [368, 95, 318]),
        ],
    },
    Furniture {
        asset: "settlement.props.chest",
        position: [318, 60],
        quarters: 0,
        boxes: &[([272, 0, 34], [364, 55, 86])],
    },
];

/// A cottage's walls round the doorway, `(min, max)` in the frame.
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

/// A farmhouse's walls (#886), as the kit modules' collision proxies: the outer walls round the
/// front doorway, the hearth standing out of the −X gable (`wall-hearth`), and the partition
/// either side of and over its doorway (`partition-door`). Both doorways stay free.
const FARMHOUSE_WALLS: [([i32; 3], [i32; 3]); 10] = {
    let [hx, hz] = FARMHOUSE_HALF;
    let [x0, x1] = [PARTITION_X - PARTITION_HALF, PARTITION_X + PARTITION_HALF];
    let [d0, d1] = PARTITION_DOOR;
    [
        ([-hx, 0, -hz], [hx, WALL_HEIGHT, -hz + WALL]),
        ([-hx, 0, -hz + WALL], [-hx + WALL, WALL_HEIGHT, hz - WALL]),
        ([hx - WALL, 0, -hz + WALL], [hx, WALL_HEIGHT, hz - WALL]),
        ([-hx, 0, hz - WALL], [-DOOR_HALF, WALL_HEIGHT, hz]),
        ([DOOR_HALF, 0, hz - WALL], [hx, WALL_HEIGHT, hz]),
        (
            [-DOOR_HALF, DOOR_HEIGHT, hz - WALL],
            [DOOR_HALF, WALL_HEIGHT, hz],
        ),
        // The hearth: 1.8 m wide, from the gable's inner face to 0.84 m inside its outer face.
        ([-hx + WALL, 0, -90], [-hx + 84, WALL_HEIGHT, 90]),
        ([x0, 0, d1], [x1, WALL_HEIGHT, hz - WALL]),
        ([x0, 0, -hz + WALL], [x1, WALL_HEIGHT, d0]),
        ([x0, PARTITION_DOOR_HEIGHT, d0], [x1, WALL_HEIGHT, d1]),
    ]
};

/// Where a resident goes: outside the doorway, in it, a step inside, the room's middle, beside the bed (facing it)
/// and before the hearth (facing it), in the frame.
pub const DOORSTEP: [i32; 2] = [0, 380];
pub const DOORWAY: [i32; 2] = [0, 230];
pub const INSIDE: [i32; 2] = [0, 120];
/// The room's middle, between the table and the bed, from where the bed and the hearth are clear.
pub const MIDROOM: [i32; 2] = [0, -60];
pub const BEDSIDE: [i32; 2] = [-225, -160];
pub const HEARTHSIDE: [i32; 2] = [180, -130];

/// A farmhouse's (#886): the doorstep, doorway and a step inside; the first resident walks down
/// the hall, through the partition's doorway and up beside the bed, the second to the hearth.
pub const FARMHOUSE_DOORSTEP: [i32; 2] = [0, 430];
const FARMHOUSE_DOORWAY: [i32; 2] = [0, 280];
const FARMHOUSE_INSIDE: [i32; 2] = [0, 170];
pub const FARMHOUSE_TO_BED: [[i32; 2]; 3] = [[0, -190], [170, -190], [215, 200]];
pub const FARMHOUSE_TO_HEARTH: [[i32; 2]; 2] = [[0, 0], [-240, 0]];

/// The farmland hamlets' cottages' quarter turns, in [`farmland::BUILDINGS`] order: each door faces
/// the hamlet's lane or its resident's way home (the southern hamlet's west cottage, to the south).
const HAMLET_QUARTERS: [u8; 9] = [0, 0, 0, 0, 0, 0, 0, 0, 2];

/// The farmhouses (#886): each house collider, its footprint `(min, max)` and its door's quarter
/// turns. The Barrow Rest's and Millbrook's doors face north, to the yard and to the farmer's
/// post; the Ford Inn's south to its yard; the hamlets' to their wells and lanes.
fn farmhouses() -> [(u32, [i32; 2], [i32; 2], u8); 6] {
    let settlement = |name| {
        (*ids::SETTLEMENTS.start()..)
            .zip(settlements::SETTLEMENTS.iter().flat_map(|s| s.structures))
            .find(|(_, (kind, centre, ..))| {
                *kind == settlements::Building::Farmhouse && *centre == name
            })
            .map(|(id, &(_, [x, z], [w, d], _))| {
                (id, [x - w / 2, z - d / 2], [x + w / 2, z + d / 2])
            })
            .expect("settlement farmhouse")
    };
    let hamlet = |index: usize| {
        let (id, &(_, min, max, _)) = (*ids::FARMLAND_BUILDINGS.start()..)
            .zip(farmland::BUILDINGS)
            .filter(|(_, (kind, ..))| *kind == farmland::Building::Farmhouse)
            .nth(index)
            .expect("hamlet farmhouse");
        (id, min, max)
    };
    let millbrook = vale::STRUCTURES
        .iter()
        .find(|(id, ..)| *id == ids::FARMHOUSE)
        .map(|&(_, min, max, _)| (min, max))
        .expect("Millbrook farmhouse");
    let [barrow, ford] = [[56_400, 67_400], [106_000, 20_000]].map(settlement);
    let [west, east, south] = [0, 1, 2].map(hamlet);
    [
        (barrow.0, barrow.1, barrow.2, 2),
        (ford.0, ford.1, ford.2, 0),
        (ids::FARMHOUSE, millbrook.0, millbrook.1, 2),
        (west.0, west.1, west.2, 0),
        (east.0, east.1, east.2, 0),
        (south.0, south.1, south.2, 2),
    ]
}

/// Every walk-in house: the settlements' cottages, in settlement collider order, then the
/// hamlets', then the farmhouses.
#[must_use]
pub fn cottages() -> Vec<Cottage> {
    let mut cottages = settlement_cottages();
    let houses = (*ids::FARMLAND_BUILDINGS.start()..)
        .zip(farmland::BUILDINGS)
        .filter(|(_, (kind, min, max, _))| {
            *kind == farmland::Building::House
                && [max[0] - min[0], max[1] - min[1]] == [2 * HALF[0], 2 * HALF[1]]
        });
    cottages.extend(
        houses
            .zip(HAMLET_QUARTERS)
            .map(|((house, &(_, min, max, _)), quarters)| Cottage {
                house,
                centre: [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2],
                floor: lowest_corner(min, max),
                quarters,
                plan: Plan::Cottage,
            }),
    );
    cottages.extend(
        farmhouses()
            .into_iter()
            .map(|(house, min, max, quarters)| Cottage {
                house,
                centre: [(min[0] + max[0]) / 2, (min[1] + max[1]) / 2],
                floor: lowest_corner(min, max),
                quarters,
                plan: Plan::Farmhouse,
            }),
    );
    cottages
}

fn settlement_cottages() -> Vec<Cottage> {
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
                floor: lowest_corner(min, max),
                // The doorway is on the frame's +Z side: turn it to face the yard.
                quarters: if (yard.0[1] + yard.1[1]) / 2 >= centre[1] {
                    0
                } else {
                    2
                },
                plan: Plan::Cottage,
            }
        })
        .collect()
}

/// The cottage, if any, whose house collider `id` is.
#[must_use]
pub fn of_house(id: u32) -> Option<Cottage> {
    cottages().into_iter().find(|cottage| cottage.house == id)
}

impl Plan {
    /// The footprint's half extents, units.
    #[must_use]
    pub const fn half(self) -> [i32; 2] {
        match self {
            Self::Cottage => HALF,
            Self::Farmhouse => FARMHOUSE_HALF,
        }
    }

    /// The Outpost kit composition it is built from (`outpost-buildings.json`).
    #[must_use]
    pub const fn template(self) -> &'static str {
        match self {
            Self::Cottage => "house",
            Self::Farmhouse => "farmhouse",
        }
    }

    /// Its furniture.
    #[must_use]
    pub const fn furniture(self) -> &'static [Furniture] {
        match self {
            Self::Cottage => &FURNITURE,
            Self::Farmhouse => &FARMHOUSE_FURNITURE,
        }
    }

    /// The walls' and furniture's boxes `(min, max)` in the frame, in collider order.
    #[must_use]
    pub fn boxes(self) -> Vec<([i32; 3], [i32; 3])> {
        let walls: &[_] = match self {
            Self::Cottage => &WALLS,
            Self::Farmhouse => &FARMHOUSE_WALLS,
        };
        walls
            .iter()
            .copied()
            .chain(
                self.furniture()
                    .iter()
                    .flat_map(|piece| piece.boxes.iter().copied()),
            )
            .collect()
    }

    /// The doorstep, in the frame.
    #[must_use]
    pub const fn doorstep(self) -> [i32; 2] {
        match self {
            Self::Cottage => DOORSTEP,
            Self::Farmhouse => FARMHOUSE_DOORSTEP,
        }
    }

    /// Collider IDs each house reserves, and their range.
    const fn ids(self) -> (u32, u32) {
        match self {
            Self::Cottage => (16, *ids::COTTAGES.start()),
            Self::Farmhouse => (24, *ids::FARMHOUSES.start()),
        }
    }
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
        self.plan
            .boxes()
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
        let half = self.plan.half();
        let doorstep = self.plan.doorstep();
        let from = path.pop().map_or(doorstep, |end| self.local(end));
        let side = if from[0] < 0 { -1 } else { 1 };
        let clear = half[0] + 150;
        let mut local = vec![from];
        // From behind the house, along its side to the front, so no leg crosses the walls.
        if from[1] < half[1] + 50 && from[0].abs() <= clear + 50 {
            if from[1] < -half[1] {
                local.push([side * clear, from[1]]);
            }
            local.push([side * clear, doorstep[1]]);
        }
        let facing = match self.plan {
            Plan::Cottage => {
                local.extend([DOORSTEP, DOORWAY, INSIDE, MIDROOM]);
                local.push(if second { HEARTHSIDE } else { BEDSIDE });
                // Towards the bed (frame −X) or the fire (frame −Z).
                if second { 32_768 } else { 49_152 }
            }
            Plan::Farmhouse => {
                local.extend([FARMHOUSE_DOORSTEP, FARMHOUSE_DOORWAY, FARMHOUSE_INSIDE]);
                if second {
                    local.extend(FARMHOUSE_TO_HEARTH);
                } else {
                    local.extend(FARMHOUSE_TO_BED);
                }
                // Towards the bed (frame +X) or the fire (frame −X).
                if second { 49_152 } else { 16_384 }
            }
        };
        path.extend(local.into_iter().map(|point| self.spot(point)));
        path.dedup();
        self.facing(facing)
    }
}

/// The cottage, if any, whose doorstep a walk home ending at `end` comes to: within 9 m.
#[must_use]
pub fn home_of(end: [i32; 2]) -> Option<Cottage> {
    cottages().into_iter().find(|cottage| {
        let [x, z] = cottage.spot(cottage.plan.doorstep());
        (i64::from(x - end[0])).pow(2) + (i64::from(z - end[1])).pow(2) < 900 * 900
    })
}

/// Every walk-in house's walls and furniture: a cottage's 16 IDs apart in [`ids::COTTAGES`], a
/// farmhouse's 24 apart in [`ids::FARMHOUSES`].
pub(super) fn colliders() -> Vec<StaticCollider> {
    let mut next = [Plan::Cottage, Plan::Farmhouse].map(|plan| plan.ids().1);
    cottages()
        .iter()
        .flat_map(|cottage| {
            let (parts, _) = cottage.plan.ids();
            let slot = &mut next[cottage.plan as usize];
            let first = *slot;
            *slot += parts;
            cottage.colliders(first)
        })
        .collect()
}

/// Whether collider `id` is part of a walk-in house.
#[must_use]
pub fn is_cottage(id: u32) -> bool {
    ids::COTTAGES.contains(&id) || ids::FARMHOUSES.contains(&id)
}

/// Whether collider `id` is a walk-in house's own box, which no longer collides.
#[must_use]
pub fn is_house(id: u32) -> bool {
    of_house(id).is_some()
}
