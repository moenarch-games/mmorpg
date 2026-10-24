//! Destructible structures, pathing and line of sight (#307): a creature walks around a wall
//! in its way, comes through a gap a destroyed piece opens, and sight lines for spells follow
//! the pieces that stand.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureId, EntityRef, ErrorCode, PieceId, StaticCollider, StructurePiece, StructureSite,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena;

const ARCANIST: u8 = 2;
/// Spawn slots lie along +X and +Z; the tests play out toward −Z.
const SOUTH: u16 = 32_768;

fn piece(id: u16, x: i32, z: i32, half: [i32; 3]) -> StructurePiece {
    StructurePiece {
        id: PieceId::new(id),
        site: 1,
        body: 60 + u32::from(id),
        position: [x, half[1], z],
        half_extents: half,
        health: 10,
        supports: Vec::new(),
        rubble: None,
    }
}

/// Flat ground, `walls` and `pieces`, and an aggressive wolf at `wolf`.
fn content(
    walls: Vec<StaticCollider>,
    pieces: Vec<StructurePiece>,
    creature: mmorpg_core::CreatureTemplate,
    at: [i32; 2],
) -> Arc<ZoneContent> {
    let template = creature.id;
    let mut definition_colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    definition_colliders.extend(walls);
    let definition = mmorpg_core::ZoneDefinition::with_spawn_grid(
        77,
        mmorpg_core::STANDARD_GRAVITY,
        mmorpg_core::SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        definition_colliders,
    )
    .unwrap();
    Arc::new(
        ZoneContent::new(
            definition,
            mmorpg_core::ZoneAreas::default(),
            vec![creature],
            vec![arena::spawn(1, template, at)],
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(mmorpg_core::ABILITY_CATALOG_REVISION, Vec::new())
        .unwrap()
        .with_structures(
            vec![StructureSite {
                id: 1,
                name: "Test Wall".into(),
                regrowth_ticks: mmorpg_core::golden::t30(90_000) as _,
            }],
            pieces,
        )
        .unwrap(),
    )
}

fn wolf_at(zone: &ZoneSimulation) -> [f32; 2] {
    let state = zone.snapshot().unwrap();
    let wolf = state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == CreatureId::new(1))
        .unwrap();
    [wolf.position[0], wolf.position[2]]
}

fn reaches_player(content: Arc<ZoneContent>, ticks: u32) -> bool {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
    zone.add_player(1).unwrap();
    (0..ticks).any(|_| {
        zone.advance_tick().unwrap();
        let [x, z] = wolf_at(&zone);
        x.hypot(z) < 300.0
    })
}

#[test]
fn a_creature_walks_around_a_wall_in_its_way() {
    // A 12 m wall 4 m south of the player; the wolf aggroes from 9 m south of it.
    let wall = StaticCollider {
        id: 2,
        position: [0, 150, -400],
        half_extents: [600, 150, 20],
    };
    let wolf = arena::wolf(500, [1, 1]);
    assert!(reaches_player(
        content(vec![wall], Vec::new(), wolf, [0, -900]),
        240
    ));
}

#[test]
fn a_destroyed_piece_opens_the_way_and_the_sight_line() {
    // A box of four wall pieces around the boar 15 m south; only the north wall faces the
    // arcanist. The boar is neutral: it never leaves on its own.
    let boar_box = |north_health: u32| {
        let mut north = piece(1, 0, -1_250, [300, 125, 20]);
        north.health = north_health;
        vec![
            north,
            piece(2, 0, -1_650, [300, 125, 20]),
            piece(3, -320, -1_450, [20, 125, 220]),
            piece(4, 320, -1_450, [20, 125, 220]),
        ]
    };
    let boar = mmorpg_core::CreatureTemplate {
        health: 400,
        ..arena::boar()
    };
    let mut zone = ZoneSimulation::with_content(
        ZoneId::new(9),
        content(Vec::new(), boar_box(10), boar, [0, -1_450]),
    )
    .unwrap();
    zone.add_player(1).unwrap();
    let mut zone = support::classes::at_level(zone, 1, 6);
    let mut sequence = 0;
    let mut send = |zone: &mut ZoneSimulation, command| {
        sequence += 1;
        zone.apply_command(1, sequence, command).unwrap();
    };
    send(
        &mut zone,
        ZoneCommand::ChooseClass {
            class: ARCANIST,
            sex: 0,
        },
    );
    send(&mut zone, arena::stand(SOUTH));
    send(
        &mut zone,
        ZoneCommand::SelectTarget(Some(EntityRef::Creature(CreatureId::new(1)))),
    );
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.abilities.resource = 200;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let firebolt = ZoneCommand::UseAbility {
        ability: ids::FIREBOLT.get(),
        target: None,
    };
    // The north wall stands between them: no sight line.
    send(&mut zone, firebolt);
    zone.advance_tick().unwrap();
    let events = zone.snapshot_for_player(1).unwrap().events;
    assert!(
        events.contains(&ZoneEvent::Error {
            code: ErrorCode::LineOfSight,
            target: Some(EntityRef::Creature(CreatureId::new(1))),
        }),
        "{events:?}"
    );
    // Break the north wall (canonical piece state, as a blast would leave it) and recover.
    let mut state = zone.snapshot().unwrap();
    state.pieces[0].health = 0;
    state.pieces[0].changed_tick = state.tick;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    for _ in 0..60 {
        zone.advance_tick().unwrap();
    }
    send(&mut zone, firebolt);
    zone.advance_tick().unwrap();
    let view = zone.snapshot_for_player(1).unwrap();
    assert!(
        !view
            .events
            .iter()
            .any(|event| matches!(event, ZoneEvent::Error { .. })),
        "{:?}",
        view.events
    );
    assert!(
        view.viewer.cast.is_some(),
        "the cast starts through the gap"
    );
}

#[test]
fn an_enclosed_creature_comes_out_through_a_broken_wall() {
    // The wolf is boxed in by four pieces 9 m south of the player. While the box stands it
    // cannot reach the player; with the far (south) wall broken it routes out through the gap
    // and around the box.
    let walls = |south: bool| {
        let mut pieces = vec![
            piece(1, 0, -700, [300, 125, 20]),
            piece(2, -320, -900, [20, 125, 220]),
            piece(3, 320, -900, [20, 125, 220]),
        ];
        if south {
            pieces.push(piece(4, 0, -1_100, [300, 125, 20]));
        }
        pieces
    };
    let wolf = arena::wolf(500, [1, 1]);
    assert!(!reaches_player(
        content(Vec::new(), walls(true), wolf.clone(), [0, -900]),
        300
    ));
    assert!(reaches_player(
        content(Vec::new(), walls(false), wolf, [0, -900]),
        300
    ));
}
