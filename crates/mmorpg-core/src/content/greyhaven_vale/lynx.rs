//! The Southwood lynxes (#989): three Forest Lynxes lie up along the wood's eastern and
//! southern edge, where the oaks give onto the open slope, 90 m and more off the Southwood
//! Trail. Each holds a short beat of its own and ambushes a player who comes within 4 m: it
//! stalks in at a walk and pounces from a leap out (`creature_attack`). They are ordinary
//! spawns that respawn in place, so a player can seek one out or give the edge a wide berth.
//! Spawn IDs 3100–3119 are theirs.

use crate::CreatureId;
use crate::content::CreatureSpawn;

use super::units::templates::FOREST_LYNX;

/// The lynxes' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 3_100..=3_119;

use crate::trig::facing::{EAST, SOUTH};
/// A lynx keeps to a short beat along its edge.
const WANDER: i32 = 400;

/// `(id, feet XZ in metres, facing)` on the forest edge.
const LYNXES: [(u32, [i32; 2], u16); 3] = [
    (3_100, [-268, 715], EAST),
    (3_101, [-280, 790], EAST),
    (3_102, [-345, 850], SOUTH),
];

/// The lynxes' creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    LYNXES
        .iter()
        .map(|&(id, [x, z], facing)| CreatureSpawn {
            id: CreatureId::new(id),
            template: FOREST_LYNX,
            position: [x * 100, z * 100],
            facing,
            wander_radius: WANDER,
            levels: None,
        })
        .collect()
}
