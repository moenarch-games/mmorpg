//! Wall-clock runtime of the named zone scenarios in `.performance/runtime.json` (#1256), read by
//! `scripts/runtime_budget.py`. Unlike the `*_work` examples this one is timed: it measures how
//! long a Greyhaven zone takes to advance a tick at a few populations, and to project and encode
//! every viewer's snapshot once (a publication round). Timings decide budgets only, never
//! correctness (Atlas 7.1.i1256).
//!
//! Prints one JSON object: `{"schema": "mmorpg.runtime-scenarios/v1", "scenarios": {id: metrics}}`,
//! each metric a `{"p50", "p95"}` distribution in milliseconds.
use std::error::Error;
use std::time::Instant;

use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale};
use mmorpg_protocol::pack_snapshot;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Ticks advanced before timing starts: creatures wake and runners leave the spawn grid.
const WARM_TICKS: usize = 40;
/// Timed ticks per population.
const TIMED_TICKS: usize = 240;
/// Players in the timed populations.
const POPULATIONS: [u32; 3] = [16, 64, 256];

/// `samples` in milliseconds as `{"p50": …, "p95": …}`, nearest rank.
fn distribution(mut samples: Vec<f64>) -> String {
    samples.sort_by(f64::total_cmp);
    let rank = |p: f64| {
        let last = samples.len().saturating_sub(1);
        // `last` is a small sample count, exact in f64; the rounded rank is within 0..=last.
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let index = ((p / 100.0) * last as f64).round() as usize;
        samples.get(index.min(last)).copied().unwrap_or(0.0)
    };
    format!("{{\"p50\":{:.4},\"p95\":{:.4}}}", rank(50.0), rank(95.0))
}

fn millis(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1_000.0
}

/// A Greyhaven zone with `players` players running from their spawn slots in eight headings,
/// advanced past `WARM_TICKS`.
fn populated(players: u32) -> Result<ZoneSimulation> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content())?;
    for player in 1..=players {
        zone.add_player(player)?;
        let facing = u16::try_from(player % 8)? * 8_192;
        zone.apply_command(
            player,
            1,
            ZoneCommand::Move {
                forward: 1,
                strafe: 0,
                facing,
            },
        )?;
    }
    for _ in 0..WARM_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Milliseconds per tick of a populated Greyhaven zone.
fn zone_tick(zone: &mut ZoneSimulation) -> Result<String> {
    let mut ticks = Vec::with_capacity(TIMED_TICKS);
    for _ in 0..TIMED_TICKS {
        let start = Instant::now();
        zone.advance_tick()?;
        ticks.push(millis(start));
    }
    Ok(format!("{{\"tick_ms\":{}}}", distribution(ticks)))
}

/// Publication rounds timed: each projects and encodes every player's snapshot, then advances a tick.
const PUBLISH_ROUNDS: usize = 30;

/// Milliseconds to project and encode every player's snapshot once (a cold publication round, as
/// one zone-host tick publishes), over `PUBLISH_ROUNDS` ticks.
fn publish(zone: &mut ZoneSimulation, players: u32) -> Result<String> {
    let mut rounds = Vec::with_capacity(PUBLISH_ROUNDS);
    for _ in 0..PUBLISH_ROUNDS {
        let start = Instant::now();
        for viewer in 1..=players {
            let projection = zone.project_for_player(viewer)?;
            std::hint::black_box(pack_snapshot(&projection.snapshot)?);
        }
        rounds.push(millis(start));
        zone.advance_tick()?;
    }
    Ok(format!("{{\"round_ms\":{}}}", distribution(rounds)))
}

fn main() -> Result<()> {
    let mut scenarios = Vec::new();
    for players in POPULATIONS {
        let mut zone = populated(players)?;
        scenarios.push(format!(
            "\"zone-tick-greyhaven-{players}\":{}",
            zone_tick(&mut zone)?
        ));
        if players == POPULATIONS[POPULATIONS.len() - 1] {
            scenarios.push(format!(
                "\"publish-greyhaven-{players}\":{}",
                publish(&mut zone, players)?
            ));
        }
    }
    println!(
        "{{\"schema\":\"mmorpg.runtime-scenarios/v1\",\"scenarios\":{{{}}}}}",
        scenarios.join(",")
    );
    Ok(())
}
