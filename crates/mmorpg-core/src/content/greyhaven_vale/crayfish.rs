//! The Fenwater river crayfish (#996): three River Crayfish lie on the Fenwater's north bank in
//! the alders, some 100 m upstream (west) of Fenford's ford, each on the low margin a metre or
//! so beyond the channel's edge, where the bank is barely above the water. None needs to swim.
//! A player who comes within 4 m is its target: it scuttles after them, raises a claw slowly
//! and pinches on its clip's contact, holding the pinch after it (`creature_attack`). Its
//! front shell and claws turn blows from inside its front arc, like the Redbrand Brute's
//! shield (#953, `guard`); it turns at the bounded engaged rate, so a blow from its side or
//! rear lands. They are ordinary spawns that respawn in place. Spawn IDs 3160–3179 are theirs.

use crate::CreatureId;
use crate::content::CreatureSpawn;

use super::units::templates::RIVER_CRAYFISH;

/// The crayfish' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 3_160..=3_179;

use crate::trig::facing::NORTH;
/// A crayfish keeps to a metre of its spot on the margin.
const WANDER: i32 = 100;

/// `(id, feet XZ in units)`: 20 m apart along the bank, 5.2 m north of the channel's centre
/// line (its 4 m half width and 1.2 m of margin).
const CRAYFISH: [(u32, [i32; 2]); 3] = [
    (3_160, [93_106, 24_320]),
    (3_161, [95_106, 24_740]),
    (3_162, [97_106, 25_160]),
];

/// The crayfish' creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    CRAYFISH
        .iter()
        .map(|&(id, position)| CreatureSpawn {
            id: CreatureId::new(id),
            template: RIVER_CRAYFISH,
            position,
            facing: NORTH,
            wander_radius: WANDER,
            levels: None,
        })
        .collect()
}
