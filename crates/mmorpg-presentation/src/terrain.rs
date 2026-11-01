//! Terrain triangulation, overlays, roads, lake beds and water.
use super::{Draw, batching::*, geometry::*};
use crate::ground::{Ground, Layer};
use mmorpg_scenery::{Biome, Scenery, TerrainGrid};
use std::f64::consts::TAU;
use std::{collections::BTreeMap, sync::Arc};
pub fn metres(v: i32) -> f64 {
    f64::from(v) / 100.
}
pub fn height(g: &TerrainGrid, c: isize, r: isize) -> f64 {
    metres(
        g.heights[r.clamp(0, g.rows as isize - 1) as usize * g.columns
            + c.clamp(0, g.columns as isize - 1) as usize],
    )
}
pub fn surface(g: &TerrainGrid, x: f64, z: f64) -> f64 {
    let step = metres(g.step);
    let gx = (x - metres(g.origin[0])) / step;
    let gz = (z - metres(g.origin[1])) / step;
    let c = gx.floor().clamp(0., (g.columns - 2) as f64) as isize;
    let r = gz.floor().clamp(0., (g.rows - 2) as f64) as isize;
    let fx = (gx - c as f64).clamp(0., 1.);
    let fz = (gz - r as f64).clamp(0., 1.);
    let h = |dc, dr| height(g, c + dc, r + dr);
    if fx + fz <= 1. {
        h(0, 0) + (h(1, 0) - h(0, 0)) * fx + (h(0, 1) - h(0, 0)) * fz
    } else {
        h(1, 1) + (h(0, 1) - h(1, 1)) * (1. - fx) + (h(1, 0) - h(1, 1)) * (1. - fz)
    }
}
pub fn biome_color(b: Biome) -> u32 {
    match b.name() {
        "meadow" => 0x6a9a42,
        "hub" => 0x739a48,
        "woods" => 0x58803c,
        "hollow" => 0x94805f,
        "farmland" => 0x7a5836,
        "road" => 0xa4865c,
        "plaza" => 0xa8987a,
        "shore" => 0xcdbb88,
        "lake bed" => 0x5e5a44,
        "foothills" => 0x5d8a44,
        "highland" => 0x6f7d52,
        "rock" => 0x8b877e,
        "snow" => 0xeef1f5,
        _ => {
            let c = b.base_color();
            (u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2])
        }
    }
}
fn overlay(b: Biome) -> bool {
    matches!(
        b.name(),
        "road" | "lake bed" | "shore" | "plaza" | "farmland"
    )
}
pub(crate) fn ground_biomes(g: &TerrainGrid) -> Vec<Biome> {
    g.biomes
        .iter()
        .enumerate()
        .map(|(i, b)| {
            if !overlay(*b) {
                return *b;
            }
            let (c, r) = ((i % g.columns) as isize, (i / g.columns) as isize);
            let mut counts = BTreeMap::new();
            for dr in -2..=2 {
                for dc in -2..=2 {
                    let (x, z) = (c + dc, r + dr);
                    if x >= 0 && z >= 0 && x < g.columns as isize && z < g.rows as isize {
                        let b = g.biomes[z as usize * g.columns + x as usize];
                        if !overlay(b) {
                            *counts.entry(b).or_insert(0) += 1;
                        }
                    }
                }
            }
            let mut best = Biome::Meadow;
            let mut n = 0;
            for (b, count) in counts {
                if count > n {
                    best = b;
                    n = count;
                }
            }
            best
        })
        .collect()
}
pub(crate) fn noise(x: f64, z: f64, cell: f64, seed: f64) -> f64 {
    let (gx, gz) = (x / cell, z / cell);
    let (x0, z0) = (gx.floor(), gz.floor());
    let smooth = |t: f64| t * t * (3. - 2. * t);
    let (fx, fz) = (smooth(gx - x0), smooth(gz - z0));
    let a = |x, z| hash(&[seed, x0 + x, z0 + z]);
    let near = a(0., 0.) + (a(1., 0.) - a(0., 0.)) * fx;
    let far = a(0., 1.) + (a(1., 1.) - a(0., 1.)) * fx;
    near + (far - near) * fz
}
fn tone(b: Biome, x: f64, z: f64) -> u32 {
    let base = biome_color(b);
    if !matches!(
        b.name(),
        "meadow" | "hub" | "woods" | "hollow" | "foothills" | "highland" | "rock"
    ) {
        return base;
    }
    let v = noise(x, z, 22., 17.) * 0.7 + noise(x, z, 7., 29.) * 0.3;
    if v < 0.4 {
        shade(base, 0.9)
    } else if v < 0.62 {
        base
    } else {
        mix(base, 0xc2cf6e, 0.2)
    }
}
/// Whether the open rectangles `a` and `b` (`[min x, min z, max x, max z]`) overlap.
fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// Grid cell `cell` (metres; its first sample `s`) cut along the edges of the `holes` in it,
/// every piece outside them drawn on the cell's bilinear surface.
fn around_holes(
    out: &mut Geometry,
    g: &TerrainGrid,
    biomes: &[Biome],
    s: usize,
    cell: [f64; 4],
    holes: &[[f64; 4]],
) {
    let cuts = |lo: f64, hi: f64, axis: usize| {
        let mut cuts = vec![lo, hi];
        for hole in holes {
            cuts.extend(
                [hole[axis], hole[axis + 2]]
                    .into_iter()
                    .filter(|&v| lo < v && v < hi),
            );
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup();
        cuts
    };
    let (xs, zs) = (cuts(cell[0], cell[2], 0), cuts(cell[1], cell[3], 1));
    let h = |sample: usize| metres(g.heights[sample]);
    let corners = [h(s), h(s + 1), h(s + g.columns), h(s + g.columns + 1)];
    let (width, depth) = (cell[2] - cell[0], cell[3] - cell[1]);
    let dx = (corners[1] - corners[0] + corners[3] - corners[2]) / (2. * width);
    let dz = (corners[2] - corners[0] + corners[3] - corners[1]) / (2. * depth);
    let normal = norm([-dx, 1., -dz]);
    let mut vertex = |x: f64, z: f64| {
        let (u, v) = ((x - cell[0]) / width, (z - cell[1]) / depth);
        let y = (corners[0] * (1. - u) + corners[1] * u) * (1. - v)
            + (corners[2] * (1. - u) + corners[3] * u) * v;
        let index = out.p.len() as u32;
        out.p.push([x, y, z]);
        out.n.push(normal);
        out.colors.push(rgb(tone(biomes[s], x, z)).map(f64::from));
        index
    };
    for z in zs.windows(2) {
        for x in xs.windows(2) {
            let piece = [x[0], z[0], x[1], z[1]];
            if holes.iter().any(|hole| overlaps(*hole, piece)) {
                continue;
            }
            let (a, b, c, d) = (
                vertex(x[0], z[0]),
                vertex(x[0], z[1]),
                vertex(x[1], z[0]),
                vertex(x[1], z[1]),
            );
            out.indices.extend([a, b, c, c, b, d]);
        }
    }
}

/// Triangulates a grid. A far ring leaves out the cells inside `inner`, the near grid's
/// rectangle `[min x, min z, max x, max z]` in metres, and sinks its vertices inside it. A cell
/// overlapping one of `holes` (`[min x, min z, max x, max z]`, metres) is drawn around it, and
/// a triangle (its grid samples) a ground overlay `covered` entirely is left out (#525).
fn grid(
    out: &mut Geometry,
    g: &TerrainGrid,
    far: bool,
    inner: [f64; 4],
    holes: &[[f64; 4]],
    covered: &dyn Fn([usize; 3]) -> bool,
) {
    let biomes = if far {
        g.biomes.clone()
    } else {
        ground_biomes(g)
    };
    let mut map = BTreeMap::new();
    let within = |x: f64, z: f64| inner[0] < x && x < inner[2] && inner[1] < z && z < inner[3];
    let at = |c: usize, r: usize| {
        within(
            metres(g.origin[0] + c as i32 * g.step),
            metres(g.origin[1] + r as i32 * g.step),
        )
    };
    for r in 0..g.rows - 1 {
        for c in 0..g.columns - 1 {
            if far && at(c, r) && at(c + 1, r) && at(c, r + 1) && at(c + 1, r + 1) {
                continue;
            }
            let s = r * g.columns + c;
            let cell = [
                metres(g.origin[0] + c as i32 * g.step),
                metres(g.origin[1] + r as i32 * g.step),
                metres(g.origin[0] + (c as i32 + 1) * g.step),
                metres(g.origin[1] + (r as i32 + 1) * g.step),
            ];
            if holes.iter().any(|hole| overlaps(*hole, cell)) {
                around_holes(out, g, &biomes, s, cell, holes);
                continue;
            }
            for sample in [
                [s, s + g.columns, s + 1],
                [s + 1, s + g.columns, s + g.columns + 1],
            ]
            .into_iter()
            .filter(|tri| !covered(*tri))
            .flatten()
            {
                let idx = *map.entry(sample).or_insert_with(|| {
                    let (c, r) = (sample % g.columns, sample / g.columns);
                    let (x, z) = (
                        metres(g.origin[0] + c as i32 * g.step),
                        metres(g.origin[1] + r as i32 * g.step),
                    );
                    let h = metres(g.heights[sample]);
                    let y = if far && within(x, z) { h - 4. } else { h };
                    let step = metres(g.step);
                    let dx = (height(g, c as isize + 1, r as isize)
                        - height(g, c as isize - 1, r as isize))
                        / (2. * step);
                    let dz = (height(g, c as isize, r as isize + 1)
                        - height(g, c as isize, r as isize - 1))
                        / (2. * step);
                    let index = out.p.len() as u32;
                    out.p.push([x, y, z]);
                    out.n.push(norm([-dx, 1., -dz]));
                    let color = if far {
                        biome_color(biomes[sample])
                    } else {
                        tone(biomes[sample], x, z)
                    };
                    out.colors.push(rgb(color).map(f64::from));
                    index
                });
                out.indices.push(idx);
            }
        }
    }
}
pub fn bounds(g: &TerrainGrid, name: &str) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for (i, b) in g.biomes.iter().enumerate() {
        if b.name() != name {
            continue;
        }
        let p = [
            metres(g.origin[0] + (i % g.columns) as i32 * g.step),
            metres(g.origin[1] + (i / g.columns) as i32 * g.step),
        ];
        for j in 0..2 {
            lo[j] = lo[j].min(p[j]);
            hi[j] = hi[j].max(p[j]);
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}
fn ellipse(m: &mut Geometry, c: [f64; 2], r: [f64; 2], y: f64, n: u32) {
    let base = m.p.len() as u32;
    m.p.push([c[0], y, c[1]]);
    m.n.push([0., 1., 0.]);
    for i in 0..n {
        let a = f64::from(i) / f64::from(n) * TAU;
        m.p.push([c[0] + a.sin() * r[0], y, c[1] + a.cos() * r[1]]);
        m.n.push([0., 1., 0.]);
        m.indices
            .extend([base, base + 1 + i, base + 1 + (i + 1) % n]);
    }
}
pub fn build(b: &mut Batcher, s: &Scenery, g: &TerrainGrid) -> Vec<Draw> {
    let inner = [
        g.origin[0],
        g.origin[1],
        g.origin[0] + (g.columns as i32 - 1) * g.step,
        g.origin[1] + (g.rows as i32 - 1) * g.step,
    ]
    .map(metres);
    let holes: Vec<_> = s
        .terrain_holes
        .iter()
        .map(|hole| [hole.min[0], hole.min[1], hole.max[0], hole.max[1]].map(metres))
        .collect();
    // The opaque grass base, then the masked ground overlays over it (#525).
    let base = b.ground(Ground {
        layer: Layer::Grass,
        overlay: false,
    });
    let weights = crate::ground::weights(s, g);
    grid(base, g, false, inner, &holes, &|tri| weights.covers(tri));
    crate::ground::project_top(base, Layer::Grass);
    crate::ground::grid_overlays(b, &weights, g, &holes);
    grid(
        b.global("far", 0xffffff),
        &s.far_terrain_grid(2000),
        true,
        inner,
        &[],
        &|_| false,
    );
    // Worn handholds up the climbable rock faces of the terrain (#512).
    crate::handholds::terrain(g, s.climbing(), &mut |c, shape, a| {
        crate::handholds::draw(b, c, shape, a);
    });
    // The trampled plaza, then one tilled patch per ploughed field (#424).
    if let Some(plaza) = bounds(g, "plaza") {
        crate::ground::patch(b, g, Layer::Path, 0xa8987a, plaza, 1.2);
    }
    for field in s.farmland() {
        let field = (field.min.map(metres), field.max.map(metres));
        crate::ground::patch(b, g, Layer::Mud, 0x7a5836, field, 1.);
    }
    crate::ground::roads(b, s, g);
    let mut water = Vec::new();
    for (i, w) in s.water.iter().enumerate() {
        let c = w.centre.map(metres);
        let r = w.radii.map(metres);
        // The shore beyond the water's edge is the sand overlay; these rings are the bed.
        for (f, col, lift) in [
            (1.02, mix(0xcdbb88, 0x4f5a44, 0.35), 0.012),
            (0.9, mix(0xcdbb88, 0x4f5a44, 0.7), 0.02),
            (0.72, 0x4f5a44, 0.028),
            (0.45, shade(0x4f5a44, 0.85), 0.036),
        ] {
            ellipse(b.global("lake-bed", col), c, r.map(|v| v * f), lift, 48);
        }
        let mut mesh = Geometry::default();
        ellipse(&mut mesh, c, r, metres(w.surface), 64);
        let mut mat = material(0x3f86a0, false);
        mat.opacity = 0.72;
        water.push(Draw {
            id: format!("water-{i}"),
            resource_key: format!("scenery:water:{i}"),
            geometry: Arc::new(mesh),
            model: matrix(ID),
            material: mat,
        });
    }
    water
}
