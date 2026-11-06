//! Manual timing of the zone's whole physics path on Rapier and on physics-engine
//! (physics-engine #330). Not a gate: correctness lives in `tests/physics_ab.rs`.
//!
//! ```sh
//! cargo run --release -p mmorpg-core --example physics_ab --features physics-ab
//! cargo run --release -p mmorpg-core --example physics_ab --features physics-ab -- --rounds 9 --warmup 2
//! cargo run --release -p mmorpg-core --example physics_ab --features physics-ab -- --only vale-mill-hoist --rounds 3 --warmup 1
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
use mmorpg_core::physics::{Backend, TimedCalls, WorldOperationProfile, using_backend};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const LOCKFILE: &str = include_str!("../../../Cargo.lock");

/// Per-backend samples of one workload.
#[derive(Default)]
struct Samples {
    /// Physics-path nanoseconds per tick, all measured rounds.
    ticks: Vec<u64>,
    /// Complete command-plus-simulation tick, including nonphysics gameplay.
    whole_ticks: Vec<u64>,
    /// Physics-path nanoseconds per round.
    rounds: Vec<u64>,
    /// Complete tick nanoseconds per round.
    whole_rounds: Vec<u64>,
    /// Wall-clock nanoseconds to build the zone, and the physics share of it.
    setup: Vec<u64>,
    setup_physics: Vec<u64>,
    /// The most moving bodies and bodies any step reported.
    moving: usize,
    bodies: usize,
    contact_candidates: Vec<u64>,
    active_contacts: Vec<u64>,
    max_contact_degree: usize,
    max_active_degree: usize,
    max_incident_bodies: usize,
    controller_collisions: u64,
    operations: Option<WorldOperationProfile>,
    mutation_nanos: Vec<u64>,
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

fn changed(before: TimedCalls, after: TimedCalls) -> TimedCalls {
    TimedCalls {
        calls: after.calls - before.calls,
        nanos: after.nanos - before.nanos,
    }
}

fn operations_changed(
    before: WorldOperationProfile,
    after: WorldOperationProfile,
) -> WorldOperationProfile {
    WorldOperationProfile {
        steps: changed(before.steps, after.steps),
        removals: changed(before.removals, after.removals),
        replacements: changed(before.replacements, after.replacements),
        platform_moves: changed(before.platform_moves, after.platform_moves),
        platform_changes: after.platform_changes - before.platform_changes,
        platform_places: changed(before.platform_places, after.platform_places),
        position_sets: changed(before.position_sets, after.position_sets),
    }
}

fn mutation_nanos(profile: WorldOperationProfile) -> u64 {
    profile.removals.nanos
        + profile.replacements.nanos
        + profile.platform_moves.nanos
        + profile.platform_places.nanos
        + profile.position_sets.nanos
}

fn count_per_tick(total: u64, ticks: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mean = total as f64 / ticks.max(1) as f64;
    format!("{mean:.2}")
}

fn measure(workload: &Workload, backend: Backend, samples: &mut Samples, keep: bool) -> Result<()> {
    using_backend(backend, || {
        let started = Instant::now();
        let mut run = Run::new(workload)?;
        let setup = u64::try_from(started.elapsed().as_nanos())?;
        let setup_physics = run.zone.physics().spent_nanos();
        let start_ops = run.zone.physics().operation_profile();
        let mut ticks = Vec::with_capacity(workload.ticks());
        let mut whole_ticks = Vec::with_capacity(workload.ticks());
        let mut last_collisions = run
            .zone
            .physics()
            .rapier_contacts()
            .map(|contacts| contacts.controller_collisions)
            .unwrap_or(0);
        while !run.finished() {
            let before = run.zone.physics().spent_nanos();
            let tick_start = Instant::now();
            run.advance()?;
            let whole_tick = u64::try_from(tick_start.elapsed().as_nanos())?;
            whole_ticks.push(whole_tick);
            ticks.push(run.zone.physics().spent_nanos() - before);
            if let Some(stats) = run.zone.tick_work().physics {
                samples.moving = samples.moving.max(stats.moving);
                samples.bodies = samples.bodies.max(stats.bodies);
            }
            // Post-tick diagnostics are intentionally outside both timing windows.
            if keep && let Some(contacts) = run.zone.physics().rapier_contacts() {
                samples
                    .contact_candidates
                    .push(u64::try_from(contacts.pair_candidates)?);
                samples
                    .active_contacts
                    .push(u64::try_from(contacts.active_pairs)?);
                samples.max_contact_degree =
                    samples.max_contact_degree.max(contacts.max_body_degree);
                samples.max_active_degree =
                    samples.max_active_degree.max(contacts.max_active_degree);
                samples.max_incident_bodies =
                    samples.max_incident_bodies.max(contacts.incident_bodies);
                samples.controller_collisions += contacts.controller_collisions - last_collisions;
                last_collisions = contacts.controller_collisions;
            }
        }
        if keep {
            let ops = operations_changed(start_ops, run.zone.physics().operation_profile());
            if let Some(previous) = samples.operations {
                // The calls are deterministic even though wall-clock durations are not.
                assert_eq!(previous.steps.calls, ops.steps.calls);
                assert_eq!(previous.removals.calls, ops.removals.calls);
                assert_eq!(previous.replacements.calls, ops.replacements.calls);
                assert_eq!(previous.platform_moves.calls, ops.platform_moves.calls);
                assert_eq!(previous.platform_changes, ops.platform_changes);
                assert_eq!(previous.platform_places.calls, ops.platform_places.calls);
                assert_eq!(previous.position_sets.calls, ops.position_sets.calls);
            } else {
                samples.operations = Some(ops);
            }
            samples.mutation_nanos.push(mutation_nanos(ops));
            samples.rounds.push(ticks.iter().sum());
            samples.whole_rounds.push(whole_ticks.iter().sum());
            samples.ticks.extend(ticks);
            samples.whole_ticks.extend(whole_ticks);
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
    if rounds == 0 {
        return Err("--rounds must be positive".into());
    }
    let only = std::env::args()
        .collect::<Vec<_>>()
        .windows(2)
        .find(|args| args[0] == "--only")
        .map(|args| args[1].clone());
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
        "- sample: one 60 Hz tick including scripted inputs and all zone gameplay; physics \
         portion is timed within World calls (including debris and props); microseconds. \
         Post-tick contact inspection is excluded from both timed windows; it may still \
         perturb subsequent cache state. Setup is separate.\n"
    );
    println!(
        "| workload | ticks | moving / bodies | backend | physics median | physics p95 | complete median | complete p95 | physics share | setup (physics) | engine / rapier, median of paired rounds |"
    );
    println!("|---|---:|---:|---|---:|---:|---:|---:|---:|---:|---:|");
    let workloads = drivers::workloads()?;
    let matched: Vec<_> = workloads
        .iter()
        .filter(|workload| {
            only.as_ref().is_none_or(|needles| {
                needles
                    .split(',')
                    .any(|name| workload.name().contains(name))
            })
        })
        .collect();
    if matched.is_empty() {
        return Err(format!(
            "no workload matches --only {}",
            only.as_deref().unwrap_or("")
        )
        .into());
    }
    let mut contact_rows = Vec::new();
    for workload in matched {
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
                let samples = match backend {
                    Backend::Rapier => &mut rapier,
                    Backend::Engine => &mut engine,
                };
                measure(workload, backend, samples, keep)?;
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
            samples.ticks.sort_unstable();
            samples.whole_ticks.sort_unstable();
            samples.contact_candidates.sort_unstable();
            samples.active_contacts.sort_unstable();
            samples.mutation_nanos.sort_unstable();
            samples.setup.sort_unstable();
            samples.setup_physics.sort_unstable();
            #[allow(clippy::cast_precision_loss)]
            let share = 100.0 * samples.rounds.iter().sum::<u64>() as f64
                / samples.whole_rounds.iter().sum::<u64>().max(1) as f64;
            println!(
                "| {} | {} | {} / {} | {} | {} | {} | {} | {} | {:.1}% | {} ({}) | {} |",
                workload.name(),
                workload.ticks(),
                samples.moving,
                samples.bodies,
                backend.name(),
                micros(percentile(&samples.ticks, 50)),
                micros(percentile(&samples.ticks, 95)),
                micros(percentile(&samples.whole_ticks, 50)),
                micros(percentile(&samples.whole_ticks, 95)),
                share,
                micros(percentile(&samples.setup, 50)),
                micros(percentile(&samples.setup_physics, 50)),
                if backend == Backend::Engine {
                    format!("{ratio:.3}")
                } else {
                    String::new()
                },
            );
            let ops = samples.operations.unwrap_or_default();
            let contacts = if backend == Backend::Rapier {
                format!(
                    "{} / {} | {} / {} | {} / {} | {} | {}",
                    percentile(&samples.contact_candidates, 50),
                    samples.contact_candidates.last().copied().unwrap_or(0),
                    percentile(&samples.active_contacts, 50),
                    samples.active_contacts.last().copied().unwrap_or(0),
                    samples.max_incident_bodies,
                    samples.max_contact_degree,
                    samples.max_active_degree,
                    count_per_tick(
                        samples.controller_collisions,
                        samples.contact_candidates.len()
                    ),
                )
            } else {
                "n/a | n/a | n/a | n/a | n/a".to_owned()
            };
            contact_rows.push(format!(
                "| {} | {} | {} | {} | {} / {} / {}/{} / {} / {} | {} |",
                workload.name(),
                backend.name(),
                contacts,
                ops.steps.calls,
                ops.removals.calls,
                ops.replacements.calls,
                ops.platform_changes,
                ops.platform_moves.calls,
                ops.platform_places.calls,
                ops.position_sets.calls,
                micros(percentile(&samples.mutation_nanos, 50)),
            ));
        }
    }
    println!(
        "\n- Rapier contact candidates are unique body pairs in its narrow phase, not Physics Engine cache entries; active contacts and kinematic controller collision callbacks are different quantities. Physics Engine's character-only adapter does not call its persistent `World::step`, so no cache measurement is exposed here. Mutation timings cover successful public calls, not internal platform shifts during `step`.\n"
    );
    println!(
        "| workload | backend | Rapier pairs median/max | active median/max | incident bodies / maximum degree | maximum active degree | controller collisions/tick | steps | remove / replace / platform changed/scheduled / place platform / set position | mutation time per run (median µs) |"
    );
    println!("|---|---|---:|---:|---:|---:|---:|---:|---:|---:|");
    for row in contact_rows {
        println!("{row}");
    }
    Ok(())
}
