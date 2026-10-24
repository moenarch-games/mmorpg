//! Ramps (#59): Greyhaven's mill loading dock has a shallow cart ramp a player walks up and
//! down, grounded on its slope, and a steep chute nobody can climb; both stand clear of every
//! other collider, flush against the dock.

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, MILL_RAMPS, ids};
use mmorpg_core::{RampRise, ZoneCommand, ZoneId, ZoneSimulation};

/// Yaw 0 faces +Z; a quarter turn faces +X.
const EAST: u16 = 16_384;
const WEST: u16 = 49_152;
const NORTH_FACING_PLUS_Z: u16 = 0;

struct Walker {
    zone: ZoneSimulation,
    sequence: u32,
}

impl Walker {
    fn at(feet: [f32; 2]) -> Self {
        let content = greyhaven_vale::content();
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        let mut state = zone.snapshot().unwrap();
        state.players[0].position = [feet[0], 90.0, feet[1]];
        Self {
            zone: ZoneSimulation::from_snapshot(state, content).unwrap(),
            sequence: 0,
        }
    }

    fn walk(&mut self, facing: u16, ticks: usize) -> Vec<[f32; 3]> {
        self.sequence += 1;
        self.zone
            .apply_command(
                1,
                self.sequence,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing,
                },
            )
            .unwrap();
        (0..ticks)
            .map(|_| {
                self.zone.advance_tick().unwrap();
                self.zone.snapshot().unwrap().players[0].position
            })
            .collect()
    }
}

#[test]
fn a_player_walks_up_the_cart_ramp_onto_the_dock_and_back_down() {
    let ramp = MILL_RAMPS[0];
    assert_eq!(ramp.rises, RampRise::PosX);
    assert!((15.0..=20.0).contains(&ramp.slope_degrees()));
    // From 3 m west of the ramp's foot, east up the ramp and onto the dock.
    let mut walker = Walker::at([7_300.0, 1_950.0]);
    let path = walker.walk(EAST, mmorpg_core::golden::t30(44) as _);
    for [x, y, z] in &path {
        #[allow(clippy::cast_possible_truncation)]
        if let Some(top) = ramp.top_at(x.round() as i32, z.round() as i32) {
            let feet = f64::from(*y) - 90.0;
            // The capsule touches the slope uphill of its centre: at most its radius times
            // the slope above the surface under the centre.
            assert!(
                feet >= top - 0.5 && feet <= top + 30.0 * 150.0 / 500.0 + 1.0,
                "on the ramp at {x}: feet {feet}, top {top}"
            );
        }
    }
    let [x, y, _] = *path.last().unwrap();
    assert!(
        x > 8_150.0 && (y - 240.0).abs() < 0.5,
        "on the dock: {x} {y}"
    );
    // And back down to the ground.
    let [x, y, _] = *walker
        .walk(WEST, mmorpg_core::golden::t30(60) as _)
        .last()
        .unwrap();
    assert!(x < 7_550.0 && (y - 90.0).abs() < 0.5, "down again: {x} {y}");
}

#[test]
fn nobody_climbs_the_steep_chute() {
    let chute = MILL_RAMPS[1];
    assert!(chute.slope_degrees() > 55.0);
    // 2 m north of the chute, walking south into it (+Z) for 3 s.
    let mut walker = Walker::at([8_250.0, 1_500.0]);
    let path = walker.walk(NORTH_FACING_PLUS_Z, mmorpg_core::golden::t30(90) as _);
    let [_, y, z] = *path.last().unwrap();
    assert!(
        z < 1_800.0 && y < 90.0 + 75.0,
        "held below the dock: {z} {y}"
    );
    assert!(
        path.iter().all(|[_, y, _]| *y < 90.0 + 150.0),
        "never on the dock"
    );
}

#[test]
fn the_ramps_stand_flush_against_the_dock_and_clear_of_everything_else() {
    let content = greyhaven_vale::content();
    assert_eq!(content.ramps(), MILL_RAMPS);
    let dock = content
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == ids::MILL_DOCK)
        .unwrap();
    for ramp in MILL_RAMPS {
        // The high end touches the dock's side and tops out at its height.
        let [dx, dz] = ramp.rises.direction();
        let high_end = [
            ramp.position[0] + dx * ramp.half_extents[0],
            ramp.position[2] + dz * ramp.half_extents[2],
        ];
        let dock_side = [
            dock.position[0] - dx * dock.half_extents[0],
            dock.position[2] - dz * dock.half_extents[2],
        ];
        if dx != 0 {
            assert_eq!(high_end[0], dock_side[0]);
        } else {
            assert_eq!(high_end[1], dock_side[1]);
        }
        assert_eq!(
            ramp.position[1] + ramp.half_extents[1],
            dock.position[1] + dock.half_extents[1]
        );
        for collider in content.definition().colliders() {
            let overlaps = (0..3).all(|axis| {
                collider.position[axis] - collider.half_extents[axis]
                    < ramp.position[axis] + ramp.half_extents[axis]
                    && collider.position[axis] + collider.half_extents[axis]
                        > ramp.position[axis] - ramp.half_extents[axis]
            });
            assert!(
                !overlaps,
                "ramp {} overlaps collider {}",
                ramp.body, collider.id
            );
        }
    }
}
