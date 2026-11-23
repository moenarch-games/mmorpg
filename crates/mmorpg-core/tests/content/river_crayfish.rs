//! The River Crayfish (#996): it lies on the Fenwater's low bank, off the roads and out of the
//! channel, and stays there. Its shell and claws turn a Warden's swing from its front while the
//! same swing from behind lands. It raises a claw and holds still to its pinch on `Attack_A`'s
//! contact, holds the pinch through the clip's rest, and the same zone replays it exactly,
//! restored mid-raise too.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, area_at, crayfish, design, road_distance, units, wilds};
use mmorpg_core::physics::to_units;
use mmorpg_core::{
    CRAYFISH_CONTACT_TICKS, CRAYFISH_RECOVER_TICKS, CreatureAi, CreatureAttack, CreatureFamily,
    CreatureId, CreatureSpawn, EntityRef, ZoneAreas, ZoneCommand, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation, contact_ticks,
};
use support::arena::{self, NORTHWARD};

const CRAYFISH: CreatureId = CreatureId::new(3_160);
const WARDEN: u8 = 0;
/// Yaw 32 768 faces -Z, toward the origin.
const TOWARD_ORIGIN: u16 = 32_768;

/// The real River Crayfish template, shell and all, on flat ground at `position` facing south
/// (toward the origin).
fn content(position: [i32; 2]) -> Arc<ZoneContent> {
    let template = greyhaven_vale::content()
        .creature_template(units::templates::RIVER_CRAYFISH)
        .unwrap()
        .clone();
    Arc::new(
        ZoneContent::new(
            arena::ground(),
            ZoneAreas::default(),
            vec![template],
            vec![CreatureSpawn {
                levels: Some([3, 3]),
                facing: TOWARD_ORIGIN,
                ..arena::spawn(CRAYFISH.get(), units::templates::RIVER_CRAYFISH, position)
            }],
            Vec::new(),
            arena::GRAVEYARD,
        )
        .unwrap()
        .with_shielded(vec![units::templates::RIVER_CRAYFISH])
        .unwrap(),
    )
}

fn crayfish_state(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == CRAYFISH)
        .unwrap()
}

#[test]
fn river_crayfish_lie_on_the_fenwater_bank_and_stay_there() {
    let content = greyhaven_vale::content();
    let spawns: Vec<_> = content
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == units::templates::RIVER_CRAYFISH)
        .cloned()
        .collect();
    assert_eq!(spawns.len(), 3);
    for spawn in &spawns {
        let [x, z] = spawn.position;
        let id = spawn.id.get();
        assert!(crayfish::SPAWN_IDS.contains(&id));
        // The Fenwater Crossing's bank, or Fenford's by the ford (#1063).
        let area = area_at(x, z).unwrap().id();
        assert!(area == wilds::FENWATER_CROSSING || area == greyhaven_vale::settlements::FENFORD);
        assert_eq!(road_distance(x, z), i64::MAX, "spawn {id} is off the road");
        // On the bank's low margin, out of the channel and within 2 m of its edge.
        let margin = design::river_distance(x, z);
        assert!((50..=200).contains(&margin), "spawn {id}: {margin} units");
    }
    let template = content
        .creature_template(units::templates::RIVER_CRAYFISH)
        .unwrap();
    assert_eq!(template.family, CreatureFamily::Crayfish);
    assert!(template.family.ambushes() && !template.family.swims());
    assert!(
        content.is_shielded(template.id),
        "its shell guards its front"
    );
    assert!(
        mmorpg_core::loot_table(template.id).is_some(),
        "it drops loot"
    );

    // In the real zone each settles on the bank and keeps to its spot, out of the water.
    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), Arc::clone(&content)).unwrap();
    for _ in 0..mmorpg_core::golden::t30(300) {
        zone.advance_tick().unwrap();
    }
    let state = zone.snapshot().unwrap();
    for spawn in &spawns {
        let record = state
            .creatures
            .iter()
            .find(|creature| creature.creature_id == spawn.id)
            .unwrap();
        let [x, _, z] = to_units(record.position);
        let [sx, sz] = spawn.position;
        assert!(
            (x - sx).abs() <= 150 && (z - sz).abs() <= 150,
            "crayfish {} wandered to {x}, {z}",
            spawn.id.get()
        );
        assert!(
            design::river_distance(x, z) > 0,
            "it stays out of the channel"
        );
    }
}

/// One swing's outcome on the crayfish.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    Blocked,
    Landed,
}

/// A level-1 Warden standing at `at` (XZ units) swings at the crayfish at (0, 20 m) whenever
/// they may, for 4 s: each swing's outcome and the crayfish's health after.
fn swings_from(at: [f32; 2]) -> (Vec<Outcome>, u32, mmorpg_core::CanonicalZoneSnapshot) {
    let content = content([0, 2_000]);
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
    let target = EntityRef::Creature(CRAYFISH);
    let (mut outcomes, mut sequence) = (Vec::new(), 1);
    for _ in 0..mmorpg_core::golden::t30(120) {
        if let Some(swing) = arena::swing_at(&zone, 1, target) {
            sequence += 1;
            zone.apply_command(1, sequence, swing).unwrap();
        }
        zone.advance_tick().unwrap();
        for event in zone.snapshot_for_player(1).unwrap().events {
            match event {
                ZoneEvent::Blocked { target: hit, .. } if hit == target => {
                    outcomes.push(Outcome::Blocked);
                }
                ZoneEvent::DamageDealt { target: hit, .. } if hit == target => {
                    outcomes.push(Outcome::Landed);
                }
                _ => {}
            }
        }
    }
    let health = crayfish_state(&zone).health;
    (outcomes, health, zone.snapshot().unwrap())
}

#[test]
fn its_front_turns_a_swing_that_lands_from_behind() {
    let full =
        crayfish_state(&ZoneSimulation::with_content(ZoneId::new(9), content([0, 2_000])).unwrap())
            .health;
    // Head-on, at its claws, every swing is turned and it loses nothing.
    let (front, health, _) = swings_from([0.0, 1_850.0]);
    assert!(front.len() >= 2, "{front:?}");
    assert!(
        front.iter().all(|&outcome| outcome == Outcome::Blocked),
        "{front:?}"
    );
    assert_eq!(health, full);
    // The same swing from behind its tail lands before it can come round.
    let (rear, health, state) = swings_from([0.0, 2_150.0]);
    assert_eq!(rear.first(), Some(&Outcome::Landed), "{rear:?}");
    assert!(health < full);
    // Exact replay.
    assert_eq!(swings_from([0.0, 2_150.0]).2, state);
}

/// One tick of the fight: the crayfish's XZ feet and what the player saw.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Tick {
    crayfish: [i32; 2],
    began: Option<(CreatureAttack, u16)>,
    hit: Option<(u16, bool)>,
    missed: bool,
}

fn observe(zone: &mut ZoneSimulation) -> Tick {
    let [x, _, z] = to_units(crayfish_state(zone).position);
    let source = EntityRef::Creature(CRAYFISH);
    let mut tick = Tick {
        crayfish: [x, z],
        began: None,
        hit: None,
        missed: false,
    };
    for event in &zone.snapshot_for_player(1).unwrap().events {
        match *event {
            ZoneEvent::CreatureAttackBegan {
                source: from,
                attack,
                contact,
            } if from == source => tick.began = Some((attack, contact)),
            ZoneEvent::DamageTaken {
                source: from,
                amount,
                critical,
                ..
            } if from == source => tick.hit = Some((amount, critical)),
            ZoneEvent::Miss { source: from, .. } if from == source => tick.missed = true,
            _ => {}
        }
    }
    tick
}

/// A zone with the crayfish 20 m north of a player walking up to it, who stands once it
/// engages; it runs until `stop` says so.
fn fight(
    mut stop: impl FnMut(&mut ZoneSimulation, &[Tick]) -> bool,
) -> (ZoneSimulation, Vec<Tick>) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content([0, 2_000])).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let mut seen = Vec::new();
    for _ in 0..10_000 {
        if !engaged && crayfish_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        if engaged {
            seen.push(observe(&mut zone));
            if stop(&mut zone, &seen) {
                return (zone, seen);
            }
        }
    }
    panic!("the crayfish never engaged");
}

fn ticks(length: usize) -> Vec<Tick> {
    fight(|_, seen| seen.len() == length).1
}

#[test]
fn it_raises_a_claw_pinches_on_contact_and_holds_the_pinch() {
    let contact = contact_ticks(CreatureFamily::Crayfish, CreatureAttack::Quick).unwrap();
    // `Attack_A`'s contact, 0.75 s in, then the rest of its 1.25 s.
    assert_eq!(contact, CRAYFISH_CONTACT_TICKS);
    assert_eq!(contact, mmorpg_core::ticks_from_ms(750));
    assert_eq!(CRAYFISH_RECOVER_TICKS, mmorpg_core::ticks_from_ms(500));
    let [low, high] = content([0, 2_000])
        .creature_template(units::templates::RIVER_CRAYFISH)
        .unwrap()
        .damage_at(3);

    let seen = ticks(mmorpg_core::golden::t30(600) as usize);
    let (mut pinches, mut hits) = (0, 0);
    for (start, tick) in seen.iter().enumerate() {
        let Some(began) = tick.began else {
            continue;
        };
        assert_eq!(began, (CreatureAttack::Quick, contact), "only its pinch");
        let land = start + usize::from(contact);
        let Some(landed) = seen.get(land) else {
            break;
        };
        // Still through the claw raise up to its contact.
        for between in &seen[start + 1..land] {
            assert_eq!(
                between.crayfish, tick.crayfish,
                "it holds still as it warns"
            );
            assert!(between.hit.is_none() && !between.missed);
        }
        assert!(
            landed.hit.is_some() || landed.missed,
            "it pinches on its contact"
        );
        if let Some((amount, critical)) = landed.hit {
            let ceiling = if critical { high * 2 } else { high };
            assert!((low..=ceiling).contains(&amount), "a pinch deals {amount}");
            hits += 1;
        }
        // It holds the committed pinch.
        if let Some(window) = seen.get(land + 1..land + usize::from(CRAYFISH_RECOVER_TICKS)) {
            for still in window {
                assert_eq!(
                    still.crayfish, landed.crayfish,
                    "it holds its pinch in place"
                );
            }
        }
        pinches += 1;
    }
    assert!(pinches >= 4 && hits >= 1, "{pinches} pinches, {hits} hits");
}

#[test]
fn a_crayfish_fight_replays_exactly_and_restores_mid_raise() {
    let length = mmorpg_core::golden::t30(300) as usize;
    assert_eq!(ticks(length), ticks(length));

    let (mut zone, _) = fight(|zone, _| {
        crayfish_state(zone)
            .attack
            .is_some_and(|attack| attack.contact == CRAYFISH_CONTACT_TICKS / 2)
    });
    let mut restored =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), content([0, 2_000])).unwrap();
    let mut landed = false;
    for _ in 0..mmorpg_core::golden::t30(120) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        let seen = observe(&mut zone);
        landed |= seen.hit.is_some() || seen.missed;
        assert_eq!(seen, observe(&mut restored));
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(landed, "the pending pinch landed after the restore");
}
