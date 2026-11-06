//! The terrain's visual tiles and their streaming (#526).
use mmorpg_presentation::StaticScene;
use mmorpg_presentation::tiles::{BUILDS_PER_FRAME, CHUNK_SIDES, Level, NEAR_METRES, Streamer};
use mmorpg_scenery::greyhaven_vale_scenery;

#[test]
fn near_levels_stream_around_the_camera_and_show_only_once_resident() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    let tiles = &scene.terrain;
    let mut streamer = Streamer::default();
    let hub = [0., 10., 0.];
    // The first update builds every tile the camera wants near: the first frame is complete.
    streamer.update(tiles, hub, BUILDS_PER_FRAME, |_| ());
    let wanted: Vec<_> = (0..tiles.tiles.len())
        .filter(|&i| tiles.distance(i, hub) <= NEAR_METRES)
        .collect();
    assert!(wanted.len() > BUILDS_PER_FRAME);
    for &i in &wanted {
        assert_eq!(streamer.level(i), Level::Near, "tile {i}");
    }
    let resident = streamer.resident_count();
    assert!(resident < tiles.tiles.len() / 2, "{resident} resident");
    // A teleport builds a few near levels per frame; the rest keep their far level meanwhile,
    // and the ones left behind are evicted.
    let far_away = [600., 40., 600.];
    let evicted = streamer.update(tiles, far_away, BUILDS_PER_FRAME, |_| ());
    assert_eq!(evicted.len(), resident);
    let near = (0..tiles.tiles.len())
        .filter(|&i| streamer.level(i) == Level::Near)
        .count();
    assert!((1..=BUILDS_PER_FRAME).contains(&near), "{near} near");
    for _ in 0..64 {
        streamer.update(tiles, far_away, BUILDS_PER_FRAME, |_| ());
    }
    for i in 0..tiles.tiles.len() {
        let wanted = tiles.distance(i, far_away) <= NEAR_METRES;
        assert_eq!(streamer.level(i) == Level::Near, wanted, "tile {i}");
    }
}

#[test]
fn every_tile_edge_hangs_a_skirt_deep_enough_to_hide_the_far_level_seam() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    let tiles = &scene.terrain;
    for (index, tile) in tiles.tiles.iter().enumerate() {
        let near = tiles.near(index);
        let base = near
            .iter()
            .find(|draw| draw.id.ends_with(":ground-grass"))
            .unwrap_or_else(|| panic!("tile {index} has no grass base"));
        let far = &tile.far[0].geometry;
        // Skirts reach below every vertex of both levels at the tile's edges.
        let lowest = |g: &mmorpg_presentation::geometry::Geometry| {
            g.p.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min)
        };
        let edge_low = |g: &mmorpg_presentation::geometry::Geometry| {
            g.p.iter()
                .filter(|p| {
                    (p[0] - tile.min[0]).abs() < 1e-6
                        || (p[0] - tile.max[0]).abs() < 1e-6
                        || (p[2] - tile.min[2]).abs() < 1e-6
                        || (p[2] - tile.max[2]).abs() < 1e-6
                })
                .map(|p| p[1])
                .fold(f64::INFINITY, f64::min)
        };
        assert!(lowest(&base.geometry) < edge_low(far) + 0.5, "tile {index}");
        assert!(lowest(far) >= tile.min[1] - 1e-6, "tile {index}");
    }
}

#[test]
fn far_chunks_cover_every_tile_not_drawn_near_exactly_once() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    let tiles = &scene.terrain;
    for eye in [[0., 10., 0.], [600., 40., 600.], [-300., 200., -700.]] {
        let near = |i: usize| tiles.distance(i, eye) <= NEAR_METRES;
        let mut covered = vec![u32::from(false); tiles.tiles.len()];
        for i in (0..tiles.tiles.len()).filter(|&i| near(i)) {
            covered[i] += 1;
        }
        let cover = tiles.far_cover(&near);
        for &(level, index) in &cover {
            let members = if level == CHUNK_SIDES.len() {
                vec![index]
            } else {
                tiles.chunks[level][index].tiles.clone()
            };
            for tile in members {
                covered[tile] += 1;
            }
        }
        assert!(covered.iter().all(|&count| count == 1), "{eye:?}");
        assert!(
            cover.len() < tiles.tiles.len() / 4,
            "{} far draws",
            cover.len()
        );
    }
}

#[test]
fn coarse_cliff_faces_keep_rock_detail_on_the_existing_surface() {
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    let mut faces = 0;
    for draw in scene.terrain.tiles.iter().flat_map(|tile| &tile.far) {
        let ground = draw.material.ground.unwrap();
        if ground.layer != mmorpg_presentation::ground::Layer::Rock {
            continue;
        }
        assert!(ground.far, "coarse rock selects its far material");
        assert!(
            !ground.overlay,
            "rock replaces the coarse face instead of covering it"
        );
        let geometry = &draw.geometry;
        assert_eq!(geometry.uv.len(), geometry.p.len());
        assert_eq!(geometry.tangents.len(), geometry.p.len());
        assert!(
            geometry
                .n
                .iter()
                .all(|n| n[1] > 0. && n.iter().all(|v| v.is_finite()))
        );
        for &[x, y, z] in &geometry.p {
            let surface = scenery.surface_at((x * 100.).round() as i32, (z * 100.).round() as i32);
            assert!(
                (y - f64::from(surface) / 100.).abs() < 1e-6,
                "material routing moved the surface at {x},{z}"
            );
        }
        faces += geometry.indices.len() / 3;
    }
    assert!(
        faces > 0,
        "the valley's coarse cliffs retain their rock material"
    );
}

#[test]
fn lake_beds_follow_the_actual_terrace_instead_of_world_zero() {
    let scene = StaticScene::new(&greyhaven_vale_scenery());
    let mut vertices = 0;
    let mut raised = false;
    for draw in scene
        .groups
        .iter()
        .flat_map(|group| &group.draws)
        .filter(|draw| draw.id.contains("lake-bed"))
    {
        for &[x, y, z] in &draw.geometry.p {
            let above_ground = y - scene.surface_y(x, z);
            assert!(
                (0.03..=0.06).contains(&above_ground),
                "lake bed at {x},{z} is {above_ground} m above the actual terrain"
            );
            raised |= y > 5.;
            vertices += 1;
        }
    }
    assert!(vertices > 100, "the lake retains its bed dressing");
    assert!(raised, "Stillwater's terrace is not at world zero");
    let lake = scene
        .water
        .iter()
        .find(|draw| draw.id.ends_with("water-0"))
        .expect("Stillwater's authored water surface");
    assert!(lake.geometry.p.len() > 65, "the shallows are subdivided");
    assert_eq!(
        lake.geometry.p.len(),
        lake.geometry.colors.len(),
        "depth tint reaches both clients as vertex colours"
    );
    for &[x, _, z] in &lake.geometry.p {
        assert!(
            scene.surface_y(x, z)
                < mmorpg_core::greyhaven_vale::lake::SURFACE as f64 / 100. - 0.007,
            "visible water surface intersects the raised shoreline at {x},{z}"
        );
    }
}

#[test]
fn new_ground_cover_is_instanced_and_keeps_routes_and_water_clear() {
    let scenery = greyhaven_vale_scenery();
    let scene = StaticScene::new(&scenery);
    let mut clumps = 0;
    let mut ferns = 0;
    for group in scene.groups.iter().filter(|group| {
        group.instanced
            && group.draws.first().is_some_and(|draw| {
                draw.id.contains("greyhaven-ground-cover")
                    || draw.id.contains("greyhaven-fern-understory")
            })
    }) {
        for draw in &group.draws {
            ferns += usize::from(draw.id.contains("greyhaven-fern-understory"));
            let point = [
                (f64::from(draw.model[12]) * 100.).round() as i32,
                (f64::from(draw.model[14]) * 100.).round() as i32,
            ];
            assert!(scenery.road_distance(point) >= 230);
            assert!(scenery.water_distance(point) >= 220);
            assert!(!mmorpg_core::greyhaven_vale::brook::wet_ground(
                point[0], point[1]
            ));
            clumps += 1;
        }
    }
    assert!(
        (500..12_000).contains(&clumps),
        "expected a bounded ground-cover pass, got {clumps}"
    );
    assert!(
        ferns > 100,
        "the forest should have walk-through understory: {ferns} ferns among {clumps} clumps"
    );
}
