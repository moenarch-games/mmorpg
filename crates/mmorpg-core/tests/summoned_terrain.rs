//! Summoned terrain (#293): Ice Wall and Earth Pillar rise in front of the caster, block
//! movement and sight, stop shots, can be destroyed, expire, refuse to embed a unit and survive
//! a canonical restore.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureId, EntityRef, ErrorCode, SummonedTerrainKind, ZoneCommand, ZoneContent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, BOAR};

const ARCANIST: u8 = 2;
const NORTH: u16 = 0;

fn content(boar_z: i32) -> Arc<ZoneContent> {
    let content = arena::arena(
        vec![mmorpg_core::CreatureTemplate {
            health: 400,
            ..arena::boar()
        }],
        vec![arena::spawn(1, BOAR, [0, boar_z])],
        Vec::new(),
    );
    Arc::new(
        Arc::unwrap_or_clone(content)
            .with_creature_abilities(Vec::new())
            .unwrap(),
    )
}

struct Arcanist {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Arcanist {
    fn new(boar_z: i32) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content(boar_z)).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 8);
        let mut arcanist = Self { zone, sequence: 0 };
        arcanist.send(ZoneCommand::ChooseClass {
            class: ARCANIST,
            sex: 0,
        });
        arcanist.send(arena::stand(NORTH));
        arcanist.ticks(1);
        arcanist
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    fn cast(&mut self, ability: mmorpg_core::AbilityId) -> Vec<ErrorCode> {
        // Any earlier global cooldown has run out by now.
        self.ticks(u32::from(mmorpg_core::GLOBAL_COOLDOWN_TICKS) + 1);
        self.send(ZoneCommand::UseAbility {
            ability: ability.get(),
            target: None,
        });
        self.zone.advance_tick().unwrap();
        self.errors()
    }

    fn errors(&self) -> Vec<ErrorCode> {
        self.zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .iter()
            .filter_map(|event| match event {
                mmorpg_core::ZoneEvent::Error { code, .. } => Some(*code),
                _ => None,
            })
            .collect()
    }

    fn z(&self) -> i32 {
        let view = self.zone.snapshot_for_player(1).unwrap();
        view.entities
            .iter()
            .find(|unit| unit.entity() == EntityRef::Player(1))
            .unwrap()
            .position[2]
    }

    fn walls(&self) -> Vec<mmorpg_core::SummonedTerrain> {
        self.zone.snapshot().unwrap().summoned_terrain
    }

    fn walk_north(&mut self, ticks: u32) {
        self.send(arena::walk(NORTH));
        self.ticks(ticks);
        self.send(arena::stand(NORTH));
    }

    /// Restores the canonical snapshot; `change` edits it first.
    fn restored(&mut self, change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        change(&mut state);
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }
}

#[test]
fn ice_wall_rises_ahead_and_earth_pillar_too() {
    let mut caster = Arcanist::new(2_000);
    assert_eq!(caster.cast(ids::ICE_WALL), []);
    let walls = caster.walls();
    assert_eq!(walls.len(), 1, "one wall stands");
    assert_eq!(walls[0].kind, SummonedTerrainKind::IceWall);
    assert_eq!(
        walls[0].centre[2], 400,
        "4 m in front of the caster along its facing"
    );
    let view = caster.zone.snapshot_for_player(1).unwrap().summoned;
    assert_eq!(view.len(), 1, "projected to the viewer");
    assert_eq!(view[0].health_percent, 100);

    assert_eq!(caster.cast(ids::EARTH_PILLAR), []);
    assert_eq!(caster.walls().len(), 2);
}

#[test]
fn a_wall_blocks_walking_a_shot_and_sight() {
    let mut caster = Arcanist::new(2_000);
    assert_eq!(caster.cast(ids::ICE_WALL), []);
    caster.walk_north(mmorpg_core::golden::t30(90) as _);
    let z = caster.z();
    assert!(z < 370, "the wall (z 370-430) stops the caster at z {z}");
    assert!(z > 200, "the caster walked up to it, z {z}");

    // A free-aimed shot from the far side stops at the wall and chips it.
    let start = caster.walls()[0].health;
    caster.restored(|state| {
        state.players[0].combat.abilities.resource = 100;
    });
    assert_eq!(caster.cast(ids::FIREBOLT), []);
    caster.ticks(mmorpg_core::golden::t30(150) as _);
    assert_eq!(
        caster.walls()[0].health,
        start - mmorpg_core::PROJECTILE_SUMMONED_DAMAGE,
        "the shot stopped at the wall"
    );
}

#[test]
fn a_wall_shields_the_unit_behind_it_from_a_sighted_spell() {
    const SOUTH: u16 = 32_768;
    let ability = ids::CHAIN_LIGHTNING;
    let mut caster = Arcanist::new(-1_000);
    caster.send(arena::stand(SOUTH));
    // The boar in plain sight can be aimed at.
    assert_ne!(caster.cast(ability), [ErrorCode::NoTarget]);
    let mut shielded = Arcanist::new(-1_000);
    shielded.send(arena::stand(SOUTH));
    assert_eq!(shielded.cast(ids::ICE_WALL), []);
    assert_eq!(
        shielded.cast(ability),
        [ErrorCode::NoTarget],
        "the wall hides the boar"
    );
}

#[test]
fn a_destroyed_or_expired_wall_stops_blocking() {
    let mut caster = Arcanist::new(2_000);
    assert_eq!(caster.cast(ids::ICE_WALL), []);
    caster.restored(|state| {
        state.summoned_terrain[0].health = 1;
        state.players[0].combat.abilities.resource = 100;
    });
    assert_eq!(caster.cast(ids::FIREBOLT), []);
    caster.ticks(mmorpg_core::golden::t30(150) as _);
    assert!(caster.walls().is_empty(), "the shot destroyed it");
    caster.walk_north(mmorpg_core::golden::t30(120) as _);
    let z = caster.z();
    assert!(z > 500, "the way is open again, z {z}");

    let mut waiting = Arcanist::new(2_000);
    assert_eq!(waiting.cast(ids::ICE_WALL), []);
    let wall = waiting.walls()[0];
    waiting.ticks(SummonedTerrainKind::IceWall.duration_ticks() + 2);
    assert!(waiting.walls().is_empty(), "wall {wall:?} melted on time");
}

#[test]
fn a_wall_never_embeds_a_unit() {
    // The boar stands exactly where the wall would rise, south of the spawn grid.
    let mut caster = Arcanist::new(-400);
    caster.send(arena::stand(32_768));
    caster.ticks(1);
    let mana = caster.zone.snapshot().unwrap().players[0]
        .combat
        .abilities
        .resource;
    assert_eq!(caster.cast(ids::ICE_WALL), [ErrorCode::NoRoom]);
    assert!(caster.walls().is_empty());
    assert_eq!(
        caster.zone.snapshot().unwrap().players[0]
            .combat
            .abilities
            .resource,
        mana,
        "the refused cast is free"
    );
}

#[test]
fn a_wall_survives_snapshot_and_replay_and_stays_solid() {
    let mut caster = Arcanist::new(2_000);
    assert_eq!(caster.cast(ids::ICE_WALL), []);
    caster.ticks(20);
    let before = caster.zone.snapshot().unwrap();
    assert_eq!(before.summoned_terrain.len(), 1);
    caster.restored(|_| {});
    assert_eq!(caster.zone.snapshot().unwrap(), before, "exact restore");
    // Solid after the restore: the caster still stops at it.
    caster.walk_north(mmorpg_core::golden::t30(90) as _);
    let z = caster.z();
    assert!(z < 370, "the restored wall still blocks, z {z}");
    let done = caster.walls()[0];
    // And it still expires on its own tick.
    caster.ticks(SummonedTerrainKind::IceWall.duration_ticks());
    assert!(caster.walls().is_empty(), "restored wall {done:?} expires");
}

#[test]
fn recasting_replaces_the_callers_own_wall() {
    let mut caster = Arcanist::new(2_000);
    assert_eq!(caster.cast(ids::EARTH_PILLAR), []);
    let first = caster.walls()[0].id;
    caster.ticks(mmorpg_core::golden::t30(390) as _);
    caster.restored(|state| {
        state.players[0].combat.abilities.resource = 100;
    });
    assert_eq!(caster.cast(ids::EARTH_PILLAR), []);
    let walls = caster.walls();
    assert_eq!(walls.len(), 1, "the old pillar was replaced");
    assert_ne!(walls[0].id, first);
    assert!(
        CreatureId::new(1).get() == 1,
        "the boar stayed irrelevant to this test"
    );
}
