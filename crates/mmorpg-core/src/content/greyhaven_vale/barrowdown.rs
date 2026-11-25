//! Barrowdown's ring and barrow (#1019, slice 5a): Old Maud's standing stones west of the hamlet
//! and the grave robber who dug at them.
//!
//! - **The ring's measure**: four of the Barrowdown Ring's eight standing stones are
//!   [`crate::Mechanism`]s of one [`crate::MechanismSequence`]. The lintel at the ring's heart
//!   names their order, sunrise to the dark: the dawn stone in the east, the noon stone in the
//!   south, the dusk stone in the west and the night stone in the north. Each character works the
//!   order for itself; a stone out of turn starts it over.
//! - **The grave robber**: Cobb the Digger crouches over his diggings in the downs west of the
//!   ring with the barrow's torc. Talked round, he hands it over; faced with force he fights as a
//!   Grave Robber ([`GRAVE_ROBBER_FOE`], a reserved spawn beside him), and the torc comes from his
//!   body.

use crate::{
    CreatureId, CreatureSpawn, Mechanism, MechanismId, MechanismSequence, Npc, NpcId, NpcRole,
    SequenceId,
};

/// The ring's four marked stones.
pub const NIGHT_STONE: MechanismId = MechanismId::new(5);
pub const DAWN_STONE: MechanismId = MechanismId::new(6);
pub const NOON_STONE: MechanismId = MechanismId::new(7);
pub const DUSK_STONE: MechanismId = MechanismId::new(8);
/// Their order: sunrise, the sun's height, sunset and the dark.
pub const RING_MEASURE: SequenceId = SequenceId::new(1);

/// Cobb the Digger, the grave robber.
pub const COBB: NpcId = NpcId::new(60);
/// Cobb once he fights: a reserved spawn a metre north of him, standing only while he is hostile.
pub const GRAVE_ROBBER_FOE: CreatureId = CreatureId::new(2_931);

/// Where Cobb crouches over his diggings, facing back toward the ring.
pub(super) const COBB_AT: [i32; 2] = [42_600, 58_300];

/// Ground height at `(x, z)`.
fn ground(x: i32, z: i32) -> i32 {
    super::VALE_DEFINITION.ground_at(x, z)
}

/// The ring's four marked stones, each worked from beside it.
#[must_use]
pub fn mechanisms() -> Vec<Mechanism> {
    [
        (NIGHT_STONE, "the night stone", [47_000, 59_000]),
        (DAWN_STONE, "the dawn stone", [48_000, 60_000]),
        (NOON_STONE, "the noon stone", [47_000, 61_000]),
        (DUSK_STONE, "the dusk stone", [46_000, 60_000]),
    ]
    .map(|(id, name, [x, z])| Mechanism {
        id,
        name: name.to_owned(),
        at: [x, ground(x, z), z],
        reach: 200,
    })
    .to_vec()
}

/// The ring's measure, as the lintel gives it.
#[must_use]
pub fn sequences() -> Vec<MechanismSequence> {
    vec![MechanismSequence {
        id: RING_MEASURE,
        name: "the ring's stones".to_owned(),
        mechanisms: vec![DAWN_STONE, NOON_STONE, DUSK_STONE, NIGHT_STONE],
    }]
}

/// Cobb the Digger at his diggings.
pub(super) fn npcs() -> Vec<Npc> {
    vec![Npc {
        id: COBB,
        name: "Cobb the Digger".to_owned(),
        role: NpcRole::Resident,
        level: 4,
        position: COBB_AT,
        facing: 16_384,
    }]
}

/// Cobb's foe: reserved off the world until he turns.
pub(super) fn spawns() -> Vec<CreatureSpawn> {
    let [x, z] = COBB_AT;
    vec![CreatureSpawn {
        id: GRAVE_ROBBER_FOE,
        template: super::units::templates::GRAVE_ROBBER,
        position: [x, z - 100],
        facing: 16_384,
        wander_radius: 0,
        levels: None,
    }]
}
