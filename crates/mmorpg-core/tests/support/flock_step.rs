//! The `flock-step` workload (#1019, `.performance/contract.json`): on wide flat ground,
//! `strays` players each drive their own stray from 3 m behind it, so it trots away for the whole
//! measurement, while a watch over a three-sheep flock runs with its two wolves held at their gaps
//! and `idle` players stand far off. Each measured tick counts the flock step's work: strays
//! visited, ways tested and prowlers visited. Shared by `mmorpg-core`'s `flock_step` test and
//! `flock_step_work` example. Counters only; nothing is timed.
#![allow(dead_code)]

use std::{error::Error, sync::Arc};

use mmorpg_core::{
    Area, AreaId, CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn, CreatureTemplate,
    CreatureTemplateId, HedgeWatch, Npc, NpcId, NpcRole, Prowler, Quest, QuestId, QuestObjective,
    QuestRewards, SpawnGrid, StaticCollider, Stray, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneId, ZoneSimulation,
};

/// Where the strays stand, 4 m apart along X.
const STRAY_ROW_Z: i32 = 3_000;
const SPACING: i32 = 400;
/// How far behind its stray each driver stands: inside the shy radius.
const BEHIND_UNITS: i32 = 300;
/// The strays' pens, far beyond where the measurement lets them trot.
const PEN: ([i32; 2], [i32; 2]) = ([-15_000, 14_000], [15_000, 15_000]);
const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
const PROWLERS: [CreatureId; 2] = [CreatureId::new(1), CreatureId::new(2)];
/// The shepherd, her flock and the watch's ground, west of the strays.
const SHEPHERD: NpcId = NpcId::new(1_000);
const FLOCK: [NpcId; 3] = [NpcId::new(1_001), NpcId::new(1_002), NpcId::new(1_003)];
const FOLD: [i32; 2] = [-10_000, -6_000];
const WATCHER: u32 = 1_000;
const WATCH_QUEST: QuestId = QuestId::new(1_000);
/// Ticks after the drivers took their places: every stray trots.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FlockStepRun {
    pub strays_visited: usize,
    pub stray_ways_tested: usize,
    pub prowlers_visited: usize,
    /// Strays being driven after the last measured tick.
    pub driven: usize,
}

fn stray(index: usize) -> Result<NpcId, Box<dyn Error>> {
    Ok(NpcId::new(u32::try_from(index)? + 1))
}

fn column(index: usize) -> Result<i32, Box<dyn Error>> {
    Ok(i32::try_from(index % 64)? * SPACING - 32 * SPACING)
}

fn npc(id: NpcId, role: NpcRole, position: [i32; 2]) -> Npc {
    Npc {
        id,
        name: "Work Ewe".into(),
        role,
        level: 1,
        position,
        facing: 0,
    }
}

fn content(strays: usize) -> Result<Arc<ZoneContent>, Box<dyn Error>> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-3_200, -12_000],
            columns: 64,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [20_000, 50, 20_000],
        }],
    )?;
    let areas = ZoneAreas::new(vec![Area::new(
        AreaId::new(1),
        "Work Pasture",
        [-19_000, -19_000],
        [19_000, 19_000],
    )?])?;
    let wolf = CreatureTemplate {
        id: WOLF,
        name: "Work Wolf".into(),
        family: CreatureFamily::Wolf,
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
        half_extents: [30, 40, 30],
    };
    let spawns = PROWLERS
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            Ok(CreatureSpawn {
                id,
                template: WOLF,
                position: [FOLD[0] - 1_200, FOLD[1] + i32::try_from(index)? * 600],
                facing: 16_384,
                wander_radius: 0,
                levels: None,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn Error>>>()?;
    let mut npcs = Vec::new();
    for index in 0..strays {
        let row = i32::try_from(index / 64)? * SPACING;
        npcs.push(npc(
            stray(index)?,
            NpcRole::Resident,
            [column(index)?, STRAY_ROW_Z + row],
        ));
    }
    npcs.push(npc(SHEPHERD, NpcRole::QuestGiver, [FOLD[0], FOLD[1] - 600]));
    for (index, &sheep) in FLOCK.iter().enumerate() {
        npcs.push(npc(
            sheep,
            NpcRole::Resident,
            [FOLD[0] + i32::try_from(index)? * 150, FOLD[1]],
        ));
    }
    let quest = Quest {
        id: WATCH_QUEST,
        name: "Work Watch".into(),
        text: "Keep my ewes.".into(),
        giver: SHEPHERD,
        ender: SHEPHERD,
        prerequisite: None,
        objectives: vec![QuestObjective::Watch { npc: SHEPHERD }],
        rewards: QuestRewards::default(),
        supplies: Vec::new(),
        requires: Vec::new(),
        decision: Vec::new(),
    };
    let watch = HedgeWatch {
        npc: SHEPHERD,
        flock: FLOCK.to_vec(),
        health: 40,
        hold: 2,
        // Held at their gaps, far from the flock, for the whole measurement.
        prowlers: PROWLERS
            .iter()
            .enumerate()
            .map(|(index, &creature)| {
                Ok(Prowler {
                    creature,
                    after_ticks: 0,
                    gap: [FOLD[0] - 1_200, FOLD[1] + i32::try_from(index)? * 600],
                })
            })
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
        centre: FOLD,
        radius: 2_000,
    };
    let content = ZoneContent::new(definition, areas, vec![wolf], spawns, npcs, [0, -1_500])?
        .with_quests(vec![quest])?
        .with_flock(
            (0..strays)
                .map(|index| {
                    Ok(Stray {
                        npc: stray(index)?,
                        pen: PEN,
                    })
                })
                .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
            Some(watch),
        )?;
    Ok(Arc::new(content))
}

/// Restores `zone` with driver `index + 1` standing behind its stray, the watcher at the
/// shepherd and idle player `strays + 1 + i` far south.
fn place(zone: ZoneSimulation, strays: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = Arc::clone(zone.content());
    let mut state = zone.snapshot()?;
    for record in &mut state.players {
        let player = record.player_id;
        let at = if player == WATCHER {
            [FOLD[0] + 100, FOLD[1] - 600]
        } else if let Ok(index) = usize::try_from(player - 1)
            && index < strays
        {
            let [x, z] = content
                .npc(stray(index)?)
                .ok_or("stray npc is missing")?
                .position;
            [x, z - BEHIND_UNITS]
        } else {
            let index = i32::try_from(player)? - i32::try_from(strays)? - 1;
            [
                (index % 64) * SPACING - 32 * SPACING,
                -15_000 - (index / 64) * SPACING,
            ]
        };
        record.position = mmorpg_core::physics::from_units([at[0], 90, at[1]]);
        record.velocity = [0.0; 3];
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The zone of the workload: every stray driven, the watch running, warmed up.
pub fn warm_zone(strays: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content(strays)?)?;
    for player in 1..=strays + idle {
        zone.add_player(u32::try_from(player)?)?;
    }
    zone.add_player(WATCHER)?;
    let mut zone = place(zone, strays)?;
    zone.advance_tick()?;
    zone.apply_command(
        WATCHER,
        1,
        ZoneCommand::AcceptQuest {
            npc: SHEPHERD,
            quest: WATCH_QUEST.get(),
        },
    )?;
    zone.advance_tick()?;
    zone.apply_command(WATCHER, 2, ZoneCommand::QuestAction { npc: SHEPHERD })?;
    zone.advance_tick()?;
    let mut zone = place(zone, strays)?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(strays: usize, idle: usize) -> Result<FlockStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(strays, idle)?;
    let mut run = FlockStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        let work = zone.tick_work();
        run.strays_visited = run.strays_visited.max(work.strays_visited);
        run.stray_ways_tested = run.stray_ways_tested.max(work.stray_ways_tested);
        run.prowlers_visited = run.prowlers_visited.max(work.prowlers_visited);
    }
    run.driven = (0..strays)
        .map(stray)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|npc| {
            zone.stray(*npc)
                .is_some_and(|state| state.velocity != [0, 0])
        })
        .count();
    if zone.watch().is_none_or(|watch| watch.recovers_at != 0) {
        return Err("the watch is not running".into());
    }
    Ok(run)
}

/// One `flock-step` sample in the work-evidence shape.
pub fn sample(strays: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(strays, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"strays\":{strays},\"idle\":{idle}}},\"metrics\":{{\"strays_visited\":{},\"stray_ways_tested\":{},\"prowlers_visited\":{}}}}}",
        run.strays_visited, run.stray_ways_tested, run.prowlers_visited
    ))
}

/// The contract's scale points: (strays, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(4, 16), (16, 16), (64, 16), (16, 64), (16, 160)];
