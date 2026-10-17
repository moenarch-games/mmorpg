//! Player appearance on the frame actually consumed by the renderer, with real
//! core class selection and the native scene submitted to the GPU. While the
//! Ranger humanoid is the only character bundle, every class and sex is drawn
//! with it (Atlas 6.3.7 pauses #182's per-class and per-sex look); only a
//! Warden carries the bundle's sword and shield (#156).
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
    assert!(!ranger.is_empty(), "the player is drawn");
    // Sexes share one look; the Ranger and Arcanist carry nothing.
    for class in 0..3 {
        assert_eq!(
            scene(class, 0),
            scene(class, 1),
            "class {class}'s sexes share the Ranger"
        );
    }
    assert_eq!(scene(2, 0), ranger, "the Arcanist looks like the Ranger");
    // The Warden draws the same body plus its sword and shield.
    assert!(
        warden.len() > ranger.len(),
        "the Warden carries a sword and a shield"
    );
    assert!(warden.iter().any(|draw| draw.id.contains("-item-0-")));
    assert!(warden.iter().any(|draw| draw.id.contains("-item-1-")));
}
