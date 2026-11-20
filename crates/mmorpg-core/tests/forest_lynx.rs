//! The Forest Lynx (#989): it lies up on Southwood's edge, stalks a player who comes close and
//! pounces from a leap out. Its wind-up holds it still, its leap carries it in on the lunge's
//! take-off, its pounce lands on the clip's contact with the template's damage, it holds still
//! through its recovery, and the same zone replays it exactly, restored mid-pounce too.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, area_at, lynx, units, wilds};
use mmorpg_core::physics::to_units;
use mmorpg_core::{
    CreatureAi, CreatureAttack, CreatureFamily, CreatureId, CreatureSpawn, EntityRef,
    POUNCE_LEAP_TICKS, POUNCE_LEAP_UNITS, POUNCE_RECOVER_TICKS, ZoneContent, ZoneEvent, ZoneId,
    ZoneSimulation, contact_ticks,
};
use support::arena::{self, NORTHWARD};

const LYNX: CreatureId = CreatureId::new(3_100);

/// The real Forest Lynx template on flat ground, 20 m north of the player.
fn content() -> Arc<ZoneContent> {
    let greyhaven = greyhaven_vale::content();
    let template = greyhaven
        .creature_template(units::templates::FOREST_LYNX)
        .unwrap()
        .clone();
    arena::arena(
        vec![template],
        vec![CreatureSpawn {
            levels: Some([2, 2]),
            ..arena::spawn(LYNX.get(), units::templates::FOREST_LYNX, [0, 2_000])
        }],
        vec![],
    )
}

/// One tick of the fight: the lynx's XZ feet, its distance to the player and what the player saw.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Tick {
    lynx: [i32; 2],
    apart: i64,
    began: Option<(CreatureAttack, u16)>,
    hit: Option<(u16, bool)>,
    missed: bool,
}

/// The player walks north until the lynx engages, then stands for `ticks` ticks.
fn fight(ticks: usize) -> Vec<Tick> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = None;
    let mut seen = Vec::new();
    for tick in 0..10_000 {
        if engaged.is_none() && lynx_state(&zone).ai == CreatureAi::Engaged {
            engaged = Some(tick);
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        if engaged.is_some() {
            seen.push(observe(&mut zone));
            if seen.len() == ticks {
                return seen;
            }
        }
    }
    panic!("the lynx never engaged");
}

fn lynx_state(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == LYNX)
        .unwrap()
}

fn observe(zone: &mut ZoneSimulation) -> Tick {
    let snapshot = zone.snapshot().unwrap();
    let [x, _, z] = to_units(lynx_state(zone).position);
    let [px, _, pz] = to_units(snapshot.players[0].position);
    let source = EntityRef::Creature(LYNX);
    let mut tick = Tick {
        lynx: [x, z],
        apart: (i64::from(px - x).pow(2) + i64::from(pz - z).pow(2)).isqrt(),
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

#[test]
fn forest_lynxes_lie_up_on_the_southwood_edge() {
    let content = greyhaven_vale::content();
    let lynxes: Vec<_> = content
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == units::templates::FOREST_LYNX)
        .collect();
    assert_eq!(lynxes.len(), 3);
    for spawn in lynxes {
        assert!(lynx::SPAWN_IDS.contains(&spawn.id.get()));
        let area = area_at(spawn.position[0], spawn.position[1]).unwrap();
        assert_eq!(area.id(), wilds::SOUTHWOOD, "spawn {}", spawn.id.get());
    }
    let template = content
        .creature_template(units::templates::FOREST_LYNX)
        .unwrap();
    assert_eq!(template.family, CreatureFamily::Lynx);
    assert!(template.family.ambushes() && template.family.pounces());
    assert!(
        mmorpg_core::loot_table(template.id).is_some(),
        "it drops loot"
    );
}

#[test]
fn the_lynx_winds_up_leaps_lands_on_contact_and_recovers() {
    let contact = contact_ticks(CreatureFamily::Lynx, CreatureAttack::Quick).unwrap();
    // `Pounce_Windup`'s 0.5 s and `Pounce_Lunge`'s contact 0.333 s in.
    assert_eq!(contact, mmorpg_core::ticks_from_ms(833));
    let template = content()
        .creature_template(units::templates::FOREST_LYNX)
        .unwrap()
        .clone();
    let [low, high] = template.damage_at(2);
    let reach = i64::from(
        mmorpg_core::unit::CREATURE_REACH_UNITS + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[0],
    );

    let seen = fight(mmorpg_core::golden::t30(600) as usize);
    let mut pounces = 0;
    let mut hits = 0;
    for (start, tick) in seen.iter().enumerate() {
        let Some((attack, ticks)) = tick.began else {
            continue;
        };
        assert_eq!(
            (attack, ticks),
            (CreatureAttack::Quick, contact),
            "only its pounce"
        );
        let land = start + usize::from(contact);
        let Some(landed) = seen.get(land) else {
            break;
        };
        // It begins within a leap of its reach: the first from beyond it, after a stalk.
        assert!(tick.apart <= reach + i64::from(POUNCE_LEAP_UNITS) + 10);
        assert!(
            pounces > 0 || tick.apart > reach,
            "it opens from a leap out"
        );
        // Crouched still through the wind-up, then the leap closes the gap.
        let take_off = land - usize::from(POUNCE_LEAP_TICKS);
        for crouched in &seen[start..take_off] {
            assert_eq!(
                crouched.lynx, tick.lynx,
                "it holds still through its wind-up"
            );
        }
        let leapt = (i64::from(landed.lynx[0] - tick.lynx[0]).pow(2)
            + i64::from(landed.lynx[1] - tick.lynx[1]).pow(2))
        .isqrt();
        // The leap closes the gap to its reach and no further.
        let gap = (tick.apart - reach).max(0);
        assert!(
            leapt >= gap - 15 && leapt <= gap + 25,
            "leapt {leapt} over a {gap} gap"
        );
        if pounces == 0 {
            assert!(leapt >= 100, "the opening leap carried it in {leapt} units");
        }
        // The pounce resolves on the contact tick and no other.
        for between in &seen[start + 1..land] {
            assert!(between.hit.is_none() && !between.missed);
        }
        assert!(
            landed.hit.is_some() || landed.missed,
            "it lands on its contact"
        );
        if let Some((amount, critical)) = landed.hit {
            let ceiling = if critical { high * 2 } else { high };
            assert!((low..=ceiling).contains(&amount), "a pounce deals {amount}");
            hits += 1;
        }
        // It holds still through its recovery.
        let recovered = land + usize::from(POUNCE_RECOVER_TICKS);
        if let Some(window) = seen.get(land + 1..recovered) {
            for still in window {
                assert_eq!(still.lynx, seen[land].lynx, "it recovers in place");
            }
        }
        pounces += 1;
    }
    assert!(pounces >= 4 && hits >= 1, "{pounces} pounces, {hits} hits");
}

#[test]
fn a_lynx_fight_replays_exactly_and_restores_mid_pounce() {
    let length = mmorpg_core::golden::t30(300) as usize;
    assert_eq!(
        fight(length),
        fight(length),
        "the same zone pounces the same way"
    );

    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let checkpoint = loop {
        if !engaged && lynx_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        observe(&mut zone);
        // Mid-leap.
        if lynx_state(&zone)
            .attack
            .is_some_and(|attack| attack.contact == POUNCE_LEAP_TICKS / 2)
        {
            break zone.snapshot().unwrap();
        }
        assert!(
            zone.snapshot().unwrap().tick < 10_000,
            "the lynx never pounced"
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
    assert!(landed, "the pending pounce landed after the restore");
}
