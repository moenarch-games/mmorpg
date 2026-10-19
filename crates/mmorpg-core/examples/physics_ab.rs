//! Manual timing of the zone's whole physics path on Rapier and on physics-engine
//! (physics-engine #330). Not a gate: correctness lives in `tests/physics_ab.rs`.
//!
//! ```sh
//! cargo run --release -p mmorpg-core --example physics_ab --features physics-ab
//! cargo run --release -p mmorpg-core --example physics_ab --features physics-ab -- --rounds 9 --warmup 2
//! ```
//!
//! Each workload (the A/B test's drivers) runs `warmup + rounds` times per backend from a
//! fresh zone, alternating which backend goes first each round so the pairs share machine
//! state. A sample is one tick's physics path: the wall-clock time the zone spent inside
//! `physics::World` calls while applying the tick's inputs and advancing it (the step, every
//! gameplay query, body updates), as `World::spent_nanos` counts it. Debris (#288) stays on
//! Rapier in both arms and is excluded. Setup (building the zone, physics included) is
//! timed separately.

// Shared with the A/B test, which also checks scenario expectations and recovery.
#[allow(dead_code)]
#[path = "../tests/support/physics_ab.rs"]
mod drivers;
#[path = "../tests/support/greyhaven_workload.rs"]
mod greyhaven_workload;

use std::error::Error;
use std::time::Instant;

use drivers::{Run, Workload};
use mmorpg_core::physics::{Backend, using_backend};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const LOCKFILE: &str = include_str!("../../../Cargo.lock");

/// Per-backend samples of one workload.
#[derive(Default)]
struct Samples {
    /// Physics-path nanoseconds per tick, all measured rounds.
    ticks: Vec<u64>,
    /// Physics-path nanoseconds per round.
    rounds: Vec<u64>,
    /// Wall-clock nanoseconds to build the zone, and the physics share of it.
    setup: Vec<u64>,
    setup_physics: Vec<u64>,
    /// The most moving bodies and bodies any step reported.
    moving: usize,
    bodies: usize,
}

/// The nearest-rank percentile of sorted samples.
fn percentile(sorted: &[u64], percent: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) * percent).div_ceil(100)]
}

fn micros(nanos: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let micros = nanos as f64 / 1_000.0;
    format!("{micros:.1}")
}

fn measure(workload: &Workload, backend: Backend, samples: &mut Samples, keep: bool) -> Result<()> {
    using_backend(backend, || {
        let started = Instant::now();
        let mut run = Run::new(workload)?;
        let setup = u64::try_from(started.elapsed().as_nanos())?;
        let setup_physics = run.zone.physics().spent_nanos();
        let mut ticks = Vec::with_capacity(workload.ticks());
        while !run.finished() {
            let before = run.zone.physics().spent_nanos();
            run.advance()?;
            ticks.push(run.zone.physics().spent_nanos() - before);
            if let Some(stats) = run.zone.tick_work().physics {
                samples.moving = samples.moving.max(stats.moving);
                samples.bodies = samples.bodies.max(stats.bodies);
            }
        }
        if keep {
            samples.rounds.push(ticks.iter().sum());
            samples.ticks.extend(ticks);
            samples.setup.push(setup);
            samples.setup_physics.push(setup_physics);
        }
        Ok(())
    })
}

fn command(program: &str, arguments: &[&str]) -> String {
    std::process::Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |output| String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        )
}

/// A locked package's version and source lines.
fn locked(package: &str) -> String {
    LOCKFILE
        .split("[[package]]")
        .find(|entry| entry.contains(&format!("name = \"{package}\"\n")))
        .map_or_else(
            || "missing".to_owned(),
            |entry| {
                entry
                    .lines()
                    .filter(|line| line.starts_with("version = ") || line.starts_with("source = "))
                    .collect::<Vec<_>>()
                    .join(" ")
            },
        )
}

fn argument(name: &str, default: usize) -> Result<usize> {
    let arguments: Vec<String> = std::env::args().collect();
    match arguments.iter().position(|argument| argument == name) {
        Some(index) => Ok(arguments
            .get(index + 1)
            .ok_or_else(|| format!("{name} needs a value"))?
            .parse()?),
        None => Ok(default),
    }
}

fn main() -> Result<()> {
    let rounds = argument("--rounds", 7)?;
    let warmup = argument("--warmup", 2)?;
    let dirty = !command("git", &["status", "--porcelain", "--untracked-files=no"]).is_empty();
    println!("# Physics backend A/B timing (physics-engine #330)\n");
    println!(
        "- mmorpg: {}{}",
        command("git", &["rev-parse", "HEAD"]),
        if dirty { " (dirty)" } else { "" }
    );
    println!("- physics-engine: {}", locked("physics-engine"));
    println!("- rapier3d: {}", locked("rapier3d"));
    println!("- rustc: {}", command("rustc", &["--version"]));
    println!(
        "- profile: {}; {warmup} warmup + {rounds} measured rounds per backend, alternating the \
         first backend each round",
        if cfg!(debug_assertions) {
            "debug (not representative)"
        } else {
            "release"
        }
    );
    println!(
        "- sample: one tick's whole physics path (step, gameplay queries, body updates), debris \
         excluded; microseconds; setup is building the zone, its physics share in parentheses\n"
    );
    println!(
        "| workload | ticks | moving / bodies | backend | median | p95 | p99 | mean | setup (physics) | engine / rapier, median of paired rounds |"
    );
    println!("|---|---:|---:|---|---:|---:|---:|---:|---:|---:|");
    for workload in drivers::workloads()? {
        let mut rapier = Samples::default();
        let mut engine = Samples::default();
        for round in 0..warmup + rounds {
            let keep = round >= warmup;
            let order = if round % 2 == 0 {
                [Backend::Rapier, Backend::Engine]
            } else {
                [Backend::Engine, Backend::Rapier]
            };
            for backend in order {
                // A workload physics-engine cannot run is timed on Rapier only.
                if backend == Backend::Engine && workload.engine_unsupported().is_some() {
                    continue;
                }
                let samples = match backend {
                    Backend::Rapier => &mut rapier,
                    Backend::Engine => &mut engine,
                };
                measure(&workload, backend, samples, keep)?;
            }
        }
        #[allow(clippy::cast_precision_loss)]
        let mut ratios: Vec<f64> = engine
            .rounds
            .iter()
            .zip(&rapier.rounds)
            .map(|(&engine, &rapier)| engine as f64 / rapier.max(1) as f64)
            .collect();
        ratios.sort_by(f64::total_cmp);
        let ratio = ratios.get(ratios.len() / 2).copied().unwrap_or(f64::NAN);
        for (backend, samples) in [
            (Backend::Rapier, &mut rapier),
            (Backend::Engine, &mut engine),
        ] {
            if backend == Backend::Engine
                && let Some(missing) = workload.engine_unsupported()
            {
                println!(
                    "| {} | {} | | {} | unsupported: {missing} | | | | | |",
                    workload.name(),
                    workload.ticks(),
                    backend.name(),
                );
                continue;
            }
            samples.ticks.sort_unstable();
            samples.setup.sort_unstable();
            samples.setup_physics.sort_unstable();
            let mean =
                samples.ticks.iter().sum::<u64>() / u64::try_from(samples.ticks.len().max(1))?;
            println!(
                "| {} | {} | {} / {} | {} | {} | {} | {} | {} | {} ({}) | {} |",
                workload.name(),
                workload.ticks(),
                samples.moving,
                samples.bodies,
                backend.name(),
                micros(percentile(&samples.ticks, 50)),
                micros(percentile(&samples.ticks, 95)),
                micros(percentile(&samples.ticks, 99)),
                micros(mean),
                micros(percentile(&samples.setup, 50)),
                micros(percentile(&samples.setup_physics, 50)),
                if backend == Backend::Engine {
                    format!("{ratio:.3}")
                } else {
                    String::new()
                },
            );
        }
    }
    Ok(())
}
