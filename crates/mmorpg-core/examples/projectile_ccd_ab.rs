//! Non-production projectile CCD probe against physics-engine PR #464.
//!
//! Reuses the actual Rogue bomb launch, sweep, fuse, and damage rules in `mmorpg-core`
//! while shadow-simulating a persistent, finite-radius body on physics-engine. The
//! shadow is intentionally *not* fed back into gameplay. Its physical restitution
//! and Coulomb friction are not equivalent to the game's per-impact speed scaling.
//!
//! ```sh
//! cargo run --release --locked -p mmorpg-core --features physics-ab --example projectile_ccd_ab
//! cargo test --locked -p mmorpg-core --test thrown_weapons
//! cargo test --locked -p mmorpg-core --test trick_shots
//! ```
//!
//! Timing compares a real arena zone tick with the same zone tick plus a shadow CCD
//! step. It is an *incremental* estimate, not the integrated replacement's speed.

#[path = "../tests/support/arena.rs"]
mod arena;

use std::error::Error;
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

use mmorpg_core::ability::ids;
use mmorpg_core::physics::{Backend, BodyId as GameBodyId, RigidBody, Vec3i, World as GameWorld};
use mmorpg_core::unit::player_max_health;
use mmorpg_core::{
    BOMB_FUSE_TICKS, CreatureId, EntityRef, Flight, Projectile, ProjectileKind, SpawnGrid,
    StaticCollider, ZoneAreas, ZoneCommand, ZoneContent, ZoneDefinition, ZoneId, ZoneSimulation,
};
use physics_engine::BodyId;
use physics_engine::approximate::{Body, Config, Shape, Vector, World};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const DT: f64 = 1.0 / 60.0;
const TICK_HZ: f32 = 60.0;
const BOMB_RADIUS_CM: f32 = 10.0;
const ROUNDS: usize = 5;
const WARMUP: usize = 1;
const BENCH_TICKS: usize = 160;

fn content() -> Result<Arc<ZoneContent>> {
    // Same arena/wall/boar positions as tests/thrown_weapons.rs, #290.
    let definition = ZoneDefinition::with_spawn_grid(
        mmorpg_core::STANDARD_GRAVITY,
        SpawnGrid {
            origin: [-5_000, -5_000],
            columns: 32,
            spacing: 100,
        },
        vec![
            StaticCollider {
                id: 1,
                position: [0, -50, 0],
                half_extents: [10_000, 50, 10_000],
            },
            StaticCollider {
                id: 2,
                position: [0, 200, 300],
                half_extents: [2_000, 200, 50],
            },
        ],
    )?;
    let spawns = [[200, 150], [0, 600]]
        .into_iter()
        .zip(1..)
        .map(|(at, id)| arena::spawn(id, arena::BOAR, at))
        .collect();
    let content = ZoneContent::new(
        definition,
        ZoneAreas::default(),
        vec![arena::boar()],
        spawns,
        Vec::new(),
        arena::GRAVEYARD,
    )?;
    Ok(Arc::new(content.with_creature_abilities(Vec::new())?))
}

/// Spawn a real level-six Rogue and launch its existing Blasting Bomb ability.
fn launched_zone() -> Result<ZoneSimulation> {
    let content = content()?;
    let mut zone = ZoneSimulation::with_content(ZoneId::new(9), Arc::clone(&content))?;
    zone.add_player(1)?;
    let mut state = zone.snapshot()?;
    state.players[0].combat.level = 6;
    state.players[0].combat.experience = 0;
    state.players[0].combat.health = player_max_health(6);
    state.players[0].position = [0.0, 90.0, 0.0];
    let mut zone = ZoneSimulation::from_snapshot(state, content)?;
    for (sequence, command) in [
        ZoneCommand::ChooseClass { class: 4, sex: 0 },
        arena::stand(arena::NORTHWARD),
        ZoneCommand::UseAbility {
            ability: ids::BLASTING_BOMB.get(),
            target: None,
        },
    ]
    .into_iter()
    .enumerate()
    {
        zone.apply_command(1, u32::try_from(sequence)? + 1, command)?;
        zone.advance_tick()?;
    }
    let initial = bomb(&zone)?;
    if initial.kind != ProjectileKind::Bomb || initial.age == 0 {
        return Err("Rogue bomb was not launched".into());
    }
    Ok(zone)
}

fn bomb(zone: &ZoneSimulation) -> Result<Projectile> {
    zone.snapshot()?
        .projectiles
        .into_iter()
        .find(|projectile| projectile.kind == ProjectileKind::Bomb)
        .ok_or_else(|| "expected a live Rogue bomb".into())
}

fn lane_offset(index: usize, count: usize, staggered: bool) -> [f32; 3] {
    let x = (index as f32 - (count as f32 - 1.0) / 2.0) * 75.0;
    // Separate firing lanes laterally; staggered lanes also vary height and distance.
    let y = if staggered {
        (index / 8) as f32 * 8.0
    } else {
        0.0
    };
    let z = if staggered {
        (index % 8) as f32 * 10.0
    } else {
        0.0
    };
    [x, y, z]
}

/// Reuse the real game projectile movement for the same number of bombs as the shadow.
fn game_lanes(count: usize, staggered: bool) -> Result<(ZoneSimulation, Projectile)> {
    let zone = launched_zone()?;
    let initial = bomb(&zone)?;
    let mut snapshot = zone.snapshot()?;
    snapshot.projectiles = (0..count)
        .map(|i| -> Result<Projectile> {
            let mut projectile = initial;
            projectile.id = initial.id + u32::try_from(i)?;
            for (axis, offset) in projectile
                .position
                .iter_mut()
                .zip(lane_offset(i, count, staggered))
            {
                *axis += offset;
            }
            Ok(projectile)
        })
        .collect::<Result<Vec<_>>>()?;
    snapshot.next_projectile = initial.id + u32::try_from(count)?;
    Ok((
        ZoneSimulation::from_snapshot(snapshot, Arc::clone(zone.content()))?,
        initial,
    ))
}

fn static_box(world: &mut World, key: u64, center: Vector, half: Vector) -> Result<()> {
    let mut body = Body::keyed(key, Shape::Box(half), center, 0.0);
    // Keep restitution separate from friction; the game's bounce uses 0.4 directly,
    // whereas the physical backend combines coefficients and transfers impulses.
    body.restitution = 0.4;
    body.friction = 0.6;
    world.add_body(body)?;
    Ok(())
}

fn physical_bombs(
    initial: Projectile,
    count: usize,
    ccd: bool,
    staggered: bool,
) -> Result<(World, Vec<BodyId>)> {
    let mut world = World::new(Config {
        gravity: Vector(0.0, -9.81, 0.0),
        length_unit: 100.0,
        substeps: 1,
        velocity_iterations: 8,
        ..Config::default()
    })?;
    static_box(
        &mut world,
        1,
        Vector(0.0, -50.0, 0.0),
        Vector(10_000.0, 50.0, 10_000.0),
    )?;
    static_box(
        &mut world,
        2,
        Vector(0.0, 200.0, 300.0),
        Vector(2_000.0, 200.0, 50.0),
    )?;
    let mut handles = Vec::with_capacity(count);
    for i in 0..count {
        let [x, y, z] = lane_offset(i, count, staggered);
        let mut body = Body::keyed(
            10_000 + i as u64,
            Shape::Sphere(BOMB_RADIUS_CM),
            Vector(
                initial.position[0] + x,
                initial.position[1] + y,
                initial.position[2] + z,
            ),
            1.0,
        );
        body.velocity = Vector(
            initial.velocity[0] * TICK_HZ,
            initial.velocity[1] * TICK_HZ,
            initial.velocity[2] * TICK_HZ,
        );
        body.restitution = 0.4;
        body.friction = 0.6;
        body.ccd = ccd;
        body.sleep_allowed = false;
        handles.push(world.add_body(body)?);
    }
    Ok((world, handles))
}

fn state(world: &World, id: BodyId) -> Result<([f32; 3], [f32; 3])> {
    let body = world.body(id).ok_or("CCD body went missing")?;
    Ok((
        [body.position.0, body.position.1, body.position.2],
        [body.velocity.0, body.velocity.1, body.velocity.2],
    ))
}

fn bomb_comparison() -> Result<()> {
    let mut zone = launched_zone()?;
    let initial = bomb(&zone)?;
    let (mut world, ids) = physical_bombs(initial, 1, true, false)?;
    let mut swept = 0;
    let mut started = 0;
    let mut first_collision = None;
    let mut gameplay_burst = None;
    let mut actual_damage_near = 0_u32;
    let mut actual_damage_far = 0_u32;
    let mut last_game = initial.position;
    let mut last_ccd = initial.position;
    for age in initial.age + 1..=BOMB_FUSE_TICKS {
        zone.advance_tick()?;
        let next = bomb(&zone)?;
        let events = zone.snapshot_for_player(1)?.events;
        for event in events {
            if let mmorpg_core::ZoneEvent::DamageDealt { target, amount, .. } = event {
                match target {
                    EntityRef::Creature(id) if id == CreatureId::new(1) => {
                        actual_damage_near += u32::from(amount);
                    }
                    EntityRef::Creature(id) if id == CreatureId::new(2) => {
                        actual_damage_far += u32::from(amount);
                    }
                    _ => {}
                }
            }
        }
        last_game = next.position;
        if matches!(next.flight, Flight::Burst { .. }) {
            gameplay_burst = Some(age);
            break;
        }
        let report = world.step(DT)?;
        swept += report.swept_contacts;
        started += report.contact_events.iter().filter(|e| e.started()).count();
        if first_collision.is_none() && !report.contact_events.is_empty() {
            first_collision = Some(age);
        }
        last_ccd = state(&world, ids[0])?.0;
    }
    if gameplay_burst != Some(BOMB_FUSE_TICKS) || actual_damage_near == 0 || actual_damage_far > 0 {
        return Err(format!(
            "gameplay bomb contract changed: burst={gameplay_burst:?}, near={actual_damage_near}, far={actual_damage_far}"
        )
        .into());
    }
    if last_ccd.iter().any(|value| !value.is_finite()) {
        return Err("CCD produced nonfinite projectile position".into());
    }
    println!(
        "rogue-bomb\tgame_fuse_tick={}\tgame_burst_cm={last_game:?}\tccd_fuse_cm={last_ccd:?}\tccd_contact_tick={first_collision:?}\tswept_contacts={swept}\tcontact_starts={started}\tgame_damage_near={actual_damage_near}\tgame_damage_far={actual_damage_far}",
        BOMB_FUSE_TICKS
    );
    Ok(())
}

/// The game's existing ricochet is a per-tick ray and one reflection; #464's
/// finite sphere consumes its remaining tick interval and can strike both walls.
fn ricochet_comparison() -> Result<()> {
    let mut game = GameWorld::with_backend(Vec3i::ZERO, Backend::Rapier);
    for (id, x) in [(1, 45), (2, -25)] {
        game.add_body(RigidBody::fixed(
            GameBodyId(id),
            Vec3i::new(x, 0, 0),
            Vec3i::new(5, 100, 50),
        ))?;
    }
    let first = game
        .cast_ray([0.0, 0.0, 0.0], [75.0, 0.0, 0.0], |_| true)
        .ok_or("game ricochet did not hit the first wall")?;
    let point = 75.0 * first.fraction + first.normal[0]; // 1 cm clearance (#291)
    let reflected = 75.0 - 2.0 * 75.0 * first.normal[0] * first.normal[0];

    let mut world = World::new(Config {
        gravity: Vector::ZERO,
        length_unit: 100.0,
        substeps: 1,
        velocity_iterations: 8,
        ..Config::default()
    })?;
    for (id, x) in [(1, 45.0), (2, -25.0)] {
        let mut wall = Body::keyed(
            id,
            Shape::Box(Vector(5.0, 100.0, 50.0)),
            Vector(x, 0.0, 0.0),
            0.0,
        );
        wall.restitution = 1.0;
        wall.friction = 0.0;
        world.add_body(wall)?;
    }
    let mut ball = Body::keyed(3, Shape::Sphere(10.0), Vector::ZERO, 1.0);
    ball.velocity = Vector(4_500.0, 0.0, 0.0);
    ball.restitution = 1.0;
    ball.friction = 0.0;
    ball.ccd = true;
    ball.sleep_allowed = false;
    let id = world.add_body(ball)?;
    let report = world.step(DT)?;
    let (position, velocity) = state(&world, id)?;
    if (position[0] + 5.0).abs() > 4.0 || (velocity[0] - 4_500.0).abs() > 100.0 {
        return Err(format!("multi-impact CCD regressed: {position:?} / {velocity:?}").into());
    }
    println!(
        "ricochet-first-tick\tgame_ray_cm={point:.2}\tgame_ray_vx_cm_per_tick={reflected:.2}\tccd_sphere_cm={:.2}\tccd_vx_cm_per_second={:.2}\tswept_contacts={}\tcontact_starts={}",
        position[0],
        velocity[0],
        report.swept_contacts,
        report.contact_events.iter().filter(|e| e.started()).count()
    );
    Ok(())
}

/// Exact on-the-same-build replay, independent of the noisy performance timer.
fn deterministic_replay() -> Result<()> {
    let initial = bomb(&launched_zone()?)?;
    let trace = || -> Result<Vec<([u32; 3], [u32; 3])>> {
        let (mut world, ids) = physical_bombs(initial, 1, true, false)?;
        let mut observations = Vec::new();
        for _ in 0..60 {
            world.step(DT)?;
            let (p, v) = state(&world, ids[0])?;
            observations.push((p.map(f32::to_bits), v.map(f32::to_bits)));
        }
        Ok(observations)
    };
    if trace()? != trace()? {
        return Err("persistent CCD replay was not bit-exact".into());
    }
    let legacy = || -> Result<Vec<Projectile>> {
        let mut zone = launched_zone()?;
        let mut trace = Vec::new();
        for _ in 0..60 {
            zone.advance_tick()?;
            trace.push(bomb(&zone)?);
        }
        Ok(trace)
    };
    if legacy()? != legacy()? {
        return Err("existing projectile replay was not exact".into());
    }
    println!("replay\tgame=exact\tccd=bit-exact\tticks=60");
    Ok(())
}

#[derive(Clone, Copy, Default)]
struct RoundSample {
    micros: f64,
    physics_micros: f64,
    pairs: u64,
    narrows: u64,
    sweep_queries: u64,
    impacts: u64,
    budget_fallbacks: u64,
}

fn round(amount: usize, staggered: bool, ccd: Option<bool>) -> Result<RoundSample> {
    let (mut zone, initial) = game_lanes(amount, staggered)?;
    let mut shadow = ccd
        .map(|enabled| physical_bombs(initial, amount, enabled, staggered))
        .transpose()?;
    let mut sample = RoundSample::default();
    let start = Instant::now();
    for tick in 0..BENCH_TICKS {
        zone.advance_tick()?;
        if let Some((world, ids)) = shadow.as_mut() {
            let age = tick + usize::from(initial.age) + 1;
            if age < usize::from(BOMB_FUSE_TICKS) {
                let physics_start = Instant::now();
                let report = world.step(DT)?;
                sample.physics_micros += physics_start.elapsed().as_secs_f64() * 1e6;
                sample.pairs += report.pair_tests;
                sample.narrows += report.narrow_tests;
                sample.sweep_queries += report.geometry.sweep_queries;
                sample.impacts += report.swept_contacts;
                sample.budget_fallbacks += report.geometry.ccd_budget_fallbacks;
                black_box(report);
            } else if age == usize::from(BOMB_FUSE_TICKS) {
                for &id in ids.iter() {
                    black_box(world.remove_body(id));
                }
            }
        }
    }
    black_box(zone.snapshot()?.tick);
    sample.micros = start.elapsed().as_secs_f64() * 1e6 / BENCH_TICKS as f64;
    sample.physics_micros /= BENCH_TICKS as f64;
    Ok(sample)
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn bench() -> Result<()> {
    println!(
        "full-tick-shadow-cost\tworkload=rogue-bomb-arena\tticks={BENCH_TICKS}\twarmup={WARMUP}\trounds={ROUNDS}"
    );
    for (count, staggered) in [
        (1, false),
        (8, false),
        (16, false),
        (24, false),
        (32, false),
        (32, true),
    ] {
        let mut base = Vec::new();
        let mut dynamic = Vec::new();
        let mut added = Vec::new();
        let mut dynamic_step = Vec::new();
        let mut ccd_step = Vec::new();
        let mut work = RoundSample::default();
        for iteration in 0..WARMUP + ROUNDS {
            let (a, b, c) = if iteration % 2 == 0 {
                (
                    round(count, staggered, None)?,
                    round(count, staggered, Some(false))?,
                    round(count, staggered, Some(true))?,
                )
            } else {
                let c = round(count, staggered, Some(true))?;
                let b = round(count, staggered, Some(false))?;
                (round(count, staggered, None)?, b, c)
            };
            if iteration >= WARMUP {
                base.push(a.micros);
                dynamic.push(b.micros);
                added.push(c.micros);
                dynamic_step.push(b.physics_micros);
                ccd_step.push(c.physics_micros);
                work = c;
            }
        }
        let baseline = median(&mut base);
        let nonccd = median(&mut dynamic);
        let candidate = median(&mut added);
        let nonccd_physics = median(&mut dynamic_step);
        let ccd_physics = median(&mut ccd_step);
        println!(
            "bombs={count}\tstaggered={staggered}\tgame_us_per_tick={baseline:.2}\tgame_plus_dynamic_us_per_tick={nonccd:.2}\tgame_plus_ccd_us_per_tick={candidate:.2}\tdynamic_step_us={nonccd_physics:.2}\tccd_step_us={ccd_physics:.2}\tccd_increment_over_dynamic_us_per_tick={:.2}\tshadow_overhead_us_per_tick={:.2}\tratio={:.2}\tccd_pairs={}\tccd_narrow_tests={}\tccd_sweep_queries={}\tccd_impacts={}\tccd_budget_fallbacks={}",
            candidate - nonccd,
            candidate - baseline,
            candidate / baseline,
            work.pairs,
            work.narrows,
            work.sweep_queries,
            work.impacts,
            work.budget_fallbacks
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    ricochet_comparison()?;
    bomb_comparison()?;
    deterministic_replay()?;
    bench()?;
    Ok(())
}
