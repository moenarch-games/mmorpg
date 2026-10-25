//! Farmland and Hamlets (#424): from the Vale Ring Road a player walks the South Road up the
//! ridge's cliff notch onto the farmland's plateau, down the farm road to the landmark tower,
//! and back into the Vale, all on the zone's own physics.
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

#[test]
fn a_player_walks_up_the_notch_to_the_tower_and_back_into_the_vale() {
    let mut zone = player_at([0, 15_000]);
    let mut sequence = 0;
    // South Road: up the cutting, east along the gorge and up onto the plateau.
    walk(
        &mut zone,
        &mut sequence,
        &[[0, 21_200], [0, 23_200], [2_800, 23_200], [2_800, 25_600]],
    );
    let top = feet(&zone);
    assert!(
        (top[1] - farmland::PLATEAU_Y as f32).abs() < 30.0,
        "on the plateau: {top:?}"
    );
    assert_eq!(
        greyhaven_vale::area_at(top[0] as i32, top[2] as i32).map(|area| area.name()),
        Some("Farmland and Hamlets")
    );
    // Farmland Road to the landmark tower, 200 m beyond the Vale's old southern wall.
    walk(
        &mut zone,
        &mut sequence,
        &[[2_800, 44_000], [2_000, 44_000]],
    );
    let tower = feet(&zone);
    assert!(tower[2] > 40_000.0, "{tower:?}");
    // And back down into the Vale without a break.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [2_800, 44_000],
            [2_800, 23_200],
            [0, 23_200],
            [0, 20_000],
            [0, 15_000],
        ],
    );
    let home = feet(&zone);
    assert_eq!(
        greyhaven_vale::area_at(home[0] as i32, home[2] as i32).map(|area| area.name()),
        Some("Southmeadow Downs")
    );
}

#[test]
fn the_ridge_holds_a_player_beside_the_notch() {
    // At the ridge's foot west of the notch, walking south runs into its face.
    let mut zone = player_at([-4_000, 20_200]);
    let mut sequence = 0;
    for _ in 0..mmorpg_core::golden::t30(240) {
        sequence += 1;
        zone.apply_command(
            1,
            sequence,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: 0,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
    }
    let at = feet(&zone);
    assert!(at[2] < 21_800.0 && at[1] < 600.0, "held at {at:?}");
}
