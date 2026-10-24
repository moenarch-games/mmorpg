//! The Sunken Shrine (#274 part 2, owner Q10/Q11): a ruined shrine on a sunken plaza at the
//! bottom of Stillwater Lake, and the flooded cave that runs east from its floor to the altar
//! that holds the Sunken Relic.
//!
//! Everything is box colliders on the 4 m terrain grid, like the Greyfang cave: the terrain is
//! flattened to [`FLOOR_Y`] over the plaza and the cave floor, rock walls stand on it and roof
//! slabs close the cave, each slab rising to the lake bed or the shore above it so nothing
//! opens a hole in the ground. The cave is 64 m long, more than one breath swims there and
//! back, so an authored [`AirPocket`] halfway lets the round trip breathe. Beyond the lake the
//! cave lies under the eastern shore inside its own water volume, whose surface is the shore's
//! ground: on the shore nothing is wet.
//!
//! The colliders are authoritative. The clients draw asset-tooling's `shrine-kit` modules over
//! them (#431): `mmorpg-scenery`'s `shrine` placements put each module where the boxes below
//! stand, so the layout follows the kit's 4 m grid and proxies — a 2.4 m round arch with 0.8 m
//! jambs, 0.6 m broken walls, fluted pillars, the carved altar and an 8 × 8 m air-pocket room
//! halfway along the 4 m wide cave.

use crate::{AirPocket, StaticCollider, WaterVolume};

/// The shore over the cave: the lake terrace's ground (#423).
pub const SHORE_Y: i32 = super::basin::TERRACE_Y;
/// The plaza and cave floor height: the lake's 9 m deep centre.
pub const FLOOR_Y: i32 = SHORE_Y - 900;
/// Clear height of the cave under its roof (3 m).
pub const CLEAR: i32 = 300;
/// Clear height of the air-pocket room under its vault (5 m).
pub const ROOM_CLEAR: i32 = 500;
/// Every roof slab is at least this thick (1 m).
const ROOF_THICKNESS: i32 = 100;
/// The 4 m terrain grid the plaza, the cave and the kit share.
pub const CELL: i32 = 400;

/// Yaws (65 536 steps per turn, 0 keeping local +Z on +Z) of the kit modules' fronts.
pub mod facing {
    pub const SOUTH: u16 = 0;
    pub const EAST: u16 = 16_384;
    pub const NORTH: u16 = 32_768;
    pub const WEST: u16 = 49_152;
}

/// The open shrine plaza, `(min, max)` XZ, flattened but not roofed.
pub const PLAZA: ([i32; 2], [i32; 2]) = ([4_400, -6_000], [5_600, -4_800]);
/// The cave floor: from the plaza's east edge (the mouth) to the altar chamber's end wall.
pub const CAVE: ([i32; 2], [i32; 2]) = ([5_600, -5_600], [12_000, -5_200]);
/// The cave's centre line.
pub const CAVE_Z: i32 = -5_400;
/// The altar chamber, the cave's last 4 m: the relic's retrieve area.
pub const ALTAR_CHAMBER: ([i32; 2], [i32; 2]) = ([11_600, -5_600], [12_000, -5_200]);
/// The air-pocket room (asset-tooling#345, owner 2026-10-22): the kit's 8 × 8 m room over the
/// cave's 8th and 9th cells. Over its last 0.8 m at each end shoulders narrow it to the cave.
pub const ROOM: ([i32; 2], [i32; 2]) = ([8_400, -5_800], [9_200, -5_000]);
/// The ground under the room stays flat over the grid vertices around it.
const ROOM_FLOOR: ([i32; 2], [i32; 2]) = ([8_400, -6_000], [9_200, -4_800]);
/// The room's shoulders: 0.8 m deep from each end, from 2.4 m off the centre line.
const SHOULDER: [i32; 2] = [80, 240];
/// The breathable box: the room's clear interior between its shoulders, up to its vault.
pub const AIR_POCKET: AirPocket = AirPocket {
    min: [8_550, FLOOR_Y, -5_800],
    max: [9_050, FLOOR_Y + ROOM_CLEAR, -5_000],
};

/// The arch's base centre in the plaza's west wall, its frieze facing the open lake.
pub const ARCH: ([i32; 2], u16) = ([4_500, CAVE_Z], facing::WEST);
/// The broken walls' base centres and fronts: either side of the arch, then north and south.
/// Each falls from 3 m at its left (−X local) end to 1.8 m at its right.
pub const WALLS: [([i32; 2], u16); 4] = [
    ([4_500, -5_800], facing::EAST),
    ([4_500, -5_000], facing::WEST),
    ([5_000, -4_831], facing::NORTH),
    ([5_000, -5_969], facing::SOUTH),
];
/// The fluted pillars flanking the approach to the cave: a full one and a broken one.
pub const PILLARS: [[i32; 2]; 2] = [[4_900, -5_700], [4_900, -5_100]];
/// The tumbled blocks by the north wall.
pub const FALLEN_BLOCKS: [i32; 2] = [4_700, -5_050];
/// The altar's base centre, its carved front facing back down the cave.
pub const ALTAR: ([i32; 2], u16) = ([11_900, CAVE_Z], facing::WEST);

/// Whether the terrain is flattened to [`FLOOR_Y`] at `(x, z)`: on the plaza, the cave floor
/// or under the air-pocket room.
#[must_use]
pub fn flattens(x: i32, z: i32) -> bool {
    [PLAZA, CAVE, ROOM_FLOOR]
        .iter()
        .any(|(min, max)| (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z))
}

/// The flooded cave beyond the lake: an ellipse over the cave whose surface is the shore's
/// flat ground, with the air pocket. Listed after the lake, which covers the cave's west end.
#[must_use]
pub fn cave_water() -> WaterVolume {
    WaterVolume {
        name: "Sunken Shrine Cave".to_owned(),
        centre: [8_800, CAVE_Z],
        radii: [3_600, 800],
        surface: SHORE_Y,
        air_pockets: vec![AIR_POCKET],
    }
}

/// A box over `(min, max)` XZ from `base` up `height`.
const fn block(id: u32, min: [i32; 2], max: [i32; 2], base: i32, height: i32) -> StaticCollider {
    let half = [(max[0] - min[0]) / 2, height / 2, (max[1] - min[1]) / 2];
    StaticCollider {
        id,
        position: [min[0] + half[0], base + half[1], min[1] + half[2]],
        half_extents: half,
    }
}

/// A module-frame box (`centre`/`half` along local X and Z) turned by `yaw` about `origin`;
/// yaws are quarter turns.
fn turned(origin: [i32; 2], yaw: u16, centre: [i32; 2], half: [i32; 2]) -> ([i32; 2], [i32; 2]) {
    let ([cx, cz], [hx, hz]) = match yaw {
        facing::EAST => ([centre[1], -centre[0]], [half[1], half[0]]),
        facing::NORTH => ([-centre[0], -centre[1]], half),
        facing::WEST => ([-centre[1], centre[0]], [half[1], half[0]]),
        _ => (centre, half),
    };
    let [x, z] = [origin[0] + cx, origin[1] + cz];
    ([x - hx, z - hz], [x + hx, z + hz])
}

/// A module-frame box: origin, yaw, local centre, local half extents, base and height.
type Part = ([i32; 2], u16, [i32; 2], [i32; 2], i32, i32);

/// The ruined shrine on the plaza, from `first_id`, after the kit's proxies: the arch's jambs
/// and crown, each broken wall's tall and low halves, the pillars, the fallen blocks and, last,
/// the altar in the cave's end chamber.
#[must_use]
pub fn ruins(first_id: u32) -> Vec<StaticCollider> {
    let mut parts: Vec<Part> = Vec::new();
    let (arch, yaw) = ARCH;
    parts.extend([
        (arch, yaw, [-160, 0], [40, 37], 0, 400),
        (arch, yaw, [160, 0], [40, 37], 0, 400),
        (arch, yaw, [0, 0], [120, 37], CLEAR, 100),
    ]);
    for (origin, yaw) in WALLS {
        parts.push((origin, yaw, [-100, 0], [100, 31], 0, 300));
        parts.push((origin, yaw, [100, 0], [100, 31], 0, 180));
    }
    parts.push((PILLARS[0], facing::SOUTH, [0, 0], [40, 40], 0, 300));
    parts.push((PILLARS[1], facing::SOUTH, [0, 0], [40, 40], 0, 176));
    parts.push((FALLEN_BLOCKS, facing::SOUTH, [-16, -4], [98, 100], 0, 56));
    let (altar, yaw) = ALTAR;
    parts.push((altar, yaw, [0, 0], [90, 65], 0, 100));
    parts
        .iter()
        .zip(first_id..)
        .map(|(&(origin, yaw, centre, half, base, height), id)| {
            let (min, max) = turned(origin, yaw, centre, half);
            block(id, min, max, FLOOR_Y + base, height)
        })
        .collect()
}

/// The cave's rock walls from `first_id`: north and south along its length either side of the
/// room, the end wall behind the altar, the room's own side walls up to its vault, and its
/// four shoulders.
#[must_use]
pub fn walls(first_id: u32) -> Vec<StaticCollider> {
    let [(min, max)] = [CAVE];
    let ([rx0, rz0], [rx1, rz1]) = ROOM;
    let (end, side) = (max[0] + CELL, CELL);
    let [deep, off] = SHOULDER;
    let parts = [
        ([min[0], max[1]], [rx0, max[1] + side], CLEAR),
        ([rx1, max[1]], [end, max[1] + side], CLEAR),
        ([min[0], min[1] - side], [rx0, min[1]], CLEAR),
        ([rx1, min[1] - side], [end, min[1]], CLEAR),
        ([max[0], min[1]], [end, max[1]], CLEAR),
        ([rx0, rz1], [rx1, rz1 + 2 * side - 200], ROOM_CLEAR),
        ([rx0, rz0 - 2 * side + 200], [rx1, rz0], ROOM_CLEAR),
        ([rx0, rz0], [rx0 + deep, CAVE_Z - off], ROOM_CLEAR),
        ([rx0, CAVE_Z + off], [rx0 + deep, rz1], ROOM_CLEAR),
        ([rx1 - deep, rz0], [rx1, CAVE_Z - off], ROOM_CLEAR),
        ([rx1 - deep, CAVE_Z + off], [rx1, rz1], ROOM_CLEAR),
    ];
    parts
        .iter()
        .zip(first_id..)
        .map(|(&(min, max, height), id)| block(id, min, max, FLOOR_Y, height))
        .collect()
}

/// The cave's roof slabs from `first_id`, one per 4 m cell along it over walls and floor, each
/// rising from the cave ceiling (the room's vault over the room) to the highest unflattened
/// ground (`ground`) over its footprint, and at least [`ROOF_THICKNESS`] thick. Around the
/// room the slabs reach 2 m wider, over the ground that falls to its floor.
#[must_use]
pub fn roof(first_id: u32, ground: impl Fn(i32, i32) -> i32) -> Vec<StaticCollider> {
    let [(min, max)] = [CAVE];
    let ([rx0, _], [rx1, _]) = ROOM;
    (min[0]..max[0] + CELL)
        .step_by(CELL as usize)
        .zip(first_id..)
        .map(|(x, id)| {
            let room = (rx0..rx1).contains(&x);
            let wide = (rx0 - CELL..rx1 + CELL).contains(&x);
            let reach = if wide { 2 * CELL } else { CELL };
            let (z0, z1) = (min[1] - reach, max[1] + reach);
            let ceiling = FLOOR_Y + if room { ROOM_CLEAR } else { CLEAR };
            let top = (x..=x + CELL)
                .step_by(CELL as usize)
                .flat_map(|x| (z0..=z1).step_by(CELL as usize).map(move |z| (x, z)))
                .map(|(x, z)| ground(x, z))
                .max()
                .unwrap_or(0)
                .max(ceiling + ROOF_THICKNESS);
            // An even height keeps the box's half extent exact.
            let height = (top - ceiling + 19) / 20 * 20;
            block(id, [x, z0], [x + CELL, z1], ceiling, height)
        })
        .collect()
}
