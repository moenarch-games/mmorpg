//! The `creature-tick` workload (#1086, `.performance/contract.json`): one player on wide flat
//! ground, `active` aggressive wolves fighting it and `idle` wolves asleep far beyond its wake
//! radius. After a warm-up, each measured tick counts the creature systems' visits, the interest
//! query results that outgrew their buffers and, through a counting allocator, every heap
//! allocation of `advance_tick`. Shared by `mmorpg-core`'s `creature_tick_work` test and example.
//! Counters only; nothing is timed.
// The counting allocator is the workspace's one opt-in to unsafe code (Atlas 7.1.i1055).
#![allow(unsafe_code, dead_code)]

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    error::Error,
    sync::Arc,
};

use mmorpg_core::{
    CreatureBehaviour, CreatureFamily, CreatureId, CreatureSpawn, CreatureTemplate,
    CreatureTemplateId, SpawnGrid, StaticCollider, ZoneAreas, ZoneContent, ZoneDefinition, ZoneId,
    ZoneSimulation,
};

/// Counts this thread's allocations, so parallel tests do not count each other's.
struct Counting;

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

// SAFETY: every call forwards to `System`; counting touches only a const-initialised cell.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        // SAFETY: as above.
        unsafe { System.realloc(pointer, layout, size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: as above.
        unsafe { System.dealloc(pointer, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations() -> usize {
    ALLOCATIONS.with(Cell::get)
}

const WOLF: CreatureTemplateId = CreatureTemplateId::new(1);
/// Ticks before measuring: every wolf has rested or engaged, and buffers are warm.
pub const WARM_UP_TICKS: usize = 40;
/// Measured ticks; each metric is the largest of them.
pub const MEASURED_TICKS: usize = 16;

/// The largest per-tick work of the measured ticks.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CreatureTickRun {
    pub creatures_visited: usize,
    pub interest_query_allocations: usize,
    pub warm_tick_allocations: usize,
    /// Creatures the decision step visits after the last measured tick.
    pub awake: usize,
}

fn content(idle: usize, active: usize) -> Result<Arc<ZoneContent>, Box<dyn Error>> {
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [0, 0],
            columns: 8,
            spacing: 100,
        },
        vec![StaticCollider {
            id: 1,
            position: [0, -50, 0],
            half_extents: [40_000, 50, 40_000],
        }],
    )?;
    let wolf = CreatureTemplate {
        id: WOLF,
        name: "Work Wolf".into(),
        family: CreatureFamily::Wolf,
        behaviour: CreatureBehaviour::Aggressive,
        min_level: 1,
        max_level: 1,
        elite: false,
        health: 5_000,
        health_per_level: 0,
        damage: [1, 1],
        damage_per_level: 0,
        swing_ticks: 60,
        respawn_ticks: 1_800,
        half_extents: [40, 45, 40],
    };
    let spawn = |id: usize, position: [i32; 2]| -> Result<CreatureSpawn, Box<dyn Error>> {
        Ok(CreatureSpawn {
            id: CreatureId::new(u32::try_from(id)? + 1),
            template: WOLF,
            position,
            facing: 0,
            wander_radius: 0,
            levels: None,
        })
    };
    let mut spawns = Vec::new();
    // Fighters within aggro range of the player at the origin.
    for index in 0..active {
        let offset = i32::try_from(index)? * 150;
        spawns.push(spawn(spawns.len(), [offset - 600, -600])?);
    }
    // Sleepers far east, beyond every cell the player's wake radius reaches.
    for index in 0..idle {
        let value = i32::try_from(index)?;
        let at = [15_000 + (value % 16) * 1_500, (value / 16) * 1_500 - 30_000];
        spawns.push(spawn(spawns.len(), at)?);
    }
    Ok(Arc::new(
        ZoneContent::new(
            definition,
            ZoneAreas::default(),
            vec![wolf],
            spawns,
            Vec::new(),
            [0, -3_000],
        )?
        .with_creature_abilities(Vec::new())?,
    ))
}

/// The zone of the workload, warmed up.
pub fn warm_zone(idle: usize, active: usize) -> Result<ZoneSimulation, Box<dyn Error>> {
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content(idle, active)?)?;
    zone.add_player(1)?;
    for _ in 0..WARM_UP_TICKS {
        zone.advance_tick()?;
    }
    Ok(zone)
}

/// Measures [`MEASURED_TICKS`] warm ticks of the workload.
pub fn measure(idle: usize, active: usize) -> Result<CreatureTickRun, Box<dyn Error>> {
    let mut zone = warm_zone(idle, active)?;
    let mut run = CreatureTickRun::default();
    for _ in 0..MEASURED_TICKS {
        let before = allocations();
        zone.advance_tick()?;
        let allocated = allocations() - before;
        let work = zone.tick_work();
        run.creatures_visited = run.creatures_visited.max(work.creatures_visited);
        run.interest_query_allocations = run
            .interest_query_allocations
            .max(work.interest_query_allocations);
        run.warm_tick_allocations = run.warm_tick_allocations.max(allocated);
    }
    run.awake = zone.awake_creatures();
    Ok(run)
}

/// One `creature-tick` sample in the work-evidence shape.
pub fn sample(idle: usize, active: usize) -> Result<String, Box<dyn Error>> {
    let run = measure(idle, active)?;
    Ok(format!(
        "{{\"dimensions\":{{\"idle\":{idle},\"active\":{active}}},\"metrics\":{{\"creatures_visited\":{},\"interest_query_allocations\":{},\"warm_tick_allocations\":{}}}}}",
        run.creatures_visited, run.interest_query_allocations, run.warm_tick_allocations
    ))
}

/// The contract's scale points: (idle, active).
pub const SCALE_POINTS: [(usize, usize); 5] = [(64, 4), (128, 4), (256, 4), (256, 8), (256, 16)];
