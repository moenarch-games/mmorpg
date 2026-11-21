//! The Marsh Adder (#990): it lies coiled in Eastwater Fen's Adder Hollow, off the Fen Road,
//! and warns a player who comes close. It holds still through its hiss, strikes on its clip's
//! contact with the template's damage, holds still through its recovery, misses a player who
//! backs off through the hiss, and the same zone replays it exactly, restored mid-hiss too.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, adder, area_at, road_distance, units, vale};
use mmorpg_core::physics::to_units;
use mmorpg_core::{
    ADDER_RECOVER_TICKS, ADDER_WARN_TICKS, CreatureAi, CreatureAttack, CreatureFamily, CreatureId,
    CreatureSpawn, EntityRef, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, contact_ticks,
};
use support::arena::{self, NORTHWARD};

/// Away from the adder, which lies north (+Z) of the player.
const SOUTHWARD: u16 = 32_768;

const ADDER: CreatureId = CreatureId::new(3_120);

/// The real Marsh Adder template on flat ground, 20 m north of the player.
fn content() -> Arc<ZoneContent> {
    let greyhaven = greyhaven_vale::content();
    let template = greyhaven
        .creature_template(units::templates::MARSH_ADDER)
        .unwrap()
        .clone();
    arena::arena(
        vec![template],
        vec![CreatureSpawn {
            levels: Some([2, 2]),
            ..arena::spawn(ADDER.get(), units::templates::MARSH_ADDER, [0, 2_000])
        }],
        vec![],
    )
}

/// One tick of the fight: the adder's XZ feet and what the player saw.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Tick {
    adder: [i32; 2],
    began: Option<(CreatureAttack, u16)>,
    hit: Option<(u16, bool)>,
    missed: bool,
}

fn adder_state(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == ADDER)
        .unwrap()
}

fn observe(zone: &mut ZoneSimulation) -> Tick {
    let [x, _, z] = to_units(adder_state(zone).position);
    let source = EntityRef::Creature(ADDER);
    let mut tick = Tick {
        adder: [x, z],
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

/// The player walks north until the adder engages, stands for `ticks` ticks, and backs away
/// south from the first warning on when `back_off`.
fn fight(ticks: usize, back_off: bool) -> Vec<Tick> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let mut seen = Vec::new();
    for _ in 0..10_000 {
        if !engaged && adder_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        if engaged {
            let tick = observe(&mut zone);
            if back_off && tick.began.is_some() && seen.iter().all(|t: &Tick| t.began.is_none()) {
                zone.apply_command(1, 3, arena::walk(SOUTHWARD)).unwrap();
            }
            seen.push(tick);
            if seen.len() == ticks {
                return seen;
            }
        }
    }
    panic!("the adder never engaged");
}

#[test]
fn marsh_adders_lie_coiled_in_the_fen_off_its_road() {
    let content = greyhaven_vale::content();
    let adders: Vec<_> = content
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == units::templates::MARSH_ADDER)
        .collect();
    assert_eq!(adders.len(), 3);
    for spawn in adders {
        let [x, z] = spawn.position;
        assert!(adder::SPAWN_IDS.contains(&spawn.id.get()));
        assert_eq!(area_at(x, z).unwrap().id(), vale::EASTWATER_FEN);
        // Beyond the road index's 10 m reach of every road.
        assert_eq!(
            road_distance(x, z),
            i64::MAX,
            "spawn {} is off the road",
            spawn.id.get()
        );
    }
    let template = content
        .creature_template(units::templates::MARSH_ADDER)
        .unwrap();
    assert_eq!(template.family, CreatureFamily::Adder);
    assert!(template.family.ambushes());
    assert!(
        mmorpg_core::loot_table(template.id).is_some(),
        "it drops loot"
    );
}

#[test]
fn the_adder_hisses_strikes_on_contact_and_recovers() {
    let contact = contact_ticks(CreatureFamily::Adder, CreatureAttack::Quick).unwrap();
    // One `Warn_Hiss_Loop`, then `Strike`'s contact 0.208 s in.
    assert_eq!(contact, mmorpg_core::ticks_from_ms(1_541));
    assert!(ADDER_WARN_TICKS < contact);
    let [low, high] = content()
        .creature_template(units::templates::MARSH_ADDER)
        .unwrap()
        .damage_at(2);

    let seen = fight(mmorpg_core::golden::t30(600) as usize, false);
    let (mut strikes, mut hits) = (0, 0);
    for (start, tick) in seen.iter().enumerate() {
        let Some(began) = tick.began else {
            continue;
        };
        assert_eq!(began, (CreatureAttack::Quick, contact), "only its strike");
        let land = start + usize::from(contact);
        let Some(landed) = seen.get(land) else {
            break;
        };
        // Coiled still through the hiss and the strike up to its contact.
        for between in &seen[start + 1..land] {
            assert_eq!(between.adder, tick.adder, "it holds still while it warns");
            assert!(between.hit.is_none() && !between.missed);
        }
        assert!(
            landed.hit.is_some() || landed.missed,
            "it strikes on its contact"
        );
        if let Some((amount, critical)) = landed.hit {
            let ceiling = if critical { high * 2 } else { high };
            assert!((low..=ceiling).contains(&amount), "a strike deals {amount}");
            hits += 1;
        }
        // It lies open through its recovery.
        if let Some(window) = seen.get(land + 1..land + usize::from(ADDER_RECOVER_TICKS)) {
            for still in window {
                assert_eq!(still.adder, landed.adder, "it recovers in place");
            }
        }
        strikes += 1;
    }
    assert!(strikes >= 4 && hits >= 1, "{strikes} strikes, {hits} hits");
}

#[test]
fn backing_off_through_the_hiss_dodges_the_strike() {
    let contact = usize::from(contact_ticks(CreatureFamily::Adder, CreatureAttack::Quick).unwrap());
    let seen = fight(mmorpg_core::golden::t30(200) as usize, true);
    let start = seen.iter().position(|tick| tick.began.is_some()).unwrap();
    let landed = &seen[start + contact];
    assert!(
        landed.missed && landed.hit.is_none(),
        "the strike falls short"
    );
}

#[test]
fn an_adder_fight_replays_exactly_and_restores_mid_hiss() {
    let length = mmorpg_core::golden::t30(300) as usize;
    assert_eq!(fight(length, false), fight(length, false));

    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let checkpoint = loop {
        if !engaged && adder_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        observe(&mut zone);
        if adder_state(&zone)
            .attack
            .is_some_and(|attack| attack.contact == ADDER_WARN_TICKS / 2)
        {
            break zone.snapshot().unwrap();
        }
        assert!(
            zone.snapshot().unwrap().tick < 10_000,
            "the adder never struck"
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
    assert!(landed, "the pending strike landed after the restore");
}
