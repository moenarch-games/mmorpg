//! Textured ground layers for the terrain (#525).
//!
//! The terrain is one opaque grass mesh with masked overlay meshes on top, one per [`Layer`]
//! over the whole terrain (so a layer costs one draw however large): forest floor, scree,
//! mud, sand, worn paths and exposed rock. Each overlay vertex carries the layer's weight as
//! its vertex colour alpha, derived from the scenery's biomes, slope, water and roads, with
//! seeded noise breaking up the edges. The clients draw an overlay alpha-masked
//! ([`MASK_CUTOFF`]) with the layer's material from asset-tooling's `greyhaven-terrain`
//! bundle, whose base colour alpha is `0.5 + 0.5 × height`: a texel shows once
//! `weight × alpha` reaches the cut-off, so high texels (stones, tufts) poke through first and
//! every transition is a height blend rather than a fade. Vertex colours carry the
//! art-directed palette; the clients divide the textures by their mean colour, so a texture
//! adds detail around the palette instead of replacing it. Without the bundle the same meshes
//! draw flat in their vertex colours, masked at half weight: the explicit fallback.
//!
//! Overlays cast no shadows: a masked caster is cut by its material's alpha alone, without the
//! vertex alpha, so an overlay would shadow the ground under its invisible parts. Its material
//! alpha is [`OVERLAY_ALPHA`], below the cut-off, and its vertex alpha the weight divided by it,
//! so the main pass still cuts at `weight × height alpha` while every shadow texel is cut away.
use crate::batching::{mix, rgb, shade};
use crate::geometry::{Geometry, V, norm};
use crate::terrain::{ground_biomes, height, metres, noise, surface};
use mmorpg_scenery::{Biome, Scenery, TerrainGrid};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// The asset-tooling bundle the layers' materials come from; entry `<BUNDLE>.<layer>`.
pub const BUNDLE: &str = "greyhaven.terrain";
/// An overlay texel shows where vertex alpha × base colour alpha reaches this.
pub const MASK_CUTOFF: f32 = 0.5;
/// An overlay material's base colour alpha; its vertex alpha is its weight over this.
pub const OVERLAY_ALPHA: f32 = 0.25;

/// One ground material, in draw order: later overlays cover earlier ones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    Grass,
    Forest,
    Gravel,
    Mud,
    Sand,
    Path,
    Rock,
}

impl Layer {
    pub const ALL: [Self; 7] = [
        Self::Grass,
        Self::Forest,
        Self::Gravel,
        Self::Mud,
        Self::Sand,
        Self::Path,
        Self::Rock,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Grass => "grass",
            Self::Forest => "forest",
            Self::Gravel => "gravel",
            Self::Mud => "mud",
            Self::Sand => "sand",
            Self::Path => "path",
            Self::Rock => "rock",
        }
    }

    /// The bundle entry holding the layer's material.
    #[must_use]
    pub fn key(self) -> String {
        format!("{BUNDLE}.{}", self.name())
    }

    /// Metres one texture repeat covers.
    #[must_use]
    pub const fn tile_metres(self) -> f64 {
        match self {
            Self::Grass | Self::Forest | Self::Sand => 3.,
            Self::Gravel | Self::Path => 2.5,
            Self::Mud => 2.,
            Self::Rock => 4.,
        }
    }

    /// Metres an overlay floats above the terrain triangles, apart from the other layers.
    fn lift(self) -> f64 {
        0.015 + 0.01 * f64::from(self as u8)
    }
}

/// A ground draw's layer, whether it is a masked overlay (else the opaque base), and whether
/// it is a terrain tile's far level ([`crate::tiles`], #526): drawn flat in its vertex colours,
/// never textured, so a distant tile costs no texture sampling and shows no repeats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ground {
    pub layer: Layer,
    pub overlay: bool,
    pub far: bool,
}

impl Ground {
    /// The near level's opaque base or masked overlay of `layer`.
    #[must_use]
    pub const fn near(layer: Layer, overlay: bool) -> Self {
        Self {
            layer,
            overlay,
            far: false,
        }
    }

    /// The draw's name in IDs and resource keys.
    #[must_use]
    pub fn name(self) -> String {
        format!(
            "ground-{}{}{}",
            self.layer.name(),
            if self.overlay { "-overlay" } else { "" },
            if self.far { "-far" } else { "" }
        )
    }
}

/// Ground meshes by draw, in draw order.
pub(crate) type Layers = BTreeMap<Ground, Geometry>;

fn smoothstep(lo: f64, hi: f64, v: f64) -> f64 {
    let t = ((v - lo) / (hi - lo)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// The tangent along `u` for a vertex with normal `n` whose texture `u` and `v` run along
/// `du` and `dv`: Gram-Schmidt onto the surface, with MikkTSpace's bitangent sign.
fn tangent(n: V, du: V, dv: V) -> [f64; 4] {
    let d = n[0] * du[0] + n[1] * du[1] + n[2] * du[2];
    let t = norm([du[0] - n[0] * d, du[1] - n[1] * d, du[2] - n[2] * d]);
    let b = [
        n[1] * t[2] - n[2] * t[1],
        n[2] * t[0] - n[0] * t[2],
        n[0] * t[1] - n[1] * t[0],
    ];
    let sign = if b[0] * dv[0] + b[1] * dv[1] + b[2] * dv[2] < 0. {
        -1.
    } else {
        1.
    };
    [t[0], t[1], t[2], sign]
}

/// Texture coordinates and tangents projected from above (`u` along x, `v` along z) for
/// every vertex of `g` that has none yet.
pub(crate) fn project_top(g: &mut Geometry, layer: Layer) {
    let tile = layer.tile_metres();
    for i in g.uv.len()..g.p.len() {
        let [x, _, z] = g.p[i];
        g.uv.push([x / tile, z / tile]);
        g.tangents.push(tangent(g.n[i], [1., 0., 0.], [0., 0., 1.]));
    }
}

/// Appends one overlay vertex of layer weight `weight`.
fn vertex(g: &mut Geometry, p: V, n: V, color: u32, weight: f64) -> u32 {
    let index = g.p.len() as u32;
    g.p.push(p);
    g.n.push(n);
    g.colors.push(rgb(color).map(f64::from));
    g.alpha
        .push(weight.clamp(0., 1.) / f64::from(OVERLAY_ALPHA));
    index
}

/// Gentle seeded brightness variation so a layer's colour is not one flat value.
fn tinted(color: u32, x: f64, z: f64, seed: f64) -> u32 {
    shade(color, 0.92 + 0.16 * noise(x, z, 9., seed))
}

/// Per-vertex weights of the grid overlays over the near terrain grid.
#[derive(Clone, Debug)]
pub(crate) struct Weights {
    layers: BTreeMap<Layer, Vec<f64>>,
    colors: BTreeMap<Layer, Vec<u32>>,
}

impl Weights {
    /// Whether an overlay hides the terrain triangle of grid samples `tri` entirely: one at full
    /// weight on every corner always passes its mask, so the base need not draw there.
    pub(crate) fn covers(&self, tri: [usize; 3]) -> bool {
        self.layers
            .values()
            .any(|weights| tri.iter().all(|&i| weights[i] >= 1.))
    }

    /// The colour grid sample `i` shows from afar (#526): the base colour `base` covered by each
    /// overlay in draw order, by the share of its texels that pass the mask at its weight (`w ×
    /// alpha ≥ 0.5` with height alpha spread over 0.5–1: `2 − 1 / w`).
    pub(crate) fn baked(&self, i: usize, base: u32) -> u32 {
        self.layers.iter().fold(base, |color, (layer, weights)| {
            let w = weights[i];
            let shown = if w <= 0.5 { 0. } else { (2. - 1. / w).min(1.) };
            mix(color, self.colors[layer][i], shown)
        })
    }

    /// Every weight, in a stable order: identity input for the tiles built from them.
    pub(crate) fn values(&self) -> impl Iterator<Item = f64> + '_ {
        self.layers
            .values()
            .flatten()
            .copied()
            .chain(self.colors.values().flatten().map(|&c| f64::from(c)))
    }
}

/// Box blur of a per-vertex field over `(2 radius + 1)²` samples, clamped at the edges.
fn blur(g: &TerrainGrid, field: &[f64], radius: isize) -> Vec<f64> {
    let (columns, rows) = (g.columns as isize, g.rows as isize);
    let pass = |source: &[f64], horizontal: bool| -> Vec<f64> {
        (0..source.len())
            .map(|i| {
                let (c, r) = ((i as isize) % columns, (i as isize) / columns);
                let mut sum = 0.;
                for d in -radius..=radius {
                    let (c, r) = if horizontal {
                        ((c + d).clamp(0, columns - 1), r)
                    } else {
                        (c, (r + d).clamp(0, rows - 1))
                    };
                    sum += source[(r * columns + c) as usize];
                }
                sum / (2 * radius + 1) as f64
            })
            .collect()
    };
    pass(&pass(field, true), false)
}

pub(crate) fn weights(s: &Scenery, g: &TerrainGrid) -> Weights {
    let count = g.columns * g.rows;
    let grounds = ground_biomes(g);
    let step = metres(g.step);
    let position = |i: usize| {
        (
            g.origin[0] + (i % g.columns) as i32 * g.step,
            g.origin[1] + (i / g.columns) as i32 * g.step,
        )
    };
    let slope: Vec<f64> = (0..count)
        .map(|i| {
            let (c, r) = ((i % g.columns) as isize, (i / g.columns) as isize);
            let dx = (height(g, c + 1, r) - height(g, c - 1, r)) / (2. * step);
            let dz = (height(g, c, r + 1) - height(g, c, r - 1)) / (2. * step);
            1. - norm([-dx, 1., -dz])[1]
        })
        .collect();
    let indicator = |of: &[Biome], wanted: &[&str]| -> Vec<f64> {
        of.iter()
            .map(|b| f64::from(u8::from(wanted.contains(&b.name()))))
            .collect()
    };
    let woods = blur(g, &indicator(&grounds, &["woods"]), 2);
    let hollow = blur(g, &indicator(&grounds, &["hollow"]), 2);
    let highland = blur(g, &indicator(&grounds, &["foothills", "highland"]), 2);
    let bare = blur(g, &indicator(&g.biomes, &["rock"]), 1);
    let mut layers: BTreeMap<Layer, Vec<f64>> = BTreeMap::new();
    let mut colors: BTreeMap<Layer, Vec<u32>> = BTreeMap::new();
    for i in 0..count {
        let (ux, uz) = position(i);
        let (x, z) = (metres(ux), metres(uz));
        // Breakup at two scales so no edge follows the 4 m grid.
        let breakup = noise(x, z, 11., 41.) * 0.65 + noise(x, z, 4., 43.) * 0.35 - 0.5;
        let water = s.water_distance([ux, uz]);
        let shore = if water == i64::MAX {
            f64::INFINITY
        } else {
            water as f64 / 100.
        };
        let marsh = mmorpg_core::greyhaven_vale::area_at(ux, uz).is_some_and(|area| {
            area.id() == mmorpg_core::greyhaven_vale::marsh::MARSH_LAKE_WETLANDS
        });
        let rock = smoothstep(0.17, 0.29, slope[i] + 0.06 * breakup).max(bare[i]);
        let gravel = smoothstep(0.07, 0.15, slope[i] + 0.05 * breakup).max(smoothstep(
            0.35,
            0.8,
            highland[i] * (0.55 + breakup),
        ));
        let forest = smoothstep(0.3, 0.7, woods[i] + 0.45 * breakup);
        let mud = if shore > 0. {
            (1. - smoothstep(2., 6.5, shore + 3.5 * breakup)).max(if marsh {
                smoothstep(0.15, 0.35, breakup) * (1. - smoothstep(4., 12., shore))
            } else {
                0.
            })
        } else {
            0.
        };
        let sand = if shore > 0. {
            1. - smoothstep(1.2, 3.2, shore + 2. * breakup)
        } else {
            0.
        };
        let path = smoothstep(0.35, 0.75, hollow[i] + 0.5 * breakup);
        for (layer, weight, color) in [
            (Layer::Forest, forest, mix(0x58803c, 0x6e5a3c, 0.22)),
            (Layer::Gravel, gravel, 0x8f8a7c),
            (Layer::Mud, mud, 0x5e5038),
            (Layer::Sand, sand, 0xcdbb88),
            (Layer::Path, path, 0x94805f),
            (Layer::Rock, rock, 0x8b877e),
        ] {
            layers.entry(layer).or_insert_with(|| vec![0.; count])[i] = weight;
            colors.entry(layer).or_insert_with(|| vec![0; count])[i] =
                tinted(color, x, z, f64::from(layer as u8) * 7.);
        }
    }
    Weights { layers, colors }
}

/// Overlaps of two open rectangles `[min x, min z, max x, max z]`.
fn overlaps(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// How an overlay vertex is textured: from above, or (rock only) from the side along x or z.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Projection {
    Top,
    AlongX,
    AlongZ,
}

impl Projection {
    /// The world directions texture `u` and `v` run along.
    fn axes(self) -> (V, V) {
        match self {
            Self::Top => ([1., 0., 0.], [0., 0., 1.]),
            Self::AlongX => ([0., 0., 1.], [0., -1., 0.]),
            Self::AlongZ => ([1., 0., 0.], [0., -1., 0.]),
        }
    }

    /// Rock is textured along the axis its smooth normals are most across (triplanar, chosen
    /// per triangle), so steep faces show the rock upright instead of smeared; slopes up to
    /// about 53° keep the top projection, so seams stay on real cliffs.
    fn of(layer: Layer, normals: [V; 3]) -> Self {
        if layer != Layer::Rock {
            return Self::Top;
        }
        let n = norm(std::array::from_fn(|axis| {
            normals.iter().map(|n| n[axis]).sum::<f64>()
        }))
        .map(f64::abs);
        if n[1] >= 0.6 {
            Self::Top
        } else if n[0] >= n[2] {
            Self::AlongX
        } else {
            Self::AlongZ
        }
    }
}

/// The grid overlays over the grid cells `cells` (`[first column, first row, end column, end
/// row]`): every triangle where some corner's weight could show, skipping the terrain's
/// holes. Vertices are shared per grid sample and projection.
pub(crate) fn grid_overlays(
    layers: &mut Layers,
    w: &Weights,
    g: &TerrainGrid,
    holes: &[[f64; 4]],
    cells: [usize; 4],
) {
    let step = metres(g.step);
    let at = |sample: usize| -> (V, V) {
        let (c, r) = (sample % g.columns, sample / g.columns);
        let (x, z) = (
            metres(g.origin[0] + c as i32 * g.step),
            metres(g.origin[1] + r as i32 * g.step),
        );
        let (ci, ri) = (c as isize, r as isize);
        let dx = (height(g, ci + 1, ri) - height(g, ci - 1, ri)) / (2. * step);
        let dz = (height(g, ci, ri + 1) - height(g, ci, ri - 1)) / (2. * step);
        ([x, metres(g.heights[sample]), z], norm([-dx, 1., -dz]))
    };
    for (&layer, weights) in &w.layers {
        let colors = &w.colors[&layer];
        let (lift, tile) = (layer.lift(), layer.tile_metres());
        let out = layers.entry(Ground::near(layer, true)).or_default();
        let mut shared = BTreeMap::new();
        for r in cells[1]..cells[3] {
            for c in cells[0]..cells[2] {
                let s0 = r * g.columns + c;
                let cell = [
                    metres(g.origin[0] + c as i32 * g.step),
                    metres(g.origin[1] + r as i32 * g.step),
                    metres(g.origin[0] + (c as i32 + 1) * g.step),
                    metres(g.origin[1] + (r as i32 + 1) * g.step),
                ];
                if holes.iter().any(|hole| overlaps(*hole, cell)) {
                    continue;
                }
                for tri in [
                    [s0, s0 + g.columns, s0 + 1],
                    [s0 + 1, s0 + g.columns, s0 + g.columns + 1],
                ] {
                    if tri.iter().all(|&i| weights[i] < 0.5) {
                        continue;
                    }
                    let projection = Projection::of(layer, tri.map(|i| at(i).1));
                    for sample in tri {
                        let index = *shared.entry((sample, projection)).or_insert_with(|| {
                            let (p, n) = at(sample);
                            let index = vertex(
                                out,
                                [p[0], p[1] + lift, p[2]],
                                n,
                                colors[sample],
                                weights[sample],
                            );
                            let (du, dv) = projection.axes();
                            let dot = |d: V| p[0] * d[0] + p[1] * d[1] + p[2] * d[2];
                            out.uv.push([dot(du) / tile, dot(dv) / tile]);
                            out.tangents.push(tangent(n, du, dv));
                            index
                        });
                        out.indices.push(index);
                    }
                }
            }
        }
    }
}

/// Worn dirt paths along every road: a ribbon full-weight across the road that fades out
/// over `EDGE` metres either side, and a disc at every bend.
pub(crate) fn roads(layers: &mut Layers, s: &Scenery, g: &TerrainGrid) {
    const EDGE: f64 = 1.6;
    let out = layers.entry(Ground::near(Layer::Path, true)).or_default();
    let lift = Layer::Path.lift();
    let up = [0., 1., 0.];
    let color = |x: f64, z: f64| tinted(0xa4865c, x, z, 5.);
    for road in &s.roads {
        let points: Vec<[f64; 2]> = road.points.iter().map(|p| p.map(metres)).collect();
        let width = metres(road.half_width);
        for p in points.windows(2) {
            let ([ax, az], [bx, bz]) = (p[0], p[1]);
            let l = (bx - ax).hypot(bz - az);
            if l == 0. {
                continue;
            }
            let (nx, nz) = (-(bz - az) / l, (bx - ax) / l);
            let pieces = (l / 2.).ceil().max(1.) as usize;
            let base = out.p.len() as u32;
            for i in 0..=pieces {
                let t = i as f64 / pieces as f64;
                let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
                for (offset, alpha) in [
                    (-(width + EDGE), 0.),
                    (-width, 1.),
                    (width, 1.),
                    (width + EDGE, 0.),
                ] {
                    let (px, pz) = (x + nx * offset, z + nz * offset);
                    vertex(
                        out,
                        [px, surface(g, px, pz) + lift, pz],
                        up,
                        color(px, pz),
                        alpha,
                    );
                }
            }
            for i in 0..pieces as u32 {
                for strip in 0..3 {
                    let a = base + i * 4 + strip;
                    out.indices.extend([a, a + 1, a + 4, a + 1, a + 5, a + 4]);
                }
            }
        }
        for &[x, z] in &points {
            let base = out.p.len() as u32;
            vertex(out, [x, surface(g, x, z) + lift, z], up, color(x, z), 1.);
            const SIDES: u32 = 16;
            for i in 0..SIDES {
                let a = f64::from(i) / f64::from(SIDES) * TAU;
                for (radius, alpha) in [(width, 1.), (width + EDGE, 0.)] {
                    let (px, pz) = (x + a.sin() * radius, z + a.cos() * radius);
                    vertex(
                        out,
                        [px, surface(g, px, pz) + lift, pz],
                        up,
                        color(px, pz),
                        alpha,
                    );
                }
            }
            for i in 0..SIDES {
                let (inner, outer) = (base + 1 + i * 2, base + 2 + i * 2);
                let (next_inner, next_outer) = (
                    base + 1 + (i + 1) % SIDES * 2,
                    base + 2 + (i + 1) % SIDES * 2,
                );
                out.indices.extend([
                    base, inner, next_inner, inner, outer, next_inner, next_inner, outer,
                    next_outer,
                ]);
            }
        }
    }
    project_top(out, Layer::Path);
}

/// A rectangle `lo`..`hi` (metres) of `layer` that fades out over `border` metres outside it.
pub(crate) fn patch(
    layers: &mut Layers,
    g: &TerrainGrid,
    layer: Layer,
    color: u32,
    (lo, hi): ([f64; 2], [f64; 2]),
    border: f64,
) {
    let out = layers.entry(Ground::near(layer, true)).or_default();
    let lift = layer.lift();
    let (lo, hi) = (lo.map(|v| v - border), hi.map(|v| v + border));
    let cols = ((hi[0] - lo[0]) / 2.).ceil().max(1.) as usize;
    let rows = ((hi[1] - lo[1]) / 2.).ceil().max(1.) as usize;
    let base = out.p.len() as u32;
    for r in 0..=rows {
        for c in 0..=cols {
            let x = lo[0] + (hi[0] - lo[0]) * c as f64 / cols as f64;
            let z = lo[1] + (hi[1] - lo[1]) * r as f64 / rows as f64;
            let outside = |v: f64, a: f64, b: f64| (a + border - v).max(v - (b - border)).max(0.);
            let d = outside(x, lo[0], hi[0]).hypot(outside(z, lo[1], hi[1]));
            vertex(
                out,
                [x, surface(g, x, z) + lift, z],
                [0., 1., 0.],
                tinted(color, x, z, 3.),
                1. - d / border,
            );
        }
    }
    for r in 0..rows {
        for c in 0..cols {
            let a = base + (r * (cols + 1) + c) as u32;
            let b = a + cols as u32 + 1;
            out.indices.extend([a, b, a + 1, a + 1, b, b + 1]);
        }
    }
    project_top(out, layer);
}
