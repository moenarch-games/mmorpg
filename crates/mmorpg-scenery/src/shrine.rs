//! The Sunken Shrine's kit layout (#431): where the clients draw asset-tooling's `shrine-kit`
//! modules (`shrine.kit.<part>`, variant `default`) over the zone's shrine colliders.
//!
//! Every module stands on the 4 m grid at its base centre, turned so its front faces the way
//! [`mmorpg_core::greyhaven_vale::shrine`] lays out the boxes the kit's proxies describe: the
//! arch and the broken walls on the plaza, the pillars, the fallen blocks and the altar, and
//! the cave as a mouth, tunnels, the 8 × 8 m air-pocket room and the closed end along +X.
//! Weed, silt drifts and shells only render. Presentation only: the colliders decide.
use mmorpg_core::greyhaven_vale::shrine::{
    ALTAR, ARCH, CAVE, CAVE_Z, CELL, CLEAR, FALLEN_BLOCKS, FLOOR_Y, PILLARS, PLAZA, ROOM, WALLS,
    facing,
};

/// The shrine kit's rolling release the clients draw.
pub const RELEASE: &str = "shrine-kit";

/// One kit module placed in the zone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Placement {
    /// The bundle entry, `shrine.kit.<part>`.
    pub key: &'static str,
    /// The module origin in units: its base centre (the hanging weed's top anchor).
    pub position: [i32; 3],
    /// Rotation about +Y, 65 536 steps per turn, 0 keeping the module's +Z front on +Z.
    pub yaw: u16,
}

const fn at(key: &'static str, [x, z]: [i32; 2], y: i32, yaw: u16) -> Placement {
    Placement {
        key,
        position: [x, y, z],
        yaw,
    }
}

/// Every kit module of the shrine and its cave, in a fixed order.
#[must_use]
pub fn placements() -> Vec<Placement> {
    let mut placed = Vec::new();
    // The plaza's flagstones, but under the cave mouth's apron.
    let ([x0, z0], [x1, z1]) = PLAZA;
    for x in (x0..x1).step_by(CELL as usize) {
        for z in (z0..z1).step_by(CELL as usize) {
            let centre = [x + CELL / 2, z + CELL / 2];
            if centre != [x1 - CELL / 2, CAVE_Z] {
                placed.push(at("shrine.kit.floor", centre, FLOOR_Y, facing::SOUTH));
            }
        }
    }
    placed.push(at("shrine.kit.arch", ARCH.0, FLOOR_Y, ARCH.1));
    placed.extend(
        WALLS
            .iter()
            .map(|&(origin, yaw)| at("shrine.kit.wall", origin, FLOOR_Y, yaw)),
    );
    placed.push(at("shrine.kit.pillar", PILLARS[0], FLOOR_Y, facing::SOUTH));
    placed.push(at(
        "shrine.kit.pillar-broken",
        PILLARS[1],
        FLOOR_Y,
        facing::SOUTH,
    ));
    placed.push(at(
        "shrine.kit.fallen-blocks",
        FALLEN_BLOCKS,
        FLOOR_Y,
        facing::SOUTH,
    ));
    // The cave, one module per 4 m cell from the mouth; the room spans two.
    let ([start, _], [end, _]) = CAVE;
    let ([room, _], _) = ROOM;
    for x in (start..end).step_by(CELL as usize) {
        let key = match x {
            _ if x == start => "shrine.kit.cave-mouth",
            _ if x == room => "shrine.kit.air-pocket",
            _ if x == room + CELL => continue,
            _ if x + CELL == end => "shrine.kit.cave-end",
            _ => "shrine.kit.cave-tunnel",
        };
        let centre = if x == room { x + CELL } else { x + CELL / 2 };
        placed.push(at(key, [centre, CAVE_Z], FLOOR_Y, facing::SOUTH));
    }
    placed.push(at("shrine.kit.altar", ALTAR.0, FLOOR_Y, ALTAR.1));
    // Dressing: silt and shells on the floors, weed hanging from the cave's ceiling.
    let ([rx, _], _) = ROOM;
    #[rustfmt::skip]
    let dressing: [(&str, [i32; 2], i32, u16); 9] = [
        ("shrine.kit.silt-drift", [5_250, -5_850], FLOOR_Y, facing::EAST),
        ("shrine.kit.silt-drift", [rx + 400, -5_640], FLOOR_Y, facing::SOUTH),
        ("shrine.kit.silt-drift", [10_800, CAVE_Z + 100], FLOOR_Y, facing::NORTH),
        ("shrine.kit.shells", [4_700, -5_750], FLOOR_Y, facing::SOUTH),
        ("shrine.kit.shells", [rx + 600, -5_100], FLOOR_Y, facing::WEST),
        ("shrine.kit.shells", [11_700, CAVE_Z + 150], FLOOR_Y, facing::EAST),
        ("shrine.kit.hanging-weed", [6_900, CAVE_Z - 120], FLOOR_Y + CLEAR, facing::SOUTH),
        ("shrine.kit.hanging-weed", [7_700, CAVE_Z + 100], FLOOR_Y + CLEAR, facing::EAST),
        ("shrine.kit.hanging-weed", [10_100, CAVE_Z - 60], FLOOR_Y + CLEAR, facing::NORTH),
    ];
    placed.extend(
        dressing
            .iter()
            .map(|&(key, origin, y, yaw)| at(key, origin, y, yaw)),
    );
    placed
}

/// The kit entries the placements use, sorted and once each.
#[must_use]
pub fn entries() -> Vec<&'static str> {
    let mut keys: Vec<_> = placements().iter().map(|placement| placement.key).collect();
    keys.sort_unstable();
    keys.dedup();
    keys
}
