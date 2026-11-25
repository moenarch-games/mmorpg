//! The Sunken Shrine's sealed vault (#1019): beyond the relic's altar the cave runs on through a
//! passage its stone seal closes, into a dry kit room where the Shrine Warden waits.
//!
//! Two glyph stones open the seal ([`crate::Seal`]): the lakeward glyph on the plaza's south side
//! and the cave glyph in the air-pocket room's south alcove, 35 m of swimming apart, so a lone
//! diver works both within a minute and a party can share them. The warden's ward
//! ([`crate::Ward`]) turns every blow aside until the vault glyph by its far wall exposes its weak
//! point for 15 s. The relic's altar and quest stay where they were; the seal only stands where
//! the cave's end wall stood.
//!
//! Every spot here is authored in the shrine's frame and placed with it ([`super::lake::place`]).

use super::super::{ids, lake};
use super::authored::{self, CAVE_Z, FLOOR_Y, PASSAGE, VAULT, VAULT_END};
use crate::{
    CreatureId, CreatureSpawn, Mechanism, MechanismId, PieceId, PieceMaterial, Seal,
    StaticCollider, StructurePiece, StructureSite, Ward,
};

/// The shrine's structure site and its seal's piece.
pub const SITE: u32 = 3;
pub const SEAL_PIECE: PieceId = PieceId::new(6);
/// The glyphs: the two that open the seal, then the one that exposes the warden.
pub const LAKEWARD_GLYPH: MechanismId = MechanismId::new(1);
pub const CAVE_GLYPH: MechanismId = MechanismId::new(2);
pub const VAULT_GLYPH: MechanismId = MechanismId::new(3);
/// The warden's spawn.
pub const WARDEN: CreatureId = CreatureId::new(2_940);

/// A point of the shrine's frame at `height`, placed.
const fn at([x, z]: [i32; 2], height: i32) -> [i32; 3] {
    let [x, z] = lake::place([x, z]);
    [x, height, z]
}

/// The vault's walls, placed (#1019).
#[must_use]
pub fn walls() -> Vec<StaticCollider> {
    authored::vault_walls(*ids::SHRINE_VAULT_WALLS.start())
        .into_iter()
        .map(|mut collider| {
            collider.position = lake::place3(collider.position);
            collider
        })
        .collect()
}

/// The shrine's structure site: its seal never regrows by itself, so the regrowth is moot.
#[must_use]
pub fn site() -> StructureSite {
    StructureSite {
        id: SITE,
        name: "Sunken Shrine".to_owned(),
        regrowth_ticks: crate::long_ticks_from_ms(300_000),
    }
}

/// The seal's stone across the passage, standing until its glyphs sink it.
#[must_use]
pub fn seal_piece() -> StructurePiece {
    let (centre, half) = authored::SEAL;
    StructurePiece {
        id: SEAL_PIECE,
        site: SITE,
        body: ids::SHRINE_SEAL,
        position: lake::place3(centre),
        half_extents: half,
        health: 1,
        supports: Vec::new(),
        name: "the shrine's seal".to_owned(),
        rubble: None,
        material: PieceMaterial::Stone,
    }
}

/// The glyph stones, each knee-high on the floor: on the plaza's south side by its wall, in the
/// air-pocket room's south alcove, and by the vault's far wall.
#[must_use]
pub fn mechanisms() -> Vec<Mechanism> {
    let glyph = |id, name: &str, spot| Mechanism {
        id,
        name: name.to_owned(),
        at: at(spot, FLOOR_Y),
        reach: 200,
    };
    vec![
        glyph(LAKEWARD_GLYPH, "the lakeward glyph", [5_300, -5_840]),
        glyph(CAVE_GLYPH, "the cave glyph", [8_800, -5_720]),
        glyph(VAULT_GLYPH, "the vault glyph", [13_000, -5_740]),
    ]
}

/// The seal: both outer glyphs together sink it; a player beyond it, in the passage or the vault,
/// holds it open.
#[must_use]
pub fn seal() -> Seal {
    Seal {
        piece: SEAL_PIECE,
        mechanisms: vec![LAKEWARD_GLYPH, CAVE_GLYPH],
        chamber: {
            let (min, max) = lake::place_bounds((
                [PASSAGE.0[0] + 100, VAULT.0[1]],
                [VAULT_END.1[0], VAULT.1[1]],
            ));
            (
                [min[0], FLOOR_Y, min[1]],
                [max[0], FLOOR_Y + authored::ROOM_CLEAR, max[1]],
            )
        },
    }
}

/// The warden's ward, lifted by the vault glyph.
#[must_use]
pub fn ward() -> Ward {
    Ward {
        template: super::super::units::templates::SHRINE_WARDEN,
        mechanism: VAULT_GLYPH,
    }
}

/// The warden in the middle of its vault, facing the passage.
#[must_use]
pub fn spawns() -> Vec<CreatureSpawn> {
    let [x, _, z] = at([12_800, CAVE_Z], FLOOR_Y);
    vec![CreatureSpawn {
        id: WARDEN,
        template: super::super::units::templates::SHRINE_WARDEN,
        position: [x, z],
        // The passage lies toward +X once placed.
        facing: 16_384,
        wander_radius: 0,
        levels: Some([6, 6]),
    }]
}
