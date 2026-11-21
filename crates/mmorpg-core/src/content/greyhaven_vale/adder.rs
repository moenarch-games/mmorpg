//! The Eastwater Fen adders (#990): three Marsh Adders lie coiled in the fen's low, wet western
//! edge, Adder Hollow, 70 m and more south of the Fen Road. Each keeps to a metre or two of
//! its own, some 10 m apart, so a player meets them one at a time. A player who comes within
//! 4 m is warned: the adder coils and hisses for over a second, then strikes close and quick
//! on its clip's contact and lies open in its recovery (`creature_attack`). Backing off
//! through the hiss dodges the strike; it slithers after slowly enough to walk away from.
//! They are ordinary spawns that respawn in place. Spawn IDs 3120–3139 are theirs.

use crate::CreatureId;
use crate::content::CreatureSpawn;

use super::units::templates::MARSH_ADDER;

/// The adders' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 3_120..=3_139;

/// Yaw 0 faces south (+Z), 16 384 east (+X) and 32 768 north.
const EAST: u16 = 16_384;
const NORTH: u16 = 32_768;
/// An adder stays within 1.5 m of its coil.
const WANDER: i32 = 150;

/// `(id, feet XZ in units, facing)` in the hollow, facing out of it toward the fen.
const ADDERS: [(u32, [i32; 2], u16); 3] = [
    (3_120, [13_000, -8_200], NORTH),
    (3_121, [13_300, -9_200], EAST),
    (3_122, [13_800, -10_100], EAST),
];

/// The adders' creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    ADDERS
        .iter()
        .map(|&(id, position, facing)| CreatureSpawn {
            id: CreatureId::new(id),
            template: MARSH_ADDER,
            position,
            facing,
            wander_radius: WANDER,
            levels: None,
        })
        .collect()
}
