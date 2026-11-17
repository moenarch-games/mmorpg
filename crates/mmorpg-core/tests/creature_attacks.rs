//! Creature attacks with a wind-up (#889): a beast begins its attack in view, bites on its
//! family's contact tick, draws its quick and heavy attacks from its own stream, and a zone
//! restored mid-wind-up bites exactly as the original.
mod support;

use std::sync::Arc;

use mmorpg_core::{
    CreatureAi, CreatureAttack, CreatureFamily, CreatureId, EntityRef, ZoneContent, ZoneEvent,
    ZoneId, ZoneSimulation, contact_ticks,
};
use support::arena::{self, NORTHWARD, WOLF};

const HUNTER: CreatureId = CreatureId::new(1);

fn content() -> Arc<ZoneContent> {
    // A weak bite, so the player outlasts many attacks.
    arena::arena(
        vec![arena::wolf(500, [1, 1])],
        vec![arena::spawn(1, WOLF, [0, 2_000])],
        vec![],
    )
}

/// What the player saw a tick: a wolf attack beginning, or its bite landing or missing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Seen {
    Began(CreatureAttack, u16),
    Landed,
}

/// A player walks toward the wolf until it engages and then stands still for `ticks` ticks;
/// returns the zone and what the player saw, by tick.
fn fight(ticks: u64) -> (ZoneSimulation, Vec<(u64, Seen)>) {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut seen = Vec::new();
    let mut engaged = false;
    for tick in 0..ticks {
        if !engaged && creature_ai(&zone) == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        seen.extend(observe(&mut zone).into_iter().map(|event| (tick, event)));
    }
    (zone, seen)
}

fn creature_ai(zone: &ZoneSimulation) -> CreatureAi {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id == HUNTER)
        .unwrap()
        .ai
}

fn observe(zone: &mut ZoneSimulation) -> Vec<Seen> {
    let wolf = EntityRef::Creature(HUNTER);
    zone.snapshot_for_player(1)
        .unwrap()
        .events
        .iter()
        .filter_map(|event| match *event {
            ZoneEvent::CreatureAttackBegan {
                source,
                attack,
                contact,
            } if source == wolf => Some(Seen::Began(attack, contact)),
            ZoneEvent::DamageTaken { source, .. } | ZoneEvent::Miss { source, .. }
                if source == wolf =>
            {
                Some(Seen::Landed)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_wolf_bites_on_its_contact_tick() {
    let (_, seen) = fight(mmorpg_core::golden::t30(1_200));
    let mut began = None;
    let mut bites = 0;
    for &(tick, event) in &seen {
        match event {
            Seen::Began(attack, contact) => {
                assert_eq!(
                    Some(contact),
                    contact_ticks(CreatureFamily::Wolf, attack),
                    "the event carries the family's authored contact"
                );
                assert!(began.is_none(), "one attack at a time");
                began = Some((tick, contact));
            }
            Seen::Landed => {
                let (start, contact) = began.take().expect("every bite was wound up in view");
                assert_eq!(
                    tick - start,
                    u64::from(contact),
                    "the bite lands on its contact tick"
                );
                bites += 1;
            }
        }
    }
    assert!(bites >= 8, "the wolf bit {bites} times");
}

#[test]
fn quick_and_heavy_attacks_come_from_the_wolfs_own_stream() {
    let attacks = |seen: &[(u64, Seen)]| -> Vec<CreatureAttack> {
        seen.iter()
            .filter_map(|&(_, event)| match event {
                Seen::Began(attack, _) => Some(attack),
                Seen::Landed => None,
            })
            .collect()
    };
    let first = attacks(&fight(mmorpg_core::golden::t30(1_200)).1);
    assert!(first.contains(&CreatureAttack::Quick));
    assert!(first.contains(&CreatureAttack::Heavy));
    assert!(
        first
            .iter()
            .filter(|&&a| a == CreatureAttack::Quick)
            .count()
            > first
                .iter()
                .filter(|&&a| a == CreatureAttack::Heavy)
                .count(),
        "the quick attack is the common one: {first:?}"
    );
    assert_eq!(
        first,
        attacks(&fight(mmorpg_core::golden::t30(1_200)).1),
        "the same zone draws the same attacks"
    );
}

#[test]
fn a_zone_restored_mid_wind_up_bites_like_the_original() {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(5), content()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, arena::walk(NORTHWARD)).unwrap();
    let mut engaged = false;
    let checkpoint = loop {
        if !engaged && creature_ai(&zone) == CreatureAi::Engaged {
            engaged = true;
            zone.apply_command(1, 2, arena::stand(NORTHWARD)).unwrap();
        }
        zone.advance_tick().unwrap();
        observe(&mut zone);
        let snapshot = zone.snapshot().unwrap();
        let wolf = snapshot
            .creatures
            .iter()
            .find(|creature| creature.creature_id == HUNTER)
            .unwrap();
        if wolf.attack.is_some_and(|attack| attack.contact > 2) {
            break snapshot;
        }
        assert!(snapshot.tick < 10_000, "the wolf never wound up");
    };
    let mut restored = ZoneSimulation::from_snapshot(checkpoint, content()).unwrap();
    let mut bitten = false;
    for _ in 0..mmorpg_core::golden::t30(150) {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        let seen = observe(&mut zone);
        bitten |= seen.contains(&Seen::Landed);
        assert_eq!(seen, observe(&mut restored));
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert!(bitten, "the pending bite landed after the restore");
}
