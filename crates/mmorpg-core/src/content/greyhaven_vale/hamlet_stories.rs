//! The farmland's delivery, repair and village stories (#1019, slice 4a): the watermill's
//! choked sluice on the village brook, the hurdles of Shepherd Lise's fold in the eastern hamlet
//! and the watchman at the farmland's landmark tower.
//!
//! - **The sluice**: a stone stop jammed across the brook above Millbrook's watermill, a
//!   [`crate::Seal`] with one mechanism, the sluice gate on the east bank. Working the gate sinks
//!   the stop and the brook runs free to the wheel; five minutes on, once nobody stands at it,
//!   silt and branches choke it again, so every character clears it for themselves.
//! - **The hurdles**: two knee-high timber hurdles closing the north and east sides of Lise's fold,
//!   east of her house. They start unbuilt and stand once a load of timber from the hamlet's barn
//!   is set down at each ([`crate::ConstructionSite`]), for ten minutes like Millbrook's barricades.
//! - **The watchman**: Watchman Dunstan keeps the tower's yard beside the farm road and takes the
//!   bread the southern hamlet sends up.

use crate::{
    ConstructionSite, LoadSpotId, Mechanism, MechanismId, Npc, NpcId, NpcRole, PieceId,
    PieceMaterial, Seal, StructurePiece, StructureSite,
};

/// The sluice's structure site and its stop.
pub const SLUICE_SITE: u32 = 4;
pub const SLUICE_STOP: PieceId = PieceId::new(7);
/// The sluice gate that sinks it.
pub const SLUICE_GATE: MechanismId = MechanismId::new(4);
/// The stop across the brook, between the last bend above the mill and its wheel.
const SLUICE_AT: [i32; 2] = [-735, 6_200];
/// Half its width across the brook (the brook is 1.8 m wide), height and depth along it.
const SLUICE_HALF: [i32; 3] = [110, 45, 30];

/// The fold's structure site and its hurdles.
pub const FOLD_SITE: u32 = 5;
pub const NORTH_HURDLE: PieceId = PieceId::new(8);
pub const EAST_HURDLE: PieceId = PieceId::new(9);
/// The eastern hamlet's timber, south of its barn: a load source in
/// `content/greyhaven/quests.json`.
pub const HAMLET_TIMBER: LoadSpotId = LoadSpotId::new(4);

/// The watchman at the farmland's tower.
pub const WATCHMAN_DUNSTAN: NpcId = NpcId::new(49);

/// Ground height at `(x, z)`.
fn ground(x: i32, z: i32) -> i32 {
    super::VALE_DEFINITION.ground_at(x, z)
}

/// The sluice's and the fold's sites: neither regrows by itself.
#[must_use]
pub fn sites() -> Vec<StructureSite> {
    vec![
        StructureSite {
            id: SLUICE_SITE,
            name: "Millbrook Watermill".to_owned(),
            regrowth_ticks: crate::long_ticks_from_ms(300_000),
        },
        StructureSite {
            id: FOLD_SITE,
            name: "Lise's Fold".to_owned(),
            regrowth_ticks: crate::long_ticks_from_ms(300_000),
        },
    ]
}

/// The sluice's stop, standing in the brook's bed, and the fold's two hurdles, 4 m long, 1.2 m
/// high and 0.5 m thick.
#[must_use]
pub fn pieces() -> Vec<StructurePiece> {
    let [x, z] = SLUICE_AT;
    let stop = StructurePiece {
        id: SLUICE_STOP,
        site: SLUICE_SITE,
        body: super::ids::SLUICE_STOP,
        position: [x, ground(x, z) + SLUICE_HALF[1], z],
        half_extents: SLUICE_HALF,
        health: 1,
        supports: Vec::new(),
        name: "the mill sluice".to_owned(),
        rubble: None,
        material: PieceMaterial::Stone,
    };
    let hurdle = |id, body, [x, z]: [i32; 2], half: [i32; 2], name: &str| StructurePiece {
        id,
        site: FOLD_SITE,
        body,
        position: [x, ground(x, z) + 60, z],
        half_extents: [half[0], 60, half[1]],
        health: 90,
        supports: Vec::new(),
        name: name.to_owned(),
        rubble: None,
        material: PieceMaterial::Timber,
    };
    let [north, east] = [
        *super::ids::FOLD_HURDLES.start(),
        *super::ids::FOLD_HURDLES.end(),
    ];
    vec![
        stop,
        hurdle(
            NORTH_HURDLE,
            north,
            [15_200, 45_900],
            [200, 25],
            "the fold's north hurdle",
        ),
        hurdle(
            EAST_HURDLE,
            east,
            [15_425, 46_150],
            [25, 200],
            "the fold's east hurdle",
        ),
    ]
}

/// The sluice gate on the brook's east bank beside the stop.
#[must_use]
pub fn mechanisms() -> Vec<Mechanism> {
    let [x, z] = SLUICE_AT;
    let x = x + 200;
    vec![Mechanism {
        id: SLUICE_GATE,
        name: "the sluice gate".to_owned(),
        at: [x, ground(x, z), z],
        reach: 200,
    }]
}

/// The sluice: its gate alone sinks the stop; a player standing at the stop holds it open.
#[must_use]
pub fn seal() -> Seal {
    let [x, z] = SLUICE_AT;
    let floor = ground(x, z);
    Seal {
        piece: SLUICE_STOP,
        mechanisms: vec![SLUICE_GATE],
        chamber: (
            [x - 200, floor - 50, z - 200],
            [x + 200, floor + 250, z + 200],
        ),
    }
}

/// Each hurdle takes one load of the hamlet's timber.
#[must_use]
pub fn constructions() -> Vec<ConstructionSite> {
    [NORTH_HURDLE, EAST_HURDLE]
        .map(|piece| ConstructionSite {
            piece,
            source: HAMLET_TIMBER,
            materials: 1,
        })
        .to_vec()
}

/// Watchman Dunstan, east of the tower and facing the farm road; he takes his quest's turn-in.
pub(super) fn npcs() -> Vec<Npc> {
    vec![Npc {
        id: WATCHMAN_DUNSTAN,
        name: "Watchman Dunstan".to_owned(),
        role: NpcRole::QuestGiver,
        level: 6,
        position: [1_700, 44_000],
        facing: 16_384,
    }]
}
