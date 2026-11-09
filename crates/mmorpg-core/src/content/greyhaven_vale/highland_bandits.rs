//! The Bandit Highlands' Redbrand garrison (#502, follows #425): groups at the stronghold, along
//! the Highland Trail and a crossbowman at the Lookout tower, on the existing Redbrand templates,
//! figures, AI, leash, respawn and loot rules.
//!
//! The templates' level ranges are inherited, so the region is hard only at their top end: the
//! stronghold and the lookout roll level 5, the trail levels 4–5. Spawn IDs 1000–1099 are the
//! region's, after the night raiders.
//!
//! Spawn points are set so no highland tree or rock moves, with the whole wander disc clear of
//! every collider. A viewer anywhere in the yard must stay well inside the protocol's
//! 35-entity packet budget with room for other players (`tests/highland_bandits.rs`).

use crate::content::CreatureSpawn;
use crate::{CreatureId, CreatureTemplateId};

use super::units::templates::{REDBRAND_BANDIT, REDBRAND_BRUTE, REDBRAND_CROSSBOWMAN};

/// Garrison creatures idle within 3 m of their spawn point, clear of the camp's structures.
pub const WANDER_RADIUS_UNITS: i32 = 300;
/// The Lookout crossbowman keeps to the tower's foot.
pub const LOOKOUT_WANDER_RADIUS_UNITS: i32 = 200;

/// The level rolled at the stronghold and the lookout: the templates' top.
const SEASONED: [u8; 2] = [5, 5];
/// The level range rolled along the trail.
const TRAIL: [u8; 2] = [4, 5];

/// Yaw 0 faces +Z (south, toward the Vale), 32 768 north, 16 384 east and 49 152 west.
const SOUTH: u16 = 0;
const EAST: u16 = 16_384;
const NORTH: u16 = 32_768;
const WEST: u16 = 49_152;

type Row = (u32, CreatureTemplateId, [i32; 2], u16, [u8; 2], i32);

#[rustfmt::skip]
const SPAWNS: [Row; 25] = {
    const W: i32 = WANDER_RADIUS_UNITS;
    [
        // Stronghold: round the campfire …
        (1000, REDBRAND_BANDIT, [-1_800, -52_000], EAST, SEASONED, W),
        (1001, REDBRAND_BANDIT, [-600, -52_100], WEST, SEASONED, W),
        (1002, REDBRAND_CROSSBOWMAN, [-1_200, -52_700], SOUTH, SEASONED, W),
        // … among the tents …
        (1003, REDBRAND_BANDIT, [-2_300, -52_600], EAST, SEASONED, W),
        (1004, REDBRAND_BANDIT, [2_300, -52_600], WEST, SEASONED, W),
        (1005, REDBRAND_CROSSBOWMAN, [2_300, -53_800], WEST, SEASONED, W),
        (1006, REDBRAND_BANDIT, [-2_400, -54_300], EAST, SEASONED, W),
        // … the Brutes guarding the keep's door, crossbowmen behind it …
        (1007, REDBRAND_BRUTE, [-700, -53_900], SOUTH, SEASONED, W),
        (1008, REDBRAND_BRUTE, [900, -53_700], SOUTH, SEASONED, W),
        (1009, REDBRAND_CROSSBOWMAN, [-900, -55_600], EAST, SEASONED, W),
        (1010, REDBRAND_CROSSBOWMAN, [1_100, -55_600], WEST, SEASONED, W),
        // … and sentries inside the gate.
        (1011, REDBRAND_BANDIT, [-900, -50_300], SOUTH, SEASONED, W),
        (1012, REDBRAND_BANDIT, [900, -50_300], SOUTH, SEASONED, W),
        // Outside the gate, on the trail's last leg.
        (1013, REDBRAND_BANDIT, [-900, -48_300], SOUTH, TRAIL, W),
        (1014, REDBRAND_BANDIT, [800, -48_200], SOUTH, TRAIL, W),
        // Highland Trail: past the gorge's top …
        (1020, REDBRAND_BANDIT, [-2_000, -34_800], SOUTH, TRAIL, W),
        (1021, REDBRAND_BANDIT, [-2_900, -36_100], EAST, TRAIL, W),
        (1022, REDBRAND_CROSSBOWMAN, [-1_300, -35_900], WEST, TRAIL, W),
        // … at the bend north, a Brute among them …
        (1023, REDBRAND_BANDIT, [-7_300, -40_700], WEST, TRAIL, W),
        (1024, REDBRAND_CROSSBOWMAN, [-8_800, -41_200], EAST, TRAIL, W),
        (1025, REDBRAND_BRUTE, [-7_500, -42_000], SOUTH, SEASONED, W),
        // … and on the last rise before the stronghold.
        (1026, REDBRAND_BANDIT, [-4_200, -44_900], SOUTH, TRAIL, W),
        (1027, REDBRAND_CROSSBOWMAN, [-3_400, -46_100], NORTH, TRAIL, W),
        (1028, REDBRAND_BANDIT, [-5_000, -46_000], EAST, TRAIL, W),
        // A crossbowman at the Lookout tower's foot, watching the path.
        (1030, REDBRAND_CROSSBOWMAN, [12_900, -38_300], SOUTH, SEASONED, LOOKOUT_WANDER_RADIUS_UNITS),
    ]
};

/// Each garrison spawn point and its wander radius, without allocating.
pub(super) fn wander_discs() -> impl Iterator<Item = ([i32; 2], i32)> {
    SPAWNS
        .iter()
        .map(|&(_, _, position, _, _, wander_radius)| (position, wander_radius))
}

/// The garrison's content spawns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    SPAWNS
        .iter()
        .map(
            |&(id, template, position, facing, levels, wander_radius)| CreatureSpawn {
                id: CreatureId::new(id),
                template,
                position,
                facing,
                wander_radius,
                levels: Some(levels),
            },
        )
        .collect()
}
