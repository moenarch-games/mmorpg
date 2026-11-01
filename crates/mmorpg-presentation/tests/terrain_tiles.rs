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
