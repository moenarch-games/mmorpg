//! Real zone calls with separately measured publication/checkpoint work.
//! Timings are advisory. Optional traces prove byte compatibility across pins.
use std::{
    error::Error,
    fs::{self, File},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use mmorpg_core::{
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, PLAYER_HALF_EXTENTS_UNITS, ZoneCommand,
    ZoneDefinition, ZoneId, ZoneSimulation, greyhaven_vale, greyhaven_vale_definition,
};
use mmorpg_protocol::{encode_canonical_snapshot, pack_snapshot};

#[path = "../tests/support/visibility_oracle.rs"]
mod visibility_oracle;

const TICKS: usize = 120;
const WORKLOADS: [(&str, usize, usize); 8] = [
    ("quiet-128", 128, 0),
    ("quiet-512", 512, 0),
    ("sparse-512-k1", 512, 1),
    ("sparse-512-k8", 512, 8),
    ("supported-512", 512, 0),
    ("crowded-64", 64, 0),
    ("mutation-recovery-32", 32, 4),
    ("vale-units-16", 16, 1),
];

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let trace_dir = match args.as_slice() {
        [] => None,
        [flag, directory] if flag == "--trace" => Some(PathBuf::from(directory)),
        _ => return Err("usage: physics_workload [--trace DIRECTORY]".into()),
    };
    if let Some(directory) = &trace_dir {
        fs::create_dir_all(directory)?;
    }
    for (name, count, moving) in WORKLOADS {
        for trial in 0..3 {
            measure(name, count, moving, trial, trace_dir.as_deref())?;
        }
    }
    Ok(())
}

fn initial_zone(name: &str, count: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let empty = if name == "vale-units-16" {
        ZoneSimulation::with_content(ZoneId::new(1), greyhaven_vale::content())?
    } else {
        let definition = if name == "supported-512" {
            greyhaven_vale_definition()
        } else {
            ZoneDefinition::default()
        };
        ZoneSimulation::with_definition(ZoneId::new(1), definition)?
    };
    let mut canonical = empty.snapshot()?;
    for index in 0..count {
        let value = i32::try_from(index)?;
        let position = if matches!(name, "supported-512" | "vale-units-16") {
            let feet = greyhaven_vale::SPAWN_GRID
                .feet(u16::try_from(index)?)
                .ok_or("spawn slot overflows")?;
            [feet[0], PLAYER_HALF_EXTENTS_UNITS[1], feet[2]]
        } else {
            let spacing = if name == "crowded-64" { 55 } else { 900 };
            [
                (value % 23) * spacing - 10_000,
                PLAYER_HALF_EXTENTS_UNITS[1],
                (value / 23) * spacing - 10_000,
            ]
        };
        canonical.players.push(CanonicalPlayerSnapshot {
            copper: 0,
            player_id: u32::try_from(index)? + 1,
            position,
            velocity: [0; 3],
            facing: 0,
            forward: 0,
            strafe: 0,
            jump_pending: false,
            last_sequence: 0,
            spawn_slot: u16::try_from(index)?,
            inventory: mmorpg_core::Inventory::default(),
            inventory_revision: 1,
            inventory_changed_at: 0,
            equipment: mmorpg_core::Equipment::default(),
            combat: CanonicalPlayerCombat::default(),
            chat_ready_at: 0,
            chat: Vec::new(),
            quests: mmorpg_core::QuestLog::default(),
            quests_changed_at: 0,
        });
    }
    Ok(ZoneSimulation::from_snapshot(
        canonical,
        Arc::clone(empty.content()),
    )?)
}

fn commands(
    zone: &mut ZoneSimulation,
    name: &str,
    moving: usize,
    tick: usize,
) -> Result<(), Box<dyn Error>> {
    if tick.is_multiple_of(20) {
        // Repeated zero intent is representative of a stationary controller.
        for index in 0..moving.max(1) {
            let facing = if tick.is_multiple_of(40) {
                16_384
            } else {
                49_152
            };
            zone.apply_command(
                u32::try_from(index)? + 1,
                u32::try_from(tick)? + 1,
                ZoneCommand::Move {
                    forward: i8::from(moving != 0),
                    strafe: 0,
                    facing,
                },
            )?;
        }
    }
    if name == "mutation-recovery-32" {
        if tick == 25 {
            assert!(zone.remove_player(32));
        } else if tick == 26 {
            zone.add_player(32)?;
        } else if tick == 45 {
            let mut state = zone.snapshot()?;
            state.players[0].position = [-4_510, 90, -4_510];
            *zone = ZoneSimulation::from_snapshot(state, Arc::clone(zone.content()))?;
        }
    }
    if name == "supported-512" && tick == 30 {
        zone.apply_command(1, 32, ZoneCommand::Jump)?;
    }
    Ok(())
}

fn measure(
    name: &str,
    count: usize,
    moving: usize,
    trial: usize,
    trace_dir: Option<&Path>,
) -> Result<(), Box<dyn Error>> {
    let mut zone = initial_zone(name, count)?;
    let mut recovered = None;
    let mut trace = trace_dir
        .map(|directory| File::create(directory.join(format!("{name}-{trial}.bin"))))
        .transpose()?
        .map(BufWriter::new);
    let mut tick_time = Duration::ZERO;
    let mut projection_time = Duration::ZERO;
    let mut encoding_time = Duration::ZERO;
    let mut checkpoint_time = Duration::ZERO;
    let mut projection_bytes = 0;
    let mut canonical_bytes = 0;
    let mut checksum = 0xcbf2_9ce4_8422_2325_u64;
    let mut inspected = 0;
    let mut physics_queries = 0;
    let mut sweep_preparations = 0;
    let mut fixed_preparations = 0;
    let mut fixed_reuses = 0;
    let mut dynamic_preparations = 0;
    let mut staging_growths = 0;
    let mut broad_phase_growths = 0;
    let mut fixed_cache_peak_bytes = 0;
    let mut staged_bodies = 0;
    let mut body_map_rebuilds = 0;
    for tick in 0..TICKS {
        commands(&mut zone, name, moving, tick)?;
        if let Some(reference) = &mut recovered {
            commands(reference, name, moving, tick)?;
            reference.advance_tick()?;
        }
        let before = zone.interest_maintenance_stats();
        let start = Instant::now();
        zone.advance_tick()?;
        tick_time += start.elapsed();
        let work = zone
            .tick_work()
            .physics
            .ok_or("missing physics diagnostics")?
            .work;
        physics_queries += work.broad_phase_queries;
        sweep_preparations += work.sweep_bound_preparations;
        fixed_preparations += work.fixed_sweep_bound_preparations;
        fixed_reuses += work.fixed_sweep_bound_reuses;
        dynamic_preparations += work.dynamic_sweep_bound_preparations;
        staging_growths += work.staged_state_capacity_growths;
        broad_phase_growths += work.broad_phase_capacity_growths;
        fixed_cache_peak_bytes = fixed_cache_peak_bytes.max(work.fixed_bound_cache_capacity_bytes);
        staged_bodies += work.staged_bodies;
        body_map_rebuilds += work.body_map_rebuilds;
        inspected += zone.interest_maintenance_stats().units_inspected - before.units_inspected;
        let start = Instant::now();
        let canonical = zone.snapshot()?;
        let encoded = encode_canonical_snapshot(&canonical)?;
        checkpoint_time += start.elapsed();
        if let Some(reference) = &recovered {
            assert_eq!(
                canonical,
                reference.snapshot()?,
                "{name}: recovery at {tick}"
            );
        }
        if tick == 59 {
            recovered = Some(ZoneSimulation::from_snapshot(
                canonical.clone(),
                Arc::clone(zone.content()),
            )?);
        }
        canonical_bytes += encoded.len();
        record(&encoded, &mut trace, &mut checksum)?;
        // Eight recipients per tick; every recipient at the completed horizon.
        let recipients = if tick + 1 == TICKS { count } else { 8 };
        for observer in canonical.players.iter().take(recipients) {
            let start = Instant::now();
            let projection = zone.project_for_player(observer.player_id)?;
            projection_time += start.elapsed();
            let start = Instant::now();
            let payload = pack_snapshot(&projection.snapshot)?.payload;
            encoding_time += start.elapsed();
            if name != "vale-units-16" {
                let exhaustive = visibility_oracle::exhaustive_projection(&canonical, observer);
                assert_eq!(payload, pack_snapshot(&exhaustive)?.payload);
            }
            projection_bytes += payload.len();
            record(&payload, &mut trace, &mut checksum)?;
        }
    }
    if let Some(trace) = &mut trace {
        trace.flush()?;
    }
    println!(
        "{{\"schema\":\"mmorpg.physics-workload/v1\",\"workload\":\"{name}\",\"trial\":{trial},\"players\":{count},\"moving_controllers\":{moving},\"completed_ticks\":{TICKS},\"tick_ms\":{},\"projection_ms\":{},\"encoding_ms\":{},\"checkpoint_ms\":{},\"projection_bytes\":{projection_bytes},\"canonical_bytes\":{canonical_bytes},\"trace_fnv1a64\":\"{checksum:016x}\",\"units_inspected_for_maintenance\":{inspected},\"physics_queries\":{physics_queries},\"sweep_bound_preparations\":{sweep_preparations},\"fixed_bound_preparations\":{fixed_preparations},\"fixed_bound_reuses\":{fixed_reuses},\"dynamic_bound_preparations\":{dynamic_preparations},\"staged_bodies\":{staged_bodies},\"body_map_rebuilds\":{body_map_rebuilds},\"staging_capacity_growths\":{staging_growths},\"broad_phase_capacity_growths\":{broad_phase_growths},\"fixed_cache_peak_payload_bytes\":{fixed_cache_peak_bytes}}}",
        tick_time.as_secs_f64() * 1_000.0,
        projection_time.as_secs_f64() * 1_000.0,
        encoding_time.as_secs_f64() * 1_000.0,
        checkpoint_time.as_secs_f64() * 1_000.0,
    );
    Ok(())
}

fn record(
    payload: &[u8],
    trace: &mut Option<BufWriter<File>>,
    checksum: &mut u64,
) -> Result<(), Box<dyn Error>> {
    let length = u64::try_from(payload.len())?.to_be_bytes();
    for byte in length.iter().chain(payload) {
        *checksum ^= u64::from(*byte);
        *checksum = checksum.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if let Some(trace) = trace {
        trace.write_all(&length)?;
        trace.write_all(payload)?;
    }
    Ok(())
}
