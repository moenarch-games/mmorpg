//! The village and the river (#1019, slice 6a): Fenford's stories by the Fenwater's ford.
//!
//! - **The brake wheel**: Miller Wynne's windmill cracked its brake wheel. The millwright's new
//!   one came crated on the village cart by the Ford Inn; it starts unfitted at the foot of the
//!   mill's north face and stands once its crate is set down there ([`crate::ConstructionSite`]),
//!   for ten minutes like the farmland's hurdles, so every character fits it for themselves.
//! - **The watchman**: Greyfang Watch posted Watchman Corwin on the ford's north bank, where
//!   Fenford Lane comes down to the water. He hears the smuggler's token handed over.
//! - **The boatman**: Boatman Teague moors on the still reach above the ford. The shipment hidden
//!   in the alder roots upstream of him is his; the Watch takes him if the token goes to it.
//!
//! Osk's rope block on the ford's bed, the ford's three stakes, the shipment's hiding place and
//! the alder spring Hob Ferris sends players to are evidence spots and load sources in
//! `content/greyhaven/quests.json`.

use crate::{
    ConstructionSite, LoadSpotId, Npc, NpcId, NpcRole, PieceId, PieceMaterial, StructurePiece,
    StructureSite,
};

/// The mill's structure site and its brake wheel.
pub const MILL_SITE: u32 = 6;
pub const BRAKE_WHEEL: PieceId = PieceId::new(10);
/// The crated wheel on the village cart: a load source in `content/greyhaven/quests.json`.
pub const CRATED_WHEEL: LoadSpotId = LoadSpotId::new(6);
/// The jug filled at the alder spring, upstream in the Fenwater Alders.
pub const SPRING_JUG: LoadSpotId = LoadSpotId::new(7);

/// The Watch's man at the ford and the shipment's owner.
pub const WATCHMAN_CORWIN: NpcId = NpcId::new(62);
pub const BOATMAN_TEAGUE: NpcId = NpcId::new(63);

/// The wheel's feet, against the windmill's north face (the mill stands 10 m square on the yard,
/// its north face at z 23 300), west of Wynne.
const WHEEL_AT: [i32; 2] = [101_450, 23_250];
/// Half its width, height and thickness: a 1.4 m wheel on its frame.
const WHEEL_HALF: [i32; 3] = [70, 70, 20];

/// The mill's site: the wheel does not regrow by itself.
#[must_use]
pub fn sites() -> Vec<StructureSite> {
    vec![StructureSite {
        id: MILL_SITE,
        name: "Wynne's Mill".to_owned(),
        regrowth_ticks: crate::long_ticks_from_ms(300_000),
    }]
}

/// The brake wheel, a timber piece standing on its frame at the mill's foot once fitted.
#[must_use]
pub fn pieces() -> Vec<StructurePiece> {
    let [x, z] = WHEEL_AT;
    vec![StructurePiece {
        id: BRAKE_WHEEL,
        site: MILL_SITE,
        body: super::ids::MILL_BRAKE_WHEEL,
        position: [x, super::VALE_DEFINITION.ground_at(x, z) + WHEEL_HALF[1], z],
        half_extents: WHEEL_HALF,
        health: 60,
        supports: Vec::new(),
        name: "the mill's brake wheel".to_owned(),
        rubble: None,
        material: PieceMaterial::Timber,
    }]
}

/// The wheel takes its one crate from the cart.
#[must_use]
pub fn constructions() -> Vec<ConstructionSite> {
    vec![ConstructionSite {
        piece: BRAKE_WHEEL,
        source: CRATED_WHEEL,
        materials: 1,
    }]
}

/// Watchman Corwin, by the lane's foot facing the ford; Boatman Teague on the bank above it,
/// facing his mooring. Corwin hears the token, Teague answers for his shipment.
pub(super) fn npcs() -> Vec<Npc> {
    vec![
        Npc {
            id: WATCHMAN_CORWIN,
            name: "Watchman Corwin".to_owned(),
            role: NpcRole::Guard,
            level: 6,
            position: [107_700, 27_300],
            facing: 0,
        },
        Npc {
            id: BOATMAN_TEAGUE,
            name: "Boatman Teague".to_owned(),
            role: NpcRole::Resident,
            level: 4,
            position: [104_800, 27_100],
            facing: 0,
        },
    ]
}
