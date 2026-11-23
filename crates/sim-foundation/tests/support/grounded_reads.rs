//! The `grounded-reads` workload (#1148, `.performance/contract.json`): `units` bodies standing
//! still on a flat floor beside one platform. After they settle, each of `ticks` steady ticks
//! steps the world and reads every unit's grounded once; then one tick raises the platform (a
//! rising platform moves after the units, so the static geometry changes after their moves) and
//! reads them again; then one more steady tick. Shared by `sim-foundation`'s `grounded_work` test
//! and example. Counters only; nothing is timed.
#![allow(dead_code)]

use sim_foundation::physics::{
    BodyId, GroundedWork, PhysicsError, RigidBody, SUBUNITS_PER_UNIT, Vec3i, World,
};

const FLOOR: BodyId = BodyId(1);
const PLATFORM: BodyId = BodyId(2);
const BODY: Vec3i = Vec3i::new(30, 90, 30);
/// Ticks before measuring: every unit has landed and made a move.
pub const SETTLE_TICKS: usize = 30;

/// The measured reads.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GroundedRun {
    /// Support casts in the steady tick that cast most, the one after the change included.
    pub steady_tick_casts: u64,
    /// Support casts in the tick whose platform rose.
    pub geometry_change_casts: u64,
    /// Reads answered from the cache over every measured tick.
    pub cached_reads: u64,
    /// Units still grounded after the last measured tick.
    pub grounded: usize,
}

fn unit(index: usize) -> BodyId {
    BodyId(10 + index as u64)
}

/// `units` units resting on the floor in a grid 200 units apart, the platform beside them.
pub fn settled_world(units: usize) -> Result<World, PhysicsError> {
    let mut world = World::new(Vec3i::new(0, -SUBUNITS_PER_UNIT, 0));
    world.add_body(RigidBody::floor(FLOOR, 0))?;
    world.add_body(RigidBody::platform(
        PLATFORM,
        Vec3i::new(-1_000, 10, -1_000),
        Vec3i::new(100, 10, 100),
    ))?;
    for index in 0..units {
        let column = i32::try_from(index % 16).expect("small grid");
        let row = i32::try_from(index / 16).expect("small grid");
        world.add_body(RigidBody::dynamic(
            unit(index),
            Vec3i::new(200 * column, 100, 200 * row),
            Vec3i::ZERO,
            BODY,
        ))?;
    }
    for _ in 0..SETTLE_TICKS {
        world.step()?;
    }
    Ok(world)
}

/// Steps once and reads every unit's grounded once: the tick's support casts.
fn read_tick(world: &mut World, units: usize) -> Result<GroundedWork, PhysicsError> {
    world.step()?;
    world.take_query_failure()?;
    let before = world.grounded_work();
    for index in 0..units {
        let _ = world.grounded(unit(index));
    }
    world.take_query_failure()?;
    let after = world.grounded_work();
    Ok(GroundedWork {
        cached: after.cached - before.cached,
        casts: after.casts - before.casts,
    })
}

pub fn measure(units: usize, ticks: usize) -> Result<GroundedRun, PhysicsError> {
    let mut world = settled_world(units)?;
    let mut run = GroundedRun::default();
    let record_steady = |run: &mut GroundedRun, work: GroundedWork| {
        run.steady_tick_casts = run.steady_tick_casts.max(work.casts);
        run.cached_reads += work.cached;
    };
    for _ in 0..ticks {
        let work = read_tick(&mut world, units)?;
        record_steady(&mut run, work);
    }
    let platform = world.body(PLATFORM).expect("the platform").position();
    world.move_platform(
        PLATFORM,
        Vec3i::new(platform.x, platform.y + 20, platform.z),
    )?;
    let change = read_tick(&mut world, units)?;
    run.geometry_change_casts = change.casts;
    run.cached_reads += change.cached;
    let work = read_tick(&mut world, units)?;
    record_steady(&mut run, work);
    run.grounded = (0..units)
        .filter(|&index| world.grounded(unit(index)))
        .count();
    Ok(run)
}

/// One `coding-tooling/work-evidence/v1` sample.
pub fn sample(units: usize, ticks: usize) -> Result<String, PhysicsError> {
    let run = measure(units, ticks)?;
    Ok(format!(
        "{{\"dimensions\":{{\"units\":{units},\"ticks\":{ticks}}},\"metrics\":{{\"steady_tick_casts\":{},\"geometry_change_casts\":{}}}}}",
        run.steady_tick_casts, run.geometry_change_casts
    ))
}

/// The contract's scale points: (units, ticks).
pub const SCALE_POINTS: [(usize, usize); 5] = [(8, 16), (32, 16), (128, 16), (128, 4), (128, 64)];
