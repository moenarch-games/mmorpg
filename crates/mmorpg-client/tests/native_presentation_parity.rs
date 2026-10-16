//! #182 appearance acceptance on the frame actually consumed by the renderer.
//! Historical SceneBox baseline red is recorded on the issue.
//! Uses real core class selection and the native scene submitted to the GPU.
use mmorpg_client::{camera::OrbitCamera, presentation::Presentation};
use mmorpg_core::{ZoneCommand, ZoneId, ZoneSimulation, greyhaven_vale};
use mmorpg_scenery::greyhaven_vale_scenery;
use std::{sync::Arc, time::Instant};

#[test]
fn current_pages_class_and_sex_choices_have_distinct_native_appearance() {
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
    let mut collapsed = Vec::new();
    for (index, (choice, scene)) in scenes.iter().enumerate() {
        for (other_choice, other_scene) in &scenes[index + 1..] {
            if scene == other_scene {
                collapsed.push((*choice, *other_choice));
            }
        }
    }
    assert!(
        collapsed.is_empty(),
        "native GPU scene collapses distinct Pages appearances: {collapsed:?}"
    );
}
