//! The footbridges (#828): a player walks the Delving Track up one end ramp, along the deck over
//! the Delving Run and down the other, on the zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, crossings};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

const EAST: u16 = 16_384;

#[test]
fn a_player_walks_over_the_delving_run_on_the_footbridge() {
    let bridge = crossings::BRIDGES[0];
    assert!(bridge.along_x);
    let [x, z] = bridge.centre;
    let start = [x - 1_500, z];
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content()).unwrap();
    zone.add_player(1).unwrap();
    let mut state = zone.snapshot().unwrap();
    let ground = zone.content().definition().ground_at(start[0], start[1]);
    state.players[0].position =
        mmorpg_core::physics::from_units([start[0], ground + 100, start[1]]);
    let mut zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content())).unwrap();
    let feet = |zone: &ZoneSimulation| {
        let [x, y, z] = zone.snapshot().unwrap().players[0].position;
        [x, y - 90.0, z]
    };
    let mut highest_on_deck = f32::MIN;
    for sequence in 1..=mmorpg_core::golden::t30(400) {
        zone.apply_command(
            1,
            u32::try_from(sequence).unwrap(),
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: EAST,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
        let at = feet(&zone);
        if (at[0] - x as f32).abs() < 300.0 {
            highest_on_deck = highest_on_deck.max(at[1]);
        }
        if at[0] > (x + 1_500) as f32 {
            break;
        }
    }
    let end = feet(&zone);
    assert!(end[0] > (x + 1_500) as f32, "stuck at {end:?}");
    assert!((end[2] - z as f32).abs() < 100.0, "drifted to {end:?}");
    // Over the stream the player walked on the deck, 0.55 m above the bridge's ends.
    assert!(
        (highest_on_deck - (bridge.foot + 55) as f32).abs() < 10.0,
        "on the deck at {highest_on_deck}"
    );
}
