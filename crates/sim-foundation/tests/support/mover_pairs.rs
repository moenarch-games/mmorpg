//! The `mover-pairs` workload (#1149, `.performance/contract.json`): `walkers` awake units walking
//! side by side across a flat floor, 100 units apart in rows of 16, and `sleepers` asleep units
//! resting in their own block far away (creatures asleep away from the players). Each measured tick
//! gives every walker the same walking velocity and steps once; the step's `mover_pairs` counts the
//! bodies the slide-around intent examined. Shared by `sim-foundation`'s `mover_pairs` test and
//! example. Counters only; nothing is timed.
#![allow(dead_code)]

use sim_foundation::physics::{BodyId, PhysicsError, RigidBody, SUBUNITS_PER_UNIT, Vec3i, World};

const FLOOR: BodyId = BodyId(1);
const BODY: Vec3i = Vec3i::new(30, 90, 30);
const SPACING: i32 = 100;
const ROW: usize = 16;
/// The sleepers' block lies this far (units) from the walkers along -Z.
const SLEEPERS_AWAY: i32 = 20_000;
/// Ticks before measuring: every unit has landed.
pub const SETTLE_TICKS: usize = 10;
pub const MEASURED_TICKS: usize = 8;
/// Every walker's velocity: 5 units per tick along +X.
const WALK: Vec3i = Vec3i::new(5 * SUBUNITS_PER_UNIT, 0, 0);

/// The measured pair work.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PairRun {
    /// Bodies the slide-around intent examined in the measured tick that examined most.
    pub tick_pairs: usize,
    /// Walkers the last measured tick moved.
    pub moved: usize,
}

fn walker(index: usize) -> BodyId {
    BodyId(10 + index as u64)
}

fn sleeper(walkers: usize, index: usize) -> BodyId {
    BodyId(10 + (walkers + index) as u64)
}

fn grid(index: usize) -> (i32, i32) {
    let column = i32::try_from(index % ROW).expect("small grid");
    let row = i32::try_from(index / ROW).expect("small grid");
    (SPACING * column, SPACING * row)
}

/// The walkers and sleepers standing on the floor, settled.
pub fn settled_world(walkers: usize, sleepers: usize) -> Result<World, PhysicsError> {
    let mut world = World::new(Vec3i::new(0, -SUBUNITS_PER_UNIT, 0));
    world.add_body(RigidBody::floor(FLOOR, 0))?;
    for index in 0..walkers {
        let (x, z) = grid(index);
        world.add_body(RigidBody::dynamic(
            walker(index),
            Vec3i::new(x, 100, z),
            Vec3i::ZERO,
            BODY,
        ))?;
    }
    for index in 0..sleepers {
        let (x, z) = grid(index);
        let id = sleeper(walkers, index);
        world.add_body(RigidBody::dynamic(
            id,
            Vec3i::new(x, 90, z - SLEEPERS_AWAY),
            Vec3i::ZERO,
            BODY,
        ))?;
        world.set_asleep(id, true)?;
    }
    for _ in 0..SETTLE_TICKS {
        world.step()?;
    }
    Ok(world)
}

pub fn measure(walkers: usize, sleepers: usize) -> Result<PairRun, PhysicsError> {
    let mut world = settled_world(walkers, sleepers)?;
    let mut run = PairRun::default();
    for _ in 0..MEASURED_TICKS {
        for index in 0..walkers {
            world.set_velocity(walker(index), WALK)?;
        }
        let stats = world.step()?;
        world.take_query_failure()?;
        run.tick_pairs = run.tick_pairs.max(stats.mover_pairs);
        run.moved = stats.moved_bodies;
    }
    Ok(run)
}

/// One `coding-tooling/work-evidence/v1` sample.
pub fn sample(walkers: usize, sleepers: usize) -> Result<String, PhysicsError> {
    let run = measure(walkers, sleepers)?;
    Ok(format!(
        "{{\"dimensions\":{{\"walkers\":{walkers},\"sleepers\":{sleepers}}},\"metrics\":{{\"tick_pairs\":{}}}}}",
        run.tick_pairs
    ))
}

/// The contract's scale points: (walkers, sleepers).
pub const SCALE_POINTS: [(usize, usize); 6] =
    [(8, 1), (32, 1), (128, 1), (512, 1), (128, 256), (128, 1024)];
