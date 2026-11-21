//! Peddler Rowan's cart (#992): the journey runs on the zone's clock with nobody near. A quiet
//! day's cart passes the cutting and arrives safely; on an ambush day the robbers stand as the
//! cart reaches the cutting, and left alone they take it: Rowan limps home hurt while they linger
//! and then leave. A Rogue who kills all three during the hold saves the cart and is paid once,
//! however the zone is restored or replayed.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, cart};
use mmorpg_core::{
    CreatureId, CreatureLife, DAY_TICKS, EntityKind, EntityRef, PostureView, ZONE_START_TIME,
    ZoneCommand, ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const ROGUE: u8 = 4;

/// The tick of `time` (ticks since midnight, from 08:00) on zone day `day`.
fn tick_at(day: u64, time: u32) -> u64 {
    day * u64::from(DAY_TICKS) + u64::from(time - ZONE_START_TIME)
}

/// The first quiet and the first ambush day from day 1 on.
fn days() -> (u64, u64) {
    let seed = greyhaven_vale::content().rng_seed();
    let roll = |day| mmorpg_core::cart::ambush_day(seed, tick_at(day, ZONE_START_TIME));
    let quiet = (1..).find(|&day| !roll(day)).unwrap();
    let ambush = (1..).find(|&day| roll(day)).unwrap();
    (quiet, ambush)
}

/// `zone` restored at `tick`, its state otherwise unchanged.
fn at(zone: &ZoneSimulation, tick: u64) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    state.tick = tick;
    for creature in &mut state.creatures {
        if matches!(creature.life, CreatureLife::Corpse { .. }) {
            creature.loot = None;
        }
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn lives(zone: &ZoneSimulation) -> Vec<CreatureLife> {
    let bandits = &zone.content().cart().unwrap().bandits;
    zone.snapshot()
        .unwrap()
        .creatures
        .iter()
        .filter(|creature| bandits.contains(&creature.creature_id))
        .map(|creature| creature.life)
        .collect()
}

/// Rowan as a player arriving beside him now sees him: feet, posture and health.
fn observe(zone: &ZoneSimulation) -> ([i32; 2], PostureView, u8) {
    let mut zone = at(zone, zone.snapshot().unwrap().tick);
    let [x, z] = zone.npc_placement(cart::MERCHANT).unwrap().position;
    zone.add_player(9).unwrap();
    let mut state = zone.snapshot().unwrap();
    let player = state.players.iter_mut().find(|p| p.player_id == 9).unwrap();
    let ground = zone.content().definition().ground_at(x + 300, z);
    player.position = mmorpg_core::physics::from_units([x + 300, ground + 100, z]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    let view = zone.snapshot_for_player(9).unwrap();
    let rowan = view
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Npc && e.id == cart::MERCHANT.get())
        .expect("Rowan is in view");
    (
        [rowan.position[0], rowan.position[2]],
        rowan.flags.posture,
        rowan.health_percent,
    )
}

fn copper(zone: &ZoneSimulation) -> u32 {
    zone.snapshot().unwrap().players[0].copper
}

#[test]
fn unwatched_journeys_resolve_and_one_defender_saves_the_cart_and_is_paid_once() {
    let content = greyhaven_vale::content();
    let run = content.cart().unwrap();
    let post = content.npc(cart::MERCHANT).unwrap().position;
    let arrival = run.arrival(post);
    let hold = mmorpg_core::cart::HOLD_TICKS;
    let linger = mmorpg_core::cart::LINGER_TICKS;
    let (quiet, ambush) = days();
    let fresh = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    assert!(
        lives(&fresh)
            .iter()
            .all(|life| matches!(life, CreatureLife::Despawned { .. }))
    );

    // A quiet day: no robbers, and the cart rolls through the cutting to the farm.
    let mut zone = at(&fresh, tick_at(quiet, arrival) - 1);
    zone.advance_tick().unwrap();
    assert!(
        lives(&zone)
            .iter()
            .all(|life| matches!(life, CreatureLife::Despawned { .. }))
    );
    let passing = at(&zone, tick_at(quiet, arrival + 20));
    let (feet, posture, health) = observe(&passing);
    assert_eq!((posture, health), (PostureView::Hauling, 100));
    assert!(feet[1] > cart::SITE[1], "{feet:?}");
    let homeward = at(&zone, tick_at(quiet, run.end(post) - 5));
    assert_eq!(observe(&homeward).1, PostureView::Hauling);
    let home = at(&zone, tick_at(quiet, run.end(post)));
    assert_eq!(observe(&home), (post, PostureView::Free, 100));

    // An ambush day with nobody near: the robbers stand as the cart reaches the cutting ...
    let mut zone = at(&fresh, tick_at(ambush, arrival) - 1);
    zone.advance_tick().unwrap();
    assert_eq!(lives(&zone), vec![CreatureLife::Alive; 3]);
    let held = at(&zone, tick_at(ambush, arrival + hold / 2));
    assert_eq!(
        observe(&held),
        (
            cart::SITE,
            PostureView::Hauling,
            mmorpg_core::cart::THREATENED_HEALTH_PERCENT
        )
    );
    // ... take the cart when the hold ends: Rowan limps home hurt, the robbers linger ...
    let mut zone = at(&zone, tick_at(ambush, arrival + hold + 10));
    zone.advance_tick().unwrap();
    assert_eq!(lives(&zone), vec![CreatureLife::Alive; 3]);
    let (feet, posture, health) = observe(&zone);
    assert_eq!(
        (posture, health),
        (PostureView::Hurt, mmorpg_core::cart::ROBBED_HEALTH_PERCENT)
    );
    assert!(feet[1] < cart::SITE[1], "{feet:?}");
    // ... and slip away; Rowan rests at his post until dusk.
    let mut zone = at(&zone, tick_at(ambush, arrival + hold + linger));
    zone.advance_tick().unwrap();
    assert!(
        lives(&zone)
            .iter()
            .all(|life| matches!(life, CreatureLife::Despawned { .. }))
    );
    let resting = at(&zone, tick_at(ambush, mmorpg_core::DUSK - 100));
    assert_eq!(
        observe(&resting),
        (
            post,
            PostureView::Hurt,
            mmorpg_core::cart::ROBBED_HEALTH_PERCENT
        )
    );

    // The same day, a Rogue between the two bandits kills them while Rowan holds the cart; the
    // crossbowman already fell to someone else.
    let waiting = fresh
        .snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .find(|creature| creature.creature_id.get() == 2_912)
        .unwrap();
    let mut zone = fresh;
    zone.enable_reward_tracking(64, 16).unwrap();
    zone.add_player(1).unwrap();
    let zone = classes::at_level(zone, 1, 7);
    let mut zone = at(&zone, tick_at(ambush, arrival) - 1);
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    let start = state.tick;
    for creature in &mut state.creatures {
        match creature.creature_id.get() {
            2_910 | 2_911 => creature.health = 1,
            2_912 => {
                let generation = creature.generation;
                *creature = waiting.clone();
                creature.generation = generation;
                creature.life = CreatureLife::Despawned { died_at: start };
            }
            _ => {}
        }
    }
    let [x, z] = [5_400, 380];
    let ground = zone.content().definition().ground_at(x, z);
    state.players[0].position = mmorpg_core::physics::from_units([x, ground + 100, z]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let paid = copper(&zone);
    let mut sequence = 0;
    act(
        &mut zone,
        &mut sequence,
        ZoneCommand::ChooseClass {
            class: ROGUE,
            sex: 0,
        },
    );
    strike(&mut zone, &mut sequence, 2_910);
    assert!(matches!(lives(&zone)[0], CreatureLife::Corpse { .. }));
    assert_eq!(copper(&zone), paid + mmorpg_core::cart::REWARD_COPPER);
    assert_eq!(zone.raid().cart_rewarded, vec![1]);
    // Restoring mid-fight and replaying the same ticks agree, and neither pays again.
    let mut replay =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
            .unwrap();
    let mut replayed = sequence;
    strike(&mut zone, &mut sequence, 2_911);
    strike(&mut replay, &mut replayed, 2_911);
    assert_eq!(zone.snapshot().unwrap(), replay.snapshot().unwrap());
    assert!(matches!(lives(&zone)[1], CreatureLife::Corpse { .. }));
    assert_eq!(copper(&zone), paid + mmorpg_core::cart::REWARD_COPPER);
    assert!(zone.snapshot().unwrap().tick < start + u64::from(hold));
    // Saved: Rowan travels on with his cart, whole, and the hold's end robs nothing.
    let mut zone = at(&zone, tick_at(ambush, arrival + hold + 10));
    zone.advance_tick().unwrap();
    let (feet, posture, health) = observe(&zone);
    assert_eq!((posture, health), (PostureView::Hauling, 100));
    assert!(feet[1] > cart::SITE[1], "{feet:?}");
    assert_eq!(copper(&zone), paid + mmorpg_core::cart::REWARD_COPPER);
}

fn act(zone: &mut ZoneSimulation, sequence: &mut u32, command: ZoneCommand) {
    *sequence += 1;
    zone.apply_command(1, *sequence, command).unwrap();
    zone.advance_tick().unwrap();
}

/// Player 1 strikes `bandit` until it falls (a strike may miss), letting the fight run a while.
fn strike(zone: &mut ZoneSimulation, sequence: &mut u32, bandit: u32) {
    for _ in 0..6 {
        let alive = zone.snapshot().unwrap().creatures.iter().any(|creature| {
            creature.creature_id.get() == bandit && creature.life == CreatureLife::Alive
        });
        if alive {
            strike_once(zone, sequence, bandit);
        }
    }
}

fn strike_once(zone: &mut ZoneSimulation, sequence: &mut u32, bandit: u32) {
    let target = EntityRef::Creature(CreatureId::new(bandit));
    let (facing, _) = arena::aim_at(zone, 1, target).unwrap();
    act(zone, sequence, arena::stand(facing));
    act(
        zone,
        sequence,
        ZoneCommand::UseAbility {
            ability: ids::SINISTER_STRIKE.get(),
            target: Some(target),
            facing: Some(facing),
        },
    );
    for _ in 0..60 {
        zone.advance_tick().unwrap();
    }
}
