//! Native wgpu adapter. The same submitted mesh frames drive presentation and picking.
mod frame;
#[cfg(test)]
mod profile;
use crate::{
    ClientError,
    camera::CameraView,
    hud::HudRect,
    world::{SceneDraw, WorldFrame, WorldScene},
};
use std::{sync::Arc, time::Duration};
use three_d_camera::PerspectiveCamera;
use three_d_core::Vec3;
use winit::window::Window;
const FAR_PLANE_METRES: f32 = 1400.;

/// Screen-space interface painted into the finished, resolved frame after the
/// world, before it is presented. It owns its own GPU resources.
pub trait Overlay {
    fn paint(&mut self, target: OverlayTarget<'_>) -> Result<(), ClientError>;
}

/// The device and resolved colour target an [`Overlay`] paints into.
pub struct OverlayTarget<'a> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    pub view: &'a wgpu::TextureView,
    pub format: wgpu::TextureFormat,
    pub size: [u32; 2],
}

pub struct SceneRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    frame: frame::FrameRenderer,
    format: wgpu::TextureFormat,
    width: u32,
    height: u32,
}
impl SceneRenderer {
    async fn new(
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Result<Self, ClientError> {
        // render-engine's sky needs filterable float LUTs; unclipped shadow depth and the
        // sky's dual-source blend are used where the adapter has them, and timestamp queries
        // for the manual frame-time measurement (#692).
        if !adapter
            .features()
            .contains(wgpu::Features::FLOAT32_FILTERABLE)
        {
            return Err("the GPU adapter lacks FLOAT32_FILTERABLE, which the sky needs".into());
        }
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("MMORPG client"),
                required_features: adapter.features()
                    & (wgpu::Features::DEPTH_CLIP_CONTROL
                        | wgpu::Features::DUAL_SOURCE_BLENDING
                        | wgpu::Features::FLOAT32_FILTERABLE
                        | wgpu::Features::TIMESTAMP_QUERY),
                ..Default::default()
            })
            .await?;
        let frame = frame::FrameRenderer::new(&device, &queue, format, [width, height]);
        Ok(Self {
            device,
            queue,
            frame,
            format,
            width,
            height,
        })
    }
    fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.frame.resize([width, height]);
    }
    fn render(
        &mut self,
        target: &wgpu::TextureView,
        frame: &WorldFrame,
        hud: &[HudRect],
        view: CameraView,
        overlay: Option<&mut dyn Overlay>,
    ) -> Result<(), ClientError> {
        self.frame
            .render(&self.device, &self.queue, target, frame, hud, view)?;
        match overlay {
            Some(overlay) => overlay.paint(OverlayTarget {
                device: &self.device,
                queue: &self.queue,
                view: target,
                format: self.format,
                size: [self.width, self.height],
            }),
            None => Ok(()),
        }
    }
}
/// The CPU camera picking and nameplates project through; render-engine draws the same eye,
/// target and field of view (its projection is infinite reverse-Z).
pub(crate) fn scene_camera(
    view: CameraView,
    viewport: [u32; 2],
) -> Result<PerspectiveCamera, ClientError> {
    Ok(PerspectiveCamera::new(
        Vec3::new(view.eye[0], view.eye[1], view.eye[2]),
        Vec3::new(view.target[0], view.target[1], view.target[2]),
        Vec3::new(0.0, 1.0, 0.0),
        view.field_of_view_degrees.to_radians(),
        viewport[0] as f32 / viewport[1] as f32,
        0.2,
        FAR_PLANE_METRES,
    )?)
}

/// Where a world point lands in a `viewport` rendered from `view`, in pixels
/// from the top left; `None` behind the eye or beyond the far plane.
#[must_use]
pub fn project_to_viewport(
    view: CameraView,
    viewport: [u32; 2],
    point: [f32; 3],
) -> Option<[f32; 2]> {
    if viewport[0] == 0 || viewport[1] == 0 {
        return None;
    }
    let m = scene_camera(view, viewport)
        .ok()?
        .view_projection_matrix()
        .elements;
    let clip: [f32; 4] = std::array::from_fn(|r| {
        m[r] * point[0] + m[4 + r] * point[1] + m[8 + r] * point[2] + m[12 + r]
    });
    if clip[3] <= 1e-4 || !(-1.0..=1.0).contains(&(clip[2] / clip[3])) {
        return None;
    }
    let [x, y] = [clip[0] / clip[3], clip[1] / clip[3]];
    Some([
        (x + 1.0) * 0.5 * viewport[0] as f32,
        (1.0 - y) * 0.5 * viewport[1] as f32,
    ])
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
    pub async fn new(window: Arc<Window>, _world: &WorldScene) -> Result<Self, ClientError> {
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
        if let Some(format) = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|f| f.is_srgb())
        {
            config.format = format;
        }
        let renderer =
            SceneRenderer::new(&adapter, config.format, config.width, config.height).await?;
        surface.configure(&renderer.device, &config);
        Ok(Self {
            surface,
            config,
            renderer,
            suspended: false,
            picking: crate::picking::WorldPicking::default(),
            last_view: None,
        })
    }
    /// The presented surface size in physical pixels.
    #[must_use]
    pub fn viewport(&self) -> [u32; 2] {
        [self.config.width, self.config.height]
    }

    /// Pick only the actual last successfully presented frame and viewport; a click
    /// within `margin` physical pixels of a creature, NPC or door may still select it.
    pub fn pick(
        &self,
        pixel: [f64; 2],
        margin: f64,
    ) -> Result<Option<crate::picking::Picked>, ClientError> {
        let Some(view) = self.last_view else {
            return Ok(None);
        };
        if self.suspended {
            return Ok(None);
        }
        use three_d_projective::picking::{PickError, PickRay};
        let viewport = [self.config.width, self.config.height];
        let view_projection = scene_camera(view, viewport)?.view_projection_matrix();
        let ray = match PickRay::from_viewport(view_projection, viewport, pixel) {
            Ok(ray) => ray,
            Err(PickError::InvalidViewport | PickError::InvalidViewportPoint) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        self.picking.select_frame(
            ray,
            Some(crate::picking::PickScreen {
                view_projection,
                viewport,
                pixel,
                margin,
            }),
        )
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
        self.renderer.resize(self.config.width, self.config.height);
        self.surface.configure(&self.renderer.device, &self.config);
    }
    pub fn render(
        &mut self,
        frame: &WorldFrame,
        hud: &[HudRect],
        view: CameraView,
        bodies: &[(mmorpg_core::EntityRef, SceneDraw)],
        overlay: Option<&mut dyn Overlay>,
    ) -> Result<(), ClientError> {
        self.last_view = None;
        if self.suspended {
            return Ok(());
        }
        let target = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
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
            &target.texture.create_view(&Default::default()),
            frame,
            hud,
            view,
            overlay,
        )?;
        self.renderer.queue.present(target);
        self.picking.present(frame, bodies);
        self.last_view = Some(view);
        Ok(())
    }
}
/// Explicit native GPU smoke over the production frame adapter.
pub async fn render_offscreen(
    world: &WorldScene,
    units: &[SceneDraw],
    hud: &[HudRect],
    view: CameraView,
) -> Result<usize, ClientError> {
    let mut frame = world.frame(0., true, view.eye, None);
    frame.draws.extend_from_slice(units);
    let bytes = readback(&frame, hud, view, SMOKE_SIZE, None).await?;
    let mut colors = std::collections::BTreeMap::<[u8; 3], usize>::new();
    for p in bytes.as_chunks::<4>().0 {
        *colors.entry([p[0], p[1], p[2]]).or_default() += 1;
    }
    if colors.len() < 3 || colors.values().copied().max().unwrap_or(0) * 4 > bytes.len() / 4 * 3 {
        return Err("GPU frame did not contain visible scene geometry".into());
    }
    Ok(colors.len())
}
const SMOKE_SIZE: [u32; 2] = [640, 360];

pub(crate) async fn readback(
    frame: &WorldFrame,
    hud: &[HudRect],
    view: CameraView,
    [width, height]: [u32; 2],
    overlay: Option<&mut dyn Overlay>,
) -> Result<Vec<u8>, ClientError> {
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let mut renderer =
        SceneRenderer::new(&adapter, wgpu::TextureFormat::Rgba8UnormSrgb, width, height).await?;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native smoke"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    renderer.render(
        &texture.create_view(&Default::default()),
        frame,
        hud,
        view,
        overlay,
    )?;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("native readback"),
        size: u64::from(width * height * 4),
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
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    renderer.queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = sender.send(r);
    });
    renderer.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(Duration::from_secs(10)),
    })?;
    receiver.recv_timeout(Duration::from_secs(10))??;
    let mapped = buffer.slice(..).get_mapped_range()?;
    let pixels = mapped.to_vec();
    drop(mapped);
    buffer.unmap();
    Ok(pixels)
}
#[cfg(test)]
async fn offscreen_frame_pixels(
    frame: &WorldFrame,
    units: &[SceneDraw],
    view: CameraView,
) -> Result<Vec<u8>, ClientError> {
    let mut frame = frame.clone();
    frame.draws.extend_from_slice(units);
    readback(&frame, &[], view, SMOKE_SIZE, None).await
}
// Compatibility for the existing explicit GPU regression fixtures. The live
// application submits SceneDraws directly; these boxes use that same adapter.
#[cfg(test)]
async fn offscreen_pixels(
    world: &WorldScene,
    boxes: &[crate::presentation::SceneBox],
    hud: &[HudRect],
    view: CameraView,
) -> Result<Vec<u8>, ClientError> {
    let mut frame = world.frame(0., false, view.eye, None);
    let cube = Arc::new(three_d_core::Mesh::unit_cube());
    for (i, b) in boxes.iter().enumerate() {
        let (s, c) = b.yaw.sin_cos();
        let [x, y, z] = b.size;
        let [tx, ty, tz] = b.position;
        frame.draws.push(SceneDraw {
            id: format!("compatibility-box-{i}"),
            resource_key: "compatibility-cube".into(),
            mesh: Arc::clone(&cube),
            model: three_d_animation::Mat4 {
                elements: [
                    c * x,
                    0.,
                    -s * x,
                    0.,
                    0.,
                    y,
                    0.,
                    0.,
                    s * z,
                    0.,
                    c * z,
                    0.,
                    tx,
                    ty,
                    tz,
                    1.,
                ],
            },
            material: crate::world::DrawMaterial {
                color: b.color.map(|v| {
                    if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1. / 2.4) - 0.055
                    }
                }),
                opacity: 1.,
                unlit: false,
                emissive: [0.; 3],
                double_sided: false,
                ground: None,
            },
            skin: None,
        });
    }
    readback(&frame, hud, view, SMOKE_SIZE, None).await
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
                        field_of_view_degrees: 50.0,
                        eye: [20.0, 8.0, 39.0],
                        target: [0.0, 0.0, 18.0],
                    },
                ),
                (
                    "hub",
                    CameraView {
                        field_of_view_degrees: 50.0,
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

    /// Advisory frame time (PRINCIPLE-015): the Outpost hub with its units and the woods
    /// (#49), 1280×720, each frame posed, submitted and waited for. Per view it prints the
    /// median, p95, p99 and maximum of the frame's wall time, `render` (prepare, encode,
    /// submission and the wait for the GPU), render-engine's CPU `prepare` and `encode` and the
    /// GPU time (the sum of the passes' timestamp spans, where the adapter has
    /// `TIMESTAMP_QUERY`) over the frames after the first 40, then the first frame alone, the
    /// median GPU time per pass, the work counts of the last frame and the world uploads over
    /// the view, and the raw per-frame lists including the first frames (#692).
    #[test]
    #[ignore = "requires a GPU; manual frame-time measurement"]
    fn outpost_frame_time() {
        use mmorpg_core::ZoneCommand;
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 1, sex: 0 })
                .unwrap();
            zone.advance_tick().unwrap();
            let projection = zone.snapshot_for_player(1).unwrap();
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let start = Instant::now();
            let mut presentation =
                Presentation::new(1, scenery.clone(), Arc::clone(&content), start).unwrap();
            presentation.push(projection, start).unwrap();
            let views = [
                (
                    "hub",
                    CameraView {
                        field_of_view_degrees: 50.0,
                        eye: [5.0, 8.5, 31.0],
                        target: [-14.0, 3.0, 4.0],
                    },
                ),
                (
                    "woods",
                    CameraView {
                        field_of_view_degrees: 50.0,
                        eye: [-41.0, 18.0, 16.0],
                        target: [-69.0, 14.0, 0.0],
                    },
                ),
                (
                    "vale",
                    CameraView {
                        field_of_view_degrees: 50.0,
                        eye: [0.0, 70.0, 95.0],
                        target: [0.0, 0.0, -10.0],
                    },
                ),
                (
                    "lake",
                    CameraView {
                        field_of_view_degrees: 50.0,
                        eye: [-56.0, 33.0, -39.0],
                        target: [-87.0, 11.0, -53.0],
                    },
                ),
            ];
            let size = [1280, 720];
            let instance = wgpu::Instance::default();
            let adapter = instance.request_adapter(&Default::default()).await.unwrap();
            let format = wgpu::TextureFormat::Rgba8UnormSrgb;
            let mut renderer = SceneRenderer::new(&adapter, format, size[0], size[1])
                .await
                .unwrap();
            let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("frame time"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            let target = target.create_view(&Default::default());
            let timed = renderer
                .device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY);
            if timed {
                renderer.frame.enable_gpu_timing(&renderer.device);
            }
            println!("adapter={} gpu_timestamps={timed}", adapter.get_info().name);
            // Statistics leave out the first frames of each view (uploads, pipeline
            // creation); the first frame and the raw lists keep them.
            const WARM: usize = 40;
            let ms = |span: Duration| span.as_secs_f64() * 1e3;
            let stats = |name: &str, samples: &[f64]| {
                let mut warm = samples.get(WARM..).unwrap_or_default().to_vec();
                if warm.is_empty() {
                    return format!("{name} n/a");
                }
                warm.sort_by(f64::total_cmp);
                let at = |percent: usize| warm[(warm.len() * percent / 100).min(warm.len() - 1)];
                format!(
                    "{name} median={:.2} p95={:.2} p99={:.2} max={:.2} first={:.2}",
                    at(50),
                    at(95),
                    at(99),
                    warm[warm.len() - 1],
                    samples[0]
                )
            };
            let list = |samples: &[f64]| {
                samples
                    .iter()
                    .map(|value| format!("{value:.2}"))
                    .collect::<Vec<_>>()
                    .join(",")
            };
            for (name, view) in views {
                let [mut frames, mut renders, mut prepares, mut encodes, mut gpus] =
                    std::array::from_fn(|_| Vec::new());
                // GPU time per pass label (several sky or shadow passes summed), in pass order.
                let mut passes: Vec<(&str, Vec<f64>)> = Vec::new();
                let world_before = renderer.frame.profile().world;
                for index in 0..240 {
                    let now = start + Duration::from_millis(16 * index);
                    let begin = Instant::now();
                    let frame = presentation.frame(&world, now, view);
                    let rendering = Instant::now();
                    renderer.render(&target, &frame, &[], view, None).unwrap();
                    renderer
                        .device
                        .poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: Some(Duration::from_secs(10)),
                        })
                        .unwrap();
                    frames.push(ms(begin.elapsed()));
                    renders.push(ms(rendering.elapsed()));
                    let profile = renderer.frame.profile();
                    prepares.push(ms(profile.prepare));
                    encodes.push(ms(profile.encode));
                    let Some(spans) = renderer
                        .frame
                        .gpu_passes(&renderer.device, &renderer.queue)
                        .unwrap()
                    else {
                        continue;
                    };
                    gpus.push(spans.iter().map(|(_, span)| span).sum());
                    let mut summed: Vec<(&str, f64)> = Vec::new();
                    for (label, span) in spans {
                        match summed.iter_mut().find(|(seen, _)| *seen == label) {
                            Some((_, total)) => *total += span,
                            None => summed.push((label, span)),
                        }
                    }
                    for (label, span) in summed {
                        match passes.iter_mut().find(|(seen, _)| *seen == label) {
                            Some((_, samples)) => samples.push(span),
                            None => passes.push((label, vec![span])),
                        }
                    }
                }
                println!(
                    "view={name} {} | {} | {} | {} | {}",
                    stats("frame_ms", &frames),
                    stats("render_ms", &renders),
                    stats("prepare_ms", &prepares),
                    stats("encode_ms", &encodes),
                    stats("gpu_ms", &gpus),
                );
                let passes: Vec<_> = passes
                    .iter_mut()
                    .map(|(label, samples)| {
                        let skip = WARM.min(samples.len() - 1);
                        let warm = &mut samples[skip..];
                        warm.sort_by(f64::total_cmp);
                        format!("{}={:.3}", label.replace(' ', "_"), warm[warm.len() / 2])
                    })
                    .collect();
                println!("view={name} gpu_pass_median_ms {}", passes.join(" "));
                let profile = renderer.frame.profile();
                println!(
                    "view={name} prepare_work {:?} world_work_over_view {:?}",
                    profile.work,
                    profile.world - world_before
                );
                println!("view={name} raw frame_ms=[{}]", list(&frames));
                println!("view={name} raw render_ms=[{}]", list(&renders));
                println!("view={name} raw gpu_ms=[{}]", list(&gpus));
            }
        });
    }

    /// Reviewable lighting evidence (#529): the Outpost at noon, dusk and night, and a
    /// campfire and the lantern post by night, as PNGs in `MMORPG_LIGHTING_SHOTS`.
    #[test]
    #[ignore = "requires a GPU; manual lighting evidence"]
    fn greyhaven_lighting_shots() {
        use mmorpg_core::ZoneCommand;
        let Some(directory) = std::env::var_os("MMORPG_LIGHTING_SHOTS") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 1, sex: 0 })
                .unwrap();
            zone.advance_tick().unwrap();
            let projection = zone.snapshot_for_player(1).unwrap();
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let now = Instant::now();
            let mut presentation =
                Presentation::new(1, scenery.clone(), Arc::clone(&content), now).unwrap();
            presentation.push(projection, now).unwrap();
            let hub = presentation.camera_view(&mut OrbitCamera::default(), &world, now);
            let lights = world.frame(0.0, false, hub.eye, None).lighting.point_lights;
            let near = |light: &crate::environment::PointLight| CameraView {
                field_of_view_degrees: 50.0,
                eye: [
                    light.position[0] + 5.0,
                    light.position[1] + 2.5,
                    light.position[2] + 6.0,
                ],
                target: [
                    light.position[0],
                    light.position[1] - 1.2,
                    light.position[2],
                ],
            };
            let campfire = near(lights.first().unwrap());
            let lantern = near(lights.last().unwrap());
            let size = [1280, 720];
            // The woods (#49) near, at the debug `woods` viewpoint and far, so the trees show
            // each level of detail.
            let woods = |along: f32, rise: f32| {
                let (eye, target) = ([-41.0f32, 18.0, 16.0], [-69.0f32, 14.0, 0.0]);
                CameraView {
                    field_of_view_degrees: 50.0,
                    eye: std::array::from_fn(|axis| {
                        eye[axis] + (target[axis] - eye[axis]) * along + [0.0, rise, 0.0][axis]
                    }),
                    target,
                }
            };
            for (index, (name, hour, view)) in [
                ("noon", 12.0, hub),
                ("dusk", 17.8, hub),
                ("night", 23.5, hub),
                ("torch-campfire", 22.0, campfire),
                ("torch-lantern", 22.0, lantern),
                ("woods-near", 15.0, woods(0.62, 0.0)),
                ("woods-mid", 15.0, woods(0.0, 0.0)),
                ("woods-far", 15.0, woods(-2.5, 22.0)),
            ]
            .into_iter()
            .enumerate()
            {
                // Settle the trees' cross-fades at this view before the shot.
                let now = now + Duration::from_secs(2 * index as u64);
                for step in 0..4 {
                    let _ = presentation.frame(
                        &world,
                        now - Duration::from_millis(250 * (4 - step)),
                        view,
                    );
                }
                let mut frame = presentation.frame(&world, now, view);
                frame.lighting.hour = hour;
                let pixels = readback(&frame, &[], view, size, None).await.unwrap();
                image::save_buffer(
                    directory.join(format!("re9-{name}.png")),
                    &pixels,
                    size[0],
                    size[1],
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
                println!("saved re9-{name}.png");
            }
        });
    }

    /// Reviewable ground evidence (#525): the woods, vale, lake and hub debug viewpoints and a
    /// walking-height view of the Outpost road at noon, and the climbing kit (#621), as PNGs in
    /// `MMORPG_GROUND_SHOTS`.
    #[test]
    #[ignore = "requires a GPU; manual ground evidence"]
    fn greyhaven_ground_shots() {
        use mmorpg_core::ZoneCommand;
        let Some(directory) = std::env::var_os("MMORPG_GROUND_SHOTS") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        std::fs::create_dir_all(&directory).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            zone.apply_command(1, 1, ZoneCommand::ChooseClass { class: 1, sex: 0 })
                .unwrap();
            zone.advance_tick().unwrap();
            let projection = zone.snapshot_for_player(1).unwrap();
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let now = Instant::now();
            let mut presentation =
                Presentation::new(1, scenery.clone(), Arc::clone(&content), now).unwrap();
            presentation.push(projection, now).unwrap();
            let view = |eye: [f32; 3], target: [f32; 3]| CameraView {
                field_of_view_degrees: 50.0,
                eye,
                target,
            };
            // `DEBUG_VIEWPOINTS` in web/src/world/world-view.ts, and a walking-height view.
            let views = [
                ("woods", view([-41.0, 18.0, 16.0], [-69.0, 14.0, 0.0])),
                ("vale", view([0.0, 70.0, 95.0], [0.0, 0.0, -10.0])),
                ("lake", view([-56.0, 33.0, -39.0], [-87.0, 11.0, -53.0])),
                ("hub", view([5.0, 8.5, 31.0], [-14.0, 3.0, 4.0])),
                ("road", view([2.0, 2.2, 30.0], [-6.0, 0.5, 12.0])),
                // The wood and rock materials (#53): the market props and a limestone outcrop.
                ("market", view([15.6, 2.6, 27.1], [23.5, 0.6, 30.4])),
                ("rock", view([-2.2, 2.6, 78.2], [-6.5, 0.5, 82.0])),
                // The terrain tiles' levels and the distant ranges (#526).
                (
                    "overview",
                    view([-150.0, 160.0, 560.0], [260.0, 0.0, -300.0]),
                ),
                (
                    "mountains",
                    view([300.0, 60.0, -560.0], [420.0, 40.0, -900.0]),
                ),
                // The climbing kit (#621): the watchtower's ladder, the sheer crag's rope and
                // the ivy up the highland escarpment's climbing route.
                ("ladder", view([169.5, 7.8, -55.0], [167.0, 6.3, -58.0])),
                ("rope", view([-137.5, 31.0, -348.5], [-134.0, 29.5, -354.0])),
                ("ivy", view([118.0, 4.0, -205.0], [123.5, 6.0, -213.0])),
            ];
            let size = [1280, 800];
            for (index, (name, view)) in views.into_iter().enumerate() {
                let now = now + Duration::from_secs(2 * index as u64);
                for step in 0..4 {
                    let _ = presentation.frame(
                        &world,
                        now - Duration::from_millis(250 * (4 - step)),
                        view,
                    );
                }
                let mut frame = presentation.frame(&world, now, view);
                frame.lighting.hour = 12.0;
                let pixels = readback(&frame, &[], view, size, None).await.unwrap();
                image::save_buffer(
                    directory.join(format!("{name}.png")),
                    &pixels,
                    size[0],
                    size[1],
                    image::ExtendedColorType::Rgba8,
                )
                .unwrap();
            }
        });
    }

    pub(super) fn save_smoke_frame(name: &str, pixels: &[u8]) {
        save_frame(name, pixels, SMOKE_SIZE);
    }

    fn save_frame(name: &str, pixels: &[u8], [width, height]: [u32; 2]) {
        if let Some(directory) = std::env::var_os("MMORPG_SMOKE_FRAME_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
            for pixel in pixels.as_chunks::<4>().0 {
                ppm.extend_from_slice(&pixel[..3]);
            }
            std::fs::write(directory.join(name), ppm).unwrap();
        }
    }

    #[test]
    #[ignore = "requires a GPU; scripts/smoke-native.py runs this explicitly"]
    fn the_egui_interface_paints_over_the_native_world() {
        use crate::{
            interaction::NpcContext,
            ui::{GameUi, UiFrame, overlay::EguiPaint},
        };
        use mmorpg_client_model::hud::CombatHud;
        use mmorpg_core::{EntityRef, NpcId, ZoneCommand};
        let high_dpi = std::env::var("MMORPG_NATIVE_ABILITY_ART_DPI").as_deref() == Ok("2");
        let size = if high_dpi { [2560, 1440] } else { [1280, 720] };
        let scale = if high_dpi { 2.0 } else { 1.0 };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let content = greyhaven_vale::content();
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            let warden_art =
                std::env::var("MMORPG_NATIVE_ABILITY_ART_EVIDENCE").as_deref() == Ok("1");
            zone.apply_command(
                1,
                1,
                ZoneCommand::ChooseClass {
                    class: if warden_art { 0 } else { 1 },
                    sex: 0,
                },
            )
            .unwrap();
            zone.apply_command(
                1,
                2,
                ZoneCommand::SelectTarget(Some(EntityRef::Npc(NpcId::new(3)))),
            )
            .unwrap();
            // Advance to a projection that carries the self sheets.
            let projection = loop {
                zone.advance_tick().unwrap();
                let projection = zone.snapshot_for_player(1).unwrap();
                if projection.inventory.is_some() {
                    break projection;
                }
            };
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let now = Instant::now();
            let mut presentation =
                Presentation::new(1, scenery.clone(), Arc::clone(&content), now).unwrap();
            presentation.push(projection.clone(), now).unwrap();
            let view = presentation.camera_view(&mut OrbitCamera::default(), &world, now);
            let mut frame = world.frame(0.0, false, view.eye, None);
            frame
                .draws
                .extend(presentation.unit_draws(now, view).iter().cloned());

            let mut ui = GameUi::new(&scenery);
            let settings_evidence =
                std::env::var("MMORPG_NATIVE_SETTINGS_EVIDENCE").as_deref() == Ok("1");
            if settings_evidence {
                ui.settings.open = true;
                ui.settings
                    .bindings
                    .select_character(Some("settings-evidence"));
            }
            let names = |entity| presentation.unit_name(entity);
            // Cosmetic combat events float over the units they concern.
            let mut struck = projection.clone();
            struck.events.extend([
                mmorpg_core::ZoneEvent::DamageDealt {
                    source: EntityRef::Player(1),
                    target: EntityRef::Npc(NpcId::new(3)),
                    amount: 27,
                    critical: true,
                },
                mmorpg_core::ZoneEvent::Healed {
                    source: EntityRef::Player(1),
                    target: EntityRef::Player(1),
                    ability: mmorpg_core::AbilityId::new(1),
                    amount: 12,
                },
            ]);
            ui.observe(&struck, &content, &names, now);
            ui.bags_open = true;
            ui.character_open = true;
            let mut npc = NpcContext::default();
            npc.observe(&projection);
            npc.open(NpcId::new(3));
            let hud = CombatHud::from_projection(&projection);
            let ui_frame = UiFrame {
                snapshot: Some(&projection),
                content: &content,
                hud: Some(&hud),
                names: &names,
                forward: [view.target[0] - view.eye[0], view.target[2] - view.eye[2]],
                connection: None,
                choosing: false,
                labels: presentation.labels(),
                project: &|point| {
                    project_to_viewport(view, size, point).map(|[x, y]| egui::pos2(x, y))
                },
                eye: view.eye,
            };
            assert!(
                presentation.labels().len() > 1,
                "the presented units carry nameplate anchors"
            );
            let context = egui::Context::default();
            context.set_pixels_per_point(scale);
            let mut paint = EguiPaint::default();
            // Passes advance time so window layout and fade-in animations settle.
            for pass in 0..10 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0] as f32 / scale, size[1] as f32 / scale),
                    )),
                    time: Some(f64::from(pass) * 0.1),
                    ..Default::default()
                };
                let output = context.run_pass(input, |ctx| {
                    let intents = ui.show(ctx, &ui_frame, &mut npc, now);
                    assert!(intents.is_empty(), "showing alone sends nothing");
                });
                let _ = paint.prepare(&context, output);
            }
            let bare = readback(&frame, &[], view, size, None).await.unwrap();
            let shown = readback(&frame, &[], view, size, Some(&mut paint))
                .await
                .unwrap();
            save_frame(
                if warden_art && high_dpi {
                    "warden-2x.ppm"
                } else if warden_art {
                    "warden.ppm"
                } else if settings_evidence {
                    "settings.ppm"
                } else {
                    "interface.ppm"
                },
                &shown,
                size,
            );
            let changed = bare
                .as_chunks::<4>()
                .0
                .iter()
                .zip(shown.as_chunks::<4>().0)
                .filter(|(bare, shown)| bare != shown)
                .count();
            assert!(
                changed >= 50_000,
                "the interface must cover much of the frame: {changed} pixels"
            );
            println!("egui interface changed {changed} GPU pixels");
        });
    }

    #[test]
    #[ignore = "requires a GPU; scripts/smoke-native.py runs this explicitly"]
    fn the_projected_combat_hud_changes_gpu_pixels() {
        use mmorpg_client_model::hud::CombatHud;
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
            assert_eq!(
                hud.slots
                    .iter()
                    .map(|slot| slot.ability)
                    .collect::<Vec<_>>(),
                [9, 10, 11, 12, 16],
                "the complete Arcanist kit fills the bar"
            );
            let mut drained = hud.clone();
            if let Some(resource) = &mut drained.resource {
                resource.value /= 2;
            }
            let scenery = greyhaven_vale_scenery();
            let world = WorldScene::new(&scenery);
            let view = CameraView {
                field_of_view_degrees: 50.0,
                eye: [5.0, 8.5, 31.0],
                target: [-14.0, 3.0, 4.0],
            };
            let mut frames = Vec::new();
            for rects in [
                Vec::new(),
                crate::hud::rects(&hud),
                crate::hud::rects(&drained),
            ] {
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
            state.players[0].position = mmorpg_core::physics::from_units([0, 90, -2_000]);
            state.creatures[0].position = mmorpg_core::physics::from_units([180, 45, -1_700]);
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
                let view = presentation.camera_view(&mut OrbitCamera::default(), &world, now);
                let frame = world.frame(0.0, false, view.eye, None);
                let units = presentation.unit_draws(now, view);
                let pixels = offscreen_frame_pixels(&frame, &units, view).await.unwrap();
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

#[cfg(test)]
#[path = "../tests/support/world_gpu_acceptance.rs"]
mod world_gpu_acceptance;

#[cfg(test)]
#[path = "../tests/support/skinned_allocation_observation.rs"]
mod skinned_allocation_observation;

#[cfg(test)]
#[path = "../tests/support/structure_visuals.rs"]
mod structure_visuals;
