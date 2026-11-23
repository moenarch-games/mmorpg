//! A shield-bearer's guard rewards the flank (#953): the same Warden swing is blocked from the
//! front of a shielded creature and lands from its side or rear, and the creature then turns
//! its guard toward the swinger at the bounded engaged turn rate (#949), so the flank stays
//! open for a moment before the swings are blocked again. The fight replays exactly.
use crate::support;

use std::sync::Arc;

use mmorpg_core::{
    CREATURE_TURN_PER_TICK, CanonicalZoneSnapshot, CreatureBehaviour, CreatureId, EntityRef,
    FLANK_FRONT_HALF_ARC, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation,
};
use support::arena::{self, WOLF};

const WARDEN: u8 = 0;
const SOUTHWARD: u16 = 32_768;
const SHIELD_BEARER: CreatureId = CreatureId::new(1);
const HEALTH: u32 = 5_000;

/// Flat ground and one sturdy neutral shielded brawler at (0, 150) facing south, toward the
/// origin.
fn content() -> Arc<ZoneContent> {
    let template = mmorpg_core::CreatureTemplate {
        behaviour: CreatureBehaviour::Neutral,
        ..arena::brawler(HEALTH, [1, 1])
    };
    let spawn = mmorpg_core::CreatureSpawn {
        facing: SOUTHWARD,
        ..arena::spawn(SHIELD_BEARER.get(), WOLF, [0, 150])
    };
    Arc::new(
        ZoneContent::new(
            mmorpg_core::ZoneDefinition::with_spawn_grid(
                mmorpg_core::STANDARD_GRAVITY,
                mmorpg_core::SpawnGrid {
                    origin: [-5_000, -5_000],
                    columns: 32,
                    spacing: 100,
                },
                vec![mmorpg_core::StaticCollider {
                    id: 1,
                    position: [0, -50, 0],
                    half_extents: [10_000, 50, 10_000],
                }],
            )
            .unwrap(),
            ZoneAreas::default(),
            vec![template],
            vec![spawn],
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_creature_abilities(Vec::new())
        .unwrap()
        .with_shielded(vec![WOLF])
        .unwrap(),
    )
}

/// One swing's outcome on the shield-bearer.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    Blocked,
    Landed(u16),
}

/// What the fight showed: each swing's outcome with its tick, the shield-bearer's facing each
/// tick, and the final canonical state.
#[derive(Debug, PartialEq)]
struct Fight {
    outcomes: Vec<(u32, Outcome)>,
    facings: Vec<u16>,
    health: u32,
    state: CanonicalZoneSnapshot,
}

/// A level-1 Warden standing at `at` swings at the shield-bearer whenever it may, for 6 s.
fn fight(at: [f32; 2]) -> Fight {
    let content = content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(
        1,
        1,
        ZoneCommand::ChooseClass {
            class: WARDEN,
            sex: 0,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [at[0], 90.0, at[1]];
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
    let target = EntityRef::Creature(SHIELD_BEARER);
    let shield_bearer = |zone: &ZoneSimulation| {
        zone.snapshot()
            .unwrap()
            .creatures
            .iter()
            .find(|record| record.creature_id == SHIELD_BEARER)
            .map(|record| (record.facing, record.health))
            .unwrap()
    };
    let (mut outcomes, mut facings) = (Vec::new(), Vec::new());
    let mut sequence = 1;
    for tick in 0..360 {
        if let Some(swing) = arena::swing_at(&zone, 1, target) {
            sequence += 1;
            zone.apply_command(1, sequence, swing).unwrap();
        }
        zone.advance_tick().unwrap();
        for event in zone.snapshot_for_player(1).unwrap().events {
            match event {
                ZoneEvent::Blocked { target: hit, .. } if hit == target => {
                    outcomes.push((tick, Outcome::Blocked));
                }
                ZoneEvent::DamageDealt {
                    target: hit,
                    amount,
                    ..
                } if hit == target => outcomes.push((tick, Outcome::Landed(amount))),
                _ => {}
            }
        }
        facings.push(shield_bearer(&zone).0);
    }
    Fight {
        outcomes,
        facings,
        health: shield_bearer(&zone).1,
        state: zone.snapshot().unwrap(),
    }
}

/// The unsigned angle between two yaws.
fn gap(a: u16, b: u16) -> u16 {
    a.wrapping_sub(b).min(b.wrapping_sub(a))
}

/// Ticks after `tick` until the shield-bearer's front covers the player at `bearing` again.
fn covered_after(fight: &Fight, tick: u32, bearing: u16) -> usize {
    fight.facings[tick as usize..]
        .iter()
        .position(|&facing| gap(facing, bearing) <= FLANK_FRONT_HALF_ARC)
        .expect("the guard turns toward the swinger")
}

#[test]
fn the_shield_blocks_the_front_and_turns_late_toward_a_flank() {
    // From the front every swing is blocked and the shield-bearer loses nothing.
    let front = fight([0.0, 0.0]);
    assert!(front.outcomes.len() >= 2, "{:?}", front.outcomes);
    assert!(
        front
            .outcomes
            .iter()
            .all(|&(_, outcome)| outcome == Outcome::Blocked),
        "{:?}",
        front.outcomes
    );
    assert_eq!(front.health, HEALTH);

    // The same swing from the side (east, 90° off its facing) and the rear (north) lands.
    for (at, bearing) in [([150.0, 150.0], 16_384), ([0.0, 300.0], 0)] {
        let flank = fight(at);
        let Some(&(first, Outcome::Landed(_))) = flank.outcomes.first() else {
            panic!("the first swing from {at:?} lands: {:?}", flank.outcomes);
        };
        assert!(flank.health < HEALTH);
        // The guard turns at most the engaged turn rate a tick …
        assert!(
            flank
                .facings
                .windows(2)
                .all(|pair| gap(pair[0], pair[1]) <= CREATURE_TURN_PER_TICK),
            "{at:?}"
        );
        // … so the flank stays open while it turns (30° and 120° at 120°/s: 0.25 s and 1 s) …
        let open = covered_after(&flank, first, bearing);
        let least =
            usize::from((gap(SOUTHWARD, bearing) - FLANK_FRONT_HALF_ARC) / CREATURE_TURN_PER_TICK);
        assert!(
            open + 1 >= least,
            "{at:?}: open {open} ticks, at least {least}"
        );
        // … and once it faces the swinger, the swings are blocked again.
        assert!(
            flank
                .outcomes
                .iter()
                .any(|&(tick, outcome)| outcome == Outcome::Blocked
                    && tick as usize > first as usize + open),
            "{at:?}: {:?}",
            flank.outcomes
        );
    }

    // Exact replay.
    assert_eq!(fight([0.0, 300.0]), fight([0.0, 300.0]));
}
