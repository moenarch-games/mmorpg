//! The Southwood Stag (#997): it grazes Southwood's western meadow and is neutral, so a player
//! may watch it from a few metres. A player who comes within 7 m is a threat: it stands head up
//! through its `Alert`, then charges 2 m along its facing to `Attack_A`'s contact and stands
//! blown through the rest of the clip. Stepping aside as it charges dodges it, and the same
//! zone replays the fight exactly, restored mid-charge too.
mod support;

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, area_at, design, roads, stag, units, wilds};
use mmorpg_core::physics::to_units;
use mmorpg_core::{
    CreatureAi, CreatureAttack, CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn,
    EntityRef, STAG_ALERT_TICKS, STAG_CHARGE_TICKS, STAG_CHARGE_UNITS, STAG_RECOVER_TICKS,
    ZoneCommand, ZoneContent, ZoneEvent, ZoneId, ZoneSimulation, contact_ticks,
};
use support::arena::{self, NORTHWARD};

const STAG: CreatureId = CreatureId::new(3_180);

/// The real Southwood Stag template on flat ground, 20 m north of the player.
fn content() -> Arc<ZoneContent> {
    let greyhaven = greyhaven_vale::content();
    let template = greyhaven
        .creature_template(units::templates::SOUTHWOOD_STAG)
        .unwrap()
        .clone();
    arena::arena(
        vec![template],
        vec![CreatureSpawn {
            levels: Some([3, 3]),
            ..arena::spawn(STAG.get(), units::templates::SOUTHWOOD_STAG, [0, 2_000])
        }],
        vec![],
    )
}

/// One tick of the fight: the stag's XZ feet, its distance to the player and what the player saw.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Tick {
    stag: [i32; 2],
    apart: i64,
    began: Option<(CreatureAttack, u16)>,
    hit: Option<(u16, bool)>,
    missed: bool,
}

fn stag_state(zone: &ZoneSimulation) -> mmorpg_core::CanonicalCreatureSnapshot {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == STAG)
        .unwrap()
}

fn observe(zone: &mut ZoneSimulation) -> Tick {
    let snapshot = zone.snapshot().unwrap();
    let [x, _, z] = to_units(stag_state(zone).position);
    let [px, _, pz] = to_units(snapshot.players[0].position);
    let source = EntityRef::Creature(STAG);
    let mut tick = Tick {
        stag: [x, z],
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

/// The player walks up to 8 m from the stag and watches it for 10 s, then walks on until it
/// engages, and stands for `ticks` ticks; with `dodge` it steps aside as each charge takes off.
fn fight(ticks: usize, dodge: bool) -> Vec<Tick> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content()).unwrap();
    zone.add_player(1).unwrap();
    let mut sequence = 1;
    let mut command = |zone: &mut ZoneSimulation, command: ZoneCommand| {
        zone.apply_command(1, sequence, command).unwrap();
        sequence += 1;
    };
    command(&mut zone, arena::walk(NORTHWARD));
    while observe(&mut zone).apart > 800 {
        zone.advance_tick().unwrap();
    }
    command(&mut zone, arena::stand(NORTHWARD));
    for _ in 0..mmorpg_core::golden::t30(300) {
        zone.advance_tick().unwrap();
        assert!(
            matches!(stag_state(&zone).ai, CreatureAi::Idle { .. }),
            "a stag lets a player watch it from 8 m"
        );
    }
    command(&mut zone, arena::walk(NORTHWARD));
    let mut engaged = false;
    let mut seen = Vec::new();
    for _ in 0..20_000 {
        if !engaged && stag_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            command(&mut zone, arena::stand(NORTHWARD));
        }
        if dodge
            && stag_state(&zone)
                .attack
                .is_some_and(|attack| attack.contact == STAG_CHARGE_TICKS + 1)
        {
            command(
                &mut zone,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 1,
                    facing: NORTHWARD,
                },
            );
        }
        zone.advance_tick().unwrap();
        if engaged {
            seen.push(observe(&mut zone));
            if seen.len() == ticks {
                return seen;
            }
        }
    }
    panic!("the stag never engaged");
}

#[test]
fn southwood_stags_graze_the_western_meadow_off_the_roads() {
    let content = greyhaven_vale::content();
    let stags: Vec<_> = content
        .creature_spawns()
        .iter()
        .filter(|spawn| spawn.template == units::templates::SOUTHWOOD_STAG)
        .collect();
    assert_eq!(stags.len(), 2);
    for spawn in stags {
        let [x, z] = spawn.position;
        assert!(stag::SPAWN_IDS.contains(&spawn.id.get()));
        assert_eq!(area_at(x, z).unwrap().id(), wilds::SOUTHWOOD);
        assert!(
            design::forest_at(x, z).is_none_or(|forest| forest.cover < 20),
            "spawn {} grazes in the open",
            spawn.id.get()
        );
        let road = roads()
            .iter()
            .map(|road| design::polyline_distance(road.points, x, z).0)
            .min()
            .unwrap();
        assert!(
            road > 2_500,
            "spawn {} lies {road} from a road",
            spawn.id.get()
        );
    }
    let template = content
        .creature_template(units::templates::SOUTHWOOD_STAG)
        .unwrap();
    assert_eq!(template.family, CreatureFamily::Stag);
    assert_eq!(template.behaviour, CreatureBehaviour::Neutral);
    assert!(template.family.territorial() && !template.is_large());
    assert!(
        mmorpg_core::loot_table(template.id).is_some(),
        "it drops loot"
    );
}

#[test]
fn the_stag_alerts_charges_lands_on_contact_and_stands_blown() {
    let contact = contact_ticks(CreatureFamily::Stag, CreatureAttack::Quick).unwrap();
    // `Alert`'s 1 s and `Attack_A`'s contact 0.625 s in.
    assert_eq!(contact, mmorpg_core::ticks_from_ms(1_625));
    assert!(STAG_ALERT_TICKS + STAG_CHARGE_TICKS < contact);
    let template = content()
        .creature_template(units::templates::SOUTHWOOD_STAG)
        .unwrap()
        .clone();
    let [low, high] = template.damage_at(3);
    let reach = i64::from(
        mmorpg_core::unit::CREATURE_REACH_UNITS + mmorpg_core::PLAYER_HALF_EXTENTS_UNITS[0],
    );

    let seen = fight(mmorpg_core::golden::t30(600) as usize, false);
    let mut charges = 0;
    let mut hits = 0;
    for (start, tick) in seen.iter().enumerate() {
        let Some((attack, ticks)) = tick.began else {
            continue;
        };
        assert_eq!(
            (attack, ticks),
            (CreatureAttack::Quick, contact),
            "only its charge"
        );
        let land = start + usize::from(contact);
        let Some(landed) = seen.get(land) else {
            break;
        };
        assert!(tick.apart <= reach + i64::from(STAG_CHARGE_UNITS) + 10);
        assert!(
            charges > 0 || tick.apart > reach,
            "it opens from a charge out"
        );
        // Head up and still through the alert and the antlers' lowering, then the charge.
        let take_off = land - usize::from(STAG_CHARGE_TICKS);
        for alert in &seen[start..take_off] {
            assert_eq!(alert.stag, tick.stag, "it stands still through its alert");
        }
        let charged = (i64::from(landed.stag[0] - tick.stag[0]).pow(2)
            + i64::from(landed.stag[1] - tick.stag[1]).pow(2))
        .isqrt();
        let gap = (tick.apart - reach).max(0);
        assert!(
            charged >= gap - 15 && charged <= gap + 25,
            "charged {charged} over a {gap} gap"
        );
        if charges == 0 {
            assert!(
                charged >= 100,
                "the opening charge carried it in {charged} units"
            );
        }
        for between in &seen[start + 1..land] {
            assert!(between.hit.is_none() && !between.missed);
        }
        assert!(
            landed.hit.is_some() || landed.missed,
            "it lands on its contact"
        );
        if let Some((amount, critical)) = landed.hit {
            let ceiling = if critical { high * 2 } else { high };
            assert!((low..=ceiling).contains(&amount), "a charge deals {amount}");
            hits += 1;
        }
        // It stands blown through its recovery: the opening to hit back.
        let recovered = land + usize::from(STAG_RECOVER_TICKS);
        if let Some(window) = seen.get(land + 1..recovered) {
            for still in window {
                assert_eq!(still.stag, seen[land].stag, "it recovers in place");
            }
        }
        charges += 1;
    }
    assert!(charges >= 4 && hits >= 1, "{charges} charges, {hits} hits");
}

#[test]
fn stepping_aside_as_it_charges_dodges_the_antlers() {
    let contact = usize::from(contact_ticks(CreatureFamily::Stag, CreatureAttack::Quick).unwrap());
    let seen = fight(mmorpg_core::golden::t30(200) as usize, true);
    let start = seen.iter().position(|tick| tick.began.is_some()).unwrap();
    let landed = &seen[start + contact];
    assert!(
        landed.missed && landed.hit.is_none(),
        "the charge goes past"
    );
}

#[test]
fn a_stag_fight_replays_exactly_and_restores_mid_charge() {
    let length = mmorpg_core::golden::t30(300) as usize;
    assert_eq!(fight(length, false), fight(length, false));

    let mut zone = ZoneSimulation::with_content(ZoneId::new(7), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let checkpoint = loop {
        if !engaged && stag_state(&zone).ai == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        observe(&mut zone);
        if stag_state(&zone)
            .attack
            .is_some_and(|attack| attack.contact == STAG_CHARGE_TICKS / 2)
        {
            break zone.snapshot().unwrap();
        }
        assert!(
            zone.snapshot().unwrap().tick < 10_000,
            "the stag never charged"
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
    assert!(landed, "the pending charge landed after the restore");
}
