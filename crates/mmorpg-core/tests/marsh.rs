//! Marsh-Lake Wetlands (#426): from the Vale Ring Road a player walks the Marsh Pass through the
//! ridge's gorges down onto the wetlands, along the Marsh Trail and the causeway to the drowned
//! watchtower's island and back into the Vale, all on the zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, marsh};
use mmorpg_core::{WaterMode, ZoneCommand, ZoneId, ZoneSimulation};

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

fn area(at: [f32; 3]) -> Option<&'static str> {
    greyhaven_vale::area_at(at[0] as i32, at[2] as i32).map(|area| area.name())
}

#[test]
fn a_player_crosses_the_pass_to_the_island_and_back_into_the_vale() {
    let mut zone = player_at([15_000, 4_000]);
    let mut sequence = 0;
    // Marsh Pass: into the cutting, north up the gorge, east across the crest, back south and
    // down the eastern cutting onto the wetlands.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [21_200, 4_000],
            [22_400, 4_000],
            [22_400, -800],
            [24_000, -800],
            [24_000, 2_400],
            [25_600, 2_400],
        ],
    );
    let below = feet(&zone);
    assert!(
        (below[1] - marsh::FLOOR_Y as f32).abs() < 60.0,
        "on the wetlands: {below:?}"
    );
    assert_eq!(area(below), Some("Marsh-Lake Wetlands"));
    // Marsh Trail to the lake's shore and along the causeway onto the island, 460 m east of the
    // Vale's centre.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [29_000, 2_400],
            [32_000, 5_600],
            [34_400, 3_200],
            [34_400, -2_000],
            [44_000, -2_000],
            [44_800, -2_000],
        ],
    );
    let island = feet(&zone);
    assert!(
        (island[1] - marsh::ISLAND_Y as f32).abs() < 60.0,
        "on the island: {island:?}"
    );
    let waters = zone.content().waters();
    let units = |value: f32| value.round() as i32;
    assert_eq!(
        WaterMode::of_feet(waters, island.map(units)),
        WaterMode::Dry
    );
    // And back over the causeway and the pass into the Vale without a break.
    walk(
        &mut zone,
        &mut sequence,
        &[
            [44_000, -2_000],
            [34_400, -2_000],
            [34_400, 3_200],
            [32_000, 5_600],
            [29_000, 2_400],
            [25_200, 2_400],
            [24_000, 2_400],
            [24_000, -800],
            [22_400, -800],
            [22_400, 4_000],
            [15_000, 4_000],
        ],
    );
    assert_eq!(area(feet(&zone)), Some("Eastwater Fen"));
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
fn the_ridge_holds_a_player_beside_the_pass() {
    // At the ridge's foot north of the pass, walking east runs into its face.
    let mut zone = player_at([20_400, -2_000]);
    let mut sequence = 0;
    for _ in 0..mmorpg_core::golden::t30(240) {
        sequence += 1;
        zone.apply_command(
            1,
            sequence,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: 16_384,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
    }
    let at = feet(&zone);
    assert!(at[0] < 21_800.0 && at[1] < 600.0, "held at {at:?}");
}
