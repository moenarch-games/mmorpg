//! Player appearance on the frame actually consumed by the renderer, with real
//! core class selection and the native scene submitted to the GPU. While the
//! Ranger humanoid is the only character bundle, every class and sex is drawn
//! with it (Atlas 6.3.7 pauses #182's per-sex look), each class in its own
//! starter outfit with its armour plates and props (asset-tooling #379): the
//! Warden's sword and shield (#156), the Ranger's stowed bow and quiver and the
//! Arcanist's crystal staff and tome.
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
    let scene = |class: u8, sex: u8| {
        &scenes
            .iter()
            .find(|(choice, _)| *choice == (class, sex))
            .expect("every choice is drawn")
            .1
    };
    let warden = scene(0, 0);
    let ranger = scene(1, 0);
    let arcanist = scene(2, 0);
    assert!(!ranger.is_empty(), "the player is drawn");
    // Sexes share one look; each class wears its own outfit.
    for class in 0..3 {
        assert_eq!(
            scene(class, 0),
            scene(class, 1),
            "class {class}'s sexes share the Ranger"
        );
    }
    assert_ne!(warden, ranger, "the Warden and Ranger outfits differ");
    assert_ne!(arcanist, ranger, "the Arcanist and Ranger outfits differ");
    assert_ne!(arcanist, warden, "the Arcanist and Warden outfits differ");
    // Every class carries its two socket items and wears its outfit's plates.
    for (class, draws) in [(0, warden), (1, ranger), (2, arcanist)] {
        assert!(
            draws.iter().any(|draw| draw.id.contains("-item-0-")),
            "class {class}'s first item"
        );
        assert!(
            draws.iter().any(|draw| draw.id.contains("-item-1-")),
            "class {class}'s second item"
        );
        assert!(
            draws.iter().any(|draw| draw.id.contains("-plate-")),
            "class {class}'s plates"
        );
    }
}
