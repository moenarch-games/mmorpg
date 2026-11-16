//! Where grass grows, for the clients' grass fields: clumps of blades drawn around the camera
//! instead of the old upright tufts. Presentation only; gameplay never reads it.
//!
//! The terrain's own ground decides it. Grass grows on the turf the near terrain draws and thins
//! where its overlays (forest floor, gravel, mud, sand, paths and rock), its worn roads, plaza
//! and fields cover it; it stays out of water, river beds, terrain holes and the footprints of
//! everything that stands on the ground. Each clump's colour follows the ground's tone, with
//! lush and dry patches over a few metres.
//!
//! [`Grass::cell`] lays out one [`CELL_METRES`] square on a jittered lattice; its clumps come in
//! a random order, so any prefix of them is an even thinning that a client can draw at a
//! distance.
use mmorpg_scenery::{Rect, Scenery, TerrainGrid};

use crate::{
    StaticScene,
    batching::mix,
    geometry::hash,
    terrain::{ground_biomes, metres, noise, surface, tone},
};

/// The side of one grass cell, in metres.
pub const CELL_METRES: f64 = 8.;
/// Floats per clump in [`Grass::cell`]: position (metres), yaw (radians), height (metres),
/// linear RGB and its wildflower: 0 for none, else [`FLOWERS`] colour 1–4.
pub const CLUMP_FLOATS: usize = 9;
/// The wildflowers' colours: buttercup, daisy, harebell and poppy, in sRGB.
pub const FLOWERS: [u32; 4] = [0xf2cf3a, 0xf3efe2, 0x8f7ad6, 0xd9493a];
/// Roughly the spacing of the lattice clumps are jittered on, in metres.
const SPACING: f64 = 0.3;
/// Lattice points along a cell's side; the lattice step divides the cell exactly, so no seam shows.
const SIDE: i32 = (CELL_METRES / SPACING) as i32;
/// The richer green the blades lean towards, and the straw of their dry patches.
const LUSH: u32 = 0x5a8a34;
const DRY: u32 = 0xa49d56;

/// Grass cover over the terrain grid.
#[derive(Clone, Debug)]
pub struct Grass {
    grid: TerrainGrid,
    /// Per grid sample, how thickly grass grows (0–1) and its colour.
    density: Vec<f32>,
    tint: Vec<u32>,
    /// Ground nothing grows on: footprints of what stands there and terrain holes, in units.
    blocked: Vec<Rect>,
}

impl Grass {
    /// The cover over `scene`'s terrain, which was built from `s`. Built once per scene, apart
    /// from it, so a scene that draws no grass pays nothing for it.
    #[must_use]
    pub fn new(scene: &StaticScene, s: &Scenery) -> Self {
        Self::over(s, scene.terrain.grid(), &scene.terrain.grass_cover())
    }

    /// `cover` holds, per grid sample, how much the terrain's overlays and worn ground cover it.
    fn over(s: &Scenery, g: &TerrainGrid, cover: &[f64]) -> Self {
        let biomes = ground_biomes(g);
        let count = g.columns * g.rows;
        let mut density = vec![0.; count];
        let mut tint = vec![0; count];
        for i in 0..count {
            let (ux, uz) = (
                g.origin[0] + (i % g.columns) as i32 * g.step,
                g.origin[1] + (i / g.columns) as i32 * g.step,
            );
            let (x, z) = (metres(ux), metres(uz));
            let thick = match biomes[i].name() {
                "meadow" | "foothills" => 1.,
                "hub" | "hollow" => 0.85,
                "highland" => 0.55,
                "woods" => 0.4,
                _ => 0.,
            };
            let bare = matches!(g.biomes[i].name(), "lake bed" | "rock" | "snow");
            let water = s.water_distance([ux, uz]);
            let river = if s.greyhaven() {
                mmorpg_core::greyhaven_vale::design::river_distance(ux, uz).min(
                    if mmorpg_core::greyhaven_vale::brook::wet_ground(ux, uz) {
                        0
                    } else {
                        i64::MAX
                    },
                )
            } else {
                i64::MAX
            };
            let shore = |distance: i64| {
                if distance == i64::MAX {
                    1.
                } else {
                    ((distance as f64 / 100. - 0.4) / 1.6).clamp(0., 1.)
                }
            };
            density[i] = if bare {
                0.
            } else {
                (thick * (1. - cover[i]).clamp(0., 1.) * shore(water) * shore(river)) as f32
            };
            let lush = mix(tone(biomes[i], x, z), LUSH, 0.4);
            let dryness = ((noise(x, z, 18., 6101.) - 0.62) * 3.).clamp(0., 0.7);
            tint[i] = mix(lush, DRY, dryness);
        }
        let mut blocked: Vec<Rect> = s
            .props
            .iter()
            .filter(|prop| prop.collider.is_some())
            .map(|prop| prop.footprint().expanded(25))
            .collect();
        blocked.extend(s.collision_surfaces.iter().map(|mesh| {
            let bound = |axis: usize, pick: fn(i32, i32) -> i32| {
                mesh.vertices
                    .iter()
                    .map(|p| p[axis] + mesh.position[axis])
                    .reduce(pick)
                    .unwrap_or(mesh.position[axis])
            };
            Rect {
                min: [bound(0, i32::min), bound(2, i32::min)],
                max: [bound(0, i32::max), bound(2, i32::max)],
            }
            .expanded(25)
        }));
        blocked.extend(s.terrain_holes.iter().map(|hole| hole.expanded(100)));
        // The walk-in cottages (#843) have no house box, but nothing grows on their floors.
        if s.greyhaven() {
            use mmorpg_core::greyhaven_vale::cottages;
            blocked.extend(
                cottages::cottages()
                    .iter()
                    .map(|cottage| Rect::around(cottage.centre, cottages::HALF).expanded(25)),
            );
        }
        Self {
            grid: g.clone(),
            density,
            tint,
            blocked,
        }
    }

    /// Bilinear `field` at `(x, z)` metres.
    fn sample<T: Copy>(&self, field: &[T], x: f64, z: f64, lerp: impl Fn(T, T, f64) -> T) -> T {
        let g = &self.grid;
        let gx = ((x - metres(g.origin[0])) / metres(g.step)).clamp(0., (g.columns - 1) as f64);
        let gz = ((z - metres(g.origin[1])) / metres(g.step)).clamp(0., (g.rows - 1) as f64);
        let (c, r) = (
            (gx.floor() as usize).min(g.columns - 2),
            (gz.floor() as usize).min(g.rows - 2),
        );
        let (fx, fz) = (gx - c as f64, gz - r as f64);
        let at = |dc: usize, dr: usize| field[(r + dr) * g.columns + c + dc];
        let near = lerp(at(0, 0), at(1, 0), fx);
        let far = lerp(at(0, 1), at(1, 1), fx);
        lerp(near, far, fz)
    }

    /// The clumps of cell `(column, row)` (the square from `column × CELL_METRES`,
    /// `row × CELL_METRES`), [`CLUMP_FLOATS`] each, in a random order.
    #[must_use]
    pub fn cell(&self, column: i32, row: i32) -> Vec<f32> {
        let (x0, z0) = (
            f64::from(column) * CELL_METRES,
            f64::from(row) * CELL_METRES,
        );
        let units = |v: f64| (v * 100.).round() as i32;
        let area = Rect {
            min: [units(x0), units(z0)],
            max: [units(x0 + CELL_METRES), units(z0 + CELL_METRES)],
        };
        let blocked: Vec<_> = self
            .blocked
            .iter()
            .filter(|rect| {
                rect.min[0] <= area.max[0]
                    && rect.max[0] >= area.min[0]
                    && rect.min[1] <= area.max[1]
                    && rect.max[1] >= area.min[1]
            })
            .collect();
        let g = &self.grid;
        let (lo, hi) = (
            [metres(g.origin[0]), metres(g.origin[1])],
            [
                metres(g.origin[0] + (g.columns as i32 - 1) * g.step),
                metres(g.origin[1] + (g.rows as i32 - 1) * g.step),
            ],
        );
        let (side, step) = (SIDE, CELL_METRES / f64::from(SIDE));
        let mut clumps: Vec<(f64, [f32; CLUMP_FLOATS])> = Vec::new();
        for j in 0..side {
            for i in 0..side {
                let (gi, gj) = (f64::from(column * side + i), f64::from(row * side + j));
                let x = x0 + (f64::from(i) + 0.5 + 0.9 * (hash(&[gi, gj, 1.]) - 0.5)) * step;
                let z = z0 + (f64::from(j) + 0.5 + 0.9 * (hash(&[gi, gj, 2.]) - 0.5)) * step;
                if x < lo[0] || z < lo[1] || x > hi[0] || z > hi[1] {
                    continue;
                }
                // Patches over a few metres, so the cover gathers rather than spreading evenly.
                let patch = 0.7 + 0.3 * noise(x, z, 5., 6103.);
                let density =
                    f64::from(self.sample(&self.density, x, z, |a, b, t| a + (b - a) * t as f32))
                        * patch;
                if hash(&[gi, gj, 3.]) >= density {
                    continue;
                }
                let at = [units(x), units(z)];
                if blocked.iter().any(|rect| rect.distance(at) == 0) {
                    continue;
                }
                let tint = self.sample(&self.tint, x, z, mix);
                let tint = mix(tint, 0x000000, 0.12 * hash(&[gi, gj, 4.]));
                let linear = |shift: u32| {
                    let c = f64::from((tint >> shift) & 255) / 255.;
                    (if c <= 0.04045 {
                        c / 12.92
                    } else {
                        ((c + 0.055) / 1.055).powf(2.4)
                    }) as f32
                };
                // Wildflowers in drifts of one colour over several metres, on thick turf only.
                let drift = noise(x, z, 7., 6109.);
                let flower = if density > 0.5
                    && drift > 0.55
                    && hash(&[gi, gj, 8.]) < (drift - 0.55) * 1.6
                {
                    1. + (noise(x, z, 23., 6111.) * 4.).floor().min(3.)
                } else {
                    0.
                };
                // Tall meadow grass in drifts, shorter turf between them.
                let height =
                    (0.22 + 0.42 * noise(x, z, 9., 6107.)) * (0.75 + 0.5 * hash(&[gi, gj, 5.]));
                clumps.push((
                    hash(&[gi, gj, 6.]),
                    [
                        x as f32,
                        surface(g, x, z) as f32,
                        z as f32,
                        (hash(&[gi, gj, 7.]) * std::f64::consts::TAU) as f32,
                        (height * density.sqrt().max(0.5)) as f32,
                        linear(16),
                        linear(8),
                        linear(0),
                        flower as f32,
                    ],
                ));
            }
        }
        clumps.sort_by(|a, b| a.0.total_cmp(&b.0));
        clumps.into_iter().flat_map(|(_, clump)| clump).collect()
    }
}
