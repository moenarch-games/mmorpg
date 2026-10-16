use std::sync::Arc;

use mmorpg_core::{
    MAX_CONTENT_COORDINATE_UNITS, MAX_PLAYERS_PER_ZONE, PLAYER_HALF_EXTENTS_UNITS, SpawnGrid,
    StaticCollider, ZoneCommand, ZoneDefinition, ZoneId, ZoneSimulation,
};

/// World-axis headings for forward movement: yaw 0 faces +Z, 90° faces +X.
const EAST: u16 = 16_384;
const WEST: u16 = 49_152;
const TOWARD_POSITIVE_Z: u16 = 0;
const TOWARD_NEGATIVE_Z: u16 = 32_768;

fn run(facing: u16) -> ZoneCommand {
    ZoneCommand::Move {
        forward: 1,
        strafe: 0,
        facing,
    }
}

fn definition() -> ZoneDefinition {
    ZoneDefinition::new(
        7,
        [0, -1, 0],
        vec![
            StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            },
            StaticCollider {
                id: 2,
                position: [300, 100, 0],
                half_extents: [20, 100, 300],
            },
        ],
    )
    .unwrap()
}

#[test]
fn shared_physics_resolves_gravity_ground_and_wall_contact() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition()).unwrap();
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, run(EAST)).unwrap();
    for _ in 0..100 {
        zone.advance_tick().unwrap();
    }
    let snapshot = zone.snapshot().unwrap();
    // The wall face at x = 280 stops the 30-unit half-width body; feet rest on y = 0.
    assert_eq!(snapshot.players[0].position, [250, 90, 0]);
    assert_eq!(snapshot.players[0].velocity, [0, 0, 0]);
    let projection = zone.snapshot_for_player(1).unwrap();
    assert_eq!(projection.content_revision, 7);
    assert_eq!(projection.acknowledged_sequence, 1);
}

#[test]
fn recovery_continues_airborne_motion_and_collision_identically() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition()).unwrap();
    zone.add_player(1).unwrap();
    let mut snapshot = zone.snapshot().unwrap();
    snapshot.players[0].position = [0, 400, 0];
    snapshot.players[0].velocity = [0, -7, 0];
    let mut original = ZoneSimulation::from_snapshot(snapshot, Arc::clone(zone.content())).unwrap();
    original.advance_tick().unwrap();
    assert!(original.snapshot().unwrap().players[0].velocity[1] < -7);
    let mut restored =
        ZoneSimulation::from_snapshot(original.snapshot().unwrap(), Arc::clone(original.content()))
            .unwrap();
    for _ in 0..50 {
        original.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(original.snapshot().unwrap(), restored.snapshot().unwrap());
    }
    assert_eq!(restored.snapshot().unwrap().players[0].position[1], 90);
}

#[test]
fn content_rejects_ambiguous_ids_invalid_extents_and_overflow() {
    let collider = StaticCollider {
        id: 1,
        position: [0, 0, 0],
        half_extents: [1, 1, 1],
    };
    assert!(ZoneDefinition::new(1, [0; 3], vec![collider.clone(), collider.clone()]).is_err());
    assert!(
        ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                id: 1_000_000,
                ..collider.clone()
            }]
        )
        .is_err()
    );
    assert!(
        ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                half_extents: [-1, 1, 1],
                ..collider.clone()
            }]
        )
        .is_err()
    );
    assert!(
        ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                position: [i32::MAX, 0, 0],
                ..collider
            }]
        )
        .is_err()
    );
}

#[test]
fn content_keeps_colliders_and_spawn_slots_in_the_compact_coordinate_range() {
    let limit = MAX_CONTENT_COORDINATE_UNITS;
    let edge = StaticCollider {
        id: 1,
        position: [limit - 10, 0, -limit + 10],
        half_extents: [10, 10, 10],
    };
    assert!(ZoneDefinition::new(1, [0; 3], vec![edge.clone()]).is_ok());
    for position in [[limit - 9, 0, 0], [0, -limit + 9, 0], [0, 0, limit]] {
        let error = ZoneDefinition::new(
            1,
            [0; 3],
            vec![StaticCollider {
                position,
                ..edge.clone()
            }],
        )
        .unwrap_err();
        assert_eq!(
            error.message(),
            "static collider lies outside the content coordinate range"
        );
    }
    // 512 slots in 32 columns: the last slot's feet are 31 and 15 spacings
    // out. The whole spawned body, not only its feet, must lie in the range.
    let [half_x, _, half_z] = PLAYER_HALF_EXTENTS_UNITS;
    let reaching = SpawnGrid {
        origin: [limit - half_x - 31 * 100, -limit + half_z],
        columns: 32,
        spacing: 100,
    };
    assert!(ZoneDefinition::with_spawn_grid(1, [0; 3], reaching, Vec::new()).is_ok());
    for origin in [
        [reaching.origin[0] + 1, reaching.origin[1]],
        [reaching.origin[0], reaching.origin[1] - 1],
    ] {
        let beyond = SpawnGrid { origin, ..reaching };
        assert_eq!(
            ZoneDefinition::with_spawn_grid(1, [0; 3], beyond, Vec::new())
                .unwrap_err()
                .message(),
            "spawn slot lies outside the content coordinate range"
        );
    }
}

#[test]
fn world_limits_hold_bodies_in_the_compact_range_on_unenclosed_content() {
    let limit = MAX_CONTENT_COORDINATE_UNITS;
    let [half_x, half_y, half_z] = PLAYER_HALF_EXTENTS_UNITS;
    // Default content has no colliders and no gravity. Each player runs along
    // one horizontal axis for longer than an `i16` position lasts at 21
    // units per tick (about 1,561 ticks).
    let mut open = ZoneSimulation::new(ZoneId::new(1));
    for (player_id, facing) in [
        (1, WEST),
        (2, EAST),
        (3, TOWARD_NEGATIVE_Z),
        (4, TOWARD_POSITIVE_Z),
    ] {
        open.add_player(player_id).unwrap();
        open.apply_command(player_id, 1, run(facing)).unwrap();
    }
    for _ in 0..1_600 {
        open.advance_tick().unwrap();
    }
    let players = open.snapshot().unwrap().players;
    let positions: Vec<_> = players.iter().map(|player| player.position).collect();
    assert_eq!(
        positions,
        [
            [-limit + half_x, 90, 0],
            [limit - half_x, 90, 0],
            [400, 90, -limit + half_z],
            [600, 90, limit - half_z],
        ]
    );
    // Recovery installs the same limits.
    let mut restored =
        ZoneSimulation::from_snapshot(open.snapshot().unwrap(), Arc::clone(open.content()))
            .unwrap();
    open.advance_tick().unwrap();
    restored.advance_tick().unwrap();
    assert_eq!(restored.snapshot().unwrap(), open.snapshot().unwrap());
    assert_eq!(open.snapshot().unwrap().players, players);

    // Gravity without ground or ceiling carries a body to the floor or ceiling limit.
    for (gravity, y) in [(-1, -limit + half_y), (1, limit - half_y)] {
        let definition = ZoneDefinition::new(1, [0, gravity, 0], Vec::new()).unwrap();
        let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
        zone.add_player(1).unwrap();
        for _ in 0..300 {
            zone.advance_tick().unwrap();
        }
        let player = &zone.snapshot().unwrap().players[0];
        assert_eq!(player.position, [0, y, 0], "gravity {gravity}");
        assert_eq!(player.velocity, [0, 0, 0], "gravity {gravity}");
    }
}

#[test]
fn every_spawn_slot_is_validated_clear_of_colliders() {
    let grid = SpawnGrid {
        origin: [-1_000, 2_000],
        columns: 16,
        spacing: 60,
    };
    let ground = StaticCollider {
        id: 1,
        position: [0, -50, 0],
        half_extents: [5_000, 50, 5_000],
    };
    let definition =
        ZoneDefinition::with_spawn_grid(3, [0, -1, 0], grid, vec![ground.clone()]).unwrap();
    assert_eq!(definition.spawn_grid(), grid);
    // The last slot of the configured capacity, and a crate on top of it.
    let last = u16::try_from(MAX_PLAYERS_PER_ZONE - 1).unwrap();
    let feet = grid.feet(last).unwrap();
    assert_eq!(feet, [-1_000 + 15 * 60, 0, 2_000 + 31 * 60]);
    let crate_on_last_slot = StaticCollider {
        id: 2,
        position: [feet[0] + 50, 100, feet[2]],
        half_extents: [25, 100, 25],
    };
    assert_eq!(
        ZoneDefinition::with_spawn_grid(
            3,
            [0, -1, 0],
            grid,
            vec![ground.clone(), crate_on_last_slot]
        )
        .unwrap_err()
        .message(),
        "spawn slot overlaps a static collider"
    );
    // Touching the body's side is allowed, like touching the ground.
    let touching = StaticCollider {
        id: 2,
        position: [feet[0] + 55, 100, feet[2]],
        half_extents: [25, 100, 25],
    };
    assert!(
        ZoneDefinition::with_spawn_grid(3, [0, -1, 0], grid, vec![ground.clone(), touching])
            .is_ok()
    );
    for invalid in [
        SpawnGrid { columns: 0, ..grid },
        SpawnGrid {
            spacing: 59,
            ..grid
        },
    ] {
        assert!(ZoneDefinition::with_spawn_grid(3, [0; 3], invalid, Vec::new()).is_err());
    }
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
    for id in 0..MAX_PLAYERS_PER_ZONE {
        zone.add_player(u32::try_from(id).unwrap()).unwrap();
    }
    let players = zone.snapshot().unwrap().players;
    assert_eq!(players[0].position, [-1_000, 90, 2_000]);
    assert_eq!(
        players[MAX_PLAYERS_PER_ZONE - 1].position,
        [feet[0], 90, feet[2]]
    );
    zone.advance_tick().unwrap();
    assert_eq!(
        zone.snapshot().unwrap().players,
        players,
        "a full grid rests"
    );
}

#[test]
fn exhausted_tick_does_not_move_physics() {
    let mut zone = ZoneSimulation::new(ZoneId::new(1));
    zone.add_player(1).unwrap();
    zone.apply_command(1, 1, run(EAST)).unwrap();
    let mut snapshot = zone.snapshot().unwrap();
    snapshot.tick = u64::MAX;
    let mut zone =
        ZoneSimulation::from_snapshot(snapshot.clone(), Arc::clone(zone.content())).unwrap();
    assert!(zone.advance_tick().is_err());
    assert_eq!(zone.snapshot().unwrap(), snapshot);
}

#[test]
fn physics_diagnostics_reuse_fixed_bounds_without_entering_recovery_state() {
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition()).unwrap();
    zone.add_player(1).unwrap();
    assert_eq!(zone.tick_work().physics, None);
    zone.advance_tick().unwrap();
    let bootstrap = zone.tick_work().physics.unwrap();
    assert_eq!(bootstrap.work.fixed_sweep_bound_preparations, 8);
    assert!(bootstrap.work.fixed_sweep_bound_reuses > 0);
    zone.advance_tick().unwrap();
    let warm = zone.tick_work().physics.unwrap();
    assert_eq!(warm.work.fixed_sweep_bound_preparations, 0);
    assert_eq!(warm.work.staged_state_capacity_growths, 0);
    assert_eq!(warm.work.broad_phase_capacity_growths, 0);
    assert!(warm.work.fixed_sweep_bound_reuses > 0);
    let snapshot = zone.snapshot().unwrap();
    let mut restored =
        ZoneSimulation::from_snapshot(snapshot.clone(), Arc::clone(zone.content())).unwrap();
    assert_eq!(restored.tick_work().physics, None);
    assert_eq!(restored.snapshot().unwrap(), snapshot);
    restored.advance_tick().unwrap();
    zone.advance_tick().unwrap();
    assert_eq!(restored.snapshot().unwrap(), zone.snapshot().unwrap());
    assert_eq!(
        restored
            .tick_work()
            .physics
            .unwrap()
            .work
            .fixed_sweep_bound_preparations,
        8
    );
}
