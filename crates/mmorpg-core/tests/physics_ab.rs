//! Physics backend A/B (physics-engine #330): the real zone simulation on Rapier and on
//! physics-engine, from the same initial state with the same inputs and tick count. Only
//! built with `--features physics-ab`; Rapier stays the production authority (Atlas 2.1.6).
//!
//! Every workload runs twice on each backend. Each run must keep shared invariants (finite
//! state, no penetration of content boxes, door leaves, props, the floor or the terrain beyond
//! [`PENETRATION_TOLERANCE_UNITS`], units at rest grounded), replay bit-identically on its own
//! backend and, restored from its canonical snapshot halfway, continue exactly (Atlas 2.1.5).
//! Across backends, scripted players' positions must agree within
//! [`SCRIPTED_POSITION_TOLERANCE_UNITS`] with the same command outcomes and door states, and
//! every query decision gameplay makes (line of sight, NPC sight, projectile contact, door
//! occupancy, ground height, grounded) must agree on identical states: Rapier's state at
//! sampled ticks, recovered onto both backends. No workload places a unit inside geometry
//! (#534). Creatures' positions (AI and a shared RNG react to every position) are compared but
//! not bounded; their divergences are printed (`--nocapture`) and listed in
//! `docs/PHYSICS_AB.md`.
//!
//! Each workload is its own trial named after it (`harness = false`, libtest-mimic, #540), so
//! nextest runs them in parallel: `cargo nextest run -p mmorpg-core --features physics-ab
//! --test physics_ab -E 'test(=vale-roll)'` reruns one.

#[path = "support/physics_ab.rs"]
mod drivers;
#[path = "support/greyhaven_workload.rs"]
mod greyhaven_workload;

use std::collections::BTreeMap;
use std::error::Error;

use drivers::{Run, Workload};
use mmorpg_core::physics::{Backend, BodyId, using_backend};
use mmorpg_core::physics_ab::UnitBody;
use mmorpg_core::{CanonicalZoneSnapshot, EntityRef, ZoneSimulation};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// Both backends keep a 0.25-unit contact offset to everything a unit touches; at the zone's
/// magnitudes (|x| ≤ 10⁴ units) `f32` resolves about 10⁻³ units. A unit may sit closer than
/// its offset only by such rounding, never inside a box.
const PENETRATION_TOLERANCE_UNITS: f32 = 0.01;
/// The contact offset the backends raise a body's capsule by (see `physics.rs`).
const CONTACT_OFFSET_UNITS: f32 = 0.25;
/// Scripted players (no AI, no unit contact) run the same intents into the same walls: both
/// controllers slide and stop at the same 0.25-unit gap, so their positions may differ only by
/// sub-unit contact-resolution detail. One unit is the rounding step gameplay sees.
const SCRIPTED_POSITION_TOLERANCE_UNITS: f32 = 1.0;
/// The workloads whose players' positions are bounded: the bot scenarios, the idle plaza, the
/// fighting fixtures (their bots stand clear of all geometry, #534) and the scripted scenes.
/// At 60 Hz (#379) `vale-wolf-hunt`'s creatures, which part from tick 1, reach the scripted
/// hunter's fights after 38 s, so it is bounded only over its scripted walk ([`CLIMBS`]).
const SCRIPTED: [&str; 14] = [
    "two-bots-move",
    "vale-hub-walk",
    "vale-idle-16",
    "vale-fights-8",
    "vale-fights-32",
    "vale-crowded-fights-64",
    "vale-swim",
    "vale-mill-hoist",
    "vale-crate-shove",
    "vale-crate-smash",
    "vale-missiles",
    "vale-traversal",
    "vale-climb",
    "vale-roll",
];
/// The controllers walk up and down heightfield slopes at slightly different paces (open gap
/// 5 in `docs/PHYSICS_AB.md`). Owner decision 2026-10-25 (#423): on a slope the scripted
/// players may part by this fraction of the distance they walked on slopes so far (0.2 %, about
/// 6 units over the Wolfrun Climb), on top of [`SCRIPTED_POSITION_TOLERANCE_UNITS`]. Flat
/// walking adds nothing, so every comparison before the first slope stays exact.
const SLOPE_DIVERGENCE_PER_UNIT: f32 = 0.002;
/// A tick on which a player's feet rise or fall this far is walked on a slope (a 1:5 slope
/// moves them about 2 units a tick; contact jitter on flat ground stays near 0.01).
const SLOPE_STEP_UNITS: f32 = 0.5;
/// Scripted walks over slopes, by workload, bounded with [`SLOPE_DIVERGENCE_PER_UNIT`] up to
/// the tick their fight can begin: `vale-wolf-hunt` up the Wolfrun Climb (#423) and through
/// Wolfrun Woods until it turns toward the den wolf.
const CLIMBS: [(&str, usize); 1] = [("vale-wolf-hunt", 1_434)];
/// Shot contact fractions on identical states: both backends solve the same ray against the
/// same boxes, capsules and triangles in `f32`.
const SHOT_FRACTION_TOLERANCE: f32 = 1.0e-4;
/// A unit at rest this many ticks must be grounded.
const REST_TICKS: usize = 3;
/// Penetration is checked on every this-many-th tick (and the last), over every content box.
const PENETRATION_EVERY: usize = mmorpg_core::golden::t30(10) as usize;
/// Query parity is checked on Rapier's state every this-many-th tick (and the last).
const PARITY_EVERY: usize = mmorpg_core::golden::t30(60) as usize;
/// Pairs within this distance (units) are probed for line of sight, shots and NPC sight.
const PROBE_RANGE_UNITS: f32 = 4_000.0;

/// What the invariant checks saw over one run.
#[derive(Debug)]
struct Check {
    /// Ogre brawl (#297): the smallest gap between the ogre's capsule and a player's, ticks
    /// a player was being shoved, and whether a stomp hurt a player.
    ogre_gap: f32,
    shoved_ticks: usize,
    stomped: bool,
    /// The smallest capsule-to-box gap (units).
    closest: f32,
    /// The first tick the zone restored halfway differs from the uninterrupted one.
    departed: Option<u64>,
}

#[derive(Debug, Default, PartialEq)]
struct Trace {
    snapshots: Vec<CanonicalZoneSnapshot>,
    units: Vec<Vec<UnitBody>>,
    outcomes: Vec<String>,
}

/// A unit's capsule as both backends build it: `(half segment, radius)` in units.
fn capsule(unit: &UnitBody) -> (f32, f32) {
    mmorpg_core::physics::capsule_dimensions(unit.body.half_extents())
}

/// An upright capsule's distance outside an axis-aligned box (negative inside).
fn capsule_box_gap(
    centre: [f32; 3],
    (segment, radius): (f32, f32),
    box_centre: [f32; 3],
    box_half: [f32; 3],
) -> f32 {
    // Horizontal: the centre's distance from the box footprint; vertical: the axis segment's
    // distance from the box's height span. The capsule is a swept sphere along the axis.
    let dx = (centre[0] - box_centre[0]).abs() - box_half[0];
    let dz = (centre[2] - box_centre[2]).abs() - box_half[2];
    let dy = (centre[1] - box_centre[1]).abs() - segment - box_half[1];
    let outside = [dx.max(0.0), dy.max(0.0), dz.max(0.0)];
    let distance = outside.iter().map(|d| d * d).sum::<f32>().sqrt();
    let inside = dx.max(dy).max(dz).min(0.0);
    distance + inside - radius
}

fn finite(snapshot: &CanonicalZoneSnapshot) -> bool {
    snapshot
        .players
        .iter()
        .flat_map(|player| player.position.iter().chain(&player.velocity))
        .chain(
            snapshot
                .creatures
                .iter()
                .flat_map(|creature| creature.position.iter().chain(&creature.velocity)),
        )
        .all(|value| value.is_finite())
}

/// Every content box, door leaf and standing prop the zone currently blocks with (NPC boxes
/// are left out: routine NPCs are placed through units without contact resolution).
fn boxes(zone: &ZoneSimulation) -> Vec<([f32; 3], [f32; 3])> {
    let exact = |value: [i32; 3]| mmorpg_core::physics::from_units(value);
    let mut boxes: Vec<_> = zone
        .definition()
        .colliders()
        .iter()
        .map(|collider| (exact(collider.position), exact(collider.half_extents)))
        .collect();
    let doors = zone.content().doors().iter().map(|door| {
        *zone
            .physics()
            .body(BodyId(u64::from(door.body)))
            .expect("door leaf body")
    });
    for body in doors.chain(zone.prop_bodies()) {
        let (centre, half) = (body.position(), body.half_extents());
        boxes.push((
            exact([centre.x, centre.y, centre.z]),
            exact([half.x, half.y, half.z]),
        ));
    }
    boxes
}

/// Whether the floor half-space is open under `unit`: over terrain that dips below it (Stillwater
/// Lake's basin, #274) only the heightfield is ground, which the sink check covers. Mirrors the
/// backends: it opens under `unit`'s footprint over any terrain cell with a sample below zero.
fn open_floor(zone: &ZoneSimulation, unit: &UnitBody) -> bool {
    let [x, _, z] = unit.body.exact_position();
    let half = unit.body.half_extents();
    #[allow(clippy::cast_possible_truncation)]
    let (x, z, reach) = (x.round() as i32, z.round() as i32, half.x.max(half.z) + 1);
    zone.definition()
        .terrain()
        .is_some_and(|terrain| terrain.low_within(x, z, reach) < 0)
}

/// Shared invariants of one tick on one backend.
fn check_invariants(
    zone: &ZoneSimulation,
    snapshot: &CanonicalZoneSnapshot,
    units: &[UnitBody],
    history: &[Vec<UnitBody>],
    check: &mut Check,
) -> std::result::Result<(), String> {
    let tick = snapshot.tick;
    if !finite(snapshot)
        || units
            .iter()
            .any(|unit| !unit.body.exact_position().iter().all(|v| v.is_finite()))
    {
        return Err(format!("non-finite state at tick {tick}"));
    }
    let every = tick == 1 || usize::try_from(tick).unwrap_or(0) % PENETRATION_EVERY == 0;
    if every {
        let boxes = boxes(zone);
        for unit in units {
            let [x, y, z] = unit.body.exact_position();
            let shape = capsule(unit);
            let centre = [x, y + CONTACT_OFFSET_UNITS, z];
            let mut deepest = None;
            let mut closest = f32::INFINITY;
            let open = open_floor(zone, unit);
            for &(box_centre, box_half) in &boxes {
                let ground = box_centre[1] + box_half[1] <= 0.0;
                if ground && open {
                    continue;
                }
                let gap = capsule_box_gap(centre, shape, box_centre, box_half);
                if gap < -PENETRATION_TOLERANCE_UNITS
                    && deepest.is_none_or(|(depth, _)| -gap > depth)
                {
                    deepest = Some((-gap, (box_centre, box_half)));
                }
                closest = closest.min(gap);
            }
            // No workload places a unit inside geometry (#534), so this holds from tick 1.
            if let Some((depth, (box_centre, box_half))) = deepest {
                return Err(format!(
                    "{:?} at {:?} penetrates box {box_centre:?}±{box_half:?} by {depth} at tick {tick}",
                    unit.unit,
                    [x, y, z]
                ));
            }
            check.closest = check.closest.min(closest);
            #[allow(clippy::cast_possible_truncation)]
            let terrain = zone
                .physics()
                .terrain_height(x.round() as i32, z.round() as i32)
                .unwrap_or(0.0);
            // The floor half-space holds units at y = 0 except where it is open.
            let ground = if open { terrain } else { terrain.max(0.0) };
            let bottom = centre[1] - shape.0 - shape.1;
            // The terrain sample is the ray height at the rounded column, so allow for its
            // slope over half a unit.
            if bottom < ground - 2.0 {
                return Err(format!(
                    "{:?} at {:?} (capsule {shape:?}) sinks {} below the ground {ground} at tick {tick}",
                    unit.unit,
                    [x, y, z],
                    ground - bottom
                ));
            }
        }
    }
    // A unit that has not moved and has no velocity for REST_TICKS ticks stands on something.
    if history.len() >= REST_TICKS {
        for unit in units {
            let rested = history[history.len() - REST_TICKS..].iter().all(|earlier| {
                earlier.iter().any(|before| {
                    before.unit == unit.unit
                        && before.body.exact_position() == unit.body.exact_position()
                        && before.body.exact_velocity() == [0.0; 3]
                })
            });
            // A weightless unit (a swimmer #274, a climber #490) rests without ground under it.
            if rested
                && unit.body.exact_velocity() == [0.0; 3]
                && !unit.grounded
                && !unit.body.weightless()
            {
                return Err(format!("{:?} rests ungrounded at tick {tick}", unit.unit));
            }
        }
    }
    Ok(())
}

/// The gap between two upright capsules given as unit bodies (their game centres).
fn capsule_gap(a: &UnitBody, b: &UnitBody) -> f32 {
    let ((sa, ra), (sb, rb)) = (capsule(a), capsule(b));
    let (p, q) = (a.body.exact_position(), b.body.exact_position());
    let vertical = ((p[1] - q[1]).abs() - sa - sb).max(0.0);
    (p[0] - q[0]).hypot(p[2] - q[2]).hypot(vertical) - ra - rb
}

/// The ogre brawl's own observations (#297): the ogre's body never takes a player in, shoves
/// (forced moves of players) happen, and the ogre stomps.
fn check_ogre(
    zone: &ZoneSimulation,
    snapshot: &CanonicalZoneSnapshot,
    units: &[UnitBody],
    check: &mut Check,
) {
    let ogre = EntityRef::Creature(mmorpg_core::CreatureId::new(710));
    let Some(ogre_body) = units.iter().find(|unit| unit.unit == ogre) else {
        return;
    };
    for unit in units
        .iter()
        .filter(|unit| matches!(unit.unit, EntityRef::Player(_)))
    {
        check.ogre_gap = check.ogre_gap.min(capsule_gap(ogre_body, unit));
    }
    check.shoved_ticks += snapshot
        .forced
        .iter()
        .filter(|(unit, state)| matches!(unit, EntityRef::Player(_)) && state.moving.is_some())
        .count();
    check.stomped |= snapshot.players.iter().any(|player| {
        zone.snapshot_for_player(player.player_id)
            .is_ok_and(|projection| {
                projection.events.iter().any(|event| {
                    matches!(
                        event,
                        mmorpg_core::ZoneEvent::AbilityUsed { ability, .. }
                            if *ability == mmorpg_core::ability::ids::STOMP
                    )
                })
            })
    });
}

/// Millbrook Farm's terrace (#423), which the dock, its ramps and the hoist stand on.
#[allow(clippy::cast_precision_loss)]
const TERRACE: f32 = mmorpg_core::greyhaven_vale::basin::TERRACE_Y as f32;

/// The ramp walk on one backend (#59): the walker is on the dock (feet 1.5 m up) when it
/// turns back and on the ground at the end; the chute never lets the other player onto the
/// dock.
fn check_mill_ramps(trace: &Trace) -> std::result::Result<(), String> {
    let feet = |tick: usize, player: usize| {
        trace.snapshots[tick - 1].players[player].position[1] - 90.0 - TERRACE
    };
    // Where the script turns the walker back west (tick 44 at 30 Hz).
    let top = usize::try_from(mmorpg_core::golden::t30(44)).unwrap();
    if (feet(top, 0) - 150.0).abs() > 3.0 || feet(trace.snapshots.len(), 0).abs() > 0.5 {
        return Err(format!(
            "the walker's feet at {top} {} and at the end {}",
            feet(top, 0),
            feet(trace.snapshots.len(), 0)
        ));
    }
    if let Some(tick) = (1..=trace.snapshots.len()).find(|&tick| feet(tick, 1) >= 149.0) {
        return Err(format!(
            "the chute let player 2 onto the dock at tick {tick}"
        ));
    }
    Ok(())
}

/// The climb (#490): the climber ends standing on the crag's top, the mantler on the dock's
/// deck, each within a unit of it.
fn check_wall_climb(trace: &Trace) -> Result<()> {
    use mmorpg_core::greyhaven_vale::ids;
    let last = trace.snapshots.last().ok_or("empty climb trace")?;
    let content = mmorpg_core::greyhaven_vale::content();
    let top = |id: u32| {
        let collider = content
            .definition()
            .colliders()
            .iter()
            .find(|collider| collider.id == id)
            .ok_or("missing climb collider")?;
        #[allow(clippy::cast_precision_loss)]
        let top = (collider.position[1] + collider.half_extents[1]) as f32;
        Ok::<_, &str>(top)
    };
    let feet = |player: usize| last.players[player].position[1] - 90.0;
    let crag = top(*ids::HIGHLAND_CRAGS.start())?;
    let dock = top(ids::MILL_DOCK)?;
    if (feet(0) - crag).abs() > 1.0 || (feet(1) - dock).abs() > 1.0 {
        return Err(format!(
            "the climber's feet end at {} (crag top {crag}), the mantler's at {} (deck {dock})",
            feet(0),
            feet(1)
        )
        .into());
    }
    Ok(())
}

/// The rolls (#516): every player rolls at some tick, the crag roller never climbs, and the
/// dock roller ends on the ground beyond the deck's edge.
fn check_rolls(trace: &Trace) -> Result<()> {
    use mmorpg_core::greyhaven_vale::ids;
    for player in 0..drivers::ROLL_PLAYERS.len() {
        if !trace
            .snapshots
            .iter()
            .any(|snapshot| snapshot.players[player].combat.roll.rolling().is_some())
        {
            return Err(format!("player {} never rolls", player + 1).into());
        }
    }
    if trace
        .snapshots
        .iter()
        .any(|snapshot| snapshot.players[1].combat.traversal.holds())
    {
        return Err("the crag roller climbs".into());
    }
    let content = mmorpg_core::greyhaven_vale::content();
    let deck = content
        .definition()
        .colliders()
        .iter()
        .find(|collider| collider.id == ids::MILL_DOCK)
        .ok_or("missing dock")?;
    let last = trace.snapshots.last().ok_or("empty roll trace")?;
    let [_, y, z] = last.players[2].position;
    #[allow(clippy::cast_precision_loss)]
    let (edge, top) = (
        (deck.position[2] + deck.half_extents[2]) as f32,
        (deck.position[1] + deck.half_extents[1]) as f32,
    );
    if z < edge + 30.0 || y - 90.0 > top - 100.0 {
        return Err(format!(
            "the dock roller ends at {:?} (deck edge z {edge}, top {top})",
            last.players[2].position
        )
        .into());
    }
    Ok(())
}

/// The hoist ride (#56): the rider ends on the windmill's gallery, 8 m up.
fn check_mill_hoist(trace: &Trace) -> Result<()> {
    let last = trace.snapshots.last().ok_or("empty hoist trace")?;
    let [_, y, z] = last.players[0].position;
    if (y - 890.0 - TERRACE).abs() > 0.1 || z < 1_900.0 {
        return Err(format!("the hoist rider ends at {:?}", last.players[0].position).into());
    }
    Ok(())
}

/// Settled debris (#288) may rest this far (units, horizontally) from Rapier's: physics-engine's
/// `debris` oracle bound for every piece.
const DEBRIS_REST_TOLERANCE_UNITS: f32 = 150.0;

/// Both crates break into six panels each on each backend, at the same tick; every piece stays
/// finite and above the ground, settles before it despawns, and rests near Rapier's.
fn check_crate_smash(name: &str, rapier: &Trace, engine: &Trace) {
    let broke = |trace: &Trace| trace.snapshots.iter().position(|s| s.debris.len() == 12);
    let lying = |trace: &Trace| {
        trace
            .snapshots
            .iter()
            .rev()
            .find(|snapshot| !snapshot.debris.is_empty())
            .map(|snapshot| snapshot.debris.clone())
            .unwrap_or_default()
    };
    for (trace, backend) in [(rapier, "rapier"), (engine, "engine")] {
        assert!(
            broke(trace).is_some(),
            "{name} on {backend}: crates did not break"
        );
        let pieces = trace.snapshots.iter().flat_map(|snapshot| &snapshot.debris);
        for piece in pieces {
            assert!(
                piece.body.is_valid() && piece.body.position[1] > -2.0,
                "{name} on {backend}: piece {} at {:?}",
                piece.id,
                piece.body.position
            );
        }
        assert!(
            lying(trace).iter().all(|piece| piece.body.resting),
            "{name} on {backend}: a piece still moves when the debris despawns"
        );
        assert!(
            trace
                .snapshots
                .last()
                .is_some_and(|last| last.debris.is_empty()),
            "{name} on {backend}: debris did not despawn"
        );
    }
    assert_eq!(
        broke(rapier),
        broke(engine),
        "{name}: crates break at different ticks"
    );
    let (a, b) = (lying(rapier), lying(engine));
    assert_eq!(a.len(), b.len(), "{name}: different debris");
    let gap = a
        .iter()
        .zip(&b)
        .map(|(x, y)| {
            (x.body.position[0] - y.body.position[0]).hypot(x.body.position[2] - y.body.position[2])
        })
        .fold(0.0_f32, f32::max);
    println!("  {name}: settled debris parts by at most {gap:.2}");
    assert!(
        gap <= DEBRIS_REST_TOLERANCE_UNITS,
        "{name}: debris parts by {gap}"
    );
}

fn run(workload: &Workload, backend: Backend) -> Result<(Trace, Check)> {
    using_backend(backend, || {
        let mut run = Run::new(workload)?;
        assert_eq!(run.zone.physics().backend(), backend);
        let mut trace = Trace::default();
        // Restored from its canonical snapshot halfway, the zone continues exactly.
        let mut restored: Option<Run> = None;
        let mut check = Check {
            ogre_gap: f32::INFINITY,
            shoved_ticks: 0,
            stomped: false,
            closest: f32::INFINITY,
            departed: None,
        };
        while !run.finished() {
            run.advance()?;
            let snapshot = run.zone.snapshot()?;
            let units = run.zone.physics_units();
            check_invariants(&run.zone, &snapshot, &units, &trace.units, &mut check).map_err(
                |failure| format!("{} on {}: {failure}", workload.name(), backend.name()),
            )?;
            if matches!(workload, Workload::OgreBrawl) {
                check_ogre(&run.zone, &snapshot, &units, &mut check);
            }
            if let Some(restored) = restored.as_mut() {
                restored.advance()?;
                if check.departed.is_none() && restored.zone.snapshot()? != snapshot {
                    check.departed = Some(snapshot.tick);
                }
            }
            if run.tick == workload.ticks() / 2 {
                restored = Some(run.recovered()?);
            }
            trace.snapshots.push(snapshot);
            trace.units.push(units);
        }
        trace.outcomes = run.outcomes;
        Ok((trace, check))
    })
}

/// Largest horizontal and vertical difference between the backends' players, and the first
/// tick their rounded positions differ.
/// Checks the scripted players over the first `until` ticks: on every tick they agree within
/// [`SCRIPTED_POSITION_TOLERANCE_UNITS`] plus [`SLOPE_DIVERGENCE_PER_UNIT`] of the distance
/// each has walked on slopes so far (measured on Rapier's arm, the reference). Returns the
/// largest divergence and the largest allowance.
fn check_climb(
    rapier: &Trace,
    engine: &Trace,
    until: usize,
) -> std::result::Result<[f32; 2], String> {
    let players = rapier.snapshots.first().map_or(0, |s| s.players.len());
    let mut sloped = vec![0.0_f32; players];
    let (mut worst_allowance, mut worst) = (0.0_f32, 0.0_f32);
    let ticks = rapier.snapshots.iter().zip(&engine.snapshots).take(until);
    for (index, (left, right)) in ticks.enumerate() {
        for (player, (p, q)) in left.players.iter().zip(&right.players).enumerate() {
            if let Some(before) = index
                .checked_sub(1)
                .and_then(|previous| rapier.snapshots[previous].players.get(player))
                && let Some(walked) = sloped.get_mut(player)
            {
                let [dx, dy, dz] = [0, 1, 2].map(|axis| p.position[axis] - before.position[axis]);
                if dy.abs() > SLOPE_STEP_UNITS {
                    *walked += dx.hypot(dz);
                }
            }
            let allowance = SCRIPTED_POSITION_TOLERANCE_UNITS
                + SLOPE_DIVERGENCE_PER_UNIT * sloped.get(player).copied().unwrap_or(0.0);
            worst_allowance = worst_allowance.max(allowance);
            let d = [0, 1, 2].map(|axis| (p.position[axis] - q.position[axis]).abs());
            worst = worst.max(d[0].hypot(d[2])).max(d[1]);
            if d[0].hypot(d[2]) > allowance || d[1] > allowance {
                return Err(format!(
                    "tick {}: player {player} Rapier {:?} engine {:?} beyond {allowance:.2} units",
                    left.tick, p.position, q.position
                ));
            }
        }
    }
    Ok([worst, worst_allowance])
}

fn player_divergence(a: &Trace, b: &Trace) -> (f32, f32, Option<u64>) {
    let (mut horizontal, mut vertical, mut first) = (0.0_f32, 0.0_f32, None);
    for (left, right) in a.snapshots.iter().zip(&b.snapshots) {
        for (p, q) in left.players.iter().zip(&right.players) {
            let d = [0, 1, 2].map(|axis| (p.position[axis] - q.position[axis]).abs());
            horizontal = horizontal.max(d[0].hypot(d[2]));
            vertical = vertical.max(d[1]);
            if first.is_none()
                && mmorpg_core::physics::to_units(p.position)
                    != mmorpg_core::physics::to_units(q.position)
            {
                first = Some(left.tick);
            }
        }
    }
    (horizontal, vertical, first)
}

/// Largest final horizontal creature difference, how many creatures end a unit or more apart
/// or in a different life state, and the first tick a creature's rounded position differs.
fn creature_divergence(a: &Trace, b: &Trace) -> (f32, usize, Option<u64>) {
    let (mut worst, mut differing) = (0.0_f32, 0);
    if let (Some(left), Some(right)) = (a.snapshots.last(), b.snapshots.last()) {
        for (p, q) in left.creatures.iter().zip(&right.creatures) {
            let d = (p.position[0] - q.position[0]).hypot(p.position[2] - q.position[2]);
            worst = worst.max(d);
            differing += usize::from(p.life != q.life || d >= 1.0);
        }
    }
    let first = a
        .snapshots
        .iter()
        .zip(&b.snapshots)
        .find_map(|(left, right)| {
            left.creatures
                .iter()
                .zip(&right.creatures)
                .any(|(p, q)| {
                    mmorpg_core::physics::to_units(p.position)
                        != mmorpg_core::physics::to_units(q.position)
                })
                .then_some(left.tick)
        });
    (worst, differing, first)
}

/// Query decisions on one state, by probe: the same probes on both backends must agree.
#[derive(Debug, Default, PartialEq)]
struct Decisions {
    /// Every unit's grounded state, over the lake basin's open floor (#274) too.
    grounded: BTreeMap<String, bool>,
    sight: BTreeMap<String, bool>,
    npc_sight: BTreeMap<String, bool>,
    doors: BTreeMap<String, bool>,
    ground: BTreeMap<String, i32>,
    shots: BTreeMap<String, Option<(Option<EntityRef>, BodyId, f32)>>,
    /// Whether the player's body fits along a charge at the creature (#57, shape cast).
    charges: BTreeMap<String, bool>,
    /// Where the creature would steer to reach the player (#307, navigation).
    routes: BTreeMap<String, Option<[i32; 2]>>,
}

fn decisions(zone: &mut ZoneSimulation) -> Result<Decisions> {
    let mut decisions = Decisions::default();
    let units = zone.physics_units();
    for unit in &units {
        let position = unit.body.position();
        let (x, z) = (position.x, position.z);
        decisions
            .grounded
            .insert(format!("{:?}", unit.unit), unit.grounded);
        decisions
            .ground
            .insert(format!("{:?}", unit.unit), zone.ground_probe(x, z));
    }
    let near = |a: &UnitBody, b: &UnitBody| {
        let (p, q) = (a.body.exact_position(), b.body.exact_position());
        (p[0] - q[0]).hypot(p[2] - q[2]) <= PROBE_RANGE_UNITS
    };
    for from in units
        .iter()
        .filter(|unit| matches!(unit.unit, EntityRef::Player(_)))
    {
        for to in units
            .iter()
            .filter(|unit| matches!(unit.unit, EntityRef::Creature(_)))
        {
            if !near(from, to) {
                continue;
            }
            let pair = format!("{:?}->{:?}", from.unit, to.unit);
            decisions
                .sight
                .insert(pair.clone(), zone.line_of_sight_probe(from.unit, to.unit)?);
            let shot = zone
                .shot_probe(from.unit, to.unit)?
                .map(|probe| (probe.unit, probe.hit.body, probe.hit.fraction));
            decisions
                .charges
                .insert(pair.clone(), zone.charge_path_probe(from.unit, to.unit)?);
            decisions
                .routes
                .insert(pair.clone(), zone.route_probe(to.unit, from.unit)?);
            decisions.shots.insert(pair, shot);
        }
        let EntityRef::Player(player) = from.unit else {
            continue;
        };
        for npc in zone.content().npcs() {
            let [x, z] = npc.position;
            let p = from.body.exact_position();
            #[allow(clippy::cast_precision_loss)]
            if (p[0] - x as f32).hypot(p[2] - z as f32) <= PROBE_RANGE_UNITS {
                decisions.npc_sight.insert(
                    format!("{player}->{:?}", npc.id),
                    zone.npc_sight_probe(player, npc.id)?,
                );
            }
        }
    }
    for door in zone.content().doors() {
        for open in [false, true] {
            decisions.doors.insert(
                format!("{:?} open={open}", door.id),
                zone.door_blocked_probe(door.id, open)?,
            );
        }
    }
    Ok(decisions)
}

/// Differences between the backends' decisions on one state, shots compared within
/// [`SHOT_FRACTION_TOLERANCE`].
fn decision_differences(rapier: &Decisions, engine: &Decisions) -> Vec<String> {
    fn exact<T: PartialEq + std::fmt::Debug>(
        what: &str,
        a: &BTreeMap<String, T>,
        b: &BTreeMap<String, T>,
        out: &mut Vec<String>,
    ) {
        for (key, left) in a {
            let right = b.get(key);
            if right != Some(left) {
                out.push(format!("{what} {key}: rapier {left:?}, engine {right:?}"));
            }
        }
    }
    let mut out = Vec::new();
    exact("grounded", &rapier.grounded, &engine.grounded, &mut out);
    exact("sight", &rapier.sight, &engine.sight, &mut out);
    exact("npc sight", &rapier.npc_sight, &engine.npc_sight, &mut out);
    exact("door blocked", &rapier.doors, &engine.doors, &mut out);
    exact("ground", &rapier.ground, &engine.ground, &mut out);
    exact("charge path", &rapier.charges, &engine.charges, &mut out);
    exact("route", &rapier.routes, &engine.routes, &mut out);
    for (key, left) in &rapier.shots {
        let right = engine.shots.get(key).copied().flatten();
        let same = match (left, right) {
            (None, None) => true,
            (Some((unit, body, fraction)), Some((other_unit, other_body, other_fraction))) => {
                *unit == other_unit
                    && *body == other_body
                    && (fraction - other_fraction).abs() <= SHOT_FRACTION_TOLERANCE
            }
            _ => false,
        };
        if !same {
            out.push(format!("shot {key}: rapier {left:?}, engine {right:?}"));
        }
    }
    out
}

/// Rapier's state at sampled ticks, recovered onto each backend, must yield the same
/// decisions. Returns the number of probes compared.
fn query_parity(workload: &Workload, rapier: &Trace) -> Result<usize> {
    let content = mmorpg_core::greyhaven_vale::content();
    let mut probes = 0;
    let last = rapier.snapshots.len() - 1;
    for (index, snapshot) in rapier.snapshots.iter().enumerate() {
        if index % PARITY_EVERY != 0 && index != last {
            continue;
        }
        let on = |backend| {
            using_backend(backend, || {
                ZoneSimulation::from_snapshot(snapshot.clone(), std::sync::Arc::clone(&content))
            })
        };
        let (mut left, mut right) = (on(Backend::Rapier)?, on(Backend::Engine)?);
        assert_eq!(right.physics().backend(), Backend::Engine);
        let (a, b) = (decisions(&mut left)?, decisions(&mut right)?);
        probes += a.grounded.len()
            + a.sight.len()
            + a.npc_sight.len()
            + a.doors.len()
            + a.ground.len()
            + a.charges.len()
            + a.routes.len()
            + a.shots.len();
        let differences = decision_differences(&a, &b);
        assert!(
            differences.is_empty(),
            "{} tick {}: decisions differ on identical state:\n{}\n{:?}",
            workload.name(),
            snapshot.tick,
            differences.join("\n"),
            snapshot
                .players
                .iter()
                .map(|p| (p.player_id, p.position, p.velocity))
                .collect::<Vec<_>>()
        );
    }
    Ok(probes)
}

/// Every check of one workload on both backends: one trial per workload (#540).
fn check_workload(workload: Workload) -> Result<()> {
    let started = std::time::Instant::now();
    let name = workload.name().to_owned();
    let (rapier, rapier_check) = run(&workload, Backend::Rapier)?;
    let (engine, engine_check) = run(&workload, Backend::Engine)?;
    // Restored halfway, both backends continue exactly (Atlas 2.1.5).
    assert_eq!(
        rapier_check.departed, None,
        "{name}: Rapier recovery departs"
    );
    assert_eq!(
        engine_check.departed, None,
        "{name}: engine recovery departs"
    );
    // Same-backend replay is bit-identical.
    assert!(
        run(&workload, Backend::Rapier)?.0 == rapier,
        "{name}: Rapier replay differs"
    );
    assert!(
        run(&workload, Backend::Engine)?.0 == engine,
        "{name}: engine replay differs"
    );

    let (horizontal, vertical, first) = player_divergence(&rapier, &engine);
    let (creatures, creatures_differing, creatures_first) = creature_divergence(&rapier, &engine);
    let doors_differ = rapier
        .snapshots
        .iter()
        .zip(&engine.snapshots)
        .any(|(a, b)| a.doors != b.doors);
    let outcomes_differ = rapier.outcomes != engine.outcomes;
    // Units that walk into a settled crate stop where it rests (`vale-crate-shove`).
    let mut crate_gap = 0.0_f32;
    let probes = query_parity(&workload, &rapier)?;
    println!(
        "{name}: ticks={} players max Δxz={horizontal:.4} Δy={vertical:.4} first rounded \
         difference at {first:?}; creatures max final Δxz={creatures:.3} ({creatures_differing} \
         differ, first rounded difference at {creatures_first:?}); doors differ={doors_differ}; command outcomes differ={outcomes_differ}; \
         closest box gap rapier={:.4} engine={:.4}; {probes} query probes agree [{:?}]",
        rapier.snapshots.len(),
        rapier_check.closest,
        engine_check.closest,
        started.elapsed(),
    );
    if matches!(workload, Workload::Traversal) {
        // On each backend the Warden, keys released at 30 Hz tick 22, still flies south on
        // its momentum two ticks later (#413).
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            let warden = trace.units[2 * 24]
                .iter()
                .find(|unit| unit.unit == EntityRef::Player(1))
                .ok_or("missing traversal Warden")?;
            let velocity = warden.body.velocity();
            println!("  {name} on {backend}: Warden velocity {velocity:?}");
            assert!(
                !warden.grounded && velocity.z < 0,
                "{name} on {backend}: no momentum {warden:?}"
            );
        }
    }
    if matches!(workload, Workload::Climb) {
        // On each backend the climber ends on the crag's top and the mantler on the dock's
        // deck (#490).
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            check_wall_climb(trace).map_err(|failure| format!("{name} on {backend}: {failure}"))?;
        }
    }
    if matches!(workload, Workload::Roll) {
        // On each backend every player rolls, the crag stops one without a climb and the
        // dock roller lands on the ground beyond the deck (#516).
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            check_rolls(trace).map_err(|failure| format!("{name} on {backend}: {failure}"))?;
        }
    }
    if matches!(workload, Workload::MillHoist) {
        // On each backend the hoist carries its rider to the gallery (#56).
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            check_mill_hoist(trace).map_err(|failure| format!("{name} on {backend}: {failure}"))?;
        }
    }
    if matches!(workload, Workload::WatchPost) {
        // On each backend the fight breaks pieces of the post (#305).
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            let last = trace.snapshots.last().ok_or("empty watch post trace")?;
            let content = mmorpg_core::greyhaven_vale::content();
            let damaged: Vec<_> = last
                .pieces
                .iter()
                .zip(content.pieces())
                .filter(|(state, piece)| state.health < piece.health)
                .map(|(state, _)| (state.piece.get(), state.health))
                .collect();
            println!("  {name} on {backend}: pieces {damaged:?}");
            assert!(!damaged.is_empty(), "{name} on {backend}: no piece damaged");
        }
    }
    if matches!(workload, Workload::WatchPost) {
        // The same blasts and arrows break the same pieces on both backends.
        let pieces = |trace: &Trace| trace.snapshots.last().map(|last| last.pieces.clone());
        assert_eq!(
            pieces(&rapier),
            pieces(&engine),
            "{name}: piece states differ"
        );
    }
    if matches!(workload, Workload::ChargePath) {
        // On each backend some Warden charges (#57).
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            let charging: std::collections::BTreeSet<_> = trace
                .snapshots
                .iter()
                .flat_map(|snapshot| &snapshot.forced)
                .filter(|(unit, state)| {
                    matches!(unit, EntityRef::Player(_))
                        && state.moving.is_some_and(|moving| moving.charge.is_some())
                })
                .map(|(unit, _)| *unit)
                .collect();
            println!("  {name} on {backend}: charged {charging:?}");
            assert!(!charging.is_empty(), "{name} on {backend}: nobody charged");
        }
    }
    if matches!(workload, Workload::Missiles) {
        // On each backend the homing Firebolts fly and strike wolves or trunks in the way, and
        // the free ones fly straight (#289); both arms spend the same missiles the same way.
        let outcomes = |trace: &Trace| -> std::collections::BTreeMap<u32, (bool, [f32; 3])> {
            trace
                .snapshots
                .iter()
                .flat_map(|snapshot| &snapshot.projectiles)
                .filter_map(|projectile| match projectile.flight {
                    mmorpg_core::Flight::Stuck { unit, .. }
                        if projectile.kind == mmorpg_core::ProjectileKind::Missile =>
                    {
                        Some((projectile.id, (unit.is_some(), projectile.position)))
                    }
                    _ => None,
                })
                .collect()
        };
        let (a, b) = (outcomes(&rapier), outcomes(&engine));
        let struck = |outcome: &std::collections::BTreeMap<u32, (bool, [f32; 3])>| {
            outcome
                .iter()
                .filter(|(_, (unit, _))| *unit)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };
        println!(
            "  {name}: missiles struck units {:?} / {:?}, spent {} / {}",
            struck(&a),
            struck(&b),
            a.len(),
            b.len()
        );
        assert!(
            !struck(&a).is_empty(),
            "{name} on rapier: no missile struck"
        );
        assert!(
            !struck(&b).is_empty(),
            "{name} on engine: no missile struck"
        );
        let same_ends =
            a.keys().eq(b.keys()) && a.values().zip(b.values()).all(|(x, y)| x.0 == y.0);
        assert!(same_ends, "{name}: missiles end differently: {a:?} / {b:?}");
        // Missiles spent in the world end within the creatures' drift of each other.
        let gap = a
            .values()
            .zip(b.values())
            .filter(|(x, _)| !x.0)
            .map(|(x, y)| (x.1[0] - y.1[0]).hypot(x.1[2] - y.1[2]))
            .fold(0.0_f32, f32::max);
        println!("  {name}: missiles spent in the world part by at most {gap:.3}");
    }
    if matches!(workload, Workload::CrateShove) {
        // On each backend both bashes slide their crates over a metre and they come to rest
        // (#58), where they agree (both average the pair's materials, physics-engine #391).
        let crates = |trace: &Trace| -> Vec<([f32; 3], [f32; 3])> {
            let last = trace.snapshots.last().expect("crate trace");
            [917, 918]
                .map(|id| {
                    let record = last
                        .creatures
                        .iter()
                        .find(|record| record.creature_id.get() == id)
                        .expect("heavy crate");
                    (record.position, record.velocity)
                })
                .to_vec()
        };
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            let ends = crates(trace);
            println!("  {name} on {backend}: crates end at {ends:?}");
            for ((position, velocity), start) in ends.iter().zip([7_400.0, 7_550.0]) {
                assert_eq!(
                    *velocity, [0.0; 3],
                    "{name} on {backend}: a crate still slides"
                );
                assert!(
                    position[2] > 3_300.0 && (position[0] - start).abs() < 20.0,
                    "{name} on {backend}: a crate did not slide north: {position:?}"
                );
            }
        }
        let gap = crates(&rapier)
            .iter()
            .zip(crates(&engine))
            .map(|((a, _), (b, _))| (a[0] - b[0]).hypot(a[2] - b[2]))
            .fold(0.0_f32, f32::max);
        // physics-engine's `props` oracle bound: within 5% of each slide (about 265 units).
        assert!(gap <= 13.0, "{name}: crates part by {gap}");
        crate_gap = gap;
    }
    if matches!(workload, Workload::CrateSmash) {
        check_crate_smash(&name, &rapier, &engine);
    }
    if matches!(workload, Workload::MillRamps) {
        // Both hold the chute's walker at its foot (physics-engine #351: too-steep slopes are
        // walls); they part only where Rapier's controller tops the dock's edge 2.5 units
        // high for a tick before settling.
        assert!(
            horizontal <= 2.0 && vertical <= 3.0,
            "{name}: players diverge by Δxz={horizontal} Δy={vertical}"
        );
    }
    if matches!(workload, Workload::MillRamps) {
        for (trace, backend) in [(&rapier, "rapier"), (&engine, "engine")] {
            check_mill_ramps(trace).map_err(|failure| format!("{name} on {backend}: {failure}"))?;
        }
    }
    if matches!(workload, Workload::OgreBrawl) {
        // On each backend the ogre's body blocks every player, it shoves the ones in its
        // way and its stomps land (#297).
        for (check, backend) in [(&rapier_check, "rapier"), (&engine_check, "engine")] {
            println!(
                "  {name} on {backend}: ogre–player gap ≥ {:.4}, {} shoved player-ticks, stomped={}",
                check.ogre_gap, check.shoved_ticks, check.stomped
            );
            assert!(
                check.ogre_gap >= -PENETRATION_TOLERANCE_UNITS,
                "{name} on {backend}: a player inside the ogre by {}",
                -check.ogre_gap
            );
            assert!(check.shoved_ticks > 0, "{name} on {backend}: no shove");
            assert!(check.stomped, "{name} on {backend}: no stomp landed");
        }
    }
    if let Workload::Bots(scenario) = &workload {
        // The replay drives the zone as the bot runner does: Rapier reproduces the
        // scenario's expected positions exactly.
        for expectation in &scenario.positions {
            let tick = usize::try_from(expectation.tick)? - 1;
            let at = |trace: &Trace| {
                let player = u32::try_from(expectation.bot)? + 1;
                trace.snapshots[tick]
                    .players
                    .iter()
                    .find(|state| state.player_id == player)
                    .map(|state| mmorpg_core::physics::to_units(state.position))
                    .ok_or_else(|| Box::<dyn Error>::from("missing player"))
            };
            assert_eq!(
                at(&rapier)?,
                expectation.position,
                "{name} Rapier expectation"
            );
            let engine_at = at(&engine)?;
            if engine_at != expectation.position {
                println!(
                    "  {name} tick {}: engine at {engine_at:?}, scenario expects {:?}",
                    expectation.tick, expectation.position
                );
            }
        }
    }
    // Scripted walks over slopes agree within the owner's climb allowance until their fight.
    if let Some(&(_, until)) = CLIMBS.iter().find(|(climb, _)| *climb == name) {
        let [worst, allowance] = check_climb(&rapier, &engine, until)
            .map_err(|error| format!("{name}: players diverge on the climb: {error}"))?;
        println!(
            "  {name}: players part by at most {worst:.2} units (allowed {allowance:.2}) to \
             tick {until}"
        );
    }
    // Workloads whose players are only scripted (no fixture placement inside geometry)
    // must agree: positions within the tolerance, the same command outcomes and doors.
    if SCRIPTED.contains(&name.as_str()) {
        assert!(
            horizontal <= SCRIPTED_POSITION_TOLERANCE_UNITS + crate_gap
                && vertical <= SCRIPTED_POSITION_TOLERANCE_UNITS,
            "{name}: players diverge by Δxz={horizontal} Δy={vertical}"
        );
        assert!(!outcomes_differ, "{name}: command outcomes differ");
        // Both report ground within the contact offset plus 5 cm: the scripted players'
        // grounded state agrees on every tick.
        for (tick, (a, b)) in rapier.units.iter().zip(&engine.units).enumerate() {
            let players = |units: &[UnitBody]| -> Vec<_> {
                units
                    .iter()
                    .filter(|unit| matches!(unit.unit, EntityRef::Player(_)))
                    .map(|unit| (unit.unit, unit.grounded))
                    .collect()
            };
            assert_eq!(
                players(a),
                players(b),
                "{name}: grounded differs at tick {}",
                tick + 1
            );
        }
        assert!(!doors_differ, "{name}: door states differ");
    }
    Ok(())
}

fn main() {
    let arguments = libtest_mimic::Arguments::from_args();
    let trials = drivers::workloads()
        .expect("the A/B workloads load")
        .into_iter()
        .map(|workload| {
            let name = workload.name().to_owned();
            libtest_mimic::Trial::test(name, move || {
                check_workload(workload).map_err(|error| error.to_string().into())
            })
        })
        .collect();
    libtest_mimic::run(&arguments, trials).exit();
}
