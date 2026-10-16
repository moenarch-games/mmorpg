#[path = "support/visibility_oracle.rs"]
mod visibility_oracle;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, INTEREST_RADIUS_UNITS,
    MAX_CONTENT_COORDINATE_UNITS, ZoneCommand, ZoneDefinition, ZoneId, ZoneSimulation,
    greyhaven_vale_definition,
};
use mmorpg_protocol::encode_snapshot;
use visibility_oracle::exhaustive_projection;

/// Interest radius; cell width equals it, so layouts below scale with it.
const R: i32 = INTEREST_RADIUS_UNITS;

/// World-axis headings for forward movement: yaw 0 faces +Z, 90° faces +X.
const EAST: u16 = 16_384;
const SOUTH: u16 = 32_768;
const SOUTH_WEST: u16 = 40_960;

fn run(facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    }
}

fn assert_wire_parity(zone: &ZoneSimulation) {
    let canonical = zone.snapshot().unwrap();
    for observer in &canonical.players {
        let expected = exhaustive_projection(&canonical, observer);
        let actual = zone.snapshot_for_player(observer.player_id).unwrap();
        assert_eq!(actual, expected, "observer {}", observer.player_id);
        // Positions beyond the compact wire range fail closed identically.
        assert_eq!(
            encode_snapshot(&actual),
            encode_snapshot(&expected),
            "observer {} at tick {}",
            observer.player_id,
            canonical.tick
        );
    }
}

fn zone_at(positions: &[[i32; 3]], definition: ZoneDefinition) -> ZoneSimulation {
    let empty = ZoneSimulation::with_definition(ZoneId::new(9), definition).unwrap();
    let mut canonical = empty.snapshot().unwrap();
    canonical.players = positions
        .iter()
        .enumerate()
        .map(|(index, &position)| CanonicalPlayerSnapshot {
            copper: 0,
            player_id: u32::try_from(positions.len() - index).unwrap(),
            position,
            velocity: [0; 3],
            facing: 0,
            forward: 0,
            strafe: 0,
            jump_pending: false,
            last_sequence: 3,
            spawn_slot: u16::try_from(index).unwrap(),
            inventory: mmorpg_core::Inventory::default(),
            inventory_revision: 1,
            inventory_changed_at: 0,
            equipment: mmorpg_core::Equipment::default(),
            combat: CanonicalPlayerCombat::default(),
            chat_ready_at: 0,
            chat: Vec::new(),
            quests: mmorpg_core::QuestLog::default(),
            quests_changed_at: 0,
        })
        .collect();
    ZoneSimulation::from_snapshot(canonical, Arc::clone(empty.content())).unwrap()
}

#[test]
fn multi_tick_membership_transitions_match_exhaustive_wire_output() {
    let mut zone = zone_at(
        &[
            [R - 10, 50, 0],
            [-R + 1, 50, -R + 1],
            [3 * R, 50, 3 * R],
            [0, 50, R],
            [3 * R / 5, 50, 4 * R / 5],
        ],
        ZoneDefinition::default(),
    );
    assert_wire_parity(&zone);
    zone.advance_tick().unwrap(); // stationary
    assert_wire_parity(&zone);
    zone.apply_command(3, 4, run(EAST)).unwrap(); // same cell
    zone.advance_tick().unwrap();
    assert_wire_parity(&zone);
    zone.apply_command(5, 4, run(EAST)).unwrap();
    zone.apply_command(4, 4, run(SOUTH_WEST)).unwrap(); // crosses both negative axes
    for tick in 0..18 {
        zone.advance_tick().unwrap();
        assert_wire_parity(&zone);
        if tick == 6 {
            let checkpoint = zone.snapshot().unwrap();
            zone = ZoneSimulation::from_snapshot(checkpoint.clone(), Arc::clone(zone.content()))
                .unwrap();
            assert_eq!(zone.snapshot().unwrap(), checkpoint);
            assert_wire_parity(&zone);
        }
    }
    assert!(zone.remove_player(2));
    assert_wire_parity(&zone);
    zone.add_player(2).unwrap();
    assert_wire_parity(&zone);
    zone.advance_tick().unwrap();
    assert_wire_parity(&zone);
}

#[test]
fn collision_corrections_and_failed_step_preserve_reference_parity() {
    // Player 2 runs from the hub plaza into the keep's south face at z = 700.
    let mut colliding = zone_at(
        &[[-1_550, 90, 1_250], [450, 90, 1_250]],
        greyhaven_vale_definition(),
    );
    colliding.apply_command(2, 4, run(SOUTH)).unwrap();
    for _ in 0..120 {
        colliding.advance_tick().unwrap();
        assert_wire_parity(&colliding);
    }
    assert_eq!(
        colliding
            .snapshot()
            .unwrap()
            .players
            .iter()
            .find(|player| player.player_id == 2)
            .unwrap()
            .position,
        [-1_550, 90, 730],
        "the shared physics collider must correct the nominal movement"
    );

    let mut edge = zone_at(
        &[[i32::MAX - 50, 50, 0], [i32::MIN + 31, 50, 0]],
        ZoneDefinition::default(),
    );
    edge.apply_command(2, 4, run(EAST)).unwrap();
    edge.advance_tick().unwrap();
    edge.advance_tick().unwrap();
    let work = edge.interest_maintenance_stats();
    assert!(edge.advance_tick().is_err());
    assert_eq!(edge.interest_maintenance_stats(), work);
    assert_wire_parity(&edge);
    assert_eq!(
        encode_snapshot(&edge.snapshot_for_player(1).unwrap())
            .unwrap_err()
            .to_string(),
        "entity position is outside the compact wire range",
        "unrepresentable positions never reach the wire"
    );
}

#[test]
fn players_running_on_unenclosed_content_stay_encodable() {
    // Default content has no walls, no ground and no gravity. Two players who
    // see each other run east for longer than an `i16` position lasts at 21
    // units per tick (about 1,561 ticks); the world limits stop them first.
    let mut zone = ZoneSimulation::new(ZoneId::new(9));
    for player_id in [1, 2] {
        zone.add_player(player_id).unwrap();
        zone.apply_command(player_id, 1, run(EAST)).unwrap();
    }
    for _ in 0..1_600 {
        zone.advance_tick().unwrap();
        for viewer in [1, 2] {
            let projection = zone.snapshot_for_player(viewer).unwrap();
            assert_eq!(projection.entities.len(), 2);
            encode_snapshot(&projection).unwrap();
        }
    }
    let positions: Vec<_> = zone
        .snapshot()
        .unwrap()
        .players
        .iter()
        .map(|player| player.position)
        .collect();
    // Player 2 spawned 2 m ahead and rests against the limit; player 1 rests against it.
    let front = MAX_CONTENT_COORDINATE_UNITS - 30;
    assert_eq!(positions, [[front - 60, 90, 0], [front, 90, 0]]);
    assert_wire_parity(&zone);
}
