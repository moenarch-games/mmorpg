//! Recovery through the canonical wire format must continue movement, jumps and
//! pending intent exactly, tick by tick.
use std::sync::Arc;

use mmorpg_core::{StaticCollider, ZoneCommand, ZoneDefinition, ZoneId, ZoneSimulation};
use mmorpg_protocol::{decode_canonical_snapshot, encode_canonical_snapshot, encode_snapshot};

fn definition() -> ZoneDefinition {
    ZoneDefinition::new(
        11,
        mmorpg_core::STANDARD_GRAVITY,
        vec![
            StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            },
            StaticCollider {
                id: 2,
                position: [900, 150, 0],
                half_extents: [30, 150, 2_000],
            },
        ],
    )
    .unwrap()
}

fn recover(zone: &ZoneSimulation) -> ZoneSimulation {
    let bytes = encode_canonical_snapshot(&zone.snapshot().unwrap()).unwrap();
    let recovered = ZoneSimulation::from_snapshot(
        decode_canonical_snapshot(&bytes).unwrap(),
        Arc::clone(zone.content()),
    )
    .expect("canonical bytes restore a zone");
    assert_eq!(recovered.snapshot().unwrap(), zone.snapshot().unwrap());
    recovered
}

/// Applies the same commands to both zones, then compares every tick.
fn assert_identical_continuation(
    original: &mut ZoneSimulation,
    recovered: &mut ZoneSimulation,
    ticks: usize,
    commands: &[(usize, u32, u32, ZoneCommand)],
) {
    for tick in 0..ticks {
        for &(at, player, sequence, command) in commands {
            if at == tick {
                original.apply_command(player, sequence, command).unwrap();
                recovered.apply_command(player, sequence, command).unwrap();
            }
        }
        original.advance_tick().unwrap();
        recovered.advance_tick().unwrap();
        let expected = original.snapshot().unwrap();
        assert_eq!(recovered.snapshot().unwrap(), expected, "tick {tick}");
        for player in &expected.players {
            assert_eq!(
                encode_snapshot(&recovered.snapshot_for_player(player.player_id).unwrap()),
                encode_snapshot(&original.snapshot_for_player(player.player_id).unwrap()),
            );
        }
    }
}

#[test]
fn recovery_mid_run_and_mid_jump_reproduces_the_continuation_exactly() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(3), definition()).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    // Player 1 runs forward-right toward the wall; player 2 backpedals.
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 1,
            facing: 20_000,
        },
    )
    .unwrap();
    zone.apply_command(
        2,
        1,
        ZoneCommand::Move {
            forward: -1,
            strafe: 0,
            facing: 50_000,
        },
    )
    .unwrap();
    for _ in 0..mmorpg_core::golden::t30(3) {
        zone.advance_tick().unwrap();
    }

    // Checkpoint with a pending, not yet evaluated jump.
    zone.apply_command(1, 2, ZoneCommand::Jump).unwrap();
    assert!(zone.snapshot().unwrap().players[0].jump_pending);
    let mut recovered = recover(&zone);
    assert_identical_continuation(&mut zone, &mut recovered, 4, &[]);
    let rising = zone.snapshot().unwrap();
    assert!(
        rising.players[0].velocity[1] > 0.0,
        "mid-jump while running"
    );
    assert!(!rising.players[0].jump_pending);

    // Checkpoint while rising; later inputs change direction and jump again.
    let mut recovered = recover(&zone);
    assert_identical_continuation(
        &mut zone,
        &mut recovered,
        60,
        &[
            (2, 1, 3, ZoneCommand::Jump),
            (
                5,
                1,
                4,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: -1,
                    facing: 20_000,
                },
            ),
            (8, 2, 2, ZoneCommand::Jump),
            (40, 1, 5, ZoneCommand::Jump),
        ],
    );

    // Checkpoint while falling back toward the ground.
    let falling = zone.snapshot().unwrap();
    assert!(
        falling
            .players
            .iter()
            .any(|player| player.velocity[1] < 0.0)
    );
    let mut recovered = recover(&zone);
    assert_identical_continuation(&mut zone, &mut recovered, 40, &[]);
    for player in zone.snapshot().unwrap().players {
        assert_eq!(
            mmorpg_core::physics::to_units(player.position)[1],
            90,
            "both players land on the ground"
        );
        assert_eq!(player.velocity[1], 0.0);
    }
}
