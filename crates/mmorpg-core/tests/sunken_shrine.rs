//! The Sunken Shrine (#274 part 2): the flooded cave and its air pocket, quest 10's relic,
//! the lurkers that trail submerged players, and no combat across the water's surface.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::quests::{MARSHAL, SUNKEN_RELIC, THE_SUNKEN_SHRINE};
use mmorpg_core::greyhaven_vale::{self, shrine};
use mmorpg_core::water::{BREATH_MAX_TICKS, TRAIL_TICKS};
use mmorpg_core::{
    CreatureAi, CreatureId, EntityRef, ErrorCode, QuestEntry, QuestLog, ThreatEntry, WaterMode,
    ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
};

/// World-axis heading of forward movement toward +X.
const EAST: u16 = 16_384;
const LURKER: CreatureId = CreatureId::new(303);
/// Inside the cave, along its middle.
const CAVE_Z: i32 = -5_400;
/// A swimmer's body centre just over the shrine's floor.
const LOW: i32 = shrine::FLOOR_Y + 110;

fn zone_with_player_at(centre: [i32; 3], with_quest: bool) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units(centre);
    if with_quest {
        // Quests 1–5 turned in; The Sunken Shrine accepted.
        state.players[0].quests = QuestLog {
            entries: vec![QuestEntry {
                quest: THE_SUNKEN_SHRINE,
                progress: [0; 3],
            }],
            completed: 0b1_1111,
        };
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn water(zone: &ZoneSimulation) -> WaterMode {
    let position = zone.snapshot_for_player(1).unwrap().entities[0].position;
    WaterMode::of_player(zone.content().waters(), position)
}

fn relics(zone: &ZoneSimulation) -> u32 {
    zone.snapshot().unwrap().players[0]
        .inventory
        .count(SUNKEN_RELIC)
}

fn run(zone: &mut ZoneSimulation, ticks: u32) -> Vec<ZoneEvent> {
    let mut events = Vec::new();
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot().unwrap().players[0].combat.events.clone());
    }
    events
}

#[test]
fn the_cave_is_flooded_dark_to_its_end_and_breathable_only_in_its_pocket() {
    let content = greyhaven_vale::content();
    let terrain = content.definition().terrain().unwrap();
    assert_eq!(terrain.height_at(9_600, CAVE_Z), shrine::FLOOR_Y);
    assert_eq!(terrain.height_at(5_400, CAVE_Z), shrine::FLOOR_Y);
    let mode = |x| WaterMode::of_player(content.waters(), [x, shrine::FLOOR_Y + 90, CAVE_Z]);
    assert_eq!(mode(5_400), WaterMode::Submerged, "the plaza");
    assert_eq!(mode(7_000), WaterMode::Submerged, "under the lake");
    assert_eq!(mode(10_000), WaterMode::Submerged, "under the shore");
    assert_eq!(mode(8_800), WaterMode::Swimming, "the air pocket");
    // The air pocket is the shrine kit's 8 × 8 m room (#431): flat and breathable off the axis.
    for z in [-5_750, -5_050] {
        assert_eq!(terrain.height_at(8_800, z), shrine::FLOOR_Y);
        assert_eq!(
            WaterMode::of_player(content.waters(), [8_800, shrine::FLOOR_Y + 90, z]),
            WaterMode::Swimming
        );
    }
    // The shore over the cave is dry ground, roofed flush with it.
    assert_eq!(
        WaterMode::of_player(content.waters(), [10_000, 90, CAVE_Z]),
        WaterMode::Dry
    );
    let roof = content
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == greyhaven_vale::ids::SHRINE_CAVE_ROOF.start() + 10)
        .unwrap();
    assert_eq!(roof.position[1] + roof.half_extents[1], 0);
}

#[test]
fn the_air_pocket_refills_breath_halfway_along_the_cave() {
    let mut zone = zone_with_player_at([8_800, LOW, CAVE_Z], false);
    let mut state = zone.snapshot().unwrap();
    state.players[0].breath.remaining = 30;
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    run(&mut zone, 1);
    assert_eq!(water(&zone), WaterMode::Swimming);
    run(&mut zone, 150);
    assert_eq!(
        zone.snapshot_for_player(1).unwrap().viewer.breath,
        BREATH_MAX_TICKS
    );
}

#[test]
fn swimming_the_cave_to_the_altar_retrieves_the_relic_and_the_marshal_takes_it() {
    // From the shrine's plaza, straight east through the mouth and along the cave.
    let mut zone = zone_with_player_at([5_400, LOW, CAVE_Z], true);
    run(&mut zone, 2);
    assert_eq!(water(&zone), WaterMode::Submerged);
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: EAST,
        },
    )
    .unwrap();
    let mut ticks = 0;
    let mut events = Vec::new();
    while relics(&zone) == 0 {
        events.extend(run(&mut zone, 1));
        ticks += 1;
        assert!(
            ticks < 900,
            "the altar is reached on one breath from the plaza"
        );
    }
    // More than half a breath one way: the round trip needs the air pocket.
    assert!(
        ticks > u32::from(BREATH_MAX_TICKS) / 2,
        "{ticks} ticks to the altar"
    );
    assert!(
        events.contains(&ZoneEvent::QuestProgress {
            quest: THE_SUNKEN_SHRINE,
            objective: 0,
            count: 1,
        }),
        "{events:?}"
    );
    let x = zone.snapshot().unwrap().players[0].position[0];
    assert!(x >= shrine::ALTAR_CHAMBER.0[0] as f32, "at the altar: {x}");
    let viewer = zone.snapshot_for_player(1).unwrap().viewer;
    assert!(viewer.breath > 0 && !viewer.dead);
    // One relic only, however long the player stays.
    run(&mut zone, 30);
    assert_eq!(relics(&zone), 1);

    // Back at the outpost, the Marshal takes it: 300 XP and 25 copper.
    let marshal = zone.content().npc(MARSHAL).unwrap().position;
    let mut state = zone.snapshot().unwrap();
    state.players[0].position =
        mmorpg_core::physics::from_units([marshal[0] + 200, 90, marshal[1]]);
    let copper = state.players[0].copper;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(
        1,
        2,
        ZoneCommand::CompleteQuest {
            npc: MARSHAL,
            quest: THE_SUNKEN_SHRINE.get(),
            choice: 0,
        },
    )
    .unwrap();
    let events = run(&mut zone, 2);
    assert!(
        events.contains(&ZoneEvent::QuestCompleted {
            quest: THE_SUNKEN_SHRINE
        }),
        "{events:?}"
    );
    let player = &zone.snapshot().unwrap().players[0];
    assert_eq!(player.inventory.count(SUNKEN_RELIC), 0);
    assert_eq!(player.copper, copper + 25);
    assert!(player.quests.is_completed(THE_SUNKEN_SHRINE));
}

#[test]
fn walking_over_the_altar_on_the_shore_retrieves_nothing() {
    let mut zone = zone_with_player_at([11_800, 95, CAVE_Z], true);
    run(&mut zone, 30);
    assert_eq!(water(&zone), WaterMode::Dry);
    assert_eq!(relics(&zone), 0);
}

/// The player submerged over the plaza and lurker 303 in the water 4 m away.
fn lurker_beside_a_diver() -> ZoneSimulation {
    let zone = zone_with_player_at([5_400, -600, CAVE_Z], false);
    let mut state = zone.snapshot().unwrap();
    let lurker = state
        .creatures
        .iter_mut()
        .find(|creature| creature.creature_id == LURKER)
        .unwrap();
    lurker.position = [5_400.0, -600.0, CAVE_Z as f32 + 400.0];
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn lurker(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == LURKER)
        .unwrap()
}

#[test]
fn a_lurker_trails_a_diver_without_harm_then_loses_interest_and_swims_home() {
    let mut zone = lurker_beside_a_diver();
    run(&mut zone, 2);
    assert!(
        matches!(lurker(&zone).ai, CreatureAi::Trailing { player: 1, .. }),
        "{:?}",
        lurker(&zone).ai
    );
    let health = zone.snapshot_for_player(1).unwrap().viewer.health;
    let events = run(&mut zone, u32::from(TRAIL_TICKS));
    assert!(
        events.contains(&ZoneEvent::LostInterest {
            source: EntityRef::Creature(LURKER),
        }),
        "{events:?}"
    );
    assert!(matches!(lurker(&zone).ai, CreatureAi::Evading { .. }));
    assert!(!events.iter().any(|event| matches!(
        event,
        ZoneEvent::DamageTaken { .. } | ZoneEvent::AttackPerformed { .. }
    )));
    assert_eq!(zone.snapshot_for_player(1).unwrap().viewer.health, health);
    // Recovery keeps the trail's state exactly.
    let state = zone.snapshot().unwrap();
    let mut restored = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    run(&mut restored, 20);
    run(&mut zone, 20);
    assert_eq!(restored.snapshot().unwrap(), zone.snapshot().unwrap());
}

#[test]
fn a_lurker_drops_a_player_who_dives_and_divers_cannot_attack() {
    let mut zone = lurker_beside_a_diver();
    let mut state = zone.snapshot().unwrap();
    let lurker_state = state
        .creatures
        .iter_mut()
        .find(|creature| creature.creature_id == LURKER)
        .unwrap();
    lurker_state.ai = CreatureAi::Engaged;
    lurker_state.threat = vec![ThreatEntry {
        entity: EntityRef::Player(1),
        threat: 10,
    }];
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    run(&mut zone, 1);
    let after = lurker(&zone);
    assert!(after.threat.is_empty());
    assert!(matches!(after.ai, CreatureAi::Evading { .. }));

    zone.apply_command(
        1,
        1,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(LURKER))),
    )
    .unwrap();
    zone.apply_command(1, 2, ZoneCommand::StartAttack).unwrap();
    let events = run(&mut zone, 2);
    assert!(
        events.contains(&ZoneEvent::Error {
            code: ErrorCode::NotAttackable,
            target: Some(EntityRef::Creature(LURKER)),
        }),
        "{events:?}"
    );
}
