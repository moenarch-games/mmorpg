//! Shepherd Lise's flock (#1019, slice 4b): the eastern hamlet's simulated sheep
//! ([`crate::flock`]).
//!
//! - **The stray**: Lise's missing ewe stands on the slope north of the hamlet's barn, down toward
//!   the brook, and shies from anyone who comes near. Her pen is Lise's fold, east of Lise's house:
//!   the open ground inside the two hurdles of quest 56, which close its north and east sides while
//!   they stand. Driven from the north she has to be walked round them, in from the west or south.
//! - **The watch**: three ewes stand in the fold. Two wolves come up out of the brook's gully at
//!   once and two more 25 s and 35 s later, each first to its gap in the hedges east of the fold,
//!   where Lise's bait (north-east) or snare (east) holds it, then for the nearest ewe. Two of the
//!   three must stand. The prowlers are the hamlets' spawns 1412-1415, timber wolves of the pack
//!   east of the pastures held at level 2.

use crate::content::CreatureSpawn;
use crate::{CreatureId, HedgeWatch, Npc, NpcId, NpcRole, Prowler, Stray};

use super::units::templates::TIMBER_WOLF;

/// Shepherd Lise, who starts the watch.
pub const SHEPHERD_LISE: NpcId = NpcId::new(39);
/// Her missing ewe and the three ewes of her fold.
pub const STRAY_EWE: NpcId = NpcId::new(50);
pub const FLOCK: [NpcId; 3] = [NpcId::new(51), NpcId::new(52), NpcId::new(53)];
/// Lise's fold: inside its north and east hurdles, east of her house.
pub const FOLD: ([i32; 2], [i32; 2]) = ([14_600, 45_950], [15_380, 46_600]);
/// The gaps in the hedges where the bait and the snare are laid: evidence spots 16 and 17.
pub const BAIT_GAP: [i32; 2] = [15_700, 45_500];
pub const SNARE_GAP: [i32; 2] = [15_900, 46_700];
/// The prowlers' spawn IDs, among the hamlets' 1400-1419.
pub const PROWLERS: [CreatureId; 4] = [
    CreatureId::new(1_412),
    CreatureId::new(1_413),
    CreatureId::new(1_414),
    CreatureId::new(1_415),
];

use crate::trig::facing::{SOUTH, WEST};

/// `(id, name, feet XZ, facing)` of the stray and the fold's ewes.
const SHEEP: [(u32, &str, [i32; 2], u16); 4] = [
    (50, "Lise's Stray Ewe", [12_800, 38_400], SOUTH),
    (51, "Black-faced Ewe", [14_800, 46_150], WEST),
    (52, "Speckled Ewe", [15_100, 46_300], SOUTH),
    (53, "Old Ewe", [14_900, 46_450], WEST),
];

/// The stray and the fold's ewes: level 1 residents the zone moves as livestock.
#[must_use]
pub fn npcs() -> Vec<Npc> {
    SHEEP
        .iter()
        .map(|&(id, name, position, facing)| Npc {
            id: NpcId::new(id),
            name: name.to_owned(),
            role: NpcRole::Resident,
            level: 1,
            position,
            facing,
        })
        .collect()
}

/// The missing ewe, penned in Lise's fold.
#[must_use]
pub fn strays() -> Vec<Stray> {
    vec![Stray {
        npc: STRAY_EWE,
        pen: FOLD,
    }]
}

/// The night watch over the fold: 40 health a ewe, two of three to stand.
#[must_use]
pub fn watch() -> HedgeWatch {
    HedgeWatch {
        npc: SHEPHERD_LISE,
        flock: FLOCK.to_vec(),
        health: 40,
        hold: 2,
        prowlers: vec![
            Prowler {
                creature: PROWLERS[0],
                after_ticks: 0,
                gap: BAIT_GAP,
            },
            Prowler {
                creature: PROWLERS[1],
                after_ticks: 0,
                gap: SNARE_GAP,
            },
            Prowler {
                creature: PROWLERS[2],
                after_ticks: crate::long_ticks_from_ms(25_000),
                gap: BAIT_GAP,
            },
            Prowler {
                creature: PROWLERS[3],
                after_ticks: crate::long_ticks_from_ms(35_000),
                gap: SNARE_GAP,
            },
        ],
        centre: [15_000, 46_250],
        radius: 2_500,
    }
}

/// The prowlers' spawns at the hamlet's east edge above the brook's gully, reserved for the watch.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    [
        (PROWLERS[0], [16_300, 44_800]),
        (PROWLERS[1], [16_300, 47_300]),
        (PROWLERS[2], [16_250, 44_650]),
        (PROWLERS[3], [16_250, 47_450]),
    ]
    .into_iter()
    .map(|(id, position)| CreatureSpawn {
        id,
        template: TIMBER_WOLF,
        position,
        facing: WEST,
        wander_radius: 0,
        levels: Some([2, 2]),
    })
    .collect()
}
