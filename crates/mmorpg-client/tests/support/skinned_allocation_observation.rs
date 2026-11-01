//! Manual #338 observation. No thresholds, production instrumentation or CI gate.
use super::*;
use std::time::Instant;

#[test]
#[ignore = "manual native GPU allocation observation for issue #338"]
fn imported_mesh_buffer_observation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let instance = wgpu::Instance::default();
        let adapter = instance.request_adapter(&Default::default()).await.unwrap();
        println!("adapter={:?}", adapter.get_info());
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let world = WorldScene::new(&scenery);
        let view = CameraView { field_of_view_degrees: 50.0, eye: [12.0, 16.0, 25.0], target: [8.0, 0.0, 4.0] };
        for count in [1, 10, 50] {
            for mode in ["animated", "unchanged", "retained"] {
                let mut renderer = SceneRenderer::new(&adapter, wgpu::TextureFormat::Rgba8UnormSrgb, 640, 360).await.unwrap();
                let target = renderer.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("allocation observation"),
                    size: wgpu::Extent3d { width: 640, height: 360, depth_or_array_layers: 1 },
                    mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[],
                });
                let target = target.create_view(&Default::default());
                let retained = crate::presentation::observed_imported_draws(count, 0.25);
                for frame_index in 0..3 {
                    let before = renderer.frame.observation().0;
                    let skin_start = Instant::now();
                    let draws = if mode == "retained" { retained.clone() } else {
                        crate::presentation::observed_imported_draws(count,
                            if mode == "animated" { 0.25 + frame_index as f32 / 30.0 } else { 0.25 })
                    };
                    let cpu_meshes = if mode == "retained" { 0 } else { draws.len() };
                    let cpu_us = skin_start.elapsed().as_micros();
                    let mut frame = world.frame(0.0, false, view.eye, None);
                    frame.batches.clear();
                    frame.draws = draws;
                    let render_start = Instant::now();
                    renderer.render(&target, &frame, &[], view, None).unwrap();
                    let submit_us = render_start.elapsed().as_micros();
                    renderer.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(Duration::from_secs(30)) }).unwrap();
                    let completed_us = render_start.elapsed().as_micros();
                    let (after, live) = renderer.frame.observation();
                    println!("mode={mode} units={count} frame={frame_index} cpu_meshes={cpu_meshes} gpu_buffers={} gpu_mesh_reuses={} uploaded_bytes={} palette_uploads={} live_meshes={live} cpu_materialize_us={cpu_us} render_submit_us={submit_us} render_complete_us={completed_us}", after[0]-before[0], after[1]-before[1], after[2]-before[2], after[3]-before[3]);
                }
            }
        }
    });
}
