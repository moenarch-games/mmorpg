//! The `seal-step` workload (#1019, `.performance/contract.json`): in Greyhaven, the Sunken
//! Shrine's seal stands open past its time while `occupants` players stand in its vault, holding it
//! open, and `idle` players stand at the Outpost far from it. Each measured tick counts the
//! mechanism step's seals and wards visited and the bodies its chamber query returned. Shared by
//! `mmorpg-core`'s `seal_step` test and `seal_step_work` example. Counters only; nothing is timed.
#![allow(dead_code)]

use std::error::Error;

use mmorpg_core::greyhaven_vale::{self, shrine::vault};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation};

/// Warm ticks before the measurement.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SealStepRun {
    pub mechanisms_visited: usize,
    pub seal_bodies_checked: usize,
    /// Whether the seal still stood open after the last measured tick.
    pub open: bool,
}

/// The zone of the workload, warmed up: the seal opened by its two glyphs and due to close,
/// players `1..=occupants` in a row across its vault and the rest of `1..=occupants + idle` at the
/// Outpost. The warden stands in its vault as ever.
pub fn warm_zone(occupants: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = greyhaven_vale::content();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone())?;
    let players = u32::try_from(occupants + idle)?;
    for player in 1..=players {
        zone.add_player(player)?;
    }
    zone.advance_tick()?;
    let glyph = |id| {
        content
            .mechanism(id)
            .map(|mechanism| mechanism.at)
            .ok_or("the shrine has its glyphs")
    };
    // Player 1 works both glyphs, standing beside each in turn.
    for (sequence, id) in [(1, vault::LAKEWARD_GLYPH), (2, vault::CAVE_GLYPH)] {
        let at = glyph(id)?;
        let mut state = zone.snapshot()?;
        state.players[0].position = [at[0] as f32, (at[1] + 90) as f32, at[2] as f32];
        zone = ZoneSimulation::from_snapshot(state, content.clone())?;
        zone.apply_command(
            1,
            sequence,
            ZoneCommand::Operate {
                mechanism: id.get(),
            },
        )?;
        zone.advance_tick()?;
    }
    if !zone.seal_views().first().is_some_and(|seal| seal.open) {
        return Err("the glyphs open the seal".into());
    }
    let mut state = zone.snapshot()?;
    // Five minutes on, the seal is due to close.
    state.tick += u64::from(mmorpg_core::SEAL_OPEN_TICKS);
    let ([x0, _, z0], [x1, _, _]) = content.seals()[0].chamber;
    let spacing = (x1 - x0) / i32::try_from(occupants + 1)?;
    for (index, record) in state.players.iter_mut().enumerate() {
        if index < occupants {
            let x = x0 + spacing * i32::try_from(index + 1)?;
            record.position = [x as f32, 290.0, (z0 + 400) as f32];
        }
    }
    let mut zone = ZoneSimulation::from_snapshot(state, content)?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(occupants: usize, idle: usize) -> Result<SealStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(occupants, idle)?;
    let mut run = SealStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        let work = zone.tick_work();
        run.mechanisms_visited = run.mechanisms_visited.max(work.mechanisms_visited);
        run.seal_bodies_checked = run.seal_bodies_checked.max(work.seal_bodies_checked);
    }
    run.open = zone.seal_views().first().is_some_and(|seal| seal.open);
    Ok(run)
}

/// One `seal-step` sample in the work-evidence shape.
pub fn sample(occupants: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(occupants, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"occupants\":{occupants},\"idle\":{idle}}},\"metrics\":{{\"mechanisms_visited\":{},\"seal_bodies_checked\":{}}}}}",
        run.mechanisms_visited, run.seal_bodies_checked
    ))
}

/// The contract's scale points: (occupants, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(1, 16), (2, 16), (4, 16), (2, 64), (2, 160)];
