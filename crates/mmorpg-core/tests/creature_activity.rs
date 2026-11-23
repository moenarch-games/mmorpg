//! The activity-driven creature tick (#1086) against its simple reference, every creature
//! visited by every system each tick (PRINCIPLE-010): Greyhaven's combat workloads and s
//! walking across the vale, waking and leaving sleepers behind, stay identical tick for tick,
//! canonical state and projections alike, while the activity-driven zone visits fewer creatures.
#[path = "support/greyhaven_workload.rs"]
mod greyhaven_workload;

use std::sync::Arc;

use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale};

/// Advances both zones through `ticks` ticks of `drive`, comparing canonical state every tick and
/// every player's projection every `every` ticks. Returns (activity-driven, reference) visits.
fn compare(
    mut zone: ZoneSimulation,
    mut reference: ZoneSimulation,
    ticks: usize,
    every: usize,
    mut drive: impl FnMut(&mut ZoneSimulation, usize),
) -> (usize, usize) {
    reference.visit_every_creature(true);
    let mut visits = (0, 0);
    for tick in 0..ticks {
        drive(&mut zone, tick);
        drive(&mut reference, tick);
        zone.advance_tick().unwrap();
        reference.advance_tick().unwrap();
        visits.0 += zone.tick_work().creatures_visited;
        visits.1 += reference.tick_work().creatures_visited;
        assert_eq!(
            zone.tick_work().creature_ai_evaluations,
            reference.tick_work().creature_ai_evaluations,
            "tick {tick}: decisions"
        );
        assert_eq!(
            zone.snapshot().unwrap(),
            reference.snapshot().unwrap(),
            "tick {tick}: canonical state"
        );
        if tick % every == 0 {
            for player in zone.snapshot().unwrap().players.iter().map(|p| p.player_id) {
                assert_eq!(
                    zone.snapshot_for_player(player).unwrap(),
                    reference.snapshot_for_player(player).unwrap(),
                    "tick {tick}: player {player}'s projection"
                );
            }
        }
    }
    visits
}

#[test]
fn greyhaven_fights_match_the_every_creature_reference() {
    for fixture in [
        greyhaven_workload::FIXTURES[0],
        greyhaven_workload::FIXTURES[1],
    ] {
        let zone = greyhaven_workload::initial_zone(fixture).unwrap();
        let reference = greyhaven_workload::initial_zone(fixture).unwrap();
        let (visits, every) = compare(
            zone,
            reference,
            greyhaven_workload::TICKS,
            20,
            |zone, tick| greyhaven_workload::commands(zone, fixture, tick).unwrap(),
        );
        assert!(
            visits * 4 < every,
            "{}: {visits} visits against {every}",
            fixture.name
        );
    }
}

#[test]
fn players_walking_across_the_vale_wake_and_leave_sleepers_like_the_reference() {
    let content = greyhaven_vale::content();
    let zone = |players: u32| {
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
        for player in 1..=players {
            zone.add_player(player).unwrap();
        }
        zone
    };
    // Each walker runs one way, turns a quarter every 15 s and jumps now and then; one of them
    // leaves after a while and another arrives.
    let drive = |zone: &mut ZoneSimulation, tick: usize| {
        let sequence = u32::try_from(tick).unwrap() * 4 + 1;
        if tick == 700 {
            zone.remove_player(2);
        }
        if tick == 900 {
            zone.add_player(5).unwrap();
        }
        for player in [1_u32, 2, 3, 4, 5] {
            if (tick == 900 && player == 5) || (tick.is_multiple_of(450) && player != 5) {
                let turn = u16::try_from(tick / 450).unwrap();
                let facing = u16::try_from(player)
                    .unwrap()
                    .wrapping_mul(13_107)
                    .wrapping_add(turn.wrapping_mul(16_384));
                let _ = zone.apply_command(
                    player,
                    sequence,
                    ZoneCommand::Move {
                        forward: 1,
                        strafe: 0,
                        facing,
                    },
                );
            } else if tick % 97 == usize::try_from(player).unwrap() * 7 {
                let _ = zone.apply_command(player, sequence, ZoneCommand::Jump);
            }
        }
    };
    let (visits, every) = compare(zone(4), zone(4), 1_800, 60, drive);
    assert!(visits < every, "{visits} visits against {every}");
}
