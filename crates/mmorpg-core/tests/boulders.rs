//! The boulder trap (#299): walking contact rolls a boulder slowly, a Shield Bash sends it fast,
//! a fast boulder hurts and knocks back what it hits and a slow one does not, an idle boulder
//! returns to its spawn, a restored zone continues a roll exactly and Greyhaven's boulder runs
//! downhill onto the Redbrand camp when walked off its rise.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale;
use mmorpg_core::{
    CreatureBehaviour, CreatureFamily, CreatureId, CreatureLife, CreatureTemplate,
    CreatureTemplateId, EntityRef, IMPACT_SPEED, PropKind, RETURN_TICKS, StaticCollider,
    WALK_PUSH_SPEED, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneId, ZoneSimulation,
};
use support::arena;

const BOULDER: CreatureTemplateId = CreatureTemplateId::new(6);
const WARDEN: u8 = 0;
const BOULDER_ID: CreatureId = CreatureId::new(1);
const BOAR_ID: CreatureId = CreatureId::new(2);

fn boulder() -> CreatureTemplate {
    CreatureTemplate {
        id: BOULDER,
        name: "Test Boulder".into(),
        family: CreatureFamily::Prop,
        behaviour: CreatureBehaviour::Prop(PropKind::Boulder),
        half_extents: [70, 70, 70],
        ..arena::wolf(1, [1, 1])
    }
}

/// Flat ground, a boulder 2 m north of the origin and a boar `boar_at` further north, if any.
fn content(boar_at: Option<i32>) -> Arc<ZoneContent> {
    let colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        mmorpg_core::SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let mut spawns = vec![arena::spawn(1, BOULDER, [0, 200])];
    if let Some(z) = boar_at {
        spawns.push(arena::spawn(2, arena::BOAR, [0, z]));
    }
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![boulder(), arena::boar()],
            spawns,
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Vec::new())
        .unwrap(),
    )
}

struct Scene {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Scene {
    fn new(content: Arc<ZoneContent>) -> Self {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
        zone.add_player(1).unwrap();
        let zone = support::classes::at_level(zone, 1, 6);
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [0.0, 90.0, 0.0];
        let zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
        let mut scene = Self { zone, sequence: 0 };
        scene.send(ZoneCommand::ChooseClass {
            class: WARDEN,
            sex: 0,
        });
        scene.send(arena::stand(arena::NORTHWARD));
        scene.zone.advance_tick().unwrap();
        let mut state = scene.zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        scene.zone =
            ZoneSimulation::from_snapshot(state, Arc::clone(scene.zone.content())).unwrap();
        scene
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn walk(&mut self, facing: u16) {
        self.send(ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        });
    }

    fn boulder(&self) -> ([f32; 3], [f32; 3]) {
        record(&self.zone, BOULDER_ID)
    }

    fn ticks(&mut self, count: u32) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    fn with_state(&mut self, change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot)) {
        let mut state = self.zone.snapshot().unwrap();
        change(&mut state);
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }
}

fn record(zone: &ZoneSimulation, id: CreatureId) -> ([f32; 3], [f32; 3]) {
    let state = zone.snapshot().unwrap();
    let creature = state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == id)
        .unwrap();
    (creature.position, creature.velocity)
}

fn health(zone: &ZoneSimulation, id: CreatureId) -> u32 {
    let state = zone.snapshot().unwrap();
    state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == id)
        .unwrap()
        .health
}

fn speed([x, y, z]: [f32; 3]) -> f32 {
    (x * x + y * y + z * z).sqrt()
}

#[test]
fn walking_into_a_boulder_rolls_it_slowly() {
    let mut scene = Scene::new(content(None));
    let (start, _) = scene.boulder();
    scene.walk(arena::NORTHWARD);
    // Reach it (about 12 ticks of walking), then shoulder it for a second.
    scene.ticks(72);
    let (rolled, velocity) = scene.boulder();
    assert!(
        rolled[2] > start[2] + 5.0,
        "the walk moved it: {start:?} -> {rolled:?}"
    );
    assert!(
        speed(velocity) <= WALK_PUSH_SPEED * 1.5,
        "a shoulder is slow: {velocity:?}"
    );
    assert!(
        rolled[2] < start[2] + 150.0,
        "and the player keeps it slow: {rolled:?}"
    );
    let player = scene.zone.snapshot().unwrap().players[0].position;
    assert!(
        player[2] < rolled[2] - 70.0,
        "units meet its sphere and stay behind it: {player:?} vs {rolled:?}"
    );
    // Stopping leaves it to stop too.
    scene.send(arena::stand(arena::NORTHWARD));
    scene.ticks(180);
    let (_, velocity) = scene.boulder();
    assert_eq!(velocity, [0.0; 3], "it comes to rest");
}

#[test]
fn a_shield_bash_sends_it_much_faster_than_a_walk() {
    let mut scene = Scene::new(content(None));
    scene.send(ZoneCommand::UseAbility {
        ability: ids::SHIELD_BASH.get(),
        target: Some(EntityRef::Creature(BOULDER_ID)),
    });
    scene.ticks(2);
    let (_, velocity) = scene.boulder();
    assert!(
        speed(velocity) > WALK_PUSH_SPEED * 3.0,
        "a bash launches it far faster than a walk: {velocity:?}"
    );
}

#[test]
fn a_fast_boulder_hurts_and_knocks_back_a_slow_one_stops() {
    // Fast: 8 units per tick, northward, from 2 m south of the boar.
    let mut fast = Scene::new(content(Some(600)));
    fast.with_state(|state| {
        state.creatures[0].velocity = [0.0, 0.0, 8.0];
        state.boulders[0].pusher = Some(1);
    });
    let boar_health = health(&fast.zone, BOAR_ID);
    let (boar_before, _) = record(&fast.zone, BOAR_ID);
    fast.ticks(90);
    assert!(
        health(&fast.zone, BOAR_ID) < boar_health,
        "a fast boulder hurts what it hits"
    );
    let (boar_after, _) = record(&fast.zone, BOAR_ID);
    assert!(
        boar_after[2] > boar_before[2] + 50.0,
        "and knocks it back: {boar_before:?} -> {boar_after:?}"
    );

    // Slow: below impact speed, the same boar stops it, unhurt.
    let mut slow = Scene::new(content(Some(330)));
    slow.with_state(|state| {
        state.creatures[0].velocity = [0.0, 0.0, IMPACT_SPEED * 0.6];
        state.boulders[0].pusher = Some(1);
    });
    let boar_health = health(&slow.zone, BOAR_ID);
    let (boar_before, _) = record(&slow.zone, BOAR_ID);
    slow.ticks(60);
    assert_eq!(
        health(&slow.zone, BOAR_ID),
        boar_health,
        "a slow boulder does no harm"
    );
    let (boar_after, _) = record(&slow.zone, BOAR_ID);
    assert!(
        (boar_after[2] - boar_before[2]).abs() < 5.0,
        "and no knockback: {boar_before:?} -> {boar_after:?}"
    );
    let (rolled, _) = slow.boulder();
    assert!(
        rolled[2] < boar_after[2] - 70.0 && rolled[2] > 205.0,
        "it reaches the boar and is stopped by it: {rolled:?}, boar {boar_after:?}"
    );
}

#[test]
fn a_boulder_cannot_be_broken() {
    let mut scene = Scene::new(content(None));
    scene.with_state(|state| state.creatures[0].health = 1);
    scene.send(ZoneCommand::UseAbility {
        ability: ids::SHIELD_BASH.get(),
        target: Some(EntityRef::Creature(BOULDER_ID)),
    });
    scene.ticks(30);
    let state = scene.zone.snapshot().unwrap();
    assert_eq!(state.creatures[0].life, CreatureLife::Alive);
    assert_eq!(state.creatures[0].health, 1, "attacks never reduce it");
}

#[test]
fn an_idle_boulder_returns_to_its_spawn_after_the_timer() {
    let mut scene = Scene::new(content(None));
    let (start, _) = scene.boulder();
    scene.with_state(|state| {
        state.creatures[0].position = [400.0, 70.0, 900.0];
        state.boulders[0].idle = RETURN_TICKS - 5;
    });
    assert_eq!(scene.boulder().0[2], 900.0, "it stayed where it was left");
    scene.ticks(10);
    let (back, velocity) = scene.boulder();
    assert_eq!(back, start, "it returned to its spot");
    assert_eq!(velocity, [0.0; 3], "at rest");
    let state = scene.zone.snapshot().unwrap();
    assert_eq!(state.boulders[0].idle, 0, "its timer is cleared");
}

#[test]
fn a_restored_zone_continues_the_roll_exactly() {
    let mut original = Scene::new(content(Some(600)));
    original.walk(arena::NORTHWARD);
    original.ticks(40);
    original.send(ZoneCommand::UseAbility {
        ability: ids::SHIELD_BASH.get(),
        target: Some(EntityRef::Creature(BOULDER_ID)),
    });
    original.ticks(1);
    let snapshot = original.zone.snapshot().unwrap();
    assert!(snapshot.boulders[0].spin != [0.0; 3] || snapshot.creatures[0].velocity != [0.0; 3]);
    let mut restored = Scene {
        zone: ZoneSimulation::from_snapshot(snapshot, Arc::clone(original.zone.content())).unwrap(),
        sequence: original.sequence,
    };
    original.ticks(120);
    restored.ticks(120);
    let (left, right) = (
        original.zone.snapshot().unwrap(),
        restored.zone.snapshot().unwrap(),
    );
    assert_eq!(left.creatures, right.creatures, "identical continuation");
    assert_eq!(left.boulders, right.boulders, "spin and timers too");
}

#[test]
fn greyhavens_boulder_rolls_downhill_onto_the_camp() {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let boulder_id = CreatureId::new(925);
    let boulder = state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == boulder_id)
        .unwrap()
        .position;
    // South of the boulder on the rise: uphill, with the camp to the north (-Z).
    state.players[0].position = [boulder[0], boulder[1] + 40.0, boulder[2] + 140.0];
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
    let mut sequence = 1;
    zone.apply_command(
        1,
        sequence,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 32_768, // -Z
        },
    )
    .unwrap();
    sequence += 1;
    let (start, _) = record(&zone, boulder_id);
    let mut peak = 0.0_f32;
    for tick in 0..240 {
        zone.advance_tick().unwrap();
        let (position, velocity) = record(&zone, boulder_id);
        peak = peak.max(speed(velocity));
        if tick == 10 {
            // A single shoulder is enough to dislodge it.
            zone.apply_command(
                1,
                sequence,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: 32_768,
                },
            )
            .unwrap();
        }
        if position[2] < start[2] - 500.0 {
            break;
        }
    }
    let (position, _) = record(&zone, boulder_id);
    assert!(
        peak >= IMPACT_SPEED,
        "rolling downhill makes it dangerous: peak {peak}"
    );
    assert!(
        position[2] < start[2] - 100.0,
        "it left its rise toward the camp: {start:?} -> {position:?}"
    );
}
