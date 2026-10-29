//! The Greyhaven inn door (#158): reach, blocking, idempotence, collision, recovery and
//! NPC service sight lines through the real hosted content.
use std::sync::Arc;

use mmorpg_core::{
    DoorView, ErrorCode, NpcId, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};

const DOOR: mmorpg_core::DoorId = greyhaven_vale::inn::DOOR;
const BRAM: NpcId = NpcId::new(3);
/// In front of the inn door, outside: x 14.95 m, z 6 m.
const OUTSIDE: [i32; 2] = [1_495, 600];

struct Zone {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Zone {
    fn new(feet: [i32; 2]) -> Self {
        let mut zone =
            ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
        zone.add_player(1).unwrap();
        let mut zone = Self { zone, sequence: 0 };
        zone.teleport(feet);
        zone
    }

    fn teleport(&mut self, feet: [i32; 2]) {
        let mut state = self.zone.snapshot().unwrap();
        state.players[0].position = mmorpg_core::physics::from_units([feet[0], 90, feet[1]]);
        state.players[0].velocity = [0.0; 3];
        self.zone = ZoneSimulation::from_snapshot(state, Arc::clone(self.zone.content())).unwrap();
    }

    /// Sends one command, advances one tick and returns the refusals it caused.
    fn act(&mut self, command: ZoneCommand) -> Vec<ErrorCode> {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
        self.zone.advance_tick().unwrap();
        self.errors()
    }

    fn errors(&self) -> Vec<ErrorCode> {
        self.zone
            .snapshot_for_player(1)
            .unwrap()
            .events
            .into_iter()
            .filter_map(|event| match event {
                ZoneEvent::Error { code, .. } => Some(code),
                _ => None,
            })
            .collect()
    }

    fn door_open(&self) -> bool {
        self.zone.snapshot_for_player(1).unwrap().doors[0]
            == DoorView {
                door: DOOR,
                open: true,
            }
    }

    fn walk(&mut self, facing: u16, ticks: u32) -> [i32; 3] {
        self.act(ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        });
        for _ in 1..ticks {
            self.zone.advance_tick().unwrap();
        }
        self.act(ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing,
        });
        mmorpg_core::physics::to_units(self.zone.snapshot().unwrap().players[0].position)
    }
}

const NORTH: u16 = 32_768;

#[test]
fn the_door_blocks_until_opened_within_reach_and_projects_to_everyone() {
    let mut zone = Zone::new([OUTSIDE[0], 1_200]);
    assert!(!zone.door_open());
    // Out of reach on the plaza.
    assert_eq!(
        zone.act(ZoneCommand::UseDoor {
            door: DOOR,
            open: true
        }),
        [ErrorCode::OutOfRange]
    );
    zone.teleport(OUTSIDE);
    // Closed, the leaf stops the walk at the doorway.
    let stopped = zone.walk(NORTH, 40);
    assert!(stopped[2] > 460, "stopped at {stopped:?}");
    zone.teleport(OUTSIDE);
    assert_eq!(
        zone.act(ZoneCommand::UseDoor {
            door: DOOR,
            open: true
        }),
        []
    );
    assert!(zone.door_open());
    // Asking for the state it has changes nothing.
    assert_eq!(
        zone.act(ZoneCommand::UseDoor {
            door: DOOR,
            open: true
        }),
        []
    );
    assert!(zone.door_open());
    let inside = zone.walk(NORTH, 40);
    assert!(inside[2] < 300, "walked in to {inside:?}");
    // Every player sees the same door.
    zone.zone.add_player(2).unwrap();
    assert!(
        zone.zone.snapshot_for_player(2).unwrap().doors[0].open,
        "a second player sees it open"
    );
}

#[test]
fn a_body_in_the_leafs_way_blocks_it_and_recovery_keeps_the_state() {
    // Standing in the doorway, the leaf cannot close onto the player.
    let mut zone = Zone::new(OUTSIDE);
    zone.act(ZoneCommand::UseDoor {
        door: DOOR,
        open: true,
    });
    zone.teleport([1_500, 453]);
    assert!(zone.door_open(), "recovery keeps the open door");
    assert_eq!(
        zone.act(ZoneCommand::UseDoor {
            door: DOOR,
            open: false
        }),
        [ErrorCode::DoorBlocked]
    );
    assert!(zone.door_open());
    zone.teleport(OUTSIDE);
    assert_eq!(
        zone.act(ZoneCommand::UseDoor {
            door: DOOR,
            open: false
        }),
        []
    );
    assert!(!zone.door_open());
    let state = zone.zone.snapshot().unwrap();
    assert_eq!(state.doors.len(), 2, "the inn and Elden's house");
    assert!(!state.doors[0].open && state.doors[0].changed_tick > 0);
}

#[test]
fn bram_serves_across_his_bar_but_never_through_the_inn_walls() {
    let sell = ZoneCommand::SellItem {
        npc: BRAM,
        bag_slot: 0,
        quantity: 1,
    };
    // Behind the back wall, 1.5 m from Bram.
    let mut zone = Zone::new([1_920, -530]);
    assert_eq!(zone.act(sell), [ErrorCode::OutOfRange]);
    // In front of the bar, across the counter.
    zone.teleport([1_920, -180]);
    assert_eq!(zone.act(sell), []);
}
