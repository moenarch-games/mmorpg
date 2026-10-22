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
//! The visuals are collider-box stand-ins (rock walls, cave roofs, a standing stone for the
//! altar). The asset-tooling shrine kit (#345) replaces them by mapping the [`super::ids`]
//! ranges below to its models in `mmorpg-scenery`'s `visual_kind`; the colliders stay.

use crate::{AirPocket, StaticCollider, WaterVolume};

/// The plaza and cave floor height: the lake's 9 m deep centre.
pub const FLOOR_Y: i32 = -900;
/// Clear height of the cave under its roof (3 m).
pub const CLEAR: i32 = 300;
/// Every roof slab is at least this thick (1 m).
const ROOF_THICKNESS: i32 = 100;

/// The open shrine plaza, `(min, max)` XZ, flattened but not roofed.
pub const PLAZA: ([i32; 2], [i32; 2]) = ([4_400, -6_000], [5_600, -4_800]);
/// The cave floor: from the plaza's east edge (the mouth) to the altar chamber's end wall.
pub const CAVE: ([i32; 2], [i32; 2]) = ([5_600, -5_600], [12_000, -5_200]);
/// The altar chamber, the cave's last 4 m: the relic's retrieve area.
pub const ALTAR_CHAMBER: ([i32; 2], [i32; 2]) = ([11_600, -5_600], [12_000, -5_200]);
/// The breathable box halfway along the cave.
pub const AIR_POCKET: AirPocket = AirPocket {
    min: [8_600, FLOOR_Y, -5_600],
    max: [9_000, FLOOR_Y + CLEAR, -5_200],
};

/// Whether the terrain is flattened to [`FLOOR_Y`] at `(x, z)`: on the plaza or cave floor.
#[must_use]
pub fn flattens(x: i32, z: i32) -> bool {
    [PLAZA, CAVE]
        .iter()
        .any(|(min, max)| (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z))
}

/// The flooded cave beyond the lake: an ellipse over the cave whose surface is the shore's
/// flat ground, with the air pocket. Listed after the lake, which covers the cave's west end.
#[must_use]
pub fn cave_water() -> WaterVolume {
    WaterVolume {
        name: "Sunken Shrine Cave".to_owned(),
        centre: [8_800, -5_400],
        radii: [3_600, 800],
        surface: 0,
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

/// The ruined shrine on the plaza, from `first_id`: a broken west wall with an arch through
/// it, low broken north and south walls, and the altar stone in the cave's end chamber.
#[must_use]
pub fn ruins(first_id: u32) -> Vec<StaticCollider> {
    #[rustfmt::skip]
    let parts: [([i32; 2], [i32; 2], i32, i32); 8] = [
        // West wall, either side of the arch, broken to different heights.
        ([4_400, -6_000], [4_600, -5_600], 0, 250),
        ([4_400, -5_200], [4_600, -4_800], 0, 180),
        // The arch: two pillars and the lintel resting on them.
        ([4_400, -5_600], [4_600, -5_520], 0, CLEAR),
        ([4_400, -5_280], [4_600, -5_200], 0, CLEAR),
        ([4_400, -5_600], [4_600, -5_200], CLEAR, 60),
        // Broken north and south walls.
        ([4_600, -5_000], [5_200, -4_800], 0, 150),
        ([4_800, -6_000], [5_400, -5_800], 0, 120),
        // The altar stone at the cave's end.
        ([11_840, -5_460], [11_960, -5_340], 0, 100),
    ];
    parts
        .iter()
        .zip(first_id..)
        .map(|(&(min, max, base, height), id)| block(id, min, max, FLOOR_Y + base, height))
        .collect()
}

/// The cave's rock walls from `first_id`: north and south along its length, and the end
/// wall behind the altar.
#[must_use]
pub fn walls(first_id: u32) -> Vec<StaticCollider> {
    let [(min, max)] = [CAVE];
    let parts = [
        ([min[0], max[1]], [max[0] + 400, max[1] + 400]),
        ([min[0], min[1] - 400], [max[0] + 400, min[1]]),
        ([max[0], min[1]], [max[0] + 400, max[1]]),
    ];
    parts
        .iter()
        .zip(first_id..)
        .map(|(&(min, max), id)| block(id, min, max, FLOOR_Y, CLEAR))
        .collect()
}

/// The cave's roof slabs from `first_id`, one per 4 m along it over walls and floor, each
/// rising from the cave ceiling to the highest unflattened ground (`ground`) over its
/// footprint, and at least [`ROOF_THICKNESS`] thick.
#[must_use]
pub fn roof(first_id: u32, ground: impl Fn(i32, i32) -> i32) -> Vec<StaticCollider> {
    let [(min, max)] = [CAVE];
    let (z0, z1) = (min[1] - 400, max[1] + 400);
    let ceiling = FLOOR_Y + CLEAR;
    (min[0]..max[0] + 400)
        .step_by(400)
        .zip(first_id..)
        .map(|(x, id)| {
            let top = (x..=x + 400)
                .step_by(400)
                .flat_map(|x| (z0..=z1).step_by(400).map(move |z| (x, z)))
                .map(|(x, z)| ground(x, z))
                .max()
                .unwrap_or(0)
                .max(ceiling + ROOF_THICKNESS);
            // An even height keeps the box's half extent exact.
            let height = (top - ceiling + 19) / 20 * 20;
            block(id, [x, z0], [x + 400, z1], ceiling, height)
        })
        .collect()
}
