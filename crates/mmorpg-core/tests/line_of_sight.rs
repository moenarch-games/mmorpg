//! Authoritative line of sight: targeted ranged abilities need a clear
//! physics ray from the caster to the target through static content.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, CreatureId, EntityRef, ErrorCode, SpawnGrid, StaticCollider,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR};
use support::classes;

const TARGET: EntityRef = EntityRef::Creature(CreatureId::new(1));
const ARCANIST: u8 = 2;

/// Flat ground, a neutral boar 25 m north of the spawn and, optionally, a
/// 2 m high wall halfway between them. Classes need the bound ability catalog.
fn content(wall: bool) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    if wall {
        colliders.push(StaticCollider {
            id: 2,
            position: [0, 100, 1_250],
            half_extents: [400, 100, 20],
        });
    }
    let definition = ZoneDefinition::with_spawn_grid(
        78,
        [0, -1, 0],
        SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![arena::boar()],
            vec![arena::spawn(1, BOAR, [0, 2_500])],
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(ABILITY_CATALOG_REVISION, Vec::new())
        .unwrap(),
    )
}

/// The first-tick events of an arcanist that targets the boar and casts Firebolt.
fn cast_firebolt(wall: bool) -> (ZoneSimulation, Vec<ZoneEvent>, u16) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content(wall)).unwrap();
    zone.add_player(1).unwrap();
    let mut zone = classes::at_level(zone, 1, 6);
    let commands = [
        ZoneCommand::ChooseClass {
            class: ARCANIST,
            sex: 1,
        },
        ZoneCommand::SelectTarget(Some(TARGET)),
    ];
    for (sequence, command) in (1..).zip(commands) {
        zone.apply_command(1, sequence, command).unwrap();
        zone.advance_tick().unwrap();
    }
    let mana = zone
        .snapshot_for_player(1)
        .unwrap()
        .viewer
        .resource
        .unwrap()
        .value;
    zone.apply_command(
        1,
        3,
        ZoneCommand::UseAbility {
            ability: ids::FIREBOLT.get(),
            target: None,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let events = zone.snapshot_for_player(1).unwrap().events;
    (zone, events, mana)
}

#[test]
fn a_static_wall_blocks_a_ranged_ability_that_a_clear_line_allows() {
    let (blocked, events, mana) = cast_firebolt(true);
    assert!(
        events.contains(&ZoneEvent::Error {
            code: ErrorCode::LineOfSight,
            target: Some(TARGET),
        }),
        "{events:?}"
    );
    let view = blocked.snapshot_for_player(1).unwrap();
    assert_eq!(view.viewer.cast, None, "no cast starts behind cover");
    assert_eq!(view.viewer.resource.unwrap().value, mana, "nothing is paid");

    let (clear, events, _) = cast_firebolt(false);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ZoneEvent::Error { .. })),
        "{events:?}"
    );
    assert!(
        clear.snapshot_for_player(1).unwrap().viewer.cast.is_some(),
        "the same cast starts with a clear line"
    );
}
