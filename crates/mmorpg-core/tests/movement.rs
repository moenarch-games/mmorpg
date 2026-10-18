//! Movement v3 semantics through the public zone API: facing-relative intent,
//! run and backpedal speeds, grounded jumps, and fail-closed sequencing.
use std::sync::Arc;

use mmorpg_core::{
    BACKPEDAL_SPEED_UNITS_PER_TICK, JUMP_VELOCITY_UNITS_PER_TICK, PLAYER_HALF_EXTENTS_UNITS,
    RUN_SPEED_UNITS_PER_TICK, StaticCollider, ZoneCommand, ZoneDefinition, ZoneId, ZoneSimulation,
};

const REST_Y: i32 = PLAYER_HALF_EXTENTS_UNITS[1];

fn ground(extra: Vec<StaticCollider>) -> ZoneDefinition {
    let mut colliders = vec![StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [10_000, 50, 10_000],
    }];
    colliders.extend(extra);
    ZoneDefinition::new(5, [0, -1, 0], colliders).unwrap()
}

/// The player's body as gameplay rules see it: whole units (#271).
#[derive(Clone, Copy, Debug)]
struct Body {
    position: [i32; 3],
    velocity: [i32; 3],
    jump_pending: bool,
}

fn player(zone: &ZoneSimulation) -> Body {
    let state = &zone.snapshot().unwrap().players[0];
    Body {
        position: mmorpg_core::physics::to_units(state.position),
        velocity: mmorpg_core::physics::to_units(state.velocity),
        jump_pending: state.jump_pending,
    }
}

fn one_tick_velocity(forward: i8, strafe: i8, facing: u16) -> [i32; 3] {
    // No gravity and no colliders: one tick moves the body by its velocity.
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    let before = player(&zone).position;
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward,
            strafe,
            facing,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    let after = player(&zone);
    assert_eq!(
        std::array::from_fn(|axis| after.position[axis] - before[axis]),
        after.velocity
    );
    after.velocity
}

/// Floating-point reference built from the forward and right vectors,
/// independent of the production eight-way offset table.
fn reference_velocity(forward: i8, strafe: i8, facing: u16) -> (f64, f64) {
    let angle = f64::from(facing) * std::f64::consts::TAU / 65_536.0;
    let forward_vector = (angle.sin(), angle.cos());
    // The character's right is direction(facing - 90°) = (-cos, sin).
    let right_vector = (-angle.cos(), angle.sin());
    let x = f64::from(forward) * forward_vector.0 + f64::from(strafe) * right_vector.0;
    let z = f64::from(forward) * forward_vector.1 + f64::from(strafe) * right_vector.1;
    let length = x.hypot(z);
    let speed = if forward < 0 {
        BACKPEDAL_SPEED_UNITS_PER_TICK
    } else {
        RUN_SPEED_UNITS_PER_TICK
    };
    (f64::from(speed) * x / length, f64::from(speed) * z / length)
}

#[test]
fn eight_directions_follow_facing_at_run_and_backpedal_speed() {
    for facing in [0, 1, 4_321, 8_192, 16_384, 30_000, 32_768, 45_000, 65_535] {
        for forward in -1..=1_i8 {
            for strafe in -1..=1_i8 {
                if (forward, strafe) == (0, 0) {
                    continue;
                }
                let velocity = one_tick_velocity(forward, strafe, facing);
                let (expected_x, expected_z) = reference_velocity(forward, strafe, facing);
                let context = format!("forward {forward}, strafe {strafe}, facing {facing}");
                // Q16 trig adds at most 21 / 2^17 before rounding to whole units.
                let tolerance = 0.5 + 1e-3;
                assert!(
                    (f64::from(velocity[0]) - expected_x).abs() <= tolerance,
                    "{context}: {velocity:?} vs ({expected_x}, {expected_z})"
                );
                assert!(
                    (f64::from(velocity[2]) - expected_z).abs() <= tolerance,
                    "{context}: {velocity:?} vs ({expected_x}, {expected_z})"
                );
                assert_eq!(velocity[1], 0, "{context}");
                let speed = f64::from(velocity[0]).hypot(f64::from(velocity[2]));
                let nominal = if forward < 0 {
                    BACKPEDAL_SPEED_UNITS_PER_TICK
                } else {
                    RUN_SPEED_UNITS_PER_TICK
                };
                assert!(
                    (speed - f64::from(nominal)).abs() <= 0.75,
                    "{context}: {speed}"
                );
            }
        }
    }
}

#[test]
fn facing_convention_names_forward_right_and_backpedal() {
    // Yaw 0 faces +Z; its right is -X.
    assert_eq!(one_tick_velocity(1, 0, 0), [0, 0, 21]);
    assert_eq!(one_tick_velocity(0, 1, 0), [-21, 0, 0]);
    assert_eq!(one_tick_velocity(0, -1, 0), [21, 0, 0]);
    assert_eq!(one_tick_velocity(-1, 0, 0), [0, 0, -13]);
    // A quarter turn faces +X; its right is +Z.
    assert_eq!(one_tick_velocity(1, 0, 16_384), [21, 0, 0]);
    assert_eq!(one_tick_velocity(0, 1, 16_384), [0, 0, 21]);
    // Diagonals keep the run speed rather than adding the two axes.
    assert_eq!(one_tick_velocity(1, 1, 0), [-15, 0, 15]);
    assert_eq!(one_tick_velocity(-1, -1, 0), [9, 0, -9]);
}

#[test]
fn zero_intent_stops_horizontally_but_gravity_keeps_vertical_velocity() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), ground(Vec::new())).unwrap();
    zone.add_player(1).unwrap();
    let mut airborne = zone.snapshot().unwrap();
    airborne.players[0].position = mmorpg_core::physics::from_units([0, 1_000, 0]);
    airborne.players[0].velocity = mmorpg_core::physics::from_units([21, -3, 21]);
    let mut zone = ZoneSimulation::from_snapshot(airborne, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    let falling = player(&zone);
    assert_eq!(falling.velocity, [0, -4, 0]);
    assert_eq!(falling.position, [0, 996, 0]);
}

#[test]
fn a_grounded_jump_rises_to_its_apex_and_lands() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), ground(Vec::new())).unwrap();
    zone.add_player(1).unwrap();
    assert_eq!(player(&zone).position[1], REST_Y, "feet rest on y = 0");
    zone.apply_command(1, 1, ZoneCommand::Jump).unwrap();
    assert!(player(&zone).jump_pending);

    zone.advance_tick().unwrap();
    let first = player(&zone);
    assert!(!first.jump_pending, "the tick consumes the intent");
    assert_eq!(first.velocity[1], JUMP_VELOCITY_UNITS_PER_TICK - 1);
    let mut apex = first.position[1];
    let mut landed_after = None;
    for tick in 2..=60 {
        zone.advance_tick().unwrap();
        let state = player(&zone);
        apex = apex.max(state.position[1]);
        if state.position[1] == REST_Y && state.velocity[1] == 0 && landed_after.is_none() {
            landed_after = Some(tick);
        }
    }
    // Gravity of one unit per tick: 15 + 14 + … + 1 units of rise.
    assert_eq!(apex, REST_Y + 120);
    assert!(
        landed_after.is_some_and(|tick| tick <= 33),
        "{landed_after:?}"
    );
    assert_eq!(player(&zone).position[1], REST_Y);

    // Landing restores the ability to jump.
    zone.apply_command(1, 2, ZoneCommand::Jump).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(player(&zone).velocity[1], JUMP_VELOCITY_UNITS_PER_TICK - 1);
}

#[test]
fn a_mid_air_jump_is_consumed_without_effect() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), ground(Vec::new())).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, ZoneCommand::Jump).unwrap();
    for _ in 0..5 {
        zone.advance_tick().unwrap();
    }
    let rising = player(&zone);
    assert!(rising.position[1] > REST_Y);

    zone.apply_command(1, 2, ZoneCommand::Jump).unwrap();
    zone.advance_tick().unwrap();
    let after = player(&zone);
    assert_eq!(after.velocity[1], rising.velocity[1] - 1, "no double jump");
    assert!(!after.jump_pending, "no buffering until landing");
    for _ in 0..40 {
        zone.advance_tick().unwrap();
    }
    let landed = player(&zone);
    assert_eq!(landed.position[1], REST_Y);
    assert_eq!(
        landed.velocity[1], 0,
        "the ignored jump never fires on landing"
    );
}

#[test]
fn touching_only_a_wall_does_not_allow_a_jump() {
    // Zero gravity: the body hovers flush against a wall that extends below it.
    let wall = StaticCollider {
        id: 1,
        position: [80, 0, 0],
        half_extents: [50, 1_000, 1_000],
    };
    let definition = ZoneDefinition::new(5, [0, 0, 0], vec![wall]).unwrap();
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
    zone.add_player(1).unwrap();
    assert_eq!(player(&zone).position, [0, REST_Y, 0]);
    // Push into the wall as well; physics keeps the body at the contact plane.
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 16_384,
        },
    )
    .unwrap();
    zone.advance_tick().unwrap();
    zone.apply_command(1, 2, ZoneCommand::Jump).unwrap();
    zone.advance_tick().unwrap();
    let state = player(&zone);
    assert_eq!(state.position, [0, REST_Y, 0]);
    assert_eq!(state.velocity[1], 0);
    assert!(!state.jump_pending);
}

#[test]
fn a_falling_body_beside_a_wall_cannot_jump() {
    let wall = StaticCollider {
        id: 2,
        position: [80, 1_000, 0],
        half_extents: [50, 1_000, 1_000],
    };
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), ground(vec![wall])).unwrap();
    zone.add_player(1).unwrap();
    let mut airborne = zone.snapshot().unwrap();
    airborne.players[0].position = mmorpg_core::physics::from_units([0, 1_500, 0]);
    let mut zone = ZoneSimulation::from_snapshot(airborne, Arc::clone(zone.content())).unwrap();
    zone.advance_tick().unwrap();
    let before = player(&zone);
    zone.apply_command(1, 1, ZoneCommand::Jump).unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(player(&zone).velocity[1], before.velocity[1] - 1);
}

/// Another unit's body is no ground (#271, as in ARPG): a player resting on another player's
/// capsule cannot jump off it.
#[test]
fn another_player_is_no_ground_to_jump_from() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), ground(Vec::new())).unwrap();
    zone.add_player(1).unwrap();
    zone.add_player(2).unwrap();
    let mut stacked = zone.snapshot().unwrap();
    stacked.players[1].position = mmorpg_core::physics::from_units([0, 3 * REST_Y, 0]);
    let mut zone = ZoneSimulation::from_snapshot(stacked, Arc::clone(zone.content())).unwrap();
    zone.apply_command(2, 1, ZoneCommand::Jump).unwrap();
    zone.advance_tick().unwrap();
    let top = zone.snapshot().unwrap().players[1].clone();
    assert!(top.velocity[1] < 0.0, "{:?}", top.velocity);
}

#[test]
fn stale_and_duplicate_sequences_fail_closed_for_every_command() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), ground(Vec::new())).unwrap();
    zone.add_player(1).unwrap();
    let run = ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing: 0,
    };
    zone.apply_command(1, 10, run).unwrap();
    for (sequence, command) in [(10, ZoneCommand::Jump), (9, run), (0, ZoneCommand::Jump)] {
        assert_eq!(
            zone.apply_command(1, sequence, command)
                .unwrap_err()
                .message(),
            "command sequence is stale"
        );
    }
    zone.advance_tick().unwrap();
    assert_eq!(
        player(&zone).position[1],
        REST_Y,
        "the stale jump never fired"
    );
}
