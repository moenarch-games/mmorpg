//! The Cave Bat (#995): two bats roost on the Greyfang cave's stalactite and hold there. A
//! player who walks under one is its target: it flies over the player's head, out of a sword's
//! reach, then dives on its clip's contact, hangs low through its recovery, where a sword reaches
//! it, and climbs back. Stepping away through the wind-up dodges the dive, running off escapes
//! it, and the same zone replays a fight exactly, restored mid-dive too.
use crate::support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, bat, cave, units};
use mmorpg_core::physics::to_units;
use mmorpg_core::{
    BAT_DIVE_TICKS, BAT_RECOVER_TICKS, CreatureAi, CreatureAttack, CreatureFamily, CreatureId,
    CreatureSpawn, EntityRef, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, contact_ticks,
    flight,
};
use support::arena::{self, NORTHWARD};

/// Away from the bat, which roosts north (+Z) of the player.
const SOUTHWARD: u16 = 32_768;

const BAT: CreatureId = CreatureId::new(3_140);

/// The real Cave Bat template over flat ground, 20 m north of the player.
fn content() -> Arc<ZoneContent> {
    let greyhaven = greyhaven_vale::content();
    let template = greyhaven
        .creature_template(units::templates::CAVE_BAT)
        .unwrap()
        .clone();
    arena::arena(
        vec![template],
        vec![CreatureSpawn {
            levels: Some([3, 3]),
            wander_radius: 0,
            ..arena::spawn(BAT.get(), units::templates::CAVE_BAT, [0, 2_000])
        }],
        vec![],
    )
}

/// A zone with a level 4 player, the cave's level, at the arena's start.
fn zone() -> ZoneSimulation {
    let content = content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content.clone()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].combat.level = 4;
    state.players[0].combat.health = mmorpg_core::unit::player_max_health(4);
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

/// One tick of the fight: the bat's centre, its height over the player's centre and what the
/// player saw.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Tick {
    bat: [i32; 3],
    above: i32,
    began: Option<(CreatureAttack, u16)>,
    hit: Option<(u16, bool)>,
    missed: bool,
    /// The player's own blow landed on the bat.
    struck: bool,
    dead: bool,
}

fn bat_state(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == BAT)
        .unwrap()
}

fn player_y(zone: &ZoneSimulation) -> i32 {
    to_units(zone.snapshot().unwrap().players[0].position)[1]
}

fn observe(zone: &mut ZoneSimulation) -> Tick {
    let bat = to_units(bat_state(zone).position);
    let source = EntityRef::Creature(BAT);
    let mut tick = Tick {
        bat,
        above: bat[1] - player_y(zone),
        began: None,
        hit: None,
        missed: false,
        struck: false,
        dead: false,
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
            ZoneEvent::DamageDealt { target, .. } if target == source => tick.struck = true,
            ZoneEvent::Died { entity, .. } if entity == source => tick.dead = true,
            _ => {}
        }
    }
    tick
}

/// How the player fights once the bat engages: `Stand`s still, `BackOff` walks away from the
/// first wind-up on, `Run`s away at once, or `Counter`s, swinging only while the bat hangs low.
#[derive(Clone, Copy, PartialEq)]
enum Plan {
    Stand,
    BackOff,
    Run,
    Counter,
}

/// The player walks north under the bat until it engages, then follows `plan` for `ticks` ticks.
fn fight(ticks: usize, plan: Plan) -> (ZoneSimulation, Vec<Tick>) {
    let mut zone = zone();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut sequence = 2;
    let mut command = |zone: &mut ZoneSimulation, command| {
        zone.apply_command(1, sequence, command).unwrap();
        sequence += 1;
    };
    let mut engaged = false;
    let mut seen: Vec<Tick> = Vec::new();
    for _ in 0..10_000 {
        if !engaged && bat_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            match plan {
                Plan::Run => command(&mut zone, arena::walk(SOUTHWARD)),
                _ => command(&mut zone, arena::stand(NORTHWARD)),
            }
        }
        if engaged
            && plan == Plan::Counter
            && seen
                .last()
                .is_some_and(|tick| tick.above <= flight::BITE_HEIGHT_UNITS)
            && let Some(swing) = arena::swing_at(&zone, 1, EntityRef::Creature(BAT))
        {
            command(&mut zone, swing);
        }
        zone.advance_tick().unwrap();
        if engaged {
            let tick = observe(&mut zone);
            if plan == Plan::BackOff
                && tick.began.is_some()
                && seen.iter().all(|t| t.began.is_none())
            {
                command(&mut zone, arena::walk(SOUTHWARD));
            }
            let dead = tick.dead;
            seen.push(tick);
            if seen.len() == ticks || dead {
                return (zone, seen);
            }
        }
    }
    panic!("the bat never engaged");
}

#[test]
fn cave_bats_roost_in_the_greyfang_cave_and_hang_there() {
    let content = greyhaven_vale::content();
    let bats: Vec<_> = content
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == units::templates::CAVE_BAT)
        .collect();
    assert_eq!(bats.len(), 2, "one or two bats, never a swarm");
    let template = content
        .creature_template(units::templates::CAVE_BAT)
        .unwrap();
    assert_eq!(template.family, CreatureFamily::Bat);
    assert!(template.family.flies() && template.family.ambushes());
    assert!(
        mmorpg_core::loot_table(template.id).is_some(),
        "it drops loot"
    );
    for spawn in &bats {
        let [x, z] = spawn.position;
        assert!(bat::SPAWN_IDS.contains(&spawn.id.get()));
        assert!(
            cave::GREYFANG.flattens(x, z),
            "{} is in the cave",
            spawn.id.get()
        );
        assert!((x - bat::ROOST[0]).abs() <= 100 && (z - bat::ROOST[1]).abs() <= 100);
    }

    // In the real cave each hangs at its roost under the rock, clear of roof and floor, and
    // stays there: no gravity, no wandering.
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone()).unwrap();
    let roosts: Vec<_> = bats
        .iter()
        .map(|spawn| {
            let [x, z] = spawn.position;
            let ground = content.definition().ground_at(x, z);
            [x, ground + flight::ROOST_RISE_UNITS, z]
        })
        .collect();
    for _ in 0..mmorpg_core::golden::t30(150) {
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    for (spawn, roost) in bats.iter().zip(&roosts) {
        let state = snapshot
            .creatures
            .iter()
            .find(|creature| creature.creature_id == spawn.id)
            .unwrap();
        let at = to_units(state.position);
        for axis in 0..3 {
            assert!(
                (at[axis] - roost[axis]).abs() <= 2,
                "{} hangs at its roost {roost:?}, not {at:?}",
                spawn.id.get()
            );
        }
    }
}

#[test]
fn the_bat_flies_overhead_dives_on_contact_and_hangs_low_to_recover() {
    let contact = contact_ticks(CreatureFamily::Bat, CreatureAttack::Quick).unwrap();
    // `Dive_Windup`, then `Dive_Contact`'s contact 0.333 s in.
    assert_eq!(contact, mmorpg_core::ticks_from_ms(833));
    assert!(BAT_DIVE_TICKS < contact);
    let [low, high] = content()
        .creature_template(units::templates::CAVE_BAT)
        .unwrap()
        .damage_at(3);

    let (_, seen) = fight(mmorpg_core::golden::t30(600) as usize, Plan::Stand);
    let (mut dives, mut hits) = (0, 0);
    for (start, tick) in seen.iter().enumerate() {
        let Some(began) = tick.began else {
            continue;
        };
        assert_eq!(began, (CreatureAttack::Quick, contact), "only its dive");
        let land = start + usize::from(contact);
        let Some(landed) = seen.get(land) else {
            break;
        };
        // It hovers in place through the wind-up, over the player's head and out of a sword's
        // height band ([-1, 2] m about the player's feet, the player's centre 0.9 m up).
        let stoop = land - usize::from(BAT_DIVE_TICKS);
        for hover in &seen[start + 1..stoop] {
            assert_eq!([hover.bat[0], hover.bat[2]], [tick.bat[0], tick.bat[2]]);
            assert!(
                hover.above > 200 - 90 + 19,
                "it hovers {} over",
                hover.above
            );
            assert!(hover.hit.is_none() && !hover.missed);
        }
        // Then it stoops down and across to the player, landing at chest height.
        assert!(
            landed.above <= flight::BITE_HEIGHT_UNITS,
            "it dives to {}",
            landed.above
        );
        assert!(
            landed.hit.is_some() || landed.missed,
            "it bites on its contact"
        );
        if let Some((amount, critical)) = landed.hit {
            let ceiling = if critical { high * 2 } else { high };
            assert!((low..=ceiling).contains(&amount), "a dive deals {amount}");
            hits += 1;
        }
        // It hangs low and still through its recovery, within reach.
        if let Some(window) = seen.get(land + 1..land + usize::from(BAT_RECOVER_TICKS)) {
            for still in window {
                assert_eq!([still.bat[0], still.bat[2]], [landed.bat[0], landed.bat[2]]);
                assert!(still.above <= flight::BITE_HEIGHT_UNITS, "it recovers low");
            }
        }
        // And climbs back out of reach before its next dive.
        let after = land + usize::from(BAT_RECOVER_TICKS) + mmorpg_core::golden::t30(15) as usize;
        if let Some(climbed) = seen.get(after) {
            assert!(
                climbed.above > 120,
                "it climbs back, {} over",
                climbed.above
            );
        }
        dives += 1;
    }
    assert!(dives >= 4 && hits >= 1, "{dives} dives, {hits} hits");
}

#[test]
fn stepping_away_through_the_wind_up_dodges_the_dive() {
    let contact = usize::from(contact_ticks(CreatureFamily::Bat, CreatureAttack::Quick).unwrap());
    let (_, seen) = fight(mmorpg_core::golden::t30(200) as usize, Plan::BackOff);
    let start = seen.iter().position(|tick| tick.began.is_some()).unwrap();
    let landed = &seen[start + contact];
    assert!(
        landed.missed && landed.hit.is_none(),
        "the dive falls short"
    );
}

#[test]
fn a_player_who_counters_in_its_recovery_brings_it_down() {
    let (zone, seen) = fight(mmorpg_core::golden::t30(3_000) as usize, Plan::Counter);
    let blows: Vec<_> = seen.iter().filter(|tick| tick.struck).collect();
    assert!(!blows.is_empty(), "the player hits back");
    for blow in &blows {
        assert!(
            blow.above <= flight::BITE_HEIGHT_UNITS,
            "a blow lands only while it is low"
        );
    }
    assert!(seen.last().unwrap().dead, "the player brings it down");
    // Its corpse lies on the ground under where it fell.
    let state = bat_state(&zone);
    let corpse = to_units(state.position);
    let half = content()
        .creature_template(units::templates::CAVE_BAT)
        .unwrap()
        .half_extents[1];
    assert!(corpse[1] <= half + 2, "the corpse lies at {}", corpse[1]);
}

#[test]
fn running_off_escapes_it_and_it_flies_home_to_roost() {
    let (mut zone, _) = fight(mmorpg_core::golden::t30(60) as usize, Plan::Run);
    for _ in 0..mmorpg_core::golden::t30(600) {
        zone.advance_tick().unwrap();
    }
    let state = bat_state(&zone);
    assert!(matches!(state.ai, CreatureAi::Idle { .. }), "it gave up");
    let at = to_units(state.position);
    let (dx, dz) = (i64::from(at[0]), i64::from(at[2] - 2_000));
    assert!(
        dx * dx + dz * dz <= 20 * 20,
        "back on its roost, not {at:?}"
    );
    let roost = zone_ground() + flight::ROOST_RISE_UNITS;
    assert!(
        (at[1] - roost).abs() <= 2,
        "at its roost height, not {}",
        at[1]
    );
}

/// The arena's ground under the bat.
fn zone_ground() -> i32 {
    content().definition().ground_at(0, 2_000)
}

#[test]
fn a_bat_fight_replays_exactly_and_restores_mid_dive() {
    let length = mmorpg_core::golden::t30(300) as usize;
    assert_eq!(fight(length, Plan::Stand).1, fight(length, Plan::Stand).1);

    let mut zone = zone();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let checkpoint = loop {
        if !engaged && bat_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        observe(&mut zone);
        if bat_state(&zone)
            .attack
            .is_some_and(|attack| attack.contact == BAT_DIVE_TICKS / 2)
        {
            break zone.snapshot().unwrap();
        }
        assert!(
            zone.snapshot().unwrap().tick < 10_000,
            "the bat never dived"
        );
    };
    let mut restored = ZoneSimulation::from_snapshot(checkpoint, content()).unwrap();
    let mut landed = false;
    for _ in 0..mmorpg_core::golden::t30(120) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        let seen = observe(&mut zone);
        landed |= seen.hit.is_some() || seen.missed;
        assert_eq!(seen, observe(&mut restored));
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(landed, "the pending dive landed after the restore");
}
