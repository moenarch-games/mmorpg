//! A Redbrand patrol on the Greyfang Approach (#911): a Brute holding the front, a Bandit flanking
//! and a Crossbowman shooting from behind, posted beside the road where it climbs onto Greyfang
//! Ridge, so a player heading for the cavern's north mouth meets them. They fight by their [`crate::PatrolRole`]s on the existing
//! Redbrand templates, attacks, leash, aggro and family assist; idle, they wander close together,
//! so one pull brings all three.

use crate::content::CreatureSpawn;
use crate::{CreatureId, CreatureTemplateId, PatrolMember, PatrolRole};

use super::units::templates::{REDBRAND_BANDIT, REDBRAND_BRUTE, REDBRAND_CROSSBOWMAN};

/// The patrol's creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 1_190..=1_192;
/// Patrol members idle within 2 m of their posts.
pub const WANDER_RADIUS_UNITS: i32 = 200;
/// The Greyfang Approach patrol's number.
pub const GREYFANG_APPROACH: u16 = 1;

/// Yaw 49 152 faces west (-X), toward the road.
const WEST: u16 = 49_152;

/// `(spawn id, template, role, feet XZ, levels)`.
type Row = (u32, CreatureTemplateId, PatrolRole, [i32; 2], [u8; 2]);

#[rustfmt::skip]
const MEMBERS: [Row; 3] = [
    (1_190, REDBRAND_BRUTE, PatrolRole::Bruiser, [10_400, -12_300], [5, 5]),
    (1_191, REDBRAND_BANDIT, PatrolRole::Skirmisher, [10_750, -12_150], [4, 5]),
    (1_192, REDBRAND_CROSSBOWMAN, PatrolRole::Marksman, [11_000, -12_600], [4, 5]),
];

/// The patrol's creature spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    MEMBERS
        .iter()
        .map(|&(id, template, _, position, levels)| CreatureSpawn {
            id: CreatureId::new(id),
            template,
            position,
            facing: WEST,
            wander_radius: WANDER_RADIUS_UNITS,
            levels: Some(levels),
        })
        .collect()
}

/// The patrol's roles.
#[must_use]
pub fn members() -> Vec<PatrolMember> {
    MEMBERS
        .iter()
        .map(|&(id, _, role, ..)| PatrolMember {
            creature: CreatureId::new(id),
            patrol: GREYFANG_APPROACH,
            role,
        })
        .collect()
}
