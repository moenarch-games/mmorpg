//! Worn handholds on climbable rock (#512, Atlas 5.1.40): pale, scuffed streaks with small
//! ledge notches running up a face, like a well-used climbing route. Both clients draw them
//! wherever the content's [`mmorpg_core::ClimbSurfaces`] mark rock, on rock props and on
//! steep terrain, so what looks climbable is exactly what climbs. Sheer rock stays smooth.
use super::batching::{Batcher, Cull};
use super::geometry::{Affine, Geometry, V, box_shape, hash, norm, point};
use super::terrain::{height, metres, surface};
use mmorpg_core::{ClimbSurfaces, Grip};
use mmorpg_scenery::TerrainGrid;
use std::f64::consts::TAU;
use std::sync::LazyLock;

/// The worn streak, in two tones so it reads scuffed rather than painted.
pub const STREAK: u32 = 0xc2baa8;
pub const SCUFF: u32 = 0xd3ccba;
/// A notch's pale, worn lip and the dark hollow under it.
pub const NOTCH: u32 = 0xdcd5c4;
pub const NOTCH_SHADOW: u32 = 0x4f4b44;

/// The streak is cut into pieces about this long (m).
const PIECE: f64 = 0.7;
/// A notch every this far up the route (m).
const NOTCH_STEP: f64 = 0.7;
/// The streak stands this far proud of the face (m), clear of depth fighting.
const LIFT: f64 = 0.03;
/// About one route every this far along a face (m).
pub const ROUTE_SPACING: f64 = 4.;
const UP: V = [0., 1., 0.];

/// A handhold piece's shape, in the unit cube (-1..1 on every axis; local +Z faces out of
/// the rock, +Y up the face). Only the faces a climber can see are built, to keep the many
/// pieces cheap.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    /// The front face alone, at z = 1: a streak piece or a notch's shadow.
    Quad,
    /// The front, top and sides: a notch's protruding lip.
    Ledge,
}

impl Shape {
    #[must_use]
    pub fn geometry(self) -> &'static Geometry {
        static QUAD: LazyLock<Geometry> = LazyLock::new(|| faces(|n| n[2] > 0.5));
        static LEDGE: LazyLock<Geometry> = LazyLock::new(|| faces(|n| n[2] > -0.5 && n[1] > -0.5));
        match self {
            Self::Quad => &QUAD,
            Self::Ledge => &LEDGE,
        }
    }
}

/// Draws one piece as an instance of its shape (#520): the thousands of pieces across the
/// world share two tiny meshes instead of each adding its own vertices to the static scene,
/// and like other small detail they fade out of a frame at mid distance.
pub fn draw(b: &mut Batcher, c: u32, shape: Shape, a: Affine) {
    b.instance(
        "handholds",
        c,
        shape.geometry(),
        a,
        [a.t[0], a.t[2]],
        Cull::Mid,
    );
}

/// The unit box's faces (four corners each) whose outward normal passes `keep`.
fn faces(keep: impl Fn(V) -> bool) -> Geometry {
    let cube = box_shape();
    let mut out = Geometry::default();
    for (corners, normals) in cube.p.chunks(4).zip(cube.n.chunks(4)) {
        if keep(normals[0]) {
            out.face(corners);
        }
    }
    out
}

/// One route up a planar face. `base` is the foot of the route on the face, `up` the unit
/// direction up along the face, `out` the face's outward unit normal and `length` how far
/// the route runs (m). `put` receives each piece as a colour, a [`Shape`] and the transform
/// of its unit cube.
pub fn route(
    base: V,
    up: V,
    out: V,
    length: f64,
    seed: f64,
    put: &mut impl FnMut(u32, Shape, Affine),
) {
    if length < 0.2 {
        return;
    }
    let side = cross(up, out);
    let along = |s: f64, x: f64, o: f64| -> V {
        std::array::from_fn(|i| base[i] + up[i] * s + side[i] * x + out[i] * o)
    };
    let frame = |centre: V, half: V| Affine {
        m: std::array::from_fn(|i| {
            let (r, c) = (i / 3, i % 3);
            [side, up, out][c][r] * half[c]
        }),
        t: centre,
    };
    // The streak wanders a little side to side and leaves bare gaps where the rock shows.
    let pieces = (length / PIECE).round().max(1.);
    let piece = length / pieces;
    for k in 0..pieces as usize {
        let h = |n: f64| hash(&[seed, k as f64, n]);
        if h(1.) < 0.12 {
            continue;
        }
        let s = (k as f64 + 0.5) * piece;
        let width = 0.09 + h(2.) * 0.08;
        let drift = (h(3.) - 0.5) * 0.08;
        put(
            if h(4.) < 0.5 { STREAK } else { SCUFF },
            Shape::Quad,
            frame(
                along(s, drift, LIFT * 0.5),
                [width, piece * 0.48, LIFT * 0.5],
            ),
        );
    }
    // Ledge notches, alternating left and right of the streak like hand and foot holds.
    let notches = (length / NOTCH_STEP).floor().max(1.);
    let step = length / notches;
    for k in 0..notches as usize {
        let h = |n: f64| hash(&[seed, k as f64, n + 10.]);
        let s = (k as f64 + 0.35 + h(1.) * 0.3) * step;
        let left = (k + (seed * 1e3) as usize).is_multiple_of(2);
        let x = if left { -0.14 } else { 0.14 } + (h(2.) - 0.5) * 0.06;
        let width = 0.08 + h(3.) * 0.05;
        put(
            NOTCH,
            Shape::Ledge,
            frame(along(s, x, LIFT + 0.04), [width, 0.035, 0.05]),
        );
        put(
            NOTCH_SHADOW,
            Shape::Quad,
            frame(along(s - 0.05, x, LIFT + 0.01), [width * 0.9, 0.018, 0.02]),
        );
    }
}

/// Routes up the four sides of a block of stacked layers centred on the origin, each layer
/// `(half extents along x and z, bottom, top)`: about one route every [`ROUTE_SPACING`]
/// along each side, climbing every layer at the same place.
pub fn block(layers: &[([f64; 2], f64, f64)], seed: f64, put: &mut impl FnMut(u32, Shape, Affine)) {
    let Some(&([wx, wz], ..)) = layers.first() else {
        return;
    };
    let faces = [
        ([0., 0., 1.], wx),
        ([0., 0., -1.], wx),
        ([1., 0., 0.], wz),
        ([-1., 0., 0.], wz),
    ];
    for (face, (out, width)) in faces.into_iter().enumerate() {
        let side = cross(UP, out);
        let count = (width * 2. / ROUTE_SPACING).round().max(1.);
        let slot = width * 2. / count;
        for r in 0..count as usize {
            let key = [seed, face as f64, r as f64];
            let u = -width + slot * (r as f64 + 0.25 + hash(&key) * 0.5);
            for (l, &([hx, hz], from, to)) in layers.iter().enumerate() {
                let (half, depth) = if face < 2 { (hx, hz) } else { (hz, hx) };
                let u = u.clamp(-half + 0.3, (half - 0.3).max(-half + 0.3));
                let mut base: V = std::array::from_fn(|i| out[i] * depth + side[i] * u);
                base[1] = from;
                let seed = hash(&[key[0], key[1], key[2], l as f64]);
                route(base, UP, out, to - from, seed, put);
            }
        }
    }
}

/// Routes up a rounded rock: the mesh `g` placed by `a`, round `centre` with `radii` (m).
/// Each route follows the mesh's facets from just above the ground (y = 0) toward the top.
pub fn boulder(
    g: &Geometry,
    a: Affine,
    centre: V,
    radii: V,
    seed: f64,
    put: &mut impl FnMut(u32, Shape, Affine),
) {
    let routes = ((radii[0] + radii[2]) * 2. / ROUTE_SPACING)
        .round()
        .clamp(2., 5.);
    let low = ((0.1 - centre[1]) / radii[1]).clamp(-0.9, 0.5).asin();
    let high = 1.15;
    let steps = (((high - low) * radii[1]) / 0.5).round().max(2.) as usize;
    for r in 0..routes as usize {
        let yaw = TAU * (r as f64 + hash(&[seed, r as f64]) * 0.6) / routes;
        let hit_at = |k: usize| {
            let pitch = low + (high - low) * k as f64 / steps as f64;
            let dir = [
                pitch.cos() * yaw.sin() * radii[0],
                pitch.sin() * radii[1],
                pitch.cos() * yaw.cos() * radii[2],
            ];
            hit(g, a, centre, dir)
        };
        let hits: Vec<_> = (0..=steps).map(hit_at).collect();
        for (k, pair) in hits.windows(2).enumerate() {
            let [Some((p, n)), Some((q, _))] = [pair[0], pair[1]] else {
                continue;
            };
            let d: V = std::array::from_fn(|i| q[i] - p[i]);
            let length = dot(d, d).sqrt();
            if length < 1e-6 {
                continue;
            }
            let up = d.map(|v| v / length);
            let along = dot(n, up);
            let out = norm(std::array::from_fn(|i| n[i] - up[i] * along));
            // The chord between facets runs a little inside the rock; lift it clear.
            let base = std::array::from_fn(|i| p[i] + out[i] * 0.02);
            route(
                base,
                up,
                out,
                length,
                hash(&[seed, r as f64, k as f64]),
                put,
            );
        }
    }
}

/// Routes up the terrain wherever `climbing` marks rock and the drawn ground rises more than
/// a metre per metre (45°, steeper than anyone walks): in each 4 m cell, one route every
/// [`ROUTE_SPACING`] across the slope, at the same places in every cell up a face so the
/// routes run unbroken from foot to top.
pub fn terrain(
    g: &TerrainGrid,
    climbing: &ClimbSurfaces,
    put: &mut impl FnMut(u32, Shape, Affine),
) {
    if climbing
        .terrain()
        .iter()
        .all(|&(_, grip)| grip != Some(Grip::Rock))
    {
        return;
    }
    let step = metres(g.step);
    for r in 0..g.rows.saturating_sub(1) {
        for c in 0..g.columns.saturating_sub(1) {
            let (c, r) = (c as isize, r as isize);
            let x0 = metres(g.origin[0]) + c as f64 * step;
            let z0 = metres(g.origin[1]) + r as f64 * step;
            let units = |v: f64| (v * 100.).round() as i32;
            if climbing.terrain_at(units(x0 + step / 2.), units(z0 + step / 2.)) != Some(Grip::Rock)
            {
                continue;
            }
            let h = |dc, dr| height(g, c + dc, r + dr);
            let gx = (h(1, 0) + h(1, 1) - h(0, 0) - h(0, 1)) / (2. * step);
            let gz = (h(0, 1) + h(1, 1) - h(0, 0) - h(1, 0)) / (2. * step);
            if gx.hypot(gz) <= 1. {
                continue;
            }
            // Routes run along the steeper axis, at fixed positions across it.
            let along_x = gx.abs() >= gz.abs();
            let (lo, across0) = if along_x { (x0, z0) } else { (z0, x0) };
            let first = (across0 / ROUTE_SPACING).floor() as i64;
            let last = ((across0 + step) / ROUTE_SPACING).floor() as i64;
            for slot in first..=last {
                let at =
                    (slot as f64 + 0.2 + hash(&[slot as f64, f64::from(u8::from(along_x))]) * 0.6)
                        * ROUTE_SPACING;
                if at < across0 + 0.2 || at > across0 + step - 0.2 {
                    continue;
                }
                let place = |a: f64| if along_x { [a, at] } else { [at, a] };
                // The cell's two triangles meet on its diagonal: the route bends there.
                let f = (at - across0) / step;
                let marks = [lo, lo + step * (1. - f), lo + step];
                let mut ends = marks.map(|a| {
                    let [x, z] = place(a);
                    [x, surface(g, x, z), z]
                });
                let rising = if along_x { gx } else { gz };
                if rising < 0. {
                    ends.reverse();
                }
                for pair in ends.windows(2) {
                    let (p, q) = (pair[0], pair[1]);
                    let d: V = std::array::from_fn(|i| q[i] - p[i]);
                    let length = dot(d, d).sqrt();
                    if length < 0.2 {
                        continue;
                    }
                    let up = d.map(|v| v / length);
                    let across = if along_x { [0., 0., 1.] } else { [1., 0., 0.] };
                    let mut out = norm(cross(across, up));
                    if out[1] < 0. {
                        out = out.map(|v| -v);
                    }
                    let seed = hash(&[p[0], p[2], 7.]);
                    route(p, up, out, length, seed, put);
                }
            }
        }
    }
}

/// The nearest point where the ray from `origin` along `dir` leaves the mesh `g` placed by
/// `a`, with that facet's outward normal.
fn hit(g: &Geometry, a: Affine, origin: V, dir: V) -> Option<(V, V)> {
    let mut best: Option<(f64, V)> = None;
    for tri in g.indices.as_chunks::<3>().0 {
        let [p0, p1, p2] = tri.map(|i| point(a, g.p[i as usize]));
        let e1: V = std::array::from_fn(|i| p1[i] - p0[i]);
        let e2: V = std::array::from_fn(|i| p2[i] - p0[i]);
        let h = cross(dir, e2);
        let det = dot(e1, h);
        if det.abs() < 1e-12 {
            continue;
        }
        let s: V = std::array::from_fn(|i| origin[i] - p0[i]);
        let u = dot(s, h) / det;
        let q = cross(s, e1);
        let v = dot(dir, q) / det;
        let t = dot(e2, q) / det;
        if u < 0. || v < 0. || u + v > 1. || t <= 0. || best.is_some_and(|(b, _)| b <= t) {
            continue;
        }
        let mut n = norm(cross(e1, e2));
        if dot(n, dir) < 0. {
            n = n.map(|v| -v);
        }
        best = Some((t, n));
    }
    best.map(|(t, n)| (std::array::from_fn(|i| origin[i] + dir[i] * t), n))
}

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
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

    #[test]
    fn a_route_runs_up_the_face_and_stands_proud_of_it() {
        let mut pieces = Vec::new();
        route(
            [0., 0., 1.],
            [0., 1., 0.],
            [0., 0., 1.],
            4.,
            0.3,
            &mut |c, _, a| pieces.push((c, a)),
        );
        assert!(pieces.iter().any(|&(c, _)| c == NOTCH));
        assert!(pieces.iter().any(|&(c, _)| c == STREAK || c == SCUFF));
        for (_, a) in &pieces {
            let low = point(*a, [0., -1., -1.]);
            let high = point(*a, [0., 1., -1.]);
            assert!(low[1] < high[1] && high[1] <= 4.01);
            // Every piece's back stays on or in front of the face.
            assert!(low[2] >= 1. - 1e-9, "{low:?}");
        }
    }
}
