//! Player appearance on the frame actually consumed by the renderer, with real
//! core class selection and the native scene submitted to the GPU. While the
//! Ranger humanoid is the only character bundle, every class and sex is drawn
//! with it (Atlas 6.3.7 pauses #182's per-class and per-sex look).
use mmorpg_client::{camera::OrbitCamera, presentation::Presentation};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale};
use mmorpg_scenery::greyhaven_vale_scenery;
use std::{sync::Arc, time::Instant};

#[test]
fn every_class_and_sex_is_drawn_as_the_ranger_humanoid() {
    let content = greyhaven_vale::content();
    let now = Instant::now();
    let mut scenes = Vec::new();
    for class in 0..3 {
        for sex in 0..2 {
            let mut zone =
                ZoneSimulation::with_content(ZoneId::new(1), Arc::clone(&content)).unwrap();
            zone.add_player(1).unwrap();
            zone.apply_command(1, 1, ZoneCommand::ChooseClass { class, sex })
                .unwrap();
            zone.advance_tick().unwrap();
            let projection = zone.snapshot_for_player(1).unwrap();
            let mut presentation =
                Presentation::new(1, greyhaven_vale_scenery(), Arc::clone(&content), now).unwrap();
            presentation.push(projection, now).unwrap();
            let view = OrbitCamera::default().view(presentation.camera_target(now));
            scenes.push(((class, sex), presentation.unit_draws(now, view)));
        }
    }
    let (_, first) = &scenes[0];
    assert!(!first.is_empty(), "the player is drawn");
    for (choice, scene) in &scenes[1..] {
        assert_eq!(
            scene, first,
            "class and sex {choice:?} share the Ranger humanoid"
        );
    }
}
