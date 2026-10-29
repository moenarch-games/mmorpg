//! Greyhaven's first night raid target (#530, Atlas 5.1.57, 5.1.67): Millbrook Farm's barn.
//!
//! Redbrand raiders stand up at three outskirts staging lines out of the farmyard's sight. The
//! north line is on the Stillwater Rim Trail, the east line is in the open rim east of the farm,
//! and the south-east line is beyond the field fence's corner. From there they walk to the barn's
//! west and east walls. Survivors withdraw along the roads. They take the Rim Trail to the lake's
//! south shore, go round to the top of the Lakeshore Climb, down its ledge to the valley floor's
//! northern notch, up the Hollow Climb, and on to Redbrand Hollow's camp.

use crate::content::CreatureSpawn;
use crate::{CreatureId, RaidApproach, RaidSite};

use super::ids;
use super::units::templates::REDBRAND_BANDIT;

/// Raiders are Redbrand bandits at the low end of the Hollow's levels.
const RAIDER_LEVELS: [u8; 2] = [3, 4];
/// Raiders reserved per approach.
const SLOTS: u32 = 6;

/// `(first raider ID, first staging slot, slot step, facing, route to the barn, spread at it, way
/// out to the retreat route)`.
type ApproachRow = (
    u32,
    [i32; 2],
    [i32; 2],
    u16,
    &'static [[i32; 2]],
    [i32; 2],
    &'static [[i32; 2]],
);

#[rustfmt::skip]
const APPROACHES: [ApproachRow; 3] = [
    // North: on the Rim Trail, into the farmyard and to the barn's west wall, clear of the crates
    // at its north-west corner.
    (960, [6_100, 600], [120, 0], 0, &[[6_400, 2_900], [6_900, 3_800], [7_280, 3_900]], [0, 110], &[[6_900, 3_800]]),
    // East: across the open rim to the barn's east wall, its north half. Both east lines leave
    // round the barn's north side, north of the crates by its corners.
    (966, [11_200, 3_500], [0, 120], 49_152, &[[9_300, 3_900], [8_720, 3_800]], [0, 110], &[[8_900, 3_700], [8_900, 3_000]]),
    // South-east: beyond the field fence's corner, to the east wall's south half.
    (972, [10_800, 6_600], [120, 0], 32_768, &[[9_900, 5_300], [8_720, 4_550]], [0, 110], &[[9_000, 4_600], [9_000, 3_000]]),
];

/// From the barn to the camp, last point the camp.
#[rustfmt::skip]
const RETREAT: [[i32; 2]; 14] = [
    [6_400, 3_000],
    [6_400, -2_400],
    [5_600, -3_600],
    [3_400, -3_200],
    [2_600, -3_200],
    [2_600, -6_200],
    [1_400, -6_200],
    [1_400, -2_800],
    [0, -2_800],
    [0, -6_200],
    [-3_600, -6_200],
    [-3_600, -7_000],
    [0, -7_000],
    [0, -8_400],
];

fn slot(row: &ApproachRow, index: u32) -> [i32; 2] {
    let step = i32::try_from(index).unwrap_or(0);
    [row.1[0] + row.2[0] * step, row.1[1] + row.2[1] * step]
}

/// The reserved raiders' content spawns at their staging slots.
pub(super) fn raider_spawns() -> Vec<CreatureSpawn> {
    APPROACHES
        .iter()
        .flat_map(|row| {
            (0..SLOTS).map(move |index| CreatureSpawn {
                id: CreatureId::new(row.0 + index),
                template: REDBRAND_BANDIT,
                position: slot(row, index),
                facing: row.3,
                wander_radius: super::units::WANDER_RADIUS_UNITS,
                levels: Some(RAIDER_LEVELS),
            })
        })
        .collect()
}

/// Millbrook Farm's barn as the raid target.
#[must_use]
pub fn millbrook_barn() -> RaidSite {
    RaidSite {
        name: "Millbrook Farm".to_owned(),
        objective: ids::BARN,
        footprint: [[7_400, 3_600], [8_600, 5_200]],
        approaches: APPROACHES
            .iter()
            .map(|row| RaidApproach {
                raiders: (0..SLOTS)
                    .map(|index| CreatureId::new(row.0 + index))
                    .collect(),
                route: row.4.to_vec(),
                spread: row.5,
                exit: row.6.to_vec(),
            })
            .collect(),
        retreat: RETREAT.to_vec(),
        camp_spread: [100, 0],
    }
}
