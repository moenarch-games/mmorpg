//! Stillwater Lake's water (#274, Atlas 5.1.41–5.1.42): wading, swimming, diving, breath
//! and drowning in the deepened basin.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, lake, lake_depth, shrine, terrain};
use mmorpg_core::water::{
    BREATH_MAX_TICKS, BREATH_REFILL_PER_TICK, DROWN_INTERVAL_TICKS, FLOAT_DEPTH_UNITS,
};
use mmorpg_core::{WaterMode, ZoneCommand, ZoneId, ZoneSimulation};

/// World-axis heading of forward movement toward +X.
const EAST: u16 = 16_384;

fn zone_with_player_at(centre: [i32; 3]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units(centre);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn feet_y(zone: &ZoneSimulation) -> f32 {
    zone.snapshot().unwrap().players[0].position[1] - 90.0
}

fn water(zone: &ZoneSimulation) -> WaterMode {
    let position = zone.snapshot_for_player(1).unwrap().entities[0].position;
    WaterMode::of_player(zone.content().waters(), position)
}

fn breath(zone: &ZoneSimulation) -> u16 {
    zone.snapshot_for_player(1).unwrap().viewer.breath
}

fn run(zone: &mut ZoneSimulation, ticks: u32) {
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
    }
}

#[test]
fn the_basin_is_about_nine_metres_deep_with_walkable_banks() {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    let grid = definition.terrain().unwrap();
    let height = |column: i32, row: i32| {
        grid.heights[usize::try_from(row * i32::from(grid.columns) + column).unwrap()]
    };
    let mut deepest = 0;
    for row in 0..i32::from(grid.rows) - 1 {
        for column in 0..i32::from(grid.columns) - 1 {
            let [x, z] = [
                terrain::ORIGIN[0] + column * terrain::STEP,
                terrain::ORIGIN[1] + row * terrain::STEP,
            ];
            if lake_depth(x, z) == 0 && lake_depth(x + terrain::STEP, z + terrain::STEP) == 0 {
                continue;
            }
            deepest = deepest.max(-height(column, row));
            // The Sunken Shrine's sunken plaza and cave floor drop away under its walls.
            let step = terrain::STEP;
            if [[x, z], [x + step, z], [x, z + step]]
                .iter()
                .any(|&[x, z]| shrine::flattens(x, z))
            {
                continue;
            }
            for next in [height(column + 1, row), height(column, row + 1)] {
                let rise = (next - height(column, row)).abs();
                // Steeper than the hills, but within the controller's 45° climb (tan 40°).
                assert!(
                    rise * 1_000 <= terrain::STEP * 839,
                    "bank too steep at {x}, {z}"
                );
            }
        }
    }
    assert!((800..=1_000).contains(&deepest), "deepest {deepest}");
    assert_eq!(
        lake_depth(lake::CENTRE[0] + lake::RADII[0], lake::CENTRE[1]),
        0
    );
}

#[test]
fn a_player_wades_in_and_swims_at_the_surface_without_touching_the_floor() {
    // West shore, on the lake's long axis.
    let mut zone = zone_with_player_at([lake::CENTRE[0] - 2_600, 90, lake::CENTRE[1]]);
    run(&mut zone, 5);
    zone.apply_command(
        1,
        1,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: EAST,
        },
    )
    .unwrap();
    let mut modes = Vec::new();
    for _ in 0..240 {
        zone.advance_tick().unwrap();
        let mode = water(&zone);
        if modes.last() != Some(&mode) {
            modes.push(mode);
        }
        if mode == WaterMode::Swimming
            && feet_y(&zone) <= (lake::SURFACE - FLOAT_DEPTH_UNITS) as f32 + 1.0
        {
            break;
        }
    }
    assert_eq!(
        modes,
        [WaterMode::Dry, WaterMode::Wading, WaterMode::Swimming],
        "shore, wading, swimming"
    );
    // Floating: the feet hold 1.4 m under the surface, far above the 9 m floor.
    run(&mut zone, 60);
    let [x, _, z] = zone.snapshot().unwrap().players[0].position;
    assert!(lake_depth(x as i32, z as i32) > 300, "out over deep water");
    assert!(
        (feet_y(&zone) - (lake::SURFACE - FLOAT_DEPTH_UNITS) as f32).abs() <= 2.0,
        "feet at {}",
        feet_y(&zone)
    );
    assert_eq!(water(&zone), WaterMode::Swimming);
    assert_eq!(breath(&zone), BREATH_MAX_TICKS);
}

#[test]
fn diving_spends_breath_drowning_hurts_and_surfacing_refills() {
    let surface_feet = lake::SURFACE - FLOAT_DEPTH_UNITS;
    let mut zone = zone_with_player_at([lake::CENTRE[0], surface_feet + 90, lake::CENTRE[1]]);
    run(&mut zone, 2);
    assert_eq!(water(&zone), WaterMode::Swimming);
    zone.apply_command(1, 1, ZoneCommand::Rise(-1)).unwrap();
    run(&mut zone, 30);
    assert_eq!(water(&zone), WaterMode::Submerged);
    let dived = feet_y(&zone);
    assert!(dived < (surface_feet - 200) as f32, "dived to {dived}");
    // Level: the depth holds without input.
    zone.apply_command(1, 2, ZoneCommand::Rise(0)).unwrap();
    run(&mut zone, 10);
    assert!((feet_y(&zone) - dived).abs() <= 1.0);
    let left = breath(&zone);
    assert!(left < BREATH_MAX_TICKS);
    // Out of breath, then one drowning interval.
    run(&mut zone, u32::from(left) + u32::from(DROWN_INTERVAL_TICKS));
    let viewer = zone.snapshot_for_player(1).unwrap().viewer;
    assert_eq!(viewer.breath, 0);
    assert_eq!(viewer.health, viewer.max_health - viewer.max_health / 10);
    // Recovery continues the drowning clock exactly.
    let state = zone.snapshot().unwrap();
    let mut restored = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    run(&mut restored, u32::from(DROWN_INTERVAL_TICKS));
    run(&mut zone, u32::from(DROWN_INTERVAL_TICKS));
    assert_eq!(restored.snapshot().unwrap(), zone.snapshot().unwrap());
    // Surface: breath refills and the drowning stops.
    zone.apply_command(1, 3, ZoneCommand::Rise(1)).unwrap();
    run(&mut zone, 60);
    assert_eq!(water(&zone), WaterMode::Swimming);
    let health = zone.snapshot_for_player(1).unwrap().viewer.health;
    run(
        &mut zone,
        u32::from(BREATH_MAX_TICKS / BREATH_REFILL_PER_TICK),
    );
    let viewer = zone.snapshot_for_player(1).unwrap().viewer;
    assert_eq!(viewer.breath, BREATH_MAX_TICKS);
    assert!(viewer.health >= health);
}

#[test]
fn drowning_kills_through_the_normal_death_rules() {
    let floor_feet = -800;
    let mut zone = zone_with_player_at([lake::CENTRE[0], floor_feet + 90, lake::CENTRE[1]]);
    zone.apply_command(1, 1, ZoneCommand::Rise(-1)).unwrap();
    let mut ticks = 0;
    while !zone.snapshot_for_player(1).unwrap().viewer.dead {
        zone.advance_tick().unwrap();
        ticks += 1;
        assert!(ticks < 2_000, "the player drowns");
    }
    assert_eq!(
        breath(&zone),
        BREATH_MAX_TICKS,
        "the dead hold no breath clock"
    );
}
