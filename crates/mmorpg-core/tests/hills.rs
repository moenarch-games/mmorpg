//! The outer valley's hills (#159): authored terrain the zone's Rapier physics walks units on.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, cave::GREYFANG, shrine, terrain};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

/// The physical ground: the hills, or the basin's rim descending into them (#423).
fn ground_at(x: i32, z: i32) -> i32 {
    greyhaven_vale::content().definition().ground_at(x, z)
}

/// World-axis heading of forward movement toward +X.
const EAST: u16 = 16_384;

fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    let mut zone = zone;
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = ground_at(feet[0], feet[1]);
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
    assert_eq!((grid.columns, grid.rows), (terrain::COLUMNS, terrain::ROWS));
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
            // Stillwater Lake's basin and the Sunken Shrine's cave under its shore (#274) are the
            // only dips in the original valley; Millbrook Farm's terrace and its cliffs (#423)
            // its only rise.
            let basin = |x: i32| {
                greyhaven_vale::lake_depth(x, z) > 0
                    || shrine::flattens(x, z)
                    || greyhaven_vale::basin::height_at(x, z).is_some()
            };
            // The farmland's ridge and plateau (#424), the highlands' escarpment and high
            // ground (#425) and the wetlands' ridge and lowland (#426) beyond the Vale are not
            // the ring's hills.
            if z >= greyhaven_vale::farmland::RIDGE[0] - terrain::STEP
                || z <= greyhaven_vale::highlands::ESCARPMENT[0] + terrain::STEP
                || x >= greyhaven_vale::marsh::RIDGE[0] - terrain::STEP
            {
                continue;
            }
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
    for _ in 0..mmorpg_core::golden::t30(60) {
        zone.advance_tick().unwrap();
    }
    let rest = feet(&zone);
    let ground = ground_at(start[0], start[1]);
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
    for _ in 0..mmorpg_core::golden::t30(240) {
        zone.advance_tick().unwrap();
        heights.push(feet(&zone));
    }
    let end = feet(&zone);
    assert!(
        end[2] < rest[2] - 3_000.0,
        "walked uphill and down: {rest:?} -> {end:?}"
    );
    for at in heights.iter().step_by(10) {
        let ground = ground_at(at[0].round() as i32, at[2].round() as i32) as f32;
        assert!(
            (at[1] - ground).abs() <= 25.0,
            "feet {at:?} follow the ground {ground}"
        );
    }
    let _ = EAST;
}
