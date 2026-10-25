//! The zone clock and Greyhaven's day/night routine (#158): Hilda walks from her tannery into
//! the inn at dusk, opening its door, and back at dawn; her quests follow her; Bram's shop
//! closes at night; player door toggles have a cooldown. Placements derive from the tick, so
//! recovery mid-walk continues exactly.
use std::sync::Arc;

use mmorpg_core::{
    DAWN, DAY_TICKS, DUSK, ErrorCode, HOUR_TICKS, NpcId, QuestId, ZONE_START_TIME, ZoneCommand,
    ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale, is_night, time_of_day,
};

const HILDA: NpcId = NpcId::new(2);
const BRAM: NpcId = NpcId::new(3);
const DOOR: mmorpg_core::DoorId = greyhaven_vale::inn::DOOR;
const WORK: [i32; 2] = [-2_400, 3_150];
const HOME: [i32; 2] = [1_600, 100];

/// The tick whose time of day is `time`, on the zone's first day.
fn at(time: u32) -> u64 {
    u64::from((time + DAY_TICKS - ZONE_START_TIME) % DAY_TICKS)
}

/// A zone with one player at `feet`, recovered at `tick`.
fn zone_at(tick: u64, feet: [i32; 2]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.tick = tick;
    state.players[0].position = mmorpg_core::physics::from_units([feet[0], 90, feet[1]]);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn errors(zone: &ZoneSimulation) -> Vec<ErrorCode> {
    zone.snapshot_for_player(1)
        .unwrap()
        .events
        .into_iter()
        .filter_map(|event| match event {
            ZoneEvent::Error { code, .. } => Some(code),
            _ => None,
        })
        .collect()
}

fn hilda(zone: &ZoneSimulation) -> [i32; 2] {
    zone.npc_placement(HILDA).unwrap().position
}

#[test]
fn the_zone_clock_starts_in_the_morning_and_runs_a_two_hour_day() {
    assert_eq!(DAY_TICKS, 2 * 60 * 60 * u32::from(mmorpg_core::TICK_HZ));
    assert_eq!(time_of_day(0), 8 * HOUR_TICKS);
    assert_eq!(time_of_day(u64::from(DAY_TICKS)), 8 * HOUR_TICKS);
    assert!(!is_night(time_of_day(0)));
    assert!(is_night(DUSK) && is_night(DAWN - 1) && !is_night(DAWN));
}

#[test]
fn hilda_walks_into_the_inn_at_dusk_opening_its_door_and_back_at_dawn() {
    let mut zone = zone_at(at(DUSK) - 1, WORK);
    assert_eq!(hilda(&zone), WORK);
    let mut previous = hilda(&zone);
    let mut opened_at = None;
    let mut recovered = None;
    for step in 0..2_000 {
        zone.advance_tick().unwrap();
        let now = hilda(&zone);
        let travelled =
            i64::from(now[0] - previous[0]).pow(2) + i64::from(now[1] - previous[1]).pow(2);
        assert!(travelled <= 9 * 9, "Hilda walks, never teleports");
        previous = now;
        let open = zone.snapshot().unwrap().doors[0].open;
        if open && opened_at.is_none() {
            opened_at = Some(now);
        }
        if step == 400 {
            // Recovery mid-walk continues exactly.
            let snapshot = zone.snapshot().unwrap();
            recovered =
                Some(ZoneSimulation::from_snapshot(snapshot, Arc::clone(zone.content())).unwrap());
        }
        if let Some(copy) = recovered.as_mut()
            && step > 400
        {
            copy.advance_tick().unwrap();
            assert_eq!(copy.snapshot().unwrap(), zone.snapshot().unwrap());
        }
    }
    assert_eq!(hilda(&zone), HOME, "Hilda spends the night in the inn");
    let opened_at = opened_at.expect("Hilda opened the closed inn door");
    assert!(
        opened_at[1] > 450,
        "she opened it from outside, at {opened_at:?}"
    );
    // At dawn she walks back out to her tannery.
    let mut zone = zone_at(at(DAWN) + 2_000, WORK);
    zone.advance_tick().unwrap();
    assert_eq!(hilda(&zone), WORK);
    assert_ne!(
        zone_at(at(DAWN) + 100, WORK)
            .npc_placement(HILDA)
            .unwrap()
            .velocity,
        [0, 0]
    );
}

#[test]
fn bram_closes_his_shop_at_night_but_hildas_quests_follow_her_home() {
    let sell = ZoneCommand::SellItem {
        npc: BRAM,
        bag_slot: 0,
        quantity: 1,
    };
    // Across the bar from Bram, at night: closed, not out of range.
    let mut zone = zone_at(at(22 * HOUR_TICKS), [1_920, -200]);
    zone.apply_command(1, 1, sell).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(errors(&zone), [ErrorCode::VendorClosed]);
    // By day the same request reaches the trade itself (an empty bag slot).
    let mut zone = zone_at(at(12 * HOUR_TICKS), [1_920, -200]);
    zone.apply_command(1, 1, sell).unwrap();
    zone.advance_tick().unwrap();
    assert_ne!(errors(&zone), [ErrorCode::VendorClosed]);

    // At night, a player beside Hilda in the inn accepts her quest.
    let mut zone = zone_at(at(23 * HOUR_TICKS), [HOME[0] + 100, HOME[1]]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].quests.completed = 1; // Trouble in the Woods turned in.
    zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let pelts = QuestId::new(2);
    zone.apply_command(
        1,
        1,
        ZoneCommand::AcceptQuest {
            npc: HILDA,
            quest: pelts.get(),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(errors(&zone), []);
    assert!(
        zone.snapshot().unwrap().players[0]
            .quests
            .entry(pelts)
            .is_some()
    );
    // Her empty tannery offers nothing at night.
    let zone = zone_at(at(23 * HOUR_TICKS), [WORK[0] + 100, WORK[1]]);
    let mut state = zone.snapshot().unwrap();
    state.players[0].quests.completed = 1;
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    zone.apply_command(
        1,
        1,
        ZoneCommand::AcceptQuest {
            npc: HILDA,
            quest: pelts.get(),
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(errors(&zone), [ErrorCode::OutOfRange]);
}

#[test]
fn players_cannot_spam_the_inn_door() {
    let mut zone = zone_at(at(12 * HOUR_TICKS), [1_495, 600]);
    let mut sequence = 0;
    let mut use_door = |zone: &mut ZoneSimulation, open: bool| {
        sequence += 1;
        zone.apply_command(1, sequence, ZoneCommand::UseDoor { door: DOOR, open })
            .unwrap();
        zone.advance_tick().unwrap();
        errors(zone)
    };
    assert_eq!(use_door(&mut zone, true), []);
    assert_eq!(use_door(&mut zone, false), [ErrorCode::DoorBusy]);
    assert!(zone.snapshot().unwrap().doors[0].open);
    for _ in 0..mmorpg_core::DOOR_COOLDOWN_TICKS {
        zone.advance_tick().unwrap();
    }
    assert_eq!(use_door(&mut zone, false), []);
    assert!(!zone.snapshot().unwrap().doors[0].open);
}
