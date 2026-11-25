//! The missing family and the granary (#1019, slice 4c): the western hamlet's last four stories.
//!
//! - **The Tollys**: Hal, Wenna and young Bram Tolly left their cottage, the one east of the lane
//!   beside Ena's farmhouse, before dawn and hid in the hollow by the plateau's west wall. They stand
//!   there, by their handcart, until a player leads them home: Hal follows the player's trail and
//!   Wenna and Bram walk behind him in a line ([`crate::EscortCompanion`]), to Goodwife Ena.
//! - **The granary**: Ena's barn, the hamlet's grain store, is a second raid site
//!   ([`crate::RaidSite`]) that only *The Granary Stand*'s defence raids. Redbrands stand up north
//!   of the hamlet and south of it, out of its sight, and walk to the barn's west and south walls.
//!   Three granary hands guard the stores at its doors; two must stand. Survivors withdraw north
//!   of the west fields toward the plateau's north-west corner and leave the world there.
//!   Raiders are the hamlets' spawns 1420-1431.

use crate::content::CreatureSpawn;
use crate::{
    AidKind, AidTask, Civilian, CreatureId, EscortCompanion, Npc, NpcId, NpcRole, RaidApproach,
    RaidSite,
};

use super::units::templates::REDBRAND_BANDIT;

/// Goodwife Ena, who keeps the granary and gives the four quests.
pub const GOODWIFE_ENA: NpcId = NpcId::new(36);
/// The Tollys: Hal leads, Wenna follows him and Bram follows her.
pub const HAL_TOLLY: NpcId = NpcId::new(54);
pub const WENNA_TOLLY: NpcId = NpcId::new(55);
pub const BRAM_TOLLY: NpcId = NpcId::new(56);
/// The granary hands who guard the stores in a defence.
pub const GRANARY_HANDS: [NpcId; 3] = [NpcId::new(57), NpcId::new(58), NpcId::new(59)];
/// Ena's barn, the western hamlet's fifth farmland building.
pub const GRANARY: u32 = *super::ids::FARMLAND_BUILDINGS.start() + 4;
/// The granary raiders' spawn IDs.
pub const RAIDER_IDS: std::ops::RangeInclusive<u32> = 1_420..=1_431;
/// What a granary hand can take, as Millbrook's farmfolk.
pub const HAND_HEALTH: u32 = 240;

const SOUTH: u16 = 0;
const EAST: u16 = 16_384;
const WEST: u16 = 49_152;

/// `(id, name, level, feet XZ, facing)` of the Tollys in the hollow and the granary hands at the
/// barn's doors: one by the west door, two by the south doors.
#[rustfmt::skip]
const PEOPLE: [(u32, &str, u8, [i32; 2], u16); 6] = [
    (54, "Hal Tolly", 3, [-17_400, 41_000], EAST),
    (55, "Wenna Tolly", 2, [-17_650, 41_200], EAST),
    (56, "Bram Tolly", 1, [-17_450, 40_750], EAST),
    (57, "Granary Hand Oda", 2, [-11_650, 35_600], WEST),
    (58, "Granary Hand Piet", 2, [-10_950, 36_650], SOUTH),
    (59, "Granary Hand Wilm", 2, [-10_450, 36_650], SOUTH),
];

/// The Tollys and the granary hands: residents without routines.
#[must_use]
pub fn npcs() -> Vec<Npc> {
    PEOPLE
        .iter()
        .map(|&(id, name, level, position, facing)| Npc {
            id: NpcId::new(id),
            name: name.to_owned(),
            role: NpcRole::Resident,
            level,
            position,
            facing,
        })
        .collect()
}

/// Wenna follows Hal and Bram follows his mother.
#[must_use]
pub fn companions() -> Vec<EscortCompanion> {
    vec![
        EscortCompanion {
            npc: WENNA_TOLLY,
            follows: HAL_TOLLY,
        },
        EscortCompanion {
            npc: BRAM_TOLLY,
            follows: WENNA_TOLLY,
        },
    ]
}

/// Raiders are Redbrand bandits at the low end of the Hollow's levels, as at Millbrook.
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
const APPROACHES: [ApproachRow; 2] = [
    // North: out of sight beyond the hamlet's north edge, then down past Ploughman Garr's furrows,
    // west of the houses, to the barn's west wall.
    (1_420, [-12_200, 28_200], [120, 0], 0, &[[-11_900, 31_000], [-11_900, 34_400], [-11_520, 35_100]], [0, 110], &[[-11_900, 34_400]]),
    // South: up from the rolling land south of the hamlet to the barn's south wall.
    (1_426, [-11_300, 40_600], [120, 0], 32_768, &[[-10_900, 38_200], [-11_050, 36_520]], [110, 0], &[[-11_000, 37_800], [-11_900, 37_600]]),
];

/// From the barn round the hamlet's west side and north of the west fields, last point the
/// hollow in the plateau's north-west corner where survivors gather before they leave.
#[rustfmt::skip]
const RETREAT: [[i32; 2]; 3] = [
    [-12_000, 29_800],
    [-15_000, 29_200],
    [-18_400, 28_800],
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

/// The granary as a quest defence site: Ena starts its defence, the hands guard its stores, and a
/// defence leaves breaches in its west and south walls to mend.
#[must_use]
pub fn site() -> RaidSite {
    RaidSite {
        name: "Ena's granary".to_owned(),
        objective: GRANARY,
        footprint: [[-11_400, 34_800], [-10_200, 36_400]],
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
        camp_spread: [0, 100],
        aid: vec![
            AidTask {
                kind: AidKind::MendBreach,
                at: [-11_550, 35_000],
            },
            AidTask {
                kind: AidKind::MendBreach,
                at: [-10_700, 36_550],
            },
        ],
        // No night raid comes here, so nothing is forewarned or braced: the tracks and the
        // lookout are only ever the night raid target's.
        tracks: RETREAT[0],
        lookout: GRANARY_HANDS[0],
        braces: Vec::new(),
        civilians: PEOPLE[3..]
            .iter()
            .map(|&(id, _, _, at, _)| Civilian {
                npc: NpcId::new(id),
                at,
                health: HAND_HEALTH,
            })
            .collect(),
        steward: Some(GOODWIFE_ENA),
    }
}
