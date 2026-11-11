//! Presentation work sentinel (#645): a camera-only frame over Greyhaven does no static
//! scenery work. Once residency has settled, a stationary camera and a camera orbiting a
//! standing viewer build, upload and evict no static group or near terrain tile, so an
//! incremental-update regression that reprocesses the static world fails here without any
//! GPU or wall-clock timing. The broader counts are advisory (`docs/PRESENTATION_WORKLOADS.md`).
#[path = "support/presentation_work.rs"]
mod presentation_work;

use mmorpg_presentation::StaticScene;
use mmorpg_scenery::greyhaven_vale_scenery;
use presentation_work::{FRAME_BUILD_BUDGET, LAP_FRAMES, camera_only, region_walk};

#[test]
fn camera_only_frames_rebuild_and_upload_no_static_scenery() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    for radius in [0., 6.] {
        let work = camera_only(&scene, radius, LAP_FRAMES);
        assert_eq!(work.frames, LAP_FRAMES);
        assert_eq!(
            (work.builds(), work.evictions, work.uploaded_bytes),
            (0, 0, 0),
            "a camera-only frame at orbit {radius} m reprocessed static scenery: {work:?}"
        );
        // Nothing is hidden to get there: the frame still draws the resident world.
        assert!(
            work.resident_tiles > 0 && work.largest_frame_draws > 0,
            "{work:?}"
        );
    }
    // Walking into and out of regions legitimately streams, within the per-frame budget.
    let walk = region_walk(&scene, 256);
    assert!(walk.builds() > 0 && walk.evictions > 0, "{walk:?}");
    assert!(walk.largest_frame_builds <= FRAME_BUILD_BUDGET, "{walk:?}");
}
