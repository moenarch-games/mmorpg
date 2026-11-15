//! Bandit Highlands (#425): from the Vale Ring Road a player walks the Highland Pass up the
//! escarpment's switchbacks onto the high ground, along the Highland Trail into the bandit
//! stronghold and back into the Vale, all on the zone's own physics.
use std::sync::Arc;

use mmorpg_core::greyhaven_vale::{self, highlands, terrain};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

/// The walking route tested on bare terrain: the region's Redbrand garrison (#502) has its own
/// test, and a level 1 walker would not survive it.
fn player_at(feet: [i32; 2]) -> ZoneSimulation {
    let definition = greyhaven_vale::greyhaven_vale_definition();
    let mut zone = ZoneSimulation::with_definition(ZoneId::new(1), definition).unwrap();
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
    let mut zone = player_at([6_400, -16_000]);
    let mut sequence = 0;
    // Highland Pass: up the cutting, east along the first ledge, round the hairpin, back west
    // along the second ledge and north through the gorge onto the high ground.
    walk(
        &mut zone,
        &mut sequence,
        &[
            // Leave the north-entrance camp through its gate before turning toward the pass.
            [6_400, -17_600],
            [5_200, -17_600],
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
    // The Greyfang Road and the Stronghold Way (#816) through the stronghold's gate to its
    // campfire, 300 m beyond the Vale's old northern wall.
    let to_gate = to_stronghold_gate();
    walk(&mut zone, &mut sequence, &to_gate);
    walk(&mut zone, &mut sequence, &[[-800, -52_000]]);
    let camp = feet(&zone);
    assert!(camp[2] < -50_000.0, "{camp:?}");
    // And back down the pass into the Vale without a break.
    let back: Vec<[i32; 2]> = to_gate.iter().rev().copied().collect();
    walk(&mut zone, &mut sequence, &back);
    walk(
        &mut zone,
        &mut sequence,
        &[
            [3_600, -25_600],
            [10_400, -25_600],
            [10_400, -23_200],
            [5_200, -23_200],
            [5_200, -17_600],
            [6_400, -17_600],
            [6_400, -16_000],
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
    // The weathered toe varies in depth, but ordinary walking must stop below the first
    // pass ledge rather than cross the rock face or reach the crest.
    assert!(
        at[2] > highlands::PASS[1].max[1] as f32 && at[1] < 600.0,
        "held at {at:?}"
    );
}

#[test]
fn a_player_can_walk_along_the_western_rock_terrace() {
    let mut zone = player_at([-15_200, -22_400]);
    let mut sequence = 0;
    walk(
        &mut zone,
        &mut sequence,
        &[
            [-15_200, -22_800],
            [-15_200, -23_200],
            [-14_800, -23_200],
            [-14_800, -22_400],
        ],
    );
    let at = feet(&zone);
    assert!(
        (2_100. ..2_900.).contains(&at[1]),
        "supported by the upper terrace: {at:?}"
    );
}

/// The Vale Ring Road, the pass's switchbacks, the Lookout Path, the Highland Trail over its
/// saddle to the stronghold's gate, a cross-country return to the lookout by way of the
/// saddle's overlook, and back down into the Vale (#528).
#[test]
fn a_player_walks_the_roads_to_the_lookout_and_the_stronghold_and_back() {
    let mut zone = player_at([6_400, -15_000]);
    let mut sequence = 0;
    let climb = [
        [6_400, -16_000],
        [6_400, -17_600],
        [5_200, -17_600],
        [5_200, -23_200],
        [10_400, -23_200],
        [10_400, -25_600],
        [3_600, -25_600],
        [3_600, -28_000],
        [3_600, -33_000],
    ];
    walk(&mut zone, &mut sequence, &climb[..climb.len() - 1]);
    // Out along the Lookout Path (#816) to the lookout's knoll and back to the Greyfang Road.
    let lookout = road("Lookout Path");
    walk(&mut zone, &mut sequence, lookout);
    let knoll = feet(&zone);
    assert_grounded_near(&knoll, lookout[lookout.len() - 1]);
    assert!(knoll[1] > highlands::PLATEAU_Y as f32 - 100.0, "{knoll:?}");
    let back: Vec<[i32; 2]> = lookout.iter().rev().copied().collect();
    walk(&mut zone, &mut sequence, &back);
    // Along the Greyfang Road to the stronghold's gate.
    let to_gate = to_stronghold_gate();
    walk(&mut zone, &mut sequence, &to_gate);
    assert_grounded_near(&feet(&zone), to_gate[to_gate.len() - 1]);
    // Home the same way and down the switchbacks.
    let back: Vec<[i32; 2]> = to_gate.iter().rev().copied().collect();
    walk(&mut zone, &mut sequence, &back);
    let mut down = climb;
    down.reverse();
    walk(&mut zone, &mut sequence, &down);
    walk(&mut zone, &mut sequence, &[[6_400, -15_000]]);
    assert_grounded_near(&feet(&zone), [6_400, -15_000]);
}

/// A road's centre line by name (#816).
fn road(name: &str) -> &'static [[i32; 2]] {
    greyhaven_vale::roads()
        .iter()
        .find(|road| road.name == name)
        .unwrap_or_else(|| panic!("no road {name}"))
        .points
}

/// From the top of the Highland Pass along the Greyfang Road to where the Stronghold Way leaves
/// it, then along the Stronghold Way through the stronghold's gate.
fn to_stronghold_gate() -> Vec<[i32; 2]> {
    let way = road("Stronghold Way");
    let fork = way[0];
    let greyfang = road("Greyfang Road");
    let until = greyfang
        .iter()
        .position(|point| (point[0] - fork[0]).abs() + (point[1] - fork[1]).abs() < 1_500)
        .unwrap_or(greyfang.len() - 1);
    greyfang[..=until].iter().chain(way).copied().collect()
}

fn assert_grounded_near(at: &[f32; 3], goal: [i32; 2]) {
    let ground = greyhaven_vale::greyhaven_vale_definition().ground_at(at[0] as i32, at[2] as i32);
    assert!(
        (at[1] - ground as f32).abs() < 60.0,
        "floating or sunken at {at:?} near {goal:?} (ground {ground})"
    );
}

/// Whether the grid step from `a` to `b` is walkable: no steeper than the controller's 45°.
fn walkable_step(a: i32, b: i32) -> bool {
    (a - b).abs() <= terrain::STEP
}

#[test]
fn the_sculpted_landforms_vary_and_stay_walkable_and_connected() {
    let grid = greyhaven_vale::authored_terrain();
    let (columns, rows) = (i32::from(grid.columns), i32::from(grid.rows));
    let index = |column: i32, row: i32| usize::try_from(row * columns + column).unwrap();
    let position = |column: i32, row: i32| {
        [
            terrain::ORIGIN[0] + column * terrain::STEP,
            terrain::ORIGIN[1] + row * terrain::STEP,
        ]
    };
    let cell = |[x, z]: [i32; 2]| {
        [
            (x - terrain::ORIGIN[0]) / terrain::STEP,
            (z - terrain::ORIGIN[1]) / terrain::STEP,
        ]
    };

    // The high ground proper: inside the walls, north of the escarpment's rear.
    let on_high_ground = |[x, z]: [i32; 2]| {
        x > highlands::BOUNDS.min[0]
            && x < highlands::BOUNDS.max[0]
            && z > highlands::BOUNDS.min[1]
            && z <= highlands::ESCARPMENT[3] - 1_600
    };

    let mut reachable = vec![false; grid.heights.len()];
    let [start_column, start_row] = cell([0, -47_000]);
    let mut frontier = vec![(start_column, start_row)];
    reachable[index(start_column, start_row)] = true;
    while let Some((column, row)) = frontier.pop() {
        for (dc, dr) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let (c, r) = (column + dc, row + dr);
            if !(0..columns).contains(&c) || !(0..rows).contains(&r) {
                continue;
            }
            let next = index(c, r);
            if reachable[next]
                || !on_high_ground(position(c, r))
                || !walkable_step(grid.heights[index(column, row)], grid.heights[next])
            {
                continue;
            }
            reachable[next] = true;
            frontier.push((c, r));
        }
    }

    let mut total = 0;
    let mut stranded = 0;
    let (mut lowest, mut highest) = (i32::MAX, i32::MIN);
    for row in 0..rows {
        for column in 0..columns {
            let at = position(column, row);
            if !on_high_ground(at) || highlands::level(at[0], at[1]) {
                continue;
            }
            total += 1;
            stranded += i32::from(!reachable[index(column, row)]);
            lowest = lowest.min(grid.heights[index(column, row)]);
            highest = highest.max(grid.heights[index(column, row)]);
        }
    }
    assert!(
        stranded * 100 <= total * 3,
        "{stranded} of {total} high-ground samples are cut off"
    );
    // The high ground rises from the escarpment towards the mountains (#816).
    assert!(
        highest - lowest >= 1_200 && lowest >= highlands::PLATEAU_Y,
        "relief spans {lowest}..{highest}"
    );

    // Every route point, spawn and ledge of the high ground is on the connected walkable ground.
    let content = greyhaven_vale::content();
    let garrison = content
        .creature_spawns()
        .iter()
        .filter(|spawn| (1_000..1_100).contains(&spawn.id.get()))
        .map(|spawn| spawn.position);
    // The designed roads over the old highlands' high ground (#816) and the garrison's posts.
    let roads = ["Greyfang Road", "Stronghold Way", "Lookout Path"]
        .into_iter()
        .flat_map(|name| road(name).iter().copied())
        .filter(|point| point[1] > highlands::BOUNDS.min[1] + 2_000);
    for at in roads.chain(garrison).filter(|&at| on_high_ground(at)) {
        let [column, row] = cell(at);
        assert!(reachable[index(column, row)], "{at:?} is cut off");
    }
}
