//! Environmental quest credit (#1019, slice 2): *Stone on the Road* counts creatures a boulder
//! the player set rolling hits, once per impact, never a plain kill; *Breaking the Watch
//! Post* counts structure pieces the player's own blast destroys. Both are checked on small
//! arenas with the same rules Greyhaven's boulder trap and Redbrand watch post use.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureBehaviour, CreatureFamily, CreatureId, CreatureTemplate, CreatureTemplateId, EntityRef,
    NpcId, NpcRole, PieceId, PropKind, Quest, QuestEntry, QuestId, QuestLog, QuestObjective,
    QuestRewards, StaticCollider, StructurePiece, StructureSite, ZoneAreas, ZoneCommand,
    ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena;

const GIVER: NpcId = NpcId::new(1);
const QUEST: QuestId = QuestId::new(1);
const BOULDER: CreatureTemplateId = CreatureTemplateId::new(6);
const BOULDER_ID: CreatureId = CreatureId::new(1);
const BOAR_ID: CreatureId = CreatureId::new(2);

fn quest(objectives: Vec<QuestObjective>) -> Quest {
    Quest {
        id: QUEST,
        name: "Test Quest".into(),
        text: "Do it.".into(),
        giver: GIVER,
        ender: GIVER,
        prerequisite: None,
        objectives,
        rewards: QuestRewards::default(),
        supplies: Vec::new(),
    }
}

fn giver() -> mmorpg_core::Npc {
    mmorpg_core::Npc {
        role: NpcRole::QuestGiver,
        ..arena::npc(GIVER.get(), [3_000, 3_000])
    }
}

/// The players' logs hold the quest, nothing done.
fn with_quest(zone: ZoneSimulation) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    for player in &mut state.players {
        player.quests = QuestLog {
            entries: vec![QuestEntry {
                quest: QUEST,
                progress: [0; mmorpg_core::MAX_QUEST_OBJECTIVES],
            }],
            completed: Default::default(),
        };
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn progress(zone: &ZoneSimulation, player: u32) -> [u8; mmorpg_core::MAX_QUEST_OBJECTIVES] {
    zone.snapshot()
        .unwrap()
        .players
        .iter()
        .find(|record| record.player_id == player)
        .unwrap()
        .quests
        .entry(QUEST)
        .unwrap()
        .progress
}

fn restore(
    zone: &mut ZoneSimulation,
    change: impl FnOnce(&mut mmorpg_core::CanonicalZoneSnapshot),
) {
    let mut state = zone.snapshot().unwrap();
    change(&mut state);
    *zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
}

/// Flat ground, a boulder 2 m north of the origin, a sturdy boar 4 m further and the quest
/// *crush two boars*.
fn boulder_arena() -> ZoneSimulation {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        mmorpg_core::SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [10_000, 50, 10_000],
        }],
    )
    .unwrap();
    let boulder = CreatureTemplate {
        id: BOULDER,
        name: "Test Boulder".into(),
        family: CreatureFamily::Prop,
        behaviour: CreatureBehaviour::Prop(PropKind::Boulder),
        half_extents: [70, 70, 70],
        ..arena::wolf(1, [1, 1])
    };
    let boar = CreatureTemplate {
        health: 400,
        ..arena::boar()
    };
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![boulder, boar],
        vec![
            arena::spawn(1, BOULDER, [0, 200]),
            arena::spawn(2, arena::BOAR, [0, 600]),
        ],
        vec![giver()],
        arena::GRAVEYARD,
    )
    .unwrap()
    .with_creature_abilities(Vec::new())
    .unwrap()
    .with_quests(vec![quest(vec![QuestObjective::Crush {
        template: arena::BOAR,
        count: 2,
    }])])
    .unwrap();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::new(content)).unwrap();
    zone.add_player(1).unwrap();
    // Out of the boulder's way, behind it.
    restore(&mut zone, |state| {
        state.players[0].position = [-600.0, 90.0, 0.0];
    });
    with_quest(zone)
}

/// Sends the boulder north at 8 units a tick from its spot, set rolling by `pusher`.
fn roll(zone: &mut ZoneSimulation, pusher: Option<u32>) -> Vec<ZoneEvent> {
    restore(zone, |state| {
        let boulder = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == BOULDER_ID)
            .unwrap();
        boulder.position = [0.0, boulder.position[1], 200.0];
        boulder.velocity = [0.0, 0.0, 8.0];
        state.boulders[0].pusher = pusher;
        // The boar back on its spot, 4 m ahead.
        let boar = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == BOAR_ID)
            .unwrap();
        boar.position = [0.0, boar.position[1], 600.0];
        boar.velocity = [0.0; 3];
        // Calm again, so it stands in the boulder's path instead of charging the player.
        boar.threat.clear();
        boar.attack = None;
        boar.ai = mmorpg_core::CreatureAi::Idle {
            timer: 0,
            destination: None,
        };
    });
    let mut events = Vec::new();
    for _ in 0..mmorpg_core::golden::t30(90) {
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
    }
    events
}

fn boar_health(zone: &ZoneSimulation) -> u32 {
    zone.snapshot()
        .unwrap()
        .creatures
        .iter()
        .find(|creature| creature.creature_id == BOAR_ID)
        .unwrap()
        .health
}

#[test]
fn a_boulder_the_player_set_rolling_counts_each_impact_once() {
    let mut zone = boulder_arena();
    // A boulder nobody pushed hurts nobody and credits nothing.
    let before = boar_health(&zone);
    roll(&mut zone, None);
    assert_eq!(boar_health(&zone), before);
    assert_eq!(progress(&zone, 1)[0], 0);

    // The player's boulder hits the boar: the environment's damage counts.
    let events = roll(&mut zone, Some(1));
    assert!(boar_health(&zone) < before, "the boulder hit");
    assert!(
        events.contains(&ZoneEvent::QuestProgress {
            quest: QUEST,
            objective: 0,
            count: 1,
        }),
        "{events:?}"
    );
    assert_eq!(progress(&zone, 1)[0], 1);

    // A second roll is a second impact: the stone did the work twice, and that is all it needs.
    let hurt = boar_health(&zone);
    roll(&mut zone, Some(1));
    assert!(boar_health(&zone) < hurt, "the boulder hit again");
    assert_eq!(progress(&zone, 1)[0], 2);
    let hurt = boar_health(&zone);
    roll(&mut zone, Some(1));
    assert!(boar_health(&zone) < hurt, "a third impact");
    assert_eq!(progress(&zone, 1)[0], 2, "a done objective stays done");
}

#[test]
fn a_sword_kill_is_no_crush() {
    let mut zone = boulder_arena();
    restore(&mut zone, |state| {
        state.players[0].position = [0.0, 90.0, 520.0];
        let boar = state
            .creatures
            .iter_mut()
            .find(|creature| creature.creature_id == BOAR_ID)
            .unwrap();
        boar.health = 1;
    });
    let target = EntityRef::Creature(BOAR_ID);
    zone.apply_command(1, 1, ZoneCommand::SelectTarget(Some(target)))
        .unwrap();
    let mut sequence = 1;
    for _ in 0..mmorpg_core::golden::t30(300) {
        if boar_health(&zone) == 0 {
            break;
        }
        if let Some(swing) = arena::swing_at(&zone, 1, target) {
            sequence += 1;
            zone.apply_command(1, sequence, swing).unwrap();
        }
        zone.advance_tick().unwrap();
    }
    assert_eq!(boar_health(&zone), 0, "the boar fell to the sword");
    assert_eq!(progress(&zone, 1)[0], 0);
}

/// Flat ground, a sturdy boar 14.5 m south between two weak walls and a deck on both, and the
/// quest *bring down both walls*.
fn post_arena() -> Arc<ZoneContent> {
    let wall = |id: u32, x: i32| StructurePiece {
        id: PieceId::new(id),
        site: 1,
        body: 50 + id,
        position: [x, 125, -1_450],
        half_extents: [20, 125, 150],
        health: 10,
        supports: Vec::new(),
        name: format!("wall {id}"),
        rubble: None,
    };
    let pieces = vec![
        wall(1, -300),
        wall(2, 300),
        StructurePiece {
            id: PieceId::new(3),
            site: 1,
            body: 53,
            position: [0, 265, -1_450],
            half_extents: [320, 15, 160],
            health: 10,
            supports: vec![PieceId::new(1), PieceId::new(2)],
            name: "deck".into(),
            rubble: None,
        },
    ];
    let content = arena::arena(
        vec![CreatureTemplate {
            health: 400,
            ..arena::boar()
        }],
        vec![arena::spawn(1, arena::BOAR, [0, -1_450])],
        vec![giver()],
    );
    Arc::new(
        Arc::unwrap_or_clone(content)
            .with_creature_abilities(Vec::new())
            .unwrap()
            .with_structures(
                vec![StructureSite {
                    id: 1,
                    name: "Test Post".into(),
                    regrowth_ticks: 9_000,
                }],
                pieces,
            )
            .unwrap()
            .with_quests(vec![quest(vec![
                QuestObjective::Demolish {
                    piece: PieceId::new(1),
                },
                QuestObjective::Demolish {
                    piece: PieceId::new(2),
                },
            ])])
            .unwrap(),
    )
}

#[test]
fn walls_the_player_blasts_down_count_for_the_player_alone() {
    // An Arcanist (player 1) and a Warden standing by (player 2), both on the quest.
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), post_arena()).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    let zone = support::classes::at_level(zone, 1, 6);
    let mut zone = with_quest(support::classes::at_level(zone, 2, 6));
    for (player, class) in [(1, 2), (2, 0)] {
        zone.apply_command(player, 1, ZoneCommand::ChooseClass { class, sex: 0 })
            .unwrap();
        zone.apply_command(player, 2, arena::stand(32_768)).unwrap();
    }
    zone.advance_tick().unwrap();
    restore(&mut zone, |state| {
        for player in &mut state.players {
            player.combat.abilities.resource = 100;
        }
    });
    zone.apply_command(
        1,
        3,
        ZoneCommand::UseAbility {
            ability: ids::FIRE_BLAST.get(),
            target: Some(EntityRef::Creature(CreatureId::new(1))),
            facing: None,
        },
    )
    .unwrap();
    for _ in 0..mmorpg_core::golden::t30(50) {
        zone.advance_tick().unwrap();
    }
    assert!(
        zone.snapshot()
            .unwrap()
            .pieces
            .iter()
            .all(|piece| piece.health == 0),
        "both walls broke and the deck fell"
    );
    assert_eq!(progress(&zone, 1)[..2], [1, 1]);
    assert_eq!(
        progress(&zone, 2)[..2],
        [0, 0],
        "standing by brings nothing down"
    );
    // Stored progress: a restored zone keeps it.
    restore(&mut zone, |_| {});
    assert_eq!(progress(&zone, 1)[..2], [1, 1]);
}
