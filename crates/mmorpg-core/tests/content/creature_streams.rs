//! Per-creature random streams (#861): adding a spawn elsewhere changes no
//! other creature's level, wander, swings, damage taken or loot.
use crate::support;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalCreatureSnapshot, CreatureAi, CreatureId, CreatureLife, CreatureTemplate, EntityRef,
    ItemId, LootOutcome, LootTable, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR, NORTHWARD, WOLF};

const HUNTED: CreatureId = CreatureId::new(1);
const DUMMY: CreatureId = CreatureId::new(2);
const ROAMER: CreatureId = CreatureId::new(3);

/// A level 1–5 wolf to hunt and a wandering boar elsewhere; `dummy` adds a second wandering
/// boar whose ID sorts between them, so it also ticks between them.
fn content(dummy: bool) -> Arc<ZoneContent> {
    let wolf = CreatureTemplate {
        max_level: 5,
        health_per_level: 6,
        ..arena::wolf(40, [1, 4])
    };
    let wander = |id, position| mmorpg_core::CreatureSpawn {
        wander_radius: 600,
        levels: None,
        ..arena::spawn(id, BOAR, position)
    };
    let mut spawns = vec![arena::spawn(1, WOLF, [0, 2_000])];
    if dummy {
        spawns.push(wander(2, [-4_000, 4_000]));
    }
    spawns.push(wander(3, [4_000, 4_000]));
    let table = LootTable::new(
        [1, 500],
        &[
            LootOutcome::Nothing { weight: 1 },
            LootOutcome::Item {
                weight: 1,
                item: ItemId::new(1),
                quantity: [1, 5],
            },
        ],
    )
    .unwrap();
    let content = arena::arena(vec![wolf, arena::boar()], spawns, vec![]);
    Arc::new(
        content
            .as_ref()
            .clone()
            .with_rng_seed(0x861)
            .with_loot_tables(vec![(WOLF, table)])
            .unwrap(),
    )
}

fn creature(zone: &ZoneSimulation, id: CreatureId) -> CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == id)
        .unwrap()
}

/// Per tick: the hunted wolf, the roaming boar, the player's health and what the player saw.
type Trace = Vec<(
    CanonicalCreatureSnapshot,
    CanonicalCreatureSnapshot,
    u32,
    Vec<ZoneEvent>,
)>;

/// A player walks to the wolf, swings at it until it dies, and the corpse lies a while.
fn hunt(dummy: bool) -> (Trace, ZoneSimulation) {
    hunt_in(dummy, 0)
}

/// [`hunt`] in a world with the host's `world_seed` (#1111).
fn hunt_in(dummy: bool, world_seed: u64) -> (Trace, ZoneSimulation) {
    let mut zone =
        ZoneSimulation::with_world_seed(ZoneId::new(5), content(dummy), world_seed).unwrap();
    zone.add_player(1).unwrap();
    let wolf = EntityRef::Creature(HUNTED);
    let mut sequence = 0;
    let mut send = |zone: &mut ZoneSimulation, command| {
        sequence += 1;
        zone.apply_command(1, sequence, command).unwrap();
    };
    send(&mut zone, arena::walk(NORTHWARD));
    let mut trace = Trace::new();
    let mut engaged = false;
    let mut died = None;
    for tick in 0..mmorpg_core::golden::t30(4_000) {
        if !engaged && creature(&zone, HUNTED).ai == CreatureAi::Engaged {
            engaged = true;
            send(&mut zone, arena::stand(NORTHWARD));
            send(&mut zone, ZoneCommand::SelectTarget(Some(wolf)));
        }
        if engaged
            && died.is_none()
            && let Some(swing) = arena::swing_at(&zone, 1, wolf)
        {
            send(&mut zone, swing);
        }
        zone.advance_tick().unwrap();
        let view = zone.snapshot_for_player(1).unwrap();
        trace.push((
            creature(&zone, HUNTED),
            creature(&zone, ROAMER),
            view.viewer.health,
            view.events,
        ));
        if died.is_none() && !matches!(creature(&zone, HUNTED).life, CreatureLife::Alive) {
            died = Some(tick);
        }
        if died.is_some_and(|at| tick > at + 60) {
            return (trace, zone);
        }
    }
    panic!("the wolf never died");
}

#[test]
fn another_spawn_changes_no_existing_creature_roll() {
    let (alone, base) = hunt(false);
    let (beside, extended) = hunt(true);
    // The fight was a real one: the wolf rolled a level, swung and hit, and dropped loot.
    let wolf = &alone.last().unwrap().0;
    assert!(wolf.loot.is_some(), "the wolf drops loot");
    assert!(alone.iter().any(|(_, _, health, _)| *health < alone[0].2));
    // The dummy drew from its own stream meanwhile: it wandered.
    let fresh = ZoneSimulation::with_content(ZoneId::new(5), content(true)).unwrap();
    assert_ne!(
        creature(&extended, DUMMY).rng_state,
        creature(&fresh, DUMMY).rng_state
    );
    assert_eq!(alone.len(), beside.len(), "the fight ends on the same tick");
    for (tick, (a, b)) in alone.iter().zip(&beside).enumerate() {
        assert_eq!(a, b, "tick {tick} differs with another spawn in the zone");
    }
    assert_eq!(
        creature(&base, HUNTED).loot,
        creature(&extended, HUNTED).loot
    );
}

const WORLD: u64 = 0x5eed_0000_1111_0001;
const OTHER_WORLD: u64 = 0x5eed_0000_1111_0002;

#[test]
fn a_world_seed_rerolls_the_same_creature_life() {
    let loot = |world_seed| {
        let (_, zone) = hunt_in(false, world_seed);
        let wolf = creature(&zone, HUNTED);
        assert!(wolf.loot.is_some(), "the wolf drops loot");
        (wolf.generation, wolf.loot)
    };
    let (public, world, other) = (loot(0), loot(WORLD), loot(OTHER_WORLD));
    // The same life of the same creature, so only the world seed differs.
    assert_eq!(public.0, world.0);
    assert_eq!(world.0, other.0);
    assert_ne!(world.1, other.1, "two worlds roll different loot");
    assert_ne!(
        world.1, public.1,
        "the public seed alone does not tell the drop"
    );
    assert_eq!(loot(WORLD), world, "one world rolls the same loot again");
}

#[test]
fn a_restored_world_keeps_its_seed_and_its_next_draws() {
    let (_, mut live) = hunt_in(false, WORLD);
    let checkpoint = live.snapshot().unwrap();
    assert_eq!(checkpoint.world_seed, WORLD);
    let content = Arc::clone(live.content());
    let mut restored =
        ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(&content)).unwrap();
    assert_eq!(restored.snapshot().unwrap(), checkpoint);
    // The same state under another seed: its next life draws differently.
    let mut reseeded = ZoneSimulation::from_snapshot(
        mmorpg_core::CanonicalZoneSnapshot {
            world_seed: OTHER_WORLD,
            ..checkpoint.clone()
        },
        content,
    )
    .unwrap();
    let generation = creature(&live, HUNTED).generation;
    while creature(&live, HUNTED).generation == generation
        || !matches!(creature(&live, HUNTED).life, CreatureLife::Alive)
    {
        assert!(
            live.current_tick() < checkpoint.tick + mmorpg_core::golden::t30(4_000),
            "the wolf never came back"
        );
        live.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        reseeded.advance_tick().unwrap();
    }
    assert_eq!(restored.snapshot().unwrap(), live.snapshot().unwrap());
    assert_ne!(
        creature(&reseeded, HUNTED).rng_state,
        creature(&live, HUNTED).rng_state,
        "the respawned life draws from the restored world seed"
    );
}
