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

/// The facing from `player` toward `target` and their XZ distance in the player's projection,
/// as a player turns to aim (#517); `None` while either is out of view.
pub fn aim_at(
    zone: &mmorpg_core::ZoneSimulation,
    player: u32,
    target: mmorpg_core::EntityRef,
) -> Option<(u16, i64)> {
    let view = zone.snapshot_for_player(player).ok()?;
    let at = |entity: mmorpg_core::EntityRef| {
        view.entities
            .iter()
            .find(|unit| unit.entity() == entity)
            .map(|unit| unit.position)
    };
    let (me, unit) = (at(mmorpg_core::EntityRef::Player(player))?, at(target)?);
    let (dx, dz) = (unit[0] - me[0], unit[2] - me[2]);
    let facing = mmorpg_core::trig::yaw_from_vector(dx, dz).unwrap_or(0);
    Some((facing, i64::from(dx).pow(2) + i64::from(dz).pow(2)))
}

/// A player pressing attack at `target` (#517): a swing toward it while it stands within melee
/// reach in the player's projection, otherwise nothing, as a player swings once the target is
/// in front of them.
pub fn swing_at(
    zone: &mmorpg_core::ZoneSimulation,
    player: u32,
    target: mmorpg_core::EntityRef,
) -> Option<ZoneCommand> {
    let (facing, distance) = aim_at(zone, player, target)?;
    let reach = i64::from(mmorpg_core::unit::PLAYER_REACH_UNITS);
    (distance <= reach.pow(2)).then_some(ZoneCommand::Swing { facing })
}

/// The player turns in place to face `target` (#517), keeping still; `stand(0)` while it is out
/// of view.
pub fn face(
    zone: &mmorpg_core::ZoneSimulation,
    player: u32,
    target: mmorpg_core::EntityRef,
) -> ZoneCommand {
    stand(aim_at(zone, player, target).map_or(0, |(facing, _)| facing))
}
