//! Deterministic operation/byte counts, not a wall-clock throughput benchmark.
use std::error::Error;

use std::sync::Arc;

use mmorpg_core::{
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, INTEREST_RADIUS_UNITS, MAX_PLAYERS_PER_ZONE,
    PLAYER_HALF_EXTENTS_UNITS, SNAPSHOT_SCHEMA_VERSION, ZoneDefinition, ZoneId, ZoneSimulation,
    greyhaven_vale, greyhaven_vale_definition,
};
use mmorpg_protocol::{SNAPSHOT_WIRE_VERSION, encode_canonical_snapshot, pack_snapshot};
#[path = "../tests/support/visibility_oracle.rs"]
mod visibility_oracle;
use visibility_oracle::exhaustive_projection;

fn main() -> Result<(), Box<dyn Error>> {
    for name in ["sparse-grid-512", "dense-hub-512", "vale-spawn-512"] {
        measure(name)?;
    }
    Ok(())
}

fn measure(name: &str) -> Result<(), Box<dyn Error>> {
    // The vale workload places players on the vale's collision content; the
    // interest workloads measure player visibility only, without its units.
    let definition = if name == "vale-spawn-512" {
        greyhaven_vale_definition()
    } else {
        ZoneDefinition::default()
    };
    let empty = ZoneSimulation::with_definition(ZoneId::new(1), definition)?;
    let mut canonical = empty.snapshot()?;
    // Feet rest on y = 0, the vale ground; the empty workloads have no gravity.
    let y = PLAYER_HALF_EXTENTS_UNITS[1];
    for index in 0..MAX_PLAYERS_PER_ZONE {
        let value = i32::try_from(index)?;
        let position = match name {
            // 23 columns 28 m apart span ±308 m, inside the compact wire range.
            "sparse-grid-512" => [
                (value % 23) * 2_800 - 30_800,
                y,
                (value / 23) * 2_800 - 30_800,
            ],
            "dense-hub-512" => [(value % 23) * 64 - 704, y, (value / 23) * 64 - 704],
            "vale-spawn-512" => {
                let feet = greyhaven_vale::SPAWN_GRID
                    .feet(u16::try_from(index)?)
                    .ok_or("spawn slot overflows")?;
                [feet[0], y, feet[2]]
            }
            _ => return Err("unknown workload".into()),
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
    let mut zone = ZoneSimulation::from_snapshot(canonical, Arc::clone(empty.content()))?;
    // Exclude checkpoint construction from the measured tick maintenance.
    let before = zone.interest_maintenance_stats();
    zone.advance_tick()?;
    let after = zone.interest_maintenance_stats();
    let rebuilds = after.full_rebuilds - before.full_rebuilds;
    let inserts = after.bucket_inserts - before.bucket_inserts;
    let removes = after.bucket_removes - before.bucket_removes;
    let moves = after.bucket_moves - before.bucket_moves;
    let inspected = after.units_inspected - before.units_inspected;
    if name == "sparse-grid-512" && (rebuilds != 0 || inserts != 0 || removes != 0 || moves != 0) {
        return Err("stationary workload rewrote bucket memberships".into());
    }
    let canonical = zone.snapshot()?;
    let canonical_bytes = encode_canonical_snapshot(&canonical)?.len();
    let mut candidates = 0;
    let mut cells = 0;
    let mut visible = 0;
    let mut relevant = 0;
    let mut bytes = 0;
    let mut max_bytes = 0;
    for observer in &canonical.players {
        let projection = zone.project_for_player(observer.player_id)?;
        let baseline = exhaustive_projection(&canonical, observer);
        let encoded = pack_snapshot(&projection.snapshot)?.payload;
        if encoded != pack_snapshot(&baseline)?.payload {
            return Err(format!("{name}: projection differs for {}", observer.player_id).into());
        }
        candidates += projection.stats.candidates_tested;
        cells += projection.stats.cells_visited;
        visible += pack_snapshot(&projection.snapshot)?.packed_entities;
        relevant += projection.stats.relevant;
        bytes += encoded.len();
        max_bytes = max_bytes.max(encoded.len());
    }
    let players = canonical.players.len();
    let baseline_tests = players * players;
    let revision = canonical.content_revision;
    println!(
        "{{\"schema\":\"mmorpg.interest-workload/v4\",\"workload\":\"{name}\",\"core_schema\":{SNAPSHOT_SCHEMA_VERSION},\"wire_version\":{SNAPSHOT_WIRE_VERSION},\"content_revision\":{revision},\"radius_units\":{INTEREST_RADIUS_UNITS},\"players\":{players},\"full_index_rebuilds\":{rebuilds},\"bucket_inserts\":{inserts},\"bucket_removes\":{removes},\"bucket_moves\":{moves},\"units_inspected_for_maintenance\":{inspected},\"baseline_distance_tests\":{baseline_tests},\"exact_distance_tests\":{candidates},\"query_bucket_visits\":{cells},\"relevant_records\":{relevant},\"visible_records\":{visible},\"snapshot_payload_bytes\":{bytes},\"largest_snapshot_payload_bytes\":{max_bytes},\"canonical_payload_bytes\":{canonical_bytes},\"wire_parity\":true}}"
    );
    Ok(())
}
