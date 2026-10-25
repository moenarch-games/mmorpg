//! The Rogue's thrown weapons (#290): a throwing knife flies along the facing (#517); a venom
//! flask is lobbed along the facing and shatters on impact, poisoning everything near; a bomb
//! is lobbed, bounces off walls and rolls, and bursts on a unit or after its fuse.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    AbilityId, CreatureId, EntityRef, Flight, ProjectileKind, SpawnGrid, StaticCollider, ZoneAreas,
    ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR, NORTHWARD};
use support::classes;

fn boar(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground, plus an optional wall: `(centre XZ, half extents XZ)`, 4 m tall, and neutral
/// boars that stand still until hurt.
fn content(wall: Option<([i32; 2], [i32; 2])>, boars: &[[i32; 2]]) -> Arc<ZoneContent> {
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
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let spawns = boars
        .iter()
        .zip(1..)
        .map(|(&at, id)| arena::spawn(id, BOAR, at))
        .collect();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![arena::boar()],
        spawns,
        Vec::new(),
        arena::GRAVEYARD,
    )
    .unwrap();
    Arc::new(content.with_creature_abilities(Vec::new()).unwrap())
}

/// A level 6 Rogue at the origin facing `facing` throws `ability` (at `target`, if any), then
/// the zone runs `ticks` more ticks; every tick's zone state is kept.
fn throw(
    content: Arc<ZoneContent>,
    facing: u16,
    ability: AbilityId,
    target: Option<EntityRef>,
    ticks: usize,
) -> (Vec<mmorpg_core::CanonicalZoneSnapshot>, Vec<ZoneEvent>) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), content).unwrap();
    zone.add_player(1).unwrap();
    let zone = classes::at_level(zone, 1, 6);
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [0.0, 90.0, 0.0];
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let commands = [
        ZoneCommand::ChooseClass { class: 4, sex: 0 },
        arena::stand(facing),
        ZoneCommand::UseAbility {
            ability: ability.get(),
            target,
        },
    ];
    let mut events = Vec::new();
    let mut states = Vec::new();
    for (sequence, command) in (1..).zip(commands) {
        zone.apply_command(1, sequence, command).unwrap();
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
    }
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
        states.push(zone.snapshot().unwrap());
    }
    (states, events)
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

fn poisoned(state: &mmorpg_core::CanonicalZoneSnapshot, id: u32) -> bool {
    state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == CreatureId::new(id))
        .is_some_and(|creature| {
            creature
                .abilities
                .auras
                .iter()
                .any(|aura| aura.ability == ids::VENOM_FLASK)
        })
}

fn creature_xz(state: &mmorpg_core::CanonicalZoneSnapshot, id: u32) -> [f32; 2] {
    let creature = state
        .creatures
        .iter()
        .find(|creature| creature.creature_id == CreatureId::new(id))
        .unwrap();
    [creature.position[0], creature.position[2]]
}

#[test]
fn a_throwing_knife_flies_along_the_facing_whatever_is_selected() {
    // The boar stands off the facing: the knife follows the facing, not the selection (#517).
    let (states, events) = throw(
        content(None, &[[800, 1_200]]),
        NORTHWARD,
        ids::THROWING_KNIFE,
        Some(boar(1)),
        60,
    );
    assert_eq!(damage_to(&events, boar(1)), 0);
    let knife = states.last().unwrap().projectiles[0];
    assert_eq!(knife.kind, ProjectileKind::Knife);
    assert!(!matches!(knife.flight, Flight::Stuck { unit: Some(unit), .. } if unit == boar(1)));
}

#[test]
fn a_venom_flask_shatters_on_the_boar_ahead_and_poisons_those_near_it() {
    // The flask arcs onto the boar 10 m ahead; its neighbour 2 m beyond is splashed, the boar
    // 8 m away is not.
    let (states, _) = throw(
        content(None, &[[0, 1_000], [0, 1_200], [800, 1_000]]),
        NORTHWARD,
        ids::VENOM_FLASK,
        None,
        120,
    );
    let last = states.last().unwrap();
    assert!(poisoned(last, 1) && poisoned(last, 2));
    assert!(!poisoned(last, 3));
    // It burst rather than sticking, and is gone once the burst has shown.
    assert!(states.iter().any(|state| {
        state
            .projectiles
            .iter()
            .any(|p| matches!(p.flight, Flight::Burst { .. }))
    }));
    assert!(last.projectiles.is_empty());
}

#[test]
fn a_bomb_bounces_off_a_wall_and_bursts_on_its_fuse_where_the_wall_shields_the_far_side() {
    // A wall 3 m ahead: the bomb bounces back toward the thrower, rolls and bursts on its fuse
    // (2.5 s); the boar beside the thrower's side of the wall is blasted, the one behind it is
    // not.
    let wall = Some(([0, 300], [2_000, 50]));
    let (states, events) = throw(
        content(wall, &[[200, 150], [0, 600]]),
        NORTHWARD,
        ids::BLASTING_BOMB,
        None,
        usize::from(mmorpg_core::BOMB_FUSE_TICKS) + 10,
    );
    let flights: Vec<_> = states
        .iter()
        .filter_map(|state| state.projectiles.first())
        .collect();
    let burst_at = flights
        .iter()
        .position(|projectile| matches!(projectile.flight, Flight::Burst { .. }))
        .expect("the bomb bursts");
    // Nothing set it off early: it burst on its fuse, on the thrower's side of the wall.
    assert!(
        burst_at + 4 >= usize::from(mmorpg_core::BOMB_FUSE_TICKS),
        "{burst_at}"
    );
    let bomb = flights[burst_at];
    assert!(bomb.position[2] < 250.0, "{:?}", bomb.position);
    // It moved away from the wall after touching it: it bounced.
    let nearest = flights[..burst_at]
        .iter()
        .map(|projectile| projectile.position[2])
        .fold(f32::MIN, f32::max);
    assert!(
        nearest > 200.0 && bomb.position[2] < nearest - 20.0,
        "{nearest}"
    );
    assert!(damage_to(&events, boar(1)) > 0);
    assert_eq!(
        damage_to(&events, boar(2)),
        0,
        "the wall shields the far boar"
    );
    // The blast threw the near boar away from the burst.
    let before = creature_xz(&states[burst_at - 1], 1);
    let after = creature_xz(states.last().unwrap(), 1);
    assert!(after[0] > before[0], "{before:?} -> {after:?}");
}

#[test]
fn a_bomb_bursts_at_once_on_the_boar_it_lands_on() {
    let (states, events) = throw(
        content(None, &[[0, 1_000]]),
        NORTHWARD,
        ids::BLASTING_BOMB,
        None,
        120,
    );
    assert!(damage_to(&events, boar(1)) > 0);
    let burst_at = states
        .iter()
        .position(|state| {
            state
                .projectiles
                .iter()
                .any(|p| matches!(p.flight, Flight::Burst { .. }))
        })
        .expect("the bomb bursts");
    assert!(
        burst_at < usize::from(mmorpg_core::BOMB_FUSE_TICKS) / 2,
        "{burst_at}"
    );
}
