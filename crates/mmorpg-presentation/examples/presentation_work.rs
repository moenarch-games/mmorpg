//! Deterministic work samples of Greyhaven's client presentation (#645), printed as the
//! `operations` part of `coding-tooling/work-evidence/v1` for `.performance/contract.json`
//! (`scripts/operation-work.py` merges them with the zone's). With `--report` it prints one
//! advisory JSON line per declared scenario instead. Nothing here is timed.
#[path = "../tests/support/presentation_work.rs"]
mod presentation_work;

use mmorpg_presentation::StaticScene;
use mmorpg_scenery::greyhaven_vale_scenery;
use presentation_work::{LAP_FRAMES, StaticWork, UnitWork, camera_only, crowd, region_walk};

fn sample(dimensions: &[(&str, usize)], metrics: &[(&str, usize)]) -> String {
    let list = |pairs: &[(&str, usize)]| {
        pairs
            .iter()
            .map(|(name, value)| format!("\"{name}\":{value}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!(
        "{{\"dimensions\":{{{}}},\"metrics\":{{{}}}}}",
        list(dimensions),
        list(metrics)
    )
}

fn statics(work: &StaticWork) -> String {
    format!(
        "\"frames\":{},\"group_builds\":{},\"tile_builds\":{},\"evictions\":{},\"uploaded_vertices\":{},\"uploaded_bytes\":{},\"largest_frame_builds\":{},\"largest_frame_draws\":{},\"resident_tiles\":{}",
        work.frames,
        work.group_builds,
        work.tile_builds,
        work.evictions,
        work.uploaded_vertices,
        work.uploaded_bytes,
        work.largest_frame_builds,
        work.largest_frame_draws,
        work.resident_tiles,
    )
}

fn units(work: &UnitWork) -> String {
    format!(
        "\"frames\":{},\"units\":{},\"unit_draws\":{},\"geometries\":{},\"geometry_uploads\":{},\"uploaded_vertices\":{}",
        work.frames,
        work.units,
        work.unit_draws,
        work.geometries,
        work.geometry_uploads,
        work.uploaded_vertices,
    )
}

fn report(scene: &StaticScene) {
    let schema = "mmorpg.presentation-work/v1";
    println!(
        "{{\"schema\":\"{schema}\",\"scene\":{{\"groups\":{},\"group_draws\":{},\"tiles\":{}}}}}",
        scene.groups.len(),
        scene.groups.iter().map(|g| g.draws.len()).sum::<usize>(),
        scene.terrain.tiles.len(),
    );
    for (name, work) in [
        ("stationary-frame", camera_only(scene, 0., LAP_FRAMES)),
        ("camera-only-orbit", camera_only(scene, 6., LAP_FRAMES)),
        ("region-walk-512", region_walk(scene, 512)),
    ] {
        println!(
            "{{\"schema\":\"{schema}\",\"scenario\":\"{name}\",{}}}",
            statics(&work)
        );
    }
    for (name, work) in [
        ("one-moving-character", crowd(1, 1, LAP_FRAMES)),
        ("dense-crowd-64", crowd(64, 8, LAP_FRAMES)),
    ] {
        println!(
            "{{\"schema\":\"{schema}\",\"scenario\":\"{name}\",{}}}",
            units(&work)
        );
    }
}

fn main() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    if std::env::args().any(|arg| arg == "--report") {
        report(&scene);
        return;
    }
    let mut camera = Vec::new();
    for (frames, orbit) in [(64, 4), (128, 4), (256, 4), (128, 2), (128, 8)] {
        let work = camera_only(&scene, orbit as f64, frames);
        camera.push(sample(
            &[("frames", frames), ("orbit_metres", orbit)],
            &[
                ("static_builds", work.builds()),
                ("static_evictions", work.evictions),
                ("uploaded_bytes", work.uploaded_bytes),
                ("largest_frame_draws", work.largest_frame_draws),
            ],
        ));
    }
    let walk: Vec<_> = [128, 256, 512]
        .into_iter()
        .map(|metres| {
            let work = region_walk(&scene, metres);
            sample(
                &[("metres", metres)],
                &[("largest_frame_builds", work.largest_frame_builds)],
            )
        })
        .collect();
    let unit_frames: Vec<_> = [8, 16, 32, 64]
        .into_iter()
        .map(|characters| {
            let work = crowd(characters, 1, LAP_FRAMES);
            sample(
                &[("characters", characters)],
                &[
                    ("unit_draws", work.unit_draws),
                    ("geometry_uploads", work.geometry_uploads),
                ],
            )
        })
        .collect();
    let operations = [
        ("camera-only-frames", camera),
        ("region-walk", walk),
        ("unit-frame", unit_frames),
    ]
    .into_iter()
    .map(|(id, samples)| format!("{{\"id\":\"{id}\",\"samples\":[{}]}}", samples.join(",")))
    .collect::<Vec<_>>()
    .join(",");
    println!("{{\"operations\":[{operations}]}}");
}
