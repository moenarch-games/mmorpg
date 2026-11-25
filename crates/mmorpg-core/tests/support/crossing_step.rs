//! The `crossing-step` workload (#1019, `.performance/contract.json`): on wide flat ground,
//! `crossings` carters drive their carts north along their own straight roads for the whole
//! measurement, each started by its own player who then walks far off, while three raiders of each
//! run for its cart and `idle` players stand far south. Each measured tick counts the crossing
//! step's work: crossings visited and raiders visited. Shared by `mmorpg-core`'s `crossing_step`
//! test and `crossing_step_work` example. Counters only; nothing is timed.
#![allow(dead_code)]

use std::{error::Error, sync::Arc};

use mmorpg_core::{
    Area, AreaId, CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn, CreatureTemplate,
    CreatureTemplateId, CrossingRaider, Npc, NpcId, NpcRole, Quest, QuestId, QuestObjective,
    QuestRewards, SpawnGrid, StaticCollider, WagonCrossing, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneId, ZoneSimulation,
};

/// Each cart's road runs north along its own X, 20 m apart.
const SPACING: i32 = 2_000;
/// Where every cart sets out and where its road ends, far beyond the measurement.
const START_Z: i32 = -8_000;
const BANK_Z: i32 = 16_000;
const RAIDERS: usize = 3;
const BANDIT: CreatureTemplateId = CreatureTemplateId::new(1);
/// Ticks after the starters walked off: every raider stands and runs for its cart.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CrossingStepRun {
    pub crossings_visited: usize,
    pub raiders_visited: usize,
    /// Crossings still under way after the last measured tick.
    pub rolling: usize,
}

fn road(index: usize) -> Result<i32, Box<dyn Error>> {
    Ok(i32::try_from(index)? * SPACING - 4 * SPACING)
}

fn carter(index: usize) -> Result<NpcId, Box<dyn Error>> {
    Ok(NpcId::new(1_000 + u32::try_from(index)?))
}

fn quest(index: usize) -> Result<QuestId, Box<dyn Error>> {
    Ok(QuestId::new(1 + u32::try_from(index)?))
}

fn raider(index: usize, place: usize) -> Result<CreatureId, Box<dyn Error>> {
    Ok(CreatureId::new(u32::try_from(index * RAIDERS + place)? + 1))
}

fn content(crossings: usize) -> Result<Arc<ZoneContent>, Box<dyn Error>> {
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
        "Work Ford",
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
    let mut carts = Vec::new();
    for index in 0..crossings {
        let x = road(index)?;
        let mut raiders = Vec::new();
        for place in 0..RAIDERS {
            let id = raider(index, place)?;
            // Beside the road, a few metres ahead of the cart.
            spawns.push(CreatureSpawn {
                id,
                template: BANDIT,
                position: [x + 600, START_Z + 400 * i32::try_from(place)?],
                facing: 49_152,
                wander_radius: 0,
                levels: None,
            });
            raiders.push(CrossingRaider {
                creature: id,
                after_ticks: 0,
            });
        }
        npcs.push(Npc {
            id: carter(index)?,
            name: "Work Carter".into(),
            role: NpcRole::QuestGiver,
            level: 1,
            position: [x, START_Z],
            facing: 32_768,
        });
        quests.push(Quest {
            id: quest(index)?,
            name: "Work Crossing".into(),
            text: "See it over.".into(),
            giver: carter(index)?,
            ender: carter(index)?,
            prerequisite: None,
            objectives: vec![QuestObjective::Crossing {
                npc: carter(index)?,
            }],
            rewards: QuestRewards::default(),
            supplies: Vec::new(),
            requires: Vec::new(),
            decision: Vec::new(),
        });
        carts.push(WagonCrossing {
            npc: carter(index)?,
            route: vec![[x, START_Z], [x, 0], [x, BANK_Z]],
            health: 100_000,
            raiders,
            radius: 500,
        });
    }
    let content = ZoneContent::new(definition, areas, vec![bandit], spawns, npcs, [0, -30_000])?
        .with_quests(quests)?
        .with_crossings(carts)?;
    Ok(Arc::new(content))
}

/// Restores `zone` with starter `index + 1` at `at(index)` and idle player `crossings + 1 + i` far
/// south.
fn place(
    zone: ZoneSimulation,
    crossings: usize,
    at: impl Fn(usize) -> Result<[i32; 2], Box<dyn Error>>,
) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = Arc::clone(zone.content());
    let mut state = zone.snapshot()?;
    for record in &mut state.players {
        let player = record.player_id;
        let spot = if let Ok(index) = usize::try_from(player - 1)
            && index < crossings
        {
            at(index)?
        } else {
            let index = i32::try_from(player)? - i32::try_from(crossings)? - 1;
            [(index % 64) * 400 - 32 * 400, -32_000 - (index / 64) * 400]
        };
        record.position = mmorpg_core::physics::from_units([spot[0], 90, spot[1]]);
        record.velocity = [0.0; 3];
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The zone of the workload: every cart started and rolling with its raiders standing, its
/// starter far off, warmed up.
pub fn warm_zone(crossings: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content(crossings)?)?;
    for player in 1..=crossings + idle {
        zone.add_player(u32::try_from(player)?)?;
    }
    let beside = |index: usize| -> Result<[i32; 2], Box<dyn Error>> {
        Ok([road(index)? - 100, START_Z - 100])
    };
    let mut zone = place(zone, crossings, beside)?;
    zone.advance_tick()?;
    for index in 0..crossings {
        zone.apply_command(
            u32::try_from(index + 1)?,
            1,
            ZoneCommand::AcceptQuest {
                npc: carter(index)?,
                quest: quest(index)?.get(),
            },
        )?;
    }
    zone.advance_tick()?;
    for index in 0..crossings {
        zone.apply_command(
            u32::try_from(index + 1)?,
            2,
            ZoneCommand::QuestAction {
                npc: carter(index)?,
            },
        )?;
    }
    zone.advance_tick()?;
    // The starters walk far off, west of every road.
    let mut zone = place(zone, crossings, |index| {
        Ok([-36_000, -30_000 + i32::try_from(index)? * 400])
    })?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(crossings: usize, idle: usize) -> Result<CrossingStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(crossings, idle)?;
    let mut run = CrossingStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        let work = zone.tick_work();
        run.crossings_visited = run.crossings_visited.max(work.crossings_visited);
        run.raiders_visited = run.raiders_visited.max(work.crossing_raiders_visited);
    }
    run.rolling = (0..crossings)
        .map(carter)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|npc| {
            zone.crossing_run(*npc)
                .is_some_and(|run| run.rests_until == 0)
        })
        .count();
    Ok(run)
}

/// One `crossing-step` sample in the work-evidence shape.
pub fn sample(crossings: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(crossings, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"crossings\":{crossings},\"idle\":{idle}}},\"metrics\":{{\"crossings_visited\":{},\"crossing_raiders_visited\":{}}}}}",
        run.crossings_visited, run.raiders_visited
    ))
}

/// The contract's scale points: (crossings, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(1, 16), (2, 16), (4, 16), (4, 64), (4, 160)];
