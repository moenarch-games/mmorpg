//! Authoritative line of sight: targeted ranged abilities need a clear
//! physics ray from the caster to the target through static content.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureId, EntityRef, ErrorCode, SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand,
    ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR};
use support::classes;

const TARGET: EntityRef = EntityRef::Creature(CreatureId::new(1));
const ARCANIST: u8 = 2;

/// What stands halfway between the caster and the boar (#57).
#[derive(Clone, Copy, Debug)]
enum Between {
    Nothing,
    /// A 2 m high wall.
    Wall,
    /// A 2-unit-thin plank, 2 m high.
    Plank,
    /// A ramp wedge (#59) whose top rises past the sight line.
    Ramp,
    /// A wall with a gap the sight line runs through.
    Gap,
}

/// Flat ground, a neutral boar 25 m north of the spawn and, optionally, a
/// 2 m high wall halfway between them. Classes need the bound ability catalog.
fn content(wall: bool) -> Arc<ZoneContent> {
    content_with(if wall {
        Between::Wall
    } else {
        Between::Nothing
    })
}

fn content_with(between: Between) -> Arc<ZoneContent> {
    let wall = matches!(between, Between::Wall);
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
    match between {
        Between::Plank => colliders.push(StaticCollider {
            id: 2,
            position: [0, 100, 1_250],
            half_extents: [400, 100, 1],
        }),
        // Two walls leaving a 1 m gap around the sight line at x = 0.
        Between::Gap => colliders.extend([-1, 1].map(|side: i32| StaticCollider {
            id: if side < 0 { 2 } else { 3 },
            position: [side * 450, 100, 1_250],
            half_extents: [400, 100, 20],
        })),
        Between::Nothing | Between::Wall | Between::Ramp => {}
    }
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![arena::boar()],
        vec![arena::spawn(1, BOAR, [0, 2_500])],
        Vec::new(),
        arena::GRAVEYARD,
    )
    .unwrap()
    .with_creature_abilities(Vec::new())
    .unwrap();
    let ramps = match between {
        // 3 m high at its far end, rising toward the boar: the sight line at about 1 m meets
        // its slope.
        Between::Ramp => vec![mmorpg_core::Ramp {
            body: 9,
            position: [0, 150, 1_250],
            half_extents: [400, 150, 300],
            rises: mmorpg_core::RampRise::PosZ,
        }],
        _ => Vec::new(),
    };
    Arc::new(content.with_ramps(ramps).unwrap())
}

/// The first-tick events of an arcanist that targets the boar and casts Firebolt.
fn cast_firebolt(wall: bool) -> (ZoneSimulation, Vec<ZoneEvent>, u16) {
    cast_firebolt_with(content(wall))
}

fn cast_firebolt_with(content: Arc<ZoneContent>) -> (ZoneSimulation, Vec<ZoneEvent>, u16) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
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

/// Whether the Firebolt at the boar is refused for line of sight with `between` in the way.
fn refused(between: Between) -> bool {
    let (_, events, _) = cast_firebolt_with(content_with(between));
    events.contains(&ZoneEvent::Error {
        code: ErrorCode::LineOfSight,
        target: Some(TARGET),
    })
}

#[test]
fn thin_planks_and_ramps_block_sight_and_a_gap_lets_it_through() {
    let run = || {
        for (between, blocked) in [
            (Between::Nothing, false),
            (Between::Plank, true),
            (Between::Ramp, true),
            (Between::Gap, false),
        ] {
            assert_eq!(refused(between), blocked, "{between:?}");
        }
    };
    run();
    // The same decisions on physics-engine (#57, physics-engine #330).
    #[cfg(feature = "physics-ab")]
    mmorpg_core::physics::using_backend(mmorpg_core::physics::Backend::Engine, run);
}
