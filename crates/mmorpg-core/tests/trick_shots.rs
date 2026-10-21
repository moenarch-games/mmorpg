//! Aimed specials (#291): a Piercing Shot passes through up to three enemies in a line, each
//! later hit weaker, and walls stop it; a Ricochet Shot mirrors off a wall into what it could
//! not reach directly.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    ABILITY_CATALOG_REVISION, AbilityId, CreatureId, EntityRef, Flight, SpawnGrid, StaticCollider,
    ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, NORTHWARD, WOLF};
use support::classes;

fn wolf(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground, plus an optional wall: `(centre XZ, half extents XZ)`, 4 m tall.
fn content(wall: Option<([i32; 2], [i32; 2])>, wolves: &[[i32; 2]]) -> Arc<ZoneContent> {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    if let Some(([x, z], [hx, hz])) = wall {
        colliders.push(StaticCollider {
            id: 2,
            position: [x, 200, z],
            half_extents: [hx, 200, hz],
        });
    }
    let definition = ZoneDefinition::with_spawn_grid(
        77,
        [0, -1, 0],
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

/// A level 6 Ranger at the origin facing `facing`, who looses `ability` and lets it fly.
fn shoot(
    content: Arc<ZoneContent>,
    facing: u16,
    ability: AbilityId,
) -> (ZoneSimulation, Vec<ZoneEvent>) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
    zone.add_player(1).unwrap();
    let zone = classes::at_level(zone, 1, 6);
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [0.0, 90.0, 0.0];
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let commands = [
        ZoneCommand::ChooseClass { class: 1, sex: 0 },
        arena::stand(facing),
        // A selected target does not matter: aimed specials always fly along the facing.
        ZoneCommand::UseAbility {
            ability: ability.get(),
            target: None,
        },
    ];
    let mut events = Vec::new();
    for (sequence, command) in (1..).zip(commands) {
        zone.apply_command(1, sequence, command).unwrap();
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
    }
    for _ in 0..30 {
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

#[test]
fn a_piercing_shot_passes_three_wolves_each_weaker_and_spares_the_fourth() {
    // A free arrow leaves at chest height and drops to wolf height from about 12 m out.
    let line = [[0, 1_300], [0, 1_500], [0, 1_700], [0, 1_900]];
    let (zone, events) = shoot(content(None, &line), NORTHWARD, ids::PIERCING_SHOT);
    let hits: Vec<_> = (1..=4).map(|id| damage_to(&events, wolf(id))).collect();
    assert!(
        hits[0] > hits[1] && hits[1] > hits[2] && hits[2] > 0,
        "{hits:?}"
    );
    assert_eq!(hits[3], 0, "the fourth wolf is spared: {hits:?}");
    let projectile = zone.snapshot().unwrap().projectiles[0];
    assert!(matches!(projectile.flight, Flight::Stuck { unit: Some(unit), .. } if unit == wolf(3)));
}

#[test]
fn a_wall_stops_a_piercing_shot() {
    let (_, events) = shoot(
        content(Some(([0, 1_550], [500, 50])), &[[0, 1_300], [0, 1_800]]),
        NORTHWARD,
        ids::PIERCING_SHOT,
    );
    assert!(damage_to(&events, wolf(1)) > 0);
    assert_eq!(damage_to(&events, wolf(2)), 0, "the wall stopped it");
}

#[test]
fn a_ricochet_shot_mirrors_off_a_wall_into_a_wolf_out_of_line() {
    // Facing north-east, the wall's south face at z = 950 turns the shot south-east.
    let wall = Some(([0, 1_000], [3_000, 50]));
    let around = [[1_300, 600]];
    let (zone, events) = shoot(content(wall, &around), 8_192, ids::RICOCHET_SHOT);
    assert!(
        damage_to(&events, wolf(1)) > 0,
        "the bounce reached the wolf"
    );
    assert_eq!(zone.snapshot().unwrap().projectiles[0].bounces, 1);
    // The same line without a ricochet sticks in the wall.
    let (_, events) = shoot(content(wall, &around), 8_192, ids::AIMED_SHOT);
    assert_eq!(damage_to(&events, wolf(1)), 0);
}
