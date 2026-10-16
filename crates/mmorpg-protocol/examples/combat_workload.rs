//! Deterministic complete-zone operation/byte evidence; no clock or GPU.
use std::error::Error;

#[path = "../tests/support/combat_workload.rs"]
mod combat_workload;

fn main() -> Result<(), Box<dyn Error>> {
    let content = mmorpg_core::greyhaven_vale::content();
    for fixture in combat_workload::FIXTURES {
        let r = combat_workload::measure(fixture)?;
        println!(
            "{{\"schema\":\"mmorpg.combat-workload/v1\",\"workload\":\"{}\",\"core_schema\":{},\"wire_version\":{},\"content_revision\":{},\"content_fingerprint\":\"{:016x}\",\"players\":{},\"ticks\":{},\"ai_evaluations\":{},\"physics_steps\":{},\"physics_body_visits\":{},\"dynamic_body_visits\":{},\"staged_bodies\":{},\"pair_checks\":{},\"toi_tests\":{},\"contact_resolutions\":{},\"maintenance_inspections\":{},\"projections\":{},\"candidates_tested\":{},\"packed_entities\":{},\"event_records\":{},\"damage_records\":{},\"death_records\":{},\"loot_sheets\":{},\"projection_bytes\":{},\"max_projection_bytes\":{},\"trace_hash\":\"{:016x}\",\"replay_parity\":true,\"recovery_parity\":true}}",
            fixture.name,
            mmorpg_core::SNAPSHOT_SCHEMA_VERSION,
            mmorpg_protocol::SNAPSHOT_WIRE_VERSION,
            content.revision(),
            content.fingerprint(),
            fixture.players,
            combat_workload::TICKS,
            r.ai_evaluations,
            r.physics_steps,
            r.physics_body_visits,
            r.dynamic_body_visits,
            r.staged_bodies,
            r.pair_checks,
            r.toi_tests,
            r.contact_resolutions,
            r.maintenance_inspections,
            r.projections,
            r.candidates_tested,
            r.packed_entities,
            r.event_records,
            r.damage_records,
            r.death_records,
            r.loot_sheets,
            r.projection_bytes,
            r.max_projection_bytes,
            r.trace_hash
        );
    }
    Ok(())
}
