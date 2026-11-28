//! The Rimewood scouts (#933): an Arcanist's group-control fight on the level plateau between
//! Hollowpine Reach and the Crown Foothills, far from roads, outposts and the Wolfrun clearing.
//! Two level-5 Redbrand bandits stand side by side and a third keeps watch 7 m behind and to
//! one side, inside their assist radius. Pulled, the pair arrive together and a Frost Nova (8 m) roots
//! them while the lookout is still outside it; a caster who then backs off draws the lookout
//! away, so a Chain Lightning on the rooted pair jumps between them and not to it (8 m jumps).
//! Existing templates, AI, auras and spells only. Spawn IDs 1800–1809 are the scouts'.

use crate::CreatureId;
use crate::content::CreatureSpawn;

use super::units::templates::REDBRAND_BANDIT;

/// The scouts' creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 1_800..=1_809;
/// The side-by-side pair.
pub const PAIR: [CreatureId; 2] = [CreatureId::new(1_800), CreatureId::new(1_801)];
/// The lookout 7 m behind them and to one side, so its path to a retreating caster clears them.
pub const LOOKOUT: CreatureId = CreatureId::new(1_802);
/// Where the pair stand: a caster coming from the east meets them first.
pub const PAIR_AT: [i32; 2] = [-57_000, -33_000];

use crate::trig::facing::EAST;
/// A short wander keeps the scouts in their formation.
const WANDER: i32 = 100;

/// The Rimewood scouts.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    let [x, z] = PAIR_AT;
    [
        (PAIR[0], [x, z - 75]),
        (PAIR[1], [x, z + 75]),
        (LOOKOUT, [x - 600, z + 400]),
    ]
    .into_iter()
    .map(|(id, position)| CreatureSpawn {
        id,
        template: REDBRAND_BANDIT,
        position,
        facing: EAST,
        wander_radius: WANDER,
        levels: Some([5, 5]),
    })
    .collect()
}
