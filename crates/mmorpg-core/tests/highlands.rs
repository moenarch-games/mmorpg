//! Bandit Highlands (#425): from the Vale Ring Road a player walks the Highland Pass up the
//! escarpment's switchbacks onto the high ground, along the Highland Trail into the bandit
//! stronghold and back into the Vale, all on the zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, highlands};
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
fn a_player_winds_up_the_pass_to_the_stronghold_and_back_into_the_vale() {
    let mut zone = player_at([5_200, -15_000]);
    let mut sequence = 0;
    // Highland Pass: up the cutting, east along the first ledge, round the hairpin, back west
    // along the second ledge and north through the gorge onto the high ground.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [5_200, -21_200],
            [5_200, -23_200],
            [10_400, -23_200],
            [10_400, -25_600],
            [3_600, -25_600],
            [3_600, -28_000],
        ],
    );
    let top = feet(&zone);
    assert!(
        (top[1] - highlands::PLATEAU_Y as f32).abs() < 60.0,
        "on the high ground: {top:?}"
    );
    assert_eq!(
        greyhaven_vale::area_at(top[0] as i32, top[2] as i32).map(|area| area.name()),
        Some("Bandit Highlands")
    );
    // Highland Trail through the stronghold's gate to its campfire, 300 m beyond the Vale's old
    // northern wall.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [3_600, -33_000],
            [-8_000, -38_000],
            [-8_000, -44_000],
            [0, -47_000],
            [0, -51_000],
            [-800, -52_000],
        ],
    );
    let camp = feet(&zone);
    assert!(camp[2] < -50_000.0, "{camp:?}");
    // And back down the pass into the Vale without a break.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [0, -51_000],
            [0, -47_000],
            [-8_000, -44_000],
            [-8_000, -38_000],
            [3_600, -33_000],
            [3_600, -25_600],
            [10_400, -25_600],
            [10_400, -23_200],
            [5_200, -23_200],
            [5_200, -15_000],
        ],
    );
    let home = feet(&zone);
    assert_eq!(
        greyhaven_vale::area_at(home[0] as i32, home[2] as i32).map(|area| area.name()),
        Some("Greyfang Ridge")
    );
}

#[test]
fn the_escarpment_holds_a_player_beside_the_pass() {
    // At the escarpment's foot west of the pass, walking north runs into its face.
    let mut zone = player_at([1_000, -20_200]);
    let mut sequence = 0;
    for _ in 0..mmorpg_core::golden::t30(240) {
        sequence += 1;
        zone.apply_command(
            1,
            sequence,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: 32_768,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
    }
    let at = feet(&zone);
    assert!(at[2] > -21_800.0 && at[1] < 600.0, "held at {at:?}");
}
