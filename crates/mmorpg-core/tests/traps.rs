//! World traps (#300): pressure-plate rocks, swinging blades and spike pits hit units on their
//! schedule, reset on theirs, and carry their state through a snapshot.
mod support;

use std::sync::Arc;

use mmorpg_core::ZoneCommand;
use mmorpg_core::greyhaven_vale::traps::{CAMP_SPIKES, ROCKFALL_TRAP};
use mmorpg_core::{
    Blade, CreatureId, CreatureSpawn, CreatureTemplate, EntityRef, RockDrop, Rockfall, SpawnGrid,
    StaticCollider, Trap, TrapEffect, TrapId, TrapTrigger, TriggerVolume, ZoneContent,
    ZoneDefinition, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, BOAR};

const PLATE: TrapId = TrapId::new(1);
const BLADE: TrapId = TrapId::new(2);
const SPIKES: TrapId = TrapId::new(3);
const BOAR_SPIKES: TrapId = TrapId::new(4);

const PLATE_AT: [i32; 2] = [0, -1_000];
const BLADE_AT: [i32; 2] = [1_500, -1_000];
const SPIKES_AT: [i32; 2] = [-1_500, -1_000];
const BOAR_AT: [i32; 2] = [-3_000, -1_000];

const REARM: u32 = 300;
const ROCK_DAMAGE: u16 = 26;
const BLADE_DAMAGE: u16 = 11;
const SPIKE_DAMAGE: u16 = 7;
const SPIKE_REHIT: u16 = 22;

fn blade() -> Blade {
    Blade {
        pivot: [BLADE_AT[0], 400, BLADE_AT[1]],
        arm: 280,
        half_extents: [20, 60, 70],
        swing_yaw: 0,
        amplitude: 7_282,
        period: 90,
        phase: 0,
    }
}

fn traps() -> Vec<Trap> {
    let volume = |at: [i32; 2]| TriggerVolume {
        centre: at,
        half: [100, 100],
        floor: 0,
        height: 40,
    };
    vec![
        Trap {
            id: PLATE,
            trigger: TrapTrigger::Pressure {
                plate: TriggerVolume {
                    centre: PLATE_AT,
                    half: [70, 70],
                    floor: 0,
                    height: 25,
                },
                rearm_ticks: REARM,
            },
            effect: TrapEffect::Rockfall(Rockfall {
                rocks: vec![
                    RockDrop {
                        at: PLATE_AT,
                        delay: 0,
                    },
                    RockDrop {
                        at: [PLATE_AT[0] - 40, PLATE_AT[1]],
                        delay: 5,
                    },
                ],
                start_y: 400,
                floor: 0,
                radius: 25,
                rubble_ticks: 40,
            }),
            damage: ROCK_DAMAGE,
            rehit_ticks: 299,
        },
        Trap {
            id: BLADE,
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Blade(blade()),
            damage: BLADE_DAMAGE,
            rehit_ticks: 30,
        },
        Trap {
            id: SPIKES,
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Spikes(volume(SPIKES_AT)),
            damage: SPIKE_DAMAGE,
            rehit_ticks: SPIKE_REHIT,
        },
        Trap {
            id: BOAR_SPIKES,
            trigger: TrapTrigger::Live,
            effect: TrapEffect::Spikes(volume(BOAR_AT)),
            damage: SPIKE_DAMAGE,
            rehit_ticks: SPIKE_REHIT,
        },
    ]
}

/// Flat ground with the test traps and a boar in the second spike pit.
fn content() -> Arc<ZoneContent> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [0, 0],
            columns: 8,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [10_000, 50, 10_000],
        }],
    )
    .unwrap();
    let spawns: Vec<CreatureSpawn> = vec![arena::spawn(1, BOAR, BOAR_AT)];
    let content = ZoneContent::new(
        definition,
        mmorpg_core::ZoneAreas::default(),
        vec![CreatureTemplate {
            health: 200,
            ..arena::boar()
        }],
        spawns,
        Vec::new(),
        arena::GRAVEYARD,
    )
    .unwrap();
    Arc::new(
        content
            .with_creature_abilities(Vec::new())
            .unwrap()
            .with_traps(traps())
            .unwrap(),
    )
}

/// A zone with one player standing at `at` on the flat ground.
fn zone_with_player_at(at: [i32; 2]) -> ZoneSimulation {
    let content = content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [at[0] as f32, 90.0, at[1] as f32];
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// Ticks `zone` `count` times, collecting this player's trap hits as `(tick, trap, amount)`.
fn collect_hits(zone: &mut ZoneSimulation, count: u32, player: u32) -> Vec<(u64, TrapId, u16)> {
    let mut hits = Vec::new();
    for _ in 0..count {
        zone.advance_tick().unwrap();
        let tick = zone.snapshot().unwrap().tick;
        for event in zone.snapshot_for_player(player).unwrap().events {
            if let ZoneEvent::TrapHit {
                trap,
                target: EntityRef::Player(id),
                amount,
            } = event
                && id == player
            {
                hits.push((tick, trap, amount));
            }
        }
    }
    hits
}

fn health(zone: &ZoneSimulation) -> u32 {
    zone.snapshot().unwrap().players[0].combat.health
}

#[test]
fn a_plate_fires_rocks_that_hit_the_unit_on_it_once_per_fall() {
    let mut zone = zone_with_player_at(PLATE_AT);
    let start = health(&zone);
    let hits = collect_hits(&mut zone, 200, 1);
    assert_eq!(
        hits.len(),
        1,
        "one fall, one hit, though two rocks fell: {hits:?}"
    );
    assert_eq!(hits[0].1, PLATE);
    assert_eq!(
        health(&zone),
        start - u32::from(ROCK_DAMAGE),
        "the rock hurt it"
    );
    // The plate shows its fall to the viewer.
    let view = zone.snapshot_for_player(1).unwrap();
    assert_eq!(view.traps.len(), 1);
    assert_eq!(view.traps[0].id, PLATE);
    assert!(view.traps[0].age > 0);
}

#[test]
fn a_plate_rearms_and_a_unit_clear_of_it_never_fires_it() {
    // Standing on the plate: a second fall after the re-arm.
    let mut zone = zone_with_player_at(PLATE_AT);
    let hits = collect_hits(&mut zone, 200 + REARM, 1);
    assert_eq!(hits.len(), 2, "a second fall after re-arming: {hits:?}");
    assert!(hits[1].0 >= hits[0].0 + u64::from(REARM));

    // Beside it: nothing fires.
    let mut zone = zone_with_player_at([PLATE_AT[0] + 400, PLATE_AT[1]]);
    assert!(collect_hits(&mut zone, 120, 1).is_empty());
    assert!(zone.snapshot_for_player(1).unwrap().traps.is_empty());
}

#[test]
fn a_blade_hits_only_when_its_head_passes_the_unit() {
    let mut zone = zone_with_player_at(BLADE_AT);
    let hits = collect_hits(&mut zone, 180, 1);
    assert!(hits.len() >= 2, "swinging through the unit: {hits:?}");
    let schedule = blade();
    for &(tick, trap, amount) in &hits {
        assert_eq!(trap, BLADE);
        assert_eq!(amount, BLADE_DAMAGE);
        // The head reaches the unit only near the middle of its swing, never out at the ends.
        assert!(
            schedule.angle_at(tick).abs() < 3_000,
            "tick {tick} angle {}",
            schedule.angle_at(tick)
        );
    }
    // Spaced apart by the re-hit window.
    assert!(hits[1].0 - hits[0].0 >= 30);
}

#[test]
fn spikes_hit_a_player_again_and_again_and_a_creature_too() {
    let mut zone = zone_with_player_at(SPIKES_AT);
    let hits = collect_hits(&mut zone, 80, 1);
    assert!(hits.len() >= 3, "repeated spikes: {hits:?}");
    assert!(
        hits.windows(2)
            .all(|pair| pair[1].0 - pair[0].0 >= u64::from(SPIKE_REHIT))
    );
    assert!(
        hits.iter()
            .all(|&(_, trap, amount)| trap == SPIKES && amount == SPIKE_DAMAGE)
    );
    // A boar stands in the other pit from its spawn: the spikes hurt it too, and nobody credited.
    let boar_health = zone.snapshot().unwrap().creatures[0].health;
    assert!(boar_health < 200, "the boar was hit: {boar_health}");
}

#[test]
fn a_trap_snapshot_continues_exactly_and_never_hits_twice() {
    // Advance until the first rock has hit, take a snapshot while that re-hit memory runs and
    // other rocks of the fall are still dropping, and continue both the original and the
    // restored zone.
    let mut original = zone_with_player_at(PLATE_AT);
    let mut first = Vec::new();
    while first.is_empty() {
        first = collect_hits(&mut original, 1, 1);
    }
    let snapshot = original.snapshot().unwrap();
    assert!(
        snapshot
            .traps
            .iter()
            .any(|trap| trap.id == PLATE && !trap.hits.is_empty()),
        "the snapshot remembers the hit"
    );
    let restored_traps = snapshot
        .traps
        .iter()
        .find(|trap| trap.id == PLATE)
        .unwrap()
        .clone();
    assert!(restored_traps.fired_at.is_some(), "fired by tick 10");
    let mut restored =
        ZoneSimulation::from_snapshot(snapshot, Arc::clone(original.content())).unwrap();
    let a = collect_hits(&mut original, 150, 1);
    let b = collect_hits(&mut restored, 150, 1);
    assert_eq!(a, b, "the same hits after the restore");
    assert!(a.is_empty(), "no second hit from this fall: {a:?}");
    assert_eq!(original.snapshot().unwrap(), restored.snapshot().unwrap());
}

#[test]
fn snapshots_reject_trap_state_that_does_not_match_content() {
    let zone = zone_with_player_at(PLATE_AT);
    let content = Arc::clone(zone.content());
    let mut bad = zone.snapshot().unwrap();
    bad.traps.pop();
    assert!(ZoneSimulation::from_snapshot(bad, Arc::clone(&content)).is_err());
    let mut bad = zone.snapshot().unwrap();
    bad.traps[0].hits = vec![
        mmorpg_core::CanonicalTrapHit {
            unit: EntityRef::Creature(CreatureId::new(2)),
            until: 10,
        },
        mmorpg_core::CanonicalTrapHit {
            unit: EntityRef::Creature(CreatureId::new(1)),
            until: 10,
        },
    ];
    assert!(ZoneSimulation::from_snapshot(bad, content).is_err());
}

#[test]
fn the_vale_authors_visible_traps_that_validate() {
    let content = mmorpg_core::greyhaven_vale::content();
    assert_eq!(content.traps().len(), 4);
    assert!(
        content
            .traps()
            .iter()
            .any(|trap| matches!(trap.trigger, TrapTrigger::Pressure { .. }))
    );
}

/// The Vale with one player standing with feet at `at` on the camp or cave's floor.
fn vale_player_at(at: [i32; 2]) -> ZoneSimulation {
    let content = mmorpg_core::greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [at[0] as f32, 1_100.0 + 90.0, at[1] as f32];
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

#[test]
fn a_player_can_walk_into_the_camp_spike_pit_and_it_hurts() {
    let mut zone = vale_player_at([-1_400, -8_600]);
    let start = health(&zone);
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 49_152,
        },
    )
    .unwrap();
    let hits = collect_hits(&mut zone, 90, 1);
    assert!(
        hits.iter().any(|&(_, trap, _)| trap == CAMP_SPIKES),
        "the camp spikes hit the walker: {hits:?}"
    );
    assert!(health(&zone) < start);
}

#[test]
fn the_cave_plate_drops_rocks_on_the_vale_floor() {
    let mut zone = vale_player_at([5_950, -14_150]);
    let hits = collect_hits(&mut zone, 90, 1);
    assert_eq!(hits.len(), 1, "one hit from the fall: {hits:?}");
    assert_eq!(hits[0].1, ROCKFALL_TRAP);
}
