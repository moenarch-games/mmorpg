//! Circling against a group (#949, Atlas 5.2.i949): a level-8 Rogue Gouges the boar blocking the
//! way north while two aggressive wolves engage it. Strafing round a wolf inside its reach still
//! opens its flank once, but a flanked creature then guards against its flanker, so circling the
//! same wolf every global cooldown no longer buys a flank each time; switching to the other wolf
//! does, and from the front every strike is ordinary. The fight replays exactly.
use crate::support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::{
    CanonicalZoneSnapshot, CreatureFamily, CreatureId, EntityRef, GLOBAL_COOLDOWN_TICKS, SpawnGrid,
    StaticCollider, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneEvent, ZoneId,
    ZoneSimulation,
};
use support::arena::{self, BOAR, NORTHWARD, WOLF};

const ROGUE: u8 = 4;
const SOUTHWARD: u16 = 32_768;
const BLOCKER: u32 = 1;
const WEST_WOLF: u32 = 2;
const EAST_WOLF: u32 = 3;
/// How long the Rogue circles for one strike before striking anyway: one global cooldown.
const PATIENCE: u16 = GLOBAL_COOLDOWN_TICKS;

fn creature(id: u32) -> EntityRef {
    EntityRef::Creature(CreatureId::new(id))
}

/// Flat ground, a neutral boar blocking the way north and two aggressive wolves 1.5 m west and
/// east of the Rogue, facing it.
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
        (WEST_WOLF, WOLF, [-150, 0], 16_384),
        (EAST_WOLF, WOLF, [150, 0], 49_152),
    ]
    .into_iter()
    .map(|(id, template, at, facing)| mmorpg_core::CreatureSpawn {
        facing,
        ..arena::spawn(id, template, at)
    })
    .collect();
    let wolf = arena::wolf(50_000, [1, 1]);
    let boar = mmorpg_core::CreatureTemplate {
        id: BOAR,
        name: "Test Boar".into(),
        family: CreatureFamily::Boar,
        behaviour: mmorpg_core::CreatureBehaviour::Neutral,
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

/// One Sinister Strike: its target, damage, whether it flanked and how many ticks the Rogue
/// circled for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Strike {
    target: u32,
    damage: u32,
    flanked: bool,
    circled: u16,
}

#[derive(Debug, PartialEq)]
struct Fight {
    strikes: Vec<Strike>,
    state: CanonicalZoneSnapshot,
}

/// Whether the Rogue stands on `target`'s flank now.
fn on_flank(zone: &ZoneSimulation, target: u32) -> bool {
    let state = zone.snapshot().unwrap();
    let record = state
        .creatures
        .iter()
        .find(|record| record.creature_id == CreatureId::new(target))
        .unwrap();
    let toward = arena::aim_at(zone, 1, creature(target)).unwrap().0;
    mmorpg_core::flanks(record.facing, toward.wrapping_add(SOUTHWARD))
}

/// The Rogue Gouges the blocker, then strikes each of `plan`'s wolves in turn: circling it
/// (strafing while aiming at it) until it stands on its flank, for at most [`PATIENCE`], when
/// `circle`, else from where it stands.
fn fight(plan: &[(u32, i8)], circle: bool) -> Fight {
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
    state.players[0].position = [0.0, 90.0, 0.0];
    state.players[0].combat.abilities.resource = 100;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
    let mut sequence = 1;
    let mut send = |zone: &mut ZoneSimulation, command: ZoneCommand| {
        sequence += 1;
        zone.apply_command(1, sequence, command).unwrap();
    };
    let mut strikes = Vec::new();
    send(&mut zone, arena::stand(NORTHWARD));
    zone.advance_tick().unwrap();
    send(
        &mut zone,
        ZoneCommand::UseAbility {
            ability: ids::GOUGE.get(),
            target: None,
            facing: Some(NORTHWARD),
        },
    );
    for &(target, strafe) in plan {
        let wolf = creature(target);
        // Wait out the global cooldown facing the wolf, then circle if asked.
        for _ in 0..GLOBAL_COOLDOWN_TICKS {
            let facing = arena::aim_at(&zone, 1, wolf).unwrap().0;
            send(&mut zone, arena::stand(facing));
            zone.advance_tick().unwrap();
        }
        let mut circled = 0;
        while circle && circled < PATIENCE && !on_flank(&zone, target) {
            let (facing, distance) = arena::aim_at(&zone, 1, wolf).unwrap();
            // Keep to about 1.5 m from it, inside its reach.
            let forward = match distance.isqrt() {
                ..=130 => -1,
                131..=170 => 0,
                _ => 1,
            };
            send(
                &mut zone,
                ZoneCommand::Move {
                    forward,
                    strafe,
                    facing,
                },
            );
            zone.advance_tick().unwrap();
            circled += 1;
        }
        // A full energy bar for every strike, so only the flank decides.
        let mut state = zone.snapshot().unwrap();
        state.players[0].combat.abilities.resource = 100;
        zone = ZoneSimulation::from_snapshot(state, Arc::clone(&content)).unwrap();
        let facing = arena::aim_at(&zone, 1, wolf).unwrap().0;
        send(&mut zone, arena::stand(facing));
        send(
            &mut zone,
            ZoneCommand::UseAbility {
                ability: ids::SINISTER_STRIKE.get(),
                target: Some(wolf),
                facing: Some(facing),
            },
        );
        let mut events = Vec::new();
        for _ in 0..2 {
            zone.advance_tick().unwrap();
            events.extend(zone.snapshot_for_player(1).unwrap().events);
        }
        let damage = events
            .iter()
            .find_map(|event| match *event {
                ZoneEvent::DamageDealt {
                    source: EntityRef::Player(1),
                    target,
                    amount,
                    ..
                } if target == wolf => Some(u32::from(amount)),
                _ => None,
            })
            .unwrap_or(0);
        let flanked = events
            .iter()
            .any(|event| matches!(*event, ZoneEvent::Flanked { target, .. } if target == wolf));
        strikes.push(Strike {
            target,
            damage,
            flanked,
            circled,
        });
    }
    Fight {
        strikes,
        state: zone.snapshot().unwrap(),
    }
}

#[test]
fn circling_one_wolf_is_contested_but_switching_targets_opens_a_flank() {
    // From where it stands, every strike on the west wolf is ordinary.
    let still = fight(&[(WEST_WOLF, 1); 4], false);
    assert!(
        still
            .strikes
            .iter()
            .all(|strike| !strike.flanked && strike.damage > 0),
        "{:?}",
        still.strikes
    );

    // Circling it opens its flank about as fast as before (#978: 32 ticks): a 150% strike. Then
    // it guards its flank, so the next circle inside one global cooldown fails and that strike
    // is ordinary; only after the wariness has passed does circling flank it again. Before,
    // every one of these strikes flanked.
    let spam = fight(&[(WEST_WOLF, 1); 6], true);
    eprintln!("circling the west wolf: {:?}", spam.strikes);
    let flags: Vec<_> = spam.strikes.iter().map(|strike| strike.flanked).collect();
    assert_eq!(
        flags,
        [true, false, true, false, true, false],
        "{:?}",
        spam.strikes
    );
    let first = spam.strikes[0];
    assert!(first.circled <= 40, "{first:?}");
    assert!(spam.strikes.iter().all(|strike| strike.damage > 0));
    let ordinary = still
        .strikes
        .iter()
        .map(|strike| strike.damage)
        .max()
        .unwrap();
    assert!(first.damage * 100 >= ordinary * 125, "{first:?} {ordinary}");

    // Target selection still pays: right after flanking the west wolf, circling the east wolf,
    // which is not wary, flanks it inside one global cooldown.
    let switch = fight(&[(WEST_WOLF, 1), (EAST_WOLF, -1)], true);
    eprintln!("switching to the east wolf: {:?}", switch.strikes);
    assert!(
        switch.strikes.iter().all(|strike| strike.flanked),
        "{:?}",
        switch.strikes
    );

    // Exact replay.
    assert_eq!(fight(&[(WEST_WOLF, 1); 6], true), spam);
}
