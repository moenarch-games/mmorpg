//! Native GPU adapter for shared 3d-lab geometry/camera models. Two pipelines
//! share one camera binding: indexed vertex-coloured meshes (terrain, water)
//! and lit instanced boxes (static props uploaded once, units every frame). A
//! third draws the flat combat HUD rectangles over the world.
use crate::{
    ClientError,
    camera::CameraView,
    hud::{HudRect, MAX_HUD_RECTS},
    presentation::{MAX_SCENE_BOXES, SceneBox},
    world::{Mesh as WorldMesh, WorldScene},
};
use bytemuck::{Pod, Zeroable};
use std::{mem, sync::Arc, time::Duration};
use three_d_camera::PerspectiveCamera;
use three_d_core::{Mesh, Vec3};
use wgpu::util::DeviceExt;
use winit::window::Window;

const MAX_DYNAMIC_INSTANCES: usize = MAX_SCENE_BOXES;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// Sees across the 240 m vale to the mountains 200 m out.
const FAR_PLANE_METRES: f32 = 600.0;
const SKY: wgpu::Color = wgpu::Color {
    r: 0.46,
    g: 0.62,
    b: 0.82,
    a: 1.0,
};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Instance {
    position: [f32; 3],
    size: [f32; 3],
    color: [f32; 3],
    yaw: f32,
}
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ColoredVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct HudInstance {
    min: [f32; 2],
    max: [f32; 2],
    color: [f32; 3],
}

impl From<&HudRect> for HudInstance {
    fn from(rect: &HudRect) -> Self {
        Self {
            min: rect.min,
            max: rect.max,
            color: rect.color,
        }
    }
}

impl From<&SceneBox> for Instance {
    fn from(item: &SceneBox) -> Self {
        Self {
            position: item.position,
            size: item.size,
            color: item.color,
            yaw: item.yaw,
        }
    }
}

struct GpuMesh {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_count: u32,
}

impl GpuMesh {
    fn upload(
        device: &wgpu::Device,
        label: &str,
        mesh: &WorldMesh,
    ) -> Result<Option<Self>, ClientError> {
        if mesh.indices.is_empty() {
            return Ok(None);
        }
        let vertices: Vec<_> = mesh
            .vertices
            .iter()
            .map(|vertex| ColoredVertex {
                position: vertex.position,
                normal: vertex.normal,
                color: vertex.color,
            })
            .collect();
        Ok(Some(Self {
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            }),
            index_count: u32::try_from(mesh.indices.len())?,
        }))
    }
}

pub struct SceneRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    box_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    hud_pipeline: wgpu::RenderPipeline,
    hud_instances: wgpu::Buffer,
    cube: wgpu::Buffer,
    cube_vertex_count: u32,
    static_instances: wgpu::Buffer,
    static_count: u32,
    dynamic_instances: wgpu::Buffer,
    meshes: Vec<GpuMesh>,
    camera: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    depth: wgpu::TextureView,
    width: u32,
    height: u32,
}

impl SceneRenderer {
    async fn new(
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        world: &WorldScene,
    ) -> Result<Self, ClientError> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("MMORPG client"),
                ..Default::default()
            })
            .await?;
        let mesh = Mesh::unit_cube();
        let mut vertices = Vec::new();
        for (triangle, indices) in mesh.indices().as_chunks::<3>().0.iter().enumerate() {
            let normal = mesh
                .triangle_normal(triangle)
                .ok_or("cube triangle has no normal")?;
            for index in indices {
                let vertex = mesh.vertices()[*index as usize];
                vertices.push(Vertex {
                    position: [vertex.x, vertex.y, vertex.z],
                    normal: [normal.x, normal.y, normal.z],
                });
            }
        }
        let cube_vertex_count = u32::try_from(vertices.len())?;
        let cube = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shared cube mesh"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        // Zero-sized buffers are invalid: an empty world keeps one unused slot.
        let mut static_boxes: Vec<Instance> = world.props.iter().map(Instance::from).collect();
        let static_count = u32::try_from(static_boxes.len())?;
        if static_boxes.is_empty() {
            static_boxes.push(Instance::zeroed());
        }
        let static_instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("static prop instances"),
            contents: bytemuck::cast_slice(&static_boxes),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let dynamic_instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded unit instances"),
            size: (MAX_DYNAMIC_INSTANCES * mem::size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut meshes = Vec::new();
        for (label, mesh) in [
            ("terrain mesh", &world.terrain),
            ("water mesh", &world.water),
        ] {
            meshes.extend(GpuMesh::upload(&device, label, mesh)?);
        }
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scene layout"),
            bind_group_layouts: &[Some(&camera_layout)],
            immediate_size: 0,
        });
        let box_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("box shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("scene.wgsl").into()),
        });
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mesh.wgsl").into()),
        });
        const VERTEX_ATTRIBUTES: [wgpu::VertexAttribute; 2] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        const INSTANCE_ATTRIBUTES: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
            2 => Float32x3,
            3 => Float32x3,
            4 => Float32x3,
            5 => Float32
        ];
        const COLORED_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3];
        let pipeline = |label: &str,
                        shader: &wgpu::ShaderModule,
                        buffers: &[Option<wgpu::VertexBufferLayout<'_>>],
                        cull_mode: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    cull_mode,
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
                    module: shader,
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
            })
        };
        let box_pipeline = pipeline(
            "box pipeline",
            &box_shader,
            &[
                Some(wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &VERTEX_ATTRIBUTES,
                }),
                Some(wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<Instance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &INSTANCE_ATTRIBUTES,
                }),
            ],
            Some(wgpu::Face::Back),
        );
        // Terrain is only seen from above, but culling is off so winding never hides it.
        let mesh_pipeline = pipeline(
            "mesh pipeline",
            &mesh_shader,
            &[Some(wgpu::VertexBufferLayout {
                array_stride: mem::size_of::<ColoredVertex>() as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &COLORED_ATTRIBUTES,
            })],
            None,
        );
        let hud_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("hud layout"),
            bind_group_layouts: &[],
            immediate_size: 0,
        });
        let hud_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("hud shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hud.wgsl").into()),
        });
        const HUD_ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x3];
        // Drawn last over everything: no depth test, no depth write.
        let hud_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("hud pipeline"),
            layout: Some(&hud_layout),
            vertex: wgpu::VertexState {
                module: &hud_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: mem::size_of::<HudInstance>() as u64,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &HUD_ATTRIBUTES,
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
        let hud_instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bounded hud rectangles"),
            size: (MAX_HUD_RECTS * mem::size_of::<HudInstance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera binding"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let depth = depth_texture(&device, width, height);
        Ok(Self {
            device,
            queue,
            box_pipeline,
            mesh_pipeline,
            hud_pipeline,
            hud_instances,
            cube,
            cube_vertex_count,
            static_instances,
            static_count,
            dynamic_instances,
            meshes,
            camera,
            camera_bind_group,
            depth,
            width,
            height,
        })
    }

    fn render(
        &self,
        target: &wgpu::TextureView,
        scene: &[SceneBox],
        hud: &[HudRect],
        view: CameraView,
    ) -> Result<(), ClientError> {
        if scene.len() > MAX_DYNAMIC_INSTANCES {
            return Err("scene exceeds instance capacity".into());
        }
        if hud.len() > MAX_HUD_RECTS {
            return Err("HUD exceeds rectangle capacity".into());
        }
        let hud_instances: Vec<_> = hud.iter().map(HudInstance::from).collect();
        let instances: Vec<_> = scene.iter().map(Instance::from).collect();
        let camera = scene_camera(view, [self.width, self.height])?;
        self.queue.write_buffer(
            &self.camera,
            0,
            bytemuck::cast_slice(&camera.view_projection_matrix().elements),
        );
        if !instances.is_empty() {
            self.queue
                .write_buffer(&self.dynamic_instances, 0, bytemuck::cast_slice(&instances));
        }
        if !hud_instances.is_empty() {
            self.queue
                .write_buffer(&self.hud_instances, 0, bytemuck::cast_slice(&hud_instances));
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("scene frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("opaque world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(SKY),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            pass.set_pipeline(&self.mesh_pipeline);
            for mesh in &self.meshes {
                pass.set_vertex_buffer(0, mesh.vertices.slice(..));
                pass.set_index_buffer(mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
            pass.set_pipeline(&self.box_pipeline);
            pass.set_vertex_buffer(0, self.cube.slice(..));
            pass.set_vertex_buffer(1, self.static_instances.slice(..));
            pass.draw(0..self.cube_vertex_count, 0..self.static_count);
            pass.set_vertex_buffer(1, self.dynamic_instances.slice(..));
            pass.draw(0..self.cube_vertex_count, 0..u32::try_from(scene.len())?);
            if !hud_instances.is_empty() {
                pass.set_pipeline(&self.hud_pipeline);
                pass.set_vertex_buffer(0, self.hud_instances.slice(..));
                pass.draw(0..6, 0..u32::try_from(hud_instances.len())?);
            }
        }
        self.queue.submit([encoder.finish()]);
        Ok(())
    }
}

fn depth_texture(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

fn scene_camera(view: CameraView, viewport: [u32; 2]) -> Result<PerspectiveCamera, ClientError> {
    Ok(PerspectiveCamera::new(
        Vec3::new(view.eye[0], view.eye[1], view.eye[2]),
        Vec3::new(view.target[0], view.target[1], view.target[2]),
        Vec3::new(0.0, 1.0, 0.0),
        50_f32.to_radians(),
        viewport[0] as f32 / viewport[1] as f32,
        0.1,
        FAR_PLANE_METRES,
    )?)
}

pub struct WindowRenderer {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: SceneRenderer,
    suspended: bool,
    picking: crate::picking::WorldPicking,
    last_view: Option<CameraView>,
}

impl WindowRenderer {
    pub async fn new(window: Arc<Window>, world: &WorldScene) -> Result<Self, ClientError> {
        let size = window.inner_size();
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("surface has no compatible format")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        let renderer =
            SceneRenderer::new(&adapter, config.format, config.width, config.height, world).await?;
        surface.configure(&renderer.device, &config);
        Ok(Self {
            surface,
            config,
            renderer,
            suspended: false,
            picking: crate::picking::WorldPicking::new(world)?,
            last_view: None,
        })
    }

    /// Queries exactly the camera/viewport and static geometry uploaded for the visible frame.
    pub fn pick(
        &self,
        pixel: [f64; 2],
        bodies: &[(mmorpg_core::EntityRef, SceneBox)],
    ) -> Result<Option<mmorpg_core::EntityRef>, ClientError> {
        let Some(view) = self.last_view else {
            return Ok(None);
        };
        if self.suspended {
            return Ok(None);
        }
        let viewport = [self.config.width, self.config.height];
        // Outside/retired viewport coordinates are an ordinary miss. Camera or
        // geometry errors still reach the application's observed failure path.
        use three_d_projective::picking::{PickError, PickRay};
        let ray = match PickRay::from_viewport(
            scene_camera(view, viewport)?.view_projection_matrix(),
            viewport,
            pixel,
        ) {
            Ok(ray) => ray,
            Err(PickError::InvalidViewport | PickError::InvalidViewportPoint) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        self.picking.select(ray, bodies)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.last_view = None;
        self.suspended = width == 0 || height == 0;
        if self.suspended {
            return;
        }
        let limit = self.renderer.device.limits().max_texture_dimension_2d;
        self.config.width = width.min(limit);
        self.config.height = height.min(limit);
        self.renderer.width = self.config.width;
        self.renderer.height = self.config.height;
        self.renderer.depth =
            depth_texture(&self.renderer.device, self.config.width, self.config.height);
        self.surface.configure(&self.renderer.device, &self.config);
    }

    pub fn render(
        &mut self,
        scene: &[SceneBox],
        hud: &[HudRect],
        view: CameraView,
    ) -> Result<(), ClientError> {
        // Presentation has already sampled this candidate frame's body geometry.
        // Until that frame is presented, no view may be paired with those bodies.
        self.last_view = None;
        if self.suspended {
            return Ok(());
        }
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.config.width, self.config.height);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                return Err("GPU surface lost; restart the client".into());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err("GPU surface validation failed".into());
            }
        };
        self.renderer.render(
            &frame.texture.create_view(&Default::default()),
            scene,
            hud,
            view,
        )?;
        self.renderer.queue.present(frame);
        self.last_view = Some(view);
        Ok(())
    }
}

/// Explicit GPU smoke check. Readback proves a frame was rendered, rather than
/// accepting successful command submission as graphics evidence.
pub async fn render_offscreen(
    world: &WorldScene,
    scene: &[SceneBox],
    hud: &[HudRect],
    view: CameraView,
) -> Result<usize, ClientError> {
    let bytes = offscreen_pixels(world, scene, hud, view).await?;
    let mut colors = std::collections::BTreeMap::<[u8; 3], usize>::new();
    for pixel in bytes.as_chunks::<4>().0 {
        *colors.entry([pixel[0], pixel[1], pixel[2]]).or_default() += 1;
    }
    let count = colors.len();
    let dominant = colors.values().copied().max().unwrap_or(0);
    let pixels = bytes.len() / 4;
    if count < 3 || dominant * 4 > pixels * 3 {
        return Err("GPU frame did not contain visible scene geometry".into());
    }
    Ok(count)
}

async fn offscreen_pixels(
    world: &WorldScene,
    scene: &[SceneBox],
    hud: &[HudRect],
    view: CameraView,
) -> Result<Vec<u8>, ClientError> {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let renderer = SceneRenderer::new(
        &adapter,
        wgpu::TextureFormat::Rgba8UnormSrgb,
        WIDTH,
        HEIGHT,
        world,
    )
    .await?;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("smoke target"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    renderer.render(&texture.create_view(&Default::default()), scene, hud, view)?;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("smoke readback"),
        size: u64::from(WIDTH * HEIGHT * 4),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = renderer.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(WIDTH * 4),
                rows_per_image: Some(HEIGHT),
            },
        },
        wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    renderer.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    receiver.recv_timeout(Duration::from_secs(10))??;
    let bytes = buffer.slice(..).get_mapped_range()?;
    let pixels = bytes.to_vec();
    drop(bytes);
    buffer.unmap();
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{camera::OrbitCamera, presentation::Presentation};
    use mmorpg_core::{ZoneId, ZoneSimulation, greyhaven_vale};
    use mmorpg_scenery::greyhaven_vale_scenery;
    use std::time::Instant;

    #[test]
    #[ignore = "requires a GPU; scripts/smoke-native.py runs this explicitly"]
    fn authored_outpost_relief_approaches_render_on_the_native_gpu() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let scenery = greyhaven_vale_scenery();
            for [x, z] in [
                [1_050, 650],
                [250, 950],
                [-850, 1_050],
                [-1_100, 2_950],
                [1_050, 2_950],
            ] {
                assert_eq!(scenery.height_at(x, z), 0);
            }
            let world = WorldScene::new(&scenery);
            for (name, view) in [
                (
                    "relief",
                    CameraView {
                        eye: [20.0, 8.0, 39.0],
                        target: [0.0, 0.0, 18.0],
                    },
                ),
                (
                    "hub",
                    CameraView {
                        eye: [5.0, 8.5, 31.0],
                        target: [-14.0, 3.0, 4.0],
                    },
                ),
            ] {
                let pixels = offscreen_pixels(&world, &[], &[], view).await.unwrap();
                let colours: std::collections::BTreeSet<_> = pixels
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|pixel| [pixel[0], pixel[1], pixel[2]])
                    .collect();
                assert!(colours.len() > 100, "{name} must show the rendered Outpost");
                save_smoke_frame(&format!("outpost-relief-{name}.ppm"), &pixels);
                println!("authored Outpost {name} frame: {} colours", colours.len());
            }
        });
    }

    fn save_smoke_frame(name: &str, pixels: &[u8]) {
        if let Some(directory) = std::env::var_os("MMORPG_SMOKE_FRAME_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let mut ppm = b"P6\n640 360\n255\n".to_vec();
            for pixel in pixels.as_chunks::<4>().0 {
                ppm.extend_from_slice(&pixel[..3]);
            }
            std::fs::write(directory.join(name), ppm).unwrap();
        }
    }

    #[test]
    #[ignore = "requires a GPU; scripts/smoke-native.py runs this explicitly"]
    fn the_projected_combat_hud_changes_gpu_pixels() {
        use crate::hud::CombatHud;
        use mmorpg_core::ZoneCommand;
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 2, sex: 0 })
                .unwrap();
            zone.advance_tick().unwrap();
            let projection = zone.snapshot_for_player(1).unwrap();
            let hud = CombatHud::from_projection(&projection);
            assert_eq!(hud.slots.len(), 4, "the Arcanist kit fills the bar");
            let mut drained = hud.clone();
            if let Some(resource) = &mut drained.resource {
                resource.value /= 2;
            }
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let view = CameraView {
                eye: [5.0, 8.5, 31.0],
                target: [-14.0, 3.0, 4.0],
            };
            let mut frames = Vec::new();
            for rects in [Vec::new(), hud.rects(), drained.rects()] {
                frames.push(offscreen_pixels(&world, &[], &rects, view).await.unwrap());
            }
            save_smoke_frame("combat-hud.ppm", &frames[1]);
            let changed = |left: &[u8], right: &[u8]| {
                left.as_chunks::<4>()
                    .0
                    .iter()
                    .zip(right.as_chunks::<4>().0)
                    .filter(|(left, right)| left != right)
                    .count()
            };
            let shown = changed(&frames[0], &frames[1]);
            let drained = changed(&frames[1], &frames[2]);
            assert!(
                shown >= 1_000,
                "the HUD must cover the frame: {shown} pixels"
            );
            assert!(
                drained >= 100,
                "a projected resource change must move the bar: {drained} pixels"
            );
            println!("combat HUD changed {shown} GPU pixels, half the mana {drained}");
        });
    }

    #[test]
    #[ignore = "requires a GPU; scripts/smoke-native.py runs this explicitly"]
    fn projected_health_bars_change_gpu_pixels() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            let mut state = zone.snapshot().unwrap();
            state.players[0].position = [0, 90, -2_000];
            state.creatures[0].position = [180, 45, -1_700];
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let now = Instant::now();
            let mut frames = Vec::new();
            for damaged in [false, true] {
                let mut checkpoint = state.clone();
                if damaged {
                    checkpoint.creatures[0].health /= 2;
                }
                let restored =
                    ZoneSimulation::from_snapshot(checkpoint, Arc::clone(&content)).unwrap();
                let projection = restored.snapshot_for_player(1).unwrap();
                assert!(projection.entities.iter().any(|entity| {
                    entity.entity()
                        == mmorpg_core::EntityRef::Creature(state.creatures[0].creature_id)
                        && entity.health_percent == if damaged { 50 } else { 100 }
                }));
                let mut presentation =
                    Presentation::new(1, scenery.clone(), Arc::clone(&content), now).unwrap();
                presentation.push(projection, now).unwrap();
                let view = OrbitCamera::default().view(presentation.camera_target(now));
                let pixels = offscreen_pixels(&world, &presentation.scene(now, view), &[], view)
                    .await
                    .unwrap();
                save_smoke_frame(if damaged { "damaged.ppm" } else { "full.ppm" }, &pixels);
                frames.push(pixels);
            }
            let changed = frames[0]
                .as_chunks::<4>()
                .0
                .iter()
                .zip(frames[1].as_chunks::<4>().0)
                .filter(|(full, damaged)| full != damaged)
                .count();
            assert!(
                changed >= 10,
                "projected damage must visibly shorten the bar: {changed} pixels"
            );
            println!("projected creature damage changed {changed} GPU pixels");
        });
    }
}
