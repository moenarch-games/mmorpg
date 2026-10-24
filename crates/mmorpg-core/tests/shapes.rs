//! Cones, beams and chains (#292): Frost Breath strikes and slows every visible hostile in its
//! arc and range; Lightning Beam strikes everything on its line until a wall; Chain Lightning
//! jumps twice to the nearest unhit hostile its last victim can see.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, AbilityId, CreatureId, EntityRef, SpawnGrid, StaticCollider,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, NORTHWARD, WOLF};
use support::classes;

fn wolf(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground plus optional 4 m tall walls: `(centre XZ, half extents XZ)`.
fn content(walls: &[([i32; 2], [i32; 2])], wolves: &[[i32; 2]]) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    for (&([x, z], [hx, hz]), id) in walls.iter().zip(2..) {
        colliders.push(StaticCollider {
            id,
            position: [x, 200, z],
            half_extents: [hx, 200, hz],
        });
    }
    let definition = ZoneDefinition::with_spawn_grid(
        77,
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let spawns = wolves
        .iter()
        .zip(1..)
        .map(|(&at, id)| arena::spawn(id, WOLF, at))
        .collect();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![arena::wolf(400, [1, 1])],
        spawns,
        Vec::new(),
        arena::GRAVEYARD,
    )
    .unwrap();
    Arc::new(
        content
            .with_creature_abilities(ABILITY_CATALOG_REVISION, Vec::new())
            .unwrap(),
    )
}

/// A level 8 Arcanist at the origin facing north uses `ability` (at `target`) and waits `ticks`.
fn cast(
    content: Arc<ZoneContent>,
    ability: AbilityId,
    target: Option<EntityRef>,
    ticks: u32,
) -> (ZoneSimulation, Vec<ZoneEvent>) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
    zone.add_player(1).unwrap();
    let zone = classes::at_level(zone, 1, 8);
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [0.0, 90.0, 0.0];
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let commands = [
        ZoneCommand::ChooseClass { class: 2, sex: 0 },
        arena::stand(NORTHWARD),
        ZoneCommand::UseAbility {
            ability: ability.get(),
            target,
        },
    ];
    let mut events = Vec::new();
    for (sequence, command) in (1..).zip(commands) {
        zone.apply_command(1, sequence, command).unwrap();
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
    }
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
    }
    (zone, events)
}

fn damage_to(events: &[ZoneEvent], unit: EntityRef) -> u32 {
    events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::DamageDealt { target, amount, .. } if target == unit => {
                Some(u32::from(amount))
            }
            _ => None,
        })
        .sum()
}

fn snared(zone: &ZoneSimulation, id: u32) -> bool {
    zone.snapshot()
        .unwrap()
        .creatures
        .iter()
        .find(|creature| creature.creature_id == CreatureId::new(id))
        .is_some_and(|creature| {
            creature
                .abilities
                .auras
                .iter()
                .any(|aura| aura.ability == ids::FROST_BREATH)
        })
}

#[test]
fn frost_breath_strikes_and_slows_what_it_can_see_in_its_arc_and_range() {
    // In the arc; 51° off the facing; beyond 8 m; behind a wall.
    let wolves = [[0, 500], [500, 400], [0, 1_000], [-500, 300]];
    let wall = ([-400, 200], [60, 60]);
    let (zone, events) = cast(
        content(&[wall], &wolves),
        ids::FROST_BREATH,
        None,
        mmorpg_core::golden::t30(2) as _,
    );
    let hits: Vec<_> = (1..=4).map(|id| damage_to(&events, wolf(id))).collect();
    assert!(hits[0] > 0, "{hits:?}");
    assert_eq!(hits[1..], [0, 0, 0], "{hits:?}");
    assert!(snared(&zone, 1));
    assert!(!snared(&zone, 2));
}

#[test]
fn lightning_beam_strikes_everything_on_its_line_until_a_wall() {
    let wolves = [[0, 600], [0, 1_200], [0, 1_800]];
    let (_, events) = cast(
        content(&[], &wolves),
        ids::LIGHTNING_BEAM,
        None,
        mmorpg_core::golden::t30(62) as _,
    );
    for id in 1..=3 {
        assert!(damage_to(&events, wolf(id)) > 0, "wolf {id} on the line");
    }
    let wall = ([0, 1_000], [500, 50]);
    let (_, events) = cast(
        content(&[wall], &wolves),
        ids::LIGHTNING_BEAM,
        None,
        mmorpg_core::golden::t30(62) as _,
    );
    assert!(damage_to(&events, wolf(1)) > 0);
    assert_eq!(
        damage_to(&events, wolf(2)) + damage_to(&events, wolf(3)),
        0,
        "the wall stops it"
    );
}

#[test]
fn chain_lightning_jumps_twice_to_the_nearest_visible_hostiles() {
    // A row 24 m out, 6 m apart: the chain reaches the third and spares the fourth.
    let row = [[0, 2_400], [600, 2_400], [1_200, 2_400], [1_800, 2_400]];
    let (_, events) = cast(
        content(&[], &row),
        ids::CHAIN_LIGHTNING,
        Some(wolf(1)),
        mmorpg_core::golden::t30(50) as _,
    );
    let hits: Vec<_> = (1..=4).map(|id| damage_to(&events, wolf(id))).collect();
    assert!(hits[..3].iter().all(|&hit| hit > 0), "{hits:?}");
    assert_eq!(hits[3], 0, "{hits:?}");
    // A wall between the first and second wolf stops the jump.
    let wall = ([300, 2_400], [50, 400]);
    let (_, events) = cast(
        content(&[wall], &row),
        ids::CHAIN_LIGHTNING,
        Some(wolf(1)),
        mmorpg_core::golden::t30(50) as _,
    );
    let hits: Vec<_> = (1..=4).map(|id| damage_to(&events, wolf(id))).collect();
    assert!(hits[0] > 0);
    assert_eq!(hits[1..], [0, 0, 0], "{hits:?}");
}
