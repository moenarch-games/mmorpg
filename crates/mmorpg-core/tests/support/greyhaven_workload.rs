//! The Greyhaven combat workload script (#26): fixed players, commands and horizon over the
//! real zone simulation. Shared by `mmorpg-protocol`'s combat workload gate and example and by
//! `mmorpg-core`'s physics backend A/B test and example (physics-engine #330); it uses only
//! `mmorpg-core`.
use std::{error::Error, sync::Arc};

use mmorpg_core::{
    CanonicalPlayerCombat, CanonicalPlayerSnapshot, CreatureLife, EntityRef,
    PLAYER_HALF_EXTENTS_UNITS, ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale,
};

pub const TICKS: usize = 360;

#[derive(Clone, Copy, Debug)]
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

pub fn initial_zone(fixture: Fixture) -> Result<ZoneSimulation, Box<dyn Error>> {
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
            let [x, z] = [spawn.position[0] + 150 + row * 65, spawn.position[1]];
            // Feet on the basin's terraces, such as Millbrook Farm's (#423); elsewhere on the
            // valley floor as before, so the outer hills' fights keep their geometry.
            let ground = greyhaven_vale::basin::height_at(x, z).unwrap_or(0);
            [x, ground + PLAYER_HALF_EXTENTS_UNITS[1], z]
        };
        state.players.push(CanonicalPlayerSnapshot {
            copper: 0,
            player_id: u32::try_from(index)? + 1,
            position: mmorpg_core::physics::from_units(position),
            velocity: [0.0; 3],
            facing: 0,
            forward: 0,
            strafe: 0,
            jump_pending: false,
            rise: 0,
            breath: mmorpg_core::Breath::FULL,
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
            waystones: mmorpg_core::WaystoneSet::EMPTY,
        });
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

pub fn commands(
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
                .zip(zone.content().creature_spawns())
                // The bots fight creatures, not breakable props (#288).
                .filter(|(creature, spawn)| {
                    creature.life == CreatureLife::Alive
                        && zone
                            .content()
                            .creature_template(spawn.template)
                            .is_some_and(|template| template.prop().is_none())
                })
                .map(|(creature, _)| creature)
                .min_by_key(|creature| {
                    let (creature_at, player_at) = (
                        mmorpg_core::physics::to_units(creature.position),
                        mmorpg_core::physics::to_units(player.position),
                    );
                    let dx = i64::from(creature_at[0]) - i64::from(player_at[0]);
                    let dz = i64::from(creature_at[2]) - i64::from(player_at[2]);
                    (dx * dx + dz * dz, creature.creature_id)
                })
                .map(|creature| EntityRef::Creature(creature.creature_id));
            zone.apply_command(
                player.player_id,
                sequence,
                ZoneCommand::SelectTarget(target),
            )?;
        }
        // Each living bot swings at its selected creature whenever it stands in reach (#517).
        let in_reach = player.combat.target.and_then(|target| {
            let EntityRef::Creature(id) = target else {
                return None;
            };
            let creature = state
                .creatures
                .iter()
                .find(|creature| creature.creature_id == id)?;
            let (creature_at, player_at) = (
                mmorpg_core::physics::to_units(creature.position),
                mmorpg_core::physics::to_units(player.position),
            );
            let (dx, dz) = (creature_at[0] - player_at[0], creature_at[2] - player_at[2]);
            let reach = i64::from(mmorpg_core::unit::PLAYER_REACH_UNITS);
            (i64::from(dx).pow(2) + i64::from(dz).pow(2) <= reach.pow(2))
                .then(|| mmorpg_core::trig::yaw_from_vector(dx, dz).unwrap_or(0))
        });
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
        // A swing turns its bot, so none interrupts the scripted run north (ticks 30 to 35).
        if let Some(facing) =
            in_reach.filter(|_| player.combat.health > 0 && !(30..35).contains(&tick))
        {
            zone.apply_command(
                player.player_id,
                sequence + 3,
                ZoneCommand::Swing { facing },
            )?;
        }
    }
    Ok(())
}
