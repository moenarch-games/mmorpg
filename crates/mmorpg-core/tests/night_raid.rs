//! Greyhaven's night raid on Millbrook Farm's barn (#530): reserved raiders stand up only for a
//! committed incident, walk from the outskirts to the barn and hit it; an unwatched raid breaks
//! the barn, the survivors withdraw to Redbrand Hollow's camp, and recovery and the cooldown gate
//! the next incident. Recovery mid-raid continues exactly.
use std::sync::Arc;

use mmorpg_core::raid::{
    COOLDOWN_TICKS, OBJECTIVE_INTEGRITY, PHASE_TICKS, RAID_PHASES, TRIGGER_TIME, raid_night,
    wave_size,
};
use mmorpg_core::{
    CanonicalCreatureSnapshot, CreatureLife, DAY_TICKS, RaidLeg, RaidOutcome, ZONE_START_TIME,
    ZoneId, ZoneSimulation, greyhaven_vale,
};

fn zone() -> ZoneSimulation {
    ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap()
}

/// Living raiders of any incident.
fn raiders(zone: &ZoneSimulation) -> Vec<CanonicalCreatureSnapshot> {
    zone.snapshot()
        .unwrap()
        .creatures
        .into_iter()
        .filter(|creature| creature.raid.is_some() && creature.life == CreatureLife::Alive)
        .collect()
}

fn xz(creature: &CanonicalCreatureSnapshot) -> [i32; 2] {
    [
        creature.position[0].round() as i32,
        creature.position[2].round() as i32,
    ]
}

#[test]
fn reserved_raiders_stand_only_for_a_committed_incident() {
    let mut zone = zone();
    assert!(raiders(&zone).is_empty());
    for _ in 0..600 {
        zone.advance_tick().unwrap();
    }
    assert!(
        raiders(&zone).is_empty(),
        "raiders never respawn on their own"
    );
    assert!(zone.begin_raid().unwrap());
    let first = raiders(&zone);
    assert_eq!(first.len(), usize::from(wave_size(1, 1)));
    assert!(first.iter().all(|raider| {
        raider
            .raid
            .is_some_and(|member| member.incident == 1 && member.leg == RaidLeg::Assault)
    }));
    assert!(!zone.begin_raid().unwrap(), "one incident at a time");
    assert_eq!(zone.raid().active.as_ref().unwrap().scaled_for, vec![0]);
}

#[test]
fn phases_scale_their_waves_with_participants_up_to_a_bound() {
    assert_eq!(wave_size(1, 1), 3);
    assert_eq!(wave_size(3, 1), 4);
    assert_eq!(wave_size(2, 3), 5);
    assert_eq!(wave_size(3, 40), 6);
}

#[test]
fn two_of_three_nights_are_raid_nights_without_wall_clock_or_zone_rng() {
    let seed = greyhaven_vale::content().rng_seed();
    let check = |night: u64| {
        u64::from(TRIGGER_TIME + DAY_TICKS - ZONE_START_TIME) + night * u64::from(DAY_TICKS)
    };
    let nights = (0..300)
        .filter(|&night| raid_night(seed, check(night)))
        .count();
    assert!((170..=230).contains(&nights), "{nights} raid nights of 300");
    assert_eq!(raid_night(seed, check(7)), raid_night(seed, check(7)));
}

#[test]
fn an_unwatched_raid_breaks_the_barn_and_the_survivors_reinforce_the_camp() {
    let mut zone = zone();
    assert!(zone.begin_raid().unwrap());
    let start = raiders(&zone);
    let barn = [8_000, 4_400];
    let distance = |at: [i32; 2]| {
        (f64::from(at[0] - barn[0]).powi(2) + f64::from(at[1] - barn[1]).powi(2)).sqrt()
    };
    let mut recovered: Option<ZoneSimulation> = None;
    let bound = u64::from(PHASE_TICKS) * u64::from(RAID_PHASES) + 10;
    for step in 0..bound {
        zone.advance_tick().unwrap();
        if let Some(copy) = recovered.as_mut() {
            copy.advance_tick().unwrap();
            assert_eq!(
                copy.snapshot().unwrap(),
                zone.snapshot().unwrap(),
                "recovery continues exactly"
            );
        }
        if step == 1_920 {
            recovered = None;
        }
        if step == 1_800 {
            // 30 s in, every first-wave raider closed in on the barn and it took hits.
            for raider in &start {
                let now = raiders(&zone)
                    .into_iter()
                    .find(|other| other.creature_id == raider.creature_id)
                    .unwrap();
                assert!(distance(xz(&now)) < distance(xz(raider)) - 1_000.0);
            }
            assert!(zone.raid().damage > 0);
            let snapshot = zone.snapshot().unwrap();
            recovered =
                Some(ZoneSimulation::from_snapshot(snapshot, Arc::clone(zone.content())).unwrap());
        }
        if zone.raid().last.is_some() {
            break;
        }
    }
    let last = zone
        .raid()
        .last
        .expect("the raid resolves within its phases");
    assert_eq!(last.outcome, RaidOutcome::Lost, "nobody defended the barn");
    assert_eq!(zone.raid().damage, OBJECTIVE_INTEGRITY);
    assert!(zone.raid().active.is_none());
    assert!(
        !zone.raid_eligible(last.at + u64::from(COOLDOWN_TICKS)),
        "the barn still recovers"
    );
    let survivors = raiders(&zone);
    assert!(!survivors.is_empty());
    assert!(
        survivors
            .iter()
            .all(|raider| raider.raid.unwrap().leg == RaidLeg::Retreat)
    );
    // They withdraw all the way to the camp and stand there as its defenders.
    let mut arrived = false;
    for step in 0..u64::from(mmorpg_core::raid::RETREAT_TIMEOUT_TICKS) {
        zone.advance_tick().unwrap();
        if step % 60 != 0 {
            continue;
        }
        let now = raiders(&zone);
        if now
            .iter()
            .all(|raider| raider.raid.unwrap().leg == RaidLeg::Camp)
        {
            arrived = now.len() == survivors.len();
            break;
        }
    }
    let camp = raiders(&zone);
    assert!(
        arrived,
        "every survivor reached the camp: {:?}",
        camp.iter().map(xz).collect::<Vec<_>>()
    );
    assert!(camp.iter().all(|raider| {
        let [x, z] = xz(raider);
        x.abs() < 2_500 && (-10_000..-7_000).contains(&z)
    }));
    assert!(
        !zone.begin_raid().unwrap(),
        "no new raid before recovery and cooldown"
    );
}

/// A zone at `tick` with `players` standing at `feet`, a running incident committed one tick
/// earlier when `raid` is set.
fn zone_at(tick: u64, feet: &[[i32; 2]]) -> ZoneSimulation {
    let mut zone = zone();
    for (index, _) in feet.iter().enumerate() {
        zone.add_player(u32::try_from(index).unwrap() + 1).unwrap();
    }
    let mut state = zone.snapshot().unwrap();
    state.tick = tick;
    for (player, at) in state.players.iter_mut().zip(feet) {
        player.position = mmorpg_core::physics::from_units([at[0], 1_190, at[1]]);
    }
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

/// The first tick at `TRIGGER_TIME` after `from` whose night is (or is not) a raid night.
fn trigger_tick(from: u64, raid: bool) -> u64 {
    let seed = greyhaven_vale::content().rng_seed();
    let first = u64::from(TRIGGER_TIME + DAY_TICKS - ZONE_START_TIME) % u64::from(DAY_TICKS);
    (0..)
        .map(|night| first + night * u64::from(DAY_TICKS))
        .find(|&tick| tick > from && raid_night(seed, tick) == raid)
        .unwrap()
}

#[test]
fn the_nightly_check_commits_a_raid_only_on_raid_nights_once_eligible() {
    for raid in [true, false] {
        let tick = trigger_tick(0, raid);
        let mut zone = zone_at(tick - 1, &[]);
        zone.advance_tick().unwrap();
        assert_eq!(zone.raid().active.is_some(), raid, "raid night: {raid}");
        assert_eq!(!raiders(&zone).is_empty(), raid);
    }
    // A raid night inside the last incident's cooldown passes without a raid.
    let tick = trigger_tick(u64::from(COOLDOWN_TICKS), true);
    let mut zone = zone_at(tick - 1, &[]);
    let mut state = zone.snapshot().unwrap();
    state.raid.incidents = 1;
    state.raid.last = Some(mmorpg_core::RaidResolution {
        incident: 1,
        at: tick - u64::from(COOLDOWN_TICKS) + 1,
        outcome: RaidOutcome::Defended,
    });
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    assert!(zone.raid().active.is_none());
}

#[test]
fn a_phase_boundary_sizes_the_next_wave_for_nearby_participants_and_the_last_defends_the_barn() {
    let near_barn = [[6_900, 3_000], [7_000, 3_000], [0, 2_000]];
    let mut zone = zone_at(100_000, &near_barn);
    assert!(zone.begin_raid().unwrap());
    let first = raiders(&zone).len();
    // All three fought the raid, but the third stands far from the barn.
    let mut state = zone.snapshot().unwrap();
    let incident = state.raid.active.as_mut().unwrap();
    incident.participants = vec![1, 2, 3];
    incident.phase_started = state.tick + 1 - u64::from(PHASE_TICKS);
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    let incident = zone.raid().active.clone().unwrap();
    assert_eq!((incident.phase, incident.scaled_for), (2, vec![0, 2]));
    assert_eq!(raiders(&zone).len(), first + usize::from(wave_size(2, 2)));
    // The barn still stands when the last phase ends: the raid is defended and the living
    // raiders withdraw.
    let mut state = zone.snapshot().unwrap();
    let incident = state.raid.active.as_mut().unwrap();
    incident.phase = RAID_PHASES;
    incident.scaled_for = vec![0, 2, 2];
    incident.phase_started = state.tick + 1 - u64::from(PHASE_TICKS);
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    let last = zone.raid().last.unwrap();
    assert_eq!(last.outcome, RaidOutcome::Defended);
    assert!(
        raiders(&zone)
            .iter()
            .all(|raider| raider.raid.unwrap().leg == RaidLeg::Retreat)
    );
    // A defended, undamaged barn still waits for the cooldown.
    assert!(!zone.raid_eligible(last.at + u64::from(COOLDOWN_TICKS) - 1));
    assert!(zone.raid_eligible(last.at + u64::from(COOLDOWN_TICKS)));
}
