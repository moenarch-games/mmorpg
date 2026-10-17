//! Deterministic work samples of the operations declared in
//! `.performance/contract.json` (#138), printed as the `operations` part of
//! `coding-tooling/work-evidence/v1`; `scripts/operation-work.py` adds the
//! envelope. Counters come from `mmorpg-core`; nothing here is timed. Every
//! projection is also checked against the exhaustive visibility oracle, so a
//! cheaper count can never come from a wrong answer.
use std::error::Error;
use std::sync::Arc;

use mmorpg_core::{
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, INTEREST_RADIUS_UNITS,
    PLAYER_HALF_EXTENTS_UNITS, ZoneDefinition, ZoneId, ZoneSimulation,
};
use mmorpg_protocol::pack_snapshot;
#[path = "../tests/support/visibility_oracle.rs"]
mod visibility_oracle;
use visibility_oracle::exhaustive_projection;

/// Interest cells are one interest radius wide.
const CELL: i32 = INTEREST_RADIUS_UNITS;
/// A quarter turn faces +X.
const EAST: u16 = 16_384;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn player(
    id: usize,
    position: [i32; 2],
    forward: i8,
    facing: u16,
) -> Result<CanonicalPlayerSnapshot> {
    Ok(CanonicalPlayerSnapshot {
        copper: 0,
        player_id: u32::try_from(id)? + 1,
        position: [position[0], PLAYER_HALF_EXTENTS_UNITS[1], position[1]],
        velocity: [0; 3],
        facing,
        forward,
        strafe: 0,
        jump_pending: false,
        last_sequence: 0,
        spawn_slot: u16::try_from(id)?,
        inventory: mmorpg_core::Inventory::default(),
        inventory_revision: 1,
        inventory_changed_at: 0,
        equipment: mmorpg_core::Equipment::default(),
        combat: CanonicalPlayerCombat::default(),
        chat_ready_at: 0,
        chat: Vec::new(),
        quests: mmorpg_core::QuestLog::default(),
        quests_changed_at: 0,
    })
}

/// A zone without content units holding `players`, in ID order.
fn zone(players: Vec<CanonicalPlayerSnapshot>) -> Result<ZoneSimulation> {
    let empty = ZoneSimulation::with_definition(ZoneId::new(1), ZoneDefinition::default())?;
    let mut canonical = empty.snapshot()?;
    canonical.players = players;
    Ok(ZoneSimulation::from_snapshot(
        canonical,
        Arc::clone(empty.content()),
    )?)
}

/// Grid points at least three cells away from cell (0, 0), in a stable order.
fn far_points() -> Vec<[i32; 2]> {
    let mut points = Vec::new();
    for row in 0..51 {
        for column in 0..51 {
            let (x, z) = (column * 1_200 - 30_000, row * 1_200 - 30_000);
            let near = |v: i32| (-2 * CELL..3 * CELL).contains(&v);
            if !(near(x) && near(z)) {
                points.push([x, z]);
            }
        }
    }
    points
}

/// The projection of `viewer`, checked against the oracle: (tests, visible records).
fn project(zone: &ZoneSimulation, viewer: u32) -> Result<(usize, usize)> {
    let canonical = zone.snapshot()?;
    let observer = canonical
        .players
        .iter()
        .find(|p| p.player_id == viewer)
        .ok_or("the viewer is missing")?;
    let projection = zone.project_for_player(viewer)?;
    let packed = pack_snapshot(&projection.snapshot)?;
    if packed.payload != pack_snapshot(&exhaustive_projection(&canonical, observer))?.payload {
        return Err(format!("the projection for {viewer} differs from the oracle").into());
    }
    Ok((projection.stats.candidates_tested, packed.packed_entities))
}

fn sample(dimensions: &[(&str, usize)], metrics: &[(&str, usize)]) -> String {
    let pairs = |values: &[(&str, usize)]| {
        values
            .iter()
            .map(|(name, value)| format!("\"{name}\":{value}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{{\"dimensions\":{{{}}},\"metrics\":{{{}}}}}",
        pairs(dimensions),
        pairs(metrics)
    )
}

/// The viewer at a cell centre, `local` players around it and the rest far away.
fn project_one_viewer(population: usize, local: usize) -> Result<String> {
    let mut players = vec![player(0, [CELL / 2, CELL / 2], 0, 0)?];
    for index in 0..local {
        let offset = i32::try_from(index + 1)? * 100;
        players.push(player(players.len(), [CELL / 2 + offset, CELL / 2], 0, 0)?);
    }
    for point in far_points().into_iter().take(population - 1 - local) {
        players.push(player(players.len(), point, 0, 0)?);
    }
    let zone = zone(players)?;
    let (tests, visible) = project(&zone, 1)?;
    Ok(sample(
        &[("population", population), ("local", local)],
        &[
            ("exact_distance_tests", tests),
            ("visible_records", visible),
        ],
    ))
}

/// Clusters of 21 players, each at the centre of a cell three cells from the next.
fn publish_sparse(recipients: usize) -> Result<String> {
    let mut players = Vec::new();
    'clusters: for kz in -2..=2 {
        for kx in -2..=2 {
            for index in 0..21 {
                if players.len() == recipients {
                    break 'clusters;
                }
                let spread = i32::try_from(index)? * 40;
                let at = [kx * 3 * CELL + CELL / 2 + spread, kz * 3 * CELL + CELL / 2];
                players.push(player(players.len(), at, 0, 0)?);
            }
        }
    }
    let zone = zone(players)?;
    let mut tests = 0;
    for viewer in 1..=u32::try_from(recipients)? {
        tests += project(&zone, viewer)?.0;
    }
    Ok(sample(
        &[("recipients", recipients)],
        &[("exact_distance_tests", tests)],
    ))
}

/// Everyone inside one interest radius.
fn publish_dense_hub(recipients: usize) -> Result<String> {
    let players = (0..recipients)
        .map(|index| {
            let value = i32::try_from(index)?;
            player(
                index,
                [(value % 16) * 64 + CELL / 4, (value / 16) * 64 + CELL / 4],
                0,
                0,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let zone = zone(players)?;
    let (mut largest, mut visible) = (0, 0);
    for viewer in 1..=u32::try_from(recipients)? {
        let (tests, records) = project(&zone, viewer)?;
        largest = largest.max(tests);
        visible = visible.max(records);
    }
    Ok(sample(
        &[("recipients", recipients)],
        &[
            ("largest_recipient_tests", largest),
            ("visible_records_per_recipient", visible),
        ],
    ))
}

/// Stationary players far apart plus `moved` players running east across a cell boundary.
fn tick_maintenance(population: usize, moved: usize) -> Result<String> {
    let mut players = Vec::new();
    for point in far_points().into_iter().take(population) {
        players.push(player(players.len(), point, 0, 0)?);
    }
    for index in 0..moved {
        let row = i32::try_from(index)? * 300;
        players.push(player(
            players.len(),
            [CELL - 5, CELL / 2 + row - 4_000],
            1,
            EAST,
        )?);
    }
    let mut zone = zone(players)?;
    let before = zone.interest_maintenance_stats();
    zone.advance_tick()?;
    let after = zone.interest_maintenance_stats();
    let writes = (after.bucket_inserts - before.bucket_inserts)
        + (after.bucket_removes - before.bucket_removes)
        + (after.bucket_moves - before.bucket_moves);
    if after.bucket_moves - before.bucket_moves != moved {
        return Err("every runner must cross exactly one cell boundary".into());
    }
    Ok(sample(
        &[("population", population), ("moved", moved)],
        &[
            (
                "full_index_rebuilds",
                after.full_rebuilds - before.full_rebuilds,
            ),
            ("bucket_writes", writes),
            (
                "units_inspected",
                after.units_inspected - before.units_inspected,
            ),
        ],
    ))
}

fn main() -> Result<()> {
    let operations = [
        (
            "project-one-viewer",
            [(128, 8), (256, 8), (512, 8), (512, 4), (512, 16)]
                .into_iter()
                .map(|(population, local)| project_one_viewer(population, local))
                .collect::<Result<Vec<_>>>()?,
        ),
        (
            "publish-sparse",
            [126, 252, 504]
                .into_iter()
                .map(publish_sparse)
                .collect::<Result<Vec<_>>>()?,
        ),
        (
            "publish-dense-hub",
            [64, 128, 256]
                .into_iter()
                .map(publish_dense_hub)
                .collect::<Result<Vec<_>>>()?,
        ),
        (
            "tick-maintenance",
            [(128, 8), (256, 8), (448, 8), (448, 16), (448, 32)]
                .into_iter()
                .map(|(population, moved)| tick_maintenance(population, moved))
                .collect::<Result<Vec<_>>>()?,
        ),
    ];
    let operations = operations
        .iter()
        .map(|(id, samples)| format!("{{\"id\":\"{id}\",\"samples\":[{}]}}", samples.join(",")))
        .collect::<Vec<_>>()
        .join(",");
    println!("{{\"operations\":[{operations}]}}");
    Ok(())
}
