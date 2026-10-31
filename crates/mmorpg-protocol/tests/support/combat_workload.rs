//! Fixed Greyhaven combat scripts shared by the example and regression gate. The script
//! itself (fixtures, initial zone and commands) lives in `mmorpg-core`'s test support, which
//! the including crate root declares as `greyhaven_workload`.
use std::{error::Error, sync::Arc};

pub use crate::greyhaven_workload::{FIXTURES, Fixture, TICKS};
use crate::greyhaven_workload::{commands, initial_zone};
use mmorpg_core::{CreatureLife, ZoneEvent, ZoneSimulation};
use mmorpg_protocol::{
    MAX_PLAYER_PROJECTION_BYTES, SnapshotDecoder, encode_canonical_snapshot,
    pack_snapshot_acknowledged,
};

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub ai_evaluations: usize,
    pub physics_steps: usize,
    /// Sum of admitted bodies reported at the start of each engine step.
    pub physics_body_visits: usize,
    pub dynamic_body_visits: usize,
    pub maintenance_inspections: usize,
    pub projections: usize,
    pub candidates_tested: usize,
    pub packed_entities: usize,
    /// Player-scoped feedback records, including copies sent to different viewers.
    pub event_records: usize,
    pub damage_records: usize,
    pub death_records: usize,
    pub loot_sheets: usize,
    pub projection_bytes: usize,
    pub max_projection_bytes: usize,
    pub trace_hash: u64,
}

fn hash_record(hash: &mut u64, bytes: &[u8]) {
    // Diagnostic FNV-1a over length-prefixed payloads; direct comparisons below
    // establish recovery compatibility, rather than relying on this checksum.
    for byte in u64::try_from(bytes.len())
        .expect("payload length fits u64")
        .to_be_bytes()
        .iter()
        .chain(bytes)
    {
        *hash = (*hash ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
    }
}

pub fn measure(fixture: Fixture) -> Result<Report, Box<dyn Error>> {
    let mut zone = initial_zone(fixture)?;
    let mut reference = initial_zone(fixture)?;
    let mut report = Report {
        trace_hash: 0xcbf2_9ce4_8422_2325,
        ..Report::default()
    };
    // Each player's client acknowledges its sheets with its next command, a tick later (#544).
    let mut clients = std::collections::BTreeMap::<u32, SnapshotDecoder>::new();
    for tick in 0..TICKS {
        commands(&mut zone, fixture, tick)?;
        commands(&mut reference, fixture, tick)?;
        reference.advance_tick()?;
        let expected_ai = zone
            .snapshot()?
            .creatures
            .iter()
            .zip(zone.content().creature_spawns())
            // Breakable props (#288) decide nothing.
            .filter(|(creature, spawn)| {
                creature.life == CreatureLife::Alive
                    && zone
                        .content()
                        .creature_template(spawn.template)
                        .is_some_and(|template| template.prop().is_none())
            })
            .count();
        let maintenance_before = zone.interest_maintenance_stats();
        zone.advance_tick()?;
        let work = zone.tick_work();
        assert_eq!(work.creature_ai_evaluations, expected_ai);
        let physics = work.physics.ok_or("missing completed physics step")?;
        report.ai_evaluations += work.creature_ai_evaluations;
        report.physics_steps += 1;
        report.physics_body_visits += physics.bodies;
        report.dynamic_body_visits += physics.moving;
        report.maintenance_inspections +=
            zone.interest_maintenance_stats().units_inspected - maintenance_before.units_inspected;

        let canonical = zone.snapshot()?;
        let canonical_bytes = encode_canonical_snapshot(&canonical)?;
        assert_eq!(
            canonical_bytes,
            encode_canonical_snapshot(&reference.snapshot()?)?,
            "{} replay/recovery at tick {}",
            fixture.name,
            tick + 1
        );
        hash_record(&mut report.trace_hash, &canonical_bytes);
        for player in &canonical.players {
            let projection = zone.project_for_player(player.player_id)?;
            let client = clients.entry(player.player_id).or_default();
            let acknowledged = client.revisions();
            let packed = pack_snapshot_acknowledged(&projection.snapshot, acknowledged)?;
            let decoded = client.decode(&packed.payload)?;
            if !packed.deferred_sheets {
                // The held sheets are the zone's, timers included.
                let expected = &projection.snapshot;
                assert_eq!(
                    (&decoded.inventory, &decoded.quests, &decoded.loot),
                    (&expected.inventory, &expected.quests, &expected.loot)
                );
                assert_eq!(
                    (&decoded.cooldowns, &decoded.auras, &decoded.target_detail),
                    (
                        &expected.cooldowns,
                        &expected.auras,
                        &expected.target_detail
                    )
                );
            }
            assert_eq!(decoded.tick, u64::try_from(tick)? + 1);
            assert_eq!(decoded.viewer_id, player.player_id);
            // v15 packing drops cosmetic observations that name an entity
            // outside the packed prefix; every other event is retained.
            let packed_ids: Vec<_> = decoded
                .entities
                .iter()
                .map(|entity| entity.entity())
                .collect();
            let expected_events: Vec<_> = projection
                .snapshot
                .events
                .iter()
                .filter(|event| {
                    event.observation_refs(player.player_id).is_none_or(|refs| {
                        refs.into_iter()
                            .flatten()
                            .all(|entity| packed_ids.contains(&entity))
                    })
                })
                .cloned()
                .collect();
            assert_eq!(decoded.events, expected_events);
            assert_eq!(
                decoded.entities,
                projection.snapshot.entities[..packed.packed_entities]
            );
            assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
            assert_eq!(
                packed.payload,
                pack_snapshot_acknowledged(
                    &reference.snapshot_for_player(player.player_id)?,
                    acknowledged
                )?
                .payload
            );
            report.projections += 1;
            report.candidates_tested += projection.stats.candidates_tested;
            report.packed_entities += packed.packed_entities;
            report.event_records += decoded.events.len();
            report.damage_records += decoded
                .events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        ZoneEvent::DamageDealt { .. } | ZoneEvent::DamageTaken { .. }
                    )
                })
                .count();
            report.death_records += decoded
                .events
                .iter()
                .filter(|event| matches!(event, ZoneEvent::Died { .. }))
                .count();
            report.loot_sheets += usize::from(decoded.loot.is_some());
            report.projection_bytes += packed.payload.len();
            report.max_projection_bytes = report.max_projection_bytes.max(packed.payload.len());
            hash_record(&mut report.trace_hash, &packed.payload);
        }
        if tick + 1 == TICKS / 2 {
            reference = ZoneSimulation::from_snapshot(canonical, Arc::clone(zone.content()))?;
        }
    }
    Ok(report)
}
