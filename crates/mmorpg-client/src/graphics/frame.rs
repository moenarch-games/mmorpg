//! The application's renderer-neutral draw frames on render-engine (#529): meshes and
//! materials live in its `World` behind stable handles, the lighting is its physical `Scene`
//! (sun, moon, sky, cascaded shadows, sky ambient light, fog, point lights), and the combat HUD
//! is drawn over its tone-mapped output.
use super::ClientError;
use crate::{
    camera::CameraView,
    environment::Lighting,
    hud::{HudRect, MAX_HUD_RECTS},
    world::{DrawBatch, DrawMaterial, SceneDraw, WorldFrame},
};
use bytemuck::{Pod, Zeroable};
use render_engine::{
    glam::{Mat4, Vec3, Vec4},
    material::{AlphaMode, StandardMaterial},
    mesh::MeshData,
    renderer::{Renderer, Viewport},
    scene::{
        Ambient, Camera, DayCycle, Fog, FogFalloff, Moon, PointLight, RAW_SUNLIGHT, Scene,
        ShadowFilter, ShadowMapSizes, Sky, Sun, SunShadows, Tonemapping,
    },
    world::{Instance, MaterialId, MeshId, World},
};
use std::{
    collections::{BTreeSet, HashMap},
    mem,
    sync::{Arc, Weak},
};
use three_d_core::{Color3, Mesh};

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
/// oracle views at noon, 15° and 2°, and a brighter-than-physical night for play.
pub const EXPOSURE: [(f32, f32); 5] = [
    (-12.0, 2.0),
    (-4.0, 6.0),
    (2.0, 11.0),
    (15.0, 13.5),
    (60.0, 14.5),
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
    /// Kept once the resource is rewritten (CPU-skinned units).
    rewrite: Option<Rewrite>,
}

/// A rewritten resource's last converted data and source colours (#588). A skinned unit's
/// next pose only moves its vertices: its colours and triangles are reused, not converted again.
struct Rewrite {
    data: MeshData,
    colors: Option<Vec<Color3>>,
}

impl Rewrite {
    fn new(mesh: &Mesh) -> Self {
        Self {
            data: mesh_data(mesh),
            colors: mesh.attributes().colors.clone(),
        }
    }

    /// Takes `mesh`'s positions and normals in place when its colours and triangles are the
    /// ones converted last.
    fn update(&mut self, mesh: &Mesh) -> Update {
        let attributes = mesh.attributes();
        let Some(normals) = &attributes.normals else {
            return Update::Convert;
        };
        if mesh.vertices().len() != self.data.positions.len()
            || mesh.indices() != self.data.indices.as_slice()
            || attributes.colors != self.colors
        {
            return Update::Convert;
        }
        let mut moved = false;
        for (out, p) in self.data.positions.iter_mut().zip(mesh.vertices()) {
            let p = Vec3::new(p.x, p.y, p.z);
            moved |= *out != p;
            *out = p;
        }
        for (out, n) in self.data.normals.iter_mut().zip(normals) {
            let n = Vec3::new(n.x, n.y, n.z);
            moved |= *out != n;
            *out = n;
        }
        if moved {
            Update::Moved
        } else {
            Update::Unchanged
        }
    }
}

/// How a rewritten resource's new geometry differs from its last upload.
#[derive(PartialEq)]
enum Update {
    /// Same vertices: nothing to upload.
    Unchanged,
    /// Only positions or normals changed.
    Moved,
    /// Converted anew.
    Convert,
}

struct CachedMaterial {
    id: MaterialId,
    used: bool,
    /// Live cached batches whose instances use this material.
    batches: u32,
}

/// A material's exact field bits.
type MaterialKey = [u32; 9];

/// A [`DrawBatch`] resolved into instances once (#588). It stays valid while the batch lives,
/// drawn or culled, and is dropped with it; its meshes and materials stay resident until then.
struct CachedBatch {
    source: Weak<[SceneDraw]>,
    instances: Vec<Instance>,
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
    ]
}

fn standard_material(m: &DrawMaterial) -> StandardMaterial {
    StandardMaterial {
        base_color: linear3(m.color).extend(m.opacity),
        emissive: linear3(m.emissive) * EMISSIVE_NITS,
        // The browser's MeshStandardMaterial: roughness 0.86, metalness 0.02.
        perceptual_roughness: 0.86,
        metallic: 0.02,
        alpha_mode: if m.opacity < 1. {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        },
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
    Scene {
        camera: Camera {
            eye: Vec3::from(view.eye),
            target: Vec3::from(view.target),
            fov_y: view.field_of_view_degrees.to_radians(),
            near: NEAR_PLANE_METRES,
            ev100: ev100(elevation),
            shadow_filter: ShadowFilter::Gaussian,
            tonemapping: Tonemapping::TonyMcMapface,
        },
        clear_color: Vec3::ZERO,
        sun: Some(Sun {
            direction: -to_sun,
            color: Vec3::ONE,
            illuminance: RAW_SUNLIGHT,
            shadows: (elevation > SHADOWLESS_BELOW).then_some(SUN_SHADOWS),
        }),
        moon: Some(Moon {
            direction: -DAY_CYCLE.to_moon(lighting.hour),
            color: MOON_COLOR,
            illuminance: MOON_LUX,
        }),
        // The sky's environment light is the ambient light.
        sky: Some(Sky::default()),
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
            falloff: FogFalloff::Linear {
                start: fog.near,
                end: fog.far,
            },
            ..Fog::default()
        }),
    }
}

pub(super) struct FrameRenderer {
    renderer: Renderer,
    world: World,
    viewport: Viewport,
    meshes: HashMap<String, CachedMesh>,
    materials: HashMap<MaterialKey, CachedMaterial>,
    batches: HashMap<usize, CachedBatch>,
    /// This frame's instances; the allocation is kept across frames.
    instances: Vec<Instance>,
    hud_pipeline: wgpu::RenderPipeline,
    hud_buffer: wgpu::Buffer,
    /// Mesh uploads (new or rewritten), reuses and uploaded vertex bytes.
    #[cfg(test)]
    observation: [u64; 3],
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
            viewport,
            meshes: HashMap::new(),
            materials: HashMap::new(),
            batches: HashMap::new(),
            instances: Vec::new(),
            hud_pipeline,
            hud_buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("combat HUD"),
                size: (MAX_HUD_RECTS * mem::size_of::<HudInstance>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            #[cfg(test)]
            observation: [0; 3],
        }
    }

    #[cfg(test)]
    pub(super) fn observation(&self) -> ([u64; 3], usize) {
        (self.observation, self.meshes.len())
    }

    pub fn resize(&mut self, [width, height]: [u32; 2]) {
        self.viewport.resize(&self.renderer, width, height);
    }

    /// The mesh handle for `draw`: uploaded once per resource and rewritten in place when the
    /// resource's geometry changed (CPU-skinned units, every frame).
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
            let update = cached
                .rewrite
                .as_mut()
                .map_or(Update::Convert, |rewrite| rewrite.update(&draw.mesh));
            if update == Update::Unchanged {
                #[cfg(test)]
                {
                    self.observation[1] += 1;
                }
                return cached.id;
            }
            if update == Update::Convert {
                cached.rewrite = Some(Rewrite::new(&draw.mesh));
            }
            let data = &cached.rewrite.as_ref().expect("converted above").data;
            #[cfg(test)]
            {
                self.observation[0] += 1;
                self.observation[2] += (data.positions.len() * 64 + data.indices.len() * 4) as u64;
            }
            self.world.set_mesh(cached.id, data);
            return cached.id;
        }
        let data = mesh_data(&draw.mesh);
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
                rewrite: None,
            },
        );
        id
    }

    fn material(&mut self, material: &DrawMaterial) -> MaterialId {
        let key = material_key(material);
        let world = &mut self.world;
        let entry = self.materials.entry(key).or_insert_with(|| CachedMaterial {
            id: world.add_material(&standard_material(material)),
            used: true,
            batches: 0,
        });
        entry.used = true;
        entry.id
    }

    fn instance(&mut self, draw: &SceneDraw) -> Instance {
        Instance {
            mesh: self.mesh(draw),
            material: self.material(&draw.material),
            world_from_local: Mat4::from_cols_array(&draw.model.elements),
        }
    }

    /// Appends `batch`'s instances, resolving them only the first time the batch is drawn.
    fn push_batch(&mut self, batch: &DrawBatch) {
        let key = batch_key(batch);
        if !self.batches.contains_key(&key) {
            let mut instances = Vec::with_capacity(batch.len());
            let mut meshes = BTreeSet::new();
            let mut materials = BTreeSet::new();
            for draw in batch.iter().filter(|draw| drawn(draw)) {
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
                    meshes: meshes.into_iter().map(str::to_owned).collect(),
                    materials: materials.into_iter().collect(),
                },
            );
        }
        self.instances
            .extend_from_slice(&self.batches[&key].instances);
    }

    /// Drops the batches that are gone, then frees meshes whose resource was not drawn, whose
    /// geometry is gone and that no live batch uses (static scenery stays uploaded while
    /// culled), and materials neither drawn this frame nor used by a live batch.
    fn evict(&mut self) {
        let Self {
            world,
            meshes,
            materials,
            batches,
            ..
        } = self;
        batches.retain(|_, batch| {
            let live = batch.source.strong_count() > 0;
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
        self.evict();
        let scene = scene(&frame.lighting, view);
        self.renderer
            .prepare(&mut self.viewport, &self.world, &scene, &self.instances);
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
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("presented world"),
        });
        self.renderer
            .encode(&mut encoder, &self.viewport, &self.world, target, None);
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
    fn exposure_follows_the_oracle_views_and_brightens_night() {
        assert_eq!(ev100(60.0), 14.5);
        assert_eq!(ev100(90.0), 14.5);
        assert_eq!(ev100(15.0), 13.5);
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
    fn a_rewritten_mesh_reconverts_only_when_more_than_its_vertices_moved() {
        let normals = |mesh: &Mesh| {
            let mut attributes = mesh.attributes().clone();
            attributes.normals = Some(vec![
                three_d_core::Vec3::new(0., 1., 0.);
                mesh.vertices().len()
            ]);
            attributes.colors = Some(vec![Color3::new(0.5, 0.5, 0.5); mesh.vertices().len()]);
            Mesh::with_attributes(
                mesh.vertices().to_vec(),
                mesh.indices().to_vec(),
                attributes,
            )
            .unwrap()
        };
        let cube = normals(&Mesh::unit_cube());
        let mut rewrite = Rewrite::new(&cube);
        assert!(rewrite.update(&cube) == Update::Unchanged);
        let mut moved = cube.vertices().to_vec();
        moved[0].x += 1.;
        let moved =
            Mesh::with_attributes(moved, cube.indices().to_vec(), cube.attributes().clone())
                .unwrap();
        assert!(rewrite.update(&moved) == Update::Moved);
        assert_eq!(rewrite.data.positions[0].x, moved.vertices()[0].x);
        let mut recoloured = moved.attributes().clone();
        recoloured.colors = Some(vec![Color3::new(1., 0., 0.); moved.vertices().len()]);
        let recoloured = Mesh::with_attributes(
            moved.vertices().to_vec(),
            moved.indices().to_vec(),
            recoloured,
        )
        .unwrap();
        assert!(rewrite.update(&recoloured) == Update::Convert);
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
