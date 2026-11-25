//! The village and the river (#1019, slices 6a and 6b): Fenford's stories by the Fenwater's ford.
//!
//! - **The brake wheel**: Miller Wynne's windmill cracked its brake wheel. The millwright's new
//!   one came crated on the village cart by the Ford Inn; it starts unfitted at the foot of the
//!   mill's north face and stands once its crate is set down there ([`crate::ConstructionSite`]),
//!   for ten minutes like the farmland's hurdles, so every character fits it for themselves.
//! - **The watchman**: Greyfang Watch posted Watchman Corwin on the ford's north bank, where
//!   Fenford Lane comes down to the water. He hears the smuggler's token handed over.
//! - **The boatman**: Boatman Teague moors on the still reach above the ford. The shipment hidden
//!   in the alder roots upstream of him is his; the Watch takes him if the token goes to it.
//! - **The carter** (slice 6b): Carter Aldo waits with his cart on the Fen Road's south bank and
//!   drives it over the ford ([`ROUTE`], about 47 m) when asked ([`crate::WagonCrossing`]). Two
//!   Redbrand Bandits break from the south bank's reeds six seconds in and a third comes down the
//!   north-east bank eighteen seconds in, all for the cart.
//! - **The barricade** (slice 6b): a timber barricade on the north bank east of the ford's stakes,
//!   across the third raider's way down to the water, raised from the hurdles stacked by the
//!   lane's foot and standing ten minutes like the farmland's hurdles.
//! - **The lure** (slice 6b): three casting stakes on the bank's margin, 2.5 m downstream of the
//!   river crayfish, where Netter Ilsa's gut-baited line is cast ([`crate::Lure`]): a crayfish
//!   within 6 m turns to the bait, its side to the bank.
//! - **The reed-cutter** (slice 6b): Ilsa's sister Nell went up the Fenwater to cut reeds at the
//!   Marsh Lake and sheltered on the drowned watchtower's island. The lake's deep ring round the
//!   island stands between her and home; she walks the causeway and the ankle-deep shelf, but
//!   will not swim.
//!
//! Osk's rope block on the ford's bed, the ford's three stakes, the shipment's hiding place, the
//! casting stakes, the alder spring Hob Ferris sends players to and the lane's hurdles are
//! evidence spots and load sources in `content/greyhaven/quests.json`.

use crate::{
    ConstructionSite, CreatureId, CreatureSpawn, CrossingRaider, LoadSpotId, Lure, Npc, NpcId,
    NpcRole, PieceId, PieceMaterial, StructurePiece, StructureSite, WagonCrossing,
};

use super::quests::{CRAYFISH_LURE, LURE_STONES};
use super::units::templates::{REDBRAND_BANDIT, RIVER_CRAYFISH};

/// The mill's structure site and its brake wheel.
pub const MILL_SITE: u32 = 6;
pub const BRAKE_WHEEL: PieceId = PieceId::new(10);
/// The crated wheel on the village cart: a load source in `content/greyhaven/quests.json`.
pub const CRATED_WHEEL: LoadSpotId = LoadSpotId::new(6);
/// The jug filled at the alder spring, upstream in the Fenwater Alders.
pub const SPRING_JUG: LoadSpotId = LoadSpotId::new(7);

/// The ford's structure site and its barricade, raised from the hurdles by the lane's foot.
pub const FORD_SITE: u32 = 7;
pub const FORD_BARRICADE: PieceId = PieceId::new(11);
pub const LANE_HURDLES: LoadSpotId = LoadSpotId::new(8);

/// The Watch's man at the ford and the shipment's owner.
pub const WATCHMAN_CORWIN: NpcId = NpcId::new(62);
pub const BOATMAN_TEAGUE: NpcId = NpcId::new(63);
/// The carter on the south bank and the reed-cutter on the Marsh Lake's island.
pub const CARTER_ALDO: NpcId = NpcId::new(64);
pub const REEDCUTTER_NELL: NpcId = NpcId::new(65);

/// The crossing's raiders: two bandits from the south bank's reeds, one down the north-east bank.
pub const SPAWN_IDS: std::ops::RangeInclusive<u32> = 2_960..=2_962;
pub const REED_BANDITS: [CreatureId; 2] = [CreatureId::new(2_960), CreatureId::new(2_961)];
pub const BANK_BANDIT: CreatureId = CreatureId::new(2_962);

/// The cart's way: Aldo's post on the Fen Road's south bank, down to the south stake's approach,
/// through the water at the middle stake, up past the north stake and off the bank by the lane's
/// foot.
pub const ROUTE: [[i32; 2]; 5] = [
    [105_800, 31_200],
    [107_600, 29_400],
    [108_200, 28_800],
    [108_200, 28_150],
    [107_800, 27_700],
];
/// The cart's health: two bandits at it unhindered break it in about half a minute.
pub const CART_HEALTH: u32 = 200;
/// Players within 25 m of the cart when it reaches the bank see it over.
pub const CROSSING_RADIUS_UNITS: i32 = 2_500;

/// The wheel's feet, against the windmill's north face (the mill stands 10 m square on the yard,
/// its north face at z 23 300), west of Wynne.
const WHEEL_AT: [i32; 2] = [101_450, 23_250];
/// Half its width, height and thickness: a 1.4 m wheel on its frame.
const WHEEL_HALF: [i32; 3] = [70, 70, 20];
/// The barricade's feet on the north bank, east of the stakes, and its half extents: 4 m long,
/// 1.2 m high and 0.5 m thick, across the bank.
const BARRICADE_AT: [i32; 2] = [109_300, 28_250];
const BARRICADE_HALF: [i32; 3] = [200, 60, 25];

/// Yaw 0 faces +Z (south), 32 768 north, 16 384 east and 49 152 west.
const NORTH: u16 = 32_768;
const WEST: u16 = 49_152;
/// Aldo faces down the road to the ford (north-east).
const TO_THE_FORD: u16 = 24_576;

/// The mill's site and the ford's: neither piece regrows by itself.
#[must_use]
pub fn sites() -> Vec<StructureSite> {
    vec![
        StructureSite {
            id: MILL_SITE,
            name: "Wynne's Mill".to_owned(),
            regrowth_ticks: crate::long_ticks_from_ms(300_000),
        },
        StructureSite {
            id: FORD_SITE,
            name: "Fenford's Ford".to_owned(),
            regrowth_ticks: crate::long_ticks_from_ms(300_000),
        },
    ]
}

/// The brake wheel, a timber piece standing on its frame at the mill's foot once fitted, and the
/// ford's timber barricade.
#[must_use]
pub fn pieces() -> Vec<StructurePiece> {
    let piece =
        |id, site, body, [x, z]: [i32; 2], half: [i32; 3], health, name: &str| StructurePiece {
            id,
            site,
            body,
            position: [x, super::VALE_DEFINITION.ground_at(x, z) + half[1], z],
            half_extents: half,
            health,
            supports: Vec::new(),
            name: name.to_owned(),
            rubble: None,
            material: PieceMaterial::Timber,
        };
    vec![
        piece(
            BRAKE_WHEEL,
            MILL_SITE,
            super::ids::MILL_BRAKE_WHEEL,
            WHEEL_AT,
            WHEEL_HALF,
            60,
            "the mill's brake wheel",
        ),
        piece(
            FORD_BARRICADE,
            FORD_SITE,
            super::ids::FORD_BARRICADE,
            BARRICADE_AT,
            BARRICADE_HALF,
            90,
            "the ford barricade",
        ),
    ]
}

/// The wheel takes its one crate from the cart; the barricade two loads of the lane's hurdles.
#[must_use]
pub fn constructions() -> Vec<ConstructionSite> {
    vec![
        ConstructionSite {
            piece: BRAKE_WHEEL,
            source: CRATED_WHEEL,
            materials: 1,
        },
        ConstructionSite {
            piece: FORD_BARRICADE,
            source: LANE_HURDLES,
            materials: 2,
        },
    ]
}

/// Watchman Corwin, by the lane's foot facing the ford; Boatman Teague on the bank above it,
/// facing his mooring; Carter Aldo with his cart on the south bank; Nell on the lake's island.
/// Corwin hears the token, Teague answers for his shipment.
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
        Npc {
            id: CARTER_ALDO,
            name: "Carter Aldo".to_owned(),
            role: NpcRole::Resident,
            level: 4,
            position: ROUTE[0],
            facing: TO_THE_FORD,
        },
        Npc {
            id: REEDCUTTER_NELL,
            name: "Reedcutter Nell".to_owned(),
            role: NpcRole::Resident,
            level: 2,
            position: [47_400, -2_600],
            facing: WEST,
        },
    ]
}

/// The crossing's raiders: reserved off the world until a crossing stands them.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    [
        (REED_BANDITS[0], [106_300, 29_300], NORTH),
        (REED_BANDITS[1], [104_800, 29_100], NORTH),
        (BANK_BANDIT, [110_600, 27_900], WEST),
    ]
    .into_iter()
    .map(|(id, position, facing)| CreatureSpawn {
        id,
        template: REDBRAND_BANDIT,
        position,
        facing,
        wander_radius: 0,
        levels: Some([4, 4]),
    })
    .collect()
}

/// Carter Aldo's crossing of the ford.
#[must_use]
pub fn crossing() -> WagonCrossing {
    WagonCrossing {
        npc: CARTER_ALDO,
        route: ROUTE.to_vec(),
        health: CART_HEALTH,
        raiders: vec![
            CrossingRaider {
                creature: REED_BANDITS[0],
                after_ticks: crate::long_ticks_from_ms(6_000),
            },
            CrossingRaider {
                creature: REED_BANDITS[1],
                after_ticks: crate::long_ticks_from_ms(6_000),
            },
            CrossingRaider {
                creature: BANK_BANDIT,
                after_ticks: crate::long_ticks_from_ms(18_000),
            },
        ],
        radius: CROSSING_RADIUS_UNITS,
    }
}

/// The casting stakes by the three crayfish, each drawing a river crayfish to Ilsa's lure.
#[must_use]
pub fn lures() -> Vec<Lure> {
    LURE_STONES
        .iter()
        .map(|&spot| Lure {
            spot,
            item: CRAYFISH_LURE,
            template: RIVER_CRAYFISH,
        })
        .collect()
}
