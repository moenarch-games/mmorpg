//! The Greyfang cave (#159): a real enclosed chamber the zone's physics walks units into.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, cave::GREYFANG};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

const NORTH: u16 = 32_768;
const EAST: u16 = 16_384;

fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = mmorpg_core::physics::from_units([feet[0], 120, feet[1]]);
    ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap()
}

fn feet(zone: &ZoneSimulation) -> [f32; 3] {
    let [x, y, z] = zone.snapshot().unwrap().players[0].position;
    [x, y - 90.0, z]
}

fn walk(zone: &mut ZoneSimulation, sequence: u32, facing: u16, ticks: usize) {
    zone.apply_command(
        1,
        sequence,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing,
        },
    )
    .unwrap();
    for _ in 0..ticks {
        zone.advance_tick().unwrap();
    }
}

#[test]
fn a_player_walks_through_the_mouth_into_both_chambers() {
    let mut zone = player_at([5_800, -12_400]);
    walk(&mut zone, 1, NORTH, mmorpg_core::golden::t30(70) as _);
    let inside = feet(&zone);
    assert!(
        inside[2] < -13_600.0 && inside[1].abs() < 6.0,
        "in the outer cavern on its flat floor: {inside:?}"
    );
    assert!(GREYFANG.flattens(inside[0] as i32, inside[2] as i32));
    // East through the passage into the inner chamber, stopped by its far wall.
    walk(&mut zone, 2, EAST, mmorpg_core::golden::t30(160) as _);
    let inner = feet(&zone);
    assert!(
        inner[0] > 7_300.0 && inner[0] < 8_000.0 && inner[1].abs() < 6.0,
        "in the inner chamber: {inner:?}"
    );
}

#[test]
fn the_south_wall_blocks_beside_the_mouth() {
    let mut zone = player_at([5_000, -12_400]);
    walk(&mut zone, 1, NORTH, mmorpg_core::golden::t30(80) as _);
    let at = feet(&zone);
    assert!(at[2] > -12_800.0, "held outside the wall: {at:?}");
}
