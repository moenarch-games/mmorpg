//! The Rogue's flank (#949, Atlas 5.2.i949): Gouge holds the wolf blocking the way north while
//! Sinister Strike from behind another wolf deals the flank bonus and turns it round, so a
//! second strike from the same spot is ordinary; one facing the Rogue takes the ordinary hit.
//! The fight replays exactly.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, CanonicalZoneSnapshot, CreatureFamily, CreatureId, EntityRef, GLOBAL_COOLDOWN_TICKS,
    SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent,
    ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR, NORTHWARD, WOLF};

const ROGUE: u8 = 4;
const SOUTHWARD: u16 = 32_768;
const TOWARD_SIDE: u16 = 49_152;
/// The blocker north of the Rogue, the wolf to the south and the wolf to the east.
const BLOCKER: u32 = 1;
const SOUTH_WOLF: u32 = 2;
const SIDE_WOLF: u32 = 3;

fn creature(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground and three sturdy neutral creatures around the origin: a boar blocking the way
/// north, whose fight the wolves do not join, a wolf to the south facing south and one to the
/// side facing the origin.
fn content() -> Arc<ZoneContent> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
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
    let spawns = [
        (BLOCKER, BOAR, [0, 200], SOUTHWARD),
        (SOUTH_WOLF, WOLF, [0, -200], SOUTHWARD),
        (SIDE_WOLF, WOLF, [-200, 0], 16_384),
    ]
    .into_iter()
    .map(|(id, template, at, facing)| mmorpg_core::CreatureSpawn {
        facing,
        ..arena::spawn(id, template, at)
    })
    .collect();
    let wolf = mmorpg_core::CreatureTemplate {
        behaviour: mmorpg_core::CreatureBehaviour::Neutral,
        ..arena::wolf(5_000, [1, 1])
    };
    let boar = mmorpg_core::CreatureTemplate {
        id: BOAR,
        name: "Test Boar".into(),
        family: CreatureFamily::Boar,
        ..wolf.clone()
    };
    Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![wolf, boar],
            spawns,
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Vec::new())
        .unwrap(),
    )
}

/// What the fight showed: the strikes' damage in order and whether each flanked, the blocker's
/// position each tick of its stun, and the final canonical state.
#[derive(Debug, PartialEq)]
struct Fight {
    strikes: Vec<(u32, u16, bool)>,
    blocker_stunned: bool,
    blocker_path: Vec<[f32; 3]>,
    state: CanonicalZoneSnapshot,
}

/// A level-8 Rogue at the origin, or `front` of the south wolf, gouges the blocker (out of reach
/// from the front), strikes the south wolf twice from where it
/// stands, then strikes the side wolf.
fn fight(front: bool) -> Fight {
    let content = content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut zone = support::classes::at_level(zone, 1, 8);
    zone.apply_command(
        1,
        1,
        ZoneCommand::ChooseClass {
            class: ROGUE,
            sex: 0,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [0.0, 90.0, if front { -400.0 } else { 0.0 }];
    state.players[0].combat.abilities.resource = 100;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
    let mut sequence = 1;
    let mut cast = |zone: &mut ZoneSimulation, ability: Option<AbilityId>, facing: u16| {
        sequence += 1;
        let command = match ability {
            Some(ability) => ZoneCommand::UseAbility {
                ability: ability.get(),
                target: None,
                facing: Some(facing),
            },
            None => arena::stand(facing),
        };
        zone.apply_command(1, sequence, command).unwrap();
    };
    // The events of `ticks` ticks, and the blocker's position after each.
    let run = |zone: &mut ZoneSimulation, ticks: u16| {
        let mut events = Vec::new();
        let mut path = Vec::new();
        for _ in 0..ticks {
            zone.advance_tick().unwrap();
            events.extend(zone.snapshot_for_player(1).unwrap().events);
            path.extend(
                zone.snapshot()
                    .unwrap()
                    .creatures
                    .iter()
                    .filter(|record| record.creature_id == CreatureId::new(BLOCKER))
                    .map(|record| record.position),
            );
        }
        (events, path)
    };
    cast(&mut zone, None, NORTHWARD);
    run(&mut zone, 5);
    cast(&mut zone, Some(ids::GOUGE), NORTHWARD);
    run(&mut zone, 2);
    let blocker_stunned = zone
        .snapshot()
        .unwrap()
        .creatures
        .iter()
        .find(|record| record.creature_id == CreatureId::new(BLOCKER))
        .is_some_and(|record| {
            record
                .abilities
                .auras
                .iter()
                .any(|aura| aura.ability == ids::GOUGE)
        });
    let mut strikes = Vec::new();
    let mut blocker_path = Vec::new();
    let toward_south = if front { NORTHWARD } else { SOUTHWARD };
    for facing in [toward_south, toward_south, TOWARD_SIDE] {
        let (_, path) = run(&mut zone, GLOBAL_COOLDOWN_TICKS);
        blocker_path.extend(path);
        cast(&mut zone, Some(ids::SINISTER_STRIKE), facing);
        let (events, path) = run(&mut zone, 2);
        blocker_path.extend(path);
        for event in &events {
            if let ZoneEvent::DamageDealt {
                target: EntityRef::Creature(id),
                amount,
                ..
            } = *event
            {
                let flanked = events.iter().any(|event| {
                    matches!(*event, ZoneEvent::Flanked { target, .. } if target == creature(id.get()))
                });
                strikes.push((id.get(), amount, flanked));
            }
        }
    }
    Fight {
        strikes,
        blocker_stunned,
        blocker_path,
        state: zone.snapshot().unwrap(),
    }
}

#[test]
fn gouge_holds_the_blocker_and_only_a_flank_pays() {
    let behind = fight(false);
    let facing = fight(true);

    // Gouge holds the blocker: stunned, and it does not move during the strikes it lasts.
    assert!(behind.blocker_stunned);
    assert!(
        behind.blocker_path[..usize::from(GLOBAL_COOLDOWN_TICKS)]
            .windows(2)
            .all(|pair| pair[0] == pair[1]),
        "{:?}",
        behind.blocker_path
    );

    let ids: Vec<_> = behind.strikes.iter().map(|&(id, ..)| id).collect();
    assert_eq!(ids, [SOUTH_WOLF, SOUTH_WOLF, SIDE_WOLF], "{behind:?}");
    // From behind the south wolf: the flank and its bonus; then it has turned, so the same spot
    // gives an ordinary hit, as does the side wolf, which faces the Rogue.
    let flags: Vec<_> = behind.strikes.iter().map(|&(.., flank)| flank).collect();
    assert_eq!(flags, [true, false, false]);
    // The same strike from in front of the south wolf: the same roll, the ordinary hit.
    assert!(
        facing.strikes.iter().all(|&(.., flank)| !flank),
        "{facing:?}"
    );
    let (rear, front) = (
        u32::from(behind.strikes[0].1),
        u32::from(facing.strikes[0].1),
    );
    assert!(
        rear * 100 >= front * 145 && rear * 100 <= front * 155,
        "rear {rear}, front {front}"
    );

    // Exact replay.
    assert_eq!(fight(false), behind);
}
