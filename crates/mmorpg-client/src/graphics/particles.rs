//! The game-owned GPU particle effects of the native world (#769) over render-engine's
//! emitters: Greyhaven's campfire flames, sparks and smoke, mist over its still water, and an
//! authored burst on every zone-confirmed hit or block. render-engine owns the GPU simulation
//! and drawing; this adapter only decides where and when. Anchors come from authored scenery
//! and from projection events the zone already delivered, never the other way round: nothing
//! here feeds back into gameplay. The smoke and flame descriptions follow
//! `render-engine/examples/kit_showcase.rs` (campfire_day/night), scaled to small campfires.
use crate::world::{ImpactBurst, ImpactKind, MistPatch, ParticleSites};
use render_engine::{
    glam::{Mat4, Quat, UVec2, Vec2, Vec3, Vec4},
    material::{Sampler, TextureData},
    particles::{
        ColorBlend, ColorOverLife, Emitter, Flipbook, Gradient, InitModifier, Orient,
        ParticleBlend, ParticleLighting, ParticleTexture, ShapeDimension, Spawner, TextureMapping,
        Value,
    },
    wgpu,
    world::{EmitterId, TextureId, World},
};
use std::sync::Arc;

pub(super) const STEP_SECONDS: f32 = 1.0 / 60.0;
/// Most simulation steps one frame runs, so a stalled frame does not queue a GPU backlog.
const MAX_STEPS_PER_FRAME: u32 = 3;
/// Most live impact sprites per kind and intensity (the browser's `MAX_ACTIVE`); bursts beyond
/// it are dropped by the emitter's capacity.
const IMPACT_CAPACITY: u32 = 32;
/// Smaller copies of the authored sequence thrown along the blow's direction (full motion).
const SPRAY_COUNT: u32 = 4;
/// Mist wisps per square metre of water, and the most one water body holds.
const MIST_DENSITY: f32 = 1.0 / 14.0;
const MIST_MAX: u32 = 96;

/// Every GPU effect of one renderer. Shared textures are allocated once, on first use, and live
/// as long as the renderer; emitters live as long as the scene or effect that owns them.
#[derive(Default)]
pub(super) struct Effects {
    /// The soft white sprite every campfire and mist emitter shares.
    puff: Option<TextureId>,
    environment: Option<Environment>,
    impacts: Option<Impacts>,
    last_seconds: Option<f64>,
    step_remainder: f64,
}

/// The emitters of one scene's [`ParticleSites`].
struct Environment {
    sites: Arc<ParticleSites>,
    emitters: Vec<EmitterId>,
}

/// One inactive emitter per impact kind and intensity, burst at each confirmed impact.
struct Impacts {
    /// `[kind][intensity]`, intensity 0 strong (full motion), 1 subtle (reduced motion).
    core: [[EmitterId; 2]; 2],
    /// `[kind]`, full motion only.
    spray: [EmitterId; 2],
}

impl Effects {
    /// Reconciles the emitters with `frame`'s scene and impacts and returns how many fixed
    /// steps to simulate this frame. A new scene (not a moved camera or a flickering light)
    /// replaces the environment emitters; reduced motion retires them and plays impacts with
    /// the authored subtle sequences in place. Steps run at a bounded fixed 60 Hz, so a
    /// high-refresh display does not multiply the GPU work per second.
    pub(super) fn sync(
        &mut self,
        world: &mut World,
        sites: &Arc<ParticleSites>,
        impacts: &[ImpactBurst],
        seconds: f64,
        animate: bool,
    ) -> u32 {
        let wanted = animate && !(sites.campfires.is_empty() && sites.mist.is_empty());
        if self
            .environment
            .as_ref()
            .is_some_and(|live| !wanted || !Arc::ptr_eq(&live.sites, sites))
            && let Some(old) = self.environment.take()
        {
            for id in old.emitters {
                world.remove_emitter(id);
            }
        }
        if wanted && self.environment.is_none() {
            let puff = *self.puff.get_or_insert_with(|| puff_texture(world));
            self.environment = Some(Environment::new(world, puff, Arc::clone(sites)));
        }
        if !impacts.is_empty() {
            let live = self.impacts.get_or_insert_with(|| Impacts::new(world));
            for impact in impacts {
                live.burst(world, impact, animate);
            }
        }
        if self.environment.is_none() && self.impacts.is_none() || !seconds.is_finite() {
            self.last_seconds = None;
            return 0;
        }
        // Do not catch up minutes of hidden-window time with one enormous particle step.
        let dt = self.last_seconds.map_or(f64::from(STEP_SECONDS), |last| {
            (seconds - last).clamp(0.0, 0.05)
        });
        self.last_seconds = Some(seconds);
        self.step_remainder += dt;
        let steps = ((self.step_remainder / f64::from(STEP_SECONDS)).floor() as u32)
            .min(MAX_STEPS_PER_FRAME);
        self.step_remainder =
            (self.step_remainder - f64::from(steps) * f64::from(STEP_SECONDS)).max(0.0);
        steps
    }

    /// Live environment emitters (campfires and mist), for lifetime checks.
    #[cfg(test)]
    pub(super) fn environment_emitters(&self) -> usize {
        self.environment
            .as_ref()
            .map_or(0, |live| live.emitters.len())
    }

    /// The impact emitters, once the first impact created them.
    #[cfg(test)]
    pub(super) fn impact_emitters(&self) -> Vec<EmitterId> {
        self.impacts.as_ref().map_or_else(Vec::new, |live| {
            live.core
                .iter()
                .flatten()
                .chain(&live.spray)
                .copied()
                .collect()
        })
    }
}

impl Environment {
    fn new(world: &mut World, puff: TextureId, sites: Arc<ParticleSites>) -> Self {
        let mut emitters = Vec::new();
        for (index, &embers) in sites.campfires.iter().enumerate() {
            let seed = 0x5A0C_E000 ^ (index as u32).wrapping_mul(0x9E37_79B9);
            let at = Mat4::from_translation(Vec3::from(embers));
            for description in [
                flames(Some(puff), seed),
                sparks(seed ^ 0x51),
                smoke(Some(puff), seed ^ 0xA7),
            ] {
                let id = world.add_emitter(&description);
                world.set_emitter_transform(id, at);
                emitters.push(id);
            }
        }
        for (index, patch) in sites.mist.iter().enumerate() {
            let seed = 0x0315_7000 ^ (index as u32).wrapping_mul(0x85EB_CA6B);
            let id = world.add_emitter(&mist(Some(puff), patch, seed));
            world.set_emitter_transform(id, Mat4::from_translation(Vec3::from(patch.centre)));
            emitters.push(id);
        }
        Self { sites, emitters }
    }
}

impl Impacts {
    fn new(world: &mut World) -> Self {
        let mut core = |kind: ImpactKind, strong: bool| {
            let sheet = Sheet::embedded(kind, strong);
            let texture = world.add_texture(&sheet.texture());
            let core = world.add_emitter(&impact(&sheet, Some(texture), false));
            let spray = strong.then(|| world.add_emitter(&impact(&sheet, Some(texture), true)));
            (core, spray)
        };
        let (hit, hit_spray) = core(ImpactKind::Hit, true);
        let (hit_subtle, _) = core(ImpactKind::Hit, false);
        let (block, block_spray) = core(ImpactKind::Block, true);
        let (block_subtle, _) = core(ImpactKind::Block, false);
        Self {
            core: [[hit, hit_subtle], [block, block_subtle]],
            spray: [
                hit_spray.expect("strong sequences spray"),
                block_spray.expect("strong sequences spray"),
            ],
        }
    }

    fn burst(&self, world: &mut World, impact: &ImpactBurst, animate: bool) {
        let kind = match impact.kind {
            ImpactKind::Hit => 0,
            ImpactKind::Block => 1,
        };
        let direction = Vec3::from(impact.direction)
            .try_normalize()
            .unwrap_or(Vec3::Y);
        // The emitter's +Z is the blow's direction.
        let at = Mat4::from_rotation_translation(
            Quat::from_rotation_arc(Vec3::Z, direction),
            Vec3::from(impact.position),
        );
        world.burst(self.core[kind][usize::from(!animate)], 1, at);
        if animate {
            world.burst(self.spray[kind], SPRAY_COUNT, at);
        }
    }
}

/// An authored asset-tooling impact sequence (`web/public/effects`, the browser's #52 sprites)
/// as a render-engine flipbook: each trimmed frame is put back on its source canvas, centred on
/// its pivot, and repeated for its share of the sequence's time, so a uniform flipbook keeps
/// the authored per-frame timing.
struct Sheet {
    cell: u32,
    grid: UVec2,
    frames: u32,
    seconds: f32,
    /// Quad size in metres (the browser's `SIZE_METRES`).
    size: f32,
    rgba: Vec<u8>,
}

#[derive(serde::Deserialize)]
struct Atlas {
    sprites: Vec<AtlasFrame>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AtlasFrame {
    empty: bool,
    rect: Rect,
    pivot: Point,
    trim_offset: Point,
    source_size: Size,
    duration_ms: u32,
}

#[derive(serde::Deserialize)]
struct Rect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(serde::Deserialize)]
struct Point {
    x: i32,
    y: i32,
}

#[derive(serde::Deserialize)]
struct Size {
    width: u32,
    height: u32,
}

macro_rules! effect {
    ($name:literal, $file:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../web/public/effects/",
            $name,
            "/",
            $file
        ))
    };
}

impl Sheet {
    fn embedded(kind: ImpactKind, strong: bool) -> Self {
        let (json, png, size): (&[u8], &[u8], f32) = match (kind, strong) {
            (ImpactKind::Hit, true) => (
                effect!("puff.strong", "atlas.json"),
                effect!("puff.strong", "atlas.png"),
                1.1,
            ),
            (ImpactKind::Hit, false) => (
                effect!("puff.subtle", "atlas.json"),
                effect!("puff.subtle", "atlas.png"),
                1.1,
            ),
            (ImpactKind::Block, true) => (
                effect!("ring.strong", "atlas.json"),
                effect!("ring.strong", "atlas.png"),
                1.3,
            ),
            (ImpactKind::Block, false) => (
                effect!("ring.subtle", "atlas.json"),
                effect!("ring.subtle", "atlas.png"),
                1.3,
            ),
        };
        Self::new(json, png, size).expect("the committed impact sequences are valid")
    }

    fn new(json: &[u8], png: &[u8], size: f32) -> Result<Self, String> {
        let atlas: Atlas = serde_json::from_slice(json).map_err(|error| error.to_string())?;
        let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
            .map_err(|error| error.to_string())?
            .to_rgba8();
        let quantum = atlas
            .sprites
            .iter()
            .map(|frame| frame.duration_ms)
            .reduce(gcd)
            .filter(|quantum| *quantum > 0)
            .ok_or("an impact sequence needs timed frames")?;
        let cell = atlas
            .sprites
            .iter()
            .map(|frame| frame.source_size.width.max(frame.source_size.height))
            .max()
            .unwrap_or(1);
        let frames: u32 = atlas
            .sprites
            .iter()
            .map(|frame| frame.duration_ms / quantum)
            .sum();
        let columns = (frames as f32).sqrt().ceil() as u32;
        let grid = UVec2::new(columns, frames.div_ceil(columns));
        let width = grid.x * cell;
        let mut rgba = vec![0; (width * grid.y * cell * 4) as usize];
        let mut slot = 0;
        for frame in &atlas.sprites {
            for _ in 0..frame.duration_ms / quantum {
                let (cx, cy) = ((slot % grid.x) * cell, (slot / grid.x) * cell);
                slot += 1;
                if frame.empty {
                    continue;
                }
                // The pivot lands on the cell's centre.
                let half = i32::try_from(cell / 2).unwrap_or(0);
                let left = half - frame.pivot.x + frame.trim_offset.x;
                let top = half - frame.pivot.y + frame.trim_offset.y;
                for y in 0..frame.rect.height {
                    for x in 0..frame.rect.width {
                        let (dx, dy) = (left + x as i32, top + y as i32);
                        if !(0..cell as i32).contains(&dx) || !(0..cell as i32).contains(&dy) {
                            continue;
                        }
                        let Some(pixel) =
                            image.get_pixel_checked(frame.rect.x + x, frame.rect.y + y)
                        else {
                            continue;
                        };
                        let at = (((cy + dy as u32) * width + cx + dx as u32) * 4) as usize;
                        rgba[at..at + 4].copy_from_slice(&pixel.0);
                    }
                }
            }
        }
        Ok(Self {
            cell,
            grid,
            frames,
            seconds: (frames * quantum) as f32 / 1000.0,
            size,
            rgba,
        })
    }

    fn texture(&self) -> TextureData {
        TextureData {
            width: self.grid.x * self.cell,
            height: self.grid.y * self.cell,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            // One level: mipmaps would bleed neighbouring cells into each other.
            levels: vec![self.rgba.clone()],
            sampler: Sampler {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                ..Sampler::default()
            },
        }
    }
}

const fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// One impact sequence, spawned only by bursts: the full-size sprite resting on the struck
/// unit, or (`spray`) half-size copies thrown in a cone along the blow and falling.
fn impact(sheet: &Sheet, texture: Option<TextureId>, spray: bool) -> Emitter {
    let size = if spray { sheet.size * 0.45 } else { sheet.size };
    Emitter {
        capacity: if spray {
            IMPACT_CAPACITY * SPRAY_COUNT
        } else {
            IMPACT_CAPACITY
        },
        spawner: Spawner::once(0.0),
        seed: if spray { 0x1A9C_7001 } else { 0x1A9C_7000 },
        init: vec![
            InitModifier::PositionSphere {
                center: Vec3::ZERO,
                radius: if spray { 0.08 } else { 0.0 },
                dimension: ShapeDimension::Volume,
            },
            if spray {
                InitModifier::VelocityCone {
                    direction: Vec3::Z,
                    spread: 0.7,
                    speed: Value::Uniform { min: 1.2, max: 2.6 },
                }
            } else {
                InitModifier::Velocity(Value::Const(Vec3::ZERO))
            },
            InitModifier::Lifetime(Value::Const(sheet.seconds)),
            InitModifier::Color(Value::Const(Vec4::ONE)),
        ],
        accel: if spray {
            Vec3::new(0.0, -3.0, 0.0)
        } else {
            Vec3::ZERO
        },
        size_over_life: Some(Gradient::new(vec![(0.0, Vec2::splat(size))])),
        color_over_life: None,
        orient: Orient::CameraPlane,
        texture: texture.map(|texture| ParticleTexture {
            texture,
            mapping: TextureMapping::Modulate,
            flipbook: Some(Flipbook {
                grid: sheet.grid,
                frames: sheet.frames,
            }),
        }),
        roundness: 0.0,
        depth_fade: 0.15,
        lighting: None,
        fog: true,
        blend: ParticleBlend::Alpha,
        active: false,
    }
}

/// The shared soft sprite, untouched by a flipbook.
fn soft(puff: Option<TextureId>) -> Option<ParticleTexture> {
    puff.map(|texture| ParticleTexture {
        texture,
        mapping: TextureMapping::Modulate,
        flipbook: None,
    })
}

/// Additive, unlit flame licks over the embers (the showcase's `fire`, a third of its rate
/// and smaller: these are small campfires whose flame mesh already glows).
fn flames(puff: Option<TextureId>, seed: u32) -> Emitter {
    Emitter {
        capacity: 48,
        spawner: Spawner::rate(45.0),
        seed,
        init: vec![
            InitModifier::PositionSphere {
                center: Vec3::new(0.0, 0.08, 0.0),
                radius: 0.16,
                dimension: ShapeDimension::Volume,
            },
            InitModifier::Velocity(Value::Uniform {
                min: Vec3::new(-0.1, 0.5, -0.1),
                max: Vec3::new(0.1, 1.1, 0.1),
            }),
            InitModifier::Lifetime(Value::Uniform { min: 0.4, max: 0.7 }),
        ],
        accel: Vec3::new(0.0, 0.6, 0.0),
        size_over_life: Some(Gradient::new(vec![
            (0.0, Vec2::splat(0.36)),
            (1.0, Vec2::splat(0.08)),
        ])),
        color_over_life: Some(ColorOverLife {
            gradient: Gradient::new(vec![
                (0.0, Vec4::new(6.0, 2.6, 0.7, 0.85)),
                (0.45, Vec4::new(3.2, 0.9, 0.15, 0.65)),
                (1.0, Vec4::new(0.6, 0.08, 0.0, 0.0)),
            ]),
            blend: ColorBlend::Overwrite,
        }),
        orient: Orient::CameraPlane,
        texture: soft(puff),
        roundness: 0.0,
        depth_fade: 0.3,
        lighting: None,
        fog: true,
        blend: ParticleBlend::Add,
        active: true,
    }
}

/// Sparks streaking up and falling (the showcase's `embers`, sparser).
fn sparks(seed: u32) -> Emitter {
    Emitter {
        capacity: 48,
        spawner: Spawner::rate(14.0),
        seed,
        init: vec![
            InitModifier::PositionSphere {
                center: Vec3::new(0.0, 0.2, 0.0),
                radius: 0.15,
                dimension: ShapeDimension::Volume,
            },
            InitModifier::Velocity(Value::Uniform {
                min: Vec3::new(-0.5, 1.0, -0.5),
                max: Vec3::new(0.6, 2.6, 0.5),
            }),
            InitModifier::Lifetime(Value::Uniform { min: 0.9, max: 2.0 }),
        ],
        accel: Vec3::new(0.2, -0.6, 0.0),
        size_over_life: Some(Gradient::new(vec![
            (0.0, Vec2::new(0.05, 0.012)),
            (1.0, Vec2::new(0.025, 0.008)),
        ])),
        color_over_life: Some(ColorOverLife {
            gradient: Gradient::new(vec![
                (0.0, Vec4::new(9.0, 3.5, 0.6, 1.0)),
                (1.0, Vec4::new(2.0, 0.25, 0.0, 0.0)),
            ]),
            blend: ColorBlend::Overwrite,
        }),
        orient: Orient::AlongVelocity,
        texture: None,
        roundness: 0.0,
        depth_fade: 0.0,
        lighting: None,
        fog: true,
        blend: ParticleBlend::Add,
        active: true,
    }
}

/// Lit, fogged, alpha-blended puffs drift upwards and with a light, constant breeze. The
/// environment's actual sun, moon, firelights, shadows, exposure and fog shade them (#91).
/// Lighter, sparser than the showcase's large smoke column: these are small campfires.
fn smoke(puff: Option<TextureId>, seed: u32) -> Emitter {
    Emitter {
        capacity: 128,
        spawner: Spawner::rate(16.0),
        seed,
        init: vec![
            InitModifier::PositionSphere {
                center: Vec3::new(0.0, 0.9, 0.0),
                radius: 0.2,
                dimension: ShapeDimension::Volume,
            },
            InitModifier::Velocity(Value::Uniform {
                min: Vec3::new(-0.1, 0.5, -0.1),
                max: Vec3::new(0.1, 0.9, 0.1),
            }),
            InitModifier::Lifetime(Value::Uniform { min: 4.0, max: 6.0 }),
        ],
        accel: Vec3::new(0.12, 0.05, 0.03),
        size_over_life: Some(Gradient::new(vec![
            (0.0, Vec2::splat(0.35)),
            (1.0, Vec2::splat(1.8)),
        ])),
        color_over_life: Some(ColorOverLife {
            gradient: Gradient::new(vec![
                (0.0, Vec4::new(0.35, 0.34, 0.33, 0.0)),
                (0.08, Vec4::new(0.48, 0.47, 0.46, 0.26)),
                (0.6, Vec4::new(0.58, 0.59, 0.59, 0.14)),
                (1.0, Vec4::new(0.64, 0.65, 0.65, 0.0)),
            ]),
            blend: ColorBlend::Overwrite,
        }),
        orient: Orient::CameraPlane,
        texture: soft(puff),
        roundness: 0.0,
        depth_fade: 0.8,
        lighting: Some(ParticleLighting::diffuse()),
        fog: true,
        blend: ParticleBlend::Alpha,
        active: true,
    }
}

/// Thin, lit wisps lying on one water body (the showcase's `mist`): large, slow, long-lived
/// and faint, so they read as localized mist rather than the renderer's distance fog.
fn mist(puff: Option<TextureId>, patch: &MistPatch, seed: u32) -> Emitter {
    let lifetime = 12.0;
    let [hx, hz] = patch.half;
    let count = ((4.0 * hx * hz * MIST_DENSITY).ceil() as u32).clamp(4, MIST_MAX);
    let size = (hx.min(hz) * 0.8).clamp(1.5, 5.0);
    let color = Vec4::new(0.82, 0.85, 0.88, 0.16);
    Emitter {
        capacity: count,
        spawner: Spawner::rate(count as f32 / lifetime),
        seed,
        init: vec![
            InitModifier::Position(Value::Uniform {
                min: Vec3::new(-hx, 0.15, -hz),
                max: Vec3::new(hx, 0.7, hz),
            }),
            InitModifier::Velocity(Value::Uniform {
                min: Vec3::new(-0.08, -0.005, -0.08),
                max: Vec3::new(0.12, 0.02, 0.08),
            }),
            InitModifier::Lifetime(Value::Const(lifetime)),
            // Start part-way through life so the mist is already established.
            InitModifier::Age(Value::Uniform { min: 0.0, max: 7.0 }),
        ],
        accel: Vec3::ZERO,
        size_over_life: Some(Gradient::new(vec![
            (0.0, Vec2::splat(size * 0.7)),
            (1.0, Vec2::splat(size * 1.3)),
        ])),
        color_over_life: Some(ColorOverLife {
            gradient: Gradient::new(vec![
                (0.0, color.with_w(0.0)),
                (0.3, color),
                (0.7, color),
                (1.0, color.with_w(0.0)),
            ]),
            blend: ColorBlend::Overwrite,
        }),
        orient: Orient::CameraPlane,
        texture: soft(puff),
        roundness: 0.0,
        depth_fade: 1.5,
        lighting: Some(ParticleLighting::diffuse()),
        fog: true,
        blend: ParticleBlend::Alpha,
        active: true,
    }
}

/// The showcase's 64×64 white soft puff, with an alpha mask and box-filtered mip levels.
/// Procedural at startup: no extra asset format or copy per emitter.
fn puff_texture(world: &mut World) -> TextureId {
    const N: u32 = 64;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let p = (Vec2::new(x as f32, y as f32) + 0.5) / N as f32 * 2.0 - 1.0;
            let r2 = p.length_squared();
            let wobble = 1.0 + 0.18 * ((p.x * 5.1).sin() * (p.y * 4.3 + 1.0).cos());
            let alpha = ((1.0 - r2 * wobble).max(0.0)).powi(2);
            rgba.extend_from_slice(&[255, 255, 255, (alpha * 255.0).round() as u8]);
        }
    }
    let mut levels = vec![rgba];
    let mut size = N as usize;
    while size > 1 {
        let next_size = size / 2;
        let previous = levels.last().expect("a base mip exists");
        let mut next = vec![0; next_size * next_size * 4];
        for y in 0..next_size {
            for x in 0..next_size {
                for channel in 0..4 {
                    let sum = [(0, 0), (1, 0), (0, 1), (1, 1)]
                        .into_iter()
                        .map(|(dx, dy)| {
                            u32::from(previous[((2 * y + dy) * size + 2 * x + dx) * 4 + channel])
                        })
                        .sum::<u32>();
                    next[(y * next_size + x) * 4 + channel] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push(next);
        size = next_size;
    }
    world.add_texture(&TextureData {
        width: N,
        height: N,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        levels,
        sampler: Sampler {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Sampler::default()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_uses_scene_lights_fog_and_soft_geometry_edges() {
        let effect = smoke(None, 1);
        assert_eq!(effect.blend, ParticleBlend::Alpha);
        assert_eq!(effect.lighting, Some(ParticleLighting::diffuse()));
        assert!(effect.fog);
        assert!(effect.depth_fade > 0.0);
        assert!(effect.capacity >= 16 * 6);
        assert_eq!(effect.init.len(), 3);
    }

    /// The authored sequences keep their timing: a uniform flipbook over the whole sequence,
    /// each frame repeated for its share, and the pivot on the cell's centre.
    #[test]
    fn impact_sheets_keep_the_authored_frames_and_timing() {
        for (kind, strong, seconds, frames) in [
            (ImpactKind::Hit, true, 0.6, 60),
            (ImpactKind::Hit, false, 0.6, 60),
            (ImpactKind::Block, true, 0.42, 21),
            (ImpactKind::Block, false, 0.42, 21),
        ] {
            let sheet = Sheet::embedded(kind, strong);
            assert!((sheet.seconds - seconds).abs() < 1e-6, "{kind:?} {strong}");
            assert_eq!(sheet.frames, frames);
            assert!(sheet.grid.x * sheet.grid.y >= frames);
            assert_eq!(sheet.cell, 64);
            assert_eq!(
                sheet.rgba.len(),
                (sheet.grid.x * sheet.grid.y * 64 * 64 * 4) as usize
            );
            // The first frame is empty; later ones draw something.
            let width = (sheet.grid.x * 64) as usize;
            let cell_alpha = |slot: u32| {
                let (cx, cy) = ((slot % sheet.grid.x) * 64, (slot / sheet.grid.x) * 64);
                (0..64usize)
                    .flat_map(|y| (0..64usize).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        u32::from(sheet.rgba[((cy as usize + y) * width + cx as usize + x) * 4 + 3])
                    })
                    .sum::<u32>()
            };
            assert_eq!(cell_alpha(0), 0);
            assert!(cell_alpha(frames / 2) > 0);
        }
    }

    #[test]
    fn impacts_burst_only_and_stay_bounded() {
        let sheet = Sheet::embedded(ImpactKind::Hit, true);
        for spray in [false, true] {
            let effect = impact(&sheet, None, spray);
            assert!(!effect.active, "only confirmed impacts spawn");
            assert!(effect.capacity <= IMPACT_CAPACITY * SPRAY_COUNT);
            assert_eq!(
                effect.init[2],
                InitModifier::Lifetime(Value::Const(sheet.seconds))
            );
        }
    }

    #[test]
    fn mist_is_bounded_by_its_water() {
        let lake = mist(
            None,
            &MistPatch {
                centre: [0.0; 3],
                half: [16.5, 12.0],
            },
            1,
        );
        assert!(lake.capacity <= MIST_MAX);
        assert_eq!(
            lake.init[0],
            InitModifier::Position(Value::Uniform {
                min: Vec3::new(-16.5, 0.15, -12.0),
                max: Vec3::new(16.5, 0.7, 12.0),
            })
        );
    }
}
