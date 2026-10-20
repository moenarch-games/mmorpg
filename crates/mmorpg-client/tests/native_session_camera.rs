//! Actual published browser startup policy; no actor relocation or gallery-only camera.
use mmorpg_client::{
    camera::{CameraView, OrbitCamera},
    presentation::Presentation,
    world::WorldScene,
};
use mmorpg_core::{ZoneId, ZoneSimulation, greyhaven_vale};
use std::{sync::Arc, time::Instant};

fn obstructed(
    view: CameraView,
    scenery: &mmorpg_scenery::Scenery,
    content: &mmorpg_core::ZoneContent,
) -> bool {
    // The inn's footprint is a presentation anchor without a collider (#158).
    let colliders = content.definition().colliders();
    scenery.props.iter().filter_map(|p| p.collider).any(|id| {
        let Some(c) = colliders.iter().find(|c| c.id == id) else {
            return false;
        };
        [0.35_f32, 0.6, 0.8, 1.0].into_iter().any(|t| {
            (0..3).all(|axis| {
                let p = view.target[axis] + (view.eye[axis] - view.target[axis]) * t;
                let low = (c.position[axis] - c.half_extents[axis]) as f32 / 100.0
                    - if axis == 1 { 0.0 } else { 0.4 };
                let high = (c.position[axis] + c.half_extents[axis]) as f32 / 100.0 + 0.4;
                p >= low && p <= high
            })
        })
    })
}
fn browser_view(focus: [f32; 3], heading: f32, world: &WorldScene) -> CameraView {
    let target = [focus[0], focus[1] + 0.6, focus[2]];
    let horizontal = 9.0 * 0.36_f32.cos();
    let mut eye = [
        target[0] - heading.sin() * horizontal,
        target[1] + 9.0 * 0.36_f32.sin(),
        target[2] - heading.cos() * horizontal,
    ];
    eye[1] = eye[1].max(world.surface_y(eye[0], eye[2]) + 0.35);
    CameraView {
        eye,
        target,
        field_of_view_degrees: 50.0,
    }
}
fn close(a: CameraView, b: CameraView) {
    for (x, y) in a
        .eye
        .into_iter()
        .chain(a.target)
        .zip(b.eye.into_iter().chain(b.target))
    {
        assert!((x - y).abs() < 1e-4, "camera component {x} != browser {y}");
    }
}
#[test]
fn session_first_view_matches_published_heading_pitch_and_rendered_ground() {
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let content = greyhaven_vale::content();
    let world = WorldScene::new(&scenery);
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
    zone.add_player(1).unwrap();
    let snapshot = zone.snapshot_for_player(1).unwrap();
    let self_unit = snapshot
        .entities
        .iter()
        .find(|e| e.entity() == mmorpg_core::EntityRef::Player(1))
        .unwrap();
    assert_eq!(
        [self_unit.position[0], self_unit.position[2]],
        [-1550, 1250]
    );
    let facing = f32::from(self_unit.facing) / 65536.0 * std::f32::consts::TAU;
    let now = Instant::now();
    let mut presentation =
        Presentation::new(1, scenery.clone(), Arc::clone(&content), now).unwrap();
    presentation.push(snapshot.clone(), now).unwrap();
    let focus = presentation.camera_target(now);
    let expected = [0, 2, -2, 1, -1, 3, -3, 4]
        .into_iter()
        .map(|step| {
            browser_view(
                focus,
                facing + step as f32 * std::f32::consts::FRAC_PI_4,
                &world,
            )
        })
        .find(|v| !obstructed(*v, &scenery, &content))
        .unwrap_or_else(|| browser_view(focus, facing, &world));
    assert!(
        obstructed(browser_view(focus, facing, &world), &scenery, &content),
        "fixture must expose actual startup obstruction"
    );
    let mut camera = OrbitCamera::default();
    close(presentation.camera_view(&mut camera, &world, now), expected);
    camera.orbit(100.0, 0.0);
    let heading = camera.facing();
    presentation.push(snapshot.clone(), now).unwrap();
    let moved = presentation.camera_view(&mut camera, &world, now);
    assert_eq!(
        camera.facing(),
        heading,
        "later projections must preserve user orbit"
    );
    assert_ne!(moved.eye, expected.eye);
    assert!(moved.eye[1] >= world.surface_y(moved.eye[0], moved.eye[2]) + 0.35);
    presentation.reset(now);
    presentation.push(snapshot, now).unwrap();
    close(presentation.camera_view(&mut camera, &world, now), expected);
}

#[test]
fn ground_sampler_matches_independent_four_metre_triangle_barycentres() {
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let grid = scenery.terrain_grid(400);
    let world = WorldScene::new(&scenery);
    let mut steep = None;
    let mut high = None;
    for r in 0..grid.rows - 1 {
        for c in 0..grid.columns - 1 {
            let vertices = [(c, r), (c, r + 1), (c + 1, r), (c + 1, r + 1)]
                .map(|(c, r)| grid.vertex(c, r).unwrap().0);
            let range = vertices.iter().map(|v| v[1]).max().unwrap()
                - vertices.iter().map(|v| v[1]).min().unwrap();
            if steep.as_ref().is_none_or(|(best, _)| range > *best) {
                steep = Some((range, vertices));
            }
            let height = vertices.iter().map(|v| v[1]).max().unwrap();
            if high.as_ref().is_none_or(|(best, _)| height > *best) {
                high = Some((height, vertices));
            }
        }
    }
    assert!(steep.as_ref().unwrap().0 > 0);
    assert!(high.as_ref().unwrap().0 > 0);
    for (_, v) in [steep.unwrap(), high.unwrap()] {
        for indices in [[0, 1, 2], [2, 1, 3]] {
            // An interior point of each actually rendered triangle, independently
            // constructed from its three source-grid vertices (not surface()).
            let p: [f64; 3] = std::array::from_fn(|axis| {
                indices
                    .iter()
                    .zip([0.2, 0.3, 0.5])
                    .map(|(i, w)| f64::from(v[*i][axis]) * w / 100.0)
                    .sum()
            });
            let actual = world.surface_y(p[0] as f32, p[2] as f32);
            let tolerance = 64.0 * f32::EPSILON * (p[1].abs() as f32).max(1.0);
            assert!(
                (actual - p[1] as f32).abs() <= tolerance,
                "rendered triangle floor {p:?}: actual {actual}"
            );
        }
    }
}

#[test]
fn no_viewer_does_not_consume_first_session_framing() {
    let scenery = mmorpg_scenery::greyhaven_vale_scenery();
    let world = WorldScene::new(&scenery);
    let content = greyhaven_vale::content();
    let now = Instant::now();
    let mut presentation = Presentation::new(1, scenery, Arc::clone(&content), now).unwrap();
    let mut camera = OrbitCamera::default();
    camera.orbit(70.0, 0.0);
    let heading = camera.facing();
    presentation.camera_view(&mut camera, &world, now);
    assert_eq!(heading, camera.facing());
    let mut zone = ZoneSimulation::with_content(ZoneId::new(1), content).unwrap();
    zone.add_player(1).unwrap();
    presentation
        .push(zone.snapshot_for_player(1).unwrap(), now)
        .unwrap();
    presentation.camera_view(&mut camera, &world, now);
    assert_ne!(
        heading,
        camera.facing(),
        "first actual viewer must still initialize session"
    );
}

#[test]
fn default_class_appearance_matches_published_entry_without_changing_explicit_none() {
    use mmorpg_core::{PlayerClass, ZoneCommand};
    for (class, hat) in [
        (0, "ironcrest-helm"),
        (1, "ranger-cap"),
        (2, "wayfarer-hood"),
    ] {
        let content = greyhaven_vale::content();
        let scenery = mmorpg_scenery::greyhaven_vale_scenery();
        let world = WorldScene::new(&scenery);
        let mut zone = ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
        zone.add_player(1).unwrap();
        zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex: 0 })
            .unwrap();
        zone.advance_tick().unwrap();
        let snapshot = zone.snapshot_for_player(1).unwrap();
        let now = Instant::now();
        let mut default = Presentation::new(1, scenery.clone(), Arc::clone(&content), now).unwrap();
        let mut explicit = Presentation::new(1, scenery, content, now).unwrap();
        default.set_default_appearance(PlayerClass::from_code(class).unwrap());
        explicit.set_headwear(Some(hat)).unwrap();
        default.push(snapshot.clone(), now).unwrap();
        explicit.push(snapshot, now).unwrap();
        let view = default.camera_view(&mut OrbitCamera::default(), &world, now);
        assert_eq!(
            default.unit_draws(now, view),
            explicit.unit_draws(now, view),
            "class {class} default hat differs from published entry"
        );
        default.set_headwear(None).unwrap();
        assert_ne!(
            default.unit_draws(now, view),
            explicit.unit_draws(now, view),
            "explicit none must remain possible"
        );
    }
}
