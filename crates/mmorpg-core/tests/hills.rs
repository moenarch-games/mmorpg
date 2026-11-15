//! The land round the Vale's heart (#159, #816): the region's designed landscape, joined to the
//! heart's authored rim without a seam, which the zone's physics walks units on.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, SPAWN_PLAZA, design, terrain};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

fn ground_at(x: i32, z: i32) -> i32 {
    greyhaven_vale::content().definition().ground_at(x, z)
}

fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
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

/// The heading from `from` toward `to`: 0 faces +Z, 16 384 faces +X.
fn heading(from: [f32; 3], to: [i32; 2]) -> u16 {
    let angle = (to[0] as f32 - from[0]).atan2(to[1] as f32 - from[2]);
    (angle.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU * 65_536.0) as u32 as u16
}

#[test]
fn the_outposts_floor_stays_level_and_the_land_round_the_heart_rolls_walkably() {
    let grid = greyhaven_vale::authored_terrain();
    assert_eq!((grid.columns, grid.rows), (terrain::COLUMNS, terrain::ROWS));
    let at = |column: i32, row: i32| {
        grid.heights[usize::try_from(row * i32::from(grid.columns) + column).unwrap()]
    };
    let (mut lowest, mut highest, mut steepest) = (i32::MAX, i32::MIN, 0_i64);
    for row in 0..i32::from(grid.rows) {
        for column in 0..i32::from(grid.columns) - 1 {
            let [x, z] = [
                terrain::ORIGIN[0] + column * terrain::STEP,
                terrain::ORIGIN[1] + row * terrain::STEP,
            ];
            if SPAWN_PLAZA.contains(x, z) {
                assert_eq!(at(column, row), 0, "the plaza stays level at {x}, {z}");
            }
            // The designed land round the heart, within the Vale's old square, off its rivers.
            let round_heart = x.abs().max(z.abs()) <= 20_400
                && greyhaven_vale::beyond_core(x, z) > 0
                && greyhaven_vale::beyond_core(x + terrain::STEP, z) > 0
                && design::river_distance(x, z) > 600
                && design::river_distance(x + terrain::STEP, z) > 600;
            if !round_heart {
                continue;
            }
            lowest = lowest.min(at(column, row));
            highest = highest.max(at(column, row));
            steepest = steepest.max(i64::from((at(column + 1, row) - at(column, row)).abs()));
        }
    }
    assert!(
        highest - lowest >= 600,
        "the land rolls {lowest}..{highest}"
    );
    // At most a 35° rise between neighbouring samples: walkable for the controller (45°).
    assert!(
        steepest * 1_000 <= i64::from(terrain::STEP) * 700,
        "steepest step {steepest}"
    );
}

#[test]
fn a_player_walks_from_the_heart_out_along_the_westmere_road() {
    let road = greyhaven_vale::roads()
        .iter()
        .find(|road| road.name == "Westmere Road")
        .expect("the Westmere Road leaves the heart")
        .points;
    // From the top of the Wolfrun Climb out across the heart's edge, about 150 m.
    let route: Vec<[i32; 2]> = road
        .iter()
        .copied()
        .take_while(|point| point[0] > -20_000)
        .collect();
    assert!(greyhaven_vale::beyond_core(route[0][0], route[0][1]) < 0);
    assert!(greyhaven_vale::beyond_core(route[route.len() - 1][0], route[route.len() - 1][1]) > 0);
    let mut zone = player_at(route[0]);
    let mut sequence = 0;
    for &goal in &route[1..] {
        let mut ticks = 0;
        loop {
            let at = feet(&zone);
            if (goal[0] as f32 - at[0]).hypot(goal[1] as f32 - at[2]) < 40.0 {
                break;
            }
            assert!(
                ticks < mmorpg_core::golden::t30(600),
                "stuck at {at:?} on the way to {goal:?}"
            );
            sequence += 1;
            zone.apply_command(
                1,
                sequence,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: heading(at, goal),
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
            ticks += 1;
            if ticks % 10 == 0 {
                let ground = ground_at(at[0].round() as i32, at[2].round() as i32) as f32;
                assert!(
                    (at[1] - ground).abs() <= 40.0,
                    "feet {at:?} follow the ground {ground}"
                );
            }
        }
    }
}
