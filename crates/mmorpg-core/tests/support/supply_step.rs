//! The `supply-step` workload (#1019, `.performance/contract.json`): on wide flat ground,
//! `patrols` supply patrols of three march north along their own straight roads for the whole
//! measurement, each called out by its own player who then walks far off, while `idle` players
//! stand far south. Each measured tick counts the supply step's work: patrols visited and members
//! visited. Shared by `mmorpg-core`'s `supply_step` test and `supply_step_work` example. Counters
//! only; nothing is timed.
#![allow(dead_code)]

use std::{error::Error, sync::Arc};

use mmorpg_core::{
    Area, AreaId, CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn, CreatureTemplate,
    CreatureTemplateId, Npc, NpcId, NpcRole, Quest, QuestId, QuestObjective, QuestRewards,
    SpawnGrid, StaticCollider, SupplyPatrol, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition,
    ZoneId, ZoneSimulation,
};

/// Each patrol's road runs north along its own X, 20 m apart.
const SPACING: i32 = 2_000;
/// Where every column forms up and where its road ends, far beyond the measurement.
const FORM_Z: i32 = -8_000;
const CAMP_Z: i32 = 16_000;
/// The lookouts stand 60 m west of their roads' start: nothing comes into their sight.
const LOOKOUT_OFFSET: [i32; 2] = [-1_000, -6_000];
const MEMBERS: usize = 3;
const BANDIT: CreatureTemplateId = CreatureTemplateId::new(1);
/// Ticks after the callers walked off: every column marches.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SupplyStepRun {
    pub patrols_visited: usize,
    pub members_visited: usize,
    /// Patrols still marching after the last measured tick.
    pub marching: usize,
}

fn road(index: usize) -> Result<i32, Box<dyn Error>> {
    Ok(i32::try_from(index)? * SPACING - 16 * SPACING)
}

fn lookout(index: usize) -> Result<NpcId, Box<dyn Error>> {
    Ok(NpcId::new(1_000 + u32::try_from(index)?))
}

fn quest(index: usize) -> Result<QuestId, Box<dyn Error>> {
    Ok(QuestId::new(1 + u32::try_from(index)?))
}

fn member(index: usize, place: usize) -> Result<CreatureId, Box<dyn Error>> {
    Ok(CreatureId::new(u32::try_from(index * MEMBERS + place)? + 1))
}

fn content(patrols: usize) -> Result<Arc<ZoneContent>, Box<dyn Error>> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-3_200, -15_000],
            columns: 64,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [40_000, 50, 40_000],
        }],
    )?;
    let areas = ZoneAreas::new(vec![Area::new(
        AreaId::new(1),
        "Work Downs",
        [-39_000, -39_000],
        [39_000, 39_000],
    )?])?;
    let bandit = CreatureTemplate {
        id: BANDIT,
        name: "Work Bandit".into(),
        family: CreatureFamily::Redbrand,
        behaviour: CreatureBehaviour::Aggressive,
        min_level: 1,
        max_level: 1,
        elite: false,
        health: 50,
        health_per_level: 0,
        damage: [1, 1],
        damage_per_level: 0,
        swing_ticks: 60,
        respawn_ticks: 1_800,
        half_extents: [30, 90, 30],
    };
    let mut spawns = Vec::new();
    let mut npcs = Vec::new();
    let mut quests = Vec::new();
    let mut supply = Vec::new();
    for index in 0..patrols {
        let x = road(index)?;
        let mut members = Vec::new();
        for place in 0..MEMBERS {
            let id = member(index, place)?;
            spawns.push(CreatureSpawn {
                id,
                template: BANDIT,
                position: [x, FORM_Z - 250 * i32::try_from(place)?],
                facing: 32_768,
                wander_radius: 0,
                levels: None,
            });
            members.push(id);
        }
        npcs.push(Npc {
            id: lookout(index)?,
            name: "Work Lookout".into(),
            role: NpcRole::QuestGiver,
            level: 1,
            position: [x + LOOKOUT_OFFSET[0], FORM_Z + LOOKOUT_OFFSET[1]],
            facing: 0,
        });
        quests.push(Quest {
            id: quest(index)?,
            name: "Work Ambush".into(),
            text: "Stop them.".into(),
            giver: lookout(index)?,
            ender: lookout(index)?,
            prerequisite: None,
            objectives: vec![QuestObjective::Ambush {
                npc: lookout(index)?,
            }],
            rewards: QuestRewards::default(),
            supplies: Vec::new(),
            requires: Vec::new(),
            decision: Vec::new(),
        });
        supply.push(SupplyPatrol {
            npc: lookout(index)?,
            members,
            route: vec![[x, FORM_Z], [x, 0], [x, CAMP_Z]],
            sight: 500,
            post: 500,
        });
    }
    let content = ZoneContent::new(definition, areas, vec![bandit], spawns, npcs, [0, -30_000])?
        .with_quests(quests)?
        .with_supply_patrols(supply)?;
    Ok(Arc::new(content))
}

/// Restores `zone` with caller `index + 1` at `at(index)` and idle player `patrols + 1 + i` far
/// south.
fn place(
    zone: ZoneSimulation,
    patrols: usize,
    at: impl Fn(usize) -> Result<[i32; 2], Box<dyn Error>>,
) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = Arc::clone(zone.content());
    let mut state = zone.snapshot()?;
    for record in &mut state.players {
        let player = record.player_id;
        let spot = if let Ok(index) = usize::try_from(player - 1)
            && index < patrols
        {
            at(index)?
        } else {
            let index = i32::try_from(player)? - i32::try_from(patrols)? - 1;
            [(index % 64) * 400 - 32 * 400, -32_000 - (index / 64) * 400]
        };
        record.position = mmorpg_core::physics::from_units([spot[0], 90, spot[1]]);
        record.velocity = [0.0; 3];
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The zone of the workload: every patrol called out and marching, its caller far off, warmed up.
pub fn warm_zone(patrols: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content(patrols)?)?;
    for player in 1..=patrols + idle {
        zone.add_player(u32::try_from(player)?)?;
    }
    let beside = |index: usize| -> Result<[i32; 2], Box<dyn Error>> {
        let x = road(index)?;
        Ok([
            x + LOOKOUT_OFFSET[0] + 100,
            FORM_Z + LOOKOUT_OFFSET[1] + 100,
        ])
    };
    let mut zone = place(zone, patrols, beside)?;
    zone.advance_tick()?;
    for index in 0..patrols {
        zone.apply_command(
            u32::try_from(index + 1)?,
            1,
            ZoneCommand::AcceptQuest {
                npc: lookout(index)?,
                quest: quest(index)?.get(),
            },
        )?;
    }
    zone.advance_tick()?;
    for index in 0..patrols {
        zone.apply_command(
            u32::try_from(index + 1)?,
            2,
            ZoneCommand::QuestAction {
                npc: lookout(index)?,
            },
        )?;
    }
    zone.advance_tick()?;
    // The callers walk far off, west of every road.
    let mut zone = place(zone, patrols, |index| {
        Ok([-36_000, -30_000 + i32::try_from(index)? * 400])
    })?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(patrols: usize, idle: usize) -> Result<SupplyStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(patrols, idle)?;
    let mut run = SupplyStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        let work = zone.tick_work();
        run.patrols_visited = run.patrols_visited.max(work.supply_patrols_visited);
        run.members_visited = run.members_visited.max(work.supply_members_visited);
    }
    run.marching = (0..patrols)
        .map(lookout)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|npc| {
            zone.supply_run(*npc)
                .is_some_and(|run| run.rests_until == 0)
        })
        .count();
    Ok(run)
}

/// One `supply-step` sample in the work-evidence shape.
pub fn sample(patrols: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(patrols, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"patrols\":{patrols},\"idle\":{idle}}},\"metrics\":{{\"supply_patrols_visited\":{},\"supply_members_visited\":{}}}}}",
        run.patrols_visited, run.members_visited
    ))
}

/// The contract's scale points: (patrols, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(1, 16), (4, 16), (16, 16), (4, 64), (4, 160)];
