//! Greyhaven campfires using render-engine's GPU particles. The smoke description and soft
//! procedural sprite follow `render-engine/examples/kit_showcase.rs` (campfire_day/night).
//! Anchors come from the authored firelights, never from gameplay or network state.
use render_engine::{
    glam::{Mat4, Vec2, Vec3, Vec4},
    material::{Sampler, TextureData},
    particles::{
        ColorBlend, ColorOverLife, Emitter, Gradient, InitModifier, Orient, ParticleBlend,
        ParticleLighting, ParticleTexture, ShapeDimension, Spawner, TextureMapping, Value,
    },
    wgpu,
    world::{EmitterId, TextureId, World},
};
use std::sync::Arc;

pub(super) const STEP_SECONDS: f32 = 1.0 / 60.0;

/// One texture for every campfire, but an independent spawner and lifetime per fire.
pub(super) struct CampfireSmoke {
    sources: Arc<[[f32; 3]]>,
    puff: TextureId,
    emitters: Vec<EmitterId>,
    last_seconds: Option<f64>,
    step_remainder: f64,
}

impl CampfireSmoke {
    fn new(world: &mut World, sources: Arc<[[f32; 3]]>) -> Self {
        let puff = puff_texture(world);
        let emitters = sources
            .iter()
            .enumerate()
            .map(|(index, &light)| {
                let seed = 0x5A0C_E000 ^ (index as u32).wrapping_mul(0x9E37_79B9);
                let id = world.add_emitter(&emitter(
                    Some(ParticleTexture {
                        texture: puff,
                        mapping: TextureMapping::Modulate,
                        flipbook: None,
                    }),
                    seed,
                ));
                // Lights sit 0.45 m above embers; the showcase starts smoke 0.9 m above them.
                let ember = Vec3::from(light) - Vec3::Y * 0.45;
                world.set_emitter_transform(id, Mat4::from_translation(ember));
                id
            })
            .collect();
        Self {
            sources,
            puff,
            emitters,
            last_seconds: None,
            step_remainder: 0.0,
        }
    }

    fn remove(self, world: &mut World) {
        for id in self.emitters {
            world.remove_emitter(id);
        }
        world.remove_texture(self.puff);
    }

    /// Reconcile the source *scene*, not the camera or the per-frame light flicker. A new
    /// scene retires its emitters before releasing their shared texture; unchanged scenery
    /// keeps its living particles while the player moves. Simulate at a bounded fixed 60 Hz,
    /// so a high-refresh display does not multiply the GPU work per second.
    pub(super) fn sync(
        live: &mut Option<Self>,
        world: &mut World,
        sources: &Arc<[[f32; 3]]>,
        seconds: f64,
        animate: bool,
    ) -> u32 {
        if !animate || sources.is_empty() || !seconds.is_finite() {
            if let Some(old) = live.take() {
                old.remove(world);
            }
            return 0;
        }
        if live
            .as_ref()
            .is_none_or(|existing| !Arc::ptr_eq(&existing.sources, sources))
        {
            if let Some(old) = live.take() {
                old.remove(world);
            }
            *live = Some(Self::new(world, Arc::clone(sources)));
        }
        let Some(current) = live.as_mut() else {
            return 0;
        };
        // Do not catch up minutes of hidden-window time with one enormous particle step.
        let dt = current
            .last_seconds
            .map_or(f64::from(STEP_SECONDS), |last| {
                (seconds - last).clamp(0.0, 0.05)
            });
        current.last_seconds = Some(seconds);
        current.step_remainder += dt;
        let steps = (current.step_remainder / f64::from(STEP_SECONDS)).floor() as u32;
        let steps = steps.min(3);
        current.step_remainder -= f64::from(steps) * f64::from(STEP_SECONDS);
        steps
    }
}

/// Lit, fogged, alpha-blended puffs drift upwards and with a light, constant breeze. The
/// environment's actual sun, moon, firelights, shadows, exposure and fog shade them (#91).
/// Lighter, sparser than the showcase's large smoke column: these are small campfires.
fn emitter(texture: Option<ParticleTexture>, seed: u32) -> Emitter {
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
        texture,
        roundness: 0.0,
        depth_fade: 0.8,
        lighting: Some(ParticleLighting::diffuse()),
        fog: true,
        blend: ParticleBlend::Alpha,
        active: true,
    }
}

/// The showcase's 64×64 white soft puff, with an alpha mask and box-filtered mip levels.
/// Procedural at startup: no extra asset format or copy per campfire.
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
        let effect = emitter(None, 1);
        assert_eq!(effect.blend, ParticleBlend::Alpha);
        assert_eq!(effect.lighting, Some(ParticleLighting::diffuse()));
        assert!(effect.fog);
        assert!(effect.depth_fade > 0.0);
        assert!(effect.capacity >= 16 * 6);
        assert_eq!(effect.init.len(), 3);
    }
}
