//! Visual terrain tiles with two levels of detail (#526).
//!
//! The terrain grid is cut into square tiles of [`TILE_CELLS`] grid cells (64 m). Each tile has
//! a **near** level, the textured ground of #525 (the opaque grass base and its masked overlays
//! over the tile's cells, built on demand by [`Tiles::near`]), and a **far** level built once
//! for every tile: the relief at every second grid sample, its vertex colours the ground
//! layers, roads, the plaza and the fields baked by their weights ([`Ground::far`]: flat,
//! untextured, so distant ground neither samples textures nor shows their repeats). The far
//! levels together are the coarse terrain that is always resident; merged into chunks
//! ([`Tiles::far_cover`]) they cost a few dozen draws.
//!
//! Every level hangs a skirt below each tile edge, as deep as the coarse edge strays from the
//! fine one, so a near tile beside a far one shows no crack. A [`Streamer`] keeps the near
//! levels around the camera resident: it builds missing ones nearest first, a few per frame, and
//! evicts the ones left behind; a tile shows its near level only once that is resident, so a
//! tile is never blank. Tiles that hold a terrain hole (a platform's footprint) always draw near:
//! the coarse level would cover the hole.
//!
//! Presentation only: the authoritative ground is `mmorpg-core`'s physics; nothing here
//! changes a collider.
use std::collections::BTreeMap;
use std::sync::Arc;

use mmorpg_scenery::{Biome, Scenery, TerrainGrid};

use crate::batching::{material, matrix, mix, rgb};
use crate::geometry::{Geometry, ID, V, norm};
use crate::ground::{self, Ground, Layer, Layers, Weights};
use crate::terrain::{self, height, metres, overlaps};
use crate::{Draw, Material};

/// Grid cells along a tile's side: 64 m of four-metre cells.
pub const TILE_CELLS: usize = 16;
/// A tile draws its near level within this camera distance (metres, to its bounds).
pub const NEAR_METRES: f64 = 160.;
/// A near tile stays near until this much farther, so a tile on the edge does not flicker.
const NEAR_HYSTERESIS_METRES: f64 = 16.;
/// Near levels are built ahead of need within this distance.
pub const PREFETCH_METRES: f64 = 208.;
/// Near levels beyond this distance are evicted.
pub const EVICT_METRES: f64 = 256.;
/// Near levels a frame builds at most, once the first frame's are resident.
pub const BUILDS_PER_FRAME: usize = 4;
/// The far level samples every second grid sample (eight metres).
const FAR_STRIDE: usize = 2;
/// Far levels merge into chunks of this many tiles a side, smallest first ([`Tiles::far_cover`]).
pub const CHUNK_SIDES: [usize; 2] = [2, 4];

/// Several tiles' far levels merged into one draw per ground.
#[derive(Clone, Debug)]
pub struct FarChunk {
    /// Tiles a side.
    pub side: usize,
    /// The tiles it covers.
    pub tiles: Vec<usize>,
    pub far: Vec<Draw>,
}

/// Appends `part` to `out`, both with the same vertex attributes.
fn append(out: &mut Geometry, part: &Geometry) {
    let base = out.p.len() as u32;
    out.p.extend_from_slice(&part.p);
    out.n.extend_from_slice(&part.n);
    out.colors.extend_from_slice(&part.colors);
    out.uv.extend_from_slice(&part.uv);
    out.tangents.extend_from_slice(&part.tangents);
    out.alpha.extend_from_slice(&part.alpha);
    out.indices.extend(part.indices.iter().map(|i| base + i));
}

/// Which level a tile draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Near,
    Far,
}

/// One tile's bounds and far level.
#[derive(Clone, Debug)]
pub struct Tile {
    pub column: usize,
    pub row: usize,
    /// Grid cells `[first column, first row, end column, end row]`.
    cells: [usize; 4],
    /// Bounds in metres, skirts and overlays included.
    pub min: V,
    pub max: V,
    /// The tile holds a terrain hole, so it always draws its near level.
    pub always_near: bool,
    /// How far the skirts hang below the tile's edges, in metres.
    skirt: f64,
    /// The far level's draws.
    pub far: Vec<Draw>,
    /// The tile's road and patch overlay pieces on its near level (#424, #525).
    extras: Vec<(Layer, Arc<Geometry>)>,
}

/// The terrain's tiles and what their near levels are built from.
#[derive(Clone, Debug)]
pub struct Tiles {
    /// Tiles across and along the grid.
    pub columns: usize,
    pub rows: usize,
    /// Row-major.
    pub tiles: Vec<Tile>,
    /// The far levels merged into square chunks of 2 and 4 tiles a side ([`Self::far_cover`]),
    /// so the distant terrain costs a few dozen draws, not one per tile.
    pub chunks: Vec<Vec<FarChunk>>,
    /// Prefixes the near levels' IDs and resource keys (`StaticScene::prefix`).
    prefix: String,
    grid: TerrainGrid,
    biomes: Vec<Biome>,
    weights: Weights,
    /// Per grid sample, how much a road, the plaza or a field covers it from afar, and its colour.
    worn: Vec<(f64, u32)>,
    holes: Vec<[f64; 4]>,
}

impl Tiles {
    pub(crate) fn new(s: &Scenery, g: &TerrainGrid) -> Self {
        let holes: Vec<_> = s
            .terrain_holes
            .iter()
            .map(|hole| [hole.min[0], hole.min[1], hole.max[0], hole.max[1]].map(metres))
            .collect();
        let weights = ground::weights(s, g);
        let biomes = terrain::ground_biomes(g);
        // The road and patch overlays, cut into tiles by their triangles' centres.
        let mut extras = Layers::new();
        if let Some(plaza) = terrain::bounds(g, "plaza") {
            ground::patch(&mut extras, g, Layer::Path, 0xa8987a, plaza, 1.2);
        }
        for field in s.farmland() {
            let field = (field.min.map(metres), field.max.map(metres));
            ground::patch(&mut extras, g, Layer::Mud, 0x7a5836, field, 1.);
        }
        ground::roads(&mut extras, s, g);
        let (cells_x, cells_z) = (g.columns - 1, g.rows - 1);
        let (columns, rows) = (cells_x.div_ceil(TILE_CELLS), cells_z.div_ceil(TILE_CELLS));
        let size = metres(g.step) * TILE_CELLS as f64;
        let origin = [metres(g.origin[0]), metres(g.origin[1])];
        let mut pieces: BTreeMap<(usize, Layer), Geometry> = BTreeMap::new();
        for (ground, geometry) in &extras {
            for tri in geometry.indices.as_chunks::<3>().0 {
                let centre = |axis: usize| {
                    tri.iter()
                        .map(|&i| geometry.p[i as usize][axis])
                        .sum::<f64>()
                        / 3.
                };
                let column =
                    (((centre(0) - origin[0]) / size).floor().max(0.) as usize).min(columns - 1);
                let row = (((centre(2) - origin[1]) / size).floor().max(0.) as usize).min(rows - 1);
                let piece = pieces
                    .entry((row * columns + column, ground.layer))
                    .or_default();
                let mut remap = BTreeMap::new();
                for &i in tri {
                    let index = *remap.entry(i).or_insert_with(|| {
                        let i = i as usize;
                        piece.p.push(geometry.p[i]);
                        piece.n.push(geometry.n[i]);
                        piece.colors.push(geometry.colors[i]);
                        piece.uv.push(geometry.uv[i]);
                        piece.tangents.push(geometry.tangents[i]);
                        piece.alpha.push(geometry.alpha[i]);
                        piece.p.len() as u32 - 1
                    });
                    piece.indices.push(index);
                }
            }
        }
        let mut tiles = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            for column in 0..columns {
                let cells = [
                    column * TILE_CELLS,
                    row * TILE_CELLS,
                    ((column + 1) * TILE_CELLS).min(cells_x),
                    ((row + 1) * TILE_CELLS).min(cells_z),
                ];
                let rect = [
                    metres(g.origin[0] + cells[0] as i32 * g.step),
                    metres(g.origin[1] + cells[1] as i32 * g.step),
                    metres(g.origin[0] + cells[2] as i32 * g.step),
                    metres(g.origin[1] + cells[3] as i32 * g.step),
                ];
                let index = row * columns + column;
                let extras: Vec<_> = Layer::ALL
                    .iter()
                    .filter_map(|&layer| {
                        pieces
                            .remove(&(index, layer))
                            .map(|piece| (layer, Arc::new(piece)))
                    })
                    .collect();
                let skirt = skirt_depth(g, cells);
                let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
                for r in cells[1]..=cells[3] {
                    for c in cells[0]..=cells[2] {
                        let h = metres(g.heights[r * g.columns + c]);
                        low = low.min(h);
                        high = high.max(h);
                    }
                }
                for (_, piece) in &extras {
                    for p in &piece.p {
                        low = low.min(p[1]);
                        high = high.max(p[1]);
                    }
                }
                tiles.push(Tile {
                    column,
                    row,
                    cells,
                    min: [rect[0], low - skirt, rect[1]],
                    max: [rect[2], high + 0.1, rect[3]],
                    always_near: holes.iter().any(|hole| overlaps(*hole, rect)),
                    skirt,
                    far: Vec::new(),
                    extras,
                });
            }
        }
        let mut built = Self {
            columns,
            rows,
            tiles,
            chunks: Vec::new(),
            prefix: "scenery".into(),
            grid: g.clone(),
            biomes,
            weights,
            worn: worn(s, g),
            holes,
        };
        for index in 0..built.tiles.len() {
            built.tiles[index].far = built.far(index);
        }
        built.chunks = CHUNK_SIDES
            .iter()
            .map(|&side| built.merge_far(side))
            .collect();
        built
    }

    /// The far levels merged into chunks of `side` × `side` tiles, row-major.
    fn merge_far(&self, side: usize) -> Vec<FarChunk> {
        let (columns, rows) = (self.columns.div_ceil(side), self.rows.div_ceil(side));
        let mut chunks = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            for column in 0..columns {
                let tiles: Vec<usize> = (row * side..((row + 1) * side).min(self.rows))
                    .flat_map(|r| {
                        (column * side..((column + 1) * side).min(self.columns))
                            .map(move |c| r * self.columns + c)
                    })
                    .collect();
                let mut merged: BTreeMap<Ground, Geometry> = BTreeMap::new();
                for &tile in &tiles {
                    for draw in &self.tiles[tile].far {
                        let ground = draw.material.ground.expect("far draws are ground");
                        append(merged.entry(ground).or_default(), &draw.geometry);
                    }
                }
                let far = merged
                    .into_iter()
                    .map(|(ground, geometry)| {
                        let key =
                            format!("scenery:terrain-far{side}-{column}-{row}:{}", ground.name());
                        Draw {
                            id: key.clone(),
                            resource_key: key,
                            geometry: Arc::new(geometry),
                            model: matrix(ID),
                            material: Material {
                                ground: Some(ground),
                                ..material(0xffffff, false)
                            },
                        }
                    })
                    .collect();
                chunks.push(FarChunk { side, tiles, far });
            }
        }
        chunks
    }

    /// Where the far levels draw while `near` tiles draw their near level: the fewest chunks
    /// (`(level, index)` into [`Self::chunks`]) and single tiles (`(CHUNK_SIDES.len(), tile)`)
    /// that cover every other tile, each chunk as large as holds no near tile.
    #[must_use]
    pub fn far_cover(&self, near: &dyn Fn(usize) -> bool) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let top = CHUNK_SIDES.len() - 1;
        for index in 0..self.chunks[top].len() {
            self.cover(top, index, near, &mut out);
        }
        out
    }

    fn cover(
        &self,
        level: usize,
        index: usize,
        near: &dyn Fn(usize) -> bool,
        out: &mut Vec<(usize, usize)>,
    ) {
        let chunk = &self.chunks[level][index];
        if !chunk.tiles.iter().any(|&tile| near(tile)) {
            out.push((level, index));
        } else if level == 0 {
            out.extend(
                chunk
                    .tiles
                    .iter()
                    .filter(|&&tile| !near(tile))
                    .map(|&tile| (CHUNK_SIDES.len(), tile)),
            );
        } else {
            // The chunks of the level below inside this one.
            let (side, child) = (CHUNK_SIDES[level], CHUNK_SIDES[level - 1]);
            let per_row = self.columns.div_ceil(side);
            let child_row = self.columns.div_ceil(child);
            let (column, row) = (index % per_row, index / per_row);
            let ratio = side / child;
            for r in row * ratio..((row + 1) * ratio).min(self.rows.div_ceil(child)) {
                for c in column * ratio..((column + 1) * ratio).min(child_row) {
                    self.cover(level - 1, r * child_row + c, near, out);
                }
            }
        }
    }

    /// The near levels' identity input beyond the far draws: the grid and the layer weights.
    pub(crate) fn identity_values(&self) -> impl Iterator<Item = f64> + '_ {
        self.grid
            .heights
            .iter()
            .map(|&h| f64::from(h))
            .chain(self.weights.values())
            .chain(self.holes.iter().flatten().copied())
    }

    /// Prefixes every ID and resource key with `prefix` (`StaticScene::prefix`).
    pub(crate) fn set_prefix(&mut self, prefix: &str) {
        for draw in self.tiles.iter_mut().flat_map(|tile| &mut tile.far).chain(
            self.chunks
                .iter_mut()
                .flatten()
                .flat_map(|chunk| &mut chunk.far),
        ) {
            draw.resource_key = draw.resource_key.replacen("scenery", prefix, 1);
            draw.id = draw.id.replacen("scenery", prefix, 1);
        }
        prefix.clone_into(&mut self.prefix);
    }

    /// The rendered terrain grid.
    pub(crate) fn grid(&self) -> &TerrainGrid {
        &self.grid
    }

    /// Camera distance from `eye` (metres) to tile `index`'s bounds.
    #[must_use]
    pub fn distance(&self, index: usize, eye: [f64; 3]) -> f64 {
        let tile = &self.tiles[index];
        let d: Vec<f64> = (0..3)
            .map(|axis| {
                (tile.min[axis] - eye[axis])
                    .max(eye[axis] - tile.max[axis])
                    .max(0.)
            })
            .collect();
        d[0].hypot(d[1]).hypot(d[2])
    }

    fn draw(&self, index: usize, name: &str, geometry: Arc<Geometry>, ground: Ground) -> Draw {
        let tile = &self.tiles[index];
        let key = format!(
            "{}:terrain-{}-{}:{name}",
            self.prefix, tile.column, tile.row
        );
        Draw {
            id: key.clone(),
            resource_key: key,
            geometry,
            model: matrix(ID),
            material: Material {
                ground: Some(ground),
                ..material(0xffffff, false)
            },
        }
    }

    /// Tile `index`'s near level: the textured grass base and its overlays over the tile's
    /// cells (#525), with skirts, and the tile's road and patch overlays.
    #[must_use]
    pub fn near(&self, index: usize) -> Vec<Draw> {
        let tile = &self.tiles[index];
        let g = &self.grid;
        let mut layers = Layers::new();
        let base = layers.entry(Ground::near(Layer::Grass, false)).or_default();
        terrain::grid(
            base,
            g,
            &self.biomes,
            false,
            terrain::extent(g),
            &self.holes,
            &|tri| self.weights.covers(tri),
            tile.cells,
        );
        skirts(
            base,
            g,
            tile.cells,
            1,
            tile.skirt,
            &self.holes,
            &|sample, x, z| terrain::tone(self.biomes[sample], x, z),
        );
        ground::project_top(base, Layer::Grass);
        ground::grid_overlays(&mut layers, &self.weights, g, &self.holes, tile.cells);
        let mut draws: Vec<_> = layers
            .into_iter()
            .filter(|(_, geometry)| !geometry.indices.is_empty())
            .map(|(ground, geometry)| self.draw(index, &ground.name(), Arc::new(geometry), ground))
            .collect();
        draws.extend(tile.extras.iter().map(|(layer, piece)| {
            self.draw(
                index,
                &format!("extra-{}", layer.name()),
                Arc::clone(piece),
                Ground::near(*layer, true),
            )
        }));
        draws
    }

    /// Tile `index`'s far level: the relief every [`FAR_STRIDE`] samples with the ground layers,
    /// roads and patches baked into its vertex colours, with skirts, drawn flat.
    fn far(&self, index: usize) -> Vec<Draw> {
        let tile = &self.tiles[index];
        let g = &self.grid;
        let [c0, r0, c1, r1] = tile.cells;
        let step = metres(g.step) * FAR_STRIDE as f64;
        let samples = |lo: usize, hi: usize| -> Vec<usize> {
            let mut out: Vec<usize> = (lo..=hi).step_by(FAR_STRIDE).collect();
            if out.last() != Some(&hi) {
                out.push(hi);
            }
            out
        };
        let (columns, rows) = (samples(c0, c1), samples(r0, r1));
        let color = |sample: usize, x: f64, z: f64| {
            let (cover, worn) = self.worn[sample];
            mix(
                self.weights
                    .baked(sample, terrain::tone(self.biomes[sample], x, z)),
                worn,
                cover,
            )
        };
        let mut out = Geometry::default();
        for &r in &rows {
            for &c in &columns {
                let sample = r * g.columns + c;
                let (x, z) = (
                    metres(g.origin[0] + c as i32 * g.step),
                    metres(g.origin[1] + r as i32 * g.step),
                );
                let (ci, ri, s) = (c as isize, r as isize, FAR_STRIDE as isize);
                let dx = (height(g, ci + s, ri) - height(g, ci - s, ri)) / (2. * step);
                let dz = (height(g, ci, ri + s) - height(g, ci, ri - s)) / (2. * step);
                out.p.push([x, metres(g.heights[sample]), z]);
                out.n.push(norm([-dx, 1., -dz]));
                out.colors.push(rgb(color(sample, x, z)).map(f64::from));
            }
        }
        let across = columns.len() as u32;
        for r in 0..rows.len() as u32 - 1 {
            for c in 0..across - 1 {
                let a = r * across + c;
                out.indices
                    .extend([a, a + across, a + 1, a + 1, a + across, a + across + 1]);
            }
        }
        skirts(&mut out, g, tile.cells, FAR_STRIDE, tile.skirt, &[], &color);
        let base = Ground {
            layer: Layer::Grass,
            overlay: false,
            far: true,
        };
        vec![self.draw(index, &base.name(), Arc::new(out), base)]
    }
}

/// How much the roads, the plaza and the ploughed fields cover each grid sample from afar, and
/// their colours (#526): the far levels bake them in, widened over [`WORN_FADE_METRES`] so a
/// road a few metres wide does not break into dots between eight-metre samples.
fn worn(s: &Scenery, g: &TerrainGrid) -> Vec<(f64, u32)> {
    const WORN_FADE_METRES: f64 = 6.;
    let mut out = vec![(0., 0); g.columns * g.rows];
    let step = metres(g.step);
    let origin = [metres(g.origin[0]), metres(g.origin[1])];
    // Covers every sample within `reach` of the box `lo`..`hi` by `cover(x, z)`, keeping the most.
    let mut paint = |lo: [f64; 2], hi: [f64; 2], color: u32, cover: &dyn Fn(f64, f64) -> f64| {
        let range = |axis: usize, count: usize| {
            let at = |v: f64| ((v - origin[axis]) / step).clamp(0., (count - 1) as f64);
            at(lo[axis]).floor() as usize..=at(hi[axis]).ceil() as usize
        };
        for r in range(1, g.rows) {
            for c in range(0, g.columns) {
                let (x, z) = (origin[0] + c as f64 * step, origin[1] + r as f64 * step);
                let value = cover(x, z);
                let cell = &mut out[r * g.columns + c];
                if value > cell.0 {
                    *cell = (value, color);
                }
            }
        }
    };
    let fade = |inside: f64, d: f64| 0.9 * (1. - ((d - inside) / WORN_FADE_METRES).clamp(0., 1.));
    let rect = |lo: [f64; 2], hi: [f64; 2]| {
        move |x: f64, z: f64| {
            let dx = (lo[0] - x).max(x - hi[0]).max(0.);
            let dz = (lo[1] - z).max(z - hi[1]).max(0.);
            dx.hypot(dz)
        }
    };
    let grow = |lo: [f64; 2], hi: [f64; 2], by: f64| (lo.map(|v| v - by), hi.map(|v| v + by));
    if let Some((lo, hi)) = terrain::bounds(g, "plaza") {
        let (glo, ghi) = grow(lo, hi, WORN_FADE_METRES);
        paint(glo, ghi, 0xa8987a, &|x, z| fade(0., rect(lo, hi)(x, z)));
    }
    for field in s.farmland() {
        let (lo, hi) = (field.min.map(metres), field.max.map(metres));
        let (glo, ghi) = grow(lo, hi, WORN_FADE_METRES);
        paint(glo, ghi, 0x7a5836, &|x, z| fade(0., rect(lo, hi)(x, z)));
    }
    for road in &s.roads {
        let width = metres(road.half_width);
        for pair in road.points.windows(2) {
            let (a, b) = (pair[0].map(metres), pair[1].map(metres));
            let reach = width + WORN_FADE_METRES;
            let lo = [a[0].min(b[0]) - reach, a[1].min(b[1]) - reach];
            let hi = [a[0].max(b[0]) + reach, a[1].max(b[1]) + reach];
            paint(lo, hi, 0xa4865c, &|x, z| {
                let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
                let length = dx * dx + dz * dz;
                let t = if length == 0. {
                    0.
                } else {
                    (((x - a[0]) * dx + (z - a[1]) * dz) / length).clamp(0., 1.)
                };
                fade(width, (x - a[0] - t * dx).hypot(z - a[1] - t * dz))
            });
        }
    }
    out
}

/// How deep a tile's skirts hang: the most its coarse edges stray from the fine ones, and a
/// margin.
fn skirt_depth(g: &TerrainGrid, [c0, r0, c1, r1]: [usize; 4]) -> f64 {
    let h = |c: usize, r: usize| metres(g.heights[r * g.columns + c]);
    let mut stray: f64 = 0.;
    let mut edge = |at: &dyn Fn(usize) -> f64, lo: usize, hi: usize| {
        let mut i = lo;
        while i + FAR_STRIDE <= hi {
            let line = (at(i) + at(i + FAR_STRIDE)) / 2.;
            for j in i + 1..i + FAR_STRIDE {
                stray = stray.max((at(j) - line).abs());
            }
            i += FAR_STRIDE;
        }
    };
    edge(&|c| h(c, r0), c0, c1);
    edge(&|c| h(c, r1), c0, c1);
    edge(&|r| h(c0, r), r0, r1);
    edge(&|r| h(c1, r), r0, r1);
    stray + 0.5
}

/// Hangs a skirt `depth` metres below each edge of the cells `cells`, through every `stride`-th
/// grid sample, facing both ways and lit like the ground above it; `color` gives a sample's
/// colour. Segments in a terrain hole are left out.
fn skirts(
    out: &mut Geometry,
    g: &TerrainGrid,
    [c0, r0, c1, r1]: [usize; 4],
    stride: usize,
    depth: f64,
    holes: &[[f64; 4]],
    color: &dyn Fn(usize, f64, f64) -> u32,
) {
    let along = |lo: usize, hi: usize| -> Vec<usize> {
        let mut out: Vec<usize> = (lo..=hi).step_by(stride).collect();
        if out.last() != Some(&hi) {
            out.push(hi);
        }
        out
    };
    let step = metres(g.step) * stride as f64;
    let s = stride as isize;
    let sides: [Vec<(usize, usize)>; 4] = [
        along(c0, c1).into_iter().map(|c| (c, r0)).collect(),
        along(c0, c1).into_iter().map(|c| (c, r1)).collect(),
        along(r0, r1).into_iter().map(|r| (c0, r)).collect(),
        along(r0, r1).into_iter().map(|r| (c1, r)).collect(),
    ];
    for side in sides {
        let base = out.p.len() as u32;
        for &(c, r) in &side {
            let sample = r * g.columns + c;
            let (x, z) = (
                metres(g.origin[0] + c as i32 * g.step),
                metres(g.origin[1] + r as i32 * g.step),
            );
            let (ci, ri) = (c as isize, r as isize);
            let dx = (height(g, ci + s, ri) - height(g, ci - s, ri)) / (2. * step);
            let dz = (height(g, ci, ri + s) - height(g, ci, ri - s)) / (2. * step);
            let n = norm([-dx, 1., -dz]);
            let y = metres(g.heights[sample]);
            let c = rgb(color(sample, x, z)).map(f64::from);
            for y in [y, y - depth] {
                out.p.push([x, y, z]);
                out.n.push(n);
                out.colors.push(c);
            }
        }
        for (k, pair) in side.windows(2).enumerate() {
            let mid = |axis: usize| {
                let point = |(c, r): (usize, usize)| {
                    if axis == 0 {
                        metres(g.origin[0] + c as i32 * g.step)
                    } else {
                        metres(g.origin[1] + r as i32 * g.step)
                    }
                };
                (point(pair[0]) + point(pair[1])) / 2.
            };
            let (x, z) = (mid(0), mid(1));
            if holes
                .iter()
                .any(|hole| hole[0] <= x && x <= hole[2] && hole[1] <= z && z <= hole[3])
            {
                continue;
            }
            let (top, bottom) = (base + 2 * k as u32, base + 2 * k as u32 + 1);
            let (next_top, next_bottom) = (top + 2, bottom + 2);
            out.indices.extend([
                top,
                bottom,
                next_top,
                next_top,
                bottom,
                next_bottom,
                top,
                next_top,
                bottom,
                next_top,
                next_bottom,
                bottom,
            ]);
        }
    }
}

/// Keeps the near levels around the camera resident (#526): `R` is what a client keeps per
/// resident near level (its lowered draws, or nothing when the renderer keeps them).
#[derive(Clone, Debug)]
pub struct Streamer<R> {
    resident: BTreeMap<usize, R>,
    /// Per tile, whether it wants its near level (with hysteresis).
    wanted: Vec<bool>,
}

impl<R> Default for Streamer<R> {
    fn default() -> Self {
        Self {
            resident: BTreeMap::new(),
            wanted: Vec::new(),
        }
    }
}

impl<R> Streamer<R> {
    /// Advances residency for the camera `eye` (metres): evicts near levels beyond
    /// [`EVICT_METRES`] and builds missing ones within [`PREFETCH_METRES`], nearest first, at
    /// most `budget` (on the first update every wanted one, so the first frame is complete).
    /// `build` makes a tile's resident value; returns the evicted tiles and their values.
    pub fn update(
        &mut self,
        tiles: &Tiles,
        eye: [f64; 3],
        budget: usize,
        mut build: impl FnMut(usize) -> R,
    ) -> Vec<(usize, R)> {
        if self.wanted.len() != tiles.tiles.len() {
            self.wanted = vec![false; tiles.tiles.len()];
        }
        let first = self.resident.is_empty();
        let mut evicted = Vec::new();
        let mut missing = Vec::new();
        for (index, tile) in tiles.tiles.iter().enumerate() {
            let distance = tiles.distance(index, eye);
            let reach = NEAR_METRES
                + if self.wanted[index] {
                    NEAR_HYSTERESIS_METRES
                } else {
                    0.
                };
            self.wanted[index] = tile.always_near || distance <= reach;
            if !tile.always_near && distance > EVICT_METRES {
                if let Some(value) = self.resident.remove(&index) {
                    evicted.push((index, value));
                }
            } else if !self.resident.contains_key(&index)
                && (tile.always_near || distance <= PREFETCH_METRES)
            {
                let urgent = self.wanted[index];
                missing.push((!urgent, distance, index));
            }
        }
        missing.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let limit = if first {
            missing
                .iter()
                .filter(|(later, ..)| !later)
                .count()
                .max(budget)
        } else {
            budget
        };
        for &(_, _, index) in missing.iter().take(limit) {
            self.resident.insert(index, build(index));
        }
        evicted
    }

    /// The level tile `index` draws: near once it wants and has its near level.
    #[must_use]
    pub fn level(&self, index: usize) -> Level {
        if self.wanted.get(index).copied().unwrap_or(false) && self.resident.contains_key(&index) {
            Level::Near
        } else {
            Level::Far
        }
    }

    /// Tile `index`'s resident value, if its near level is resident.
    #[must_use]
    pub fn resident(&self, index: usize) -> Option<&R> {
        self.resident.get(&index)
    }

    /// How many near levels are resident.
    #[must_use]
    pub fn resident_count(&self) -> usize {
        self.resident.len()
    }
}
