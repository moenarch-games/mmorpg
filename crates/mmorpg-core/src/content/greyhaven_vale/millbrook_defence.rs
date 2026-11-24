//! Millbrook Farm's defence (#1019, *Barricades at Millbrook* and *Hold Until Dawn*): two
//! timber barricades players raise across the raiders' north and east approaches, the timber
//! pile they carry the timber from, and the farmfolk who hold the barn's doors in a defence.
//!
//! The north barricade stands across the Rim Trail's descent into the farmyard, the east one
//! across the open rim east of the barn; both are 4 m of knee-high timber the raiders must walk
//! round. They start unbuilt and stand for a while once raised ([`crate::ConstructionSite`]).
//! Marta Mill keeps the barn's west door; Farmhands Cole and Edda the east wall's two doors.

use crate::{
    Civilian, ConstructionSite, LoadSpotId, NpcId, PieceId, StructurePiece, StructureSite,
};

/// The barricades' site: a broken one regrows its damage, never itself from nothing.
pub const SITE: u32 = 2;
/// The north and east barricades.
pub const NORTH_BARRICADE: PieceId = PieceId::new(4);
pub const EAST_BARRICADE: PieceId = PieceId::new(5);
/// Millbrook's timber pile, between the farmhouse and the barn: a load source in
/// `content/greyhaven/quests.json`.
pub const TIMBER_PILE: LoadSpotId = LoadSpotId::new(2);
/// Timber loads one barricade takes.
pub const TIMBER_PER_BARRICADE: u8 = 2;

/// The farmfolk (#1019): Osric's wife and his two farmhands.
pub const MARTA_MILL: NpcId = NpcId::new(46);
pub const FARMHAND_COLE: NpcId = NpcId::new(47);
pub const FARMHAND_EDDA: NpcId = NpcId::new(48);
/// What a civilian can take: about a minute and a half of one raider's blows.
pub const CIVILIAN_HEALTH: u32 = 240;

/// `(id, name, level, feet XZ, facing)` of the farmfolk, each at its door.
#[rustfmt::skip]
const FARMFOLK: [(u32, &str, u8, [i32; 2], u16); 3] = [
    // At the barn's west door, beside the north approach's end, facing out (west).
    (46, "Marta Mill", 3, [7_150, 4_100], 49_152),
    // At the east wall's north door, beside the east approach's end, facing out (east).
    (47, "Farmhand Cole", 2, [8_850, 3_950], 16_384),
    // At the east wall's south door, beside the south-east approach's end.
    (48, "Farmhand Edda", 2, [8_850, 4_750], 16_384),
];

/// The barricades' site.
#[must_use]
pub fn site() -> StructureSite {
    StructureSite {
        id: SITE,
        name: "Millbrook Farm".to_owned(),
        regrowth_ticks: crate::long_ticks_from_ms(300_000),
    }
}

/// The two barricades: 4 m long, 1.2 m high and 0.5 m thick, on the ground.
#[must_use]
pub fn barricades() -> Vec<StructurePiece> {
    let piece =
        |id: PieceId, body: u32, [x, z]: [i32; 2], half: [i32; 2], name: &str| StructurePiece {
            id,
            site: SITE,
            body,
            position: [x, super::VALE_DEFINITION.ground_at(x, z) + 60, z],
            half_extents: [half[0], 60, half[1]],
            health: 90,
            supports: Vec::new(),
            name: name.to_owned(),
            rubble: None,
        };
    let [north, east] = [
        *super::ids::MILLBROOK_BARRICADES.start(),
        *super::ids::MILLBROOK_BARRICADES.end(),
    ];
    vec![
        // Across the Rim Trail's descent, between its two last bends.
        piece(
            NORTH_BARRICADE,
            north,
            [6_650, 3_350],
            [200, 25],
            "the north barricade",
        ),
        // Across the open rim east of the barn.
        piece(
            EAST_BARRICADE,
            east,
            [9_050, 3_850],
            [25, 200],
            "the east barricade",
        ),
    ]
}

/// Both barricades take two loads of the timber pile's.
#[must_use]
pub fn constructions() -> Vec<ConstructionSite> {
    [NORTH_BARRICADE, EAST_BARRICADE]
        .map(|piece| ConstructionSite {
            piece,
            source: TIMBER_PILE,
            materials: TIMBER_PER_BARRICADE,
        })
        .to_vec()
}

/// The farmfolk as residents, at their doors.
pub(super) fn npcs() -> Vec<crate::Npc> {
    FARMFOLK
        .iter()
        .map(|&(id, name, level, position, facing)| crate::Npc {
            id: NpcId::new(id),
            name: name.to_owned(),
            role: crate::NpcRole::Resident,
            level,
            position,
            facing,
        })
        .collect()
}

/// The farmfolk as the raid site's civilians, at their doors.
#[must_use]
pub fn civilians() -> Vec<Civilian> {
    FARMFOLK
        .iter()
        .map(|&(id, _, _, at, _)| Civilian {
            npc: NpcId::new(id),
            at,
            health: CIVILIAN_HEALTH,
        })
        .collect()
}
