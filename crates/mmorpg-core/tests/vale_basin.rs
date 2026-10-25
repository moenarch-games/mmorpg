//! Greyhaven's village basin (#423): Millbrook Farm's terrace stands 11 m above the Outpost's
//! valley floor behind a cliff no unit climbs, reached up the Millbrook Climb and left down the
//! Millbrook Steps; Stillwater Lake shares the terrace, up the Lakeshore Climb and along the
//! Stillwater Rim Trail from the farm. The rim closes round the basin (slice 3): the Wolfrun,
//! Hollow and South Climbs lead up to Wolfrun Woods, Redbrand Hollow and the south, and the
//! Wolfrun Ridge Path joins the woods to the Hollow. All walked on the zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, basin};
use mmorpg_core::{EntityKind, ZoneCommand, ZoneId, ZoneSimulation};

const EAST: u16 = 16_384;

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

/// Walks the player along `waypoints`, returning every tick's feet. Panics if a waypoint is not
/// reached in time or the player ever jumps more than half a metre in one tick.
fn walk(zone: &mut ZoneSimulation, waypoints: &[[i32; 2]]) -> Vec<[f32; 3]> {
    let mut sequence = 0;
    let mut trail = vec![feet(zone)];
    for &goal in waypoints {
        let mut ticks: u64 = 0;
        loop {
            let at = feet(zone);
            let distance = (goal[0] as f32 - at[0]).hypot(goal[1] as f32 - at[2]);
            if distance < 40.0 {
                break;
            }
            assert!(
                ticks < mmorpg_core::golden::t30(1_200),
                "stuck at {at:?} on the way to {goal:?}"
            );
            sequence += 1;
            zone.apply_command(
                1,
                sequence,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: heading(at, goal),
                },
            )
            .unwrap();
            zone.advance_tick().unwrap();
            let next = feet(zone);
            let step = (next[0] - at[0]).hypot(next[2] - at[2]);
            assert!(
                step <= 50.0 && (next[1] - at[1]).abs() <= 50.0,
                "{at:?} -> {next:?}"
            );
            trail.push(next);
            ticks += 1;
        }
    }
    trail
}

fn settle(zone: &mut ZoneSimulation) {
    for _ in 0..mmorpg_core::golden::t30(30) {
        zone.advance_tick().unwrap();
    }
}

#[test]
fn the_farm_terrace_rises_above_the_basin_behind_a_cliff() {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    // The Outpost's floor, the farmyard on the terrace.
    assert_eq!(definition.ground_at(0, 2_000), 0);
    assert_eq!(definition.ground_at(4_000, 9_000), 0);
    assert_eq!(definition.ground_at(6_200, 4_150), basin::TERRACE_Y);
    // One 4 m cell carries the whole 11 m: 70°, far beyond the controllers' 45°.
    assert_eq!(definition.ground_at(4_400, 9_000), 0);
    assert_eq!(definition.ground_at(4_800, 9_000), basin::TERRACE_Y);
}

#[test]
fn a_player_walks_up_the_millbrook_climb_to_the_farm() {
    let mut zone = player_at([3_600, 2_000]);
    settle(&mut zone);
    assert!(feet(&zone)[1].abs() < 10.0);
    let trail = walk(
        &mut zone,
        &[
            [4_200, 2_000],
            [4_200, -1_800],
            [5_400, -1_800],
            [5_400, 1_200],
            [5_400, 2_800],
            [6_400, 3_300],
        ],
    );
    let top = feet(&zone);
    assert!(
        (top[1] - basin::TERRACE_Y as f32).abs() < 20.0,
        "on the terrace: {top:?}"
    );
    // The landing sits halfway up, on the ledge above the basin.
    assert!(
        trail
            .iter()
            .any(|at| at[2] < -1_600.0 && (at[1] - basin::LANDING_Y as f32).abs() < 30.0)
    );
}

#[test]
fn a_player_walks_down_the_millbrook_steps_to_the_south_road() {
    let mut zone = player_at([5_400, 2_800]);
    settle(&mut zone);
    assert!((feet(&zone)[1] - basin::TERRACE_Y as f32).abs() < 20.0);
    walk(
        &mut zone,
        &[[4_800, 3_400], [4_200, 3_400], [4_200, 8_000], [400, 8_000]],
    );
    assert!(feet(&zone)[1].abs() < 10.0, "back on the valley floor");
}

#[test]
fn the_basin_cliff_cannot_be_walked_up() {
    let mut zone = player_at([4_000, 9_000]);
    settle(&mut zone);
    for sequence in 1..=240 {
        zone.apply_command(
            1,
            sequence,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing: EAST,
            },
        )
        .unwrap();
        zone.advance_tick().unwrap();
    }
    let at = feet(&zone);
    assert!(
        at[0] < 4_700.0 && at[1] < 400.0,
        "the cliff holds the player at its foot: {at:?}"
    );
}

#[test]
fn farm_folk_and_creatures_stand_on_the_terrace() {
    let mut zone = player_at([6_000, 3_800]);
    settle(&mut zone);
    let entities = zone.snapshot_for_player(1).unwrap().entities;
    let osric = entities
        .iter()
        .find(|entity| entity.kind == EntityKind::Npc && entity.id == 4)
        .expect("Farmer Osric is in view");
    assert!(osric.position[1] > basin::TERRACE_Y);
    let rats: Vec<_> = entities
        .iter()
        .filter(|entity| entity.kind == EntityKind::Creature && (200..206).contains(&entity.id))
        .collect();
    assert!(!rats.is_empty());
    for rat in rats {
        assert!(rat.position[1] > basin::TERRACE_Y, "{rat:?}");
    }
}

#[test]
fn the_lake_and_the_shrine_lie_in_the_terrace() {
    use greyhaven_vale::{lake, shrine};
    let definition = greyhaven_vale::greyhaven_vale_definition();
    // The lake's west shore and the shore over the flooded cave stand on the terrace …
    assert_eq!(definition.ground_at(3_200, -5_600), basin::TERRACE_Y);
    assert_eq!(definition.ground_at(10_000, -4_400), basin::TERRACE_Y);
    // … the water lies just above it and the shrine's floor 9 m below.
    assert_eq!(lake::SURFACE, basin::TERRACE_Y + 15);
    assert_eq!(definition.ground_at(5_000, -5_400), shrine::FLOOR_Y);
    assert_eq!(shrine::FLOOR_Y, basin::TERRACE_Y - 900);
    // The south cliff over the basin: the terrace, then the floor one cell on.
    assert_eq!(definition.ground_at(2_000, -2_400), 0);
    assert_eq!(definition.ground_at(2_000, -2_800), basin::TERRACE_Y);
}

#[test]
fn a_player_walks_up_the_lakeshore_climb_to_the_lake() {
    let mut zone = player_at([0, -2_800]);
    settle(&mut zone);
    assert!(feet(&zone)[1].abs() < 10.0);
    let trail = walk(
        &mut zone,
        &[
            [1_400, -2_800],
            [1_400, -6_200],
            [2_600, -6_200],
            [2_600, -3_200],
            [3_400, -3_200],
            [3_400, -4_800],
        ],
    );
    let shore = feet(&zone);
    assert!(
        (shore[1] - basin::TERRACE_Y as f32).abs() < 30.0,
        "on the lake shore: {shore:?}"
    );
    assert!(
        trail
            .iter()
            .any(|at| at[2] < -6_000.0 && (at[1] - basin::LAKE_LANDING_Y as f32).abs() < 30.0)
    );
}

#[test]
fn the_rim_trail_joins_the_farm_and_the_lake_on_top() {
    let mut zone = player_at([6_400, 3_300]);
    settle(&mut zone);
    let trail = walk(&mut zone, &[[6_400, -2_400], [5_600, -3_600]]);
    assert!(
        trail
            .iter()
            .all(|at| (at[1] - basin::TERRACE_Y as f32).abs() < 30.0),
        "the trail stays on the terrace"
    );
}

/// Walks the player `facing` for four seconds and returns where they end.
fn push(zone: &mut ZoneSimulation, facing: u16) -> [f32; 3] {
    for sequence in 1..=240 {
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
        zone.advance_tick().unwrap();
    }
    feet(zone)
}

#[test]
fn the_rim_closes_round_the_basin_behind_cliffs() {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    // West under the woods, south of the Outpost, north under the Hollow and the lake: the
    // floor, then the rim one cell on.
    for (floor, rim) in [
        ([-4_400, 6_000], [-4_800, 6_000]),
        ([2_000, 9_600], [2_000, 10_000]),
        ([800, -6_400], [800, -6_800]),
        ([1_600, -2_800], [2_000, -2_800]),
    ] {
        assert_eq!(definition.ground_at(floor[0], floor[1]), 0, "{floor:?}");
        assert_eq!(
            definition.ground_at(rim[0], rim[1]),
            basin::TERRACE_Y,
            "{rim:?}"
        );
    }
    // Redbrand Hollow's camp, the Greyfang cave's floor and the den clearing are on the rim.
    for at in [[0, -9_200], [6_000, -13_800], [-8_000, 0]] {
        assert_eq!(
            definition.ground_at(at[0], at[1]),
            basin::TERRACE_Y,
            "{at:?}"
        );
    }
    // Beyond 128 m the rim descends into the outer valley's hills.
    assert!(definition.ground_at(0, 16_800) < basin::TERRACE_Y);
    // Each cliff holds a player at its foot: west under the woods, south, north under the Hollow.
    let held = |start: [i32; 2], facing: u16, below: fn([f32; 3]) -> bool| {
        let mut zone = player_at(start);
        settle(&mut zone);
        let at = push(&mut zone, facing);
        assert!(
            below(at) && at[1] < 400.0,
            "the cliff holds the player at its foot: {at:?}"
        );
    };
    held([-4_000, 6_000], 49_152, |at| at[0] > -4_700.0);
    held([2_000, 8_800], 0, |at| at[2] < 9_900.0);
    held([-400, -5_000], 32_768, |at| at[2] > -6_700.0);
}

#[test]
fn a_player_walks_up_the_wolfrun_climb_into_the_woods() {
    let mut zone = player_at([-3_600, 2_000]);
    settle(&mut zone);
    assert!(feet(&zone)[1].abs() < 10.0);
    let trail = walk(
        &mut zone,
        &[
            [-4_200, 2_000],
            [-4_200, -1_800],
            [-5_400, -1_800],
            [-5_400, 1_200],
            [-6_600, 300],
            [-7_600, 0],
        ],
    );
    let den = feet(&zone);
    assert!(
        (den[1] - basin::TERRACE_Y as f32).abs() < 20.0,
        "in the den clearing on the rim: {den:?}"
    );
    assert!(
        trail
            .iter()
            .any(|at| at[2] < -1_600.0 && (at[1] - basin::LANDING_Y as f32).abs() < 30.0)
    );
}

#[test]
fn a_player_walks_up_the_hollow_climb_to_the_camp() {
    let mut zone = player_at([0, -5_000]);
    settle(&mut zone);
    assert!(feet(&zone)[1].abs() < 10.0);
    let trail = walk(
        &mut zone,
        &[
            [0, -6_200],
            [-3_600, -6_200],
            [-3_600, -7_000],
            [0, -7_000],
            [0, -8_600],
        ],
    );
    let camp = feet(&zone);
    assert!(
        (camp[1] - basin::TERRACE_Y as f32).abs() < 20.0,
        "in the Hollow's camp: {camp:?}"
    );
    assert!(
        trail
            .iter()
            .any(|at| at[0] < -3_200.0 && (at[1] - basin::LANDING_Y as f32).abs() < 30.0)
    );
}

#[test]
fn a_player_walks_up_the_south_climb_onto_the_rim() {
    let mut zone = player_at([0, 8_600]);
    settle(&mut zone);
    assert!(feet(&zone)[1].abs() < 10.0);
    walk(
        &mut zone,
        &[
            [0, 9_400],
            [-3_600, 9_400],
            [-3_600, 10_200],
            [0, 10_200],
            [0, 12_000],
        ],
    );
    let top = feet(&zone);
    assert!(
        (top[1] - basin::TERRACE_Y as f32).abs() < 20.0,
        "on the rim: {top:?}"
    );
}

#[test]
fn the_ridge_path_joins_the_woods_to_the_hollow_on_the_rim() {
    let mut zone = player_at([-6_600, 300]);
    settle(&mut zone);
    // To the camp's western edge, short of its bandits.
    let trail = walk(&mut zone, &[[-6_600, -7_700], [-3_200, -7_700]]);
    assert!(
        trail
            .iter()
            .all(|at| (at[1] - basin::TERRACE_Y as f32).abs() < 30.0),
        "the path stays on the rim"
    );
}

#[test]
fn a_player_walks_down_the_hollow_climb_to_the_outpost() {
    let mut zone = player_at([0, -7_000]);
    settle(&mut zone);
    assert!((feet(&zone)[1] - basin::TERRACE_Y as f32).abs() < 20.0);
    walk(
        &mut zone,
        &[[-3_600, -7_000], [-3_600, -6_200], [0, -6_200], [0, -1_400]],
    );
    assert!(feet(&zone)[1].abs() < 10.0, "back on the valley floor");
}

#[test]
fn a_player_walks_from_the_hollow_into_the_greyfang_cave() {
    let mut zone = player_at([5_800, -11_000]);
    settle(&mut zone);
    walk(&mut zone, &[[5_800, -13_700]]);
    let inside = feet(&zone);
    assert!(
        (inside[1] - basin::TERRACE_Y as f32).abs() < 20.0,
        "on the cave's floor: {inside:?}"
    );
}
