//! The `carry-step` workload (#1077, `.performance/contract.json`): on wide flat ground,
//! `carriers` players each hold a sack in both arms while `idle` players stand about empty-handed.
//! Each measured tick counts the carry step's work: loads visited. Shared by `mmorpg-core`'s
//! `carry_step` test and `carry_step_work` example. Counters only; nothing is timed.
#![allow(dead_code)]

use std::{error::Error, sync::Arc};

use mmorpg_core::{
    Area, AreaId, LoadKind, LoadSpot, LoadSpotId, LoadState, SpawnGrid, StaticCollider, ZoneAreas,
    ZoneContent, ZoneDefinition, ZoneId, ZoneSimulation,
};

const SOURCE: LoadSpotId = LoadSpotId::new(1);
/// Warm ticks before the measurement: every body has settled on the ground.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CarryStepRun {
    pub loads_visited: usize,
    /// Players still carrying after the last measured tick.
    pub carrying: usize,
}

fn content() -> Result<Arc<ZoneContent>, Box<dyn Error>> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-3_200, -3_200],
            columns: 64,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [20_000, 50, 20_000],
        }],
    )?;
    let areas = ZoneAreas::new(vec![Area::new(
        AreaId::new(1),
        "Work Yard",
        [-15_000, -15_000],
        [15_000, 15_000],
    )?])?;
    let content = ZoneContent::new(
        definition,
        areas,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        [0, 0],
    )?
    .with_loads(vec![LoadSpot {
        id: SOURCE,
        name: "the work sack".into(),
        kind: LoadKind::Sack,
        at: [0, 0, 0],
        reach: 300,
    }])?;
    Ok(Arc::new(content))
}

/// The zone of the workload, warmed up: players `1..=carriers` carry a sack, the next `idle`
/// stand empty-handed.
pub fn warm_zone(carriers: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = content()?;
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content))?;
    for player in 1..=carriers + idle {
        zone.add_player(u32::try_from(player)?)?;
    }
    zone.advance_tick()?;
    let mut state = zone.snapshot()?;
    for player in 1..=carriers {
        state.loads.push(LoadState {
            player: u32::try_from(player)?,
            source: SOURCE,
            placed: None,
        });
    }
    let mut zone = ZoneSimulation::from_snapshot(state, content)?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(carriers: usize, idle: usize) -> Result<CarryStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(carriers, idle)?;
    let mut run = CarryStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        run.loads_visited = run.loads_visited.max(zone.tick_work().loads_visited);
    }
    run.carrying = (1..=carriers + idle)
        .filter(|&player| {
            u32::try_from(player).is_ok_and(|player| zone.carried_load(player).is_some())
        })
        .count();
    Ok(run)
}

/// One `carry-step` sample in the work-evidence shape.
pub fn sample(carriers: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(carriers, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"carriers\":{carriers},\"idle\":{idle}}},\"metrics\":{{\"loads_visited\":{}}}}}",
        run.loads_visited
    ))
}

/// The contract's scale points: (carriers, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(4, 16), (16, 16), (64, 16), (16, 64), (16, 160)];
