//! The farmland hamlets' creatures (#859), for the hamlets' quests: grain rats about the western
//! hamlet's barn, young boars in the barley west of the southern hamlet and a timber wolf pack
//! east of the eastern hamlet's pastures, on the existing templates, AI, leash, respawn and loot
//! rules. Each spawn's wander disc keeps clear of the hamlets' structures, roads, fields and every
//! resident's way. Spawn IDs 1400–1419 are the hamlets'.

use crate::content::CreatureSpawn;
use crate::{CreatureId, CreatureTemplateId};

use super::units::WANDER_RADIUS_UNITS;

/// The hamlets' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 1_400..=1_419;
use super::units::templates::{GRAIN_RAT, TIMBER_WOLF, YOUNG_BOAR};

/// Yaw 0 faces +Z (south), 32 768 north, 16 384 east and 49 152 west.
const SOUTH: u16 = 0;
const EAST: u16 = 16_384;
const NORTH: u16 = 32_768;
const WEST: u16 = 49_152;

#[rustfmt::skip]
const SPAWNS: [(u32, CreatureTemplateId, [i32; 2], u16); 12] = [
    // The western hamlet's granary: rats about the barn and the haystack.
    (1400, GRAIN_RAT, [-12_100, 34_300], EAST),
    (1401, GRAIN_RAT, [-11_800, 37_000], NORTH),
    (1402, GRAIN_RAT, [-9_900, 37_000], WEST),
    (1403, GRAIN_RAT, [-9_600, 35_500], WEST),
    // The southern hamlet's barley: boars along the fields' eastern edge.
    (1404, YOUNG_BOAR, [-10_800, 53_500], WEST),
    (1405, YOUNG_BOAR, [-10_600, 56_800], WEST),
    (1406, YOUNG_BOAR, [-9_000, 58_900], NORTH),
    (1407, YOUNG_BOAR, [-11_000, 59_500], NORTH),
    // East of the eastern hamlet's pastures: a wolf pack.
    (1408, TIMBER_WOLF, [19_400, 46_700], WEST),
    (1409, TIMBER_WOLF, [19_700, 48_200], WEST),
    (1410, TIMBER_WOLF, [18_900, 52_600], NORTH),
    (1411, TIMBER_WOLF, [20_500, 50_500], SOUTH),
];

/// The hamlets' creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    SPAWNS
        .iter()
        .map(|&(id, template, position, facing)| CreatureSpawn {
            id: CreatureId::new(id),
            template,
            position,
            facing,
            wander_radius: WANDER_RADIUS_UNITS,
            levels: None,
        })
        .collect()
}
