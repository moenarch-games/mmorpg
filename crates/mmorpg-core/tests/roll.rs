//! The dodge roll (#516) through the public zone API: direction from held movement or straight
//! back, distance, the cooldown, and where a roll cannot start.
use std::sync::Arc;

use mmorpg_core::roll::{ROLL_COOLDOWN_TICKS, ROLL_TICKS};
use mmorpg_core::{
    Roll, StaticCollider, TraversalView, ZoneCommand, ZoneDefinition, ZoneId, ZoneSimulation,
};

/// Facing +x: yaw 16 384 points along `direction` = (sin, cos) = (1, 0).
const EAST: u16 = 16_384;

fn zone() -> ZoneSimulation {
    let ground = StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    };
    let definition = ZoneDefinition::new(mmorpg_core::STANDARD_GRAVITY, vec![ground]).unwrap();
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
    zone.add_player(1).unwrap();
    zone
}

struct Player {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Player {
    fn new() -> Self {
        let mut player = Self {
            zone: zone(),
            sequence: 0,
        };
        player.send(ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing: EAST,
        });
        player.ticks(5);
        player
    }

    fn send(&mut self, command: ZoneCommand) {
        self.sequence += 1;
        self.zone.apply_command(1, self.sequence, command).unwrap();
    }

    fn ticks(&mut self, count: u16) {
        for _ in 0..count {
            self.zone.advance_tick().unwrap();
        }
    }

    fn feet(&self) -> [i32; 3] {
        let state = &self.zone.snapshot().unwrap().players[0];
        mmorpg_core::physics::to_units(state.position)
    }

    fn roll(&self) -> Roll {
        self.zone.snapshot().unwrap().players[0].combat.roll
    }

    fn view(&self) -> TraversalView {
        self.zone.snapshot_for_player(1).unwrap().entities[0]
            .flags
            .traversal
    }
}

#[test]
fn standing_still_rolls_straight_back_about_three_and_a_half_metres() {
    let mut player = Player::new();
    let start = player.feet();
    player.send(ZoneCommand::Roll);
    assert_eq!(player.roll(), Roll::Pending);
    player.ticks(1);
    assert!(player.roll().rolling().is_some());
    assert_eq!(player.view(), TraversalView::Rolling);
    player.ticks(ROLL_TICKS);
    assert_eq!(player.roll(), Roll::Recovering { ticks: 0 });
    assert_eq!(player.view(), TraversalView::Free);
    let end = player.feet();
    // Facing east, so back is west.
    let moved = start[0] - end[0];
    assert!((340..=360).contains(&moved), "{start:?} → {end:?}");
    assert!((end[2] - start[2]).abs() <= 1, "{start:?} → {end:?}");
    player.ticks(10);
    assert_eq!(player.feet(), end, "the roll ends where it ends");
}

#[test]
fn a_roll_follows_the_held_movement_and_ignores_later_intent() {
    let mut player = Player::new();
    let start = player.feet();
    // The character's right is direction(facing − 90°): +z while facing east.
    player.send(ZoneCommand::Move {
        forward: 0,
        strafe: 1,
        facing: EAST,
    });
    player.send(ZoneCommand::Roll);
    player.ticks(1);
    player.send(ZoneCommand::Move {
        forward: 0,
        strafe: 0,
        facing: EAST,
    });
    player.send(ZoneCommand::Jump);
    player.ticks(ROLL_TICKS);
    let end = player.feet();
    assert!((start[0] - end[0]).abs() <= 1, "{start:?} → {end:?}");
    assert!(
        (340..=360).contains(&(end[2] - start[2])),
        "{start:?} → {end:?}"
    );
    assert_eq!(end[1], start[1], "a jump mid-roll does nothing");
}

#[test]
fn a_roll_cannot_be_chained_until_its_cooldown_ends() {
    let mut player = Player::new();
    player.send(ZoneCommand::Roll);
    player.ticks(1);
    // A press mid-roll is dropped, not buffered.
    player.send(ZoneCommand::Roll);
    player.ticks(ROLL_TICKS);
    assert!(matches!(player.roll(), Roll::Recovering { .. }));
    let resting = player.feet();
    player.send(ZoneCommand::Roll);
    player.ticks(ROLL_COOLDOWN_TICKS - 1);
    assert_eq!(player.feet(), resting, "no roll during the cooldown");
    player.ticks(1);
    assert_eq!(player.roll(), Roll::Ready);
    player.send(ZoneCommand::Roll);
    player.ticks(1);
    assert!(player.roll().rolling().is_some());
}

#[test]
fn an_airborne_press_is_dropped() {
    let mut player = Player::new();
    player.send(ZoneCommand::Jump);
    player.ticks(5);
    player.send(ZoneCommand::Roll);
    player.ticks(1);
    assert_eq!(player.roll(), Roll::Ready);
}

#[test]
fn a_restored_zone_continues_the_roll_exactly() {
    let mut player = Player::new();
    player.send(ZoneCommand::Roll);
    player.ticks(10);
    let snapshot = player.zone.snapshot().unwrap();
    let mut restored =
        ZoneSimulation::from_snapshot(snapshot, Arc::clone(player.zone.content())).unwrap();
    player.ticks(30);
    for _ in 0..30 {
        restored.advance_tick().unwrap();
    }
    assert_eq!(
        restored.snapshot().unwrap(),
        player.zone.snapshot().unwrap()
    );
}
