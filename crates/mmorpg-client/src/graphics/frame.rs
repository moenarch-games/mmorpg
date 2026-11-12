//! The application's renderer-neutral draw frames on render-engine (#529): meshes and
//! materials live in its `World` behind stable handles, the lighting is its physical `Scene`
//! (sun, moon, sky, cascaded shadows, sky ambient light, fog, point lights), and the combat HUD
//! is drawn over its tone-mapped output.
use super::{
    ClientError,
    particles::{Effects, STEP_SECONDS},
};
use crate::{
    camera::CameraView,
    environment::Lighting,
    hud::{HudRect, MAX_HUD_RECTS},
    world::{DrawBatch, DrawMaterial, DrawSkin, SceneDraw, WorldFrame},
};
use bytemuck::{Pod, Zeroable};
use mmorpg_presentation::ground::{Ground, Layer, MASK_CUTOFF, OVERLAY_ALPHA};
use render_engine::{
    glam::{Mat4, Vec2, Vec3, Vec4},
    material::{AlphaMode, StandardMaterial},
    mesh::MeshData,
    renderer::{Renderer, Viewport},
    scene::{
        Ambient, Camera, DayCycle, Fog, FogFalloff, Moon, PointLight, RAW_SUNLIGHT, Scene,
        ShadowFilter, ShadowMapSizes, Sky, Sun, SunShadows, Tonemapping,
    },
    world::{Instance, InstanceId, MaterialId, MeshId, SkinId, TextureId, World},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    mem,
    sync::{Arc, Weak},
};
use three_d_core::Mesh;

/// Greyhaven's sun path: 60° above the southern horizon at noon, the moon opposite.
pub const DAY_CYCLE: DayCycle = DayCycle {
    noon_elevation: 60f32.to_radians(),
    moon_lag_hours: 12.0,
};
/// Moonlight in lux: brighter than a real full moon (0.25 lx) so night stays playable.
pub const MOON_LUX: f32 = 2.0;
/// Cool, slightly blue moonlight.
pub const MOON_COLOR: Vec3 = Vec3::new(0.75, 0.85, 1.0);
/// EV100 by sun elevation in degrees, interpolated linearly: render-engine's `sky_day_cycle`
/// oracle curve with a half-stop darker noon for Greyhaven's earth/stone palette, and a
/// brighter-than-physical night for play.
pub const EXPOSURE: [(f32, f32); 5] = [
    (-12.0, 2.0),
    (-4.0, 6.0),
    (2.0, 11.0),
    (15.0, 13.8),
    (60.0, 15.0),
];
/// Emissive presentation colours are scaled to this luminance in cd/m²: flames, lamp glass and
/// lit windows glow in their own colour at night; by day the sun outshines them.
pub const EMISSIVE_NITS: f32 = 80.0;
/// Sun cascades sized for the gameplay camera, 4 to 30 m from the player.
pub const SUN_SHADOWS: SunShadows = SunShadows {
    num_cascades: 4,
    min_distance: 0.2,
    first_cascade_far_bound: 12.0,
    max_distance: 160.0,
    overlap_proportion: 0.2,
    depth_bias: 0.02,
    normal_bias: 1.8,
};
/// Below this sun elevation (degrees) the sun casts no shadows; its light has faded out.
const SHADOWLESS_BELOW: f32 = -3.0;
const NEAR_PLANE_METRES: f32 = 0.2;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HudInstance {
    min: [f32; 2],
    max: [f32; 2],
    color: [f32; 3],
}

struct CachedMesh {
    id: MeshId,
    source: Weak<Mesh>,
    used: bool,
    /// Live cached batches whose instances use this mesh.
    batches: u32,
}

/// A figure's joint palette on the GPU (#595): added when the figure is first drawn, its pose
/// replaced when the figure's palette changes and removed once it is no longer drawn.
struct CachedSkin {
    id: SkinId,
    /// The palette uploaded last.
    palette: Weak<[[f32; 16]]>,
    used: bool,
}

struct CachedMaterial {
    id: MaterialId,
    used: bool,
    /// Live cached batches whose instances use this material.
    batches: u32,
}

/// A material's exact field bits.
type MaterialKey = [u32; 10];

/// A [`DrawBatch`] resolved into instances once (#588). It stays valid while the batch lives,
/// drawn or culled, and is dropped with it; its meshes and materials stay resident until then.
/// While drawn, its instances are retained in the world (#728), so a steady frame lists and
/// uploads none of them; they are removed when a frame no longer draws it or it dies.
struct CachedBatch {
    source: Weak<[SceneDraw]>,
    instances: Vec<Instance>,
    /// The world's retained copies of `instances` while the batch is drawn; empty otherwise.
    retained: Vec<InstanceId>,
    /// Whether this frame draws the batch.
    drawn: bool,
    meshes: Vec<String>,
    materials: Vec<MaterialKey>,
}

/// A live batch's identity: its allocation, which the cached `Weak` keeps from being reused.
fn batch_key(batch: &DrawBatch) -> usize {
    Arc::as_ptr(batch).cast::<SceneDraw>() as usize
}

/// Draws that render nothing are skipped.
fn drawn(draw: &SceneDraw) -> bool {
    draw.material.opacity > 0. && !draw.mesh.indices().is_empty()
}

fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn linear3(c: [f32; 3]) -> Vec3 {
    Vec3::from(c.map(linear))
}

/// render-engine's mesh for a draw's mesh: a skinned draw's bind pose with its joints and
/// weights, every other mesh as it is; meshes without normals are flat shaded.
fn draw_mesh_data(draw: &SceneDraw) -> MeshData {
    let mut data = mesh_data(&draw.mesh);
    if let Some(skin) = &draw.skin {
        assert!(
            draw.mesh.attributes().normals.is_some()
                && skin.influences.joints.len() == data.positions.len(),
            "a skinned mesh has normals and one influence per vertex"
        );
        data.joints.clone_from(&skin.influences.joints);
        data.weights = skin
            .influences
            .weights
            .iter()
            .map(|&w| Vec4::from(w))
            .collect();
    }
    // A ground overlay's vertex colour alpha, which its mask cuts by (#525).
    if let Some(ground) = &draw.material.ground
        && ground.alpha.len() == data.positions.len()
    {
        if data.colors.len() != data.positions.len() {
            data.colors = vec![Vec4::ONE; data.positions.len()];
        }
        for (color, &alpha) in data.colors.iter_mut().zip(ground.alpha.iter()) {
            color.w = alpha;
        }
    }
    data
}

/// render-engine's mesh for a presentation mesh; meshes without normals are flat shaded.
fn mesh_data(mesh: &Mesh) -> MeshData {
    let colors = mesh.attributes().colors.as_ref();
    let color =
        |i: usize| colors.map_or(Vec4::ONE, |c| linear3([c[i].r, c[i].g, c[i].b]).extend(1.));
    let position = |i: usize| {
        let p = mesh.vertices()[i];
        Vec3::new(p.x, p.y, p.z)
    };
    let mut data = MeshData::default();
    if let Some(normals) = &mesh.attributes().normals {
        let count = mesh.vertices().len();
        data.positions = (0..count).map(position).collect();
        data.normals = normals.iter().map(|n| Vec3::new(n.x, n.y, n.z)).collect();
        if colors.is_some() {
            data.colors = (0..count).map(color).collect();
        }
        if let Some(uvs) = &mesh.attributes().uvs {
            data.uvs = uvs.iter().map(|uv| Vec2::new(uv.x, uv.y)).collect();
        }
        if let Some(tangents) = &mesh.attributes().tangents {
            data.tangents = tangents
                .iter()
                .map(|t| Vec4::new(t.x, t.y, t.z, t.w))
                .collect();
        }
        data.indices = mesh.indices().to_vec();
    } else {
        for (triangle, corners) in mesh.indices().as_chunks::<3>().0.iter().enumerate() {
            let Some(n) = mesh.triangle_normal(triangle) else {
                continue;
            };
            for &index in corners {
                let i = index as usize;
                data.indices.push(data.positions.len() as u32);
                data.positions.push(position(i));
                data.normals.push(Vec3::new(n.x, n.y, n.z));
                if colors.is_some() {
                    data.colors.push(color(i));
                }
            }
        }
    }
    data
}

fn material_key(m: &DrawMaterial) -> MaterialKey {
    let [r, g, b] = m.color.map(f32::to_bits);
    let [er, eg, eb] = m.emissive.map(f32::to_bits);
    [
        r,
        g,
        b,
        er,
        eg,
        eb,
        m.opacity.to_bits(),
        u32::from(m.unlit),
        u32::from(m.double_sided),
        m.ground.as_ref().map_or(0, |g| {
            1 + 4 * g.ground.layer as u32
                + 2 * u32::from(g.ground.far)
                + u32::from(g.ground.overlay)
        }),
    ]
}

/// One ground layer's uploaded textures (#525).
struct GroundLayerGpu {
    base_color: TextureId,
    normal: Option<TextureId>,
    occlusion_roughness_metal: Option<TextureId>,
    mean: Vec3,
}

/// The `greyhaven-terrain` layers uploaded once; a layer the bundle lacks is absent.
struct GroundGpu(BTreeMap<Layer, GroundLayerGpu>);

impl GroundGpu {
    fn upload(world: &mut World) -> Self {
        Self(
            crate::ground::library()
                .iter()
                .map(|(&layer, textures)| {
                    (
                        layer,
                        GroundLayerGpu {
                            base_color: world.add_texture(&textures.base_color),
                            normal: textures.normal.as_ref().map(|t| world.add_texture(t)),
                            occlusion_roughness_metal: textures
                                .occlusion_roughness_metal
                                .as_ref()
                                .map(|t| world.add_texture(t)),
                            mean: textures.mean,
                        },
                    )
                })
                .collect(),
        )
    }

    /// A ground draw's material: its layer's textures divided by their mean colour, so the
    /// vertex colours keep the palette; an overlay masked by weight × height
    /// (`mmorpg_presentation::ground`). Coarse rock keeps colour/roughness detail without
    /// its near normal/occlusion maps. Other far ground and missing textures use flat vertex colours.
    fn material(&self, m: &DrawMaterial, ground: Ground) -> StandardMaterial {
        let (alpha, alpha_mode) = if ground.overlay {
            (OVERLAY_ALPHA, AlphaMode::Mask(MASK_CUTOFF))
        } else {
            (1., AlphaMode::Opaque)
        };
        let Some(layer) = self
            .0
            .get(&ground.layer)
            .filter(|_| !ground.far || ground.layer == Layer::Rock)
        else {
            let flat = standard_material(m);
            return StandardMaterial {
                base_color: flat.base_color.truncate().extend(alpha),
                alpha_mode,
                ..flat
            };
        };
        StandardMaterial {
            base_color: (linear3(m.color) / layer.mean).extend(alpha),
            base_color_texture: Some(layer.base_color),
            normal_map_texture: if ground.far { None } else { layer.normal },
            metallic_roughness_texture: layer.occlusion_roughness_metal,
            occlusion_texture: if ground.far {
                None
            } else {
                layer.occlusion_roughness_metal
            },
            metallic: 0.,
            perceptual_roughness: 1.,
            alpha_mode,
            ..StandardMaterial::default()
        }
    }
}

fn standard_material(m: &DrawMaterial) -> StandardMaterial {
    StandardMaterial {
        base_color: linear3(m.color).extend(m.opacity),
        emissive: linear3(m.emissive) * EMISSIVE_NITS,
        // Match the shared browser presentation's wet/translucent surface response.
        perceptual_roughness: if m.opacity < 1. { 0.3 } else { 0.86 },
        metallic: if m.opacity < 1. { 0. } else { 0.02 },
        alpha_mode: if m.opacity < 1. {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
        // Procedural water/foam receives shadows without casting an opaque silhouette,
        // matching the browser's blended-material shadow cutoff. Opaque modes ignore it.
        blend_shadow_alpha_cutoff: 1.,
        double_sided: m.double_sided,
        unlit: m.unlit,
        ..StandardMaterial::default()
    }
}

/// EV100 for a sun `elevation` in degrees, from [`EXPOSURE`].
#[must_use]
pub fn ev100(elevation: f32) -> f32 {
    let first = EXPOSURE[0];
    let last = EXPOSURE[EXPOSURE.len() - 1];
    if elevation <= first.0 {
        return first.1;
    }
    for pair in EXPOSURE.windows(2) {
        let [(e0, v0), (e1, v1)] = [pair[0], pair[1]];
        if elevation <= e1 {
            return v0 + (v1 - v0) * (elevation - e0) / (e1 - e0);
        }
    }
    last.1
}

/// render-engine's scene for one frame's lighting seen from `view`.
#[must_use]
pub fn scene(lighting: &Lighting, view: CameraView) -> Scene {
    let to_sun = DAY_CYCLE.to_sun(lighting.hour);
    let elevation = to_sun.y.clamp(-1., 1.).asin().to_degrees();
    let fog = &lighting.fog;
    let interior = lighting.interior.clamp(0., 1.);
    let exposure = ev100(elevation);
    let transmission = 10_f32.powf(-3. * interior);
    // Match the browser: retain night/cave fill, with less diffuse fill in daylight so
    // the enclosing relief and the settlement's cast shadows retain their contrast.
    let daylight = (elevation / 15.).clamp(0., 1.);
    let sky_fill = 0.9 - 0.25 * daylight;
    Scene {
        camera: Camera {
            eye: Vec3::from(view.eye),
            target: Vec3::from(view.target),
            fov_y: view.field_of_view_degrees.to_radians(),
            near: NEAR_PLANE_METRES,
            ev100: exposure + (exposure.min(7.5) - exposure) * interior,
            shadow_filter: ShadowFilter::Gaussian,
            tonemapping: Tonemapping::TonyMcMapface,
        },
        clear_color: Vec3::ZERO,
        sun: Some(Sun {
            direction: -to_sun,
            color: Vec3::ONE,
            illuminance: RAW_SUNLIGHT * transmission,
            shadows: (elevation > SHADOWLESS_BELOW).then_some(SUN_SHADOWS),
        }),
        moon: Some(Moon {
            direction: -DAY_CYCLE.to_moon(lighting.hour),
            color: MOON_COLOR,
            illuminance: MOON_LUX * transmission,
        }),
        // The sky's environment light is the ambient light.
        sky: Some(Sky {
            environment_intensity: Some(
                (sky_fill + (1. - sky_fill) * interior) * 10_f32.powf(-interior),
            ),
            ..Sky::default()
        }),
        point_lights: lighting
            .point_lights
            .iter()
            .map(|light| PointLight {
                position: Vec3::from(light.position),
                color: linear3(light.color),
                intensity: light.lumens,
                range: light.range,
                radius: 0.1,
                shadows: None,
            })
            .collect(),
        spot_lights: Vec::new(),
        shadow_maps: ShadowMapSizes::default(),
        ambient: Ambient {
            color: Vec3::ONE,
            brightness: 0.,
        },
        environment: None,
        fog: Some(Fog {
            color: linear3(fog.color).extend(1.),
            falloff: {
                let k = FogFalloff::koschmieder(
                    fog.visibility,
                    FogFalloff::REVISED_KOSCHMIEDER_CONTRAST_THRESHOLD,
                );
                FogFalloff::Atmospheric {
                    extinction: Vec3::from(crate::environment::FOG_EXTINCTION) * k,
                    inscattering: Vec3::from(crate::environment::FOG_INSCATTERING) * k,
                }
            },
            ..Fog::default()
        }),
    }
}

/// One uploaded primitive of a tree or rock level (#49).
struct NaturePart {
    mesh: MeshId,
    fade_in: MeshId,
    fade_out: MeshId,
    material: MaterialId,
    /// One dissolve material per fade step (`crate::nature::FADE_STEPS`).
    fades: Vec<MaterialId>,
    /// A layered surface's overlay (`crate::nature::Part::overlay`).
    overlay: bool,
}

/// The `greyhaven-nature` library, uploaded once: its textures, then per entry, per level, its
/// primitives. It fills in library order, all at once or a few items per frame
/// ([`FrameRenderer::preload`]), and is drawn only once `complete`.
#[derive(Default)]
struct NatureGpu {
    textures: Vec<TextureId>,
    levels: Vec<Vec<Vec<NaturePart>>>,
    complete: bool,
}

/// A drawn tree or rock level's identity among the frame's nature draws (#728): its entry,
/// level, placement and dissolve state.
type NatureKey = (usize, usize, [u32; 16], u32);

fn nature_key(draw: &crate::world::NatureDraw) -> NatureKey {
    (
        draw.entry,
        draw.level,
        draw.model.map(f32::to_bits),
        draw.fade.to_bits(),
    )
}

/// The retained instances of the nature draws sharing one [`NatureKey`], one group per draw.
#[derive(Default)]
struct RetainedNature {
    groups: Vec<Vec<InstanceId>>,
    /// How many groups this frame drew.
    used: usize,
}

/// Whether an upload spread over frames has used up this frame's share.
fn past(deadline: Option<std::time::Instant>) -> bool {
    deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline)
}

impl NatureGpu {
    /// Uploads the rest of the library, stopping at `deadline`; true once all of it is resident.
    fn upload(&mut self, world: &mut World, deadline: Option<std::time::Instant>) -> bool {
        if self.complete {
            return true;
        }
        let library = crate::nature::library();
        for texture in &library.textures[self.textures.len()..] {
            if past(deadline) {
                return false;
            }
            self.textures.push(world.add_texture(texture));
        }
        let textures = &self.textures;
        for (entry, levels) in library.levels.iter().enumerate() {
            if self.levels.len() == entry {
                self.levels.push(Vec::new());
            }
            let uploaded = &mut self.levels[entry];
            for (level, parts) in levels.iter().enumerate() {
                if uploaded.len() == level {
                    uploaded.push(Vec::new());
                }
                let uploaded = &mut uploaded[level];
                for part in &parts[uploaded.len()..] {
                    if past(deadline) {
                        return false;
                    }
                    let texture = |slot: Option<usize>| slot.map(|i| textures[i]);
                    let material = StandardMaterial {
                        base_color_texture: texture(part.textures.base_color),
                        normal_map_texture: part
                            .textures
                            .normal
                            .filter(|_| !part.mesh.tangents.is_empty())
                            .map(|i| textures[i]),
                        metallic_roughness_texture: texture(part.textures.metallic_roughness),
                        ..part.material
                    };
                    uploaded.push(NaturePart {
                        mesh: world.add_mesh(&part.mesh),
                        fade_in: world.add_mesh(&part.fade_in),
                        fade_out: world.add_mesh(&part.fade_out),
                        fades: (0..crate::nature::FADE_STEPS)
                            .map(|step| {
                                world.add_material(&crate::nature::fade_material(
                                    &material,
                                    crate::nature::fade_threshold(step),
                                ))
                            })
                            .collect(),
                        material: world.add_material(&material),
                        overlay: part.overlay,
                    });
                }
            }
        }
        self.complete = true;
        true
    }

    /// The instances of one drawn tree or rock level: settled, or its dissolve copy.
    fn instances(&self, draw: &crate::world::NatureDraw, out: &mut Vec<Instance>) {
        let Some(parts) = self
            .levels
            .get(draw.entry)
            .and_then(|levels| levels.get(draw.level))
        else {
            return;
        };
        let world_from_local = Mat4::from_cols_array(&draw.model);
        let steps = crate::nature::FADE_STEPS;
        let step = |threshold: f32| ((threshold * steps as f32) as usize).min(steps - 1);
        for part in parts {
            // A layered surface's overlay (#732) covers only a settled level.
            if part.overlay && draw.fade < 1. {
                continue;
            }
            let (mesh, material) = if draw.fade >= 1. {
                (part.mesh, part.material)
            } else if draw.fade > 0. {
                // Incoming: kept where its noise reaches 1 - t.
                (part.fade_in, part.fades[step(1. - draw.fade)])
            } else {
                // Outgoing: kept where the complementary noise reaches t.
                (part.fade_out, part.fades[step(-draw.fade)])
            };
            out.push(Instance {
                mesh,
                material,
                world_from_local,
                skin: None,
            });
        }
    }
}

pub(super) struct FrameRenderer {
    renderer: Renderer,
    world: World,
    /// The GPU particle effects (#769); independent of camera visibility and zone gameplay.
    effects: Effects,
    nature: NatureGpu,
    /// The ground layer textures (#525), uploaded with the first ground material.
    ground: Option<GroundGpu>,
    viewport: Viewport,
    meshes: HashMap<String, CachedMesh>,
    materials: HashMap<MaterialKey, CachedMaterial>,
    /// Joint palettes by figure.
    skins: HashMap<String, CachedSkin>,
    /// A palette converted for upload; the allocation is kept across frames.
    palette: Vec<Mat4>,
    batches: HashMap<usize, CachedBatch>,
    /// The drawn trees and rocks, retained in the world while drawn (#728).
    nature_drawn: HashMap<NatureKey, RetainedNature>,
    /// Scratch space for one nature draw's instances; the allocation is kept across frames.
    nature_scratch: Vec<Instance>,
    /// This frame's listed (not retained) instances; the allocation is kept across frames.
    instances: Vec<Instance>,
    hud_pipeline: wgpu::RenderPipeline,
    hud_buffer: wgpu::Buffer,
    /// Mesh uploads (new or rewritten), reuses, uploaded vertex bytes and palette uploads.
    #[cfg(test)]
    observation: [u64; 4],
    /// The last frame's CPU spans and work counts (#692).
    #[cfg(test)]
    profile: super::profile::FrameProfile,
    /// GPU pass timing, once enabled (#692).
    #[cfg(test)]
    gpu_timer: Option<super::profile::GpuTimer>,
    #[cfg(test)]
    particle_timer: Option<super::profile::GpuTimer>,
    #[cfg(test)]
    particle_step_timed: bool,
}

impl FrameRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        [width, height]: [u32; 2],
    ) -> Self {
        let renderer = Renderer::new(device, queue, format);
        let world = World::new(&renderer);
        // Clustered light assignment where the device supports it, the per-fragment loop
        // otherwise (render-engine#94, #725): each fragment shades only its cluster's point
        // lights instead of all of Greyhaven's lamps and campfires.
        let viewport = Viewport::new(&renderer, width, height);
        let hud_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("combat HUD"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../hud.wgsl").into()),
        });
        let hud_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("combat HUD"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        const HA: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0=>Float32x2,1=>Float32x2,2=>Float32x3];
        let hud_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("combat HUD"),
            layout: Some(&hud_layout),
            vertex: wgpu::VertexState {
                module: &hud_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<HudInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &HA,
                })],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &hud_shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            renderer,
            world,
            effects: Effects::default(),
            nature: NatureGpu::default(),
            ground: None,
            viewport,
            meshes: HashMap::new(),
            materials: HashMap::new(),
            skins: HashMap::new(),
            palette: Vec::new(),
            batches: HashMap::new(),
            nature_drawn: HashMap::new(),
            nature_scratch: Vec::new(),
            instances: Vec::new(),
            hud_pipeline,
            hud_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("combat HUD"),
                size: (MAX_HUD_RECTS * mem::size_of::<HudInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            #[cfg(test)]
            observation: [0; 4],
            #[cfg(test)]
            profile: Default::default(),
            #[cfg(test)]
            gpu_timer: None,
            #[cfg(test)]
            particle_timer: None,
            #[cfg(test)]
            particle_step_timed: false,
        }
    }

    /// How the viewport assigns point lights to fragments: clustered, or per fragment on a
    /// device without the clustered path's storage buffers (#725).
    #[cfg(test)]
    pub(super) fn light_assignment(&self) -> render_engine::renderer::LightAssignment {
        self.viewport.light_assignment()
    }

    #[cfg(test)]
    pub(super) fn profile(&self) -> super::profile::FrameProfile {
        self.profile
    }

    /// Writes timestamps around each pass from the next frame on; the device needs
    /// `TIMESTAMP_QUERY`.
    #[cfg(test)]
    pub(super) fn enable_gpu_timing(&mut self, device: &wgpu::Device) {
        self.gpu_timer = Some(super::profile::GpuTimer::new(device, 64));
        self.particle_timer = Some(super::profile::GpuTimer::particle(device));
    }

    /// The last submitted frame's GPU time per pass, in milliseconds.
    #[cfg(test)]
    pub(super) fn gpu_passes(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Option<Vec<(&'static str, f64)>>, ClientError> {
        let mut passes = self
            .gpu_timer
            .as_ref()
            .map(|timer| timer.read(device, queue))
            .transpose()?;
        if self.particle_step_timed
            && let (Some(passes), Some(timer)) = (&mut passes, &self.particle_timer)
        {
            passes.splice(0..0, timer.read(device, queue)?);
        }
        Ok(passes)
    }

    #[cfg(test)]
    pub(super) fn observation(&self) -> ([u64; 4], usize) {
        (self.observation, self.meshes.len())
    }

    #[cfg(test)]
    pub(super) fn environment_emitter_count(&self) -> usize {
        self.effects.environment_emitters()
    }

    #[cfg(test)]
    pub(super) fn impact_emitters(&self) -> Vec<render_engine::world::EmitterId> {
        self.effects.impact_emitters()
    }

    /// Every live emitter and its particles alive after the last step (a GPU readback).
    #[cfg(test)]
    pub(super) fn alive_particles(&self) -> Vec<(render_engine::world::EmitterId, u32)> {
        self.world.read_alive_particles()
    }

    /// Screenshot-only fast-forward: warm the *real GPU simulation* without redrawing all
    /// Greyhaven scenery hundreds of times on a software Vulkan adapter. The frame's impacts
    /// burst when it is rendered, not here.
    #[cfg(test)]
    pub(super) fn prewarm_particles(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &WorldFrame,
        steps: u32,
    ) {
        let _ = self.effects.sync(
            &mut self.world,
            &frame.particle_sites,
            &[],
            frame.particle_seconds,
            frame.particles_animated,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("campfire screenshot warmup"),
        });
        for _ in 0..steps {
            self.world.step_particles(&mut encoder, STEP_SECONDS, None);
        }
        queue.submit([encoder.finish()]);
    }

    pub fn resize(&mut self, [width, height]: [u32; 2]) {
        self.viewport.resize(&self.renderer, width, height);
    }

    /// The mesh handle for `draw`: uploaded once per resource and rewritten in place when the
    /// resource's geometry changed. Skinned units keep their bind pose; only their palettes move.
    fn mesh(&mut self, draw: &SceneDraw) -> MeshId {
        let source = Arc::downgrade(&draw.mesh);
        if let Some(cached) = self.meshes.get_mut(&draw.resource_key) {
            cached.used = true;
            if cached.source.ptr_eq(&source) {
                #[cfg(test)]
                {
                    self.observation[1] += 1;
                }
                return cached.id;
            }
            cached.source = source;
            let data = draw_mesh_data(draw);
            #[cfg(test)]
            {
                self.observation[0] += 1;
                self.observation[2] += (data.positions.len() * 64 + data.indices.len() * 4) as u64;
            }
            self.world.set_mesh(cached.id, &data);
            return cached.id;
        }
        let data = draw_mesh_data(draw);
        #[cfg(test)]
        {
            self.observation[0] += 1;
            self.observation[2] += (data.positions.len() * 64 + data.indices.len() * 4) as u64;
        }
        let id = self.world.add_mesh(&data);
        self.meshes.insert(
            draw.resource_key.clone(),
            CachedMesh {
                id,
                source,
                used: true,
                batches: 0,
            },
        );
        id
    }

    /// The figure's skin for a skinned draw, its palette uploaded when it changed.
    fn skin(&mut self, skin: &DrawSkin) -> SkinId {
        let Self {
            world,
            skins,
            palette,
            ..
        } = self;
        let mut convert = || {
            palette.clear();
            palette.extend(skin.palette.iter().map(Mat4::from_cols_array));
        };
        let cached = match skins.get_mut(skin.figure.as_str()) {
            Some(cached) => {
                cached.used = true;
                if std::ptr::addr_eq(cached.palette.as_ptr(), Arc::as_ptr(&skin.palette)) {
                    return cached.id;
                }
                convert();
                world.set_skin(cached.id, palette);
                cached.palette = Arc::downgrade(&skin.palette);
                cached.id
            }
            None => {
                convert();
                let id = world.add_skin(palette);
                skins.insert(
                    skin.figure.clone(),
                    CachedSkin {
                        id,
                        palette: Arc::downgrade(&skin.palette),
                        used: true,
                    },
                );
                id
            }
        };
        #[cfg(test)]
        {
            self.observation[3] += 1;
        }
        cached
    }

    fn material(&mut self, material: &DrawMaterial) -> MaterialId {
        let key = material_key(material);
        let Self {
            world,
            ground,
            materials,
            ..
        } = self;
        let entry = materials.entry(key).or_insert_with(|| {
            let standard = match &material.ground {
                Some(draw) => ground
                    .get_or_insert_with(|| GroundGpu::upload(world))
                    .material(material, draw.ground),
                None => standard_material(material),
            };
            CachedMaterial {
                id: world.add_material(&standard),
                used: true,
                batches: 0,
            }
        });
        entry.used = true;
        entry.id
    }

    fn instance(&mut self, draw: &SceneDraw) -> Instance {
        Instance {
            mesh: self.mesh(draw),
            material: self.material(&draw.material),
            world_from_local: Mat4::from_cols_array(&draw.model.elements),
            skin: draw.skin.as_ref().map(|skin| self.skin(skin)),
        }
    }

    /// Draws `batch`, resolving it only the first time it is seen and retaining its instances
    /// in the world while it stays drawn (#728). A batch drawn twice in one frame lists its
    /// second copy.
    fn push_batch(&mut self, batch: &DrawBatch) {
        let key = self.resolve_batch(batch);
        let Self {
            world,
            batches,
            instances,
            ..
        } = self;
        let cached = batches.get_mut(&key).expect("resolved above");
        if cached.drawn {
            instances.extend_from_slice(&cached.instances);
            return;
        }
        cached.drawn = true;
        if cached.retained.is_empty() {
            cached.retained = cached
                .instances
                .iter()
                .map(|instance| world.add_instance(*instance))
                .collect();
        }
    }

    /// Retains this frame's trees and rocks (#728): a draw seen last frame keeps its instances,
    /// a new one adds them, and those no longer drawn are removed.
    fn push_nature(&mut self, frame: &WorldFrame) {
        let Self {
            nature,
            world,
            nature_drawn,
            nature_scratch,
            ..
        } = self;
        if !frame.nature.is_empty() {
            nature.upload(world, None);
        }
        for draw in &frame.nature {
            let retained = nature_drawn.entry(nature_key(draw)).or_default();
            if retained.used == retained.groups.len() {
                nature_scratch.clear();
                nature.instances(draw, nature_scratch);
                retained.groups.push(
                    nature_scratch
                        .iter()
                        .map(|instance| world.add_instance(*instance))
                        .collect(),
                );
            }
            retained.used += 1;
        }
        nature_drawn.retain(|_, retained| {
            for group in retained.groups.drain(retained.used..) {
                for id in group {
                    world.remove_instance(id);
                }
            }
            let keep = retained.used > 0;
            retained.used = 0;
            keep
        });
    }

    /// Uploads what `frame` draws into the world without drawing it, stopping at `deadline`
    /// (#726): the nature library, the static batches, then the frame's own draws. Returns true
    /// once all of it is resident, so the frame's first draw uploads nothing more. A window
    /// keeps presenting another frame meanwhile; the static batches stay resident while they
    /// live, the per-frame draws until a drawn frame no longer uses them.
    pub fn preload(&mut self, frame: &WorldFrame, deadline: std::time::Instant) -> bool {
        if !frame.nature.is_empty() && !self.nature.upload(&mut self.world, Some(deadline)) {
            return false;
        }
        for batch in &frame.batches {
            if self.batches.contains_key(&batch_key(batch)) {
                continue;
            }
            if past(Some(deadline)) {
                return false;
            }
            self.resolve_batch(batch);
        }
        for draw in frame.draws.iter().filter(|draw| drawn(draw)) {
            if past(Some(deadline)) {
                return false;
            }
            self.instance(draw);
        }
        true
    }

    /// Resolves `batch` into cached instances the first time it is seen; returns its key.
    fn resolve_batch(&mut self, batch: &DrawBatch) -> usize {
        let key = batch_key(batch);
        if !self.batches.contains_key(&key) {
            let mut instances = Vec::with_capacity(batch.len());
            let mut meshes = BTreeSet::new();
            let mut materials = BTreeSet::new();
            for draw in batch.iter().filter(|draw| drawn(draw)) {
                debug_assert!(draw.skin.is_none(), "a batch never changes, so never poses");
                instances.push(self.instance(draw));
                meshes.insert(draw.resource_key.as_str());
                materials.insert(material_key(&draw.material));
            }
            for mesh in &meshes {
                self.meshes.get_mut(*mesh).expect("resolved above").batches += 1;
            }
            for material in &materials {
                self.materials
                    .get_mut(material)
                    .expect("resolved above")
                    .batches += 1;
            }
            self.batches.insert(
                key,
                CachedBatch {
                    source: Arc::downgrade(batch),
                    instances,
                    retained: Vec::new(),
                    drawn: false,
                    meshes: meshes.into_iter().map(str::to_owned).collect(),
                    materials: materials.into_iter().collect(),
                },
            );
        }
        key
    }

    /// Drops the batches that are gone, then frees meshes whose resource was not drawn, whose
    /// geometry is gone and that no live batch uses (static scenery stays uploaded while
    /// culled), and materials neither drawn this frame nor used by a live batch.
    fn evict(&mut self) {
        let Self {
            world,
            meshes,
            materials,
            skins,
            batches,
            ..
        } = self;
        skins.retain(|_, cached| {
            let keep = cached.used;
            cached.used = false;
            if !keep {
                world.remove_skin(cached.id);
            }
            keep
        });
        batches.retain(|_, batch| {
            let live = batch.source.strong_count() > 0;
            // Retained instances go before their meshes and materials can be freed.
            if !(live && batch.drawn) {
                for id in batch.retained.drain(..) {
                    world.remove_instance(id);
                }
            }
            batch.drawn = false;
            if !live {
                for key in &batch.meshes {
                    if let Some(mesh) = meshes.get_mut(key) {
                        mesh.batches -= 1;
                    }
                }
                for key in &batch.materials {
                    if let Some(material) = materials.get_mut(key) {
                        material.batches -= 1;
                    }
                }
            }
            live
        });
        meshes.retain(|_, cached| {
            let keep = cached.used || cached.source.strong_count() > 0 || cached.batches > 0;
            cached.used = false;
            if !keep {
                world.remove_mesh(cached.id);
            }
            keep
        });
        materials.retain(|_, cached| {
            let keep = cached.used || cached.batches > 0;
            cached.used = false;
            if !keep {
                world.remove_material(cached.id);
            }
            keep
        });
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        frame: &WorldFrame,
        hud: &[HudRect],
        view: CameraView,
    ) -> Result<(), ClientError> {
        if hud.len() > MAX_HUD_RECTS {
            return Err("too many combat HUD rectangles".into());
        }
        self.instances.clear();
        for batch in &frame.batches {
            self.push_batch(batch);
        }
        for draw in frame.draws.iter().filter(|draw| drawn(draw)) {
            let instance = self.instance(draw);
            self.instances.push(instance);
        }
        self.push_nature(frame);
        self.evict();
        let particle_steps = self.effects.sync(
            &mut self.world,
            &frame.particle_sites,
            &frame.impacts,
            frame.particle_seconds,
            frame.particles_animated,
        );
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("presented world"),
        });
        #[cfg(test)]
        let particle_started = std::time::Instant::now();
        #[cfg(test)]
        {
            self.particle_step_timed = particle_steps > 0 && self.particle_timer.is_some();
        }
        for _step in 0..particle_steps {
            // The GPU runs this before the draw pass, at a bounded presentation-only dt.
            // Prepare follows the step, as in render-engine's kit_showcase.
            #[cfg(test)]
            let timestamps = if _step == 0 {
                self.particle_timer.as_ref().map(|timer| timer.stamp())
            } else {
                None
            };
            #[cfg(not(test))]
            let timestamps = None;
            self.world
                .step_particles(&mut encoder, STEP_SECONDS, timestamps);
        }
        #[cfg(test)]
        {
            self.profile.particle_step = particle_started.elapsed();
        }
        let scene = scene(&frame.lighting, view);
        #[cfg(test)]
        let started = std::time::Instant::now();
        self.renderer
            .prepare(&mut self.viewport, &self.world, &scene, &self.instances);
        #[cfg(test)]
        {
            self.profile.prepare = started.elapsed();
        }
        let hud_instances: Vec<_> = hud
            .iter()
            .map(|r| HudInstance {
                min: r.min,
                max: r.max,
                color: r.color,
            })
            .collect();
        if !hud_instances.is_empty() {
            queue.write_buffer(&self.hud_buffer, 0, bytemuck::cast_slice(&hud_instances));
        }
        #[cfg(not(test))]
        let timestamps = None;
        #[cfg(test)]
        let timestamps = self
            .gpu_timer
            .as_mut()
            .map(|timer| timer.frame(device, &self.viewport));
        #[cfg(test)]
        let started = std::time::Instant::now();
        self.renderer.encode(
            &mut encoder,
            &self.viewport,
            &self.world,
            target,
            timestamps,
        );
        #[cfg(test)]
        {
            self.profile.encode = started.elapsed();
            self.profile.work = self.viewport.work();
            self.profile.world = self.world.work();
            if let Some(timer) = &self.gpu_timer {
                timer.resolve(&mut encoder);
            }
            if self.particle_step_timed
                && let Some(timer) = &self.particle_timer
            {
                timer.resolve(&mut encoder);
            }
        }
        if !hud_instances.is_empty() {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("combat HUD"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.hud_pipeline);
            pass.set_vertex_buffer(0, self.hud_buffer.slice(..));
            pass.draw(0..6, 0..u32::try_from(hud_instances.len())?);
        }
        queue.submit([encoder.finish()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposure_keeps_daylight_contrast_and_brightens_night() {
        assert_eq!(ev100(60.0), 15.0);
        assert_eq!(ev100(90.0), 15.0);
        assert_eq!(ev100(15.0), 13.8);
        assert_eq!(ev100(2.0), 11.0);
        assert_eq!(ev100(-40.0), 2.0);
        assert!(ev100(-8.0) > 2.0 && ev100(-8.0) < 6.0);
    }

    #[test]
    fn the_zone_clock_moves_the_sun_and_moon() {
        const VIEW: CameraView = CameraView {
            field_of_view_degrees: 50.0,
            eye: [0.0, 5.0, 10.0],
            target: [0.0, 0.0, 0.0],
        };
        let noon = scene(&Lighting::at(12.0), VIEW);
        let night = scene(&Lighting::at(0.0), VIEW);
        assert!(noon.sun.unwrap().direction.y < -0.8, "noon sun shines down");
        assert!(noon.sun.unwrap().shadows.is_some());
        assert!(
            night.sun.unwrap().shadows.is_none(),
            "no sun shadows at night"
        );
        assert!(
            night.moon.unwrap().direction.y < -0.8,
            "midnight moon shines down"
        );
        assert!(noon.camera.ev100 > night.camera.ev100);
    }

    #[test]
    fn a_skinned_draw_uploads_its_bind_pose_with_joints_and_weights() {
        let cube = Mesh::unit_cube();
        let mut attributes = cube.attributes().clone();
        let count = cube.vertices().len();
        attributes.normals = Some(vec![three_d_core::Vec3::new(0., 1., 0.); count]);
        let cube = Mesh::with_attributes(
            cube.vertices().to_vec(),
            cube.indices().to_vec(),
            attributes,
        )
        .unwrap();
        let draw = SceneDraw {
            id: "unit".into(),
            resource_key: "unit".into(),
            mesh: Arc::new(cube.clone()),
            model: three_d_animation::Mat4::IDENTITY,
            material: DrawMaterial {
                color: [1.; 3],
                emissive: [0.; 3],
                opacity: 1.,
                unlit: false,
                double_sided: false,
                ground: None,
            },
            skin: Some(crate::world::DrawSkin {
                figure: "unit".into(),
                influences: Arc::new(crate::world::Influences {
                    joints: vec![[1, 0, 0, 0]; count],
                    weights: vec![[1., 0., 0., 0.]; count],
                }),
                palette: Arc::from([Mat4::IDENTITY.to_cols_array(); 2]),
            }),
        };
        let data = draw_mesh_data(&draw);
        assert_eq!(data.positions[0].x, cube.vertices()[0].x, "the bind pose");
        assert_eq!(data.joints, vec![[1, 0, 0, 0]; count]);
        assert_eq!(data.weights, vec![Vec4::X; count]);
    }

    #[test]
    fn unnormalled_meshes_are_flat_shaded_with_linear_vertex_colors() {
        let mesh = Mesh::unit_cube();
        let data = mesh_data(&mesh);
        assert_eq!(data.positions.len(), data.indices.len());
        assert_eq!(data.normals.len(), data.positions.len());
        assert!(data.colors.is_empty() || data.colors.len() == data.positions.len());
    }
}
