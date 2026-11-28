//! Immutable authored frame for the shrine modules; these are local layout coordinates.
//! Use the parent shrine module for placed world collision, water and quest metadata.
//! The Sunken Shrine (#274 part 2, owner Q10/Q11): a ruined shrine on a sunken plaza at the
//! bottom of Stillwater Lake, and the flooded cave that runs east from its floor to the altar
//! that holds the Sunken Relic.
//!
//! The terrain is flattened to [`FLOOR_Y`] over the plaza and cave floor. The shrine kit's
//! wall proxies retain their 4 m grid, while repository-authored static meshes form a joined
//! rock cover above them, closing the trench to the lake bed and dry shore. The cave is 64 m
//! long, more than one breath swims there and back, so an authored [`AirPocket`] halfway lets
//! the round trip breathe. Beyond the lake the
//! cave lies under the eastern shore inside its own water volume, whose surface is the shore's
//! ground: on the shore nothing is wet.
//!
//! The colliders are authoritative. The clients draw asset-tooling's `shrine-kit` modules over
//! them (#431): `mmorpg-scenery`'s `shrine` placements put each module where the boxes below
//! stand, so the layout follows the kit's 4 m grid and proxies — a 2.4 m round arch with 0.8 m
//! jambs, 0.6 m broken walls, fluted pillars, the carved altar and an 8 × 8 m air-pocket room
//! halfway along the 4 m wide cave.

use crate::{AirPocket, AreaId, StaticCollider, WaterVolume};

/// The shore over the cave: the lake terrace's ground (#423).
pub const SHORE_Y: i32 = super::super::basin::TERRACE_Y;
/// The plaza and cave floor height: the lake's 9 m deep centre.
pub const FLOOR_Y: i32 = SHORE_Y - 900;
/// Clear height of the cave under its roof (3 m).
pub const CLEAR: i32 = 300;
/// Clear height of the air-pocket room under its vault (5 m).
pub const ROOM_CLEAR: i32 = 500;
/// The 4 m terrain grid the plaza, the cave and the kit share.
pub const CELL: i32 = 400;

/// Yaws of the kit modules' fronts.
pub use crate::trig::facing;

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

/// The sealed passage beyond the altar (#1019): the cave's former end wall, now the shrine's
/// seal at its start.
pub const PASSAGE: ([i32; 2], [i32; 2]) = ([12_000, -5_600], [12_400, -5_200]);
/// The guardian's vault (#1019): a second 8 × 8 m kit room beyond the passage, dry because the
/// seal held the lake back, then a last tunnel cell to its end wall.
pub const VAULT: ([i32; 2], [i32; 2]) = ([12_400, -5_800], [13_200, -5_000]);
pub const VAULT_END: ([i32; 2], [i32; 2]) = ([13_200, -5_600], [13_600, -5_200]);
/// The ground under the vault stays flat over the grid vertices around it, as under the room.
const VAULT_FLOOR: ([i32; 2], [i32; 2]) = ([12_400, -6_000], [13_200, -4_800]);
/// The seal's stone across the passage's start: 1 m thick, the cave's height and width.
pub const SEAL: ([i32; 3], [i32; 3]) =
    ([12_050, FLOOR_Y + CLEAR / 2, CAVE_Z], [50, CLEAR / 2, 200]);

/// Whether the terrain is flattened to [`FLOOR_Y`] at `(x, z)`: on the plaza, the cave floor,
/// under the air-pocket room, along the sealed passage or under the vault (#1019).
#[must_use]
pub fn flattens(x: i32, z: i32) -> bool {
    [PLAZA, CAVE, ROOM_FLOOR, PASSAGE, VAULT_FLOOR, VAULT_END]
        .iter()
        .any(|(min, max)| (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z))
}

/// The flooded cave beyond the lake: an ellipse over the cave whose surface is the shore's
/// flat ground, with the air pocket. Listed after the lake, which covers the cave's west end.
#[must_use]
pub fn cave_water() -> WaterVolume {
    WaterVolume {
        channel: None,
        name: "Sunken Shrine Cave".to_owned(),
        centre: [8_800, CAVE_Z],
        radii: [3_600, 800],
        surface: SHORE_Y,
        air_pockets: vec![AIR_POCKET],
    }
}

/// A box over `(min, max)` XZ from `base` up `height`.
const fn block(id: u32, min: [i32; 2], max: [i32; 2], base: i32, height: i32) -> StaticCollider {
    StaticCollider::over_footprint(id, min, max, base, height)
}

/// A module-frame box (`centre`/`half` along local X and Z) turned by `yaw` about `origin`;
/// yaws are quarter turns.
fn turned(origin: [i32; 2], yaw: u16, centre: [i32; 2], half: [i32; 2]) -> ([i32; 2], [i32; 2]) {
    let quarters = crate::trig::nearest_quarter(yaw);
    let [cx, cz] = crate::trig::turn_quarters(centre, quarters);
    let [hx, _, hz] = crate::trig::turn_extents([half[0], 0, half[1]], quarters);
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

/// The cave's rock walls from `first_id`: north and south along its length (on through the
/// sealed passage) either side of the room, the room's own side walls up to its vault, and its
/// four shoulders. The seal (#1019) closes the passage where the end wall stood.
#[must_use]
pub fn walls(first_id: u32) -> Vec<StaticCollider> {
    let [(min, max)] = [CAVE];
    let ([rx0, _], [rx1, _]) = ROOM;
    let (end, side) = (PASSAGE.1[0], CELL);
    let parts = [
        ([min[0], max[1]], [rx0, max[1] + side], CLEAR),
        ([rx1, max[1]], [end, max[1] + side], CLEAR),
        ([min[0], min[1] - side], [rx0, min[1]], CLEAR),
        ([rx1, min[1] - side], [end, min[1]], CLEAR),
    ]
    .into_iter()
    .chain(room_walls(ROOM));
    parts
        .zip(first_id..)
        .map(|((min, max, height), id)| block(id, min, max, FLOOR_Y, height))
        .collect()
}

/// A kit room's side walls up to its vault and its four shoulders, as `(min, max, height)`.
fn room_walls(([rx0, rz0], [rx1, rz1]): ([i32; 2], [i32; 2])) -> [([i32; 2], [i32; 2], i32); 6] {
    let side = CELL;
    let [deep, off] = SHOULDER;
    [
        ([rx0, rz1], [rx1, rz1 + 2 * side - 200], ROOM_CLEAR),
        ([rx0, rz0 - 2 * side + 200], [rx1, rz0], ROOM_CLEAR),
        ([rx0, rz0], [rx0 + deep, CAVE_Z - off], ROOM_CLEAR),
        ([rx0, CAVE_Z + off], [rx0 + deep, rz1], ROOM_CLEAR),
        ([rx1 - deep, rz0], [rx1, CAVE_Z - off], ROOM_CLEAR),
        ([rx1 - deep, CAVE_Z + off], [rx1, rz1], ROOM_CLEAR),
    ]
}

/// The guardian's vault (#1019) from `first_id`: the kit room's side walls and shoulders, the
/// last cell's north and south walls, and its end wall.
#[must_use]
pub fn vault_walls(first_id: u32) -> Vec<StaticCollider> {
    let (min, max) = VAULT_END;
    let end = max[0] + CELL;
    room_walls(VAULT)
        .into_iter()
        .chain([
            ([min[0], max[1]], [end, max[1] + CELL], CLEAR),
            ([min[0], min[1] - CELL], [end, min[1]], CLEAR),
            ([max[0], min[1]], [end, max[1]], CLEAR),
        ])
        .zip(first_id..)
        .map(|((min, max, height), id)| block(id, min, max, FLOOR_Y, height))
        .collect()
}

/// The rock cover over the vault (#1019): the room's two cover cells, the cave's last tunnel cell
/// and its end cell, moved beyond the passage; IDs from `first_id`.
#[must_use]
pub fn vault_cover(first_id: u32) -> Vec<crate::StaticMesh> {
    let meshes = collision_meshes();
    let cell = |from: i32| {
        meshes
            .iter()
            .find(|mesh| {
                mesh.vertices
                    .iter()
                    .map(|vertex| vertex[0] + mesh.position[0])
                    .min()
                    == Some(from)
            })
            .cloned()
            .expect("the rock cover has a cell there")
    };
    let [room0, room1] = [ROOM.0[0], ROOM.0[0] + CELL];
    let shift = VAULT.0[0] - ROOM.0[0];
    let tail = VAULT_END.0[0] - (CAVE.1[0] - CELL);
    [
        (cell(room0), shift),
        (cell(room1), shift),
        (cell(CAVE.1[0] - CELL), tail),
        (cell(CAVE.1[0]), tail),
    ]
    .into_iter()
    .zip(first_id..)
    .map(|((mut mesh, dx), id)| {
        mesh.id = id;
        mesh.position[0] += dx;
        mesh
    })
    .collect()
}

/// Repository-authored closed rock cover, with the former roof IDs and shared cell edges.
/// The underside keeps the tunnel's clearance and the higher air-room vault.
#[must_use]
pub fn collision_meshes() -> Vec<crate::StaticMesh> {
    serde_json::from_str(include_str!(
        "../../../../content/collision/sunken-shore.json"
    ))
    .expect("committed Sunken Shrine rock cover is valid")
}

/// The Sunken Shrine's altar chamber (#274), the relic's retrieve area.
pub const SUNKEN_SHRINE: AreaId = AreaId::new(10);
