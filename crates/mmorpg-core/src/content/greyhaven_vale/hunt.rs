//! Trapper Sela's afternoon hunt in Southwood (#991): a living-world encounter on the zone's
//! clock. At 14:00 she leaves the hunters' camp along her morning trap line to its far end and
//! stalks there for an hour. On a quiet day she comes home with her catch; on a confrontation
//! day a Brown Bear stands in the clearing beyond, and unless someone kills it while she holds
//! her ground she limps home hurt and rests bandaged at the camp until dusk. Huntsman Edric Vale
//! keeps his post and his quests. Existing templates, combat and routines only. Spawn IDs
//! 2900–2909 are the hunt's.

use crate::content::CreatureSpawn;
use crate::{CreatureId, HOUR_TICKS, HuntOuting, NpcId};

use super::units::templates::BROWN_BEAR;

/// The hunt's creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 2_900..=2_909;
/// The bear a confrontation day brings.
pub const BEAR: CreatureId = CreatureId::new(2_900);
/// Trapper Sela.
pub const HUNTER: NpcId = NpcId::new(33);
/// Where she stalks: the far end of her trap line.
pub const GROUND: [i32; 2] = [-53_600, 74_200];
/// Where the bear stands, 17 m south-west of her across the clearing.
pub const BEAR_AT: [i32; 2] = [-54_900, 73_100];

/// The hunt's creature spawns: the reserved bear, pacing a few metres.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    vec![CreatureSpawn {
        id: BEAR,
        template: BROWN_BEAR,
        position: BEAR_AT,
        // Facing her.
        facing: 9_000,
        wander_radius: 300,
        levels: Some([3, 3]),
    }]
}

/// Sela's outing: her trap line from the camp at 14:00, an hour's stalk facing the bear's spot.
#[must_use]
pub fn outing() -> HuntOuting {
    HuntOuting {
        hunter: HUNTER,
        leave: 14 * HOUR_TICKS,
        path: vec![[-49_000, 77_600], [-51_000, 76_000], GROUND],
        facing: 41_800,
        stalk: HOUR_TICKS,
        beast: BEAR,
    }
}
