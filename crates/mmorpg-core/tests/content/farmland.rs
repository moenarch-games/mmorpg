//! Farmland and Hamlets (#424) in the redesigned region (#816): from the Vale a player walks the
//! King's Road across the open downs to the landmark tower and back, and on to the region's
//! southern edge, all on the zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, farmland};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
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

/// The designed road named `name` (#816).
fn road(name: &str) -> &'static [[i32; 2]] {
    greyhaven_vale::roads()
        .iter()
        .find(|road| road.name == name)
        .unwrap_or_else(|| panic!("no road {name}"))
        .points
}

#[test]
fn a_player_walks_the_kings_road_to_the_tower_and_back_into_the_vale() {
    let mut zone = player_at([0, 15_000]);
    let mut sequence = 0;
    // The King's Road leaves the Vale southward across the open downs (#816).
    let kings_road = road("King's Road");
    let to_tower: Vec<[i32; 2]> = kings_road
        .iter()
        .copied()
        .take_while(|point| point[1] <= 48_000)
        .collect();
    walk(&mut zone, &mut sequence, &to_tower);
    walk(&mut zone, &mut sequence, &[[2_000, 44_000]]);
    let tower = feet(&zone);
    assert_eq!(
        greyhaven_vale::area_at(tower[0] as i32, tower[2] as i32).map(|area| area.name()),
        Some("Farmland and Hamlets"),
        "{tower:?}"
    );
    // And back up into the Vale without a break.
    let home: Vec<[i32; 2]> = to_tower
        .iter()
        .rev()
        .copied()
        .chain([[0, 15_000]])
        .collect();
    walk(&mut zone, &mut sequence, &home);
    let home = feet(&zone);
    assert_eq!(
        greyhaven_vale::area_at(home[0] as i32, home[2] as i32).map(|area| area.name()),
        Some("Southmeadow Downs")
    );
}

#[test]
fn the_kings_road_runs_on_to_the_regions_southern_edge() {
    let kings_road = road("King's Road");
    let start = kings_road[kings_road.len() / 2];
    let mut zone = player_at(start);
    let mut sequence = 0;
    walk(
        &mut zone,
        &mut sequence,
        &kings_road[kings_road.len() / 2..],
    );
    let end = feet(&zone);
    assert!(end[2] > 90_000.0, "{end:?}");
    assert!(farmland::BOUNDS.max[1] < end[2] as i32);
}
