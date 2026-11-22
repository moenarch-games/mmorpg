//! The Southwood stags (#997): two Southwood Stags graze the open meadow on Southwood's western
//! edge, where the oaks give way to grass, 28 m and more off any road and far from the wood's
//! wolves, boars and lynxes. They are neutral: a player may watch one graze, or hunt it. But
//! each keeps its ground, and a player who comes within 7 m of an idle stag is a threat: it
//! throws its head up in alarm, then lowers its antlers and charges (`creature_attack`), and
//! stands blown after the blow. Stepping aside as it charges dodges it. They are ordinary
//! spawns that respawn in place. Spawn IDs 3180–3199 are theirs.

use crate::CreatureId;
use crate::content::CreatureSpawn;

use super::units::templates::SOUTHWOOD_STAG;

/// The stags' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 3_180..=3_199;

/// Yaw 16 384 faces east (+X), toward the wood.
const EAST: u16 = 16_384;
/// A stag grazes within 6 m of its spawn.
const WANDER: i32 = 600;

/// `(id, feet XZ in metres, facing)` in the meadow, 50 m apart.
const STAGS: [(u32, [i32; 2], u16); 2] = [(3_180, [-750, 520], EAST), (3_181, [-748, 570], EAST)];

/// The stags' creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    STAGS
        .iter()
        .map(|&(id, [x, z], facing)| CreatureSpawn {
            id: CreatureId::new(id),
            template: SOUTHWOOD_STAG,
            position: [x * 100, z * 100],
            facing,
            wander_radius: WANDER,
            levels: None,
        })
        .collect()
}
