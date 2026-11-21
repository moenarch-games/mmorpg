//! Peddler Rowan's cart to Millbrook Farm (#992): a public incident on the zone's clock. At 17:00
//! the peddler pushes a handcart of wares from the Outpost out of the east gate, up the Millbrook
//! Climb and into the farmyard, trades there half an hour and pushes it home. On an ambush day
//! (one in two, a hash of the content seed and the day) three Redbrands off the Eastern Rim Trail
//! wait in the climb's cutting, where its walls hide them from the farm and the gate. Unless
//! someone kills all three while Rowan holds the cart, they take it: Rowan limps home hurt and
//! rests until dusk, and the robbers linger over the spoils for a minute before slipping away.
//! Existing templates, combat and routines only. Spawn IDs 2910–2919 are the cart's.

use crate::content::CreatureSpawn;
use crate::{CartRun, CreatureId, HOUR_TICKS, NpcId};

use super::units::templates::{REDBRAND_BANDIT, REDBRAND_CROSSBOWMAN};

/// The cart's creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 2_910..=2_919;
/// Peddler Rowan.
pub const MERCHANT: NpcId = NpcId::new(17);
/// The robbery site: halfway up the Millbrook Climb's cutting.
pub const SITE: [i32; 2] = [5_400, -300];
/// Robbers are Redbrands at the low end of the Hollow's levels, like the night raiders.
const LEVELS: [u8; 2] = [3, 4];

/// `(spawn id, template, feet XZ, facing)`: two bandits blocking the cutting ahead of the cart
/// and a crossbowman cutting off its retreat. Yaw 0 faces +Z, 32 768 faces -Z.
#[rustfmt::skip]
const ROBBERS: [(u32, crate::CreatureTemplateId, [i32; 2], u16); 3] = [
    (2_910, REDBRAND_BANDIT, [5_330, 300], 32_768),
    (2_911, REDBRAND_BANDIT, [5_470, 450], 32_768),
    (2_912, REDBRAND_CROSSBOWMAN, [5_400, -1_050], 0),
];

/// The robbers, waiting off the world until an ambush day stands them.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    ROBBERS
        .iter()
        .map(|&(id, template, position, facing)| CreatureSpawn {
            id: CreatureId::new(id),
            template,
            position,
            facing,
            wander_radius: 100,
            levels: Some(LEVELS),
        })
        .collect()
}

/// Rowan's journey: the Millbrook Road from the Outpost's square to the farmyard, the robbery
/// site at the cutting, half an hour's trade facing the farmhouse.
#[must_use]
pub fn journey() -> CartRun {
    CartRun {
        merchant: MERCHANT,
        leave: 17 * HOUR_TICKS,
        path: vec![
            [1_900, 2_000],
            [3_200, 2_000],
            [4_200, 2_000],
            [4_200, -1_800],
            [5_400, -1_800],
            SITE,
            [5_400, 1_200],
            [5_400, 2_800],
            [6_400, 3_300],
            [6_900, 3_400],
        ],
        ambush: 5,
        // Facing the farmhouse to the south-west.
        facing: 40_000,
        trade: HOUR_TICKS / 2,
        bandits: ROBBERS
            .iter()
            .map(|&(id, ..)| CreatureId::new(id))
            .collect(),
    }
}
