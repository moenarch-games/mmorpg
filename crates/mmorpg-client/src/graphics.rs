//! Native wgpu adapter. The same submitted mesh frames drive presentation and picking.
mod frame;
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
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const FAR_PLANE_METRES: f32 = 1400.;

pub struct SceneRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    frame: frame::FrameRenderer,
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
    ) -> Result<Self, ClientError> {
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("MMORPG client"),
                ..Default::default()
            })
            .await?;
        let frame = frame::FrameRenderer::new(&device, format);
        let depth = depth_texture(&device, width, height);
        Ok(Self {
            device,
            queue,
            frame,
            depth,
            width,
            height,
        })
    }
    fn render(
        &mut self,
        target: &wgpu::TextureView,
        frame: &WorldFrame,
        hud: &[HudRect],
        view: CameraView,
    ) -> Result<(), ClientError> {
        self.frame.render(
            &self.device,
            &self.queue,
            frame::Target {
                view: target,
                depth: &self.depth,
                viewport: [self.width, self.height],
            },
            frame,
            hud,
            view,
        )
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
        0.2,
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
    /// Pick only the actual last successfully presented frame and viewport.
    pub fn pick(&self, pixel: [f64; 2]) -> Result<Option<mmorpg_core::EntityRef>, ClientError> {
        let Some(view) = self.last_view else {
            return Ok(None);
        };
        if self.suspended {
            return Ok(None);
        }
        use three_d_projective::picking::{PickError, PickRay};
        let viewport = [self.config.width, self.config.height];
        let ray = match PickRay::from_viewport(
            scene_camera(view, viewport)?.view_projection_matrix(),
            viewport,
            pixel,
        ) {
            Ok(ray) => ray,
            Err(PickError::InvalidViewport | PickError::InvalidViewportPoint) => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        self.picking.select_frame(ray)
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
        frame: &WorldFrame,
        hud: &[HudRect],
        view: CameraView,
        bodies: &[(mmorpg_core::EntityRef, SceneDraw)],
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
    let mut frame = world.frame(0., true, view.eye);
    frame.draws.extend_from_slice(units);
    let bytes = readback(&frame, hud, view).await?;
    let mut colors = std::collections::BTreeMap::<[u8; 3], usize>::new();
    for p in bytes.chunks_exact(4) {
        *colors.entry([p[0], p[1], p[2]]).or_default() += 1;
    }
    if colors.len() < 3 || colors.values().copied().max().unwrap_or(0) * 4 > bytes.len() / 4 * 3 {
        return Err("GPU frame did not contain visible scene geometry".into());
    }
    Ok(colors.len())
}
async fn readback(
    frame: &WorldFrame,
    hud: &[HudRect],
    view: CameraView,
) -> Result<Vec<u8>, ClientError> {
    const WIDTH: u32 = 640;
    const HEIGHT: u32 = 360;
    let instance = wgpu::Instance::default();
    let adapter = instance.request_adapter(&Default::default()).await?;
    let mut renderer =
        SceneRenderer::new(&adapter, wgpu::TextureFormat::Rgba8UnormSrgb, WIDTH, HEIGHT).await?;
    let texture = renderer.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native smoke"),
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
    renderer.render(&texture.create_view(&Default::default()), frame, hud, view)?;
    let buffer = renderer.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("native readback"),
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
    readback(&frame, &[], view).await
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
    let mut frame = world.frame(0., false, view.eye);
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
            },
        });
    }
    readback(&frame, hud, view).await
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

#[cfg(test)]
#[path = "../tests/support/world_gpu_acceptance.rs"]
mod world_gpu_acceptance;
