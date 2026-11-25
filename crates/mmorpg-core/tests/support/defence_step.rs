//! The `defence-step` workload (#1019, `.performance/contract.json`): in Greyhaven, a defence of
//! Millbrook Farm runs with `raiders` of its reserved raiders standing at the barn's walls, beside
//! the farmfolk, while `idle` players stand in the Outpost far from the fight. Each measured tick
//! counts the raid step's assault raiders and raid sites visited and the construction step's sites visited.
//! Shared by `mmorpg-core`'s `defence_step` test and `defence_step_work` example. Counters only;
//! nothing is timed.
#![allow(dead_code)]

use std::error::Error;

use mmorpg_core::{RaidLeg, RaidMember, ZoneId, ZoneSimulation, greyhaven_vale};

/// Warm ticks before the measurement.
pub const WARM_UP_TICKS: usize = 4;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DefenceStepRun {
    pub raid_raiders_visited: usize,
    pub constructions_visited: usize,
    /// Raid sites the raid step visited (#1019).
    pub raid_sites_visited: usize,
    /// Whether the defence still ran after the last measured tick.
    pub running: bool,
}

/// The zone of the workload, warmed up: players `1..=idle` in the Outpost and a defence with
/// `raiders` raiders at the barn.
pub fn warm_zone(raiders: usize, idle: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let content = greyhaven_vale::content();
    let site = content.raid().ok_or("Greyhaven has a raid site")?.clone();
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content.clone())?;
    for player in 1..=idle {
        zone.add_player(u32::try_from(player)?)?;
    }
    zone.advance_tick()?;
    // A raid's first wave stands up living raiders to copy from.
    if !zone.begin_raid()? {
        return Err("the raid site is eligible".into());
    }
    let mut state = zone.snapshot()?;
    let incident = state.raid.active.as_mut().ok_or("an incident runs")?;
    // A defence of the farmfolk, at full health.
    incident.civilians = site
        .civilians
        .iter()
        .map(|civilian| civilian.health)
        .collect();
    let id = incident.id;
    let template = state
        .creatures
        .iter()
        .find(|record| record.raid.is_some())
        .ok_or("the wave stood a raider up")?
        .clone();
    // The first `raiders` reserved raiders, round the approaches, each at its spot on the wall.
    let mut slots = Vec::new();
    for slot in 0.. {
        let before = slots.len();
        for (approach, way) in site.approaches.iter().enumerate() {
            if let Some(&raider) = way.raiders.get(slot) {
                slots.push((approach, slot, raider));
            }
        }
        if slots.len() >= raiders || slots.len() == before {
            break;
        }
    }
    slots.truncate(raiders);
    if slots.len() != raiders {
        return Err("too few reserved raiders".into());
    }
    for record in &mut state.creatures {
        if record.raid.is_some() {
            // Only the chosen raiders stand.
            if !slots
                .iter()
                .any(|&(_, _, raider)| raider == record.creature_id)
            {
                record.raid = None;
                record.life = mmorpg_core::CreatureLife::Despawned {
                    died_at: state.tick,
                };
                record.health = 0;
            }
        }
    }
    for &(approach, slot, raider) in &slots {
        let (approach, slot) = (u8::try_from(approach)?, u8::try_from(slot)?);
        let last = u8::try_from(site.leg_len(RaidLeg::Assault, approach))?;
        let at = site
            .waypoint(RaidLeg::Assault, approach, slot, last - 1)
            .ok_or("the approach has a route")?;
        let ground = content.definition().ground_at(at[0], at[1]);
        let record = state
            .creatures
            .iter_mut()
            .find(|record| record.creature_id == raider)
            .ok_or("the raider is reserved")?;
        *record = mmorpg_core::CanonicalCreatureSnapshot {
            creature_id: raider,
            position: [at[0] as f32, (ground + 90) as f32, at[1] as f32],
            velocity: [0.0; 3],
            raid: Some(RaidMember {
                incident: id,
                leg: RaidLeg::Assault,
                waypoint: last,
                anchor: at,
                since: state.tick,
            }),
            ..template.clone()
        };
    }
    let mut zone = ZoneSimulation::from_snapshot(state, content)?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(raiders: usize, idle: usize) -> Result<DefenceStepRun, Box<dyn Error>> {
    let mut zone = warm_zone(raiders, idle)?;
    let mut run = DefenceStepRun::default();
    for _ in 0..MEASURED_TICKS {
        zone.advance_tick()?;
        let work = zone.tick_work();
        run.raid_raiders_visited = run.raid_raiders_visited.max(work.raid_raiders_visited);
        run.constructions_visited = run.constructions_visited.max(work.constructions_visited);
        run.raid_sites_visited = run.raid_sites_visited.max(work.raid_sites_visited);
    }
    run.running = zone
        .raid()
        .active
        .as_ref()
        .is_some_and(mmorpg_core::RaidIncident::defence);
    Ok(run)
}

/// One `defence-step` sample in the work-evidence shape.
pub fn sample(raiders: usize, idle: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(raiders, idle)?;
    Ok(format!(
        "{{\"dimensions\":{{\"raiders\":{raiders},\"idle\":{idle}}},\"metrics\":{{\"raid_raiders_visited\":{},\"constructions_visited\":{},\"raid_sites_visited\":{}}}}}",
        run.raid_raiders_visited, run.constructions_visited, run.raid_sites_visited
    ))
}

/// The contract's scale points: (raiders, idle).
pub const SCALE_POINTS: [(usize, usize); 5] = [(3, 16), (6, 16), (12, 16), (6, 64), (6, 160)];
