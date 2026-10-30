//! Marshal Elden Greywatch's private home (#158, Atlas 5.1.48): only characters with one of his
//! quests may open his door; at night he sleeps by his bed, a knock with a turn-in ready wakes
//! him to the doorway, he takes the turn-in from the doorstep but offers no new quests, and
//! after a minute idle he closes the door and goes back to bed. Recovery mid-wake continues
//! exactly.
use std::sync::Arc;

use mmorpg_core::{
    DAY_TICKS, DUSK, ErrorCode, HOUR_TICKS, NpcId, PRIVATE_DOOR_CLOSE_TICKS, QuestEntry, QuestId,
    WAKE_IDLE_TICKS, ZONE_START_TIME, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation,
    greyhaven_vale::{self, marshal_house},
};

const ELDEN: NpcId = marshal_house::RESIDENT;
const DOOR: mmorpg_core::DoorId = marshal_house::DOOR;
const TROUBLE: QuestId = greyhaven_vale::quests::TROUBLE_IN_THE_WOODS;

fn at(time: u32) -> u64 {
    u64::from((time + DAY_TICKS - ZONE_START_TIME) % DAY_TICKS)
}

/// A zone at `tick` with one player on Elden's doorstep holding `quests`.
fn zone_at(tick: u64, quests: Vec<QuestEntry>) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.tick = tick;
    let [x, z] = marshal_house::DOORSTEP;
    state.players[0].position = mmorpg_core::physics::from_units([x, 90, z]);
    state.players[0].quests.entries = quests;
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn trouble(kills: u8) -> Vec<QuestEntry> {
    let mut progress = [0; mmorpg_core::MAX_QUEST_OBJECTIVES];
    progress[0] = kills;
    vec![QuestEntry {
        quest: TROUBLE,
        progress,
    }]
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

fn command(zone: &mut ZoneSimulation, sequence: u32, command: ZoneCommand) -> Vec<ErrorCode> {
    zone.apply_command(1, sequence, command).unwrap();
    zone.advance_tick().unwrap();
    errors(zone)
}

fn door_open(zone: &ZoneSimulation) -> bool {
    zone.door_views()
        .iter()
        .any(|view| view.door == DOOR && view.open)
}

fn elden(zone: &ZoneSimulation) -> [i32; 2] {
    zone.npc_placement(ELDEN).unwrap().position
}

const OPEN: ZoneCommand = ZoneCommand::UseDoor {
    door: DOOR,
    open: true,
};

#[test]
fn only_characters_with_eldens_quests_may_open_his_door_and_it_closes_itself() {
    let mut stranger = zone_at(at(12 * HOUR_TICKS), Vec::new());
    assert_eq!(command(&mut stranger, 1, OPEN), [ErrorCode::PrivateHome]);
    assert!(!door_open(&stranger));

    let mut invited = zone_at(at(12 * HOUR_TICKS), trouble(0));
    assert_eq!(command(&mut invited, 1, OPEN), []);
    assert!(door_open(&invited));
    for _ in 0..PRIVATE_DOOR_CLOSE_TICKS {
        invited.advance_tick().unwrap();
    }
    assert!(!door_open(&invited), "the private door closes behind you");
}

#[test]
fn elden_walks_home_at_dusk_and_sleeps_by_his_bed() {
    let mut zone = zone_at(at(DUSK) - 1, Vec::new());
    for _ in 0..2_000 {
        zone.advance_tick().unwrap();
    }
    assert_eq!(elden(&zone), marshal_house::BEDSIDE);
    assert!(!door_open(&zone), "he left his door closed behind him");
}

#[test]
fn a_night_knock_wakes_elden_for_a_doorstep_turn_in_and_he_returns_to_bed() {
    let night = at(23 * HOUR_TICKS);
    // Nothing urgent: no one answers, and he offers no new quests at night.
    let mut idle = zone_at(night, Vec::new());
    assert_eq!(command(&mut idle, 1, OPEN), [ErrorCode::NoAnswer]);
    assert_eq!(
        command(
            &mut idle,
            2,
            ZoneCommand::AcceptQuest {
                npc: ELDEN,
                quest: TROUBLE.get()
            }
        ),
        [ErrorCode::NotAtNight]
    );
    // The door is no way in at night, invited or not.
    let mut urgent = zone_at(night, trouble(6));
    assert_eq!(elden(&urgent), marshal_house::BEDSIDE);
    assert_eq!(command(&mut urgent, 1, OPEN), []);
    let mut recovered = None;
    for step in 0..200 {
        urgent.advance_tick().unwrap();
        if step == 20 {
            let snapshot = urgent.snapshot().unwrap();
            recovered = Some(
                ZoneSimulation::from_snapshot(snapshot, Arc::clone(urgent.content())).unwrap(),
            );
        }
        if let Some(copy) = recovered.as_mut()
            && step > 20
        {
            copy.advance_tick().unwrap();
            assert_eq!(copy.snapshot().unwrap(), urgent.snapshot().unwrap());
        }
    }
    assert_eq!(elden(&urgent), marshal_house::DOORWAY);
    assert!(door_open(&urgent), "he opened the door to answer");
    let turn_in = ZoneCommand::CompleteQuest {
        npc: ELDEN,
        quest: TROUBLE.get(),
        choice: 0,
    };
    assert_eq!(command(&mut urgent, 2, turn_in), []);
    let state = urgent.snapshot().unwrap();
    assert!(state.players[0].quests.is_completed(TROUBLE));
    // A minute later he closes the door and goes back to bed.
    for _ in 0..WAKE_IDLE_TICKS + 200 {
        urgent.advance_tick().unwrap();
    }
    assert!(!door_open(&urgent));
    assert_eq!(elden(&urgent), marshal_house::BEDSIDE);
    assert!(urgent.snapshot().unwrap().wakes.is_empty());
}
