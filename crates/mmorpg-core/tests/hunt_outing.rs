//! Trapper Sela's hunt (#991): the outing runs on the zone's clock with nobody near. A quiet
//! day brings her home with her catch; a confrontation day stands the bear at her hunting
//! ground, and left alone it sends her limping home hurt. A Rogue who kills the bear while she
//! holds her ground turns that day into a catch. Restoring mid-outing places her exactly.
mod support;

use std::sync::Arc;

use mmorpg_core::ability::ids;
use mmorpg_core::greyhaven_vale::{self, hunt};
use mmorpg_core::{
    CreatureLife, DAY_TICKS, EntityKind, EntityRef, PostureView, ZONE_START_TIME, ZoneCommand,
    ZoneId, ZoneSimulation,
};
use support::{arena, classes};

const ROGUE: u8 = 4;
/// Sela's post at the hunters' camp.
const CAMP: [i32; 2] = [-49_000, 78_900];

/// The tick of `time` (ticks since midnight, from 08:00) on zone day `day`.
fn tick_at(day: u64, time: u32) -> u64 {
    day * u64::from(DAY_TICKS) + u64::from(time - ZONE_START_TIME)
}

/// The first quiet and the first confrontation day from day 1 on.
fn days() -> (u64, u64) {
    let seed = greyhaven_vale::content().rng_seed();
    let roll = |day| mmorpg_core::hunt::confrontation_day(seed, tick_at(day, ZONE_START_TIME));
    let quiet = (1..).find(|&day| !roll(day)).unwrap();
    let confrontation = (1..).find(|&day| roll(day)).unwrap();
    (quiet, confrontation)
}

/// `zone` restored at `tick`, its state otherwise unchanged.
fn at(zone: &ZoneSimulation, tick: u64) -> ZoneSimulation {
    let mut state = zone.snapshot().unwrap();
    state.tick = tick;
    // A corpse left behind has been looted or has rotted by then.
    for creature in &mut state.creatures {
        if matches!(creature.life, CreatureLife::Corpse { .. }) {
            creature.loot = None;
        }
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn bear(zone: &ZoneSimulation) -> CreatureLife {
    zone.snapshot()
        .unwrap()
        .creatures
        .iter()
        .find(|creature| creature.creature_id == hunt::BEAR)
        .unwrap()
        .life
}

/// Sela as a player arriving beside her now sees her: feet, posture and health.
fn observe(zone: &ZoneSimulation) -> ([i32; 2], PostureView, u8) {
    let mut zone = at(zone, zone.snapshot().unwrap().tick);
    let [x, z] = zone.npc_placement(hunt::HUNTER).unwrap().position;
    zone.add_player(9).unwrap();
    let mut state = zone.snapshot().unwrap();
    let player = state.players.iter_mut().find(|p| p.player_id == 9).unwrap();
    let ground = zone.content().definition().ground_at(x + 400, z);
    player.position = mmorpg_core::physics::from_units([x + 400, ground + 100, z]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    // One tick moves her body, and the interest index with it, to where the clock puts her.
    zone.advance_tick().unwrap();
    let view = zone.snapshot_for_player(9).unwrap();
    let sela = view
        .entities
        .iter()
        .find(|e| e.kind == EntityKind::Npc && e.id == hunt::HUNTER.get())
        .expect("Sela is in view");
    (
        [sela.position[0], sela.position[2]],
        sela.flags.posture,
        sela.health_percent,
    )
}

#[test]
fn unwatched_outings_end_by_the_day_and_a_helper_can_turn_a_confrontation() {
    let content = greyhaven_vale::content();
    let outing = content.hunt().unwrap();
    let (arrival, end) = (outing.arrival(CAMP), outing.end(CAMP));
    let walk = arrival - outing.leave;
    let (quiet, confrontation) = days();
    let fresh = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    assert!(matches!(bear(&fresh), CreatureLife::Despawned { .. }));

    // A quiet day: no bear, and she comes home with her catch.
    let mut zone = at(&fresh, tick_at(quiet, arrival) - 1);
    zone.advance_tick().unwrap();
    assert!(matches!(bear(&zone), CreatureLife::Despawned { .. }));
    assert_eq!(
        zone.npc_placement(hunt::HUNTER).unwrap().position,
        hunt::GROUND
    );
    let home = at(&zone, tick_at(quiet, end + walk + 10));
    assert_eq!(observe(&home), (CAMP, PostureView::Carrying, 100));

    // A confrontation day with nobody near: the bear stands when she reaches the ground ...
    let mut zone = at(&fresh, tick_at(confrontation, arrival) - 1);
    zone.advance_tick().unwrap();
    assert_eq!(bear(&zone), CreatureLife::Alive);
    let stalking = zone.npc_placement(hunt::HUNTER).unwrap();
    assert_eq!(stalking.position, hunt::GROUND);
    // ... restoring mid-outing places her and the bear exactly, and replays the same ticks ...
    let mut replay = at(&zone, zone.snapshot().unwrap().tick);
    for _ in 0..30 {
        zone.advance_tick().unwrap();
        replay.advance_tick().unwrap();
    }
    assert_eq!(zone.snapshot().unwrap(), replay.snapshot().unwrap());
    // ... leaves once her stalk is over, and she limps home hurt and rests until dusk.
    let mut zone = at(&zone, tick_at(confrontation, end) - 1);
    zone.advance_tick().unwrap();
    assert!(matches!(bear(&zone), CreatureLife::Despawned { .. }));
    let limping = at(&zone, tick_at(confrontation, end + walk / 2));
    let (feet, posture, health) = observe(&limping);
    assert_eq!((posture, health), (PostureView::Hurt, hunt_injured()));
    assert!(feet != CAMP && feet != hunt::GROUND, "{feet:?}");
    let resting = at(&zone, tick_at(confrontation, end + 2 * walk + 10));
    assert_eq!(observe(&resting), (CAMP, PostureView::Hurt, hunt_injured()));
    let dusk = at(&zone, tick_at(confrontation, mmorpg_core::DUSK));
    assert_eq!(observe(&dusk).1, PostureView::Free);

    // The same day, a Rogue kills the bear while she holds her ground: her catch comes home.
    let mut zone = fresh;
    zone.add_player(1).unwrap();
    let zone = classes::at_level(zone, 1, 7);
    let mut zone = at(&zone, tick_at(confrontation, arrival) - 1);
    zone.advance_tick().unwrap();
    let mut state = zone.snapshot().unwrap();
    let beast = state
        .creatures
        .iter_mut()
        .find(|creature| creature.creature_id == hunt::BEAR)
        .unwrap();
    beast.health = 1;
    let [x, z] = hunt::BEAR_AT;
    let ground = zone.content().definition().ground_at(x + 250, z);
    state.players[0].position = mmorpg_core::physics::from_units([x + 250, ground + 100, z]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let mut sequence = 0;
    let mut command = |zone: &mut ZoneSimulation, command| {
        sequence += 1;
        zone.apply_command(1, sequence, command).unwrap();
        zone.advance_tick().unwrap();
    };
    command(
        &mut zone,
        ZoneCommand::ChooseClass {
            class: ROGUE,
            sex: 0,
        },
    );
    let target = EntityRef::Creature(hunt::BEAR);
    let (facing, _) = arena::aim_at(&zone, 1, target).unwrap();
    command(&mut zone, arena::stand(facing));
    command(
        &mut zone,
        ZoneCommand::UseAbility {
            ability: ids::SINISTER_STRIKE.get(),
            target: Some(target),
            facing: Some(facing),
        },
    );
    for _ in 0..60 {
        zone.advance_tick().unwrap();
    }
    assert!(
        matches!(bear(&zone), CreatureLife::Corpse { .. }),
        "{:?}",
        bear(&zone)
    );
    let home = at(&zone, tick_at(confrontation, end + walk + 10));
    assert_eq!(observe(&home), (CAMP, PostureView::Carrying, 100));
}

fn hunt_injured() -> u8 {
    mmorpg_core::hunt::INJURED_HEALTH_PERCENT
}
