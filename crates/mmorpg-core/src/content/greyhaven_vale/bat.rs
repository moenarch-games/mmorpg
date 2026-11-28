//! The Greyfang cave bats (#995): two Cave Bats hang from the outer cavern's one low rock, the
//! stalactite by the north mouth's passage, 3.2 m over the floor. A player who walks under
//! them within 4 m is their target: they drop off the roost and fly over the player's head,
//! out of a sword's reach, and dive one at a time as their swing allows. Each dive is
//! telegraphed by a flaring hiss (`creature_attack`): stepping away through it dodges, and the
//! bat hangs low and open to a counter after its contact. A bat flies slower than a player
//! runs and gives up 15 m from its roost, so the cave mouth is always an escape (`flight`).
//! They are ordinary spawns that respawn on their roost. Spawn IDs 3140–3159 are theirs.

use crate::CreatureId;
use crate::content::CreatureSpawn;

use super::units::templates::CAVE_BAT;

/// The bats' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 3_140..=3_159;
/// The roost: the outer cavern's stalactite (`content/collision/redbrand.json`, collider 660),
/// whose tip hangs 3.4 m over the floor.
pub const ROOST: [i32; 2] = [5_705, -14_148];

use crate::trig::facing::NORTH;

/// `(id, roost XZ in units)`: a bat either side of the stalactite's tip.
const BATS: [(u32, [i32; 2]); 2] = [(3_140, [5_665, -14_148]), (3_141, [5_745, -14_148])];

/// The bats' creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    BATS.iter()
        .map(|&(id, position)| CreatureSpawn {
            id: CreatureId::new(id),
            template: CAVE_BAT,
            position,
            facing: NORTH,
            wander_radius: 0,
            levels: None,
        })
        .collect()
}
