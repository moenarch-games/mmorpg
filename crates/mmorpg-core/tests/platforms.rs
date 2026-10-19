//! Moving platforms (#56): Greyhaven's mill hoist carries a player standing on it up to the
//! windmill's gallery, the player walks off onto it, the hoist sinks back without them, and a
//! zone restored mid-ride continues exactly (Atlas 2.1.5).

use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, MILL_HOIST};
use mmorpg_core::{PlatformPhase, ZoneCommand, ZoneId, ZoneSimulation};

const NORTH: u16 = 0;

/// Greyhaven with player 1 standing on the resting hoist.
fn zone_on_the_hoist() -> ZoneSimulation {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    state.players[0].position = [8_800.0, 90.0, 1_748.0];
    ZoneSimulation::from_snapshot(state, content).unwrap()
}

fn player_at(zone: &ZoneSimulation) -> [f32; 3] {
    zone.snapshot().unwrap().players[0].position
}

#[test]
fn the_mill_hoist_lifts_a_player_to_the_gallery_and_sinks_without_them() {
    let hoist = MILL_HOIST;
    let rise = u64::from(hoist.dwell);
    let top = rise + u64::from(hoist.travel_ticks());
    let mut zone = zone_on_the_hoist();
    let mut sequence = 0;
    let mut send = |zone: &mut ZoneSimulation, command| {
        sequence += 1;
        zone.apply_command(1, sequence, command).unwrap();
    };
    while zone.current_tick() < top {
        zone.advance_tick().unwrap();
        let tick = zone.current_tick();
        let [_, y, _] = player_at(&zone);
        // The feet ride the hoist's top: its centre plus its half height.
        let surface = hoist.centre_at(tick)[1] + hoist.half_extents[1];
        assert!(
            (y - 90.0 - surface as f32).abs() < 0.1,
            "tick {tick}: feet at {} on a top at {surface}",
            y - 90.0
        );
    }
    assert_eq!(hoist.phase_at(top).0, PlatformPhase::AtHigh);
    // Off onto the windmill's flat top, 8 m up.
    send(
        &mut zone,
        ZoneCommand::Move {
            forward: 1,
            strafe: 0,
            facing: NORTH,
        },
    );
    for _ in 0..20 {
        zone.advance_tick().unwrap();
    }
    send(
        &mut zone,
        ZoneCommand::Move {
            forward: 0,
            strafe: 0,
            facing: NORTH,
        },
    );
    let gallery = player_at(&zone);
    assert!(gallery[2] > 1_900.0, "{gallery:?}");
    // The hoist sinks back to the ground; the player stays on the gallery.
    while hoist.phase_at(zone.current_tick()).0 != PlatformPhase::AtLow {
        zone.advance_tick().unwrap();
    }
    let [_, y, z] = player_at(&zone);
    assert!((y - 890.0).abs() < 0.1 && z > 1_900.0, "{y} {z}");
}

#[test]
fn a_zone_restored_mid_ride_continues_exactly() {
    let mut zone = zone_on_the_hoist();
    let midway = u64::from(MILL_HOIST.dwell) + 40;
    while zone.current_tick() < midway {
        zone.advance_tick().unwrap();
    }
    let mut restored =
        ZoneSimulation::from_snapshot(zone.snapshot().unwrap(), Arc::clone(zone.content()))
            .unwrap();
    for _ in 0..400 {
        zone.advance_tick().unwrap();
        restored.advance_tick().unwrap();
        assert_eq!(zone.snapshot().unwrap(), restored.snapshot().unwrap());
    }
}

#[test]
fn the_hoist_sweeps_only_its_own_shaft_and_tops_out_at_the_gallery() {
    let content = greyhaven_vale::content();
    assert_eq!(content.platforms(), [MILL_HOIST]);
    let hoist = MILL_HOIST;
    // The whole sweep, from the low stop's bottom to the high stop's top.
    let min =
        [0, 1, 2].map(|axis| hoist.low[axis].min(hoist.high[axis]) - hoist.half_extents[axis]);
    let max =
        [0, 1, 2].map(|axis| hoist.low[axis].max(hoist.high[axis]) + hoist.half_extents[axis]);
    for collider in content.definition().colliders() {
        let overlaps = (0..3).all(|axis| {
            collider.position[axis] - collider.half_extents[axis] < max[axis]
                && collider.position[axis] + collider.half_extents[axis] > min[axis]
        });
        // The ground slab lies wholly below the surface the hoist rests flush with.
        let ground = collider.position[1] + collider.half_extents[1] <= 0;
        assert!(!overlaps || ground, "collider {} in the shaft", collider.id);
    }
    let windmill = content
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == greyhaven_vale::ids::WINDMILL)
        .unwrap();
    assert_eq!(
        hoist.high[1] + hoist.half_extents[1],
        windmill.position[1] + windmill.half_extents[1],
        "the high stop is flush with the gallery"
    );
    assert_eq!(
        hoist.low[1] + hoist.half_extents[1],
        0,
        "the low stop is flush with the ground"
    );
}
