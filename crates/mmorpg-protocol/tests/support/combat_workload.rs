//! Fixed Greyhaven combat scripts shared by the example and regression gate.
use std::{error::Error, sync::Arc};

use mmorpg_core::{
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, CreatureLife, EntityRef,
    PLAYER_HALF_EXTENTS_UNITS, ZoneCommand, ZoneEvent, ZoneId, ZoneSimulation, greyhaven_vale,
};
use mmorpg_protocol::{
    MAX_PLAYER_PROJECTION_BYTES, decode_snapshot, encode_canonical_snapshot, pack_snapshot,
};

pub const TICKS: usize = 360;

#[derive(Clone, Copy)]
pub struct Fixture {
    pub name: &'static str,
    pub players: usize,
    pub fighting: bool,
    pub crowded: bool,
}

pub const FIXTURES: [Fixture; 4] = [
    Fixture {
        name: "vale-idle-16",
        players: 16,
        fighting: false,
        crowded: false,
    },
    Fixture {
        name: "vale-fights-8",
        players: 8,
        fighting: true,
        crowded: false,
    },
    Fixture {
        name: "vale-fights-32",
        players: 32,
        fighting: true,
        crowded: false,
    },
    Fixture {
        name: "vale-crowded-fights-64",
        players: 64,
        fighting: true,
        crowded: true,
    },
];

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub ai_evaluations: usize,
    pub physics_steps: usize,
    /// Sum of admitted bodies reported at the start of each engine step.
    pub physics_body_visits: usize,
    pub dynamic_body_visits: usize,
    pub staged_bodies: usize,
    pub pair_checks: usize,
    pub toi_tests: usize,
    pub contact_resolutions: usize,
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

fn initial_zone(fixture: Fixture) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = greyhaven_vale::content();
    let empty = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content))?;
    let mut state = empty.snapshot()?;
    for index in 0..fixture.players {
        let position = if !fixture.fighting {
            let feet = greyhaven_vale::SPAWN_GRID
                .feet(u16::try_from(index)?)
                .ok_or("spawn overflow")?;
            [feet[0], PLAYER_HALF_EXTENTS_UNITS[1], feet[2]]
        } else {
            // Spread across the full authored spawn table, or concentrate around
            // the six farm rats. Checkpoint placement is fixture setup only;
            // all subsequent motion and combat go through commands and physics.
            let spawn = if fixture.crowded {
                content
                    .creature_spawns()
                    .iter()
                    .find(|spawn| {
                        spawn.id.get() == 200 + u32::try_from(index % 6).expect("six rat IDs")
                    })
                    .ok_or("missing rat")?
            } else {
                &content.creature_spawns()
                    [index * content.creature_spawns().len() / fixture.players]
            };
            let row = i32::try_from(index / 6)?;
            [
                spawn.position[0] + 150 + row * 65,
                PLAYER_HALF_EXTENTS_UNITS[1],
                spawn.position[1],
            ]
        };
        state.players.push(CanonicalPlayerSnapshot {
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
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

fn commands(
    zone: &mut ZoneSimulation,
    fixture: Fixture,
    tick: usize,
) -> Result<(), Box<dyn Error>> {
    if !fixture.fighting {
        return Ok(());
    }
    let state = zone.snapshot()?;
    for player in &state.players {
        let sequence = u32::try_from(tick)? * 4 + 1;
        if tick.is_multiple_of(60) {
            if player.combat.health == 0 {
                zone.apply_command(player.player_id, sequence, ZoneCommand::ReleaseSpirit)?;
                continue;
            }
            // A deterministic bot target policy over canonical fixture state.
            // Core still decides visibility, attack validity, hits and damage.
            let target = state
                .creatures
                .iter()
                .filter(|creature| creature.life == CreatureLife::Alive)
                .min_by_key(|creature| {
                    let dx = i64::from(creature.position[0]) - i64::from(player.position[0]);
                    let dz = i64::from(creature.position[2]) - i64::from(player.position[2]);
                    (dx * dx + dz * dz, creature.creature_id)
                })
                .map(|creature| EntityRef::Creature(creature.creature_id));
            zone.apply_command(
                player.player_id,
                sequence,
                ZoneCommand::SelectTarget(target),
            )?;
            zone.apply_command(player.player_id, sequence + 1, ZoneCommand::StartAttack)?;
        }
        if tick == 30 {
            zone.apply_command(player.player_id, sequence, ZoneCommand::Jump)?;
            zone.apply_command(
                player.player_id,
                sequence + 1,
                ZoneCommand::Move {
                    forward: 1,
                    strafe: 0,
                    facing: 0,
                },
            )?;
        } else if tick == 35 {
            zone.apply_command(
                player.player_id,
                sequence,
                ZoneCommand::Move {
                    forward: 0,
                    strafe: 0,
                    facing: 0,
                },
            )?;
        }
    }
    Ok(())
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
    for tick in 0..TICKS {
        commands(&mut zone, fixture, tick)?;
        commands(&mut reference, fixture, tick)?;
        reference.advance_tick()?;
        let expected_ai = zone
            .snapshot()?
            .creatures
            .iter()
            .filter(|creature| creature.life == CreatureLife::Alive)
            .count();
        let maintenance_before = zone.interest_maintenance_stats();
        zone.advance_tick()?;
        let work = zone.tick_work();
        assert_eq!(work.creature_ai_evaluations, expected_ai);
        let physics = work.physics.ok_or("missing completed physics step")?;
        report.ai_evaluations += work.creature_ai_evaluations;
        report.physics_steps += 1;
        report.physics_body_visits += physics.body_count;
        report.dynamic_body_visits += physics.work.dynamic_bodies;
        report.staged_bodies += physics.work.staged_bodies;
        report.pair_checks += physics.pair_checks;
        report.toi_tests += physics.toi_tests;
        report.contact_resolutions += physics.contact_resolutions;
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
            let packed = pack_snapshot(&projection.snapshot)?;
            let decoded = decode_snapshot(&packed.payload)?;
            assert_eq!(decoded.tick, u64::try_from(tick)? + 1);
            assert_eq!(decoded.viewer_id, player.player_id);
            assert_eq!(decoded.events, projection.snapshot.events);
            assert_eq!(
                decoded.entities,
                projection.snapshot.entities[..packed.packed_entities]
            );
            assert!(packed.payload.len() <= MAX_PLAYER_PROJECTION_BYTES);
            assert_eq!(
                packed.payload,
                pack_snapshot(&reference.snapshot_for_player(player.player_id)?)?.payload
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
