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
//! sampled ticks, recovered onto both backends. Creatures (AI and a shared RNG react to every
//! position) and the fighting fixtures are compared but not bounded; their divergences are
//! printed (`--nocapture`) and listed in `docs/PHYSICS_AB.md`.

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
/// The workloads whose players are only scripted: the bot scenarios and the idle plaza. The
/// fighting fixtures place players inside props, boxes and hills, which Rapier leaves embedded
/// and physics-engine pushes out, so their fights (and every position after) legitimately part.
const SCRIPTED: [&str; 4] = [
    "two-bots-move",
    "vale-hub-walk",
    "vale-wolf-hunt",
    "vale-idle-16",
];
/// Shot contact fractions on identical states: both backends solve the same ray against the
/// same boxes, capsules and triangles in `f32`.
const SHOT_FRACTION_TOLERANCE: f32 = 1.0e-4;
/// A unit at rest this many ticks must be grounded.
const REST_TICKS: usize = 3;
/// Penetration is checked on every this-many-th tick (and the last), over every content box.
const PENETRATION_EVERY: usize = 10;
/// Query parity is checked on Rapier's state every this-many-th tick (and the last).
const PARITY_EVERY: usize = 60;
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
    /// The smallest capsule-to-box gap (units), authored ground overlap excluded.
    closest: f32,
    /// The first tick the zone restored halfway differs from the uninterrupted one.
    departed: Option<u64>,
    /// Units the workload placed overlapping a box or the terrain at the first tick.
    placed_inside: std::collections::BTreeSet<EntityRef>,
}

#[derive(Debug, Default, PartialEq)]
struct Trace {
    snapshots: Vec<CanonicalZoneSnapshot>,
    units: Vec<Vec<UnitBody>>,
    outcomes: Vec<String>,
}

/// An upright capsule's distance outside an axis-aligned box (negative inside).
fn capsule_box_gap(
    centre: [f32; 3],
    half: [f32; 3],
    box_centre: [f32; 3],
    box_half: [f32; 3],
) -> f32 {
    let radius = half[0].max(half[2]);
    let segment = (half[1] - radius).max(0.0);
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

/// Whether `unit`'s capsule overlaps a content box, door leaf, prop or the terrain beyond the
/// tolerances (the ground's authored overlap excepted).
fn embedded(zone: &ZoneSimulation, unit: &UnitBody, boxes: &[([f32; 3], [f32; 3])]) -> bool {
    let [x, y, z] = unit.body.exact_position();
    let half = unit.body.half_extents();
    #[allow(clippy::cast_precision_loss)]
    let half = [half.x, half.y, half.z].map(|v| v as f32);
    let radius = half[0].max(half[2]);
    let authored = (radius - half[1]).max(0.0);
    #[allow(clippy::cast_possible_truncation)]
    let ground = zone
        .physics()
        .terrain_height(x.round() as i32, z.round() as i32)
        .unwrap_or(0.0);
    if y + CONTACT_OFFSET_UNITS - half[1].max(radius) < ground - authored - 2.0 {
        return true;
    }
    boxes.iter().any(|&(box_centre, box_half)| {
        let ground = box_centre[1] + box_half[1] <= 0.0;
        capsule_box_gap([x, y + CONTACT_OFFSET_UNITS, z], half, box_centre, box_half)
            < -(PENETRATION_TOLERANCE_UNITS + if ground { authored } else { 0.0 })
    })
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
            let half = unit.body.half_extents();
            #[allow(clippy::cast_precision_loss)]
            let half = [half.x, half.y, half.z].map(|v| v as f32);
            let radius = half[0].max(half[2]);
            // Squat creatures' capsules (radius above half height) are authored overlapping
            // the ground by the difference: Rapier keeps that overlap, so it is allowed
            // against the ground only, never against walls.
            let authored = (radius - half[1]).max(0.0);
            let centre = [x, y + CONTACT_OFFSET_UNITS, z];
            let mut deepest = None;
            let mut closest = f32::INFINITY;
            for &(box_centre, box_half) in &boxes {
                let gap = capsule_box_gap(centre, half, box_centre, box_half);
                let ground = box_centre[1] + box_half[1] <= 0.0;
                let allowed = PENETRATION_TOLERANCE_UNITS + if ground { authored } else { 0.0 };
                if gap < -allowed && deepest.is_none_or(|(depth, _)| -gap > depth) {
                    deepest = Some((-gap, (box_centre, box_half)));
                }
                closest = closest.min(gap + if ground { authored } else { 0.0 });
            }
            if let Some((depth, (box_centre, box_half))) = deepest {
                // The Greyhaven fixture places players by spawn table, some inside a box or
                // hill: no backend owes them a way out, so they are exempt from then on.
                if tick == 1 {
                    check.placed_inside.insert(unit.unit);
                } else if !check.placed_inside.contains(&unit.unit) {
                    return Err(format!(
                        "{:?} penetrates box {box_centre:?}±{box_half:?} by {depth} at tick {tick}",
                        unit.unit
                    ));
                }
            }
            if !check.placed_inside.contains(&unit.unit) {
                check.closest = check.closest.min(closest);
            }
            #[allow(clippy::cast_possible_truncation)]
            let ground = zone
                .physics()
                .terrain_height(x.round() as i32, z.round() as i32)
                .unwrap_or(0.0)
                .max(0.0);
            let bottom = centre[1] - half[1].max(radius);
            // The terrain sample is the ray height at the rounded column, so allow for its
            // slope over half a unit. The Greyhaven fixture puts every player's feet at y = 0,
            // some inside a hill.
            let sunk = bottom < ground - authored - 2.0;
            if tick == 1 && sunk {
                check.placed_inside.insert(unit.unit);
            }
            if sunk && !check.placed_inside.contains(&unit.unit) {
                return Err(format!(
                    "{:?} sinks {} below the ground at tick {tick}",
                    unit.unit,
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
            if rested && unit.body.exact_velocity() == [0.0; 3] && !unit.grounded {
                return Err(format!("{:?} rests ungrounded at tick {tick}", unit.unit));
            }
        }
    }
    Ok(())
}

/// The gap between two upright capsules given as unit bodies (their game centres).
fn capsule_gap(a: &UnitBody, b: &UnitBody) -> f32 {
    let shape = |unit: &UnitBody| {
        let half = unit.body.half_extents();
        #[allow(clippy::cast_precision_loss)]
        let (radius, half_height) = (half.x.max(half.z) as f32, half.y as f32);
        (radius, (half_height - radius).max(0.0))
    };
    let ((ra, sa), (rb, sb)) = (shape(a), shape(b));
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

/// The hoist ride on Rapier (#56): the rider ends on the windmill's gallery, 8 m up.
fn check_mill_hoist(trace: &Trace) -> Result<()> {
    let last = trace.snapshots.last().ok_or("empty hoist trace")?;
    let [_, y, z] = last.players[0].position;
    if (y - 890.0).abs() > 0.1 || z < 1_900.0 {
        return Err(format!("the hoist rider ends at {:?}", last.players[0].position).into());
    }
    Ok(())
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
            placed_inside: std::collections::BTreeSet::new(),
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
    /// Units embedded in a box or hill (placed there by a fixture): grounded is not compared for them.
    embedded: usize,
    grounded: BTreeMap<String, bool>,
    sight: BTreeMap<String, bool>,
    npc_sight: BTreeMap<String, bool>,
    doors: BTreeMap<String, bool>,
    ground: BTreeMap<String, i32>,
    shots: BTreeMap<String, Option<(Option<EntityRef>, BodyId, f32)>>,
}

fn decisions(zone: &ZoneSimulation) -> Result<Decisions> {
    let mut decisions = Decisions::default();
    let units = zone.physics_units();
    let boxes = boxes(zone);
    for unit in &units {
        let position = unit.body.position();
        let (x, z) = (position.x, position.z);
        // Inside a box or hill, "ground" is whichever face the penetration normal picks.
        if embedded(zone, unit, &boxes) {
            decisions.embedded += 1;
        } else {
            decisions
                .grounded
                .insert(format!("{:?}", unit.unit), unit.grounded);
        }
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
fn query_parity(workload: &Workload, rapier: &Trace) -> Result<(usize, usize)> {
    let content = mmorpg_core::greyhaven_vale::content();
    let (mut probes, mut embedded) = (0, 0);
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
        let (left, right) = (on(Backend::Rapier)?, on(Backend::Engine)?);
        assert_eq!(right.physics().backend(), Backend::Engine);
        let (a, b) = (decisions(&left)?, decisions(&right)?);
        embedded = embedded.max(a.embedded);
        probes += a.grounded.len()
            + a.sight.len()
            + a.npc_sight.len()
            + a.doors.len()
            + a.ground.len()
            + a.shots.len();
        let differences = decision_differences(&a, &b);
        assert!(
            differences.is_empty(),
            "{} tick {}: decisions differ on identical state:\n{}",
            workload.name(),
            snapshot.tick,
            differences.join("\n")
        );
    }
    Ok((probes, embedded))
}

#[test]
fn backends_keep_shared_invariants_and_agree_on_the_zone_workloads() -> Result<()> {
    for workload in drivers::workloads()? {
        let started = std::time::Instant::now();
        let name = workload.name().to_owned();
        let (rapier, rapier_check) = run(&workload, Backend::Rapier)?;
        if let Some(missing) = workload.engine_unsupported() {
            // Disclosed, never substituted: physics-engine refuses the workload with an
            // unsupported error. Rapier alone keeps the shared invariants, replays and
            // continues exactly when restored halfway.
            let refusal = run(&workload, Backend::Engine)
                .err()
                .ok_or_else(|| format!("{name}: physics-engine ran an unsupported workload"))?
                .to_string();
            assert!(refusal.contains("unsupported"), "{name}: {refusal}");
            assert_eq!(
                rapier_check.departed, None,
                "{name}: Rapier recovery departs"
            );
            assert!(
                run(&workload, Backend::Rapier)?.0 == rapier,
                "{name}: Rapier replay differs"
            );
            check_mill_hoist(&rapier)?;
            println!(
                "{name}: physics-engine unsupported ({missing}): {refusal}; Rapier keeps the \
                 invariants, replays and continues [{:?}]",
                started.elapsed()
            );
            continue;
        }
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
        let (creatures, creatures_differing, creatures_first) =
            creature_divergence(&rapier, &engine);
        let doors_differ = rapier
            .snapshots
            .iter()
            .zip(&engine.snapshots)
            .any(|(a, b)| a.doors != b.doors);
        let outcomes_differ = rapier.outcomes != engine.outcomes;
        let (probes, embedded) = query_parity(&workload, &rapier)?;
        println!(
            "{name}: ticks={} players max Δxz={horizontal:.4} Δy={vertical:.4} first rounded \
             difference at {first:?}; creatures max final Δxz={creatures:.3} ({creatures_differing} \
             differ, first rounded difference at {creatures_first:?}); doors differ={doors_differ}; command outcomes differ={outcomes_differ}; \
             closest box gap rapier={:.4} engine={:.4}; placed overlapping {}; {probes} \
             query probes agree ({embedded} embedded units' grounded not compared) [{:?}]",
            rapier.snapshots.len(),
            rapier_check.closest,
            engine_check.closest,
            rapier_check.placed_inside.len(),
            started.elapsed(),
        );
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
        // Workloads whose players are only scripted (no fixture placement inside geometry)
        // must agree: positions within the tolerance, the same command outcomes and doors.
        if SCRIPTED.contains(&name.as_str()) {
            assert!(
                horizontal <= SCRIPTED_POSITION_TOLERANCE_UNITS
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
    }
    Ok(())
}
