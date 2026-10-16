//! Retained wgpu resources for the application's renderer-neutral draw frames.
use super::{ClientError, DEPTH_FORMAT, scene_camera};
use crate::{
    camera::CameraView,
    hud::HudRect,
    world::{SceneDraw, WorldFrame},
};
use bytemuck::{Pod, Zeroable};
use std::{
    collections::{BTreeMap, BTreeSet},
    mem,
    sync::{Arc, Weak},
};
use three_d_core::Mesh;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [f32; 16],
    normal: [[f32; 4]; 3],
    color: [f32; 4],
    emissive: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Uniforms {
    camera: [f32; 16],
    eye: [f32; 4],
    direction: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    sun_direction: [f32; 4],
    sun: [f32; 4],
    fog: [f32; 4],
    settings: [f32; 4],
    zenith: [f32; 4],
    upper: [f32; 4],
    horizon: [f32; 4],
    sky_ground: [f32; 4],
    clouds: [f32; 4],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HudInstance {
    min: [f32; 2],
    max: [f32; 2],
    color: [f32; 3],
}
struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    source: Weak<Mesh>,
}
struct Draw {
    key: String,
    pipeline: usize,
    instance: Instance,
    depth: f32,
}
fn linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
fn color(c: [f32; 3], w: f32) -> [f32; 4] {
    [linear(c[0]), linear(c[1]), linear(c[2]), w]
}
impl GpuMesh {
    fn upload(device: &wgpu::Device, draw: &SceneDraw) -> Result<Self, ClientError> {
        let mesh = &draw.mesh;
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        if let Some(normals) = &mesh.attributes().normals {
            for (i, p) in mesh.vertices().iter().enumerate() {
                let n = normals[i];
                let c = mesh
                    .attributes()
                    .colors
                    .as_ref()
                    .map_or([1.; 3], |c| [c[i].r, c[i].g, c[i].b]);
                vertices.push(Vertex {
                    position: [p.x, p.y, p.z],
                    normal: [n.x, n.y, n.z],
                    color: c,
                });
            }
            indices.extend_from_slice(mesh.indices());
        } else {
            for (triangle, tri) in mesh.indices().chunks_exact(3).enumerate() {
                let Some(n) = mesh.triangle_normal(triangle) else {
                    continue;
                };
                for index in tri {
                    let i = *index as usize;
                    let p = mesh.vertices()[i];
                    let c = mesh
                        .attributes()
                        .colors
                        .as_ref()
                        .map_or([1.; 3], |c| [c[i].r, c[i].g, c[i].b]);
                    indices.push(u32::try_from(vertices.len())?);
                    vertices.push(Vertex {
                        position: [p.x, p.y, p.z],
                        normal: [n.x, n.y, n.z],
                        color: c,
                    });
                }
            }
        }
        Ok(Self {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&draw.resource_key),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("scene indices"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: u32::try_from(indices.len())?,
            source: Arc::downgrade(mesh),
        })
    }
}
impl Instance {
    fn new(draw: &SceneDraw) -> Result<Self, ClientError> {
        let inverse = three_d_projective::inverse(draw.model)?;
        let m = inverse.elements;
        Ok(Self {
            model: draw.model.elements,
            normal: [
                [m[0], m[4], m[8], 0.],
                [m[1], m[5], m[9], 0.],
                [m[2], m[6], m[10], 0.],
            ],
            color: color(draw.material.color, draw.material.opacity),
            emissive: color(
                draw.material.emissive,
                if draw.material.unlit { 1. } else { 0. },
            ),
        })
    }
}

pub(super) struct Target<'a> {
    pub view: &'a wgpu::TextureView,
    pub depth: &'a wgpu::TextureView,
    pub viewport: [u32; 2],
}

pub(super) struct FrameRenderer {
    pipelines: Vec<wgpu::RenderPipeline>,
    sky_pipeline: wgpu::RenderPipeline,
    hud_pipeline: wgpu::RenderPipeline,
    uniforms: wgpu::Buffer,
    binding: wgpu::BindGroup,
    instances: wgpu::Buffer,
    capacity: usize,
    meshes: BTreeMap<String, GpuMesh>,
}
impl FrameRenderer {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("world environment"),
            size: mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let binding_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("world environment"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("world environment"),
            layout: &binding_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("world frame"),
            bind_group_layouts: &[Some(&binding_layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("world frame"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../world.wgsl").into()),
        });
        const VA: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x3,2=>Float32x3];
        const IA: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![3=>Float32x4,4=>Float32x4,5=>Float32x4,6=>Float32x4,7=>Float32x4,8=>Float32x4,9=>Float32x4,10=>Float32x4,11=>Float32x4];
        let buffers = [
            Some(wgpu::VertexBufferLayout {
                array_stride: mem::size_of::<Vertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &VA,
            }),
            Some(wgpu::VertexBufferLayout {
                array_stride: mem::size_of::<Instance>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &IA,
            }),
        ];
        let pipelines = (0..8)
            .map(|flags| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("world materials"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &buffers,
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: if flags & 1 == 0 {
                            Some(wgpu::Face::Back)
                        } else {
                            None
                        },
                        front_face: if flags & 2 == 0 {
                            wgpu::FrontFace::Ccw
                        } else {
                            wgpu::FrontFace::Cw
                        },
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: if flags & 4 != 0 {
                                Some(wgpu::BlendState::ALPHA_BLENDING)
                            } else {
                                None
                            },
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            })
            .collect();
        let sky_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sky gradient and clouds"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("sky_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("sky_fragment"),
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
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
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
            pipelines,
            sky_pipeline,
            hud_pipeline,
            uniforms,
            binding,
            instances: instance_buffer(device, 1),
            capacity: 1,
            meshes: BTreeMap::new(),
        }
    }
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: Target<'_>,
        frame: &WorldFrame,
        hud: &[HudRect],
        view: CameraView,
    ) -> Result<(), ClientError> {
        let Target {
            view: target,
            depth,
            viewport,
        } = target;
        let camera = scene_camera(view, viewport)?;
        let e = &frame.environment;
        let d = std::array::from_fn::<_, 3, _>(|i| view.target[i] - view.eye[i]);
        let distance = d.iter().map(|v| v * v).sum::<f32>().sqrt();
        let horizontal = d[0].hypot(d[2]).max(f32::EPSILON);
        let horizon =
            ((0.5 + d[1] / horizontal / (2. * 25_f32.to_radians().tan())).clamp(-0.5, 1.5) * 1000.)
                .round()
                / 1000.;
        let u = Uniforms {
            camera: camera.view_projection_matrix().elements,
            eye: [view.eye[0], view.eye[1], view.eye[2], 0.],
            direction: [d[0] / distance, d[1] / distance, d[2] / distance, 0.],
            sky: color(e.sky.sky_color, e.sky.intensity),
            ground: color(e.sky.ground_color, 0.),
            sun_direction: [
                e.sun.direction[0],
                e.sun.direction[1],
                e.sun.direction[2],
                e.sun.intensity,
            ],
            sun: color(e.sun.color, 0.),
            fog: color(e.fog.color, e.fog.near),
            settings: [e.fog.far, viewport[0] as f32, viewport[1] as f32, horizon],
            zenith: [
                e.sky_stops.zenith[0],
                e.sky_stops.zenith[1],
                e.sky_stops.zenith[2],
                0.,
            ],
            upper: [
                e.sky_stops.upper[0],
                e.sky_stops.upper[1],
                e.sky_stops.upper[2],
                0.,
            ],
            horizon: [
                e.sky_stops.horizon[0],
                e.sky_stops.horizon[1],
                e.sky_stops.horizon[2],
                0.,
            ],
            sky_ground: [
                e.sky_stops.ground[0],
                e.sky_stops.ground[1],
                e.sky_stops.ground[2],
                0.,
            ],
            clouds: [e.cloud_offset, 0., 0., 0.],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&u));
        let mut draws = Vec::new();
        for draw in &frame.draws {
            if draw.material.opacity <= 0. || draw.mesh.indices().is_empty() {
                continue;
            }
            let instance = Instance::new(draw)?;
            if self
                .meshes
                .get(&draw.resource_key)
                .is_none_or(|g| !g.source.ptr_eq(&Arc::downgrade(&draw.mesh)))
            {
                self.meshes
                    .insert(draw.resource_key.clone(), GpuMesh::upload(device, draw)?);
            }
            let m = draw.model.elements;
            let det = m[0] * (m[5] * m[10] - m[9] * m[6]) - m[4] * (m[1] * m[10] - m[9] * m[2])
                + m[8] * (m[1] * m[6] - m[5] * m[2]);
            let flags = usize::from(draw.material.double_sided)
                | if det < 0. { 2 } else { 0 }
                | if draw.material.opacity < 1. { 4 } else { 0 };
            let centre = draw
                .mesh
                .bounds()
                .map(|b| {
                    [
                        (b.min.x + b.max.x) * 0.5,
                        (b.min.y + b.max.y) * 0.5,
                        (b.min.z + b.max.z) * 0.5,
                    ]
                })
                .unwrap_or([0.; 3]);
            let p = std::array::from_fn::<_, 3, _>(|r| {
                m[r] * centre[0] + m[r + 4] * centre[1] + m[r + 8] * centre[2] + m[r + 12]
                    - view.eye[r]
            });
            let z = p.iter().zip(d).map(|(a, b)| a * b).sum();
            draws.push(Draw {
                key: draw.resource_key.clone(),
                pipeline: flags,
                instance,
                depth: z,
            });
        }
        draws.sort_by(|a, b| match (a.pipeline & 4 != 0, b.pipeline & 4 != 0) {
            (false, false) => (&a.key, a.pipeline).cmp(&(&b.key, b.pipeline)),
            (true, true) => b.depth.total_cmp(&a.depth),
            (false, true) => std::cmp::Ordering::Less,
            (true, false) => std::cmp::Ordering::Greater,
        });
        let used: BTreeSet<_> = draws.iter().map(|d| d.key.as_str()).collect();
        self.meshes.retain(|k, _| used.contains(k.as_str()));
        if draws.len() > self.capacity {
            self.capacity = draws.len().next_power_of_two();
            self.instances = instance_buffer(device, self.capacity);
        }
        let instances: Vec<_> = draws.iter().map(|d| d.instance).collect();
        if !instances.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }
        let hud_instances: Vec<_> = hud
            .iter()
            .map(|r| HudInstance {
                min: r.min,
                max: r.max,
                color: r.color,
            })
            .collect();
        let hud_buffer = (!hud_instances.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("combat HUD"),
                contents: bytemuck::cast_slice(&hud_instances),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("presented world"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("presented world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(linear(e.background[0])),
                            g: f64::from(linear(e.background[1])),
                            b: f64::from(linear(e.background[2])),
                            a: 1.,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.binding, &[]);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            pass.set_vertex_buffer(1, self.instances.slice(..));
            let mut start = 0;
            while start < draws.len() {
                let draw = &draws[start];
                let mut end = start + 1;
                // Transparent objects retain sorted order; only adjacent compatible draws coalesce.
                while end < draws.len()
                    && draws[end].key == draw.key
                    && draws[end].pipeline == draw.pipeline
                {
                    end += 1;
                }
                let mesh = &self.meshes[&draw.key];
                pass.set_pipeline(&self.pipelines[draw.pipeline]);
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.count, 0, u32::try_from(start)?..u32::try_from(end)?);
                start = end;
            }
            if let Some(buffer) = &hud_buffer {
                pass.set_pipeline(&self.hud_pipeline);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..6, 0..u32::try_from(hud_instances.len())?);
            }
        }
        queue.submit([encoder.finish()]);
        Ok(())
    }
}
fn instance_buffer(device: &wgpu::Device, count: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("world instances"),
        size: (count * mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
