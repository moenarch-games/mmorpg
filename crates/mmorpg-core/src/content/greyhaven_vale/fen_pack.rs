//! The Eastwater Fen wolf pack (#932): three timber wolves on an open, level meadow of the fen,
//! a Ranger's first group fight. A lead wolf faces the approach from the north, a second
//! stands 6 m south of it and 2.5 m east, and a flanker 6 m west, all inside one assist radius so
//! one pull brings the three. Slowing the lead wolf with Concussive Shot and stepping west lines
//! it up with the second for one Piercing Shot while the flanker runs in off that line. The
//! meadow is level and clear of colliders, roads, water and other creatures. Spawn IDs
//! 1500–1519 are the fen's.

use crate::content::CreatureSpawn;
use crate::{CreatureId, CreatureTemplateId};

use super::units::templates::TIMBER_WOLF;

/// The fen pack's creature spawn IDs.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 1_500..=1_519;
/// The lead wolf, the one a Ranger slows first.
pub const LEAD: CreatureId = CreatureId::new(1_500);
/// The wolf behind the lead one.
pub const FOLLOWER: CreatureId = CreatureId::new(1_501);
/// The wolf off to the west.
pub const FLANKER: CreatureId = CreatureId::new(1_502);
/// Each wolf stays within a metre of its post, so the pack keeps its shape until pulled.
const WANDER_RADIUS_UNITS: i32 = 100;
/// Yaw 0 faces +Z (south); the pack faces north, toward the approach.
const FACING: u16 = 32_768;

#[rustfmt::skip]
const SPAWNS: [(CreatureId, CreatureTemplateId, [i32; 2]); 3] = [
    (LEAD, TIMBER_WOLF, [16_000, 10_600]),
    (FOLLOWER, TIMBER_WOLF, [16_250, 11_200]),
    (FLANKER, TIMBER_WOLF, [15_400, 11_000]),
];

/// The fen pack's spawns, all level 2.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    SPAWNS
        .iter()
        .map(|&(id, template, position)| CreatureSpawn {
            id,
            template,
            position,
            facing: FACING,
            wander_radius: WANDER_RADIUS_UNITS,
            levels: Some([2, 2]),
        })
        .collect()
}
