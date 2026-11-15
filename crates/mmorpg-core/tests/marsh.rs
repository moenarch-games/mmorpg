//! Marsh-Lake Wetlands (#426) in the redesigned region (#816): from the Vale Ring Road a player
//! walks the Fen Road onto the wetlands, along the Marsh Trail and the causeway to the drowned
//! watchtower's island and back into the Vale, and on down the Fenwater to its ford, all on the
//! zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, marsh};
use mmorpg_core::{WaterMode, ZoneCommand, ZoneId, ZoneSimulation};

/// The full-length walk runs on bare terrain: a level 1 walker would not survive the region's
/// creatures (#506), which have their own test. The short water walk keeps real content.
fn bare_player_at(feet: [i32; 2]) -> ZoneSimulation {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    let zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
    placed_at(zone, feet)
}

fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    placed_at(zone, feet)
}

fn placed_at(mut zone: ZoneSimulation, feet: [i32; 2]) -> ZoneSimulation {
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = zone.content().definition().ground_at(feet[0], feet[1]);
    state.players[0].position = mmorpg_core::physics::from_units([feet[0], ground + 100, feet[1]]);
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

/// Walks the player along `waypoints`. Panics if a waypoint is not reached in time.
fn walk(zone: &mut ZoneSimulation, sequence: &mut u32, waypoints: &[[i32; 2]]) {
    for &goal in waypoints {
        let mut ticks: u64 = 0;
        loop {
            let at = feet(zone);
            if (goal[0] as f32 - at[0]).hypot(goal[1] as f32 - at[2]) < 40.0 {
                break;
            }
            assert!(
                ticks < mmorpg_core::golden::t30(1_500),
                "stuck at {at:?} on the way to {goal:?}"
            );
            *sequence += 1;
            zone.apply_command(
                1,
                *sequence,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: heading(at, goal),
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
            ticks += 1;
        }
    }
}

fn area(at: [f32; 3]) -> Option<&'static str> {
    greyhaven_vale::area_at(at[0] as i32, at[2] as i32).map(|area| area.name())
}

/// A road's centre line by name.
fn road(name: &str) -> &'static [[i32; 2]] {
    greyhaven_vale::roads()
        .iter()
        .find(|road| road.name == name)
        .unwrap_or_else(|| panic!("no road {name}"))
        .points
}

#[test]
fn a_player_crosses_the_pass_to_the_island_and_back_into_the_vale() {
    let mut zone = bare_player_at(road("Fen Road")[0]);
    let mut sequence = 0;
    // The Fen Road leaves the top of the Millbrook Climb eastward onto the wetlands (#816).
    let fen_road: Vec<[i32; 2]> = road("Fen Road")
        .iter()
        .copied()
        .take_while(|point| point[0] <= 30_000)
        .collect();
    walk(&mut zone, &mut sequence, &fen_road);
    let below = feet(&zone);
    assert!(
        (below[1] - marsh::FLOOR_Y as f32).abs() < 300.0,
        "on the wetlands: {below:?}"
    );
    assert_eq!(area(below), Some("Marsh-Lake Wetlands"));
    // Marsh Trail to the lake's shore and along the causeway onto the island, 460 m east of the
    // Vale's centre.
    let trail = road("Marsh Trail");
    walk(&mut zone, &mut sequence, trail);
    walk(
        &mut zone,
        &mut sequence,
        &[[44_000, -2_000], [44_800, -2_000]],
    );
    let island = feet(&zone);
    assert!(
        (island[1] - marsh::ISLAND_Y as f32).abs() < 60.0,
        "on the island: {island:?}"
    );
    let content = greyhaven_vale::content();
    let units = |value: f32| value.round() as i32;
    assert_eq!(
        WaterMode::of_feet(content.waters(), island.map(units)),
        WaterMode::Dry
    );
    // And back over the causeway and the Fen Road into the Vale without a break.
    let back: Vec<[i32; 2]> = [[44_000, -2_000]]
        .into_iter()
        .chain(trail.iter().rev().copied())
        .chain(fen_road.iter().rev().copied())
        .collect();
    walk(&mut zone, &mut sequence, &back);
    let home = feet(&zone);
    assert!(
        greyhaven_vale::beyond_core(home[0] as i32, home[2] as i32) <= 0,
        "{home:?}"
    );
}

#[test]
fn the_reed_path_wades_through_the_ford() {
    let mut zone = player_at([29_000, -6_000]);
    let mut sequence = 0;
    walk(&mut zone, &mut sequence, &[[31_400, -8_800]]);
    let at = feet(&zone);
    let waters = zone.content().waters();
    assert_eq!(
        WaterMode::of_feet(waters, at.map(|value| value.round() as i32)),
        WaterMode::Wading,
        "{at:?}"
    );
    walk(&mut zone, &mut sequence, &[[34_000, -12_000]]);
}

#[test]
fn the_fen_road_follows_the_fenwater_to_its_ford() {
    // Beyond the wetlands the Fen Road runs down the Fenwater and wades it at the ford (#816).
    let fen_road: Vec<[i32; 2]> = road("Fen Road")
        .iter()
        .copied()
        .skip_while(|point| point[0] < 56_000)
        .collect();
    let mut zone = bare_player_at(fen_road[0]);
    let mut sequence = 0;
    walk(&mut zone, &mut sequence, &fen_road);
    let at = feet(&zone);
    assert!(at[0] > 110_000.0, "{at:?}");
}
