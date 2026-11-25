//! The Downs road (#1019, slice 5b): a lookout on the rise north-east of Barrowdown watches the
//! Fenwater Link where it comes down off the downs, and a Redbrand supply patrol walks it to the
//! marauders' camp in the southern hollow when she calls it out.
//!
//! - **The lookout**: Lookout Brenna stands by the cold signal beacon 21 m west of the road
//!   ([`LOOKOUT_AT`]). Quest actions at her call the patrol out; players within [`POST_UNITS`] of
//!   her watch with her, and a member within [`SIGHT_UNITS`] of her is in sight.
//! - **The patrol**: a Redbrand Bandit leads, the Packbearer with the order pouch follows and a
//!   Crossbowman brings up the rear, reserved spawns in a column where they form up on the road
//!   ([`ROUTE`]'s first point). They walk the road past the lookout and turn off it south to the
//!   camp, about 290 m, two minutes at a walk.

use crate::{CreatureId, CreatureSpawn, Npc, NpcId, NpcRole, SupplyPatrol};

use super::units::templates::{REDBRAND_BANDIT, REDBRAND_CROSSBOWMAN, REDBRAND_PACKBEARER};

/// The Downs road's lookout.
pub const LOOKOUT: NpcId = NpcId::new(61);
/// Where she stands, by the beacon on the rise west of the road.
pub const LOOKOUT_AT: [i32; 2] = [65_500, 57_500];
/// The patrol's creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 2_950..=2_952;
/// The packbearer, the patrol's carrier.
pub const PACKBEARER: CreatureId = CreatureId::new(2_950);
/// The bandit who leads and the crossbowman at the rear.
pub const LEAD_BANDIT: CreatureId = CreatureId::new(2_951);
pub const REAR_CROSSBOWMAN: CreatureId = CreatureId::new(2_952);
/// Where the patrol forms up on the road, its waypoints, then the camp in the hollow.
pub const ROUTE: [[i32; 2]; 5] = [
    [72_000, 50_000],
    [69_000, 55_500],
    [66_000, 61_000],
    [65_000, 70_000],
    [64_200, 76_500],
];
/// A member within 35 m of the lookout is in her sight: about where the road passes the rise.
pub const SIGHT_UNITS: i32 = 3_500;
/// Players within 15 m of the lookout watch with her.
pub const POST_UNITS: i32 = 1_500;

/// The column's members face down the road (south-south-west).
const DOWN_THE_ROAD: u16 = 60_330;
/// 2.5 m between members of the column, back up the road.
const COLUMN_STEP: [i32; 2] = [120, -220];

/// Lookout Brenna by the beacon, facing the road.
pub(super) fn npcs() -> Vec<Npc> {
    vec![Npc {
        id: LOOKOUT,
        name: "Lookout Brenna".to_owned(),
        role: NpcRole::Resident,
        level: 4,
        position: LOOKOUT_AT,
        facing: 16_384,
    }]
}

/// The patrol: reserved off the world until it is called out.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    let [x, z] = ROUTE[0];
    [
        (LEAD_BANDIT, REDBRAND_BANDIT, 0, Some([4, 5])),
        (PACKBEARER, REDBRAND_PACKBEARER, 1, None),
        (REAR_CROSSBOWMAN, REDBRAND_CROSSBOWMAN, 2, Some([4, 5])),
    ]
    .into_iter()
    .map(|(id, template, place, levels)| CreatureSpawn {
        id,
        template,
        position: [x + COLUMN_STEP[0] * place, z + COLUMN_STEP[1] * place],
        facing: DOWN_THE_ROAD,
        wander_radius: 0,
        levels,
    })
    .collect()
}

/// The Downs road's supply patrol, its packbearer first.
#[must_use]
pub fn patrol() -> SupplyPatrol {
    SupplyPatrol {
        npc: LOOKOUT,
        members: vec![PACKBEARER, LEAD_BANDIT, REAR_CROSSBOWMAN],
        route: ROUTE.to_vec(),
        sight: SIGHT_UNITS,
        post: POST_UNITS,
    }
}
