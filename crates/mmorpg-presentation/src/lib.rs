//! Shared procedural presentation for Greyhaven Vale (#184).
//!
//! One implementation builds the static world both clients draw: vertex-coloured
//! terrain and its far ring, plaza/farmland patches, road ribbons, lake beds and
//! water, and a low-poly model for every scenery prop. The native client lowers
//! these draws to wgpu meshes; `mmorpg-wasm` exports the same draws to the
//! browser renderer. Geometry is built once per scenery value and never copied
//! per frame: [`StaticScene::frame`] only selects visible groups and
//! [`StaticScene::motion`] only computes the animated parts' transforms.
//!
//! This is presentation: it reads `mmorpg-scenery`, never feeds gameplay, and
//! network hosts never depend on it.
#![forbid(unsafe_code)]

pub mod ambient;
pub mod batching;
pub mod buildings;
pub mod geometry;
pub mod outpost_kit;
mod props;
pub mod settlement_props;
pub mod shrine;
pub mod structures;
mod terrain;
pub mod units;

use std::sync::Arc;

use geometry::{Affine, Geometry, compose, ry, rz, scale, translate};
use mmorpg_scenery::{Scenery, TerrainGrid};

pub use batching::Cull;

/// Terrain samples are four metres apart.
pub const TERRAIN_STEP_UNITS: i32 = 400;

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    /// Linear 0–1 channels of the authored sRGB colour.
    pub color: [f32; 3],
    pub emissive: [f32; 3],
    pub opacity: f32,
    pub unlit: bool,
    pub double_sided: bool,
}

/// One draw: immutable shared geometry, a column-major model matrix and a material.
#[derive(Clone, Debug)]
pub struct Draw {
    pub id: String,
    /// Stable identity of `geometry`; equal keys always carry equal geometry.
    pub resource_key: String,
    pub geometry: Arc<Geometry>,
    pub model: [f32; 16],
    pub material: Material,
}

/// Draws hidden together beyond their cull class's camera distance.
#[derive(Clone, Debug)]
pub struct Group {
    pub draws: Vec<Draw>,
    pub cull: Cull,
    /// Whether the group shares one geometry across many instance transforms.
    pub instanced: bool,
    /// XZ bounds of the group's vertices, in metres.
    pub min: [f64; 2],
    pub max: [f64; 2],
}

impl Group {
    /// Whether the group is within its class's distance of the camera eye (XZ, metres).
    #[must_use]
    pub fn visible(&self, eye: [f32; 3]) -> bool {
        let centre = [
            (self.min[0] + self.max[0]) / 2.,
            (self.min[1] + self.max[1]) / 2.,
        ];
        let radius = (self.max[0] - self.min[0]).hypot(self.max[1] - self.min[1]) / 2.;
        (centre[0] - f64::from(eye[0])).hypot(centre[1] - f64::from(eye[2])) - radius
            <= self.cull.reach()
    }
}

/// How an animated part moves.
#[derive(Clone, Copy, Debug)]
pub enum Motion {
    /// Windmill sails turn about their hub.
    Sail { hub: [f64; 3], yaw: f64 },
    /// A campfire flame flickers and turns.
    Flame { base: [f64; 3], seed: f64 },
    /// Reeds sway, sheared about the ground.
    Reed { phase: f64, ground_y: f64 },
    /// A cave roof (#159) stands at `at` (metres) and is cut away while the viewer's feet are
    /// under it: inside the XZ rect `[min, max]` and below its base.
    Roof {
        at: [f64; 3],
        min: [f64; 2],
        max: [f64; 2],
    },
    /// The procedural stand-in for an asset-tooling `kit`'s models, standing at `frame`: drawn
    /// only while the client does not draw the kit (not yet loaded, or failed to load).
    Fallback { frame: Affine, kit: Kit },
}

/// An asset-tooling bundle the clients draw over a procedural fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kit {
    /// The Outpost kit buildings (#420); their blockouts are the fallback.
    Outpost,
    /// The market square's settlement props (#421); boxes of their collision proxies are the
    /// fallback.
    SettlementProps,
}

/// Which kits a client draws; the static scene hides a drawn kit's fallback.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Kits {
    pub outpost: bool,
    pub settlement_props: bool,
}

impl Kits {
    /// Every kit drawn: the native client embeds them all.
    pub const ALL: Self = Self {
        outpost: true,
        settlement_props: true,
    };

    #[must_use]
    pub const fn draws(self, kit: Kit) -> bool {
        match kit {
            Kit::Outpost => self.outpost,
            Kit::SettlementProps => self.settlement_props,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Animated {
    /// Geometry in the part's own frame; [`StaticScene::motion`] places it.
    pub draws: Vec<Draw>,
    pub motion: Motion,
}

/// Everything static both clients draw for one scenery value.
#[derive(Clone, Debug)]
pub struct StaticScene {
    /// `scenery:<content revision>:<geometry hash>`; prefixes every ID and resource key.
    pub prefix: String,
    pub groups: Vec<Group>,
    pub water: Vec<Draw>,
    pub animated: Vec<Animated>,
    grid: TerrainGrid,
}

impl StaticScene {
    #[must_use]
    pub fn new(scenery: &Scenery) -> Self {
        let grid = scenery.terrain_grid(TERRAIN_STEP_UNITS);
        let mut batcher = batching::Batcher::default();
        let mut water = terrain::build(&mut batcher, scenery, &grid);
        let mut animated = props::build(&mut batcher, scenery, &grid);
        let mut groups = batcher.finish();
        let hash = identity(
            groups
                .iter()
                .flat_map(|group| &group.draws)
                .chain(&water)
                .chain(animated.iter().flat_map(|part| &part.draws)),
        );
        let prefix = format!("scenery:{}:{hash:016x}", scenery.content_revision);
        for draw in groups
            .iter_mut()
            .flat_map(|group| &mut group.draws)
            .chain(&mut water)
            .chain(animated.iter_mut().flat_map(|part| &mut part.draws))
        {
            draw.resource_key = draw.resource_key.replacen("scenery", &prefix, 1);
            draw.id = draw.id.replacen("scenery", &prefix, 1);
        }
        Self {
            prefix,
            groups,
            water,
            animated,
            grid,
        }
    }

    /// Height of the rendered terrain triangles at `(x, z)` metres, in metres.
    #[must_use]
    pub fn surface_y(&self, x: f64, z: f64) -> f64 {
        terrain::surface(&self.grid, x, z)
    }

    /// Model matrices of the animated parts at a cosmetic clock, one per
    /// [`Self::animated`] entry; without motion every part holds its rest pose.
    #[must_use]
    pub fn motion(&self, seconds: f64, animate: bool) -> Vec<[f32; 16]> {
        let t = if animate && seconds.is_finite() {
            seconds
        } else {
            0.
        };
        self.animated
            .iter()
            .map(|part| batching::matrix(motion(part.motion, t, animate)))
            .collect()
    }

    /// Whether each animated part is drawn for the `viewer`'s feet (metres), one per
    /// [`Self::animated`] entry: a cave roof is cut away while the viewer stands under it, and
    /// a kit's fallback ([`Motion::Fallback`]) is hidden while the client draws that kit
    /// (`kits`).
    #[must_use]
    pub fn shown(&self, viewer: Option<[f64; 3]>, kits: Kits) -> Vec<bool> {
        self.animated
            .iter()
            .map(|part| match part.motion {
                Motion::Roof { at, min, max } => !viewer.is_some_and(|[x, y, z]| {
                    (min[0]..=max[0]).contains(&x) && (min[1]..=max[1]).contains(&z) && y < at[1]
                }),
                Motion::Fallback { kit, .. } => !kits.draws(kit),
                _ => true,
            })
            .collect()
    }

    /// Visible static draws, water and placed animated parts for one camera eye; `kits`
    /// as for [`Self::shown`].
    #[must_use]
    pub fn frame(
        &self,
        seconds: f64,
        animate: bool,
        eye: [f32; 3],
        viewer: Option<[f64; 3]>,
        kits: Kits,
    ) -> Vec<Draw> {
        let mut draws: Vec<_> = self
            .groups
            .iter()
            .filter(|group| group.visible(eye))
            .flat_map(|group| group.draws.iter().cloned())
            .collect();
        draws.extend(self.water.iter().cloned());
        for (part, model) in self
            .animated
            .iter()
            .zip(self.motion(seconds, animate))
            .zip(self.shown(viewer, kits))
            .filter_map(|(placed, shown)| shown.then_some(placed))
        {
            draws.extend(part.draws.iter().map(|draw| Draw {
                model,
                ..draw.clone()
            }));
        }
        draws
    }
}

fn motion(motion: Motion, t: f64, animate: bool) -> Affine {
    match motion {
        Motion::Fallback { frame, .. } => frame,
        Motion::Roof { at, .. } => translate(at[0], at[1], at[2]),
        Motion::Sail { hub, yaw } => {
            compose(&[translate(hub[0], hub[1], hub[2]), ry(yaw), rz(t * 0.45)])
        }
        Motion::Flame { base, seed } => {
            let flicker = if animate {
                [
                    1. + 0.1 * (t * 9.1 + seed).sin(),
                    1. + 0.18 * (t * 7.3 + seed * 2.).sin() * (t * 3.1).sin(),
                    1. + 0.1 * (t * 8.3).cos(),
                ]
            } else {
                [1.; 3]
            };
            compose(&[
                translate(base[0], base[1], base[2]),
                ry(t * 0.7),
                scale(flicker[0], flicker[1], flicker[2]),
            ])
        }
        Motion::Reed { phase, ground_y } => {
            let (kx, kz) = if animate {
                (
                    0.045 * (t * 1.1 + phase).sin(),
                    0.035 * (t * 0.83 + phase * 1.7).sin(),
                )
            } else {
                (0., 0.)
            };
            Affine {
                m: [1., kx, 0., 0., 1., 0., 0., kz, 1.],
                t: [-kx * ground_y, 0., -kz * ground_y],
            }
        }
    }
}

/// FNV-1a over every draw's identity, placement, material and unique geometry.
fn identity<'a>(draws: impl Iterator<Item = &'a Draw>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut seen = std::collections::BTreeSet::new();
    let mut bytes = |values: &[u8]| {
        for byte in values {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for draw in draws {
        bytes(draw.id.as_bytes());
        for value in draw
            .model
            .iter()
            .chain(&draw.material.color)
            .chain(&draw.material.emissive)
            .chain([&draw.material.opacity])
        {
            bytes(&value.to_le_bytes());
        }
        if seen.insert(draw.resource_key.clone()) {
            let g = &draw.geometry;
            for value in g.p.iter().chain(&g.n).chain(&g.colors).flatten() {
                bytes(&(*value as f32).to_le_bytes());
            }
            for index in &g.indices {
                bytes(&index.to_le_bytes());
            }
        }
    }
    hash
}
