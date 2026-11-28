//! The Rimewood signal post (#952): a target-priority fight on the plateau 50 m west of the
//! Rimewood scouts. A Redbrand bandit, the signaler, keeps watch beside a crossbowman; pulled,
//! he stands and sounds his signal horn, and the bandit posted 23 m up the slope behind them,
//! out of aggro and assist reach, runs in when the call completes. A Shield Bash, a Gouge or any
//! other stun breaks the call, and killing the signaler first answers it too. Existing
//! templates, AI and engagement only. Spawn IDs 2800–2819 are the post's.

use crate::content::CreatureSpawn;
use crate::{CreatureId, CreatureTemplateId, SignalCall};

use super::units::templates::{REDBRAND_BANDIT, REDBRAND_CROSSBOWMAN};

/// The post's creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 2_800..=2_819;
/// The bandit who calls for help.
pub const SIGNALER: CreatureId = CreatureId::new(2_800);
/// The crossbowman beside him, inside his assist radius.
pub const CROSSBOWMAN: CreatureId = CreatureId::new(2_801);
/// The bandit his call brings, 23 m up the slope behind them.
pub const PARTNER: CreatureId = CreatureId::new(2_802);
/// Where the signaler stands: a player coming from the east meets him first.
pub const SIGNALER_AT: [i32; 2] = [-62_000, -33_000];

use crate::trig::facing::EAST;
/// A short wander keeps the post in its places.
const WANDER: i32 = 150;

/// The post's creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    let [x, z] = SIGNALER_AT;
    let rows: [(CreatureId, CreatureTemplateId, [i32; 2]); 3] = [
        (SIGNALER, REDBRAND_BANDIT, [x, z]),
        (CROSSBOWMAN, REDBRAND_CROSSBOWMAN, [x - 300, z + 300]),
        (PARTNER, REDBRAND_BANDIT, [x - 1_800, z + 1_500]),
    ];
    rows.into_iter()
        .map(|(id, template, position)| CreatureSpawn {
            id,
            template,
            position,
            facing: EAST,
            wander_radius: WANDER,
            levels: Some([5, 5]),
        })
        .collect()
}

/// The signaler's call and the partner it brings.
#[must_use]
pub fn signal_calls() -> Vec<SignalCall> {
    vec![SignalCall {
        caller: SIGNALER,
        partner: PARTNER,
    }]
}
