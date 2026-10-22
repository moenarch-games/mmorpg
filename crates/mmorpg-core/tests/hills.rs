//! The outer valley's hills (#159): authored terrain the zone's Rapier physics walks units on.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, cave::GREYFANG, hill_height, terrain};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

/// World-axis heading of forward movement toward +X.
const EAST: u16 = 16_384;

fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    let mut zone = zone;
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = hill_height(feet[0], feet[1]);
    state.players[0].position = mmorpg_core::physics::from_units([feet[0], ground + 120, feet[1]]);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn feet(zone: &ZoneSimulation) -> [f32; 3] {
    let [x, y, z] = zone.snapshot().unwrap().players[0].position;
    [x, y - 90.0, z]
}

#[test]
fn the_original_valley_is_flat_and_the_ring_has_gentle_hills() {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    let grid = definition.terrain().expect("Greyhaven has terrain");
    assert_eq!((grid.columns, grid.rows), (terrain::COUNT, terrain::COUNT));
    let mut tallest = 0;
    let mut steepest = 0_i64;
    for row in 0..i32::from(grid.rows) {
        for column in 0..i32::from(grid.columns) {
            let [x, z] = [
                terrain::ORIGIN[0] + column * terrain::STEP,
                terrain::ORIGIN[1] + row * terrain::STEP,
            ];
            let height =
                grid.heights[usize::try_from(row * i32::from(grid.columns) + column).unwrap()];
            // Stillwater Lake's basin (#274) is the one dip in the original valley.
            let basin = |x: i32| greyhaven_vale::lake_depth(x, z) > 0;
            if x.abs().max(z.abs()) <= 12_500 && !basin(x) {
                assert_eq!(height, 0, "the original valley stays flat at {x}, {z}");
            }
            tallest = tallest.max(height);
            // The cave's floor drops away under its walls, where nothing walks.
            let walled =
                |x: i32| GREYFANG.flattens(x, z) || GREYFANG.flattens(x + terrain::STEP, z);
            if column + 1 < i32::from(grid.columns)
                && !walled(x)
                && !basin(x)
                && !basin(x + terrain::STEP)
            {
                let next = grid.heights
                    [usize::try_from(row * i32::from(grid.columns) + column + 1).unwrap()];
                steepest = steepest.max(i64::from((next - height).abs()));
            }
        }
    }
    assert!(tallest >= 600, "hills rise to {tallest}");
    // At most a 30° rise between neighbouring samples: walkable for the controller (45°).
    assert!(
        steepest * 1_000 <= i64::from(terrain::STEP) * 577,
        "steepest step {steepest}"
    );
}

#[test]
fn a_player_stands_on_and_walks_over_the_hills() {
    // Find a hillside in Hollowpine Reach, clear of the woods' trunks: the ring road.
    let start = [-15_000, -6_000];
    let mut zone = player_at(start);
    for _ in 0..60 {
        zone.advance_tick().unwrap();
    }
    let rest = feet(&zone);
    let ground = hill_height(start[0], start[1]);
    assert!(
        (rest[1] - ground as f32).abs() <= 6.0,
        "feet {rest:?} on ground {ground}"
    );
    // Walk north along the ring road (toward −Z) across the slopes.
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: 32_768,
        },
    )
    .unwrap();
    let mut heights = Vec::new();
    for _ in 0..240 {
        zone.advance_tick().unwrap();
        heights.push(feet(&zone));
    }
    let end = feet(&zone);
    assert!(
        end[2] < rest[2] - 3_000.0,
        "walked uphill and down: {rest:?} -> {end:?}"
    );
    for at in heights.iter().step_by(10) {
        let ground = hill_height(at[0].round() as i32, at[2].round() as i32) as f32;
        assert!(
            (at[1] - ground).abs() <= 25.0,
            "feet {at:?} follow the ground {ground}"
        );
    }
    let _ = EAST;
}
