//! Minimal combat content for deterministic tests (TEST-010): flat ground,
//! spawn slots along +X from the origin, a graveyard to the south (−Z) and
//! only the creatures and NPCs a test asks for. Shared with
//! `mmorpg-protocol` tests through `#[path]`.
#![allow(dead_code)]

use std::sync::Arc;

use mmorpg_core::{
    CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn, CreatureTemplate,
    CreatureTemplateId, Npc, NpcId, NpcRole, SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand,
    ZoneContent, ZoneDefinition,
};

pub const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
pub const BOAR: CreatureTemplateId = CreatureTemplateId::new(2);
pub const GRAVEYARD: [i32; 2] = [0, -3_000];
/// Yaw 0 faces +Z, where tests place creatures.
pub const NORTHWARD: u16 = 0;

/// A level-1 wolf: aggressive, 60-tick swings, 60 s respawn.
pub fn wolf(health: u32, damage: [u16; 2]) -> CreatureTemplate {
    CreatureTemplate {
        id: WOLF,
        name: "Test Wolf".into(),
        family: CreatureFamily::Wolf,
        behaviour: CreatureBehaviour::Aggressive,
        min_level: 1,
        max_level: 1,
        elite: false,
        health,
        health_per_level: 0,
        damage,
        damage_per_level: 0,
        swing_ticks: mmorpg_core::golden::t30(60) as _,
        respawn_ticks: mmorpg_core::golden::t30(1_800) as _,
        half_extents: [40, 45, 40],
    }
}

/// A neutral level-1 boar.
pub fn boar() -> CreatureTemplate {
    CreatureTemplate {
        id: BOAR,
        name: "Test Boar".into(),
        family: CreatureFamily::Boar,
        behaviour: CreatureBehaviour::Neutral,
        damage: [2, 3],
        ..wolf(40, [2, 3])
    }
}

pub fn spawn(id: u32, template: CreatureTemplateId, position: [i32; 2]) -> CreatureSpawn {
    CreatureSpawn {
        id: CreatureId::new(id),
        template,
        position,
        facing: 0,
        wander_radius: 0,
        levels: None,
    }
}

pub fn npc(id: u32, position: [i32; 2]) -> Npc {
    Npc {
        id: NpcId::new(id),
        name: "Test Guard".into(),
        role: NpcRole::Guard,
        level: 10,
        position,
        facing: 0,
    }
}

/// 200 m of flat ground with gravity; spawn slot 0 is the origin.
pub fn ground() -> ZoneDefinition {
    ZoneDefinition::with_spawn_grid(
        77,
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [10_000, 50, 10_000],
        }],
    )
    .expect("test ground is valid")
}

pub fn arena(
    templates: Vec<CreatureTemplate>,
    spawns: Vec<CreatureSpawn>,
    npcs: Vec<Npc>,
) -> Arc<ZoneContent> {
    Arc::new(
        ZoneContent::new(
            ground(),
            ZoneAreas::default(),
            templates,
            spawns,
            npcs,
            GRAVEYARD,
        )
        .expect("test arena is valid"),
    )
}

pub fn walk(facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    }
}

pub fn stand(facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward: 0,
        strafe: 0,
        facing,
    }
}
