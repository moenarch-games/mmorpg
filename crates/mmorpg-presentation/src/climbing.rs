//! Greyhaven's climbing routes from asset-tooling's `climbing-kit` bundle (#621).
//!
//! - **Ladders.** Each ladder collider ([`PropKind::Ladder`]) is a stack of 1.2 m
//!   `ladder-section`s finished by a `ladder-top` whose `topOut.parapetTop` (1 m up the cap)
//!   meets the tower's top; the lowest section sinks into the ground to absorb the remainder, so
//!   the rungs keep their 0.30 m pitch from the top down ([`Rungs`]).
//! - **Ropes.** Each rope collider ([`PropKind::Rope`], `Grip::Rope`) is a `rope-end`, 1.2 m
//!   `rope-section`s and a `rope-anchor` whose beam sits just under the face's top, the rope's
//!   axis 0.30 m in front of the face (the collider's front). The end hangs clear of the ground.
//! - **Ivy.** A few climbable faces ([`COVERS`]: palisade, cliff and crag faces) and the
//!   boundary climbing routes on the terrain wear climbing ivy, a `vine-3m` cover on a low face
//!   or `vine-strip`s capped by a `vine-top`, so a climbable route reads from about 20 m. The ivy
//!   is a cover: the face beneath keeps its own grip, and sheer faces stay bare.
//!
//! The shared [`Lods`] picks each part's level from the descriptor's `lodSelection`. While a
//! client does not draw the kit, the static scene draws the procedural ladders and ropes
//! ([`crate::Kit::Climbing`] fallback groups) and no ivy. Presentation only.
use mmorpg_core::{
    UNITS_PER_METRE,
    greyhaven_vale::{self, ids},
};
use mmorpg_scenery::{Prop, PropKind, Scenery, TerrainGrid};

use crate::{
    Cull,
    batching::matrix,
    geometry::{Affine, V, compose, ry, translate},
    terrain::surface,
};

pub use crate::lod::{EntryLods, Lods, Placement, Shown};

pub const LADDER_SECTION: &str = "climbing.kit.ladder-section";
pub const LADDER_TOP: &str = "climbing.kit.ladder-top";
pub const ROPE_ANCHOR: &str = "climbing.kit.rope-anchor";
pub const ROPE_END: &str = "climbing.kit.rope-end";
pub const ROPE_SECTION: &str = "climbing.kit.rope-section";
pub const VINE_3M: &str = "climbing.kit.vine-3m";
pub const VINE_STRIP: &str = "climbing.kit.vine-strip";
pub const VINE_TOP: &str = "climbing.kit.vine-top";
/// The kit entries Greyhaven places, sorted.
pub const ENTRIES: [&str; 8] = [
    LADDER_SECTION,
    LADDER_TOP,
    ROPE_ANCHOR,
    ROPE_END,
    ROPE_SECTION,
    VINE_3M,
    VINE_STRIP,
    VINE_TOP,
];

/// The rungs keep this pitch (`rungSpacing`).
pub const RUNG_SPACING: f64 = 0.3;
/// A ladder section tiles every 1.2 m, its first rung 0.15 m up; the cap's parapet top is 1 m up.
const SECTION: f64 = 1.2;
const FIRST_RUNG: f64 = 0.15;
const PARAPET_TOP: f64 = 1.0;
/// A rope section tiles every 1.2 m, the end reaches 0.6 m up to its joint and the anchor's
/// beam tops out 0.425 m above its joint, which sits this far under the face's top.
const ROPE_TILE: f64 = 1.2;
const ROPE_END_HEIGHT: f64 = 0.6;
const ANCHOR_HEIGHT: f64 = 0.425;
const ANCHOR_BELOW_TOP: f64 = 0.15;
/// Ivy strips tile every 2 m, 1 m wide; the ragged top is 1 m tall and ends this far under the
/// face's top; a face at most this tall wears one rooted `vine-3m` stretched to fit.
const VINE_PITCH: f64 = 2.;
const VINE_TOP_HEIGHT: f64 = 1.;
const VINE_BELOW_TOP: f64 = 0.1;
const LOW_FACE: f64 = 4.;
/// Terrain steeper than a metre per metre (45°) is a climbing face, as for the handholds.
const STEEP: f64 = 1.;

/// One ivy cover over a collider's face: `(collider, outward normal on XZ, the strips' centres
/// along the face in metres from the face's centre)`.
pub type Cover = (u32, [i32; 2], &'static [f64]);

/// The covered collider faces (#621), each a route two or three strips wide: the Outpost
/// palisade's north wall from inside and its east wall from outside, the Redbrand Hollow's
/// western cliff, and three climbable highland crags by the gorge, the trail and the lookout.
pub const COVERS: [Cover; 6] = [
    (*ids::PALISADE.start(), [0, 1], &[8.0, 9.0]),
    (*ids::PALISADE.start() + 6, [1, 0], &[-4.0, -3.0]),
    (*ids::CLIFFS.start(), [1, 0], &[4.0, 5.0, 6.0]),
    (*ids::HIGHLAND_CRAGS.start(), [0, 1], &[-0.5, 0.5]),
    (*ids::HIGHLAND_CRAGS.start() + 2, [0, 1], &[-0.5, 0.5]),
    (*ids::HIGHLAND_CRAGS.start() + 7, [-1, 0], &[-0.5, 0.5]),
];
/// Each boundary climbing route on the terrain wears three strips up its middle.
const ROUTE_STRIPS: [f64; 3] = [-1., 0., 1.];

fn metres(units: i32) -> f64 {
    f64::from(units) / f64::from(UNITS_PER_METRE)
}

/// The turn about +Y (radians) that faces a ladder's or rope's local +Z out of the face it
/// climbs: every ladder stands on a tower's +Z face, a rope on its content's face
/// ([`greyhaven_vale::Rope::out`]).
#[must_use]
pub fn face_yaw(p: &Prop) -> f64 {
    let out = p
        .collider
        .and_then(|id| id.checked_sub(*ids::ROPES.start()))
        .filter(|_| p.kind == PropKind::Rope)
        .and_then(|index| greyhaven_vale::ROPES.get(usize::try_from(index).ok()?))
        .map_or([0, 1], |rope| rope.out);
    f64::from(out[0]).atan2(f64::from(out[1]))
}

/// Whether the kit draws `p` (the procedural model is its fallback): the ladders and ropes.
#[must_use]
pub const fn draws(p: &Prop) -> bool {
    matches!(p.kind, PropKind::Ladder | PropKind::Rope)
}

/// Part placed with its back-base-centre at `base` on a face, its +Z out along `out` and its
/// +Y up along `up` (unit vectors), stretched by `height` along +Y.
fn on_face(base: V, up: V, out: V, height: f64) -> Affine {
    let side = cross(up, out);
    Affine {
        m: std::array::from_fn(|i| {
            let (r, c) = (i / 3, i % 3);
            [side, up, out][c][r] * if c == 1 { height } else { 1. }
        }),
        t: base,
    }
}

fn placement(entry: &'static str, frame: Affine, largest: f64) -> Placement {
    Placement {
        entry,
        model: matrix(frame),
        scale: largest,
        cull: Cull::Far,
    }
}

/// The face frame of a ladder or rope prop: its back-base-centre on the face, at height `y`.
fn prop_face(p: &Prop, y: f64) -> Affine {
    compose(&[
        translate(metres(p.position[0]), y, metres(p.position[2])),
        ry(face_yaw(p)),
        translate(0., 0., -metres(p.half_extents[2])),
    ])
}

fn top(p: &Prop) -> f64 {
    metres(p.position[1] + 2 * p.half_extents[1])
}

/// The world height of the cap's base: its parapet top meets the ladder's top.
fn cap_base(p: &Prop) -> f64 {
    top(p) - PARAPET_TOP
}

fn ladder(p: &Prop, out: &mut Vec<Placement>) {
    let cap = cap_base(p);
    out.push(placement(LADDER_TOP, prop_face(p, cap), 1.));
    let sections = ((cap - metres(p.position[1])) / SECTION).ceil().max(0.) as usize;
    for k in 1..=sections {
        out.push(placement(
            LADDER_SECTION,
            prop_face(p, cap - SECTION * k as f64),
            1.,
        ));
    }
}

fn rope(p: &Prop, g: &TerrainGrid, out: &mut Vec<Placement>) {
    let joint = top(p) - ANCHOR_BELOW_TOP - ANCHOR_HEIGHT;
    out.push(placement(ROPE_ANCHOR, prop_face(p, joint), 1.));
    // The rope's axis, where it meets the ground.
    let axis = crate::geometry::point(prop_face(p, 0.), [0., 0., 0.3]);
    let ground = surface(g, axis[0], axis[2]);
    let sections = ((joint - ROPE_END_HEIGHT - ground) / ROPE_TILE)
        .floor()
        .max(0.) as usize;
    for k in 1..=sections {
        out.push(placement(
            ROPE_SECTION,
            prop_face(p, joint - ROPE_TILE * k as f64),
            1.,
        ));
    }
    out.push(placement(
        ROPE_END,
        prop_face(p, joint - ROPE_TILE * sections as f64 - ROPE_END_HEIGHT),
        1.,
    ));
}

/// Ivy up a vertical face from `ground` to `top` at `base` (XZ on the face, metres).
fn vertical_cover(base: [f64; 2], out: V, ground: f64, top: f64, parts: &mut Vec<Placement>) {
    let up = [0., 1., 0.];
    let at = |y: f64| [base[0], y, base[1]];
    let height = top - ground;
    if height < 0.5 {
        return;
    }
    if height <= LOW_FACE {
        let stretch = height / 3.;
        parts.push(placement(
            VINE_3M,
            on_face(at(ground), up, out, stretch),
            stretch.max(1.),
        ));
        return;
    }
    let cap = top - VINE_BELOW_TOP - VINE_TOP_HEIGHT;
    parts.push(placement(VINE_TOP, on_face(at(cap), up, out, 1.), 1.));
    let strips = ((cap - ground) / VINE_PITCH).ceil().max(0.) as usize;
    for k in 1..=strips {
        parts.push(placement(
            VINE_STRIP,
            on_face(at(cap - VINE_PITCH * k as f64), up, out, 1.),
            1.,
        ));
    }
}

fn covers(s: &Scenery, g: &TerrainGrid, parts: &mut Vec<Placement>) {
    for (id, normal, strips) in COVERS {
        let Some(p) = s.props.iter().find(|p| p.collider == Some(id)) else {
            continue;
        };
        let [hx, _, hz] = p.half_extents.map(metres);
        let centre = [metres(p.position[0]), metres(p.position[2])];
        let out = [f64::from(normal[0]), 0., f64::from(normal[1])];
        let along = [-out[2], out[0]];
        let face = [centre[0] + out[0] * hx, centre[1] + out[2] * hz];
        for &offset in strips {
            let base = [face[0] + along[0] * offset, face[1] + along[1] * offset];
            let ground = surface(g, base[0] + out[0] * 0.2, base[1] + out[2] * 0.2);
            vertical_cover(base, out, ground, top(p), parts);
        }
    }
}

/// Ivy up the steep stretches of one column of the terrain, along `axis` (0 for X, 2 for Z)
/// at `across` on the other axis, from `from` to `to` (metres).
fn terrain_column(
    g: &TerrainGrid,
    axis: usize,
    across: f64,
    [from, to]: [f64; 2],
    parts: &mut Vec<Placement>,
) {
    const STEP: f64 = 0.5;
    let point = |a: f64| -> V {
        let (x, z) = if axis == 0 { (a, across) } else { (across, a) };
        [x, surface(g, x, z), z]
    };
    let samples: Vec<V> = (0..=((to - from) / STEP) as usize)
        .map(|k| point(from + STEP * k as f64))
        .collect();
    // Each steep run, ordered from its foot to its top.
    let mut run: Vec<V> = Vec::new();
    let mut runs = Vec::new();
    for pair in samples.windows(2) {
        if (pair[1][1] - pair[0][1]).abs() / STEP > STEEP {
            if run.is_empty() {
                run.push(pair[0]);
            }
            run.push(pair[1]);
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    runs.extend((!run.is_empty()).then_some(run));
    let across_axis: V = if axis == 0 {
        [0., 0., 1.]
    } else {
        [1., 0., 0.]
    };
    for mut run in runs {
        if run
            .last()
            .zip(run.first())
            .is_some_and(|(l, f)| l[1] < f[1])
        {
            run.reverse();
        }
        let lengths: Vec<f64> = std::iter::once(0.)
            .chain(run.windows(2).scan(0., |total, pair| {
                *total += distance(pair[0], pair[1]);
                Some(*total)
            }))
            .collect();
        let length = lengths.last().copied().unwrap_or(0.);
        if length < 1.5 {
            continue;
        }
        let at = |s: f64| -> V {
            let k = lengths
                .windows(2)
                .position(|w| s <= w[1])
                .unwrap_or(lengths.len().saturating_sub(2));
            let span = (lengths[k + 1] - lengths[k]).max(1e-9);
            let t = ((s - lengths[k]) / span).clamp(0., 1.);
            std::array::from_fn(|i| run[k][i] + (run[k + 1][i] - run[k][i]) * t)
        };
        // Strips from the foot, the ragged top last, all stretched alike to fill the run.
        let strips = ((length - VINE_TOP_HEIGHT) / VINE_PITCH).round().max(0.) as usize;
        let stretch = length / (VINE_PITCH * strips as f64 + VINE_TOP_HEIGHT);
        let mut s = 0.;
        for k in 0..=strips {
            let (entry, nominal) = if k == strips {
                (VINE_TOP, VINE_TOP_HEIGHT)
            } else {
                (VINE_STRIP, VINE_PITCH)
            };
            let span = nominal * stretch;
            let (p, q) = (at(s), at((s + span).min(length)));
            let chord = distance(p, q).max(1e-6);
            let up: V = std::array::from_fn(|i| (q[i] - p[i]) / chord);
            let mut out = norm(cross(across_axis, up));
            if out[1] < 0. {
                out = out.map(|v| -v);
            }
            parts.push(placement(
                entry,
                on_face(p, up, out, chord / nominal),
                (chord / nominal).max(1.),
            ));
            s += span;
        }
    }
}

fn routes(g: &TerrainGrid, parts: &mut Vec<Placement>) {
    for route in greyhaven_vale::CLIMBING_ROUTES {
        let [min, max] = [route.min.map(metres), route.max.map(metres)];
        // A route climbs along its long side.
        let axis = if max[0] - min[0] >= max[1] - min[1] {
            0
        } else {
            2
        };
        let (along, across) = if axis == 0 { (0, 1) } else { (1, 0) };
        let middle = (min[across] + max[across]) / 2.;
        for offset in ROUTE_STRIPS {
            terrain_column(g, axis, middle + offset, [min[along], max[along]], parts);
        }
    }
}

/// Every climbing kit part placed on `s`: the ladders and ropes in prop order, then the ivy.
/// Only Greyhaven Vale has them.
#[must_use]
pub fn placements(s: &Scenery) -> Vec<Placement> {
    let mut parts = Vec::new();
    if !s.props.iter().any(draws) {
        return parts;
    }
    let g = s.terrain_grid(crate::TERRAIN_STEP_UNITS);
    for p in &s.props {
        match p.kind {
            PropKind::Ladder => ladder(p, &mut parts),
            PropKind::Rope => rope(p, &g, &mut parts),
            _ => {}
        }
    }
    covers(s, &g, &mut parts);
    routes(&g, &mut parts);
    parts
}

/// A ladder's rung grid (#621), which a climber's hands hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rungs {
    /// The ladder's centre on XZ and its half width along the face (metres).
    pub centre: [f64; 2],
    pub half_width: f64,
    /// The height of every rung modulo [`RUNG_SPACING`], in `[0, RUNG_SPACING)`.
    pub phase: f64,
}

/// Every ladder's rungs on `s`: the cap's rungs at 0.15 m and 0.45 m above its base continue
/// every section's below it.
#[must_use]
pub fn rungs(s: &Scenery) -> Vec<Rungs> {
    s.props
        .iter()
        .filter(|p| p.kind == PropKind::Ladder)
        .map(|p| Rungs {
            centre: [metres(p.position[0]), metres(p.position[2])],
            half_width: metres(p.half_extents[0].max(p.half_extents[2])),
            phase: (cap_base(p) + FIRST_RUNG).rem_euclid(RUNG_SPACING),
        })
        .collect()
}

/// How far the feet at `feet` (metres) stand above the rung grid of the ladder they hold:
/// in `[0, RUNG_SPACING)`, or `None` away from every ladder.
#[must_use]
pub fn rung_offset(rungs: &[Rungs], feet: [f64; 3]) -> Option<f64> {
    rungs
        .iter()
        .map(|ladder| {
            let d = (ladder.centre[0] - feet[0]).hypot(ladder.centre[1] - feet[2]);
            (d, ladder)
        })
        .filter(|(d, ladder)| *d <= ladder.half_width + 1.)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, ladder)| (feet[1] - ladder.phase).rem_euclid(RUNG_SPACING))
}

/// Every placed part's levels from the bundle's `climbing-descriptor` attachment JSON.
///
/// # Errors
///
/// For malformed JSON or levels of detail.
pub fn descriptor_lods(json: &str) -> Result<Vec<EntryLods>, String> {
    let value =
        serde_json::from_str(json).map_err(|error| format!("climbing descriptor: {error}"))?;
    crate::lod::descriptor_lods(&value)
}

fn distance(a: V, b: V) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn norm(v: V) -> V {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-12);
    v.map(|c| c / length)
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::point;

    fn origin(placement: &Placement) -> V {
        let m = placement.model;
        [m[12], m[13], m[14]].map(f64::from)
    }

    #[test]
    fn every_ladder_is_sections_under_a_cap_that_meets_its_tower_top() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let parts = placements(&scenery);
        let ladders: Vec<_> = scenery
            .props
            .iter()
            .filter(|p| p.kind == PropKind::Ladder)
            .collect();
        assert_eq!(ladders.len(), 3);
        let caps: Vec<_> = parts.iter().filter(|p| p.entry == LADDER_TOP).collect();
        assert_eq!(caps.len(), 3);
        for (ladder, cap) in ladders.iter().zip(&caps) {
            // The cap's parapet top is the ladder's (the tower's) top, its back on the face.
            assert!((origin(cap)[1] + PARAPET_TOP - top(ladder)).abs() < 1e-4);
            let face = metres(ladder.position[2] - ladder.half_extents[2]);
            assert!((origin(cap)[2] - face).abs() < 1e-4);
            // Sections stack down to the ground with the rungs on one 0.3 m grid.
            let sections: Vec<_> = parts
                .iter()
                .filter(|p| {
                    p.entry == LADDER_SECTION
                        && (origin(p)[0] - origin(cap)[0]).abs() < 1e-3
                        && (origin(p)[2] - origin(cap)[2]).abs() < 1e-3
                })
                .collect();
            let lowest = sections
                .iter()
                .map(|p| origin(p)[1])
                .fold(f64::MAX, f64::min);
            assert!(lowest <= metres(ladder.position[1]) + 1e-4);
            assert!(lowest > metres(ladder.position[1]) - SECTION);
            for section in &sections {
                let pitch = (origin(cap)[1] - origin(section)[1]) / RUNG_SPACING;
                assert!((pitch - pitch.round()).abs() < 1e-4);
            }
        }
        let grid = rungs(&scenery);
        assert_eq!(grid.len(), 3);
        let rung = origin(caps[0])[1] + FIRST_RUNG;
        let feet = [grid[0].centre[0], rung + 0.4, grid[0].centre[1] + 0.4];
        assert!((rung_offset(&grid, feet).unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(rung_offset(&grid, [0., 0., 0.]), None);
    }

    #[test]
    fn every_rope_hangs_its_axis_off_its_face_from_an_anchor_under_the_top() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let parts = placements(&scenery);
        let ropes: Vec<_> = scenery
            .props
            .iter()
            .filter(|p| p.kind == PropKind::Rope)
            .collect();
        assert_eq!(ropes.len(), greyhaven_vale::ROPES.len());
        let anchors: Vec<_> = parts.iter().filter(|p| p.entry == ROPE_ANCHOR).collect();
        let ends: Vec<_> = parts.iter().filter(|p| p.entry == ROPE_END).collect();
        assert_eq!((anchors.len(), ends.len()), (ropes.len(), ropes.len()));
        for ((rope, anchor), end) in ropes.iter().zip(&anchors).zip(&ends) {
            let beam = origin(anchor)[1] + ANCHOR_HEIGHT;
            assert!(beam < top(rope) && beam > top(rope) - 0.5);
            // The rope's axis is the collider's front, 0.3 m out of the face.
            let model = anchor.model.map(f64::from);
            let axis = point(
                Affine {
                    m: [
                        model[0], model[4], model[8], model[1], model[5], model[9], model[2],
                        model[6], model[10],
                    ],
                    t: [model[12], model[13], model[14]],
                },
                [0., 0., 0.3],
            );
            let centre = [metres(rope.position[0]), metres(rope.position[2])];
            let front = (axis[0] - centre[0]).hypot(axis[2] - centre[1]);
            assert!((front - metres(rope.half_extents[2])).abs() < 1e-3);
            assert!(origin(end)[1] < origin(anchor)[1]);
        }
    }

    #[test]
    fn ivy_covers_the_listed_faces_and_every_boundary_route() {
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let parts = placements(&scenery);
        let ivy: Vec<_> = parts
            .iter()
            .filter(|p| p.entry.starts_with("climbing.kit.vine"))
            .collect();
        for route in greyhaven_vale::CLIMBING_ROUTES {
            let on = ivy.iter().filter(|p| {
                let [x, _, z] = origin(p);
                (metres(route.min[0])..=metres(route.max[0])).contains(&x)
                    && (metres(route.min[1])..=metres(route.max[1])).contains(&z)
            });
            assert!(on.count() >= 6, "{route:?}");
        }
        assert!(ivy.iter().any(|p| p.entry == VINE_3M));
        assert!(ivy.iter().filter(|p| p.entry == VINE_TOP).count() >= 9);
        // Every covered face is climbable: the ivy marks routes, never sheer rock.
        for (id, ..) in COVERS {
            assert!(scenery.climbing().collider(id).is_some(), "{id}");
        }
    }
}
