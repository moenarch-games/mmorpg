//! Cleaving swings (#909): with Greyhaven's timed combat (#679) one well-placed directed slash
//! strikes every eligible unit its arc sweeps, each once, while units out of the arc, out of its
//! height lane or behind cover are spared; the outcome replays identically.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CreatureBehaviour, CreatureId, CreatureTemplate, CreatureTemplateId, EntityRef, StaticCollider,
    StrikeAim, StrikeHeight, StrikeSide, TimedCombat, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, GRAVEYARD};

/// Spawn slots fill +X/+Z from the origin, so creatures stand to the south.
const SOUTH: u16 = 32_768;
const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
const RAT: CreatureTemplateId = CreatureTemplateId::new(2);

fn template(id: CreatureTemplateId, half_extents: [i32; 3]) -> CreatureTemplate {
    CreatureTemplate {
        id,
        behaviour: CreatureBehaviour::Neutral,
        half_extents,
        ..arena::wolf(500, [1, 1])
    }
}

/// Flat ground with Greyhaven's timed combat, a pillar south-west of the Warden, and:
/// two wolves ahead (1, 2), a wolf off to the side (3), a rat too short for a middle slash (4)
/// and a wolf behind the pillar (5).
fn content() -> Arc<ZoneContent> {
    let colliders = vec![
        StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [10_000, 50, 10_000],
        },
        StaticCollider {
            id: 2,
            position: [-65, 100, -130],
            half_extents: [12, 100, 12],
        },
    ];
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        mmorpg_core::SpawnGrid {
            origin: [0, 0],
            columns: 32,
            spacing: 100,
        },
        colliders,
    )
    .unwrap();
    let spawns = [
        (1, WOLF, 0, -170),
        (2, WOLF, 85, -180),
        (3, WOLF, 180, -90),
        (4, RAT, -30, -90),
        (5, WOLF, -90, -190),
    ]
    .iter()
    .map(|&(id, template, x, z)| arena::spawn(id, template, [x, z]))
    .collect();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![template(WOLF, [40, 45, 40]), template(RAT, [25, 20, 25])],
        spawns,
        Vec::new(),
        GRAVEYARD,
    )
    .unwrap()
    .with_creature_abilities(Default::default())
    .unwrap()
    .with_timed_combat(TimedCombat::ARENA)
    .unwrap();
    Arc::new(content)
}

/// A settled Warden at the origin with full Rage.
fn warden(content: &Arc<ZoneContent>) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(content)).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 0, sex: 0 })
        .unwrap();
    for _ in 0..20 {
        zone.advance_tick().unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.abilities.resource = 100;
    ZoneSimulation::from_snapshot(state, Arc::clone(content)).unwrap()
}

/// One middle centre slash to the south, run to its end, optionally restored mid-sweep:
/// the events it produced and the final canonical state.
fn slash(restore: bool) -> (Vec<ZoneEvent>, mmorpg_core::CanonicalZoneSnapshot) {
    let content = content();
    let mut zone = warden(&content);
    zone.apply_command(
        1,
        2,
        ZoneCommand::DirectedStrike {
            ability: ids::HEROIC_STRIKE.get(),
            facing: SOUTH,
            aim: StrikeAim::new(StrikeSide::Centre, StrikeHeight::Middle),
        },
    )
    .unwrap();
    let timed = zone.timed_combat().unwrap();
    let mut events = Vec::new();
    for tick in 0..(timed.startup_ticks() + timed.active_ticks() + 2) {
        if restore && tick == timed.startup_ticks() + timed.active_ticks() / 2 {
            zone = ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(&content))
                .unwrap();
        }
        zone.advance_tick().unwrap();
        events.extend(zone.snapshot_for_player(1).unwrap().events);
    }
    (events, zone.snapshot().unwrap())
}

fn hits_on(events: &[ZoneEvent], creature: u32) -> usize {
    let target = EntityRef::Creature(CreatureId::new(creature));
    events
        .iter()
        .filter(|event| {
            matches!(event, ZoneEvent::DamageDealt { target: hit, amount, .. }
                if *hit == target && *amount > 0)
        })
        .count()
}

#[test]
fn one_slash_cleaves_two_wolves_and_spares_the_excluded_units() {
    let (events, state) = slash(false);
    assert_eq!(hits_on(&events, 1), 1, "the near wolf ahead is struck once");
    assert_eq!(
        hits_on(&events, 2),
        1,
        "the second wolf in the arc is struck once"
    );
    assert_eq!(
        hits_on(&events, 3),
        0,
        "the wolf off to the side is out of the arc"
    );
    assert_eq!(
        hits_on(&events, 4),
        0,
        "the middle slash passes over the rat"
    );
    assert_eq!(hits_on(&events, 5), 0, "the pillar covers the far wolf");
    // Each struck wolf reports its own sweep, so both clients attribute both hits.
    let swept: Vec<_> = events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::StrikeSwept {
                source: EntityRef::Player(1),
                target,
                ..
            } => Some(target),
            _ => None,
        })
        .collect();
    assert_eq!(swept.len(), 2);
    for wolf in [1, 2] {
        assert!(swept.contains(&Some(EntityRef::Creature(CreatureId::new(wolf)))));
    }
    // The same input replays identically, also through a mid-sweep restore.
    assert_eq!(slash(false), (events.clone(), state.clone()));
    assert_eq!(slash(true), (events, state));
}
