//! The `escort-step` workload (#1019, `.performance/contract.json`): on wide flat ground,
//! `escorts` players each lead their own wounded NPC, standing 20 m ahead of it so it limps after
//! them for the whole measurement, while `idle` bystander NPCs stand about and one confrontation
//! waits for its evidence. Each measured tick counts the escort step's work: escorts advanced,
//! trail points examined and confrontations checked. Shared by `mmorpg-core`'s `escort_step`
//! test and `escort_step_work` example. Counters only; nothing is timed.
#![allow(dead_code)]

use std::{error::Error, sync::Arc};

use mmorpg_core::{
    Area, AreaId, Confrontation, CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn,
    CreatureTemplate, CreatureTemplateId, ItemId, Npc, NpcId, NpcRole, Quest, QuestId,
    QuestObjective, QuestRewards, SpawnGrid, StaticCollider, ZoneAreas, ZoneCommand, ZoneContent,
    ZoneDefinition, ZoneId, ZoneSimulation,
};

/// Where the escorted NPCs stand, 3 m apart along X.
const ESCORT_ROW_Z: i32 = 3_000;
/// How far ahead of its NPC each player waits: inside the leash, beyond the gap.
const LEAD_UNITS: i32 = 2_000;
const SPACING: i32 = 300;
/// The escorts' destination, far beyond where the measurement lets them limp.
const HAVEN: AreaId = AreaId::new(1);
const FOE_TEMPLATE: CreatureTemplateId = CreatureTemplateId::new(1);
const FOE: CreatureId = CreatureId::new(1);
const COURIER: NpcId = NpcId::new(255);
/// Ticks after the players moved ahead: every follower has a trail and walks.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EscortStepRun {
    pub escorts_advanced: usize,
    pub escort_trail_points: usize,
    pub confrontations_checked: usize,
    /// Escorts still running after the last measured tick.
    pub running: usize,
}

fn escorted(index: usize) -> Result<NpcId, Box<dyn Error>> {
    Ok(NpcId::new(u32::try_from(index)? + 1))
}

fn column(index: usize) -> Result<i32, Box<dyn Error>> {
    Ok(i32::try_from(index % 64)? * SPACING - 32 * SPACING)
}

fn npc(id: NpcId, role: NpcRole, position: [i32; 2]) -> Npc {
    Npc {
        id,
        name: "Work Villager".into(),
        role,
        level: 1,
        position,
        facing: 0,
    }
}

fn content(escorts: usize, idle: usize) -> Result<Arc<ZoneContent>, Box<dyn Error>> {
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
        HAVEN,
        "Work Haven",
        [-15_000, 15_000],
        [15_000, 16_000],
    )?])?;
    let foe = CreatureTemplate {
        id: FOE_TEMPLATE,
        name: "Work Courier".into(),
        family: CreatureFamily::Marauder,
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
        half_extents: [40, 90, 40],
    };
    let spawn = CreatureSpawn {
        id: FOE,
        template: FOE_TEMPLATE,
        position: [0, 8_000],
        facing: 0,
        wander_radius: 0,
        levels: None,
    };
    let mut npcs = Vec::new();
    let mut quests = Vec::new();
    for index in 0..escorts {
        let id = escorted(index)?;
        let row = i32::try_from(index / 64)? * SPACING;
        npcs.push(npc(
            id,
            NpcRole::QuestGiver,
            [column(index)?, ESCORT_ROW_Z + row],
        ));
        quests.push(Quest {
            id: QuestId::new(id.get()),
            name: format!("Work Escort {}", id.get()),
            text: "Lead me home.".into(),
            giver: id,
            ender: id,
            prerequisite: None,
            objectives: vec![QuestObjective::Escort {
                npc: id,
                area: HAVEN,
            }],
            rewards: QuestRewards::default(),
            supplies: Vec::new(),
            requires: Vec::new(),
            decision: Vec::new(),
        });
    }
    // Bystanders in rows of 64, north of the spawn slots.
    for index in 0..idle {
        let row = i32::try_from(index / 64)? * SPACING;
        npcs.push(npc(
            NpcId::new(u32::try_from(escorts + index)? + 1),
            NpcRole::Resident,
            [column(index)?, -3_000 - row],
        ));
    }
    npcs.push(npc(COURIER, NpcRole::Resident, [0, 7_000]));
    let content = ZoneContent::new(definition, areas, vec![foe], vec![spawn], npcs, [0, -1_500])?
        .with_quests(quests)?
        .with_npc_quests(
            Vec::new(),
            vec![Confrontation {
                npc: COURIER,
                evidence: ItemId::new(1),
                foe: FOE,
            }],
        )?;
    Ok(Arc::new(content))
}

/// Restores `zone` with player `index + 1` standing `ahead` units north of its NPC's post.
fn place(
    zone: ZoneSimulation,
    escorts: usize,
    ahead: i32,
) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = Arc::clone(zone.content());
    let mut state = zone.snapshot()?;
    for index in 0..escorts {
        let post = content
            .npc(escorted(index)?)
            .ok_or("escorted npc is missing")?
            .position;
        let player = u32::try_from(index)? + 1;
        let record = state
            .players
            .iter_mut()
            .find(|record| record.player_id == player)
            .ok_or("escorting player is missing")?;
        record.position = mmorpg_core::physics::from_units([post[0] + 100, 90, post[1] + ahead]);
        record.velocity = [0.0; 3];
    }
    Ok(ZoneSimulation::from_snapshot(state, content)?)
}

/// The zone of the workload: every escort running, its player 20 m ahead, warmed up.
pub fn warm_zone(escorts: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content(escorts, idle)?)?;
    for index in 0..escorts {
        zone.add_player(u32::try_from(index)? + 1)?;
    }
    let mut zone = place(zone, escorts, 100)?;
    zone.advance_tick()?;
    for index in 0..escorts {
        let npc = escorted(index)?;
        let player = u32::try_from(index)? + 1;
        zone.apply_command(
            player,
            1,
            ZoneCommand::AcceptQuest {
                npc,
                quest: npc.get(),
            },
        )?;
    }
    zone.advance_tick()?;
    for index in 0..escorts {
        zone.apply_command(
            u32::try_from(index)? + 1,
            2,
            ZoneCommand::QuestAction {
                npc: escorted(index)?,
            },
        )?;
    }
    zone.advance_tick()?;
    let mut zone = place(zone, escorts, LEAD_UNITS)?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(escorts: usize, idle: usize) -> Result<EscortStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(escorts, idle)?;
    let mut run = EscortStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        let work = zone.tick_work();
        run.escorts_advanced = run.escorts_advanced.max(work.escorts_advanced);
        run.escort_trail_points = run.escort_trail_points.max(work.escort_trail_points);
        run.confrontations_checked = run.confrontations_checked.max(work.confrontations_checked);
    }
    run.running = (0..escorts)
        .map(escorted)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|npc| zone.escort(*npc).is_some())
        .count();
    Ok(run)
}

/// One `escort-step` sample in the work-evidence shape.
pub fn sample(escorts: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(escorts, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"escorts\":{escorts},\"idle\":{idle}}},\"metrics\":{{\"escorts_advanced\":{},\"escort_trail_points\":{},\"confrontations_checked\":{}}}}}",
        run.escorts_advanced, run.escort_trail_points, run.confrontations_checked
    ))
}

/// The contract's scale points: (escorts, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(4, 16), (16, 16), (64, 16), (16, 64), (16, 160)];
